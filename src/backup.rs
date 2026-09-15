use crate::{config::Config, persist};
use anyhow::{Context, Result, bail};
use chrono::Local;
use std::{
    fs::{self, File},
    io,
    path::{Path, PathBuf},
};
use zip::{ZipArchive, ZipWriter, write::SimpleFileOptions};

pub fn create() -> Result<PathBuf> {
    // Same-second creates (e.g. the pre-restore snapshot right after a
    // manual backup) must not overwrite each other: disambiguate.
    let mut destination = Config::backups_dir().join(format!(
        "clashlime-{}.zip",
        Local::now().format("%Y-%m-%d_%H-%M-%S")
    ));
    for n in 1.. {
        if !destination.exists() {
            break;
        }
        destination = Config::backups_dir().join(format!(
            "clashlime-{}-{n}.zip",
            Local::now().format("%Y-%m-%d_%H-%M-%S")
        ));
    }
    // Write to a temp name first: a crash must not leave a partial `.zip`
    // that `list()` would present as a valid backup.
    let temporary = destination.with_file_name(format!(
        ".{}.tmp-{}-{}.zip",
        destination
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("clashlime"),
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    if let Some(parent) = temporary.parent() {
        fs::create_dir_all(parent)?;
    }
    let result: Result<()> = (|| {
        let file = File::create(&temporary)?;
        let mut zip = ZipWriter::new(file);
        add_if_exists(&mut zip, &Config::profiles_path(), "profiles.yaml")?;
        add_if_exists(&mut zip, &Config::default_path(), "config.toml")?;
        for entry in fs::read_dir(Config::profiles_dir())?.filter_map(Result::ok) {
            if entry.file_type()?.is_file() {
                add_if_exists(
                    &mut zip,
                    &entry.path(),
                    &format!("profiles/{}", entry.file_name().to_string_lossy()),
                )?;
            }
        }
        zip.finish()?;
        fs::rename(&temporary, &destination).with_context(|| {
            format!(
                "failed to commit backup {} (temp {} preserved)",
                destination.display(),
                temporary.display()
            )
        })?;
        Ok(())
    })();
    if result.is_err() {
        let _ = fs::remove_file(&temporary);
    }
    result?;
    Ok(destination)
}

pub fn list() -> Result<Vec<PathBuf>> {
    let mut files: Vec<_> = fs::read_dir(Config::backups_dir())?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "zip"))
        .collect();
    files.sort_by(|a, b| b.file_name().cmp(&a.file_name()));
    Ok(files)
}

/// Restore a backup without ever leaving truncated live files:
/// extract into a staging dir, validate every entry parses, snapshot the
/// current state with `create()`, then commit each file atomically.
pub fn restore(path: &Path) -> Result<()> {
    if !path.starts_with(Config::backups_dir()) {
        bail!("backup must be inside the clashlime backup directory");
    }
    let staging = Config::backups_dir().join(format!(
        ".restore-{}-{}",
        std::process::id(),
        uuid::Uuid::new_v4().simple()
    ));
    fs::create_dir_all(&staging)?;
    let result = restore_inner(path, &staging);
    let _ = fs::remove_dir_all(&staging);
    result
}

fn restore_inner(path: &Path, staging: &Path) -> Result<()> {
    let mut archive = ZipArchive::new(File::open(path)?)?;
    // 1. Extract to staging, resolving destinations up front.
    let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
    for index in 0..archive.len() {
        let mut item = archive.by_index(index)?;
        let enclosed = item.enclosed_name().context("unsafe path in backup")?;
        let key = enclosed.to_str().map(str::to_owned);
        let destination = match key.as_deref() {
            Some("profiles.yaml") => Config::profiles_path(),
            Some("config.toml") => Config::default_path(),
            Some(name) if name.starts_with("profiles/") => Config::data_dir().join(&enclosed),
            _ => continue,
        };
        let staged_path = staging.join(&enclosed);
        if let Some(parent) = staged_path.parent() {
            fs::create_dir_all(parent)?;
        }
        io::copy(&mut item, &mut File::create(&staged_path)?)?;
        staged.push((staged_path, destination));
    }
    if staged.is_empty() {
        bail!("backup contains no clashlime files");
    }
    // 2. Validate everything before touching live state.
    for (staged_path, _) in &staged {
        validate_staged(staged_path)?;
    }
    // 3. Snapshot current state so a bad restore is reversible.
    create().context("refusing to restore before a fresh backup can be taken")?;
    // 4. Commit atomically, one file at a time.
    for (staged_path, destination) in &staged {
        let content = fs::read(staged_path)?;
        let mode = if destination == &Config::default_path() {
            Some(persist::SECRET_MODE)
        } else {
            None
        };
        persist::atomic_write(destination, &content, mode)?;
    }
    Ok(())
}

/// Syntax check only (not a full schema parse): catches truncation and
/// corruption without rejecting backups written by older versions.
fn validate_staged(path: &Path) -> Result<()> {
    let text = fs::read_to_string(path)
        .with_context(|| format!("backup entry {} is not readable", path.display()))?;
    if path.extension().is_some_and(|ext| ext == "toml") {
        toml::from_str::<toml::Value>(&text)
            .with_context(|| format!("backup entry {} is not valid TOML", path.display()))?;
    } else {
        serde_yaml_ng::from_str::<serde_yaml_ng::Value>(&text)
            .with_context(|| format!("backup entry {} is not valid YAML", path.display()))?;
    }
    Ok(())
}

fn add_if_exists(zip: &mut ZipWriter<File>, path: &Path, name: &str) -> Result<()> {
    if path.exists() {
        zip.start_file(name, SimpleFileOptions::default())?;
        io::copy(&mut File::open(path)?, zip)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn env_lock() -> &'static std::sync::Mutex<()> {
        crate::config::tests::test_env_lock()
    }

    fn isolate() -> (
        tempfile::TempDir,
        Option<std::ffi::OsString>,
        Option<std::ffi::OsString>,
    ) {
        let dir = tempfile::tempdir().unwrap();
        let orig_cfg = std::env::var_os("XDG_CONFIG_HOME");
        let orig_data = std::env::var_os("XDG_DATA_HOME");
        unsafe {
            std::env::set_var("XDG_CONFIG_HOME", dir.path().join("config"));
            std::env::set_var("XDG_DATA_HOME", dir.path().join("data"));
        }
        (dir, orig_cfg, orig_data)
    }

    fn restore_env(orig_cfg: Option<std::ffi::OsString>, orig_data: Option<std::ffi::OsString>) {
        unsafe {
            match orig_cfg {
                Some(v) => std::env::set_var("XDG_CONFIG_HOME", v),
                None => std::env::remove_var("XDG_CONFIG_HOME"),
            }
            match orig_data {
                Some(v) => std::env::set_var("XDG_DATA_HOME", v),
                None => std::env::remove_var("XDG_DATA_HOME"),
            }
        }
    }

    fn seed_state() {
        fs::create_dir_all(Config::profiles_dir()).unwrap();
        fs::write(
            Config::profiles_path(),
            "current: R1\nitems:\n- uid: R1\n  type: remote\n  name: Sub\n  file: R1.yaml\n  updated: 0\n",
        )
        .unwrap();
        fs::write(Config::profiles_dir().join("R1.yaml"), "proxies: []\n").unwrap();
        let config_path = Config::default_path();
        fs::create_dir_all(config_path.parent().unwrap()).unwrap();
        fs::write(
            &config_path,
            "controller = 'http://127.0.0.1:9090'\nsecret = 's'\n",
        )
        .unwrap();
    }

    #[test]
    fn create_restore_roundtrip() {
        let _guard = env_lock().lock().unwrap();
        let (_dir, orig_cfg, orig_data) = isolate();
        seed_state();
        let backup = create().unwrap();
        assert!(backup.exists());
        // Wipe live state, restore, compare.
        fs::remove_file(Config::profiles_path()).unwrap();
        fs::remove_file(Config::profiles_dir().join("R1.yaml")).unwrap();
        restore(&backup).unwrap();
        assert!(
            fs::read_to_string(Config::profiles_path())
                .unwrap()
                .contains("uid: R1")
        );
        assert!(
            fs::read_to_string(Config::profiles_dir().join("R1.yaml"))
                .unwrap()
                .contains("proxies")
        );
        // The pre-restore snapshot keeps backups restorable in a chain.
        assert!(list().unwrap().len() >= 2);
        restore_env(orig_cfg, orig_data);
    }

    #[test]
    fn corrupt_backup_is_rejected_before_touching_live_state() {
        let _guard = env_lock().lock().unwrap();
        let (_dir, orig_cfg, orig_data) = isolate();
        seed_state();
        let before = fs::read(Config::profiles_path()).unwrap();
        // Craft a backup whose profiles.yaml is truncated garbage.
        let bad = Config::backups_dir().join("clashlime-bad.zip");
        fs::create_dir_all(Config::backups_dir()).unwrap();
        {
            let file = File::create(&bad).unwrap();
            let mut zip = ZipWriter::new(file);
            zip.start_file("profiles.yaml", SimpleFileOptions::default())
                .unwrap();
            io::copy(&mut "{truncated: [".as_bytes(), &mut zip).unwrap();
            zip.finish().unwrap();
        }
        let error = format!("{:#}", restore(&bad).expect_err("corrupt backup must fail"));
        assert!(error.contains("not valid YAML"), "{error}");
        assert_eq!(fs::read(Config::profiles_path()).unwrap(), before);
        restore_env(orig_cfg, orig_data);
    }
}
