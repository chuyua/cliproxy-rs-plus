//! Isolated SQLite proxy-group persistence for gcl2api Antigravity.
//!
//! ponytail: This file uses a dedicated companion database with `rust_ag_*`
//! tables, *not* Python's live database or the Rust credential JSON files.
//! Until credential/network management is wired through one coordinator,
//! this adapter is deliberately not called from the request-time dispatcher.
//! Only account names and proxy URLs (not OAuth tokens) are persisted.
//!
//! One `BEGIN IMMEDIATE` transaction owns group cursor changes, node cooldown,
//! the account-node binding and its cached health invalidation.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Transaction, TransactionBehavior, params};

use crate::antigravity_proxy_group::{self, Group, Node, Strategy};

const SCHEMA: &str = r#"
CREATE TABLE rust_ag_meta (version INTEGER PRIMARY KEY CHECK(version=2));
INSERT INTO rust_ag_meta (version) VALUES (2);
CREATE TABLE rust_ag_groups (
    id INTEGER PRIMARY KEY, name TEXT NOT NULL UNIQUE,
    strategy TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
    cursor INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE rust_ag_nodes (
    id INTEGER PRIMARY KEY, group_id INTEGER NOT NULL REFERENCES rust_ag_groups(id),
    proxy_url TEXT NOT NULL, enabled INTEGER NOT NULL DEFAULT 1,
    failure_count INTEGER NOT NULL DEFAULT 0,
    cooldown_until INTEGER, last_used_at INTEGER,
    UNIQUE(group_id, proxy_url)
);
CREATE TABLE rust_ag_accounts (
    credential_name TEXT PRIMARY KEY,
    proxy_mode TEXT NOT NULL DEFAULT 'group' CHECK(proxy_mode='group'),
    group_id INTEGER NOT NULL REFERENCES rust_ag_groups(id),
    bound_node_id INTEGER REFERENCES rust_ag_nodes(id),
    binding_generation INTEGER NOT NULL DEFAULT 0 CHECK(binding_generation >= 0),
    binding_status TEXT NOT NULL DEFAULT 'unchecked',
    eligibility_status TEXT NOT NULL DEFAULT 'unchecked',
    egress_ip TEXT, observed_egress_ip TEXT
);
"#;

#[derive(Clone)]
pub struct ProxyGroupDb {
    path: PathBuf,
}

/// Proxy URLs can contain credentials; do not derive Debug or print these.
pub struct BoundProxy {
    pub node_id: i64,
    pub proxy_url: String,
    pub binding_generation: i64,
}

/// Validated group assignment snapshot from the companion database.
pub struct ActiveBoundProxy {
    pub group_id: i64,
    pub binding: BoundProxy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingState {
    pub node_id: Option<i64>,
    pub binding_generation: i64,
    pub binding_status: String,
    pub eligibility_status: String,
    pub egress_ip: Option<String>,
    pub observed_egress_ip: Option<String>,
}

impl ProxyGroupDb {
    /// Create a *new* explicit companion database, never an existing Python DB.
    /// The marker table prevents accidentally opening an unrelated database.
    pub fn create_new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        ensure!(!path.as_os_str().is_empty(), "database path required");
        ensure!(path.parent().is_some_and(|p| p.is_dir()), "parent directory missing");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).context("create isolated proxy database")?;
        let db = Self { path: path.to_owned() };
        let connection = db.connection_raw()?;
        connection
            .execute_batch(SCHEMA)
            .context("initialize isolated proxy database")?;
        Ok(db)
    }

    pub fn reopen(path: impl AsRef<Path>) -> Result<Self> {
        let db = Self {
            path: path.as_ref().to_owned(),
        };
        let mut connection = db.connection_raw()?;
        let tx = connection.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let version: i64 = tx
            .query_row("SELECT version FROM rust_ag_meta LIMIT 1", [], |row| row.get(0))
            .context("not a supported isolated proxy-group database")?;
        if version == 1 {
            tx.execute_batch(
                "ALTER TABLE rust_ag_accounts ADD COLUMN binding_generation INTEGER NOT NULL DEFAULT 0 CHECK(binding_generation >= 0);
                 UPDATE rust_ag_accounts SET binding_generation=1 WHERE bound_node_id IS NOT NULL;
                 DROP TABLE rust_ag_meta;
                 CREATE TABLE rust_ag_meta (version INTEGER PRIMARY KEY CHECK(version=2));
                 INSERT INTO rust_ag_meta (version) VALUES (2);",
            )?;
        } else {
            ensure!(version == 2, "unsupported proxy-group schema");
        }
        tx.commit()?;
        Ok(db)
    }

    fn connection_raw(&self) -> Result<Connection> {
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("open isolated proxy-group database")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        conn.execute_batch("PRAGMA foreign_keys=ON")?;
        Ok(conn)
    }

    fn connection(&self) -> Result<Connection> {
        let conn = self.connection_raw()?;
        let version: i64 = conn
            .query_row("SELECT version FROM rust_ag_meta LIMIT 1", [], |r| r.get(0))
            .context("isolated proxy-group schema missing")?;
        ensure!(version == 2, "unsupported proxy-group schema");
        Ok(conn)
    }

    pub fn add_group(&self, id: i64, name: &str, strategy: Strategy, enabled: bool) -> Result<()> {
        ensure!(id > 0, "group ID must be positive");
        ensure!(!name.trim().is_empty(), "group name required");
        let label = match strategy {
            Strategy::RoundRobin => "round_robin",
            Strategy::Random => "random",
            Strategy::Failover => "failover",
        };
        self.connection()?.execute(
            "INSERT INTO rust_ag_groups(id,name,strategy,enabled) VALUES (?1,?2,?3,?4)",
            params![id, name, label, enabled],
        )?;
        Ok(())
    }

    pub fn add_node(&self, node: Node, proxy_url: &str) -> Result<()> {
        ensure!(node.id > 0 && node.group_id > 0, "node and group IDs must be positive");
        ensure!(
            matches!(cpa_exec::proxy::Proxy::parse(proxy_url), cpa_exec::proxy::Proxy::Url(_)),
            "proxy node requires a supported proxy URL"
        );
        let failures = i64::try_from(node.failure_count).context("failure count too large")?;
        self.connection()?.execute(
            "INSERT INTO rust_ag_nodes(id,group_id,proxy_url,enabled,failure_count,cooldown_until)
             VALUES (?1,?2,?3,?4,?5,?6)",
            params![
                node.id,
                node.group_id,
                proxy_url,
                node.enabled,
                failures,
                node.cooldown_until
            ],
        )?;
        Ok(())
    }

    pub fn add_account(&self, name: &str, group_id: i64) -> Result<()> {
        ensure!(group_id > 0, "group ID must be positive");
        ensure!(!name.trim().is_empty(), "credential name required");
        ensure!(
            Path::new(name).file_name().is_some_and(|base| base == name),
            "credential must be a basename"
        );
        self.connection()?.execute(
            "INSERT INTO rust_ag_accounts(credential_name,group_id) VALUES (?1,?2)",
            params![name, group_id],
        )?;
        Ok(())
    }

    pub fn binding(&self, name: &str) -> Result<BindingState> {
        self.connection()?
            .query_row(
                "SELECT bound_node_id,binding_generation,binding_status,eligibility_status,egress_ip,observed_egress_ip
                 FROM rust_ag_accounts WHERE credential_name=?1",
                [name],
                |r| {
                    Ok(BindingState {
                        node_id: r.get(0)?,
                        binding_generation: r.get(1)?,
                        binding_status: r.get(2)?,
                        eligibility_status: r.get(3)?,
                        egress_ip: r.get(4)?,
                        observed_egress_ip: r.get(5)?,
                    })
                },
            )
            .context("account binding not found")
    }

    /// Snapshot a usable group binding without changing SQLite or OAuth JSON.
    /// Disabled/cooling/mismatched nodes are never treated as direct access.
    pub fn active_bound_proxy(&self, name: &str, now: i64) -> Result<Option<ActiveBoundProxy>> {
        let conn = self.connection()?;
        let selected = conn
            .query_row(
                "SELECT a.group_id,n.id,a.binding_generation,n.proxy_url
                 FROM rust_ag_accounts a
                 JOIN rust_ag_groups g ON g.id=a.group_id AND g.enabled=1
                 JOIN rust_ag_nodes n ON n.id=a.bound_node_id
                   AND n.group_id=a.group_id AND n.enabled=1
                 WHERE a.credential_name=?1 AND a.proxy_mode='group'
                   AND (n.cooldown_until IS NULL OR n.cooldown_until<=?2)",
                params![name, now],
                |r| {
                    Ok(ActiveBoundProxy {
                        group_id: r.get(0)?,
                        binding: BoundProxy {
                            node_id: r.get(1)?,
                            binding_generation: r.get(2)?,
                            proxy_url: r.get(3)?,
                        },
                    })
                },
            )
            .optional()?;
        Ok(selected)
    }

    /// Stable assignment: concurrent callers get the same bound node.
    /// The caller supplies an RNG index for Strategy::Random.
    pub fn select_and_bind(&self, name: &str, now: i64, random_slot: usize) -> Result<Option<BoundProxy>> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let selected = bind_in_tx(&tx, name, now, random_slot)?;
        tx.commit()?;
        Ok(selected)
    }

    /// Atomically cool a bound failed node, release it, reset cached health,
    /// and select a different node if one is available. If no replacement
    /// exists, persist an unbound account; NEVER silently use direct.
    pub fn invalidate_and_rebind(
        &self,
        name: &str,
        expected_node_id: i64,
        expected_generation: i64,
        now: i64,
        cooldown_seconds: i64,
        random_slot: usize,
    ) -> Result<Option<BoundProxy>> {
        self.invalidate_inner(
            name,
            expected_node_id,
            expected_generation,
            now,
            cooldown_seconds,
            random_slot,
            false,
        )
    }

    #[expect(
        clippy::too_many_arguments,
        reason = "transaction helper carries the complete optimistic binding/version preconditions"
    )]
    fn invalidate_inner(
        &self,
        name: &str,
        expected_node_id: i64,
        expected_generation: i64,
        now: i64,
        cooldown_seconds: i64,
        random_slot: usize,
        inject_failure: bool,
    ) -> Result<Option<BoundProxy>> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (group_id, node_id, generation): (i64, Option<i64>, i64) = tx
            .query_row(
                "SELECT group_id,bound_node_id,binding_generation FROM rust_ag_accounts WHERE credential_name=?1 AND proxy_mode='group'",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
            )
            .context("group account not found")?;
        let Some(node_id) = node_id else {
            tx.commit()?;
            return Ok(None);
        };
        ensure!(
            node_id == expected_node_id && generation == expected_generation,
            "stale proxy failure: binding node or generation changed"
        );
        let until = now.saturating_add(cooldown_seconds.max(0));
        let updated = tx.execute(
            "UPDATE rust_ag_nodes SET failure_count=CASE
               WHEN failure_count < 9223372036854775807 THEN failure_count+1
               ELSE failure_count END,
               cooldown_until=?1 WHERE id=?2 AND group_id=?3",
            params![until, node_id, group_id],
        )?;
        ensure!(updated == 1, "bound proxy node missing from account group");
        // Used only by isolated rollback regression; SQLite rolls this back.
        ensure!(!inject_failure, "injected failure before binding commit");
        reset_account(&tx, name, None)?;
        let replacement = bind_in_tx(&tx, name, now, random_slot)?;
        tx.commit()?;
        Ok(replacement)
    }

    #[cfg(test)]
    fn update_cached_health_for_test(&self, name: &str) -> Result<()> {
        self.connection()?.execute(
            "UPDATE rust_ag_accounts SET binding_status='healthy',
             eligibility_status='eligible',egress_ip='203.0.113.8',
             observed_egress_ip='203.0.113.9' WHERE credential_name=?1",
            [name],
        )?;
        Ok(())
    }
}

fn read_group(tx: &Transaction<'_>, group_id: i64) -> Result<Group> {
    let (id, enabled, strategy, cursor): (i64, bool, String, i64) = tx
        .query_row(
            "SELECT id,enabled,strategy,cursor FROM rust_ag_groups WHERE id=?1",
            [group_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
        )
        .context("proxy group not found")?;
    let strategy = Strategy::parse(&strategy).context("invalid proxy group strategy")?;
    let cursor = usize::try_from(cursor).context("invalid proxy group cursor")?;
    Ok(Group {
        id,
        enabled,
        strategy,
        cursor,
    })
}

fn read_nodes(tx: &Transaction<'_>, group_id: i64) -> Result<Vec<Node>> {
    let mut statement = tx.prepare(
        "SELECT id,group_id,enabled,cooldown_until,failure_count FROM rust_ag_nodes
         WHERE group_id=?1 ORDER BY id",
    )?;
    let rows = statement.query_map([group_id], |r| {
        let failures: i64 = r.get(4)?;
        Ok((
            r.get::<_, i64>(0)?,
            r.get::<_, i64>(1)?,
            r.get::<_, bool>(2)?,
            r.get::<_, Option<i64>>(3)?,
            failures,
        ))
    })?;
    let mut nodes = Vec::new();
    for row in rows {
        let (id, group_id, enabled, cooldown_until, failures) = row?;
        ensure!(failures >= 0, "negative failure counter");
        nodes.push(Node {
            id,
            group_id,
            enabled,
            cooldown_until,
            failure_count: failures as u64,
        });
    }
    Ok(nodes)
}

fn reset_account(tx: &Transaction<'_>, name: &str, selected: Option<i64>) -> Result<()> {
    let updated = tx.execute(
        "UPDATE rust_ag_accounts SET bound_node_id=?2,
         binding_status='unchecked',eligibility_status='unchecked',
         egress_ip=NULL,observed_egress_ip=NULL,
         binding_generation=binding_generation+1
         WHERE credential_name=?1 AND binding_generation<9223372036854775807",
        params![name, selected],
    )?;
    ensure!(updated == 1, "account binding disappeared");
    Ok(())
}

fn bound_proxy(tx: &Transaction<'_>, name: &str, node_id: i64, group_id: i64) -> Result<BoundProxy> {
    let binding_generation: i64 = tx.query_row(
        "SELECT binding_generation FROM rust_ag_accounts WHERE credential_name=?1",
        [name],
        |r| r.get(0),
    )?;
    let proxy_url: String = tx.query_row(
        "SELECT proxy_url FROM rust_ag_nodes WHERE id=?1 AND group_id=?2",
        params![node_id, group_id],
        |r| r.get(0),
    )?;
    Ok(BoundProxy {
        node_id,
        proxy_url,
        binding_generation,
    })
}

fn bind_in_tx(tx: &Transaction<'_>, name: &str, now: i64, random_slot: usize) -> Result<Option<BoundProxy>> {
    let (group_id, bound_node): (i64, Option<i64>) = tx
        .query_row(
            "SELECT group_id,bound_node_id FROM rust_ag_accounts
             WHERE credential_name=?1 AND proxy_mode='group'",
            [name],
            |r| Ok((r.get(0)?, r.get(1)?)),
        )
        .context("group account not found")?;
    let group = read_group(tx, group_id)?;
    let nodes = read_nodes(tx, group_id)?;
    if let Some(id) = bound_node {
        if group.enabled
            && nodes
                .iter()
                .any(|node| node.id == id && node.enabled && node.cooldown_until.is_none_or(|until| until <= now))
        {
            return bound_proxy(tx, name, id, group_id).map(Some);
        }
        reset_account(tx, name, None)?;
    }
    let Some(chosen) = antigravity_proxy_group::select(&group, &nodes, now, random_slot) else {
        return Ok(None);
    };
    reset_account(tx, name, Some(chosen.node_id))?;
    if group.strategy == Strategy::RoundRobin {
        tx.execute(
            "UPDATE rust_ag_groups SET cursor=?1 WHERE id=?2",
            params![i64::try_from(chosen.next_cursor)?, group_id],
        )?;
    }
    tx.execute(
        "UPDATE rust_ag_nodes SET last_used_at=?1 WHERE id=?2 AND group_id=?3",
        params![now, chosen.node_id, group_id],
    )?;
    bound_proxy(tx, name, chosen.node_id, group_id).map(Some)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sandbox() -> (ProxyGroupDb, PathBuf) {
        let unique = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let directory = std::env::temp_dir().join(format!("gcl2api-proxy-txn-{}-{}", std::process::id(), unique));
        std::fs::create_dir_all(&directory).unwrap();
        let db = ProxyGroupDb::create_new(directory.join("proxy.sqlite")).unwrap();
        for id in [10, 20] {
            db.add_group(id, &format!("g{id}"), Strategy::RoundRobin, true).unwrap();
        }
        for (id, group_id) in [(1, 10), (2, 10), (3, 20)] {
            db.add_node(
                Node {
                    id,
                    group_id,
                    enabled: true,
                    cooldown_until: None,
                    failure_count: 0,
                },
                &format!("http://mock-proxy-{id}.invalid"),
            )
            .unwrap();
        }
        db.add_account("mock-1.json", 10).unwrap();
        (db, directory)
    }

    fn db_value(db: &ProxyGroupDb, query: &str) -> i64 {
        db.connection().unwrap().query_row(query, [], |r| r.get(0)).unwrap()
    }

    #[test]
    fn rejects_invalid_proxy_nodes_without_persisting_them() {
        let (db, dir) = sandbox();
        for (offset, url) in ["", " ", "direct", "none", "not-a-url", "ftp://example.com"]
            .iter()
            .enumerate()
        {
            let id = 100 + offset as i64;
            assert!(
                db.add_node(
                    Node {
                        id,
                        group_id: 10,
                        enabled: true,
                        cooldown_until: None,
                        failure_count: 0
                    },
                    url
                )
                .is_err(),
                "unexpectedly accepted {url:?}"
            );
        }
        assert_eq!(db_value(&db, "SELECT COUNT(*) FROM rust_ag_nodes"), 3);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn rejects_nonpositive_identifiers_before_writing_sqlite() {
        let (db, dir) = sandbox();
        assert!(db.add_group(0, "invalid", Strategy::RoundRobin, true).is_err());
        assert!(db.add_account("invalid.json", 0).is_err());
        for (id, group_id) in [(0, 10), (-1, 10), (100, 0), (100, -1)] {
            assert!(
                db.add_node(
                    Node {
                        id,
                        group_id,
                        enabled: true,
                        cooldown_until: None,
                        failure_count: 0,
                    },
                    "http://127.0.0.1:8080"
                )
                .is_err()
            );
        }
        assert_eq!(db_value(&db, "SELECT COUNT(*) FROM rust_ag_groups"), 1);
        assert_eq!(db_value(&db, "SELECT COUNT(*) FROM rust_ag_nodes"), 3);
        assert_eq!(db_value(&db, "SELECT COUNT(*) FROM rust_ag_accounts"), 1);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn atomic_bind_and_restart_does_not_advance_cursor_twice() {
        let (db, dir) = sandbox();
        let first = db.select_and_bind("mock-1.json", 100, 0).unwrap().unwrap();
        assert_eq!(first.node_id, 1);
        assert_eq!(db_value(&db, "SELECT cursor FROM rust_ag_groups WHERE id=10"), 1);
        let reopened = ProxyGroupDb::reopen(dir.join("proxy.sqlite")).unwrap();
        assert_eq!(
            reopened
                .select_and_bind("mock-1.json", 101, 5)
                .unwrap()
                .unwrap()
                .node_id,
            1
        );
        assert_eq!(db_value(&db, "SELECT cursor FROM rust_ag_groups WHERE id=10"), 1);
        drop(reopened);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cooldown_rebind_clears_health_with_one_sqlite_transaction() {
        let (db, dir) = sandbox();
        db.select_and_bind("mock-1.json", 100, 0).unwrap();
        db.update_cached_health_for_test("mock-1.json").unwrap();
        let changed = db
            .invalidate_and_rebind("mock-1.json", 1, 1, 101, 300, 0)
            .unwrap()
            .unwrap();
        assert_eq!(changed.node_id, 2);
        assert_eq!(
            db.binding("mock-1.json").unwrap(),
            BindingState {
                node_id: Some(2),
                binding_generation: 3,
                binding_status: "unchecked".into(),
                eligibility_status: "unchecked".into(),
                egress_ip: None,
                observed_egress_ip: None,
            }
        );
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=1"), 1);
        assert_eq!(
            db_value(&db, "SELECT cooldown_until FROM rust_ag_nodes WHERE id=1"),
            401
        );
        assert_eq!(db_value(&db, "SELECT cursor FROM rust_ag_groups WHERE id=10"), 0);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn no_available_proxy_unbinds_and_never_falls_back_to_direct() {
        let (db, dir) = sandbox();
        db.select_and_bind("mock-1.json", 100, 0).unwrap();
        db.connection()
            .unwrap()
            .execute("UPDATE rust_ag_nodes SET enabled=0 WHERE id=2", [])
            .unwrap();
        db.update_cached_health_for_test("mock-1.json").unwrap();
        assert!(
            db.invalidate_and_rebind("mock-1.json", 1, 1, 101, 300, 0)
                .unwrap()
                .is_none()
        );
        let state = db.binding("mock-1.json").unwrap();
        assert!(state.node_id.is_none());
        assert_eq!(state.binding_status, "unchecked");
        assert!(state.egress_ip.is_none());
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=1"), 1);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn injected_transaction_error_rolls_back_cooldown_and_binding() {
        let (db, dir) = sandbox();
        db.select_and_bind("mock-1.json", 100, 0).unwrap();
        db.update_cached_health_for_test("mock-1.json").unwrap();
        assert!(db.invalidate_inner("mock-1.json", 1, 1, 101, 300, 0, true).is_err());
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=1"), 0);
        assert_eq!(db.binding("mock-1.json").unwrap().binding_status, "healthy");
        assert_eq!(db.binding("mock-1.json").unwrap().node_id, Some(1));
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn stale_failure_cannot_cool_the_replacement_node() {
        let (db, dir) = sandbox();
        let original = db.select_and_bind("mock-1.json", 100, 0).unwrap().unwrap();
        assert_eq!(original.node_id, 1);
        let replacement = db
            .invalidate_and_rebind(
                "mock-1.json",
                original.node_id,
                original.binding_generation,
                101,
                300,
                0,
            )
            .unwrap()
            .unwrap();
        assert_eq!(replacement.node_id, 2);
        db.update_cached_health_for_test("mock-1.json").unwrap();
        let before = db.binding("mock-1.json").unwrap();
        assert!(
            db.invalidate_and_rebind(
                "mock-1.json",
                original.node_id,
                original.binding_generation,
                102,
                300,
                0
            )
            .is_err()
        );
        assert_eq!(db.binding("mock-1.json").unwrap(), before);
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=2"), 0);
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=1"), 1);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn same_node_aba_rebinding_rejects_old_request_generation() {
        let (db, dir) = sandbox();
        let first = db.select_and_bind("mock-1.json", 100, 0).unwrap().unwrap();
        assert_eq!(first.node_id, 1);
        let second = db
            .invalidate_and_rebind("mock-1.json", first.node_id, first.binding_generation, 101, 300, 0)
            .unwrap()
            .unwrap();
        assert_eq!(second.node_id, 2);
        // Once node 1 leaves cooldown, A -> B -> A reuses the same node ID.
        let third = db
            .invalidate_and_rebind("mock-1.json", second.node_id, second.binding_generation, 402, 300, 0)
            .unwrap()
            .unwrap();
        assert_eq!(third.node_id, first.node_id);
        assert!(third.binding_generation > first.binding_generation);
        db.update_cached_health_for_test("mock-1.json").unwrap();
        let before = db.binding("mock-1.json").unwrap();
        assert!(
            db.invalidate_and_rebind("mock-1.json", first.node_id, first.binding_generation, 403, 300, 0,)
                .is_err()
        );
        assert_eq!(db.binding("mock-1.json").unwrap(), before);
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=1"), 1);
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=2"), 1);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn old_v1_companion_database_migrates_without_losing_binding() {
        let (db, dir) = sandbox();
        let initial = db.select_and_bind("mock-1.json", 100, 0).unwrap().unwrap();
        let path = dir.join("proxy.sqlite");
        let old = Connection::open(&path).unwrap();
        old.execute_batch(
            "ALTER TABLE rust_ag_accounts DROP COLUMN binding_generation;
             DROP TABLE rust_ag_meta;
             CREATE TABLE rust_ag_meta (version INTEGER PRIMARY KEY CHECK(version=1));
             INSERT INTO rust_ag_meta (version) VALUES (1);",
        )
        .unwrap();
        drop(old);
        drop(db);
        let upgraded = ProxyGroupDb::reopen(&path).unwrap();
        assert_eq!(db_value(&upgraded, "SELECT version FROM rust_ag_meta"), 2);
        assert_eq!(upgraded.binding("mock-1.json").unwrap().node_id, Some(initial.node_id));
        assert_eq!(upgraded.binding("mock-1.json").unwrap().binding_generation, 1);
        let same = upgraded.select_and_bind("mock-1.json", 101, 0).unwrap().unwrap();
        assert_eq!(same.node_id, initial.node_id);
        assert_eq!(same.binding_generation, 1);
        drop(upgraded);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn concurrent_double_bind_keeps_single_node_and_one_cursor_advance() {
        let (db, dir) = sandbox();
        let a = db.clone();
        let b = db.clone();
        let first = std::thread::spawn(move || a.select_and_bind("mock-1.json", 100, 0).unwrap().unwrap().node_id);
        let second = std::thread::spawn(move || b.select_and_bind("mock-1.json", 100, 1).unwrap().unwrap().node_id);
        assert_eq!(first.join().unwrap(), 1);
        assert_eq!(second.join().unwrap(), 1);
        assert_eq!(db_value(&db, "SELECT cursor FROM rust_ag_groups WHERE id=10"), 1);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn cannot_open_unrelated_sqlite_file_or_duplicate_database() {
        let (db, dir) = sandbox();
        assert!(ProxyGroupDb::create_new(dir.join("proxy.sqlite")).is_err());
        let unrelated = dir.join("other.sqlite");
        let conn = Connection::open(&unrelated).unwrap();
        conn.execute_batch("CREATE TABLE irrelevant(id INTEGER)").unwrap();
        drop(conn);
        assert!(ProxyGroupDb::reopen(&unrelated).is_err());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
