//! Durable, atomic file replacement for persisted state.
//!
//! Every writer in this module follows the same protocol so a crash or
//! `kill -9` between any two steps can never leave a truncated file at
//! the live path:
//!
//! 1. write the full content to a uniquely-named temp file in the **same
//!    directory** (same filesystem, so the final rename is atomic;
//!    unique name so concurrent writers cannot clobber each other);
//! 2. `sync_all` the temp file so the bytes are on disk before the rename;
//! 3. apply the requested permissions **before** the rename, so secrets
//!    are never briefly world-readable at the live path;
//! 4. `rename` the temp file over the destination (atomic on POSIX);
//! 5. remove the temp file again if any step failed.
//!
//! Readers pair with this by treating "file missing" (fresh install) and
//! "file unreadable or invalid" (corruption) as different outcomes: the
//! former falls back to defaults, the latter is a loud error and the
//! file is left untouched — never silently reset, never overwritten.

use anyhow::{Context, Result};
use std::{fs, io::Write as _, path::Path};

/// Permission bits for files holding secrets (`config.json`, `config.toml`).
pub const SECRET_MODE: u32 = 0o600;

/// Atomically replace `path` with `content` (see module docs).
/// `mode` is applied to the temp file before the rename when `Some`.
pub fn atomic_write(path: &Path, content: &[u8], mode: Option<u32>) -> Result<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("failed to create {}", parent.display()))?;
    }
    let stem = path
        .file_name()
        .and_then(|name| name.to_str())
        .unwrap_or("file");
    let temporary = path.with_file_name(format!(
        ".{stem}.tmp-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    let result = (|| {
        let mut file = fs::File::create(&temporary)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        file.write_all(content)
            .with_context(|| format!("failed to write {}", temporary.display()))?;
        #[cfg(unix)]
        if let Some(mode) = mode {
            use std::os::unix::fs::PermissionsExt as _;
            file.set_permissions(fs::Permissions::from_mode(mode))
                .with_context(|| format!("failed to chmod {}", temporary.display()))?;
        }
        file.sync_all()
            .with_context(|| format!("failed to sync {}", temporary.display()))?;
        drop(file);
        fs::rename(&temporary, path).with_context(|| {
            format!(
                "failed to commit {} (temp {} preserved)",
                path.display(),
                temporary.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result
}

/// Read a file that may legitimately not exist yet.
/// Returns `Ok(None)` only when the file is missing; any other IO error
/// is returned so callers cannot mistake "disk error" for "fresh install".
pub fn read_optional(path: &Path) -> Result<Option<String>> {
    match fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error).with_context(|| format!("failed to read {}", path.display())),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_no_temp_leftovers() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.json");
        atomic_write(&path, b"{\"a\":1}", Some(SECRET_MODE)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":1}");
        // Second write replaces; only the live file may remain.
        atomic_write(&path, b"{\"a\":2}", Some(SECRET_MODE)).unwrap();
        assert_eq!(fs::read(&path).unwrap(), b"{\"a\":2}");
        let leftovers: Vec<_> = fs::read_dir(dir.path())
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .collect();
        assert_eq!(leftovers.len(), 1, "temp file leaked: {leftovers:?}");
    }

    #[cfg(unix)]
    #[test]
    fn secret_mode_applied() {
        use std::os::unix::fs::PermissionsExt as _;
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("secret.toml");
        atomic_write(&path, b"secret = 'x'", Some(SECRET_MODE)).unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // No mode: umask default, just must succeed.
        let plain = dir.path().join("plain.txt");
        atomic_write(&plain, b"hi", None).unwrap();
        assert_eq!(fs::read(&plain).unwrap(), b"hi");
    }

    #[test]
    fn read_optional_distinguishes_missing() {
        let dir = tempfile::tempdir().unwrap();
        assert_eq!(read_optional(&dir.path().join("nope.json")).unwrap(), None);
        let path = dir.path().join("x.txt");
        fs::write(&path, "hi").unwrap();
        assert_eq!(read_optional(&path).unwrap().as_deref(), Some("hi"));
    }
}
