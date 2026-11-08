// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::landscape::material_weights::{DirtyUpload, MaterialWeightAtlas, PAGE_DIM, PageId};
use crate::world::terrain::{TerrainState, WaterBody, WaterBodyKind, WaterControlPoint};
use std::collections::{BTreeMap, BTreeSet};
const DIM: usize = PAGE_DIM + 2;
#[derive(Default)]
pub(super) struct LayerGpuPages {
    textures: BTreeMap<String, BTreeMap<PageId, u32>>,
    bodies: BTreeMap<u64, BodyBuffer>,
}
struct BodyBuffer {
    samples: Vec<[f32; 4]>,
    source: Vec<WaterControlPoint>,
    kind: Option<WaterBodyKind>,
    bounds: [f32; 4],
    half_width: f32,
    texture: u32,
    buffer: u32,
}

pub(super) fn layer_pixel(
    atlas: &MaterialWeightAtlas,
    id: PageId,
    layer: &str,
    x: i32,
    y: i32,
) -> [u8; 4] {
    let gx = id.x as i64 * PAGE_DIM as i64 + x as i64;
    let gy = id.y as i64 * PAGE_DIM as i64 + y as i64;
    let other = PageId {
        chart: id.chart,
        x: gx.div_euclid(PAGE_DIM as i64) as i32,
        y: gy.div_euclid(PAGE_DIM as i64) as i32,
    };
    let i = gy.rem_euclid(PAGE_DIM as i64) as usize * PAGE_DIM
        + gx.rem_euclid(PAGE_DIM as i64) as usize;
    let Some(page) = atlas.pages.get(&other) else {
        return [0; 4];
    };
    let total: u32 = page.layers.values().map(|w| w[i] as u32).sum();
    let prior: u32 = page
        .layers
        .iter()
        .filter(|(key, _)| key.as_str() <= layer)
        .map(|(_, w)| w[i] as u32)
        .sum();
    let weight = page.layers.get(layer).map_or(0, |w| w[i] as u32);
    let denominator = 255 - total.min(255) + prior;
    let alpha = if denominator == 0 {
        0
    } else {
        ((weight * 255 + denominator / 2) / denominator).min(255) as u8
    };
    [0, 0, 0, alpha]
}
fn pixels(
    atlas: &MaterialWeightAtlas,
    id: PageId,
    layer: &str,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
) -> Vec<u8> {
    let mut result = Vec::with_capacity(w as usize * h as usize * 4);
    for iy in y..y + h {
        for ix in x..x + w {
            result.extend_from_slice(&layer_pixel(atlas, id, layer, ix - 1, iy - 1));
        }
    }
    result
}
impl LayerGpuPages {
    unsafe fn upload(
        &mut self,
        atlas: &MaterialWeightAtlas,
        id: PageId,
        layer: &str,
        rect: [i32; 4],
    ) {
        let map = self.textures.entry(layer.into()).or_default();
        let fresh = !map.contains_key(&id);
        let mut texture = map.get(&id).copied().unwrap_or(0);
        let rect = if fresh {
            [0, 0, DIM as i32, DIM as i32]
        } else {
            rect
        };
        let bytes = pixels(atlas, id, layer, rect[0], rect[1], rect[2], rect[3]);
        unsafe {
            if fresh {
                gl::GenTextures(1, &mut texture);
            }
            gl::BindTexture(gl::TEXTURE_2D, texture);
            if fresh {
                gl::TexImage2D(
                    gl::TEXTURE_2D,
                    0,
                    gl::RGBA8 as i32,
                    DIM as i32,
                    DIM as i32,
                    0,
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    bytes.as_ptr().cast(),
                );
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
                gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            } else {
                gl::TexSubImage2D(
                    gl::TEXTURE_2D,
                    0,
                    rect[0],
                    rect[1],
                    rect[2],
                    rect[3],
                    gl::RGBA,
                    gl::UNSIGNED_BYTE,
                    bytes.as_ptr().cast(),
                );
            }
        }
        map.insert(id, texture);
    }
    unsafe fn clear(&mut self) {
        unsafe {
            for map in self.textures.values() {
                for texture in map.values() {
                    gl::DeleteTextures(1, texture);
                }
            }
        }
        self.textures.clear();
    }
    pub unsafe fn sync(&mut self, terrain: &TerrainState, uploads: &[DirtyUpload], rebuilt: bool) {
        if rebuilt {
            unsafe {
                self.clear();
            }
        }
        let atlas = &terrain.paint_weights;
        let ids: BTreeSet<_> = if rebuilt {
            atlas.pages.keys().copied().collect()
        } else {
            uploads.iter().map(|u| u.id).collect()
        };
        unsafe {
            gl::ActiveTexture(gl::TEXTURE10);
        }
        for id in ids {
            let Some(page) = atlas.pages.get(&id) else {
                continue;
            };
            let layer_ids: Vec<_> = page.layers.keys().cloned().collect();
            for layer in layer_ids {
                for dy in -1..=1 {
                    for dx in -1..=1 {
                        let target = PageId {
                            chart: id.chart,
                            x: id.x + dx,
                            y: id.y + dy,
                        };
                        let rect = if dx == 0 && dy == 0 {
                            uploads
                                .iter()
                                .filter(|u| u.id == id)
                                .map(|u| u.rect)
                                .reduce(|a, b| crate::landscape::material_weights::DirtyRect {
                                    x0: a.x0.min(b.x0),
                                    y0: a.y0.min(b.y0),
                                    x1: a.x1.max(b.x1),
                                    y1: a.y1.max(b.y1),
                                })
                                .map_or([0, 0, DIM as i32, DIM as i32], |r| {
                                    [
                                        r.x0 as i32 + 1,
                                        r.y0 as i32 + 1,
                                        (r.x1 - r.x0 + 1) as i32,
                                        (r.y1 - r.y0 + 1) as i32,
                                    ]
                                })
                        } else {
                            let x = if dx == -1 { DIM as i32 - 1 } else { 0 };
                            let y = if dy == -1 { DIM as i32 - 1 } else { 0 };
                            if dx != 0 && dy != 0 {
                                [x, y, 1, 1]
                            } else if dx != 0 {
                                [x, 0, 1, DIM as i32]
                            } else {
                                [0, y, DIM as i32, 1]
                            }
                        };
                        unsafe {
                            self.upload(atlas, target, &layer, rect);
                        }
                    }
                }
            }
        }
        self.bodies.retain(|id, buffer| {
            let keep = terrain
                .config
                .water_bodies
                .iter()
                .any(|b| b.id == *id && b.visible);
            if !keep {
                unsafe {
                    gl::DeleteTextures(1, &buffer.texture);
                    gl::DeleteBuffers(1, &buffer.buffer);
                }
            }
            keep
        });
        for body in terrain.config.water_bodies.iter().filter(|b| b.visible) {
            if self
                .bodies
                .get(&body.id)
                .is_some_and(|b| b.source == body.points && b.kind == Some(body.kind))
            {
                continue;
            }
            let samples: Vec<_> = body
                .spline_samples()
                .iter()
                .map(|(p, w, h)| [p[0], p[1], *w, *h])
                .collect();
            let buffer = self.bodies.entry(body.id).or_insert(BodyBuffer {
                samples: Vec::new(),
                source: Vec::new(),
                kind: None,
                bounds: [0.0; 4],
                half_width: 0.0,
                texture: 0,
                buffer: 0,
            });
            unsafe {
                gl::ActiveTexture(gl::TEXTURE15);
                if buffer.texture == 0 {
                    gl::GenTextures(1, &mut buffer.texture);
                    gl::GenBuffers(1, &mut buffer.buffer);
                }
                gl::BindBuffer(gl::TEXTURE_BUFFER, buffer.buffer);
                gl::BufferData(
                    gl::TEXTURE_BUFFER,
                    (samples.len() * 16) as isize,
                    samples.as_ptr().cast(),
                    gl::STATIC_DRAW,
                );
                gl::BindTexture(gl::TEXTURE_BUFFER, buffer.texture);
                gl::TexBuffer(gl::TEXTURE_BUFFER, gl::RGBA32F, buffer.buffer);
                gl::BindBuffer(gl::TEXTURE_BUFFER, 0);
            }
            let min = samples
                .iter()
                .fold([f32::MAX; 2], |a, p| [a[0].min(p[0]), a[1].min(p[1])]);
            let max = samples
                .iter()
                .fold([f32::MIN; 2], |a, p| [a[0].max(p[0]), a[1].max(p[1])]);
            buffer.bounds = [min[0], min[1], max[0], max[1]];
            buffer.half_width = samples.iter().map(|p| p[2] * 0.5).fold(0.0_f32, f32::max);
            buffer.source = body.points.clone();
            buffer.kind = Some(body.kind);
            buffer.samples = samples;
        }
        unsafe {
            gl::ActiveTexture(gl::TEXTURE1);
        }
    }
    pub fn active_layers(&self, key: [u32; 2]) -> Vec<String> {
        self.textures
            .iter()
            .filter(|(_, map)| {
                (0..2).any(|dy| {
                    (0..2).any(|dx| {
                        map.contains_key(&PageId {
                            chart: 1,
                            x: key[0] as i32 * 2 + dx,
                            y: key[1] as i32 * 2 + dy,
                        })
                    })
                })
            })
            .map(|(id, _)| id.clone())
            .collect()
    }
    pub unsafe fn bind(&self, key: [u32; 2], layer: &str) -> u32 {
        let Some(map) = self.textures.get(layer) else {
            return 0;
        };
        let mut flags = 0;
        for dy in 0..2 {
            for dx in 0..2 {
                let index = dy * 2 + dx;
                let id = PageId {
                    chart: 1,
                    x: key[0] as i32 * 2 + dx,
                    y: key[1] as i32 * 2 + dy,
                };
                let texture = map.get(&id).copied().unwrap_or(0);
                unsafe {
                    gl::ActiveTexture(gl::TEXTURE7 + index as u32);
                    gl::BindTexture(gl::TEXTURE_2D, texture);
                }
                if texture != 0 {
                    flags |= 1 << index;
                }
            }
        }
        unsafe {
            gl::ActiveTexture(gl::TEXTURE1);
        }
        flags
    }
    pub unsafe fn bind_body(&self, body: &WaterBody, origin: [f32; 2]) -> Option<usize> {
        let buffer = self.bodies.get(&body.id)?;
        let margin = body.bank_width + buffer.half_width;
        let min = [buffer.bounds[0], buffer.bounds[1]];
        let max = [buffer.bounds[2], buffer.bounds[3]];
        if origin[0] > max[0] + margin
            || origin[0] + 64.0 < min[0] - margin
            || origin[1] > max[1] + margin
            || origin[1] + 64.0 < min[1] - margin
        {
            return None;
        }
        unsafe {
            gl::ActiveTexture(gl::TEXTURE15);
            gl::BindTexture(gl::TEXTURE_BUFFER, buffer.texture);
            gl::ActiveTexture(gl::TEXTURE1);
        }
        Some(buffer.samples.len())
    }
}
impl Drop for LayerGpuPages {
    fn drop(&mut self) {
        unsafe {
            self.clear();
            for body in self.bodies.values() {
                gl::DeleteTextures(1, &body.texture);
                gl::DeleteBuffers(1, &body.buffer);
            }
        }
    }
}

#[cfg(test)]
mod unified_terrain_tests {
    use super::*;
    use crate::landscape::{material_weights::MaterialWeightPage, paint_mask::CircularPaintBrush};
    #[test]
    fn unified_terrain_gpu_weights_use_stable_ids_and_neighbor_halos() {
        let mut atlas = MaterialWeightAtlas::default();
        for x in 0..2 {
            let id = PageId { chart: 1, x, y: 0 };
            atlas.pages.insert(
                id,
                MaterialWeightPage::new(id, [x as f32 * 32.0, 0.0], 0.25).unwrap(),
            );
        }
        let brush = CircularPaintBrush {
            center: [32.0, 0.0, 16.0],
            radius: 5.0,
            edge_softness: 0.5,
        };
        for i in 0..12 {
            atlas.stroke_layer(1, &brush, &format!("target-{i:02}"), 0.15);
        }
        let left = PageId {
            chart: 1,
            x: 0,
            y: 0,
        };
        let right = PageId {
            chart: 1,
            x: 1,
            y: 0,
        };
        for i in 0..12 {
            let layer = format!("target-{i:02}");
            assert_eq!(
                layer_pixel(&atlas, left, &layer, 128, 64),
                layer_pixel(&atlas, right, &layer, 0, 64)
            );
            assert_eq!(
                layer_pixel(&atlas, left, &layer, 127, 64),
                layer_pixel(&atlas, right, &layer, -1, 64)
            );
        }
        let page = &atlas.pages[&right];
        let texel = 64 * PAGE_DIM;
        let mut result = 1.0
            - page
                .layers
                .values()
                .map(|w| w[texel] as f32 / 255.0)
                .sum::<f32>();
        for id in page.layers.keys() {
            let alpha = layer_pixel(&atlas, right, id, 0, 64)[3] as f32 / 255.0;
            result = result * (1.0 - alpha) + alpha;
        }
        assert!(result.is_finite() && (0.0..=1.0).contains(&result));
        assert_eq!(layer_pixel(&atlas, right, "missing", 0, 64), [0; 4]);
    }
}

#[cfg(all(test, windows))]
mod unified_terrain_gpu_tests {
    use super::*;
    use crate::landscape::{material_weights::MaterialWeightPage, paint_mask::CircularPaintBrush};
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
    #[ignore = "Requires a desktop OpenGL driver; isolated hidden shader fixture, never launches the engine"]
    fn unified_terrain_actual_gpu_custom_layers_and_lake_river_shores() {
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
        let attributes = ContextAttributesBuilder::new().build(Some(handle));
        let context = unsafe { config.display().create_context(&config, &attributes) }.unwrap();
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
        let uniform = |name: &str| super::super::resources::get_uniform_location(program, name);
        let mut terrain = TerrainState::default();
        let id = PageId {
            chart: 1,
            x: 0,
            y: 0,
        };
        terrain
            .paint_weights
            .pages
            .insert(id, MaterialWeightPage::new(id, [0.0, 0.0], 0.25).unwrap());
        let brush = CircularPaintBrush {
            center: [16.0, 0.0, 16.0],
            radius: 4.0,
            edge_softness: 0.0,
        };
        for i in 0..12 {
            terrain
                .paint_weights
                .stroke_layer(1, &brush, &format!("target-{i:02}"), 0.1);
        }
        terrain
            .config
            .water_bodies
            .push(WaterBody::new(1, WaterBodyKind::Lake, [16.0, 16.0], 0.0));
        terrain.config.water_bodies.push(WaterBody::new(
            2,
            WaterBodyKind::River,
            [16.0, 16.0],
            0.0,
        ));
        let mut gpu = LayerGpuPages::default();
        let mut vao = 0;
        let mut vbo = 0;
        let mut fbo = 0;
        let mut output = 0;
        let mut color_texture = 0;
        unsafe {
            gpu.sync(&terrain, &[], true);
            gl::UseProgram(program);
            for (name, slot) in [
                ("u_texture", 1),
                ("u_shadow_map", 0),
                ("u_gi_cache", 2),
                ("u_terrain_surface_texture", 3),
                ("u_terrain_subsurface_texture", 4),
                ("u_terrain_rock_texture", 5),
                ("u_terrain_sand_texture", 6),
                ("u_paint_page0", 7),
                ("u_paint_page1", 8),
                ("u_paint_page2", 9),
                ("u_paint_page3", 10),
                ("u_terrain_snow_texture", 11),
                ("u_terrain_mud_texture", 12),
                ("u_local_water_atlas", 13),
                ("u_terrain_shore_points", 15),
            ] {
                gl::Uniform1i(uniform(name), slot);
            }
            gl::Uniform1i(uniform("u_debug_view"), 3);
            gl::Uniform1i(uniform("u_terrain_vertex_layers"), 1);
            gl::Uniform1f(uniform("u_alpha"), 1.0);
            gl::Uniform3f(uniform("u_base_color"), 1.0, 1.0, 1.0);
            gl::Uniform2f(uniform("u_paint_origin"), 0.0, 0.0);
            let model = glam::Mat4::from_translation(glam::Vec3::new(16.0, 0.0, 16.0));
            gl::UniformMatrix4fv(
                uniform("u_model"),
                1,
                gl::FALSE,
                model.to_cols_array().as_ptr(),
            );
            gl::UniformMatrix4fv(
                uniform("u_view_projection"),
                1,
                gl::FALSE,
                model.inverse().to_cols_array().as_ptr(),
            );
            gl::GenVertexArrays(1, &mut vao);
            gl::BindVertexArray(vao);
            gl::GenBuffers(1, &mut vbo);
            gl::BindBuffer(gl::ARRAY_BUFFER, vbo);
            let mut vertices = Vec::<f32>::new();
            for p in [
                [-1.0, -1.0],
                [1.0, -1.0],
                [1.0, 1.0],
                [-1.0, -1.0],
                [1.0, 1.0],
                [-1.0, 1.0],
            ] {
                vertices.extend_from_slice(&[
                    p[0], p[1], 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 0.0,
                ]);
            }
            gl::BufferData(
                gl::ARRAY_BUFFER,
                (vertices.len() * 4) as isize,
                vertices.as_ptr().cast(),
                gl::STATIC_DRAW,
            );
            for (index, size, offset) in [(0, 3, 0), (1, 3, 12), (2, 4, 24), (3, 2, 40)] {
                gl::EnableVertexAttribArray(index);
                gl::VertexAttribPointer(index, size, gl::FLOAT, gl::FALSE, 48, offset as *const _);
            }
            gl::GenTextures(1, &mut output);
            gl::BindTexture(gl::TEXTURE_2D, output);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                64,
                64,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                std::ptr::null(),
            );
            gl::GenFramebuffers(1, &mut fbo);
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
            gl::FramebufferTexture2D(
                gl::FRAMEBUFFER,
                gl::COLOR_ATTACHMENT0,
                gl::TEXTURE_2D,
                output,
                0,
            );
            assert_eq!(
                gl::CheckFramebufferStatus(gl::FRAMEBUFFER),
                gl::FRAMEBUFFER_COMPLETE
            );
            gl::Viewport(0, 0, 64, 64);
            gl::Disable(gl::DEPTH_TEST);
            gl::Disable(gl::CULL_FACE);
            gl::GenTextures(1, &mut color_texture);
        }
        let set_color = |color: [u8; 3]| unsafe {
            gl::ActiveTexture(gl::TEXTURE1);
            gl::BindTexture(gl::TEXTURE_2D, color_texture);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGB8 as i32,
                1,
                1,
                0,
                gl::RGB,
                gl::UNSIGNED_BYTE,
                color.as_ptr().cast(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        };
        let read = || unsafe {
            let mut pixel = [0u8; 4];
            gl::ReadPixels(
                32,
                32,
                1,
                1,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixel.as_mut_ptr().cast(),
            );
            pixel
        };
        unsafe {
            gl::ClearColor(0.5, 0.5, 0.5, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::Uniform1i(uniform("u_terrain_layer_pass"), 1);
        }
        let weights = &terrain.paint_weights.pages[&id].layers;
        let texel = 64 * PAGE_DIM + 64;
        let residual = 1.0
            - weights
                .values()
                .map(|w| w[texel] as f32 / 255.0)
                .sum::<f32>();
        let mut expected = [127.5 * residual; 3];
        for (i, target) in gpu.active_layers([0, 0]).iter().enumerate() {
            let color = if i % 3 == 0 {
                [255, 0, 0]
            } else if i % 3 == 1 {
                [0, 255, 0]
            } else {
                [0, 0, 255]
            };
            set_color(color);
            for k in 0..3 {
                expected[k] += color[k] as f32 * weights[target][texel] as f32 / 255.0;
            }
            unsafe {
                gl::Uniform1i(uniform("u_paint_flags"), gpu.bind([0, 0], target) as i32);
                gl::DrawArrays(gl::TRIANGLES, 0, 6);
            }
        }
        let pixel = read();
        for k in 0..3 {
            assert!(
                (pixel[k] as f32 - expected[k]).abs() < 4.0,
                "GPU {pixel:?}, expected {expected:?}"
            );
        }
        set_color([240, 40, 10]);
        unsafe {
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::Disable(gl::BLEND);
            gl::Uniform1i(uniform("u_paint_flags"), 0);
        }
        for body in &terrain.config.water_bodies {
            unsafe {
                let count = gpu.bind_body(body, [0.0, 0.0]).unwrap();
                gl::Uniform1i(uniform("u_terrain_layer_pass"), 2);
                gl::Uniform1i(uniform("u_terrain_shore_count"), count as i32);
                gl::Uniform4f(
                    uniform("u_terrain_shore_params"),
                    if body.kind == WaterBodyKind::River {
                        1.0
                    } else {
                        0.0
                    },
                    body.elevation,
                    body.bank_width,
                    1.0,
                );
                gl::DrawArrays(gl::TRIANGLES, 0, 6);
            }
            assert_eq!(
                &read()[..3],
                &[240, 40, 10],
                "selected shore Material must render for {:?}",
                body.kind
            );
        }
        unsafe {
            gl::ClearColor(0.0, 0.0, 0.0, 1.0);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::Uniform1i(uniform("u_terrain_layer_pass"), 3);
            gl::Uniform4f(uniform("u_terrain_shore_params"), 0.0, -10.0, 3.0, 1.0);
            gl::DrawArrays(gl::TRIANGLES, 0, 6);
        }
        assert_eq!(
            &read()[..3],
            &[0, 0, 0],
            "high ground is outside the shoreline band"
        );
        assert_eq!(unsafe { gl::GetError() }, gl::NO_ERROR);
        drop(gpu);
        let renderer = super::super::Renderer::new(64.0, 64.0);
        let mut world = crate::world::World::new();
        let target = world.terrain.config.add_material_layer();
        world
            .terrain
            .config
            .material_layers
            .last_mut()
            .unwrap()
            .material = "Material:gpu-fixture".into();
        world.terrain.config.grass.enabled = false;
        world
            .terrain
            .generate_authored(crate::world::terrain::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [64, 40, 64],
            })
            .unwrap();
        assert!(
            world
                .terrain
                .paint_base_and_record(&crate::world::terrain::TerrainPaintRequest {
                    center: [32.0, 16.0, 32.0],
                    radius: 5.0,
                    material: format!("layer:{target}"),
                    strength: 1.0,
                    falloff: Some(0.3)
                })
        );
        let mut bound_texture = 0;
        unsafe {
            gl::ActiveTexture(gl::TEXTURE1);
            gl::GenTextures(1, &mut bound_texture);
            gl::BindTexture(gl::TEXTURE_2D, bound_texture);
            let color = [240u8, 40, 10, 255];
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                1,
                1,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                color.as_ptr().cast(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::NEAREST as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::NEAREST as i32);
        }
        renderer
            .textures
            .borrow_mut()
            .insert("Material:gpu-fixture".into(), bound_texture);
        let vp = glam::Mat4::orthographic_rh(-32.0, 32.0, -32.0, 32.0, 0.1, 150.0)
            * glam::Mat4::look_at_rh(
                glam::Vec3::new(32.0, 80.0, 32.0),
                glam::Vec3::new(32.0, 0.0, 32.0),
                -glam::Vec3::Z,
            );
        unsafe {
            gl::BindFramebuffer(gl::FRAMEBUFFER, fbo);
            gl::Viewport(0, 0, 64, 64);
            gl::Clear(gl::COLOR_BUFFER_BIT);
            gl::Disable(gl::DEPTH_TEST);
            gl::Disable(gl::CULL_FACE);
            gl::Disable(gl::BLEND);
            gl::UseProgram(renderer.grid_program);
            gl::UniformMatrix4fv(
                renderer.grid_view_projection_location,
                1,
                gl::FALSE,
                vp.to_cols_array().as_ptr(),
            );
            gl::Uniform1i(renderer.grid_debug_view_location, 3);
            gl::Uniform1f(renderer.grid_alpha_location, 1.0);
            gl::Uniform1i(
                super::super::resources::get_uniform_location(
                    renderer.grid_program,
                    "u_local_water_atlas",
                ),
                13,
            );
        }
        let stats = renderer.draw_terrain(
            &world,
            &super::super::chunk::CameraFrustum::from_view_projection(vp),
            glam::Vec3::new(32.0, 16.0, 32.0),
            4,
            false,
        );
        assert!(stats.draws >= 2);
        assert_eq!(
            &read()[..3],
            &[240, 40, 10],
            "native draw must resolve the custom Material target through its weight pages"
        );
        assert_eq!(unsafe { gl::GetError() }, gl::NO_ERROR);
        drop(renderer);
        unsafe {
            gl::DeleteProgram(program);
            gl::DeleteTextures(1, &output);
            gl::DeleteTextures(1, &color_texture);
            gl::DeleteFramebuffers(1, &fbo);
            gl::DeleteBuffers(1, &vbo);
            gl::DeleteVertexArrays(1, &vao);
        }
    }
}
