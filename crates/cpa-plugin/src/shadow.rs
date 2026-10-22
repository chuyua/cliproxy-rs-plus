//! Content-addressed native images (Go v8.0.35 `internal/pluginhost/shadow.go`).
//! Images and their callback tables remain resident after logical unload, so each
//! distinct build needs its own path and the source artifact must stay replaceable.
//!
//! Unlike Go's shared `$TMPDIR/cliproxy-pluginhost`, shadows live under a per-user root,
//! `$TMPDIR/cliproxy-pluginhost-<euid>/pid-<pid>`. Both directories must be real
//! directories owned by the effective user and not group- or other-writable; anything
//! else fails closed instead of being repaired, so no other local user can swap a
//! library between its hash check and `dlopen`. This trusts `$TMPDIR` itself (a sticky
//! `/tmp` or a private directory) and needs it to allow executable mappings.

use std::fs::{self, File};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, Once};

use sha2::{Digest, Sha256};

const PREFIX: &str = "cliproxy-plugin-";
const TEMP_PREFIX: &str = ".cliproxy-plugin-";

/// Serialises check, unlink and rename, so concurrent loads of one image in this process
/// never see a shadow that another load is replacing. Directories are per process.
static PUBLISH: Mutex<()> = Mutex::new(());

pub(crate) fn copy(source: &Path, id: &str) -> Result<PathBuf, String> {
    if !crate::platform::valid_id(id) {
        return Err("invalid plugin id".into());
    }
    // SAFETY: geteuid has no preconditions.
    let euid = unsafe { libc::geteuid() };
    let root = std::env::temp_dir().join(format!("cliproxy-pluginhost-{euid}"));
    private_dir(&root, euid, true)?;
    let dir = root.join(format!("pid-{}", std::process::id()));
    private_dir(&dir, euid, true)?;
    static CLEANUP: Once = Once::new();
    CLEANUP.call_once(|| {
        remove_stale_files(&dir);
        remove_stale_dirs(&root, euid);
    });
    copy_to(source, id, &dir)
}

/// Creates `path` as 0700 when missing, then requires a real directory owned by `euid`
/// that only its owner can write. Never repairs ownership or permissions.
fn private_dir(path: &Path, euid: u32, create: bool) -> Result<(), String> {
    if create {
        match fs::DirBuilder::new().mode(0o700).create(path) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(format!("create shadow plugin directory {}: {e}", path.display())),
        }
    }
    let meta = fs::symlink_metadata(path).map_err(|e| format!("shadow plugin directory {}: {e}", path.display()))?;
    let problem = if !meta.file_type().is_dir() {
        "is not a directory (symlinks are refused)".to_string()
    } else if meta.uid() != euid {
        format!("is owned by uid {}, not {euid}", meta.uid())
    } else if meta.mode() & 0o022 != 0 {
        format!(
            "has mode {:o}; it must not be group- or other-writable",
            meta.mode() & 0o7777
        )
    } else {
        return Ok(());
    };
    Err(format!("unsafe shadow plugin directory {}: {problem}", path.display()))
}

fn copy_to(source: &Path, id: &str, dir: &Path) -> Result<PathBuf, String> {
    let (mut temp, temp_path) = crate::store::create_temp(dir, &format!("{PREFIX}{id}"))?;
    let result = (|| {
        let mut input = File::open(source).map_err(|e| e.to_string())?;
        let mut hash = Sha256::new();
        let mut size = 0;
        let mut buf = [0; 64 * 1024];
        loop {
            let len = input.read(&mut buf).map_err(|e| e.to_string())?;
            if len == 0 {
                break;
            }
            temp.write_all(&buf[..len]).map_err(|e| e.to_string())?;
            hash.update(&buf[..len]);
            size += len as u64;
        }
        drop(temp);
        let digest = hash.finalize();
        let extension = source
            .extension()
            .map(|e| format!(".{}", e.to_string_lossy()))
            .unwrap_or_default();
        let digest_hex = format!("{digest:x}");
        let target = dir.join(format!("{PREFIX}{id}-{}{extension}", &digest_hex[..32]));
        let _publish = PUBLISH.lock().unwrap_or_else(|e| e.into_inner());
        if matches(&target, size, &digest) {
            return Ok(target);
        }
        // Go os.Remove also removes an empty directory, never its contents.
        let removed = fs::remove_file(&target).or_else(|unlink_error| {
            fs::remove_dir(&target).map_err(|dir_error| {
                if dir_error.raw_os_error() == Some(libc::ENOTDIR) {
                    unlink_error
                } else {
                    dir_error
                }
            })
        });
        if let Err(e) = removed
            && e.kind() != std::io::ErrorKind::NotFound
        {
            if matches(&target, size, &digest) {
                return Ok(target);
            }
            return Err(format!("remove stale shadow plugin: {e}"));
        }
        if let Err(e) = fs::rename(&temp_path, &target) {
            if matches(&target, size, &digest) {
                return Ok(target);
            }
            return Err(format!("move shadow plugin: {e}"));
        }
        Ok(target)
    })();
    let _ = fs::remove_file(temp_path);
    result
}

/// Reuses only an existing regular file owned by this user, writable by nobody else, of
/// the right size and hash. `O_NOFOLLOW` refuses a symlink and `O_NONBLOCK` keeps a FIFO
/// from blocking; anything else is replaced.
fn matches(path: &Path, size: u64, digest: &[u8]) -> bool {
    let Ok(mut file) = fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
    else {
        return false;
    };
    // SAFETY: geteuid has no preconditions.
    let euid = unsafe { libc::geteuid() };
    if !file
        .metadata()
        .is_ok_and(|m| m.is_file() && m.uid() == euid && m.mode() & 0o022 == 0 && m.len() == size)
    {
        return false;
    }
    let mut hash = Sha256::new();
    std::io::copy(&mut file, &mut hash).is_ok() && hash.finalize().as_slice() == digest
}

fn remove_stale_files(dir: &Path) {
    let Ok(entries) = fs::read_dir(dir) else { return };
    for entry in entries.flatten() {
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !entry.file_type().is_ok_and(|t| t.is_dir()) && (name.starts_with(PREFIX) || name.starts_with(TEMP_PREFIX)) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn remove_stale_dirs(base: &Path, euid: u32) {
    let Ok(entries) = fs::read_dir(base) else { return };
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let Some(pid) = name
            .to_str()
            .and_then(|n| n.strip_prefix("pid-"))
            .and_then(|n| n.parse::<i32>().ok())
        else {
            continue;
        };
        if pid <= 0 || pid as u32 == std::process::id() {
            continue;
        }
        // Only ESRCH proves the process has exited; EPERM also means keep it.
        // SAFETY: signal zero checks process liveness without sending a signal.
        if unsafe { libc::kill(pid, 0) } == -1
            && std::io::Error::last_os_error().raw_os_error() == Some(libc::ESRCH)
            && private_dir(&entry.path(), euid, false).is_ok()
        {
            let _ = fs::remove_dir_all(entry.path());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn content_addressed_copies_reuse_change_and_repair() {
        // Go TestShadowCopyPlugin{ReusesContentAddressedShadow,
        // CreatesNewPathForChangedContent,ReplacesCorruptSameSizeShadow}.
        let root = std::env::temp_dir().join(format!("cpa-shadow-content-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let source = root.join("alpha.so");
        fs::write(&source, b"plugin-v1").unwrap();
        let first = copy_to(&source, "alpha", &root).unwrap();
        // Independently calculated SHA-256 of plugin-v1, first 128 bits.
        assert_eq!(
            first.file_name().unwrap(),
            "cliproxy-plugin-alpha-d3fee1b1103ae4a2e7fd52a5939b5a04.so"
        );
        assert_eq!(copy_to(&source, "alpha", &root).unwrap(), first);
        fs::write(&first, b"corrupt!!").unwrap();
        assert_eq!(copy_to(&source, "alpha", &root).unwrap(), first);
        assert_eq!(fs::read(&first).unwrap(), b"plugin-v1");
        fs::remove_file(&first).unwrap();
        fs::create_dir(&first).unwrap();
        fs::write(first.join("keep"), b"x").unwrap();
        let error = copy_to(&source, "alpha", &root).unwrap_err();
        assert!(
            error.ends_with(&std::io::Error::from_raw_os_error(libc::ENOTEMPTY).to_string()),
            "{error}"
        );
        assert_eq!(
            fs::read(first.join("keep")).unwrap(),
            b"x",
            "nonempty directories are never removed"
        );
        fs::remove_file(first.join("keep")).unwrap();
        assert_eq!(
            copy_to(&source, "alpha", &root).unwrap(),
            first,
            "empty directory is repaired"
        );
        fs::remove_file(&first).unwrap();
        let fifo = std::ffi::CString::new(first.as_os_str().as_encoded_bytes()).unwrap();
        // SAFETY: valid NUL-terminated path in this test's private directory.
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        let (tx, rx) = std::sync::mpsc::channel();
        let (src, dir) = (source.clone(), root.clone());
        let repair = std::thread::spawn(move || tx.send(copy_to(&src, "alpha", &dir)).unwrap());
        assert_eq!(
            rx.recv_timeout(std::time::Duration::from_secs(2))
                .expect("FIFO repair must not block")
                .unwrap(),
            first
        );
        repair.join().unwrap();
        assert_eq!(fs::read(&first).unwrap(), b"plugin-v1");
        fs::write(&source, b"plugin-v2").unwrap();
        let second = copy_to(&source, "alpha", &root).unwrap();
        assert_ne!(first, second);
        assert_eq!(fs::read(&first).unwrap(), b"plugin-v1");
        assert_eq!(fs::read(&second).unwrap(), b"plugin-v2");
        assert!(copy_to(&root.join("missing.so"), "alpha", &root).is_err());
        assert_eq!(fs::read_dir(&root).unwrap().count(), 3, "temporary files are removed");
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn cleanup_preserves_unrelated_files_and_live_processes() {
        use std::process::{Command, Stdio};
        // Go TestRemoveStaleShadowPluginsOnlyRemovesShadowFiles and
        // TestRemoveStaleShadowPluginDirsOnlyRemovesDeadProcesses.
        let root = std::env::temp_dir().join(format!("cpa-shadow-cleanup-{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        for name in [
            "cliproxy-plugin-alpha-old.so",
            ".cliproxy-plugin-alpha.tmp-old",
            "keep.so",
        ] {
            fs::write(root.join(name), b"x").unwrap();
        }
        remove_stale_files(&root);
        assert_eq!(fs::read_dir(&root).unwrap().count(), 1);
        assert!(root.join("keep.so").is_file());
        let mut live = Command::new("cat")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
            .unwrap();
        let mut dead = Command::new("true").spawn().unwrap();
        dead.wait().unwrap();
        let current_dir = root.join(format!("pid-{}", std::process::id()));
        let live_dir = root.join(format!("pid-{}", live.id()));
        let dead_dir = root.join(format!("pid-{}", dead.id()));
        for dir in [
            &current_dir,
            &live_dir,
            &dead_dir,
            &root.join("pid-invalid"),
            &root.join("unrelated"),
        ] {
            fs::create_dir_all(dir).unwrap();
            fs::write(dir.join("keep.so"), b"x").unwrap();
        }
        // SAFETY: geteuid has no preconditions.
        remove_stale_dirs(&root, unsafe { libc::geteuid() });
        assert!(!dead_dir.exists());
        for dir in [
            &current_dir,
            &live_dir,
            &root.join("pid-invalid"),
            &root.join("unrelated"),
        ] {
            assert!(dir.join("keep.so").is_file());
        }
        drop(live.stdin.take());
        live.wait().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unsafe_directories_and_shadows_are_refused() {
        use std::os::unix::fs::{PermissionsExt, symlink};
        // SAFETY: geteuid has no preconditions.
        let euid = unsafe { libc::geteuid() };
        let root = std::env::temp_dir().join(format!("cpa-shadow-trust-{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let private = root.join("private");
        private_dir(&private, euid, true).unwrap();
        assert_eq!(fs::metadata(&private).unwrap().permissions().mode() & 0o777, 0o700);
        private_dir(&private, euid, true).unwrap();
        let link = root.join("link");
        symlink(&private, &link).unwrap();
        let error = private_dir(&link, euid, true).unwrap_err();
        assert!(error.contains("symlinks are refused"), "{error}");
        fs::set_permissions(&private, fs::Permissions::from_mode(0o770)).unwrap();
        let error = private_dir(&private, euid, true).unwrap_err();
        assert!(error.contains("group- or other-writable"), "{error}");
        assert_eq!(
            fs::metadata(&private).unwrap().permissions().mode() & 0o777,
            0o770,
            "never repaired"
        );
        let error = private_dir(&private, euid + 1, false).unwrap_err();
        assert!(error.contains(&format!("not {}", euid + 1)), "{error}");

        // A symlink or a writable file with the right bytes is replaced, not reused.
        let dir = root.join("shadows");
        fs::create_dir(&dir).unwrap();
        let source = root.join("alpha.so");
        fs::write(&source, b"plugin-v1").unwrap();
        let target = copy_to(&source, "alpha", &dir).unwrap();
        let decoy = root.join("decoy.so");
        fs::write(&decoy, b"plugin-v1").unwrap();
        fs::remove_file(&target).unwrap();
        symlink(&decoy, &target).unwrap();
        assert_eq!(copy_to(&source, "alpha", &dir).unwrap(), target);
        assert!(fs::symlink_metadata(&target).unwrap().file_type().is_file());
        fs::set_permissions(&target, fs::Permissions::from_mode(0o620)).unwrap();
        let before = fs::metadata(&target).unwrap().ino();
        assert_eq!(copy_to(&source, "alpha", &dir).unwrap(), target);
        let after = fs::metadata(&target).unwrap();
        assert_ne!(after.ino(), before, "a group-writable shadow is replaced");
        assert_eq!(after.permissions().mode() & 0o777, 0o600);
        assert_eq!(fs::read(&target).unwrap(), b"plugin-v1");

        // Stale-process cleanup never follows a symlink or deletes an unsafe directory.
        let mut dead = std::process::Command::new("true").spawn().unwrap();
        dead.wait().unwrap();
        let elsewhere = root.join("elsewhere");
        fs::create_dir(&elsewhere).unwrap();
        fs::write(elsewhere.join("keep.so"), b"x").unwrap();
        let base = root.join("base");
        fs::create_dir(&base).unwrap();
        symlink(&elsewhere, base.join(format!("pid-{}", dead.id()))).unwrap();
        remove_stale_dirs(&base, euid);
        assert!(elsewhere.join("keep.so").is_file());
        fs::remove_file(base.join(format!("pid-{}", dead.id()))).unwrap();
        let writable = base.join(format!("pid-{}", dead.id()));
        fs::create_dir(&writable).unwrap();
        fs::set_permissions(&writable, fs::Permissions::from_mode(0o777)).unwrap();
        remove_stale_dirs(&base, euid);
        assert!(writable.is_dir(), "an unsafe stale directory is left alone");
        fs::remove_dir_all(root).unwrap();
    }
}
