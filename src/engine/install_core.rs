// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::Write,
    path::{Path, PathBuf},
};
const TARGETS: [&str; 4] = [
    "Worldkiln.exe",
    "LICENSE",
    ".assets/version.txt",
    "engine-core-manifest.json",
];
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    schema: u32,
    committed: bool,
    files: Vec<SavedFile>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct SavedFile {
    target: String,
    backup: String,
    sha256: Option<String>,
    bytes: u64,
}
fn checked(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(ancestor) {
            if meta.file_type().is_symlink() {
                return Err(format!("Linked engine update path: {}", ancestor.display()));
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err(format!(
                        "Reparse engine update path: {}",
                        ancestor.display()
                    ));
                }
            }
        }
    }
    Ok(())
}
fn receipt(stage: &Path) -> Result<Receipt, String> {
    checked(stage)?;
    let receipt_path = stage.join("receipt.json");
    checked(&receipt_path)?;
    let bytes = fs::read(&receipt_path).map_err(|error| {
        format!(
            "Failed to read engine recovery receipt {}: {error}",
            receipt_path.display()
        )
    })?;
    let value: Receipt = serde_json::from_slice(&bytes).map_err(|error| {
        format!(
            "Invalid engine recovery receipt {}: {error}",
            receipt_path.display()
        )
    })?;
    if value.schema != 1 || value.files.len() != TARGETS.len() {
        return Err(format!(
            "Unrecognized engine recovery receipt {}; preserved",
            receipt_path.display()
        ));
    }
    let mut seen = std::collections::BTreeSet::new();
    let mut allowed = std::collections::BTreeSet::from(["receipt.json".to_owned()]);
    for file in &value.files {
        if !TARGETS.contains(&file.target.as_str())
            || !seen.insert(&file.target)
            || !crate::project::asset_packs::safe_component(&file.backup)
        {
            return Err(format!(
                "Unsafe engine recovery receipt {}; preserved",
                receipt_path.display()
            ));
        }
        if let Some(digest) = &file.sha256 {
            if !allowed.insert(file.backup.clone()) {
                return Err(format!(
                    "Duplicate engine recovery backup in {}; preserved",
                    receipt_path.display()
                ));
            }
            let path = stage.join(&file.backup);
            checked(&path)?;
            if crate::engine::game_package::sha256_file(&path).map_err(|error| {
                format!(
                    "Failed to verify engine recovery backup {}: {error}",
                    path.display()
                )
            })? != (digest.clone(), file.bytes)
            {
                return Err(format!(
                    "Engine recovery verification failed for {}; preserved",
                    path.display()
                ));
            }
        }
    }
    for entry in fs::read_dir(stage).map_err(|error| {
        format!(
            "Failed to list engine recovery stage {}: {error}",
            stage.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to read engine recovery entry in {}: {error}",
                stage.display()
            )
        })?;
        checked(&entry.path())?;
        if !entry
            .file_type()
            .map_err(|error| {
                format!(
                    "Failed to inspect engine recovery entry {}: {error}",
                    entry.path().display()
                )
            })?
            .is_file()
            || !allowed.contains(&entry.file_name().to_string_lossy().into_owned())
        {
            return Err(format!(
                "Unexpected engine recovery contents at {}; preserved for inspection",
                entry.path().display()
            ));
        }
    }
    Ok(value)
}
fn restore(root: &Path, stage: &Path) -> Result<(), String> {
    let receipt = receipt(stage)?;
    if receipt.committed {
        return Ok(());
    }
    for file in receipt.files {
        let target = root.join(&file.target);
        checked(&target)?;
        if file.sha256.is_some() {
            fs::create_dir_all(target.parent().unwrap()).map_err(|error| {
                format!(
                    "Failed to create recovery directory {}: {error}",
                    target.parent().unwrap().display()
                )
            })?;
            let temporary =
                target.with_extension(format!("restore-{:032x}", rand::random::<u128>()));
            let backup = stage.join(&file.backup);
            fs::copy(&backup, &temporary).map_err(|error| {
                format!(
                    "Failed to copy engine recovery backup {} to {}: {error}",
                    backup.display(),
                    temporary.display()
                )
            })?;
            if target.exists() {
                fs::remove_file(&target).map_err(|error| {
                    format!(
                        "Failed to remove engine recovery target {}: {error}",
                        target.display()
                    )
                })?;
            }
            fs::rename(&temporary, &target).map_err(|error| {
                format!(
                    "Failed to restore engine file {} from {}: {error}",
                    target.display(),
                    temporary.display()
                )
            })?;
        } else if target.is_file() {
            fs::remove_file(&target).map_err(|error| {
                format!(
                    "Failed to remove new engine file during recovery {}: {error}",
                    target.display()
                )
            })?;
        }
    }
    Ok(())
}
fn preserve_legacy_unpublished_stage(root: &Path, stage: &Path) -> Result<bool, String> {
    let Some(name) = stage.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    let Some(id) = name.strip_prefix(".core-update-") else {
        return Ok(false);
    };
    if id.len() != 32
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Ok(false);
    }
    checked(stage)?;
    for entry in fs::read_dir(stage).map_err(|error| {
        format!(
            "Failed to inspect interrupted engine stage {}: {error}",
            stage.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to read interrupted engine stage entry in {}: {error}",
                stage.display()
            )
        })?;
        let path = entry.path();
        checked(&path)?;
        let metadata = fs::symlink_metadata(&path).map_err(|error| {
            format!(
                "Failed to inspect interrupted engine backup {}: {error}",
                path.display()
            )
        })?;
        if !metadata.is_file()
            || !["before-0", "before-1", "before-2", "before-3"]
                .iter()
                .any(|allowed| entry.file_name() == std::ffi::OsStr::new(allowed))
        {
            return Err(format!(
                "Unrecognized interrupted engine stage contents at {}; preserved for inspection",
                path.display()
            ));
        }
    }
    let preserved = root.join(format!(
        ".core-recovery-preserved-{id}-{:032x}",
        rand::random::<u128>()
    ));
    fs::create_dir(&preserved).map_err(|error| {
        format!(
            "Failed to reserve interrupted engine recovery directory {}: {error}",
            preserved.display()
        )
    })?;
    let destination = preserved.join("stage");
    fs::rename(stage, &destination).map_err(|error| {
        format!(
            "Failed to preserve interrupted engine stage {} at {}: {error}",
            stage.display(),
            destination.display()
        )
    })?;
    Ok(true)
}

fn retire_stage(root: &Path, stage: &Path) -> Result<PathBuf, String> {
    let retired = root.join(format!(".core-retired-{:032x}", rand::random::<u128>()));
    fs::create_dir(&retired).map_err(|error| {
        format!(
            "Failed to reserve completed engine recovery directory {}: {error}",
            retired.display()
        )
    })?;
    let destination = retired.join("stage");
    fs::rename(stage, &destination).map_err(|error| {
        format!(
            "Failed to retire completed engine recovery stage {} at {}: {error}",
            stage.display(),
            destination.display()
        )
    })?;
    Ok(retired)
}

fn finish_stage(root: &Path, stage: &Path) -> Result<(), String> {
    let retired = retire_stage(root, stage)?;
    fs::remove_dir_all(&retired).map_err(|error| {
        format!(
            "Failed to clean retired engine recovery stage {}: {error}",
            retired.display()
        )
    })
}

fn recover(root: &Path) -> Result<(), String> {
    for entry in fs::read_dir(root).map_err(|error| {
        format!(
            "Failed to list engine install directory {}: {error}",
            root.display()
        )
    })? {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect engine install entry in {}: {error}",
                root.display()
            )
        })?;
        if !entry
            .file_name()
            .to_string_lossy()
            .starts_with(".core-update-")
        {
            continue;
        }
        let stage = entry.path();
        if !stage.is_dir() {
            return Err(format!(
                "Unrecognized engine update stage {}; preserved",
                stage.display()
            ));
        }
        checked(&stage)?;
        let receipt_path = stage.join("receipt.json");
        match fs::symlink_metadata(&receipt_path) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if preserve_legacy_unpublished_stage(root, &stage)? {
                    continue;
                }
            }
            Err(error) => {
                return Err(format!(
                    "Failed to inspect engine recovery receipt {}: {error}",
                    receipt_path.display()
                ));
            }
            Ok(_) => {}
        }
        receipt(&stage)?;
        restore(root, &stage)?;
        finish_stage(root, &stage)?;
    }
    Ok(())
}
pub(crate) struct Transaction {
    root: PathBuf,
    stage: PathBuf,
    lock: PathBuf,
    finished: bool,
}
fn process_alive(pid: u32) -> Result<bool, String> {
    if pid == std::process::id() {
        return Ok(true);
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let output = std::process::Command::new("tasklist.exe")
            .args(["/FO", "CSV", "/NH", "/FI", &format!("PID eq {pid}")])
            .creation_flags(0x08000000)
            .output()
            .map_err(|error| {
                format!("Failed to start tasklist.exe to inspect installer process {pid}: {error}")
            })?;
        if !output.status.success() {
            return Err(format!(
                "tasklist.exe failed to inspect installer process {pid} ({}): {}; recovery retained",
                output.status,
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).lines().any(|line| {
            line.split("\",\"")
                .nth(1)
                .is_some_and(|part| part.trim_matches('"').parse::<u32>() == Ok(pid))
        }))
    }
    #[cfg(not(windows))]
    {
        Ok(Path::new("/proc").join(pid.to_string()).exists())
    }
}
impl Transaction {
    pub fn begin(root: &Path) -> Result<Self, String> {
        checked(root)?;
        fs::create_dir_all(root).map_err(|error| {
            format!(
                "Failed to create engine install directory {}: {error}",
                root.display()
            )
        })?;
        let lock = root.join(".core-update.lock");
        checked(&lock)?;
        if lock.exists() {
            let owner = fs::read_to_string(&lock)
                .map_err(|error| {
                    format!(
                        "Failed to read engine update lock {}: {error}",
                        lock.display()
                    )
                })?
                .trim()
                .parse::<u32>()
                .map_err(|error| {
                    format!(
                        "Unrecognized engine update lock {}; retained: {error}",
                        lock.display()
                    )
                })?;
            if owner == 0
                || process_alive(owner).map_err(|error| {
                    format!(
                        "Failed to inspect engine update lock {}: {error}",
                        lock.display()
                    )
                })?
            {
                return Err(format!(
                    "Engine update already active (lock {}); files preserved",
                    lock.display()
                ));
            }
            fs::remove_file(&lock).map_err(|error| {
                format!(
                    "Failed to remove stale engine update lock {}: {error}",
                    lock.display()
                )
            })?;
        }
        let mut guard = fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&lock)
            .map_err(|e| {
                format!("Failed to create engine update lock {}; engine update may be active or interrupted; recovery retained: {e}", lock.display())
            })?;
        guard
            .write_all(std::process::id().to_string().as_bytes())
            .and_then(|_| guard.sync_all())
            .map_err(|error| {
                format!(
                    "Failed to write engine update lock {}: {error}",
                    lock.display()
                )
            })?;
        drop(guard);
        let id = format!("{:032x}", rand::random::<u128>());
        let prepare = root.join(format!(".core-prepare-{id}"));
        let stage = root.join(format!(".core-update-{id}"));
        let result = (|| {
            recover(root)?;
            fs::create_dir(&prepare).map_err(|error| {
                format!(
                    "Failed to create engine recovery stage {}: {error}",
                    prepare.display()
                )
            })?;
            let mut files = Vec::new();
            for (index, target) in TARGETS.iter().enumerate() {
                let source = root.join(target);
                checked(&source)?;
                let backup = format!("before-{index}");
                let (sha256, bytes) = if source.exists() {
                    if !source.is_file() {
                        return Err(format!(
                            "Engine update target is not a file: {}",
                            source.display()
                        ));
                    }
                    let expected =
                        crate::engine::game_package::sha256_file(&source).map_err(|error| {
                            format!(
                                "Failed to hash installed engine file {}: {error}",
                                source.display()
                            )
                        })?;
                    let backup_path = prepare.join(&backup);
                    let mut input = fs::File::open(&source).map_err(|error| format!("Failed to open installed engine file {} for recovery snapshot: {error}", source.display()))?;
                    let mut output = fs::OpenOptions::new()
                        .write(true)
                        .create_new(true)
                        .open(&backup_path)
                        .map_err(|error| {
                            format!(
                                "Failed to create engine recovery backup {}: {error}",
                                backup_path.display()
                            )
                        })?;
                    std::io::copy(&mut input, &mut output).map_err(|error| {
                        format!(
                            "Failed to copy engine recovery snapshot {} to {}: {error}",
                            source.display(),
                            backup_path.display()
                        )
                    })?;
                    output.sync_all().map_err(|error| {
                        format!(
                            "Failed to sync staged engine recovery backup {}: {error}",
                            backup_path.display()
                        )
                    })?;
                    drop(output);
                    if crate::engine::game_package::sha256_file(&backup_path).map_err(|error| {
                        format!(
                            "Failed to verify staged engine recovery backup {}: {error}",
                            backup_path.display()
                        )
                    })? != expected
                    {
                        return Err(format!(
                            "Engine rollback snapshot changed during staging: {} (source {})",
                            prepare.join(&backup).display(),
                            source.display()
                        ));
                    }
                    (Some(expected.0), expected.1)
                } else {
                    (None, 0)
                };
                files.push(SavedFile {
                    target: target.to_string(),
                    backup,
                    sha256,
                    bytes,
                });
            }
            crate::project::atomic_file::write_atomically(
                &prepare.join("receipt.json"),
                &serde_json::to_vec_pretty(&Receipt {
                    schema: 1,
                    committed: false,
                    files,
                })
                .map_err(|error| {
                    format!(
                        "Failed to encode engine recovery receipt {}: {error}",
                        prepare.join("receipt.json").display()
                    )
                })?,
            )
            .map_err(|error| {
                format!(
                    "Failed to write engine recovery receipt {}: {error}",
                    prepare.join("receipt.json").display()
                )
            })?;
            receipt(&prepare)?;
            match fs::symlink_metadata(&stage) {
                Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                Err(error) => {
                    return Err(format!(
                        "Failed to inspect engine recovery publication target {}: {error}",
                        stage.display()
                    ));
                }
                Ok(_) => {
                    return Err(format!(
                        "Engine recovery publication target already exists {}; preserved",
                        stage.display()
                    ));
                }
            }
            fs::rename(&prepare, &stage).map_err(|error| {
                format!(
                    "Failed to publish verified engine recovery stage {} at {}: {error}",
                    prepare.display(),
                    stage.display()
                )
            })?;
            Ok(Self {
                root: root.into(),
                stage: stage.clone(),
                lock: lock.clone(),
                finished: false,
            })
        })();
        if result.is_err() {
            let _ = fs::remove_file(&lock);
        }
        result
    }
    pub fn commit(mut self) -> Result<(), String> {
        let mut value = receipt(&self.stage)?;
        value.committed = true;
        crate::project::atomic_file::write_atomically(
            &self.stage.join("receipt.json"),
            &serde_json::to_vec_pretty(&value).map_err(|error| {
                format!(
                    "Failed to encode committed engine recovery receipt {}: {error}",
                    self.stage.join("receipt.json").display()
                )
            })?,
        )
        .map_err(|error| {
            format!(
                "Failed to commit engine recovery receipt {}: {error}",
                self.stage.join("receipt.json").display()
            )
        })?;
        self.finished = true;
        finish_stage(&self.root, &self.stage)?;
        Ok(())
    }
    pub fn rollback(mut self) -> Result<(), String> {
        restore(&self.root, &self.stage)?;
        self.finished = true;
        finish_stage(&self.root, &self.stage)
    }
}
impl Drop for Transaction {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.lock);
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn installer_recovery_snapshots_readonly_source_without_changing_its_permissions() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-readonly-snapshot-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let license = root.join("LICENSE");
        fs::write(&license, b"installed license").unwrap();
        let mut permissions = fs::metadata(&license).unwrap().permissions();
        permissions.set_readonly(true);
        fs::set_permissions(&license, permissions).unwrap();
        let transaction = Transaction::begin(&root).unwrap();
        assert_eq!(
            fs::read(transaction.stage.join("before-1")).unwrap(),
            b"installed license"
        );
        assert!(fs::metadata(&license).unwrap().permissions().readonly());
        transaction.commit().unwrap();
        assert_eq!(fs::read(&license).unwrap(), b"installed license");
        assert!(fs::metadata(&license).unwrap().permissions().readonly());
        let mut permissions = fs::metadata(&license).unwrap().permissions();
        permissions.set_readonly(false);
        fs::set_permissions(&license, permissions).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_recovery_resumes_committed_stage_without_rolling_back_engine() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-commit-interrupt-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("Worldkiln.exe"), b"before update").unwrap();
        let transaction = Transaction::begin(&root).unwrap();
        let published = transaction.stage.clone();
        fs::write(root.join("Worldkiln.exe"), b"committed engine").unwrap();
        let mut committed = receipt(&published).unwrap();
        committed.committed = true;
        crate::project::atomic_file::write_atomically(
            &published.join("receipt.json"),
            &serde_json::to_vec(&committed).unwrap(),
        )
        .unwrap();
        drop(transaction);
        Transaction::begin(&root).unwrap().commit().unwrap();
        assert!(!published.exists());
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"committed engine"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_recovery_ignores_interrupted_retired_snapshot_cleanup() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-retire-interrupt-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        fs::write(root.join("Worldkiln.exe"), b"before update").unwrap();
        let transaction = Transaction::begin(&root).unwrap();
        fs::write(root.join("Worldkiln.exe"), b"committed engine").unwrap();
        let mut committed = receipt(&transaction.stage).unwrap();
        committed.committed = true;
        crate::project::atomic_file::write_atomically(
            &transaction.stage.join("receipt.json"),
            &serde_json::to_vec(&committed).unwrap(),
        )
        .unwrap();
        let retired = retire_stage(&root, &transaction.stage).unwrap();
        drop(transaction);
        fs::remove_file(retired.join("stage/before-0")).unwrap();
        Transaction::begin(&root).unwrap().commit().unwrap();
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"committed engine"
        );
        assert!(retired.join("stage/receipt.json").is_file());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_recovery_ignores_interrupted_prepare_and_preserves_its_bytes() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-prepare-interrupt-{:032x}",
            rand::random::<u128>()
        ));
        let prepare = root.join(format!(".core-prepare-{:032x}", 1_u128));
        fs::create_dir_all(&prepare).unwrap();
        fs::write(prepare.join("before-0"), b"partial snapshot").unwrap();
        fs::write(root.join("Worldkiln.exe"), b"current engine").unwrap();
        Transaction::begin(&root).unwrap().commit().unwrap();
        assert_eq!(
            fs::read(prepare.join("before-0")).unwrap(),
            b"partial snapshot"
        );
        assert!(!prepare.join("receipt.json").exists());
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"current engine"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_recovery_publishes_complete_receipt_before_allowing_engine_changes() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-publish-interrupt-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir_all(root.join("UserData/Project")).unwrap();
        fs::write(root.join("UserData/Project/world.dat"), b"authored").unwrap();
        fs::write(root.join("Worldkiln.exe"), b"before update").unwrap();
        let transaction = Transaction::begin(&root).unwrap();
        let published = transaction.stage.clone();
        assert!(
            published
                .file_name()
                .unwrap()
                .to_string_lossy()
                .starts_with(".core-update-")
        );
        receipt(&published).unwrap();
        assert_eq!(
            fs::read(published.join("before-0")).unwrap(),
            b"before update"
        );
        fs::write(root.join("Worldkiln.exe"), b"interrupted replacement").unwrap();
        drop(transaction);
        let resumed = Transaction::begin(&root).unwrap();
        assert!(!published.exists());
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"before update"
        );
        assert_eq!(
            fs::read(root.join("UserData/Project/world.dat")).unwrap(),
            b"authored"
        );
        resumed.commit().unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_recovery_preserves_known_legacy_orphan_without_restoring_it() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-legacy-orphan-{:032x}",
            rand::random::<u128>()
        ));
        let stage = root.join(format!(".core-update-{:032x}", 2_u128));
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("before-0"), b"unverified old engine").unwrap();
        fs::write(root.join("Worldkiln.exe"), b"current engine").unwrap();
        Transaction::begin(&root).unwrap().commit().unwrap();
        assert!(!stage.exists());
        let preserved: Vec<_> = fs::read_dir(&root)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .filter(|path| {
                path.file_name()
                    .unwrap()
                    .to_string_lossy()
                    .starts_with(".core-recovery-preserved-")
            })
            .collect();
        assert_eq!(preserved.len(), 1);
        assert_eq!(
            fs::read(preserved[0].join("stage/before-0")).unwrap(),
            b"unverified old engine"
        );
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"current engine"
        );
        Transaction::begin(&root).unwrap().commit().unwrap();
        assert_eq!(
            fs::read(preserved[0].join("stage/before-0")).unwrap(),
            b"unverified old engine"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_recovery_unknown_orphan_contents_fail_closed() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-unknown-orphan-{:032x}",
            rand::random::<u128>()
        ));
        let stage = root.join(format!(".core-update-{:032x}", 3_u128));
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("before-0"), b"saved engine").unwrap();
        fs::write(stage.join("authored.txt"), b"authored").unwrap();
        fs::write(root.join("Worldkiln.exe"), b"current engine").unwrap();
        let error = Transaction::begin(&root)
            .err()
            .expect("unknown contents must stop recovery");
        assert!(
            error.contains(&stage.join("authored.txt").display().to_string()),
            "{error}"
        );
        assert_eq!(fs::read(stage.join("before-0")).unwrap(), b"saved engine");
        assert_eq!(fs::read(stage.join("authored.txt")).unwrap(), b"authored");
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"current engine"
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_error_identifies_missing_recovery_receipt_and_preserves_stage() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-missing-receipt-{:032x}",
            rand::random::<u128>()
        ));
        let stage = root.join(".core-update-interrupted");
        fs::create_dir_all(&stage).unwrap();
        fs::write(stage.join("before-0"), b"saved engine").unwrap();
        fs::write(root.join("Worldkiln.exe"), b"installed engine").unwrap();
        let error = Transaction::begin(&root)
            .err()
            .expect("missing receipt must fail closed");
        assert!(
            error.contains("Failed to read engine recovery receipt"),
            "{error}"
        );
        assert!(
            error.contains(&stage.join("receipt.json").display().to_string()),
            "{error}"
        );
        assert_eq!(fs::read(stage.join("before-0")).unwrap(), b"saved engine");
        assert_eq!(
            fs::read(root.join("Worldkiln.exe")).unwrap(),
            b"installed engine"
        );
        assert!(!root.join(".core-update.lock").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_error_identifies_invalid_lock_without_removing_it() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-invalid-lock-{:032x}",
            rand::random::<u128>()
        ));
        fs::create_dir(&root).unwrap();
        let lock = root.join(".core-update.lock");
        fs::write(&lock, b"unknown owner").unwrap();
        let error = Transaction::begin(&root)
            .err()
            .expect("invalid lock must fail closed");
        assert!(error.contains("Unrecognized engine update lock"), "{error}");
        assert!(error.contains(&lock.display().to_string()), "{error}");
        assert_eq!(fs::read(&lock).unwrap(), b"unknown owner");
        fs::remove_dir_all(root).unwrap();
    }
}
