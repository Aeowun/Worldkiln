// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::engine::entity::EntityManager;
use crate::engine::physics::PhysicsWorld;
use crate::world::World;

use super::Renderer;
use super::SHADOW_RES;
use super::chunk::CameraFrustum;
use super::profile::DrawStats;
use super::settings::ShadowMethod;

pub(super) fn shadow_caster_in_light_frustum(
    bounds: Option<(Vec3, Vec3)>,
    shadow_frustum: &CameraFrustum,
) -> bool {
    bounds.is_none_or(|(min, max)| shadow_frustum.intersects_aabb(min, max))
}

#[cfg(test)]
mod static_entity_submission_regression {
    use super::super::character_render::MeshDrawStats;
    use super::*;

    #[test]
    fn material_batches_are_counted_as_actual_gl_submissions() {
        for count in [1_usize, 2, 3, 4] {
            let batches = vec![3_i32; count];
            let actual = MeshDrawStats::from_batch_vertex_counts(&batches);
            assert_eq!(actual.draw_calls, count as u64);
            assert_eq!(actual.vertices, (count * 3) as i32);
        }
        assert_eq!(MeshDrawStats::from_batch_vertex_counts(&[]).draw_calls, 0);
    }

    #[test]
    fn light_frustum_keeps_casters_outside_the_game_camera() {
        let shadow = CameraFrustum::from_view_projection(Mat4::orthographic_rh_gl(
            -20.0, 20.0, -20.0, 20.0, -30.0, 30.0,
        ));
        let camera = CameraFrustum::from_view_projection(
            Mat4::perspective_rh_gl(60_f32.to_radians(), 1.0, 0.1, 100.0)
                * Mat4::look_at_rh(Vec3::new(0.0, 2.0, 0.0), Vec3::new(0.0, 2.0, -1.0), Vec3::Y),
        );
        let off_camera = (Vec3::new(8.0, 0.0, 4.0), Vec3::new(10.0, 5.0, 6.0));
        assert!(!camera.intersects_aabb(off_camera.0, off_camera.1));
        assert!(
            shadow_caster_in_light_frustum(Some(off_camera), &shadow),
            "off-camera tree inside the light frustum must cast shadows"
        );
        assert!(
            !shadow_caster_in_light_frustum(
                Some((Vec3::new(80.0, 0.0, 80.0), Vec3::new(85.0, 10.0, 85.0))),
                &shadow
            ),
            "distant tree must not issue shadow draws"
        );
        assert!(
            shadow_caster_in_light_frustum(None, &shadow),
            "missing bounds must fail open, not delete potentially visible shadows"
        );
    }
}

impl Renderer {
    pub fn render_game(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_manager: &EntityManager,
        character_system: &crate::character::CharacterSystem,
        gameplay_camera: &crate::renderer::camera::GameplayCamera,
        actor_system: &crate::actor::ActorSystem,
    ) {
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
        let camera_pos = gameplay_camera.current_position;
        if world.terrain.config.water_realism == crate::world::terrain::WaterRealismMode::Basic {
            let mut wakes = self.water_wakes.borrow_mut();
            wakes.trails.clear();
            wakes.contacts.clear();
            wakes.previous.clear();
        } else {
            self.refresh_water_contacts(world, camera_pos, entity_manager, character_system);
        }
        let chunk_focus = gameplay_camera.current_target;
        let chunk_radius = world.render_distance.radius(true);
        let view = gameplay_camera.get_view_matrix();

        let aspect_ratio = self.width / self.height.max(1.0);

        let projection = glam::camera::rh::proj::opengl::perspective(
            60.0_f32.to_radians(),
            aspect_ratio,
            0.1,
            1000.0,
        );

        let view_projection = projection * view;

        const CHARACTER_CAMERA_HIDE_DISTANCE: f32 = 2.0;

        let light_space_matrix =
            self.calculate_light_space_matrix_with_range(world.lighting.global_light_direction, camera_pos, 3.0);
        let render_settings = self.active_lighting_settings();
        let render_shadows =
            world.lighting.shadows_enabled && render_settings.shadow_method != ShadowMethod::Off;
        let shadow_frustum = CameraFrustum::from_view_projection(light_space_matrix);
        let light_camera_frustum = CameraFrustum::from_view_projection(view_projection);
        let mut point_lights = self.collect_point_lights(world);
        point_lights.retain(|light| {
            let radius = Vec3::splat(light.range.max(0.0));
            light_camera_frustum.intersects_aabb(light.position - radius, light.position + radius)
        });
        self.prepare_point_shadows(world, camera_pos, render_shadows, &mut point_lights);
        self.update_gi_cache(world, physics, camera_pos, &point_lights, render_settings);
        let mut draw_stats = DrawStats::default();

        unsafe {
            gl::Enable(gl::DEPTH_TEST);
            gl::DepthMask(gl::TRUE);
            gl::DepthFunc(gl::LESS);

            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
            gl::FrontFace(gl::CCW);

            gl::Disable(gl::BLEND);

            self.update_chunk_cache(world, super::RenderMode::Game);
            if let Some(profiler) = self.profiler.borrow_mut().as_mut() {
                profiler.begin();
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
                        draw_stats.shadow_draws += 1;
                        draw_stats.shadow_triangles += chunk_mesh.shadow_vertex_count as u64 / 3;
                    }
                }

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
                    draw_stats.shadow_draws += 1;
                    draw_stats.shadow_triangles += count as u64 / 3;
                }

                for entity in entity_manager.iter() {
                    if !should_render_generic_entity(entity.id, entity_manager, character_system)
                        || !entity.visible
                        || !entity.enabled
                    {
                        continue;
                    }

                    if !shadow_caster_in_light_frustum(
                        crate::engine::entity_visual::static_entity_world_bounds(entity),
                        &shadow_frustum,
                    ) {
                        continue;
                    }

                    let model = entity_model_matrix(entity.position, entity.rotation, entity.scale);

                    gl::UniformMatrix4fv(
                        self.shadow_model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    let submitted = self.draw_runtime_entity_mesh_stats(entity, false);
                    if submitted.vertices <= 0 {
                        continue;
                    }
                    draw_stats.shadow_draws += submitted.draw_calls;
                    draw_stats.shadow_triangles += submitted.vertices as u64 / 3;
                }

                for character in character_system.get_active_characters() {
                    let center = character.transform.position;
                    let extent = glam::Vec3::splat(3.0)
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
                    let submitted = self.draw_character_batches_stats(&batches, false);
                    draw_stats.shadow_draws += submitted.draw_calls;
                    draw_stats.shadow_triangles += submitted.vertices.max(0) as u64 / 3;
                }
            }
            if render_shadows {
                let mut character_shadow_meshes = None;
                self.render_point_shadow_faces(world, &mut point_lights, |light| {
                    use std::hash::{Hash, Hasher};
                    let mut hash = std::collections::hash_map::DefaultHasher::new();
                    let radius2 = light.range * light.range;
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
                    hash.finish()
                }, |renderer, light_frustum| {
                    let cache = renderer.chunk_cache.borrow();
                    for mesh in cache.values() {
                        let (low, high) = mesh.chunk_coord.aabb_min_max();
                        if !light_frustum.intersects_aabb(low, high)
                            || mesh.shadow_vao == 0 || mesh.shadow_vertex_count <= 0 {
                            continue;
                        }
                        let model = Mat4::from_translation(mesh.chunk_coord.world_origin());
                        gl::UniformMatrix4fv(renderer.shadow_model_location, 1, gl::FALSE,
                            model.to_cols_array().as_ptr());
                        gl::BindVertexArray(mesh.shadow_vao);
                        gl::DrawArrays(gl::TRIANGLES, 0, mesh.shadow_vertex_count);
                        draw_stats.shadow_draws += 1;
                        draw_stats.shadow_triangles += mesh.shadow_vertex_count as u64 / 3;
                    }
                    let (first, count) = renderer.get_block_mask_range(63);
                    gl::BindVertexArray(renderer.block_vao);
                    for body in physics.bodies.iter().filter(|body| body.solid) {
                        let radius = body.size.abs() * 0.5 + Vec3::splat(1.0);
                        if !light_frustum.intersects_aabb(body.position - radius, body.position + radius) { continue; }
                        let model = super::physics_body_model_matrix(body);
                        gl::UniformMatrix4fv(renderer.shadow_model_location, 1, gl::FALSE,
                            model.to_cols_array().as_ptr());
                        gl::DrawArrays(gl::TRIANGLES, first, count);
                        draw_stats.shadow_draws += 1;
                        draw_stats.shadow_triangles += count as u64 / 3;
                    }
                    for entity in entity_manager.iter() {
                        if !should_render_generic_entity(entity.id, entity_manager, character_system)
                            || !entity.visible || !entity.enabled { continue; }
                        if !shadow_caster_in_light_frustum(
                            crate::engine::entity_visual::static_entity_world_bounds(entity),
                            light_frustum) { continue; }
                        let model = entity_model_matrix(entity.position, entity.rotation, entity.scale);
                        gl::UniformMatrix4fv(renderer.shadow_model_location, 1, gl::FALSE,
                            model.to_cols_array().as_ptr());
                        let submitted = renderer.draw_runtime_entity_mesh_stats(entity, false);
                        draw_stats.shadow_draws += submitted.draw_calls;
                        draw_stats.shadow_triangles += submitted.vertices.max(0) as u64 / 3;
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
                        let submitted = renderer.draw_character_batches_stats(mesh, false);
                        draw_stats.shadow_draws += submitted.draw_calls;
                        draw_stats.shadow_triangles += submitted.vertices.max(0) as u64 / 3;
                    }
                });
            }
            if let Some(profiler) = self.profiler.borrow().as_ref() {
                profiler.mark(1);
            }

            let mirage_ready = world.sky.horizon_mirage_enabled
                && world.sky.effective_mirage_strength() > 0.0
                && self
                    .mirage
                    .borrow_mut()
                    .target(self.width as i32, self.height as i32)
                    .is_some();
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);

            gl::Viewport(0, 0, self.width as i32, self.height as i32);

            gl::Disable(gl::SCISSOR_TEST);

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

            let camera_frustum = CameraFrustum::from_view_projection(view_projection);

            gl::Uniform1i(use_tex_location, 1);
            if let Some(profiler) = self.profiler.borrow().as_ref() {
                profiler.mark(2);
            }

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
                    draw_stats.static_draws += 1;
                    draw_stats.static_triangles += batch.vertex_count as u64 / 3;
                }
            }

            self.bind_cell_pbr("", &cell_materials);
            if let Some(profiler) = self.profiler.borrow().as_ref() {
                profiler.mark(3);
            }
            let terrain_stats =
                self.draw_terrain(world, &camera_frustum, chunk_focus, chunk_radius, false);
            draw_stats.terrain_draws += terrain_stats.draws;
            draw_stats.terrain_triangles += terrain_stats.triangles;
            draw_stats.terrain_chunks_drawn += terrain_stats.chunks_drawn;
            draw_stats.terrain_chunks_culled += terrain_stats.chunks_culled;

            gl::Uniform1i(use_tex_location, 0);
            if let Some(profiler) = self.profiler.borrow().as_ref() {
                profiler.mark(4);
            }

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

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::DrawArrays(gl::TRIANGLES, first_vertex, count);
                draw_stats.dynamic_draws += 1;
                draw_stats.dynamic_triangles += count as u64 / 3;
            }

            gl::Uniform1i(use_tex_location, 0);

            for entity in entity_manager.iter() {
                if !should_render_generic_entity(entity.id, entity_manager, character_system) {
                    continue;
                }

                if !entity.visible || !entity.enabled {
                    continue;
                }

                if let Some((bounds_min, bounds_max)) =
                    crate::engine::entity_visual::static_entity_world_bounds(entity)
                    && !camera_frustum.intersects_aabb(bounds_min, bounds_max)
                {
                    continue;
                }

                let model = entity_model_matrix(entity.position, entity.rotation, entity.scale);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                let submitted = self.draw_runtime_entity_mesh_stats(entity, true);
                if submitted.vertices <= 0 {
                    continue;
                }
                draw_stats.dynamic_draws += submitted.draw_calls;
                draw_stats.dynamic_triangles += submitted.vertices as u64 / 3;
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

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                let submitted = self
                    .draw_gpu_skinned_character(character, true)
                    .unwrap_or_else(|| {
                        let batches =
                            crate::character::character::generate_character_mesh_batches(character);
                        self.draw_character_batches_stats(&batches, true)
                    });
                draw_stats.dynamic_draws += submitted.draw_calls;
                draw_stats.dynamic_triangles += (submitted.vertices / 3) as u64;
            }

            self.contact_foam.borrow_mut().capture(
                (0, 0, self.width as i32, self.height as i32),
                projection,
                world.terrain.effective().is_some(),
            );
            let water_stats = self.draw_terrain_water(
                world,
                camera_pos,
                view_projection,
                underwater.is_some_and(|water| camera_pos.y < water.surface_y),
                &camera_frustum,
                chunk_focus,
                chunk_radius,
            );
            draw_stats.water_draws += water_stats.draws;
            draw_stats.water_triangles += water_stats.triangles;

            let project_assets = self.project_assets_dir.borrow();
            let project_root = project_assets.as_deref().and_then(|path| path.parent());
            let _ = super::effekseer::draw(world, view, projection, camera_pos, project_root);
            self.render_particles(&world.particles, view, projection);

            if mirage_ready {
                let pitch = (-view.inverse().z_axis.y).clamp(-1.0, 1.0).asin();
                self.mirage.borrow_mut().composite(
                    (0, 0, self.width as i32, self.height as i32),
                    pitch,
                    self.water_started.elapsed().as_secs_f32(),
                    world.sky.effective_mirage_strength(),
                );
            }

            if let Err(error) = self.post_process.borrow_mut().composite(
                (0, 0, self.width as i32, self.height as i32),
                &self.materials.borrow(),
                self.water_started.elapsed().as_secs_f32(),
                projection.inverse().to_cols_array(),
                |program, channels| {
                    self.draw_post_process_masks_game(
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

            gl::Uniform1i(use_tex_location, 0);

            gl::Uniform1f(alpha_location, 1.0);

            gl::DepthMask(gl::TRUE);

            gl::Enable(gl::DEPTH_TEST);

            gl::Enable(gl::CULL_FACE);

            gl::BindVertexArray(0);
            if let Some(profiler) = self.profiler.borrow_mut().as_mut() {
                profiler.end(draw_stats);
            }
        }
    }
}

fn should_render_generic_entity(
    entity_id: crate::engine::entity::EntityId,
    _entity_manager: &EntityManager,
    character_system: &crate::character::CharacterSystem,
) -> bool {
    character_system
        .get_character_for_entity(entity_id)
        .is_none()
}

fn entity_model_matrix(position: Vec3, rotation: Vec3, scale: Vec3) -> Mat4 {
    let rotation = Quat::from_euler(EulerRot::XYZ, rotation.x, rotation.y, rotation.z);

    Mat4::from_scale_rotation_translation(scale, rotation, position)
}

impl Renderer {
    fn draw_post_process_masks_game(
        &self,
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
            let camera_player = characters.explicit_active_player_id();
            for (&object, &channel) in channels {
                if channel == 0 {
                    continue;
                }
                match object.kind {
                    Kind::Cell => {
                        let Some(cell) = world.get_effective_cell_by_id(object.id) else {
                            continue;
                        };
                        if cell.cell_type != crate::world::CellType::Block || !cell.visible {
                            continue;
                        }
                        let Some(pos) = world.resolve_cell_id(object.id) else {
                            continue;
                        };
                        let model = Mat4::from_translation(Vec3::new(
                            pos.x as f32,
                            pos.y as f32,
                            pos.z as f32,
                        ));
                        uniforms.set(model, channel);
                        gl::BindVertexArray(self.block_vao);
                        gl::DrawArrays(gl::TRIANGLES, first, count);
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
                            if camera_player == Some(character.id)
                                && character
                                    .transform
                                    .position
                                    .distance_squared(camera.current_position)
                                    < 4.0
                            {
                                continue;
                            }
                            let model = Mat4::from_scale_rotation_translation(
                                character.transform.scale,
                                character.transform.rotation,
                                character.transform.position,
                            );
                            uniforms.set(model, channel);
                            let batches =
                                crate::character::character::generate_character_mesh_batches(
                                    character,
                                );
                            self.draw_character_batches(&batches, false);
                        } else if let Some(entity) = actor_entity
                            .and_then(|id| entity_manager.get_entity(id))
                            .filter(|entity| {
                                entity.visible
                                    && entity.enabled
                                    && should_render_generic_entity(
                                        entity.id,
                                        entity_manager,
                                        characters,
                                    )
                            })
                        {
                            let model =
                                entity_model_matrix(entity.position, entity.rotation, entity.scale);
                            uniforms.set(model, channel);
                            self.draw_runtime_entity_mask(entity);
                        }
                    }
                }
            }
            gl::BindVertexArray(0);
        }
    }
}
