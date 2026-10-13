use super::{Renderer, mesh::VERTEX_3D_FLOATS};
use crate::engine::particles::ParticleSystem;
use glam::{Mat4, Vec3};

pub(super) struct ParticleRenderer {
    program: u32,
    view_projection: i32,
}

impl ParticleRenderer {
    pub(super) fn new() -> Self {
        let program = super::shader::create_program(VERTEX_SHADER, FRAGMENT_SHADER);
        Self {
            program,
            view_projection: super::resources::get_uniform_location(program, "u_view_projection"),
        }
    }
}

impl Drop for ParticleRenderer {
    fn drop(&mut self) {
        unsafe {
            gl::DeleteProgram(self.program);
        }
    }
}

impl Renderer {
    pub(super) fn render_particles(&self, system: &ParticleSystem, view: Mat4, projection: Mat4) {
        let Some(renderer) = &self.particle_renderer else {
            return;
        };
        let vertices = billboard_vertices(system, view);
        if vertices.is_empty() {
            return;
        }
        unsafe {
            gl::UseProgram(renderer.program);
            gl::UniformMatrix4fv(
                renderer.view_projection,
                1,
                gl::FALSE,
                (projection * view).to_cols_array().as_ptr(),
            );
            gl::Enable(gl::DEPTH_TEST);
            // Smoke and flames occlude against the scene, but never write depth
            // over another translucent particle. Geometry is sorted back to front.
            gl::DepthMask(gl::FALSE);
            gl::Disable(gl::CULL_FACE);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            self.draw_character_mesh_persistent(&vertices);
            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::CULL_FACE);
        }
    }
}

fn billboard_vertices(system: &ParticleSystem, view: Mat4) -> Vec<f32> {
    let right = Vec3::new(view.x_axis.x, view.y_axis.x, view.z_axis.x);
    let up = Vec3::new(view.x_axis.y, view.y_axis.y, view.z_axis.y);
    let mut particles: Vec<_> = system.particles().iter().collect();
    particles.sort_by(|a, b| {
        view.transform_point3(a.position())
            .z
            .total_cmp(&view.transform_point3(b.position()).z)
    });
    let mut vertices = Vec::with_capacity(particles.len() * 6 * VERTEX_3D_FLOATS);
    for particle in particles {
        let center = particle.position();
        let half = particle.size() * 0.5;
        let color = particle.color();
        for [x, y] in [
            [-1.0, -1.0],
            [1.0, -1.0],
            [1.0, 1.0],
            [-1.0, -1.0],
            [1.0, 1.0],
            [-1.0, 1.0],
        ] {
            let position = center + (right * x + up * y) * half;
            // The existing shared dynamic vertex buffer supplies local coordinates
            // in the normal slot; this unlit shader doesn't consume mesh normals.
            vertices.extend_from_slice(&[
                position.x, position.y, position.z, x, y, 0.0, color.x, color.y, color.z, color.w,
            ]);
        }
    }
    vertices
}

const VERTEX_SHADER: &str = r#"#version 330 core
layout(location = 0) in vec3 a_position;
layout(location = 1) in vec3 a_local;
layout(location = 2) in vec4 a_color;
uniform mat4 u_view_projection;
out vec2 v_local;
out vec4 v_color;
void main() {
    gl_Position = u_view_projection * vec4(a_position, 1.0);
    v_local = a_local.xy;
    v_color = a_color;
}
"#;

const FRAGMENT_SHADER: &str = r#"#version 330 core
in vec2 v_local;
in vec4 v_color;
out vec4 FragColor;
void main() {
    float radius = length(v_local);
    float opacity = 1.0 - smoothstep(0.15, 1.0, radius);
    if (opacity < 0.001) discard;
    FragColor = vec4(v_color.rgb, v_color.a * opacity);
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::particles::BurstConfig;

    #[test]
    fn particle_billboards_face_camera_and_sort_back_to_front() {
        let mut system = ParticleSystem::default();
        let config = BurstConfig::preset("smoke").unwrap();
        system
            .burst(Vec3::new(0.0, 0.0, -2.0), 1, config.clone())
            .unwrap();
        system.burst(Vec3::new(0.0, 0.0, -8.0), 1, config).unwrap();
        let vertices = billboard_vertices(&system, Mat4::IDENTITY);
        assert_eq!(vertices.len(), 12 * VERTEX_3D_FLOATS);
        assert_eq!(vertices[2], -8.0);
        assert_eq!(vertices[6 * VERTEX_3D_FLOATS + 2], -2.0);
        let view = Mat4::look_at_rh(Vec3::X * 5.0, Vec3::ZERO, Vec3::Y);
        let vertices = billboard_vertices(&system, view);
        assert!((vertices[0] - vertices[VERTEX_3D_FLOATS]).abs() < 1e-6);
    }
}
