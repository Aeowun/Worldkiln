// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
pub mod camera;
pub mod camera_spring_arm;
pub mod capabilities;
pub mod character_render;
pub mod chunk;
pub mod chunk_cache;
mod contact_foam;
pub mod distance;
#[cfg(feature = "editor")]
mod editor_profile;
#[cfg(feature = "editor")]
pub mod editor_render;
pub(crate) mod effekseer;
pub mod game_render;
mod gi_cache;
mod gi_scene;
mod gpu_skin;
#[cfg(feature = "editor")]
pub mod home_render;
#[cfg(feature = "editor")]
mod lighting_preview;
mod material_graph_bake;
#[cfg(feature = "editor")]
mod material_preview;
mod material_preview_landscape;
mod material_preview_water;
pub mod mesh;
mod mirage;
mod native_landscape_preview;
mod paint_gl;
mod particles;
mod point_lights_gpu;
mod point_shadows_gpu;
mod post_process;
mod profile;
pub mod resources;
pub mod rhi;
pub mod settings;
pub mod shader;
pub mod sky;
pub mod terrain;
mod terrain_layer_gl;
mod water_contacts;
mod water_fluid;
mod water_volume_mask;

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

#[derive(Clone, Copy)]
pub(super) struct WaterWake {
    start: glam::Vec2,
    end: glam::Vec2,
    born: f32,
    power: f32,
}
#[derive(Default)]
pub(super) struct WaterWakeState {
    previous: std::collections::HashMap<u64, (glam::Vec3, f32)>,
    trails: Vec<WaterWake>,
    contacts: std::collections::HashMap<u64, water_contacts::WaterContact>,
    contact_camera: glam::Vec3,
    floor_samples: std::collections::HashMap<(u32, u32), Option<f32>>,
    floor_revision: u64,
}
impl WaterWakeState {
    pub(super) fn record(&mut self, id: u64, position: glam::Vec3, time: f32, sea: f32) {
        if !position.is_finite() || (position.y - sea).abs() > 2.5 {
            self.previous.remove(&id);
            return;
        }
        if let Some((last, prior)) = self.previous.get(&id).copied() {
            let delta = glam::Vec2::new(position.x, position.z) - glam::Vec2::new(last.x, last.z);
            let elapsed = (time - prior).max(0.0001);
            let distance = delta.length();
            if distance > 0.22 && elapsed > 0.10 && elapsed < 2.0 {
                self.trails.push(WaterWake {
                    start: glam::Vec2::new(last.x, last.z),
                    end: glam::Vec2::new(position.x, position.z),
                    born: time,
                    power: (distance / elapsed / 4.0).clamp(0.25, 1.0),
                });
            }
        }
        if self.previous.get(&id).is_none_or(|(last, t)| {
            time - *t > 2.0
                || (time - *t >= 0.12
                    && glam::Vec2::new(position.x - last.x, position.z - last.z).length() > 0.22)
        }) {
            self.previous.insert(id, (position, time));
        }
        self.trails.retain(|wake| time - wake.born < 4.0);
        if self.trails.len() > 24 {
            self.trails.drain(..self.trails.len() - 24);
        }
    }
}
pub struct Renderer {
    capabilities: GraphicsCapabilities,
    lighting_state: RefCell<RendererLightingState>,
    pub(super) grid_program: u32,
    pub(super) water_program: u32,
    pub(super) water_started: Instant,
    pub(super) water_view_projection_location: i32,
    pub(super) water_camera_position_location: i32,
    pub(super) water_time_location: i32,
    pub(super) water_camera_underwater_location: i32,
    pub(super) water_wave_height_location: i32,
    pub(super) water_wave_speed_location: i32,
    pub(super) water_wave_length_location: i32,
    pub(super) water_wave_direction_location: i32,
    pub(super) water_wave_steepness_location: i32,
    pub(super) water_ocean_parity_wave_location: i32,
    pub(super) water_absorption_color_location: i32,
    pub(super) water_absorption_strength_location: i32,
    pub(super) water_scattering_color_location: i32,
    pub(super) water_scattering_strength_location: i32,
    pub(super) water_local_flow_location: i32,
    pub(super) water_foam_amount_location: i32,
    pub(super) water_foam_scale_location: i32,
    pub(super) water_shallow_color_location: i32,
    pub(super) water_deep_color_location: i32,
    pub(super) water_foam_color_location: i32,
    pub(super) water_underwater_color_location: i32,
    pub(super) water_opacity_location: i32,
    pub(super) water_underwater_opacity_location: i32,
    pub(super) water_reflection_strength_location: i32,
    pub(super) water_specular_strength_location: i32,
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
    pub(super) grid_underwater_enabled_location: i32,
    pub(super) grid_ocean_optics_mode_location: i32,
    pub(super) grid_underwater_camera_position_location: i32,
    pub(super) grid_underwater_color_location: i32,
    pub(super) grid_underwater_density_location: i32,
    pub(super) grid_underwater_depth_fog_location: i32,
    pub(super) grid_underwater_depth_fog_limit_location: i32,
    pub(super) grid_underwater_fog_limit_location: i32,
    pub(super) grid_underwater_surface_y_location: i32,
    pub(super) grid_atmosphere_enabled_location: i32,
    pub(super) grid_atmosphere_camera_position_location: i32,
    pub(super) grid_fog_color_location: i32,
    pub(super) grid_fog_start_location: i32,
    pub(super) grid_fog_end_location: i32,
    pub(super) grid_terrain_blend_enabled_location: i32,
    pub(super) grid_terrain_layer_pass_location: i32,
    pub(super) grid_material_preview_location: i32,
    pub(super) grid_cell_pbr_location: i32,
    pub(super) grid_terrain_source_role_location: i32,
    pub(super) grid_terrain_vertex_layers_location: i32,
    pub(super) grid_terrain_default_layer_location: i32,
    pub(super) grid_terrain_water_level_location: i32,
    pub(super) grid_ocean_wetness_location: i32,
    pub(super) grid_ocean_wetness_direction_location: i32,
    pub(super) grid_terrain_blend_params_location: i32,
    pub(super) grid_paint_origin_location: i32,
    pub(super) grid_paint_flags_location: i32,
    pub(super) grid_point_light_count_location: i32,
    pub(super) point_light_stream: point_lights_gpu::PointLightStream,

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
    #[cfg(feature = "editor")]
    pub(super) brush_ring_cache: RefCell<[Option<editor_render::BrushRingGpu>; 2]>,
    pub(super) water_wakes: RefCell<WaterWakeState>,
    pub(super) contact_foam: RefCell<contact_foam::ContactFoamDepth>,
    pub(super) fluid_gpu: RefCell<Option<water_fluid::ThreeDimensionalWaterGpu>>,
    pub(super) paint_gpu: RefCell<paint_gl::PaintGpuPages>,
    pub(super) last_render_revision: RefCell<u64>,
    pub(super) last_render_mode: RefCell<Option<RenderMode>>,

    pub(super) ghost_cache: RefCell<HashMap<ChunkCoord, GhostChunkMesh>>,
    pub(super) last_ghost_revision: RefCell<u64>,
    pub(super) last_ghost_mode: RefCell<Option<RenderMode>>,

    pub(super) billboard_vao: u32,
    pub(super) billboard_vbo: u32,

    pub(super) character_vao: u32,
    pub(super) character_vbo: u32,
    pub(super) gpu_skin_state: RefCell<gpu_skin::GpuSkinState>,
    pub(super) grid_skin_enabled_location: i32,
    pub(super) grid_skin_matrices_location: i32,
    pub(super) grid_skin_material_color_location: i32,
    pub(super) entity_mesh_cache: RefCell<HashMap<String, StaticEntityGpuMesh>>,

    pub(super) shadow_program: u32,
    pub(super) shadow_fbo: u32,
    pub(super) shadow_depth_tex: u32,
    pub(super) point_shadow_layers: RefCell<i32>,
    pub(super) point_shadow_cache: RefCell<point_shadows_gpu::PointShadowCache>,

    pub(super) textures: RefCell<HashMap<String, u32>>,
    pub(super) materials: RefCell<std::sync::Arc<crate::project::material::MaterialCatalog>>,
    #[cfg(feature = "editor")]
    pub(super) graph_preview_programs:
        RefCell<HashMap<String, Result<material_preview::GraphPreviewProgram, String>>>,
    pub(super) graph_baked_textures: RefCell<HashMap<String, Result<u32, String>>>,
    pub(super) layer_source_images:
        RefCell<HashMap<std::path::PathBuf, std::sync::Arc<image::RgbaImage>>>,
    pub(super) water_preview_program:
        RefCell<Option<Result<material_preview_water::WaterPreviewProgram, String>>>,
    pub(super) landscape_preview_mesh: RefCell<Option<(u32, u32, i32)>>,
    pub(super) project_assets_dir: RefCell<Option<std::path::PathBuf>>,
    pub(super) cubemaps: RefCell<HashMap<String, u32>>,
    pub(super) active_sky_material: RefCell<Option<String>>,
    pub(super) fallback_tex: u32,

    pub(super) sky_renderer: sky::SkyRenderer,
    pub(super) water_volume_mask: RefCell<water_volume_mask::WaterVolumeGpu>,
    pub(super) mirage: RefCell<mirage::MiragePass>,
    pub(super) post_process: RefCell<post_process::ScenePostProcess>,
    #[cfg(feature = "editor")]
    pub(super) lighting_preview: RefCell<lighting_preview::LightingPreviewCapture>,
    pub(super) profiler: RefCell<Option<profile::RenderProfiler>>,
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

pub(super) fn physics_body_model_matrix(
    body: &crate::engine::physics::PhysicsBody,
) -> Mat4 {
    let center = body.size * 0.5;
    Mat4::from_translation(body.position + center)
        * Mat4::from_quat(body.rotation)
        * Mat4::from_translation(-center)
}

impl Renderer {
    #[cfg(feature = "editor")]
    pub(crate) fn configure_development_profile(
        &self,
        tag: Option<(u64, u64, std::time::Instant)>,
        gpu: bool,
    ) {
        let mut profiler = self.editor_profiler.borrow_mut();
        if tag.is_some() && profiler.is_none() {
            *profiler = editor_profile::EditorProfiler::for_capture();
        }
        if tag.is_none() && profiler.as_ref().is_some_and(|p| !p.report_enabled()) {
            *profiler = None;
        }
        if let Some(profiler) = profiler.as_mut() {
            profiler.capture_gpu = gpu;
            profiler.configure_capture(tag);
        }
    }
    #[cfg(feature = "editor")]
    pub(crate) fn pending_development_samples(&self) -> u64 {
        self.editor_profiler
            .borrow()
            .as_ref()
            .map_or(0, |p| p.pending_capture_samples())
    }
    #[cfg(feature = "editor")]
    pub(crate) fn take_development_profile(
        &self,
    ) -> (
        Vec<(
            u64,
            &'static str,
            std::time::Duration,
            std::time::Instant,
            bool,
        )>,
        u64,
    ) {
        self.editor_profiler
            .borrow_mut()
            .as_mut()
            .map_or_else(|| (Vec::new(), 0), |p| p.take_capture())
    }
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

        let mut grid_fragment_shader = GRID_FRAGMENT_SHADER.to_owned();
        if let Ok(probe) = std::env::var("WORLDKILN_RENDER_PROBE") {
            let replacement = match probe.as_str() {
                "no-material-blend" => Some((
                    "else if (u_terrain_blend_enabled) {",
                    "else if (false && u_terrain_blend_enabled) {",
                )),
                "no-ocean-optics" => Some((
                    "if (u_underwater_enabled && u_ocean_contact_mapping.z > 0.0 &&",
                    "if (false && u_underwater_enabled && u_ocean_contact_mapping.z > 0.0 &&",
                )),
                "no-shore-wetness" => Some((
                    "if (!u_water_basic && terrain_wet_pass && u_ocean_wetness.x>0.00001 &&",
                    "if (false && !u_water_basic && terrain_wet_pass && u_ocean_wetness.x>0.00001 &&",
                )),
                "no-local-water" => Some((
                    "vec4 local=wk_apply_local_water(\n        local_water_air_color,u_underwater_camera_position,v_world_pos,false);",
                    "vec4 local=vec4(local_water_air_color,0.0);",
                )),
                "no-painted-texture" => Some((
                    "vec4 native_paint=sample_native_paint(v_world_pos.xz);",
                    "vec4 native_paint=vec4(0.0);",
                )),
                _ => None,
            };
            if let Some((before, after)) = replacement {
                assert!(
                    grid_fragment_shader.contains(before),
                    "Renderer diagnostic shader anchor changed for {probe}"
                );
                grid_fragment_shader = grid_fragment_shader.replacen(before, after, 1);
                eprintln!(
                    "[WORLDKILN RENDER PROBE] {probe}: diagnostic shader, NOT a production feature switch"
                );
            }
        }
        let tiles_per_axis = SHADOW_RES / point_shadows_gpu::SHADOW_TILE_SIZE;
        assert!(tiles_per_axis > 0 && SHADOW_RES % point_shadows_gpu::SHADOW_TILE_SIZE == 0);
        grid_fragment_shader = grid_fragment_shader.replace(
            "__POINT_SHADOW_TILES_PER_AXIS__",
            &format!("{tiles_per_axis}.0"),
        );
        let grid_program = create_program(GRID_VERTEX_SHADER, &grid_fragment_shader);
        let water_fragment = terrain::WATER_FRAGMENT_SHADER.replace(
            "// WK_OBJECT_CONTACT_FOAM_FUNCTION",
            contact_foam::OBJECT_CONTACT_FOAM_GLSL,
        );
        let water_program = create_program(terrain::WATER_VERTEX_SHADER, &water_fragment);

        let capabilities = GraphicsCapabilities::detect();
        if std::env::var_os("AEO_RENDER_CAPS").is_some() {
            capabilities.log();
        }

        let sky_renderer = sky::SkyRenderer::new();
        let water_volume_mask =
            water_volume_mask::WaterVolumeGpu::new(grid_program, sky_renderer.program);
        let ghost_program = create_program(GHOST_VERTEX_SHADER, GHOST_FRAGMENT_SHADER);

        let shadow_program = create_program(SHADOW_VERTEX_SHADER, SHADOW_FRAGMENT_SHADER);

        let water_view_projection_location =
            get_uniform_location(water_program, "u_view_projection");
        let water_camera_position_location =
            get_uniform_location(water_program, "u_camera_position");
        let water_time_location = get_uniform_location(water_program, "u_time");
        let water_camera_underwater_location =
            get_uniform_location(water_program, "u_camera_underwater");
        let water_wave_height_location = get_uniform_location(water_program, "u_wave_height");
        let water_wave_speed_location = get_uniform_location(water_program, "u_wave_speed");
        let water_wave_length_location = get_uniform_location(water_program, "u_wave_length");
        let water_wave_direction_location = get_uniform_location(water_program, "u_wave_direction");
        let water_wave_steepness_location = get_uniform_location(water_program, "u_wave_steepness");
        let water_ocean_parity_wave_location =
            get_uniform_location(water_program, "u_ocean_parity_wave");
        let water_absorption_color_location =
            get_uniform_location(water_program, "u_absorption_color");
        let water_absorption_strength_location =
            get_uniform_location(water_program, "u_absorption_strength");
        let water_scattering_color_location =
            get_uniform_location(water_program, "u_scattering_color");
        let water_scattering_strength_location =
            get_uniform_location(water_program, "u_scattering_strength");
        let water_local_flow_location = get_uniform_location(water_program, "u_local_flow");
        let water_foam_amount_location = get_uniform_location(water_program, "u_foam_amount");
        let water_foam_scale_location = get_uniform_location(water_program, "u_foam_scale");
        let water_shallow_color_location = get_uniform_location(water_program, "u_shallow_color");
        let water_deep_color_location = get_uniform_location(water_program, "u_deep_color");
        let water_foam_color_location = get_uniform_location(water_program, "u_foam_color");
        let water_underwater_color_location =
            get_uniform_location(water_program, "u_underwater_color");
        let water_opacity_location = get_uniform_location(water_program, "u_opacity");
        let water_underwater_opacity_location =
            get_uniform_location(water_program, "u_underwater_opacity");
        let water_reflection_strength_location =
            get_uniform_location(water_program, "u_reflection_strength");
        let water_specular_strength_location =
            get_uniform_location(water_program, "u_specular_strength");
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
        let grid_underwater_enabled_location =
            get_uniform_location(grid_program, "u_underwater_enabled");
        let grid_ocean_optics_mode_location =
            get_uniform_location(grid_program, "u_ocean_optics_mode");
        let grid_underwater_camera_position_location =
            get_uniform_location(grid_program, "u_underwater_camera_position");
        let grid_underwater_color_location =
            get_uniform_location(grid_program, "u_underwater_color");
        let grid_underwater_density_location =
            get_uniform_location(grid_program, "u_underwater_density");
        let grid_underwater_depth_fog_location =
            get_uniform_location(grid_program, "u_underwater_depth_fog");
        let grid_underwater_depth_fog_limit_location =
            get_uniform_location(grid_program, "u_underwater_depth_fog_limit");
        let grid_underwater_fog_limit_location =
            get_uniform_location(grid_program, "u_underwater_fog_limit");
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
        let grid_terrain_layer_pass_location =
            get_uniform_location(grid_program, "u_terrain_layer_pass");
        let grid_material_preview_location =
            get_uniform_location(grid_program, "u_material_preview");
        let grid_cell_pbr_location = get_uniform_location(grid_program, "u_cell_pbr");
        let grid_terrain_source_role_location =
            get_uniform_location(grid_program, "u_terrain_source_role");
        let grid_terrain_vertex_layers_location =
            get_uniform_location(grid_program, "u_terrain_vertex_layers");
        let grid_terrain_default_layer_location =
            get_uniform_location(grid_program, "u_terrain_default_layer");
        let grid_terrain_water_level_location =
            get_uniform_location(grid_program, "u_terrain_water_level");
        let grid_ocean_wetness_location = get_uniform_location(grid_program, "u_ocean_wetness");
        let grid_ocean_wetness_direction_location =
            get_uniform_location(grid_program, "u_ocean_wetness_direction");
        let grid_terrain_blend_params_location =
            get_uniform_location(grid_program, "u_terrain_blend_params");
        let grid_paint_origin_location = get_uniform_location(grid_program, "u_paint_origin");
        let grid_paint_flags_location = get_uniform_location(grid_program, "u_paint_flags");
        let grid_point_light_count_location =
            get_uniform_location(grid_program, "u_point_light_count");

        point_lights_gpu::PointLightStream::bind_sampler(grid_program);

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

            gl::BindTexture(gl::TEXTURE_2D_ARRAY, shadow_depth_tex);

            gl::TexImage3D(
                gl::TEXTURE_2D_ARRAY,
                0,
                gl::DEPTH_COMPONENT24 as i32,
                SHADOW_RES,
                SHADOW_RES,
                1,
                0,
                gl::DEPTH_COMPONENT,
                gl::UNSIGNED_INT,
                std::ptr::null(),
            );

            gl::TexParameteri(
                gl::TEXTURE_2D_ARRAY,
                gl::TEXTURE_MIN_FILTER,
                gl::NEAREST as i32,
            );

            gl::TexParameteri(
                gl::TEXTURE_2D_ARRAY,
                gl::TEXTURE_MAG_FILTER,
                gl::NEAREST as i32,
            );

            gl::TexParameteri(
                gl::TEXTURE_2D_ARRAY,
                gl::TEXTURE_WRAP_S,
                gl::CLAMP_TO_BORDER as i32,
            );

            gl::TexParameteri(
                gl::TEXTURE_2D_ARRAY,
                gl::TEXTURE_WRAP_T,
                gl::CLAMP_TO_BORDER as i32,
            );

            let border_color = [1.0, 1.0, 1.0, 1.0];

            gl::TexParameterfv(
                gl::TEXTURE_2D_ARRAY,
                gl::TEXTURE_BORDER_COLOR,
                border_color.as_ptr(),
            );

            gl::BindFramebuffer(gl::FRAMEBUFFER, shadow_fbo);

            gl::FramebufferTextureLayer(
                gl::FRAMEBUFFER,
                gl::DEPTH_ATTACHMENT,
                shadow_depth_tex,
                0,
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

        let renderer = Self {
            capabilities,
            lighting_state: RefCell::new(RendererLightingState::default()),
            grid_program,
            #[cfg(feature = "editor")]
            graph_preview_programs: RefCell::new(HashMap::new()),
            graph_baked_textures: RefCell::new(HashMap::new()),
            layer_source_images: RefCell::new(HashMap::new()),
            water_preview_program: RefCell::new(None),
            landscape_preview_mesh: RefCell::new(None),
            water_program,
            water_started: Instant::now(),
            water_view_projection_location,
            water_camera_position_location,
            water_time_location,
            water_camera_underwater_location,
            water_wave_height_location,
            water_wave_speed_location,
            water_wave_length_location,
            water_wave_direction_location,
            water_wave_steepness_location,
            water_ocean_parity_wave_location,
            water_absorption_color_location,
            water_absorption_strength_location,
            water_scattering_color_location,
            water_scattering_strength_location,
            water_local_flow_location,
            water_foam_amount_location,
            water_foam_scale_location,
            water_shallow_color_location,
            water_deep_color_location,
            water_foam_color_location,
            water_underwater_color_location,
            water_opacity_location,
            water_underwater_opacity_location,
            water_reflection_strength_location,
            water_specular_strength_location,
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
            grid_ocean_optics_mode_location,
            grid_underwater_camera_position_location,
            grid_underwater_color_location,
            grid_underwater_density_location,
            grid_underwater_depth_fog_location,
            grid_underwater_depth_fog_limit_location,
            grid_underwater_fog_limit_location,
            grid_underwater_surface_y_location,
            grid_atmosphere_enabled_location,
            grid_atmosphere_camera_position_location,
            grid_fog_color_location,
            grid_fog_start_location,
            grid_fog_end_location,
            grid_terrain_blend_enabled_location,
            grid_terrain_layer_pass_location,
            grid_material_preview_location,
            grid_cell_pbr_location,
            grid_terrain_source_role_location,
            grid_terrain_vertex_layers_location,
            grid_terrain_default_layer_location,
            grid_terrain_water_level_location,
            grid_ocean_wetness_location,
            grid_ocean_wetness_direction_location,
            grid_terrain_blend_params_location,
            grid_paint_origin_location,
            grid_paint_flags_location,
            grid_point_light_count_location,
            point_light_stream: point_lights_gpu::PointLightStream::new(),

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
            #[cfg(feature = "editor")]
            brush_ring_cache: RefCell::new([None, None]),
            water_wakes: RefCell::new(WaterWakeState::default()),
            contact_foam: RefCell::new(contact_foam::ContactFoamDepth::default()),
            fluid_gpu: RefCell::new(None),
            paint_gpu: RefCell::new(Default::default()),
            last_render_revision: RefCell::new(0),
            last_render_mode: RefCell::new(None),

            ghost_cache: RefCell::new(HashMap::new()),
            last_ghost_revision: RefCell::new(0),
            last_ghost_mode: RefCell::new(None),

            billboard_vao,
            billboard_vbo,

            character_vao,
            character_vbo,
            gpu_skin_state: RefCell::new(Default::default()),
            grid_skin_enabled_location: get_uniform_location(grid_program, "u_skin_enabled"),
            grid_skin_matrices_location: get_uniform_location(grid_program, "u_skin_matrices"),
            grid_skin_material_color_location: get_uniform_location(
                grid_program,
                "u_skin_material_color",
            ),
            entity_mesh_cache: RefCell::new(HashMap::new()),

            shadow_program,
            shadow_fbo,
            shadow_depth_tex,
            point_shadow_layers: RefCell::new(1),
            point_shadow_cache: RefCell::new(Default::default()),

            textures: RefCell::new(textures),
            materials: RefCell::new(std::sync::Arc::new(
                crate::project::material::MaterialCatalog::default(),
            )),
            project_assets_dir: RefCell::new(None),
            cubemaps: RefCell::new(HashMap::new()),
            active_sky_material: RefCell::new(None),
            fallback_tex,

            sky_renderer,
            water_volume_mask: RefCell::new(water_volume_mask),
            mirage: RefCell::new(mirage::MiragePass::default()),
            post_process: RefCell::new(post_process::ScenePostProcess::default()),
            #[cfg(feature = "editor")]
            lighting_preview: RefCell::new(lighting_preview::LightingPreviewCapture::default()),
            profiler: RefCell::new(profile::RenderProfiler::new()),
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

            let surface_sampler = get_uniform_location(grid_program, "u_terrain_surface_texture");
            let subsurface_sampler =
                get_uniform_location(grid_program, "u_terrain_subsurface_texture");
            let rock_sampler = get_uniform_location(grid_program, "u_terrain_rock_texture");
            let sand_sampler = get_uniform_location(grid_program, "u_terrain_sand_texture");
            gl::Uniform1i(surface_sampler, 3);
            gl::Uniform1i(subsurface_sampler, 4);
            gl::Uniform1i(rock_sampler, 5);
            gl::Uniform1i(sand_sampler, 6);
            gl::Uniform1i(
                get_uniform_location(grid_program, "u_terrain_snow_texture"),
                11,
            );
            gl::Uniform1i(
                get_uniform_location(grid_program, "u_terrain_mud_texture"),
                12,
            );
            for n in 0..4 {
                let location = get_uniform_location(grid_program, &format!("u_paint_page{n}"));
                gl::Uniform1i(location, 7 + n);
            }
            gl::Uniform1i(
                get_uniform_location(grid_program, "u_terrain_shore_points"),
                15,
            );
            gl::Uniform1i(
                get_uniform_location(grid_program, "u_ocean_contact_atlas"),
                13,
            );
            gl::Uniform1i(
                get_uniform_location(grid_program, "u_terrain_layer_pass"),
                0,
            );
            gl::Uniform1i(renderer.grid_paint_flags_location, 0);
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

    #[cfg(test)]
    pub(crate) fn new_for_tests(width: f32, height: f32) -> Self {
        Self {
            capabilities: GraphicsCapabilities::default(),
            lighting_state: RefCell::new(RendererLightingState::default()),
            grid_program: 0,
            #[cfg(feature = "editor")]
            graph_preview_programs: RefCell::new(HashMap::new()),
            graph_baked_textures: RefCell::new(HashMap::new()),
            layer_source_images: RefCell::new(HashMap::new()),
            water_preview_program: RefCell::new(None),
            landscape_preview_mesh: RefCell::new(None),
            water_program: 0,
            water_started: Instant::now(),
            water_view_projection_location: -1,
            water_camera_position_location: -1,
            water_time_location: -1,
            water_camera_underwater_location: -1,
            water_wave_height_location: -1,
            water_wave_speed_location: -1,
            water_wave_length_location: -1,
            water_wave_direction_location: -1,
            water_wave_steepness_location: -1,
            water_ocean_parity_wave_location: -1,
            water_absorption_color_location: -1,
            water_absorption_strength_location: -1,
            water_scattering_color_location: -1,
            water_scattering_strength_location: -1,
            water_local_flow_location: -1,
            water_foam_amount_location: -1,
            water_foam_scale_location: -1,
            water_shallow_color_location: -1,
            water_deep_color_location: -1,
            water_foam_color_location: -1,
            water_underwater_color_location: -1,
            water_opacity_location: -1,
            water_underwater_opacity_location: -1,
            water_reflection_strength_location: -1,
            water_specular_strength_location: -1,
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
            grid_ocean_optics_mode_location: -1,
            grid_underwater_camera_position_location: -1,
            grid_underwater_color_location: -1,
            grid_underwater_density_location: -1,
            grid_underwater_depth_fog_location: -1,
            grid_underwater_depth_fog_limit_location: -1,
            grid_underwater_fog_limit_location: -1,
            grid_underwater_surface_y_location: -1,
            grid_atmosphere_enabled_location: -1,
            grid_atmosphere_camera_position_location: -1,
            grid_fog_color_location: -1,
            grid_fog_start_location: -1,
            grid_fog_end_location: -1,
            grid_terrain_blend_enabled_location: -1,
            grid_terrain_layer_pass_location: -1,
            grid_material_preview_location: -1,
            grid_cell_pbr_location: -1,
            grid_terrain_source_role_location: -1,
            grid_terrain_vertex_layers_location: -1,
            grid_terrain_default_layer_location: -1,
            grid_terrain_water_level_location: -1,
            grid_ocean_wetness_location: -1,
            grid_ocean_wetness_direction_location: -1,
            grid_terrain_blend_params_location: -1,
            grid_paint_origin_location: -1,
            grid_paint_flags_location: -1,
            grid_point_light_count_location: -1,

            point_light_stream: Default::default(),

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
            #[cfg(feature = "editor")]
            brush_ring_cache: RefCell::new([None, None]),
            water_wakes: RefCell::new(WaterWakeState::default()),
            fluid_gpu: RefCell::new(None),
            paint_gpu: RefCell::new(Default::default()),
            last_render_revision: RefCell::new(0),
            last_render_mode: RefCell::new(None),

            ghost_cache: RefCell::new(HashMap::new()),
            last_ghost_revision: RefCell::new(0),
            last_ghost_mode: RefCell::new(None),

            billboard_vao: 0,
            billboard_vbo: 0,

            character_vao: 0,
            character_vbo: 0,
            gpu_skin_state: RefCell::new(Default::default()),
            grid_skin_enabled_location: -1,
            grid_skin_matrices_location: -1,
            grid_skin_material_color_location: -1,
            entity_mesh_cache: RefCell::new(HashMap::new()),

            shadow_program: 0,
            shadow_fbo: 0,
            shadow_depth_tex: 0,
            point_shadow_layers: RefCell::new(0),
            point_shadow_cache: RefCell::new(Default::default()),

            textures: RefCell::new(HashMap::new()),
            materials: RefCell::new(std::sync::Arc::new(
                crate::project::material::MaterialCatalog::default(),
            )),
            project_assets_dir: RefCell::new(None),
            cubemaps: RefCell::new(HashMap::new()),
            active_sky_material: RefCell::new(None),
            fallback_tex: 0,

            contact_foam: RefCell::new(contact_foam::ContactFoamDepth::no_gl()),
            water_volume_mask: RefCell::new(water_volume_mask::WaterVolumeGpu::no_gl()),
            sky_renderer: sky::SkyRenderer {
                program: 0,
                vao: 0,
                vbo: 0,
                view_projection_location: -1,
                model_location: -1,
                sky_stop_count_location: -1,
                sky_positions_location: -1,
                sky_colors_location: -1,
                sky_smooth_location: -1,
                underwater_enabled_location: -1,
                underwater_color_location: -1,
                underwater_blend_location: -1,
                underwater_camera_y_location: -1,
                local_water_camera_xz_location: -1,
                underwater_surface_y_location: -1,
                atmosphere_enabled_location: -1,
                fog_color_location: -1,
                horizon_strength_location: -1,
            },
            mirage: RefCell::new(mirage::MiragePass::default()),
            post_process: RefCell::new(post_process::ScenePostProcess::default()),
            #[cfg(feature = "editor")]
            lighting_preview: RefCell::new(lighting_preview::LightingPreviewCapture::default()),
            profiler: RefCell::new(None),
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
        self.calculate_light_space_matrix_with_range(light_direction, camera_pos, 1.0)
    }

    pub(super) fn calculate_light_space_matrix_with_range(
        &self,
        light_direction: Vec3,
        camera_pos: Vec3,
        range_scale: f32,
    ) -> Mat4 {
        let range_scale = range_scale.max(1.0);
        let dir = if light_direction.length_squared() > 0.000001 {
            light_direction.normalize()
        } else {
            Vec3::new(-0.5, -1.0, -0.5).normalize()
        };

        let shadow_center = camera_pos;
        let light_pos = shadow_center - dir * (SHADOW_LIGHT_DISTANCE * range_scale);

        let up = if dir.x.abs() < 0.0001 && dir.z.abs() < 0.0001 {
            Vec3::Z
        } else {
            Vec3::Y
        };

        let light_view = Mat4::look_at_rh(light_pos, shadow_center, up);

        let light_proj = glam::camera::rh::proj::opengl::orthographic(
            -SHADOW_ORTHO_HALF_EXTENT * range_scale,
            SHADOW_ORTHO_HALF_EXTENT * range_scale,
            -SHADOW_ORTHO_HALF_EXTENT * range_scale,
            SHADOW_ORTHO_HALF_EXTENT * range_scale,
            SHADOW_NEAR,
            SHADOW_FAR * range_scale,
        );

        light_proj * light_view
    }

    pub(super) fn collect_point_lights(&self, world: &World) -> Vec<PointLightData> {
        let mut point_lights = Vec::new();

        for light_id in world.iter_light_ids() {
            let Some(position) = world.light_world_position(light_id) else {
                continue;
            };
            if !world.is_light_enabled_by_id(light_id) {
                continue;
            }

            let Some(cell) = world.get_effective_cell_by_id(light_id) else {
                continue;
            };

            if cell.cell_type == CellType::Light && world.is_light_enabled_by_id(light_id) {
                let Some((light_color, light_intensity, light_range, light_shadows)) =
                    world.emitted_light_runtime(light_id)
                else {
                    continue;
                };
                point_lights.push(PointLightData {
                    id: light_id,
                    casts_shadows: light_shadows,
                    shadow_slot: -1,
                    position,
                    color: light_color,
                    intensity: light_intensity,
                    range: light_range,
                });
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
            if std::env::var("WORLDKILN_PERF_TRACE").as_deref() == Ok("1") {
                static GI_SAMPLE: std::sync::atomic::AtomicU64 =
                    std::sync::atomic::AtomicU64::new(0);
                if GI_SAMPLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed) % 120 == 0 {
                    let stats = self.gi_cache.borrow().stats();
                    eprintln!(
                        "[PERF GI] update_ms={:.2} probes={} traced_rays={} scene_voxels={} dynamic_aabbs={}",
                        stats.update_cpu_ms,
                        stats.updated_probes,
                        stats.traced_rays,
                        stats.scene_voxels,
                        stats.dynamic_aabbs
                    );
                }
            }
        }
    }

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
    pub(super) fn bind_underwater_uniforms(
        &self,
        state: Option<terrain::UnderwaterState>,
        camera_position: Vec3,
        mode: crate::world::OceanOpticsMode,
    ) {
        unsafe {
            gl::Uniform1i(self.grid_ocean_optics_mode_location, mode.shader_id());
            gl::Uniform1i(
                self.grid_underwater_enabled_location,
                i32::from(state.is_some_and(|water| water.enabled)),
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
                gl::Uniform1f(self.grid_underwater_depth_fog_location, state.depth_fog);
                gl::Uniform1f(
                    self.grid_underwater_depth_fog_limit_location,
                    state.depth_fog_limit,
                );
                gl::Uniform1f(self.grid_underwater_fog_limit_location, state.fog_limit);
                gl::Uniform1f(self.grid_underwater_surface_y_location, state.surface_y);
            } else {
                gl::Uniform3f(self.grid_underwater_color_location, 0.0, 0.0, 0.0);
                gl::Uniform1f(self.grid_underwater_density_location, 0.0);
                gl::Uniform1f(self.grid_underwater_depth_fog_location, 0.0);
                gl::Uniform1f(self.grid_underwater_depth_fog_limit_location, 0.0);
                gl::Uniform1f(self.grid_underwater_fog_limit_location, 0.0);
                gl::Uniform1f(self.grid_underwater_surface_y_location, 0.0);
            }
        }
    }
    pub(super) fn bind_atmosphere_uniforms(&self, world: &World, camera_position: Vec3) {
        let enabled = world.sky.atmosphere_enabled;
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
    pub(super) id: u64,
    pub(super) casts_shadows: bool,
    pub(super) shadow_slot: i32,
    pub(super) position: Vec3,
    pub(super) color: Vec3,
    pub(super) intensity: f32,
    pub(super) range: f32,
}

impl Drop for Renderer {
    fn drop(&mut self) {
        effekseer::shutdown();
        self.gi_cache.get_mut().free_gl_resources();
        self.point_light_stream.free();
        self.water_volume_mask.get_mut().free();
        self.contact_foam.get_mut().free();
        self.gpu_skin_state.get_mut().free();
        if let Some(mut fluid) = self.fluid_gpu.get_mut().take() {
            fluid.free();
        }
        self.mirage.get_mut().free();
        self.post_process.get_mut().free();
        #[cfg(feature = "editor")]
        self.lighting_preview.get_mut().free();
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

            #[cfg(feature = "editor")]
            for ring in self.brush_ring_cache.get_mut().iter_mut() {
                if let Some(gpu) = ring.take() {
                    if gpu.vao != 0 {
                        gl::DeleteVertexArrays(1, &gpu.vao);
                    }
                    if gpu.vbo != 0 {
                        gl::DeleteBuffers(1, &gpu.vbo);
                    }
                }
            }
            if let Some((vao, vbo, _)) = self.landscape_preview_mesh.get_mut().take() {
                if vao != 0 {
                    gl::DeleteVertexArrays(1, &vao);
                }
                if vbo != 0 {
                    gl::DeleteBuffers(1, &vbo);
                }
            }
            if let Some(Ok(preview)) = self.water_preview_program.get_mut().take() {
                if preview.program != 0 {
                    gl::DeleteProgram(preview.program);
                }
            }
            for (_, texture) in self.graph_baked_textures.get_mut().drain() {
                if let Ok(texture) = texture {
                    if texture != 0 {
                        gl::DeleteTextures(1, &texture);
                    }
                }
            }
            #[cfg(feature = "editor")]
            for (_, variant) in self.graph_preview_programs.get_mut().drain() {
                if let Ok(variant) = variant {
                    if variant.program != 0 {
                        gl::DeleteProgram(variant.program);
                    }
                }
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
layout (location = 4) in vec4 a_joints;
layout (location = 5) in vec4 a_weights;

uniform bool u_skin_enabled;
uniform samplerBuffer u_skin_matrices;
uniform vec4 u_skin_material_color;
uniform mat4 u_view_projection;
uniform mat4 u_model;
uniform vec3 u_base_color;
uniform bool u_terrain_vertex_layers;

out vec4 v_layer_primary;
out vec2 v_layer_secondary;
out vec3 v_normal;
out vec4 v_color;
out vec3 v_world_pos;
out vec2 v_uv;

void main() {
    // GPU skinning keeps bind-pose geometry resident in the GPU. Each bone
    // matrix is supplied as four RGBA32F texels; other geometry is unchanged.
    vec3 position = a_position;
    vec3 normal = a_normal;
    if (u_skin_enabled) {
        position = vec3(0.0);
        normal = vec3(0.0);
        for (int i = 0; i < 4; ++i) {
            float weight = a_weights[i];
            if (weight <= 0.0) continue;
            int offset = int(a_joints[i]) * 4;
            mat4 skin = mat4(
                texelFetch(u_skin_matrices, offset),
                texelFetch(u_skin_matrices, offset+1),
                texelFetch(u_skin_matrices, offset+2),
                texelFetch(u_skin_matrices, offset+3));
            position += (skin * vec4(a_position, 1.0)).xyz * weight;
            normal += mat3(skin) * a_normal * weight;
        }
        normal = dot(normal, normal) > 1e-12 ? normalize(normal) : vec3(0.0, 1.0, 0.0);
    }
    vec4 world_pos =
        u_model *
        vec4(position, 1.0);

    v_world_pos =
        world_pos.xyz;

    gl_Position =
        u_view_projection *
        world_pos;

    v_normal =
        mat3(u_model) *
        normal;

    // Material-weight channels are only used on voxel terrain. Other
    // editor objects retain their conventional vertex colors and UVs.
    v_layer_primary = a_color;
    v_layer_secondary = a_uv;
    vec4 actor_color = u_skin_enabled ? (a_color*u_skin_material_color) : a_color;
    v_color = u_terrain_vertex_layers ? vec4(1.0) :
        vec4(actor_color.rgb*u_base_color,actor_color.a);
    v_uv = a_uv;
}
"#;

const GRID_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec3 v_normal;
in vec4 v_color;
in vec4 v_layer_primary;
in vec2 v_layer_secondary;
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

uniform int u_point_light_count;
// A texture-buffer stream is indexed by Light, without a fixed uniform array.
uniform samplerBuffer u_point_light_data;

uniform sampler2DArray u_shadow_map;
uniform mat4 u_light_space_matrix;
uniform bool u_shadows_enabled;
uniform float u_alpha;
// x = roughness, y = metallic, z = editor preview enabled.
// This branch is off for normal World/Editor/Play scene drawing.
uniform vec3 u_material_preview;
// x = roughness, y = metallic, z = authored Surface Material on a Cell.
// Unassigned Cells and Landscape leave this disabled.
uniform vec3 u_cell_pbr;

uniform sampler2D u_texture;
uniform bool u_use_texture;

// Worldkiln 0.9.3 | WO 0.9.3-20 | Smooth Terrain layer blending
uniform bool u_terrain_blend_enabled;
uniform int u_terrain_source_role;
uniform bool u_terrain_vertex_layers;
uniform int u_terrain_default_layer;
uniform float u_terrain_water_level;
// WK-WATER-015: (wave height, speed, length, elapsed seconds).
uniform vec4 u_ocean_wetness;
uniform sampler2D u_ocean_contact_atlas;
uniform vec4 u_ocean_contact_mapping;
uniform bool u_terrain_ocean_contact_enabled;
uniform bool u_water_basic;
uniform int u_contact_count;
uniform vec4 u_contact_bounds[24];
uniform vec4 u_contact_props[24];
uniform float u_ocean_wetness_direction;
uniform float u_ocean_wetness_seed;
uniform vec4 u_terrain_blend_params; // rock start/end, dirt strength, beach width
uniform sampler2D u_terrain_surface_texture;
uniform sampler2D u_terrain_subsurface_texture;
uniform sampler2D u_terrain_rock_texture;
uniform sampler2D u_terrain_sand_texture;
uniform sampler2D u_terrain_snow_texture;
uniform sampler2D u_terrain_mud_texture;
// Native 0.9 paintmask pages: 32x32 studs, 0.25 studs per texel.
uniform int u_terrain_layer_pass;
uniform samplerBuffer u_terrain_shore_points;
uniform int u_terrain_shore_count;
// x: kind (0 lake, 1 river), y: elevation, z: bank width, w: ocean enabled
uniform vec4 u_terrain_shore_params;
float terrain_shore_coverage(vec3 p) {
    float signed_distance=-1e20;
    float height=u_terrain_shore_params.y;
    if (u_terrain_layer_pass==3) {
        if (u_terrain_shore_params.w<0.5) return 0.0;
        signed_distance=1e20;
    } else {
        bool inside=false;
        float minimum_distance=1e20;
        int count=u_terrain_shore_count;
        for (int i=0;i<count;i++) {
            if (u_terrain_shore_params.x>0.5 && i+1==count) break;
            vec4 a=texelFetch(u_terrain_shore_points,i);
            vec4 b=texelFetch(u_terrain_shore_points,(i+1)%count);
            vec2 segment=b.xy-a.xy;
            float t=clamp(dot(p.xz-a.xy,segment)/max(dot(segment,segment),1e-10),0.0,1.0);
            float distance=length(p.xz-mix(a.xy,b.xy,t));
            if (distance<minimum_distance) {
                minimum_distance=distance;
                height=u_terrain_shore_params.y+mix(a.w,b.w,t);
            }
            if (u_terrain_shore_params.x>0.5) signed_distance=max(signed_distance,mix(a.z,b.z,t)*0.5-distance);
            else if ((a.y>p.z)!=(b.y>p.z) && p.x<(b.x-a.x)*(p.z-a.y)/(b.y-a.y)+a.x) inside=!inside;
        }
        if (u_terrain_shore_params.x<0.5) signed_distance=inside?minimum_distance:-minimum_distance;
    }
    // Same soil-only submerged extent / small dry band as generation. Shore
    // shading never changes authored density, sculpt or saved explicit paint.
    float eligible=u_terrain_vertex_layers ? 1.0-clamp(v_layer_primary.b+v_layer_secondary.x+v_layer_secondary.y,0.0,1.0) : ((u_terrain_default_layer==2 || u_terrain_default_layer==4 || u_terrain_default_layer==5)?0.0:1.0);
    float vertical=1.0-smoothstep(height+1.75,height+2.0,p.y);
    float lateral=1.0-smoothstep(0.0,max(u_terrain_shore_params.z,0.001),max(-signed_distance,0.0));
    return eligible*vertical*lateral;
}
uniform sampler2D u_paint_page0;
uniform sampler2D u_paint_page1;
uniform sampler2D u_paint_page2;
uniform sampler2D u_paint_page3;
uniform vec2 u_paint_origin;
uniform int u_paint_flags;
vec4 sample_native_paint(vec2 world_xz) {
    // No native paint pages bound: every sample is exactly zero. Avoid
    // page/UV math for the common unpainted landscape.
    if (u_paint_flags==0) return vec4(0.0);
    vec2 page=(world_xz-u_paint_origin)/32.0;
    ivec2 cell=ivec2(floor(page));
    // Each 128² weightmap page has a one-texel true-neighbor halo. UVs
    // interpolate seamlessly at page and 64-stud render-chunk boundaries.
    // No page-specific clamp, hard branch or material-weight discontinuity.
    vec2 uv=(fract(page)*128.0+1.0)/130.0;
    if (cell.x==0 && cell.y==0 && (u_paint_flags&1)!=0)
        return texture(u_paint_page0,uv);
    if (cell.x==1 && cell.y==0 && (u_paint_flags&2)!=0)
        return texture(u_paint_page1,uv);
    if (cell.x==0 && cell.y==1 && (u_paint_flags&4)!=0)
        return texture(u_paint_page2,uv);
    if (cell.x==1 && cell.y==1 && (u_paint_flags&8)!=0)
        return texture(u_paint_page3,uv);
    return vec4(0.0);
}


uniform int u_debug_view;
uniform int u_lighting_method;
uniform bool u_indirect_enabled;
uniform sampler3D u_gi_cache;
uniform vec3 u_gi_origin;
uniform float u_gi_spacing;
uniform ivec3 u_gi_dimensions;
uniform int u_gi_denoising;

uniform bool u_underwater_enabled;
uniform int u_ocean_optics_mode; // 0=Off, 1=Height, 2=Ray
uniform vec3 u_underwater_camera_position;
uniform vec3 u_underwater_color;
uniform float u_underwater_density;
uniform float u_underwater_depth_fog;
uniform float u_underwater_depth_fog_limit;
uniform float u_underwater_fog_limit;
uniform float u_underwater_surface_y;

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
        textureSize(u_shadow_map, 0).xy;

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
                    vec3(shadow_coord.xy + vec2(x, y) * texel_size, 0.0)
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


// Six perspective shadow views per point Light, packed in 512px face tiles
// in depth-array layers after the directional layer (0). Lights outside
// the user-controlled camera shadow distance still illuminate normally.
float point_shadow_visibility(vec3 ray, float light_range, float slot, vec3 normal) {
    if (slot < -0.5) { return 1.0; }
    vec3 v = ray + normal * 0.045;
    vec3 a = abs(v);
    float forward = max(max(a.x, a.y), a.z);
    if (forward <= 0.10 || forward >= light_range) { return 1.0; }
    float face = 0.0;
    vec2 face_uv;
    if (a.x >= a.y && a.x >= a.z) {
        face = v.x >= 0.0 ? 0.0 : 1.0;
        face_uv = vec2(v.x >= 0.0 ? -v.z : v.z, -v.y) / a.x;
    } else if (a.y >= a.z) {
        face = v.y >= 0.0 ? 2.0 : 3.0;
        face_uv = vec2(v.x, v.y >= 0.0 ? v.z : -v.z) / a.y;
    } else {
        face = v.z >= 0.0 ? 4.0 : 5.0;
        face_uv = vec2(v.z >= 0.0 ? v.x : -v.x, -v.y) / a.z;
    }
    float tiles_per_axis = __POINT_SHADOW_TILES_PER_AXIS__;
    float tiles_per_layer = tiles_per_axis * tiles_per_axis;
    float tile = slot * 6.0 + face;
    float local_tile = mod(tile, tiles_per_layer);
    float layer = 1.0 + floor(tile / tiles_per_layer);
    vec2 pixel = vec2(mod(local_tile, tiles_per_axis), floor(local_tile / tiles_per_axis));
    vec2 local_uv = clamp(face_uv * 0.5 + 0.5, vec2(0.003), vec2(0.997));
    vec2 atlas_uv = (pixel + local_uv) / tiles_per_axis;
    float shadow_depth = texture(u_shadow_map, vec3(atlas_uv, layer)).r;
    float near_plane = 0.1;
    float biased_distance = max(near_plane, forward - max(0.025, forward * 0.003));
    float projected_depth = light_range / (light_range - near_plane)
        - light_range * near_plane / ((light_range - near_plane) * biased_distance);
    return shadow_depth + 0.0005 < projected_depth ? 0.0 : 1.0;
}

// WK-WATER-013 | 2026-10-12 — camera-ray length inside local Lake/River
// volume. GPU atlas contains XZ spline signed distance + top/bottom planes.
#define WK_LOCAL_WATER_BODIES 8
uniform sampler2DArray u_local_water_atlas;
uniform int u_local_water_count;
uniform vec4 u_local_water_rect[WK_LOCAL_WATER_BODIES];
uniform vec4 u_local_water_fog[WK_LOCAL_WATER_BODIES];
uniform vec4 u_local_water_params[WK_LOCAL_WATER_BODIES];
uniform vec4 u_local_water_waves[WK_LOCAL_WATER_BODIES];
uniform float u_local_water_time;
// Approximate the displaced crest at the pixel's water-column position,
// using the SAME dominant components, phase and wave material as the
// actual rendered water mesh. Compute only near the surface.
float wk_local_wave_crest(vec2 p,int i,float depth) {
    vec4 wave=u_local_water_waves[i];
    float c=cos(wave.w),s=sin(wave.w);
    vec2 uv=vec2(c*p.x-s*p.y,s*p.x+c*p.y)*
        (50.0/max(wave.z,1.0));
    float t=u_local_water_time*wave.y;
    float w=sin(dot(uv,vec2(0.115,0.072))+t*0.90)*0.18
          +sin(dot(uv,vec2(-0.083,0.145))+t*1.27+1.7)*0.09
          +sin(dot(uv,vec2(0.215,-0.047))+t*0.54+3.1)*0.045;
    float normalized_depth=clamp(depth/20.0,0.0,1.0);
    float shore=mix(0.08,1.0,smoothstep(0.03,0.48,normalized_depth));
    return w*wave.x*shore;
}


vec2 wk_volume_segment(vec3 ray_a,vec3 ray_b,int i){
    vec4 rect=u_local_water_rect[i];
    vec2 lo=rect.xy;
    vec2 hi=rect.xy+1.0/rect.zw;
    vec2 origin_xz=ray_a.xz;
    vec2 delta=ray_b.xz-origin_xz;
    float enter=0.0,exit=1.0;
    for(int axis=0;axis<2;++axis){
        if(abs(delta[axis])<0.00001){
            if(origin_xz[axis]<lo[axis] || origin_xz[axis]>hi[axis]){
                return vec2(0.0,0.0);
            }
        }else{
            float a=(lo[axis]-origin_xz[axis])/delta[axis];
            float b=(hi[axis]-origin_xz[axis])/delta[axis];
            enter=max(enter,min(a,b));
            exit=min(exit,max(a,b));
        }
    }
    if(exit<=enter){return vec2(0.0,0.0);}
    float ray_length=length(ray_b-ray_a);
    if(ray_length<0.0001){return vec2(0.0,0.0);}
    float wet_length=0.0;
    float maximum_depth=0.0;
    // Constrained to the body's own footprint; only ray intersections with
    // the spline footprint AND water top/floor count. Fixed bounded budget.
    for(int step=0;step<32;++step){
        float t=mix(enter,exit,(float(step)+0.5)/32.0);
        vec3 p=mix(ray_a,ray_b,t);
        vec2 uv=(p.xz-rect.xy)*rect.zw;
        vec4 column=texture(u_local_water_atlas,vec3(uv,float(i)));
        if(column.a<=0.02 || column.b<=0.0){continue;}
        float edge=clamp(column.b*1.5,0.0,1.0)*clamp(column.a*3.0,0.0,1.0);
        float surface=column.r;
        if(abs(p.y-surface)<1.6 && u_local_water_waves[i].x>0.0){
            surface+=wk_local_wave_crest(p.xz,i,surface-column.g);
        }
        if(p.y>=column.g && p.y<surface){
            wet_length+=ray_length*(exit-enter)/32.0*edge;
            maximum_depth=max(maximum_depth,surface-p.y);
        }
    }
    return vec2(wet_length,maximum_depth);
}
vec4 wk_apply_local_water(vec3 original,vec3 camera,vec3 target,bool is_sky){
    vec3 final_color=original;
    float strongest=0.0;
    float any_volume=0.0;
    for(int i=0;i<WK_LOCAL_WATER_BODIES;++i){
        if(i>=u_local_water_count)break;
        vec4 fog=u_local_water_fog[i];
        vec4 params=u_local_water_params[i];
        vec2 wet=wk_volume_segment(camera,target,i);
        if(wet.x<=0.0)continue;
        any_volume=1.0;
        float fog_amount=1.0-exp(-max(fog.a,0.0)*wet.x);
        float deep=clamp(wet.y*max(params.x,0.0),0.0,params.y);
        float blend=clamp(fog_amount+deep,0.0,params.z);
        if(is_sky){blend=max(blend,params.w*clamp(wet.x/12.0,0.0,1.0));}
        // Deterministic priority for overlapping bodies: strongest optical
        // contribution wins, independent of iteration order.
        if(blend>strongest){
            strongest=blend;
            final_color=mix(original,fog.rgb,blend);
        }
    }
    return vec4(final_color,any_volume);
}

// WK-WAVES-009: contact with the same three seeded ocean bands used by
// actual displaced water and the terrain-following beach wash.
float wk_wet_seed_hash(float x) {
    return fract(sin(x*17.197+13.123)*43758.5453);
}
float wk_ocean_wet_wave(vec2 p,float t) {
    float k=6.28318530718/max(u_ocean_wetness.z,1.0);
    float seed=wk_wet_seed_hash(u_ocean_wetness_seed+0.123);
    float a=-dot(p,vec2(cos(u_ocean_wetness_direction),
                          sin(u_ocean_wetness_direction)))*k+
            t*u_ocean_wetness.y;
    float angle2=u_ocean_wetness_direction+0.43+seed*0.47;
    float angle3=u_ocean_wetness_direction-0.91+
        wk_wet_seed_hash(u_ocean_wetness_seed+7.0)*0.39;
    float b=-dot(p,vec2(cos(angle2),sin(angle2)))*k*1.71+
        t*u_ocean_wetness.y*1.18+seed*6.28318530718;
    float c=-dot(p,vec2(cos(angle3),sin(angle3)))*k*0.58+
        t*u_ocean_wetness.y*0.74+
        wk_wet_seed_hash(u_ocean_wetness_seed+5.0)*6.28318530718;
    return sin(a)*0.72+sin(b)*0.19+sin(c)*0.09;
}
float wk_ocean_contact(vec3 p,float t) {
    float elevation=p.y-u_terrain_water_level;
    float wave=wk_ocean_wet_wave(p.xz,t);
    if (elevation>=0.0) {
        float reach=min(max(u_ocean_wetness.x,0.0)*0.62,5.0);
        float front=reach*clamp(0.5+0.5*wave,0.0,1.0);
        return 1.0-smoothstep(front-0.18,front+0.18,elevation);
    }
    float depth=smoothstep(0.0,16.0,u_ocean_wetness.x-elevation);
    float wave_surface=wave*u_ocean_wetness.x*smoothstep(0.0,0.20,depth);
    return 1.0-smoothstep(wave_surface-0.18,wave_surface+0.18,elevation);
}

void main() {
    vec3 normal =
        normalize(v_normal);

    vec4 tex_color = vec4(1.0);

    if (u_terrain_layer_pass>0) {
        tex_color.rgb=texture(u_texture,v_world_pos.xz).rgb;
        if (u_terrain_layer_pass==1) tex_color.a=sample_native_paint(v_world_pos.xz).a;
        else tex_color.a=terrain_shore_coverage(v_world_pos)*(1.0-sample_native_paint(v_world_pos.xz).a);
        if (tex_color.a<=0.00001) discard;
    } else if (u_terrain_blend_enabled) {
        vec2 terrain_uv = v_world_pos.xz;
        // Defer texture reads until the *actual* layer weights are known.
        // Most Terrain fragments need only one or two of the six materials.
        // Explicit gradients keep mipmap derivatives defined across divergent
        // material boundaries, matching the original texture() filtering.
        vec2 terrain_dx = dFdx(terrain_uv);
        vec2 terrain_dy = dFdy(terrain_uv);

        float up = clamp(normal.y, 0.0, 1.0);
        float slope = 1.0 - up;
        float rock_mask = smoothstep(u_terrain_blend_params.x, u_terrain_blend_params.y, slope);

        float shore_height = abs(v_world_pos.y - u_terrain_water_level);
        float shore_mask = 1.0 - smoothstep(0.6, max(0.61,u_terrain_blend_params.w), shore_height);

        // WK-MAT-011 | 2026-10-11: Interpolate all six material weights
        // from shared voxel vertices, not a discrete palette batch role.
        // This also removes whole 64-stud source-role changes when a paint
        // atlas page happens to be present in one chunk.
        vec4 base0 = vec4(0.0);
        vec2 base1 = vec2(0.0);
        if (u_terrain_vertex_layers) {
            base0 = max(v_layer_primary,vec4(0.0));
            base1 = max(v_layer_secondary,vec2(0.0));
        } else {
            if (u_terrain_default_layer == 0) base0.r = 1.0;
            else if (u_terrain_default_layer == 1) base0.g = 1.0;
            else if (u_terrain_default_layer == 2) base0.b = 1.0;
            else if (u_terrain_default_layer == 3) base0.a = 1.0;
            else if (u_terrain_default_layer == 4) base1.x = 1.0;
            else if (u_terrain_default_layer == 5) base1.y = 1.0;
            else base0.r = 1.0;
        }
        float authored_total = dot(base0,vec4(1.0)) + dot(base1,vec2(1.0));
        if (authored_total < 0.0001) {base0.r=1.0;authored_total=1.0;}
        base0 /= authored_total;
        base1 /= authored_total;

        // Procedural slope, soil and shoreline effects are secondary,
        // gradual modifiers. They never zero out authored Snow/Mud layers.
        float cliff = rock_mask * 0.54;
        float beach = 0.0; // Shore layers are composed below through their stable IDs.
        float grass = base0.r * (1.0 - cliff) * (1.0 - beach);
        float dirt = base0.g + smoothstep(0.2,0.85,slope)
            * (1.0-rock_mask) * u_terrain_blend_params.z;
        float rock = base0.b + cliff * (1.0 - base1.x);
        float sand = base0.a + beach * (1.0 - base1.x);
        float snow = base1.x;
        float mud = base1.y;
        float total_weight=max(grass+dirt+rock+sand+snow+mud,0.0001);

        // Preserve the exact six-layer and painted-layer composition, but
        // fetch only materials that contribute to the current pixel. Zero
        // weights are common across large smooth terrain surfaces. Gradients
        // were captured before branches to preserve mip filtering at seams.
        vec4 native_paint=sample_native_paint(v_world_pos.xz);
        float coverage=clamp(native_paint.a,0.0,1.0);
        float painted_dirt=coverage>0.0001 ? clamp(native_paint.r,0.0,1.0) : 0.0;
        float painted_rock=coverage>0.0001 ? clamp(native_paint.g,0.0,1.0) : 0.0;
        float painted_sand=coverage>0.0001 ? clamp(native_paint.b,0.0,1.0) : 0.0;
        vec3 blended_sum=vec3(0.0);
        vec3 painted_sum=vec3(0.0);
        if (grass>0.0) {
            blended_sum+=textureGrad(u_terrain_surface_texture,terrain_uv,terrain_dx,terrain_dy).rgb*grass;
        }
        if (dirt>0.0 || painted_dirt>0.0) {
            vec3 value=textureGrad(u_terrain_subsurface_texture,terrain_uv,terrain_dx,terrain_dy).rgb;
            blended_sum+=value*dirt;
            painted_sum+=value*painted_dirt;
        }
        if (rock>0.0 || painted_rock>0.0) {
            vec3 value=textureGrad(u_terrain_rock_texture,terrain_uv,terrain_dx,terrain_dy).rgb;
            blended_sum+=value*rock;
            painted_sum+=value*painted_rock;
        }
        if (sand>0.0 || painted_sand>0.0) {
            vec3 value=textureGrad(u_terrain_sand_texture,terrain_uv,terrain_dx,terrain_dy).rgb;
            blended_sum+=value*sand;
            painted_sum+=value*painted_sand;
        }
        if (snow>0.0) {
            blended_sum+=textureGrad(u_terrain_snow_texture,terrain_uv,terrain_dx,terrain_dy).rgb*snow;
        }
        if (mud>0.0) {
            blended_sum+=textureGrad(u_terrain_mud_texture,terrain_uv,terrain_dx,terrain_dy).rgb*mud;
        }
        tex_color.rgb=blended_sum/total_weight;
        // WK-MAT-011: paint remains continuously interpolated with original
        // authored layer; no removed texture, material, or paint channel.
        if (coverage>0.0001) {
            tex_color.rgb=mix(tex_color.rgb,painted_sum/max(coverage,0.0001),coverage);
        }
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
        vec4 position_range = texelFetch(u_point_light_data, 3 * i);
        vec4 color_intensity = texelFetch(u_point_light_data, 3 * i + 1);
        vec4 shadow_info = texelFetch(u_point_light_data, 3 * i + 2);
        vec3 L_vec = position_range.xyz - v_world_pos;

        float dist =
            length(L_vec);

        if (
            dist > 0.00001 &&
            dist < position_range.w
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
                    position_range.w
                );

            attenuation *=
                attenuation;

            point_contribution +=
                color *
                diff *
                color_intensity.rgb *
                color_intensity.a *
                attenuation *
                point_shadow_visibility(-L_vec, shadow_info.y, shadow_info.x, normal);
        }
    }

    vec3 result =
        ambient +
        diffuse +
        point_contribution;

    if (u_material_preview.z > 0.5) {
        // The shared scene shader lights the same cube mesh used by Cells.
        // Preview-only specular response makes authored roughness/metallic
        // legible while leaving ordinary scene rendering unchanged.
        float roughness = clamp(u_material_preview.x, 0.04, 1.0);
        float metallic = clamp(u_material_preview.y, 0.0, 1.0);
        vec3 L = normalize(-u_global_light_direction);
        vec3 V = normalize(vec3(2.3, 1.85, 2.6) - v_world_pos);
        vec3 H = normalize(L + V);
        float ndotl = max(dot(normal, L), 0.0);
        float gloss_power = mix(160.0, 6.0, roughness * roughness);
        float specular = pow(max(dot(normal, H), 0.0), gloss_power) * ndotl;
        vec3 f0 = mix(vec3(0.04), color, metallic);
        result = ambient + diffuse * (1.0 - metallic * 0.7) +
            f0 * specular * u_global_light_intensity * 1.5 +
            point_contribution;
    } else if (u_cell_pbr.z > 0.5) {
        // WK-MAT-012: Cell Surface Material uses its authored PBR constants.
        // Read the live camera instead of the fixed Material preview camera.
        float roughness = clamp(u_cell_pbr.x, 0.04, 1.0);
        float metallic = clamp(u_cell_pbr.y, 0.0, 1.0);
        vec3 L = normalize(-u_global_light_direction);
        vec3 V = normalize(u_atmosphere_camera_position - v_world_pos);
        vec3 H = normalize(L + V);
        float ndotl = max(dot(normal, L), 0.0);
        float gloss_power = mix(160.0, 6.0, roughness * roughness);
        float specular = pow(max(dot(normal, H), 0.0), gloss_power) * ndotl;
        vec3 f0 = mix(vec3(0.04), color, metallic);
        result = ambient + diffuse * (1.0 - metallic * 0.7) +
            f0 * specular * u_global_light_intensity * 1.5 + point_contribution;
    }

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

    // WK-WAVES-007: object-local wetness uses persistent collider contact
    // history and keeps the material untouched. Applies to all grid-shader
    // geometry, including Actors and Cells, as world-space fragments.
    float object_wet=0.0;
    for(int i=0;i<8;++i){
        if (u_water_basic) break;
        // Landscape already has its own accepted contact wetness shader.
        if(i>=u_contact_count || u_terrain_layer_pass!=0) break;
        vec4 b=u_contact_bounds[i];
        vec4 c=u_contact_props[i];
        bool inside=all(greaterThanEqual(v_world_pos.xz,b.xy-vec2(0.015))) &&
                    all(lessThanEqual(v_world_pos.xz,b.zw+vec2(0.015)));
        bool vertical=v_world_pos.y>=c.y-0.015 && v_world_pos.y<=c.z+0.015;
        bool contacted=v_world_pos.y<=c.x+0.15 || abs(c.w)>0.20;
        if(inside && vertical && contacted)
            object_wet=max(object_wet,1.0-smoothstep(0.0,12.0,abs(c.w)));
    }
    result*=1.0-0.24*object_wet;
    result+=vec3(0.10,0.13,0.14)*object_wet*
        pow(1.0-clamp(dot(normal,normalize(u_atmosphere_camera_position-v_world_pos)),0.0,1.0),3.0);

    // WK-WAVES-009: the same persistent seed, three directions, amplitudes,
    // phases, speed and wavelength as WATER_VERTEX_SHADER. No independent
    // single-sine wetness clock or terrain-chunk quantization.
    bool terrain_wet_pass = u_terrain_ocean_contact_enabled &&
        u_terrain_layer_pass != 2;
    if (!u_water_basic && terrain_wet_pass && u_ocean_wetness.x>0.00001 &&
        u_ocean_contact_mapping.z>0.0) {
        vec2 atlas_uv=(v_world_pos.xz-u_ocean_contact_mapping.xy)*
                       u_ocean_contact_mapping.zw;
        if (all(greaterThanEqual(atlas_uv,vec2(0.0))) &&
            all(lessThanEqual(atlas_uv,vec2(1.0)))) {
            // R8 stores signed distance in +/-3 studs, 0.5 at water edge.
            // Blend in world units; the original binary tile never becomes
            // a rectangle on a large sloping beach.
            float signed_coast=(texture(u_ocean_contact_atlas,atlas_uv).r-
                0.5)*6.0;
            // Match the real shoreline + bounded wave runup; never use the
            // old full-square candidate wash envelope as a wetness footprint.
            float shore_reach=min(3.5,1.35+u_ocean_wetness.x*0.35);
            float footprint=smoothstep(-shore_reach-0.35,0.70,signed_coast);
            float wet=0.0;
            float dark_wet=0.0;
            // Deep seabed and high inland pixels skip expensive wave sampling.
            // Only fragments within physical crest/runup reach participate.
            if (footprint>0.001 &&
                abs(v_world_pos.y-u_terrain_water_level)<u_ocean_wetness.x+0.50) {
                // Wave contact is evaluated at the CURRENT fragment, then at
                // fixed past phases to leave a bounded receding damp trail.
                float current=wk_ocean_contact(v_world_pos,u_ocean_wetness.w);
                float recent=wk_ocean_contact(v_world_pos,
                    u_ocean_wetness.w-0.8);
                float older=wk_ocean_contact(v_world_pos,
                    u_ocean_wetness.w-2.2);
                wet=max(current,max(recent*0.78,older*0.38))*footprint;
                dark_wet=max(current,max(recent*0.90,older*0.63))*footprint;
            }
            // Contact applies to every touched terrain material and slope;
            // no separate uphill reach or slope filter is permitted.
            // Damp sand retains its original material color but gains a
            // glossy grazing-angle sky reflection rather than a tiny glint.
            result*=1.0-0.18*dark_wet;
            vec3 V=normalize(u_atmosphere_camera_position-v_world_pos);
            vec3 L=normalize(-u_global_light_direction);
            vec3 H=normalize(V+L);
            float fresnel=pow(1.0-clamp(dot(normal,V),0.0,1.0),5.0);
            float broad_gloss=pow(max(dot(normal,H),0.0),12.0)*
                max(dot(normal,L),0.0);
            vec3 reflected_sky=mix(vec3(0.13,0.18,0.20),u_fog_color,0.6);
            result+=wet*(reflected_sky*(0.14+0.65*fresnel)+
                u_global_light_color*u_global_light_intensity*broad_gloss*0.24);
        }
    }

    // WK-WATER-013: air and water can occupy different pixels in one frame.
    if (u_atmosphere_enabled) {
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

    vec3 local_water_air_color=result;

    // Height mode: sample the moving ocean surface at this receiver and
    // approximate the water fraction of the camera-to-pixel segment.
    // One coastline/height lookup replaces the full 16-sample ray marcher.
    // Off performs neither optical mode; both preserve geometry and physics.
    if (u_ocean_optics_mode == 1 && u_underwater_enabled &&
        u_ocean_contact_mapping.z>0.0) {
        vec2 ocean_uv=(v_world_pos.xz-u_ocean_contact_mapping.xy)*
            u_ocean_contact_mapping.zw;
        if (all(greaterThanEqual(ocean_uv,vec2(0.0))) &&
            all(lessThanEqual(ocean_uv,vec2(1.0))) &&
            texture(u_ocean_contact_atlas,ocean_uv).r>0.5) {
            float surface=u_underwater_surface_y+
                wk_ocean_wet_wave(v_world_pos.xz,u_ocean_wetness.w)*
                max(u_ocean_wetness.x,0.0);
            float start_y=u_underwater_camera_position.y;
            float end_y=v_world_pos.y;
            float fraction=0.0;
            if (abs(end_y-start_y)<0.00001)
                fraction=start_y<surface?1.0:0.0;
            else {
                float crossing=clamp((surface-start_y)/(end_y-start_y),0.0,1.0);
                fraction=start_y<surface?crossing:1.0-crossing;
            }
            float submerged=length(v_world_pos-u_underwater_camera_position)*
                clamp(fraction,0.0,1.0);
            if (submerged>0.0) {
                vec3 transmission=exp(-submerged*max(u_underwater_density,0.0)*
                    vec3(1.65,1.12,0.71));
                float approximate_depth=max(surface-min(start_y,end_y),0.0);
                float depth_absorption=clamp(approximate_depth*
                    max(u_underwater_depth_fog,0.0),0.0,
                    clamp(u_underwater_depth_fog_limit,0.0,1.0));
                transmission*=vec3(1.0-depth_absorption);
                transmission=mix(vec3(1.0),transmission,
                    clamp(u_underwater_fog_limit,0.0,1.0));
                vec3 in_scatter=mix(u_underwater_color,u_fog_color,0.02);
                result=result*transmission+in_scatter*(vec3(1.0)-transmission);
            }
        }
    }

    // WK-WAVES-005: each opaque pixel evaluates its own ray against the
    // ACTUAL animated ocean elevation and triangle coverage. Camera position
    // enters only as a ray endpoint: never as a whole-screen underwater toggle.
    // An entire camera ray above the highest physically possible wave crest
    // cannot intersect Ocean water. Skip the costly optical ray marcher for
    // those dry pixels without changing underwater, wave, or shoreline behavior.
    // Wave bands are normalized to at most +/-1.0 in wk_ocean_wet_wave().
    float highest_ocean_crest=u_underwater_surface_y+max(u_ocean_wetness.x,0.0);
    vec2 origin_ocean_uv=(u_underwater_camera_position.xz-
        u_ocean_contact_mapping.xy)*u_ocean_contact_mapping.zw;
    vec2 target_ocean_uv=(v_world_pos.xz-
        u_ocean_contact_mapping.xy)*u_ocean_contact_mapping.zw;
    bool ocean_map_intersect=all(greaterThanEqual(
        max(origin_ocean_uv,target_ocean_uv),vec2(0.0))) &&
        all(lessThanEqual(min(origin_ocean_uv,target_ocean_uv),vec2(1.0)));
    if (u_ocean_optics_mode == 2 && u_underwater_enabled &&
        u_ocean_contact_mapping.z > 0.0 && ocean_map_intersect &&
        min(u_underwater_camera_position.y,v_world_pos.y)<=highest_ocean_crest) {
        vec3 origin=u_underwater_camera_position;
        vec3 ray=v_world_pos-origin;
        float ray_distance=length(ray);
        // The three seeded wave bands are phase-linear along the camera ray.
        // Compute their directions and origin phases ONCE per fragment, not
        // their trigonometric setup again for each of 16 ray samples. This is
        // the identical spectral function evaluated at origin+ray*t.
        float wave_k=6.28318530718/max(u_ocean_wetness.z,1.0);
        float wave_seed=wk_wet_seed_hash(u_ocean_wetness_seed+0.123);
        float wave_angle2=u_ocean_wetness_direction+0.43+wave_seed*0.47;
        float wave_angle3=u_ocean_wetness_direction-0.91+
            wk_wet_seed_hash(u_ocean_wetness_seed+7.0)*0.39;
        vec2 wave_dir0=vec2(cos(u_ocean_wetness_direction),sin(u_ocean_wetness_direction));
        vec2 wave_dir1=vec2(cos(wave_angle2),sin(wave_angle2));
        vec2 wave_dir2=vec2(cos(wave_angle3),sin(wave_angle3));
        vec3 ocean_wave_initial_phase=vec3(
            -dot(origin.xz,wave_dir0)*wave_k+u_ocean_wetness.w*u_ocean_wetness.y,
            -dot(origin.xz,wave_dir1)*wave_k*1.71+
                u_ocean_wetness.w*u_ocean_wetness.y*1.18+wave_seed*6.28318530718,
            -dot(origin.xz,wave_dir2)*wave_k*0.58+
                u_ocean_wetness.w*u_ocean_wetness.y*0.74+
                wk_wet_seed_hash(u_ocean_wetness_seed+5.0)*6.28318530718
        );
        vec3 ocean_wave_phase_delta=vec3(
            -dot(ray.xz,wave_dir0)*wave_k,
            -dot(ray.xz,wave_dir1)*wave_k*1.71,
            -dot(ray.xz,wave_dir2)*wave_k*0.58
        );
        float submerged=0.0;
        float accumulated_depth=0.0;
        float suspended_silt=0.0;
        float rising_bubbles=0.0;
        float underwater_shafts=0.0;
        int steps=u_water_basic ? 4 : 16;
        for (int i=0;i<16;++i){
            if(i>=steps) break;
            float t=(float(i)+0.5)/float(steps);
            vec3 point=origin+ray*t;
            // Wave extrema are bounded by the sum of the three spectral
            // weights. This sample cannot contribute even partial shoreline
            // crest coverage when more than 0.025 above the highest crest.
            if (point.y>highest_ocean_crest+0.025) continue;
            vec2 uv=(point.xz-u_ocean_contact_mapping.xy)*
                u_ocean_contact_mapping.zw;
            if (any(lessThan(uv,vec2(0.0))) ||
                any(greaterThan(uv,vec2(1.0)))) continue;
            float ocean_coverage=texture(u_ocean_contact_atlas,uv).r;
            if (ocean_coverage>0.5) {
                // Reuse the same three-band phase for crest, shafts, and
                // bubbles instead of re-evaluating it at identical XZ/time.
                float wave_at_point=dot(
                    sin(ocean_wave_initial_phase+ocean_wave_phase_delta*t),
                    vec3(0.72,0.19,0.09));
                float surface_y=u_underwater_surface_y+
                    wave_at_point*max(u_ocean_wetness.x,0.0);
                float below=surface_y-point.y;
                // Smooth transition across the crest, not a camera threshold.
                float coverage=smoothstep(-0.025,0.025,below);
                if (coverage<=0.0) continue;
                float water_sample=coverage*ray_distance/float(steps);
                submerged+=water_sample;
                accumulated_depth+=max(below,0.0)*coverage/float(steps);
                // Low-frequency, soft wave-focused shafts only where
                // actually submerged. Avoid screen-space striped overlays.
                float beam_phase=point.x*0.17+point.z*0.11+
                    wave_at_point*1.2;
                float shaft_sample=pow(max(0.0,
                    0.5+0.5*cos(beam_phase)),7.0);
                underwater_shafts+=shaft_sample*water_sample*
                    exp(-0.24*max(below,0.0));
                // Only samples INSIDE the actual moving water contribute.
                // World-space 3D cells drift slowly; nothing is screen-locked.
                float underwater_coverage=coverage;
                if(underwater_coverage>0.0 && (i%3)==0) {
                    // Pixar-inspired very fine, dim, sparse marine snow.
                    // Coherent world-space surge follows the ocean phase.
                    float surge=sin(u_ocean_wetness.w*
                        u_ocean_wetness.y*0.63+point.x*0.045+
                        point.z*0.08);
                    vec3 dust_p=point*4.5+
                        vec3(u_ocean_wetness.w*0.012+surge*0.07,
                             sin(u_ocean_wetness.w*0.08)*0.045,
                             u_ocean_wetness.w*0.009+surge*0.05);
                    vec3 dust_cell=floor(dust_p);
                    float dust_seed=fract(sin(dot(dust_cell,
                        vec3(12.9898,78.233,37.719)))*43758.5453);
                    float dust_dist=length(fract(dust_p)-vec3(0.5));
                    float dust_point=(1.0-smoothstep(0.014,0.072,dust_dist))*
                        smoothstep(0.87,0.976,dust_seed);
                    suspended_silt+=dust_point*water_sample;
                    // Entrained bubbles occur near breaking, energetic waves
                    // and the shoreline, not in calm open-water columns.
                    // Shore mask is the SAME atlas that clips this ray.
                    float coast=1.0-smoothstep(0.52,0.82,ocean_coverage);
                    float wave_strength=max(u_ocean_wetness.x,0.0)*
                        6.28318530718/max(u_ocean_wetness.z,1.0);
                    float crest=0.5+0.5*wave_at_point;
                    float breaking_activity=
                        smoothstep(0.16,0.64,
                            wave_strength*(0.28+coast*2.8))*
                        smoothstep(0.63,0.95,crest)*
                        (0.15+coast*0.85)*
                        (1.0-smoothstep(0.3,2.1,max(below,0.0)));
                    // Rise with varying size/speed. No extra screen-space
                    // or flat-height gate: inside-water samples only.
                    vec3 bubble_p=point*0.91+
                        vec3(surge*0.08,
                            -u_ocean_wetness.w*
                              (0.13+0.065*coast),surge*0.06);
                    vec3 bubble_cell=floor(bubble_p);
                    float bubble_seed=fract(sin(dot(bubble_cell,
                        vec3(39.346,11.135,83.155)))*24375.325);
                    vec3 bubble_center=vec3(
                        fract(bubble_seed*14.17),
                        fract(bubble_seed*5.67),
                        fract(bubble_seed*3.81));
                    float bubble_dist=length(fract(bubble_p)-bubble_center);
                    float bubble_radius=mix(0.028,0.09,
                        fract(bubble_seed*29.113));
                    rising_bubbles+=breaking_activity*
                        (1.0-smoothstep(bubble_radius*0.38,
                            bubble_radius,bubble_dist))*
                        smoothstep(0.931,0.976,bubble_seed)*
                        water_sample;
                }
            }
        }
        if (submerged>0.0) {
            // Underwater *path length* matters only after per-pixel
            // dynamic ocean volume and coastline coverage are established.
            float water_depth=submerged;
            vec3 transmission=exp(-water_depth*max(u_underwater_density,0.0)*
                vec3(1.65,1.12,0.71));
            float depth_absorption=clamp(accumulated_depth*
                max(u_underwater_depth_fog,0.0),0.0,
                clamp(u_underwater_depth_fog_limit,0.0,1.0));
            transmission*=vec3(1.0-depth_absorption);
            float authored_limit=clamp(u_underwater_fog_limit,0.0,1.0);
            transmission=mix(vec3(1.0),transmission,authored_limit);
            // 98% authored post-processing water color, only 2% sky fog.
            vec3 in_scatter=mix(u_underwater_color,u_fog_color,0.02);
            result=result*transmission+
                in_scatter*(vec3(1.0)-transmission);
            // Wave curvature focuses moving patches onto submerged terrain.
            // Never run for dry pixels, and attenuate below the sunlit zone.
            // WK-WAVES-015: caustics illuminate ONLY receiver surfaces
            // physically beneath the same wave and inside the ocean mask.
            // Seeing an above-water target THROUGH a submerged camera ray
            // must never paint that dry target with caustic lighting.
            vec2 cp=v_world_pos.xz;
            vec2 target_uv=(cp-u_ocean_contact_mapping.xy)*
                u_ocean_contact_mapping.zw;
            // First establish physical reception. Dry pixels, off-atlas
            // surfaces and receivers above the maximum possible crest have
            // zero caustic contribution, so they must not evaluate the five
            // expensive wave-curvature functions at all.
            if (v_world_pos.y<=highest_ocean_crest+0.038 &&
                all(greaterThanEqual(target_uv,vec2(0.0))) &&
                all(lessThanEqual(target_uv,vec2(1.0))) &&
                texture(u_ocean_contact_atlas,target_uv).r>0.5) {
                float receiver_surface=u_underwater_surface_y+
                    wk_ocean_wet_wave(cp,u_ocean_wetness.w)*
                    max(u_ocean_wetness.x,0.0);
                float receiver_wet=smoothstep(-0.012,0.038,
                    receiver_surface-v_world_pos.y);
                if (receiver_wet>0.0) {
                    float caustic_depth=max(
                        receiver_surface-v_world_pos.y,0.0);
                    float c0=wk_ocean_wet_wave(cp,u_ocean_wetness.w);
                    float cx=wk_ocean_wet_wave(cp+vec2(0.45,0.0),
                        u_ocean_wetness.w)+
                        wk_ocean_wet_wave(cp-vec2(0.45,0.0),
                        u_ocean_wetness.w)-2.0*c0;
                    float cz=wk_ocean_wet_wave(cp+vec2(0.0,0.45),
                        u_ocean_wetness.w)+
                        wk_ocean_wet_wave(cp-vec2(0.0,0.45),
                        u_ocean_wetness.w)-2.0*c0;
                    float caustic_focus=smoothstep(0.022,0.11,
                        abs(cx)+abs(cz));
                    float caustic_fade=exp(-0.22*caustic_depth)*
                        smoothstep(0.02,0.35,submerged)*receiver_wet;
                    result+=u_global_light_color*
                        caustic_focus*caustic_fade*0.095*authored_limit;
                }
            }
            // Restrained volumetric sunlight, integrated INSIDE the liquid.
            result+=u_global_light_color*
                min(underwater_shafts*0.014,0.065)*
                authored_limit;
            result+=vec3(0.012,0.018,0.018)*
                min(suspended_silt*0.30,0.30)*authored_limit;
            result+=vec3(0.048,0.075,0.088)*
                min(rising_bubbles*0.36,0.50)*authored_limit;
            // Optional deep void at a downward view: no camera height
            // switch, no flat-water test and NO dry-pixel modification.
            // Uses ONLY accumulated ACTUAL submerged ray samples.
            float looking_down=smoothstep(0.28,0.86,
                max(-ray.y/max(ray_distance,0.001),0.0));
            float deep_murk=smoothstep(3.0,28.0,submerged)*
                smoothstep(1.0,16.0,accumulated_depth);
            float deep_void=looking_down*deep_murk;
            result*=1.0-0.985*deep_void;
        }
    }

    // WK-WATER-013: shape-clipped, terrain-bed-limited Lake/River optics,
    // evaluated per opaque fragment; never an entire-screen tint.
    // A zero body count means the original function returns the same
    // zero-blend result; avoid the per-pixel volume loop entirely.
    if (u_local_water_count>0) {
        vec4 local=wk_apply_local_water(
            local_water_air_color,u_underwater_camera_position,v_world_pos,false);
        // Local bodies override Ocean optics where their real 3D volume is
        // present. Disabling local effects also suppresses inherited Ocean fog.
        result=mix(result,local.rgb,local.a);
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
    fn physics_body_shadow_model_rotates_about_body_center() {
        let mut body = crate::engine::physics::PhysicsBody::new(
            crate::engine::physics::PhysicsBodyId(1),
            1,
            Vec3::new(3.0, 2.0, 5.0),
            Vec3::new(2.0, 1.0, 2.0),
        );
        body.rotation = glam::Quat::from_rotation_y(std::f32::consts::FRAC_PI_2);
        let transform = physics_body_model_matrix(&body);
        let center = body.size * 0.5;
        assert!((transform.transform_point3(center) - body.center()).length() < 0.0001);
        assert!((transform.transform_point3(Vec3::ZERO) - body.position).length() > 0.1);
    }

    #[test]
    fn point_shadow_atlas_shader_uses_renderer_tile_size() {
        let tiles_per_axis = SHADOW_RES / point_shadows_gpu::SHADOW_TILE_SIZE;
        let source = GRID_FRAGMENT_SHADER.replace(
            "__POINT_SHADOW_TILES_PER_AXIS__",
            &format!("{tiles_per_axis}.0"),
        );
        assert!(!source.contains("__POINT_SHADOW_TILES_PER_AXIS__"));
        assert!(source.contains(&format!("float tiles_per_axis = {tiles_per_axis}.0;")));
    }

    #[test]
    fn editor_shadow_range_reaches_three_times_farther_than_game_projection() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let camera = Vec3::new(8.0, 5.0, 12.0);
        let light = Vec3::new(1.0, -1.0, 0.0).normalize();
        let game = renderer.calculate_light_space_matrix(light, camera);
        assert_eq!(
            game,
            renderer.calculate_light_space_matrix_with_range(light, camera, 1.0)
        );
        let editor = renderer.calculate_light_space_matrix_with_range(light, camera, 3.0);
        let sideways = light.cross(Vec3::Y).normalize();
        let far_point = (camera + sideways * 55.0).extend(1.0);
        let game_clip = game * far_point;
        let editor_clip = editor * far_point;
        assert!(game_clip.x.abs() > game_clip.w.abs());
        assert!(editor_clip.x.abs() < editor_clip.w.abs());
    }

    #[test]
    fn test_directional_shadow_matrix_invariant_to_camera_rotation() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let light_dir = Vec3::new(0.5, -1.0, 0.5).normalize();
        let camera_pos = Vec3::new(10.0, 5.0, 20.0);

        let matrix1 = renderer.calculate_light_space_matrix(light_dir, camera_pos);
        let matrix2 = renderer.calculate_light_space_matrix(light_dir, camera_pos);

        assert_eq!(matrix1, matrix2);
    }

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

        world.set_cell(block_coord, CellType::Block);
        if let Some(cell) = world.get_mut(block_coord) {
            cell.light_enabled = true;
        }

        let lights = renderer.collect_point_lights(&world);
        assert!(
            lights.is_empty(),
            "Block with light_enabled=true must NOT produce a point light"
        );

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

        world.set_cell(light_disabled_coord, CellType::Light);
        world.set_light_enabled_runtime(light_disabled_coord, false);
        let lights = renderer.collect_point_lights(&world);
        assert_eq!(
            lights.len(),
            1,
            "Disabled Light cell must NOT produce an additional point light"
        );

        let light_id = world.get(light_enabled_coord).unwrap().id;
        world.delete_cell_runtime(light_id);
        let lights = renderer.collect_point_lights(&world);
        assert!(
            lights.is_empty(),
            "Deleted Light cell must NOT produce a point light"
        );

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

        world.light_ids.clear();
        assert!(renderer.collect_point_lights(&world).is_empty());

        world.rebuild_id_mapping();
        assert!(world.light_ids.contains(&light_id));
        assert_eq!(renderer.collect_point_lights(&world).len(), 1);
    }

    #[test]
    fn every_light_representation_is_collected_without_sixteen_light_ceiling() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);
        let mut world = World::new();
        for index in 0..40 {
            world.add_source(CellType::Light, Vec3::new(index as f32 * 3.0, 2.0, 0.0));
        }
        let voxel = world.set_cell(crate::world::WorldCoord::new(0, 7, 5), CellType::Light);
        let live = world.create_runtime_cell(CellType::Light);
        let attached_at = Vec3::new(27.5, 12.25, 16.5);
        world.set_runtime_cell_position(live, attached_at).unwrap();
        let lights = renderer.collect_point_lights(&world);
        assert_eq!(
            lights.len(),
            42,
            "all authored, voxel and runtime Lights must illuminate"
        );
        assert!(lights.iter().any(|light| light.position == attached_at));
        assert!(
            lights
                .iter()
                .any(|light| light.position == Vec3::new(0.5, 7.5, 5.5))
        );
        let attached_voxel_at = Vec3::new(13.5, 2.75, 20.25);
        world
            .set_attachment_source_position(voxel, attached_voxel_at)
            .unwrap();
        assert!(
            renderer
                .collect_point_lights(&world)
                .iter()
                .any(|light| light.position == attached_voxel_at)
        );
        let moved = Vec3::new(91.25, 15.0, 19.75);
        world.set_runtime_cell_position(live, moved).unwrap();
        let lights = renderer.collect_point_lights(&world);
        assert!(
            lights.iter().any(|light| light.position == moved),
            "attached Play Light updates must reach the renderer"
        );
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

    #[test]
    fn moving_objects_create_bounded_fading_water_wakes() {
        let mut wakes = super::WaterWakeState::default();
        let origin = glam::Vec3::new(0.0, 0.0, 0.0);
        wakes.record(1, origin, 0.0, 0.0);
        wakes.record(1, glam::Vec3::new(1.0, 0.0, 0.0), 0.25, 0.0);
        assert_eq!(wakes.trails.len(), 1);
        let wake = wakes.trails[0];
        assert_eq!(wake.start, glam::Vec2::ZERO);
        assert_eq!(wake.end, glam::Vec2::new(1.0, 0.0));
        wakes.record(1, glam::Vec3::new(1.0, 0.0, 0.0), 5.0, 0.0);
        assert!(wakes.trails.is_empty());
        wakes.record(2, glam::Vec3::new(0.0, 8.0, 0.0), 5.1, 0.0);
        assert!(!wakes.previous.contains_key(&2));
    }
}
