// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::{
    editor::script_editor::{ScriptDocument, ScriptEditor},
    world::World,
};
use std::{
    collections::BTreeMap,
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicU64, Ordering},
        mpsc,
    },
};

pub(super) struct PreparedProject {
    pub path: PathBuf,
    pub world: World,
    pub hierarchy: crate::project::ProjectHierarchy,
    pub scripts: PreparedScripts,
    pub editor_state: crate::project::EditorProjectState,
    pub ui: crate::engine::ui_authoring::AuthoredUiWorkspace,
    pub camera: Option<[f32; 6]>,
    pub needs_save: bool,
    pub characters: crate::character::package::CharacterPackageRegistry,
    pub entities: crate::engine::entity_package::EntityPackageRegistry,
    pub materials: crate::project::material::MaterialCatalog,
    pub terrain: Option<crate::renderer::terrain::PreparedProjectTerrain>,
    pub disk_snapshot: BTreeMap<PathBuf, u64>,
}
pub(super) struct PreparedScripts {
    source: Option<(PathBuf, String)>,
    scripts: Vec<PathBuf>,
    players: Vec<PathBuf>,
    scripts_tree: Option<crate::project::ScriptTree>,
    players_tree: Option<crate::project::ScriptTree>,
    index: Option<crate::project::ProjectScriptIndex>,
    problems: Vec<crate::editor::script_editor::ProjectProblem>,
}
impl PreparedScripts {
    fn capture(mut editor: ScriptEditor) -> Self {
        let source = editor.active_document.take().and_then(|path| {
            editor
                .open_documents
                .remove(&path)
                .map(|d| (path, d.source))
        });
        Self {
            source,
            scripts: editor.scripts_list,
            players: editor.playerscripts_list,
            scripts_tree: editor.scripts_tree,
            players_tree: editor.playerscripts_tree,
            index: editor.project_script_index,
            problems: editor.project_problems,
        }
    }
    pub fn publish(self) -> ScriptEditor {
        let mut editor = ScriptEditor::new();
        editor.scripts_list = self.scripts;
        editor.playerscripts_list = self.players;
        editor.scripts_tree = self.scripts_tree;
        editor.playerscripts_tree = self.players_tree;
        editor.project_script_index = self.index;
        editor.project_problems = self.problems;
        if let Some((path, source)) = self.source {
            editor
                .open_documents
                .insert(path.clone(), ScriptDocument::new(path.clone(), source));
            editor.active_document = Some(path);
        }
        editor
    }
}

type Completion = (u64, Result<PreparedProject, String>, Vec<String>);
enum Command {
    Load {
        token: u64,
        path: PathBuf,
        prepare_open: bool,
    },
    Retire(Box<World>),
    Recent {
        root: PathBuf,
        paths: Vec<PathBuf>,
    },
}
#[derive(Clone, Copy)]
pub(super) struct ProjectLoadProgress {
    pub completed: u8,
    pub total: u8,
    pub current: &'static str,
}
impl ProjectLoadProgress {
    fn from_completed(completed: u8) -> Self {
        let current = match completed.min(8) {
            0 => "Preparing project",
            1 => "Reading project files",
            2 => "Reading asset registries",
            3 => "Loading world.dat",
            4 => "Preparing scripts and editor",
            5 => "Loading authored UI",
            6 => "Preparing materials and terrain",
            7 => "Verifying project data",
            _ => "Opening editor",
        };
        Self {
            completed: completed.min(8),
            total: 9,
            current,
        }
    }
}

pub(super) struct BackgroundProjectLoader {
    sender: mpsc::Sender<Command>,
    latest: Arc<AtomicU64>,
    completed: Arc<Mutex<Option<Completion>>>,
    progress: Arc<AtomicU64>,
    pub pending: Option<u64>,
}
impl BackgroundProjectLoader {
    pub fn new() -> Self {
        let (sender, receiver) = mpsc::channel();
        let latest = Arc::new(AtomicU64::new(0));
        let completed = Arc::new(Mutex::new(None));
        let progress = Arc::new(AtomicU64::new(0));
        let worker_progress = progress.clone();
        let worker_latest = latest.clone();
        let worker_completed = completed.clone();
        std::thread::Builder::new()
            .name("Worldkiln project loader".into())
            .spawn(move || {
                while let Ok(command) = receiver.recv() {
                    match command {
                        Command::Retire(world) => drop(world),
                        Command::Recent { root, paths } => {
                            let recent = crate::project::recent::RecentProjects {
                                paths: paths
                                    .iter()
                                    .map(|p| p.to_string_lossy().into_owned())
                                    .collect(),
                            };
                            match serde_json::to_vec_pretty(&recent)
                                .map_err(|e| e.to_string())
                                .and_then(|bytes| {
                                    crate::project::atomic_file::write_atomically(
                                        &root.join("recent_projects.json"),
                                        &bytes,
                                    )
                                    .map_err(|e| e.to_string())
                                }) {
                                Ok(()) => (),
                                Err(error) => {
                                    crate::engine::diagnostics::report("ERROR", "Project", error)
                                }
                            }
                        }
                        Command::Load {
                            token,
                            path,
                            prepare_open,
                        } => {
                            let current = || worker_latest.load(Ordering::Acquire) == token;
                            if !current() {
                                continue;
                            }
                            crate::engine::diagnostics::clear();
                            let result =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                    prepare(path, prepare_open, token, &current, &worker_progress)
                                }))
                                .unwrap_or_else(|_| {
                                    Err(
                                    "Project preparation worker panicked; active project preserved"
                                        .into(),
                                )
                                });
                            if current() {
                                *worker_completed.lock().unwrap() =
                                    Some((token, result, crate::engine::diagnostics::drain()));
                            }
                        }
                    }
                }
            })
            .expect("project preparation worker");
        Self {
            sender,
            latest,
            completed,
            progress,
            pending: None,
        }
    }
    pub fn request(&mut self, path: PathBuf, prepare_open: bool) {
        let token = self.latest.fetch_add(1, Ordering::AcqRel) + 1;
        self.pending = Some(token);
        self.progress.store(token << 8, Ordering::Release);
        self.sender
            .send(Command::Load {
                token,
                path,
                prepare_open,
            })
            .expect("project worker alive");
    }
    pub fn progress(&self) -> ProjectLoadProgress {
        let progress = self.progress.load(Ordering::Acquire);
        let phase = if self.pending.is_some_and(|token| progress >> 8 == token) {
            (progress & 0xff) as u8
        } else {
            0
        };
        ProjectLoadProgress::from_completed(phase)
    }
    pub fn cancel(&mut self) {
        self.latest.fetch_add(1, Ordering::AcqRel);
        self.pending = None;
        self.progress.store(0, Ordering::Release);
    }
    pub fn poll(&mut self) -> Option<(Result<PreparedProject, String>, Vec<String>)> {
        let completion = self.completed.try_lock().ok()?.take()?;
        if self.pending != Some(completion.0) {
            return None;
        }
        self.pending = None;
        Some((completion.1, completion.2))
    }
    pub fn retire(&self, world: World) {
        let _ = self.sender.send(Command::Retire(Box::new(world)));
    }
    pub fn record_recent(&self, root: PathBuf, paths: Vec<PathBuf>) {
        let _ = self.sender.send(Command::Recent { root, paths });
    }
}
impl Drop for BackgroundProjectLoader {
    fn drop(&mut self) {
        self.cancel();
    }
}

fn prepare(
    mut path: PathBuf,
    prepare_open: bool,
    token: u64,
    current: &(impl Fn() -> bool + Sync),
    progress: &AtomicU64,
) -> Result<PreparedProject, String> {
    let check = || {
        if current() {
            Ok(())
        } else {
            Err("Superseded project load".to_owned())
        }
    };
    let advance = |phase: u8| {
        if current() {
            let _ = progress.fetch_update(Ordering::AcqRel, Ordering::Acquire, |value| {
                (value >> 8 == token).then_some((token << 8) | u64::from(phase))
            });
        }
    };
    let load_started = std::time::Instant::now();
    let mut stage_started = load_started;
    let mut timing = |stage: &str| {
        eprintln!(
            "[PROJECT LOAD] {stage}: {:.1} ms (total {:.1} ms)",
            stage_started.elapsed().as_secs_f64() * 1000.0,
            load_started.elapsed().as_secs_f64() * 1000.0
        );
        stage_started = std::time::Instant::now();
    };
    if prepare_open {
        path = crate::project::ProjectManager::prepare_open_project(path)?;
    }
    timing("prepare path");
    check()?;
    advance(1);
    advance(2);
    let before =
        crate::project::atomic_file::snapshot_persisted_files(&path).map_err(|e| e.to_string())?;
    let characters = crate::character::package::CharacterPackageRegistry::load(Some(&path));
    let entities = crate::engine::entity_package::EntityPackageRegistry::load(Some(&path));
    timing("snapshot and asset registries");
    check()?;
    advance(3);
    let mut world = World::new();
    crate::world::persistence::load_world(&mut world, &path.join("world.dat"))
        .map_err(|e| format!("Failed to load world.dat: {e}"))?;
    check()?;
    timing("world persistence");
    advance(4);
    let mut editor = crate::editor::Editor::new();
    editor.project_hierarchy = crate::project::load_project_metadata(&path)
        .map_err(|e| format!("Failed to load project.json: {e}"))?;
    editor.script_editor.refresh_scripts(&Some(path.clone()));
    check()?;
    super::app_project::reconcile_loaded_project(&mut editor, &mut world, &path);
    timing("editor construction, scripts and reconciliation");
    advance(5);
    let editor_state = crate::project::load_editor_state(&path)
        .map_err(|e| format!("Failed to load editor state: {e}"))?;
    if let Some(logical) = &editor_state.active_script {
        if let Some(file) = editor.script_editor.indexed_module_path(logical) {
            editor.script_editor.open_file(file);
        }
    }
    let ui = crate::engine::ui_authoring::AuthoredUiWorkspace::load_from_project(&path)
        .map_err(|e| format!("Failed to load project UI: {e}"))?;
    let camera = match std::fs::read_to_string(path.join("camera.dat")) {
        Ok(content) => Some(
            super::app_project::parse_camera_state(&content)
                .map_err(|e| format!("Invalid camera state: {e}"))?,
        ),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        Err(e) => return Err(format!("Failed to load camera state: {e}")),
    };
    check()?;
    timing("editor state, UI and camera");
    advance(6);
    let (materials, terrain) = std::thread::scope(|scope| {
        let catalog = scope.spawn(|| crate::project::material::MaterialCatalog::load(&path));
        let terrain = crate::renderer::terrain::prepare_project_terrain(&world, current);
        let materials = catalog
            .join()
            .map_err(|_| "Material catalog preparation panicked".to_owned())?;
        Ok::<_, String>((materials, terrain?))
    })?;
    check()?;
    timing("material catalog and terrain preparation (parallel)");
    advance(7);
    check()?;
    let disk_snapshot =
        crate::project::atomic_file::snapshot_persisted_files(&path).map_err(|e| e.to_string())?;
    timing("second persisted-file snapshot");
    if before != disk_snapshot {
        return Err(
            "Project files changed during background loading; active project preserved".into(),
        );
    }
    advance(8);
    Ok(PreparedProject {
        path,
        world,
        hierarchy: editor.project_hierarchy,
        scripts: PreparedScripts::capture(editor.script_editor),
        editor_state,
        ui,
        camera,
        needs_save: editor.needs_save,
        characters,
        entities,
        materials,
        terrain,
        disk_snapshot,
    })
}

#[cfg(test)]
mod background_project_tests {
    use super::*;

    #[test]
    fn background_project_dispatch_cancellation_and_latest_failure_are_nonblocking() {
        let mut loader = BackgroundProjectLoader::new();
        let missing =
            std::env::temp_dir().join(format!("missing-project-{:032x}", rand::random::<u128>()));
        let started = std::time::Instant::now();
        loader.request(missing.clone(), true);
        assert!(started.elapsed() < std::time::Duration::from_millis(250));
        loader.cancel();
        assert!(loader.poll().is_none());
        assert!(loader.pending.is_none());
        loader.request(missing.join("latest"), true);
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            if let Some((result, _)) = loader.poll() {
                assert!(result.is_err());
                break;
            }
            assert!(std::time::Instant::now() < deadline);
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        assert!(loader.pending.is_none());
    }
}
