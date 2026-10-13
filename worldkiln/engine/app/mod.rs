//! Application state and frame/update orchestration.
//!
//! Owns the main application state shared by the editor, renderer, world,
//! gameplay systems, scripting runtime, input handling, and mode transitions.
pub mod app_home;
pub mod app_input;
pub mod app_project;
pub mod app_render;
pub mod app_runtime;
pub mod app_ui;

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

/// Application state shared by the window, editor, renderer, world,
/// scripting runtime, input system, and gameplay systems.
pub struct App {
    pub splash_started: Instant,
    pub development_build: bool,

    pub view: View,
    pub renderer: Renderer,
    pub editor: Editor,
    pub world: World,
    pub project_manager: ProjectManager,
    // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
    // A failed load must never serialize the partial World over authored data.
    pub(crate) project_load_failed: bool,
    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
    // File fingerprints are scoped to the currently loaded project session.
    pub(crate) project_disk_snapshot: Option<BTreeMap<std::path::PathBuf, u64>>,

    pub last_frame_instant: Instant,
    pub physics_clock: physics::PhysicsClock,
    pub physics_world: physics::PhysicsWorld,
    pub last_mode: EditorMode,

    /// Starts when Play mode becomes active. Used only for the short-lived
    /// "Press Escape to quit Play" affordance over the full-screen renderer.
    pub play_mode_hint_started: Option<Instant>,

    pub show_new_project_dialog: bool,
    pub new_project_name: String,
    pub show_open_project_dialog: bool,
    pub show_unsaved_scripts_dialog: bool,
    pub home_emblem_texture: Option<egui::TextureHandle>,
    pub(crate) home_management: Option<app_home::HomeManagement>,
    pub(crate) home_management_message: Option<String>,

    pub is_right_mouse_down: bool,
    pub is_middle_mouse_down: bool,
    pub last_cursor_pos: Option<(f64, f64)>,
    pub mouse_pos: (f64, f64),
    pub mouse: crate::engine::mouse::MouseController,

    pub rmb_edge_scroll_velocity: glam::Vec2,
    pub last_set_cursor_pos: Option<(f64, f64)>,

    pub is_left_mouse_down: bool,
    pub drag_start_coord: Option<crate::world::WorldCoord>,

    pub keys_down: HashSet<KeyCode>,
    pub jump_requested: bool,

    pub character_system: CharacterSystem,
    pub attachment_system: AttachmentSystem,
    pub actor_system: crate::actor::ActorSystem,
    pub gameplay_camera: GameplayCamera,
    pub saved_editor_camera: Option<CameraController>,

    pub script_scene: Option<ScriptScene>,

    pub script_dynamic_properties: HashMap<(HandleKind, u64), BTreeMap<String, DynamicProperty>>,

    pub script_contacted_last_frame: HashSet<u64>,

    pub script_pending_events: Vec<(String, Vec<Value>)>,

    pub script_test_results: BTreeMap<String, bool>,

    pub script_pending_enable: Vec<String>,
    pub script_pending_disable: Vec<String>,

    pub script_pending_spawns: Vec<(String, u64)>,
    pub script_pending_destroys: Vec<u64>,

    /// Runtime save data is intentionally not part of `World` or project
    /// authoring state. It is loaded per project when Play mode starts.
    pub save_state: Option<crate::engine::save_state::SaveState>,

    pub show_exit_confirmation_dialog: bool,
    pub exit_requested: bool,

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
}

impl App {
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

    // Worldkiln 0.8.7 | WO 0.8.7-23 | 2026-10-06 16:22 -05:00
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
            // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
            project_load_failed: false,
            // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
            project_disk_snapshot: None,

            splash_started: Instant::now(),
            development_build: cfg!(debug_assertions),
            last_frame_instant: Instant::now(),

            physics_clock: physics::PhysicsClock::new(),
            physics_world: physics::PhysicsWorld::new(),

            last_mode: EditorMode::default(),
            play_mode_hint_started: None,

            show_new_project_dialog: false,
            new_project_name: String::new(),
            show_open_project_dialog: false,
            show_unsaved_scripts_dialog: false,
            home_emblem_texture: None,
            home_management: None,
            home_management_message: None,

            show_exit_confirmation_dialog: false,
            exit_requested: false,

            is_right_mouse_down: false,
            is_middle_mouse_down: false,
            last_cursor_pos: None,
            mouse_pos: (0.0, 0.0),
            mouse: crate::engine::mouse::MouseController::default(),

            rmb_edge_scroll_velocity: glam::Vec2::ZERO,
            last_set_cursor_pos: None,

            is_left_mouse_down: false,
            drag_start_coord: None,

            keys_down: HashSet::new(),
            jump_requested: false,

            character_system: CharacterSystem::new(),
            attachment_system: AttachmentSystem::default(),
            actor_system: crate::actor::ActorSystem::new(),
            gameplay_camera: GameplayCamera::new(),
            saved_editor_camera: None,

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
        }
    }

    pub fn check_autosave(&mut self) {
        let current_interval = self.editor.theme_settings.autosave;
        if current_interval != self.last_autosave_interval {
            self.last_autosave_interval = current_interval;
            self.autosave_last = Instant::now();
        }

        if self.view == View::Editor
            && self.project_manager.current_project.is_some()
            // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
            && !self.project_load_failed
            && self.editor.mode == EditorMode::Editor
        {
            if let Some(seconds) = current_interval.seconds() {
                if self.autosave_last.elapsed() >= std::time::Duration::from_secs(seconds) {
                    let dirty_script_count = self
                        .editor
                        .script_editor
                        .open_documents
                        .values()
                        .filter(|document| document.dirty)
                        .count();

                    self.editor.script_editor.save_all();
                    self.save_project();

                    self.editor.append_output(&format!(
                        "[{}] [SYSTEM] [Autosave] Saved project and {} modified script{}.\n",
                        get_timestamp(),
                        dirty_script_count,
                        if dirty_script_count == 1 { "" } else { "s" },
                    ));
                    self.editor.terminal_auto_scroll = true;

                    self.autosave_last = Instant::now();
                }
            }
        }
    }

    pub fn update(&mut self, egui_ctx: &egui::Context) {
        let now = Instant::now();

        let frame_time = now.duration_since(self.last_frame_instant).as_secs_f32();

        self.last_frame_instant = now;

        self.check_autosave();

        if self.view == View::Editor {
            if self.editor.mode == EditorMode::Play && self.last_mode == EditorMode::Editor {
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

                self.gameplay_camera
                    .set_mode_from_name(&self.world.selected_camera);

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

                if spawned_id.is_some() {
                    self.mouse
                        .apply_world_defaults(self.world.cursor_visible, self.world.screen_locked);
                } else {
                    // No Spawn Point means simulation-only Play; never possess an Actor.
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

                self.play_engine_sound(crate::engine::audio::EngineSound::PlayStarted);
            }

            self.handle_mode_transition();

            if self.editor.mode == EditorMode::Play {
                self.update_gameplay(frame_time);
            }
            self.world.update_particles(frame_time);

            if egui_ctx.wants_keyboard_input() {
                return;
            }

            let mut move_vec = glam::Vec3::ZERO;

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
            }

            if self.editor.needs_clear_world {
                // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
                self.push_full_undo_snapshot();

                self.renderer.clear_chunk_cache();

                self.world = World::new();
                self.editor.clear_clipboard();
                self.editor.needs_clear_world = false;
            }

            if self.editor.needs_save {
                self.save_project();
                self.editor.needs_save = false;
            }

            if self.editor.needs_exit {
                self.exit_to_home();
                self.editor.needs_exit = false;
            }
        }
    }

    pub(crate) fn handle_mode_transition(&mut self) {
        if self.editor.mode == EditorMode::Editor && self.last_mode == EditorMode::Play {
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
    fn play_ownership_requires_spawn_point_and_cleans_up_between_sessions() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        let mut actor = crate::world::actor::Actor::new(42, "Bystander", glam::Vec3::ZERO);
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
        actor.package = Some("Rogue".to_string());
        app.world.actors.insert(actor.id, actor);
        let ctx = egui::Context::default();
        for assigned in [false, true, false] {
            let coord = crate::world::WorldCoord::new(10, 0, 10);
            if assigned {
                app.world
                    .set_cell(coord, crate::world::CellType::SpawnPoint);
            } else {
                app.world.set_cell(coord, crate::world::CellType::Empty);
            }
            app.editor.mode = EditorMode::Play;
            app.update(&ctx);
            assert_eq!(app.character_system.get_active_player().is_some(), assigned);
            assert_eq!(
                app.entity_manager.lookup_entity("Player").is_some(),
                assigned
            );
            assert_eq!(
                app.actor_system.actors().len(),
                if assigned { 2 } else { 1 }
            );
            if !assigned {
                assert!(app.mouse.cursor_visible);
                assert!(!app.mouse.screen_locked);
                assert!(app.controller_object.is_none());
                assert!(app.camera_object.is_none());
            }
            app.editor.mode = EditorMode::Editor;
            app.update(&ctx);
            assert!(app.character_system.get_active_player().is_none());
            assert!(app.actor_system.actors().is_empty());
            assert!(app.world.actors.contains_key(&42));
        }
        assert_eq!(
            app.editor
                .terminal_output
                .matches("No player assigned:")
                .count(),
            2
        );
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

        app.handle_mode_transition();

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
