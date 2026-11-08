// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug)]
pub(crate) struct InstallBackup {
    pub(crate) path: PathBuf,
}

fn checked_metadata(path: &Path) -> Result<Metadata, String> {
    let metadata = fs::symlink_metadata(path)
        .map_err(|err| format!("Cannot inspect {}: {err}", path.display()))?;
    if metadata.file_type().is_symlink() {
        return Err(format!("Refusing linked install path: {}", path.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if metadata.file_attributes() & 0x400 != 0 {
            return Err(format!("Refusing reparse point: {}", path.display()));
        }
    }
    Ok(metadata)
}

fn file_bytes_equal(left: &Path, right: &Path) -> Result<bool, String> {
    let (mut a, mut b) = (
        File::open(left).map_err(|err| format!("Cannot read {}: {err}", left.display()))?,
        File::open(right).map_err(|err| format!("Cannot read {}: {err}", right.display()))?,
    );
    let mut a_buf = [0u8; 64 * 1024];
    let mut b_buf = [0u8; 64 * 1024];
    loop {
        let a_len = a
            .read(&mut a_buf)
            .map_err(|err| format!("Read error: {err}"))?;
        let b_len = b
            .read(&mut b_buf)
            .map_err(|err| format!("Read error: {err}"))?;
        if a_len != b_len || a_buf[..a_len] != b_buf[..b_len] {
            return Ok(false);
        }
        if a_len == 0 {
            return Ok(true);
        }
    }
}

fn copy_file_new(source: &Path, dest: &Path) -> Result<(), String> {
    let mut input =
        File::open(source).map_err(|err| format!("Cannot open {}: {err}", source.display()))?;
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(dest)
        .map_err(|err| format!("Cannot create {}: {err}", dest.display()))?;
    std::io::copy(&mut input, &mut output)
        .map_err(|err| format!("Cannot copy {}: {err}", source.display()))?;
    output
        .flush()
        .map_err(|err| format!("Cannot flush {}: {err}", dest.display()))?;
    output
        .sync_all()
        .map_err(|err| format!("Cannot sync {}: {err}", dest.display()))?;
    if !file_bytes_equal(source, dest)? {
        return Err(format!(
            "Backup copy verification failed for {}",
            source.display()
        ));
    }
    Ok(())
}

fn copy_snapshot_tree(source: &Path, dest: &Path) -> Result<(), String> {
    if !checked_metadata(source)?.is_dir() {
        return Err(format!("Expected directory: {}", source.display()));
    }
    for entry in
        fs::read_dir(source).map_err(|err| format!("Cannot list {}: {err}", source.display()))?
    {
        let entry = entry.map_err(|err| format!("Cannot read directory entry: {err}"))?;
        let original = entry.path();
        let target = dest.join(entry.file_name());
        let metadata = checked_metadata(&original)?;
        if metadata.is_dir() {
            fs::create_dir(&target)
                .map_err(|err| format!("Cannot create {}: {err}", target.display()))?;
            copy_snapshot_tree(&original, &target)?;
        } else if metadata.is_file() {
            copy_file_new(&original, &target)?;
        } else {
            return Err(format!(
                "Unsupported install file type: {}",
                original.display()
            ));
        }
    }
    Ok(())
}

fn verify_snapshot_tree(source: &Path, backup: &Path) -> Result<(), String> {
    if !checked_metadata(backup)?.is_dir() {
        return Err(format!("Backup is not a directory: {}", backup.display()));
    }
    let mut source_count = 0usize;
    for entry in
        fs::read_dir(source).map_err(|err| format!("Cannot list {}: {err}", source.display()))?
    {
        let entry = entry.map_err(|err| format!("Cannot read directory entry: {err}"))?;
        source_count += 1;
        let from = entry.path();
        let to = backup.join(entry.file_name());
        let from_meta = checked_metadata(&from)?;
        let to_meta = checked_metadata(&to)?;
        if from_meta.is_dir() && to_meta.is_dir() {
            verify_snapshot_tree(&from, &to)?;
        } else if from_meta.is_file() && to_meta.is_file() {
            if !file_bytes_equal(&from, &to)? {
                return Err(format!("Snapshot mismatch: {}", from.display()));
            }
        } else {
            return Err(format!("Snapshot file-type mismatch: {}", from.display()));
        }
    }
    let backup_count = fs::read_dir(backup)
        .map_err(|err| format!("Cannot list backup {}: {err}", backup.display()))?
        .count();
    if source_count != backup_count {
        return Err(format!(
            "Snapshot entry-count mismatch in {}",
            source.display()
        ));
    }
    Ok(())
}

fn installer_backup_name(name: &str) -> bool {
    let Some(name) = name.strip_prefix("Aeowun-") else {
        return false;
    };
    let Some((stamp, rest)) = name.split_once('-') else {
        return false;
    };
    let pieces: Vec<_> = rest.split('-').collect();
    stamp.len() >= 10
        && stamp.bytes().all(|b| b.is_ascii_digit())
        && pieces.len() == 2
        && pieces
            .iter()
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
}

fn purge_old_verified_backups(root: &Path, current: &Path) -> Result<(), String> {
    if !root.exists() {
        return Ok(());
    }
    if !checked_metadata(root)?.is_dir() {
        return Err("Backup root is not a safe directory".into());
    }
    for item in fs::read_dir(root).map_err(|err| format!("Cannot list backup directory: {err}"))? {
        let item = item.map_err(|err| format!("Cannot inspect backup entry: {err}"))?;
        let path = item.path();
        if path == current {
            continue;
        }
        let name = item.file_name().to_string_lossy().into_owned();
        let is_installer_snapshot = installer_backup_name(&name)
            || name
                .strip_prefix('.')
                .and_then(|n| n.strip_suffix(".incomplete"))
                .is_some_and(installer_backup_name);
        if !is_installer_snapshot {
            continue;
        }
        if !checked_metadata(&path)?.is_dir() {
            continue;
        }
        fs::remove_dir_all(&path).map_err(|err| {
            format!(
                "Cannot remove retired installer backup {}: {err}",
                path.display()
            )
        })?;
    }
    Ok(())
}

pub(crate) fn finish_install_backup(backup: &InstallBackup) -> Result<(), String> {
    let parent = backup.path.parent().ok_or("Backup path has no parent")?;
    let name = backup
        .path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or("Invalid backup path")?;
    if !installer_backup_name(name)
        || parent.file_name().and_then(|name| name.to_str()) != Some("Aeowun-Backups")
    {
        return Err("Refusing to clean a non-installer backup path".into());
    }
    if !checked_metadata(parent)?.is_dir() || !checked_metadata(&backup.path)?.is_dir() {
        return Err("Refusing unsafe installer backup cleanup".into());
    }
    fs::remove_dir_all(&backup.path).map_err(|err| {
        format!(
            "Could not clean completed install backup {}: {err}",
            backup.path.display()
        )
    })
}

pub(crate) fn create_install_backup(install_dir: &Path) -> Result<Option<InstallBackup>, String> {
    let userdata = install_dir.join("UserData");
    match fs::symlink_metadata(&userdata) {
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(format!("Cannot inspect existing UserData: {err}")),
        Ok(_) => {}
    }
    if !checked_metadata(&userdata)?.is_dir() || !checked_metadata(install_dir)?.is_dir() {
        return Err("Existing UserData or install location is not a safe directory".into());
    }

    let parent = install_dir
        .parent()
        .ok_or("Install directory has no parent")?;
    let backups = parent.join("Aeowun-Backups");
    if backups.exists() {
        if !checked_metadata(&backups)?.is_dir() {
            return Err(format!("Unsafe backup location: {}", backups.display()));
        }
    } else {
        fs::create_dir(&backups)
            .map_err(|err| format!("Cannot create recovery directory: {err}"))?;
    }

    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|err| format!("Cannot generate backup timestamp: {err}"))?
        .as_millis();
    for attempt in 0..100 {
        let name = format!("Aeowun-{stamp}-{}-{attempt}", std::process::id());
        let destination = backups.join(&name);
        let pending = backups.join(format!(".{name}.incomplete"));
        if destination.exists() {
            continue;
        }
        match fs::create_dir(&pending) {
            Ok(()) => {}
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(err) => return Err(format!("Cannot stage recovery backup: {err}")),
        }
        let pending_userdata = pending.join("UserData");
        let result = fs::create_dir(&pending_userdata)
            .map_err(|err| format!("Cannot stage UserData backup: {err}"))
            .and_then(|()| copy_snapshot_tree(&userdata, &pending_userdata))
            .and_then(|()| verify_snapshot_tree(&userdata, &pending_userdata));
        if let Err(err) = result {
            let _ = fs::remove_dir_all(&pending);
            return Err(format!(
                "Installation stopped before changing files because backup failed: {err}"
            ));
        }
        fs::rename(&pending, &destination).map_err(|err| {
            format!(
                "Backup verified but could not be finalized at {}: {err}; installation stopped",
                pending.display()
            )
        })?;
        if let Err(error) = purge_old_verified_backups(&backups, &destination) {
            eprintln!("[Installer] Recovery snapshot retention: {error}");
        }
        return Ok(Some(InstallBackup { path: destination }));
    }
    Err("Cannot allocate a unique recovery backup folder".into())
}

fn preflight_merge(source: &Path, destination: &Path) -> Result<(), String> {
    let saved = checked_metadata(source)?;
    let installed = match fs::symlink_metadata(destination) {
        Ok(_) => Some(checked_metadata(destination)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("Cannot inspect {}: {err}", destination.display())),
    };
    match installed {
        None => Ok(()),
        Some(installed) if saved.is_file() && installed.is_file() => {
            if file_bytes_equal(source, destination)? {
                Ok(())
            } else {
                Err(format!(
                    "UserData conflict at {}; original is in recovery backup",
                    destination.display()
                ))
            }
        }
        Some(installed) if saved.is_dir() && installed.is_dir() => {
            for entry in
                fs::read_dir(source).map_err(|err| format!("Cannot list saved UserData: {err}"))?
            {
                let entry = entry.map_err(|err| format!("Cannot read saved entry: {err}"))?;
                preflight_merge(&entry.path(), &destination.join(entry.file_name()))?;
            }
            Ok(())
        }
        _ => Err(format!(
            "UserData path type conflict at {}",
            destination.display()
        )),
    }
}

fn restore_missing(source: &Path, destination: &Path) -> Result<(), String> {
    let source_meta = checked_metadata(source)?;
    let destination_meta = match fs::symlink_metadata(destination) {
        Ok(_) => Some(checked_metadata(destination)?),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => None,
        Err(err) => return Err(format!("Cannot inspect {}: {err}", destination.display())),
    };
    if source_meta.is_dir() {
        if destination_meta.is_none() {
            fs::create_dir(destination)
                .map_err(|err| format!("Cannot restore {}: {err}", destination.display()))?;
        }
        for entry in
            fs::read_dir(source).map_err(|err| format!("Cannot list saved directory: {err}"))?
        {
            let entry = entry.map_err(|err| format!("Cannot read saved entry: {err}"))?;
            restore_missing(&entry.path(), &destination.join(entry.file_name()))?;
        }
    } else if source_meta.is_file() && destination_meta.is_none() {
        copy_file_new(source, destination)?;
    }
    Ok(())
}

pub(crate) fn reconcile_userdata(backup: &InstallBackup, install_dir: &Path) -> Result<(), String> {
    let source = backup.path.join("UserData");
    let destination = install_dir.join("UserData");
    preflight_merge(&source, &destination)
        .and_then(|()| restore_missing(&source, &destination))
        .and_then(|()| preflight_merge(&source, &destination))
        .map_err(|err| format!("{err}. Recovery backup: {}", backup.path.display()))
}

pub(crate) fn detected_legacy_assets(_backup: &InstallBackup) -> Vec<PathBuf> {
    Vec::new()
}

#[cfg(test)]
mod backup_retention_tests {
    use super::*;
    #[test]
    fn newer_verified_snapshot_retires_older_but_not_arbitrary_user_folders() {
        let root = std::env::temp_dir().join(format!("wk_backup_retention_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        let install = root.join("Aeowun");
        fs::create_dir_all(install.join("UserData/Projects")).unwrap();
        fs::write(install.join("UserData/Projects/game.txt"), b"original").unwrap();
        fs::create_dir_all(install.join(".assets")).unwrap();
        fs::write(install.join(".assets/engine-data.bin"), b"engine").unwrap();
        let first = create_install_backup(&install).unwrap().unwrap();
        assert!(!first.path.join(".assets").exists());
        assert!(first.path.exists());
        let protected = root.join("Aeowun-Backups/MyPersonalBackup");
        fs::create_dir_all(&protected).unwrap();
        let stale = root.join("Aeowun-Backups/.Aeowun-1234567890000-123-0.incomplete");
        fs::create_dir_all(&stale).unwrap();
        let second = create_install_backup(&install).unwrap().unwrap();
        assert!(!stale.exists(), "old incomplete backup must be removed");
        assert!(
            !first.path.exists(),
            "old snapshot must be retired after new verified snapshot"
        );
        assert!(
            second.path.exists(),
            "current failure recovery must be retained"
        );
        assert!(
            protected.exists(),
            "unrecognized user directories must not be deleted"
        );
        assert_eq!(
            fs::read(second.path.join("UserData/Projects/game.txt")).unwrap(),
            b"original"
        );
        finish_install_backup(&second).unwrap();
        assert!(
            !second.path.exists(),
            "successful install must delete its temporary snapshot"
        );
        assert!(protected.exists());
        assert_eq!(
            fs::read(install.join("UserData/Projects/game.txt")).unwrap(),
            b"original"
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn backup_cleanup_rejects_unrecognized_paths() {
        assert!(!installer_backup_name("MyPersonalBackup"));
        assert!(!installer_backup_name(".Aeowun-123-4-0.incomplete"));
        assert!(installer_backup_name("Aeowun-1234567890000-123-0"));
    }
}
