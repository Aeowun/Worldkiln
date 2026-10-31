// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::HashMap;
use std::sync::{Arc, OnceLock};

use super::{Renderer, character_render::MeshDrawStats};
use crate::character::geometry::part_visible;
use crate::character::{
    Character,
    package::{CharacterPackage, GeometryPrimitive},
};
use glam::Mat4;

const FLOATS_PER_VERTEX: usize = 20;
const SKIN_TEXTURE_UNIT: u32 = 15;

struct GpuPart {
    vao: u32,
    vbo: u32,
    vertices: i32,
    source_part_index: usize,
}

struct GpuMesh {
    package: Arc<CharacterPackage>,
    parts: Vec<GpuPart>,
}

impl GpuMesh {
    fn free(&mut self) {
        unsafe {
            for part in &self.parts {
                if part.vao != 0 {
                    gl::DeleteVertexArrays(1, &part.vao);
                }
                if part.vbo != 0 {
                    gl::DeleteBuffers(1, &part.vbo);
                }
            }
        }
    }
}

#[derive(Default)]
pub(super) struct GpuSkinState {
    meshes: HashMap<String, GpuMesh>,
    bone_buffer: u32,
    bone_texture: u32,
}

impl GpuSkinState {
    pub(super) fn free(&mut self) {
        for mesh in self.meshes.values_mut() {
            mesh.free();
        }
        self.meshes.clear();
        unsafe {
            if self.bone_texture != 0 {
                gl::DeleteTextures(1, &self.bone_texture);
            }
            if self.bone_buffer != 0 {
                gl::DeleteBuffers(1, &self.bone_buffer);
            }
        }
        self.bone_texture = 0;
        self.bone_buffer = 0;
    }

    fn ensure_palette(&mut self) -> bool {
        if self.bone_texture != 0 {
            return true;
        }
        if !gl::TexBuffer::is_loaded() {
            return false;
        }
        unsafe {
            gl::GenBuffers(1, &mut self.bone_buffer);
            gl::GenTextures(1, &mut self.bone_texture);
            if self.bone_buffer == 0 || self.bone_texture == 0 {
                return false;
            }
            gl::BindBuffer(gl::TEXTURE_BUFFER, self.bone_buffer);
            gl::BufferData(
                gl::TEXTURE_BUFFER,
                (16 * std::mem::size_of::<f32>()) as isize,
                std::ptr::null(),
                gl::STREAM_DRAW,
            );
            gl::ActiveTexture(gl::TEXTURE0 + SKIN_TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_BUFFER, self.bone_texture);
            gl::TexBuffer(gl::TEXTURE_BUFFER, gl::RGBA32F, self.bone_buffer);
            let status = gl::GetError();
            gl::ActiveTexture(gl::TEXTURE1);
            if status != gl::NO_ERROR {
                eprintln!(
                    "[Worldkiln GPU skin] Palette attachment failed: GL error {status:#x}; falling back to CPU skinning"
                );
                gl::DeleteTextures(1, &self.bone_texture);
                gl::DeleteBuffers(1, &self.bone_buffer);
                self.bone_texture = 0;
                self.bone_buffer = 0;
                return false;
            }
        }
        true
    }
}

fn upload_mesh(package: Arc<CharacterPackage>) -> Option<GpuMesh> {
    if package.geometry.is_empty()
        || package
            .geometry
            .iter()
            .any(|p| !matches!(p.primitive, GeometryPrimitive::SkinnedTriangleMesh { .. }))
    {
        return None;
    }

    let mut parts = Vec::new();
    for (source_part_index, part) in package.geometry.iter().enumerate() {
        let GeometryPrimitive::SkinnedTriangleMesh {
            vertices,
            inverse_bind_matrices,
        } = &part.primitive
        else {
            unreachable!("checked above");
        };
        if inverse_bind_matrices.len() != package.skeleton.joints.len()
            || vertices.len() > i32::MAX as usize
        {
            for p in &parts {
                let p: &GpuPart = p;
                unsafe {
                    gl::DeleteVertexArrays(1, &p.vao);
                    gl::DeleteBuffers(1, &p.vbo);
                }
            }
            return None;
        }
        let mut data = Vec::with_capacity(vertices.len() * FLOATS_PER_VERTEX);
        for v in vertices {
            data.extend_from_slice(&v.position.to_array());
            data.extend_from_slice(&v.normal.to_array());
            data.extend_from_slice(&v.color);
            data.extend_from_slice(&v.tex_coord.to_array());
            data.extend(v.joints.map(|index| index as f32));
            data.extend_from_slice(&v.weights);
        }

        let mut vao = 0;
        let mut vbo = 0;
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                std::mem::size_of_val(data.as_slice()) as isize,
                data.as_ptr().cast(),
                gl::STATIC_DRAW,
            );
            let stride = (FLOATS_PER_VERTEX * std::mem::size_of::<f32>()) as i32;
            for (attrib, count, offset) in [
                (0, 3, 0),
                (1, 3, 3),
                (2, 4, 6),
                (3, 2, 10),
                (4, 4, 12),
                (5, 4, 16),
            ] {
                gl::EnableVertexAttribArray(attrib);
                gl::VertexAttribPointer(
                    attrib,
                    count,
                    gl::FLOAT,
                    gl::FALSE,
                    stride,
                    (offset * std::mem::size_of::<f32>()) as *const _,
                );
            }
            gl::BindVertexArray(0);
        }
        parts.push(GpuPart {
            vao,
            vbo,
            vertices: vertices.len() as i32,
            source_part_index,
        });
    }
    Some(GpuMesh { package, parts })
}

impl Renderer {
    pub(super) fn draw_gpu_skinned_character(
        &self,
        character: &Character,
        textured: bool,
    ) -> Option<MeshDrawStats> {
        static CPU_FALLBACK: OnceLock<bool> = OnceLock::new();
        if *CPU_FALLBACK.get_or_init(|| std::env::var_os("WORLDKILN_CPU_SKINNING").is_some()) {
            return None;
        }
        if self.grid_skin_enabled_location < 0
            || self.grid_skin_matrices_location < 0
            || self.grid_skin_material_color_location < 0
            || !gl::TexBuffer::is_loaded()
        {
            return None;
        }

        let package = &character.package;
        let key = &character.package_name;
        let mut state = self.gpu_skin_state.borrow_mut();
        if !state.ensure_palette() {
            return None;
        }
        if state
            .meshes
            .get(key)
            .is_some_and(|mesh| !Arc::ptr_eq(&mesh.package, package))
        {
            if let Some(mut old) = state.meshes.remove(key) {
                old.free();
            }
        }
        if !state.meshes.contains_key(key) {
            let mesh = upload_mesh(Arc::clone(package))?;
            state.meshes.insert(key.clone(), mesh);
        }
        let mesh = state.meshes.get(key)?;
        let selected_part = character
            .model_material_choice
            .as_deref()
            .and_then(|material| {
                package
                    .geometry
                    .iter()
                    .find(|part| part.material == material)
            });
        let tint_material = selected_part.map(|part| part.material.as_str());
        let mut stats = MeshDrawStats::default();

        unsafe {
            gl::Uniform1i(self.grid_skin_enabled_location, 1);
            gl::Uniform1i(self.grid_skin_matrices_location, SKIN_TEXTURE_UNIT as i32);
            gl::ActiveTexture(gl::TEXTURE0 + SKIN_TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_BUFFER, state.bone_texture);
            gl::ActiveTexture(gl::TEXTURE1);
        }

        for batch in &mesh.parts {
            let part = &package.geometry[batch.source_part_index];
            if !part_visible(part, &character.appearance, character.has_gun) {
                continue;
            }
            let GeometryPrimitive::SkinnedTriangleMesh {
                inverse_bind_matrices,
                ..
            } = &part.primitive
            else {
                unreachable!("GPU package contains only skinned geometry");
            };

            let mut palette = Vec::<f32>::with_capacity(inverse_bind_matrices.len() * 16);
            for (joint, inverse_bind) in inverse_bind_matrices.iter().enumerate() {
                let skin: Mat4 = character.current_pose.matrices[joint] * *inverse_bind;
                palette.extend_from_slice(&skin.to_cols_array());
            }
            let material = character
                .appearance
                .material(tint_material.unwrap_or(part.material.as_str()));
            let texture = selected_part.unwrap_or(part).base_color_texture.as_deref();
            unsafe {
                gl::BindBuffer(gl::TEXTURE_BUFFER, state.bone_buffer);
                gl::BufferData(
                    gl::TEXTURE_BUFFER,
                    std::mem::size_of_val(palette.as_slice()) as isize,
                    palette.as_ptr().cast(),
                    gl::STREAM_DRAW,
                );
                gl::Uniform4f(
                    self.grid_skin_material_color_location,
                    material[0],
                    material[1],
                    material[2],
                    material[3],
                );
                if textured {
                    gl::Uniform1i(self.grid_use_texture_location, i32::from(texture.is_some()));
                    if let Some(path) = texture {
                        gl::ActiveTexture(gl::TEXTURE1);
                        gl::BindTexture(gl::TEXTURE_2D, self.get_texture_file(path));
                    }
                }
                gl::BindVertexArray(batch.vao);
                gl::DrawArrays(gl::TRIANGLES, 0, batch.vertices);
            }
            stats.vertices += batch.vertices;
            stats.draw_calls += 1;
        }

        unsafe {
            gl::Uniform1i(self.grid_skin_enabled_location, 0);
            gl::BindVertexArray(0);
            if textured {
                gl::Uniform1i(self.grid_use_texture_location, 0);
            }
            gl::ActiveTexture(gl::TEXTURE0 + SKIN_TEXTURE_UNIT);
            gl::BindTexture(gl::TEXTURE_BUFFER, 0);
            gl::ActiveTexture(gl::TEXTURE1);
        }
        Some(stats)
    }
}
