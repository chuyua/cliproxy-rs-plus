//! Go's Antigravity credits fallback through the real router, runtime and executor
//! (sdk/cliproxy/auth antigravity_credits_test.go
//! TestManagerExecuteStream_AntigravityCreditsFallbackAfterBootstrap429): a Claude-model
//! request whose normal attempt ends on quota is tried again on the Antigravity
//! credential with credits enabled; other models and a disabled switch end on the 429.
//! The upstream is a local mock reached through the credential's `base_url`.

use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use axum::response::{IntoResponse, Response};
use cpa_core::config::Config;
use cpa_exec::Executors;
use cpa_exec::claude::ClaudeExecutor;
use cpa_server::router;

/// Path with query and body of every upstream request.
type Seen = Mutex<Vec<(String, String)>>;

const QUOTA: &str = r#"{"error":{"code":429,"message":"You have exhausted your capacity on this model.","status":"RESOURCE_EXHAUSTED","details":[{"@type":"type.googleapis.com/google.rpc.ErrorInfo","reason":"QUOTA_EXHAUSTED"},{"@type":"type.googleapis.com/google.rpc.RetryInfo","retryDelay":"3600s"}]}}"#;
const ANSWER: &str = r#"{"response":{"candidates":[{"content":{"role":"model","parts":[{"text":"paid with credits"}]},"finishReason":"STOP"}],"usageMetadata":{"promptTokenCount":3,"candidatesTokenCount":4,"totalTokenCount":7},"modelVersion":"claude-sonnet-4-6","responseId":"msg-credits"},"traceId":"t"}"#;

/// Quota-exhausted until a request asks for credits.
async fn upstream(State(seen): State<Arc<Seen>>, req: Request) -> Response {
    let path = req.uri().path_and_query().map(|p| p.to_string()).unwrap_or_default();
    let body = axum::body::to_bytes(req.into_body(), usize::MAX).await.unwrap();
    let body = String::from_utf8(body.to_vec()).unwrap();
    seen.lock().unwrap().push((path.clone(), body.clone()));
    if path.contains("loadCodeAssist") {
        // The executor's credits lookup: no answer that changes the hint.
        return ([("content-type", "application/json")], "{}").into_response();
    }
    if !body.contains(r#""enabledCreditTypes":["GOOGLE_ONE_AI"]"#) {
        return (
            axum::http::StatusCode::TOO_MANY_REQUESTS,
            [("content-type", "application/json")],
            QUOTA,
        )
            .into_response();
    }
    let sse = format!("data: {ANSWER}\r\n\r\n");
    ([("content-type", "text/event-stream")], sse).into_response()
}

async fn serve(app: axum::Router) -> String {
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    url
}

/// A private `auth-dir`, so nothing reads or writes the user's `~/.cli-proxy-api`.
fn auth_dir(id: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("cpa-{id}-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

async fn proxy(credits: bool, id: &str) -> (String, Arc<Seen>) {
    let seen = Arc::new(Seen::default());
    let up = serve(axum::Router::new().fallback(upstream).with_state(seen.clone())).await;
    let config = Config::parse(&format!(
        "auth-dir: {}\naccess:\n  api-keys: [client-key]\nquota-exceeded:\n  antigravity-credits: {credits}\n",
        auth_dir(id).display()
    ))
    .unwrap();
    let mut metadata = serde_json::Map::new();
    for (k, v) in [
        ("type", "antigravity"),
        ("access_token", "ya29.fake-credits"),
        ("expired", "2099-01-01T00:00:00Z"),
        ("project_id", "proj-credits"),
        ("email", "credits@example.com"),
    ] {
        metadata.insert(k.into(), v.into());
    }
    let dir = std::path::Path::new("/fixture");
    let mut credential =
        cpa_core::credential::Credential::from_file(dir, &dir.join(format!("{id}.json")), metadata).unwrap();
    credential.attributes.insert("base_url".into(), up);
    let executors = Executors {
        claude: ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
        codex: Default::default(),
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    };
    let rt = Arc::new(cpa_server::testing::runtime(config, vec![credential], executors));
    (serve(router(rt)).await, seen)
}

async fn messages(url: &str, model: &str) -> (u16, String) {
    let res = wreq::Client::new()
        .post(format!("{url}/v1/messages"))
        .header("x-api-key", "client-key")
        .body(format!(
            r#"{{"model":"{model}","max_tokens":64,"messages":[{{"role":"user","content":"credits please"}}]}}"#
        ))
        .send()
        .await
        .unwrap();
    (res.status().as_u16(), res.text().await.unwrap())
}

fn generate_bodies(seen: &Seen) -> Vec<String> {
    seen.lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.contains("enerateContent"))
        .map(|(_, body)| body.clone())
        .collect()
}

#[tokio::test]
async fn claude_quota_falls_back_to_credits() {
    let (url, seen) = proxy(true, "ag-credits-claude").await;
    let (status, body) = messages(&url, "claude-sonnet-4-6").await;
    assert_eq!(status, 200, "{body}");
    assert!(body.contains("paid with credits"), "{body}");
    let bodies = generate_bodies(&seen);
    assert_eq!(bodies.len(), 2, "{bodies:?}");
    assert!(!bodies[0].contains("enabledCreditTypes"), "{}", bodies[0]);
    assert!(
        bodies[1].contains(r#""enabledCreditTypes":["GOOGLE_ONE_AI"]"#),
        "{}",
        bodies[1]
    );
}

#[tokio::test]
async fn gemini_quota_does_not_use_credits() {
    let (url, seen) = proxy(true, "ag-credits-gemini").await;
    let (status, body) = messages(&url, "gemini-3-flash").await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(generate_bodies(&seen).len(), 1);
}

#[tokio::test]
async fn credits_switch_off_keeps_the_quota_error() {
    let (url, seen) = proxy(false, "ag-credits-off").await;
    let (status, body) = messages(&url, "claude-sonnet-4-6").await;
    assert_eq!(status, 429, "{body}");
    assert_eq!(generate_bodies(&seen).len(), 1);
}

/// Go's ExecuteCount has no credits fallback: a Claude-model token count that ends on
/// quota returns the 429 after one countTokens request.
#[tokio::test]
async fn token_counts_do_not_use_credits() {
    let (url, seen) = proxy(true, "ag-credits-count").await;
    let res = wreq::Client::new()
        .post(format!("{url}/v1/messages/count_tokens"))
        .header("x-api-key", "client-key")
        .body(r#"{"model":"claude-sonnet-4-6","messages":[{"role":"user","content":"count please"}]}"#)
        .send()
        .await
        .unwrap();
    let status = res.status().as_u16();
    let body = res.text().await.unwrap();
    assert_eq!(status, 429, "{body}");
    let counts: Vec<String> = seen
        .lock()
        .unwrap()
        .iter()
        .filter(|(path, _)| path.contains("countTokens"))
        .map(|(path, _)| path.clone())
        .collect();
    assert_eq!(counts.len(), 1, "{counts:?}");
}
