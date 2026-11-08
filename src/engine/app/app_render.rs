// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{App, View};

impl App {
    pub fn render(&self) {
        let recording = self.development_capture.borrow().state()
            == crate::engine::development_test_run::CaptureState::Running;
        if recording {
            let slow_frame = self.development_capture.borrow_mut().frame_boundary();
            if let Some(trigger_frame) = slow_frame {
                let config = self.development_capture.borrow().config();
                if config.resources {
                    let mut packages = std::collections::BTreeMap::<String, usize>::new();
                    let mut characters = 0usize;
                    for character in self.character_system.get_active_characters() {
                        characters += 1;
                        *packages.entry(character.package_name.clone()).or_default() += 1;
                    }
                    let counts = crate::engine::development_test_run::ResourceSnapshot {
                        physics_entities: self
                            .entity_manager
                            .iter()
                            .filter(|entity| entity.enabled && entity.physics.enabled)
                            .count(),
                        active_characters: characters,
                        dynamic_bodies: self.physics_world.bodies.len(),
                        active_dynamic_bodies: self
                            .physics_world
                            .bodies
                            .iter()
                            .filter(|b| !b.is_sleeping && !b.anchored)
                            .count(),
                        sleeping_dynamic_bodies: self
                            .physics_world
                            .bodies
                            .iter()
                            .filter(|b| b.is_sleeping && !b.anchored)
                            .count(),
                        broadphase_cells: self.physics_world.dynamic_bodies_by_cell.len(),
                        broadphase_references: self
                            .physics_world
                            .dynamic_bodies_by_cell
                            .values()
                            .map(Vec::len)
                            .sum(),
                        anchored_colliders: self.physics_world.static_colliders.len(),
                        static_collider_chunks: self.physics_world.static_colliders_by_chunk.len(),
                        noncolliding_physics_entities: self
                            .entity_manager
                            .iter()
                            .filter(|e| {
                                e.enabled
                                    && e.physics.enabled
                                    && (!e.physics.solid || !e.physics.collision_enabled)
                            })
                            .count(),
                        character_packages: packages.into_iter().collect(),
                    };
                    self.development_capture
                        .borrow_mut()
                        .note_trigger_resources(trigger_frame, counts);
                }
            }
            crate::engine::development_capture::engine_frame(
                self.development_capture.borrow().next_frame(),
            );
            crate::renderer::effekseer::set_profiling(true);
        }
        #[cfg(feature = "editor")]
        {
            let capture = self.development_capture.borrow();
            self.renderer.configure_development_profile(
                if recording && (capture.config().cpu || capture.config().gpu) {
                    Some((
                        capture.generation(),
                        capture.next_frame(),
                        std::time::Instant::now(),
                    ))
                } else {
                    None
                },
                capture.config().gpu,
            );
        }
        let started = recording.then(std::time::Instant::now);
        match self.view {
            View::Splash => {
                self.renderer.render_home();
            }

            View::Home => {
                self.renderer.render_home();
            }

            View::Editor => {
                if self.editor.post_process_dirty.replace(false) {
                    self.renderer.refresh_post_process_pipeline(
                        self.project_manager.current_project.as_deref(),
                    );
                }
                for command in self
                    .editor
                    .post_process_runtime_commands
                    .borrow_mut()
                    .drain(..)
                {
                    if let Err(error) = self.renderer.apply_runtime_post_process_command(command) {
                        crate::engine::diagnostics::report("ERROR", "Post Processing", error);
                    }
                }
                self.renderer.render_editor(
                    &self.editor,
                    &self.world,
                    &self.physics_world,
                    &self.entity_manager,
                    &self.character_system,
                    &self.gameplay_camera,
                    &self.actor_system,
                    if self.is_left_mouse_down {
                        self.drag_start_coord
                    } else {
                        None
                    },
                );
            }
        }
        #[cfg(feature = "editor")]
        if recording {
            let (samples, dropped) = self.renderer.take_development_profile();
            self.development_capture
                .borrow_mut()
                .record_render_samples(samples, dropped);
        }
        if let Some(started) = started {
            if let Some(fx_cpu) = crate::renderer::effekseer::take_profile_duration() {
                self.development_capture.borrow_mut().record(
                    "fx",
                    "native_effekseer_draw_cpu",
                    fx_cpu,
                );
            }
            if let Some(fx_gpu) = crate::renderer::effekseer::take_profile_gpu_duration() {
                self.development_capture.borrow_mut().record(
                    "fx",
                    "native_effekseer_draw_gpu",
                    fx_gpu,
                );
            }
            crate::renderer::effekseer::set_profiling(false);
            let operation = match self.view {
                View::Editor => "editor_render_cpu",
                View::Home => "home_render_cpu",
                View::Splash => "splash_render_cpu",
            };
            self.development_capture
                .borrow_mut()
                .record("render", operation, started.elapsed());
        }
    }
}
