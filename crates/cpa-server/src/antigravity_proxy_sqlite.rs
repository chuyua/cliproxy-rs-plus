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
use rusqlite::{Connection, OpenFlags, Transaction, TransactionBehavior, params};

use crate::antigravity_proxy_group::{self, Group, Node, Strategy};

const SCHEMA: &str = r#"
CREATE TABLE rust_ag_meta (version INTEGER PRIMARY KEY CHECK(version=1));
INSERT INTO rust_ag_meta (version) VALUES (1);
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
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BindingState {
    pub node_id: Option<i64>,
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
        let connection = db.connection_raw()?;
        let version: i64 = connection
            .query_row("SELECT version FROM rust_ag_meta LIMIT 1", [], |row| row.get(0))
            .context("not a supported isolated proxy-group database")?;
        ensure!(version == 1, "unsupported proxy-group schema");
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
        ensure!(version == 1, "unsupported proxy-group schema");
        Ok(conn)
    }

    pub fn add_group(&self, id: i64, name: &str, strategy: Strategy, enabled: bool) -> Result<()> {
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
        ensure!(!proxy_url.trim().is_empty(), "proxy URL required");
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
                "SELECT bound_node_id,binding_status,eligibility_status,egress_ip,observed_egress_ip
                 FROM rust_ag_accounts WHERE credential_name=?1",
                [name],
                |r| {
                    Ok(BindingState {
                        node_id: r.get(0)?,
                        binding_status: r.get(1)?,
                        eligibility_status: r.get(2)?,
                        egress_ip: r.get(3)?,
                        observed_egress_ip: r.get(4)?,
                    })
                },
            )
            .context("account binding not found")
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
        now: i64,
        cooldown_seconds: i64,
        random_slot: usize,
    ) -> Result<Option<BoundProxy>> {
        self.invalidate_inner(name, now, cooldown_seconds, random_slot, false)
    }

    fn invalidate_inner(
        &self,
        name: &str,
        now: i64,
        cooldown_seconds: i64,
        random_slot: usize,
        inject_failure: bool,
    ) -> Result<Option<BoundProxy>> {
        let mut conn = self.connection()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let (group_id, node_id): (i64, Option<i64>) = tx
            .query_row(
                "SELECT group_id,bound_node_id FROM rust_ag_accounts WHERE credential_name=?1 AND proxy_mode='group'",
                [name],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .context("group account not found")?;
        let Some(node_id) = node_id else {
            tx.commit()?;
            return Ok(None);
        };
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
         egress_ip=NULL,observed_egress_ip=NULL WHERE credential_name=?1",
        params![name, selected],
    )?;
    ensure!(updated == 1, "account binding disappeared");
    Ok(())
}

fn bound_proxy(tx: &Transaction<'_>, node_id: i64, group_id: i64) -> Result<BoundProxy> {
    let proxy_url: String = tx.query_row(
        "SELECT proxy_url FROM rust_ag_nodes WHERE id=?1 AND group_id=?2",
        params![node_id, group_id],
        |r| r.get(0),
    )?;
    Ok(BoundProxy { node_id, proxy_url })
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
            return bound_proxy(tx, id, group_id).map(Some);
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
    bound_proxy(tx, chosen.node_id, group_id).map(Some)
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
        let changed = db.invalidate_and_rebind("mock-1.json", 101, 300, 0).unwrap().unwrap();
        assert_eq!(changed.node_id, 2);
        assert_eq!(
            db.binding("mock-1.json").unwrap(),
            BindingState {
                node_id: Some(2),
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
        assert!(db.invalidate_and_rebind("mock-1.json", 101, 300, 0).unwrap().is_none());
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
        assert!(db.invalidate_inner("mock-1.json", 101, 300, 0, true).is_err());
        assert_eq!(db_value(&db, "SELECT failure_count FROM rust_ag_nodes WHERE id=1"), 0);
        assert_eq!(db.binding("mock-1.json").unwrap().binding_status, "healthy");
        assert_eq!(db.binding("mock-1.json").unwrap().node_id, Some(1));
        drop(db);
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
