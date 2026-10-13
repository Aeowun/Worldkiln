use std::collections::HashSet;

use winit::keyboard::KeyCode;

use crate::scripting::api::HostContext;
use crate::scripting::host::ScriptHostBridge;
use crate::scripting::scene::ScriptScene;
use crate::scripting::value::{HandleKind, Value};

use super::App;
use super::app_project::get_timestamp;

impl App {
    pub(crate) fn sync_player_entity_from_character(&mut self) {
        self.character_system
            .sync_entity_positions(&mut self.entity_manager);
    }

    pub(crate) fn spawn_authored_actors(&mut self) -> Result<(), String> {
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
                entity.physics.enabled = actor.physics.enabled;
                entity.physics.solid = actor.physics.solid;
                entity.physics.anchored = actor.physics.anchored;
                entity.physics.collision_enabled = actor.physics.collision_enabled;
                entity.physics.mass = actor.physics.mass;
                // Authored collision dimensions are package-local. Apply the
                // Actor transform here so non-uniformly scaled imported meshes
                // retain collision aligned with their rendered geometry.
                entity.physics.size = actor.physics.collision_size * actor.scale.abs();
                entity.physics.offset = actor.physics.collision_offset * actor.scale;
            }

            let runtime_actor = crate::actor::RuntimeActor {
                entity_id,
                authored_actor_id: Some(actor.id),
                package_name: actor.package.clone(),
                initial_position: actor.position,
            };

            self.actor_system.register_actor(runtime_actor);

            if let Some(package_name) = actor.package.as_deref() {
                if crate::project::is_character_package(
                    self.project_manager.current_project.as_deref(),
                    package_name,
                ) {
                    let character_id = self
                        .character_system
                        .spawn_character(actor.position, Some(package_name));

                    self.character_system
                        .associate_entity(entity_id, character_id);
                    self.character_system.set_entity_kinematic(
                        entity_id,
                        actor.physics.anchored || !actor.physics.enabled,
                    );
                    if let Some(character) = self
                        .character_system
                        .get_character_for_entity_mut(entity_id)
                    {
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

    pub(crate) fn start_scripting(&mut self) {
        self.editor.synchronize_project_actor_nodes(&self.world);

        self.script_contacted_last_frame.clear();
        self.runtime_ui.clear();
        self.world.particles.clear();
        if let Err(error) = crate::engine::ui_authoring::populate_runtime_ui_named(
            &mut self.runtime_ui,
            &self.editor.ui_document,
            &self
                .editor
                .ui_workspace
                .document(self.editor.ui_workspace.active_document_id)
                .map(|d| d.key.as_str())
                .unwrap_or("main_ui"),
        ) {
            crate::engine::diagnostics::report(
                "ERROR",
                "Runtime UI",
                format!("Failed to load authored runtime UI: {error}"),
            );
        }

        self.controller_object = None;
        self.camera_object = None;
        // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
        self.world.runtime_camera_script = None;
        self.world.runtime_camera_applied = None;

        self.authored_disabled_scripts = self.world.disabled_scripts.clone();

        let Some(project_path) = &self.project_manager.current_project else {
            return;
        };

        self.save_state = match crate::engine::save_state::SaveState::load(project_path) {
            Ok(state) => Some(state),
            Err(error) => {
                self.editor.append_output(&format!(
                    "[{}] [ERROR] [Save] Failed to load runtime save state: {}\n",
                    get_timestamp(),
                    error
                ));
                None
            }
        };

        let controller_name = self.world.selected_controller.clone();

        match ScriptScene::load_from_bindings(
            project_path,
            &self.world,
            &self.world.script_bindings,
            &mut self.entity_manager,
            self.actor_system.actor_to_entity_map(),
            0.0,
        ) {
            Ok(mut scene) => {
                // Use the live editor hierarchy rather than relying only on
                // the last project.json save when entering Play.
                scene.set_project_hierarchy(self.editor.project_hierarchy.clone(), &self.world);

                let has_player = self.character_system.get_active_player().is_some();
                let mut bridge = ScriptHostBridge {
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
                    viewport_size: [
                        self.editor.viewport_rect.width(),
                        self.editor.viewport_rect.height(),
                    ],
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
                    project_path: self.project_manager.current_project.as_deref(),
                    save_state: self.save_state.as_mut(),
                };

                let mut context = HostContext {
                    delta_time: 0.0,
                    engine: &mut bridge,
                };

                if let Err(error) = scene.start(&mut context) {
                    self.editor.append_output(&format!(
                        "[{}] [ERROR] [Script] Scripting startup error: {}\n",
                        get_timestamp(),
                        error
                    ));
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
                            Err(error) => crate::engine::diagnostics::report(
                                "ERROR",
                                "Script",
                                format!("Player controller package initialization failed: {error}"),
                            ),
                        }

                        // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
                        // Scene startup may request a camera different from Project Settings.
                        let camera_name = context.engine.get_camera_script();
                        match scene.instantiate_package_object(
                            "cameras",
                            &camera_name,
                            vec![player_value],
                            &mut context,
                        ) {
                            Ok(object) => {
                                self.camera_object = Some(object);
                                self.world.runtime_camera_applied = Some(camera_name.clone());
                            }
                            Err(error) => crate::engine::diagnostics::report(
                                "ERROR",
                                "Script",
                                format!("Player camera package initialization failed: {error}"),
                            ),
                        }
                    }

                    self.script_scene = Some(scene);
                }
            }
            Err(error) => {
                self.editor.append_output(&format!(
                    "[{}] [ERROR] [Script] Scripting load error: {}\n",
                    get_timestamp(),
                    error
                ));
            }
        }
    }

    pub(crate) fn stop_scripting(&mut self) {
        let scene_opt = self.script_scene.take();

        if let Some(mut scene) = scene_opt {
            let mut bridge = ScriptHostBridge {
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

                viewport_size: [
                    self.editor.viewport_rect.width(),
                    self.editor.viewport_rect.height(),
                ],

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

                project_path: self.project_manager.current_project.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: 0.0,
                engine: &mut bridge,
            };

            if let Err(err) = scene.stop(&mut context) {
                eprintln!("[RUNTIME TEARDOWN ERROR]\n{}", err);
                self.editor
                    .append_output(&format!("\n[ERROR] [Script Teardown] {}\n", err));
            }
        }

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:45 -05:00
        self.editor.script_editor.clear_debug_runtime_state();

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
        // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
        self.world.runtime_camera_script = None;
        self.world.runtime_camera_applied = None;

        self.world.disabled_scripts = std::mem::take(&mut self.authored_disabled_scripts);

        self.attachment_system
            .clear(&mut self.entity_manager, &mut self.character_system);
        self.entity_manager.clear();
    }

    pub(crate) fn fire_player_spawned_event(&mut self, player_entity_id: u64) {
        if let Some(scene) = &mut self.script_scene {
            let mut bridge = ScriptHostBridge {
                entity_manager: &mut self.entity_manager,
                attachment_system: Some(&mut self.attachment_system),

                world: &mut self.world,

                mouse: &mut self.mouse,

                viewport_size: [
                    self.editor.viewport_rect.width(),
                    self.editor.viewport_rect.height(),
                ],

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

                project_path: self.project_manager.current_project.as_deref(),
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
                self.editor.append_output(&format!(
                    "[{}] [ERROR] Event PlayerSpawned error: {}\n",
                    get_timestamp(),
                    error
                ));
            }
        }
    }

    pub(crate) fn update_gameplay(&mut self, frame_time: f32) {
        let key_state = self.keys_down.clone();

        /*
         * ----------------------------------------------------------
         * SCRIPT PHASE
         * ----------------------------------------------------------
         *
         * CharacterSystem is authoritative for the player's current
         * transform. Publish that transform to the script-facing
         * Player entity before scripts run.
         *
         * There is NO reverse position comparison here.
         *
         * When AeoScript explicitly calls:
         *
         *   get_entity("Player").set_position(...)
         *
         * the existing EngineHost::set_position() implementation
         * handles the deliberate teleport immediately.
         */
        self.sync_player_entity_from_character();

        if let Some(scene) = self.script_scene.as_mut() {
            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:45 -05:00
            scene.set_debug_breakpoints(self.editor.script_editor.runtime_breakpoints());
            if let Some(command) = self.editor.script_editor.pending_debug_command.take() {
                if let Err(error) = scene.debug_command(command) {
                    self.editor.script_editor.debug_status = Some(error);
                }
            }

            let mut bridge = ScriptHostBridge {
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

                viewport_size: [
                    self.editor.viewport_rect.width(),
                    self.editor.viewport_rect.height(),
                ],

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

                project_path: self.project_manager.current_project.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: frame_time as f64,

                engine: &mut bridge,
            };

            let runtime_error = scene.update(frame_time, &mut context).err();
            if let Some(error) = &runtime_error {
                self.editor.append_output(&format!(
                    "[{}] [ERROR] [Script] Scripting runtime error: {}\n",
                    get_timestamp(),
                    error
                ));
            }

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:45 -05:00
            let snapshot = scene.debug_snapshot();
            self.editor
                .script_editor
                .set_debug_snapshot(snapshot, self.project_manager.current_project.as_deref());

            for record in scene.drain_output() {
                // The interpreter also records the original failure. Keep the
                // scene's enriched path/frame message once, not both copies.
                if record.severity == crate::scripting::log::LogSeverity::Error
                    && runtime_error
                        .as_ref()
                        .is_some_and(|error| error.contains(&record.message))
                {
                    continue;
                }
                let timestamp = get_timestamp();

                let formatted = if record.script_path.is_none() && record.entity_name.is_none() {
                    let engine_context = match record.severity {
                        crate::scripting::log::LogSeverity::Error => "Engine Error",

                        crate::scripting::log::LogSeverity::Warning => "Engine Warning",

                        _ => "Engine",
                    };

                    format!(
                        "[{}] [{}] {} | {}\n",
                        timestamp,
                        record.severity.name(),
                        engine_context,
                        record.message
                    )
                } else {
                    let script_part = if let Some(path) = &record.script_path {
                        format!(
                            "{}{} | ",
                            path,
                            record
                                .line
                                .map(|line| format!(":{}:{}", line, record.column.unwrap_or(1)))
                                .unwrap_or_default()
                        )
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
                                record.entity_id.unwrap_or(0)
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

                    format!(
                        "[{}] [{}] [Script] {}{}{}{}\n",
                        timestamp,
                        record.severity.name(),
                        script_part,
                        entity_part,
                        context_part,
                        record.message
                    )
                };

                self.editor.append_output(&formatted);
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

        /*
         * Input requests are edge-triggered per render frame.
         *
         * A fixed-timestep loop may execute several simulation steps
         * during one render frame, but a single key press should not
         * become several jump requests.
         */
        let mut step_jump = self.jump_requested;

        let mut step_orbit = self.orbit_delta;

        self.orbit_delta = [0.0, 0.0];

        self.physics_clock.update(frame_time, |dt| {
            /*
             * --------------------------------------------------
             * SCRIPTED CONTROLLER
             * --------------------------------------------------
             *
             * The controller writes player movement state
             * directly through EngineHost.
             */
            if let (Some(scene), Some(controller)) = (scene_opt.as_mut(), controller_opt) {
                let mut bridge = ScriptHostBridge {
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

                    viewport_size: [
                        self.editor.viewport_rect.width(),
                        self.editor.viewport_rect.height(),
                    ],

                    keys_down: Some(&key_state),

                    orbit_delta: step_orbit,

                    character_system: Some(character_system),

                    actor_system: Some(actor_system),

                    gameplay_camera: Some(gameplay_camera),

                    valid_entity_declarations: valid_decls.clone(),

                    pending_spawns,

                    pending_destroys,

                    project_path: self.project_manager.current_project.as_deref(),
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

            // CharacterSystem consumes the horizontal velocity supplied by the
            // scripted controller above. It owns gravity, integration, collision,
            // grounded state, and animation.

            let p_world = &mut self.physics_world;
            // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
            // Editor Play and packaged runtime share the floor-safe Cell solver.
            p_world.step_simulation(gravity, dt);

            let character_entities: HashSet<_> = character_system.associated_entity_ids().collect();
            p_world.step_entities_excluding(entity_manager, gravity, dt, &character_entities);
            entity_manager.update_animations(dt);

            let attached_children: HashSet<_> =
                attachment_system.attached_children().into_iter().collect();
            let contacted =
                character_system.update_scripted_excluding(world, p_world, dt, &attached_children);

            // Contact dispatch reads Entity transforms. CharacterSystem owns
            // package-backed Actor movement, so publish its latest positions
            // before evaluating Entity/Character overlaps.
            character_system.sync_entity_positions(entity_manager);

            // Navigation completes inside CharacterSystem's physics-owned step.
            // Queue one ordinary script event per completed Actor so callbacks
            // run on the next scene tick rather than re-entering scripts here.
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
                .process_entity_contacts_and_collisions(entity_manager, Some(character_system));

            if let Some(scene) = scene_opt.as_mut() {
                let mut bridge = ScriptHostBridge {
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
                    viewport_size: [
                        self.editor.viewport_rect.width(),
                        self.editor.viewport_rect.height(),
                    ],
                    keys_down: Some(&key_state),
                    orbit_delta: step_orbit,
                    character_system: Some(character_system),
                    actor_system: Some(actor_system),
                    gameplay_camera: Some(gameplay_camera),
                    valid_entity_declarations: valid_decls.clone(),
                    pending_spawns,
                    pending_destroys,
                    project_path: self.project_manager.current_project.as_deref(),
                    save_state: save_state.as_mut(),
                };

                let mut context = HostContext {
                    delta_time: dt as f64,
                    engine: &mut bridge,
                };

                if let Err(error) = scene.on_entity_contacts(&entity_contacts, &mut context) {
                    self.editor.append_output(&format!(
                        "[{}] [ERROR] Entity contact error: {}\n",
                        get_timestamp(),
                        error
                    ));
                }
            }

            attachment_system.update(entity_manager, character_system);
            character_system.sync_entity_positions(entity_manager);

            if let Some(player_entity_id) = entity_manager.lookup_entity("Player") {
                for &(entity_id, cell_id) in &contacted {
                    if entity_id == player_entity_id {
                        contacted_this_frame.insert(cell_id);
                    }
                }
            }

            touch_contacts_this_frame.extend(contacted);

            // Update the scripted camera after the character has completed this
            // fixed simulation step so it follows the latest authoritative transform.
            // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
            // Switch the real camera package object on request, not only the view mode.
            if let Some(scene) = scene_opt.as_mut() {
                let requested = world.active_camera_script().to_owned();
                let previous = world.runtime_camera_applied.clone();
                let switching = previous.as_deref() != Some(requested.as_str());
                let has_player = character_system.get_active_player().is_some();
                let mut bridge = ScriptHostBridge {
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
                    viewport_size: [
                        self.editor.viewport_rect.width(),
                        self.editor.viewport_rect.height(),
                    ],
                    keys_down: Some(&key_state),
                    orbit_delta: step_orbit,
                    character_system: Some(character_system),
                    actor_system: Some(actor_system),
                    gameplay_camera: Some(gameplay_camera),
                    valid_entity_declarations: valid_decls.clone(),
                    pending_spawns,
                    pending_destroys,
                    project_path: self.project_manager.current_project.as_deref(),
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

            // These inputs are render-frame requests, not continuous fixed-step state.
            // Consuming them here prevents one key press from becoming multiple jumps
            // or repeated camera orbit operations during catch-up steps.
            step_jump = false;
            step_orbit = [0.0, 0.0];
        });

        /*
         * ----------------------------------------------------------
         * FINAL SCRIPT-FACING PLAYER SYNC
         * ----------------------------------------------------------
         *
         * Physics is authoritative. Publish the final transform
         * after the fixed simulation has completed.
         */
        self.sync_player_entity_from_character();

        /*
         * Collision/event processing remains outside the fixed
         * movement implementation so overlap transitions are evaluated
         * once per rendered frame.
         */
        if let Some(scene) = &mut self.script_scene {
            let mut bridge = ScriptHostBridge {
                entity_manager: &mut self.entity_manager,
                attachment_system: Some(&mut self.attachment_system),

                world: &mut self.world,

                mouse: &mut self.mouse,

                viewport_size: [
                    self.editor.viewport_rect.width(),
                    self.editor.viewport_rect.height(),
                ],

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

                project_path: self.project_manager.current_project.as_deref(),
                save_state: self.save_state.as_mut(),
            };

            let mut context = HostContext {
                delta_time: frame_time as f64,

                engine: &mut bridge,
            };

            for &cell_id in &contacted_this_frame {
                if !self.script_contacted_last_frame.contains(&cell_id) {
                    if let Err(error) = scene.on_player_overlap(cell_id, true, &mut context) {
                        self.editor.append_output(&format!(
                            "[{}] [ERROR] Overlap start error: {}\n",
                            get_timestamp(),
                            error
                        ));
                    }
                }
            }

            for &cell_id in &self.script_contacted_last_frame {
                if !contacted_this_frame.contains(&cell_id) {
                    if let Err(error) = scene.on_player_overlap(cell_id, false, &mut context) {
                        self.editor.append_output(&format!(
                            "[{}] [ERROR] Overlap end error: {}\n",
                            get_timestamp(),
                            error
                        ));
                    }
                }
            }

            if !touch_contacts_this_frame.is_empty() {
                if let Err(error) =
                    scene.on_character_contact(&touch_contacts_this_frame, &mut context)
                {
                    self.editor.append_output(&format!(
                        "[{}] [ERROR] [Script] Scripting contact event error: {}\n",
                        get_timestamp(),
                        error
                    ));
                }
            }
        }

        self.audio_system
            .update(&self.world, &self.project_manager.current_project);

        self.script_contacted_last_frame = contacted_this_frame;

        if self.camera_object.is_none() {
            if let Some(player) = self.character_system.get_active_player() {
                self.gameplay_camera.update(player, &self.world);
            }
        }

        self.jump_requested = false;
    }
}
