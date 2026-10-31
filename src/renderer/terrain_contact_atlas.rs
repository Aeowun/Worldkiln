// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::*;

const DISTANCE_RANGE: f32 = 3.0;

#[derive(Clone, Copy, Debug)]
struct Rect {
    x0: usize,
    z0: usize,
    x1: usize,
    z1: usize,
}
impl Rect {
    fn expand(self, halo: usize, n: usize) -> Self {
        Self {
            x0: self.x0.saturating_sub(halo),
            z0: self.z0.saturating_sub(halo),
            x1: self.x1.saturating_add(halo).min(n),
            z1: self.z1.saturating_add(halo).min(n),
        }
    }
}

pub(super) struct AtlasUpload {
    pub x: usize,
    pub z: usize,
    pub width: usize,
    pub height: usize,
    pub pixels: Vec<u8>,
    #[cfg(test)]
    pub sampled_columns: usize,
}

pub(super) struct OceanContactAtlas {
    pub n: usize,
    pub mapping: [f32; 4],
    pitch: f32,
    mask: Vec<bool>,
    #[cfg(test)]
    pub pixels: Vec<u8>,
}

impl OceanContactAtlas {
    pub fn new(result: &TerrainResult) -> Self {
        let (min, max) = match &result.data {
            TerrainData::Heightfield(f) => (
                [f.origin[0] as f32 + 0.5, f.origin[2] as f32 + 0.5],
                [
                    f.origin[0] as f32 + f.size[0].saturating_sub(1) as f32 + 0.5,
                    f.origin[2] as f32 + f.size[2].saturating_sub(1) as f32 + 0.5,
                ],
            ),
            TerrainData::Voxel(g) => {
                let (a, b) = g.world_bounds();
                ([a[0], a[2]], [b[0], b[2]])
            }
        };
        let span = (max[0] - min[0]).max(max[1] - min[1]).max(1.0) + 6.0;
        let n = ((span / 0.75).ceil() as usize)
            .next_power_of_two()
            .clamp(256, 2048);
        Self {
            n,
            mapping: [min[0] - 3.0, min[1] - 3.0, 1.0 / span, 1.0 / span],
            pitch: span / n as f32,
            mask: vec![false; n * n],
            #[cfg(test)]
            pixels: vec![0; n * n],
        }
    }

    pub fn rebuild(&mut self, result: &TerrainResult, config: &TerrainConfig) -> AtlasUpload {
        self.refresh(
            result,
            config,
            Rect {
                x0: 0,
                z0: 0,
                x1: self.n,
                z1: self.n,
            },
        )
    }

    pub fn update_chunk(
        &mut self,
        result: &TerrainResult,
        config: &TerrainConfig,
        key: [u32; 2],
    ) -> AtlasUpload {
        let (origin, spacing) = match &result.data {
            TerrainData::Heightfield(f) => {
                ([f.origin[0] as f32 + 0.5, f.origin[2] as f32 + 0.5], 1.0)
            }
            TerrainData::Voxel(g) => ([g.origin[0] as f32, g.origin[2] as f32], g.spacing_studs),
        };
        let extent = terrain_render_chunk_samples(spacing) as f32 * spacing;
        let min = [
            origin[0] + key[0] as f32 * extent,
            origin[1] + key[1] as f32 * extent,
        ];
        let max = [min[0] + extent, min[1] + extent];
        let pixel = |p: f32, axis: usize| {
            ((p - self.mapping[axis]) / self.pitch)
                .floor()
                .max(0.0)
                .min(self.n as f32) as usize
        };
        let rect = Rect {
            x0: pixel(min[0], 0),
            z0: pixel(min[1], 1),
            x1: (pixel(max[0], 0) + 1).min(self.n),
            z1: (pixel(max[1], 1) + 1).min(self.n),
        };
        self.refresh(result, config, rect.expand(1, self.n))
    }

    fn refresh(
        &mut self,
        result: &TerrainResult,
        config: &TerrainConfig,
        rect: Rect,
    ) -> AtlasUpload {
        let (origin, spacing, cols, rows) = match &result.data {
            TerrainData::Heightfield(f) => (
                [f.origin[0] as f32 + 0.5, f.origin[2] as f32 + 0.5],
                1.0,
                f.size[0].saturating_sub(1),
                f.size[2].saturating_sub(1),
            ),
            TerrainData::Voxel(g) => (
                [g.origin[0] as f32, g.origin[2] as f32],
                g.spacing_studs,
                g.size[0],
                g.size[2],
            ),
        };
        let low = |v: usize, a: usize, limit: u32| {
            (((self.mapping[a] + v as f32 * self.pitch - origin[a]) / spacing).floor() as i64 - 2)
                .clamp(0, limit as i64) as u32
        };
        let high = |v: usize, a: usize, limit: u32| {
            (((self.mapping[a] + v as f32 * self.pitch - origin[a]) / spacing).ceil() as i64 + 2)
                .clamp(0, limit as i64) as u32
        };
        let (x0, x1, z0, z1) = (
            low(rect.x0, 0, cols),
            high(rect.x1, 0, cols),
            low(rect.z0, 1, rows),
            high(rect.z1, 1, rows),
        );
        let vertices = if !config.ocean_enabled {
            Vec::new()
        } else {
            match &result.data {
                TerrainData::Heightfield(f) => build_heightfield_water_region_with_runup(
                    f,
                    config.plains.water_level,
                    0.0,
                    0.0,
                    x0,
                    x1,
                    z0,
                    z1,
                ),
                TerrainData::Voxel(g) => build_voxel_water_region_with_runup(
                    g,
                    config.plains.water_level,
                    0.0,
                    0.0,
                    x0,
                    x1,
                    z0,
                    z1,
                ),
            }
        };
        for z in rect.z0..rect.z1 {
            self.mask[z * self.n + rect.x0..z * self.n + rect.x1].fill(false);
        }
        for tri in vertices.chunks_exact(36) {
            let pts: [[_; 2]; 3] = std::array::from_fn(|i| {
                [
                    (tri[i * 12] - self.mapping[0]) / self.pitch,
                    (tri[i * 12 + 2] - self.mapping[1]) / self.pitch,
                ]
            });
            let ab = [pts[1][0] - pts[0][0], pts[1][1] - pts[0][1]];
            let ac = [pts[2][0] - pts[0][0], pts[2][1] - pts[0][1]];
            let det = ab[0] * ac[1] - ab[1] * ac[0];
            if det.abs() < 1e-9 {
                continue;
            }
            let lo = |a: usize, lower: usize| {
                (pts.iter()
                    .map(|p| p[a])
                    .fold(f32::INFINITY, f32::min)
                    .floor()
                    .max(0.0) as usize)
                    .max(lower)
            };
            let hi = |a: usize, upper: usize| {
                (pts.iter()
                    .map(|p| p[a])
                    .fold(f32::NEG_INFINITY, f32::max)
                    .ceil()
                    .max(0.0) as usize
                    + 1)
                .min(upper)
            };
            for z in lo(1, rect.z0)..hi(1, rect.z1) {
                for x in lo(0, rect.x0)..hi(0, rect.x1) {
                    let rel = [x as f32 + 0.5 - pts[0][0], z as f32 + 0.5 - pts[0][1]];
                    let u = (rel[0] * ac[1] - rel[1] * ac[0]) / det;
                    let v = (ab[0] * rel[1] - ab[1] * rel[0]) / det;
                    if u >= -0.0001 && v >= -0.0001 && u + v <= 1.0001 {
                        self.mask[z * self.n + x] = true;
                    }
                }
            }
        }
        let halo = (DISTANCE_RANGE / self.pitch).ceil() as usize + 2;
        let output = rect.expand(halo, self.n);
        let input = output.expand(halo, self.n);
        let width = input.x1 - input.x0;
        let height = input.z1 - input.z0;
        let mut mask = Vec::with_capacity(width * height);
        for z in input.z0..input.z1 {
            mask.extend_from_slice(&self.mask[z * self.n + input.x0..z * self.n + input.x1]);
        }
        let dry: Vec<_> = mask.iter().map(|v| !*v).collect();
        let to_wet = ocean_nearest_squared_distance(&mask, width, height);
        let to_dry = ocean_nearest_squared_distance(&dry, width, height);
        let mut pixels = Vec::with_capacity((output.x1 - output.x0) * (output.z1 - output.z0));
        for z in output.z0..output.z1 {
            for x in output.x0..output.x1 {
                let i = (z - input.z0) * width + x - input.x0;
                let cells = if mask[i] { to_dry[i] } else { to_wet[i] };
                let edge = (cells.sqrt() - 0.5).max(0.0) * self.pitch;
                let signed = if mask[i] { edge } else { -edge };
                let value = ((0.5 + 0.5 * (signed / DISTANCE_RANGE).clamp(-1.0, 1.0)) * 255.0)
                    .round() as u8;
                pixels.push(value);
                #[cfg(test)]
                {
                    self.pixels[z * self.n + x] = value;
                }
            }
        }
        AtlasUpload {
            x: output.x0,
            z: output.z0,
            width: output.x1 - output.x0,
            height: output.z1 - output.z0,
            pixels,
            #[cfg(test)]
            sampled_columns: (x1 - x0) as usize * (z1 - z0) as usize,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn field(n: u32) -> TerrainResult {
        TerrainResult {
            request: crate::world::TerrainGenerationRequest {
                position: [-100, 0, -200],
                size: [n as i32, 64, n as i32],
            },
            generator: crate::world::TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: [-100, 0, -200],
                size: [n, 64, n],
                heights: (0..n * n)
                    .map(|i| if i % n < n / 2 { 4.0 } else { 16.0 })
                    .collect(),
            }),
        }
    }
    #[test]
    fn terrain_edit_perf_shore_patch_matches_full_rebuild_and_scales_with_chunk() {
        for n in [257, 1025] {
            let mut result = field(n);
            let mut config = TerrainConfig::default();
            config.ocean_enabled = true;
            config.plains.water_level = 10.0;
            let mut atlas = OceanContactAtlas::new(&result);
            atlas.rebuild(&result, &config);
            let initial_mapping = atlas.mapping;
            let x = n / 2;
            if let TerrainData::Heightfield(f) = &mut result.data {
                for z in 61..=67 {
                    for xx in x - 3..=x + 3 {
                        f.heights[(z * n + xx) as usize] = if xx < x { 20.0 } else { 2.0 };
                    }
                }
            }
            let mut max_columns = 0;
            for cz in 0..=1 {
                for cx in x.saturating_sub(4) / 64..=(x + 4) / 64 {
                    let upload = atlas.update_chunk(&result, &config, [cx, cz]);
                    max_columns = max_columns.max(upload.sampled_columns);
                    assert!(upload.width * upload.height < atlas.n * atlas.n / 2);
                }
            }
            let mut reference = OceanContactAtlas::new(&result);
            reference.rebuild(&result, &config);
            assert_eq!(atlas.mapping, initial_mapping);
            assert_eq!(atlas.mask, reference.mask, "coverage n={n}");
            assert_eq!(atlas.pixels, reference.pixels, "distance n={n}");
            assert!(
                max_columns < 80 * 80,
                "sampled {max_columns} columns on n={n}"
            );
        }
    }
    #[test]
    fn terrain_edit_perf_atlas_floods_previously_dry_land_and_drains_to_empty() {
        let mut result = field(129);
        let mut config = TerrainConfig::default();
        config.ocean_enabled = true;
        config.plains.water_level = 10.0;
        if let TerrainData::Heightfield(f) = &mut result.data {
            f.heights.fill(20.0);
        }
        let mut atlas = OceanContactAtlas::new(&result);
        atlas.rebuild(&result, &config);
        for height in [2.0, 20.0] {
            if let TerrainData::Heightfield(f) = &mut result.data {
                for z in 70..75 {
                    for x in 70..75 {
                        f.heights[z * 129 + x] = height;
                    }
                }
            }
            atlas.update_chunk(&result, &config, [1, 1]);
            let mut reference = OceanContactAtlas::new(&result);
            reference.rebuild(&result, &config);
            assert_eq!(atlas.mask, reference.mask);
            assert_eq!(atlas.pixels, reference.pixels);
        }
    }
    #[test]
    fn terrain_edit_perf_voxel_shore_patch_matches_full_rebuild() {
        let mut world = World::new();
        world.import_authored_heightfield(field(257)).unwrap();
        let mut config = world.terrain.config.clone();
        config.ocean_enabled = true;
        config.plains.water_level = 10.0;
        let mut result = world.terrain.effective().unwrap().clone();
        let mut atlas = OceanContactAtlas::new(&result);
        atlas.rebuild(&result, &config);
        for (tool, target) in [
            (crate::world::terrain::SculptTool::Raise, Some(18.0)),
            (crate::world::terrain::SculptTool::Lower, Some(2.0)),
        ] {
            let request = crate::world::terrain::TerrainSculptRequest {
                tool,
                center: [28.0, 10.0, -136.0],
                radius: 7.0,
                strength: 12.0,
                flatten_target: target,
            };
            assert!(result.apply_sculpt(&request));
            for z in 0..=1 {
                for x in 1..=2 {
                    atlas.update_chunk(&result, &config, [x, z]);
                }
            }
            let mut reference = OceanContactAtlas::new(&result);
            reference.rebuild(&result, &config);
            assert_eq!(atlas.mask, reference.mask, "voxel {tool:?}");
            assert_eq!(atlas.pixels, reference.pixels, "voxel distance {tool:?}");
        }
    }
}
