// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::landscape::material_weights::{DirtyUpload, PAGE_DIM, PageId};
use crate::world::terrain::TerrainState;
use std::collections::BTreeMap;

#[derive(Default)]
pub(super) struct PaintGpuPages {
    pub textures: BTreeMap<PageId, u32>,
    pub layers: super::terrain_layer_gl::LayerGpuPages,
    epoch: u64,
    initialized: bool,
}
fn convert_source(source: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(source.len());
    for pixel in source.chunks_exact(4) {
        let paint = 255u8.saturating_sub(pixel[0]);
        out.extend_from_slice(&[pixel[1], pixel[2], pixel[3], paint]);
    }
    out
}
const FILTER_DIM: usize = PAGE_DIM + 2;
fn converted_pixel(
    atlas: &crate::landscape::material_weights::MaterialWeightAtlas,
    id: PageId,
    x: i32,
    y: i32,
) -> [u8; 4] {
    let global_x = id.x as i64 * PAGE_DIM as i64 + x as i64;
    let global_y = id.y as i64 * PAGE_DIM as i64 + y as i64;
    let page_x = global_x.div_euclid(PAGE_DIM as i64) as i32;
    let page_y = global_y.div_euclid(PAGE_DIM as i64) as i32;
    let local_x = global_x.rem_euclid(PAGE_DIM as i64) as usize;
    let local_y = global_y.rem_euclid(PAGE_DIM as i64) as usize;
    let other = PageId {
        chart: id.chart,
        x: page_x,
        y: page_y,
    };
    let source = atlas.pages.get(&other).and_then(|page| {
        let offset = (local_y * PAGE_DIM + local_x) * 4;
        page.rgba.get(offset..offset + 4)
    });
    match source {
        Some(p) => [p[1], p[2], p[3], 255u8.saturating_sub(p[0])],
        None => [0, 0, 0, 0],
    }
}
fn bordered_pixels(
    atlas: &crate::landscape::material_weights::MaterialWeightAtlas,
    id: PageId,
) -> Vec<u8> {
    let mut rgba = Vec::with_capacity(FILTER_DIM * FILTER_DIM * 4);
    for y in -1..=PAGE_DIM as i32 {
        for x in -1..=PAGE_DIM as i32 {
            rgba.extend_from_slice(&converted_pixel(atlas, id, x, y));
        }
    }
    rgba
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct EdgeUpdate {
    target: PageId,
    x: i32,
    y: i32,
    width: i32,
    height: i32,
}
fn edge_updates(
    id: PageId,
    rect: crate::landscape::material_weights::DirtyRect,
) -> Vec<EdgeUpdate> {
    let mut result = Vec::new();
    for dy in -1i32..=1 {
        if dy == -1 && rect.y0 != 0 {
            continue;
        }
        if dy == 1 && rect.y1 != PAGE_DIM as u16 - 1 {
            continue;
        }
        for dx in -1i32..=1 {
            if dx == 0 && dy == 0 {
                continue;
            }
            if dx == -1 && rect.x0 != 0 {
                continue;
            }
            if dx == 1 && rect.x1 != PAGE_DIM as u16 - 1 {
                continue;
            }
            let (x, width) = match dx {
                -1 => (PAGE_DIM as i32 + 1, 1),
                1 => (0, 1),
                _ => (rect.x0 as i32 + 1, (rect.x1 - rect.x0 + 1) as i32),
            };
            let (y, height) = match dy {
                -1 => (PAGE_DIM as i32 + 1, 1),
                1 => (0, 1),
                _ => (rect.y0 as i32 + 1, (rect.y1 - rect.y0 + 1) as i32),
            };
            result.push(EdgeUpdate {
                target: PageId {
                    chart: id.chart,
                    x: id.x + dx,
                    y: id.y + dy,
                },
                x,
                y,
                width,
                height,
            });
        }
    }
    result
}
impl PaintGpuPages {
    pub(super) fn invalidate(&mut self) {
        self.initialized = false;
    }
    unsafe fn create(
        &mut self,
        atlas: &crate::landscape::material_weights::MaterialWeightAtlas,
        id: PageId,
    ) {
        let pixels = bordered_pixels(atlas, id);
        let mut texture = 0;
        unsafe {
            gl::GenTextures(1, &mut texture);
            gl::BindTexture(gl::TEXTURE_2D, texture);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::RGBA8 as i32,
                FILTER_DIM as i32,
                FILTER_DIM as i32,
                0,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
        }
        self.textures.insert(id, texture);
    }
    fn required_pages(
        atlas: &crate::landscape::material_weights::MaterialWeightAtlas,
    ) -> std::collections::BTreeSet<PageId> {
        let mut keys = std::collections::BTreeSet::new();
        for id in atlas.pages.keys() {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    keys.insert(PageId {
                        chart: id.chart,
                        x: id.x + dx,
                        y: id.y + dy,
                    });
                }
            }
        }
        keys
    }
    unsafe fn upload_region(
        &self,
        atlas: &crate::landscape::material_weights::MaterialWeightAtlas,
        region: EdgeUpdate,
    ) {
        let Some(texture) = self.textures.get(&region.target) else {
            return;
        };
        let mut pixels = Vec::with_capacity((region.width * region.height * 4) as usize);
        for y in region.y..region.y + region.height {
            for x in region.x..region.x + region.width {
                pixels.extend_from_slice(&converted_pixel(atlas, region.target, x - 1, y - 1));
            }
        }
        unsafe {
            gl::BindTexture(gl::TEXTURE_2D, *texture);
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                region.x,
                region.y,
                region.width,
                region.height,
                gl::RGBA,
                gl::UNSIGNED_BYTE,
                pixels.as_ptr().cast(),
            );
            gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
        }
    }
    pub unsafe fn sync(&mut self, terrain: &TerrainState) {
        let rebuilt = !self.initialized || self.epoch != terrain.paint_epoch;
        if rebuilt {
            unsafe {
                for tex in self.textures.values() {
                    gl::DeleteTextures(1, tex);
                }
            }
            self.textures.clear();
            self.epoch = terrain.paint_epoch;
            self.initialized = true;
            unsafe {
                gl::ActiveTexture(gl::TEXTURE10);
                for id in Self::required_pages(&terrain.paint_weights) {
                    self.create(&terrain.paint_weights, id);
                }
            }
        }
        let uploads = terrain
            .paint_pending
            .borrow_mut()
            .drain(..)
            .collect::<Vec<DirtyUpload>>();
        unsafe {
            self.layers.sync(terrain, &uploads, rebuilt);
        }
        if uploads.is_empty() {
            unsafe {
                gl::ActiveTexture(gl::TEXTURE1);
            }
            return;
        }
        unsafe {
            gl::ActiveTexture(gl::TEXTURE10);
        }
        let mut newly_created = std::collections::BTreeSet::new();
        for upload in &uploads {
            for dy in -1..=1 {
                for dx in -1..=1 {
                    let id = PageId {
                        chart: upload.id.chart,
                        x: upload.id.x + dx,
                        y: upload.id.y + dy,
                    };
                    if !self.textures.contains_key(&id) {
                        unsafe {
                            self.create(&terrain.paint_weights, id);
                        }
                        newly_created.insert(id);
                    }
                }
            }
        }
        for upload in uploads {
            if !newly_created.contains(&upload.id) {
                let region = EdgeUpdate {
                    target: upload.id,
                    x: upload.rect.x0 as i32 + 1,
                    y: upload.rect.y0 as i32 + 1,
                    width: upload.row_length as i32,
                    height: (upload.rect.y1 - upload.rect.y0 + 1) as i32,
                };
                if let Some(texture) = self.textures.get(&region.target) {
                    let converted = convert_source(&upload.rgba);
                    unsafe {
                        gl::BindTexture(gl::TEXTURE_2D, *texture);
                        gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
                        gl::TexSubImage2D(
                            gl::TEXTURE_2D,
                            0,
                            region.x,
                            region.y,
                            region.width,
                            region.height,
                            gl::RGBA,
                            gl::UNSIGNED_BYTE,
                            converted.as_ptr().cast(),
                        );
                        gl::PixelStorei(gl::UNPACK_ALIGNMENT, 4);
                    }
                }
            }
            for region in edge_updates(upload.id, upload.rect) {
                if newly_created.contains(&region.target) {
                    continue;
                }
                unsafe {
                    self.upload_region(&terrain.paint_weights, region);
                }
            }
        }
        unsafe {
            gl::ActiveTexture(gl::TEXTURE1);
        }
    }
    pub unsafe fn bind_chunk(&self, key: [u32; 2], origin: [f32; 2]) -> u32 {
        let x = (key[0] as i32) * 2;
        let y = (key[1] as i32) * 2;
        let mut flags = 0u32;
        for dz in 0..2 {
            for dx in 0..2 {
                let i = dz * 2 + dx;
                let id = PageId {
                    chart: 1,
                    x: x + dx as i32,
                    y: y + dz as i32,
                };
                let texture = self.textures.get(&id).copied().unwrap_or(0);
                unsafe {
                    gl::ActiveTexture(gl::TEXTURE7 + i as u32);
                    gl::BindTexture(gl::TEXTURE_2D, texture);
                }
                if texture != 0 {
                    flags |= 1 << i;
                }
            }
        }
        let _ = origin;
        unsafe {
            gl::ActiveTexture(gl::TEXTURE1);
        }
        flags
    }
}
impl Drop for PaintGpuPages {
    fn drop(&mut self) {
        unsafe {
            for tex in self.textures.values() {
                gl::DeleteTextures(1, tex)
            }
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn untouched_map_is_transparent_and_rock_is_solid() {
        assert_eq!(convert_source(&[255, 0, 0, 0]), [0, 0, 0, 0]);
        assert_eq!(convert_source(&[12, 3, 220, 20]), [3, 220, 20, 243]);
    }
}

#[cfg(test)]
mod continuous_paint_tests {
    use super::convert_source;
    #[test]
    fn paint_gl_uses_fractional_brush_coverage_not_binary_rectangles() {
        let pixels = [255, 0, 0, 0, 230, 25, 0, 0, 128, 64, 63, 0, 0, 255, 0, 0];
        let result = convert_source(&pixels);
        assert_eq!(&result[0..4], &[0, 0, 0, 0]);
        assert_eq!(&result[4..8], &[25, 0, 0, 25]);
        assert_eq!(&result[8..12], &[64, 63, 0, 127]);
        assert_eq!(&result[12..16], &[255, 0, 0, 255]);
    }
}

#[cfg(test)]
mod border_continuity_tests {
    use super::*;
    use crate::landscape::material_weights::{DirtyRect, MaterialWeightAtlas, MaterialWeightPage};

    fn atlas() -> MaterialWeightAtlas {
        let mut a = MaterialWeightAtlas::default();
        for (x, y) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
            let id = PageId { chart: 1, x, y };
            a.pages.insert(
                id,
                MaterialWeightPage::new(id, [x as f32 * 32.0, y as f32 * 32.0], 0.25).unwrap(),
            );
        }
        a
    }
    fn sample(pixels: &[u8], x: usize, y: usize) -> [u8; 4] {
        let i = (y * FILTER_DIM + x) * 4;
        pixels[i..i + 4].try_into().unwrap()
    }
    #[test]
    fn paint_pages_have_identical_halo_texels_on_all_four_edges_and_corners() {
        let mut atlas = atlas();
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
        let corner = PageId {
            chart: 1,
            x: 1,
            y: 1,
        };
        let p = atlas.pages.get_mut(&left).unwrap();
        for y in 0..PAGE_DIM {
            let i = (y * PAGE_DIM + PAGE_DIM - 1) * 4;
            p.rgba[i..i + 4].copy_from_slice(&[55, 100, 100, 0]);
        }
        let p = atlas.pages.get_mut(&right).unwrap();
        for y in 0..PAGE_DIM {
            let i = (y * PAGE_DIM) * 4;
            p.rgba[i..i + 4].copy_from_slice(&[5, 250, 0, 0]);
        }
        let a = bordered_pixels(&atlas, left);
        let b = bordered_pixels(&atlas, right);
        for y in 1..=PAGE_DIM {
            assert_eq!(sample(&a, PAGE_DIM + 1, y), sample(&b, 1, y));
            assert_eq!(sample(&a, PAGE_DIM, y), sample(&b, 0, y));
        }
        let c = bordered_pixels(&atlas, corner);
        assert_eq!(sample(&a, PAGE_DIM + 1, PAGE_DIM + 1), sample(&c, 1, 1));
    }
    #[test]
    fn border_samples_are_continuous_across_render_chunk_edges_and_missing_pages() {
        let mut atlas = atlas();
        let id = PageId {
            chart: 1,
            x: 1,
            y: 1,
        };
        let p = atlas.pages.get_mut(&id).unwrap();
        let i = (100 * PAGE_DIM + 127) * 4;
        p.rgba[i..i + 4].copy_from_slice(&[15, 0, 240, 0]);
        let next = PageId {
            chart: 1,
            x: 2,
            y: 1,
        };
        let left = bordered_pixels(&atlas, id);
        let right = bordered_pixels(&atlas, next);
        assert_eq!(sample(&left, PAGE_DIM + 1, 101), sample(&right, 1, 101));
        assert_eq!(sample(&left, PAGE_DIM, 101), sample(&right, 0, 101));
        assert!(PaintGpuPages::required_pages(&atlas).contains(&next));
    }
    #[test]
    fn ordinary_paint_strokes_only_upload_dirty_interior_and_touched_halo() {
        let id = PageId {
            chart: 1,
            x: 0,
            y: 0,
        };
        let interior = DirtyRect {
            x0: 40,
            y0: 40,
            x1: 45,
            y1: 45,
        };
        assert!(edge_updates(id, interior).is_empty());
        let boundary = DirtyRect {
            x0: 126,
            y0: 0,
            x1: 127,
            y1: 4,
        };
        let neighbors = edge_updates(id, boundary);
        assert_eq!(neighbors.len(), 3);
        assert!(neighbors.iter().all(|n| n.width == 1 || n.height == 1));
        assert!(neighbors.iter().all(|n| n.width * n.height <= 5));
    }
}

#[cfg(test)]
mod shader_boundary_sampling_tests {
    use super::*;
    use crate::landscape::material_weights::{MaterialWeightAtlas, MaterialWeightPage};

    fn page(a: &mut MaterialWeightAtlas, x: i32, y: i32) -> PageId {
        let id = PageId { chart: 1, x, y };
        a.pages.insert(
            id,
            MaterialWeightPage::new(id, [x as f32 * 32.0, y as f32 * 32.0], 0.25).unwrap(),
        );
        id
    }
    fn bilinear(data: &[u8], u: f32, v: f32) -> [f32; 4] {
        let tx = (u * FILTER_DIM as f32 - 0.5).clamp(0.0, (FILTER_DIM - 1) as f32);
        let ty = (v * FILTER_DIM as f32 - 0.5).clamp(0.0, (FILTER_DIM - 1) as f32);
        let x = tx.floor() as usize;
        let y = ty.floor() as usize;
        let x2 = (x + 1).min(FILTER_DIM - 1);
        let y2 = (y + 1).min(FILTER_DIM - 1);
        let ax = tx - x as f32;
        let ay = ty - y as f32;
        let mut result = [0.0; 4];
        for c in 0..4 {
            let pix = |x: usize, y: usize| data[(y * FILTER_DIM + x) * 4 + c] as f32;
            let upper = pix(x, y) * (1.0 - ax) + pix(x2, y) * ax;
            let lower = pix(x, y2) * (1.0 - ax) + pix(x2, y2) * ax;
            result[c] = upper * (1.0 - ay) + lower * ay;
        }
        result
    }
    fn sample_world(atlas: &MaterialWeightAtlas, x: f32, y: f32) -> [f32; 4] {
        let page_x = (x / 32.0).floor() as i32;
        let page_y = (y / 32.0).floor() as i32;
        let uv_x = ((x / 32.0).fract().rem_euclid(1.0) * 128.0 + 1.0) / FILTER_DIM as f32;
        let uv_y = ((y / 32.0).fract().rem_euclid(1.0) * 128.0 + 1.0) / FILTER_DIM as f32;
        let pixels = bordered_pixels(
            atlas,
            PageId {
                chart: 1,
                x: page_x,
                y: page_y,
            },
        );
        bilinear(&pixels, uv_x, uv_y)
    }
    fn assert_continuous(a: &[f32; 4], b: &[f32; 4]) {
        for (left, right) in a.iter().zip(b) {
            assert!(
                (left - right).abs() < 0.5,
                "visible shader-page seam: left {a:?}, right {b:?}"
            );
        }
    }
    #[test]
    fn filtered_color_is_continuous_across_32_and_64_stud_boundaries() {
        let mut a = MaterialWeightAtlas::default();
        for x in 0..4 {
            let id = page(&mut a, x, 0);
            let data = &mut a.pages.get_mut(&id).unwrap().rgba;
            for y in 0..PAGE_DIM {
                for px in 0..PAGE_DIM {
                    let v = if px < 64 { 65 } else { 235 };
                    let i = (y * PAGE_DIM + px) * 4;
                    data[i..i + 4].copy_from_slice(&[255 - v, v, 0, 0]);
                }
            }
        }
        for at in [32.0, 64.0, 96.0] {
            let before = sample_world(&a, at - 0.0001, 15.875);
            let after = sample_world(&a, at + 0.0001, 15.875);
            assert_continuous(&before, &after);
        }
    }
    #[test]
    fn filtered_color_is_continuous_over_negative_and_missing_pages() {
        let mut a = MaterialWeightAtlas::default();
        for x in [-1, 0, 1] {
            let id = page(&mut a, x, 0);
            for y in 0..PAGE_DIM {
                let i = (y * PAGE_DIM + 127) * 4;
                a.pages.get_mut(&id).unwrap().rgba[i..i + 4].copy_from_slice(&[15, 0, 240, 0]);
            }
        }
        for at in [-32.0, 0.0, 32.0, 64.0] {
            assert_continuous(
                &sample_world(&a, at - 0.0001, 12.125),
                &sample_world(&a, at + 0.0001, 12.125),
            );
        }
    }
    #[test]
    fn filtered_color_is_continuous_on_vertical_page_and_chunk_boundaries() {
        let mut a = MaterialWeightAtlas::default();
        for y in 0..4 {
            let id = page(&mut a, 0, y);
            let p = a.pages.get_mut(&id).unwrap();
            for x in 0..PAGE_DIM {
                let idx = ((PAGE_DIM - 1) * PAGE_DIM + x) * 4;
                p.rgba[idx..idx + 4].copy_from_slice(&[20, 160, 75, 0]);
            }
        }
        for at in [32.0, 64.0, 96.0] {
            assert_continuous(
                &sample_world(&a, 10.125, at - 0.0001),
                &sample_world(&a, 10.125, at + 0.0001),
            );
        }
    }
    #[test]
    fn fragment_shader_uses_padded_page_filtering_and_single_coverage() {
        let source = super::super::GRID_FRAGMENT_SHADER;
        assert!(source.contains("vec2 uv=(fract(page)*128.0+1.0)/130.0"));
        assert!(source.contains("tex_color.rgb=mix(tex_color.rgb,paint_color,coverage)"));
        assert!(!source.contains("vec2 uv=fract(page);"));
    }
}
