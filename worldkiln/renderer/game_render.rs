use glam::{EulerRot, Mat4, Quat, Vec3};

use crate::engine::entity::EntityManager;
use crate::engine::physics::PhysicsWorld;
use crate::world::World;

use super::Renderer;
use super::SHADOW_RES;
use super::chunk::CameraFrustum;
use super::profile::DrawStats;
use super::settings::ShadowMethod;

impl Renderer {
    pub fn render_game(
        &self,
        world: &World,
        physics: &PhysicsWorld,
        entity_manager: &EntityManager,
        character_system: &crate::character::CharacterSystem,
        gameplay_camera: &crate::renderer::camera::GameplayCamera,
    ) {
        let camera_pos = gameplay_camera.current_position;
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
            self.calculate_light_space_matrix(world.lighting.global_light_direction, camera_pos);
        let render_settings = self.active_lighting_settings();
        let render_shadows =
            world.lighting.shadows_enabled && render_settings.shadow_method != ShadowMethod::Off;
        // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
        // Match shadow submission to the actual directional-shadow volume.
        // Without this, Unlimited render distance submits every resident static
        // chunk to a 60x60x100 light-space map even when the chunk cannot
        // contribute a shadow.
        let shadow_frustum = CameraFrustum::from_view_projection(light_space_matrix);
        let point_lights = self.collect_point_lights(world);
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

            // Shadow pass.
            if render_shadows {
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

                    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
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

                    let model = Mat4::from_translation(body.position);

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

                    let model = entity_model_matrix(entity.position, entity.rotation, entity.scale);

                    gl::UniformMatrix4fv(
                        self.shadow_model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
                    let vertex_count = self.draw_runtime_entity_mesh(entity, false);
                    if vertex_count <= 0 {
                        continue;
                    }
                    draw_stats.shadow_draws += 1;
                    draw_stats.shadow_triangles += vertex_count as u64 / 3;
                }
            }
            if let Some(profiler) = self.profiler.borrow().as_ref() {
                profiler.mark(1);
            }

            // Main rendering pass.
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);

            gl::Viewport(0, 0, self.width as i32, self.height as i32);

            gl::Disable(gl::SCISSOR_TEST);

            gl::ClearColor(0.0, 0.0, 0.0, 1.0);

            gl::Clear(gl::COLOR_BUFFER_BIT | gl::DEPTH_BUFFER_BIT);

            // Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
            let underwater = super::terrain::underwater_state(world, camera_pos);
            self.sky_renderer
                .render(self, world, camera_pos, view_projection, underwater);

            gl::UseProgram(self.grid_program);

            gl::Uniform1i(
                self.grid_debug_view_location,
                render_settings.debug_view as i32,
            );
            self.bind_gi_uniforms(render_settings);
            self.bind_underwater_uniforms(underwater, camera_pos);
            self.bind_atmosphere_uniforms(world, camera_pos, underwater.is_some());

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

            // Global lighting.
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

            // Shadow uniforms.
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

            gl::BindTexture(gl::TEXTURE_2D, self.shadow_depth_tex);

            gl::Uniform1i(self.grid_shadow_map_location, 0);

            gl::ActiveTexture(gl::TEXTURE1);

            gl::BindTexture(gl::TEXTURE_2D, self.fallback_tex);

            // Point lights.
            gl::Uniform1i(
                self.grid_point_light_count_location,
                point_lights.len() as i32,
            );

            for (i, light) in point_lights.iter().enumerate() {
                let uniforms = self.grid_point_light_uniforms[i];

                gl::Uniform3f(
                    uniforms.position,
                    light.position.x,
                    light.position.y,
                    light.position.z,
                );

                gl::Uniform3f(uniforms.color, light.color.x, light.color.y, light.color.z);
                gl::Uniform1f(uniforms.intensity, light.intensity);
                gl::Uniform1f(uniforms.range, light.range);
            }

            // Static voxel chunk rendering.
            let camera_frustum = CameraFrustum::from_view_projection(view_projection);

            gl::Uniform1i(use_tex_location, 1);
            if let Some(profiler) = self.profiler.borrow().as_ref() {
                profiler.mark(2);
            }

            let cache = self.chunk_cache.borrow();

            // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
            let mut last_cell_texture: Option<u32> = None;
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

            // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
            // Worldkiln 0.9.3 | WO 0.9.3-16 | Terrain performance
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

            // Runtime physics bodies, generic Entities, and Characters.
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

                let model = Mat4::from_translation(body.position);

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

                // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
                if let Some((bounds_min, bounds_max)) =
                    crate::engine::entity_visual::static_entity_world_bounds(entity)
                    && !camera_frustum.intersects_aabb(bounds_min, bounds_max)
                {
                    continue;
                }

                let model = entity_model_matrix(entity.position, entity.rotation, entity.scale);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                let vertex_count = self.draw_runtime_entity_mesh(entity, true);
                if vertex_count <= 0 {
                    continue;
                }
                draw_stats.dynamic_draws += 1;
                draw_stats.dynamic_triangles += vertex_count as u64 / 3;
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

                let vertices = crate::character::generate_character_mesh(character);

                let model = Mat4::from_scale_rotation_translation(
                    character.transform.scale,
                    character.transform.rotation,
                    character.transform.position,
                );

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                self.draw_character_mesh_persistent(&vertices);
                draw_stats.dynamic_draws += 1;
                draw_stats.dynamic_triangles +=
                    (vertices.len() / super::mesh::VERTEX_3D_FLOATS / 3) as u64;
            }

            // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
            let water_stats = self.draw_terrain_water(
                world,
                camera_pos,
                view_projection,
                underwater.is_some(),
                &camera_frustum,
                chunk_focus,
                chunk_radius,
            );
            draw_stats.water_draws += water_stats.draws;
            draw_stats.water_triangles += water_stats.triangles;

            self.render_particles(&world.particles, view, projection);

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
