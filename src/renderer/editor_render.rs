// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use glam::{Mat4, Vec3};
use std::collections::HashMap;

use crate::editor::{Editor, GridPlane};
use crate::engine::EditorMode;
use crate::engine::entity::{EntityManager, RuntimeEntity};
use crate::engine::physics::PhysicsWorld;
use crate::world::{CellType, World, WorldCoord};

use super::Renderer;
use super::SHADOW_RES;
use super::chunk::CameraFrustum;
use super::settings::ShadowMethod;

fn actor_preview_delta(editor: &Editor, actor_id: u64) -> Vec3 {
    editor
        .actor_grab_state
        .as_ref()
        .filter(|grab| grab.actor_id == actor_id)
        .map(|grab| grab.current_position - grab.start_position)
        .unwrap_or(Vec3::ZERO)
}

fn actor_preview_position(editor: &Editor, actor: &crate::world::Actor) -> Vec3 {
    actor.position + actor_preview_delta(editor, actor.id)
}

enum AuthoredActorMesh {
    Character(Vec<crate::engine::entity_visual::StaticEntityMeshBatch>),
    Entity(RuntimeEntity),
}

fn authored_actor_transform(editor: &Editor, actor: &crate::world::Actor) -> Mat4 {
    let preview = editor
        .actor_transform_preview
        .as_ref()
        .filter(|p| p.actor_id == actor.id);
    let position = preview
        .map(|p| p.current_position)
        .unwrap_or_else(|| actor_preview_position(editor, actor));
    let rotation_value = preview
        .map(|p| p.current_rotation)
        .unwrap_or(actor.rotation);
    let scale = preview.map(|p| p.current_scale).unwrap_or(actor.scale);
    let rotation = glam::Quat::from_euler(
        glam::EulerRot::XYZ,
        rotation_value.x,
        rotation_value.y,
        rotation_value.z,
    );
    let model = Mat4::from_scale_rotation_translation(scale, rotation, position);
    model
}

fn authored_actor_mesh(
    editor: &Editor,
    actor: &crate::world::Actor,
) -> Option<(AuthoredActorMesh, Mat4)> {
    let package_name = actor.package.as_deref()?;
    let model = authored_actor_transform(editor, actor);
    let position = actor_preview_position(editor, actor);
    let rotation = glam::Quat::from_euler(
        glam::EulerRot::XYZ,
        actor.rotation.x,
        actor.rotation.y,
        actor.rotation.z,
    );
    let scale = actor.scale;
    if let Some(package) = crate::character::package::get_package(package_name) {
        let mut character = crate::character::Character::from_package(actor.id, position, package);
        character.transform.rotation = rotation;
        character.transform.scale = scale;
        crate::character::character::set_model_material(&mut character, &actor.attributes);
        return Some((
            AuthoredActorMesh::Character(
                crate::character::character::generate_character_mesh_batches(&character),
            ),
            model,
        ));
    }

    crate::engine::entity_package::get_package(package_name)?;
    let mut entity = RuntimeEntity::new(crate::engine::entity::EntityId(actor.id), &actor.identity);
    entity.visible = actor.visible;
    entity.enabled = actor.enabled;
    entity.package_name = Some(package_name.to_string());
    entity.attributes = actor.attributes.clone();
    Some((AuthoredActorMesh::Entity(entity), model))
}

fn draw_authored_actor_mesh(renderer: &Renderer, mesh: &AuthoredActorMesh, textured: bool) -> i32 {
    match mesh {
        AuthoredActorMesh::Character(batches) => renderer.draw_character_batches(batches, textured),
        AuthoredActorMesh::Entity(entity) => renderer.draw_runtime_entity_mesh(entity, textured),
    }
}

fn actor_preview_bounds(editor: &Editor, actor: &crate::world::Actor) -> (Vec3, Vec3) {
    if let Some(preview) = editor
        .actor_transform_preview
        .as_ref()
        .filter(|p| p.actor_id == actor.id)
    {
        let mut visual = actor.clone();
        visual.position = preview.current_position;
        visual.rotation = preview.current_rotation;
        visual.scale = preview.current_scale;
        return crate::editor::grid::picking::actor_bounds(&visual);
    }
    let (min, max) = crate::editor::grid::picking::actor_bounds(actor);
    let delta = actor_preview_delta(editor, actor.id);
    (min + delta, max + delta)
}

fn source_preview_position(editor: &Editor, source: &crate::world::AuthoredSource) -> Vec3 {
    editor
        .source_grab_state
        .as_ref()
        .filter(|grab| grab.source_id == source.cell.id)
        .map(|grab| grab.current_position)
        .unwrap_or(source.position)
}

fn editor_camera_frame(
    editor: &Editor,
    characters: &crate::character::CharacterSystem,
    gameplay_camera: &crate::renderer::camera::GameplayCamera,
) -> (Vec3, Mat4, Vec3) {
    if editor.mode == EditorMode::Play && characters.get_active_player().is_some() {
        (
            gameplay_camera.current_position,
            gameplay_camera.get_view_matrix(),
            gameplay_camera.current_target,
        )
    } else {
        (
            editor.camera.get_position(),
            editor.camera.get_view_matrix(),
            editor.camera.target,
        )
    }
}

pub(super) struct BrushRingGpu {
    pub key: [u32; 9],
    pub vao: u32,
    pub vbo: u32,
    pub count: i32,
}

impl Renderer {
    pub fn render_editor(
        &self,
        editor: &Editor,
        world: &World,
        physics: &PhysicsWorld,
        entity_manager: &EntityManager,
        character_system: &crate::character::CharacterSystem,
        gameplay_camera: &crate::renderer::camera::GameplayCamera,
        actor_system: &crate::actor::ActorSystem,
        drag_start: Option<WorldCoord>,
    ) {
        let mut profile = self.editor_profiler.borrow_mut();
        if let Some(p) = profile.as_mut() {
            p.begin();
        }
        let mut terrain_stats = super::terrain::TerrainDrawStats::default();
        let mut water_stats = super::terrain::TerrainDrawStats::default();

        if self.grid_program != 0 {
            unsafe {
                gl::UseProgram(self.grid_program);
                gl::Uniform1i(
                    super::resources::get_uniform_location(self.grid_program, "u_water_basic"),
                    i32::from(
                        world.terrain.config.water_realism
                            == crate::world::terrain::WaterRealismMode::Basic,
                    ),
                );
                gl::UseProgram(0);
            }
        }
        let (camera_pos, view, target) =
            editor_camera_frame(editor, character_system, gameplay_camera);
        let perf_setup_start = std::time::Instant::now();
        if world.terrain.config.water_realism == crate::world::terrain::WaterRealismMode::Basic {
            let mut wakes = self.water_wakes.borrow_mut();
            wakes.trails.clear();
            wakes.contacts.clear();
            wakes.previous.clear();
        } else {
            self.refresh_water_contacts(world, camera_pos, entity_manager, character_system);
        }

        let perf_after_contacts = std::time::Instant::now();
        let ppp = editor.viewport_ppp.max(1.0);
        let chunk_radius = crate::world::world::settings::RenderDistanceSettings {
            editor_chunks: editor.editor_max_distance_chunks,
            play_chunks: editor.play_max_distance_chunks,
        }
        .radius(editor.mode == EditorMode::Play);
        let chunk_focus = if editor.mode == EditorMode::Play {
            target
        } else {
            camera_pos
        };

        let rect = editor.viewport_rect;

        let (vp_x, vp_y, vp_w, vp_h, aspect_ratio) = if rect.width() > 1.0 && rect.height() > 1.0 {
            let x = (rect.min.x * ppp) as i32;
            let w = (rect.width() * ppp) as i32;
            let h = (rect.height() * ppp) as i32;
            let y = self.height as i32 - (rect.max.y * ppp) as i32;

            (x, y, w, h, w as f32 / h.max(1) as f32)
        } else {
            (
                0,
                0,
                self.width as i32,
                self.height as i32,
                self.width / self.height.max(1.0),
            )
        };

        let projection = glam::camera::rh::proj::opengl::perspective(
            60.0_f32.to_radians(),
            aspect_ratio,
            0.1,
            1000.0,
        );

        let view_projection = projection * view;
        let camera_frustum = CameraFrustum::from_view_projection(view_projection);

        const CHARACTER_CAMERA_HIDE_DISTANCE: f32 = 2.0;

        let light_space_matrix = self.calculate_light_space_matrix_with_range(
            world.lighting.global_light_direction,
            camera_pos,
            3.0,
        );
        let render_settings = self.active_lighting_settings();
        let render_shadows =
            world.lighting.shadows_enabled && render_settings.shadow_method != ShadowMethod::Off;
        let shadow_frustum = CameraFrustum::from_view_projection(light_space_matrix);
        let mut point_lights = self.collect_point_lights(world);
        point_lights.retain(|light| {
            let radius = Vec3::splat(light.range.max(0.0));
            camera_frustum.intersects_aabb(light.position - radius, light.position + radius)
        });
        self.prepare_point_shadows(world, camera_pos, render_shadows, &mut point_lights);
        let perf_before_gi = std::time::Instant::now();
        self.update_gi_cache(world, physics, camera_pos, &point_lights, render_settings);
        let perf_after_gi = std::time::Instant::now();
        let mut prepared_actor_meshes: HashMap<u64, (AuthoredActorMesh, Mat4)> = HashMap::new();

        unsafe {
            gl::Enable(gl::DEPTH_TEST);
            gl::DepthMask(gl::TRUE);
            gl::DepthFunc(gl::LESS);

            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
            gl::FrontFace(gl::CCW);

            gl::Disable(gl::BLEND);

            let perf_before_chunks = std::time::Instant::now();
            self.update_chunk_cache(
                world,
                match editor.mode {
                    crate::engine::EditorMode::Editor => super::RenderMode::Editor,
                    crate::engine::EditorMode::Play => super::RenderMode::Game,
                },
            );
            if std::env::var("WORLDKILN_PERF_TRACE").as_deref() == Ok("1") {
                static SETUP_SAMPLE: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                if SETUP_SAMPLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 120 == 0 {
                    let ms = |a: std::time::Instant, b: std::time::Instant| {
                        (b - a).as_secs_f64() * 1000.0
                    };
                    eprintln!(
                        "[PERF SETUP] water_contacts={:.2} ms camera/setup={:.2} ms gi={:.2} ms chunk_cache={:.2} ms",
                        ms(perf_setup_start, perf_after_contacts),
                        ms(perf_after_contacts, perf_before_gi),
                        ms(perf_before_gi, perf_after_gi),
                        ms(perf_before_chunks, std::time::Instant::now())
                    );
                }
            }

            if let Some(p) = profile.as_mut() {
                p.mark(1);
            }
            if render_shadows {
                gl::Disable(gl::SCISSOR_TEST);
                gl::BindFramebuffer(gl::FRAMEBUFFER, self.shadow_fbo);

                gl::Viewport(0, 0, SHADOW_RES, SHADOW_RES);

                gl::Clear(gl::DEPTH_BUFFER_BIT);

                gl::UseProgram(self.shadow_program);

                gl::UniformMatrix4fv(
                    self.shadow_light_space_matrix_location,
                    1,
                    gl::FALSE,
                    light_space_matrix.to_cols_array().as_ptr(),
                );

                let cache = self.chunk_cache.borrow();

                for chunk_mesh in cache.values() {
                    if !super::distance::within_chunk_radius(
                        chunk_mesh.chunk_coord,
                        chunk_focus,
                        chunk_radius,
                    ) {
                        continue;
                    }

                    let (shadow_min, shadow_max) = chunk_mesh.chunk_coord.aabb_min_max();
                    if !shadow_frustum.intersects_aabb(shadow_min, shadow_max) {
                        continue;
                    }

                    if chunk_mesh.shadow_vao != 0 && chunk_mesh.shadow_vertex_count > 0 {
                        let origin = chunk_mesh.chunk_coord.world_origin();

                        let model = Mat4::from_translation(origin);

                        gl::UniformMatrix4fv(
                            self.shadow_model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );

                        gl::BindVertexArray(chunk_mesh.shadow_vao);

                        gl::DrawArrays(gl::TRIANGLES, 0, chunk_mesh.shadow_vertex_count);
                    }
                }

                if editor.mode == EditorMode::Editor {
                    for actor in world.actors.values() {
                        if !actor.visible || !actor.enabled {
                            continue;
                        }

                        let (bounds_min, bounds_max) = actor_preview_bounds(editor, actor);
                        if !shadow_frustum.intersects_aabb(bounds_min, bounds_max) {
                            continue;
                        }

                        if actor.package.as_deref().is_some_and(|name| {
                            crate::character::package::get_package(name).is_some()
                        }) {
                            let model = authored_actor_transform(editor, actor);
                            gl::UniformMatrix4fv(
                                self.shadow_model_location,
                                1,
                                gl::FALSE,
                                model.to_cols_array().as_ptr(),
                            );
                            self.draw_authored_character_cached(actor, false);
                            continue;
                        }
                        let Some((mesh, model)) = authored_actor_mesh(editor, actor) else {
                            continue;
                        };

                        gl::UniformMatrix4fv(
                            self.shadow_model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );

                        draw_authored_actor_mesh(self, &mesh, false);
                        prepared_actor_meshes.insert(actor.id, (mesh, model));
                    }
                }

                if editor.mode == EditorMode::Play {
                    let (first_vertex, count) = self.get_block_mask_range(63);

                    gl::BindVertexArray(self.block_vao);

                    for body in &physics.bodies {
                        if !body.solid {
                            continue;
                        }

                        let model = super::physics_body_model_matrix(body);

                        gl::UniformMatrix4fv(
                            self.shadow_model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );

                        gl::DrawArrays(gl::TRIANGLES, first_vertex, count);
                    }

                    for entity in entity_manager.iter() {
                        if character_system
                            .get_character_for_entity(entity.id)
                            .is_some()
                            || !entity.visible
                            || !entity.enabled
                        {
                            continue;
                        }
                        if !super::game_render::shadow_caster_in_light_frustum(
                            crate::engine::entity_visual::static_entity_world_bounds(entity),
                            &shadow_frustum,
                        ) {
                            continue;
                        }
                        let rotation = glam::Quat::from_euler(
                            glam::EulerRot::XYZ,
                            entity.rotation.x,
                            entity.rotation.y,
                            entity.rotation.z,
                        );
                        let model = Mat4::from_scale_rotation_translation(
                            entity.scale,
                            rotation,
                            entity.position,
                        );
                        gl::UniformMatrix4fv(
                            self.shadow_model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );
                        self.draw_runtime_entity_mesh(entity, false);
                    }

                    for character in character_system.get_active_characters() {
                        let center = character.transform.position;
                        let extent = Vec3::splat(3.0)
                            * character.transform.scale.abs().max_element().max(0.1);
                        if !shadow_frustum.intersects_aabb(center - extent, center + extent) {
                            continue;
                        }
                        let model = Mat4::from_scale_rotation_translation(
                            character.transform.scale,
                            character.transform.rotation,
                            character.transform.position,
                        );
                        gl::UniformMatrix4fv(
                            self.shadow_model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );
                        let batches =
                            crate::character::character::generate_character_mesh_batches(character);
                        self.draw_character_batches(&batches, false);
                    }
                }
            }

            if render_shadows {
                let mut character_shadow_meshes = None;
                self.render_point_shadow_faces(world, &mut point_lights, |light| {
                    use std::hash::{Hash, Hasher};
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    let radius2 = light.range * light.range;
                    if editor.mode == EditorMode::Editor {
                        for actor in world.actors.values().filter(|a| a.enabled && a.visible) {
                            if actor.position.distance_squared(light.position) > radius2 { continue; }
                            actor.position.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                            actor.rotation.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                        }
                    } else {
                        for entity in entity_manager.iter().filter(|e| e.enabled && e.visible) {
                            if entity.position.distance_squared(light.position) > radius2 { continue; }
                            entity.position.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                            entity.rotation.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                        }
                        for character in character_system.get_active_characters() {
                            if character.transform.position.distance_squared(light.position) > radius2 { continue; }
                            character.transform.position.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                            character.transform.rotation.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                        }
                        for body in physics.bodies.iter().filter(|body| body.solid) {
                            if body.position.distance_squared(light.position) > radius2 { continue; }
                            body.position.to_array().iter().for_each(|n| n.to_bits().hash(&mut hash));
                        }
                    }
                    hash.finish()
                }, |renderer, light_frustum| {
                    let cache = renderer.chunk_cache.borrow();
                    for mesh in cache.values() {
                        let (low, high) = mesh.chunk_coord.aabb_min_max();
                        if !light_frustum.intersects_aabb(low, high)
                            || mesh.shadow_vao == 0 || mesh.shadow_vertex_count <= 0 { continue; }
                        let model = Mat4::from_translation(mesh.chunk_coord.world_origin());
                        gl::UniformMatrix4fv(renderer.shadow_model_location, 1, gl::FALSE,
                            model.to_cols_array().as_ptr());
                        gl::BindVertexArray(mesh.shadow_vao);
                        gl::DrawArrays(gl::TRIANGLES, 0, mesh.shadow_vertex_count);
                    }
                    if editor.mode == EditorMode::Editor {
                        for actor in world.actors.values().filter(|actor| actor.visible && actor.enabled) {
                            let (low, high) = actor_preview_bounds(editor, actor);
                            if !light_frustum.intersects_aabb(low, high) { continue; }
                            if actor.package.as_deref().is_some_and(|name|
                                crate::character::package::get_package(name).is_some()) {
                                let model = authored_actor_transform(editor, actor);
                                gl::UniformMatrix4fv(renderer.shadow_model_location, 1,
                                    gl::FALSE, model.to_cols_array().as_ptr());
                                renderer.draw_authored_character_cached(actor, false);
                            } else if let Some((mesh, model)) = authored_actor_mesh(editor, actor) {
                                gl::UniformMatrix4fv(renderer.shadow_model_location, 1,
                                    gl::FALSE, model.to_cols_array().as_ptr());
                                draw_authored_actor_mesh(renderer, &mesh, false);
                            }
                        }
                    } else {
                        let (first, count) = renderer.get_block_mask_range(63);
                        gl::BindVertexArray(renderer.block_vao);
                        for body in physics.bodies.iter().filter(|body| body.solid) {
                            let radius = body.size.abs() * 0.5 + Vec3::splat(1.0);
                            if !light_frustum.intersects_aabb(body.position - radius, body.position + radius) { continue; }
                            let model = super::physics_body_model_matrix(body);
                            gl::UniformMatrix4fv(renderer.shadow_model_location, 1,
                                gl::FALSE, model.to_cols_array().as_ptr());
                            gl::DrawArrays(gl::TRIANGLES, first, count);
                        }
                        for entity in entity_manager.iter() {
                            if character_system.get_character_for_entity(entity.id).is_some()
                                || !entity.visible || !entity.enabled { continue; }
                            if !super::game_render::shadow_caster_in_light_frustum(
                                crate::engine::entity_visual::static_entity_world_bounds(entity),
                                light_frustum) { continue; }
                            let rotation = glam::Quat::from_euler(glam::EulerRot::XYZ,
                                entity.rotation.x, entity.rotation.y, entity.rotation.z);
                            let model = Mat4::from_scale_rotation_translation(
                                entity.scale, rotation, entity.position);
                            gl::UniformMatrix4fv(renderer.shadow_model_location, 1,
                                gl::FALSE, model.to_cols_array().as_ptr());
                            renderer.draw_runtime_entity_mesh(entity, false);
                        }
                        let meshes = character_shadow_meshes.get_or_insert_with(|| {
                            character_system.get_active_characters().map(|character| {
                                let model = Mat4::from_scale_rotation_translation(
                                    character.transform.scale, character.transform.rotation,
                                    character.transform.position);
                                (model, crate::character::character::generate_character_mesh_batches(character))
                            }).collect::<Vec<_>>()
                        });
                        for (model, mesh) in meshes.iter() {
                            gl::UniformMatrix4fv(renderer.shadow_model_location, 1,
                                gl::FALSE, model.to_cols_array().as_ptr());
                            renderer.draw_character_batches(mesh, false);
                        }
                    }
                });
            }
            if let Some(p) = profile.as_mut() {
                p.mark(2);
            }
            let mirage_ready = world.sky.horizon_mirage_enabled
                && world.sky.effective_mirage_strength() > 0.0
                && self
                    .mirage
                    .borrow_mut()
                    .target(self.width as i32, self.height as i32)
                    .is_some();
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);

            gl::Viewport(vp_x, vp_y, vp_w, vp_h);

            gl::Enable(gl::SCISSOR_TEST);

            gl::Scissor(vp_x, vp_y, vp_w, vp_h);

            gl::ClearColor(0.0, 0.0, 0.0, 1.0);

            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);

            let underwater = super::terrain::underwater_state_with_material(
                world,
                camera_pos,
                &self.materials.borrow(),
            );
            self.water_volume_mask.borrow_mut().sync(
                world,
                camera_pos,
                self.water_started.elapsed().as_secs_f32(),
                &self.materials.borrow(),
            );
            self.sky_renderer
                .render(self, world, camera_pos, view_projection, underwater);

            gl::UseProgram(self.grid_program);
            self.bind_water_contact_uniforms(self.grid_program);

            gl::Uniform1i(
                self.grid_debug_view_location,
                render_settings.debug_view as i32,
            );
            self.bind_gi_uniforms(render_settings);
            self.bind_underwater_uniforms(
                underwater,
                camera_pos,
                world.terrain.config.ocean_optics_mode,
            );
            self.water_volume_mask.borrow().bind_grid();
            self.bind_atmosphere_uniforms(world, camera_pos);

            let vp_location = self.grid_view_projection_location;
            let model_location = self.grid_model_location;
            let base_color_location = self.grid_base_color_location;
            let alpha_location = self.grid_alpha_location;
            let use_tex_location = self.grid_use_texture_location;

            gl::UniformMatrix4fv(
                vp_location,
                1,
                gl::FALSE,
                view_projection.to_cols_array().as_ptr(),
            );

            gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

            gl::Uniform1f(alpha_location, 1.0);

            gl::Uniform1f(
                self.grid_ambient_intensity_location,
                world.lighting.ambient_intensity,
            );

            gl::Uniform1i(
                self.grid_global_light_enabled_location,
                if world.lighting.global_light_enabled {
                    1
                } else {
                    0
                },
            );

            gl::Uniform3f(
                self.grid_global_light_direction_location,
                world.lighting.global_light_direction.x,
                world.lighting.global_light_direction.y,
                world.lighting.global_light_direction.z,
            );

            gl::Uniform3f(
                self.grid_global_light_color_location,
                world.lighting.global_light_color.x,
                world.lighting.global_light_color.y,
                world.lighting.global_light_color.z,
            );

            gl::Uniform1f(
                self.grid_global_light_intensity_location,
                world.lighting.global_light_intensity,
            );

            gl::UniformMatrix4fv(
                self.grid_light_space_matrix_location,
                1,
                gl::FALSE,
                light_space_matrix.to_cols_array().as_ptr(),
            );

            gl::Uniform1i(
                self.grid_shadows_enabled_location,
                if render_shadows { 1 } else { 0 },
            );

            gl::ActiveTexture(gl::TEXTURE0);

            gl::BindTexture(gl::TEXTURE_2D_ARRAY, self.shadow_depth_tex);

            gl::Uniform1i(self.grid_shadow_map_location, 0);

            gl::ActiveTexture(gl::TEXTURE1);

            gl::BindTexture(gl::TEXTURE_2D, self.fallback_tex);

            self.point_light_stream
                .upload(&point_lights, self.grid_point_light_count_location);

            let cam_right = Vec3::new(view.col(0).x, view.col(1).x, view.col(2).x);

            let cam_up = Vec3::new(view.col(0).y, view.col(1).y, view.col(2).y);

            if editor.mode == EditorMode::Play {
                if let Some(p) = profile.as_mut() {
                    p.mark(3);
                }
            }
            if editor.mode == EditorMode::Editor {
                for marker_id in world.iter_editor_marker_ids() {
                    let (cell, center) = if let Some(source) = world.source(marker_id) {
                        (&source.cell, source_preview_position(editor, source))
                    } else {
                        let Some(coord) = world.resolve_cell_id(marker_id) else {
                            continue;
                        };
                        let Some(cell) = world.get_effective_cell_by_id(marker_id) else {
                            continue;
                        };
                        (
                            cell,
                            Vec3::new(
                                coord.x as f32 + 0.5,
                                coord.y as f32 + 0.5,
                                coord.z as f32 + 0.5,
                            ),
                        )
                    };

                    gl::BindVertexArray(self.billboard_vao);

                    gl::Uniform1i(use_tex_location, 1);

                    let tex_name = match cell.cell_type {
                        CellType::Light => "lightbulb",
                        CellType::AudioEmitter => "speaker",
                        CellType::ParticleEmitter => "lightbulb",
                        _ => continue,
                    };

                    let tex = self.get_texture(tex_name);

                    gl::ActiveTexture(gl::TEXTURE1);

                    gl::BindTexture(gl::TEXTURE_2D, tex);

                    let scale = 0.35;

                    let model = Mat4::from_cols(
                        cam_right.extend(0.0) * scale,
                        cam_up.extend(0.0) * scale,
                        Vec3::ZERO.extend(0.0),
                        center.extend(1.0),
                    );

                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                    gl::DrawArrays(gl::TRIANGLES, 0, 6);
                }

                if let Some(p) = profile.as_mut() {
                    p.mark(3);
                }
                gl::Uniform1i(use_tex_location, 0);

                for actor in world.actors.values() {
                    if !actor.visible || !actor.enabled {
                        continue;
                    }

                    let (bounds_min, bounds_max) = actor_preview_bounds(editor, actor);
                    if !camera_frustum.intersects_aabb(bounds_min, bounds_max) {
                        continue;
                    }

                    if actor
                        .package
                        .as_deref()
                        .is_some_and(|name| crate::character::package::get_package(name).is_some())
                    {
                        let model = authored_actor_transform(editor, actor);
                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );
                        gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);
                        self.draw_authored_character_cached(actor, true);
                        continue;
                    }
                    if !prepared_actor_meshes.contains_key(&actor.id) {
                        let Some(prepared) = authored_actor_mesh(editor, actor) else {
                            continue;
                        };
                        prepared_actor_meshes.insert(actor.id, prepared);
                    }
                    let (mesh, model) = &prepared_actor_meshes[&actor.id];

                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                    draw_authored_actor_mesh(self, mesh, true);
                }
            }

            if let Some(p) = profile.as_mut() {
                p.mark(4);
            }
            gl::Uniform1i(use_tex_location, 1);

            let cache = self.chunk_cache.borrow();

            let mut last_cell_texture: Option<u32> = None;
            let cell_materials = self.material_snapshot();
            for chunk_mesh in cache.values() {
                if !super::distance::within_chunk_radius(
                    chunk_mesh.chunk_coord,
                    chunk_focus,
                    chunk_radius,
                ) {
                    continue;
                }
                if chunk_mesh.main_vao == 0 || chunk_mesh.main_texture_batches.is_empty() {
                    continue;
                }

                let (aabb_min, aabb_max) = chunk_mesh.chunk_coord.aabb_min_max();

                if !camera_frustum.intersects_aabb(aabb_min, aabb_max) {
                    continue;
                }

                let origin = chunk_mesh.chunk_coord.world_origin();

                let model = Mat4::from_translation(origin);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::BindVertexArray(chunk_mesh.main_vao);

                for batch in &chunk_mesh.main_texture_batches {
                    self.bind_cell_pbr(&batch.texture_id, &cell_materials);
                    let tex = self.get_texture(&batch.texture_id);

                    gl::ActiveTexture(gl::TEXTURE1);
                    if super::resources::texture_binding_changed(&mut last_cell_texture, tex) {
                        gl::BindTexture(gl::TEXTURE_2D, tex);
                    }

                    gl::DrawArrays(gl::TRIANGLES, batch.start_vertex, batch.vertex_count);
                }
            }
            self.bind_cell_pbr("", &cell_materials);

            if let Some(p) = profile.as_mut() {
                p.mark(5);
            }
            terrain_stats = self.draw_terrain(
                world,
                &camera_frustum,
                chunk_focus,
                chunk_radius,
                profile.is_some(),
            );
            if let Some(p) = profile.as_mut() {
                p.mark(6);
            }

            gl::Uniform1i(use_tex_location, 0);

            if editor.mode == EditorMode::Play {
                let (first_vertex, count) = self.get_block_mask_range(63);

                gl::BindVertexArray(self.block_vao);

                for body in &physics.bodies {
                    if !body.visible {
                        continue;
                    }

                    gl::Uniform1i(use_tex_location, 1);
                    gl::ActiveTexture(gl::TEXTURE1);
                    gl::BindTexture(gl::TEXTURE_2D, self.get_texture(&body.texture));

                    gl::Uniform3f(
                        base_color_location,
                        body.color_rgb.x,
                        body.color_rgb.y,
                        body.color_rgb.z,
                    );

                    let model = super::physics_body_model_matrix(body);

                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::DrawArrays(gl::TRIANGLES, first_vertex, count);
                }

                gl::Uniform1i(use_tex_location, 0);

                for entity in entity_manager.iter() {
                    if character_system
                        .get_character_for_entity(entity.id)
                        .is_some()
                        || !entity.visible
                        || !entity.enabled
                    {
                        continue;
                    }
                    if let Some((bounds_min, bounds_max)) =
                        crate::engine::entity_visual::static_entity_world_bounds(entity)
                        && !camera_frustum.intersects_aabb(bounds_min, bounds_max)
                    {
                        continue;
                    }

                    let rotation = glam::Quat::from_euler(
                        glam::EulerRot::XYZ,
                        entity.rotation.x,
                        entity.rotation.y,
                        entity.rotation.z,
                    );
                    let model = Mat4::from_scale_rotation_translation(
                        entity.scale,
                        rotation,
                        entity.position,
                    );
                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );
                    gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);
                    self.draw_runtime_entity_mesh(entity, true);
                }

                let active_player_id = character_system.explicit_active_player_id();

                for character in character_system.get_active_characters() {
                    let is_camera_player = active_player_id == Some(character.id);
                    let camera_distance_squared = character
                        .transform
                        .position
                        .distance_squared(gameplay_camera.current_position);

                    if is_camera_player
                        && camera_distance_squared < CHARACTER_CAMERA_HIDE_DISTANCE.powi(2)
                    {
                        continue;
                    }

                    let model = Mat4::from_scale_rotation_translation(
                        character.transform.scale,
                        character.transform.rotation,
                        character.transform.position,
                    );

                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                    if self.draw_gpu_skinned_character(character, true).is_none() {
                        let batches =
                            crate::character::character::generate_character_mesh_batches(character);
                        self.draw_character_batches(&batches, true);
                    }
                }
            }

            if let Some(p) = profile.as_mut() {
                p.mark(7);
            }
            self.contact_foam.borrow_mut().capture(
                (vp_x, vp_y, vp_w, vp_h),
                projection,
                world.terrain.effective().is_some(),
            );
            water_stats = self.draw_terrain_water(
                world,
                camera_pos,
                view_projection,
                underwater.is_some_and(|water| camera_pos.y < water.surface_y),
                &camera_frustum,
                chunk_focus,
                chunk_radius,
            );

            if let Some(p) = profile.as_mut() {
                p.mark(8);
            }
            if mirage_ready {
                let pitch = (-view.inverse().z_axis.y).clamp(-1.0, 1.0).asin();
                self.mirage.borrow_mut().composite(
                    (vp_x, vp_y, vp_w, vp_h),
                    pitch,
                    self.water_started.elapsed().as_secs_f32(),
                    world.sky.effective_mirage_strength(),
                );
                gl::UseProgram(self.grid_program);
                gl::Uniform1i(use_tex_location, 0);
            }
            let project_assets = self.project_assets_dir.borrow();
            let project_root = project_assets.as_deref().and_then(|path| path.parent());
            let _ = super::effekseer::draw(world, view, projection, camera_pos, project_root);
            self.render_particles(&world.particles, view, projection);

            if let Err(error) = self.post_process.borrow_mut().composite(
                (vp_x, vp_y, vp_w, vp_h),
                &self.materials.borrow(),
                self.water_started.elapsed().as_secs_f32(),
                projection.inverse().to_cols_array(),
                |program, channels| {
                    self.draw_post_process_masks_editor(
                        editor,
                        world,
                        entity_manager,
                        character_system,
                        gameplay_camera,
                        actor_system,
                        view_projection,
                        program,
                        channels,
                    );
                },
            ) {
                crate::engine::diagnostics::report("ERROR", "Post Processing", error);
            }
            gl::UseProgram(self.grid_program);

            let plane = editor.plane_drag.as_ref().map_or_else(
                || crate::editor::grid::select_grid_plane(camera_pos, target),
                |drag| drag.plane,
            );

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            let actor_grid_mode = editor.mode == EditorMode::Editor
                && editor.current_tool == crate::editor::EditorTool::Build
                && editor.build_is_actor
                && !editor.actor_build_surface;
            if editor.mode == EditorMode::Editor && (editor.plane_picking || actor_grid_mode) {
                let grid_plane = if actor_grid_mode {
                    GridPlane::Xz
                } else {
                    plane
                };
                let (_normal, anchor_offset) = match grid_plane {
                    GridPlane::Xz => (Vec3::Y, editor.anchor.y as f32),
                    GridPlane::Yz => (Vec3::X, editor.anchor.x as f32),
                    GridPlane::Xy => (Vec3::Z, editor.anchor.z as f32),
                };

                let center = Vec3::new(
                    editor.anchor.x as f32,
                    editor.anchor.y as f32,
                    editor.anchor.z as f32,
                );

                let model = Mat4::from_translation(center);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                self.bind_grid_vao(grid_plane);

                gl::DrawArrays(gl::LINES, 0, self.get_grid_count(grid_plane));
            }

            if editor.mode == EditorMode::Editor {
                let anchor_pos = Vec3::new(
                    editor.anchor.x as f32,
                    editor.anchor.y as f32,
                    editor.anchor.z as f32,
                );

                let accent = editor.theme_settings.accent.color();
                let hover = editor.current_tool == crate::editor::EditorTool::Build
                    && editor.plane_picking
                    && editor.plane_grab_hover;
                let model = Mat4::from_translation(anchor_pos)
                    * Mat4::from_scale(Vec3::splat(if hover { 1.25 } else { 1.0 }));
                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());
                gl::Uniform3f(
                    base_color_location,
                    accent.r() as f32 / 255.0,
                    accent.g() as f32 / 255.0,
                    accent.b() as f32 / 255.0,
                );
                gl::BindVertexArray(self.anchor_vao);
                gl::DrawArrays(gl::LINES, 0, self.anchor_vertex_count);
                gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                if editor.theme_settings.show_navmesh
                    && editor
                        .navigation_window
                        .debug_overlays_are_current(world.navigation_revision())
                    && let Some(grid) = &editor.navigation_window.debug_grid
                {
                    gl::BindVertexArray(self.highlight_vao);
                    gl::Uniform1f(alpha_location, 0.9);
                    let debug_radius =
                        (editor.editor_max_distance_chunks.max(1) as f32) * 16.0 * 16.0;
                    let visible = |coord: WorldCoord| {
                        let dx = coord.x as f32 + 0.5 - camera_pos.x;
                        let dz = coord.z as f32 + 0.5 - camera_pos.z;
                        dx * dx + dz * dz <= debug_radius * debug_radius
                    };
                    let draw_marker = |coord: WorldCoord, color: Vec3, scale: Vec3| {
                        if !visible(coord) {
                            return;
                        }
                        gl::Uniform3f(base_color_location, color.x, color.y, color.z);
                        let center = Vec3::new(
                            coord.x as f32 + 0.5,
                            grid.debug_surface_height(coord) + 0.04,
                            coord.z as f32 + 0.5,
                        );
                        let model = Mat4::from_scale_rotation_translation(
                            scale,
                            glam::Quat::IDENTITY,
                            center - scale * 0.5,
                        );
                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );
                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                    };
                    if editor.navigation_window.show_walkable {
                        for coord in grid.walkable_positions() {
                            draw_marker(
                                coord,
                                Vec3::new(0.15, 0.85, 0.45),
                                Vec3::new(0.72, 0.05, 0.72),
                            );
                        }
                    }
                    if editor.navigation_window.show_blocked {
                        for (coord, _) in grid.blocked_positions() {
                            draw_marker(
                                coord,
                                Vec3::new(1.0, 0.2, 0.2),
                                Vec3::new(0.82, 0.82, 0.82),
                            );
                        }
                    }
                    if editor.navigation_window.show_path {
                        if let Some(path) = &editor.navigation_window.debug_path {
                            for &coord in &path.nodes {
                                draw_marker(
                                    coord,
                                    Vec3::new(1.0, 0.8, 0.05),
                                    Vec3::new(0.9, 0.16, 0.9),
                                );
                            }
                        }
                    }
                    gl::Uniform1f(alpha_location, 1.0);
                }

                if editor.navigation_window.is_open
                    && (editor.theme_settings.show_navmesh
                        || editor.right_panel_tab
                            == crate::editor::editor::RightPanelTab::Navigation)
                {
                    gl::BindVertexArray(self.highlight_vao);
                    gl::Uniform1f(alpha_location, 0.8);
                    for volume in &world.navmesh_volumes {
                        if !volume.enabled {
                            continue;
                        }
                        let Some((min, _)) = volume.bounds() else {
                            continue;
                        };
                        let center = Vec3::from_array(volume.center);
                        let scale = Vec3::from_array(volume.size);
                        let dist = Vec3::new(center.x - camera_pos.x, 0.0, center.z - camera_pos.z);
                        if dist.length()
                            > (editor.editor_max_distance_chunks.max(1) * 16) as f32
                                + scale.length()
                        {
                            continue;
                        }
                        let selected =
                            editor.navigation_window.selected_volume_id == Some(volume.id);
                        let color = if selected {
                            Vec3::new(1.0, 0.8, 0.12)
                        } else {
                            Vec3::new(0.15, 0.85, 0.45)
                        };
                        gl::Uniform3f(base_color_location, color.x, color.y, color.z);
                        let model = Mat4::from_scale_rotation_translation(
                            scale,
                            glam::Quat::IDENTITY,
                            Vec3::from_array(min),
                        );
                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );
                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                    }
                    gl::Uniform1f(alpha_location, 1.0);
                }

                if editor.current_tool == crate::editor::EditorTool::Build && editor.build_is_actor
                {
                    if let Some(preview) = editor.actor_build_preview {
                        let color = if preview.valid {
                            Vec3::new(0.15, 0.85, 0.45)
                        } else {
                            Vec3::new(1.0, 0.2, 0.2)
                        };
                        gl::Uniform3f(base_color_location, color.x, color.y, color.z);
                        gl::Uniform1f(alpha_location, 0.72);
                        let rotation = glam::Quat::from_euler(
                            glam::EulerRot::XYZ,
                            preview.rotation.x,
                            preview.rotation.y,
                            preview.rotation.z,
                        );
                        let model = Mat4::from_translation(preview.position)
                            * Mat4::from_quat(rotation)
                            * Mat4::from_translation(preview.bounds_min)
                            * Mat4::from_scale(
                                (preview.bounds_max - preview.bounds_min).max(Vec3::splat(0.01)),
                            );
                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );
                        gl::BindVertexArray(self.highlight_vao);
                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                        gl::Uniform1f(alpha_location, 1.0);
                    }
                }

                if let Some(hover) = editor.hovered_cell.filter(|_| {
                    editor.current_tool != crate::editor::EditorTool::Build
                        || !editor.build_is_actor
                }) {
                    gl::Uniform3f(base_color_location, 1.0, 0.78, 0.12);
                    let model = Mat4::from_translation(Vec3::new(
                        hover.x as f32,
                        hover.y as f32,
                        hover.z as f32,
                    ));

                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::BindVertexArray(self.highlight_vao);

                    gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                }

                gl::BindVertexArray(self.highlight_vao);

                if let Some(ref grab) = editor.grab_state {
                    let color = if grab.valid {
                        (0.2, 0.6, 1.0)
                    } else {
                        (1.0, 0.2, 0.2)
                    };

                    gl::Uniform3f(base_color_location, color.0, color.1, color.2);

                    for coord in &grab.source_coords {
                        let model = Mat4::from_translation(Vec3::new(
                            (coord.x + grab.delta.x) as f32,
                            (coord.y + grab.delta.y) as f32,
                            (coord.z + grab.delta.z) as f32,
                        ));

                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );

                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                    }
                } else {
                    gl::Uniform3f(base_color_location, 0.2, 0.6, 1.0);

                    for coord in &editor.selected_coords {
                        let model = Mat4::from_translation(Vec3::new(
                            coord.x as f32,
                            coord.y as f32,
                            coord.z as f32,
                        ));

                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );

                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                    }
                }

                if let Some(actor_id) = editor.hovered_actor_id {
                    if !editor.selected_actor_ids().contains(&actor_id) {
                        if let Some(actor) = world.actors.get(&actor_id) {
                            let (min, max) = actor_preview_bounds(editor, actor);
                            let size = (max - min).max(Vec3::splat(0.01));
                            let model = Mat4::from_translation(min) * Mat4::from_scale(size);

                            gl::UniformMatrix4fv(
                                model_location,
                                1,
                                gl::FALSE,
                                model.to_cols_array().as_ptr(),
                            );

                            gl::Uniform3f(base_color_location, 1.0, 0.78, 0.12);

                            gl::BindVertexArray(self.highlight_vao);

                            gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                        }
                    }
                }

                for actor_id in editor.selected_actor_ids() {
                    if let Some(actor) = world.actors.get(&actor_id) {
                        let (min, max) = actor_preview_bounds(editor, actor);
                        let size = (max - min).max(Vec3::splat(0.01));
                        let model = Mat4::from_translation(min) * Mat4::from_scale(size);

                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            model.to_cols_array().as_ptr(),
                        );

                        gl::Uniform3f(base_color_location, 0.2, 0.6, 1.0);

                        gl::BindVertexArray(self.highlight_vao);

                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                    }
                }

                for actor_id in editor.selected_actor_ids() {
                    let Some(actor) = world.actors.get(&actor_id) else {
                        continue;
                    };
                    let Some(mut instance) =
                        crate::engine::collision_geometry::preview_actor_collider(actor)
                    else {
                        continue;
                    };
                    if let Some(preview) = editor
                        .actor_transform_preview
                        .as_ref()
                        .filter(|preview| preview.actor_id == actor.id)
                    {
                        instance.rotation = preview.current_rotation;
                        instance.scale = preview.current_scale;
                    }
                    let edges = crate::engine::collision_geometry::debug_edges(
                        &instance,
                        actor_preview_position(editor, actor),
                    );
                    if edges.is_empty() {
                        continue;
                    }
                    let mut line_vertices = Vec::with_capacity(edges.len() * 24);
                    for (a, b) in edges {
                        super::mesh::add_line(
                            &mut line_vertices,
                            a.to_array(),
                            b.to_array(),
                            [0.15, 0.98, 0.62, 1.0],
                        );
                    }
                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        Mat4::IDENTITY.to_cols_array().as_ptr(),
                    );
                    gl::Uniform3f(base_color_location, 0.15, 0.98, 0.62);
                    gl::Uniform1i(use_tex_location, 0);
                    let (vao, vbo, count) = super::mesh::upload_vertices_3d(&line_vertices);
                    gl::BindVertexArray(vao);
                    gl::DrawArrays(gl::LINES, 0, count);
                    gl::BindVertexArray(0);
                    gl::DeleteVertexArrays(1, &vao);
                    gl::DeleteBuffers(1, &vbo);
                }

                if (editor.sculpt_tool != crate::world::terrain::SculptTool::None
                    || editor.actor_brush_enabled)
                    && !editor.ui_mode
                    && !editor.show_script_workspace
                {
                    if let Some(hit) = &editor.hovered_terrain_hit {
                        gl::Uniform1i(use_tex_location, 0);
                        gl::Uniform1f(alpha_location, 1.0);
                        gl::UniformMatrix4fv(
                            model_location,
                            1,
                            gl::FALSE,
                            Mat4::IDENTITY.to_cols_array().as_ptr(),
                        );

                        let is_paint =
                            editor.sculpt_tool == crate::world::terrain::SculptTool::Paint;
                        let is_actor_brush = editor.actor_brush_enabled;
                        let (outer_radius, inner_radius) = crate::world::sculpt_preview_radii(
                            editor.sculpt_brush_radius,
                            editor.sculpt_strength,
                        );
                        let segments = 32;

                        gl::LineWidth(2.0);

                        let mut ring_cache = self.brush_ring_cache.borrow_mut();
                        let mut draw_ring = |slot: usize, radius: f32, color: Vec3| {
                            if radius <= 0.01 {
                                return;
                            }
                            let revision = world.terrain.render_revision();
                            let key = [
                                hit.point.x.to_bits(),
                                hit.point.y.to_bits(),
                                hit.point.z.to_bits(),
                                radius.to_bits(),
                                color.x.to_bits(),
                                color.y.to_bits(),
                                color.z.to_bits(),
                                revision as u32,
                                (revision >> 32) as u32,
                            ];
                            if ring_cache[slot]
                                .as_ref()
                                .is_none_or(|cached| cached.key != key)
                            {
                                let mut circle_verts = Vec::with_capacity(segments * 20);
                                for i in 0..segments {
                                    let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
                                    let a1 =
                                        ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
                                    let x0 = hit.point.x + radius * a0.cos();
                                    let z0 = hit.point.z + radius * a0.sin();
                                    let y0 = world
                                        .terrain
                                        .surface_height_at(x0, z0)
                                        .unwrap_or(hit.point.y)
                                        + 0.08;
                                    let x1 = hit.point.x + radius * a1.cos();
                                    let z1 = hit.point.z + radius * a1.sin();
                                    let y1 = world
                                        .terrain
                                        .surface_height_at(x1, z1)
                                        .unwrap_or(hit.point.y)
                                        + 0.08;
                                    super::mesh::add_line(
                                        &mut circle_verts,
                                        [x0, y0, z0],
                                        [x1, y1, z1],
                                        [color.x, color.y, color.z, 1.0],
                                    );
                                }
                                let (vao, vbo, count) =
                                    super::mesh::upload_vertices_3d(&circle_verts);
                                if let Some(previous) = ring_cache[slot].take() {
                                    gl::DeleteVertexArrays(1, &previous.vao);
                                    gl::DeleteBuffers(1, &previous.vbo);
                                }
                                ring_cache[slot] = Some(BrushRingGpu {
                                    key,
                                    vao,
                                    vbo,
                                    count,
                                });
                            }
                            let cached = ring_cache[slot].as_ref().unwrap();
                            gl::Uniform3f(base_color_location, color.x, color.y, color.z);
                            gl::BindVertexArray(cached.vao);
                            gl::DrawArrays(gl::LINES, 0, cached.count);
                        };

                        draw_ring(
                            0,
                            outer_radius,
                            if is_paint {
                                Vec3::new(0.25, 0.75, 1.0)
                            } else if is_actor_brush {
                                Vec3::new(0.35, 0.95, 0.45)
                            } else {
                                Vec3::new(1.0, 0.55, 0.0)
                            },
                        );

                        if !is_paint && !is_actor_brush {
                            draw_ring(1, inner_radius, Vec3::new(0.9, 0.9, 0.9));
                        }

                        gl::LineWidth(1.0);
                    }
                }

                for (source_id, color) in [
                    (
                        editor
                            .hovered_source_id
                            .filter(|id| editor.selected_source_id != Some(*id)),
                        Vec3::new(1.0, 0.78, 0.12),
                    ),
                    (editor.selected_source_id, Vec3::new(0.2, 0.6, 1.0)),
                ] {
                    let Some(source_id) = source_id else {
                        continue;
                    };
                    let Some(source) = world.source(source_id) else {
                        continue;
                    };
                    let center = source_preview_position(editor, source);
                    let model = Mat4::from_translation(center - Vec3::splat(0.35))
                        * Mat4::from_scale(Vec3::splat(0.7));
                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );
                    gl::Uniform3f(base_color_location, color.x, color.y, color.z);
                    gl::BindVertexArray(self.highlight_vao);
                    gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                }
            }

            if editor.mode == EditorMode::Editor {
                if let Some(ref grab) = editor.grab_state {
                    gl::Enable(gl::BLEND);
                    gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

                    gl::DepthMask(gl::FALSE);

                    gl::Uniform1f(alpha_location, 0.5);

                    for coord in &grab.source_coords {
                        let Some(cell) = world.get(*coord) else {
                            continue;
                        };

                        let preview_coord = WorldCoord::new(
                            coord.x + grab.delta.x,
                            coord.y + grab.delta.y,
                            coord.z + grab.delta.z,
                        );

                        let is_marker = matches!(
                            cell.cell_type,
                            CellType::Light | CellType::AudioEmitter | CellType::ParticleEmitter
                        );

                        if is_marker {
                            gl::BindVertexArray(self.highlight_vao);

                            let (r, g, b) = if !grab.valid {
                                (1.0, 0.2, 0.2)
                            } else if cell.cell_type == CellType::AudioEmitter {
                                (0.2, 0.8, 1.0)
                            } else {
                                (1.0, 1.0, 0.2)
                            };

                            gl::Uniform3f(base_color_location, r, g, b);

                            let model = Mat4::from_translation(Vec3::new(
                                preview_coord.x as f32,
                                preview_coord.y as f32,
                                preview_coord.z as f32,
                            ));

                            gl::UniformMatrix4fv(
                                model_location,
                                1,
                                gl::FALSE,
                                model.to_cols_array().as_ptr(),
                            );

                            gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                        } else {
                            gl::BindVertexArray(self.block_vao);

                            let (first_vertex, count) = self.get_block_mask_range(63);

                            let color = if grab.valid {
                                cell.color_rgb
                            } else {
                                Vec3::new(1.0, 0.2, 0.2)
                            };

                            gl::Uniform3f(base_color_location, color.x, color.y, color.z);

                            gl::Uniform1i(use_tex_location, 0);

                            let model = Mat4::from_translation(Vec3::new(
                                preview_coord.x as f32,
                                preview_coord.y as f32,
                                preview_coord.z as f32,
                            ));

                            gl::UniformMatrix4fv(
                                model_location,
                                1,
                                gl::FALSE,
                                model.to_cols_array().as_ptr(),
                            );

                            gl::DrawArrays(gl::TRIANGLES, first_vertex, count);
                        }
                    }

                    gl::DepthMask(gl::TRUE);

                    gl::Uniform1f(alpha_location, 1.0);
                }

                gl::UseProgram(self.ghost_program);

                gl::UniformMatrix4fv(
                    self.ghost_view_projection_location,
                    1,
                    gl::FALSE,
                    view_projection.to_cols_array().as_ptr(),
                );

                gl::Uniform1f(self.ghost_alpha_location, 0.4);

                gl::Uniform3f(self.ghost_color_location, 1.0, 1.0, 1.0);

                gl::Enable(gl::BLEND);
                gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

                gl::DepthMask(gl::FALSE);

                let ghost_chunks = self.ghost_cache.borrow();

                for ghost_mesh in ghost_chunks.values() {
                    if !super::distance::within_chunk_radius(
                        ghost_mesh.chunk_coord,
                        chunk_focus,
                        chunk_radius,
                    ) {
                        continue;
                    }
                    let (min, max) = ghost_mesh.chunk_coord.aabb_min_max();
                    if !camera_frustum.intersects_aabb(min, max) {
                        continue;
                    }
                    if ghost_mesh.vao == 0 || ghost_mesh.vertex_count <= 0 {
                        continue;
                    }

                    let model = Mat4::from_translation(ghost_mesh.chunk_coord.world_origin());

                    gl::UniformMatrix4fv(
                        self.ghost_model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::BindVertexArray(ghost_mesh.vao);

                    gl::DrawArrays(gl::TRIANGLES, 0, ghost_mesh.vertex_count);
                }

                drop(ghost_chunks);

                gl::UseProgram(self.grid_program);

                gl::DepthMask(gl::TRUE);

                gl::Uniform1f(alpha_location, 1.0);

                if let (Some(start), Some(end)) = (drag_start, editor.hovered_cell) {
                    let x_min = start.x.min(end.x);
                    let x_max = start.x.max(end.x);

                    let y_min = start.y.min(end.y);
                    let y_max = start.y.max(end.y);

                    let z_min = start.z.min(end.z);
                    let z_max = start.z.max(end.z);

                    gl::Enable(gl::BLEND);
                    gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

                    gl::DepthMask(gl::FALSE);

                    gl::Uniform1f(alpha_location, 0.4);

                    match editor.current_tool {
                        crate::editor::EditorTool::Build => {
                            let is_marker = matches!(
                                editor.build_template.cell_type,
                                CellType::Light
                                    | CellType::AudioEmitter
                                    | CellType::ParticleEmitter
                            );

                            if is_marker {
                                gl::BindVertexArray(self.highlight_vao);

                                let (r, g, b) =
                                    if editor.build_template.cell_type == CellType::AudioEmitter {
                                        (0.2, 0.8, 1.0)
                                    } else {
                                        (1.0, 1.0, 0.2)
                                    };

                                gl::Uniform3f(base_color_location, r, g, b);

                                for x in x_min..=x_max {
                                    for y in y_min..=y_max {
                                        for z in z_min..=z_max {
                                            let model = Mat4::from_translation(Vec3::new(
                                                x as f32, y as f32, z as f32,
                                            ));

                                            gl::UniformMatrix4fv(
                                                model_location,
                                                1,
                                                gl::FALSE,
                                                model.to_cols_array().as_ptr(),
                                            );

                                            gl::DrawArrays(
                                                gl::LINES,
                                                0,
                                                self.highlight_vertex_count,
                                            );
                                        }
                                    }
                                }
                            } else {
                                gl::BindVertexArray(self.block_vao);

                                let (first_vertex, count) = self.get_block_mask_range(63);

                                let color = editor.build_template.color_rgb;

                                gl::Uniform3f(base_color_location, color.x, color.y, color.z);

                                gl::Uniform1i(use_tex_location, 0);

                                for x in x_min..=x_max {
                                    for y in y_min..=y_max {
                                        for z in z_min..=z_max {
                                            let model = Mat4::from_translation(Vec3::new(
                                                x as f32, y as f32, z as f32,
                                            ));

                                            gl::UniformMatrix4fv(
                                                model_location,
                                                1,
                                                gl::FALSE,
                                                model.to_cols_array().as_ptr(),
                                            );

                                            gl::DrawArrays(gl::TRIANGLES, first_vertex, count);
                                        }
                                    }
                                }
                            }
                        }

                        crate::editor::EditorTool::Erase => {
                            gl::BindVertexArray(self.highlight_vao);

                            gl::Uniform3f(base_color_location, 1.0, 0.2, 0.2);

                            for x in x_min..=x_max {
                                for y in y_min..=y_max {
                                    for z in z_min..=z_max {
                                        let model = Mat4::from_translation(Vec3::new(
                                            x as f32, y as f32, z as f32,
                                        ));

                                        gl::UniformMatrix4fv(
                                            model_location,
                                            1,
                                            gl::FALSE,
                                            model.to_cols_array().as_ptr(),
                                        );

                                        gl::DrawArrays(gl::LINES, 0, self.highlight_vertex_count);
                                    }
                                }
                            }
                        }

                        _ => {}
                    }

                    gl::DepthMask(gl::TRUE);

                    gl::Uniform1f(alpha_location, 1.0);
                }
            }

            gl::Uniform1i(use_tex_location, 0);

            gl::Uniform1f(alpha_location, 1.0);

            gl::DepthMask(gl::TRUE);

            gl::Enable(gl::DEPTH_TEST);

            gl::Enable(gl::CULL_FACE);

            gl::BindVertexArray(0);
        }
        if let Some(p) = profile.as_mut() {
            let mut packages = std::collections::BTreeMap::new();
            let mut count = 0;
            for actor in world.actors.values().filter(|a| a.visible && a.enabled) {
                count += 1;
                *packages
                    .entry(actor.package.clone().unwrap_or_else(|| "<none>".to_owned()))
                    .or_insert(0) += 1;
            }
            p.finish(super::editor_profile::SceneCounters {
                actors: count,
                by_package: packages,
                terrain: terrain_stats,
                water: water_stats,
            });
        }
    }

    fn bind_grid_vao(&self, plane: GridPlane) {
        unsafe {
            match plane {
                GridPlane::Xz => gl::BindVertexArray(self.grid_vao_xz),
                GridPlane::Yz => gl::BindVertexArray(self.grid_vao_yz),
                GridPlane::Xy => gl::BindVertexArray(self.grid_vao_xy),
            }
        }
    }

    fn get_grid_count(&self, plane: GridPlane) -> i32 {
        match plane {
            GridPlane::Xz => self.grid_count_xz,
            GridPlane::Yz => self.grid_count_yz,
            GridPlane::Xy => self.grid_count_xy,
        }
    }
}

#[cfg(test)]
mod camera_ownership_tests {
    use super::*;

    #[test]
    fn actor_preview_position_tracks_absolute_surface_target() {
        let mut editor = Editor::new();
        let actor = crate::world::Actor::new(77, "Preview", Vec3::new(2.5, 1.25, -4.5));

        editor.actor_grab_state = Some(crate::editor::ActorGrabState {
            actor_id: 77,
            start_position: actor.position,
            current_position: Vec3::new(5.75, 3.0, -1.125),
        });

        assert_eq!(
            actor_preview_position(&editor, &actor),
            Vec3::new(5.75, 3.0, -1.125),
        );
    }

    #[test]
    fn source_drag_preview_preserves_fractional_surface_target() {
        let mut editor = Editor::new();
        let source =
            crate::world::AuthoredSource::new(91, CellType::Light, Vec3::new(1.0, 11.2, 0.0));
        let target = Vec3::new(2.5, 1.125, 55.75);
        editor.source_grab_state = Some(crate::editor::editor::SourceGrabState {
            source_id: 91,
            start_position: source.position,
            current_position: target,
        });

        assert_eq!(source_preview_position(&editor, &source), target);
    }

    #[test]
    fn unassigned_actor_cannot_select_stale_gameplay_camera() {
        let mut editor = Editor::new();
        editor.mode = EditorMode::Play;
        editor.camera.target = Vec3::new(10.0, 20.0, 30.0);
        let gameplay = crate::renderer::camera::GameplayCamera::new();
        let mut characters = crate::character::CharacterSystem::new();
        characters.spawn_character(Vec3::ZERO, Some("Rogue"));
        let editor_frame = (editor.camera.get_position(), editor.camera.target);
        let (position, _, target) = editor_camera_frame(&editor, &characters, &gameplay);
        assert_eq!((position, target), editor_frame);
        let mut world = World::new();
        world.set_cell(WorldCoord::new(5, 0, 5), CellType::SpawnPoint);
        characters.spawn_player(&world, None).unwrap();
        let (position, _, target) = editor_camera_frame(&editor, &characters, &gameplay);
        assert_eq!(
            (position, target),
            (gameplay.current_position, gameplay.current_target)
        );
        characters.destroy_active_player();
        assert!(characters.has_characters());
        let (position, _, target) = editor_camera_frame(&editor, &characters, &gameplay);
        assert_eq!((position, target), editor_frame);
    }
}

impl Renderer {
    fn draw_post_process_masks_editor(
        &self,
        editor: &Editor,
        world: &World,
        entity_manager: &EntityManager,
        characters: &crate::character::CharacterSystem,
        camera: &crate::renderer::camera::GameplayCamera,
        actor_system: &crate::actor::ActorSystem,
        view_projection: Mat4,
        program: u32,
        channels: &std::collections::BTreeMap<
            crate::project::post_process_pipeline::PostProcessObject,
            u8,
        >,
    ) {
        use crate::project::post_process_pipeline::PostProcessObjectKind as Kind;
        unsafe {
            let uniforms = super::post_process::MaskBindings::new(program, view_projection);
            let (first, count) = self.get_block_mask_range(63);
            for (&object, &channel) in channels {
                if channel == 0 {
                    continue;
                }
                match object.kind {
                    Kind::Cell => {
                        let Some(cell) = world.get_effective_cell_by_id(object.id) else {
                            continue;
                        };
                        if cell.cell_type != CellType::Block || !cell.visible {
                            continue;
                        }
                        let Some(pos) = world.resolve_cell_id(object.id) else {
                            continue;
                        };
                        uniforms.set(
                            Mat4::from_translation(Vec3::new(
                                pos.x as f32,
                                pos.y as f32,
                                pos.z as f32,
                            )),
                            channel,
                        );
                        gl::BindVertexArray(self.block_vao);
                        gl::DrawArrays(gl::TRIANGLES, first, count);
                    }
                    Kind::Actor if editor.mode == EditorMode::Editor => {
                        let Some(actor) = world.actors.get(&object.id) else {
                            continue;
                        };
                        if !actor.visible || !actor.enabled {
                            continue;
                        }
                        let model = authored_actor_transform(editor, actor);
                        uniforms.set(model, channel);
                        if actor.package.as_deref().is_some_and(|package| {
                            crate::character::package::get_package(package).is_some()
                        }) {
                            self.draw_authored_character_cached(actor, false);
                        } else if let Some((mesh, _)) = authored_actor_mesh(editor, actor) {
                            match &mesh {
                                AuthoredActorMesh::Entity(entity) => {
                                    self.draw_runtime_entity_mask(entity)
                                }
                                AuthoredActorMesh::Character(batches) => {
                                    self.draw_character_batches(batches, false);
                                }
                            }
                        }
                    }
                    Kind::Actor | Kind::Character => {
                        let actor_entity = if object.kind == Kind::Actor {
                            actor_system.get_entity_for_actor(object.id)
                        } else {
                            None
                        };
                        let character = if let Some(entity) = actor_entity {
                            characters.get_character_for_entity(entity)
                        } else if object.kind == Kind::Character {
                            characters
                                .get_active_characters()
                                .find(|c| c.id == object.id)
                        } else {
                            None
                        };
                        if let Some(character) = character {
                            if characters.explicit_active_player_id() == Some(character.id)
                                && character
                                    .transform
                                    .position
                                    .distance_squared(camera.current_position)
                                    < 4.0
                            {
                                continue;
                            }
                            uniforms.set(
                                Mat4::from_scale_rotation_translation(
                                    character.transform.scale,
                                    character.transform.rotation,
                                    character.transform.position,
                                ),
                                channel,
                            );
                            let batches =
                                crate::character::character::generate_character_mesh_batches(
                                    character,
                                );
                            self.draw_character_batches(&batches, false);
                        } else if let Some(entity) = actor_entity
                            .and_then(|id| entity_manager.get_entity(id))
                            .filter(|entity| {
                                entity.enabled
                                    && entity.visible
                                    && characters.get_character_for_entity(entity.id).is_none()
                            })
                        {
                            let rotation = glam::Quat::from_euler(
                                glam::EulerRot::XYZ,
                                entity.rotation.x,
                                entity.rotation.y,
                                entity.rotation.z,
                            );
                            uniforms.set(
                                Mat4::from_scale_rotation_translation(
                                    entity.scale,
                                    rotation,
                                    entity.position,
                                ),
                                channel,
                            );
                            self.draw_runtime_entity_mask(entity);
                        }
                    }
                }
            }
            gl::BindVertexArray(0);
        }
    }
}
