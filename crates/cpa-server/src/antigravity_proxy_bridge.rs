//! Side-effect-free, opt-in bridge from companion SQLite group bindings to
//! runtime credentials. SQLite owns proxy selection, JSON owns OAuth tokens;
//! never write a chosen proxy back to the credential file.
//! This is not wired to the live dispatcher yet.

use anyhow::{Context, Result, ensure};
use cpa_core::credential::{Credential, Source};
use cpa_exec::proxy::Proxy;

use crate::antigravity_proxy_sqlite::ProxyGroupDb;
use crate::runtime::CredentialStore;

/// Bind an eligible proxy on first use without persisting it in auth JSON.
/// Callers must revalidate the returned generation before dispatch.
pub fn bind_and_resolve_group_proxy_lease(
    db: &ProxyGroupDb,
    credential: &Credential,
    now: i64,
    random_slot: usize,
) -> Result<GroupProxyLease> {
    ensure!(credential.provider == "antigravity", "group mode requires Antigravity");
    ensure!(credential.str("proxy_mode") == Some("group"), "group mode not opted in");
    ensure!(!credential.disabled, "group credential disabled");
    ensure!(
        matches!(credential.source, Source::File(_)),
        "group credential must be file backed"
    );
    ensure!(
        credential
            .attributes
            .get("proxy_url")
            .is_none_or(|p| p.trim().is_empty()),
        "explicit proxy conflicts with group mode"
    );
    ensure!(
        credential.str("proxy_url").is_none_or(|p| p.trim().is_empty()),
        "explicit proxy conflicts with group mode"
    );
    let configured_group = credential.metadata.get("proxy_group_id").context("group ID missing")?;
    let group_id = configured_group
        .as_i64()
        .or_else(|| configured_group.as_str().and_then(|s| s.parse::<i64>().ok()))
        .context("invalid group ID")?;
    ensure!(group_id > 0, "group ID must be positive");
    // Reject mismatched groups before any selection transaction mutates
    // the cursor or creates a binding for an invalid credential.
    ensure!(
        db.account_group_id(&credential.id)? == group_id,
        "credential and SQLite groups differ"
    );
    db.select_and_bind(&credential.id, now, random_slot)?
        .context("no eligible group proxy; direct fallback forbidden")?;
    resolve_group_proxy_lease(db, credential, now)
}

/// Ephemeral, request-scoped view. Carries secrets; never log or serialize.
pub struct GroupProxyLease {
    pub credential: Credential,
    pub group_id: i64,
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
        group_id,
        node_id: active.binding.node_id,
        binding_generation: active.binding.binding_generation,
        credential_revision: credential.revision,
    })
}

/// Best-effort pre-dispatch revalidation against the live credential revision
/// and the current SQLite assignment. Any stale revision, disabled/cooling
/// node, reassignment, or edited proxy URL refuses admission. This is not a
/// cross-store atomic snapshot; request leases retain their generation for
/// safe failure callbacks even if binding changes after this check.
pub fn lease_is_current(db: &ProxyGroupDb, store: &CredentialStore, lease: &GroupProxyLease, now: i64) -> Result<bool> {
    let Some(current) = store.get(&lease.credential.id) else {
        return Ok(false);
    };
    if current.revision != lease.credential_revision || current.disabled {
        return Ok(false);
    }
    let Some(active) = db.active_bound_proxy(&current.id, now)? else {
        return Ok(false);
    };
    Ok(active.group_id == lease.group_id
        && active.binding.node_id == lease.node_id
        && active.binding.binding_generation == lease.binding_generation
        && lease.credential.attributes.get("proxy_url") == Some(&active.binding.proxy_url))
}

/// Apply a failure to the exact lease generation that served the request.
/// A late callback cannot invalidate a newer binding. A replacement must
/// receive a fresh lease and be revalidated before dispatch.
pub fn rebind_failed_group_proxy(
    db: &ProxyGroupDb,
    lease: &GroupProxyLease,
    now: i64,
    cooldown_seconds: i64,
    random_slot: usize,
) -> Result<Option<crate::antigravity_proxy_sqlite::BoundProxy>> {
    db.invalidate_and_rebind(
        &lease.credential.id,
        lease.node_id,
        lease.binding_generation,
        now,
        cooldown_seconds,
        random_slot,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::{Arc, Barrier};
    use std::thread;
    use crate::antigravity_proxy_group::{Node, Strategy};
    use cpa_core::config::Config;
    use cpa_core::credential::MetadataPatch;
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
    fn concurrent_first_use_never_splits_proxy_generation() {
        let (db, credential, dir) = sandbox();
        let db = Arc::new(db);
        let barrier = Arc::new(Barrier::new(16));
        let handles: Vec<_> = (0..16)
            .map(|slot| {
                let db = Arc::clone(&db);
                let barrier = Arc::clone(&barrier);
                let credential = credential.clone();
                thread::spawn(move || {
                    barrier.wait();
                    let lease = bind_and_resolve_group_proxy_lease(&db, &credential, 100, slot).unwrap();
                    (lease.node_id, lease.binding_generation)
                })
            })
            .collect();
        let leases: Vec<_> = handles.into_iter().map(|handle| handle.join().unwrap()).collect();
        assert!(leases.iter().all(|lease| *lease == leases[0]));
        assert!(leases[0].1 > 0);
        let db = Arc::try_unwrap(db).unwrap_or_else(|_| panic!("outstanding DB handles"));
        finish(db, dir);
    }

    #[test]
    fn first_use_binds_and_reuses_generation() {
        let (db, credential, dir) = sandbox();
        assert!(resolve_group_proxy_lease(&db, &credential, 100).is_err());
        let first = bind_and_resolve_group_proxy_lease(&db, &credential, 100, 0).unwrap();
        let again = bind_and_resolve_group_proxy_lease(&db, &credential, 100, 9).unwrap();
        assert_eq!(first.node_id, again.node_id);
        assert_eq!(first.binding_generation, again.binding_generation);
        assert_eq!(first.node_id, 11);
        finish(db, dir);
    }

    #[test]
    fn mismatched_group_does_not_create_binding() {
        let (db, mut credential, dir) = sandbox();
        credential.metadata.insert("proxy_group_id".into(), json!(999));
        assert!(bind_and_resolve_group_proxy_lease(&db, &credential, 100, 0).is_err());
        assert_eq!(db.binding(&credential.id).unwrap().binding_generation, 0);
        assert!(db.active_bound_proxy(&credential.id, 100).unwrap().is_none());
        finish(db, dir);
    }

    #[test]
    fn disabled_account_cannot_bind_on_first_use() {
        let (db, mut credential, dir) = sandbox();
        credential.disabled = true;
        assert!(bind_and_resolve_group_proxy_lease(&db, &credential, 100, 0).is_err());
        assert!(db.active_bound_proxy(&credential.id, 100).unwrap().is_none());
        finish(db, dir);
    }

    #[test]
    fn late_failure_cannot_cool_replacement_binding() {
        let (db, credential, dir) = sandbox();
        let first = db.select_and_bind(&credential.id, 100, 0).unwrap().unwrap();
        let stale_lease = resolve_group_proxy_lease(&db, &credential, 100).unwrap();
        let replacement = rebind_failed_group_proxy(&db, &stale_lease, 101, 300, 0).unwrap();
        assert!(replacement.is_none());
        assert!(
            rebind_failed_group_proxy(&db, &stale_lease, 102, 300, 0)
                .unwrap()
                .is_none()
        );
        assert_eq!(first.node_id, stale_lease.node_id);
        assert!(db.active_bound_proxy(&credential.id, 102).unwrap().is_none());
        finish(db, dir);
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

    #[test]
    fn revalidation_rejects_stale_sqlite_binding_after_node_reuse() {
        let (db, credential, dir) = sandbox();
        let store = CredentialStore::new(vec![credential.clone()]);
        let first = db.select_and_bind(&credential.id, 100, 0).unwrap().unwrap();
        // CredentialStore assigns a new monotonically increasing revision on
        // registration; the actual lease must use that live snapshot.
        let current = store.get(&credential.id).unwrap();
        let lease = resolve_group_proxy_lease(&db, &current, 100).unwrap();
        assert!(lease_is_current(&db, &store, &lease, 100).unwrap());
        assert!(
            db.invalidate_and_rebind(&credential.id, first.node_id, first.binding_generation, 101, 300, 0)
                .unwrap()
                .is_none()
        );
        assert!(!lease_is_current(&db, &store, &lease, 101).unwrap());
        db.select_and_bind(&credential.id, 402, 0).unwrap().unwrap();
        assert!(!lease_is_current(&db, &store, &lease, 402).unwrap());
        finish(db, dir);
    }

    #[test]
    fn revalidation_rejects_a_refreshed_credential_revision() {
        let (db, credential, dir) = sandbox();
        let store = CredentialStore::new(vec![credential.clone()]);
        db.select_and_bind(&credential.id, 100, 0).unwrap();
        let current = store.get(&credential.id).unwrap();
        // A view created before store registration is stale by definition.
        let stale = resolve_group_proxy_lease(&db, &credential, 100).unwrap();
        assert!(!lease_is_current(&db, &store, &stale, 100).unwrap());
        let lease = resolve_group_proxy_lease(&db, &current, 100).unwrap();
        assert!(lease_is_current(&db, &store, &lease, 100).unwrap());
        let mut patch = MetadataPatch::default();
        patch.set.insert("access_token".into(), json!("dummy-new-token"));
        let updated = store.apply_patch(&credential.id, current.revision, &patch).unwrap();
        assert!(updated.revision > lease.credential_revision);
        assert!(!lease_is_current(&db, &store, &lease, 101).unwrap());
        assert_eq!(updated.str("access_token"), Some("dummy-new-token"));
        finish(db, dir);
    }
}
