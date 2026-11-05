// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub mod app_home;
mod home_dev_notes;
pub mod app_input;
pub mod app_project;
pub mod app_render;
pub mod app_runtime;
pub mod app_ui;
mod background_project;
mod character_picker;
mod live_project_sync;
mod new_project_character;
mod pack_install_queue;
mod recovery;

use std::collections::{BTreeMap, HashMap, HashSet};
use std::time::Instant;

use app_project::get_timestamp;

use winit::keyboard::KeyCode;

use crate::character::CharacterSystem;
use crate::editor::Editor;
use crate::engine::attachment::AttachmentSystem;
use crate::engine::entity::EntityManager;
use crate::engine::ui::RuntimeUi;
use crate::project::ProjectManager;
use crate::renderer::Renderer;
use crate::renderer::camera::{CameraController, GameplayCamera};
use crate::scripting::scene::ScriptScene;
use crate::scripting::value::{DynamicProperty, HandleKind, Value};
use crate::world::World;

use super::{EditorMode, View, physics};

pub use app_input::{PendingActorGrab, PendingGrab, PendingSourceGrab};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WindowAction {
    Drag,
    MaximizeRestore,
    Minimize,
    Close,
}

pub(crate) const COLOR_VOID: egui::Color32 = egui::Color32::from_rgb(1, 1, 2);

pub(crate) const COLOR_VOID_ELEVATED: egui::Color32 = egui::Color32::from_rgb(6, 7, 9);

pub(crate) const COLOR_VOID_PANEL: egui::Color32 = egui::Color32::from_rgb(10, 12, 15);

pub(crate) const COLOR_BORDER: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(255, 255, 255, 20);

pub(crate) const COLOR_BORDER_BRIGHT: egui::Color32 =
    egui::Color32::from_rgba_premultiplied(255, 255, 255, 51);

pub(crate) const COLOR_TEXT: egui::Color32 = egui::Color32::from_rgb(160, 164, 171);

pub(crate) const COLOR_TEXT_DIM: egui::Color32 = egui::Color32::from_rgb(92, 98, 108);

pub(crate) const COLOR_TEXT_BRIGHT: egui::Color32 = egui::Color32::from_rgb(255, 255, 255);

pub(crate) const COLOR_ACCENT_RED: egui::Color32 = egui::Color32::from_rgb(158, 52, 29);

pub(crate) const COLOR_ACCENT_ORANGE: egui::Color32 = egui::Color32::from_rgb(228, 91, 36);

pub(crate) const COLOR_ACCENT_LIGHT: egui::Color32 = egui::Color32::from_rgb(255, 173, 99);

pub(crate) const COLOR_ACCENT_CREAM: egui::Color32 = egui::Color32::from_rgb(255, 225, 194);

pub(crate) const COLOR_ACCENT_COOL: egui::Color32 = egui::Color32::from_rgb(96, 115, 125);

pub(crate) const RMB_GUARD_MAX_RADIUS_FACTOR: f32 = 0.45;
pub(crate) const RMB_GUARD_INNER_RADIUS_FACTOR: f32 = 0.38;
pub(crate) const RMB_GUARD_EDGE_SCROLL_SPEED: f32 = 500.0;

pub struct App {
    pub splash_started: Instant,
    pub development_build: bool,
    pub development_capture:
        std::cell::RefCell<crate::engine::development_test_run::TestRunRecorder>,
    pub(crate) development_session_project: std::cell::RefCell<Option<std::path::PathBuf>>,
    pub(crate) runtime_scene_analysis: crate::engine::runtime_scene_analysis::SceneAnalysis,
    pub(crate) development_project_load_started: Option<Instant>,
    pub(crate) development_confirm_replace: bool,
    pub(crate) development_report_message: Option<String>,
    pub(crate) development_save_task: Option<crate::engine::development_test_run::CaptureSaveTask>,
    pub(crate) development_summary_cache:
        Option<crate::engine::development_test_run::CaptureSummary>,
    pub(crate) development_summary_refreshed: Instant,
    pub(crate) development_load_task: Option<(
        bool,
        std::thread::JoinHandle<Result<crate::engine::development_capture::Capture, String>>,
    )>,

    pub view: View,
    pub renderer: Renderer,
    pub editor: Editor,
    pub world: World,
    pub project_manager: ProjectManager,
    pub(crate) project_load_failed: bool,
    pub(crate) project_load_error: Option<String>,
    project_loader: Option<background_project::BackgroundProjectLoader>,
    project_loading_screen: Option<(Instant, String)>,
    loaded_project_path: Option<std::path::PathBuf>,
    pub(crate) project_disk_snapshot: Option<BTreeMap<std::path::PathBuf, u64>>,
    live_project_sync: Option<live_project_sync::LiveProjectSync>,

    pub last_frame_instant: Instant,
    pub physics_clock: physics::PhysicsClock,
    pub physics_world: physics::PhysicsWorld,
    pub last_mode: EditorMode,

    pub play_mode_hint_started: Option<Instant>,

    pub show_new_project_dialog: bool,
    pub new_project_catalog_open: bool,
    pub new_project_name: String,
    pub(crate) new_project_name_warning: Option<Instant>,
    pub(crate) new_project_type: crate::project::character_offerings::Setup,
    pub(crate) background_character_jobs: Vec<new_project_character::CharacterSetupJob>,
    pub(crate) background_asset_pack_jobs: Vec<pack_install_queue::PackInstallQueue>,
    pub(crate) new_project_packs: app_home::NewProjectPacks,
    pub(crate) asset_manager_packs: app_home::NewProjectPacks,
    pub(crate) asset_manager_packs_open: bool,
    pub show_open_project_dialog: bool,
    pub show_unsaved_scripts_dialog: bool,
    pub home_emblem_texture: Option<egui::TextureHandle>,
    pub(crate) home_dev_notes: home_dev_notes::DeveloperNotesFeed,
    pub(crate) home_member: crate::engine::member_services::MemberClient,
    pub(crate) home_publish_build: Option<crate::project::project::BuiltGame>,
    pub(crate) home_management: Option<app_home::HomeManagement>,
    pub(crate) home_management_message: Option<String>,
    pub(crate) home_recovery: Option<(std::path::PathBuf, Vec<recovery::RecoveryCandidate>, usize)>,
    pub(crate) home_recovery_job:
        Option<std::sync::mpsc::Receiver<Result<std::path::PathBuf, String>>>,

    pub is_right_mouse_down: bool,
    pub is_middle_mouse_down: bool,
    pub last_cursor_pos: Option<(f64, f64)>,
    pub mouse_pos: (f64, f64),
    pub mouse: crate::engine::mouse::MouseController,

    pub rmb_edge_scroll_velocity: glam::Vec2,
    pub last_set_cursor_pos: Option<(f64, f64)>,

    pub is_left_mouse_down: bool,
    pub(crate) gameplay_left_mouse_captured: bool,
    pub(crate) sculpt_pointer_pending: bool,
    pub drag_start_coord: Option<crate::world::WorldCoord>,

    pub keys_down: HashSet<KeyCode>,
    pub(crate) editor_shortcut_keys: HashSet<KeyCode>,
    pub jump_requested: bool,

    pub character_system: CharacterSystem,
    pub attachment_system: AttachmentSystem,
    pub actor_system: crate::actor::ActorSystem,
    pub gameplay_camera: GameplayCamera,
    pub saved_editor_camera: Option<CameraController>,
    pub(crate) saved_editor_actors:
        Option<std::collections::HashMap<u64, crate::world::actor::Actor>>,

    pub script_scene: Option<ScriptScene>,

    pub script_dynamic_properties: HashMap<(HandleKind, u64), BTreeMap<String, DynamicProperty>>,

    pub script_contacted_last_frame: HashSet<u64>,

    pub script_pending_events: Vec<(String, Vec<Value>)>,

    pub script_test_results: BTreeMap<String, bool>,

    pub script_pending_enable: Vec<String>,
    pub script_pending_disable: Vec<String>,

    pub script_pending_spawns: Vec<(String, u64)>,
    pub script_pending_destroys: Vec<u64>,

    pub save_state: Option<crate::engine::save_state::SaveState>,

    pub show_exit_confirmation_dialog: bool,
    pub exit_requested: bool,
    pub window_action: Option<WindowAction>,

    pub authored_disabled_scripts: Vec<String>,

    pub entity_manager: EntityManager,

    pub runtime_ui: RuntimeUi,

    pub controller_object: Option<Value>,
    pub camera_object: Option<Value>,

    pub orbit_delta: [f32; 2],

    pub audio_system: crate::engine::audio::AudioSystem,

    pub pending_grab: Option<PendingGrab>,
    pub pending_actor_grab: Option<PendingActorGrab>,
    pub pending_source_grab: Option<PendingSourceGrab>,

    pub autosave_last: Instant,
    pub last_autosave_interval: crate::engine::preferences::AutosaveInterval,
    pub last_autosave_failure: Option<String>,
    pub(crate) recovery_last_failure: Option<String>,
    pub(crate) recovery_last: Instant,
    pub(crate) recovery_job: Option<std::sync::mpsc::Receiver<Result<(), String>>>,
    pub(crate) packaging_open: bool,
    game_state: std::sync::Arc<std::sync::Mutex<crate::scripting::api::GameLifecycleState>>,
    pub(crate) packaging_options: crate::engine::build::GameBuildOptions,
    pub(crate) packaging_run_after: bool,
    pub(crate) packaging_status: String,
    pub(crate) last_built_zip: Option<std::path::PathBuf>,
    pub(crate) publish_description: String,
    pub(crate) publish_rights: bool,
    pub(crate) publish_safety: bool,
    pub(crate) publish_publication: bool,
    pub(crate) packaging_cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    pub(crate) game_build_job:
        Option<std::sync::mpsc::Receiver<Result<crate::engine::build::BuildResult, String>>>,
}

impl App {
    pub fn start_test_run(&self, discard_unsaved: bool) -> bool {
        let mut capture = self.development_capture.borrow_mut();
        let generation = capture.generation();
        let started = capture.start(discard_unsaved);
        if started && capture.generation() != generation {
            let caps = self.renderer.capabilities();
            capture.conditions.hardware = std::collections::BTreeMap::from([
                (
                    "cpu".into(),
                    std::env::var("PROCESSOR_IDENTIFIER").unwrap_or_else(|_| "Unavailable".into()),
                ),
                ("gpu".into(), caps.renderer.clone()),
                ("gpu_vendor".into(), caps.vendor.clone()),
                ("opengl".into(), caps.version.clone()),
                ("os".into(), std::env::consts::OS.into()),
                ("architecture".into(), std::env::consts::ARCH.into()),
            ]);
            capture.conditions.build = std::env::current_exe()
                .ok()
                .and_then(|p| std::fs::metadata(p).ok())
                .map(|m| {
                    format!(
                        "{}:{:?}:debug={}",
                        m.len(),
                        m.modified().ok(),
                        cfg!(debug_assertions)
                    )
                })
                .unwrap_or_default();
            capture
                .conditions
                .build
                .push_str(&format!(";commit={}", env!("WORLDKILN_SOURCE_COMMIT")));
            capture.conditions.settings = std::collections::BTreeMap::from([
                (
                    "lighting".into(),
                    format!("{:?}", self.renderer.active_lighting_settings()),
                ),
                (
                    "terrain".into(),
                    serde_json::to_string(&self.world.terrain.config).unwrap_or_default(),
                ),
                (
                    "viewport".into(),
                    format!(
                        "{}x{}",
                        self.editor.viewport_rect.width(),
                        self.editor.viewport_rect.height()
                    ),
                ),
                ("play".into(), format!("{:?}", self.editor.mode)),
            ]);
            if !cfg!(test) {
                crate::engine::development_capture::engine_session(
                    capture.generation(),
                    capture.config().detailed,
                );
            }

            *self.development_session_project.borrow_mut() =
                self.project_manager.current_project.clone();
        }
        started
    }

    pub fn last_test_run_log_directory(&self) -> Option<std::path::PathBuf> {
        self.development_capture
            .borrow()
            .last_saved_directory()
            .map(std::path::Path::to_path_buf)
            .or_else(|| {
                self.project_manager
                    .current_project
                    .as_ref()
                    .map(|project| {
                        crate::engine::development_test_run::TestRunRecorder::log_directory(project)
                    })
                    .filter(|path| path.is_dir())
            })
    }

    pub fn pause_test_run(&self) {
        self.development_capture.borrow_mut().pause_display();
    }

    pub fn stop_test_run(&self) {
        if !cfg!(test) {
            let (samples, dropped) = crate::engine::development_capture::drain_engine();
            crate::engine::development_capture::engine_session(0, false);
            self.development_capture
                .borrow_mut()
                .collect_spans(samples, dropped);
        }
        #[cfg(feature = "editor")]
        self.development_capture
            .borrow_mut()
            .note_missing_samples(self.renderer.pending_development_samples());
        self.development_capture.borrow_mut().stop();
    }

    pub fn save_test_run(&self) -> std::io::Result<std::path::PathBuf> {
        let project = self.development_session_project.borrow();
        let project = project.as_deref().ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, "session has no project")
        })?;
        self.development_capture.borrow_mut().save(project)
    }

    pub fn save_test_run_async(
        &self,
    ) -> std::io::Result<crate::engine::development_test_run::CaptureSaveTask> {
        let project = self
            .development_session_project
            .borrow()
            .clone()
            .ok_or_else(|| {
                std::io::Error::new(std::io::ErrorKind::NotFound, "session has no project")
            })?;
        self.development_capture.borrow().save_async(project)
    }

    pub fn finish_test_run_save(
        &self,
        task: crate::engine::development_test_run::CaptureSaveTask,
    ) -> std::io::Result<std::path::PathBuf> {
        task.finish(&mut self.development_capture.borrow_mut())
    }

    pub(crate) fn play_engine_sound(&self, sound: crate::engine::audio::EngineSound) {
        self.audio_system
            .play_engine_sound(sound, &self.editor.engine_audio_settings);
    }

    pub fn new(width: f32, height: f32) -> Self {
        Self::with_renderer_and_audio(
            Renderer::new(width, height),
            crate::engine::audio::AudioSystem::new(),
        )
    }

    #[cfg(test)]
    pub(crate) fn new_for_tests(width: f32, height: f32) -> Self {
        Self::with_renderer_and_audio(
            Renderer::new_for_tests(width, height),
            crate::engine::audio::AudioSystem::new_for_tests(),
        )
    }

    fn with_renderer_and_audio(
        renderer: Renderer,
        audio_system: crate::engine::audio::AudioSystem,
    ) -> Self {
        Self {
            view: View::Splash,

            renderer,
            editor: Editor::new(),
            world: World::new(),
            project_manager: ProjectManager::new(),
            project_load_failed: false,
            project_load_error: None,
            project_loader: None,
            project_loading_screen: None,
            loaded_project_path: None,
            project_disk_snapshot: None,
            live_project_sync: None,

            splash_started: Instant::now(),
            development_build: cfg!(debug_assertions),
            development_capture: std::cell::RefCell::new(Default::default()),
            development_session_project: std::cell::RefCell::new(None),
            runtime_scene_analysis: Default::default(),
            development_project_load_started: None,
            development_confirm_replace: false,
            development_report_message: None,
            development_save_task: None,
            development_summary_cache: None,
            development_summary_refreshed: Instant::now(),
            development_load_task: None,
            last_frame_instant: Instant::now(),

            physics_clock: physics::PhysicsClock::new(),
            physics_world: physics::PhysicsWorld::new(),

            last_mode: EditorMode::default(),
            play_mode_hint_started: None,

            show_new_project_dialog: false,
            new_project_catalog_open: false,
            new_project_name: String::new(),
            new_project_name_warning: None,
            new_project_type: crate::project::character_offerings::Setup::Blank,
            background_character_jobs: Vec::new(),
            background_asset_pack_jobs: Vec::new(),
            new_project_packs: app_home::NewProjectPacks::default(),
            asset_manager_packs: app_home::NewProjectPacks::default(),
            asset_manager_packs_open: false,
            show_open_project_dialog: false,
            show_unsaved_scripts_dialog: false,
            home_emblem_texture: None,
            home_dev_notes: home_dev_notes::DeveloperNotesFeed::default(),
            home_member: crate::engine::member_services::MemberClient::default(),
            home_publish_build: None,
            home_management: None,
            home_management_message: None,
            home_recovery: None,
            home_recovery_job: None,

            show_exit_confirmation_dialog: false,
            exit_requested: false,
            window_action: None,

            is_right_mouse_down: false,
            is_middle_mouse_down: false,
            last_cursor_pos: None,
            mouse_pos: (0.0, 0.0),
            mouse: crate::engine::mouse::MouseController::default(),

            rmb_edge_scroll_velocity: glam::Vec2::ZERO,
            last_set_cursor_pos: None,

            is_left_mouse_down: false,
            gameplay_left_mouse_captured: false,
            sculpt_pointer_pending: false,
            drag_start_coord: None,

            keys_down: HashSet::new(),
            editor_shortcut_keys: HashSet::new(),
            jump_requested: false,

            character_system: CharacterSystem::new(),
            attachment_system: AttachmentSystem::default(),
            actor_system: crate::actor::ActorSystem::new(),
            gameplay_camera: GameplayCamera::new(),
            saved_editor_camera: None,
            saved_editor_actors: None,

            script_scene: None,
            script_dynamic_properties: HashMap::new(),
            script_contacted_last_frame: HashSet::new(),
            script_pending_events: Vec::new(),
            script_test_results: BTreeMap::new(),
            script_pending_enable: Vec::new(),
            script_pending_disable: Vec::new(),
            script_pending_spawns: Vec::new(),
            script_pending_destroys: Vec::new(),
            save_state: None,

            authored_disabled_scripts: Vec::new(),

            entity_manager: EntityManager::new(),

            runtime_ui: RuntimeUi::new(),
            controller_object: None,
            camera_object: None,

            orbit_delta: [0.0, 0.0],
            audio_system,
            pending_grab: None,
            pending_actor_grab: None,
            pending_source_grab: None,

            autosave_last: Instant::now(),
            last_autosave_interval: crate::engine::preferences::AutosaveInterval::Off,
            last_autosave_failure: None,
            packaging_open: false,
            game_state: std::sync::Arc::new(std::sync::Mutex::new(
                crate::scripting::api::GameLifecycleState {
                    is_editor: true,
                    ..Default::default()
                },
            )),

            packaging_options: Default::default(),
            packaging_run_after: false,
            packaging_status: "Idle ? ready to package".into(),
            last_built_zip: None,
            publish_description: String::new(),
            publish_rights: false,
            publish_safety: false,
            publish_publication: false,
            packaging_cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            game_build_job: None,
            recovery_last_failure: None,
            recovery_last: Instant::now(),
            recovery_job: None,
        }
    }

    pub fn check_autosave(&mut self) {
        let current_interval = self.editor.theme_settings.autosave;
        if current_interval != self.last_autosave_interval {
            self.last_autosave_interval = current_interval;
            self.autosave_last = Instant::now();
            self.last_autosave_failure = None;
        }
        if self.view != View::Editor
            || self.project_manager.current_project.is_none()
            || self.project_loading()
            || self.project_load_failed
            || self.editor.mode != EditorMode::Editor
        {
            return;
        }
        let Some(seconds) = current_interval.seconds() else {
            return;
        };
        if self.autosave_last.elapsed() < std::time::Duration::from_secs(seconds) {
            return;
        }
        self.autosave_last = Instant::now();
        let scripts_dirty = self
            .editor
            .script_editor
            .open_documents
            .values()
            .any(|document| document.dirty);
        let project_dirty = self.editor.needs_save;
        if !scripts_dirty && !project_dirty {
            self.last_autosave_failure = None;
            return;
        }

        let _diagnostic_autosave =
            crate::engine::development_capture::operation("autosave", "save", false);
        let script_result = if scripts_dirty {
            self.editor.script_editor.save_all_checked_quiet()
        } else {
            Ok(())
        };
        let project_result = if project_dirty {
            self.save_project_checked()
        } else {
            Ok(())
        };
        let failures = [script_result.err(), project_result.err()]
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        if failures.is_empty() {
            self.last_autosave_failure = None;
        } else {
            let error = failures.join("\n");
            if self.last_autosave_failure.as_deref() != Some(error.as_str()) {
                self.editor
                    .script_editor
                    .push_output(format!("Autosave failed: {error}"));
                self.last_autosave_failure = Some(error);
            }
        }
    }

    pub fn update(&mut self, egui_ctx: &egui::Context) {
        self.home_dev_notes.update(egui_ctx, self.view == View::Home);
        self.home_member.poll(egui_ctx);
        self.poll_project_load();
        if self.project_loading() {
            egui_ctx.request_repaint();
            self.last_frame_instant = Instant::now();
            return;
        }
        let now = Instant::now();

        let frame_time = now.duration_since(self.last_frame_instant).as_secs_f32();

        self.last_frame_instant = now;

        if self.view == View::Editor && self.editor.mode == EditorMode::Play {
            let (request, paused, events) = {
                let mut state = self.game_state.lock().unwrap();
                state.advance(frame_time as f64);
                let request = state.requested.take();
                let paused = state.paused;
                let events = std::mem::take(&mut state.pending_events);
                (request, paused, events)
            };
            for event in events {
                self.script_pending_events
                    .push((event.to_string(), Vec::new()));
            }
            if let Some(request) = request {
                self.script_pending_events
                    .push(("game_on_quit".into(), Vec::new()));
                self.update_gameplay(0.0);
                match request {
                    crate::scripting::api::GameLifecycleRequest::Quit => {
                        self.editor.mode = EditorMode::Editor;
                    }
                    crate::scripting::api::GameLifecycleRequest::Restart => {
                        self.restart_play_in_place();
                        self.game_state.lock().unwrap().reset_session();
                    }
                }
            }
            let _ = paused;
        }

        self.poll_live_project_sync();
        self.check_autosave();

        if self.editor.sculpt_stroke_active
            && self.is_left_mouse_down
            && self.sculpt_pointer_pending
        {
            self.update_continuous_sculpt(egui_ctx);
        }

        if self.view == View::Editor {
            if self.editor.mode == EditorMode::Play && self.last_mode == EditorMode::Editor {
                self.game_state.lock().unwrap().reset_session();
                self.play_mode_hint_started = Some(Instant::now());

                let has_dirty_scripts = self
                    .editor
                    .script_editor
                    .open_documents
                    .values()
                    .any(|document| document.dirty);

                if has_dirty_scripts && !self.show_unsaved_scripts_dialog {
                    self.editor.mode = EditorMode::Editor;
                    self.show_unsaved_scripts_dialog = true;
                    self.play_engine_sound(crate::engine::audio::EngineSound::Warning);

                    return;
                }

                self.editor.exit_ui_mode();

                self.saved_editor_camera = Some(self.editor.camera.clone());
                self.saved_editor_actors = Some(self.world.actors.clone());

                let configured_camera = self
                    .project_manager
                    .current_project
                    .as_deref()
                    .and_then(|project| {
                        crate::project::character_asset::load_reference(
                            project,
                            &self.world.selected_character,
                        )
                        .ok()
                    })
                    .and_then(|asset| asset.camera)
                    .unwrap_or_else(|| self.world.selected_camera.clone());
                let camera = self
                    .project_manager
                    .current_project
                    .as_deref()
                    .map(|project| {
                        crate::project::asset_registry::AssetRegistry::native_logic_reference(
                            project,
                            crate::project::asset_registry::AssetType::Camera,
                            &configured_camera,
                        )
                    })
                    .unwrap_or(configured_camera);
                self.gameplay_camera.set_mode_from_name(&camera);

                self.physics_world.register_from_world(&self.world);

                self.world.physics_dirty_cells.clear();

                let spawned_id = self
                    .character_system
                    .spawn_player(&self.world, Some(self.world.selected_character.as_str()));

                if let Err(error) = self.spawn_authored_actors() {
                    self.editor.append_output(&format!(
                        "[{}] [ERROR] Actor initialization error: {}\n",
                        get_timestamp(),
                        error
                    ));

                    self.actor_system.clear();
                    self.character_system.clear();
                    self.entity_manager.clear();
                    self.physics_world.bodies.clear();
                    self.physics_clock.reset();

                    self.editor.mode = EditorMode::Editor;

                    return;
                }

                let player_entity_id = if let Some(character_id) = spawned_id {
                    let entity_id = self.entity_manager.create_entity("Player");

                    self.character_system
                        .associate_entity(entity_id, character_id);

                    self.sync_player_entity_from_character();

                    let initial_position = self
                        .entity_manager
                        .get_position(entity_id)
                        .unwrap_or(glam::Vec3::ZERO);

                    self.actor_system
                        .register_actor(crate::actor::RuntimeActor {
                            entity_id,
                            authored_actor_id: None,
                            package_name: Some(self.world.selected_character.clone()),
                            initial_position,
                        });

                    Some(entity_id.0)
                } else {
                    None
                };

                if let (Some(player), Some(project)) = (
                    player_entity_id,
                    self.project_manager.current_project.as_deref(),
                ) {
                    match crate::project::character_attachment_manifest::load_usable_for_play(
                        project,
                    ) {
                        Ok((manifest, warnings)) => {
                            for warning in warnings {
                                self.editor
                                    .append_output(&format!("[Character Attachments] {warning}\n"));
                            }
                            for template in manifest.attachments.iter().filter(|row| row.enabled) {
                                let cell = self.world.create_manifest_attachment(template);
                                self.attachment_system.attach_spatial(
                                    cell,
                                    crate::engine::entity::EntityId(player),
                                    template.joint_target.clone(),
                                    glam::Vec3::from_array(template.local_origin_offset),
                                );
                            }
                        }
                        Err(error) => self
                            .editor
                            .append_output(&format!("[Character Attachments] {error}\n")),
                    }
                }

                if spawned_id.is_some() {
                    self.mouse
                        .apply_world_defaults(self.world.cursor_visible, self.world.screen_locked);
                } else {
                    self.mouse.set_editor_defaults();
                    self.editor.append_output(&format!(
                        "[{}] [SYSTEM] [Play] No player assigned: simulation-only Play. Add a Spawn Point and choose World > Character to spawn a player.\n",
                        get_timestamp()
                    ));
                    self.editor.script_editor.show_output = true;
                }

                self.start_scripting();

                if let Some(entity_id) = player_entity_id {
                    self.fire_player_spawned_event(entity_id);
                }

                if let Some(project) = self.project_manager.current_project.as_deref() {
                    match self
                        .runtime_scene_analysis
                        .start(project, self.development_build)
                    {
                        Ok(Some(directory)) => {
                            let location = directory.display().to_string();
                            self.editor.append_output(&format!(
                                "[Scene Analysis] Play actor log: {location}\n"
                            ));
                            eprintln!("[Worldkiln Scene Analysis] Play actor log: {location}");
                        }
                        Ok(None) => {}
                        Err(error) => self.editor.append_output(&format!(
                            "[Scene Analysis] Could not create log: {error}\n"
                        )),
                    }
                }
                self.play_engine_sound(crate::engine::audio::EngineSound::PlayStarted);
            }

            self.handle_mode_transition();

            if self.editor.mode == EditorMode::Play {
                let capture = self.development_capture.borrow().state()
                    == crate::engine::development_test_run::CaptureState::Running;
                if capture {
                    crate::engine::development_capture::begin(
                        self.development_capture.borrow().config().detailed,
                    );
                }
                let delta = if self.game_state.lock().unwrap().paused {
                    0.0
                } else {
                    frame_time
                };
                self.update_gameplay(delta);

                if capture {
                    let (samples, dropped) = crate::engine::development_capture::finish();
                    self.development_capture
                        .borrow_mut()
                        .collect_spans(samples, dropped);
                }
            }
            self.world.update_particles(frame_time);

            if egui_ctx.wants_keyboard_input() {
                return;
            }

            let mut move_vec = glam::Vec3::ZERO;
            let mut terrain_hover_camera_changed = false;

            if self.is_right_mouse_down && self.editor_camera_controls_active() {
                if self
                    .editor
                    .theme_settings
                    .bind_camera_forward
                    .is_down(&self.keys_down)
                {
                    move_vec.z += 1.0;
                }

                if self
                    .editor
                    .theme_settings
                    .bind_camera_backward
                    .is_down(&self.keys_down)
                {
                    move_vec.z -= 1.0;
                }

                if self
                    .editor
                    .theme_settings
                    .bind_camera_left
                    .is_down(&self.keys_down)
                {
                    move_vec.x -= 1.0;
                }

                if self
                    .editor
                    .theme_settings
                    .bind_camera_right
                    .is_down(&self.keys_down)
                {
                    move_vec.x += 1.0;
                }

                if self
                    .editor
                    .theme_settings
                    .bind_camera_up
                    .is_down(&self.keys_down)
                {
                    move_vec.y += 1.0;
                }

                if self
                    .editor
                    .theme_settings
                    .bind_camera_down
                    .is_down(&self.keys_down)
                {
                    move_vec.y -= 1.0;
                }
            }

            if move_vec != glam::Vec3::ZERO {
                terrain_hover_camera_changed = true;
                let speed = self.editor.camera.distance * self.editor.camera_speed() * frame_time;

                self.editor
                    .camera
                    .translate_free(move_vec.normalize() * speed);

                let target = self.editor.camera.target;

                self.editor.navigation_window.x_buf = target.x.floor().to_string();
                self.editor.navigation_window.y_buf = target.y.floor().to_string();
                self.editor.navigation_window.z_buf = target.z.floor().to_string();
            }

            if self.is_right_mouse_down
                && self.editor_camera_controls_active()
                && self.rmb_edge_scroll_velocity != glam::Vec2::ZERO
            {
                self.editor.camera.look(
                    self.rmb_edge_scroll_velocity.x * frame_time,
                    self.rmb_edge_scroll_velocity.y * frame_time,
                );
                terrain_hover_camera_changed = true;
            }

            if terrain_hover_camera_changed
                && (self.editor.sculpt_tool != crate::world::terrain::SculptTool::None
                    || self.editor.actor_brush_enabled)
            {
                let pixels_per_point = egui_ctx.pixels_per_point();
                let logical = egui::pos2(
                    self.mouse_pos.0 as f32 / pixels_per_point,
                    self.mouse_pos.1 as f32 / pixels_per_point,
                );
                if self.editor.viewport_rect.contains(logical) && !egui_ctx.wants_pointer_input() {
                    self.update_hover();
                }
            }

            if self.editor.needs_clear_world {
                self.push_full_undo_snapshot();

                self.renderer.clear_chunk_cache();

                self.world = World::new();
                self.editor.clear_clipboard();
                self.editor.needs_clear_world = false;
                self.editor.needs_save = true;
            }

            if self.editor.needs_exit {
                self.exit_to_home();
                self.editor.needs_exit = false;
            }
        }
    }

    fn restart_play_in_place(&mut self) {
        self.runtime_scene_analysis.stop();
        self.stop_scripting();
        self.physics_clock.reset();
        self.audio_system.stop_all();
        self.character_system.clear();
        self.actor_system.clear();
        self.physics_world.bodies.clear();
        self.world.clear_runtime_state();
        self.renderer.reset_runtime_post_process();
        self.editor
            .post_process_runtime_preview
            .borrow_mut()
            .reset();
        self.editor
            .post_process_runtime_commands
            .borrow_mut()
            .clear();
        if let Some(authored) = self.saved_editor_actors.as_ref() {
            self.world.actors = authored.clone();
        }
        self.editor
            .append_output("[Game restart] Reloading runtime without modifying saved data.\n");
        let configured_camera = self
            .project_manager
            .current_project
            .as_deref()
            .and_then(|project| {
                crate::project::character_asset::load_reference(
                    project,
                    &self.world.selected_character,
                )
                .ok()
            })
            .and_then(|asset| asset.camera)
            .unwrap_or_else(|| self.world.selected_camera.clone());
        let camera = self
            .project_manager
            .current_project
            .as_deref()
            .map(|project| {
                crate::project::asset_registry::AssetRegistry::native_logic_reference(
                    project,
                    crate::project::asset_registry::AssetType::Camera,
                    &configured_camera,
                )
            })
            .unwrap_or(configured_camera);
        self.gameplay_camera.set_mode_from_name(&camera);

        self.physics_world.register_from_world(&self.world);

        self.world.physics_dirty_cells.clear();

        let spawned_id = self
            .character_system
            .spawn_player(&self.world, Some(self.world.selected_character.as_str()));

        if let Err(error) = self.spawn_authored_actors() {
            self.editor.append_output(&format!(
                "[{}] [ERROR] Actor initialization error: {}\n",
                get_timestamp(),
                error
            ));

            self.actor_system.clear();
            self.character_system.clear();
            self.entity_manager.clear();
            self.physics_world.bodies.clear();
            self.physics_clock.reset();

            return;
        }

        let player_entity_id = if let Some(character_id) = spawned_id {
            let entity_id = self.entity_manager.create_entity("Player");

            self.character_system
                .associate_entity(entity_id, character_id);

            self.sync_player_entity_from_character();

            let initial_position = self
                .entity_manager
                .get_position(entity_id)
                .unwrap_or(glam::Vec3::ZERO);

            self.actor_system
                .register_actor(crate::actor::RuntimeActor {
                    entity_id,
                    authored_actor_id: None,
                    package_name: Some(self.world.selected_character.clone()),
                    initial_position,
                });

            Some(entity_id.0)
        } else {
            None
        };

        if spawned_id.is_some() {
            self.mouse
                .apply_world_defaults(self.world.cursor_visible, self.world.screen_locked);
        } else {
            self.mouse.set_editor_defaults();
            self.editor.append_output(&format!(
                        "[{}] [SYSTEM] [Play] No player assigned: simulation-only Play. Add a Spawn Point and choose World > Character to spawn a player.\n",
                        get_timestamp()
                    ));
            self.editor.script_editor.show_output = true;
        }

        self.start_scripting();

        if let Some(entity_id) = player_entity_id {
            self.fire_player_spawned_event(entity_id);
        }
        if let Some(project) = self.project_manager.current_project.as_deref() {
            if let Err(error) = self
                .runtime_scene_analysis
                .start(project, self.development_build)
            {
                self.editor
                    .append_output(&format!("[Scene Analysis] restart log: {error}\n"));
            }
        }
    }

    pub(crate) fn handle_mode_transition(&mut self) {
        if self.last_mode != self.editor.mode {
            self.editor
                .post_process_runtime_preview
                .borrow_mut()
                .reset();
            self.editor
                .post_process_runtime_commands
                .borrow_mut()
                .clear();
        }
        if self.editor.mode == EditorMode::Editor && self.last_mode == EditorMode::Play {
            self.runtime_scene_analysis.stop();
            self.play_mode_hint_started = None;
            self.stop_scripting();

            if let Some(saved) = self.saved_editor_camera.take() {
                self.editor.camera = saved;
            }

            self.physics_clock.reset();

            self.audio_system.stop_all();

            self.character_system.clear();

            self.actor_system.clear();

            self.physics_world.bodies.clear();

            self.mouse.set_editor_defaults();

            self.world.clear_runtime_state();
            self.renderer.reset_runtime_post_process();
            if let Some(authored) = self.saved_editor_actors.take() {
                self.world.actors = authored;
            }

            self.play_engine_sound(crate::engine::audio::EngineSound::PlayStopped);
        }

        self.last_mode = self.editor.mode;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::physics::{PhysicsBody, PhysicsBodyId};

    #[test]
    fn development_settings_page_draws_in_home_and_editor() {
        let mut app = App::new_for_tests(1024.0, 768.0);
        app.editor
            .open_engine_settings(crate::editor::editor::SettingsPage::Development);
        let ctx = egui::Context::default();
        for view in [View::Home, View::Editor] {
            app.view = view;
            let _ = ctx.run(egui::RawInput::default(), |ctx| {
                app.update_ui(ctx);
            });
            assert_eq!(
                app.editor.settings_page,
                crate::editor::editor::SettingsPage::Development
            );
        }
    }

    #[test]
    fn development_ui_spans_continue_while_display_is_paused() {
        let mut app = App::new_for_tests(1024.0, 768.0);
        app.view = View::Editor;
        let ctx = egui::Context::default();
        let render = |app: &mut App| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1024.0, 768.0),
                    )),
                    ..Default::default()
                },
                |ctx| app.update_ui(ctx),
            );
        };
        render(&mut app);
        assert!(app.development_capture.borrow().events().is_empty());
        assert!(app.start_test_run(false));
        render(&mut app);
        let count = {
            let recorder = app.development_capture.borrow();
            assert!(
                recorder
                    .events()
                    .iter()
                    .any(|e| e.category == "ui" && e.operation == "menu_bar")
            );
            assert!(
                recorder
                    .events()
                    .iter()
                    .any(|e| e.category == "ui" && e.operation == "workspace_tabs")
            );
            recorder.events().len()
        };
        app.pause_test_run();
        render(&mut app);
        assert!(app.development_capture.borrow().events().len() > count);
        assert!(app.development_capture.borrow().display_paused());
    }

    #[test]
    fn development_test_run_app_lifecycle_and_report() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_dev001_app_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let mut app = App::new_for_tests(800.0, 600.0);
        app.project_manager.current_project = Some(root.clone());
        assert!(app.last_test_run_log_directory().is_none());
        assert!(app.start_test_run(false));
        app.development_capture.borrow_mut().record(
            "ui",
            "test_event",
            std::time::Duration::from_millis(8),
        );
        app.pause_test_run();
        assert!(app.start_test_run(false));
        app.stop_test_run();
        assert!(!app.start_test_run(false));
        let task = app.save_test_run_async().unwrap();
        let first = app.finish_test_run_save(task).unwrap();
        assert!(first.join("events.csv").is_file());
        assert!(first.join("summary.json").is_file());
        assert_eq!(app.last_test_run_log_directory(), Some(first.clone()));
        assert!(app.start_test_run(false));
        app.project_manager.current_project = Some(root.join("different_project"));
        app.stop_test_run();
        let second = app.save_test_run().unwrap();
        assert!(second.starts_with(root.join(".worldkiln")));
        assert_ne!(first, second);
        assert!(first.join("summary.json").exists());
        let mut reopened = App::new_for_tests(640.0, 480.0);
        reopened.project_manager.current_project = Some(root.clone());
        assert_eq!(
            reopened.last_test_run_log_directory(),
            Some(crate::engine::development_test_run::TestRunRecorder::log_directory(&root))
        );
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn bad_scripts_are_visible_with_normalized_source_context_and_survive_stop() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_wo04_script_output_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join("scripts")).unwrap();
        let path = root.join("scripts/bad.aeo");
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.project_manager.current_project = Some(root.clone());
        let ctx = egui::Context::default();
        for (source, expected) in [
            ("fn {", "scripts/bad.aeo:1:4 | <parse>"),
            (
                "print(missing_wo04_value)",
                "scripts/bad.aeo:1:7 | top-level",
            ),
        ] {
            std::fs::write(&path, source).unwrap();
            app.editor.clear_output();
            app.editor.script_editor.show_output = false;
            app.editor.mode = EditorMode::Play;
            app.update(&ctx);
            assert!(app.editor.script_editor.show_output);
            assert!(app.editor.terminal_output.contains("[ERROR] [Script]"));
            assert!(
                app.editor.terminal_output.contains(expected),
                "{}",
                app.editor.terminal_output
            );
            app.update(&ctx);
            if source.starts_with("print") {
                assert_eq!(
                    app.editor
                        .terminal_output
                        .matches("unknown variable 'missing_wo04_value'")
                        .count(),
                    1
                );
            }
            let diagnostics = app.editor.terminal_output.clone();
            app.editor.mode = EditorMode::Editor;
            app.update(&ctx);
            assert!(app.editor.terminal_output.starts_with(&diagnostics));
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn test_editor_idle_and_transition_cleanup() {
        let mut app = App::new_for_tests(800.0, 600.0);

        app.view = View::Editor;

        app.last_mode = EditorMode::Play;
        app.editor.mode = EditorMode::Editor;

        app.physics_world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(1),
            1,
            glam::Vec3::ZERO,
            glam::Vec3::ONE,
        ));

        app.physics_clock.update(0.1, |_| {});

        app.actor_system.register_actor(crate::actor::RuntimeActor {
            entity_id: crate::engine::entity::EntityId(1),
            authored_actor_id: Some(10),
            package_name: Some("Rogue".to_string()),
            initial_position: glam::Vec3::ZERO,
        });

        app.saved_editor_camera = Some(app.editor.camera.clone());
        let coin_a = app.world.add_actor("BombPickupA", glam::Vec3::ZERO);
        let coin_b = app.world.add_actor("BombPickupB", glam::Vec3::X);
        app.saved_editor_actors = Some(app.world.actors.clone());
        for id in [coin_a, coin_b] {
            let actor = app.world.actors.get_mut(&id).unwrap();
            actor.visible = false;
            actor.enabled = false;
        }

        app.handle_mode_transition();
        for id in [coin_a, coin_b] {
            let actor = app.world.actors.get(&id).unwrap();
            assert!(
                actor.visible && actor.enabled,
                "Stop must restore authored pickup flags"
            );
        }
        assert!(app.saved_editor_actors.is_none());

        assert!(
            app.actor_system.actors().is_empty(),
            "Play -> Editor must clear actor_system"
        );

        assert!(
            app.physics_world.bodies.is_empty(),
            "Play -> Editor must clear physics bodies"
        );

        assert_eq!(
            app.physics_clock.accumulator, 0.0,
            "Play -> Editor must reset the physics clock"
        );

        assert!(
            app.saved_editor_camera.is_none(),
            "Play -> Editor must consume the saved gameplay camera"
        );

        assert_eq!(
            app.last_mode,
            EditorMode::Editor,
            "Transition handler must update last_mode"
        );
    }
}
