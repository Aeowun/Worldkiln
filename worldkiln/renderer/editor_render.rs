use glam::{Mat4, Vec3};

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
    Character(Vec<f32>),
    Entity(RuntimeEntity),
}

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
fn authored_actor_mesh(
    editor: &Editor,
    actor: &crate::world::Actor,
) -> Option<(AuthoredActorMesh, Mat4)> {
    let package_name = actor.package.as_deref()?;
    // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
    // Non-destructive real-time viewport preview for Actor Move/Rotate/Scale.
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

    if let Some(package) = crate::character::package::get_package(package_name) {
        let mut character = crate::character::Character::from_package(actor.id, position, package);
        character.transform.rotation = rotation;
        character.transform.scale = scale;
        return Some((
            AuthoredActorMesh::Character(crate::character::generate_character_mesh(&character)),
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
        AuthoredActorMesh::Character(vertices) => {
            if vertices.is_empty() {
                return 0;
            }
            renderer.draw_character_mesh_persistent(vertices);
            (vertices.len() / super::mesh::VERTEX_3D_FLOATS) as i32
        }
        AuthoredActorMesh::Entity(entity) => renderer.draw_runtime_entity_mesh(entity, textured),
    }
}

fn actor_preview_bounds(editor: &Editor, actor: &crate::world::Actor) -> (Vec3, Vec3) {
    // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
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
    // Ordinary authored Characters must not select a stale gameplay camera.
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

impl Renderer {
    pub fn render_editor(
        &self,
        editor: &Editor,
        world: &World,
        physics: &PhysicsWorld,
        entity_manager: &EntityManager,
        character_system: &crate::character::CharacterSystem,
        gameplay_camera: &crate::renderer::camera::GameplayCamera,
        drag_start: Option<WorldCoord>,
    ) {
        // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
        let mut profile = self.editor_profiler.borrow_mut();
        if let Some(p) = profile.as_mut() {
            p.begin();
        }
        // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:03 -05:00
        // Published statistics are read after the GL section ends.
        let mut terrain_stats = super::terrain::TerrainDrawStats::default();
        let mut water_stats = super::terrain::TerrainDrawStats::default();

        let (camera_pos, view, target) =
            editor_camera_frame(editor, character_system, gameplay_camera);

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
        // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
        let camera_frustum = CameraFrustum::from_view_projection(view_projection);

        const CHARACTER_CAMERA_HIDE_DISTANCE: f32 = 2.0;

        let light_space_matrix =
            self.calculate_light_space_matrix(world.lighting.global_light_direction, camera_pos);
        let render_settings = self.active_lighting_settings();
        let render_shadows =
            world.lighting.shadows_enabled && render_settings.shadow_method != ShadowMethod::Off;
        // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
        // The shadow map only covers the light-space volume around the camera.
        // Cull static chunks against that volume before submitting draw calls;
        // render-distance filtering alone still submitted the entire resident
        // world when legacy radius 0 (Unlimited) was selected.
        let shadow_frustum = CameraFrustum::from_view_projection(light_space_matrix);
        let point_lights = self.collect_point_lights(world);
        self.update_gi_cache(world, physics, camera_pos, &point_lights, render_settings);

        unsafe {
            gl::Enable(gl::DEPTH_TEST);
            gl::DepthMask(gl::TRUE);
            gl::DepthFunc(gl::LESS);

            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
            gl::FrontFace(gl::CCW);

            gl::Disable(gl::BLEND);

            self.update_chunk_cache(
                world,
                match editor.mode {
                    crate::engine::EditorMode::Editor => super::RenderMode::Editor,
                    crate::engine::EditorMode::Play => super::RenderMode::Game,
                },
            );

            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            if let Some(p) = profile.as_mut() {
                p.mark(1);
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
                    }
                }

                if editor.mode == EditorMode::Editor {
                    // Authored Actors participate in the same shadow map as the
                    // rest of the scene. Reuse the exact package mesh and transform
                    // contract used by the editor's main Actor rendering path.
                    for actor in world.actors.values() {
                        if !actor.visible || !actor.enabled {
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
                    }
                }

                if editor.mode == EditorMode::Play {
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
                }
            }

            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            if let Some(p) = profile.as_mut() {
                p.mark(2);
            }
            // Main rendering pass.
            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);

            gl::Viewport(vp_x, vp_y, vp_w, vp_h);

            gl::Enable(gl::SCISSOR_TEST);

            gl::Scissor(vp_x, vp_y, vp_w, vp_h);

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

            // Authored world editor markers (Light and AudioEmitter).
            let cam_right = Vec3::new(view.col(0).x, view.col(1).x, view.col(2).x);

            let cam_up = Vec3::new(view.col(0).y, view.col(1).y, view.col(2).y);

            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:03 -05:00
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
                        // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
                        // Runtime effect markers are handle-owned so a colocated
                        // Light and ParticleEmitter never intercept one another.
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

                // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
                if let Some(p) = profile.as_mut() {
                    p.mark(3);
                }
                // Authored Actors render using their actual CharacterPackage
                // instead of the generic Actor billboard.
                //
                // Actors remain authored World objects. We construct a
                // temporary Character representation only for editor rendering.
                gl::Uniform1i(use_tex_location, 0);

                for actor in world.actors.values() {
                    if !actor.visible || !actor.enabled {
                        continue;
                    }

                    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
                    if actor
                        .package
                        .as_deref()
                        .and_then(crate::engine::entity_package::get_package)
                        .is_some()
                    {
                        let (bounds_min, bounds_max) = actor_preview_bounds(editor, actor);
                        if !camera_frustum.intersects_aabb(bounds_min, bounds_max) {
                            continue;
                        }
                    }

                    let Some((mesh, model)) = authored_actor_mesh(editor, actor) else {
                        continue;
                    };

                    gl::UniformMatrix4fv(
                        model_location,
                        1,
                        gl::FALSE,
                        model.to_cols_array().as_ptr(),
                    );

                    gl::Uniform3f(base_color_location, 1.0, 1.0, 1.0);

                    draw_authored_actor_mesh(self, &mesh, true);
                }
            }

            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            if let Some(p) = profile.as_mut() {
                p.mark(4);
            }
            // Static voxel chunk rendering.
            gl::Uniform1i(use_tex_location, 1);

            let cache = self.chunk_cache.borrow();

            // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
            // Reuse adjacent material bindings across cached Cell batches.
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
                }
            }

            // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
            // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
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

            // Runtime physics bodies and characters.
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

                    let model = Mat4::from_translation(body.position);

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
                    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 20:06 -05:00
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

                    let vertices = crate::character::generate_character_mesh(character);

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

                    self.draw_character_mesh_persistent(&vertices);
                }
            }

            // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            if let Some(p) = profile.as_mut() {
                p.mark(7);
            }
            water_stats = self.draw_terrain_water(
                world,
                camera_pos,
                view_projection,
                underwater.is_some(),
                &camera_frustum,
                chunk_focus,
                chunk_radius,
            );

            if let Some(p) = profile.as_mut() {
                p.mark(8);
            }
            // Editor helpers.
            let plane = crate::editor::grid::select_grid_plane(camera_pos, target);

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            if editor.mode == EditorMode::Editor && editor.plane_picking {
                let (_normal, anchor_offset) = match plane {
                    GridPlane::Xz => (Vec3::Y, editor.anchor.y as f32),
                    GridPlane::Yz => (Vec3::X, editor.anchor.x as f32),
                    GridPlane::Xy => (Vec3::Z, editor.anchor.z as f32),
                };

                let center = if let Some(hover) = editor.hovered_cell {
                    Vec3::new(hover.x as f32, hover.y as f32, hover.z as f32)
                } else {
                    match plane {
                        GridPlane::Xz => {
                            Vec3::new(target.x.floor(), anchor_offset, target.z.floor())
                        }
                        GridPlane::Yz => {
                            Vec3::new(anchor_offset, target.y.floor(), target.z.floor())
                        }
                        GridPlane::Xy => {
                            Vec3::new(target.x.floor(), target.y.floor(), anchor_offset)
                        }
                    }
                };

                let model = Mat4::from_translation(center);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                self.bind_grid_vao(plane);

                gl::DrawArrays(gl::LINES, 0, self.get_grid_count(plane));
            }

            if editor.mode == EditorMode::Editor {
                let anchor_pos = Vec3::new(
                    editor.anchor.x as f32,
                    editor.anchor.y as f32,
                    editor.anchor.z as f32,
                );

                let model = Mat4::from_translation(anchor_pos);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::BindVertexArray(self.anchor_vao);

                gl::DrawArrays(gl::LINES, 0, self.anchor_vertex_count);

                // Static navigation diagnostics. The snapshot itself is owned by
                // the editor; the renderer only consumes its deterministic debug data.
                // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:20 -05:00
                if editor.theme_settings.show_navmesh
                    && editor
                        .navigation_window
                        .debug_overlays_are_current(world.navigation_revision())
                    && let Some(grid) = &editor.navigation_window.debug_grid
                {
                    gl::BindVertexArray(self.highlight_vao);
                    gl::Uniform1f(alpha_location, 0.9);
                    // Worldkiln 0.8.5 | WO MAT-037 | 2026-10-10 18:33 -05:00
                    // NavMesh snapshot overlays must remain visible 16x farther
                    // than the previous chunk-distance-based debug cutoff.
                    // This affects visualization only, not path extraction limits.
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

                // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
                // Bounds-volume outlines are cheap (one wire-box draw per volume),
                // independent of terrain mesh changes and navigation snapshots.
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

                if let Some(hover) = editor.hovered_cell {
                    // Restore hover color after rendering NavMesh Bounds Volumes.
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

                // Hovered Actor outline. Match the Cell hover affordance in intent:
                // warm/yellow means "selectable", while blue remains the selected state.
                if let Some(actor_id) = editor.hovered_actor_id {
                    if editor.selected_actor_id != Some(actor_id) {
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

                // Selected Actor outline.
                if let Some(actor_id) = editor.selected_actor_id {
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

                // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
                // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
                // Sculpt, paint, and Actor scattering share the terrain-following
                // footprint so the authored area is visible before a stroke.
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

                        // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
                        let is_paint =
                            editor.sculpt_tool == crate::world::terrain::SculptTool::Paint;
                        let is_actor_brush = editor.actor_brush_enabled;
                        let (outer_radius, inner_radius) = crate::world::sculpt_preview_radii(
                            editor.sculpt_brush_radius,
                            editor.sculpt_strength,
                        );
                        let segments = 32;

                        gl::LineWidth(2.0);

                        let draw_ring = |radius: f32, color: Vec3| {
                            if radius <= 0.01 {
                                return;
                            }
                            let mut circle_verts = Vec::with_capacity(segments * 20);
                            for i in 0..segments {
                                let a0 = (i as f32 / segments as f32) * std::f32::consts::TAU;
                                let a1 = ((i + 1) as f32 / segments as f32) * std::f32::consts::TAU;
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
                            let (vao, vbo, vertex_count) =
                                super::mesh::upload_vertices_3d(&circle_verts);
                            gl::Uniform3f(base_color_location, color.x, color.y, color.z);
                            gl::BindVertexArray(vao);
                            gl::DrawArrays(gl::LINES, 0, vertex_count);
                            gl::DeleteVertexArrays(1, &vao);
                            gl::DeleteBuffers(1, &vbo);
                        };

                        // Outer ring is the active footprint for sculpt and paint.
                        draw_ring(
                            outer_radius,
                            if is_paint {
                                Vec3::new(0.25, 0.75, 1.0)
                            } else if is_actor_brush {
                                Vec3::new(0.35, 0.95, 0.45)
                            } else {
                                Vec3::new(1.0, 0.55, 0.0)
                            },
                        );

                        // Paint has no strength/falloff control in the bare-bones slice.
                        if !is_paint && !is_actor_brush {
                            draw_ring(inner_radius, Vec3::new(0.9, 0.9, 0.9));
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

                let model = Mat4::from_translation(anchor_pos);

                gl::UniformMatrix4fv(model_location, 1, gl::FALSE, model.to_cols_array().as_ptr());

                gl::BindVertexArray(self.axis_vao);

                gl::DrawArrays(gl::LINES, 0, self.axis_vertex_count);
            }

            // Editor ghost rendering.
            if editor.mode == EditorMode::Editor {
                // Grab preview.
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

                // Invisible-cell ghosts are cached by Renderer. The cache is
                // rebuilt only when render-relevant World state or editor mode changes.
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

                // Return to the normal world shader before any
                // of the remaining editor previews use its uniforms.
                gl::UseProgram(self.grid_program);

                gl::DepthMask(gl::TRUE);

                gl::Uniform1f(alpha_location, 1.0);

                // Existing build/erase preview.
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

            self.render_particles(&world.particles, view, projection);

            gl::UseProgram(self.grid_program);

            gl::Uniform1i(use_tex_location, 0);

            gl::Uniform1f(alpha_location, 1.0);

            gl::DepthMask(gl::TRUE);

            gl::Enable(gl::DEPTH_TEST);

            gl::Enable(gl::CULL_FACE);

            gl::BindVertexArray(0);
        }
        // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
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
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
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
