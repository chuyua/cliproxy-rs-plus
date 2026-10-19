//! Go's Antigravity capability probe through the runtime (sdk/cliproxy
//! asyncProbeAntigravityCapabilities): registering an Antigravity credential's models
//! starts a `fetchAvailableModels` probe, and the models it names become
//! `supports_web_search` in the registry. The upstream is a local mock reached through
//! the credential's `base_url`.

use std::sync::{Arc, Mutex};

use axum::extract::{Request, State};
use cpa_core::config::Config;
use cpa_core::registry::dynamic::Suspension;
use cpa_exec::Executors;
use cpa_exec::claude::ClaudeExecutor;

type Seen = Mutex<Vec<(String, String)>>;

async fn upstream(State(seen): State<Arc<Seen>>, req: Request) -> &'static str {
    let path = req.uri().path().to_owned();
    let auth = req
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .unwrap_or_default()
        .to_owned();
    seen.lock().unwrap().push((path, auth));
    r#"{"webSearchModelIds":["Claude-Sonnet-4-6"]}"#
}

#[tokio::test]
async fn registration_probes_web_search_models() {
    let seen = Arc::new(Seen::default());
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let app = axum::Router::new().fallback(upstream).with_state(seen.clone());
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });

    let mut metadata = serde_json::Map::new();
    for (k, v) in [
        ("type", "antigravity"),
        ("access_token", "ya29.fake-capabilities"),
        ("expired", "2099-01-01T00:00:00Z"),
        ("project_id", "proj-capabilities"),
    ] {
        metadata.insert(k.into(), v.into());
    }
    let dir = std::path::Path::new("/fixture");
    let mut credential =
        cpa_core::credential::Credential::from_file(dir, &dir.join("ag-capabilities.json"), metadata).unwrap();
    credential.attributes.insert("base_url".into(), base);
    let executors = Executors {
        claude: ClaudeExecutor::new("http://127.0.0.1:1").unwrap(),
        codex: Default::default(),
        devices: Default::default(),
        openai: Default::default(),
        google: Default::default(),
    };
    // A private `auth-dir`, so nothing reads or writes the user's `~/.cli-proxy-api`.
    let auth_dir = std::env::temp_dir().join(format!("cpa-ag-capabilities-{}", std::process::id()));
    std::fs::create_dir_all(&auth_dir).unwrap();
    let config = Config::parse(&format!("auth-dir: {}\n", auth_dir.display())).unwrap();
    let rt = cpa_server::testing::runtime(config, vec![credential], executors);
    let flagged = |rt: &cpa_server::Runtime| {
        rt.registry()
            .available_by_provider("antigravity", |_, _| Suspension::None)
            .iter()
            .any(|m| m.id == "claude-sonnet-4-6" && m.supports_web_search)
    };
    // The static catalog does not flag it; the probe does.
    let mut probed = false;
    for _ in 0..300 {
        if flagged(&rt) {
            probed = true;
            break;
        }
        tokio::time::sleep(std::time::Duration::from_millis(10)).await;
    }
    assert!(
        probed,
        "probe result never reached the registry: {:?}",
        seen.lock().unwrap()
    );
    let seen = seen.lock().unwrap().clone();
    assert_eq!(
        seen,
        vec![(
            "/v1internal:fetchAvailableModels".to_owned(),
            "Bearer ya29.fake-capabilities".to_owned()
        )]
    );
}
