// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::Renderer;
use crate::project::material::{MaterialGraph, MaterialOutput, PinType, graph_glsl};
use glam::{Mat4, Vec3};
use std::collections::BTreeMap;
use std::sync::Arc;

#[path = "material_preview_graph.rs"]
mod graph;
use graph::compile_graph_variant;

struct CachedMaterialShader {
    key: Vec<u8>,
    shader: Result<Arc<graph_glsl::SurfaceShader>, String>,
}
thread_local! {
    static MATERIAL_SHADER_SOURCES: std::cell::RefCell<std::collections::VecDeque<CachedMaterialShader>> =
        const { std::cell::RefCell::new(std::collections::VecDeque::new()) };
}
fn cached_surface_shader(
    graph: &MaterialGraph,
    output: &MaterialOutput,
    textures: &BTreeMap<String, String>,
) -> Result<Arc<graph_glsl::SurfaceShader>, String> {
    let nodes: Vec<_> = graph
        .nodes
        .iter()
        .map(|node| (node.id, &node.operation))
        .collect();
    let key = serde_json::to_vec(&(graph.output_node, nodes, &graph.links, output, textures))
        .map_err(|e| format!("Material preview cache key: {e}"))?;
    MATERIAL_SHADER_SOURCES.with(|cell| {
        let mut cache = cell.borrow_mut();
        if let Some(index) = cache.iter().position(|entry| entry.key == key) {
            let entry = cache.remove(index).expect("cached source exists");
            let result = entry.shader.clone();
            cache.push_back(entry);
            return result;
        }
        let shader = graph_glsl::compile_surface(graph, output, textures).map(Arc::new);
        if cache.len() >= 12 {
            cache.pop_front();
        }
        cache.push_back(CachedMaterialShader {
            key,
            shader: shader.clone(),
        });
        shader
    })
}

pub(super) struct GraphPreviewProgram {
    pub program: u32,
    locations: [i32; 21],
    uniform_locations: Vec<i32>,
    default_base: i32,
}

#[derive(Clone)]
struct PreviewDraw {
    program: u32,
    vao: u32,
    first: i32,
    count: i32,
    texture: u32,
    use_texture: bool,
    color: [f32; 4],
    roughness: f32,
    metallic: f32,
    material_preview_location: i32,
    view_projection: i32,
    model: i32,
    base_color: i32,
    alpha: i32,
    use_texture_uniform: i32,
    ambient: i32,
    light_enabled: i32,
    light_direction: i32,
    light_color: i32,
    light_intensity: i32,
    point_light_count: i32,
    shadow_enabled: i32,
    indirect_enabled: i32,
    debug_view: i32,
    terrain_blend_enabled: i32,
    terrain_layer_pass: i32,
    terrain_vertex_layers: i32,
    atmosphere_enabled: i32,
    underwater_enabled: i32,
    graph_uniforms: Vec<(i32, PinType, [f32; 4])>,
    graph_default_base: i32,
    graph_texture_sampler: i32,
    graph_base_color: [f32; 4],
    landscape: bool,
}

impl Renderer {
    pub(crate) fn surface_material_preview(
        &self,
        rect: egui::Rect,
        color: [f32; 4],
        roughness: f32,
        metallic: f32,
        texture_path: Option<&str>,
        graph: Option<(&MaterialGraph, &MaterialOutput, &BTreeMap<String, String>)>,
        landscape: bool,
    ) -> Result<egui::PaintCallback, String> {
        let texture = texture_path
            .map(|path| self.get_texture(path))
            .unwrap_or(self.fallback_tex);
        let (mut first, mut count) = self.block_mask_ranges[63];
        let mut vao = self.block_vao;
        if landscape {
            if let Some((patch_vao, patch_count)) = self.landscape_preview_geometry() {
                vao = patch_vao;
                first = 0;
                count = patch_count;
            }
        }
        let mut draw = PreviewDraw {
            program: self.grid_program,
            vao,
            first,
            count,
            texture,
            use_texture: texture_path.is_some() && texture != self.fallback_tex,
            color,
            roughness,
            metallic,
            material_preview_location: self.grid_material_preview_location,
            view_projection: self.grid_view_projection_location,
            model: self.grid_model_location,
            base_color: self.grid_base_color_location,
            alpha: self.grid_alpha_location,
            use_texture_uniform: self.grid_use_texture_location,
            ambient: self.grid_ambient_intensity_location,
            light_enabled: self.grid_global_light_enabled_location,
            light_direction: self.grid_global_light_direction_location,
            light_color: self.grid_global_light_color_location,
            light_intensity: self.grid_global_light_intensity_location,
            point_light_count: self.grid_point_light_count_location,
            shadow_enabled: self.grid_shadows_enabled_location,
            indirect_enabled: self.grid_indirect_enabled_location,
            debug_view: self.grid_debug_view_location,
            terrain_blend_enabled: self.grid_terrain_blend_enabled_location,
            terrain_layer_pass: self.grid_terrain_layer_pass_location,
            terrain_vertex_layers: self.grid_terrain_vertex_layers_location,
            atmosphere_enabled: self.grid_atmosphere_enabled_location,
            underwater_enabled: self.grid_underwater_enabled_location,
            graph_uniforms: Vec::new(),
            graph_default_base: -1,
            graph_texture_sampler: -1,
            graph_base_color: color,
            landscape,
        };
        if let Some((graph, output, textures)) = graph {
            let shader = cached_surface_shader(graph, output, textures)?;
            let mut programs = self.graph_preview_programs.borrow_mut();
            if !programs.contains_key(&shader.source) {
                if programs.len() >= 12 {
                    if let Some(key) = programs.keys().next().cloned() {
                        if let Some(Ok(old)) = programs.remove(&key) {
                            unsafe {
                                gl::DeleteProgram(old.program);
                            }
                        }
                    }
                }
                programs.insert(shader.source.clone(), compile_graph_variant(&shader));
            }
            let variant = programs
                .get(&shader.source)
                .expect("graph variant cached")
                .as_ref()
                .map_err(Clone::clone)?;
            draw.use_graph_program(variant, &shader.uniforms);
        }
        Ok(egui::PaintCallback {
            rect,
            callback: Arc::new(egui_glow::CallbackFn::new(move |info, _painter| {
                draw.paint(info);
            })),
        })
    }
}

impl PreviewDraw {
    fn paint(&self, info: egui::PaintCallbackInfo) {
        let bounds = info.viewport_in_pixels();
        let clip = info.clip_rect_in_pixels();
        let x0 = bounds.left_px.max(clip.left_px);
        let y0 = bounds.from_bottom_px.max(clip.from_bottom_px);
        let x1 = (bounds.left_px + bounds.width_px).min(clip.left_px + clip.width_px);
        let y1 =
            (bounds.from_bottom_px + bounds.height_px).min(clip.from_bottom_px + clip.height_px);
        if x0 >= x1 || y0 >= y1 || self.program == 0 || self.vao == 0 || self.count == 0 {
            return;
        }
        let aspect = bounds.width_px as f32 / bounds.height_px.max(1) as f32;
        let projection = Mat4::perspective_rh_gl(45_f32.to_radians(), aspect, 0.1, 20.0);
        let camera = Mat4::look_at_rh(Vec3::new(2.3, 1.85, 2.6), Vec3::ZERO, Vec3::Y);
        let view_projection = projection * camera;
        let model = if self.landscape {
            Mat4::IDENTITY
        } else {
            Mat4::from_translation(Vec3::splat(-0.5))
        };

        unsafe {
            gl::Enable(gl::SCISSOR_TEST);
            gl::Scissor(x0, y0, x1 - x0, y1 - y0);
            gl::DepthMask(gl::TRUE);
            gl::Clear(gl::DEPTH_BUFFER_BIT);
            gl::Enable(gl::DEPTH_TEST);
            gl::DepthFunc(gl::LESS);
            gl::Enable(gl::CULL_FACE);
            gl::CullFace(gl::BACK);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::UseProgram(self.program);
            if self.graph_default_base >= 0 {
                gl::Uniform4f(
                    self.graph_default_base,
                    self.graph_base_color[0],
                    self.graph_base_color[1],
                    self.graph_base_color[2],
                    self.graph_base_color[3],
                );
                gl::Uniform1i(self.graph_texture_sampler, 1);
                for (location, kind, value) in &self.graph_uniforms {
                    match kind {
                        PinType::Scalar => gl::Uniform1f(*location, value[0]),
                        PinType::Vec2 => gl::Uniform2f(*location, value[0], value[1]),
                        PinType::Vec3 => gl::Uniform3f(*location, value[0], value[1], value[2]),
                        PinType::Color => {
                            gl::Uniform4f(*location, value[0], value[1], value[2], value[3])
                        }
                    }
                }
            }
            gl::UniformMatrix4fv(
                self.view_projection,
                1,
                gl::FALSE,
                view_projection.to_cols_array().as_ptr(),
            );
            gl::UniformMatrix4fv(self.model, 1, gl::FALSE, model.to_cols_array().as_ptr());
            gl::Uniform3f(self.base_color, self.color[0], self.color[1], self.color[2]);
            gl::Uniform1f(self.alpha, self.color[3]);
            gl::Uniform3f(
                self.material_preview_location,
                self.roughness,
                self.metallic,
                1.0,
            );
            gl::Uniform1i(self.use_texture_uniform, i32::from(self.use_texture));
            gl::Uniform1f(self.ambient, 0.45);
            gl::Uniform1i(self.light_enabled, 1);
            gl::Uniform3f(self.light_direction, 0.55, -1.0, -0.35);
            gl::Uniform3f(self.light_color, 1.0, 1.0, 1.0);
            gl::Uniform1f(self.light_intensity, 0.75);
            gl::Uniform1i(self.point_light_count, 0);
            gl::Uniform1i(self.shadow_enabled, 0);
            gl::Uniform1i(self.indirect_enabled, 0);
            gl::Uniform1i(self.debug_view, 0);
            gl::Uniform1i(self.terrain_blend_enabled, 0);
            gl::Uniform1i(self.terrain_vertex_layers, 0);
            gl::Uniform1i(self.atmosphere_enabled, 0);
            gl::Uniform1i(self.underwater_enabled, 0);
            gl::Uniform1i(self.terrain_layer_pass, 0);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, self.texture);
            gl::BindVertexArray(self.vao);
            gl::DrawArrays(gl::TRIANGLES, self.first, self.count);
            gl::Uniform3f(self.material_preview_location, 0.0, 0.0, 0.0);
            gl::BindVertexArray(0);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::UseProgram(0);
            gl::DepthMask(gl::TRUE);
        }
    }
}

#[cfg(all(test, windows))]
mod tests {
    use glutin::{
        config::ConfigTemplateBuilder,
        context::ContextAttributesBuilder,
        display::GetGlDisplay,
        prelude::*,
        surface::{SurfaceAttributesBuilder, WindowSurface},
    };
    use glutin_winit::DisplayBuilder;
    use std::{ffi::CString, num::NonZeroU32};
    use winit::{
        event_loop::EventLoop, platform::windows::EventLoopBuilderExtWindows,
        raw_window_handle::HasWindowHandle, window::WindowAttributes,
    };

    #[test]
    #[ignore = "Requires a Windows desktop OpenGL driver; creates only a hidden shader-test window"]
    fn existing_scene_shader_supports_material_preview_parameters() {
        let event_loop = EventLoop::builder().with_any_thread(true).build().unwrap();
        let (window, config) = DisplayBuilder::new()
            .with_window_attributes(Some(
                WindowAttributes::default()
                    .with_visible(false)
                    .with_inner_size(winit::dpi::PhysicalSize::new(64, 64)),
            ))
            .build(&event_loop, ConfigTemplateBuilder::new(), |configs| {
                configs.max_by_key(|c| c.num_samples()).unwrap()
            })
            .unwrap();
        let window = window.unwrap();
        let handle = window.window_handle().unwrap().as_raw();
        let context = unsafe {
            config.display().create_context(
                &config,
                &ContextAttributesBuilder::new().build(Some(handle)),
            )
        }
        .unwrap();
        let surface = unsafe {
            config.display().create_window_surface(
                &config,
                &SurfaceAttributesBuilder::<WindowSurface>::new().build(
                    handle,
                    NonZeroU32::new(64).unwrap(),
                    NonZeroU32::new(64).unwrap(),
                ),
            )
        }
        .unwrap();
        let _context = context.make_current(&surface).unwrap();
        gl::load_with(|name| {
            config
                .display()
                .get_proc_address(&CString::new(name).unwrap())
                .cast()
        });
        let program = super::super::shader::create_program(
            super::super::GRID_VERTEX_SHADER,
            super::super::GRID_FRAGMENT_SHADER,
        );
        let preview = super::super::resources::get_uniform_location(program, "u_material_preview");
        assert!(
            preview >= 0,
            "existing GL shader must expose the preview-only PBR parameters"
        );
        assert!(super::super::resources::get_uniform_location(program, "u_base_color") >= 0);
        unsafe {
            gl::DeleteProgram(program);
        }
    }
}

#[cfg(test)]
mod cached_source_tests {
    use super::*;
    use crate::project::material::{GraphLink, GraphNode, GraphOperation};
    fn sample(v: f32) -> MaterialGraph {
        MaterialGraph {
            output_node: 2,
            nodes: vec![
                GraphNode {
                    id: 1,
                    operation: GraphOperation::Color {
                        value: [v, 0.3, 0.4, 1.0],
                    },
                    position: [0.0, 0.0],
                },
                GraphNode {
                    id: 2,
                    operation: GraphOperation::SurfaceOutput,
                    position: [10.0, 10.0],
                },
            ],
            links: vec![GraphLink {
                source_node: 1,
                source_pin: "value".into(),
                target_node: 2,
                target_pin: "base_color".into(),
            }],
        }
    }
    fn output() -> MaterialOutput {
        MaterialOutput::SurfacePbr {
            base_color: [0.3; 4],
            roughness: 0.5,
            metallic: 0.0,
        }
    }
    #[test]
    fn unchanged_sources_reuse_cache_and_parameter_edits_invalidate() {
        let material = output();
        let maps = BTreeMap::new();
        let graph = sample(0.4);
        let first = cached_surface_shader(&graph, &material, &maps).unwrap();
        let second = cached_surface_shader(&graph, &material, &maps).unwrap();
        assert!(Arc::ptr_eq(&first, &second));
        let mut repositioned = graph.clone();
        repositioned.nodes[0].position = [432.0, -120.0];
        let moved = cached_surface_shader(&repositioned, &material, &maps).unwrap();
        assert!(
            Arc::ptr_eq(&first, &moved),
            "Node layout must not recompile GLSL"
        );
        let changed = cached_surface_shader(&sample(0.9), &material, &maps).unwrap();
        assert!(!Arc::ptr_eq(&first, &changed));
        assert_ne!(first.uniforms[0].value, changed.uniforms[0].value);
        assert_eq!(first.source, changed.source);
    }
    #[test]
    fn invalid_graph_remains_invalid_even_with_successful_source_cached() {
        let material = output();
        let maps = BTreeMap::new();
        let valid = sample(0.3);
        assert!(cached_surface_shader(&valid, &material, &maps).is_ok());
        let mut invalid = valid.clone();
        invalid.links[0].source_pin = "nonexistent".into();
        assert!(cached_surface_shader(&invalid, &material, &maps).is_err());
        assert!(cached_surface_shader(&invalid, &material, &maps).is_err());
    }
}
