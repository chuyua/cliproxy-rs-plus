//! Opt-in Antigravity CLI manifest candidate and credential-backed version probe.
//! Python reference: src/antigravity_cli_version.py. No scheduler, persistence,
//! account-health mutation or automatic version adoption occurs in this module.

use anyhow::{Context, Result, ensure};
use cpa_core::{config::Config, credential::Credential};
use cpa_exec::{
    antigravity_cli_ua::{build_user_agent, effective_version},
    proxy::{GoClients, Proxy},
};
use futures_util::StreamExt;
use serde_json::Value;
use std::{sync::Arc, time::Duration};

const MANIFEST_LIMIT: usize = 16 * 1024;

pub fn manifest_candidate(body: &[u8], floor: &str) -> Result<Option<String>> {
    ensure!(body.len() <= MANIFEST_LIMIT, "manifest response too large");
    let value: Value = serde_json::from_slice(body).context("invalid manifest JSON")?;
    let version = value
        .get("version")
        .and_then(Value::as_str)
        .context("manifest missing version")?
        .trim();
    ensure!(
        build_user_agent(version, "linux", "amd64", "consumer").is_some(),
        "invalid manifest version"
    );
    let floor = effective_version(floor, None);
    if version == floor || effective_version(floor, Some(version)) != version {
        return Ok(None);
    }
    Ok(Some(version.to_owned()))
}

/// Fetches only manifest JSON, never updater binaries or OAuth credentials.
pub async fn fetch_manifest_candidate(
    clients: &GoClients,
    proxy: &Proxy,
    url: &str,
    floor: &str,
) -> Result<Option<String>> {
    ensure!(!url.trim().is_empty(), "manifest URL required");
    ensure!(!matches!(proxy, Proxy::Invalid), "invalid manifest proxy");
    let client = clients.try_get(proxy).context("manifest proxy unavailable")?;
    let response = client.get(url).timeout(Duration::from_secs(15)).send().await?;
    ensure!(response.status().as_u16() == 200, "manifest HTTP non-200");
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(part) = stream.next().await {
        let part = part?;
        ensure!(part.len() <= MANIFEST_LIMIT - bytes.len(), "manifest too large");
        bytes.extend_from_slice(&part);
    }
    manifest_candidate(&bytes, floor)
}

#[derive(Debug, PartialEq, Eq)]
pub struct ProbeVerdict {
    // Opaque evidence: the validated version is bound to its successful
    // upstream fetch, so a verdict for v1 cannot be reused to adopt v2.
    verified_version: Option<String>,
    attempted: usize,
}

impl ProbeVerdict {
    pub fn verified_version(&self) -> Option<&str> {
        self.verified_version.as_deref()
    }

    pub fn attempted(&self) -> usize {
        self.attempted
    }

    #[cfg(test)]
    pub(crate) fn mock_verified(version: &str) -> Self {
        Self {
            verified_version: Some(version.to_owned()),
            attempted: 1,
        }
    }

    #[cfg(test)]
    pub(crate) fn mock_failed() -> Self {
        Self {
            verified_version: None,
            attempted: 1,
        }
    }
}

/// Tries no more than three accounts. A 2xx fetchAvailableModels response is
/// positive evidence; failed or forbidden accounts are skipped without being
/// disabled, retried, cooled or logged. The caller alone may persist adoption.
pub async fn probe_candidate(
    clients: &GoClients,
    config: &Config,
    accounts: &[Arc<Credential>],
    base_url: &str,
    version: &str,
    os_type: &str,
    arch: &str,
    auth_method: &str,
) -> Result<ProbeVerdict> {
    ensure!(!base_url.trim().is_empty(), "API base required");
    let ua = build_user_agent(version, os_type, arch, auth_method).context("invalid candidate User-Agent")?;
    let endpoint = format!("{}/v1internal:fetchAvailableModels", base_url.trim_end_matches('/'));
    let mut attempted = 0;
    for c in accounts {
        if attempted >= 3 {
            break;
        }
        if c.provider != "antigravity" || c.disabled {
            continue;
        }
        let Some(token) = c
            .str("access_token")
            .or_else(|| c.str("token"))
            .filter(|t| !t.trim().is_empty())
        else {
            continue;
        };
        // Group-mode probes MUST use an already-approved bound proxy view.
        if c.str("proxy_mode") == Some("group")
            && !c
                .attributes
                .get("proxy_url")
                .is_some_and(|u| matches!(Proxy::parse(u), Proxy::Url(_)))
        {
            continue;
        }
        let proxy = Proxy::effective(c, config);
        if matches!(proxy, Proxy::Invalid) {
            continue;
        }
        let Some(client) = clients.try_get(&proxy) else {
            continue;
        };
        attempted += 1;
        let outcome = client
            .post(endpoint.as_str())
            .header("Authorization", format!("Bearer {token}"))
            .header("User-Agent", ua.as_str())
            .header("Content-Type", "application/json")
            .timeout(Duration::from_secs(30))
            .body("{}")
            .send()
            .await;
        if outcome.is_ok_and(|r| (200..300).contains(&r.status().as_u16())) {
            return Ok(ProbeVerdict {
                verified_version: Some(version.to_owned()),
                attempted,
            });
        }
    }
    Ok(ProbeVerdict {
        verified_version: None,
        attempted,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        Json, Router,
        http::{HeaderMap, StatusCode},
        routing::{get, post},
    };
    use serde_json::json;
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn rejects_old_and_injected_versions() {
        assert_eq!(
            manifest_candidate(br#"{"version":"1.3.2"}"#, "1.3.1").unwrap(),
            Some("1.3.2".into())
        );
        assert_eq!(manifest_candidate(br#"{"version":"1.3.1"}"#, "1.3.1").unwrap(), None);
        assert_eq!(manifest_candidate(br#"{"version":"1.2.9"}"#, "1.3.1").unwrap(), None);
        assert!(manifest_candidate(br#"{"version":"1.3.2\r\nInjected:x"}"#, "1.3.1").is_err());
        assert!(manifest_candidate(br#"{"version":"1.3.x"}"#, "1.3.1").is_err());
        assert!(manifest_candidate(br#"{"version":42}"#, "1.3.1").is_err());
        assert!(manifest_candidate(&vec![b'x'; MANIFEST_LIMIT + 1], "1.3.1").is_err());
    }

    fn fake_account(name: &str, token: &str) -> Arc<Credential> {
        Arc::new(
            Credential::from_file(
                std::path::Path::new("/mock"),
                &std::path::Path::new("/mock").join(name),
                json!({
                    "type":"antigravity","proxy_url":"direct",
                    "access_token":token,"refresh_token":"dummy-refresh"
                })
                .as_object()
                .unwrap()
                .clone(),
            )
            .unwrap(),
        )
    }

    #[tokio::test]
    async fn anonymous_manifest_then_two_local_dummy_probes() {
        let posts = Arc::new(AtomicUsize::new(0));
        let count = Arc::clone(&posts);
        let routes = Router::new()
            .route(
                "/manifest.json",
                get(|h: HeaderMap| async move {
                    assert!(!h.contains_key("authorization"));
                    Json(json!({"version":"1.3.2"}))
                }),
            )
            .route(
                "/v1internal:fetchAvailableModels",
                post(move |h: HeaderMap, body: String| {
                    let count = Arc::clone(&count);
                    async move {
                        assert_eq!(
                            h.get("user-agent").unwrap(),
                            "antigravity/cli/1.3.2 (aidev_client; os_type=linux; arch=amd64; auth_method=consumer)"
                        );
                        assert_eq!(body, "{}");
                        count.fetch_add(1, Ordering::SeqCst);
                        match h.get("authorization").unwrap().to_str().unwrap() {
                            "Bearer dummy-denied" => (StatusCode::FORBIDDEN, Json(json!({"error":"mock"}))),
                            "Bearer dummy-allowed" => (StatusCode::OK, Json(json!({"models":[]}))),
                            _ => panic!("unexpected mock token"),
                        }
                    }
                }),
            );
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let server = tokio::spawn(async move { axum::serve(listener, routes).await.unwrap() });
        let clients = GoClients::new(Default::default());
        let base = format!("http://{addr}");
        let candidate = fetch_manifest_candidate(&clients, &Proxy::Direct, &format!("{base}/manifest.json"), "1.3.1")
            .await
            .unwrap()
            .unwrap();
        let accounts = vec![
            fake_account("a.json", "dummy-denied"),
            fake_account("b.json", "dummy-allowed"),
        ];
        let previous = accounts[0].metadata.clone();
        let verified = probe_candidate(
            &clients,
            &Config::default(),
            &accounts,
            &base,
            &candidate,
            "linux",
            "amd64",
            "consumer",
        )
        .await
        .unwrap();
        assert_eq!(
            verified,
            ProbeVerdict {
                verified_version: Some(candidate.clone()),
                attempted: 2
            }
        );
        // Only the version bound to actual mock HTTP 2xx evidence may be
        // adopted. Neither OAuth JSON nor account health is written.
        let dir = std::env::temp_dir().join(format!("gcl2api-probe-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let versions = crate::antigravity_cli_state::CliVersionDb::create_new(dir.join("versions.sqlite")).unwrap();
        assert_eq!(versions.load_adopted().unwrap(), None);
        assert_eq!(
            versions.adopt_after_probe("1.3.1", &verified).unwrap().as_deref(),
            Some("1.3.2")
        );
        assert_eq!(
            crate::antigravity_cli_state::CliVersionDb::reopen(dir.join("versions.sqlite"))
                .unwrap()
                .load_adopted()
                .unwrap()
                .as_deref(),
            Some("1.3.2")
        );
        drop(versions);
        std::fs::remove_dir_all(dir).unwrap();
        assert_eq!(posts.load(Ordering::SeqCst), 2);
        assert_eq!(accounts[0].metadata, previous);
        server.abort();
    }

    #[tokio::test]
    async fn no_unbound_group_fallback_or_header_injection() {
        let clients = GoClients::new(Default::default());
        let mut account = (*fake_account("g.json", "dummy-group")).clone();
        account.metadata.insert("proxy_mode".into(), json!("group"));
        account.metadata.remove("proxy_url");
        let accounts = [Arc::new(account)];
        let negative = probe_candidate(
            &clients,
            &Config::default(),
            &accounts,
            "http://127.0.0.1:1",
            "1.3.2",
            "linux",
            "amd64",
            "consumer",
        )
        .await
        .unwrap();
        assert_eq!(
            negative,
            ProbeVerdict {
                verified_version: None,
                attempted: 0
            }
        );
        assert!(
            probe_candidate(
                &clients,
                &Config::default(),
                &accounts,
                "http://127.0.0.1:1",
                "1.3.2\r\nInjected:x",
                "linux",
                "amd64",
                "consumer"
            )
            .await
            .is_err()
        );
    }
}
