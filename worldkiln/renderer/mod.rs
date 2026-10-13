//! Renderer ownership and OpenGL resource initialization.
//!
//! Owns the OpenGL programs, buffers, textures, framebuffers, cached chunk
//! meshes, cached editor ghost data, and other GPU resources used by the
//! engine renderer. The renderer reads engine state and produces graphics;
//! it does not own or modify the World or Editor.

pub mod camera;
pub mod capabilities;
pub mod character_render;
pub mod chunk;
pub mod chunk_cache;
pub mod distance;
#[cfg(feature = "editor")]
pub mod editor_render;
pub mod game_render;
mod gi_cache;
mod gi_scene;
#[cfg(feature = "editor")]
pub mod home_render;
pub mod mesh;
mod particles;
mod profile;
// Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
#[cfg(feature = "editor")]
mod editor_profile;
pub mod resources;
pub mod settings;
pub mod shader;
pub mod sky;
// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
pub mod terrain;

use std::cell::RefCell;
use std::collections::HashMap;
use std::ffi::CString;
use std::time::Instant;

use glam::{Mat4, Vec3};

#[cfg(feature = "editor")]
use crate::editor::GridPlane;

#[cfg(not(feature = "editor"))]
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum GridPlane {
    Xz,
    Yz,
    Xy,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RenderMode {
    Editor,
    Game,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct LightingPerformanceSnapshot {
    pub updated_probes: usize,
    pub traced_rays: usize,
    pub cache_probes: usize,
    pub scene_voxels: usize,
    pub scene_payload_bytes: usize,
    pub cache_update_cpu_ms: f32,
    pub dynamic_aabbs: usize,
}
use crate::world::{CellType, ChunkCoord, World};

use self::capabilities::GraphicsCapabilities;
use self::chunk::{ChunkMesh, GhostChunkMesh};
use self::gi_cache::GiRadianceCache;
use self::resources::{
    create_anchor_marker, create_axes, create_billboard_vao, create_character_vao_vbo,
    create_grid_plane_vao, create_highlight_box, get_uniform_location, load_texture_from_file,
};
use self::settings::{ApplyBehavior, ApplyResult, RendererLightingSettings, RendererLightingState};
use self::shader::create_program;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct PointLightUniformLocations {
    pub(super) position: i32,
    pub(super) color: i32,
    pub(super) intensity: i32,
    pub(super) range: i32,
}

/// Owns the OpenGL resources required by the engine renderer.
///
/// The renderer reads engine state and produces graphics. It does not own or
/// modify the World or Editor.
pub struct Renderer {
    capabilities: GraphicsCapabilities,
    lighting_state: RefCell<RendererLightingState>,
    pub(super) grid_program: u32,
    // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
    pub(super) water_program: u32,
    pub(super) water_started: Instant,
    // Worldkiln 0.9.3 | WO 0.9.3-15 | 2026-10-07
    pub(super) water_view_projection_location: i32,
    pub(super) water_camera_position_location: i32,
    pub(super) water_time_location: i32,
    pub(super) water_camera_underwater_location: i32,
    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    pub(super) water_wave_height_location: i32,
    pub(super) water_wave_speed_location: i32,
    pub(super) water_foam_amount_location: i32,
    pub(super) water_foam_scale_location: i32,
    // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
    pub(super) water_atmosphere_enabled_location: i32,
    pub(super) water_fog_color_location: i32,
    pub(super) water_fog_start_location: i32,
    pub(super) water_fog_end_location: i32,
    pub(super) ghost_program: u32,
    particle_renderer: Option<particles::ParticleRenderer>,

    pub(super) ghost_view_projection_location: i32,
    pub(super) ghost_model_location: i32,
    pub(super) ghost_color_location: i32,
    pub(super) ghost_alpha_location: i32,

    pub(super) grid_view_projection_location: i32,
    pub(super) grid_model_location: i32,
    pub(super) grid_base_color_location: i32,
    pub(super) grid_alpha_location: i32,
    pub(super) grid_use_texture_location: i32,
    pub(super) grid_ambient_intensity_location: i32,
    pub(super) grid_global_light_enabled_location: i32,
    pub(super) grid_global_light_direction_location: i32,
    pub(super) grid_global_light_color_location: i32,
    pub(super) grid_global_light_intensity_location: i32,
    pub(super) grid_light_space_matrix_location: i32,
    pub(super) grid_shadows_enabled_location: i32,
    pub(super) grid_shadow_map_location: i32,
    pub(super) grid_debug_view_location: i32,
    pub(super) grid_lighting_method_location: i32,
    pub(super) grid_indirect_enabled_location: i32,
    pub(super) grid_gi_cache_location: i32,
    pub(super) grid_gi_origin_location: i32,
    pub(super) grid_gi_spacing_location: i32,
    pub(super) grid_gi_dimensions_location: i32,
    pub(super) grid_gi_denoising_location: i32,
    // Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
    pub(super) grid_underwater_enabled_location: i32,
    pub(super) grid_underwater_camera_position_location: i32,
    pub(super) grid_underwater_color_location: i32,
    pub(super) grid_underwater_density_location: i32,
    pub(super) grid_underwater_surface_y_location: i32,
    // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
    pub(super) grid_atmosphere_enabled_location: i32,
    pub(super) grid_atmosphere_camera_position_location: i32,
    pub(super) grid_fog_color_location: i32,
    pub(super) grid_fog_start_location: i32,
    pub(super) grid_fog_end_location: i32,
    // Worldkiln 0.9.3 | WO 0.9.3-20 | Smooth Terrain layer blending
    pub(super) grid_terrain_blend_enabled_location: i32,
    pub(super) grid_terrain_source_role_location: i32,
    pub(super) grid_terrain_water_level_location: i32,
    pub(super) grid_point_light_count_location: i32,
    pub(super) grid_point_light_uniforms: [PointLightUniformLocations; 16],

    pub(super) shadow_light_space_matrix_location: i32,
    pub(super) shadow_model_location: i32,

    pub(super) grid_vao_xz: u32,
    pub(super) grid_vbo_xz: u32,
    pub(super) grid_count_xz: i32,

    pub(super) grid_vao_yz: u32,
    pub(super) grid_vbo_yz: u32,
    pub(super) grid_count_yz: i32,

    pub(super) grid_vao_xy: u32,
    pub(super) grid_vbo_xy: u32,
    pub(super) grid_count_xy: i32,

    pub(super) axis_vao: u32,
    pub(super) axis_vbo: u32,
    pub(super) axis_vertex_count: i32,

    pub(super) highlight_vao: u32,
    pub(super) highlight_vbo: u32,
    pub(super) highlight_vertex_count: i32,

    pub(super) anchor_vao: u32,
    pub(super) anchor_vbo: u32,
    pub(super) anchor_vertex_count: i32,

    pub(super) block_vao: u32,
    pub(super) block_vbo: u32,
    pub(super) block_mask_ranges: [(i32, i32); 64],

    pub(super) chunk_cache: RefCell<HashMap<ChunkCoord, ChunkMesh>>,
    pub(super) terrain_cache: RefCell<Option<terrain::TerrainGpuMesh>>,
    pub(super) last_render_revision: RefCell<u64>,
    pub(super) last_render_mode: RefCell<Option<RenderMode>>,

    pub(super) ghost_cache: RefCell<HashMap<ChunkCoord, GhostChunkMesh>>,
    pub(super) last_ghost_revision: RefCell<u64>,
    pub(super) last_ghost_mode: RefCell<Option<RenderMode>>,

    pub(super) billboard_vao: u32,
    pub(super) billboard_vbo: u32,

    pub(super) character_vao: u32,
    pub(super) character_vbo: u32,
    pub(super) entity_mesh_cache: RefCell<HashMap<String, StaticEntityGpuMesh>>,

    pub(super) shadow_program: u32,
    pub(super) shadow_fbo: u32,
    pub(super) shadow_depth_tex: u32,

    pub(super) textures: RefCell<HashMap<String, u32>>,
    pub(super) project_assets_dir: RefCell<Option<std::path::PathBuf>>,
    pub(super) cubemaps: RefCell<HashMap<String, u32>>,
    pub(super) fallback_tex: u32,

    pub(super) sky_renderer: sky::SkyRenderer,
    pub(super) profiler: RefCell<Option<profile::RenderProfiler>>,
    // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
    #[cfg(feature = "editor")]
    pub(super) editor_profiler: RefCell<Option<editor_profile::EditorProfiler>>,
    gi_cache: RefCell<GiRadianceCache>,

    pub(super) width: f32,
    pub(super) height: f32,
}

pub(super) const SHADOW_RES: i32 = 2048;
pub(super) const SHADOW_ORTHO_HALF_EXTENT: f32 = 30.0;
pub(super) const SHADOW_NEAR: f32 = 0.1;
pub(super) const SHADOW_FAR: f32 = 100.0;
pub(super) const SHADOW_LIGHT_DISTANCE: f32 = 50.0;

impl Renderer {
    // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
    #[cfg(feature = "editor")]
    pub fn record_editor_host_frame(&self, host_ms: [f64; 5]) {
        if let Some(p) = self.editor_profiler.borrow_mut().as_mut() {
            p.host_frame(host_ms);
        }
    }

    #[cfg(feature = "editor")]
    pub fn editor_profile_summary(&self) -> Option<String> {
        self.editor_profiler
            .borrow()
            .as_ref()
            .and_then(|p| p.summary())
    }

    // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:03 -05:00
    #[cfg(feature = "editor")]
    pub fn record_editor_hover_call(&self, millis: f64) {
        if let Some(p) = self.editor_profiler.borrow_mut().as_mut() {
            p.hover_call(millis);
        }
    }

    pub fn new(width: f32, height: f32) -> Self {
        unsafe {
            gl::Enable(gl::DEPTH_TEST);
            gl::DepthFunc(gl::LESS);

            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
            gl::FrontFace(gl::CCW);

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);

            gl::LineWidth(1.0);
        }

        let grid_program = create_program(GRID_VERTEX_SHADER, GRID_FRAGMENT_SHADER);
        // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
        let water_program =
            create_program(terrain::WATER_VERTEX_SHADER, terrain::WATER_FRAGMENT_SHADER);

        let capabilities = GraphicsCapabilities::detect();
        if std::env::var_os("AEO_RENDER_CAPS").is_some() {
            capabilities.log();
        }

        let ghost_program = create_program(GHOST_VERTEX_SHADER, GHOST_FRAGMENT_SHADER);

        let shadow_program = create_program(SHADOW_VERTEX_SHADER, SHADOW_FRAGMENT_SHADER);

        // Cache water uniform locations once. The water pass used to query
        // these through the OpenGL driver every frame.
        let water_view_projection_location =
            get_uniform_location(water_program, "u_view_projection");
        let water_camera_position_location =
            get_uniform_location(water_program, "u_camera_position");
        let water_time_location = get_uniform_location(water_program, "u_time");
        let water_camera_underwater_location =
            get_uniform_location(water_program, "u_camera_underwater");
        let water_wave_height_location = get_uniform_location(water_program, "u_wave_height");
        let water_wave_speed_location = get_uniform_location(water_program, "u_wave_speed");
        let water_foam_amount_location = get_uniform_location(water_program, "u_foam_amount");
        let water_foam_scale_location = get_uniform_location(water_program, "u_foam_scale");
        let water_atmosphere_enabled_location =
            get_uniform_location(water_program, "u_atmosphere_enabled");
        let water_fog_color_location = get_uniform_location(water_program, "u_fog_color");
        let water_fog_start_location = get_uniform_location(water_program, "u_fog_start");
        let water_fog_end_location = get_uniform_location(water_program, "u_fog_end");

        let grid_view_projection_location = get_uniform_location(grid_program, "u_view_projection");
        let grid_model_location = get_uniform_location(grid_program, "u_model");
        let grid_base_color_location = get_uniform_location(grid_program, "u_base_color");
        let grid_alpha_location = get_uniform_location(grid_program, "u_alpha");
        let grid_use_texture_location = get_uniform_location(grid_program, "u_use_texture");
        let grid_ambient_intensity_location =
            get_uniform_location(grid_program, "u_ambient_intensity");
        let grid_global_light_enabled_location =
            get_uniform_location(grid_program, "u_global_light_enabled");
        let grid_global_light_direction_location =
            get_uniform_location(grid_program, "u_global_light_direction");
        let grid_global_light_color_location =
            get_uniform_location(grid_program, "u_global_light_color");
        let grid_global_light_intensity_location =
            get_uniform_location(grid_program, "u_global_light_intensity");
        let grid_light_space_matrix_location =
            get_uniform_location(grid_program, "u_light_space_matrix");
        let grid_shadows_enabled_location = get_uniform_location(grid_program, "u_shadows_enabled");
        let grid_shadow_map_location = get_uniform_location(grid_program, "u_shadow_map");
        let grid_debug_view_location = get_uniform_location(grid_program, "u_debug_view");
        let grid_lighting_method_location = get_uniform_location(grid_program, "u_lighting_method");
        let grid_indirect_enabled_location =
            get_uniform_location(grid_program, "u_indirect_enabled");
        let grid_gi_cache_location = get_uniform_location(grid_program, "u_gi_cache");
        let grid_gi_origin_location = get_uniform_location(grid_program, "u_gi_origin");
        let grid_gi_spacing_location = get_uniform_location(grid_program, "u_gi_spacing");
        let grid_gi_dimensions_location = get_uniform_location(grid_program, "u_gi_dimensions");
        let grid_gi_denoising_location = get_uniform_location(grid_program, "u_gi_denoising");
        // Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
        let grid_underwater_enabled_location =
            get_uniform_location(grid_program, "u_underwater_enabled");
        let grid_underwater_camera_position_location =
            get_uniform_location(grid_program, "u_underwater_camera_position");
        let grid_underwater_color_location =
            get_uniform_location(grid_program, "u_underwater_color");
        let grid_underwater_density_location =
            get_uniform_location(grid_program, "u_underwater_density");
        let grid_underwater_surface_y_location =
            get_uniform_location(grid_program, "u_underwater_surface_y");
        let grid_atmosphere_enabled_location =
            get_uniform_location(grid_program, "u_atmosphere_enabled");
        let grid_atmosphere_camera_position_location =
            get_uniform_location(grid_program, "u_atmosphere_camera_position");
        let grid_fog_color_location = get_uniform_location(grid_program, "u_fog_color");
        let grid_fog_start_location = get_uniform_location(grid_program, "u_fog_start");
        let grid_fog_end_location = get_uniform_location(grid_program, "u_fog_end");
        let grid_terrain_blend_enabled_location =
            get_uniform_location(grid_program, "u_terrain_blend_enabled");
        let grid_terrain_source_role_location =
            get_uniform_location(grid_program, "u_terrain_source_role");
        let grid_terrain_water_level_location =
            get_uniform_location(grid_program, "u_terrain_water_level");
        let grid_point_light_count_location =
            get_uniform_location(grid_program, "u_point_light_count");

        let grid_point_light_uniforms = std::array::from_fn(|i| {
            let base = format!("u_point_lights[{i}]");

            PointLightUniformLocations {
                position: get_uniform_location(grid_program, &format!("{base}.position")),
                color: get_uniform_location(grid_program, &format!("{base}.color")),
                intensity: get_uniform_location(grid_program, &format!("{base}.intensity")),
                range: get_uniform_location(grid_program, &format!("{base}.range")),
            }
        });

        let shadow_light_space_matrix_location =
            get_uniform_location(shadow_program, "u_light_space_matrix");
        let shadow_model_location = get_uniform_location(shadow_program, "u_model");

        let ghost_view_projection_location =
            get_uniform_location(ghost_program, "u_view_projection");
        let ghost_model_location = get_uniform_location(ghost_program, "u_model");
        let ghost_color_location = get_uniform_location(ghost_program, "u_color");
        let ghost_alpha_location = get_uniform_location(ghost_program, "u_alpha");

        let (grid_vao_xz, grid_vbo_xz, grid_count_xz) = create_grid_plane_vao(GridPlane::Xz);

        let (grid_vao_yz, grid_vbo_yz, grid_count_yz) = create_grid_plane_vao(GridPlane::Yz);

        let (grid_vao_xy, grid_vbo_xy, grid_count_xy) = create_grid_plane_vao(GridPlane::Xy);

        let (axis_vao, axis_vbo, axis_vertex_count) = create_axes();

        let (highlight_vao, highlight_vbo, highlight_vertex_count) = create_highlight_box();

        let (anchor_vao, anchor_vbo, anchor_vertex_count) = create_anchor_marker();

        let (block_vao, block_vbo, block_mask_ranges) = self::mesh::create_block_masks();

        let (billboard_vao, billboard_vbo) = create_billboard_vao();

        let (character_vao, character_vbo) = create_character_vao_vbo();

        let mut shadow_fbo = 0;
        let mut shadow_depth_tex = 0;

        let mut textures = HashMap::new();
        let fallback_tex;

        unsafe {
            gl::GenFramebuffers(1, &mut shadow_fbo);
            gl::GenTextures(1, &mut shadow_depth_tex);

            gl::BindTexture(gl::TEXTURE_2D, shadow_depth_tex);

            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::DEPTH_COMPONENT as i32,
                SHADOW_RES,
                SHADOW_RES,
                0,
                gl::DEPTH_COMPONENT,
                gl::FLOAT,
                std::ptr::null(),
            );

            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);

            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);

            gl::TexParameteri(
                gl::TEXTURE_2D,
                gl::TEXTURE_WRAP_S,
                gl::CLAMP_TO_BORDER as i32,
            );

            gl::TexParameteri(
                gl::TEXTURE_2D,
                gl::TEXTURE_WRAP_T,
                gl::CLAMP_TO_BORDER as i32,
            );

            let border_color = [1.0, 1.0, 1.0, 1.0];

            gl::TexParameterfv(
                gl::TEXTURE_2D,
                gl::TEXTURE_BORDER_COLOR,
                border_color.as_ptr(),
            );

            gl::BindFramebuffer(gl::FRAMEBUFFER, shadow_fbo);

            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                gl::TEXTURE_2D,
                shadow_depth_tex,
                0,
            );

            gl::DrawBuffer(gl::NONE);
            gl::ReadBuffer(gl::NONE);

            let framebuffer_status = gl::CheckFramebufferStatus(gl::FRAMEBUFFER);

            assert_eq!(
                framebuffer_status,
                gl::FRAMEBUFFER_COMPLETE,
                "shadow framebuffer is incomplete: 0x{framebuffer_status:04X}"
            );

            gl::BindFramebuffer(gl::FRAMEBUFFER, 0);

            // 1x1 white fallback texture.
            let mut fallback = 0;

            gl::GenTextures(1, &mut fallback);
            gl::BindTexture(gl::TEXTURE_2D, fallback);

            let white_data = [255u8, 255, 255, 255];

            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA as i32,
                1,
                1,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                white_data.as_ptr() as *const _,
            );

            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);

            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);

            gl::BindTexture(gl::TEXTURE_2D, 0);

            fallback_tex = fallback;

            textures.insert("Block_tx".to_string(), fallback_tex);
        }

        if let Some(tex) = load_texture_from_file(".assets/textures/brick.png") {
            textures.insert("brick".to_string(), tex);
        }

        let renderer = Self {
            capabilities,
            lighting_state: RefCell::new(RendererLightingState::default()),
            grid_program,
            water_program,
            water_started: Instant::now(),
            water_view_projection_location,
            water_camera_position_location,
            water_time_location,
            water_camera_underwater_location,
            water_wave_height_location,
            water_wave_speed_location,
            water_foam_amount_location,
            water_foam_scale_location,
            water_atmosphere_enabled_location,
            water_fog_color_location,
            water_fog_start_location,
            water_fog_end_location,
            ghost_program,
            particle_renderer: Some(particles::ParticleRenderer::new()),

            ghost_view_projection_location,
            ghost_model_location,
            ghost_color_location,
            ghost_alpha_location,

            grid_view_projection_location,
            grid_model_location,
            grid_base_color_location,
            grid_alpha_location,
            grid_use_texture_location,
            grid_ambient_intensity_location,
            grid_global_light_enabled_location,
            grid_global_light_direction_location,
            grid_global_light_color_location,
            grid_global_light_intensity_location,
            grid_light_space_matrix_location,
            grid_shadows_enabled_location,
            grid_shadow_map_location,
            grid_debug_view_location,
            grid_lighting_method_location,
            grid_indirect_enabled_location,
            grid_gi_cache_location,
            grid_gi_origin_location,
            grid_gi_spacing_location,
            grid_gi_dimensions_location,
            grid_gi_denoising_location,
            grid_underwater_enabled_location,
            grid_underwater_camera_position_location,
            grid_underwater_color_location,
            grid_underwater_density_location,
            grid_underwater_surface_y_location,
            grid_atmosphere_enabled_location,
            grid_atmosphere_camera_position_location,
            grid_fog_color_location,
            grid_fog_start_location,
            grid_fog_end_location,
            grid_terrain_blend_enabled_location,
            grid_terrain_source_role_location,
            grid_terrain_water_level_location,
            grid_point_light_count_location,
            grid_point_light_uniforms,

            shadow_light_space_matrix_location,
            shadow_model_location,

            grid_vao_xz,
            grid_vbo_xz,
            grid_count_xz,

            grid_vao_yz,
            grid_vbo_yz,
            grid_count_yz,

            grid_vao_xy,
            grid_vbo_xy,
            grid_count_xy,

            axis_vao,
            axis_vbo,
            axis_vertex_count,

            highlight_vao,
            highlight_vbo,
            highlight_vertex_count,

            anchor_vao,
            anchor_vbo,
            anchor_vertex_count,

            block_vao,
            block_vbo,
            block_mask_ranges,

            chunk_cache: RefCell::new(HashMap::new()),
            terrain_cache: RefCell::new(None),
            last_render_revision: RefCell::new(0),
            last_render_mode: RefCell::new(None),

            ghost_cache: RefCell::new(HashMap::new()),
            last_ghost_revision: RefCell::new(0),
            last_ghost_mode: RefCell::new(None),

            billboard_vao,
            billboard_vbo,

            character_vao,
            character_vbo,
            entity_mesh_cache: RefCell::new(HashMap::new()),

            shadow_program,
            shadow_fbo,
            shadow_depth_tex,

            textures: RefCell::new(textures),
            project_assets_dir: RefCell::new(None),
            cubemaps: RefCell::new(HashMap::new()),
            fallback_tex,

            sky_renderer: sky::SkyRenderer::new(),
            profiler: RefCell::new(profile::RenderProfiler::new()),
            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            #[cfg(feature = "editor")]
            editor_profiler: RefCell::new(editor_profile::EditorProfiler::new()),
            gi_cache: RefCell::new(GiRadianceCache::default()),

            width: width.max(1.0),
            height: height.max(1.0),
        };

        unsafe {
            gl::UseProgram(grid_program);

            let tex_name = CString::new("u_texture").unwrap();

            let tex_location = gl::GetUniformLocation(grid_program, tex_name.as_ptr());

            if tex_location != -1 {
                gl::Uniform1i(tex_location, 1);
            }

            // WO 0.9.3-20: Smooth Terrain blends the four built-in layer
            // textures in shader without changing the mesh/storage contract.
            let surface_sampler = get_uniform_location(grid_program, "u_terrain_surface_texture");
            let subsurface_sampler =
                get_uniform_location(grid_program, "u_terrain_subsurface_texture");
            let rock_sampler = get_uniform_location(grid_program, "u_terrain_rock_texture");
            let sand_sampler = get_uniform_location(grid_program, "u_terrain_sand_texture");
            gl::Uniform1i(surface_sampler, 3);
            gl::Uniform1i(subsurface_sampler, 4);
            gl::Uniform1i(rock_sampler, 5);
            gl::Uniform1i(sand_sampler, 6);
            gl::Uniform1i(renderer.grid_terrain_blend_enabled_location, 0);

            gl::Uniform1i(renderer.grid_gi_cache_location, 2);

            gl::UseProgram(water_program);
            let water_sky = CString::new("u_sky").expect("water sampler uniform name is static");
            let water_sky_location = gl::GetUniformLocation(water_program, water_sky.as_ptr());
            if water_sky_location != -1 {
                gl::Uniform1i(water_sky_location, 0);
            }

            gl::UseProgram(0);
        }

        renderer
    }

    /// Creates a renderer with no OpenGL resources for unit tests.
    ///
    /// This constructor must remain free of OpenGL calls so application-state
    /// tests can construct an App without creating a graphics context.
    #[cfg(test)]
    pub(crate) fn new_for_tests(width: f32, height: f32) -> Self {
        Self {
            capabilities: GraphicsCapabilities::default(),
            lighting_state: RefCell::new(RendererLightingState::default()),
            grid_program: 0,
            water_program: 0,
            water_started: Instant::now(),
            water_view_projection_location: -1,
            water_camera_position_location: -1,
            water_time_location: -1,
            water_camera_underwater_location: -1,
            water_wave_height_location: -1,
            water_wave_speed_location: -1,
            water_foam_amount_location: -1,
            water_foam_scale_location: -1,
            water_atmosphere_enabled_location: -1,
            water_fog_color_location: -1,
            water_fog_start_location: -1,
            water_fog_end_location: -1,
            ghost_program: 0,
            particle_renderer: None,

            ghost_view_projection_location: -1,
            ghost_model_location: -1,
            ghost_color_location: -1,
            ghost_alpha_location: -1,

            grid_view_projection_location: -1,
            grid_model_location: -1,
            grid_base_color_location: -1,
            grid_alpha_location: -1,
            grid_use_texture_location: -1,
            grid_ambient_intensity_location: -1,
            grid_global_light_enabled_location: -1,
            grid_global_light_direction_location: -1,
            grid_global_light_color_location: -1,
            grid_global_light_intensity_location: -1,
            grid_light_space_matrix_location: -1,
            grid_shadows_enabled_location: -1,
            grid_shadow_map_location: -1,
            grid_debug_view_location: -1,
            grid_lighting_method_location: -1,
            grid_indirect_enabled_location: -1,
            grid_gi_cache_location: -1,
            grid_gi_origin_location: -1,
            grid_gi_spacing_location: -1,
            grid_gi_dimensions_location: -1,
            grid_gi_denoising_location: -1,
            grid_underwater_enabled_location: -1,
            grid_underwater_camera_position_location: -1,
            grid_underwater_color_location: -1,
            grid_underwater_density_location: -1,
            grid_underwater_surface_y_location: -1,
            grid_atmosphere_enabled_location: -1,
            grid_atmosphere_camera_position_location: -1,
            grid_fog_color_location: -1,
            grid_fog_start_location: -1,
            grid_fog_end_location: -1,
            grid_terrain_blend_enabled_location: -1,
            grid_terrain_source_role_location: -1,
            grid_terrain_water_level_location: -1,
            grid_point_light_count_location: -1,

            grid_point_light_uniforms: [PointLightUniformLocations {
                position: -1,
                color: -1,
                intensity: -1,
                range: -1,
            }; 16],

            shadow_light_space_matrix_location: -1,
            shadow_model_location: -1,

            grid_vao_xz: 0,
            grid_vbo_xz: 0,
            grid_count_xz: 0,

            grid_vao_yz: 0,
            grid_vbo_yz: 0,
            grid_count_yz: 0,

            grid_vao_xy: 0,
            grid_vbo_xy: 0,
            grid_count_xy: 0,

            axis_vao: 0,
            axis_vbo: 0,
            axis_vertex_count: 0,

            highlight_vao: 0,
            highlight_vbo: 0,
            highlight_vertex_count: 0,

            anchor_vao: 0,
            anchor_vbo: 0,
            anchor_vertex_count: 0,

            block_vao: 0,
            block_vbo: 0,
            block_mask_ranges: [(0, 0); 64],

            chunk_cache: RefCell::new(HashMap::new()),
            terrain_cache: RefCell::new(None),
            last_render_revision: RefCell::new(0),
            last_render_mode: RefCell::new(None),

            ghost_cache: RefCell::new(HashMap::new()),
            last_ghost_revision: RefCell::new(0),
            last_ghost_mode: RefCell::new(None),

            billboard_vao: 0,
            billboard_vbo: 0,

            character_vao: 0,
            character_vbo: 0,
            entity_mesh_cache: RefCell::new(HashMap::new()),

            shadow_program: 0,
            shadow_fbo: 0,
            shadow_depth_tex: 0,

            textures: RefCell::new(HashMap::new()),
            project_assets_dir: RefCell::new(None),
            cubemaps: RefCell::new(HashMap::new()),
            fallback_tex: 0,

            sky_renderer: sky::SkyRenderer {
                program: 0,
                vao: 0,
                vbo: 0,
                view_projection_location: -1,
                model_location: -1,
                cubemap_location: -1,
                underwater_enabled_location: -1,
                underwater_color_location: -1,
                underwater_blend_location: -1,
                atmosphere_enabled_location: -1,
                fog_color_location: -1,
                horizon_strength_location: -1,
            },
            profiler: RefCell::new(None),
            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            #[cfg(feature = "editor")]
            editor_profiler: RefCell::new(None),
            gi_cache: RefCell::new(GiRadianceCache::default()),

            width: width.max(1.0),
            height: height.max(1.0),
        }
    }

    pub fn resize(&mut self, width: f32, height: f32) {
        self.width = width.max(1.0);
        self.height = height.max(1.0);

        unsafe {
            gl::Viewport(0, 0, self.width as i32, self.height as i32);
        }
    }

    pub fn width(&self) -> f32 {
        self.width
    }

    pub fn height(&self) -> f32 {
        self.height
    }

    pub fn capabilities(&self) -> &GraphicsCapabilities {
        &self.capabilities
    }

    pub fn lighting_state(&self) -> RendererLightingState {
        self.lighting_state.borrow().clone()
    }

    pub fn active_lighting_settings(&self) -> RendererLightingSettings {
        self.lighting_state.borrow().active
    }

    pub fn lighting_performance_snapshot(&self) -> LightingPerformanceSnapshot {
        let stats = self.gi_cache.borrow().stats();
        LightingPerformanceSnapshot {
            updated_probes: stats.updated_probes,
            traced_rays: stats.traced_rays,
            cache_probes: stats.cache_probes,
            scene_voxels: stats.scene_voxels,
            scene_payload_bytes: stats.scene_payload_bytes,
            cache_update_cpu_ms: stats.update_cpu_ms,
            dynamic_aabbs: stats.dynamic_aabbs,
        }
    }

    pub fn request_lighting_settings(&self, settings: RendererLightingSettings) {
        let mut state = self.lighting_state.borrow_mut();
        state.requested = settings;
        state.last_error = None;
    }

    pub fn apply_requested_lighting_settings(&self) -> ApplyResult {
        let mut state = self.lighting_state.borrow_mut();
        if let Some(reason) = state.requested.unsupported_reason() {
            let reason = reason.to_string();
            state.last_error = Some(reason.clone());
            state.restart_pending = false;
            return ApplyResult::Unsupported(reason);
        }
        if state.requested.apply_behavior == ApplyBehavior::RestartRequired {
            state.restart_pending = state.requested != state.active;
            return ApplyResult::RestartRequired;
        }

        state.active = state.requested;
        state.restart_pending = false;
        state.last_error = None;
        ApplyResult::Applied
    }

    pub(super) fn calculate_light_space_matrix(
        &self,
        light_direction: Vec3,
        camera_pos: Vec3,
    ) -> Mat4 {
        let dir = if light_direction.length_squared() > 0.000001 {
            light_direction.normalize()
        } else {
            Vec3::new(-0.5, -1.0, -0.5).normalize()
        };

        let shadow_center = camera_pos;
        let light_pos = shadow_center - dir * SHADOW_LIGHT_DISTANCE;

        let up = if dir.x.abs() < 0.0001 && dir.z.abs() < 0.0001 {
            Vec3::Z
        } else {
            Vec3::Y
        };

        let light_view = Mat4::look_at_rh(light_pos, shadow_center, up);

        let light_proj = glam::camera::rh::proj::opengl::orthographic(
            -SHADOW_ORTHO_HALF_EXTENT,
            SHADOW_ORTHO_HALF_EXTENT,
            -SHADOW_ORTHO_HALF_EXTENT,
            SHADOW_ORTHO_HALF_EXTENT,
            SHADOW_NEAR,
            SHADOW_FAR,
        );

        light_proj * light_view
    }

    pub(super) fn collect_point_lights(&self, world: &World) -> Vec<PointLightData> {
        let mut point_lights = Vec::new();

        // `World::light_ids` is maintained for authored and runtime Lights by
        // every mutation/load path. Do not fall back to scanning `world.cells`
        // when fewer than 16 Lights exist: a large world with no Lights would
        // otherwise visit every resident Cell on every rendered frame, wholly
        // bypassing render-distance filtering.
        for light_id in world.iter_light_ids() {
            if let Some(source) = world.source(light_id) {
                if world.is_light_enabled_by_id(light_id) {
                    point_lights.push(PointLightData {
                        position: world.source_position(light_id).unwrap_or(source.position),
                        color: source.cell.light_color,
                        intensity: source.cell.light_intensity,
                        range: source.cell.light_range,
                    });
                    if point_lights.len() >= 16 {
                        return point_lights;
                    }
                }
                continue;
            }

            let Some(coord) = world.resolve_cell_id(light_id) else {
                continue;
            };

            // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
            // Resolve specialized runtime Cells by ID; coordinate lookup is
            // intentionally single-owner for voxel geometry and may contain a
            // colocated ParticleEmitter instead.
            let Some(cell) = world.get_effective_cell_by_id(light_id) else {
                continue;
            };

            if cell.cell_type == CellType::Light && world.is_light_enabled_by_id(light_id) {
                point_lights.push(PointLightData {
                    position: Vec3::new(
                        coord.x as f32 + 0.5,
                        coord.y as f32 + 0.5,
                        coord.z as f32 + 0.5,
                    ),
                    color: cell.light_color,
                    intensity: cell.light_intensity,
                    range: cell.light_range,
                });

                if point_lights.len() >= 16 {
                    return point_lights;
                }
            }
        }

        point_lights
    }

    pub(super) fn update_gi_cache(
        &self,
        world: &World,
        physics: &crate::engine::physics::PhysicsWorld,
        camera_position: Vec3,
        point_lights: &[PointLightData],
        settings: RendererLightingSettings,
    ) {
        use self::settings::{IndirectDiffuse, LightingMethod};

        if settings.lighting_method == LightingMethod::TwoLevelRadianceCache
            && settings.indirect_diffuse == IndirectDiffuse::On
        {
            self.gi_cache.borrow_mut().update(
                world,
                physics,
                camera_position,
                point_lights,
                settings,
            );
        }
    }

    /// Binds the active GI cache and uploads the uniforms consumed by the
    /// forward material shader. The caller must have `grid_program` bound.
    pub(super) unsafe fn bind_gi_uniforms(&self, settings: RendererLightingSettings) {
        use self::settings::{IndirectDiffuse, LightingMethod};

        let enabled = settings.lighting_method == LightingMethod::TwoLevelRadianceCache
            && settings.indirect_diffuse == IndirectDiffuse::On;
        let cache = self.gi_cache.borrow();
        let origin = cache.origin();
        let dimensions = cache.dimensions();

        unsafe {
            gl::Uniform1i(
                self.grid_lighting_method_location,
                settings.lighting_method as i32,
            );
            gl::Uniform1i(self.grid_indirect_enabled_location, i32::from(enabled));
            gl::Uniform3f(self.grid_gi_origin_location, origin.x, origin.y, origin.z);
            gl::Uniform1f(self.grid_gi_spacing_location, cache.spacing());
            gl::Uniform3i(
                self.grid_gi_dimensions_location,
                dimensions[0],
                dimensions[1],
                dimensions[2],
            );
            gl::Uniform1i(self.grid_gi_denoising_location, settings.denoising as i32);
            gl::ActiveTexture(gl::TEXTURE2);
            gl::BindTexture(gl::TEXTURE_3D, if enabled { cache.texture() } else { 0 });
            gl::Uniform1i(self.grid_gi_cache_location, 2);
            gl::ActiveTexture(gl::TEXTURE1);
        }
    }
    // Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
    pub(super) fn bind_underwater_uniforms(
        &self,
        state: Option<terrain::UnderwaterState>,
        camera_position: Vec3,
    ) {
        unsafe {
            gl::Uniform1i(
                self.grid_underwater_enabled_location,
                i32::from(state.is_some()),
            );
            gl::Uniform3f(
                self.grid_underwater_camera_position_location,
                camera_position.x,
                camera_position.y,
                camera_position.z,
            );

            if let Some(state) = state {
                gl::Uniform3f(
                    self.grid_underwater_color_location,
                    state.color.x,
                    state.color.y,
                    state.color.z,
                );
                gl::Uniform1f(self.grid_underwater_density_location, state.density);
                gl::Uniform1f(self.grid_underwater_surface_y_location, state.surface_y);
            } else {
                gl::Uniform3f(self.grid_underwater_color_location, 0.0, 0.0, 0.0);
                gl::Uniform1f(self.grid_underwater_density_location, 0.0);
                gl::Uniform1f(self.grid_underwater_surface_y_location, 0.0);
            }
        }
    }
    // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
    pub(super) fn bind_atmosphere_uniforms(
        &self,
        world: &World,
        camera_position: Vec3,
        underwater: bool,
    ) {
        let enabled = world.sky.atmosphere_enabled && !underwater;
        let fog_start = world.sky.fog_start_studs.max(0.0);
        let fog_end = world.sky.fog_end_studs.max(fog_start + 1.0);
        let color = world.sky.fog_color.clamp(Vec3::ZERO, Vec3::ONE);

        unsafe {
            gl::Uniform1i(self.grid_atmosphere_enabled_location, i32::from(enabled));
            gl::Uniform3f(
                self.grid_atmosphere_camera_position_location,
                camera_position.x,
                camera_position.y,
                camera_position.z,
            );
            gl::Uniform3f(self.grid_fog_color_location, color.x, color.y, color.z);
            gl::Uniform1f(self.grid_fog_start_location, fog_start);
            gl::Uniform1f(self.grid_fog_end_location, fog_end);
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(super) struct PointLightData {
    pub(super) position: Vec3,
    pub(super) color: Vec3,
    pub(super) intensity: f32,
    pub(super) range: f32,
}

impl Drop for Renderer {
    fn drop(&mut self) {
        self.gi_cache.get_mut().free_gl_resources();
        if let Some(mut terrain) = self.terrain_cache.get_mut().take() {
            terrain.free();
        }
        unsafe {
            for chunk_mesh in self.chunk_cache.borrow_mut().values_mut() {
                chunk_mesh.free_gl_resources();
            }

            for ghost_mesh in self.ghost_cache.borrow_mut().values_mut() {
                ghost_mesh.free_gl_resources();
            }

            if self.grid_program != 0 {
                gl::DeleteProgram(self.grid_program);
            }

            if self.water_program != 0 {
                gl::DeleteProgram(self.water_program);
            }

            if self.ghost_program != 0 {
                gl::DeleteProgram(self.ghost_program);
            }

            if self.shadow_program != 0 {
                gl::DeleteProgram(self.shadow_program);
            }

            if self.shadow_fbo != 0 {
                gl::DeleteFramebuffers(1, &self.shadow_fbo);
            }

            if self.shadow_depth_tex != 0 {
                gl::DeleteTextures(1, &self.shadow_depth_tex);
            }

            for &tex in self.textures.borrow().values() {
                if tex != 0 && tex != self.fallback_tex {
                    gl::DeleteTextures(1, &tex);
                }
            }

            if self.fallback_tex != 0 {
                gl::DeleteTextures(1, &self.fallback_tex);
            }

            if self.grid_vao_xz != 0 {
                gl::DeleteVertexArrays(1, &self.grid_vao_xz);
            }

            if self.grid_vbo_xz != 0 {
                gl::DeleteBuffers(1, &self.grid_vbo_xz);
            }

            if self.grid_vao_yz != 0 {
                gl::DeleteVertexArrays(1, &self.grid_vao_yz);
            }

            if self.grid_vbo_yz != 0 {
                gl::DeleteBuffers(1, &self.grid_vbo_yz);
            }

            if self.grid_vao_xy != 0 {
                gl::DeleteVertexArrays(1, &self.grid_vao_xy);
            }

            if self.grid_vbo_xy != 0 {
                gl::DeleteBuffers(1, &self.grid_vbo_xy);
            }

            if self.axis_vao != 0 {
                gl::DeleteVertexArrays(1, &self.axis_vao);
            }

            if self.axis_vbo != 0 {
                gl::DeleteBuffers(1, &self.axis_vbo);
            }

            if self.highlight_vao != 0 {
                gl::DeleteVertexArrays(1, &self.highlight_vao);
            }

            if self.highlight_vbo != 0 {
                gl::DeleteBuffers(1, &self.highlight_vbo);
            }

            if self.anchor_vao != 0 {
                gl::DeleteVertexArrays(1, &self.anchor_vao);
            }

            if self.anchor_vbo != 0 {
                gl::DeleteBuffers(1, &self.anchor_vbo);
            }

            if self.block_vao != 0 {
                gl::DeleteVertexArrays(1, &self.block_vao);
            }

            if self.block_vbo != 0 {
                gl::DeleteBuffers(1, &self.block_vbo);
            }

            if self.billboard_vao != 0 {
                gl::DeleteVertexArrays(1, &self.billboard_vao);
            }

            if self.billboard_vbo != 0 {
                gl::DeleteBuffers(1, &self.billboard_vbo);
            }

            if self.character_vao != 0 {
                gl::DeleteVertexArrays(1, &self.character_vao);
            }

            if self.character_vbo != 0 {
                gl::DeleteBuffers(1, &self.character_vbo);
            }

            for mesh in self.entity_mesh_cache.get_mut().values() {
                mesh.free_gl_resources();
            }
        }
    }
}

pub(super) struct StaticEntityGpuMesh {
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:22 -05:00
    // One immutable package can contain several glTF material textures. Keep
    // one GPU batch per translated primitive while retaining one cache entry.
    pub(super) batches: Vec<StaticEntityGpuBatch>,
    pub(super) vertex_count: i32,
}

pub(super) struct StaticEntityGpuBatch {
    pub(super) vao: u32,
    pub(super) vbo: u32,
    pub(super) vertex_count: i32,
    pub(super) texture: Option<std::path::PathBuf>,
}

impl StaticEntityGpuMesh {
    pub(super) unsafe fn free_gl_resources(&self) {
        for batch in &self.batches {
            if batch.vao != 0 {
                unsafe { gl::DeleteVertexArrays(1, &batch.vao) };
            }
            if batch.vbo != 0 {
                unsafe { gl::DeleteBuffers(1, &batch.vbo) };
            }
        }
    }
}
const GHOST_VERTEX_SHADER: &str = r#"
#version 330 core

layout (location = 0) in vec3 a_position;
layout (location = 2) in vec4 a_color;

uniform mat4 u_view_projection;
uniform mat4 u_model;

out vec4 v_color;

void main() {
    gl_Position =
        u_view_projection *
        u_model *
        vec4(a_position, 1.0);

    v_color = a_color;
}
"#;

const GHOST_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec4 v_color;

uniform vec3 u_color;
uniform float u_alpha;

out vec4 FragColor;

void main() {
    FragColor = vec4(v_color.rgb * u_color, u_alpha);
}
"#;

const GRID_VERTEX_SHADER: &str = r#"
#version 330 core

layout (location = 0) in vec3 a_position;
layout (location = 1) in vec3 a_normal;
layout (location = 2) in vec4 a_color;
layout (location = 3) in vec2 a_uv;

uniform mat4 u_view_projection;
uniform mat4 u_model;
uniform vec3 u_base_color;

out vec3 v_normal;
out vec4 v_color;
out vec3 v_world_pos;
out vec2 v_uv;

void main() {
    vec4 world_pos =
        u_model *
        vec4(a_position, 1.0);

    v_world_pos =
        world_pos.xyz;

    gl_Position =
        u_view_projection *
        world_pos;

    v_normal =
        mat3(u_model) *
        a_normal;

    v_color =
        vec4(
            a_color.rgb *
            u_base_color,
            a_color.a
        );

    v_uv =
        a_uv;
}
"#;

const GRID_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec3 v_normal;
in vec4 v_color;
in vec3 v_world_pos;
in vec2 v_uv;

uniform float u_ambient_intensity;

uniform bool u_global_light_enabled;
uniform vec3 u_global_light_direction;
uniform vec3 u_global_light_color;
uniform float u_global_light_intensity;

struct PointLight {
    vec3 position;
    vec3 color;
    float intensity;
    float range;
};

#define MAX_POINT_LIGHTS 16

uniform int u_point_light_count;
uniform PointLight u_point_lights[MAX_POINT_LIGHTS];

uniform sampler2D u_shadow_map;
uniform mat4 u_light_space_matrix;
uniform bool u_shadows_enabled;
uniform float u_alpha;

uniform sampler2D u_texture;
uniform bool u_use_texture;

// Worldkiln 0.9.3 | WO 0.9.3-20 | Smooth Terrain layer blending
uniform bool u_terrain_blend_enabled;
uniform int u_terrain_source_role;
uniform float u_terrain_water_level;
uniform sampler2D u_terrain_surface_texture;
uniform sampler2D u_terrain_subsurface_texture;
uniform sampler2D u_terrain_rock_texture;
uniform sampler2D u_terrain_sand_texture;

uniform int u_debug_view;
uniform int u_lighting_method;
uniform bool u_indirect_enabled;
uniform sampler3D u_gi_cache;
uniform vec3 u_gi_origin;
uniform float u_gi_spacing;
uniform ivec3 u_gi_dimensions;
uniform int u_gi_denoising;

// Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
uniform bool u_underwater_enabled;
uniform vec3 u_underwater_camera_position;
uniform vec3 u_underwater_color;
uniform float u_underwater_density;
uniform float u_underwater_surface_y;

// Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
uniform bool u_atmosphere_enabled;
uniform vec3 u_atmosphere_camera_position;
uniform vec3 u_fog_color;
uniform float u_fog_start;
uniform float u_fog_end;

out vec4 FragColor;

const float SHADOW_MIN_BIAS = 0.0008;
const float SHADOW_SLOPE_BIAS_SCALE = 3.0;
const float SHADOW_NORMAL_OFFSET = 0.005;

vec3 sample_indirect_unfiltered(vec3 world_pos, vec3 normal) {
    vec3 grid_position =
        (world_pos - u_gi_origin) /
        max(u_gi_spacing, 0.0001);
    vec3 dimensions = vec3(u_gi_dimensions);

    if (any(lessThan(grid_position, vec3(0.0))) ||
        any(greaterThan(grid_position, dimensions - vec3(1.0)))) {
        return vec3(0.0);
    }

    float weights[6] = float[6](
        max(normal.x, 0.0), max(-normal.x, 0.0),
        max(normal.y, 0.0), max(-normal.y, 0.0),
        max(normal.z, 0.0), max(-normal.z, 0.0)
    );
    vec3 sum = vec3(0.0);
    float weight_sum = 0.0;
    for (int direction_index = 0; direction_index < 6; ++direction_index) {
        vec3 uvw;
        uvw.xy = (grid_position.xy + vec2(0.5)) / dimensions.xy;
        uvw.z =
            (float(direction_index * u_gi_dimensions.z) + grid_position.z + 0.5) /
            float(u_gi_dimensions.z * 6);
        sum += texture(u_gi_cache, uvw).rgb * weights[direction_index];
        weight_sum += weights[direction_index];
    }
    return sum / max(weight_sum, 1.0);
}

vec3 sample_indirect(vec3 world_pos, vec3 normal) {
    if (!u_indirect_enabled || u_lighting_method == 0) {
        return vec3(0.0);
    }

    vec3 center = sample_indirect_unfiltered(world_pos, normal);
    if (u_gi_denoising == 0) {
        return center;
    }

    // Compact world-space cross filter for probe-grid transitions. Temporal
    // reuse is performed while the cache is updated.
    float offset = u_gi_spacing * 0.5;
    vec3 filtered = center;
    filtered += sample_indirect_unfiltered(world_pos + vec3(offset, 0.0, 0.0), normal);
    filtered += sample_indirect_unfiltered(world_pos - vec3(offset, 0.0, 0.0), normal);
    filtered += sample_indirect_unfiltered(world_pos + vec3(0.0, 0.0, offset), normal);
    filtered += sample_indirect_unfiltered(world_pos - vec3(0.0, 0.0, offset), normal);
    return filtered / 5.0;
}

float calculate_shadow_visibility(
    vec3 world_pos,
    vec3 normal
) {
    vec3 shadow_world_pos =
        world_pos + normal * SHADOW_NORMAL_OFFSET;

    vec4 shadow_pos =
        u_light_space_matrix *
        vec4(shadow_world_pos, 1.0);

    if (shadow_pos.w <= 0.0) {
        return 1.0;
    }

    vec3 shadow_coord =
        shadow_pos.xyz /
        shadow_pos.w;

    shadow_coord =
        shadow_coord * 0.5 +
        0.5;

    if (
        shadow_coord.x < 0.0 ||
        shadow_coord.x > 1.0 ||
        shadow_coord.y < 0.0 ||
        shadow_coord.y > 1.0 ||
        shadow_coord.z < 0.0 ||
        shadow_coord.z > 1.0
    ) {
        return 1.0;
    }

    float ndotl =
        max(
            dot(
                normal,
                -u_global_light_direction
            ),
            0.0
        );

    ivec2 shadow_texture_size =
        textureSize(
            u_shadow_map,
            0
        );

    vec2 texel_size =
        vec2(1.0) /
        vec2(shadow_texture_size);

    float bias =
        max(
            SHADOW_MIN_BIAS,
            texel_size.x *
                (1.0 + SHADOW_SLOPE_BIAS_SCALE * (1.0 - ndotl))
        );

    float current_depth =
        shadow_coord.z;

    float shadow_sum = 0.0;

    for (
        int x = -1;
        x <= 1;
        ++x
    ) {
        for (
            int y = -1;
            y <= 1;
            ++y
        ) {
            float shadow_depth =
                texture(
                    u_shadow_map,
                    shadow_coord.xy +
                        vec2(
                            x,
                            y
                        ) * texel_size
                ).r;

            if (
                shadow_depth +
                bias <
                current_depth
            ) {
                shadow_sum += 1.0;
            }
        }
    }

    return 1.0 - (shadow_sum / 9.0);
}

void main() {
    vec3 normal =
        normalize(v_normal);

    vec4 tex_color = vec4(1.0);

    if (u_terrain_blend_enabled) {
        vec2 terrain_uv = v_world_pos.xz;
        vec3 surface_tex = texture(u_terrain_surface_texture, terrain_uv).rgb;
        vec3 subsurface_tex = texture(u_terrain_subsurface_texture, terrain_uv).rgb;
        vec3 rock_tex = texture(u_terrain_rock_texture, terrain_uv).rgb;
        vec3 sand_tex = texture(u_terrain_sand_texture, terrain_uv).rgb;

        float up = clamp(normal.y, 0.0, 1.0);
        float slope = 1.0 - up;
        float rock_mask = smoothstep(0.28, 0.72, slope);

        float shore_height = abs(v_world_pos.y - u_terrain_water_level);
        float shore_mask = 1.0 - smoothstep(1.5, 8.0, shore_height);

        float surface_weight = (1.0 - rock_mask) * (1.0 - shore_mask);
        float sand_weight = (1.0 - rock_mask * 0.65) * shore_mask;
        float rock_weight = rock_mask;
        float subsurface_weight = 0.0;

        // Preserve authored/painted source intent while softening the visible
        // boundary into the procedural slope/shore context.
        if (u_terrain_source_role == 1) {
            subsurface_weight = 0.58;
            surface_weight *= 0.42;
            sand_weight *= 0.42;
            rock_weight *= 0.55;
        } else if (u_terrain_source_role == 2) {
            rock_weight += 0.72;
            surface_weight *= 0.45;
            sand_weight *= 0.45;
        } else if (u_terrain_source_role == 3) {
            sand_weight += 0.72;
            surface_weight *= 0.40;
            rock_weight *= 0.60;
        }

        // Exposed banks receive a little soil so Surface/Subsurface changes do
        // not read as a single hard stripe.
        subsurface_weight += smoothstep(0.22, 0.62, slope) * (1.0 - rock_mask) * 0.18;

        float total_weight =
            max(surface_weight + subsurface_weight + rock_weight + sand_weight, 0.0001);
        vec3 blended =
            surface_tex * surface_weight +
            subsurface_tex * subsurface_weight +
            rock_tex * rock_weight +
            sand_tex * sand_weight;
        tex_color.rgb = blended / total_weight;
    } else if (u_use_texture) {
        tex_color = texture(u_texture, v_uv);
    }

    vec3 color =
        v_color.rgb *
        tex_color.rgb;

    if (u_debug_view == 3) {
        FragColor = vec4(color, v_color.a * tex_color.a * u_alpha);
        return;
    }

    if (u_debug_view == 4) {
        FragColor = vec4(normal * 0.5 + 0.5, 1.0);
        return;
    }

    if (u_debug_view == 5) {
        FragColor = vec4(vec3(gl_FragCoord.z), 1.0);
        return;
    }

    // The legacy renderer has no indirect-light field. Give voxel faces a
    // modest axis-dependent sky fill so two unlit side faces still read as
    // separate planes. Keep the top at its previous ambient level.
    float ambient_face_factor =
        1.05 +
        0.30 * abs(normal.x) -
        0.05 * max(normal.y, 0.0) -
        0.40 * max(-normal.y, 0.0);

    vec3 ambient =
        color *
        u_ambient_intensity *
        ambient_face_factor;

    vec3 diffuse =
        vec3(0.0);

    if (u_global_light_enabled) {
        vec3 L =
            normalize(
                -u_global_light_direction
            );

        float diff =
            max(
                dot(normal, L),
                0.0
            );

        float shadow_visibility =
            u_shadows_enabled
                ? calculate_shadow_visibility(
                    v_world_pos,
                    normal
                )
                : 1.0;

        diffuse =
            shadow_visibility *
            color *
            diff *
            u_global_light_color *
            u_global_light_intensity;
    }

    vec3 point_contribution =
        vec3(0.0);

    for (
        int i = 0;
        i < u_point_light_count;
        i++
    ) {
        vec3 L_vec =
            u_point_lights[i].position -
            v_world_pos;

        float dist =
            length(L_vec);

        if (
            dist > 0.00001 &&
            dist < u_point_lights[i].range
        ) {
            vec3 L =
                L_vec / dist;

            float diff =
                max(
                    dot(normal, L),
                    0.0
                );

            float attenuation =
                1.0 -
                (
                    dist /
                    u_point_lights[i].range
                );

            attenuation *=
                attenuation;

            point_contribution +=
                color *
                diff *
                u_point_lights[i].color *
                u_point_lights[i].intensity *
                attenuation;
        }
    }

    vec3 result =
        ambient +
        diffuse +
        point_contribution;

    vec3 indirect = color * sample_indirect(v_world_pos, normal);

    if (u_debug_view == 1) {
        FragColor = vec4(result, v_color.a * tex_color.a * u_alpha);
        return;
    }

    if (u_debug_view == 2 || u_debug_view == 7) {
        FragColor = vec4(indirect, 1.0);
        return;
    }

    result += indirect;

    // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
    if (u_atmosphere_enabled && !u_underwater_enabled) {
        float atmosphere_distance =
            length(v_world_pos - u_atmosphere_camera_position);
        float fog_amount =
            smoothstep(
                max(u_fog_start, 0.0),
                max(u_fog_end, u_fog_start + 1.0),
                atmosphere_distance
            );
        result = mix(result, u_fog_color, fog_amount);
    }

    // Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
    if (u_underwater_enabled) {
        float view_distance = length(v_world_pos - u_underwater_camera_position);
        float distance_fog =
            1.0 - exp(-max(u_underwater_density, 0.0) * view_distance);
        float depth_below_surface =
            max(u_underwater_surface_y - v_world_pos.y, 0.0);
        float depth_fog =
            clamp(depth_below_surface * 0.018, 0.0, 0.32);
        float fog_amount =
            clamp(distance_fog + depth_fog, 0.0, 0.94);
        result = mix(result, u_underwater_color, fog_amount);
    }

    FragColor =
        vec4(
            result,
            v_color.a *
            tex_color.a *
            u_alpha
        );
}
"#;

const SHADOW_VERTEX_SHADER: &str = r#"
#version 330 core

layout (location = 0) in vec3 a_position;

uniform mat4 u_light_space_matrix;
uniform mat4 u_model;

void main() {
    gl_Position =
        u_light_space_matrix *
        u_model *
        vec4(a_position, 1.0);
}
"#;

const SHADOW_FRAGMENT_SHADER: &str = r#"
#version 330 core

void main() {
    // Depth is written automatically.
}
"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_directional_shadow_matrix_invariant_to_camera_rotation() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let light_dir = Vec3::new(0.5, -1.0, 0.5).normalize();
        let camera_pos = Vec3::new(10.0, 5.0, 20.0);

        let matrix1 = renderer.calculate_light_space_matrix(light_dir, camera_pos);
        let matrix2 = renderer.calculate_light_space_matrix(light_dir, camera_pos);

        assert_eq!(matrix1, matrix2);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    #[test]
    fn directional_shadow_frustum_excludes_distant_chunks() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let camera_pos = Vec3::new(8.0, 8.0, 8.0);
        let light_dir = Vec3::new(0.5, -1.0, 0.5).normalize();
        let light_space = renderer.calculate_light_space_matrix(light_dir, camera_pos);
        let frustum = chunk::CameraFrustum::from_view_projection(light_space);

        let near = ChunkCoord::new(0, 0, 0);
        let far = ChunkCoord::new(100, 0, 100);
        let (near_min, near_max) = near.aabb_min_max();
        let (far_min, far_max) = far.aabb_min_max();

        assert!(frustum.intersects_aabb(near_min, near_max));
        assert!(!frustum.intersects_aabb(far_min, far_max));
    }

    #[test]
    fn test_directional_shadow_matrix_changes_when_light_direction_changes() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let camera_pos = Vec3::new(10.0, 5.0, 20.0);

        let light_dir1 = Vec3::new(0.5, -1.0, 0.5).normalize();
        let light_dir2 = Vec3::new(-0.5, -0.8, -0.2).normalize();

        let matrix1 = renderer.calculate_light_space_matrix(light_dir1, camera_pos);
        let matrix2 = renderer.calculate_light_space_matrix(light_dir2, camera_pos);

        assert_ne!(matrix1, matrix2);
    }

    #[test]
    fn lighting_settings_hot_apply_updates_active_state() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let mut requested = renderer.lighting_state().requested;
        requested.shadow_method = settings::ShadowMethod::Off;
        requested.debug_view = settings::DebugView::Normals;
        renderer.request_lighting_settings(requested);

        assert_eq!(
            renderer.apply_requested_lighting_settings(),
            ApplyResult::Applied
        );
        assert_eq!(renderer.lighting_state().active, requested);
        assert!(!renderer.lighting_state().restart_pending);
    }

    #[test]
    fn restart_apply_behavior_preserves_active_state() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let active = renderer.lighting_state().active;
        let mut requested = active;
        requested.shadow_method = settings::ShadowMethod::Off;
        requested.apply_behavior = ApplyBehavior::RestartRequired;
        renderer.request_lighting_settings(requested);

        assert_eq!(
            renderer.apply_requested_lighting_settings(),
            ApplyResult::RestartRequired
        );
        let state = renderer.lighting_state();
        assert_eq!(state.active, active);
        assert_eq!(state.requested, requested);
        assert!(state.restart_pending);
    }

    #[test]
    fn unfinished_beta_method_does_not_replace_active_renderer() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let active = renderer.lighting_state().active;
        let mut requested = active;
        requested.lighting_method = settings::LightingMethod::SplitRadianceCascades;
        renderer.request_lighting_settings(requested);

        assert!(matches!(
            renderer.apply_requested_lighting_settings(),
            ApplyResult::Unsupported(_)
        ));
        let state = renderer.lighting_state();
        assert_eq!(state.active, active);
        assert_eq!(state.requested, requested);
        assert!(state.last_error.is_some());
    }

    #[test]
    fn test_point_light_collection_requires_cell_type_light_and_light_enabled() {
        use crate::world::WorldCoord;

        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let mut world = World::new();

        let block_coord = WorldCoord::new(1, 1, 1);
        let light_enabled_coord = WorldCoord::new(2, 2, 2);
        let light_disabled_coord = WorldCoord::new(3, 3, 3);

        // 1. Ordinary Block with light_enabled = true (persisted Block state) -> NOT a point light
        world.set_cell(block_coord, CellType::Block);
        if let Some(cell) = world.get_mut(block_coord) {
            cell.light_enabled = true;
        }

        let lights = renderer.collect_point_lights(&world);
        assert!(
            lights.is_empty(),
            "Block with light_enabled=true must NOT produce a point light"
        );

        // 2. Light cell with light_enabled = true -> IS a point light
        world.set_cell(light_enabled_coord, CellType::Light);
        let lights = renderer.collect_point_lights(&world);
        assert_eq!(
            lights.len(),
            1,
            "Light cell with light_enabled=true must produce a point light"
        );
        assert_eq!(
            lights[0].position,
            Vec3::new(
                light_enabled_coord.x as f32 + 0.5,
                light_enabled_coord.y as f32 + 0.5,
                light_enabled_coord.z as f32 + 0.5
            )
        );

        // 3. Light cell with light_enabled = false -> NOT a point light
        world.set_cell(light_disabled_coord, CellType::Light);
        world.set_light_enabled_runtime(light_disabled_coord, false);
        let lights = renderer.collect_point_lights(&world);
        assert_eq!(
            lights.len(),
            1,
            "Disabled Light cell must NOT produce an additional point light"
        );

        // 4. Light cell deleted -> NOT a point light
        let light_id = world.get(light_enabled_coord).unwrap().id;
        world.delete_cell_runtime(light_id);
        let lights = renderer.collect_point_lights(&world);
        assert!(
            lights.is_empty(),
            "Deleted Light cell must NOT produce a point light"
        );

        // 5. Block replacing a Light -> NOT a point light
        world.set_cell(light_disabled_coord, CellType::Block);
        if let Some(cell) = world.get_mut(light_disabled_coord) {
            cell.light_enabled = true;
        }
        let lights = renderer.collect_point_lights(&world);
        assert!(
            lights.is_empty(),
            "Block replacing a Light cell must NOT produce a point light"
        );
    }

    #[test]
    fn point_light_collection_uses_the_maintained_index_without_world_scan_fallback() {
        use crate::world::WorldCoord;

        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let mut world = World::new();

        for x in 0..128 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        let light_coord = WorldCoord::new(128, 0, 0);
        let light_id = world.set_cell(light_coord, CellType::Light);
        assert_eq!(renderer.collect_point_lights(&world).len(), 1);

        // Simulate an invalid/stale index. Collection must remain bounded by
        // the index instead of silently restoring an O(world size) frame scan.
        world.light_ids.clear();
        assert!(renderer.collect_point_lights(&world).is_empty());

        world.rebuild_id_mapping();
        assert!(world.light_ids.contains(&light_id));
        assert_eq!(renderer.collect_point_lights(&world).len(), 1);
    }

    #[test]
    fn point_light_collection_uses_exact_freeform_source_position() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let mut world = World::new();
        let position = Vec3::new(1.0, 11.2, 0.0);
        world.add_source(CellType::Light, position);

        let lights = renderer.collect_point_lights(&world);
        assert_eq!(lights.len(), 1);
        assert_eq!(lights[0].position, position);
        assert!(world.active_blocks().is_empty());
    }
}
