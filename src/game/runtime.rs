// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::PathBuf;

use winit::keyboard::KeyCode;

use crate::character::CharacterSystem;
use crate::engine::attachment::AttachmentSystem;
use crate::engine::audio::AudioSystem;
use crate::engine::entity::EntityManager;
use crate::engine::input::InputActions;
use crate::engine::mouse::MouseController;
use crate::engine::physics::{PhysicsClock, PhysicsWorld};
use crate::engine::ui::RuntimeUi;
use crate::renderer::Renderer;
use crate::renderer::camera::GameplayCamera;
use crate::scripting::api::HostContext;
use crate::scripting::host::ScriptHostBridge;
use crate::scripting::scene::ScriptScene;
use crate::scripting::value::{HandleKind, Value};
use crate::world::World;

use super::platform::{DesktopRuntimeServices, GameLifecycleEvent, GameRuntimeServices};

fn report_standalone_error(message: &str) {
    eprintln!("{message}");
    let root = std::env::var_os("WORLDKILN_RUNTIME_SAVE_ROOT")
        .or_else(|| std::env::var_os("LOCALAPPDATA"))
        .or_else(|| std::env::var_os("XDG_DATA_HOME"))
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir);
    let directory = root.join("Aeowun").join("Games").join("Logs");
    if std::fs::create_dir_all(&directory).is_ok() {
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(directory.join("runtime-errors.log"))
        {
            let timestamp = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|time| time.as_secs())
                .unwrap_or(0);
            let _ = std::io::Write::write_all(&mut file, format!("[{timestamp}] {message}\n").as_bytes());
        }
    }
}

pub struct GameRuntime {
    pub game_state: std::sync::Arc<std::sync::Mutex<crate::scripting::api::GameLifecycleState>>,
    platform_services: Box<dyn GameRuntimeServices>,
    suspended: bool,
    pub renderer: Renderer,
    pub world: World,
    pub project_hierarchy: crate::project::ProjectHierarchy,

    pub physics_clock: PhysicsClock,
    pub physics_world: PhysicsWorld,

    pub character_system: CharacterSystem,
    pub attachment_system: AttachmentSystem,
    pub actor_system: crate::actor::ActorSystem,
    pub gameplay_camera: GameplayCamera,

    pub script_scene: Option<ScriptScene>,
    pub entity_manager: EntityManager,
    pub runtime_ui: RuntimeUi,

    pub controller_object: Option<Value>,
    pub camera_object: Option<Value>,

    pub script_dynamic_properties:
        HashMap<(HandleKind, u64), BTreeMap<String, crate::scripting::value::DynamicProperty>>,

    pub script_contacted_last_frame: HashSet<u64>,
    pub script_pending_events: Vec<(String, Vec<Value>)>,
    pub script_test_results: BTreeMap<String, bool>,
    pub script_pending_enable: Vec<String>,
    pub script_pending_disable: Vec<String>,
    pub script_pending_spawns: Vec<(String, u64)>,
    pub script_pending_destroys: Vec<u64>,
    pub save_state: Option<crate::engine::save_state::SaveState>,

    pub authored_disabled_scripts: Vec<String>,

    pub mouse: MouseController,
    pub input_actions: InputActions,
    pub keys_down: HashSet<KeyCode>,
    pub gameplay_mouse_buttons: u8,
    pub jump_requested: bool,
    pub orbit_delta: [f32; 2],

    pub audio_system: AudioSystem,
    pub project_path: Option<PathBuf>,
}

impl GameRuntime {
    pub fn new(width: f32, height: f32) -> Self {
        Self::with_services(width, height, Box::new(DesktopRuntimeServices))
    }

    pub fn with_services(width: f32, height: f32, services: Box<dyn GameRuntimeServices>) -> Self {
        let renderer = services.create_renderer(width, height);
        let audio_system = services.create_audio();
        Self {
            platform_services: services,
            game_state: Default::default(),
            suspended: false,
            renderer,
            world: World::new(),
            project_hierarchy: crate::project::ProjectHierarchy::new(),

            physics_clock: PhysicsClock::new(),
            physics_world: PhysicsWorld::new(),

            character_system: CharacterSystem::new(),
            attachment_system: AttachmentSystem::default(),
            actor_system: crate::actor::ActorSystem::new(),
            gameplay_camera: GameplayCamera::new(),

            script_scene: None,
            entity_manager: EntityManager::new(),
            runtime_ui: RuntimeUi::new(),

            controller_object: None,
            camera_object: None,

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

            mouse: MouseController::default(),
            input_actions: InputActions::default(),
            keys_down: HashSet::new(),
            gameplay_mouse_buttons: 0,
            jump_requested: false,
            orbit_delta: [0.0, 0.0],

            audio_system,
            project_path: None,
        }
    }

    pub fn load_project(&mut self, project_path: PathBuf) {
        self.renderer.clear_chunk_cache();
        self.renderer.set_project_asset_root(Some(&project_path));

        self.project_path = Some(project_path.clone());
        self.project_hierarchy = match crate::project::metadata::load_project_metadata(&project_path) {
            Ok(hierarchy) => hierarchy,
            Err(error) => {
                eprintln!("[ERROR] Failed to load authored project hierarchy: {error}");
                crate::project::ProjectHierarchy::new()
            }
        };

        self.save_state = match self.platform_services.load_save_state(&project_path) {
            Ok(state) => Some(state),
            Err(error) => {
                eprintln!("[ERROR] Failed to load runtime save state: {}", error);
                None
            }
        };

        crate::character::package::reload_packages(Some(&project_path));
        crate::engine::entity_package::reload_packages(Some(&project_path));

        let world_path = self.platform_services.authored_world_path(&project_path);
        if !world_path.is_file() {
            eprintln!(
                "[ERROR] Cannot start game: authored world.dat is missing: {}",
                world_path.display()
            );
            std::process::exit(1);
        }
        if let Err(error) = crate::world::persistence::load_world(&mut self.world, &world_path) {
            eprintln!("[ERROR] Cannot start game: authored world failed to load: {error}");
            std::process::exit(1);
        }

        let (_, camera) = crate::project::character_asset::resolve_player_logic(
            &project_path,
            &self.world.selected_character,
            &self.world.selected_controller,
            &self.world.selected_camera,
        );
        if !camera.is_empty() {
            self.gameplay_camera.set_mode_from_name(&camera);
            let _ = self.world.set_runtime_camera_script(&camera);
        }

        self.physics_world.register_from_world(&self.world);

        self.world.physics_dirty_cells.clear();

        let spawned_character_id = self
            .character_system
            .spawn_player(&self.world, Some(self.world.selected_character.as_str()));

        if let Err(error) = self.spawn_authored_actors() {
            eprintln!("[ERROR] Actor initialization error: {}", error);

            self.actor_system.clear();
            self.character_system.clear();
            self.entity_manager.clear();
            self.physics_world.bodies.clear();
            self.physics_clock.reset();

            return;
        }

        let player_entity_id = if let Some(character_id) = spawned_character_id {
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

        if spawned_character_id.is_some() {
            self.mouse
                .apply_world_defaults(self.world.cursor_visible, self.world.screen_locked);
        } else {
            self.mouse.set_editor_defaults();
        }

        self.start_scripting();

        if let Some(entity_id) = player_entity_id {
            self.fire_player_spawned_event(entity_id);
        }
    }

    fn spawn_authored_actors(&mut self) -> Result<(), String> {
        self.actor_system.clear();

        let actors_list: Vec<_> = self.world.actors.values().cloned().collect();

        for actor in actors_list {
            if !actor.enabled {
                continue;
            }

            let entity_name = {
                let trimmed = actor.identity.trim();

                if !trimmed.is_empty() {
                    trimmed.to_string()
                } else {
                    format!("Actor_{}", actor.id)
                }
            };

            let entity_id = self.entity_manager.create_entity(&entity_name);

            if let Some(package_name) = actor.package.as_deref() {
                self.entity_manager.apply_package(entity_id, package_name);
            }

            self.entity_manager.set_position(entity_id, actor.position);
            self.entity_manager.set_rotation(entity_id, actor.rotation);
            self.entity_manager.set_scale(entity_id, actor.scale);
            self.entity_manager.set_visible(entity_id, actor.visible);
            self.entity_manager.set_enabled(entity_id, actor.enabled);

            if let Some(entity) = self.entity_manager.get_entity_mut(entity_id) {
                entity.attributes = actor.attributes.clone();
                entity.interaction =
                    crate::engine::entity::RuntimeInteraction::from_attributes(&entity.attributes);
                entity.physics.enabled = actor.physics.enabled;
                entity.physics.solid = actor.physics.solid;
                entity.physics.anchored = actor.physics.anchored;
                entity.physics.collision_enabled = actor.physics.collision_enabled;
                entity.physics.mass = actor.physics.mass;
                entity.physics.size = actor.physics.collision_size * actor.scale.abs();
                entity.physics.offset = actor.physics.collision_offset * actor.scale;
                crate::engine::collision_geometry::configure_runtime_actor(
                    &actor,
                    &mut entity.physics,
                );
            }

            let runtime_actor = crate::actor::RuntimeActor {
                entity_id,
                authored_actor_id: Some(actor.id),
                package_name: actor.package.clone(),
                initial_position: actor.position,
            };

            self.actor_system.register_actor(runtime_actor);

            if let Some(package_name) = actor.package.as_deref() {
                if crate::project::is_character_package(self.project_path.as_deref(), package_name)
                {
                    let character_id = self
                        .character_system
                        .spawn_character(actor.position, Some(package_name));

                    self.character_system
                        .associate_entity(entity_id, character_id);
                    self.character_system.set_entity_kinematic(
                        entity_id,
                        actor.physics.anchored || !actor.physics.enabled,
                    );
                    self.character_system
                        .configure_authored_collision(entity_id, &actor);
                    if let Some(character) = self
                        .character_system
                        .get_character_for_entity_mut(entity_id)
                    {
                        crate::character::character::set_model_material(
                            character,
                            &actor.attributes,
                        );
                        character.transform.rotation = glam::Quat::from_euler(
                            glam::EulerRot::XYZ,
                            actor.rotation.x,
                            actor.rotation.y,
                            actor.rotation.z,
                        );
                        character.transform.scale = actor.scale;
                    }
                }
            }
        }

        Ok(())
    }

    pub fn fire_player_spawned_event(&mut self, player_entity_id: u64) {
        if let Some(scene) = &mut self.script_scene {
            let mut bridge = ScriptHostBridge {
                game_state: Some(self.game_state.clone()),
                entity_manager: &mut self.entity_manager,
                attachment_system: Some(&mut self.attachment_system),
                world: &mut self.world,
                mouse: &mut self.mouse,
                viewport_size: [self.renderer.width(), self.renderer.height()],
                keys_down: Some(&self.keys_down),
                dynamic_properties: &mut self.script_dynamic_properties,
                pending_events: &mut self.script_pending_events,
                test_results: &mut self.script_test_results,
                pending_enable_scripts: &mut self.script_pending_enable,
                pending_disable_scripts: &mut self.script_pending_disable,
                runtime_ui: &mut self.runtime_ui,
                move_input: glam::Vec2::ZERO,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: Some(&mut self.character_system),
                actor_system: Some(&mut self.actor_system),
                gameplay_camera: Some(&mut self.gameplay_camera),
                valid_entity_declarations: Some(scene.valid_entity_declarations.clone()),
                pending_spawns: &mut self.script_pending_spawns,
                pending_destroys: &mut self.script_pending_destroys,
                project_path: self.project_path.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: 0.0,
                engine: &mut bridge,
            };

            let arguments = vec![Value::Handle {
                kind: HandleKind::Entity,
                id: player_entity_id,
            }];

            if let Err(error) = scene.dispatch_event("PlayerSpawned", arguments, &mut context) {
                eprintln!("[ERROR] Event PlayerSpawned error: {}", error);
            }
        }
    }

    pub fn sync_player_entity_from_character(&mut self) {
        self.character_system
            .sync_entity_positions(&mut self.entity_manager);
    }

    pub fn start_scripting(&mut self) {
        self.script_contacted_last_frame.clear();

        self.runtime_ui.clear();
        self.world.particles.clear();

        self.controller_object = None;
        self.camera_object = None;
        self.world.runtime_camera_script = None;
        self.world.runtime_camera_applied = None;

        self.authored_disabled_scripts = self.world.disabled_scripts.clone();

        let Some(project_path) = &self.project_path else {
            return;
        };
        match crate::engine::ui_authoring::AuthoredUiWorkspace::load_from_project(project_path) {
            Ok(workspace) => {
                let document = workspace.active_document().cloned().unwrap_or_default();
                if let Err(error) = crate::engine::ui_authoring::populate_runtime_ui_named(
                    &mut self.runtime_ui,
                    &document,
                    workspace
                        .document(workspace.active_document_id)
                        .map(|d| d.key.as_str())
                        .unwrap_or("main_ui"),
                ) {
                    report_standalone_error(&format!("[ERROR] Failed to load authored UI: {error}"));
                }
            }

            Err(error) => {
                report_standalone_error(&format!("[ERROR] Failed to load project UI: {error}"));
            }
        }

        let (controller_name, camera_name) = crate::project::character_asset::resolve_player_logic(
            project_path,
            &self.world.selected_character,
            &self.world.selected_controller,
            &self.world.selected_camera,
        );
        if !camera_name.is_empty() {
            self.gameplay_camera.set_mode_from_name(&camera_name);
            let _ = self.world.set_runtime_camera_script(&camera_name);
        }

        match ScriptScene::load_from_bindings(
            project_path,
            &self.world,
            &self.world.script_bindings,
            &mut self.entity_manager,
            self.actor_system.actor_to_entity_map(),
            0.0,
        ) {
            Ok(mut scene) => {
                let has_player = self.character_system.get_active_player().is_some();
                let mut bridge = ScriptHostBridge {
                    game_state: Some(self.game_state.clone()),
                    entity_manager: &mut self.entity_manager,
                    attachment_system: Some(&mut self.attachment_system),
                    world: &mut self.world,
                    mouse: &mut self.mouse,
                    dynamic_properties: &mut self.script_dynamic_properties,
                    pending_events: &mut self.script_pending_events,
                    test_results: &mut self.script_test_results,
                    pending_enable_scripts: &mut self.script_pending_enable,
                    pending_disable_scripts: &mut self.script_pending_disable,
                    runtime_ui: &mut self.runtime_ui,
                    viewport_size: [self.renderer.width(), self.renderer.height()],
                    keys_down: Some(&self.keys_down),
                    move_input: glam::Vec2::ZERO,
                    jump_requested: false,
                    orbit_delta: [0.0, 0.0],
                    character_system: Some(&mut self.character_system),
                    actor_system: Some(&mut self.actor_system),
                    gameplay_camera: Some(&mut self.gameplay_camera),
                    valid_entity_declarations: Some(scene.valid_entity_declarations.clone()),
                    pending_spawns: &mut self.script_pending_spawns,
                    pending_destroys: &mut self.script_pending_destroys,
                    project_path: self.project_path.as_deref(),
                    save_state: self.save_state.as_mut(),
                };

                let mut context = HostContext {
                    delta_time: 0.0,
                    engine: &mut bridge,
                };

                if let Err(error) = scene.start(&mut context) {
                    report_standalone_error(&format!("[ERROR] Scripting startup error: {error}"));
                } else {
                    if has_player {
                        let player_value = Value::Namespace("player".to_string());

                        let camera_value = Value::Namespace("camera".to_string());

                        let input_value = Value::Namespace("input".to_string());

                        match scene.instantiate_package_object(
                            "controllers",
                            &controller_name,
                            vec![
                                player_value.clone(),
                                camera_value.clone(),
                                input_value.clone(),
                            ],
                            &mut context,
                        ) {
                            Ok(object) => self.controller_object = Some(object),
                            Err(error) => report_standalone_error(&format!(
                                "[ERROR] Player controller package initialization failed: {error}"
                            )),
                        }

                        let camera_name = context.engine.get_camera_script();
                        match scene.instantiate_package_object(
                            "cameras",
                            &camera_name,
                            vec![player_value],
                            &mut context,
                        ) {
                            Ok(object) => {
                                self.camera_object = Some(object);
                                self.world.runtime_camera_applied = Some(camera_name);
                            }
                            Err(error) => report_standalone_error(&format!(
                                "[ERROR] Player camera package initialization failed: {error}"
                            )),
                        }
                    }

                    self.script_scene = Some(scene);
                    self.script_pending_events
                        .push(("level_loaded".to_string(), vec![]));
                }
            }

            Err(error) => {
                report_standalone_error(&format!("[ERROR] Scripting load error: {error}"));
            }
        }
    }

    pub fn stop_scripting(&mut self) {
        let scene_opt = self.script_scene.take();

        if let Some(mut scene) = scene_opt {
            let mut bridge = ScriptHostBridge {
                game_state: Some(self.game_state.clone()),
                entity_manager: &mut self.entity_manager,
                attachment_system: Some(&mut self.attachment_system),
                world: &mut self.world,
                mouse: &mut self.mouse,
                dynamic_properties: &mut self.script_dynamic_properties,
                pending_events: &mut self.script_pending_events,
                test_results: &mut self.script_test_results,
                pending_enable_scripts: &mut self.script_pending_enable,
                pending_disable_scripts: &mut self.script_pending_disable,
                runtime_ui: &mut self.runtime_ui,
                viewport_size: [self.renderer.width(), self.renderer.height()],
                keys_down: Some(&self.keys_down),
                move_input: glam::Vec2::ZERO,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: Some(&mut self.character_system),
                actor_system: Some(&mut self.actor_system),
                gameplay_camera: Some(&mut self.gameplay_camera),
                valid_entity_declarations: Some(scene.valid_entity_declarations.clone()),
                pending_spawns: &mut self.script_pending_spawns,
                pending_destroys: &mut self.script_pending_destroys,
                project_path: self.project_path.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: 0.0,
                engine: &mut bridge,
            };

            if let Err(err) = scene.stop(&mut context) {
                eprintln!("[RUNTIME TEARDOWN ERROR]\n{}", err);
            }
        }

        self.script_contacted_last_frame.clear();
        self.script_pending_events.clear();
        self.script_test_results.clear();
        self.script_pending_enable.clear();
        self.script_pending_disable.clear();
        self.script_pending_spawns.clear();
        self.script_pending_destroys.clear();
        self.script_dynamic_properties.clear();

        self.runtime_ui.clear();
        self.world.particles.clear();

        self.controller_object = None;
        self.camera_object = None;
        self.world.runtime_camera_script = None;
        self.world.runtime_camera_applied = None;

        self.world.disabled_scripts = std::mem::take(&mut self.authored_disabled_scripts);

        self.attachment_system
            .clear(&mut self.entity_manager, &mut self.character_system);
        self.actor_system.clear();
        self.character_system.clear();
        self.entity_manager.clear();
        self.world.runtime_post_process_changes.clear();
        self.world.runtime_post_process_authored = None;
        self.renderer.reset_runtime_post_process();
        self.world.runtime_state.clear();
    }

    pub fn on_lifecycle(&mut self, event: GameLifecycleEvent) {
        match event {
            GameLifecycleEvent::Suspend => {
                self.suspended = true;
                self.on_input(super::input::GameInputEvent::FocusLost);
            }
            GameLifecycleEvent::Resume => self.suspended = false,
            GameLifecycleEvent::Shutdown => {
                self.suspended = true;
                self.stop_scripting();
            }
        }
    }
    pub fn restart_session(&mut self) {
        let Some(project_path) = self.project_path.clone() else {
            return;
        };
        self.stop_scripting();
        self.audio_system.stop_all();
        self.entity_manager.clear();
        self.actor_system.clear();
        self.character_system.clear();
        self.attachment_system = AttachmentSystem::default();
        self.world = World::new();
        self.physics_world = PhysicsWorld::new();
        self.physics_clock.reset();
        self.gameplay_camera = GameplayCamera::new();
        self.script_pending_events.clear();
        self.script_dynamic_properties.clear();
        self.input_actions.focus_lost();
        self.game_state.lock().unwrap().reset_session();
        self.load_project(project_path);
    }

    pub fn take_game_action(&self) -> Option<crate::scripting::api::GameLifecycleRequest> {
        self.game_state.lock().ok()?.requested.take()
    }

    pub fn notify_game_quit(&mut self) {
        self.script_pending_events
            .push(("game_on_quit".into(), Vec::new()));
        self.update(0.0);
    }

    pub fn update(&mut self, frame_time: f32) {
        if self.suspended {
            return;
        }
        let frame_time = {
            let mut game = self.game_state.lock().unwrap();
            game.advance(frame_time as f64);
            for event in game.pending_events.drain(..) {
                self.script_pending_events
                    .push((event.to_string(), Vec::new()));
            }
            if game.paused { 0.0 } else { frame_time }
        };
        self.world.update_particles(frame_time);
        let key_state = self.keys_down.clone();

        self.sync_player_entity_from_character();

        if let Some(scene) = self.script_scene.as_mut() {
            let mut bridge = ScriptHostBridge {
                game_state: Some(self.game_state.clone()),
                entity_manager: &mut self.entity_manager,
                attachment_system: Some(&mut self.attachment_system),
                world: &mut self.world,
                mouse: &mut self.mouse,
                dynamic_properties: &mut self.script_dynamic_properties,
                pending_events: &mut self.script_pending_events,
                test_results: &mut self.script_test_results,
                pending_enable_scripts: &mut self.script_pending_enable,
                pending_disable_scripts: &mut self.script_pending_disable,
                runtime_ui: &mut self.runtime_ui,
                viewport_size: [self.renderer.width(), self.renderer.height()],
                keys_down: Some(&key_state),
                move_input: glam::Vec2::ZERO,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: Some(&mut self.character_system),
                actor_system: Some(&mut self.actor_system),
                gameplay_camera: Some(&mut self.gameplay_camera),
                valid_entity_declarations: Some(scene.valid_entity_declarations.clone()),
                pending_spawns: &mut self.script_pending_spawns,
                pending_destroys: &mut self.script_pending_destroys,
                project_path: self.project_path.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: frame_time as f64,
                engine: &mut bridge,
            };

            if let Err(error) = scene.update(frame_time, &mut context) {
                report_standalone_error(&format!("[ERROR] Scripting runtime error: {error}"));
            }

            for record in scene.drain_output() {
                let script_part = if let Some(path) = &record.script_path {
                    format!("{} | ", path)
                } else {
                    String::new()
                };

                let entity_part = if let Some(name) = &record.entity_name {
                    if name == "Global" {
                        String::new()
                    } else {
                        format!(
                            "Entity: {} | ID: {} | ",
                            name,
                            record.entity_id.unwrap_or(0),
                        )
                    }
                } else {
                    String::new()
                };

                let context_part = if let Some(context_name) = &record.context_name {
                    format!("{} | ", context_name)
                } else {
                    String::new()
                };

                let message = format!(
                    "[{}] {}{}{}{}",
                    record.severity.name(),
                    script_part,
                    entity_part,
                    context_part,
                    record.message
                );
                if matches!(
                    record.severity,
                    crate::scripting::log::LogSeverity::Error
                        | crate::scripting::log::LogSeverity::Warning
                ) {
                    report_standalone_error(&message);
                } else {
                    eprintln!("{message}");
                }
            }
        }

        for ((material, layer), alpha) in std::mem::take(&mut self.world.runtime_material_changes) {
            if let Err(error) = self.renderer.set_runtime_material_layer_alpha(&material, &layer, alpha) {
                eprintln!("[ERROR] AeoScript Material: {error}");
            }
        }
        self.physics_world.sync_with_world(&mut self.world);

        let gravity = self.world.gravity;

        let mut raw_input = glam::Vec2::ZERO;

        if self.keys_down.contains(&KeyCode::KeyW) {
            raw_input.y += 1.0;
        }

        if self.keys_down.contains(&KeyCode::KeyS) {
            raw_input.y -= 1.0;
        }

        if self.keys_down.contains(&KeyCode::KeyA) {
            raw_input.x -= 1.0;
        }

        if self.keys_down.contains(&KeyCode::KeyD) {
            raw_input.x += 1.0;
        }

        let mut contacted_this_frame = HashSet::new();

        let mut touch_contacts_this_frame = HashSet::new();

        let scene_opt = &mut self.script_scene;

        let controller_opt = &self.controller_object;

        let camera_opt = &mut self.camera_object;

        let entity_manager = &mut self.entity_manager;

        let attachment_system = &mut self.attachment_system;

        let world = &mut self.world;

        let mouse = &mut self.mouse;

        let dynamic_properties = &mut self.script_dynamic_properties;

        let pending_events = &mut self.script_pending_events;

        let test_results = &mut self.script_test_results;

        let pending_enable = &mut self.script_pending_enable;

        let pending_disable = &mut self.script_pending_disable;

        let runtime_ui = &mut self.runtime_ui;

        let character_system = &mut self.character_system;

        let actor_system = &mut self.actor_system;

        let gameplay_camera = &mut self.gameplay_camera;

        let pending_spawns = &mut self.script_pending_spawns;

        let pending_destroys = &mut self.script_pending_destroys;

        let save_state = &mut self.save_state;

        let valid_decls = scene_opt
            .as_ref()
            .map(|s| s.valid_entity_declarations.clone());

        let mut step_jump = self.jump_requested;

        let mut step_orbit = self.orbit_delta;

        self.orbit_delta = [0.0, 0.0];

        let proj_path = self.project_path.as_deref();
        character_system.set_ocean_appearance(self.renderer.ocean_swim_appearance(world));

        self.physics_clock.update(frame_time, |dt| {
            if character_system.is_player_controller_enabled() {
                if let (Some(scene), Some(controller)) = (scene_opt.as_mut(), controller_opt) {
                    let mut bridge = ScriptHostBridge {
                        game_state: Some(self.game_state.clone()),
                        entity_manager,
                        attachment_system: Some(attachment_system),
                        world,
                        mouse,
                        dynamic_properties,
                        pending_events,
                        test_results,
                        pending_enable_scripts: pending_enable,
                        pending_disable_scripts: pending_disable,
                        runtime_ui,
                        move_input: raw_input,
                        jump_requested: step_jump,
                        viewport_size: [self.renderer.width(), self.renderer.height()],
                        keys_down: Some(&key_state),
                        orbit_delta: step_orbit,
                        character_system: Some(character_system),
                        actor_system: Some(actor_system),
                        gameplay_camera: Some(gameplay_camera),
                        valid_entity_declarations: valid_decls.clone(),
                        pending_spawns,
                        pending_destroys,
                        project_path: proj_path,
                        save_state: save_state.as_mut(),
                    };

                    let mut context = HostContext {
                        delta_time: dt as f64,
                        engine: &mut bridge,
                    };

                    let _ = scene.call_object_method(
                        controller,
                        "update",
                        vec![Value::Number(dt as f64)],
                        &mut context,
                    );
                }
            }
            let p_world = &mut self.physics_world;

            let character_entities: HashSet<_> = character_system.associated_entity_ids().collect();
            if dt.is_finite() && dt > 0.0 {
                for entity in entity_manager.iter_mut() {
                    if !entity.enabled
                        || !entity.physics.enabled
                        || entity.physics.anchored
                        || !entity.physics.mass.is_finite()
                        || entity.physics.mass <= 0.0
                        || character_entities.contains(&entity.id)
                    {
                        continue;
                    }
                    let flow = world.water_current_at(entity.position);
                    if flow.length_squared() > 0.000001 {
                        let relative = flow
                            - glam::Vec3::new(entity.physics.velocity.x, 0.0, entity.physics.velocity.z);
                        entity.physics.accumulated_force += relative * entity.physics.mass * 2.0;
                    }
                }
            }
            let attached_children: HashSet<_> =
                attachment_system.attached_children().into_iter().collect();
            let (camera_forward, _) = gameplay_camera.get_horizontal_basis();
            character_system.set_active_swim_look_direction(
                gameplay_camera.swim_aim_y(),
                glam::Vec2::new(camera_forward.x, camera_forward.z),
            );
            if step_jump && character_system.is_player_controller_enabled() {
                character_system.try_surface_jump(6.5);
            }
            let contacted = character_system.update_scripted_runtime(
                world,
                entity_manager,
                p_world,
                dt,
                &attached_children,
            );
            character_system.sync_entity_positions(entity_manager);
            p_world.step_unified_rapier(
                entity_manager,
                gravity,
                dt,
                &character_entities,
                Some(&mut *character_system),
                Some(world),
            );
            entity_manager.update_animations(dt);

            for event in entity_manager.drain_animation_events() {
                if !character_entities.contains(&event.entity_id) {
                    pending_events.push((
                        event.name,
                        crate::animation::script_event_payload_arguments(&event.payload),
                    ));
                }
            }
            for event in character_system.drain_animation_events() {
                pending_events.push((
                    event.name,
                    crate::animation::script_event_payload_arguments(&event.payload),
                ));
            }

            character_system.sync_entity_positions(entity_manager);

            for entity_id in character_system.drain_navigation_finished() {
                if let Some(actor_id) = actor_system.script_actor_id_for_entity(entity_id) {
                    pending_events.push((
                        "navigation_finished".to_string(),
                        vec![Value::Handle {
                            kind: HandleKind::Actor,
                            id: actor_id,
                        }],
                    ));
                }
            }
            for (entity_id, reason) in character_system.drain_navigation_failed() {
                if let Some(actor_id) = actor_system.script_actor_id_for_entity(entity_id) {
                    pending_events.push((
                        "navigation_failed".to_string(),
                        vec![
                            Value::Handle {
                                kind: HandleKind::Actor,
                                id: actor_id,
                            },
                            Value::String(reason.as_str().to_string()),
                        ],
                    ));
                }
            }

            let entity_contacts = p_world
                .process_entity_contacts_and_collisions_with_world(
                    entity_manager,
                    Some(character_system),
                    world,
                );
            let player_contact_entity = entity_manager.lookup_entity("Player");

            if let Some(scene) = scene_opt.as_mut() {
                let mut bridge = ScriptHostBridge {
                    game_state: Some(self.game_state.clone()),
                    entity_manager,
                    attachment_system: Some(attachment_system),
                    world,
                    mouse,
                    dynamic_properties,
                    pending_events,
                    test_results,
                    pending_enable_scripts: pending_enable,
                    pending_disable_scripts: pending_disable,
                    runtime_ui,
                    move_input: raw_input,
                    jump_requested: step_jump,
                    viewport_size: [self.renderer.width(), self.renderer.height()],
                    keys_down: Some(&key_state),
                    orbit_delta: step_orbit,
                    character_system: Some(character_system),
                    actor_system: Some(actor_system),
                    gameplay_camera: Some(gameplay_camera),
                    valid_entity_declarations: valid_decls.clone(),
                    pending_spawns,
                    pending_destroys,
                    project_path: proj_path,
                    save_state: save_state.as_mut(),
                };

                let mut context = HostContext {
                    delta_time: dt as f64,
                    engine: &mut bridge,
                };

                if let Err(error) =
                    scene.on_entity_contacts(&entity_contacts, player_contact_entity, &mut context)
                {
                    report_standalone_error(&format!("[ERROR] Entity contact error: {error}"));
                }
            }

            attachment_system.update(entity_manager, character_system);
            attachment_system.update_spatial(world, entity_manager, character_system);
            crate::engine::attachment::AttachmentSystem::update_authored_sources(
                &self.project_hierarchy,
                world,
                actor_system,
                entity_manager,
                character_system,
            );
            character_system.sync_entity_positions(entity_manager);

            if let Some(player_entity_id) = entity_manager.lookup_entity("Player") {
                for &(entity_id, cell_id) in &contacted {
                    if entity_id == player_entity_id {
                        contacted_this_frame.insert(cell_id);
                    }
                }
            }

            touch_contacts_this_frame.extend(contacted);

            if let Some(scene) = scene_opt.as_mut() {
                let requested = world.active_camera_script().to_owned();
                let previous = world.runtime_camera_applied.clone();
                let switching = previous.as_deref() != Some(requested.as_str());
                let has_player = character_system.get_active_player().is_some();
                let mut bridge = ScriptHostBridge {
                    game_state: Some(self.game_state.clone()),
                    entity_manager,
                    attachment_system: Some(attachment_system),
                    world,
                    mouse,
                    dynamic_properties,
                    pending_events,
                    test_results,
                    pending_enable_scripts: pending_enable,
                    pending_disable_scripts: pending_disable,
                    runtime_ui,
                    move_input: raw_input,
                    jump_requested: false,
                    viewport_size: [self.renderer.width(), self.renderer.height()],
                    keys_down: Some(&key_state),
                    orbit_delta: step_orbit,
                    character_system: Some(character_system),
                    actor_system: Some(actor_system),
                    gameplay_camera: Some(gameplay_camera),
                    valid_entity_declarations: valid_decls.clone(),
                    pending_spawns,
                    pending_destroys,
                    project_path: proj_path,
                    save_state: save_state.as_mut(),
                };

                let mut context = HostContext {
                    delta_time: dt as f64,
                    engine: &mut bridge,
                };

                let mut activation: Option<Result<(), String>> = None;
                if switching && has_player {
                    let player = Value::Namespace("player".to_string());
                    activation = Some(
                        match scene.instantiate_package_object(
                            "cameras",
                            &requested,
                            vec![player],
                            &mut context,
                        ) {
                            Ok(object) => {
                                *camera_opt = Some(object);
                                Ok(())
                            }
                            Err(error) => Err(error),
                        },
                    );
                }
                if let Some(camera) = camera_opt.as_ref() {
                    let _ = scene.call_object_method(
                        camera,
                        "update",
                        vec![Value::Number(dt as f64)],
                        &mut context,
                    );
                }
                drop(context);
                match activation {
                    Some(Ok(())) => {
                        world.runtime_camera_applied = Some(requested.clone());
                        gameplay_camera.set_mode_from_name(&requested);
                    }
                    Some(Err(error)) => {
                        let fallback = previous
                            .as_deref()
                            .unwrap_or(&world.selected_camera)
                            .to_owned();
                        let _ = world.set_runtime_camera_script(&fallback);
                        crate::engine::diagnostics::report(
                            "ERROR",
                            "Script",
                            format!("camera.set_script({requested:?}) failed: {error}"),
                        );
                    }
                    None => {}
                }
            }

            step_jump = false;
            step_orbit = [0.0, 0.0];
        });

        self.sync_player_entity_from_character();

        if let Some(scene) = &mut self.script_scene {
            let mut bridge = ScriptHostBridge {
                game_state: Some(self.game_state.clone()),
                entity_manager: &mut self.entity_manager,
                attachment_system: Some(&mut self.attachment_system),
                world: &mut self.world,
                mouse: &mut self.mouse,
                viewport_size: [self.renderer.width(), self.renderer.height()],
                keys_down: Some(&key_state),
                dynamic_properties: &mut self.script_dynamic_properties,
                pending_events: &mut self.script_pending_events,
                test_results: &mut self.script_test_results,
                pending_enable_scripts: &mut self.script_pending_enable,
                pending_disable_scripts: &mut self.script_pending_disable,
                runtime_ui: &mut self.runtime_ui,
                move_input: raw_input,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: Some(&mut self.character_system),
                actor_system: Some(&mut self.actor_system),
                gameplay_camera: Some(&mut self.gameplay_camera),
                valid_entity_declarations: Some(scene.valid_entity_declarations.clone()),
                pending_spawns: &mut self.script_pending_spawns,
                pending_destroys: &mut self.script_pending_destroys,
                project_path: self.project_path.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: frame_time as f64,
                engine: &mut bridge,
            };

            for &cell_id in &contacted_this_frame {
                if !self.script_contacted_last_frame.contains(&cell_id) {
                    if let Err(error) = scene.on_player_overlap(cell_id, true, &mut context) {
                        report_standalone_error(&format!("[ERROR] Overlap start error: {error}"));
                    }
                }
            }

            for &cell_id in &self.script_contacted_last_frame {
                if !contacted_this_frame.contains(&cell_id) {
                    if let Err(error) = scene.on_player_overlap(cell_id, false, &mut context) {
                        report_standalone_error(&format!("[ERROR] Overlap end error: {error}"));
                    }
                }
            }

            if !touch_contacts_this_frame.is_empty() {
                if let Err(error) =
                    scene.on_character_contact(&touch_contacts_this_frame, &mut context)
                {
                    report_standalone_error(&format!("[ERROR] Scripting contact event error: {error}"));
                }
            }
        }

        self.world.sync_entity_sound_positions(&self.entity_manager);
        self.audio_system.update(
            &self.world,
            &self.project_path,
            self.gameplay_camera.current_position,
        );

        self.script_contacted_last_frame = contacted_this_frame;

        if self.camera_object.is_none() {
            if let Some(player) = self.character_system.get_active_player() {
                self.gameplay_camera.update(player, &self.world);
            }
        }

        for command in std::mem::take(&mut self.world.runtime_post_process_changes) {
            if let Err(error) = self.renderer.apply_runtime_post_process_command(command) {
                eprintln!("[ERROR] AeoScript Post Processing: {error}");
            }
        }
        self.jump_requested = false;
        self.input_actions.finish_frame();
    }
}
