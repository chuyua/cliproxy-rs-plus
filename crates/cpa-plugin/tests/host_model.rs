//! Ports v8.0.35's host_callbacks_test.go model cases and
//! TestHostModelExecuteStreamDetachesFromCallbackParentCancel. No provider traffic.

use std::sync::{Arc, Mutex};

use bytes::Bytes;
use cpa_plugin::Host;
use cpa_plugin::callbacks::RequestScope;
use cpa_plugin::client::{CallbackError, CallbackInstance};
use cpa_plugin::hostmodel::{ModelExecutor, ModelOutput, ModelRequest, ModelResponse};
use cpa_plugin::streams::Chunk;
use cpa_plugin::testing::{call_from_plugin, callback_context};
use futures_util::{StreamExt, future::BoxFuture};
use serde_json::{Value, json};

#[derive(Default)]
struct Executor(Mutex<Vec<(ModelRequest, String, RequestScope)>>);

impl ModelExecutor for Executor {
    fn execute(
        &self,
        req: ModelRequest,
        skip: String,
        scope: RequestScope,
    ) -> BoxFuture<'_, Result<ModelOutput, CallbackError>> {
        self.0.lock().unwrap().push((req.clone(), skip, scope));
        Box::pin(async move {
            if req.model == "fail" {
                return Err(CallbackError {
                    message: "upstream failed".into(),
                    status: 502,
                });
            }
            let headers = [("X-Model".into(), vec!["ok".into()])].into();
            Ok(if req.stream {
                let chunks = futures_util::stream::iter([
                    Chunk::data(Bytes::from_static(b"one")),
                    Chunk {
                        payload: Bytes::from_static(b"partial"),
                        error: Some("terminal".into()),
                    },
                ])
                .chain(futures_util::stream::pending())
                .boxed();
                ModelOutput::Stream {
                    status_code: 202,
                    headers,
                    chunks,
                }
            } else {
                ModelOutput::Buffered(ModelResponse {
                    status_code: 202,
                    headers,
                    body: req.body,
                })
            })
        })
    }
}

fn call(host: &Host, instance: &Arc<CallbackInstance>, method: &str, req: Value) -> Result<Value, CallbackError> {
    let raw = call_from_plugin(host, "origin", instance, method, &serde_json::to_vec(&req).unwrap())?;
    let raw: Value = serde_json::from_slice(&raw).unwrap();
    assert_eq!(raw["ok"], true);
    Ok(raw["result"].clone())
}

/// TestHostModelExecuteCallback, CarriesCallerPluginSkipID, ForwardsProxyURL,
/// RejectsInvalidProxyURL, CallbacksValidateStreamMode and CallbacksRequireExecutor.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn callback_contract_matches_go() {
    let host = Host::new();
    let instance = Arc::new(CallbackInstance::default());
    let guard = callback_context(&host, "origin", instance.clone(), RequestScope::default());
    let executor = Arc::new(Executor::default());
    let (h, i, id, e) = (host.clone(), instance, guard.id().to_owned(), executor.clone());
    tokio::task::spawn_blocking(move || {
        assert_eq!(call(&h, &i, "host.model.execute", json!({})).unwrap_err().message, "host model executor is unavailable");
        h.set_model_executor(Some(e.clone()));
        let base = json!({"entry_protocol":"openai", "exit_protocol":"claude", "model":"model-1", "body":"eyAiYSIgOiAxIH0=",
            "headers":{"X-Request":["one","two"]}, "query":{"alt":["sse","raw"]}, "alt":"raw",
            "forced_provider":"provider", "auth_id":"auth", "path":" /v1/images/edits ",
            "proxy_url":" socks5h://user:pass@127.0.0.1:1080 ", "host_callback_id":id});
        let resp = call(&h, &i, "host.model.execute", base.clone()).unwrap();
        assert_eq!(resp, json!({"status_code":202,"headers":{"X-Model":["ok"]},"body":"eyAiYSIgOiAxIH0="}));
        let seen = e.0.lock().unwrap();
        let (req, skip, _) = &seen[0];
        assert_eq!(skip, "origin");
        assert_eq!((&*req.entry_protocol, &*req.exit_protocol, &*req.model), ("openai", "claude", "model-1"));
        assert_eq!(req.headers["X-Request"], ["one", "two"]);
        assert_eq!(req.query["alt"], ["sse", "raw"]);
        assert_eq!((&*req.alt, &*req.forced_provider, &*req.auth_id), ("raw", "provider", "auth"));
        assert_eq!(req.path, "/v1/images/edits");
        assert_eq!(req.proxy_url, "socks5h://user:pass@127.0.0.1:1080");
        drop(seen);
        for (method, stream) in [("host.model.execute", false), ("host.model.execute_stream", true)] {
            for proxy in ["direct", "socks4://127.0.0.1:1080", "http://", "http://:8080", "http://proxy:99999", "http://proxy:0", "not a url"] {
                let err = call(&h, &i, method, json!({"stream":stream,"proxy_url":proxy})).unwrap_err();
                assert_eq!((err.status, err.message.as_str()), (400, "invalid proxy_url"), "{proxy}");
            }
            assert!(call(&h, &i, method, json!({"stream":!stream})).unwrap_err().message.contains("requires stream="));
        }
        assert_eq!(e.0.lock().unwrap().len(), 1, "invalid calls never reach the executor");
        assert_eq!(call(&h, &i, "host.model.execute", json!({"model":"fail","host_callback_id":id})).unwrap_err().status, 502);
        let scope = e.0.lock().unwrap().last().unwrap().2.clone();
        assert!(!scope.cancel.is_cancelled(), "a buffered failure must not cancel its parent");
        scope.cancel.cancel();
        assert_eq!(call(&h, &i, "host.model.execute", json!({"host_callback_id":id})).unwrap_err().message, "context canceled");
    }).await.unwrap();
}

/// TestHostModelStreamClosesWithCallbackScope, ReadAfterCallbackCloseReturnsDone,
/// ExecuteStreamStartupErrorCleansUp and StreamReadAndCloseValidateStreamID.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn stream_lifetime_and_terminal_reads_match_go() {
    let host = Host::new();
    let executor = Arc::new(Executor::default());
    host.set_model_executor(Some(executor.clone()));
    let instance = Arc::new(CallbackInstance::default());
    let scope = RequestScope::default();
    let parent = scope.cancel.clone();
    let guard = callback_context(&host, "origin", instance.clone(), scope);
    let id = guard.id().to_owned();
    let (h, i) = (host.clone(), instance.clone());
    let stream = tokio::task::spawn_blocking(move || {
        let resp = call(
            &h,
            &i,
            "host.model.execute_stream",
            json!({"stream":true,"host_callback_id":id}),
        )
        .unwrap();
        assert_eq!(resp["status_code"], 202);
        resp["stream_id"].as_str().unwrap().to_owned()
    })
    .await
    .unwrap();
    let stream_cancel = executor.0.lock().unwrap()[0].2.cancel.clone();
    parent.cancel();
    assert!(
        !stream_cancel.is_cancelled(),
        "parent cancellation must not end the model stream"
    );
    let (h, i, s) = (host.clone(), instance.clone(), stream.clone());
    tokio::task::spawn_blocking(move || {
        let req = json!({"stream_id":s});
        assert_eq!(
            call(&h, &i, "host.model.stream_read", req.clone()).unwrap(),
            json!({"payload":"b25l","error":"","done":false})
        );
        assert_eq!(
            call(&h, &i, "host.model.stream_read", req.clone()).unwrap(),
            json!({"payload":"cGFydGlhbA==","error":"terminal","done":true})
        );
        assert_eq!(
            call(&h, &i, "host.model.stream_read", req.clone()).unwrap(),
            json!({"payload":null,"error":"","done":true})
        );
        call(&h, &i, "host.model.stream_close", req).unwrap();
        assert_eq!(
            call(&h, &i, "host.model.stream_read", json!({})).unwrap_err().message,
            "model stream id is required"
        );
        call(&h, &i, "host.model.stream_close", json!({})).unwrap();
    })
    .await
    .unwrap();
    assert!(stream_cancel.is_cancelled());

    let (h, i, id) = (host.clone(), instance.clone(), guard.id().to_owned());
    let stream = tokio::task::spawn_blocking(move || {
        call(
            &h,
            &i,
            "host.model.execute_stream",
            json!({"stream":true,"host_callback_id":id}),
        )
        .unwrap()["stream_id"]
            .clone()
    })
    .await
    .unwrap();
    let stream_cancel = executor.0.lock().unwrap()[1].2.cancel.clone();
    drop(guard);
    assert!(stream_cancel.is_cancelled());
    let (h, i, e) = (host, instance, executor);
    tokio::task::spawn_blocking(move || {
        assert_eq!(
            call(&h, &i, "host.model.stream_read", json!({"stream_id":stream})).unwrap()["done"],
            true
        );
        assert_eq!(
            call(
                &h,
                &i,
                "host.model.execute_stream",
                json!({"stream":true,"model":"fail"})
            )
            .unwrap_err()
            .status,
            502
        );
        assert!(e.0.lock().unwrap().last().unwrap().2.cancel.is_cancelled());
    })
    .await
    .unwrap();
}

/// Closing an executor stream may drop a callback that owns another model stream.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn closing_a_parent_stream_closes_its_callback_owned_child_without_deadlock() {
    struct Nested(Mutex<Option<cpa_plugin::callbacks::ContextGuard>>);
    impl ModelExecutor for Nested {
        fn execute(
            &self,
            _: ModelRequest,
            _: String,
            _: RequestScope,
        ) -> BoxFuture<'_, Result<ModelOutput, CallbackError>> {
            let guard = self.0.lock().unwrap().take();
            Box::pin(async move {
                Ok(ModelOutput::Stream {
                    status_code: 200,
                    headers: Default::default(),
                    chunks: futures_util::stream::unfold(guard, |guard| async move {
                        std::future::pending::<()>().await;
                        Some((Chunk::data(Bytes::new()), guard))
                    })
                    .boxed(),
                })
            })
        }
    }
    let host = Host::new();
    let instance = Arc::new(CallbackInstance::default());
    let guard = callback_context(&host, "origin", instance.clone(), RequestScope::default());
    let callback = guard.id().to_owned();
    host.set_model_executor(Some(Arc::new(Nested(Mutex::new(Some(guard))))));
    let (done, wait) = std::sync::mpsc::channel();
    // A blocked cleanup must fail on a bounded wait, not hang the Tokio test runner.
    std::thread::spawn(move || {
        let parent = call(&host, &instance, "host.model.execute_stream", json!({"stream":true})).unwrap();
        let child = call(
            &host,
            &instance,
            "host.model.execute_stream",
            json!({"stream":true,"host_callback_id":callback}),
        )
        .unwrap();
        call(
            &host,
            &instance,
            "host.model.stream_close",
            json!({"stream_id":parent["stream_id"]}),
        )
        .unwrap();
        assert_eq!(
            call(
                &host,
                &instance,
                "host.model.stream_read",
                json!({"stream_id":child["stream_id"]})
            )
            .unwrap()["done"],
            true
        );
        done.send(()).unwrap();
    });
    wait.recv_timeout(std::time::Duration::from_secs(2))
        .expect("nested stream close deadlocked");
}
