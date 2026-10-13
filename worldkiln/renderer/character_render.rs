use super::mesh::VERTEX_3D_FLOATS;
use super::{Renderer, StaticEntityGpuBatch, StaticEntityGpuMesh};
use crate::engine::entity::{EntityId, RuntimeEntity};
use crate::engine::entity_package::{EntityPackage, get_package};
use crate::engine::entity_visual::{
    TEXTURED_ENTITY_VERTEX_FLOATS, entity_uses_static_package_geometry,
    generate_entity_mesh_from_package, generate_static_entity_mesh_batches,
};

impl Renderer {
    pub fn get_block_mask_range(&self, mask: u8) -> (i32, i32) {
        self.block_mask_ranges[mask as usize]
    }

    pub fn draw_character_mesh_persistent(&self, vertices: &[f32]) {
        if vertices.is_empty() {
            return;
        }

        unsafe {
            gl::BindVertexArray(self.character_vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, self.character_vbo);

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

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    pub fn draw_static_entity_package(&self, package: &EntityPackage, textured: bool) -> i32 {
        let key = package.metadata.name.to_ascii_lowercase();

        if !self.entity_mesh_cache.borrow().contains_key(&key) {
            let entity = RuntimeEntity::new(EntityId(0), package.metadata.name.clone());
            let cpu_batches = generate_static_entity_mesh_batches(&entity, package);
            if cpu_batches.is_empty() {
                return 0;
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
            return 0;
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
        mesh.vertex_count
    }

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    pub fn draw_runtime_entity_mesh(&self, entity: &RuntimeEntity, textured: bool) -> i32 {
        let Some(package_name) = entity.package_name.as_deref() else {
            return 0;
        };
        let Some(package) = get_package(package_name) else {
            return 0;
        };

        if entity_uses_static_package_geometry(entity, &package) {
            return self.draw_static_entity_package(&package, textured);
        }

        let vertices = generate_entity_mesh_from_package(entity, &package);
        if vertices.is_empty() {
            return 0;
        }

        let vertex_count = (vertices.len() / VERTEX_3D_FLOATS) as i32;
        self.draw_character_mesh_persistent(&vertices);
        vertex_count
    }
}
