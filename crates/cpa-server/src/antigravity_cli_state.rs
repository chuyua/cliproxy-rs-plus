//! Isolated, explicitly opened SQLite store for VERIFIED Antigravity CLI
//! versions. The auth-dir JSON and Go/Python config databases are untouched.
//! Only an opaque successful fetchAvailableModels verdict can adopt a version;
//! there is no automatic background updater or global UA override here.

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{Context, Result, ensure};
use cpa_exec::antigravity_cli_ua::{build_user_agent, effective_version};
use rusqlite::{Connection, OpenFlags, OptionalExtension, TransactionBehavior, params};

use crate::antigravity_cli_probe::ProbeVerdict;

const SCHEMA: &str = "
CREATE TABLE rust_ag_cli_meta (
    version INTEGER PRIMARY KEY CHECK(version=1)
);
INSERT INTO rust_ag_cli_meta(version) VALUES (1);
CREATE TABLE rust_ag_cli_adopted (
    id INTEGER PRIMARY KEY CHECK(id=1),
    version TEXT NOT NULL
);";

#[derive(Clone)]
pub struct CliVersionDb {
    path: PathBuf,
}

impl CliVersionDb {
    /// Opt-in creation, never auto-create when asked to reopen. File mode is
    /// private on Unix and an existing SQLite/config database is not overwritten.
    pub fn create_new(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        ensure!(!path.as_os_str().is_empty(), "SQLite path is required");
        ensure!(path.parent().is_some_and(Path::is_dir), "SQLite directory missing");
        let mut options = std::fs::OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(path).context("create isolated CLI version database")?;
        let db = Self {
            path: path.to_path_buf(),
        };
        if let Err(error) = db.open_raw().and_then(|c| c.execute_batch(SCHEMA).map_err(Into::into)) {
            let _ = std::fs::remove_file(path);
            return Err(error).context("initialize isolated CLI version database");
        }
        Ok(db)
    }

    /// Check the unique marker before reading or writing any adopted version.
    pub fn reopen(path: impl AsRef<Path>) -> Result<Self> {
        let db = Self {
            path: path.as_ref().to_path_buf(),
        };
        let _ = db.open()?;
        Ok(db)
    }

    fn open_raw(&self) -> Result<Connection> {
        let metadata = std::fs::symlink_metadata(&self.path).context("CLI version database not found")?;
        ensure!(metadata.file_type().is_file(), "CLI version path is not a regular file");
        let conn = Connection::open_with_flags(
            &self.path,
            OpenFlags::SQLITE_OPEN_READ_WRITE | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .context("open isolated CLI version database")?;
        conn.busy_timeout(Duration::from_secs(5))?;
        Ok(conn)
    }

    fn open(&self) -> Result<Connection> {
        let conn = self.open_raw()?;
        let schema: i64 = conn
            .query_row("SELECT version FROM rust_ag_cli_meta LIMIT 1", [], |r| r.get(0))
            .context("not an isolated CLI version database")?;
        ensure!(schema == 1, "unsupported CLI version database schema");
        Ok(conn)
    }

    pub fn load_adopted(&self) -> Result<Option<String>> {
        let version: Option<String> = self
            .open()?
            .query_row("SELECT version FROM rust_ag_cli_adopted WHERE id=1", [], |r| r.get(0))
            .optional()?;
        if let Some(ref v) = version {
            ensure!(
                build_user_agent(v, "linux", "amd64", "consumer").is_some(),
                "invalid persisted CLI version"
            );
        }
        Ok(version)
    }

    /// Only persist the version *bound to* a successful, opaque probe verdict.
    /// BEGIN IMMEDIATE serializes separate processes/instances: older concurrent
    /// results cannot overwrite a newer accepted version. The explicit floor is
    /// a minimum, not a pin. Failed evidence never touches SQLite.
    /// Returns Some(version) iff a newer version was actually adopted.
    pub fn adopt_after_probe(&self, floor: &str, verdict: &ProbeVerdict) -> Result<Option<String>> {
        let Some(candidate) = verdict.verified_version() else {
            return Ok(None);
        };
        ensure!(
            build_user_agent(candidate, "linux", "amd64", "consumer").is_some(),
            "invalid probed version"
        );
        let mut conn = self.open()?;
        let tx = conn.transaction_with_behavior(TransactionBehavior::Immediate)?;
        let current: Option<String> = tx
            .query_row("SELECT version FROM rust_ag_cli_adopted WHERE id=1", [], |r| r.get(0))
            .optional()?;
        if let Some(ref v) = current {
            ensure!(
                build_user_agent(v, "linux", "amd64", "consumer").is_some(),
                "invalid stored CLI version"
            );
        }
        let highest = effective_version(floor, current.as_deref());
        if effective_version(highest, Some(candidate)) != candidate || candidate == highest {
            tx.commit()?;
            return Ok(None);
        }
        tx.execute(
            "INSERT INTO rust_ag_cli_adopted(id, version) VALUES(1,?1)
             ON CONFLICT(id) DO UPDATE SET version=excluded.version",
            params![candidate],
        )?;
        tx.commit()?;
        Ok(Some(candidate.to_owned()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::antigravity_cli_probe::ProbeVerdict;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn sandbox() -> (CliVersionDb, PathBuf) {
        let nonce = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("gcl2api-version-db-{}-{nonce}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let db = CliVersionDb::create_new(dir.join("cli.sqlite")).unwrap();
        (db, dir)
    }

    #[test]
    fn failed_evidence_never_persists_and_reopen_restores_only_verified() {
        let (db, dir) = sandbox();
        assert_eq!(db.load_adopted().unwrap(), None);
        assert_eq!(
            db.adopt_after_probe("1.3.1", &ProbeVerdict::mock_failed()).unwrap(),
            None
        );
        assert_eq!(db.load_adopted().unwrap(), None);
        assert!(
            db.adopt_after_probe("1.3.1", &ProbeVerdict::mock_verified("1.3.2"))
                .unwrap()
                .is_some()
        );
        let reopened = CliVersionDb::reopen(dir.join("cli.sqlite")).unwrap();
        assert_eq!(reopened.load_adopted().unwrap().as_deref(), Some("1.3.2"));
        assert_eq!(
            reopened
                .adopt_after_probe("1.3.1", &ProbeVerdict::mock_verified("1.3.1"))
                .unwrap(),
            None
        );
        assert_eq!(
            reopened
                .adopt_after_probe("1.4.0", &ProbeVerdict::mock_verified("1.3.3"))
                .unwrap(),
            None
        );
        assert!(
            reopened
                .adopt_after_probe("1.3.1", &ProbeVerdict::mock_verified("1.3.x"))
                .is_err()
        );
        assert_eq!(reopened.load_adopted().unwrap().as_deref(), Some("1.3.2"));
        drop(reopened);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn two_instances_never_rollback_a_verified_version() {
        let (db, dir) = sandbox();
        let second = CliVersionDb::reopen(dir.join("cli.sqlite")).unwrap();
        std::thread::scope(|scope| {
            let a = scope.spawn(|| {
                db.adopt_after_probe("1.3.1", &ProbeVerdict::mock_verified("1.3.8"))
                    .unwrap()
            });
            let b = scope.spawn(|| {
                second
                    .adopt_after_probe("1.3.1", &ProbeVerdict::mock_verified("1.3.9"))
                    .unwrap()
            });
            a.join().unwrap();
            b.join().unwrap();
        });
        assert_eq!(db.load_adopted().unwrap().as_deref(), Some("1.3.9"));
        drop(second);
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn refuses_unrelated_or_existing_database() {
        let (db, dir) = sandbox();
        assert!(CliVersionDb::create_new(dir.join("cli.sqlite")).is_err());
        let other = dir.join("other.sqlite");
        let conn = Connection::open(&other).unwrap();
        conn.execute_batch("CREATE TABLE unrelated (id INTEGER);").unwrap();
        drop(conn);
        assert!(CliVersionDb::reopen(&other).is_err());
        assert!(CliVersionDb::reopen(dir.join("missing.sqlite")).is_err());
        drop(db);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
