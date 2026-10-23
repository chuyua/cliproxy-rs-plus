//! `host.model.*` callbacks (v8.0.35 host_callbacks.go,
//! host_model_stream_callbacks.go and model_stream_bridge.go).

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bytes::Bytes;
use futures_util::{StreamExt, future::BoxFuture, stream::BoxStream};
use tokio_util::sync::CancellationToken;

use crate::callbacks::{Caller, RequestScope};
use crate::client::CallbackError;
use crate::gojson::{self, Header};
use crate::host::{Host, lock};
use crate::{abi, go_struct};

go_struct! {
    pub struct ModelRequest("pluginhost.rpcHostModelExecutionRequest") {
        "entry_protocol" => entry_protocol: String,
        "exit_protocol" => exit_protocol: String,
        "model" => model: String,
        "stream" => stream: bool,
        "body" => body: Bytes,
        "headers" => headers: Header,
        "query" => query: Header,
        "alt" => alt: String,
        "forced_provider" omitempty => forced_provider: String,
        "auth_id" omitempty => auth_id: String,
        "proxy_url" omitempty => proxy_url: String,
        "path" omitempty => path: String,
        "host_callback_id" omitempty => host_callback_id: String,
    }
}

go_struct! {
    pub struct ModelResponse("pluginapi.HostModelExecutionResponse") {
        "status_code" => status_code: i64,
        "headers" => headers: Header,
        "body" => body: Bytes,
    }
}

go_struct! {
    pub struct ModelStreamResponse("pluginapi.HostModelStreamResponse") {
        "status_code" => status_code: i64,
        "headers" => headers: Header,
        "stream_id" => stream_id: String,
    }
}

go_struct! {
    pub struct StreamRequest("pluginapi.HostModelStreamReadRequest") {
        "stream_id" => stream_id: String,
    }
}

go_struct! {
    pub struct StreamResponse("pluginapi.HostModelStreamReadResponse") {
        "payload" => payload: Bytes,
        "error" => error: String,
        "done" => done: bool,
    }
}

/// The server's model execution boundary (Go `HostModelExecutor`). The caller is
/// skipped only by routers and interceptors; other plugins still participate.
pub trait ModelExecutor: Send + Sync {
    fn execute(
        &self,
        request: ModelRequest,
        skip_plugin: String,
        scope: RequestScope,
    ) -> BoxFuture<'_, Result<ModelOutput, CallbackError>>;
}

pub enum ModelOutput {
    Buffered(ModelResponse),
    Stream {
        status_code: i64,
        headers: Header,
        chunks: BoxStream<'static, crate::streams::Chunk>,
    },
}

struct Entry {
    chunks: tokio::sync::Mutex<BoxStream<'static, crate::streams::Chunk>>,
    cancel: CancellationToken,
}

impl Drop for Entry {
    fn drop(&mut self) {
        self.cancel.cancel();
    }
}

#[derive(Default)]
pub(crate) struct ModelBridge {
    next: AtomicU64,
    streams: Mutex<HashMap<String, Arc<Entry>>>,
}

impl ModelBridge {
    fn close(&self, id: &str) {
        // Dropping a plugin stream can close its callback-owned child streams.
        // Release the registry lock before either cancellation or destruction.
        let entry = lock(&self.streams).remove(id);
        if let Some(entry) = entry {
            entry.cancel.cancel();
        }
    }
}

impl Host {
    pub fn set_model_executor(&self, executor: Option<Arc<dyn ModelExecutor>>) {
        let previous = std::mem::replace(
            &mut *self
                .inner
                .callbacks
                .model_executor
                .write()
                .unwrap_or_else(std::sync::PoisonError::into_inner),
            executor,
        );
        drop(previous);
    }

    pub(crate) fn host_model_execute(&self, caller: &Caller, raw: &[u8], stream: bool) -> Result<Bytes, CallbackError> {
        let label = if stream {
            "host model execution stream request"
        } else {
            "host model execution request"
        };
        let mut req: ModelRequest =
            gojson::from_slice(raw).map_err(|e| CallbackError::new(format!("decode {label}: {e}")))?;
        if req.stream != stream {
            return Err(CallbackError::new(if stream {
                "host.model.execute_stream requires stream=true"
            } else {
                "host.model.execute requires stream=false"
            }));
        }
        req.proxy_url = req.proxy_url.trim().to_owned();
        if !req.proxy_url.is_empty() && !valid_proxy(&req.proxy_url) {
            return Err(CallbackError {
                message: "invalid proxy_url".into(),
                status: 400,
            });
        }
        req.path = req.path.trim().to_owned();
        let executor = self
            .inner
            .callbacks
            .model_executor
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone()
            .ok_or_else(|| CallbackError::new("host model executor is unavailable"))?;
        let runtime = self
            .inner
            .callbacks
            .runtime_handle()
            .ok_or_else(|| CallbackError::new("host model executor is unavailable"))?;
        let mut scope = self
            .inner
            .callbacks
            .lookup(&req.host_callback_id)
            .map(|(_, _, scope)| scope)
            .unwrap_or_default();
        if stream {
            // v8.0.35 detaches cancellation, not request values. Callback cleanup owns
            // the stream even when the parent request has already been cancelled.
            scope.cancel = CancellationToken::new();
        }
        let cancel = scope.cancel.clone();
        let callback_id = req.host_callback_id.clone();
        let output = runtime.block_on(async {
            tokio::select! {
                biased;
                () = cancel.cancelled() => Err(CallbackError::new("context canceled")),
                result = executor.execute(req, caller.plugin_id.trim().to_owned(), scope) => result,
            }
        });
        match output {
            Ok(ModelOutput::Buffered(resp)) => Ok(abi::ok_envelope(&resp)),
            Ok(ModelOutput::Stream {
                status_code,
                headers,
                chunks,
            }) => {
                let bridge = &self.inner.callbacks.model;
                let stream_id = (bridge.next.fetch_add(1, Ordering::SeqCst) + 1).to_string();
                lock(&bridge.streams).insert(
                    stream_id.clone(),
                    Arc::new(Entry {
                        chunks: tokio::sync::Mutex::new(chunks),
                        cancel,
                    }),
                );
                if !callback_id.is_empty() {
                    let weak = Arc::downgrade(&self.inner);
                    let id = stream_id.clone();
                    if self
                        .inner
                        .callbacks
                        .add_cleanup(
                            &callback_id,
                            Box::new(move || {
                                if let Some(inner) = weak.upgrade() {
                                    inner.callbacks.model.close(&id);
                                }
                            }),
                        )
                        .is_none()
                    {
                        bridge.close(&stream_id);
                    }
                }
                Ok(abi::ok_envelope(&ModelStreamResponse {
                    status_code,
                    headers,
                    stream_id,
                }))
            }
            Err(e) => {
                if stream {
                    cancel.cancel();
                }
                Err(e)
            }
        }
    }

    pub(crate) fn host_model_stream_read(&self, raw: &[u8]) -> Result<Bytes, CallbackError> {
        let req: StreamRequest = gojson::from_slice(raw)
            .map_err(|e| CallbackError::new(format!("decode host model stream read request: {e}")))?;
        if req.stream_id.is_empty() {
            return Err(CallbackError::new("model stream id is required"));
        }
        let bridge = &self.inner.callbacks.model;
        let entry = lock(&bridge.streams).get(&req.stream_id).cloned();
        let next = match entry {
            None => None,
            Some(entry) => {
                let runtime = self
                    .inner
                    .callbacks
                    .runtime_handle()
                    .ok_or_else(|| CallbackError::new("host model stream bridge is unavailable"))?;
                runtime.block_on(async {
                    tokio::select! {
                        biased;
                        () = entry.cancel.cancelled() => None,
                        chunk = async { entry.chunks.lock().await.next().await } => chunk,
                    }
                })
            }
        };
        let resp = match next {
            Some(chunk) => StreamResponse {
                payload: chunk.payload,
                done: chunk.error.is_some(),
                error: chunk.error.unwrap_or_default(),
            },
            None => StreamResponse {
                done: true,
                ..Default::default()
            },
        };
        if resp.done {
            bridge.close(&req.stream_id);
        }
        Ok(abi::ok_envelope(&resp))
    }

    pub(crate) fn host_model_stream_close(&self, raw: &[u8]) -> Result<Bytes, CallbackError> {
        let req: StreamRequest = gojson::from_slice(raw)
            .map_err(|e| CallbackError::new(format!("decode host model stream close request: {e}")))?;
        self.inner.callbacks.model.close(&req.stream_id);
        Ok(abi::ok_envelope(&crate::rpc::Empty {}))
    }
}

fn valid_proxy(raw: &str) -> bool {
    cpa_exec::xai_url::parse(raw).is_ok_and(|u| {
        matches!(u.scheme.as_str(), "http" | "https" | "socks5" | "socks5h")
            && !u.hostname.trim().is_empty()
            && (u.port.is_empty() || u.port.parse::<u16>().is_ok_and(|p| p != 0))
    })
}
