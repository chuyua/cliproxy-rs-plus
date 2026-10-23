//! Plugin thinking appliers live in thinking's process-wide provider table, which every
//! host refresh rewrites (Go `thinking.ClearPluginProviders`). This test has its own
//! binary so no other test's host can clear the table while it runs.

use std::collections::BTreeMap;
use std::path::Path;

use cpa_plugin::testing::scripted::{Log, Script};

fn calls(log: &Log) -> Vec<String> {
    std::mem::take(&mut *log.lock().unwrap())
}

/// Go `TestHostApplyConfigRegistersPluginThinkingApplier`: a plugin's thinking applier
/// serves its provider through thinking's own entry point; fusing the plugin drops it,
/// and the body then passes through as for any unknown provider.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn a_plugin_thinking_applier_serves_its_provider_until_fused() {
    let applied = r#"{"model":"plugin-model","plugin":"plugin-thinking","thinking_budget":10240}"#;
    let body = format!(
        r#"{{"Body":"{}"}}"#,
        base64::Engine::encode(&base64::engine::general_purpose::STANDARD, applied)
    );
    let a = Script {
        caps: r#""thinking_applier":true"#,
        results: [
            ("thinking.identifier", r#"{"identifier":"Plugin-Thinking"}"#),
            ("thinking.apply", Box::leak(body.into_boxed_str())),
        ]
        .into(),
        fuse_on: None,
    };
    let dir = Path::new(env!("CARGO_TARGET_TMPDIR")).join(format!("thinking-apply-{}", std::process::id()));
    let (host, log) = cpa_plugin::testing::scripted::host_with(&dir, BTreeMap::from([("a", a)])).await;
    let apply = || {
        cpa_common::thinking::apply_thinking(
            br#"{"model":"plugin-model"}"#,
            "plugin-model(10240)",
            "openai",
            "plugin-thinking",
            "plugin-thinking",
        )
        .unwrap()
    };
    assert_eq!(String::from_utf8(apply()).unwrap(), applied);
    assert_eq!(calls(&log), ["a thinking.apply"]);
    host.fuse("a", "ThinkingApplier.ApplyThinking", "test");
    assert_eq!(apply(), br#"{"model":"plugin-model"}"#);
    assert!(calls(&log).is_empty());
}
