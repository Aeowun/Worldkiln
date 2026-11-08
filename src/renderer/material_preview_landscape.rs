// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::Renderer;
use std::f32::consts::TAU;
const CELLS: usize = 12;
fn height(x: f32, z: f32) -> f32 {
    0.16 * (x * TAU).sin() * (z * TAU).cos() + 0.045 * (z * TAU * 2.0).sin()
}
fn vertex(x: f32, z: f32) -> [f32; 12] {
    let derivative_x = 0.16 * TAU * (x * TAU).cos() * (z * TAU).cos();
    let derivative_z =
        -0.16 * TAU * (x * TAU).sin() * (z * TAU).sin() + 0.09 * TAU * (z * TAU * 2.0).cos();
    let n = glam::Vec3::new(-derivative_x, 1.0, -derivative_z).normalize();
    [
        x - 0.5,
        height(x, z) - 0.05,
        z - 0.5,
        n.x,
        n.y,
        n.z,
        1.0,
        1.0,
        1.0,
        1.0,
        x,
        z,
    ]
}
fn geometry() -> Vec<f32> {
    let mut result = Vec::with_capacity(CELLS * CELLS * 6 * 12);
    for z in 0..CELLS {
        for x in 0..CELLS {
            let a = vertex(x as f32 / CELLS as f32, z as f32 / CELLS as f32);
            let b = vertex((x + 1) as f32 / CELLS as f32, z as f32 / CELLS as f32);
            let c = vertex(x as f32 / CELLS as f32, (z + 1) as f32 / CELLS as f32);
            let d = vertex((x + 1) as f32 / CELLS as f32, (z + 1) as f32 / CELLS as f32);
            for v in [a, c, b, b, c, d] {
                result.extend_from_slice(&v);
            }
        }
    }
    result
}
impl Renderer {
    pub(super) fn landscape_preview_geometry(&self) -> Option<(u32, i32)> {
        if let Some((vao, _, count)) = *self.landscape_preview_mesh.borrow() {
            return Some((vao, count));
        }
        if !gl::GenVertexArrays::is_loaded() {
            return None;
        }
        let data = geometry();
        let mut vao = 0u32;
        let mut vbo = 0u32;
        unsafe {
            gl::GenVertexArrays(1, &mut vao);
            gl::GenBuffers(1, &mut vbo);
            if vao == 0 || vbo == 0 {
                if vao != 0 {
                    gl::DeleteVertexArrays(1, &vao);
                }
                if vbo != 0 {
                    gl::DeleteBuffers(1, &vbo);
                }
                return None;
            }
            gl::BindVertexArray(vao);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (data.len() * std::mem::size_of::<f32>()) as isize,
                data.as_ptr().cast(),
                gl::STATIC_DRAW,
            );
            let stride = (12 * std::mem::size_of::<f32>()) as i32;
            for (index, amount, start) in [(0, 3, 0), (1, 3, 3), (2, 4, 6), (3, 2, 10)] {
                gl::VertexAttribPointer(
                    index,
                    amount,
                    gl::FLOAT,
                    gl::FALSE,
                    stride,
                    (start * std::mem::size_of::<f32>()) as *const _,
                );
                gl::EnableVertexAttribArray(index);
            }
            gl::BindVertexArray(0);
            gl::BindBuffer(gl::ARRAY_BUFFER, 0);
        }
        let result = (vao, vbo, (data.len() / 12) as i32);
        *self.landscape_preview_mesh.borrow_mut() = Some(result);
        Some((vao, result.2))
    }
}
#[cfg(test)]
mod tests {
    #[test]
    fn landscape_patch_is_triangles_with_nonflat_normals() {
        let data = super::geometry();
        assert_eq!(data.len(), 12 * 12 * 6 * 12);
        assert!(
            data.chunks_exact(12)
                .any(|v| v[3].abs() > 0.05 || v[5].abs() > 0.05)
        );
        assert!(data.chunks_exact(12).all(|v| v[4] > 0.3));
    }
}
