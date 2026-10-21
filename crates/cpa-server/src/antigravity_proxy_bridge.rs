//! Side-effect-free, opt-in bridge from companion SQLite group bindings to
//! runtime credentials. SQLite owns proxy selection, JSON owns OAuth tokens;
//! never write a chosen proxy back to the credential file.
//! This is not wired to the live dispatcher yet.

use anyhow::{Context, Result, ensure};
use cpa_core::credential::{Credential, Source};
use cpa_exec::proxy::Proxy;

use crate::antigravity_proxy_sqlite::ProxyGroupDb;

/// Ephemeral, request-scoped view. Carries secrets; never log or serialize.
pub struct GroupProxyLease {
    pub credential: Credential,
    pub node_id: i64,
    pub binding_generation: i64,
    pub credential_revision: u64,
}

/// Reject mismatched account/group settings or explicit proxy overrides.
/// Missing, disabled, or cooling bindings fail closed (never direct).
pub fn resolve_group_proxy_lease(db: &ProxyGroupDb, credential: &Credential, now: i64) -> Result<GroupProxyLease> {
    ensure!(credential.provider == "antigravity", "group mode requires Antigravity");
    ensure!(!credential.disabled, "group credential disabled");
    ensure!(
        matches!(credential.source, Source::File(_)),
        "group credential must be file backed"
    );
    ensure!(credential.str("proxy_mode") == Some("group"), "group mode not opted in");
    let configured_group = credential.metadata.get("proxy_group_id").context("group ID missing")?;
    let group_id = configured_group
        .as_i64()
        .or_else(|| configured_group.as_str().and_then(|s| s.parse::<i64>().ok()))
        .context("invalid group ID")?;
    ensure!(group_id > 0, "group ID must be positive");
    ensure!(
        credential
            .attributes
            .get("proxy_url")
            .is_none_or(|p| p.trim().is_empty())
            && credential.str("proxy_url").is_none_or(|p| p.trim().is_empty()),
        "explicit per-credential proxy conflicts with group mode"
    );
    let active = db
        .active_bound_proxy(&credential.id, now)?
        .context("no usable group proxy; direct fallback forbidden")?;
    ensure!(active.group_id == group_id, "credential and SQLite groups differ");
    ensure!(
        matches!(Proxy::parse(&active.binding.proxy_url), Proxy::Url(_)),
        "invalid or direct group node proxy"
    );
    let mut view = credential.clone();
    view.attributes.insert("proxy_url".into(), active.binding.proxy_url);
    Ok(GroupProxyLease {
        credential: view,
        node_id: active.binding.node_id,
        binding_generation: active.binding.binding_generation,
        credential_revision: credential.revision,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::antigravity_proxy_group::{Node, Strategy};
    use cpa_core::config::Config;
    use serde_json::{Value, json};
    use std::path::PathBuf;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sandbox() -> (ProxyGroupDb, Credential, PathBuf) {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("gcl2api-group-bridge-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = ProxyGroupDb::create_new(dir.join("companion.sqlite")).unwrap();
        db.add_group(9, "mock", Strategy::Failover, true).unwrap();
        db.add_node(
            Node {
                id: 11,
                group_id: 9,
                enabled: true,
                cooldown_until: None,
                failure_count: 0,
            },
            "http://mock-proxy.invalid:1234",
        )
        .unwrap();
        db.add_account("account.json", 9).unwrap();
        let path = dir.join("account.json");
        let payload = json!({
            "type":"antigravity","proxy_mode":"group","proxy_group_id":9,
            "access_token":"dummy-access","refresh_token":"dummy-refresh"
        });
        std::fs::write(&path, serde_json::to_vec(&payload).unwrap()).unwrap();
        let credential = Credential::from_file(&dir, &path, payload.as_object().unwrap().clone()).unwrap();
        (db, credential, dir)
    }

    fn finish(db: ProxyGroupDb, dir: PathBuf) {
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn selected_proxy_is_ephemeral_and_never_persisted_to_auth_json() {
        let (db, credential, dir) = sandbox();
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        let first = db.select_and_bind(&credential.id, 100, 0).unwrap().unwrap();
        let lease = resolve_group_proxy_lease(&db, &credential, 100).unwrap();
        assert_eq!(lease.node_id, first.node_id);
        assert_eq!(lease.binding_generation, first.binding_generation);
        assert_eq!(lease.credential_revision, credential.revision);
        assert!(matches!(
            Proxy::effective(&lease.credential, &Config::default()),
            Proxy::Url(ref url) if url == "http://mock-proxy.invalid:1234"
        ));
        assert_eq!(credential.str("proxy_url"), None);
        assert_eq!(lease.credential.str("proxy_url"), None);
        let persisted: Value = serde_json::from_slice(&std::fs::read(dir.join("account.json")).unwrap()).unwrap();
        assert!(persisted.get("proxy_url").is_none());
        assert_eq!(persisted["access_token"], "dummy-access");
        finish(db, dir);
    }

    #[test]
    fn reject_group_mismatch_and_custom_override() {
        let (db, mut credential, dir) = sandbox();
        db.select_and_bind(&credential.id, 100, 0).unwrap();
        credential.metadata.insert("proxy_group_id".into(), json!(100));
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        credential.metadata.insert("proxy_group_id".into(), json!("9"));
        credential.metadata.insert("proxy_url".into(), json!("direct"));
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        credential.metadata.remove("proxy_url");
        credential
            .attributes
            .insert("proxy_url".into(), "socks5://elsewhere.invalid:1".into());
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        credential.attributes.remove("proxy_url");
        credential.metadata.insert("proxy_mode".into(), json!("inherit"));
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        credential.metadata.insert("proxy_mode".into(), json!("group"));
        credential.disabled = true;
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        finish(db, dir);
    }

    #[test]
    fn cooling_node_never_falls_back_to_direct() {
        let (db, credential, dir) = sandbox();
        let first = db.select_and_bind(&credential.id, 100, 0).unwrap().unwrap();
        assert!(
            db.invalidate_and_rebind(&credential.id, first.node_id, first.binding_generation, 101, 300, 0)
                .unwrap()
                .is_none()
        );
        assert!(resolve_group_proxy_lease(&db, &credential, 102).is_err());
        let rebound = db.select_and_bind(&credential.id, 402, 0).unwrap().unwrap();
        assert_ne!(first.binding_generation, rebound.binding_generation);
        assert_eq!(
            resolve_group_proxy_lease(&db, &credential, 402)
                .unwrap()
                .binding_generation,
            rebound.binding_generation
        );
        finish(db, dir);
    }
}
