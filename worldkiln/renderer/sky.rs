use crate::renderer::Renderer;
// Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
use crate::renderer::mesh::upload_block_vertices_3d;
use crate::renderer::resources::get_uniform_location;
use crate::renderer::shader::create_program;
use crate::renderer::terrain::{UNDERWATER_SKY_BLEND, UnderwaterState};
use crate::world::World;
use glam::{Mat4, Vec3};

pub struct SkyRenderer {
    pub(crate) program: u32,
    pub(crate) vao: u32,
    pub(crate) vbo: u32,
    pub(crate) view_projection_location: i32,
    pub(crate) model_location: i32,
    pub(crate) cubemap_location: i32,
    pub(crate) underwater_enabled_location: i32,
    pub(crate) underwater_color_location: i32,
    pub(crate) underwater_blend_location: i32,
    pub(crate) atmosphere_enabled_location: i32,
    pub(crate) fog_color_location: i32,
    pub(crate) horizon_strength_location: i32,
}

impl SkyRenderer {
    pub fn new() -> Self {
        let program = create_program(SKY_VERTEX_SHADER, SKY_FRAGMENT_SHADER);

        let mut vertices = Vec::new();
        // A unit cube to sample the cubemap texture.

        // Right face (+X)
        add_face(
            &mut vertices,
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
        );
        // Left face (-X)
        add_face(
            &mut vertices,
            [-0.5, -0.5, 0.5],
            [-0.5, -0.5, -0.5],
            [-0.5, 0.5, -0.5],
            [-0.5, 0.5, 0.5],
        );
        // Top face (+Y)
        add_face(
            &mut vertices,
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        );
        // Bottom face (-Y)
        add_face(
            &mut vertices,
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
        );
        // Front face (-Z)
        add_face(
            &mut vertices,
            [-0.5, -0.5, -0.5],
            [0.5, -0.5, -0.5],
            [0.5, 0.5, -0.5],
            [-0.5, 0.5, -0.5],
        );
        // Back face (+Z)
        add_face(
            &mut vertices,
            [0.5, -0.5, 0.5],
            [-0.5, -0.5, 0.5],
            [-0.5, 0.5, 0.5],
            [0.5, 0.5, 0.5],
        );

        let (vao, vbo, _) = upload_block_vertices_3d(&vertices);

        // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
        Self {
            program,
            vao,
            vbo,
            view_projection_location: get_uniform_location(program, "u_view_projection"),
            model_location: get_uniform_location(program, "u_model"),
            cubemap_location: get_uniform_location(program, "u_cubemap"),
            underwater_enabled_location: get_uniform_location(program, "u_underwater_enabled"),
            underwater_color_location: get_uniform_location(program, "u_underwater_color"),
            underwater_blend_location: get_uniform_location(program, "u_underwater_blend"),
            atmosphere_enabled_location: get_uniform_location(program, "u_atmosphere_enabled"),
            fog_color_location: get_uniform_location(program, "u_fog_color"),
            horizon_strength_location: get_uniform_location(program, "u_horizon_strength"),
        }
    }

    pub fn render(
        &self,
        renderer: &Renderer,
        world: &World,
        camera_pos: Vec3,
        view_projection: Mat4,
        underwater: Option<UnderwaterState>,
    ) {
        if !world.sky.enabled {
            return;
        }

        unsafe {
            gl::Disable(gl::CULL_FACE);
            gl::DepthMask(gl::FALSE);

            gl::UseProgram(self.program);
            gl::UniformMatrix4fv(
                self.view_projection_location,
                1,
                gl::FALSE,
                view_projection.to_cols_array().as_ptr(),
            );

            // Render a cube centered on camera.
            let model = Mat4::from_translation(camera_pos);
            gl::UniformMatrix4fv(
                self.model_location,
                1,
                gl::FALSE,
                model.to_cols_array().as_ptr(),
            );

            gl::BindVertexArray(self.vao);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::Uniform1i(self.cubemap_location, 0);

            let underwater_enabled = underwater.is_some();
            gl::Uniform1i(
                self.underwater_enabled_location,
                i32::from(underwater_enabled),
            );
            if let Some(state) = underwater {
                gl::Uniform3f(
                    self.underwater_color_location,
                    state.color.x,
                    state.color.y,
                    state.color.z,
                );
                gl::Uniform1f(self.underwater_blend_location, UNDERWATER_SKY_BLEND);
            } else {
                gl::Uniform3f(self.underwater_color_location, 0.0, 0.0, 0.0);
                gl::Uniform1f(self.underwater_blend_location, 0.0);
            }

            // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
            let atmosphere_enabled = world.sky.atmosphere_enabled && !underwater_enabled;
            let fog_color = world.sky.fog_color.clamp(Vec3::ZERO, Vec3::ONE);
            gl::Uniform1i(
                self.atmosphere_enabled_location,
                i32::from(atmosphere_enabled),
            );
            gl::Uniform3f(
                self.fog_color_location,
                fog_color.x,
                fog_color.y,
                fog_color.z,
            );
            gl::Uniform1f(
                self.horizon_strength_location,
                world.sky.horizon_strength.clamp(0.0, 1.0),
            );

            let tex = renderer.get_cubemap(&world.sky.texture);
            if tex != 0 {
                gl::BindTexture(gl::TEXTURE_CUBE_MAP, tex);
                gl::DrawArrays(gl::TRIANGLES, 0, 36);
            }

            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::CULL_FACE);
        }
    }
}

fn add_face(vertices: &mut Vec<f32>, v1: [f32; 3], v2: [f32; 3], v3: [f32; 3], v4: [f32; 3]) {
    let normal = [0.0, 0.0, 0.0];
    let color = [1.0, 1.0, 1.0, 1.0];
    let dummy_uv = [0.0, 0.0];

    // Triangle 1
    vertices.extend_from_slice(&v1);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v2);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v3);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);

    // Triangle 2
    vertices.extend_from_slice(&v1);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v3);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
    vertices.extend_from_slice(&v4);
    vertices.extend_from_slice(&normal);
    vertices.extend_from_slice(&color);
    vertices.extend_from_slice(&dummy_uv);
}

const SKY_VERTEX_SHADER: &str = r#"
#version 330 core
layout (location = 0) in vec3 a_position;

uniform mat4 u_view_projection;
uniform mat4 u_model;

out vec3 v_pos;

void main() {
    v_pos = a_position;
    gl_Position = u_view_projection * u_model * vec4(a_position, 1.0);
}
"#;

const SKY_FRAGMENT_SHADER: &str = r#"
#version 330 core
in vec3 v_pos;
uniform samplerCube u_cubemap;
// Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
uniform bool u_underwater_enabled;
uniform vec3 u_underwater_color;
uniform float u_underwater_blend;
// Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
uniform bool u_atmosphere_enabled;
uniform vec3 u_fog_color;
uniform float u_horizon_strength;
out vec4 FragColor;

void main() {
    vec4 sky = texture(u_cubemap, v_pos);
    if (u_atmosphere_enabled) {
        vec3 direction = normalize(v_pos);
        float horizon = 1.0 - clamp(abs(direction.y), 0.0, 1.0);
        horizon = horizon * horizon;
        float atmosphere = clamp(horizon * u_horizon_strength, 0.0, 1.0);
        sky.rgb = mix(sky.rgb, u_fog_color, atmosphere);
    }
    if (u_underwater_enabled) {
        sky.rgb = mix(sky.rgb, u_underwater_color, clamp(u_underwater_blend, 0.0, 1.0));
    }
    FragColor = sky;
}
"#;
