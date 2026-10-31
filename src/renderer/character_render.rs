// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::mesh::VERTEX_3D_FLOATS;
use super::{Renderer, StaticEntityGpuBatch, StaticEntityGpuMesh};
use crate::engine::entity::{EntityId, RuntimeEntity};
use crate::engine::entity_package::{EntityPackage, get_package};
use crate::engine::entity_visual::{
    TEXTURED_ENTITY_VERTEX_FLOATS, entity_uses_static_package_geometry,
    generate_entity_mesh_from_package, generate_static_entity_mesh_batches,
};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq)]
pub struct MeshDrawStats {
    pub vertices: i32,
    pub draw_calls: u64,
}

impl MeshDrawStats {
    fn from_vertex_total_and_batches(vertices: i32, batches: usize) -> Self {
        Self {
            vertices,
            draw_calls: batches as u64,
        }
    }

    pub fn from_batch_vertex_counts(vertex_counts: &[i32]) -> Self {
        Self::from_vertex_total_and_batches(
            vertex_counts.iter().copied().sum(),
            vertex_counts.len(),
        )
    }
}

fn static_material_cache_key(package_name: &str, selected: Option<&str>) -> String {
    serde_json::to_string(&(package_name, selected)).expect("material cache identity")
}

impl Renderer {
    pub fn draw_character_batches(
        &self,
        batches: &[crate::engine::entity_visual::StaticEntityMeshBatch],
        textured: bool,
    ) -> i32 {
        self.draw_character_batches_stats(batches, textured)
            .vertices
    }

    pub fn draw_character_batches_stats(
        &self,
        batches: &[crate::engine::entity_visual::StaticEntityMeshBatch],
        textured: bool,
    ) -> MeshDrawStats {
        let mut count = 0;
        unsafe {
            gl::BindVertexArray(self.character_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.character_vbo);
            configure_character_stream(12, true);
            for batch in batches {
                if textured {
                    gl::Uniform1i(
                        self.grid_use_texture_location,
                        i32::from(batch.texture.is_some()),
                    );
                    if let Some(path) = batch.texture.as_deref() {
                        gl::ActiveTexture(gl::TEXTURE1);
                        gl::BindTexture(gl::TEXTURE_2D, self.get_texture_file(path));
                    }
                }
                gl::BufferData(
                    gl::ARRAY_BUFFER,
                    std::mem::size_of_val(batch.vertices.as_slice()) as isize,
                    batch.vertices.as_ptr() as *const _,
                    gl::STREAM_DRAW,
                );
                let vertices = (batch.vertices.len() / 12) as i32;
                gl::DrawArrays(gl::TRIANGLES, 0, vertices);
                count += vertices;
            }
            if textured {
                gl::Uniform1i(self.grid_use_texture_location, 0);
            }
            gl::BindVertexArray(0);
        }
        MeshDrawStats::from_vertex_total_and_batches(count, batches.len())
    }

    pub fn draw_authored_character_cached(
        &self,
        actor: &crate::world::Actor,
        textured: bool,
    ) -> i32 {
        let Some(package_name) = actor.package.as_deref() else {
            return 0;
        };
        let mut attributes = actor.attributes.iter().collect::<Vec<_>>();
        attributes.sort_by(|(a, _), (b, _)| a.cmp(b));
        let key = format!(
            "authored-character:{}:{}:{}",
            actor.id,
            package_name,
            serde_json::to_string(&attributes).unwrap_or_default(),
        );
        if !self.entity_mesh_cache.borrow().contains_key(&key) {
            let Some(package) = crate::character::package::get_package(package_name) else {
                return 0;
            };
            let mut character =
                crate::character::Character::from_package(actor.id, actor.position, package);
            crate::character::character::set_model_material(&mut character, &actor.attributes);
            let cpu_batches =
                crate::character::character::generate_character_mesh_batches(&character);
            let mut batches = Vec::with_capacity(cpu_batches.len());
            let mut vertex_count = 0;
            for batch in cpu_batches {
                let vertex_count_for_batch = (batch.vertices.len() / 12) as i32;
                let (mut vao, mut vbo) = (0, 0);
                unsafe {
                    gl::GenVertexArrays(1, &mut vao);
                    gl::GenBuffers(1, &mut vbo);
                    gl::BindVertexArray(vao);
                    gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
                    gl::BufferData(
                        gl::ARRAY_BUFFER,
                        std::mem::size_of_val(batch.vertices.as_slice()) as isize,
                        batch.vertices.as_ptr().cast(),
                        gl::STATIC_DRAW,
                    );
                    configure_character_stream(12, true);
                    gl::BindVertexArray(0);
                }
                vertex_count += vertex_count_for_batch;
                batches.push(StaticEntityGpuBatch {
                    vao,
                    vbo,
                    vertex_count: vertex_count_for_batch,
                    texture: batch.texture,
                });
            }
            self.entity_mesh_cache.borrow_mut().insert(
                key.clone(),
                StaticEntityGpuMesh {
                    batches,
                    vertex_count,
                },
            );
        }
        let cache = self.entity_mesh_cache.borrow();
        let Some(mesh) = cache.get(&key) else {
            return 0;
        };
        for batch in &mesh.batches {
            unsafe {
                if textured {
                    gl::Uniform1i(
                        self.grid_use_texture_location,
                        i32::from(batch.texture.is_some()),
                    );
                    if let Some(path) = batch.texture.as_deref() {
                        gl::ActiveTexture(gl::TEXTURE1);
                        gl::BindTexture(gl::TEXTURE_2D, self.get_texture_file(path));
                    }
                }
                gl::BindVertexArray(batch.vao);
                gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                gl::BindVertexArray(0);
            }
        }
        if textured {
            unsafe { gl::Uniform1i(self.grid_use_texture_location, 0) };
        }
        mesh.vertex_count
    }

    pub fn get_block_mask_range(&self, mask: u8) -> (i32, i32) {
        self.block_mask_ranges[mask as usize]
    }

    pub fn draw_character_mesh_persistent(&self, vertices: &[f32]) {
        let _diagnostic_operation =
            crate::engine::development_capture::operation("animation", "vertex_upload", false);
        if vertices.is_empty() {
            return;
        }

        unsafe {
            gl::Uniform1i(self.grid_use_texture_location, 0);
            gl::BindVertexArray(self.character_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.character_vbo);
            configure_character_stream(10, false);

            gl::BufferData(
                gl::ARRAY_BUFFER,
                (vertices.len() * std::mem::size_of::<f32>()) as isize,
                vertices.as_ptr() as *const _,
                gl::STREAM_DRAW,
            );

            let count = (vertices.len() / VERTEX_3D_FLOATS) as i32;

            gl::DrawArrays(gl::TRIANGLES, 0, count);

            gl::BindVertexArray(0);
        }
    }

    pub fn draw_static_entity_package(
        &self,
        package: &EntityPackage,
        textured: bool,
        selected_material: Option<&str>,
    ) -> i32 {
        self.draw_static_entity_package_stats(package, textured, selected_material)
            .vertices
    }

    pub fn draw_static_entity_package_stats(
        &self,
        package: &EntityPackage,
        textured: bool,
        selected_material: Option<&str>,
    ) -> MeshDrawStats {
        let key = static_material_cache_key(&package.metadata.name, selected_material);

        if !self.entity_mesh_cache.borrow().contains_key(&key) {
            let mut entity = RuntimeEntity::new(EntityId(0), package.metadata.name.clone());
            if let Some(material) = selected_material {
                entity.attributes.insert(
                    crate::engine::entity_visual::MODEL_MATERIAL_CHOICE.to_string(),
                    crate::world::AttributeValue::String(material.to_string()),
                );
            }
            let cpu_batches = generate_static_entity_mesh_batches(&entity, package);
            if cpu_batches.is_empty() {
                return MeshDrawStats::default();
            }

            let mut vertex_count = 0;
            let mut batches = Vec::with_capacity(cpu_batches.len());
            for cpu_batch in cpu_batches {
                let batch = unsafe {
                    let mut vao = 0;
                    let mut vbo = 0;
                    gl::GenVertexArrays(1, &mut vao);
                    gl::GenBuffers(1, &mut vbo);
                    gl::BindVertexArray(vao);
                    gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
                    gl::BufferData(
                        gl::ARRAY_BUFFER,
                        std::mem::size_of_val(cpu_batch.vertices.as_slice()) as isize,
                        cpu_batch.vertices.as_ptr() as *const _,
                        gl::STATIC_DRAW,
                    );

                    let stride =
                        (TEXTURED_ENTITY_VERTEX_FLOATS * std::mem::size_of::<f32>()) as i32;
                    gl::VertexAttribPointer(0, 3, gl::FLOAT, gl::FALSE, stride, std::ptr::null());
                    gl::EnableVertexAttribArray(0);
                    gl::VertexAttribPointer(
                        1,
                        3,
                        gl::FLOAT,
                        gl::FALSE,
                        stride,
                        (3 * std::mem::size_of::<f32>()) as *const _,
                    );
                    gl::EnableVertexAttribArray(1);
                    gl::VertexAttribPointer(
                        2,
                        4,
                        gl::FLOAT,
                        gl::FALSE,
                        stride,
                        (6 * std::mem::size_of::<f32>()) as *const _,
                    );
                    gl::EnableVertexAttribArray(2);
                    gl::VertexAttribPointer(
                        3,
                        2,
                        gl::FLOAT,
                        gl::FALSE,
                        stride,
                        (10 * std::mem::size_of::<f32>()) as *const _,
                    );
                    gl::EnableVertexAttribArray(3);
                    gl::BindVertexArray(0);

                    StaticEntityGpuBatch {
                        vao,
                        vbo,
                        vertex_count: (cpu_batch.vertices.len() / TEXTURED_ENTITY_VERTEX_FLOATS)
                            as i32,
                        texture: cpu_batch.texture,
                    }
                };
                vertex_count += batch.vertex_count;
                batches.push(batch);
            }
            let mesh = StaticEntityGpuMesh {
                batches,
                vertex_count,
            };
            self.entity_mesh_cache
                .borrow_mut()
                .insert(key.clone(), mesh);
        }

        let cache = self.entity_mesh_cache.borrow();
        let Some(mesh) = cache.get(&key) else {
            return MeshDrawStats::default();
        };

        for batch in &mesh.batches {
            unsafe {
                if textured {
                    if let Some(texture_path) = batch.texture.as_deref() {
                        gl::Uniform1i(self.grid_use_texture_location, 1);
                        gl::ActiveTexture(gl::TEXTURE1);
                        gl::BindTexture(gl::TEXTURE_2D, self.get_texture_file(texture_path));
                    } else {
                        gl::Uniform1i(self.grid_use_texture_location, 0);
                    }
                }
                gl::BindVertexArray(batch.vao);
                gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                gl::BindVertexArray(0);
            }
        }
        if textured {
            unsafe { gl::Uniform1i(self.grid_use_texture_location, 0) };
        }
        MeshDrawStats::from_vertex_total_and_batches(mesh.vertex_count, mesh.batches.len())
    }

    pub(super) fn draw_runtime_entity_mask(&self, entity: &RuntimeEntity) {
        let Some(package_name) = entity.package_name.as_deref() else {
            return;
        };
        let Some(package) = get_package(package_name) else {
            return;
        };
        if let Some(clip) = entity.animation.current_clip.as_deref() {
            if let Some(character_package) = crate::character::package::get_package(package_name) {
                if character_package.clip_named(clip).is_some() {
                    let mut character = crate::character::Character::from_package(
                        entity.id.0,
                        entity.position,
                        character_package,
                    );
                    if character
                        .animation_controller
                        .set_actor_clip_time(clip, entity.animation.time)
                    {
                        character.current_pose = character.animation_controller.evaluate_pose();
                        let batches = crate::character::character::generate_character_mesh_batches(
                            &character,
                        );
                        self.draw_character_batches(&batches, false);
                        return;
                    }
                }
            }
        }
        if entity_uses_static_package_geometry(entity, &package) {
            let selected = crate::engine::entity_visual::selected_model_material(entity, &package);
            self.draw_static_entity_package(&package, false, selected);
            return;
        }
        let vertices = generate_entity_mesh_from_package(entity, &package);
        if vertices.is_empty() {
            return;
        }
        unsafe {
            gl::BindVertexArray(self.character_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.character_vbo);
            configure_character_stream(10, false);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(vertices.as_slice()) as isize,
                vertices.as_ptr().cast(),
                gl::STREAM_DRAW,
            );
            gl::DrawArrays(gl::TRIANGLES, 0, (vertices.len() / VERTEX_3D_FLOATS) as i32);
            gl::BindVertexArray(0);
        }
    }

    pub fn draw_runtime_entity_mesh(&self, entity: &RuntimeEntity, textured: bool) -> i32 {
        self.draw_runtime_entity_mesh_stats(entity, textured)
            .vertices
    }

    pub fn draw_runtime_entity_mesh_stats(
        &self,
        entity: &RuntimeEntity,
        textured: bool,
    ) -> MeshDrawStats {
        let Some(package_name) = entity.package_name.as_deref() else {
            return MeshDrawStats::default();
        };
        let Some(package) = get_package(package_name) else {
            return MeshDrawStats::default();
        };

        if let Some(clip) = entity.animation.current_clip.as_deref() {
            if let Some(character_package) = crate::character::package::get_package(package_name) {
                if character_package.clip_named(clip).is_some() {
                    let mut model = crate::character::Character::from_package(
                        entity.id.0,
                        entity.position,
                        character_package,
                    );
                    if model
                        .animation_controller
                        .set_actor_clip_time(clip, entity.animation.time)
                    {
                        model.current_pose = model.animation_controller.evaluate_pose();
                        let batches =
                            crate::character::character::generate_character_mesh_batches(&model);
                        return self.draw_character_batches_stats(&batches, textured);
                    }
                }
            }
        }

        if entity_uses_static_package_geometry(entity, &package) {
            let selected = crate::engine::entity_visual::selected_model_material(entity, &package);
            return self.draw_static_entity_package_stats(&package, textured, selected);
        }

        let vertices = generate_entity_mesh_from_package(entity, &package);
        if vertices.is_empty() {
            return MeshDrawStats::default();
        }

        let vertex_count = (vertices.len() / VERTEX_3D_FLOATS) as i32;
        self.draw_character_mesh_persistent(&vertices);
        MeshDrawStats::from_vertex_total_and_batches(vertex_count, 1)
    }
}

unsafe fn configure_character_stream(floats: i32, uv: bool) {
    unsafe {
        let stride = floats * std::mem::size_of::<f32>() as i32;
        for (index, size, offset) in [(0, 3, 0), (1, 3, 3), (2, 4, 6)] {
            gl::VertexAttribPointer(
                index,
                size,
                gl::FLOAT,
                gl::FALSE,
                stride,
                (offset * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(index);
        }
        if uv {
            gl::VertexAttribPointer(
                3,
                2,
                gl::FLOAT,
                gl::FALSE,
                stride,
                (10 * std::mem::size_of::<f32>()) as *const _,
            );
            gl::EnableVertexAttribArray(3);
        } else {
            gl::DisableVertexAttribArray(3);
        }
    }
}

#[cfg(test)]
mod material_cache_tests {
    use super::static_material_cache_key as key;
    #[test]
    fn model_material_cache_isolates_defaults_choices_and_package_identity() {
        assert_ne!(key("Model", None), key("Model", Some("Warm")));
        assert_ne!(key("Model", Some("Warm")), key("Model", Some("Cool")));
        assert_ne!(key("Model", Some("Warm")), key("Other", Some("Warm")));
        assert_ne!(key("Model", Some("Warm")), key("model", Some("Warm")));
        assert_ne!(key("A::B", Some("C")), key("A", Some("B::C")));
        assert_ne!(key("Model", None), key("Model", Some("<default>")));
    }
}
