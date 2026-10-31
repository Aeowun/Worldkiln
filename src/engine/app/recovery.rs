// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{App, EditorMode, View};
use std::fs;
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const RECOVERY_INTERVAL: Duration = Duration::from_secs(180);
const RETAIN_SNAPSHOTS: usize = 3;

struct RecoverySnapshot {
    project: PathBuf,
    world: Option<crate::world::World>,
    hierarchy: Option<crate::project::ProjectHierarchy>,
    ui: Option<crate::engine::ui_authoring::AuthoredUiWorkspace>,
    camera: Option<String>,
    scripts: Vec<(PathBuf, String)>,
}

impl RecoverySnapshot {
    fn capture(app: &mut App, project: &Path) -> Self {
        let project_dirty = app.editor.needs_save;
        if project_dirty {
            app.editor.commit_pending_inspector_history();
            app.editor.sync_active_ui_document();
        }
        let camera = project_dirty.then(|| {
            let camera = &app.editor.camera;
            format!(
                "{} {} {} {} {} {}",
                camera.yaw,
                camera.pitch,
                camera.distance,
                camera.target.x,
                camera.target.y,
                camera.target.z
            )
        });
        let scripts = app
            .editor
            .script_editor
            .open_documents
            .values()
            .filter(|d| d.dirty)
            .filter_map(|d| {
                let relative = d.path.strip_prefix(project).ok()?;
                if relative
                    .components()
                    .all(|part| matches!(part, std::path::Component::Normal(_)))
                {
                    Some((relative.to_path_buf(), d.source.clone()))
                } else {
                    None
                }
            })
            .collect();
        Self {
            project: project.to_path_buf(),
            world: project_dirty.then(|| app.world.clone()),
            hierarchy: project_dirty.then(|| app.editor.project_hierarchy.clone()),
            ui: project_dirty.then(|| app.editor.ui_workspace.clone()),
            camera,
            scripts,
        }
    }

    fn persist(self) -> Result<(), String> {
        let root = self.project.join(".worldkiln").join("recovery");
        fs::create_dir_all(&root).map_err(|e| e.to_string())?;
        let stamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis();
        let name = format!("snapshot-{stamp}-{:016x}", rand::random::<u64>());
        let staging = root.join(format!(".{name}.tmp"));
        let published = root.join(name);
        fs::create_dir(&staging).map_err(|e| e.to_string())?;
        let write = (|| -> Result<(), String> {
            if let Some(world) = &self.world {
                crate::world::persistence::save_world(world, &staging.join("world.dat"))
                    .map_err(|e| format!("Recovery World: {e}"))?;
            }
            if let Some(hierarchy) = &self.hierarchy {
                crate::project::save_project_metadata(&staging, hierarchy)
                    .map_err(|e| format!("Recovery hierarchy: {e}"))?;
            }
            if let Some(ui) = &self.ui {
                ui.save_to_project(&staging)
                    .map_err(|e| format!("Recovery UI: {e}"))?;
            }
            if let Some(camera) = &self.camera {
                crate::project::atomic_file::write_atomically(
                    &staging.join("camera.dat"),
                    camera.as_bytes(),
                )
                .map_err(|e| e.to_string())?;
            }
            for (relative, source) in &self.scripts {
                crate::project::atomic_file::write_atomically(
                    &staging.join(relative),
                    source.as_bytes(),
                )
                .map_err(|e| format!("Recovery script: {e}"))?;
            }
            let manifest = serde_json::json!({
                "schema":1, "source":"unsaved_editor_snapshot",
                "scripts":self.scripts.len(),
                "world":self.world.is_some()
            });
            crate::project::atomic_file::write_atomically(
                &staging.join("recovery.json"),
                serde_json::to_string_pretty(&manifest)
                    .map_err(|e| e.to_string())?
                    .as_bytes(),
            )
            .map_err(|e| e.to_string())?;
            fs::rename(&staging, &published).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if write.is_err() {
            let _ = fs::remove_dir_all(&staging);
        }
        write?;
        prune(&root)?;
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct RecoveryCandidate {
    pub(crate) name: String,
    pub(crate) includes_world: bool,
    pub(crate) script_count: usize,
}

fn recovery_root(project: &Path) -> PathBuf {
    project.join(".worldkiln").join("recovery")
}

fn checked_regular(path: &Path) -> Result<fs::Metadata, String> {
    let meta = fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if meta.file_type().is_symlink() {
        return Err(format!("Linked recovery content: {}", path.display()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return Err(format!("Redirected recovery content: {}", path.display()));
        }
    }
    Ok(meta)
}

fn validate_candidate(root: &Path, name: &str) -> Result<RecoveryCandidate, String> {
    if !name.starts_with("snapshot-")
        || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
    {
        return Err("Invalid recovery candidate name".into());
    }
    let dir = root.join(name);
    if !checked_regular(&dir)?.is_dir() {
        return Err("Recovery candidate is not a directory".into());
    }
    let manifest_path = dir.join("recovery.json");
    if !checked_regular(&manifest_path)?.is_file() {
        return Err("Recovery marker is missing".into());
    }
    let manifest: serde_json::Value =
        serde_json::from_slice(&fs::read(manifest_path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Invalid recovery marker: {e}"))?;
    if manifest.get("schema").and_then(|v| v.as_u64()) != Some(1)
        || manifest.get("source").and_then(|v| v.as_str()) != Some("unsaved_editor_snapshot")
    {
        return Err("Unsupported recovery snapshot".into());
    }
    let includes_world = manifest
        .get("world")
        .and_then(|v| v.as_bool())
        .ok_or("Recovery marker has no World status")?;
    let script_count = manifest
        .get("scripts")
        .and_then(|v| v.as_u64())
        .ok_or("Recovery marker has no script count")? as usize;
    if includes_world {
        for file in ["world.dat", "project.json", "ui.json", "camera.dat"] {
            if !checked_regular(&dir.join(file))?.is_file() {
                return Err(format!("Recovery snapshot missing {file}"));
            }
        }
    }
    let scripts_folder = dir.join("scripts");
    if script_count > 0 && !scripts_folder.exists() {
        let native_scripts = dir.join(".assets");
        if !native_scripts.exists() {
            return Err("Recovery snapshot is missing its dirty scripts".into());
        }
    }
    if !includes_world && script_count == 0 {
        return Err("Recovery snapshot contains no authored changes".into());
    }
    Ok(RecoveryCandidate {
        name: name.to_owned(),
        includes_world,
        script_count,
    })
}

pub(crate) fn recovery_candidates(project: &Path) -> Result<Vec<RecoveryCandidate>, String> {
    let root = recovery_root(project);
    if !root.is_dir() {
        return Ok(Vec::new());
    }
    let mut candidates = Vec::new();
    for item in fs::read_dir(&root).map_err(|e| e.to_string())? {
        let entry = item.map_err(|e| e.to_string())?;
        let name = entry.file_name().to_string_lossy().into_owned();
        if !name.starts_with("snapshot-") {
            continue;
        }
        if let Ok(candidate) = validate_candidate(&root, &name) {
            candidates.push(candidate);
        }
    }
    candidates.sort_by(|a, b| b.name.cmp(&a.name));
    Ok(candidates)
}

fn copy_checked_tree(
    source: &Path,
    destination: &Path,
    relative: &Path,
    recovery_overlay: bool,
) -> Result<(), String> {
    for item in fs::read_dir(source).map_err(|e| e.to_string())? {
        let item = item.map_err(|e| e.to_string())?;
        let name = relative.join(item.file_name());
        let name_string = name.to_string_lossy().replace('\\', "/");
        if !recovery_overlay
            && matches!(
                name_string.as_str(),
                ".worldkiln" | ".git" | "target" | "Builds"
            )
        {
            continue;
        }
        if recovery_overlay {
            let first = name
                .components()
                .next()
                .and_then(|c| c.as_os_str().to_str())
                .unwrap_or("");
            if !matches!(
                first,
                "world.dat"
                    | "project.json"
                    | "ui.json"
                    | "camera.dat"
                    | "chunks"
                    | "terrain"
                    | ".world_generations"
                    | "scripts"
                    | "playerscripts"
                    | ".assets"
                    | "recovery.json"
            ) {
                return Err(format!("Unexpected recovery file: {name_string}"));
            }
            if name_string == "recovery.json" {
                continue;
            }
        }
        let meta = checked_regular(&item.path())?;
        let target = destination.join(item.file_name());
        if meta.is_dir() {
            fs::create_dir_all(&target).map_err(|e| e.to_string())?;
            copy_checked_tree(&item.path(), &target, &name, recovery_overlay)?;
        } else if meta.is_file() {
            fs::copy(item.path(), &target).map_err(|e| e.to_string())?;
        } else {
            return Err(format!("Unsupported recovery content: {name_string}"));
        }
    }
    Ok(())
}

pub(crate) fn restore_recovery_to_new_project(
    project: &Path,
    name: &str,
    destination: &Path,
) -> Result<(), String> {
    let candidate = validate_candidate(&recovery_root(project), name)?;
    if destination.exists() || destination == project {
        return Err("Recovery destination already exists".into());
    }
    let parent = destination
        .parent()
        .ok_or("Recovery destination has no parent")?;
    if !parent.is_dir() {
        return Err("Choose an existing recovery destination folder".into());
    }
    let project_root = project.canonicalize().map_err(|e| e.to_string())?;
    let destination_parent = parent.canonicalize().map_err(|e| e.to_string())?;
    if destination_parent.starts_with(&project_root) {
        return Err("Recovery destination must be outside the original project".into());
    }
    let stage = parent.join(format!(
        ".worldkiln-recover-{:032x}",
        rand::random::<u128>()
    ));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let operation = (|| -> Result<(), String> {
        copy_checked_tree(project, &stage, Path::new(""), false)?;
        let source = recovery_root(project).join(&candidate.name);
        if candidate.includes_world {
            for folder in ["chunks", "terrain"] {
                let path = stage.join(folder);
                if path.exists() {
                    fs::remove_dir_all(path).map_err(|e| e.to_string())?;
                }
            }
        }
        copy_checked_tree(&source, &stage, Path::new(""), true)?;
        if destination.exists() {
            return Err("Recovery destination appeared during restoration".into());
        }
        fs::rename(&stage, destination).map_err(|e| e.to_string())
    })();
    if operation.is_err() {
        let _ = fs::remove_dir_all(&stage);
    }
    operation
}

fn prune(root: &Path) -> Result<(), String> {
    let mut completed = fs::read_dir(root)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?
        .into_iter()
        .filter(|entry| entry.file_name().to_string_lossy().starts_with("snapshot-"))
        .filter(|entry| entry.path().is_dir() && entry.path().join("recovery.json").is_file())
        .collect::<Vec<_>>();
    completed.sort_by_key(|entry| entry.file_name());
    let old_count = completed.len().saturating_sub(RETAIN_SNAPSHOTS);
    for entry in completed.into_iter().take(old_count) {
        fs::remove_dir_all(entry.path()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

impl App {
    pub(crate) fn service_recovery(&mut self) {
        if let Some(job) = self.recovery_job.as_ref() {
            match job.try_recv() {
                Ok(result) => {
                    self.recovery_job = None;
                    match result {
                        Ok(()) => self.recovery_last_failure = None,
                        Err(error) => {
                            if self.recovery_last_failure.as_deref() != Some(error.as_str()) {
                                eprintln!("[RECOVERY] {error}");
                            }
                            self.recovery_last_failure = Some(error);
                        }
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.recovery_job = None;
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.recovery_job.is_some()
            || self.recovery_last.elapsed() < RECOVERY_INTERVAL
            || self.view != View::Editor
            || self.editor.mode != EditorMode::Editor
            || self.project_loading()
            || self.project_load_failed
        {
            return;
        }
        let Some(project) = self.project_manager.current_project.clone() else {
            return;
        };
        self.recovery_last = Instant::now();
        let dirty_scripts = self
            .editor
            .script_editor
            .open_documents
            .values()
            .any(|d| d.dirty);
        if !self.editor.needs_save && !dirty_scripts {
            return;
        }
        let captured = RecoverySnapshot::capture(self, &project);
        let (send, recv) = mpsc::channel();
        self.recovery_job = Some(recv);
        std::thread::spawn(move || {
            let _ = send.send(captured.persist());
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn fixture(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_recovery_{label}_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ))
    }

    #[test]
    fn script_recovery_creates_independent_project_copy() {
        let root = fixture("script_copy");
        let project = root.join("Authored");
        fs::create_dir_all(project.join("scripts")).unwrap();
        fs::write(project.join("world.dat"), b"saved world").unwrap();
        fs::write(project.join("scripts/main.aeo"), b"saved script").unwrap();
        RecoverySnapshot {
            project: project.clone(),
            world: None,
            hierarchy: None,
            ui: None,
            camera: None,
            scripts: vec![(PathBuf::from("scripts/main.aeo"), "unsaved script".into())],
        }
        .persist()
        .unwrap();
        let candidates = recovery_candidates(&project).unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(!candidates[0].includes_world);
        assert_eq!(candidates[0].script_count, 1);
        let restored = root.join("Restored");
        restore_recovery_to_new_project(&project, &candidates[0].name, &restored).unwrap();
        assert_eq!(
            fs::read(project.join("scripts/main.aeo")).unwrap(),
            b"saved script"
        );
        assert_eq!(
            fs::read(restored.join("scripts/main.aeo")).unwrap(),
            b"unsaved script"
        );
        assert_eq!(
            fs::read(restored.join("world.dat")).unwrap(),
            b"saved world"
        );
        assert!(!restored.join(".worldkiln").exists());
        assert!(restore_recovery_to_new_project(&project, &candidates[0].name, &restored).is_err());
        assert!(
            restore_recovery_to_new_project(
                &project,
                &candidates[0].name,
                &project.join("NestedRecovered")
            )
            .is_err()
        );
        assert!(!project.join("NestedRecovered").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn world_recovery_replaces_stale_partitions_only_in_restored_copy() {
        let root = fixture("world_copy");
        let project = root.join("Authored");
        fs::create_dir_all(project.join("chunks")).unwrap();
        fs::write(project.join("world.dat"), b"previous world").unwrap();
        fs::write(project.join("chunks/stale.bin"), b"stale").unwrap();
        RecoverySnapshot {
            project: project.clone(),
            world: Some(crate::world::World::new()),
            hierarchy: Some(crate::project::ProjectHierarchy::new()),
            ui: Some(crate::engine::ui_authoring::AuthoredUiWorkspace::default()),
            camera: Some("0 0 10 0 0 0".into()),
            scripts: Vec::new(),
        }
        .persist()
        .unwrap();
        let candidates = recovery_candidates(&project).unwrap();
        assert_eq!(candidates.len(), 1);
        assert!(candidates[0].includes_world);
        let restored = root.join("Restored");
        restore_recovery_to_new_project(&project, &candidates[0].name, &restored).unwrap();
        assert_eq!(
            fs::read(project.join("world.dat")).unwrap(),
            b"previous world"
        );
        assert!(restored.join("world.dat").is_file());
        assert_ne!(
            fs::read(restored.join("world.dat")).unwrap(),
            b"previous world"
        );
        assert!(project.join("chunks/stale.bin").exists());
        assert!(!restored.join("chunks/stale.bin").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incomplete_or_invalid_recovery_is_never_offered() {
        let root = fixture("incomplete");
        let project = root.join("Authored");
        let recovery = recovery_root(&project);
        fs::create_dir_all(recovery.join("snapshot-01")).unwrap();
        fs::create_dir_all(recovery.join(".snapshot-stage.tmp")).unwrap();
        fs::write(recovery.join("snapshot-01/recovery.json"), b"{}").unwrap();
        assert!(recovery_candidates(&project).unwrap().is_empty());
        assert!(
            restore_recovery_to_new_project(&project, "../snapshot-01", &root.join("Restored"))
                .is_err()
        );
        assert!(!root.join("Restored").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn prune_retains_three_completed_snapshots_and_ignores_incomplete_staging() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_recovery_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        for n in 0..5 {
            let dir = root.join(format!("snapshot-{n:02}"));
            fs::create_dir(&dir).unwrap();
            fs::write(dir.join("recovery.json"), "{}").unwrap();
        }
        fs::create_dir(root.join(".snapshot-in-progress.tmp")).unwrap();
        prune(&root).unwrap();
        assert_eq!(fs::read_dir(&root).unwrap().count(), 4);
        assert!(root.join("snapshot-04").exists());
        assert!(!root.join("snapshot-00").exists());
        fs::remove_dir_all(root).unwrap();
    }
}
