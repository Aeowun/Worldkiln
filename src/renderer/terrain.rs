// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use glam::{Mat4, Vec3};

use crate::world::terrain::{
    TERRAIN_FALLBACK_ROCK_MATERIAL, TERRAIN_FALLBACK_SUBSURFACE_MATERIAL,
    TERRAIN_FALLBACK_SURFACE_MATERIAL, terrain_render_chunk_samples,
};
use crate::world::{
    CHUNK_SIZE, HeightfieldData, SMOOTH_VOXEL_ISO_LEVEL, SmoothVoxelGrid, TerrainConfig,
    TerrainData, TerrainGrassSettings, TerrainMaterials, TerrainRenderChunkKey, TerrainResult,
    World,
};

use super::Renderer;
#[path = "terrain_contact_atlas.rs"]
mod contact_atlas;
use contact_atlas::{AtlasUpload, OceanContactAtlas};

pub(super) const ADVANCED_SHORELINE_ENABLED: bool = false;
const WET_SAND_MATERIAL_ENABLED: bool = false;
fn geometry_wave_parameters(config: &TerrainConfig) -> (f32, f32) {
    if ADVANCED_SHORELINE_ENABLED {
        (config.water.wave_height, config.water.wave_speed)
    } else {
        (0.0, 0.0)
    }
}
use super::chunk::CameraFrustum;
use super::mesh::{add_block_quad, upload_block_vertices_3d};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainMaterialSlot {
    Surface,
    Subsurface,
    Rock,
    Water,
    Grass,
    Palette(u16),
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TerrainDrawStats {
    pub draws: u64,
    pub triangles: u64,
    pub chunks_drawn: u64,
    pub chunks_culled: u64,
    pub texture_lookup_ns: u64,
    pub draw_submission_ns: u64,
    pub cache_sync_ns: u64,
    pub state_setup_ns: u64,
}

#[derive(Clone, Debug, Default)]
pub struct TerrainMesh {
    pub batches: Vec<TerrainMeshBatch>,
    pub source_signature: u64,
}

#[derive(Clone, Debug)]
pub struct TerrainMeshBatch {
    pub material: TerrainMaterialSlot,
    pub vertices: Vec<f32>,
    pub water_body_id: Option<u64>,
}

pub(super) struct TerrainGpuMesh {
    revision: u64,
    config_key: u64,
    source_region: ([i32; 3], [i32; 3]),
    chunks: BTreeMap<TerrainRenderChunkKey, TerrainGpuChunk>,
    horizon: Option<TerrainGpuChunk>,
    pending: std::collections::VecDeque<(TerrainRenderChunkKey, TerrainMesh)>,
    pending_keys: std::collections::VecDeque<TerrainRenderChunkKey>,
    pub(super) ocean_contact_texture: u32,
    pub(super) ocean_contact_mapping: [f32; 4],
    ocean_contact_ready: bool,
    ocean_contact_cpu: Option<OceanContactAtlas>,
    ocean_contact_dirty: std::collections::VecDeque<TerrainRenderChunkKey>,
}

struct TerrainGpuChunk {
    batches: Vec<TerrainGpuBatch>,
}

struct TerrainGpuBatch {
    material: TerrainMaterialSlot,
    water_body_id: Option<u64>,
    vao: u32,
    vbo: u32,
    vertex_count: i32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct UnderwaterState {
    pub enabled: bool,
    pub color: Vec3,
    pub density: f32,
    pub depth_fog: f32,
    pub depth_fog_limit: f32,
    pub fog_limit: f32,
    pub sky_tint: f32,
    pub surface_y: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct UnderwaterVolume {
    min: Vec3,
    max: Vec3,
}

impl UnderwaterVolume {
    fn contains(self, point: Vec3) -> bool {
        point.x >= self.min.x
            && point.x < self.max.x
            && point.y >= self.min.y
            && point.y < self.max.y
            && point.z >= self.min.z
            && point.z < self.max.z
    }
}

fn underwater_volume_for_result(
    result: &TerrainResult,
    water_level: f32,
) -> Option<UnderwaterVolume> {
    if !water_level.is_finite() {
        return None;
    }

    let min = Vec3::new(
        result.request.position[0] as f32,
        result.request.position[1] as f32,
        result.request.position[2] as f32,
    );
    let request_max = min
        + Vec3::new(
            result.request.size[0] as f32,
            result.request.size[1] as f32,
            result.request.size[2] as f32,
        );
    if water_level <= min.y {
        return None;
    }

    Some(UnderwaterVolume {
        min,
        max: Vec3::new(request_max.x, water_level, request_max.z),
    })
}

#[cfg(test)]
pub(super) fn underwater_state(world: &World, camera_position: Vec3) -> Option<UnderwaterState> {
    underwater_state_for_settings(world, camera_position, world.terrain.config.water)
}
pub(super) fn underwater_state_with_material(
    world: &World,
    camera_position: Vec3,
    catalog: &crate::project::material::MaterialCatalog,
) -> Option<UnderwaterState> {
    underwater_state_for_settings(
        world,
        camera_position,
        world.terrain.config.resolved_water(catalog),
    )
}
fn underwater_state_for_settings(
    world: &World,
    camera_position: Vec3,
    post: crate::world::terrain::TerrainWaterSettings,
) -> Option<UnderwaterState> {
    if !world.terrain.config.ocean_enabled {
        return None;
    }
    let result = world.terrain.effective()?;
    let water_level = world.terrain.config.plains.water_level;
    let volume = underwater_volume_for_result(result, water_level)?;
    let _ = camera_position;

    Some(UnderwaterState {
        enabled: post.underwater_effects_enabled,
        color: Vec3::from_array(post.underwater_fog_color),
        density: post.underwater_fog_density,
        depth_fog: post.underwater_depth_fog,
        depth_fog_limit: post.underwater_depth_fog_limit,
        fog_limit: post.underwater_fog_limit,
        sky_tint: post.underwater_sky_tint,
        surface_y: volume.max.y,
    })
}

#[cfg(test)]
fn underwater_ray_distance(camera_y: f32, fragment_y: f32, surface_y: f32, distance: f32) -> f32 {
    if !camera_y.is_finite()
        || !fragment_y.is_finite()
        || !surface_y.is_finite()
        || !distance.is_finite()
        || distance <= 0.0
    {
        return 0.0;
    }
    if camera_y >= surface_y && fragment_y >= surface_y {
        return 0.0;
    }
    if camera_y < surface_y && fragment_y < surface_y {
        return distance;
    }
    let denominator = fragment_y - camera_y;
    if denominator.abs() < 0.00001 {
        return 0.0;
    }
    let crossing = ((surface_y - camera_y) / denominator).clamp(0.0, 1.0);
    if camera_y < surface_y {
        distance * crossing
    } else {
        distance * (1.0 - crossing)
    }
}
pub fn build_terrain_mesh(
    result: &TerrainResult,
    water_level: f32,
    slope_shading_strength: f32,
) -> TerrainMesh {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("terrain", "remesh", false);
    let mut mesh = match &result.data {
        TerrainData::Heightfield(heightfield) => {
            let mut mesh = build_smooth(heightfield);
            let water = build_water(heightfield, water_level);
            if !water.is_empty() {
                mesh.batches.push(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Water,
                    vertices: water,
                    water_body_id: None,
                });
            }
            mesh
        }
        TerrainData::Voxel(grid) => {
            let mut mesh = build_voxel_smooth(grid, slope_shading_strength);
            let water = build_voxel_water(grid, water_level);
            if !water.is_empty() {
                mesh.batches.push(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Water,
                    vertices: water,
                    water_body_id: None,
                });
            }
            mesh
        }
    };
    mesh.source_signature = logical_signature(result);
    mesh
}

fn build_smooth(heightfield: &HeightfieldData) -> TerrainMesh {
    let mut surface = Vec::new();
    if heightfield.size[0] >= 2 && heightfield.size[2] >= 2 {
        for z in 0..heightfield.size[2] - 1 {
            for x in 0..heightfield.size[0] - 1 {
                let p00 = point(heightfield, x, z);
                let p10 = point(heightfield, x + 1, z);
                let p11 = point(heightfield, x + 1, z + 1);
                let p01 = point(heightfield, x, z + 1);
                push_triangle(&mut surface, p00, p11, p10);
                push_triangle(&mut surface, p00, p01, p11);
            }
        }
    }
    TerrainMesh {
        batches: vec![TerrainMeshBatch {
            material: TerrainMaterialSlot::Surface,
            vertices: surface,
            water_body_id: None,
        }],
        source_signature: 0,
    }
}

#[derive(Clone, Copy)]
struct DensitySample {
    position: Vec3,
    density: f32,
    material: Option<u16>,
}

fn build_voxel_smooth(grid: &SmoothVoxelGrid, slope_shading_strength: f32) -> TerrainMesh {
    build_voxel_smooth_region(
        grid,
        slope_shading_strength,
        0,
        grid.size[0],
        0,
        grid.size[2],
    )
}

fn build_voxel_smooth_region(
    grid: &SmoothVoxelGrid,
    slope_shading_strength: f32,
    x_start: u32,
    x_end: u32,
    z_start: u32,
    z_end: u32,
) -> TerrainMesh {
    let mut batches = vec![Vec::new(); grid.materials.len()];
    const CUBE_CORNERS: [[i32; 3]; 8] = [
        [0, 0, 0],
        [1, 0, 0],
        [1, 1, 0],
        [0, 1, 0],
        [0, 0, 1],
        [1, 0, 1],
        [1, 1, 1],
        [0, 1, 1],
    ];
    const TETRAHEDRA: [[usize; 4]; 6] = [
        [0, 5, 1, 6],
        [0, 1, 2, 6],
        [0, 2, 3, 6],
        [0, 3, 7, 6],
        [0, 7, 4, 6],
        [0, 4, 5, 6],
    ];

    let base_x_start = if x_start == 0 { -1 } else { x_start as i32 };
    let base_z_start = if z_start == 0 { -1 } else { z_start as i32 };
    for y in -1..grid.size[1] as i32 {
        for z in base_z_start..z_end as i32 {
            for x in base_x_start..x_end as i32 {
                let cube = CUBE_CORNERS.map(|offset| {
                    density_sample(grid, x + offset[0], y + offset[1], z + offset[2])
                });
                for tetrahedron in TETRAHEDRA {
                    let samples = tetrahedron.map(|index| cube[index]);
                    polygonize_tetrahedron_with_grid(
                        samples,
                        &mut batches,
                        slope_shading_strength,
                        Some(grid),
                    );
                }
            }
        }
    }

    TerrainMesh {
        batches: batches
            .into_iter()
            .enumerate()
            .filter_map(|(material, vertices)| {
                (!vertices.is_empty()).then_some(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Palette(material as u16),
                    vertices,
                    water_body_id: None,
                })
            })
            .collect(),
        source_signature: 0,
    }
}

fn density_sample(grid: &SmoothVoxelGrid, x: i32, y: i32, z: i32) -> DensitySample {
    let position = Vec3::new(
        grid.origin[0] as f32 + (x as f32 + 0.5) * grid.spacing_studs,
        grid.origin[1] as f32 + (y as f32 + 0.5) * grid.spacing_studs,
        grid.origin[2] as f32 + (z as f32 + 0.5) * grid.spacing_studs,
    );
    if x < 0 || y < 0 || z < 0 {
        return DensitySample {
            position,
            density: 0.0,
            material: None,
        };
    }
    let voxel = grid.voxel(x as u32, y as u32, z as u32);
    DensitySample {
        position,
        density: voxel.map(|value| value.occupancy_unit()).unwrap_or(0.0),
        material: voxel
            .filter(|value| !value.is_empty())
            .map(|value| value.material),
    }
}

#[cfg(test)]
fn polygonize_tetrahedron(
    samples: [DensitySample; 4],
    batches: &mut [Vec<f32>],
    slope_shading_strength: f32,
) {
    polygonize_tetrahedron_with_grid(samples, batches, slope_shading_strength, None);
}

fn polygonize_tetrahedron_with_grid(
    samples: [DensitySample; 4],
    batches: &mut [Vec<f32>],
    slope_shading_strength: f32,
    grid: Option<&SmoothVoxelGrid>,
) {
    const EDGES: [(usize, usize); 6] = [(0, 1), (1, 2), (2, 0), (0, 3), (1, 3), (2, 3)];
    let inside = samples.map(|sample| sample.density >= SMOOTH_VOXEL_ISO_LEVEL);
    let inside_count = inside.iter().filter(|&&value| value).count();
    if inside_count == 0 || inside_count == 4 {
        return;
    }

    let mut intersections = Vec::with_capacity(4);
    for (a, b) in EDGES {
        if inside[a] == inside[b] {
            continue;
        }
        let denominator = samples[b].density - samples[a].density;
        let t = if denominator.abs() <= f32::EPSILON {
            0.5
        } else {
            ((SMOOTH_VOXEL_ISO_LEVEL - samples[a].density) / denominator).clamp(0.0, 1.0)
        };
        let point = samples[a].position.lerp(samples[b].position, t);
        if !intersections
            .iter()
            .any(|existing: &Vec3| existing.distance_squared(point) < 1e-8)
        {
            intersections.push(point);
        }
    }
    if intersections.len() < 3 {
        return;
    }

    let material = samples
        .iter()
        .zip(inside)
        .filter(|(_, is_inside)| *is_inside)
        .filter_map(|(sample, _)| sample.material.map(|material| (sample.density, material)))
        .min_by(|left, right| left.0.total_cmp(&right.0))
        .map(|(_, material)| material)
        .unwrap_or(0);
    let Some(vertices) = batches.get_mut(material as usize) else {
        return;
    };

    let inside_center = average_sample_position(&samples, &inside, true);
    let outside_center = average_sample_position(&samples, &inside, false);
    let mut outward = (outside_center - inside_center).normalize_or_zero();
    if outward.length_squared() <= f32::EPSILON {
        outward = Vec3::Y;
    }

    let centroid = intersections
        .iter()
        .copied()
        .fold(Vec3::ZERO, |sum, point| sum + point)
        / intersections.len() as f32;
    let mut axis_u = if outward.x.abs() < 0.9 {
        outward.cross(Vec3::X)
    } else {
        outward.cross(Vec3::Y)
    }
    .normalize_or_zero();
    if axis_u.length_squared() <= f32::EPSILON {
        axis_u = Vec3::Z;
    }
    let axis_v = outward.cross(axis_u).normalize_or_zero();
    intersections.sort_by(|a, b| {
        let av = *a - centroid;
        let bv = *b - centroid;
        av.dot(axis_v)
            .atan2(av.dot(axis_u))
            .total_cmp(&bv.dot(axis_v).atan2(bv.dot(axis_u)))
    });

    for index in 1..intersections.len() - 1 {
        let a = intersections[0];
        let mut b = intersections[index];
        let mut d = intersections[index + 1];
        if (b - a).cross(d - a).dot(outward) < 0.0 {
            std::mem::swap(&mut b, &mut d);
        }
        if let Some(grid) = grid {
            push_blended_terrain_triangle(vertices, grid, a, b, d, material);
        } else {
            push_terrain_triangle(vertices, a, b, d, slope_shading_strength);
        }
    }
}

fn average_sample_position(
    samples: &[DensitySample; 4],
    inside: &[bool; 4],
    target_inside: bool,
) -> Vec3 {
    let mut total = Vec3::ZERO;
    let mut count = 0.0;
    for (sample, is_inside) in samples.iter().zip(inside) {
        if *is_inside == target_inside {
            total += sample.position;
            count += 1.0;
        }
    }
    if count > 0.0 {
        total / count
    } else {
        Vec3::ZERO
    }
}

fn build_voxel_water(grid: &SmoothVoxelGrid, water_level: f32) -> Vec<f32> {
    build_voxel_water_region(grid, water_level, 0, grid.size[0], 0, grid.size[2])
}

#[derive(Clone, Copy, Debug)]
struct WaterCell {
    depth: f32,
    class: u8,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct WaterPatch {
    x0: u32,
    x1: u32,
    z0: u32,
    z1: u32,
    average_depth: f32,
}

fn water_merge_class(depth: f32) -> u8 {
    let depth = depth.max(0.0);
    if depth < 2.25 {
        ((depth / 0.5).floor() as u8).min(4)
    } else {
        5
    }
}

fn greedy_water_patches(cells: &[Option<WaterCell>], width: u32, depth: u32) -> Vec<WaterPatch> {
    if width == 0 || depth == 0 || cells.len() != width as usize * depth as usize {
        return Vec::new();
    }

    let mut consumed = vec![false; cells.len()];
    let mut patches = Vec::new();

    for z in 0..depth {
        for x in 0..width {
            let index = z as usize * width as usize + x as usize;
            if consumed[index] {
                continue;
            }
            let Some(seed) = cells[index] else {
                continue;
            };

            let mut patch_width = 1_u32;
            while x + patch_width < width {
                let next = z as usize * width as usize + (x + patch_width) as usize;
                if consumed[next] || cells[next].is_none_or(|cell| cell.class != seed.class) {
                    break;
                }
                patch_width += 1;
            }

            let mut patch_depth = 1_u32;
            'grow_depth: while z + patch_depth < depth {
                let row = z + patch_depth;
                for px in x..x + patch_width {
                    let next = row as usize * width as usize + px as usize;
                    if consumed[next] || cells[next].is_none_or(|cell| cell.class != seed.class) {
                        break 'grow_depth;
                    }
                }
                patch_depth += 1;
            }

            let mut depth_sum = 0.0_f32;
            let mut sample_count = 0_u32;
            for pz in z..z + patch_depth {
                for px in x..x + patch_width {
                    let patch_index = pz as usize * width as usize + px as usize;
                    let cell = cells[patch_index]
                        .expect("greedy water patch contains only classified water cells");
                    consumed[patch_index] = true;
                    depth_sum += cell.depth;
                    sample_count += 1;
                }
            }

            patches.push(WaterPatch {
                x0: x,
                x1: x + patch_width,
                z0: z,
                z1: z + patch_depth,
                average_depth: depth_sum / sample_count as f32,
            });
        }
    }

    patches
}

fn build_voxel_water_region(
    grid: &SmoothVoxelGrid,
    water_level: f32,
    x_start: u32,
    x_end: u32,
    z_start: u32,
    z_end: u32,
) -> Vec<f32> {
    build_voxel_water_region_with_runup(grid, water_level, 0.0, 0.0, x_start, x_end, z_start, z_end)
}

fn squared_distance_transform_1d(values: &[f32]) -> Vec<f32> {
    let n = values.len();
    if n == 0 {
        return Vec::new();
    }
    let mut sites = vec![0usize; n];
    let mut breaks = vec![0.0f32; n + 1];
    let mut output = vec![0.0f32; n];
    breaks[0] = f32::NEG_INFINITY;
    breaks[1] = f32::INFINITY;
    let mut k = 0usize;
    for q in 1..n {
        let intersect = |site: usize| -> f32 {
            let a = q as f32;
            let b = site as f32;
            ((values[q] + a * a) - (values[site] + b * b)) / (2.0 * (a - b))
        };
        let mut crossing = intersect(sites[k]);
        while crossing <= breaks[k] && k > 0 {
            k -= 1;
            crossing = intersect(sites[k]);
        }
        k += 1;
        sites[k] = q;
        breaks[k] = crossing;
        breaks[k + 1] = f32::INFINITY;
    }
    k = 0;
    for q in 0..n {
        while breaks[k + 1] < q as f32 {
            k += 1;
        }
        let dx = q as f32 - sites[k] as f32;
        output[q] = dx * dx + values[sites[k]];
    }
    output
}
fn ocean_nearest_squared_distance(mask: &[bool], width: usize, height: usize) -> Vec<f32> {
    if width == 0 || height == 0 {
        return Vec::new();
    }
    assert_eq!(mask.len(), width * height);
    let mut horizontal = vec![0.0f32; mask.len()];
    for z in 0..height {
        let source = (0..width)
            .map(|x| if mask[z * width + x] { 0.0 } else { 1.0e8 })
            .collect::<Vec<_>>();
        horizontal[z * width..(z + 1) * width]
            .copy_from_slice(&squared_distance_transform_1d(&source));
    }
    let mut output = vec![0.0f32; mask.len()];
    for x in 0..width {
        let column = (0..height)
            .map(|z| horizontal[z * width + x])
            .collect::<Vec<_>>();
        let distances = squared_distance_transform_1d(&column);
        for z in 0..height {
            output[z * width + x] = distances[z];
        }
    }
    output
}

fn build_voxel_water_region_with_runup(
    grid: &SmoothVoxelGrid,
    water_level: f32,
    wave_height: f32,
    wave_speed: f32,
    x_start: u32,
    x_end: u32,
    z_start: u32,
    z_end: u32,
) -> Vec<f32> {
    let mut vertices = Vec::new();
    if !water_level.is_finite() || x_start >= x_end || z_start >= z_end {
        return vertices;
    }
    let (min, _) = grid.world_bounds();
    let runup_surface = water_level + wave_height.max(0.0);
    const PROXIMITY_HALO: u32 = 17;
    let proxy_x0 = x_start.saturating_sub(PROXIMITY_HALO);
    let proxy_z0 = z_start.saturating_sub(PROXIMITY_HALO);
    let proxy_x1 = x_end.saturating_add(PROXIMITY_HALO).min(grid.size[0]);
    let proxy_z1 = z_end.saturating_add(PROXIMITY_HALO).min(grid.size[2]);
    let proxy_width = (proxy_x1 - proxy_x0) as usize;
    let proxy_height = (proxy_z1 - proxy_z0) as usize;
    let proximity = if wave_height > 0.0 {
        let mut sea = vec![false; proxy_width * proxy_height];
        for z in proxy_z0..proxy_z1 {
            for x in proxy_x0..proxy_x1 {
                let cx = min[0] + (x as f32 + 0.5) * grid.spacing_studs;
                let cz = min[2] + (z as f32 + 0.5) * grid.spacing_studs;
                sea[(z - proxy_z0) as usize * proxy_width + (x - proxy_x0) as usize] = grid
                    .surface_height_at(cx, cz)
                    .is_some_and(|h| h < water_level);
            }
        }
        ocean_nearest_squared_distance(&sea, proxy_width, proxy_height)
    } else {
        Vec::new()
    };

    let width = x_end - x_start;
    let depth = z_end - z_start;
    let mut wet_cells = vec![false; width as usize * depth as usize];
    let mut water_cell_count = 0_usize;

    let halo_width = width as usize + 2;
    let mut halo_signed = vec![0.0_f32; halo_width * (depth as usize + 2)];
    for hz in 0..depth + 2 {
        let z = (z_start as i64 + hz as i64 - 1).clamp(0, grid.size[2] as i64 - 1) as u32;
        for hx in 0..width + 2 {
            let x = (x_start as i64 + hx as i64 - 1).clamp(0, grid.size[0] as i64 - 1) as u32;
            let center_x = min[0] + (x as f32 + 0.5) * grid.spacing_studs;
            let center_z = min[2] + (z as f32 + 0.5) * grid.spacing_studs;
            let surface = grid.surface_height_at(center_x, center_z).unwrap_or(min[1]);
            let inland = surface >= water_level;
            let mut connected = !inland;
            if inland && surface < runup_surface {
                let reach = wave_height.max(0.0) * (1.6 + 0.45 * wave_speed.abs().sqrt());
                let radius = (reach / grid.spacing_studs.max(0.1))
                    .ceil()
                    .clamp(1.0, 16.0) as i32;
                let px = (x - proxy_x0) as usize;
                let pz = (z - proxy_z0) as usize;
                connected = proximity[pz * proxy_width + px] <= (radius * radius) as f32;
            }
            let candidate_depth = if connected {
                runup_surface - surface
            } else {
                -1.0
            };
            halo_signed[hz as usize * halo_width + hx as usize] = candidate_depth;

            if hz > 0 && hz <= depth && hx > 0 && hx <= width && candidate_depth > 0.0 {
                let cell_index = (hz - 1) as usize * width as usize + (hx - 1) as usize;
                wet_cells[cell_index] = true;
                water_cell_count += 1;
            }
        }
    }

    if water_cell_count == 0 {
        return vertices;
    }

    let corner_depth = |cx: u32, cz: u32| -> f32 {
        let i = cz as usize * halo_width + cx as usize;
        (halo_signed[i]
            + halo_signed[i + 1]
            + halo_signed[i + halo_width]
            + halo_signed[i + halo_width + 1])
            * 0.25
    };

    const TILE: u32 = 2;
    let tiles_x = width.div_ceil(TILE);
    let tiles_z = depth.div_ceil(TILE);
    let mut tile_is_full = vec![false; (tiles_x * tiles_z) as usize];
    let full_at = |tx: u32, tz: u32, classification: &[bool]| -> bool {
        classification[(tz * tiles_x + tx) as usize]
    };
    for tz in 0..tiles_z {
        for tx in 0..tiles_x {
            let x0 = tx * TILE;
            let z0 = tz * TILE;
            let x1 = (x0 + TILE).min(width);
            let z1 = (z0 + TILE).min(depth);
            let mut full = true;
            for z in z0..z1 {
                for x in x0..x1 {
                    let wet = wet_cells[(z * width + x) as usize];
                    if !wet
                        || [
                            corner_depth(x, z),
                            corner_depth(x, z + 1),
                            corner_depth(x + 1, z + 1),
                            corner_depth(x + 1, z),
                        ]
                        .iter()
                        .any(|value| *value <= 0.0)
                    {
                        full = false;
                    }
                }
            }
            tile_is_full[(tz * tiles_x + tx) as usize] = full;
        }
    }

    for tz in 0..tiles_z {
        for tx in 0..tiles_x {
            let x0 = tx * TILE;
            let z0 = tz * TILE;
            let x1 = (x0 + TILE).min(width);
            let z1 = (z0 + TILE).min(depth);
            if full_at(tx, tz, &tile_is_full) {
                let points = [
                    [
                        min[0] + (x_start + x0) as f32 * grid.spacing_studs,
                        water_level,
                        min[2] + (z_start + z0) as f32 * grid.spacing_studs,
                    ],
                    [
                        min[0] + (x_start + x0) as f32 * grid.spacing_studs,
                        water_level,
                        min[2] + (z_start + z1) as f32 * grid.spacing_studs,
                    ],
                    [
                        min[0] + (x_start + x1) as f32 * grid.spacing_studs,
                        water_level,
                        min[2] + (z_start + z1) as f32 * grid.spacing_studs,
                    ],
                    [
                        min[0] + (x_start + x1) as f32 * grid.spacing_studs,
                        water_level,
                        min[2] + (z_start + z0) as f32 * grid.spacing_studs,
                    ],
                ];
                let corners = [
                    corner_depth(x0, z0),
                    corner_depth(x0, z1),
                    corner_depth(x1, z1),
                    corner_depth(x1, z0),
                ];
                let west = z1 - z0 == 2 && (tx == 0 || !full_at(tx - 1, tz, &tile_is_full));
                let east =
                    z1 - z0 == 2 && (tx + 1 == tiles_x || !full_at(tx + 1, tz, &tile_is_full));
                let north = x1 - x0 == 2 && (tz == 0 || !full_at(tx, tz - 1, &tile_is_full));
                let south =
                    x1 - x0 == 2 && (tz + 1 == tiles_z || !full_at(tx, tz + 1, &tile_is_full));
                if !(west || east || north || south) {
                    push_water_gradient_quad(&mut vertices, points, corners);
                } else {
                    let mut boundary = Vec::with_capacity(8);
                    for edge in 0..4 {
                        boundary.push((points[edge], corners[edge]));
                        let split = [west, south, east, north][edge];
                        if split {
                            let next = (edge + 1) % 4;
                            let p = std::array::from_fn(|axis| {
                                (points[edge][axis] + points[next][axis]) * 0.5
                            });
                            let mid_depth = match edge {
                                0 => corner_depth(x0, z0 + 1),
                                1 => corner_depth(x0 + 1, z1),
                                2 => corner_depth(x1, z0 + 1),
                                _ => corner_depth(x0 + 1, z0),
                            };
                            boundary.push((p, mid_depth));
                        }
                    }
                    let center = (
                        [
                            (points[0][0] + points[2][0]) * 0.5,
                            water_level,
                            (points[0][2] + points[2][2]) * 0.5,
                        ],
                        corner_depth(x0 + (x1 - x0) / 2, z0 + (z1 - z0) / 2),
                    );
                    for i in 0..boundary.len() {
                        let b = boundary[i];
                        let c = boundary[(i + 1) % boundary.len()];
                        push_water_gradient_triangle(&mut vertices, [center, b, c]);
                    }
                }
            } else {
                for z in z0..z1 {
                    for x in x0..x1 {
                        let depths = [
                            corner_depth(x, z),
                            corner_depth(x, z + 1),
                            corner_depth(x + 1, z + 1),
                            corner_depth(x + 1, z),
                        ];
                        if !depths.iter().any(|d| *d > 0.0) {
                            continue;
                        }
                        let wx = min[0] + (x_start + x) as f32 * grid.spacing_studs;
                        let wz = min[2] + (z_start + z) as f32 * grid.spacing_studs;
                        emit_shoreline_clipped_cell(
                            &mut vertices,
                            [
                                [wx, water_level, wz],
                                [wx, water_level, wz + grid.spacing_studs],
                                [
                                    wx + grid.spacing_studs,
                                    water_level,
                                    wz + grid.spacing_studs,
                                ],
                                [wx + grid.spacing_studs, water_level, wz],
                            ],
                            depths,
                        );
                    }
                }
            }
        }
    }

    let reach = (wave_height.max(0.0) * 0.62).min(5.0);
    if reach > 0.01 {
        for z in 0..depth {
            for x in 0..width {
                let signed = halo_signed[(z as usize + 1) * halo_width + x as usize + 1];
                if signed <= 0.0 {
                    continue;
                }
                let center_elevation = runup_surface - signed - water_level;
                if center_elevation < -0.20 || center_elevation > reach {
                    continue;
                }
                let wx = min[0] + (x_start + x) as f32 * grid.spacing_studs;
                let wz = min[2] + (z_start + z) as f32 * grid.spacing_studs;
                let h = |px: f32, pz: f32| grid.surface_height_at(px, pz);
                let Some((h0, h1, h2, h3)) = h(wx, wz)
                    .zip(h(wx, wz + grid.spacing_studs))
                    .zip(h(wx + grid.spacing_studs, wz + grid.spacing_studs))
                    .zip(h(wx + grid.spacing_studs, wz))
                    .map(|(((a, b), c), d)| (a, b, c, d))
                else {
                    continue;
                };
                push_shore_swash_quad(
                    &mut vertices,
                    [
                        [wx, h0, wz],
                        [wx, h1, wz + grid.spacing_studs],
                        [wx + grid.spacing_studs, h2, wz + grid.spacing_studs],
                        [wx + grid.spacing_studs, h3, wz],
                    ],
                    water_level,
                    reach,
                    grid.spacing_studs,
                );
            }
        }
    }
    vertices
}

fn emit_shoreline_clipped_cell(output: &mut Vec<f32>, points: [[f32; 3]; 4], depths: [f32; 4]) {
    for corners in [[0, 1, 2], [0, 2, 3]] {
        let mut polygon: Vec<([f32; 3], f32)> = Vec::with_capacity(4);
        for edge in 0..3 {
            let i = corners[edge];
            let j = corners[(edge + 1) % 3];
            let a = depths[i];
            let b = depths[j];
            if a > 0.0 {
                polygon.push((points[i], a));
            }
            if (a > 0.0) != (b > 0.0) {
                let t = (a / (a - b)).clamp(0.0, 1.0);
                polygon.push((
                    std::array::from_fn(|axis| {
                        points[i][axis] + (points[j][axis] - points[i][axis]) * t
                    }),
                    0.0,
                ));
            }
        }
        polygon.dedup_by(|a, b| {
            (a.0[0] - b.0[0]).abs() <= 0.000001 && (a.0[2] - b.0[2]).abs() <= 0.000001
        });
        if polygon.len() > 1 {
            let first = polygon[0].0;
            let last = polygon[polygon.len() - 1].0;
            if (first[0] - last[0]).abs() <= 0.000001 && (first[2] - last[2]).abs() <= 0.000001 {
                polygon.pop();
            }
        }
        if polygon.len() < 3 {
            continue;
        }
        for i in 1..polygon.len() - 1 {
            let triangle = [polygon[0], polygon[i], polygon[i + 1]];
            let a = triangle[0].0;
            let b = triangle[1].0;
            let c = triangle[2].0;
            let signed_area_twice = (b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0]);
            if signed_area_twice.abs() <= 0.000001 {
                continue;
            }
            for (position, depth) in triangle {
                let rgba = water_spatial_attributes(depth);
                output.extend_from_slice(&[
                    position[0],
                    position[1],
                    position[2],
                    0.0,
                    1.0,
                    0.0,
                    rgba[0],
                    rgba[1],
                    rgba[2],
                    rgba[3],
                    position[0],
                    position[2],
                ]);
            }
        }
    }
}

fn push_shore_swash_quad(
    output: &mut Vec<f32>,
    points: [[f32; 3]; 4],
    level: f32,
    reach: f32,
    spacing: f32,
) {
    if reach <= 0.001 {
        return;
    }
    let elevations = points.map(|p| p[1] - level);
    let min = elevations.iter().copied().fold(f32::INFINITY, f32::min);
    let max = elevations.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    if min > reach + 0.1 || max < -0.35 || max - min > spacing * 0.85 {
        return;
    }

    for corners in [[0usize, 1, 2], [0, 2, 3]] {
        let mut boundary: Vec<([f32; 3], f32)> = Vec::with_capacity(4);
        for edge in 0..3 {
            let i = corners[edge];
            let j = corners[(edge + 1) % 3];
            let a = elevations[i];
            let b = elevations[j];
            if a >= 0.0 {
                boundary.push((points[i], a));
            }
            if (a >= 0.0) != (b >= 0.0) {
                let t = (a / (a - b)).clamp(0.0, 1.0);
                let at = std::array::from_fn(|axis| {
                    points[i][axis] + (points[j][axis] - points[i][axis]) * t
                });
                boundary.push((at, 0.0));
            }
        }
        boundary.dedup_by(|a, b| {
            (a.0[0] - b.0[0]).abs() < 0.000001 && (a.0[2] - b.0[2]).abs() < 0.000001
        });
        if boundary.len() > 1 {
            let first = boundary[0].0;
            let last = boundary[boundary.len() - 1].0;
            if (first[0] - last[0]).abs() < 0.000001 && (first[2] - last[2]).abs() < 0.000001 {
                boundary.pop();
            }
        }
        if boundary.len() < 3 {
            continue;
        }
        for i in 1..boundary.len() - 1 {
            let tri = [boundary[0], boundary[i], boundary[i + 1]];
            let a = tri[0].0;
            let b = tri[1].0;
            let c = tri[2].0;
            let area = (b[0] - a[0]) * (c[2] - a[2]) - (b[2] - a[2]) * (c[0] - a[0]);
            if area.abs() <= 0.000001 {
                continue;
            }
            for (pt, elevation) in tri {
                output.extend_from_slice(&[
                    pt[0],
                    pt[1] + 0.045,
                    pt[2],
                    0.0,
                    1.0,
                    0.0,
                    0.85,
                    0.0,
                    1.0,
                    2.0,
                    elevation.max(0.0),
                    0.0,
                ]);
            }
        }
    }
}

fn push_water_gradient_quad(vertices: &mut Vec<f32>, points: [[f32; 3]; 4], depths: [f32; 4]) {
    let metadata = depths.map(water_spatial_attributes);
    for i in [0, 1, 2, 0, 2, 3] {
        let p = points[i];
        let c = metadata[i];
        vertices.extend_from_slice(&[
            p[0], p[1], p[2], 0.0, 1.0, 0.0, c[0], c[1], c[2], c[3], p[0], p[2],
        ]);
    }
}

fn push_water_gradient_triangle(output: &mut Vec<f32>, corners: [([f32; 3], f32); 3]) {
    for (position, depth) in corners {
        let c = water_spatial_attributes(depth);
        output.extend_from_slice(&[
            position[0],
            position[1],
            position[2],
            0.0,
            1.0,
            0.0,
            c[0],
            c[1],
            c[2],
            c[3],
            position[0],
            position[2],
        ]);
    }
}

const MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN: u32 = 8;
const MAX_PROCEDURAL_GRASS_CLUMPS: u32 = 65_536;

fn configured_surface_material_name(materials: &TerrainMaterials) -> &str {
    if materials.surface.trim().is_empty() {
        TERRAIN_FALLBACK_SURFACE_MATERIAL
    } else {
        materials.surface.as_str()
    }
}

pub(super) fn top_voxel_material(grid: &SmoothVoxelGrid, x: u32, z: u32) -> Option<u16> {
    for y in (0..grid.size[1]).rev() {
        let voxel = grid.voxel(x, y, z)?;
        if !voxel.is_empty() {
            return Some(voxel.material);
        }
    }
    None
}

fn grass_hash(seed: i32, world_x: i32, world_z: i32, slot: u32, salt: u64) -> u64 {
    let mut value = (seed as i64 as u64)
        ^ (world_x as i64 as u64).wrapping_mul(0x9E37_79B9_7F4A_7C15)
        ^ (world_z as i64 as u64).wrapping_mul(0xBF58_476D_1CE4_E5B9)
        ^ (slot as u64).wrapping_mul(0x94D0_49BB_1331_11EB)
        ^ salt;
    value ^= value >> 30;
    value = value.wrapping_mul(0xBF58_476D_1CE4_E5B9);
    value ^= value >> 27;
    value = value.wrapping_mul(0x94D0_49BB_1331_11EB);
    value ^ (value >> 31)
}

fn grass_random(seed: i32, world_x: i32, world_z: i32, slot: u32, salt: u64) -> f32 {
    let sample = (grass_hash(seed, world_x, world_z, slot, salt) >> 40) as u32;
    sample as f32 / ((1_u32 << 24) - 1) as f32
}

fn push_grass_clump(vertices: &mut Vec<f32>, center: Vec3, length: f32, angle: f32) {
    let half_width = (length * 0.09).clamp(0.04, 0.18);

    for plane_angle in [angle, angle + std::f32::consts::FRAC_PI_2] {
        let axis = Vec3::new(plane_angle.cos(), 0.0, plane_angle.sin());
        let normal = Vec3::new(-axis.z, 0.0, axis.x);
        let left = center - axis * half_width;
        let right = center + axis * half_width;
        let tip = center + Vec3::Y * length;

        for (point, uv) in [(left, [0.0, 0.0]), (right, [1.0, 0.0]), (tip, [0.5, 1.0])] {
            vertices.extend_from_slice(&[
                point.x, point.y, point.z, normal.x, normal.y, normal.z, 1.0, 1.0, 1.0, 1.0, uv[0],
                uv[1],
            ]);
        }
    }
}

fn build_voxel_grass(
    grid: &SmoothVoxelGrid,
    water_level: f32,
    settings: TerrainGrassSettings,
    materials: &TerrainMaterials,
    seed: i32,
) -> Vec<f32> {
    build_voxel_grass_region(
        grid,
        water_level,
        settings,
        materials,
        seed,
        0,
        grid.size[0],
        0,
        grid.size[2],
        MAX_PROCEDURAL_GRASS_CLUMPS,
    )
}

fn build_voxel_grass_region(
    grid: &SmoothVoxelGrid,
    water_level: f32,
    settings: TerrainGrassSettings,
    materials: &TerrainMaterials,
    seed: i32,
    x_start: u32,
    x_end: u32,
    z_start: u32,
    z_end: u32,
    chunk_clump_limit: u32,
) -> Vec<f32> {
    let mut vertices = Vec::new();
    if !settings.enabled
        || !settings.density.is_finite()
        || !settings.length_studs.is_finite()
        || settings.density <= 0.0
        || settings.length_studs <= 0.0
    {
        return vertices;
    }

    let length_studs = settings.length_studs.clamp(0.25, 4.0);
    let surface_material = configured_surface_material_name(materials);
    let (min, _) = grid.world_bounds();
    let column_count = (grid.size[0] as u64)
        .saturating_mul(grid.size[2] as u64)
        .max(1);
    let column_area_scale = (grid.spacing_studs * grid.spacing_studs) / 16.0;
    let requested_clumps = settings.density.clamp(0.0, 1.0)
        * MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN as f32
        * column_area_scale;
    let safety_limited_clumps = MAX_PROCEDURAL_GRASS_CLUMPS as f32 / column_count as f32;
    let target_clumps = requested_clumps.min(safety_limited_clumps);
    let whole_clumps = target_clumps.floor() as u32;
    let fractional_clump = target_clumps - whole_clumps as f32;
    let mut generated_clumps = 0_u32;

    for z in z_start..z_end {
        for x in x_start..x_end {
            let Some(material) = top_voxel_material(grid, x, z) else {
                continue;
            };
            if grid.material_name(material) != Some(surface_material)
                && grid.material_name(material) != Some("Surface")
            {
                continue;
            }

            let cell_world_x = grid.origin[0].wrapping_add(x as i32);
            let cell_world_z = grid.origin[2].wrapping_add(z as i32);
            let extra = if grass_random(seed, cell_world_x, cell_world_z, u32::MAX, 0xA11C_E551)
                < fractional_clump
            {
                1
            } else {
                0
            };
            let clump_count = (whole_clumps + extra).min(MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN);

            for slot in 0..clump_count {
                if generated_clumps >= chunk_clump_limit {
                    return vertices;
                }
                let jitter_x = grid.spacing_studs
                    * (0.0625 + grass_random(seed, cell_world_x, cell_world_z, slot, 0x1) * 0.875);
                let jitter_z = grid.spacing_studs
                    * (0.0625 + grass_random(seed, cell_world_x, cell_world_z, slot, 0x2) * 0.875);
                let world_x = min[0] + x as f32 * grid.spacing_studs + jitter_x;
                let world_z = min[2] + z as f32 * grid.spacing_studs + jitter_z;
                let Some(surface_y) = grid.surface_height_at(world_x, world_z) else {
                    continue;
                };
                if surface_y <= water_level + 0.05 {
                    continue;
                }

                let angle = grass_random(seed, cell_world_x, cell_world_z, slot, 0x3)
                    * std::f32::consts::TAU;
                let length_variation =
                    0.85 + grass_random(seed, cell_world_x, cell_world_z, slot, 0x4) * 0.30;
                push_grass_clump(
                    &mut vertices,
                    Vec3::new(world_x, surface_y + 0.02, world_z),
                    length_studs * length_variation,
                    angle,
                );
                generated_clumps += 1;
            }
        }
    }

    vertices
}

fn terrain_render_chunk_keys(result: &TerrainResult) -> Vec<TerrainRenderChunkKey> {
    match &result.data {
        TerrainData::Heightfield(field) => {
            let chunk = terrain_render_chunk_samples(1.0);
            let chunks_x = field.size[0].saturating_sub(1).div_ceil(chunk);
            let chunks_z = field.size[2].saturating_sub(1).div_ceil(chunk);
            (0..chunks_z)
                .flat_map(|z| (0..chunks_x).map(move |x| [x, z]))
                .collect()
        }
        TerrainData::Voxel(grid) => {
            let chunks_x = grid.size[0].div_ceil(terrain_render_chunk_samples(grid.spacing_studs));
            let chunks_z = grid.size[2].div_ceil(terrain_render_chunk_samples(grid.spacing_studs));
            let mut keys = Vec::with_capacity((chunks_x * chunks_z) as usize);
            for z in 0..chunks_z {
                for x in 0..chunks_x {
                    keys.push([x, z]);
                }
            }
            keys
        }
    }
}

fn heightfield_chunk_bounds(
    field: &HeightfieldData,
    key: TerrainRenderChunkKey,
) -> Option<(u32, u32, u32, u32)> {
    let chunk = terrain_render_chunk_samples(1.0);
    let x0 = key[0].checked_mul(chunk)?;
    let z0 = key[1].checked_mul(chunk)?;
    if x0 >= field.size[0].saturating_sub(1) || z0 >= field.size[2].saturating_sub(1) {
        return None;
    }
    let x1 = (x0 + chunk).min(field.size[0] - 1);
    let z1 = (z0 + chunk).min(field.size[2] - 1);
    Some((x0, x1.min(field.size[0] - 1), z0, z1.min(field.size[2] - 1)))
}
fn terrain_render_chunk_bounds(
    grid: &SmoothVoxelGrid,
    key: TerrainRenderChunkKey,
) -> Option<(u32, u32, u32, u32)> {
    let chunk_samples = terrain_render_chunk_samples(grid.spacing_studs);
    let x_start = key[0].checked_mul(chunk_samples)?;
    let z_start = key[1].checked_mul(chunk_samples)?;
    if x_start >= grid.size[0] || z_start >= grid.size[2] {
        return None;
    }
    let x_end = (x_start + chunk_samples).min(grid.size[0]);
    let z_end = (z_start + chunk_samples).min(grid.size[2]);
    Some((x_start, x_end, z_start, z_end))
}

fn terrain_render_chunk_aabb(
    result: &TerrainResult,
    key: TerrainRenderChunkKey,
) -> Option<(Vec3, Vec3)> {
    match &result.data {
        TerrainData::Heightfield(heightfield) => {
            let (x0, x1, z0, z1) = heightfield_chunk_bounds(heightfield, key)?;
            let mut min_y = f32::INFINITY;
            let mut max_y = f32::NEG_INFINITY;
            for z in z0..=z1 {
                for x in x0..=x1 {
                    let h = heightfield.height(x, z)?;
                    min_y = min_y.min(h);
                    max_y = max_y.max(h);
                }
            }
            Some((
                Vec3::new(
                    heightfield.origin[0] as f32 + x0 as f32,
                    min_y,
                    heightfield.origin[2] as f32 + z0 as f32,
                ),
                Vec3::new(
                    heightfield.origin[0] as f32 + x1 as f32 + 1.0,
                    max_y,
                    heightfield.origin[2] as f32 + z1 as f32 + 1.0,
                ),
            ))
        }
        TerrainData::Voxel(grid) => {
            let (x_start, x_end, z_start, z_end) = terrain_render_chunk_bounds(grid, key)?;
            let (grid_min, grid_max) = grid.world_bounds();
            Some((
                Vec3::new(
                    grid_min[0] + x_start as f32 * grid.spacing_studs,
                    grid_min[1],
                    grid_min[2] + z_start as f32 * grid.spacing_studs,
                ),
                Vec3::new(
                    grid_min[0] + x_end as f32 * grid.spacing_studs,
                    grid_max[1],
                    grid_min[2] + z_end as f32 * grid.spacing_studs,
                ),
            ))
        }
    }
}

fn terrain_chunk_within_render_distance(
    min: Vec3,
    max: Vec3,
    focus: Vec3,
    radius_chunks: i32,
) -> bool {
    if radius_chunks <= 0 {
        return true;
    }
    let radius_studs = radius_chunks as f32 * CHUNK_SIZE as f32;
    max.x >= focus.x - radius_studs
        && min.x <= focus.x + radius_studs
        && max.z >= focus.z - radius_studs
        && min.z <= focus.z + radius_studs
}

fn terrain_water_chunk_aabb(
    result: &TerrainResult,
    key: TerrainRenderChunkKey,
    water_level: f32,
) -> Option<(Vec3, Vec3)> {
    let (mut min, mut max) = terrain_render_chunk_aabb(result, key)?;
    if water_level.is_finite() {
        min.y = min.y.min(water_level);
        max.y = max.y.max(water_level);
    }
    Some((min, max))
}

fn terrain_chunk_visible(
    result: &TerrainResult,
    key: TerrainRenderChunkKey,
    camera_frustum: &CameraFrustum,
    focus: Vec3,
    radius_chunks: i32,
    water_level: Option<f32>,
) -> bool {
    let bounds = if let Some(level) = water_level {
        terrain_water_chunk_aabb(result, key, level)
    } else {
        terrain_render_chunk_aabb(result, key)
    };
    let Some((min, max)) = bounds else {
        return false;
    };
    terrain_chunk_within_render_distance(min, max, focus, radius_chunks)
        && camera_frustum.intersects_aabb(min, max)
}

fn terrain_grass_chunk_limit(grid: &SmoothVoxelGrid, key: TerrainRenderChunkKey) -> u32 {
    let Some((x_start, x_end, z_start, z_end)) = terrain_render_chunk_bounds(grid, key) else {
        return 0;
    };
    let total_columns = (grid.size[0] as u64)
        .saturating_mul(grid.size[2] as u64)
        .max(1);
    let chunk_samples = terrain_render_chunk_samples(grid.spacing_studs);
    let chunks_x = grid.size[0].div_ceil(chunk_samples);
    let mut columns_before = 0_u64;
    for chunk_z in 0..=key[1] {
        let row_z_start = chunk_z * chunk_samples;
        let row_z_end = (row_z_start + chunk_samples).min(grid.size[2]);
        let row_height = (row_z_end - row_z_start) as u64;
        let x_limit = if chunk_z == key[1] { key[0] } else { chunks_x };
        for chunk_x in 0..x_limit {
            let row_x_start = chunk_x * chunk_samples;
            let row_x_end = (row_x_start + chunk_samples).min(grid.size[0]);
            columns_before += (row_x_end - row_x_start) as u64 * row_height;
        }
    }
    let chunk_columns = (x_end - x_start) as u64 * (z_end - z_start) as u64;
    let quota_before = MAX_PROCEDURAL_GRASS_CLUMPS as u64 * columns_before / total_columns;
    let quota_after =
        MAX_PROCEDURAL_GRASS_CLUMPS as u64 * (columns_before + chunk_columns) / total_columns;
    (quota_after - quota_before) as u32
}

fn build_water_body_region(
    body: &crate::world::terrain::WaterBody,
    origin: [f32; 2],
    size: [u32; 2],
    spacing: f32,
    surface_height: impl Fn(f32, f32) -> Option<f32>,
) -> Vec<f32> {
    if !body.visible || body.validate().is_err() || spacing <= 0.0 {
        return Vec::new();
    }
    let (min, max) = body.bounds();
    let x0 = (((min[0] - origin[0]) / spacing).floor() as i64).max(0) as u32;
    let z0 = (((min[1] - origin[1]) / spacing).floor() as i64).max(0) as u32;
    let x1 = (((max[0] - origin[0]) / spacing).ceil() as i64)
        .max(0)
        .min(size[0] as i64) as u32;
    let z1 = (((max[1] - origin[1]) / spacing).ceil() as i64)
        .max(0)
        .min(size[1] as i64) as u32;
    if x1 <= x0 || z1 <= z0 {
        return Vec::new();
    }
    let spline_samples = body.spline_samples();
    let nx = (x1 - x0 + 1) as usize;
    let mut signed = Vec::with_capacity(nx * (z1 - z0 + 1) as usize);
    for z in z0..=z1 {
        for x in x0..=x1 {
            let wx = origin[0] + x as f32 * spacing;
            let wz = origin[1] + z as f32 * spacing;
            let boundary = body.signed_coverage_samples(wx, wz, &spline_samples);
            let below = surface_height(wx, wz).map_or(8.0, |height| {
                body.surface_y_at_samples(wx, wz, &spline_samples) - height
            });
            signed.push(boundary.min(below));
        }
    }
    let mut vertices = Vec::new();
    for z in z0..z1 {
        for x in x0..x1 {
            let i = (z - z0) as usize * nx + (x - x0) as usize;
            let depths = [signed[i], signed[i + nx], signed[i + nx + 1], signed[i + 1]];
            if depths.iter().all(|d| *d <= 0.0) {
                continue;
            }
            let wx = origin[0] + x as f32 * spacing;
            let wz = origin[1] + z as f32 * spacing;
            let points = [
                [wx, body.surface_y_at_samples(wx, wz, &spline_samples), wz],
                [
                    wx,
                    body.surface_y_at_samples(wx, wz + spacing, &spline_samples),
                    wz + spacing,
                ],
                [
                    wx + spacing,
                    body.surface_y_at_samples(wx + spacing, wz + spacing, &spline_samples),
                    wz + spacing,
                ],
                [
                    wx + spacing,
                    body.surface_y_at_samples(wx + spacing, wz, &spline_samples),
                    wz,
                ],
            ];
            if depths.iter().all(|d| *d > 0.0) {
                push_water_gradient_quad(&mut vertices, points, depths);
            } else {
                emit_shoreline_clipped_cell(&mut vertices, points, depths);
            }
        }
    }
    vertices
}

fn append_water_bodies(
    mesh: &mut TerrainMesh,
    result: &TerrainResult,
    config: &TerrainConfig,
    origin: [f32; 2],
    size: [u32; 2],
    spacing: f32,
) {
    if config.water_bodies.is_empty() {
        return;
    }
    for body in &config.water_bodies {
        let mut generated =
            build_water_body_region(body, origin, size, spacing, |x, z| match &result.data {
                TerrainData::Voxel(grid) => grid.surface_height_at(x, z),
                TerrainData::Heightfield(field) => field.surface_height_at(x, z),
            });
        if generated.is_empty() {
            continue;
        }
        if body.kind == crate::world::terrain::WaterBodyKind::River {
            let river_samples = body.spline_samples();
            for vertex in generated.chunks_exact_mut(12) {
                let current = body.flow_at_samples(vertex[0], vertex[2], &river_samples);
                vertex[10] = current[0];
                vertex[11] = current[1];
            }
        }
        mesh.batches.push(TerrainMeshBatch {
            material: TerrainMaterialSlot::Water,
            vertices: generated,
            water_body_id: Some(body.id),
        });
    }
}

pub fn build_water_scene_mesh(result: &TerrainResult, config: &TerrainConfig) -> TerrainMesh {
    let mut out = TerrainMesh::default();
    if config.ocean_enabled {
        let (wave_height, wave_speed) = geometry_wave_parameters(config);
        let water = match &result.data {
            TerrainData::Heightfield(field) => {
                build_water_with_runup(field, config.plains.water_level, wave_height, wave_speed)
            }
            TerrainData::Voxel(grid) => build_voxel_water_region_with_runup(
                grid,
                config.plains.water_level,
                wave_height,
                wave_speed,
                0,
                grid.size[0],
                0,
                grid.size[2],
            ),
        };
        if !water.is_empty() {
            out.batches.push(TerrainMeshBatch {
                material: TerrainMaterialSlot::Water,
                vertices: water,
                water_body_id: None,
            });
        }
    }
    match &result.data {
        TerrainData::Heightfield(field) => append_water_bodies(
            &mut out,
            result,
            config,
            [field.origin[0] as f32, field.origin[2] as f32],
            [field.size[0], field.size[2]],
            1.0,
        ),
        TerrainData::Voxel(grid) => {
            let (min, _) = grid.world_bounds();
            append_water_bodies(
                &mut out,
                result,
                config,
                [min[0], min[2]],
                [grid.size[0], grid.size[2]],
                grid.spacing_studs,
            );
        }
    }
    out
}

fn build_terrain_mesh_chunk(
    result: &TerrainResult,
    config: &TerrainConfig,
    key: TerrainRenderChunkKey,
) -> TerrainMesh {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("terrain", "remesh_chunk", false);
    let TerrainData::Voxel(grid) = &result.data else {
        let TerrainData::Heightfield(field) = &result.data else {
            return TerrainMesh::default();
        };
        let Some((x0, x1, z0, z1)) = heightfield_chunk_bounds(field, key) else {
            return TerrainMesh::default();
        };
        let mut mesh = TerrainMesh {
            batches: vec![TerrainMeshBatch {
                material: TerrainMaterialSlot::Surface,
                vertices: Vec::new(),
                water_body_id: None,
            }],
            source_signature: 0,
        };
        let surface = &mut mesh.batches[0].vertices;
        for z in z0..z1 {
            for x in x0..x1 {
                let p00 = point(field, x, z);
                let p10 = point(field, x + 1, z);
                let p11 = point(field, x + 1, z + 1);
                let p01 = point(field, x, z + 1);
                push_triangle(surface, p00, p11, p10);
                push_triangle(surface, p00, p01, p11);
            }
        }
        if config.ocean_enabled {
            let (wave_height, wave_speed) = geometry_wave_parameters(config);
            let water = build_heightfield_water_region_with_runup(
                field,
                config.plains.water_level,
                wave_height,
                wave_speed,
                x0,
                x1,
                z0,
                z1,
            );
            if !water.is_empty() {
                mesh.batches.push(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Water,
                    vertices: water,
                    water_body_id: None,
                });
            }
        }
        append_water_bodies(
            &mut mesh,
            result,
            config,
            [
                field.origin[0] as f32 + x0 as f32,
                field.origin[2] as f32 + z0 as f32,
            ],
            [x1 - x0, z1 - z0],
            1.0,
        );
        return mesh;
    };
    let Some((x_start, x_end, z_start, z_end)) = terrain_render_chunk_bounds(grid, key) else {
        return TerrainMesh::default();
    };

    let mut mesh = match super::native_landscape_preview::native_surface_for_legacy_chunk(
        result, config, x_start, x_end, z_start, z_end,
    ) {
        Some(mesh) => mesh,
        None => {
            eprintln!(
                "[Worldkiln 0.9] Native landscape import failed for tile {key:?};                  legacy voxel geometry rendering is disabled."
            );
            return TerrainMesh::default();
        }
    };

    let water = if config.ocean_enabled {
        let (wave_height, wave_speed) = geometry_wave_parameters(config);
        build_voxel_water_region_with_runup(
            grid,
            config.plains.water_level,
            wave_height,
            wave_speed,
            x_start,
            x_end,
            z_start,
            z_end,
        )
    } else {
        Vec::new()
    };
    if !water.is_empty() {
        mesh.batches.push(TerrainMeshBatch {
            material: TerrainMaterialSlot::Water,
            vertices: water,
            water_body_id: None,
        });
    }

    let (grid_min, _) = grid.world_bounds();
    append_water_bodies(
        &mut mesh,
        result,
        config,
        [
            grid_min[0] + x_start as f32 * grid.spacing_studs,
            grid_min[2] + z_start as f32 * grid.spacing_studs,
        ],
        [x_end - x_start, z_end - z_start],
        grid.spacing_studs,
    );

    let grass = build_voxel_grass_region(
        grid,
        config.plains.water_level,
        config.grass,
        &config.materials,
        config.plains.seed,
        x_start,
        x_end,
        z_start,
        z_end,
        terrain_grass_chunk_limit(grid, key),
    );
    if !grass.is_empty() {
        mesh.batches.push(TerrainMeshBatch {
            material: TerrainMaterialSlot::Grass,
            vertices: grass,
            water_body_id: None,
        });
    }
    mesh
}

fn build_terrain_mesh_with_grass(result: &TerrainResult, config: &TerrainConfig) -> TerrainMesh {
    let mut mesh = build_terrain_mesh(
        result,
        if config.ocean_enabled {
            config.plains.water_level
        } else {
            f32::NEG_INFINITY
        },
        config.voxel_layers.slope_shading_strength,
    );
    if let TerrainData::Heightfield(field) = &result.data {
        if ADVANCED_SHORELINE_ENABLED && config.ocean_enabled && config.water.wave_height > 0.0 {
            mesh.batches
                .retain(|batch| !matches!(batch.material, TerrainMaterialSlot::Water));
            let water = build_water_with_runup(
                field,
                config.plains.water_level,
                config.water.wave_height,
                config.water.wave_speed,
            );
            if !water.is_empty() {
                mesh.batches.push(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Water,
                    vertices: water,
                    water_body_id: None,
                });
            }
        }
        append_water_bodies(
            &mut mesh,
            result,
            config,
            [field.origin[0] as f32, field.origin[2] as f32],
            [field.size[0], field.size[2]],
            1.0,
        );
    }
    if let TerrainData::Voxel(grid) = &result.data {
        let grass = build_voxel_grass(
            grid,
            config.plains.water_level,
            config.grass,
            &config.materials,
            config.plains.seed,
        );
        if !grass.is_empty() {
            mesh.batches.push(TerrainMeshBatch {
                material: TerrainMaterialSlot::Grass,
                vertices: grass,
                water_body_id: None,
            });
        }
    }
    mesh
}

fn water_surface_attributes(depth: f32, wave_energy: f32) -> [f32; 4] {
    let depth = depth.max(0.0);
    let foam = 1.0 - smoothstep_cpu(0.20, 3.50, depth);
    let depth_normalized = smoothstep_cpu(0.0, 16.0, depth);
    [foam, depth_normalized, wave_energy.clamp(0.05, 1.0), 1.0]
}

fn water_spatial_attributes(depth: f32) -> [f32; 4] {
    let energy = 0.05 + 0.95 * smoothstep_cpu(0.0, 12.0, depth.max(0.0));
    water_surface_attributes(depth, energy)
}

fn smoothstep_cpu(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge1 <= edge0 {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn water_chunk_wave_energy(water_tiles: usize, total_tiles: usize, average_depth: f32) -> f32 {
    if water_tiles == 0 || total_tiles == 0 {
        return 0.05;
    }
    let coverage = (water_tiles as f32 / total_tiles as f32).clamp(0.0, 1.0);
    let openness = coverage.sqrt();
    let depth_energy = ((average_depth - 0.5) / 7.5).clamp(0.0, 1.0);
    (0.08 + 0.92 * openness * (0.35 + 0.65 * depth_energy)).clamp(0.05, 1.0)
}

fn build_water(heightfield: &HeightfieldData, water_level: f32) -> Vec<f32> {
    build_water_with_runup(heightfield, water_level, 0.0, 0.0)
}

fn build_water_with_runup(
    heightfield: &HeightfieldData,
    water_level: f32,
    wave_height: f32,
    wave_speed: f32,
) -> Vec<f32> {
    build_heightfield_water_region_with_runup(
        heightfield,
        water_level,
        wave_height,
        wave_speed,
        0,
        heightfield.size[0].saturating_sub(1),
        0,
        heightfield.size[2].saturating_sub(1),
    )
}

fn build_heightfield_water_region_with_runup(
    field: &HeightfieldData,
    level: f32,
    wave_height: f32,
    wave_speed: f32,
    x0: u32,
    x1: u32,
    z0: u32,
    z1: u32,
) -> Vec<f32> {
    let mut vertices = Vec::new();
    if !level.is_finite() || x0 >= x1 || z0 >= z1 || x1 >= field.size[0] || z1 >= field.size[2] {
        return vertices;
    }

    let halo = 16_u32;
    let rx0 = x0.saturating_sub(halo);
    let rz0 = z0.saturating_sub(halo);
    let rx1 = (x1 + halo).min(field.size[0] - 1);
    let rz1 = (z1 + halo).min(field.size[2] - 1);
    let rw = (rx1 - rx0 + 1) as usize;
    let rh = (rz1 - rz0 + 1) as usize;
    let mut nearest = Vec::new();
    if wave_height > 0.001 {
        let mut wet = vec![false; rw * rh];
        for z in rz0..=rz1 {
            for x in rx0..=rx1 {
                wet[(z - rz0) as usize * rw + (x - rx0) as usize] =
                    field.height(x, z).is_some_and(|height| height < level);
            }
        }
        if wet.iter().any(|&v| v) {
            nearest = ocean_nearest_squared_distance(&wet, rw, rh);
        }
    }
    let radius = (wave_height.max(0.0) * (1.6 + 0.45 * wave_speed.abs().sqrt()))
        .ceil()
        .clamp(1.0, 16.0);
    let reach = (wave_height.max(0.0) * 0.62).min(5.0);
    let ox = field.origin[0] as f32;
    let oz = field.origin[2] as f32;

    for z in z0..z1 {
        for x in x0..x1 {
            let Some(h0) = field.height(x, z) else {
                continue;
            };
            let Some(h1) = field.height(x, z + 1) else {
                continue;
            };
            let Some(h2) = field.height(x + 1, z + 1) else {
                continue;
            };
            let Some(h3) = field.height(x + 1, z) else {
                continue;
            };
            let wx = ox + x as f32 + 0.5;
            let wz = oz + z as f32 + 0.5;
            let ground = [
                [wx, h0, wz],
                [wx, h1, wz + 1.0],
                [wx + 1.0, h2, wz + 1.0],
                [wx + 1.0, h3, wz],
            ];
            let depths = [level - h0, level - h1, level - h2, level - h3];
            let p = ground.map(|v| [v[0], level, v[2]]);
            if depths.iter().any(|d| *d > 0.0) {
                emit_shoreline_clipped_cell(&mut vertices, p, depths);
            }
            if reach <= 0.01 || nearest.is_empty() {
                continue;
            }
            let dist = nearest[(z - rz0) as usize * rw + (x - rx0) as usize];
            if dist > radius * radius {
                continue;
            }
            if depths.iter().any(|d| *d < 0.0) {
                push_shore_swash_quad(&mut vertices, ground, level, reach, 1.0);
            }
        }
    }
    vertices
}

fn point(heightfield: &HeightfieldData, x: u32, z: u32) -> Vec3 {
    Vec3::new(
        heightfield.origin[0] as f32 + x as f32 + 0.5,
        heightfield.height(x, z).unwrap(),
        heightfield.origin[2] as f32 + z as f32 + 0.5,
    )
}

fn neighbor_top(heightfield: &HeightfieldData, x: i32, z: i32) -> Option<f32> {
    if x < 0 || z < 0 {
        None
    } else {
        heightfield.height(x as u32, z as u32)
    }
}

pub(super) fn terrain_palette_layer(name: &str) -> usize {
    match name
        .strip_prefix("PBR:")
        .unwrap_or(name)
        .trim()
        .to_ascii_lowercase()
        .as_str()
    {
        "grass" | "grass_1" | "surface" | "__worldkiln_terrain_surface" => 0,
        "dirt" | "soil" | "subsurface" | "__worldkiln_terrain_subsurface" => 1,
        "rock" | "stone" | "tile_24" | "__worldkiln_terrain_rock" => 2,
        "sand" | "beach" => 3,
        "snow" => 4,
        "mud" => 5,
        _ => 0,
    }
}
pub(super) fn voxel_layers_and_normal(
    grid: &SmoothVoxelGrid,
    position: Vec3,
    fallback: usize,
    face: Vec3,
) -> ([f32; 6], Vec3) {
    voxel_layers_and_normal_with_palette(grid, position, fallback, face, None)
}
pub(super) fn voxel_layers_and_normal_with_palette(
    grid: &SmoothVoxelGrid,
    position: Vec3,
    fallback: usize,
    face: Vec3,
    palette: Option<&[usize]>,
) -> ([f32; 6], Vec3) {
    let origin = Vec3::from_array(grid.origin.map(|v| v as f32));
    let p = (position - origin) / grid.spacing_studs - Vec3::splat(0.5);
    let base = p.floor();
    let t = p - base;
    let ix = base.x as i32;
    let iy = base.y as i32;
    let iz = base.z as i32;
    let mut weights = [0.0_f32; 6];
    let mut gradient = Vec3::ZERO;
    for z in 0..=1 {
        for y in 0..=1 {
            for x in 0..=1 {
                let coords = [ix + x, iy + y, iz + z];
                if coords.iter().any(|v| *v < 0) {
                    continue;
                }
                let Some(voxel) = grid.voxel(coords[0] as u32, coords[1] as u32, coords[2] as u32)
                else {
                    continue;
                };
                let density = voxel.occupancy_unit();
                if density <= 0.0 {
                    continue;
                }
                let wx = if x == 0 { 1.0 - t.x } else { t.x };
                let wy = if y == 0 { 1.0 - t.y } else { t.y };
                let wz = if z == 0 { 1.0 - t.z } else { t.z };
                let weight = wx * wy * wz * density;
                let layer = palette
                    .and_then(|p| p.get(voxel.material as usize).copied())
                    .unwrap_or_else(|| {
                        grid.material_name(voxel.material)
                            .map(terrain_palette_layer)
                            .unwrap_or(fallback.min(5))
                    });
                weights[layer] += weight;
                let sign = |idx: i32| if idx == 0 { -1.0 } else { 1.0 };
                gradient.x += sign(x) * wy * wz * density;
                gradient.y += sign(y) * wx * wz * density;
                gradient.z += sign(z) * wx * wy * density;
            }
        }
    }
    let total: f32 = weights.iter().sum();
    if total > 0.00001 {
        for w in &mut weights {
            *w /= total;
        }
    } else {
        weights[fallback.min(5)] = 1.0;
    }
    let normal = (-gradient).normalize_or_zero();
    let n = if normal.length_squared() < 0.001 {
        face
    } else {
        normal
    };
    (weights, n)
}
#[cfg(test)]
fn voxel_layer_weights(grid: &SmoothVoxelGrid, position: Vec3, fallback: usize) -> [f32; 6] {
    voxel_layers_and_normal(grid, position, fallback, Vec3::Y).0
}
fn push_blended_terrain_triangle(
    vertices: &mut Vec<f32>,
    grid: &SmoothVoxelGrid,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    material: u16,
) {
    let face = (b - a).cross(c - a).normalize_or_zero();
    let fallback = grid
        .material_name(material)
        .map(terrain_palette_layer)
        .unwrap_or(0);
    for p in [a, b, c] {
        let (weights, n) = voxel_layers_and_normal(grid, p, fallback, face);
        vertices.extend_from_slice(&[
            p.x, p.y, p.z, n.x, n.y, n.z, weights[0], weights[1], weights[2], weights[3],
            weights[4], weights[5],
        ]);
    }
}

fn terrain_slope_shade(normal: Vec3, strength: f32) -> f32 {
    let normalized = normal.normalize_or_zero();
    let vertical_alignment = normalized.y.abs().clamp(0.0, 1.0);
    let slope_factor = 1.0 - vertical_alignment;
    1.0 - strength.clamp(0.0, 0.75) * slope_factor * slope_factor
}

fn terrain_slope_color(normal: [f32; 3], strength: f32) -> [f32; 4] {
    let shade = terrain_slope_shade(Vec3::from_array(normal), strength);
    [shade, shade, shade, 1.0]
}

fn push_terrain_triangle(
    vertices: &mut Vec<f32>,
    a: Vec3,
    b: Vec3,
    c: Vec3,
    slope_shading_strength: f32,
) {
    let normal = (b - a).cross(c - a).normalize_or_zero();
    let shade = terrain_slope_shade(normal, slope_shading_strength);
    for point in [a, b, c] {
        vertices.extend_from_slice(&[
            point.x, point.y, point.z, normal.x, normal.y, normal.z, shade, shade, shade, 1.0,
            point.x, point.z,
        ]);
    }
}

fn push_triangle(vertices: &mut Vec<f32>, a: Vec3, b: Vec3, c: Vec3) {
    let normal = (b - a).cross(c - a).normalize_or_zero();
    for point in [a, b, c] {
        vertices.extend_from_slice(&[
            point.x, point.y, point.z, normal.x, normal.y, normal.z, 1.0, 1.0, 1.0, 1.0, point.x,
            point.z,
        ]);
    }
}

fn landscape_texture_id(name: &str, role: &str) -> String {
    if name.is_empty() || name.starts_with("Material:") {
        name.to_owned()
    } else {
        crate::renderer::rhi::material_library::landscape_pbr_name(name, role)
    }
}

fn terrain_palette_visual<'a>(
    name: &'a str,
    materials: &'a TerrainMaterials,
) -> (&'a str, [f32; 3]) {
    if name == TERRAIN_FALLBACK_SURFACE_MATERIAL
        || name == materials.surface.as_str()
        || matches!(name, "grass_1" | "grass" | "PBR:Grass")
    {
        (
            if materials.surface.starts_with("PBR:")
                || materials.surface.starts_with("Material:")
                || materials.surface.is_empty()
            {
                materials.surface.as_str()
            } else {
                "PBR:Grass"
            },
            [1.0, 1.0, 1.0],
        )
    } else if name == TERRAIN_FALLBACK_SUBSURFACE_MATERIAL
        || name == materials.subsurface.as_str()
        || matches!(name, "dirt" | "PBR:Dirt")
    {
        (
            if materials.subsurface.starts_with("PBR:")
                || materials.subsurface.starts_with("Material:")
                || materials.subsurface.is_empty()
            {
                materials.subsurface.as_str()
            } else {
                "PBR:Dirt"
            },
            [0.46, 0.31, 0.16],
        )
    } else if name == TERRAIN_FALLBACK_ROCK_MATERIAL
        || name == materials.rock.as_str()
        || matches!(name, "tile_24" | "rock" | "PBR:Rock")
    {
        (
            if materials.rock.starts_with("PBR:")
                || materials.rock.starts_with("Material:")
                || materials.rock.is_empty()
            {
                materials.rock.as_str()
            } else {
                "PBR:Rock"
            },
            [0.48, 0.48, 0.46],
        )
    } else if name == "sand" || name == "PBR:Sand" {
        ("PBR:Sand", [0.73, 0.67, 0.49])
    } else {
        ("PBR:Rock", [0.62, 0.62, 0.62])
    }
}

fn terrain_blend_role(name: &str, materials: &TerrainMaterials) -> Option<i32> {
    if name.is_empty() {
        return None;
    }
    if name == TERRAIN_FALLBACK_SURFACE_MATERIAL
        || (!materials.surface.is_empty() && name == materials.surface)
    {
        Some(0)
    } else if name == TERRAIN_FALLBACK_SUBSURFACE_MATERIAL
        || (!materials.subsurface.is_empty() && name == materials.subsurface)
    {
        Some(1)
    } else if name == TERRAIN_FALLBACK_ROCK_MATERIAL
        || (!materials.rock.is_empty() && name == materials.rock)
    {
        Some(2)
    } else if name == "PBR:Sand" || name == "sand" {
        Some(3)
    } else {
        None
    }
}

fn logical_signature(result: &TerrainResult) -> u64 {
    let mut hasher = DefaultHasher::new();
    result.request.position.hash(&mut hasher);
    result.request.size.hash(&mut hasher);
    result.generator_implementation_version.hash(&mut hasher);
    match &result.data {
        TerrainData::Heightfield(heightfield) => {
            0_u8.hash(&mut hasher);
            for height in &heightfield.heights {
                height.to_bits().hash(&mut hasher);
            }
        }
        TerrainData::Voxel(grid) => {
            1_u8.hash(&mut hasher);
            grid.origin.hash(&mut hasher);
            grid.size.hash(&mut hasher);
            grid.materials.hash(&mut hasher);
            for voxel in &grid.voxels {
                voxel.material.hash(&mut hasher);
                voxel.occupancy.hash(&mut hasher);
            }
        }
    }
    hasher.finish()
}

fn terrain_render_config_key(config: &TerrainConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    config.plains.water_level.to_bits().hash(&mut hasher);
    if ADVANCED_SHORELINE_ENABLED {
        config.water.wave_height.to_bits().hash(&mut hasher);
        config.water.wave_speed.to_bits().hash(&mut hasher);
    }
    config.ocean_enabled.hash(&mut hasher);
    config.infinite_ocean.hash(&mut hasher);
    for body in &config.water_bodies {
        body.id.hash(&mut hasher);
        match body.kind {
            crate::world::terrain::WaterBodyKind::Lake => 0u8,
            crate::world::terrain::WaterBodyKind::River => 1u8,
        }
        .hash(&mut hasher);
        body.visible.hash(&mut hasher);
        body.elevation.to_bits().hash(&mut hasher);
        for point in &body.points {
            for value in point.position {
                value.to_bits().hash(&mut hasher);
            }
            point.width.to_bits().hash(&mut hasher);
            point.height_offset.to_bits().hash(&mut hasher);
            for value in point.tangent {
                value.to_bits().hash(&mut hasher);
            }
            point.flow_angle.to_bits().hash(&mut hasher);
            point.flow_speed.to_bits().hash(&mut hasher);
        }
    }
    config.plains.seed.hash(&mut hasher);
    config
        .voxel_layers
        .slope_shading_strength
        .to_bits()
        .hash(&mut hasher);
    config.grass.enabled.hash(&mut hasher);
    config.grass.density.to_bits().hash(&mut hasher);
    config.grass.length_studs.to_bits().hash(&mut hasher);
    config.materials.surface.hash(&mut hasher);
    config.materials.subsurface.hash(&mut hasher);
    config.materials.rock.hash(&mut hasher);
    config.materials.water.hash(&mut hasher);
    hasher.finish()
}

impl TerrainGpuChunk {
    fn upload(mesh: TerrainMesh) -> Self {
        let _diagnostic_operation =
            crate::engine::development_capture::operation("terrain", "vertex_upload", false);
        let batches = mesh
            .batches
            .into_iter()
            .filter(|batch| !batch.vertices.is_empty())
            .map(|batch| {
                let (vao, vbo, vertex_count) = upload_block_vertices_3d(&batch.vertices);
                TerrainGpuBatch {
                    material: batch.material,
                    water_body_id: batch.water_body_id,
                    vao,
                    vbo,
                    vertex_count,
                }
            })
            .collect();
        Self { batches }
    }

    fn free(&mut self) {
        unsafe {
            for batch in &mut self.batches {
                if batch.vao != 0 {
                    gl::DeleteVertexArrays(1, &batch.vao);
                    batch.vao = 0;
                }
                if batch.vbo != 0 {
                    gl::DeleteBuffers(1, &batch.vbo);
                    batch.vbo = 0;
                }
            }
        }
    }
}

pub(crate) struct PreparedProjectTerrain {
    revision: u64,
    config_key: u64,
    source_region: ([i32; 3], [i32; 3]),
    chunks: std::collections::VecDeque<(TerrainRenderChunkKey, TerrainMesh)>,
}
pub(crate) fn prepare_project_terrain(
    world: &World,
    current: &(impl Fn() -> bool + Sync),
) -> Result<Option<PreparedProjectTerrain>, String> {
    let Some(result) = world.terrain.effective() else {
        return Ok(None);
    };
    let keys = terrain_render_chunk_keys(result);
    let config = &world.terrain.config;
    let workers = std::thread::available_parallelism()
        .map_or(1, |n| n.get().min(4))
        .min(keys.len().max(1));
    let next = std::sync::atomic::AtomicUsize::new(0);
    let mut built = Vec::<(usize, TerrainMesh)>::with_capacity(keys.len());
    std::thread::scope(|scope| -> Result<(), String> {
        let handles: Vec<_> = (0..workers)
            .map(|_| {
                scope.spawn(|| {
                    let mut output = Vec::new();
                    loop {
                        if !current() {
                            return Err("Superseded project terrain preparation".into());
                        }
                        let index = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        if index >= keys.len() {
                            break;
                        }
                        let mesh = build_terrain_mesh_chunk(result, config, keys[index]);
                        output.push((index, mesh));
                    }
                    Ok::<_, String>(output)
                })
            })
            .collect();
        let mut canceled = false;
        for handle in handles {
            match handle.join() {
                Ok(Ok(results)) => built.extend(results),
                Ok(Err(_)) => canceled = true,
                Err(_) => return Err("Terrain mesh worker panicked".to_owned()),
            }
        }
        if canceled || !current() {
            return Err("Superseded project terrain preparation".into());
        }
        Ok(())
    })?;
    built.sort_unstable_by_key(|(i, _)| *i);
    let chunks = built
        .into_iter()
        .map(|(index, mesh)| (keys[index], mesh))
        .collect::<std::collections::VecDeque<_>>();
    Ok(Some(PreparedProjectTerrain {
        revision: world.terrain.render_revision(),
        config_key: terrain_render_config_key(&world.terrain.config),
        source_region: (result.request.position, result.request.size),
        chunks,
    }))
}

fn ocean_contact_signed_distance(mask: &[bool], n: usize, texel_studs: f32) -> Vec<u8> {
    const RANGE_STUDS: f32 = 3.0;
    let dry: Vec<bool> = mask.iter().map(|&v| !v).collect();
    let to_wet = ocean_nearest_squared_distance(mask, n, n);
    let to_dry = ocean_nearest_squared_distance(&dry, n, n);
    mask.iter()
        .enumerate()
        .map(|(i, &inside)| {
            let cells = if inside { to_dry[i] } else { to_wet[i] };
            let edge = (cells.sqrt() - 0.5).max(0.0) * texel_studs;
            let signed = if inside { edge } else { -edge };
            ((0.5 + 0.5 * (signed / RANGE_STUDS).clamp(-1.0, 1.0)) * 255.0).round() as u8
        })
        .collect()
}

fn upload_ocean_contact_region(mut texture: u32, n: usize, upload: &AtlasUpload) -> u32 {
    unsafe {
        let allocate = texture == 0;
        if allocate {
            gl::GenTextures(1, &mut texture);
        }
        gl::ActiveTexture(gl::TEXTURE13);
        gl::BindTexture(gl::TEXTURE_2D, texture);
        gl::PixelStorei(gl::UNPACK_ALIGNMENT, 1);
        if allocate {
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MIN_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_MAG_FILTER, gl::LINEAR as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_S, gl::CLAMP_TO_EDGE as i32);
            gl::TexParameteri(gl::TEXTURE_2D, gl::TEXTURE_WRAP_T, gl::CLAMP_TO_EDGE as i32);
            gl::TexImage2D(
                gl::TEXTURE_2D,
                0,
                gl::R8 as i32,
                n as i32,
                n as i32,
                0,
                gl::RED,
                gl::UNSIGNED_BYTE,
                upload.pixels.as_ptr().cast(),
            );
        } else {
            gl::TexSubImage2D(
                gl::TEXTURE_2D,
                0,
                upload.x as i32,
                upload.z as i32,
                upload.width as i32,
                upload.height as i32,
                gl::RED,
                gl::UNSIGNED_BYTE,
                upload.pixels.as_ptr().cast(),
            );
        }
        gl::BindTexture(gl::TEXTURE_2D, 0);
        gl::ActiveTexture(gl::TEXTURE1);
    }
    texture
}

fn build_infinite_ocean_horizon(
    result: &TerrainResult,
    config: &TerrainConfig,
) -> Option<TerrainGpuChunk> {
    if !config.ocean_enabled || !config.infinite_ocean {
        return None;
    }
    let (lo, hi) = match &result.data {
        TerrainData::Heightfield(f) => {
            let x = f.origin[0] as f32 + 0.5;
            let z = f.origin[2] as f32 + 0.5;
            (
                [x, z],
                [
                    x + (f.size[0].saturating_sub(1)) as f32,
                    z + (f.size[2].saturating_sub(1)) as f32,
                ],
            )
        }
        TerrainData::Voxel(g) => {
            let (min, max) = g.world_bounds();
            ([min[0], min[2]], [max[0], max[2]])
        }
    };
    if !(lo[0] < hi[0] && lo[1] < hi[1]) {
        return None;
    }
    let level = config.plains.water_level;
    if !level.is_finite() {
        return None;
    }
    const RADII: [f32; 10] = [
        0.0, 12.0, 36.0, 96.0, 256.0, 640.0, 1500.0, 3000.0, 5600.0, 8800.0,
    ];
    const STEPS: usize = 96;
    let edge = |radius: f32, segment: usize| {
        let corner = segment / STEPS;
        let t = (segment % STEPS) as f32 / STEPS as f32;
        let x0 = lo[0] - radius;
        let x1 = hi[0] + radius;
        let z0 = lo[1] - radius;
        let z1 = hi[1] + radius;
        match corner {
            0 => [x0 + (x1 - x0) * t, z0],
            1 => [x1, z0 + (z1 - z0) * t],
            2 => [x1 - (x1 - x0) * t, z1],
            _ => [x0, z1 - (z1 - z0) * t],
        }
    };
    let mut vertices = Vec::with_capacity((RADII.len() - 1) * 4 * STEPS * 2 * 3 * 12);
    let mut push = |p: [f32; 2]| {
        vertices.extend_from_slice(&[
            p[0], level, p[1], 0.0, 1.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0,
        ]);
    };
    for r in 0..RADII.len() - 1 {
        for step in 0..4 * STEPS {
            let a = edge(RADII[r], step);
            let b = edge(RADII[r], (step + 1) % (4 * STEPS));
            let c = edge(RADII[r + 1], step);
            let d = edge(RADII[r + 1], (step + 1) % (4 * STEPS));
            for v in [a, b, c, b, d, c] {
                push(v);
            }
        }
    }
    Some(TerrainGpuChunk::upload(TerrainMesh {
        batches: vec![TerrainMeshBatch {
            material: TerrainMaterialSlot::Water,
            vertices,
            water_body_id: None,
        }],
        source_signature: 0,
    }))
}

impl TerrainGpuMesh {
    fn build_full(
        result: &TerrainResult,
        config: &TerrainConfig,
        revision: u64,
        config_key: u64,
    ) -> Self {
        let chunks = terrain_render_chunk_keys(result)
            .into_iter()
            .map(|key| {
                let mesh = build_terrain_mesh_chunk(result, config, key);
                (key, TerrainGpuChunk::upload(mesh))
            })
            .collect();
        Self {
            revision,
            config_key,
            source_region: (result.request.position, result.request.size),
            chunks,
            horizon: None,
            pending: std::collections::VecDeque::new(),
            pending_keys: std::collections::VecDeque::new(),
            ocean_contact_texture: 0,
            ocean_contact_mapping: [0.0; 4],
            ocean_contact_ready: false,
            ocean_contact_cpu: None,
            ocean_contact_dirty: Default::default(),
        }
    }

    pub(super) fn free(&mut self) {
        if self.ocean_contact_texture != 0 {
            unsafe {
                gl::DeleteTextures(1, &self.ocean_contact_texture);
            }
            self.ocean_contact_texture = 0;
        }
        for chunk in self.chunks.values_mut() {
            chunk.free();
        }
        self.chunks.clear();
        if let Some(mut horizon) = self.horizon.take() {
            horizon.free();
        }
        self.pending.clear();
        self.pending_keys.clear();
    }
}

impl Renderer {
    pub(crate) fn ocean_swim_appearance(
        &self,
        world: &World,
    ) -> crate::world::terrain::TerrainWaterSettings {
        world
            .terrain
            .config
            .resolved_water(&self.materials.borrow())
    }
    pub(crate) fn publish_project_terrain(&self, prepared: Option<PreparedProjectTerrain>) {
        if let Some(mut old) = self.terrain_cache.borrow_mut().take() {
            old.free();
        }
        if let Some(prepared) = prepared {
            *self.terrain_cache.borrow_mut() = Some(TerrainGpuMesh {
                revision: prepared.revision,
                config_key: prepared.config_key,
                source_region: prepared.source_region,
                chunks: BTreeMap::new(),
                horizon: None,
                pending: prepared.chunks,
                pending_keys: std::collections::VecDeque::new(),
                ocean_contact_texture: 0,
                ocean_contact_mapping: [0.0; 4],
                ocean_contact_ready: false,
                ocean_contact_cpu: None,
                ocean_contact_dirty: Default::default(),
            });
        }
    }
    fn ensure_terrain_cache(&self, world: &World) {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(21, file!(), line!());

        let Some(result) = world.terrain.effective() else {
            if let Some(mut cached) = self.terrain_cache.borrow_mut().take() {
                cached.free();
            }
            return;
        };

        let revision = world.terrain.render_revision();
        let config_key = terrain_render_config_key(&world.terrain.config);
        let source_region = (result.request.position, result.request.size);
        let full_dirty_revision = world.terrain.full_render_revision();

        let needs_full_rebuild = self.terrain_cache.borrow().as_ref().is_none_or(|cache| {
            cache.config_key != config_key
                || cache.source_region != source_region
                || full_dirty_revision > cache.revision
                || world
                    .terrain
                    .render_dirty_chunks_since(cache.revision)
                    .is_none()
        });

        if needs_full_rebuild {
            let keys = terrain_render_chunk_keys(result);
            let mut cache = self.terrain_cache.borrow_mut();
            if let Some(cache) = cache.as_mut() {
                if cache.source_region != source_region {
                    for old in cache.chunks.values_mut() {
                        old.free();
                    }
                    cache.chunks.clear();
                }
                cache.pending.clear();
                cache.pending_keys = keys.into();
                cache.config_key = config_key;
                cache.source_region = source_region;
                cache.revision = revision;
                cache.ocean_contact_ready = false;
                cache.ocean_contact_cpu = None;
                cache.ocean_contact_dirty.clear();
                if let Some(mut old) = cache.horizon.take() {
                    old.free();
                }
            } else {
                *cache = Some(TerrainGpuMesh {
                    revision,
                    config_key,
                    source_region,
                    chunks: BTreeMap::new(),
                    horizon: None,
                    pending: std::collections::VecDeque::new(),
                    pending_keys: keys.into(),
                    ocean_contact_texture: 0,
                    ocean_contact_mapping: [0.0; 4],
                    ocean_contact_ready: false,
                    ocean_contact_cpu: None,
                    ocean_contact_dirty: Default::default(),
                });
            }
        }

        let cached_revision = self
            .terrain_cache
            .borrow()
            .as_ref()
            .map_or(0, |cache| cache.revision);
        if cached_revision != revision {
            let dirty_keys = world
                .terrain
                .render_dirty_chunks_since(cached_revision)
                .expect("full refresh handles consumers outside the dirty journal");
            let mut holder = self.terrain_cache.borrow_mut();
            if let Some(cache) = holder.as_mut() {
                for key in dirty_keys {
                    if !cache.ocean_contact_dirty.contains(&key) {
                        cache.ocean_contact_dirty.push_back(key);
                    }
                    cache.pending.retain(|(pending, _)| *pending != key);
                    if !cache.pending_keys.contains(&key) {
                        cache.pending_keys.push_back(key);
                    }
                }
                cache.revision = revision;
            }
        }

        {
            let started = std::time::Instant::now();
            let mut cache = self.terrain_cache.borrow_mut();
            if let Some(cache) = cache.as_mut() {
                let max_uploads = if cache.pending.is_empty() && cache.pending_keys.len() <= 4 {
                    1
                } else {
                    2
                };
                for _ in 0..max_uploads {
                    let ready = cache.pending.pop_front().or_else(|| {
                        cache.pending_keys.pop_front().map(|key| {
                            (
                                key,
                                build_terrain_mesh_chunk(result, &world.terrain.config, key),
                            )
                        })
                    });
                    let Some((key, mesh)) = ready else {
                        break;
                    };
                    if let Some(mut replaced) =
                        cache.chunks.insert(key, TerrainGpuChunk::upload(mesh))
                    {
                        replaced.free();
                    }
                    if started.elapsed() >= std::time::Duration::from_millis(8) {
                        break;
                    }
                }
            }
        }
    }

    pub(super) fn draw_terrain(
        &self,
        world: &World,
        camera_frustum: &CameraFrustum,
        chunk_focus: Vec3,
        chunk_radius: i32,
        profile_draws: bool,
    ) -> TerrainDrawStats {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(20, file!(), line!());

        let profile_start = profile_draws.then(std::time::Instant::now);
        self.ensure_terrain_cache(world);
        unsafe {
            self.paint_gpu.borrow_mut().sync(&world.terrain);
        }
        let cache_sync_ns = profile_start.map_or(0, |t| t.elapsed().as_nanos() as u64);
        let Some(result) = world.terrain.effective() else {
            return TerrainDrawStats::default();
        };
        {
            let mut atlas_cache = self.terrain_cache.borrow_mut();
            if let Some(cache) = atlas_cache.as_mut() {
                if !cache.ocean_contact_ready {
                    if cache.ocean_contact_texture != 0 {
                        unsafe {
                            gl::DeleteTextures(1, &cache.ocean_contact_texture);
                        }
                        cache.ocean_contact_texture = 0;
                    }
                    cache.ocean_contact_cpu = None;
                    cache.ocean_contact_mapping = [0.0; 4];
                    if world.terrain.config.ocean_enabled {
                        let mut atlas = OceanContactAtlas::new(result);
                        let upload = atlas.rebuild(result, &world.terrain.config);
                        cache.ocean_contact_texture =
                            upload_ocean_contact_region(0, atlas.n, &upload);
                        cache.ocean_contact_mapping = atlas.mapping;
                        cache.ocean_contact_cpu = Some(atlas);
                    }
                    cache.ocean_contact_dirty.clear();
                    cache.ocean_contact_ready = true;
                } else {
                    for _ in 0..2 {
                        let Some(key) = cache.ocean_contact_dirty.pop_front() else {
                            break;
                        };
                        if let Some(atlas) = cache.ocean_contact_cpu.as_mut() {
                            let upload = atlas.update_chunk(result, &world.terrain.config, key);
                            upload_ocean_contact_region(
                                cache.ocean_contact_texture,
                                atlas.n,
                                &upload,
                            );
                        }
                    }
                }
            }
        }
        let resolved_materials = world.terrain.config.resolved_generation_materials();
        let materials = &resolved_materials;
        let cache = self.terrain_cache.borrow();
        let Some(cache) = cache.as_ref() else {
            return TerrainDrawStats::default();
        };
        let layer_pass =
            super::resources::get_uniform_location(self.grid_program, "u_terrain_layer_pass");
        let shore_count =
            super::resources::get_uniform_location(self.grid_program, "u_terrain_shore_count");
        let shore_params =
            super::resources::get_uniform_location(self.grid_program, "u_terrain_shore_params");
        let wet_eligible = super::resources::get_uniform_location(
            self.grid_program,
            "u_terrain_ocean_contact_enabled",
        );
        let mut stats = TerrainDrawStats::default();
        stats.cache_sync_ns = cache_sync_ns;
        let state_started = profile_draws.then(std::time::Instant::now);

        unsafe {
            gl::UniformMatrix4fv(
                self.grid_model_location,
                1,
                gl::FALSE,
                Mat4::IDENTITY.to_cols_array().as_ptr(),
            );
            gl::Uniform1i(self.grid_use_texture_location, 1);
            gl::Uniform4f(
                super::resources::get_uniform_location(
                    self.grid_program,
                    "u_ocean_contact_mapping",
                ),
                cache.ocean_contact_mapping[0],
                cache.ocean_contact_mapping[1],
                cache.ocean_contact_mapping[2],
                cache.ocean_contact_mapping[3],
            );
            gl::ActiveTexture(gl::TEXTURE13);
            gl::BindTexture(gl::TEXTURE_2D, cache.ocean_contact_texture);
            gl::ActiveTexture(gl::TEXTURE1);

            let b = world.terrain.config.material_blend;
            gl::Uniform4f(
                self.grid_terrain_blend_params_location,
                b.rock_start,
                b.rock_end,
                b.dirt_strength,
                b.beach_width,
            );
            gl::ActiveTexture(gl::TEXTURE3);
            gl::BindTexture(
                gl::TEXTURE_2D,
                self.get_texture(&landscape_texture_id(&materials.surface, "Surface")),
            );
            gl::ActiveTexture(gl::TEXTURE4);
            gl::BindTexture(
                gl::TEXTURE_2D,
                self.get_texture(&landscape_texture_id(&materials.subsurface, "Subsurface")),
            );
            gl::ActiveTexture(gl::TEXTURE5);
            gl::BindTexture(
                gl::TEXTURE_2D,
                self.get_texture(&landscape_texture_id(&materials.rock, "Rock")),
            );
            gl::ActiveTexture(gl::TEXTURE6);
            gl::BindTexture(
                gl::TEXTURE_2D,
                self.get_texture(
                    if world
                        .terrain
                        .config
                        .terrain_material_layers()
                        .iter()
                        .any(|l| l.id == "legacy-sand")
                    {
                        world.terrain.config.layer_material("legacy-sand")
                    } else {
                        "PBR:Sand"
                    },
                ),
            );
            gl::ActiveTexture(gl::TEXTURE11);
            gl::BindTexture(gl::TEXTURE_2D, self.get_texture("PBR:Snow"));
            gl::ActiveTexture(gl::TEXTURE12);
            gl::BindTexture(gl::TEXTURE_2D, self.get_texture("PBR:Mud"));
            gl::Uniform1f(
                self.grid_terrain_water_level_location,
                world.terrain.config.plains.water_level,
            );
            let ocean = world
                .terrain
                .config
                .resolved_water(&self.materials.borrow());
            let wet_height = if WET_SAND_MATERIAL_ENABLED
                && world.terrain.config.ocean_enabled
                && cache.ocean_contact_texture != 0
            {
                ocean.wave_height.max(0.0)
            } else {
                0.0
            };
            gl::Uniform4f(
                self.grid_ocean_wetness_location,
                wet_height,
                ocean.wave_speed,
                ocean.wave_length,
                crate::world::terrain::ocean_wave_time_seconds(),
            );
            gl::Uniform1f(
                self.grid_ocean_wetness_direction_location,
                ocean.wave_direction_degrees.to_radians(),
            );
            gl::Uniform1f(
                super::resources::get_uniform_location(self.grid_program, "u_ocean_wetness_seed"),
                world.terrain.config.plains.seed.rem_euclid(65536) as f32,
            );
            gl::ActiveTexture(gl::TEXTURE1);
            let mut last_texture: Option<u32> = None;
            if let Some(t) = state_started {
                stats.state_setup_ns = t.elapsed().as_nanos() as u64;
            }
            for (key, chunk) in &cache.chunks {
                if !terrain_chunk_visible(
                    result,
                    *key,
                    camera_frustum,
                    chunk_focus,
                    chunk_radius,
                    None,
                ) {
                    stats.chunks_culled += 1;
                    continue;
                }
                stats.chunks_drawn += 1;
                let page_origin = [
                    result.request.position[0] as f32 + key[0] as f32 * 64.0,
                    result.request.position[2] as f32 + key[1] as f32 * 64.0,
                ];
                let flags = self.paint_gpu.borrow().bind_chunk(*key, page_origin);
                gl::Uniform2f(
                    self.grid_paint_origin_location,
                    page_origin[0],
                    page_origin[1],
                );
                gl::Uniform1i(self.grid_paint_flags_location, flags as i32);

                for batch in &chunk.batches {
                    if matches!(batch.material, TerrainMaterialSlot::Water) {
                        continue;
                    }
                    let (texture, color, blend_role): (&str, [f32; 3], Option<i32>) =
                        match batch.material {
                            TerrainMaterialSlot::Surface => {
                                (materials.surface.as_str(), [1.0, 1.0, 1.0], Some(0))
                            }
                            TerrainMaterialSlot::Subsurface => {
                                (materials.subsurface.as_str(), [0.46, 0.31, 0.16], Some(1))
                            }
                            TerrainMaterialSlot::Rock => {
                                (materials.rock.as_str(), [0.48, 0.48, 0.46], Some(2))
                            }
                            TerrainMaterialSlot::Grass => {
                                (materials.surface.as_str(), [1.0, 1.0, 1.0], None)
                            }
                            TerrainMaterialSlot::Palette(index) => {
                                let name = match &result.data {
                                    TerrainData::Voxel(grid) => grid.material_name(index),
                                    TerrainData::Heightfield(_) => None,
                                }
                                .unwrap_or("");
                                let (texture, color) = terrain_palette_visual(name, materials);
                                (texture, color, terrain_blend_role(name, materials))
                            }
                            TerrainMaterialSlot::Water => unreachable!(),
                        };
                    let is_grass = matches!(batch.material, TerrainMaterialSlot::Grass);
                    unsafe {
                        gl::Uniform1i(wet_eligible, i32::from(!is_grass));
                    }
                    let vertex_layers = matches!(batch.material, TerrainMaterialSlot::Palette(_))
                        && matches!(result.data, TerrainData::Voxel(_));
                    gl::Uniform1i(
                        self.grid_terrain_vertex_layers_location,
                        i32::from(vertex_layers),
                    );
                    let default_layer = if vertex_layers {
                        0
                    } else if matches!(batch.material, TerrainMaterialSlot::Subsurface) {
                        1
                    } else if matches!(batch.material, TerrainMaterialSlot::Rock) {
                        2
                    } else {
                        terrain_palette_layer(texture)
                    };
                    gl::Uniform1i(
                        self.grid_terrain_default_layer_location,
                        default_layer as i32,
                    );
                    gl::Uniform1i(
                        self.grid_terrain_blend_enabled_location,
                        i32::from(vertex_layers || blend_role.is_some()),
                    );
                    gl::Uniform1i(self.grid_terrain_source_role_location, 0);
                    let draw_color = if vertex_layers || blend_role.is_some() {
                        [1.0, 1.0, 1.0]
                    } else {
                        color
                    };
                    gl::Uniform3f(
                        self.grid_base_color_location,
                        draw_color[0],
                        draw_color[1],
                        draw_color[2],
                    );
                    gl::Uniform1i(self.grid_use_texture_location, 1);
                    if is_grass {
                        gl::Disable(gl::CULL_FACE);
                    }
                    let started = profile_draws.then(std::time::Instant::now);
                    let resolved = self.get_texture(texture);
                    if let Some(started) = started {
                        stats.texture_lookup_ns = stats
                            .texture_lookup_ns
                            .saturating_add(started.elapsed().as_nanos() as u64);
                    }
                    gl::ActiveTexture(gl::TEXTURE1);
                    if super::resources::texture_binding_changed(&mut last_texture, resolved) {
                        gl::BindTexture(gl::TEXTURE_2D, resolved);
                    }
                    gl::BindVertexArray(batch.vao);
                    let started = profile_draws.then(std::time::Instant::now);
                    gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                    if !is_grass {
                        gl::Enable(gl::BLEND);
                        gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                        gl::DepthMask(gl::FALSE);
                        gl::DepthFunc(gl::EQUAL);
                        gl::Uniform3f(self.grid_base_color_location, 1.0, 1.0, 1.0);
                        if world.terrain.config.ocean_enabled {
                            gl::Uniform1i(layer_pass, 3);
                            gl::Uniform4f(
                                shore_params,
                                0.0,
                                world.terrain.config.plains.water_level,
                                world.terrain.config.material_blend.beach_width,
                                1.0,
                            );
                            let texture = self.get_texture(
                                world
                                    .terrain
                                    .config
                                    .layer_material(&world.terrain.config.shore_material_layer),
                            );
                            gl::ActiveTexture(gl::TEXTURE1);
                            gl::BindTexture(gl::TEXTURE_2D, texture);
                            gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                            stats.draws += 1;
                        }
                        for body in world
                            .terrain
                            .config
                            .water_bodies
                            .iter()
                            .filter(|b| b.visible)
                        {
                            let Some(count) =
                                self.paint_gpu.borrow().layers.bind_body(body, page_origin)
                            else {
                                continue;
                            };
                            gl::Uniform1i(layer_pass, 2);
                            gl::Uniform1i(shore_count, count as i32);
                            gl::Uniform4f(
                                shore_params,
                                if body.kind == crate::world::terrain::WaterBodyKind::River {
                                    1.0
                                } else {
                                    0.0
                                },
                                body.elevation,
                                body.bank_width,
                                1.0,
                            );
                            let texture = self.get_texture(
                                world
                                    .terrain
                                    .config
                                    .layer_material(&body.shore_material_layer),
                            );
                            gl::ActiveTexture(gl::TEXTURE1);
                            gl::BindTexture(gl::TEXTURE_2D, texture);
                            gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                            stats.draws += 1;
                        }
                        let active = self.paint_gpu.borrow().layers.active_layers(*key);
                        for id in active {
                            gl::Uniform1i(layer_pass, 1);
                            let flags = self.paint_gpu.borrow().layers.bind(*key, &id);
                            gl::Uniform1i(self.grid_paint_flags_location, flags as i32);
                            let texture =
                                self.get_texture(world.terrain.config.layer_material(&id));
                            gl::ActiveTexture(gl::TEXTURE1);
                            gl::BindTexture(gl::TEXTURE_2D, texture);
                            gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                            stats.draws += 1;
                        }
                        gl::Uniform1i(layer_pass, 0);
                        let flags = self.paint_gpu.borrow().bind_chunk(*key, page_origin);
                        gl::Uniform1i(self.grid_paint_flags_location, flags as i32);
                        gl::DepthMask(gl::TRUE);
                        gl::DepthFunc(gl::LESS);
                        gl::Disable(gl::BLEND);
                        last_texture = None;
                    }

                    if let Some(started) = started {
                        stats.draw_submission_ns = stats
                            .draw_submission_ns
                            .saturating_add(started.elapsed().as_nanos() as u64);
                    }
                    stats.draws += 1;
                    stats.triangles += batch.vertex_count as u64 / 3;
                    if is_grass {
                        gl::Enable(gl::CULL_FACE);
                    }
                }
            }

            gl::Uniform1i(wet_eligible, 0);
            gl::Uniform1i(self.grid_terrain_blend_enabled_location, 0);
            gl::Uniform1i(self.grid_terrain_vertex_layers_location, 0);
            gl::Uniform1i(self.grid_use_texture_location, 1);
            gl::ActiveTexture(gl::TEXTURE1);
            gl::Uniform3f(self.grid_base_color_location, 1.0, 1.0, 1.0);
        }
        stats
    }

    pub(super) fn draw_terrain_water(
        &self,
        world: &World,
        camera_position: Vec3,
        view_projection: Mat4,
        camera_underwater: bool,
        camera_frustum: &CameraFrustum,
        chunk_focus: Vec3,
        chunk_radius: i32,
    ) -> TerrainDrawStats {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(30, file!(), line!());

        self.ensure_terrain_cache(world);
        if self.water_program == 0 {
            return TerrainDrawStats::default();
        }
        let Some(result) = world.terrain.effective() else {
            return TerrainDrawStats::default();
        };
        if world.terrain.config.infinite_ocean && world.terrain.config.ocean_enabled {
            let mut cache = self.terrain_cache.borrow_mut();
            if let Some(cache) = cache.as_mut() {
                if cache.horizon.is_none() {
                    cache.horizon = build_infinite_ocean_horizon(result, &world.terrain.config);
                }
            }
        }
        let cache = self.terrain_cache.borrow();
        let Some(cache) = cache.as_ref() else {
            return TerrainDrawStats::default();
        };
        let mut stats = TerrainDrawStats::default();
        let layer_enabled_location = super::resources::get_uniform_location(
            self.water_program,
            "u_stars_water_layer_enabled",
        );
        let layer_texture_location = super::resources::get_uniform_location(
            self.water_program,
            "u_stars_water_layer_texture",
        );
        let wave_seed_location =
            super::resources::get_uniform_location(self.water_program, "u_wave_seed");
        let depth_only_location =
            super::resources::get_uniform_location(self.water_program, "u_depth_only");

        unsafe {
            gl::UseProgram(self.water_program);
            self.contact_foam.borrow().bind(self.water_program);
            gl::Uniform1i(
                super::resources::get_uniform_location(self.water_program, "u_horizon_pass"),
                0,
            );
            gl::Uniform1i(
                super::resources::get_uniform_location(self.water_program, "u_water_basic"),
                i32::from(
                    world.terrain.config.water_realism
                        == crate::world::terrain::WaterRealismMode::Basic,
                ),
            );
            let coast_loc = |name| super::resources::get_uniform_location(self.water_program, name);
            gl::Uniform1i(coast_loc("u_ocean_contact_atlas"), 13);
            gl::Uniform4f(
                coast_loc("u_ocean_contact_mapping"),
                cache.ocean_contact_mapping[0],
                cache.ocean_contact_mapping[1],
                cache.ocean_contact_mapping[2],
                cache.ocean_contact_mapping[3],
            );
            gl::ActiveTexture(gl::TEXTURE13);
            gl::BindTexture(gl::TEXTURE_2D, cache.ocean_contact_texture);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::Uniform1f(
                wave_seed_location,
                world.terrain.config.plains.seed.rem_euclid(65536) as f32,
            );
            self.bind_water_contact_uniforms(self.water_program);
            gl::Uniform1i(layer_texture_location, 14);
            let light = &world.lighting;
            let loc = |name| super::resources::get_uniform_location(self.water_program, name);
            gl::Uniform1f(loc("u_ambient_intensity"), light.ambient_intensity);
            gl::Uniform1i(
                loc("u_global_light_enabled"),
                i32::from(light.global_light_enabled),
            );
            gl::Uniform3f(
                loc("u_global_light_direction"),
                light.global_light_direction.x,
                light.global_light_direction.y,
                light.global_light_direction.z,
            );
            gl::Uniform3f(
                loc("u_global_light_color"),
                light.global_light_color.x,
                light.global_light_color.y,
                light.global_light_color.z,
            );
            gl::Uniform1f(
                loc("u_global_light_intensity"),
                light.global_light_intensity,
            );
            let matrix =
                self.calculate_light_space_matrix(light.global_light_direction, camera_position);
            gl::UniformMatrix4fv(
                loc("u_light_space_matrix"),
                1,
                gl::FALSE,
                matrix.to_cols_array().as_ptr(),
            );
            let shadows = light.shadows_enabled
                && self.active_lighting_settings().shadow_method
                    != super::settings::ShadowMethod::Off;
            gl::Uniform1i(loc("u_shadows_enabled"), i32::from(shadows));
            gl::Uniform1i(loc("u_shadow_map"), 15);
            gl::ActiveTexture(gl::TEXTURE15);
            gl::BindTexture(gl::TEXTURE_2D, self.shadow_depth_tex);
            gl::ActiveTexture(gl::TEXTURE0);
            let time = crate::world::terrain::ocean_wave_time_seconds();
            let events = self.water_wakes.borrow();
            let n = events.trails.len().min(12);
            let mut positions = [0.0f32; 96];
            let mut params = [0.0f32; 48];
            for (i, w) in events.trails.iter().rev().take(n).enumerate() {
                positions[i * 4..i * 4 + 4]
                    .copy_from_slice(&[w.start.x, w.start.y, w.end.x, w.end.y]);
                params[i * 2..i * 2 + 2].copy_from_slice(&[(time - w.born).max(0.0), w.power]);
            }
            gl::Uniform1i(
                super::resources::get_uniform_location(self.water_program, "u_wake_count"),
                n as i32,
            );
            gl::Uniform4fv(
                super::resources::get_uniform_location(self.water_program, "u_wake_positions[0]"),
                n as i32,
                positions.as_ptr(),
            );
            gl::Uniform2fv(
                super::resources::get_uniform_location(self.water_program, "u_wake_params[0]"),
                n as i32,
                params.as_ptr(),
            );
            gl::UniformMatrix4fv(
                self.water_view_projection_location,
                1,
                gl::FALSE,
                view_projection.to_cols_array().as_ptr(),
            );
            gl::Uniform3f(
                self.water_camera_position_location,
                camera_position.x,
                camera_position.y,
                camera_position.z,
            );
            gl::Uniform1f(
                self.water_time_location,
                crate::world::terrain::ocean_wave_time_seconds(),
            );
            gl::Uniform1i(
                self.water_camera_underwater_location,
                i32::from(camera_underwater),
            );
            let water = world
                .terrain
                .config
                .resolved_water(&self.materials.borrow());
            gl::Uniform1f(self.water_wave_height_location, water.wave_height);
            gl::Uniform1f(self.water_wave_speed_location, water.wave_speed);
            gl::Uniform1f(self.water_foam_amount_location, water.foam_amount);
            gl::Uniform1f(self.water_foam_scale_location, water.foam_scale);
            gl::Uniform3fv(
                self.water_shallow_color_location,
                1,
                water.shallow_color.as_ptr(),
            );
            gl::Uniform3fv(self.water_deep_color_location, 1, water.deep_color.as_ptr());
            gl::Uniform3fv(self.water_foam_color_location, 1, water.foam_color.as_ptr());
            gl::Uniform3fv(
                self.water_underwater_color_location,
                1,
                water.underwater_color.as_ptr(),
            );
            gl::Uniform1f(self.water_opacity_location, water.opacity);
            gl::Uniform1f(
                self.water_underwater_opacity_location,
                water.underwater_opacity,
            );
            gl::Uniform1f(
                self.water_reflection_strength_location,
                water.reflection_strength,
            );
            gl::Uniform1f(
                self.water_specular_strength_location,
                water.specular_strength,
            );
            let atmosphere_enabled = world.sky.atmosphere_enabled;
            let fog_start = world.sky.fog_start_studs.max(0.0);
            let fog_end = world.sky.fog_end_studs.max(fog_start + 1.0);
            let fog_color = world.sky.fog_color.clamp(Vec3::ZERO, Vec3::ONE);
            gl::Uniform1i(
                self.water_atmosphere_enabled_location,
                i32::from(atmosphere_enabled),
            );
            gl::Uniform3f(
                self.water_fog_color_location,
                fog_color.x,
                fog_color.y,
                fog_color.z,
            );
            gl::Uniform1f(self.water_fog_start_location, fog_start);
            gl::Uniform1f(self.water_fog_end_location, fog_end);
            gl::ActiveTexture(gl::TEXTURE0);
            gl::BindTexture(gl::TEXTURE_CUBE_MAP, self.get_cubemap(&world.sky.texture));

            gl::Enable(gl::BLEND);
            gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
            gl::Disable(gl::CULL_FACE);
            for depth_only in [true, false] {
                gl::Uniform1i(depth_only_location, i32::from(depth_only));
                if depth_only {
                    gl::Disable(gl::BLEND);
                    gl::ColorMask(gl::FALSE, gl::FALSE, gl::FALSE, gl::FALSE);
                    gl::DepthMask(gl::TRUE);
                    gl::DepthFunc(gl::LEQUAL);
                } else {
                    gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
                    gl::Enable(gl::BLEND);
                    gl::BlendFunc(gl::SRC_ALPHA, gl::ONE_MINUS_SRC_ALPHA);
                    gl::DepthMask(gl::FALSE);
                    gl::DepthFunc(gl::EQUAL);
                }
                for (key, chunk) in &cache.chunks {
                    if !terrain_chunk_visible(
                        result,
                        *key,
                        camera_frustum,
                        chunk_focus,
                        chunk_radius,
                        world
                            .terrain
                            .config
                            .water_bodies
                            .iter()
                            .filter(|body| body.visible)
                            .map(|body| {
                                body.maximum_surface_y()
                                    + world.terrain.config.water.wave_height.max(0.0)
                            })
                            .chain(world.terrain.config.ocean_enabled.then_some(
                                world.terrain.config.plains.water_level
                                    + world.terrain.config.water.wave_height.max(0.0),
                            ))
                            .reduce(f32::max),
                    ) {
                        stats.chunks_culled += 1;
                        continue;
                    }
                    stats.chunks_drawn += 1;
                    for batch in &chunk.batches {
                        if !matches!(batch.material, TerrainMaterialSlot::Water) {
                            continue;
                        }
                        let body = batch.water_body_id.and_then(|id| {
                            world
                                .terrain
                                .config
                                .water_bodies
                                .iter()
                                .find(|b| b.id == id)
                        });
                        let layer_material = body
                            .and_then(|b| b.material_asset.as_deref())
                            .or(world.terrain.config.water_material_asset.as_deref());
                        let overlay =
                            layer_material.and_then(|id| self.material_overlay_texture(id));
                        gl::Uniform1i(layer_enabled_location, i32::from(overlay.is_some()));
                        if let Some(texture) = overlay {
                            gl::ActiveTexture(gl::TEXTURE14);
                            gl::BindTexture(gl::TEXTURE_2D, texture);
                            gl::ActiveTexture(gl::TEXTURE0);
                        }
                        let look = body
                            .map(|b| b.resolved_appearance(&self.materials.borrow(), &water))
                            .unwrap_or(water);
                        gl::Uniform1i(
                            self.water_ocean_parity_wave_location,
                            i32::from(
                                batch.water_body_id.is_none() && world.terrain.config.ocean_enabled,
                            ),
                        );
                        gl::Uniform1f(self.water_wave_height_location, look.wave_height);
                        gl::Uniform1f(self.water_wave_speed_location, look.wave_speed);
                        gl::Uniform1f(self.water_wave_length_location, look.wave_length);
                        gl::Uniform1f(
                            self.water_wave_direction_location,
                            look.wave_direction_degrees.to_radians(),
                        );
                        gl::Uniform1f(self.water_wave_steepness_location, look.wave_steepness);
                        gl::Uniform3fv(
                            self.water_absorption_color_location,
                            1,
                            look.absorption_color.as_ptr(),
                        );
                        gl::Uniform1f(
                            self.water_absorption_strength_location,
                            look.absorption_strength,
                        );
                        gl::Uniform3fv(
                            self.water_scattering_color_location,
                            1,
                            look.scattering_color.as_ptr(),
                        );
                        gl::Uniform1f(
                            self.water_scattering_strength_location,
                            look.scattering_strength,
                        );
                        gl::Uniform1f(self.water_foam_amount_location, look.foam_amount);
                        gl::Uniform1f(self.water_foam_scale_location, look.foam_scale);
                        gl::Uniform3fv(
                            self.water_shallow_color_location,
                            1,
                            look.shallow_color.as_ptr(),
                        );
                        gl::Uniform3fv(self.water_deep_color_location, 1, look.deep_color.as_ptr());
                        gl::Uniform3fv(self.water_foam_color_location, 1, look.foam_color.as_ptr());
                        gl::Uniform3fv(
                            self.water_underwater_color_location,
                            1,
                            look.underwater_color.as_ptr(),
                        );
                        gl::Uniform3fv(
                            super::resources::get_uniform_location(
                                self.water_program,
                                "u_underwater_post_color",
                            ),
                            1,
                            look.underwater_fog_color.as_ptr(),
                        );
                        gl::Uniform1f(self.water_opacity_location, look.opacity);
                        gl::Uniform1f(
                            self.water_underwater_opacity_location,
                            look.underwater_opacity,
                        );
                        gl::Uniform1f(
                            self.water_reflection_strength_location,
                            look.reflection_strength,
                        );
                        gl::Uniform1f(
                            self.water_specular_strength_location,
                            look.specular_strength,
                        );
                        gl::Uniform1i(
                            self.water_local_flow_location,
                            i32::from(body.is_some_and(|b| {
                                b.kind == crate::world::terrain::WaterBodyKind::River
                            })),
                        );
                        gl::Uniform1i(
                            self.water_camera_underwater_location,
                            i32::from(body.map_or(camera_underwater, |b| {
                                let surface = b.surface_y_at(camera_position.x, camera_position.z);
                                camera_position.y < surface
                                    && b.signed_coverage(camera_position.x, camera_position.z) > 0.0
                            })),
                        );
                        gl::BindVertexArray(batch.vao);
                        gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                        stats.draws += 1;
                        stats.triangles += batch.vertex_count as u64 / 3;
                    }
                }
                if let Some(horizon) = cache.horizon.as_ref() {
                    gl::Uniform1i(loc("u_horizon_pass"), 1);
                    gl::Uniform1i(self.water_ocean_parity_wave_location, 1);
                    gl::Uniform1i(self.water_local_flow_location, 0);
                    gl::Uniform2f(
                        loc("u_horizon_center"),
                        world.terrain.config.plains.water_level * 0.0
                            + result.request.position[0] as f32
                            + result.request.size[0] as f32 * 0.5,
                        result.request.position[2] as f32 + result.request.size[2] as f32 * 0.5,
                    );
                    gl::Uniform1f(loc("u_horizon_fade"), 3600.0);
                    gl::Uniform1f(self.water_wave_height_location, water.wave_height);
                    gl::Uniform1f(self.water_wave_speed_location, water.wave_speed);
                    gl::Uniform1f(self.water_wave_length_location, water.wave_length);
                    gl::Uniform1f(
                        self.water_wave_direction_location,
                        water.wave_direction_degrees.to_radians(),
                    );
                    gl::Uniform1f(self.water_wave_steepness_location, water.wave_steepness);
                    gl::Uniform3fv(self.water_deep_color_location, 1, water.deep_color.as_ptr());
                    gl::Uniform1f(self.water_opacity_location, water.opacity);
                    for batch in &horizon.batches {
                        gl::BindVertexArray(batch.vao);
                        gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                        stats.draws += 1;
                        stats.triangles += batch.vertex_count as u64 / 3;
                    }
                    gl::Uniform1i(loc("u_horizon_pass"), 0);
                }
            }
            gl::ColorMask(gl::TRUE, gl::TRUE, gl::TRUE, gl::TRUE);
            gl::DepthFunc(gl::LESS);
            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::CULL_FACE);
            gl::UseProgram(self.grid_program);
        }
        stats
    }
}

pub(super) const WATER_VERTEX_SHADER: &str = r#"
#version 330 core

layout (location = 0) in vec3 a_position;
layout (location = 2) in vec4 a_color;
layout (location = 3) in vec2 a_uv;

uniform bool u_local_flow;
uniform bool u_ocean_parity_wave;
uniform bool u_water_basic;
uniform mat4 u_view_projection;
uniform float u_time;
uniform float u_wave_height;
uniform float u_wave_speed;
uniform float u_wave_length;
uniform float u_wave_direction;
uniform float u_wave_seed;
uniform float u_wave_steepness;
uniform int u_wake_count;
uniform vec4 u_wake_positions[24];
uniform vec2 u_wake_params[24];
uniform int u_contact_count;
uniform vec4 u_contact_bounds[24];
uniform vec4 u_contact_props[24];
vec2 wave_coordinates(vec2 p) {
    float c=cos(u_wave_direction);
    float s=sin(u_wave_direction);
    return vec2(c*p.x-s*p.y,s*p.x+c*p.y)*(50.0/max(u_wave_length,1.0));
}

// WK-WAVES-008: coherent world-space wave field; no per-frame random jitter.
// Each component travels, with seeded but fixed headings and wavelengths.
float wave_seed_hash(float x) {
    return fract(sin(x*17.197+13.123)*43758.5453);
}
float ocean_primary_phase(vec2 p,float t) {
    vec2 d=vec2(cos(u_wave_direction),sin(u_wave_direction));
    return -dot(p,d)*(6.28318530718/max(u_wave_length,1.0))+t*u_wave_speed;
}
float ocean_wave_field(vec2 p,float t) {
    float k=6.28318530718/max(u_wave_length,1.0);
    float seed=wave_seed_hash(u_wave_seed+0.123);
    float a=ocean_primary_phase(p,t);
    float angle2=u_wave_direction+0.43+seed*0.47;
    float angle3=u_wave_direction-0.91+wave_seed_hash(u_wave_seed+7.0)*0.39;
    float b=-dot(p,vec2(cos(angle2),sin(angle2)))*k*1.71+
        t*u_wave_speed*1.18+seed*6.28318530718;
    float c=-dot(p,vec2(cos(angle3),sin(angle3)))*k*0.58+
        t*u_wave_speed*0.74+wave_seed_hash(u_wave_seed+5.0)*6.28318530718;
    return sin(a)*0.72+sin(b)*0.19+sin(c)*0.09;
}
vec2 ocean_wave_slope(vec2 p,float t) {
    float k=6.28318530718/max(u_wave_length,1.0);
    float seed=wave_seed_hash(u_wave_seed+0.123);
    float a=ocean_primary_phase(p,t);
    vec2 d=vec2(cos(u_wave_direction),sin(u_wave_direction));
    float angle2=u_wave_direction+0.43+seed*0.47;
    float angle3=u_wave_direction-0.91+wave_seed_hash(u_wave_seed+7.0)*0.39;
    vec2 d2=vec2(cos(angle2),sin(angle2));
    vec2 d3=vec2(cos(angle3),sin(angle3));
    float b=-dot(p,d2)*k*1.71+t*u_wave_speed*1.18+seed*6.28318530718;
    float c=-dot(p,d3)*k*0.58+t*u_wave_speed*0.74+
        wave_seed_hash(u_wave_seed+5.0)*6.28318530718;
    return -k*(d*0.72*cos(a)+d2*1.71*0.19*cos(b)+
        d3*0.58*0.09*cos(c));
}

out vec3 v_world_pos;
out float v_foam;
out float v_depth;
out float v_wave_energy;
out vec2 v_flow;
out float v_swash;
out float v_swash_elevation;

float wave_height(vec2 p, float t, float depth, float wave_energy) {
    float shore_scale = mix(0.08, 1.0, smoothstep(0.03, 0.48, depth));
    float energy = clamp(wave_energy, 0.05, 1.0);
    // Keep shore water almost flat while retaining open-water movement.
    float geometry_energy = mix(0.35, 1.0, energy);
    float scaled_time = t * u_wave_speed;
    p=wave_coordinates(p);
    float w1 = sin(dot(p, vec2(0.115, 0.072)) + scaled_time * 0.90) * 0.18;
    float w2 = sin(dot(p, vec2(-0.083, 0.145)) + scaled_time * 1.27 + 1.7) * 0.09;
    float w3 = sin(dot(p, vec2(0.215, -0.047)) + scaled_time * 0.54 + 3.1) * 0.045;
    return (w1 + w2 + w3) * shore_scale * geometry_energy * u_wave_height;
}

void main() {
    v_foam = a_color.r;
    v_depth = a_color.g;
    v_wave_energy = a_color.b;
    vec3 world = a_position;
    // Flat, static coastline geometry moves gently as one ocean plane.
    // The exact same 0.15*sin(time*0.013) tide runs in CPU buoyancy/contact.
    if (u_ocean_parity_wave) world.y+=0.15*sin(u_time*0.013);
    v_flow = u_local_flow ? a_uv : vec2(0.0);
    // Transport wave phase with the local current. Ocean/lakes stay
    // independent of river flow and retain their existing wave behavior.
    vec2 wave_p=world.xz-v_flow*u_time;
    v_swash=a_color.a>1.5 ? 1.0:0.0;
    v_swash_elevation=a_uv.x;
    if (u_water_basic) {
        // One cheap non-collidable wave, no spray or shoreline ghost quads.
        if (v_swash>0.5) world.y=-100000.0;
        else world.y+=sin(ocean_primary_phase(world.xz,u_time))*
                     u_wave_height*0.16*smoothstep(0.0,0.20,v_depth);
        v_world_pos=world;
        gl_Position=u_view_projection*vec4(world,1.0);
        return;
    }
    if (v_swash>0.5) {
        // Preserve terrain-projected geometry. Evaluate the moving front
        // PER FRAGMENT; a narrow crest cannot disappear between vertices.
    } else if (u_ocean_parity_wave) {
        // Keep a real crest at the beach: zeroing displacement at the last
        // shallow vertices creates the ruler-straight black waterline.
        // The clipped ocean mesh still defines the actual coastline.
        float shoreline_fade=mix(0.38,1.0,smoothstep(0.0,0.28,v_depth));
        // Three seeded coherent waves: broad main swell + two smaller,
        // non-parallel wave bands. Zero amplitude leaves flat water.
        world.y+=ocean_wave_field(world.xz,u_time)*u_wave_height*shoreline_fade;
        if (u_wave_steepness>0.0) {
            vec2 direction=vec2(cos(u_wave_direction),sin(u_wave_direction));
            float phase=ocean_primary_phase(world.xz,u_time);
            float k=6.28318530718/max(u_wave_length,1.0);
            float sideways=min(max(u_wave_steepness,0.0)*
                abs(u_wave_height)*0.18,0.75/k);
            world.xz+=direction*cos(phase)*sideways*shoreline_fade;
        }
    } else {
        world.y += wave_height(wave_p, u_time, v_depth, v_wave_energy);
    }
    // Optional Gerstner lateral displacement. Zero-parameter legacy projects
    // retain their original surface and all chunks displace deterministically.
    if (v_swash<0.5 && !u_ocean_parity_wave && u_wave_steepness>0.0) {
        vec2 p=wave_coordinates(wave_p);
        float phase=dot(p,vec2(0.115,0.072))+u_time*u_wave_speed*0.90;
        float shore=smoothstep(0.03,0.48,v_depth);
        float c=cos(u_wave_direction);
        float s=sin(u_wave_direction);
        vec2 gerstner_dir=normalize(vec2(c*0.115+s*0.072,-s*0.115+c*0.072));
        world.xz += gerstner_dir *
            cos(phase)*u_wave_steepness*u_wave_height*0.18*shore*v_wave_energy;
    }
    // WK-WAVES-009: real movement segments share endpoints through turns.
    // Distance to the segment, never a new direction-oriented V at each
    // sample. Raising the displacement is intentional, not random noise.
    float local_wake=0.0;
    for(int i=0;i<12;++i){
        if(i>=u_wake_count)break;
        vec4 wake=u_wake_positions[i]; // XY start, ZW end
        vec2 segment=wake.zw-wake.xy;
        float len2=dot(segment,segment);
        float u=clamp(dot(world.xz-wake.xy,segment)/max(len2,0.0001),
                      0.0,1.0);
        float distance=length(world.xz-mix(wake.xy,wake.zw,u));
        float age=u_wake_params[i].x;
        float power=u_wake_params[i].y;
        float fade=1.0-smoothstep(0.1,3.8,age);
        bool impact=len2<0.020;
        float ring=abs(length(world.xz-wake.zw)-age*2.8);
        float ridge=impact ?
            1.0-smoothstep(0.10,0.85,ring) :
            1.0-smoothstep(0.14,1.0+age*0.14,distance);
        float travel=impact ? length(world.xz-wake.zw) :
            distance+u*sqrt(len2)*0.3;
        local_wake+=ridge*sin(travel*5.4-age*7.8)*fade*power;
    }
    // Displace surface OUTSIDE contacting colliders; avoid plane-only foam.
    float contact_shift=0.0;
    for(int i=0;i<8;++i){
        if(i>=u_contact_count)break;
        vec4 b=u_contact_bounds[i];
        vec2 delta=max(max(b.xy-world.xz,world.xz-b.zw),vec2(0.0));
        float distance=length(delta);
        float age=u_contact_props[i].w;
        if(age>=0.0 && age<0.3 && abs(world.y-u_contact_props[i].x)<0.8)
            contact_shift+=exp(-distance*3.1)*0.09;
    }
    if(v_swash<0.5)
        world.y+=clamp(local_wake,-1.0,1.0)*0.24+min(contact_shift,0.18);
    v_world_pos = world;
    gl_Position = u_view_projection * vec4(world, 1.0);
}
"#;

pub(super) const WATER_FRAGMENT_SHADER: &str = r#"
#version 330 core
uniform bool u_depth_only;
uniform bool u_water_basic;
uniform bool u_horizon_pass;
uniform vec2 u_horizon_center;
uniform float u_horizon_fade;

in vec3 v_world_pos;
in float v_foam;
in float v_depth;
in float v_wave_energy;
in vec2 v_flow;
in float v_swash;
in float v_swash_elevation;

uniform vec3 u_camera_position;
uniform float u_ambient_intensity;
uniform bool u_global_light_enabled;
uniform vec3 u_global_light_direction;
uniform vec3 u_global_light_color;
uniform float u_global_light_intensity;
uniform bool u_shadows_enabled;
uniform sampler2D u_shadow_map;
uniform mat4 u_light_space_matrix;
uniform float u_time;
uniform bool u_camera_underwater;
uniform bool u_ocean_parity_wave;
uniform float u_wave_height;
uniform float u_wave_speed;
uniform float u_wave_length;
uniform float u_wave_direction;
uniform float u_wave_seed;
uniform float u_absorption_strength;
uniform vec3 u_absorption_color;
uniform float u_scattering_strength;
uniform vec3 u_scattering_color;
vec2 wave_coordinates(vec2 p) {
    float c=cos(u_wave_direction);
    float s=sin(u_wave_direction);
    return vec2(c*p.x-s*p.y,s*p.x+c*p.y)*(50.0/max(u_wave_length,1.0));
}
uniform float u_foam_amount;
uniform float u_foam_scale;
uniform int u_wake_count;
uniform vec4 u_wake_positions[24];
uniform vec2 u_wake_params[24];
uniform int u_contact_count;
uniform vec4 u_contact_bounds[24];
uniform vec4 u_contact_props[24];
uniform vec3 u_shallow_color;
uniform vec3 u_deep_color;
uniform vec3 u_foam_color;
uniform vec3 u_underwater_color;
uniform vec3 u_underwater_post_color;
uniform float u_opacity;
uniform float u_underwater_opacity;
uniform float u_reflection_strength;
uniform float u_specular_strength;
uniform samplerCube u_sky;
uniform sampler2D u_ocean_contact_atlas;
uniform vec4 u_ocean_contact_mapping;
uniform bool u_stars_water_layer_enabled;
uniform sampler2D u_stars_water_layer_texture;
uniform bool u_atmosphere_enabled;
uniform vec3 u_fog_color;
uniform float u_fog_start;
uniform float u_fog_end;

out vec4 FragColor;

// WK-WAVES-008: coherent world-space wave field; no per-frame random jitter.
// Each component travels, with seeded but fixed headings and wavelengths.
float wave_seed_hash(float x) {
    return fract(sin(x*17.197+13.123)*43758.5453);
}
float ocean_primary_phase(vec2 p,float t) {
    vec2 d=vec2(cos(u_wave_direction),sin(u_wave_direction));
    return -dot(p,d)*(6.28318530718/max(u_wave_length,1.0))+t*u_wave_speed;
}
float ocean_wave_field(vec2 p,float t) {
    float k=6.28318530718/max(u_wave_length,1.0);
    float seed=wave_seed_hash(u_wave_seed+0.123);
    float a=ocean_primary_phase(p,t);
    float angle2=u_wave_direction+0.43+seed*0.47;
    float angle3=u_wave_direction-0.91+wave_seed_hash(u_wave_seed+7.0)*0.39;
    float b=-dot(p,vec2(cos(angle2),sin(angle2)))*k*1.71+
        t*u_wave_speed*1.18+seed*6.28318530718;
    float c=-dot(p,vec2(cos(angle3),sin(angle3)))*k*0.58+
        t*u_wave_speed*0.74+wave_seed_hash(u_wave_seed+5.0)*6.28318530718;
    return sin(a)*0.72+sin(b)*0.19+sin(c)*0.09;
}
vec2 ocean_wave_slope(vec2 p,float t) {
    float k=6.28318530718/max(u_wave_length,1.0);
    float seed=wave_seed_hash(u_wave_seed+0.123);
    float a=ocean_primary_phase(p,t);
    vec2 d=vec2(cos(u_wave_direction),sin(u_wave_direction));
    float angle2=u_wave_direction+0.43+seed*0.47;
    float angle3=u_wave_direction-0.91+wave_seed_hash(u_wave_seed+7.0)*0.39;
    vec2 d2=vec2(cos(angle2),sin(angle2));
    vec2 d3=vec2(cos(angle3),sin(angle3));
    float b=-dot(p,d2)*k*1.71+t*u_wave_speed*1.18+seed*6.28318530718;
    float c=-dot(p,d3)*k*0.58+t*u_wave_speed*0.74+
        wave_seed_hash(u_wave_seed+5.0)*6.28318530718;
    return -k*(d*0.72*cos(a)+d2*1.71*0.19*cos(b)+
        d3*0.58*0.09*cos(c));
}

// WK-WAVES-008: seeded, advected, irregular foam structure. Hash is
// multiplication-only to avoid eight extra trigonometric functions per pixel.
float foam_hash(vec2 p){
    vec3 v=fract(vec3(p.xyx)*0.1031);
    v+=dot(v,v.yzx+33.33);
    return fract((v.x+v.y)*v.z);
}
float foam_noise(vec2 p) {
    vec2 cell=floor(p);
    vec2 f=fract(p);
    f=f*f*(3.0-2.0*f);
    float a=foam_hash(cell);
    float b=foam_hash(cell+vec2(1.0,0.0));
    float c=foam_hash(cell+vec2(0.0,1.0));
    float d=foam_hash(cell+vec2(1.0,1.0));
    return mix(mix(a,b,f.x),mix(c,d,f.x),f.y);
}

// WK_OBJECT_CONTACT_FOAM_FUNCTION

vec3 water_normal(vec2 p, float t, float depth, float wave_energy) {
    float shore_scale = mix(0.10, 1.0, smoothstep(0.03, 0.42, depth));
    float energy = clamp(wave_energy, 0.05, 1.0);
    // WO 0.9.3-15: keep two dominant normal waves. The third micro-wave was
    // visually subtle but added two more trig evaluations per water fragment.
    float scaled_time = t * u_wave_speed;
    p=wave_coordinates(p);
    float p1 = dot(p, vec2(0.115, 0.072)) + scaled_time * 0.90;
    float p2 = dot(p, vec2(-0.083, 0.145)) + scaled_time * 1.27 + 1.7;
    float c1 = cos(p1);
    float c2 = cos(p2);
    float dx = shore_scale * energy * (
        c1 * 0.18 * 0.115 +
        c2 * 0.09 * -0.083
    );
    float dz = shore_scale * energy * (
        c1 * 0.18 * 0.072 +
        c2 * 0.09 * 0.145
    );
    // Transform normal slopes back from user-rotated/scaled wave space.
    float c=cos(u_wave_direction);
    float s=sin(u_wave_direction);
    vec2 slope=vec2(c*dx+s*dz,-s*dx+c*dz) *
        (50.0/max(u_wave_length,1.0));
    return normalize(vec3(-slope.x * u_wave_height, 1.0, -slope.y * u_wave_height));
}

void main() {
    // Artificial horizon uses only the existing 3-band wave displacement,
    // a single inexpensive color/ambient term, and no foam/contacts/shadows.
    if (u_horizon_pass) {
        if(u_depth_only){ FragColor=vec4(0.0); return; }
        // Far-field uses identical seeded three-band waves, but cheaper
        // per-pixel normals, one sky fetch and ONE low-frequency foam-noise
        // sample. No wake, contact, foam-history or shadow loops.
        vec2 slope=ocean_wave_slope(v_world_pos.xz,u_time);
        vec3 ocean_wave_normal=normalize(vec3(
            -slope.x*u_wave_height,1.0,-slope.y*u_wave_height));
        vec3 view_dir=normalize(u_camera_position-v_world_pos);
        vec3 eye_normal=gl_FrontFacing ?
            ocean_wave_normal : -ocean_wave_normal;
        float facing=clamp(abs(dot(eye_normal,view_dir)),0.0,1.0);
        float horizon_fresnel=0.045+0.30*pow(1.0-facing,4.0);
        vec3 sky_light=texture(u_sky,
            reflect(-view_dir,eye_normal)).rgb;
        float sun= u_global_light_enabled ?
            max(dot(ocean_wave_normal,
                normalize(-u_global_light_direction)),0.0) : 0.0;
        vec3 far_color=u_deep_color*(
            0.65+u_ambient_intensity*0.28)+
            sky_light*horizon_fresnel*clamp(u_reflection_strength,0.0,1.0)+
            u_global_light_color*sun*0.11*
                clamp(u_global_light_intensity,0.0,2.0);
        // Local breaking persists all the way to the horizon:
        // phase is continuous with the authored surface, with a much
        // cheaper irregular foam edge than the full shoreline shader.
        float steepness=max(u_wave_height,0.0)*
            (6.28318530718/max(u_wave_length,1.0));
        float breaking=smoothstep(0.065,0.36,steepness);
        float crest=pow(0.5+0.5*cos(
            ocean_primary_phase(v_world_pos.xz,u_time)+
            6.28318530718*0.75),3.0);
        vec2 drift=v_world_pos.xz*max(u_foam_scale,0.25)*0.35-
            vec2(cos(u_wave_direction),sin(u_wave_direction))*
                u_time*u_wave_speed*0.11;
        float noise=foam_noise(drift+u_wave_seed*0.0029);
        float breakup=smoothstep(0.37,0.69,noise);
        float horizon_foam=clamp(crest*breakup*breaking*
            u_foam_amount*0.84,0.0,0.97);
        // Gently reduce the expensive highlights, never erase foam.
        float toward_horizon=clamp(
            length(v_world_pos.xz-u_horizon_center)/
            max(u_horizon_fade,1.0),0.0,1.0);
        far_color=mix(far_color,
            u_fog_color,toward_horizon*toward_horizon*0.14);
        far_color=mix(far_color,u_foam_color,
            horizon_foam*(1.0-0.35*toward_horizon));
        FragColor=vec4(far_color,clamp(u_opacity,0.0,1.0));
        return;
    }
    if (u_water_basic) {
        if(v_swash>0.5) discard;
        if(u_depth_only){ FragColor=vec4(0.0); return; }
        // Independent depth-derived contact: any opaque terrain/Cell/Actor
        // intersection, no coast vertices or object footprint approximations.
        float contact=wk_object_contact_foam();
        vec3 water=mix(u_shallow_color,u_deep_color,smoothstep(0.06,0.9,v_depth));
        water=mix(water,u_foam_color,clamp(contact*0.82,0.0,0.85));
        FragColor=vec4(water,clamp(u_opacity,0.0,1.0));
        return;
    }
    // The nearest-depth pass only needs the rasterized position. Color is
    // masked off; skip all shading, contact loops, lighting and reflections.
    // WK-WAVES-009 review: potential runup triangles are still rendered in
    // the full ocean batch. Clip/fade their opaque color at the ACTUAL coast;
    // only the clear, foam-driven swash may pass beyond the still-water line.
    float ocean_coast_alpha=1.0;
    if (u_ocean_parity_wave && v_swash<0.5 &&
        u_ocean_contact_mapping.z>0.0) {
        vec2 uv=(v_world_pos.xz-u_ocean_contact_mapping.xy)*
                 u_ocean_contact_mapping.zw;
        float coast=(texture(u_ocean_contact_atlas,
            clamp(uv,vec2(0.0),vec2(1.0))).r-0.5)*6.0;
        ocean_coast_alpha=smoothstep(-0.20,0.10,coast);
        if(ocean_coast_alpha<0.05) discard;
    }
    // Test moving swash coverage before BOTH water passes so depth and
    // color agree, including pixel-clipped geometry transitions.
    float swash_alpha=1.0;
    if (v_swash>0.5) {
        float swell=clamp(0.5+0.5*ocean_wave_field(v_world_pos.xz,u_time),0.0,1.0);
        float reach=min(max(u_wave_height,0.0)*0.62,5.0);
        float front=reach*swell;
        float feather=max(fwidth(v_swash_elevation)*1.35,0.16);
        swash_alpha=(1.0-smoothstep(front-feather,front+feather,
                                   v_swash_elevation))*
                    smoothstep(0.0,0.12,u_wave_height);
        // Maximum wave-envelope quads are deliberately oversized. The real
        // waterline is the signed-distance atlas (0.5 = coast); fade wash
        // from that curve, not at square mesh/terrain chunk boundaries.
        if (u_ocean_contact_mapping.z>0.0) {
            vec2 coast_uv=(v_world_pos.xz-u_ocean_contact_mapping.xy)*
                          u_ocean_contact_mapping.zw;
            float signed_coast=(texture(u_ocean_contact_atlas,
                clamp(coast_uv,vec2(0.0),vec2(1.0))).r-0.5)*6.0;
            float reach_studs=min(3.5,1.35+max(u_wave_height,0.0)*0.35);
            float irregular=(foam_noise(v_world_pos.xz*0.62)-0.5)*0.36;
            swash_alpha*=smoothstep(-reach_studs,-0.1,signed_coast+irregular);
            if(signed_coast>0.22) discard;
        }
        // This IS the water-air surface: coverage decides where the wave
        // exists, not an artificially transparent film over sand.
        if (swash_alpha<0.30) discard;
    }
    if (u_depth_only) {
        FragColor = vec4(0.0);
        return;
    }
    // Wave/foam advection uses the horizontal plane; v_world_pos retains 3D height.
    vec2 flow_p = v_world_pos.xz - v_flow * u_time;
    // Compute exactly one normal path per fragment. Ocean and shallow wash
    // must not also evaluate the lake/river normal trigonometry.
    vec3 n;
    if (v_swash>0.5) {
        n=vec3(0.0,1.0,0.0);
    } else if(u_ocean_parity_wave) {
        float shore=mix(0.38,1.0,smoothstep(0.0,0.28,v_depth));
        vec2 slope=ocean_wave_slope(v_world_pos.xz,u_time)*
            u_wave_height*shore;
        n=normalize(vec3(-slope.x,1.0,-slope.y));
    } else {
        n=water_normal(flow_p,u_time,v_depth,v_wave_energy);
    }
    if (!gl_FrontFacing) {
        n = -n;
    }

    vec3 view_dir = normalize(u_camera_position - v_world_pos);
    float facing = clamp(abs(dot(n, view_dir)), 0.0, 1.0);
    float one_minus_facing = 1.0 - facing;
    float fresnel5 = one_minus_facing * one_minus_facing;
    fresnel5 *= fresnel5 * one_minus_facing;
    float fresnel = 0.04 + 0.96 * fresnel5;

    vec3 reflected = texture(u_sky, reflect(-view_dir, n)).rgb;

    vec3 shallow = u_shallow_color;
    vec3 deep = u_deep_color;
    float depth_mix = smoothstep(0.08, 0.82, v_depth);
    vec3 body = mix(shallow, deep, depth_mix);
    // Depth-dependent absorption/scattering, disabled by default for
    // migration-safe old projects. Each water batch has its own uniforms.
    float absorption=1.0-exp(-u_absorption_strength*max(v_depth,0.0));
    body=mix(body,u_absorption_color,clamp(absorption,0.0,1.0));
    float scatter=1.0-exp(-u_scattering_strength*(1.0-depth_mix));
    body=mix(body,u_scattering_color,clamp(scatter,0.0,1.0));

    // WK-WAVES-008: foam forms at propagating crest/breaking fronts,
    // not a white material painted wherever the static shore mask exists.
    // Seeded coherent noise drifts with surface flow and moving crests.
    vec2 travel=vec2(cos(u_wave_direction),sin(u_wave_direction));
    float base_phase=ocean_primary_phase(flow_p,u_time);
    float coastal=clamp(v_foam,0.0,1.0);
    float steepness=max(0.0,u_wave_height)*
        (6.28318530718/max(u_wave_length,1.0));
    float breaking=smoothstep(0.07,0.48,steepness*(0.65+coastal*3.6));
    // WK-WAVES-014: fract(phase) creates a discontinuity every wavelength.
    // A periodic crest envelope is smooth across the full 2π phase wrap.
    float crest_phase=base_phase+6.28318530718*0.75;
    float crest_pulse=0.5+0.5*cos(crest_phase);
    float decay=pow(max(crest_pulse,0.0),1.6+coastal*1.6);
    vec2 drift=flow_p*max(u_foam_scale,0.25)*0.31 -
        travel*u_time*u_wave_speed*0.13;
    float seed=u_wave_seed*0.0137;
    float detail=foam_noise(drift+vec2(seed,-seed))*0.65+
        foam_noise(drift*2.37+vec2(-seed*0.4,seed*1.3))*0.35;
    float patches=smoothstep(0.34,0.73,detail+breaking*0.19);
    float whitecaps=breaking*decay*patches*(0.12+coastal*0.88);
    // The shoreline atlas is a continuous signed distance measured in
    // world-space studs (0 at the coast, positive into the water). Build
    // foam per FRAGMENT rather than using rectangular water-cell colors.
    // The wave/foam noise breaks up the edge without extra shoreline mesh.
    float shore_foam=0.0;
    if(u_ocean_parity_wave && u_ocean_contact_mapping.z>0.0 && v_swash<0.5) {
        vec2 coast_uv=(v_world_pos.xz-u_ocean_contact_mapping.xy)*
                      u_ocean_contact_mapping.zw;
        if(all(greaterThanEqual(coast_uv,vec2(0.0))) &&
           all(lessThanEqual(coast_uv,vec2(1.0)))) {
            float coast=(texture(u_ocean_contact_atlas,coast_uv).r-0.5)*6.0;
            float micro=(detail-0.5)*0.50;
            float rim=1.0-smoothstep(0.08,1.65,coast+micro);
            float water_side=smoothstep(-0.20,0.20,coast);
            float pulse=0.48+0.52*crest_pulse;
            shore_foam=rim*water_side*pulse*
                smoothstep(0.24,0.66,detail+0.14);
        }
    }
    // Thin beach wash leaves a frothy irregular leading edge when enabled.
    float wash_front=v_swash>0.5 ?
        4.0*swash_alpha*(1.0-swash_alpha)*patches : 0.0;
    float foam=clamp((whitecaps+shore_foam+wash_front*0.85)*u_foam_amount,0.0,1.0);
    // WK-WAVES-009: foam is laid along the SAME sampled movement segments
    // as surface displacement. Connected positions curve with player turns;
    // blending overlaps via max prevents bright seams and stamped V shapes.
    float wake_ripple=0.0;
    float wake_foam=0.0;
    for(int i=0;i<12;++i){
        if(i>=u_wake_count)break;
        vec4 w=u_wake_positions[i];
        vec2 segment=w.zw-w.xy;
        float len2=dot(segment,segment);
        float u=clamp(dot(v_world_pos.xz-w.xy,segment)/max(len2,0.0001),
                      0.0,1.0);
        vec2 nearest=mix(w.xy,w.zw,u);
        float distance=length(v_world_pos.xz-nearest);
        float age=u_wake_params[i].x;
        float power=u_wake_params[i].y;
        float fade=1.0-smoothstep(0.1,3.8,age);
        bool impact=len2<0.020;
        float ring=abs(length(v_world_pos.xz-w.zw)-age*2.8);
        float ripple=impact ? (1.0-smoothstep(0.10,0.85,ring)) :
            (1.0-smoothstep(0.10,1.10+age*0.14,distance));
        // No freshly stamped V at the direction change: continuous ribbon
        // and shoulder foam follow the bent path, then trail off.
        float shoulder=1.0-smoothstep(0.0,0.30,
            abs(distance-(0.40+power*0.24)));
        float ribbon=1.0-smoothstep(0.15,0.92+power*0.15,distance);
        float foam_shape=impact ? ripple :
            (ribbon*0.50+shoulder*0.42);
        wake_ripple=max(wake_ripple,ripple*fade*power);
        wake_foam=max(wake_foam,foam_shape*fade*power);
    }
    // Legacy approximate contact rectangles removed from the active shader.
    // Their wake/ripple effects remain independent and unchanged.
    float edge_foam=0.0;
    float breakup=mix(0.50,1.0,detail);
    foam=clamp(foam+min(wake_foam,1.0)*0.90*breakup+
        min(edge_foam,1.0)*0.88*breakup,0.0,1.0);

    vec3 light_dir=normalize(-u_global_light_direction);
    float ndotl=max(dot(n,light_dir),0.0);
    float shadow=1.0;
    if(u_shadows_enabled && u_global_light_enabled){
        vec4 ls=u_light_space_matrix*vec4(v_world_pos,1.0);
        if(ls.w>0.0){
            vec3 q=ls.xyz/ls.w*0.5+0.5;
            if(all(greaterThanEqual(q,vec3(0.0))) &&
               all(lessThanEqual(q,vec3(1.0)))){
                float bias=max(0.0008,0.003*(1.0-ndotl));
                shadow=texture(u_shadow_map,q.xy).r >= q.z-bias ? 1.0:0.0;
            }
        }
    }
    vec3 direct=u_global_light_enabled ?
        u_global_light_color*max(u_global_light_intensity,0.0)*ndotl*shadow:vec3(0.0);
    float ambient=max(u_ambient_intensity,0.0);
    vec3 half_dir = normalize(light_dir + view_dir);
    // Approximate x^96 with multiplies instead of a general-purpose pow().
    float spec_base = max(dot(n, half_dir), 0.0);
    float spec2 = spec_base * spec_base;
    float spec4 = spec2 * spec2;
    float spec8 = spec4 * spec4;
    float spec16 = spec8 * spec8;
    float spec32 = spec16 * spec16;
    float spec64 = spec32 * spec32;
    float specular=spec64*spec32*1.4*u_specular_strength*
        (u_global_light_enabled ? max(u_global_light_intensity,0.0)*shadow:0.0);

    vec3 color;
    float alpha;
    // WK-WAVES-006: render front and back consistently. The separate dark
    // backside shaded inverted triangles as black patches when waves fold or
    // when overlapping transparent triangles appear at grazing angles.
    // Pixel-clipped submerged fog is computed by the underwater volume pass.
    float edge = 1.0 - smoothstep(0.02, 0.22, v_depth);
    float reflection_weight = clamp(mix(0.10, 0.22 + fresnel * 0.68, 1.0 - edge)
        * u_reflection_strength,0.0,1.0);
    color=mix(body*(ambient+direct),reflected*ambient,reflection_weight);
    color+=vec3(0.035,0.075,0.080)*min(wake_ripple,1.0);
    color+=u_global_light_color*specular*mix(0.35,1.0,1.0-edge);
    color=mix(color,u_foam_color*(ambient+direct),foam*0.72);
    // WK-FOAM: new depth-outline foam is separate from crest/wake foam.
    float object_contact=wk_object_contact_foam();
    // Depth contact helps around Actors/Cells, but it must not flood
    // whole coarse depth blocks. Coastline foam is already continuous SDF.
    color=mix(color,u_foam_color*max(vec3(0.55),vec3(ambient)+direct),object_contact*0.26);
    // Meniscus stays attached to the displaced wave geometry near the eye.
    float waterline_distance=abs(v_world_pos.y-u_camera_position.y);
    float rim_width=mix(0.045,0.18,1.0-facing);
    float waterline_rim=(1.0-smoothstep(0.0,rim_width,waterline_distance))
        *smoothstep(0.65,0.98,1.0-facing);
    color=mix(color,color*vec3(0.51,0.67,0.71),waterline_rim*0.70);
    alpha = u_opacity*ocean_coast_alpha;
    if (v_swash>0.5) {
        // Only the narrow antialiased contact edge fades. Everywhere the
        // wave exists it has authored WATER opacity (no ghost film).
        alpha=u_opacity*smoothstep(0.30,0.48,swash_alpha);
    }
    // WK-WAVES-014: foamy regions retain 50% of their OWN opacity
    // independent of water transparency. Still obey the same coastline and
    // swash coverage to avoid glowing foam outside the true water footprint.
    float coverage=v_swash>0.5 ?
        smoothstep(0.30,0.48,swash_alpha):ocean_coast_alpha;
    alpha+=coverage*clamp(foam*0.72,0.0,1.0)*
        0.5*(1.0-clamp(u_opacity,0.0,1.0));
    alpha+=coverage*object_contact*0.12*(1.0-alpha);
    alpha+=coverage*waterline_rim*0.30*(1.0-alpha);

    if (u_atmosphere_enabled && gl_FrontFacing) {
        float distance_to_camera = length(v_world_pos - u_camera_position);
        float fog_amount = smoothstep(
            max(u_fog_start, 0.0),
            max(u_fog_end, u_fog_start + 1.0),
            distance_to_camera
        );
        color = mix(color, u_fog_color, fog_amount);
    }

    // WK-WAVES-015: only the back face of the displaced physical ocean
    // receives underwater optical shading. No camera-height switch.
    if (!gl_FrontFacing) {
        float internal_fresnel=pow(1.0-
            clamp(abs(dot(view_dir,n)),0.0,1.0),5.0);
        vec3 internal_reflection=texture(u_sky,reflect(-view_dir,n)).rgb;
        vec3 underwater_scatter=mix(
            u_underwater_post_color,internal_reflection,0.02);
        color=mix(color,underwater_scatter,
            (0.15+internal_fresnel*0.68)*u_reflection_strength);
        alpha=max(alpha,clamp(u_underwater_opacity,0.0,1.0)*
            ocean_coast_alpha);
    }

    if (u_stars_water_layer_enabled) {
        vec2 uv=fract(v_world_pos.xz/32.0);
        vec4 overlay=texture(u_stars_water_layer_texture,uv);
        color=mix(color,overlay.rgb,overlay.a);
    }
    FragColor = vec4(color, clamp(alpha, 0.0, 1.0));
}
"#;

#[cfg(test)]
mod tests {
    #[test]
    fn wk_swim_water_shader_uses_the_character_wave_clock() {
        let body = super::Renderer::draw_terrain_water as usize;
        assert!(body > 0);
        let source = include_str!("terrain.rs");
        let begin = source.find("pub(super) fn draw_terrain_water(").unwrap();
        let shader = &source[begin
            ..source[begin..]
                .find("pub(super) const WATER_VERTEX_SHADER")
                .unwrap()
                + begin];
        assert!(
            !shader.contains("self.water_started.elapsed().as_secs_f32()"),
            "Rendered wave u_time still uses a different clock from buoyancy"
        );
        assert!(
            shader.contains("self.water_time_location,")
                && shader.contains("crate::world::terrain::ocean_wave_time_seconds()"),
            "The surface shader must share the character's absolute wave phase clock"
        );
    }
    #[test]
    fn wk015_project_load_readonly_timing_profile() {
        use super::*;
        let Ok(path) = std::env::var("WK015_READONLY_PROFILE_PROJECT") else {
            return;
        };
        let started = std::time::Instant::now();
        let mut world = crate::world::World::new();
        crate::world::persistence::load_world(
            &mut world,
            &std::path::Path::new(&path).join("world.dat"),
        )
        .unwrap();
        let loaded = started.elapsed();
        let Some(result) = world.terrain.effective() else {
            panic!("terrain not loaded")
        };
        let chunk_start = std::time::Instant::now();
        let keys = terrain_render_chunk_keys(result);
        let indexed = chunk_start.elapsed();
        let prepared_start = std::time::Instant::now();
        let prepared = prepare_project_terrain(&world, &|| true).unwrap().unwrap();
        let prep = prepared_start.elapsed();
        let bytes: usize = prepared
            .chunks
            .iter()
            .map(|(_, mesh)| {
                mesh.batches
                    .iter()
                    .map(|b| b.vertices.len() * 4)
                    .sum::<usize>()
            })
            .sum();
        eprintln!(
            "WK015_LOAD_PROFILE world={loaded:?} keys={:?} (count={}) prepare={prep:?} chunks={} geometry_mib={:.1}",
            indexed,
            keys.len(),
            prepared.chunks.len(),
            bytes as f64 / 1048576.0
        );
    }
    #[test]
    fn legacy_foam_envelope_has_no_wave_phase_wrap_jump() {
        let frag = super::WATER_FRAGMENT_SHADER;
        assert!(
            frag.contains("float crest_pulse=0.5+0.5*cos(crest_phase);"),
            "Legacy foam must use a periodic smooth envelope instead of fract()"
        );
        assert!(!frag.contains("float crest_age=fract(base_phase/6.28318530718+0.75);"));
        let boundary = std::f64::consts::FRAC_PI_2;
        let eps = 0.00001f64;
        let continuity = |phase: f64| {
            let pulse = 0.5 + 0.5 * (phase + std::f64::consts::TAU * 0.75).cos();
            pulse.max(0.0).powf(3.2)
        };
        assert!((continuity(boundary - eps) - continuity(boundary + eps)).abs() < 0.001);
    }

    #[test]
    fn legacy_foam_keeps_half_independent_opacity_but_obeys_coverage() {
        let frag = super::WATER_FRAGMENT_SHADER;
        assert!(
            frag.contains("0.5*(1.0-clamp(u_opacity,0.0,1.0))"),
            "Foam must retain half its own alpha when water is transparent"
        );
        assert!(
            frag.contains("alpha+=coverage*clamp(foam*0.72,0.0,1.0)*"),
            "The retained foam opacity must be multiplied by water coverage"
        );
        let alpha = |water: f32, foam: f32, coverage: f32| {
            coverage * (water + foam * 0.72 * 0.5 * (1.0 - water))
        };
        assert!((alpha(0.0, 1.0, 1.0) - 0.36).abs() < 0.00001);
        assert!((alpha(0.5, 1.0, 1.0) - 0.68).abs() < 0.00001);
        assert_eq!(alpha(0.5, 0.0, 1.0), 0.5);
        assert_eq!(alpha(1.0, 1.0, 1.0), 1.0);
        assert_eq!(alpha(0.5, 1.0, 0.0), 0.0);
    }

    #[test]
    fn legacy_swash_geometry_does_not_cover_underwater_sand() {
        let mut wash = Vec::new();
        let corners = [
            [0.0, -0.25, 0.0],
            [0.0, -0.20, 1.0],
            [1.0, 0.25, 1.0],
            [1.0, 0.20, 0.0],
        ];
        super::push_shore_swash_quad(&mut wash, corners, 0.0, 1.2, 1.0);
        assert!(
            !wash.is_empty(),
            "Swash must still render on exposed beaches"
        );
        assert!(
            wash.chunks_exact(12).all(|v| v[1] >= 0.045 - 0.00001),
            "Swash must not duplicate the submerged ocean footprint"
        );
        let all_submerged = [
            [0.0, -0.25, 0.0],
            [0.0, -0.20, 1.0],
            [1.0, -0.15, 1.0],
            [1.0, -0.10, 0.0],
        ];
        wash.clear();
        super::push_shore_swash_quad(&mut wash, all_submerged, 0.0, 1.2, 1.0);
        assert!(wash.is_empty(), "No undersea swash polygons");
    }

    #[test]
    fn ocean_wetness_uses_seeded_three_band_ocean_contract() {
        let wet = super::super::GRID_FRAGMENT_SHADER;
        let water = super::WATER_VERTEX_SHADER;
        assert!(wet.contains("uniform float u_ocean_wetness_seed;"));
        assert!(wet.contains("return sin(a)*0.72+sin(b)*0.19+sin(c)*0.09;"));
        assert!(water.contains("return sin(a)*0.72+sin(b)*0.19+sin(c)*0.09;"));
        assert!(wet.contains("wk_ocean_contact(v_world_pos,u_ocean_wetness.w)"));
        assert!(wet.contains("float signed_coast=(texture(u_ocean_contact_atlas"));
        assert!(!wet.contains("float relative_height=(v_world_pos.y-u_terrain_water_level)/"));
    }
    #[test]
    fn water_wake_curves_follow_connected_samples_not_independent_vs() {
        let vertex = super::WATER_VERTEX_SHADER;
        let frag = super::WATER_FRAGMENT_SHADER;
        assert!(vertex.contains("vec2 segment=wake.zw-wake.xy;"));
        assert!(vertex.contains("*0.24+min(contact_shift,0.18)"));
        assert!(frag.contains("vec2 nearest=mix(w.xy,w.zw,u);"));
        assert!(frag.contains("wake_foam=max(wake_foam,foam_shape*fade*power);"));
        assert!(!frag.contains("float arm=abs(lateral-behind*0.42);"));
    }
    #[test]
    fn still_water_atlas_does_not_rasterize_square_runup_envelope() {
        let source = include_str!("terrain.rs");
        let atlas = include_str!("terrain_contact_atlas.rs");
        assert!(atlas.contains("f,config.plains.water_level,0.0,0.0,x0,x1,z0,z1"));
        assert!(atlas.contains("g,config.plains.water_level,0.0,0.0,x0,x1,z0,z1"));
        assert!(source.contains("let reach_studs=min(3.5,1.35+max(u_wave_height,0.0)*0.35);"));
        assert!(source.contains("alpha=u_opacity*smoothstep(0.30,0.48,swash_alpha);"));
        assert!(source.contains("ocean_coast_alpha=smoothstep(-0.20,0.10,coast);"));
        assert!(source.contains("alpha = u_opacity*ocean_coast_alpha;"));
    }
    #[test]
    fn ocean_contact_signed_distance_feathers_binary_texel_edges() {
        let n = 16;
        let mask = (0..n * n).map(|i| i % n < 8).collect::<Vec<_>>();
        let rgba = super::ocean_contact_signed_distance(&mask, n, 0.5);
        let at = |x: usize| rgba[8 * n + x] as i32;
        assert!(at(2) > at(6) && at(6) > at(7));
        assert!(at(7) > 127 && at(8) < 128);
        assert!(at(8) > at(9) && at(9) > at(13));
        assert_eq!(rgba.len(), n * n);
    }
    #[test]
    fn swash_fragment_uses_authored_opacity_and_pixel_coverage() {
        let vertex = super::WATER_VERTEX_SHADER;
        let fragment = super::WATER_FRAGMENT_SHADER;
        assert!(vertex.contains("v_swash_elevation=a_uv.x;"));
        assert!(fragment.contains("float feather=max(fwidth(v_swash_elevation)"));
        assert!(fragment.contains("alpha=u_opacity*smoothstep(0.30,0.48,swash_alpha);"));
        assert!(fragment.contains("swash_alpha*=smoothstep(-reach_studs"));
        assert!(fragment.contains("texture(u_ocean_contact_atlas,"));
        assert!(!fragment.contains("u_opacity*0.43"));
        assert!(fragment.contains("if (swash_alpha<0.30) discard;"));
    }
    #[test]
    fn water_uses_scene_lighting_instead_of_fixed_direction() {
        let shader = super::WATER_FRAGMENT_SHADER;
        for uniform in [
            "u_ambient_intensity",
            "u_global_light_direction",
            "u_global_light_color",
            "u_global_light_intensity",
            "u_global_light_enabled",
            "u_shadow_map",
            "u_shadows_enabled",
        ] {
            assert!(
                shader.contains(uniform),
                "missing lighting input: {uniform}"
            );
        }
        assert!(!shader.contains("normalize(vec3(0.35, 0.86, 0.28))"));
        assert!(shader.contains("body*(ambient+direct)"));
        assert!(shader.contains("u_global_light_color*specular"));
    }

    #[test]
    fn ocean_faces_share_surface_shading_and_limit_crest_foldover() {
        let vertex = super::WATER_VERTEX_SHADER;
        let fragment = super::WATER_FRAGMENT_SHADER;
        assert!(vertex.contains("0.75/k"));
        assert!(vertex.contains("world.xz+=direction*cos(phase)*sideways"));
        assert!(fragment.contains("alpha = u_opacity*ocean_coast_alpha;"));
        assert!(!fragment.contains("vec3 underside = u_underwater_color;"));
        assert!(fragment.contains("if (!gl_FrontFacing)"));
    }

    #[test]
    fn stars_water_overlay_uses_world_coordinates_and_optional_shader_path() {
        let shader = super::WATER_FRAGMENT_SHADER;
        assert!(shader.contains("if (u_stars_water_layer_enabled)"));
        assert!(shader.contains("fract(v_world_pos.xz/32.0)"));
        assert!(shader.contains("mix(color,overlay.rgb,overlay.a)"));
    }
    #[test]
    fn shoreline_at_exact_corner_has_no_zero_area_triangles() {
        let mut vertices = Vec::new();
        super::emit_shoreline_clipped_cell(
            &mut vertices,
            [
                [0.0, 0.0, 0.0],
                [0.0, 0.0, 1.0],
                [1.0, 0.0, 1.0],
                [1.0, 0.0, 0.0],
            ],
            [1.0, 0.0, 1.0, -1.0],
        );
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 36, 0);
        for triangle in vertices.chunks_exact(36) {
            let a = (triangle[0], triangle[2]);
            let b = (triangle[12], triangle[14]);
            let c = (triangle[24], triangle[26]);
            let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
            assert!(
                area.abs() > 0.000001,
                "zero-area shoreline triangle at exact waterline corner"
            );
        }
    }

    #[test]
    fn shoreline_clipping_eliminates_axis_aligned_dry_rectangle() {
        let mut vertices = Vec::new();
        super::emit_shoreline_clipped_cell(
            &mut vertices,
            [
                [0.0, 2.0, 0.0],
                [0.0, 2.0, 1.0],
                [1.0, 2.0, 1.0],
                [1.0, 2.0, 0.0],
            ],
            [1.0, -1.0, -1.0, 1.0],
        );
        assert!(!vertices.is_empty());
        assert_eq!(vertices.len() % 36, 0);
        assert!(
            vertices
                .chunks_exact(12)
                .any(|v| (v[2] - 0.5).abs() < 0.0001),
            "shoreline has an interpolated edge"
        );
        assert!(vertices.chunks_exact(12).all(|v| v[1] == 2.0));
        let mut dry = Vec::new();
        super::emit_shoreline_clipped_cell(
            &mut dry,
            [
                [0.0, 2.0, 0.0],
                [0.0, 2.0, 1.0],
                [1.0, 2.0, 1.0],
                [1.0, 2.0, 0.0],
            ],
            [-1.0; 4],
        );
        assert!(dry.is_empty());
    }

    use super::*;
    use crate::world::{TerrainConfig, TerrainGenerationRequest, generate_terrain};

    #[test]
    fn water_chunks_follow_changed_sea_level_across_chunk_boundaries() {
        let grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [32, 6, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for level in [3.0_f32, 11.0] {
            let left = build_voxel_water_region(&grid, level, 0, 16, 0, 16);
            let right = build_voxel_water_region(&grid, level, 16, 32, 0, 16);
            assert!(!left.is_empty() && !right.is_empty());
            assert_eq!(left.len() % 12, 0);
            assert_eq!(right.len() % 12, 0);
            for vertex in left.chunks_exact(12).chain(right.chunks_exact(12)) {
                assert_eq!(
                    vertex[1], level,
                    "water vertices must track the changed sea level"
                );
            }
            let left_edge = left
                .chunks_exact(12)
                .filter(|v| v[0] == 32.0)
                .map(|v| v[2])
                .collect::<Vec<_>>();
            let right_edge = right
                .chunks_exact(12)
                .filter(|v| v[0] == 32.0)
                .map(|v| v[2])
                .collect::<Vec<_>>();
            assert!(!left_edge.is_empty() && !right_edge.is_empty());
            assert!(
                left_edge.iter().all(|z| right_edge.contains(z)),
                "water chunks must share the same seam coordinates"
            );
        }
    }

    #[test]
    fn smooth_painted_surface_material_wins_over_deeper_unpainted_voxel() {
        let samples = [
            DensitySample {
                position: Vec3::new(0.0, 0.0, 0.0),
                density: 1.0,
                material: Some(0),
            },
            DensitySample {
                position: Vec3::new(1.0, 0.0, 0.0),
                density: 0.65,
                material: Some(1),
            },
            DensitySample {
                position: Vec3::new(0.0, 1.0, 0.0),
                density: 0.0,
                material: None,
            },
            DensitySample {
                position: Vec3::new(0.0, 0.0, 1.0),
                density: 0.0,
                material: None,
            },
        ];
        let mut batches = vec![Vec::new(), Vec::new()];
        polygonize_tetrahedron(samples, &mut batches, 0.0);
        assert!(
            !batches[1].is_empty(),
            "painted surface triangles must use painted material"
        );
        assert!(
            batches[0].is_empty(),
            "unpainted deep material must not override brush"
        );
    }

    #[test]
    fn terrain_shader_blends_all_six_layers_and_never_branches_on_chunk_palette() {
        let shader = super::super::GRID_FRAGMENT_SHADER;
        for code in [
            "u_terrain_snow_texture",
            "u_terrain_mud_texture",
            "snow_tex*snow + mud_tex*mud",
            "u_terrain_vertex_layers",
            "v_layer_primary",
            "v_layer_secondary",
            "float coverage=clamp(native_paint.a",
        ] {
            assert!(
                shader.contains(code),
                "missing six-layer shader path: {code}"
            );
        }
        assert!(
            !shader.contains("if (u_terrain_source_role"),
            "palette draw-batch category must not create a hard material seam"
        );
        assert!(
            !shader.contains("if (native_paint.a>0.001)"),
            "paint alpha cannot be a binary atlas-tile switch"
        );
        let vertex = super::super::GRID_VERTEX_SHADER;
        assert!(vertex.contains("u_terrain_vertex_layers ? vec4(1.0)"));
        assert!(vertex.contains("v_layer_primary = a_color"));
    }

    #[test]
    fn voxel_weights_blend_snow_mud_grass_sand_between_adjacent_samples() {
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [0, 0, 0],
            [4, 4, 4],
            2.0,
            vec![
                "PBR:Grass".into(),
                "PBR:Snow".into(),
                "PBR:Mud".into(),
                "PBR:Sand".into(),
            ],
        )
        .unwrap();
        for (x, z, index) in [(1, 1, 0), (2, 1, 1), (1, 2, 2), (2, 2, 3)] {
            assert!(grid.set_voxel(
                x,
                1,
                z,
                crate::world::terrain::SmoothVoxel::from_unit_occupancy(index, 1.0)
            ));
        }
        let weights = voxel_layer_weights(&grid, Vec3::new(4.0, 3.0, 4.0), 0);
        for index in [0, 3, 4, 5] {
            assert!(
                (weights[index] - 0.25).abs() < 0.0001,
                "all four adjacent source layers must blend through the same vertex"
            );
        }
        assert!((weights.iter().sum::<f32>() - 1.0).abs() < 1e-6);
        let adjacent = voxel_layer_weights(&grid, Vec3::new(4.1, 3.0, 4.0), 0);
        assert!(
            (adjacent[0] - weights[0]).abs() < 0.1,
            "moving one tenth of a stud cannot jump a full material role"
        );
        assert_eq!(
            voxel_layer_weights(&grid, Vec3::new(4.0, 3.0, 4.0), 0),
            voxel_layer_weights(&grid, Vec3::new(4.0, 3.0, 4.0), 5),
            "chunk or source-role fallback must not change real source samples"
        );
    }

    #[test]
    fn voxel_layer_stream_has_normalized_colors_and_shared_smooth_normals() {
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [0, 0, 0],
            [4, 4, 4],
            2.0,
            vec!["PBR:Grass".into(), "PBR:Rock".into()],
        )
        .unwrap();
        for x in 0..4 {
            for z in 0..4 {
                for y in 0..2 {
                    grid.set_voxel(
                        x,
                        y,
                        z,
                        crate::world::terrain::SmoothVoxel::from_unit_occupancy(
                            if x < 2 { 0 } else { 1 },
                            1.0,
                        ),
                    );
                }
            }
        }
        let mut a = Vec::new();
        let mut b = Vec::new();
        let shared = Vec3::new(4.0, 3.0, 4.0);
        push_blended_terrain_triangle(
            &mut a,
            &grid,
            shared,
            Vec3::new(4.0, 3.0, 6.0),
            Vec3::new(6.0, 3.0, 4.0),
            0,
        );
        push_blended_terrain_triangle(
            &mut b,
            &grid,
            shared,
            Vec3::new(2.0, 3.0, 4.0),
            Vec3::new(4.0, 3.0, 2.0),
            1,
        );
        assert_eq!(
            &a[..12],
            &b[..12],
            "same world vertex must have identical weights and normal across palette batches"
        );
        assert!((a[6..10].iter().sum::<f32>() + a[10] + a[11] - 1.0).abs() < 1e-5);
    }

    #[test]
    fn pure_grass_source_retains_full_grass_layer_weight_without_brown_layers() {
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [0, 0, 0],
            [4, 4, 4],
            2.0,
            vec!["PBR:Grass".into(), "PBR:Dirt".into(), "PBR:Sand".into()],
        )
        .unwrap();
        for x in 0..4 {
            for y in 0..4 {
                for z in 0..4 {
                    grid.set_voxel(
                        x,
                        y,
                        z,
                        crate::world::terrain::SmoothVoxel::from_unit_occupancy(0, 1.0),
                    );
                }
            }
        }
        let weights = voxel_layer_weights(&grid, Vec3::new(4.0, 4.0, 4.0), 2);
        assert!(
            (weights[0] - 1.0).abs() < 1e-5,
            "fully authored Grass must not turn into Dirt/Sand because of palette batching"
        );
        assert!(weights[1..].iter().all(|w| w.abs() < 1e-6));
    }

    #[test]
    fn surface_material_identifiers_are_not_rewritten_to_builtin_packs() {
        assert_eq!(
            landscape_texture_id("Material:mat-test", "Surface"),
            "Material:mat-test"
        );
        assert_eq!(landscape_texture_id("", "Surface"), "");
        let materials = TerrainMaterials {
            surface: "Material:mat-green".into(),
            subsurface: "Material:mat-soil".into(),
            rock: "Material:mat-stone".into(),
            water: "Water".into(),
        };
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_SURFACE_MATERIAL, &materials).0,
            "Material:mat-green"
        );
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_SUBSURFACE_MATERIAL, &materials).0,
            "Material:mat-soil"
        );
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_ROCK_MATERIAL, &materials).0,
            "Material:mat-stone"
        );
    }
    #[test]
    fn smooth_terrain_blend_roles_cover_core_layers_only() {
        let materials = TerrainMaterials {
            surface: "Material:forest".into(),
            subsurface: "Material:soil".into(),
            rock: "Material:stone".into(),
            water: "Water".into(),
        };
        assert_eq!(
            terrain_blend_role(materials.surface.as_str(), &materials),
            Some(0)
        );
        assert_eq!(
            terrain_blend_role(materials.subsurface.as_str(), &materials),
            Some(1)
        );
        assert_eq!(
            terrain_blend_role(materials.rock.as_str(), &materials),
            Some(2)
        );
        assert_eq!(terrain_blend_role("sand", &materials), Some(3));
        assert_eq!(terrain_blend_role("project/custom_mud", &materials), None);
        assert_eq!(terrain_blend_role("", &materials), None);
        assert_eq!(terrain_blend_role("", &TerrainMaterials::default()), None);
    }

    #[test]
    fn ocean_nearest_field_keeps_exact_circular_runup_distance() {
        let mut ocean = vec![false; 9 * 9];
        ocean[2 * 9 + 1] = true;
        let dist = ocean_nearest_squared_distance(&ocean, 9, 9);
        assert_eq!(dist[2 * 9 + 1], 0.0);
        assert_eq!(dist[6 * 9 + 4], 25.0);
        assert_eq!(dist[3 * 9 + 2], 2.0);
        assert_eq!(dist[8 * 9 + 8], 85.0);
        let none = ocean_nearest_squared_distance(&vec![false; 9 * 9], 9, 9);
        assert!(none.iter().all(|value| *value > 250.0));
    }

    #[test]
    fn swash_quad_projects_onto_shallow_beach_not_cliffs() {
        let mut mesh = Vec::new();
        let mild = [
            [0.0, 0.00, 0.0],
            [0.0, 0.15, 1.0],
            [1.0, 0.20, 1.0],
            [1.0, 0.05, 0.0],
        ];
        push_shore_swash_quad(&mut mesh, mild, 0.0, 0.60, 1.0);
        assert_eq!(mesh.len(), 6 * 12);
        for v in mesh.chunks_exact(12) {
            assert_eq!(v[9], 2.0);
            assert!(v[1] > 0.0 && v[1] < 0.30);
            assert!((v[10] - (v[1] - 0.045)).abs() < 0.0001);
        }
        let cliff = [
            [0.0, 0.0, 0.0],
            [0.0, 4.0, 1.0],
            [1.0, 4.0, 1.0],
            [1.0, 0.0, 0.0],
        ];
        let before = mesh.len();
        push_shore_swash_quad(&mut mesh, cliff, 0.0, 0.60, 1.0);
        assert_eq!(mesh.len(), before);
    }

    #[test]
    fn zero_wave_height_is_flat_and_ten_is_valid() {
        let mut water = crate::world::terrain::TerrainWaterSettings::default();
        for height in [0.0, 1.0, 3.0, 10.0] {
            water.wave_height = height;
            assert!(water.validate().is_ok(), "height {height}");
        }
        water.wave_height = 10.01;
        assert!(water.validate().is_err());
        let vert = WATER_VERTEX_SHADER;
        assert!(vert.contains("ocean_wave_field(world.xz,u_time)*u_wave_height"));
        assert!(vert.contains("v_swash_elevation"));
        let frag = WATER_FRAGMENT_SHADER;
        assert!(frag.contains("float swash_alpha=1.0;"));
        assert!(frag.contains("foam_noise"));
    }

    #[test]
    fn shoreline_water_metadata_fades_smoothly_with_depth() {
        let edge = water_surface_attributes(0.10, 0.8);
        let shallow = water_surface_attributes(1.5, 0.8);
        let medium = water_surface_attributes(4.0, 0.8);
        let deep = water_surface_attributes(16.0, 0.8);

        assert!(edge[0] > shallow[0]);
        assert!(shallow[0] > medium[0]);
        assert_eq!(medium[0], 0.0);
        assert!(edge[1] < shallow[1]);
        assert!(shallow[1] < medium[1]);
        assert!(medium[1] < deep[1]);
        assert_eq!(deep[1], 1.0);
    }

    #[test]
    fn underwater_volume_is_bounded_by_terrain_request_and_water_level() {
        let config = TerrainConfig::default();
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [10, -4, 20],
                size: [32, 24, 48],
            },
        )
        .unwrap();

        let volume = underwater_volume_for_result(&result, 8.0).unwrap();
        assert_eq!(volume.min, Vec3::new(10.0, -4.0, 20.0));
        assert_eq!(volume.max, Vec3::new(42.0, 8.0, 68.0));
        assert!(volume.contains(Vec3::new(20.0, 0.0, 30.0)));
        assert!(!volume.contains(Vec3::new(20.0, 8.0, 30.0)));
        assert!(!volume.contains(Vec3::new(9.99, 0.0, 30.0)));
        assert!(!volume.contains(Vec3::new(42.0, 0.0, 30.0)));
    }

    #[test]
    fn underwater_fog_masks_camera_crossing_per_ray_not_whole_screen() {
        assert_eq!(super::underwater_ray_distance(8.0, 10.0, 8.0, 20.0), 0.0);
        assert_eq!(super::underwater_ray_distance(7.0, 6.0, 8.0, 20.0), 20.0);
        assert!((super::underwater_ray_distance(7.0, 9.0, 8.0, 20.0) - 10.0).abs() < 0.0001);
        assert!((super::underwater_ray_distance(9.0, 7.0, 8.0, 20.0) - 10.0).abs() < 0.0001);
        assert_eq!(super::underwater_ray_distance(9.0, 10.0, 8.0, 20.0), 0.0);
    }

    #[test]
    fn underwater_postprocess_uses_project_values_and_can_be_disabled() {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 3.0;
        world.terrain.config.plains.height_variation = 0.0;
        world.terrain.config.plains.water_level = 9.0;
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 16, 16],
            })
            .unwrap();
        {
            let post = &mut world.terrain.config.water;
            post.underwater_fog_color = [0.6, 0.2, 0.1];
            post.underwater_fog_density = 0.2;
            post.underwater_depth_fog = 0.04;
            post.underwater_depth_fog_limit = 0.45;
            post.underwater_fog_limit = 0.5;
            post.underwater_sky_tint = 0.3;
        }
        let state = underwater_state(&world, Vec3::new(8.0, 7.0, 8.0)).unwrap();
        assert!(state.enabled);
        assert_eq!(state.color, Vec3::new(0.6, 0.2, 0.1));
        assert_eq!(state.density, 0.2);
        assert_eq!(state.depth_fog, 0.04);
        assert_eq!(state.depth_fog_limit, 0.45);
        assert_eq!(state.fog_limit, 0.5);
        assert_eq!(state.sky_tint, 0.3);
        world.terrain.config.water.underwater_effects_enabled = false;
        let disabled = underwater_state(&world, Vec3::new(8.0, 7.0, 8.0)).unwrap();
        assert!(!disabled.enabled);
        assert_eq!(disabled.surface_y, 9.0);
    }

    #[test]
    fn underwater_state_requires_camera_inside_actual_water_column() {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 4.0;
        world.terrain.config.plains.height_variation = 0.0;
        world.terrain.config.plains.water_level = 8.0;
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 16, 16],
            })
            .unwrap();

        assert!(underwater_state(&world, Vec3::new(4.0, 6.0, 4.0)).is_some());
        assert!(underwater_state(&world, Vec3::new(4.0, 8.0, 4.0)).is_some());
        assert!(underwater_state(&world, Vec3::new(4.0, 8.7, 4.0)).is_some());
        assert!(underwater_state(&world, Vec3::new(4.0, 12.0, 4.0)).is_some());
        assert!(underwater_state(&world, Vec3::new(4.0, 3.0, 4.0)).is_some());
        assert!(underwater_state(&world, Vec3::new(20.0, 6.0, 4.0)).is_some());
    }

    #[test]
    fn underwater_volume_uses_water_level_as_true_top_plane() {
        let config = TerrainConfig::default();
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 5, 0],
                size: [8, 10, 8],
            },
        )
        .unwrap();
        let volume = underwater_volume_for_result(&result, 100.0).unwrap();
        assert_eq!(volume.max.y, 100.0);
        assert!(underwater_volume_for_result(&result, 5.0).is_none());
    }

    #[test]
    fn independent_object_foam_replaces_old_shoreline_fringe() {
        let fragment = WATER_FRAGMENT_SHADER;
        assert!(fragment.contains("// WK_OBJECT_CONTACT_FOAM_FUNCTION"));
        assert!(fragment.contains("float contact=wk_object_contact_foam();"));
        assert!(fragment.contains("float object_contact=wk_object_contact_foam();"));
        assert!(fragment.contains("float shore_foam=0.0;"));
        assert!(fragment.contains("float edge_foam=0.0;"));
        assert!(!ADVANCED_SHORELINE_ENABLED);
        assert!(!WET_SAND_MATERIAL_ENABLED);
    }

    #[test]
    fn water_surface_attributes_create_shore_foam_and_depth_gradient() {
        let shore = water_surface_attributes(0.25, 0.2);
        let medium = water_surface_attributes(4.0, 0.6);
        let deep = water_surface_attributes(20.0, 1.0);

        assert!(shore[0] > 0.8);
        assert_eq!(medium[0], 0.0);
        assert!(shore[1] < medium[1]);
        assert_eq!(deep[1], 1.0);
        assert_eq!(shore[2], 0.2);
        assert_eq!(deep[2], 1.0);
    }

    #[test]
    fn voxel_runup_adds_only_nearshore_potential_water_triangles() {
        use crate::world::terrain::SmoothVoxel;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [24, 16, 16], 1.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..24 {
                for y in 0..if x < 12 { 9 } else { 11 } {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        let still = build_voxel_water_region(&grid, 10.0, 0, 24, 0, 16);
        let runup = build_voxel_water_region_with_runup(&grid, 10.0, 2.0, 3.0, 0, 24, 0, 16);
        assert!(runup.len() > still.len(), "Runup should extend nearshore");
        assert!(
            runup
                .chunks_exact(12)
                .filter(|v| v[9] < 1.5)
                .all(|v| v[1] == 10.0),
            "Ocean surface remains at still level"
        );
        assert!(
            runup.chunks_exact(12).any(|v| v[9] > 1.5),
            "Swash has terrain-following vertices above sand"
        );
    }

    #[test]
    fn heightfield_water_aligns_with_terrain_and_is_chunk_local() {
        let field = HeightfieldData {
            origin: [10, -5, 20],
            size: [130, 16, 130],
            heights: vec![-2.0; 130 * 130],
        };
        let left = build_heightfield_water_region_with_runup(&field, 0.0, 1.0, 1.0, 0, 64, 0, 64);
        let right =
            build_heightfield_water_region_with_runup(&field, 0.0, 1.0, 1.0, 64, 128, 0, 64);
        assert!(!left.is_empty() && !right.is_empty());
        let x0 = field.origin[0] as f32 + 0.5;
        let seam = field.origin[0] as f32 + 64.5;
        assert!(left.chunks_exact(12).all(|v| v[0] >= x0 && v[0] <= seam));
        assert!(right.chunks_exact(12).all(|v| v[0] >= seam));
        let a = left
            .chunks_exact(12)
            .filter(|v| (v[0] - seam).abs() < 1e-5)
            .count();
        let b = right
            .chunks_exact(12)
            .filter(|v| (v[0] - seam).abs() < 1e-5)
            .count();
        assert!(a > 0 && b > 0);
        assert!(
            left.chunks_exact(12)
                .all(|v| ((v[0] - x0).fract()).abs() < 0.001
                    && ((v[2] - (field.origin[2] as f32 + 0.5)).fract()).abs() < 0.001)
        );
    }

    #[test]
    fn heightfield_ocean_render_batch_follows_its_own_frustum_chunk() {
        let field = HeightfieldData {
            origin: [0, 0, 0],
            size: [130, 32, 130],
            heights: vec![-2.0; 130 * 130],
        };
        let result = TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [130, 32, 130],
            },
            generator: crate::world::terrain::TerrainGeneratorType::Plains,
            generator_implementation_version: 0,
            data: TerrainData::Heightfield(field),
        };
        let mut config = TerrainConfig::default();
        config.ocean_enabled = true;
        config.plains.water_level = 0.0;
        let keys = terrain_render_chunk_keys(&result);
        assert!(keys.contains(&[0, 0]) && keys.contains(&[1, 0]) && keys.contains(&[1, 1]));
        for key in [[0, 0], [1, 0], [1, 1]] {
            let mesh = build_terrain_mesh_chunk(&result, &config, key);
            let water = mesh
                .batches
                .iter()
                .find(|batch| {
                    matches!(batch.material, TerrainMaterialSlot::Water)
                        && batch.water_body_id.is_none()
                })
                .expect("water must belong to its actual visible render chunk");
            let origin_x = key[0] as f32 * 64.0 + 0.5;
            let origin_z = key[1] as f32 * 64.0 + 0.5;
            assert!(water.vertices.chunks_exact(12).all(|v| v[0] >= origin_x
                && v[0] <= origin_x + 64.0
                && v[2] >= origin_z
                && v[2] <= origin_z + 64.0));
        }
    }

    #[test]
    fn heightfield_dry_cells_never_create_opaque_ocean_grid() {
        let mut field = HeightfieldData {
            origin: [0, 0, 0],
            size: [8, 8, 8],
            heights: vec![1.0; 64],
        };
        field.heights[3 * 8 + 3] = -1.0;
        let water = build_water_with_runup(&field, 0.0, 2.0, 1.0);
        let opaque = water
            .chunks_exact(12)
            .filter(|v| v[9] < 1.5)
            .collect::<Vec<_>>();
        assert!(!opaque.is_empty());
        assert!(opaque.iter().all(|v| v[1] == 0.0
            && v[0] >= 2.5
            && v[0] <= 4.5
            && v[2] >= 2.5
            && v[2] <= 4.5));
        assert!(
            water
                .chunks_exact(12)
                .filter(|v| v[9] > 1.5)
                .all(|v| v[1] > 0.0)
        );
    }

    #[test]
    fn runup_extends_from_ocean_without_flooding_inland_lowlands() {
        let mut field = HeightfieldData {
            origin: [0, 0, 0],
            size: [24, 8, 24],
            heights: vec![0.45; 24 * 24],
        };
        field.heights[12 * 24 + 12] = -1.0;
        let still = build_water(&field, 0.0);
        let slow = build_water_with_runup(&field, 0.0, 2.0, 0.0);
        let fast = build_water_with_runup(&field, 0.0, 2.0, 9.0);
        assert!(slow.len() > still.len());
        assert!(fast.len() > slow.len());
        assert!(
            fast.chunks_exact(12)
                .filter(|v| v[9] < 1.5)
                .all(|v| v[1] == 0.0),
            "Only truly submerged terrain has ocean triangles"
        );
        assert!(
            fast.chunks_exact(12).any(|v| v[9] > 1.5 && v[1] > 0.0),
            "Runup remains an actual above-sand wave front"
        );
        assert!(!fast.chunks_exact(12).any(|v| v[0] < 4.0 && v[2] < 4.0));
        field.heights.fill(0.45);
        assert!(build_water_with_runup(&field, 0.0, 2.0, 9.0).is_empty());
    }

    #[test]
    fn ocean_wave_parity_uses_archived_phase_and_keeps_sliders() {
        let v = WATER_VERTEX_SHADER;
        let f = WATER_FRAGMENT_SHADER;
        assert!(v.contains("u_ocean_parity_wave"));
        assert!(v.contains("ocean_primary_phase(world.xz,u_time)"));
        assert!(f.contains("ocean_primary_phase(flow_p,u_time)"));
        let wave_speed = 1.7_f32;
        let wave_length = 12.0_f32;
        for angle in [0.0_f32, std::f32::consts::FRAC_PI_2, 1.2_f32] {
            let dir = [angle.cos(), angle.sin()];
            let at = |x: f32, z: f32, t: f32| {
                -(x * dir[0] + z * dir[1]) * std::f32::consts::TAU / wave_length + t * wave_speed
            };
            let dt = 0.5_f32;
            let distance = dt * wave_speed * wave_length / std::f32::consts::TAU;
            assert!(
                (at(dir[0] * distance, dir[1] * distance, dt) - at(0.0, 0.0, 0.0)).abs() < 1.0e-5
            );
        }
        assert!(v.contains("u_time*u_wave_speed"));
        assert!(v.contains("max(u_wave_length,1.0)"));
        assert!(v.contains("cos(u_wave_direction)"));
        assert!(v.contains("ocean_wave_field(world.xz,u_time)*u_wave_height*shoreline_fade;"));
        assert!(v.contains("sideways*shoreline_fade"));
        assert!(v.contains("u_wave_steepness>0.0"));
        assert!(v.contains("world.y += wave_height(wave_p"));
        assert!(f.contains("else if(u_ocean_parity_wave)"));
    }

    #[test]
    fn chunk_wave_energy_keeps_puddles_calmer_than_open_deep_water() {
        let puddle = water_chunk_wave_energy(4, 256, 0.8);
        let pond = water_chunk_wave_energy(96, 256, 3.0);
        let ocean = water_chunk_wave_energy(256, 256, 12.0);

        assert!(puddle < pond);
        assert!(pond < ocean);
        assert!(puddle < 0.25);
        assert!(ocean > 0.9);
    }

    #[test]
    fn water_user_opacity_retains_normal_shading_without_forced_depth_alpha() {
        let frag = WATER_FRAGMENT_SHADER;
        assert!(frag.contains("vec3 body = mix(shallow, deep, depth_mix);"));
        assert!(
            frag.contains("color=mix(body*(ambient+direct),reflected*ambient,reflection_weight);")
        );
        assert!(frag.contains("color=mix(color,u_foam_color*(ambient+direct),foam*0.72);"));
        assert!(frag.contains("alpha = u_opacity*ocean_coast_alpha;"));
        assert!(!frag.contains("alpha = mix(0.20, 0.82"));
        assert!(!frag.contains("alpha = max(alpha, foam * 0.58)"));
        assert!(!frag.contains("FragColor = vec4(1.0, 0.0, 1.0, 1.0);"));
    }

    #[test]
    fn water_flow_coordinates_match_horizontal_vertex_and_fragment_contract() {
        let vertex = WATER_VERTEX_SHADER;
        let fragment = WATER_FRAGMENT_SHADER;
        assert!(vertex.contains("out vec2 v_flow;"));
        assert!(vertex.contains("world.xz-v_flow*u_time"));
        assert!(fragment.contains("in vec2 v_flow;"));
        assert!(fragment.contains("vec2 flow_p = v_world_pos.xz - v_flow * u_time;"));
        assert!(fragment.contains("n=water_normal(flow_p,u_time,v_depth,v_wave_energy);"));
        assert!(fragment.contains("vec2 drift=flow_p*max(u_foam_scale,0.25)"));
    }

    #[test]
    fn independent_water_material_optics_reach_both_shader_stages() {
        for uniform in ["u_wave_length", "u_wave_direction"] {
            assert!(WATER_VERTEX_SHADER.contains(uniform));
            assert!(WATER_FRAGMENT_SHADER.contains(uniform));
        }
        assert!(WATER_VERTEX_SHADER.contains("u_wave_steepness"));
        assert!(WATER_VERTEX_SHADER.contains("gerstner_dir"));
        for uniform in [
            "u_absorption_color",
            "u_absorption_strength",
            "u_scattering_color",
            "u_scattering_strength",
        ] {
            assert!(WATER_FRAGMENT_SHADER.contains(uniform));
        }
        assert!(WATER_FRAGMENT_SHADER.contains("exp(-u_absorption_strength"));
        assert!(WATER_FRAGMENT_SHADER.contains("exp(-u_scattering_strength"));
    }

    #[test]
    fn water_controls_use_creator_opacity_and_wave_driven_seeded_foam() {
        let vertex = WATER_VERTEX_SHADER;
        let fragment = WATER_FRAGMENT_SHADER;
        for uniform in ["u_wave_height", "u_wave_speed"] {
            assert!(vertex.contains(uniform));
            assert!(fragment.contains(uniform));
        }
        for uniform in ["u_foam_amount", "u_foam_scale"] {
            assert!(fragment.contains(uniform));
        }
        assert!(fragment.contains("float foam_noise(vec2 p)"));
        assert!(fragment.contains("float crest_pulse=0.5+0.5*cos(crest_phase);"));
        assert!(fragment.contains("float breaking=smoothstep("));
        assert!(!fragment.contains("v_world_pos.x * 1.9"));
        assert!(fragment.contains("alpha = u_opacity*ocean_coast_alpha;"));
    }

    #[test]
    fn water_gradient_metadata_varies_inside_a_single_deep_greedy_patch() {
        use crate::world::terrain::SmoothVoxel;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [16, 16, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..16 {
                let height = if x < 8 { 12 } else { 6 };
                for y in 0..height {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        assert_eq!(water_merge_class(6.0), water_merge_class(18.0));
        let data = build_voxel_water_region(&grid, 30.0, 0, 16, 0, 16);
        assert!(
            data.len() / 36 < 400,
            "water wave tessellation unexpectedly dense"
        );
        let colors: Vec<f32> = data.chunks_exact(12).map(|vertex| vertex[7]).collect();
        let min = colors.iter().copied().fold(f32::INFINITY, f32::min);
        let max = colors.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 0.2,
            "Greedy patch must interpolate actual depth, not patch average: {min}..{max}"
        );
    }

    #[test]
    fn water_gradient_metadata_matches_across_adjacent_render_chunks() {
        use crate::world::terrain::SmoothVoxel;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [32, 16, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..32 {
                for y in 0..if x < 16 { 6 } else { 12 } {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        let left = build_voxel_water_region(&grid, 30.0, 0, 16, 0, 16);
        let right = build_voxel_water_region(&grid, 30.0, 16, 32, 0, 16);
        let samples = |data: &Vec<f32>| {
            data.chunks_exact(12)
                .filter(|v| (v[0] - 32.0).abs() < 0.01 && (v[2] - 16.0).abs() < 0.01)
                .map(|v| [v[6], v[7]])
                .collect::<Vec<_>>()
        };
        let a = samples(&left);
        let b = samples(&right);
        assert!(!a.is_empty() && !b.is_empty(), "Must sample shared edge");
        for color in &a {
            assert!(
                b.contains(color),
                "Chunk depths or foam discontinuous across shared vertex: {color:?} / {b:?}"
            );
        }
    }

    #[test]
    fn unequal_adjacent_water_coverage_cannot_create_wave_displacement_seams() {
        use crate::world::terrain::SmoothVoxel;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [32, 16, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..32 {
                let column_height = if x < 6 { 14 } else { 2 };
                for y in 0..column_height {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        let left = build_voxel_water_region(&grid, 20.0, 0, 16, 0, 16);
        let right = build_voxel_water_region(&grid, 20.0, 16, 32, 0, 16);
        let boundary = |data: &Vec<f32>| -> Vec<(u32, u32)> {
            let mut wave: Vec<(u32, u32)> = data
                .chunks_exact(12)
                .filter(|v| (v[0] - 32.0).abs() < 0.00001)
                .map(|v| (v[2].to_bits(), v[8].to_bits()))
                .collect();
            wave.sort_unstable();
            wave.dedup();
            wave
        };
        assert!(!boundary(&left).is_empty());
        assert_eq!(
            boundary(&left),
            boundary(&right),
            "wave-energy attributes must match across differently covered chunks"
        );
    }

    #[test]
    fn wet_to_shore_wave_tile_boundary_has_no_t_junctions() {
        use crate::world::terrain::SmoothVoxel;
        use std::collections::BTreeMap;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [16, 16, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..16 {
                let height = if x == 5 && z < 2 { 14 } else { 2 };
                for y in 0..height {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        let data = build_voxel_water_region(&grid, 20.0, 0, 16, 0, 16);
        let mut edges = BTreeMap::<(u32, u32), usize>::new();
        for tri in data.chunks_exact(36) {
            let corners = [(tri[0], tri[2]), (tri[12], tri[14]), (tri[24], tri[26])];
            for i in 0..3 {
                let a = corners[i];
                let b = corners[(i + 1) % 3];
                if (a.0 - 8.0).abs() > 0.0001 || (b.0 - 8.0).abs() > 0.0001 {
                    continue;
                }
                let lo = a.1.min(b.1);
                let hi = a.1.max(b.1);
                if lo < 0.0 || hi > 4.0 || hi - lo < 0.001 {
                    continue;
                }
                *edges.entry((lo.to_bits(), hi.to_bits())).or_default() += 1;
            }
        }
        assert!(!edges.is_empty(), "no interior water seam edges found");
        assert!(
            edges.values().all(|count| *count == 2),
            "each seam edge must be shared by both tessellation levels: {edges:?}"
        );
        assert!(
            edges
                .keys()
                .all(|(a, b)| f32::from_bits(*b) - f32::from_bits(*a) <= 2.0001),
            "the coarse tile must split to the neighboring one-cell shore edge"
        );
    }

    #[test]
    fn mixed_shoreline_retains_greedy_interior_instead_of_tessellating_whole_chunk() {
        use crate::world::terrain::SmoothVoxel;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [16, 16, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..16 {
                let column_height = if x < 2 { 14 } else { 2 };
                for y in 0..column_height {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        let water = build_voxel_water_region(&grid, 20.0, 0, 16, 0, 16);
        assert!(!water.is_empty());
        assert_eq!(water.len() % (3 * 12), 0);
        assert!(
            water.len() / 36 < 256,
            "mixed shore must retain coarser open-water tiles, got {} triangles",
            water.len() / 36
        );
        assert!(
            water
                .chunks_exact(12)
                .all(|vertex| vertex[0].is_finite() && vertex[2].is_finite() && vertex[1] == 20.0)
        );
        assert!(water.chunks_exact(12).all(|v| v[0] >= 2.0));
    }

    #[test]
    fn mixed_shoreline_adjacent_chunks_agree_at_shared_world_boundary() {
        use crate::world::terrain::SmoothVoxel;
        let mut grid =
            SmoothVoxelGrid::empty_with_spacing([0, 0, 0], [32, 16, 16], 2.0, vec!["sand".into()])
                .unwrap();
        for z in 0..16 {
            for x in 0..32 {
                let column_height = if z < 5 { 14 } else { 2 };
                for y in 0..column_height {
                    grid.set_voxel(x, y, z, SmoothVoxel::from_unit_occupancy(0, 1.0));
                }
            }
        }
        let left = build_voxel_water_region(&grid, 20.0, 0, 16, 0, 16);
        let right = build_voxel_water_region(&grid, 20.0, 16, 32, 0, 16);
        let seam = |stream: &Vec<f32>| -> Vec<(u32, u32)> {
            let mut values: Vec<(u32, u32)> = stream
                .chunks_exact(12)
                .filter(|v| (v[0] - 32.0).abs() < 0.0001)
                .map(|v| (v[2].to_bits(), v[7].to_bits()))
                .collect();
            values.sort_unstable();
            values.dedup();
            values
        };
        let a = seam(&left);
        let b = seam(&right);
        assert!(!a.is_empty() && !b.is_empty());
        assert_eq!(
            a, b,
            "adjacent mixed-water chunks must sample identical shoreline depth metadata"
        );
    }

    #[test]
    fn greedy_water_mesh_collapses_full_chunk_to_one_patch() {
        let cells = vec![
            Some(WaterCell {
                depth: 6.0,
                class: water_merge_class(6.0),
            });
            16 * 16
        ];
        let patches = greedy_water_patches(&cells, 16, 16);

        assert_eq!(patches.len(), 1);
        assert_eq!(
            patches[0],
            WaterPatch {
                x0: 0,
                x1: 16,
                z0: 0,
                z1: 16,
                average_depth: 6.0,
            }
        );
    }

    #[test]
    fn greedy_water_mesh_never_spans_dry_holes() {
        let mut cells = vec![
            Some(WaterCell {
                depth: 6.0,
                class: water_merge_class(6.0),
            });
            4 * 4
        ];
        cells[1 * 4 + 1] = None;

        let patches = greedy_water_patches(&cells, 4, 4);
        assert!(patches.len() > 1);
        for patch in patches {
            assert!(
                !(patch.x0 <= 1 && patch.x1 > 1 && patch.z0 <= 1 && patch.z1 > 1),
                "a merged water patch covered the dry hole"
            );
        }
    }

    #[test]
    fn greedy_water_mesh_preserves_shore_depth_classes() {
        let shallow = WaterCell {
            depth: 0.25,
            class: water_merge_class(0.25),
        };
        let deep = WaterCell {
            depth: 6.0,
            class: water_merge_class(6.0),
        };
        let cells = vec![Some(shallow), Some(deep)];
        let patches = greedy_water_patches(&cells, 2, 1);

        assert_eq!(patches.len(), 2);
        assert!(water_surface_attributes(patches[0].average_depth, 0.5)[0] > 0.8);
        assert_eq!(
            water_surface_attributes(patches[1].average_depth, 0.5)[0],
            0.0
        );
    }

    #[test]
    fn grass_material_color_is_unmodified_in_terrain_and_grass_blades() {
        let shader = super::super::GRID_FRAGMENT_SHADER;
        assert!(shader.contains("surface_tex*grass"));
        assert!(shader.contains("snow_tex*snow + mud_tex*mud"));
        assert!(shader.contains("u_terrain_vertex_layers"));
        assert!(shader.contains("float coverage=clamp(native_paint.a"));
        assert!(!shader.contains("u_terrain_grass_green"));
        assert!(!shader.contains("surface_tex.r * 0.67"));
        assert!(!shader.contains("surface_tex.b*0.65"));

        let materials = TerrainMaterials::default();
        assert_eq!(
            terrain_palette_visual("PBR:Grass", &materials).1,
            [1.0, 1.0, 1.0]
        );
        let mut grass = Vec::new();
        push_grass_clump(&mut grass, Vec3::ZERO, 1.0, 0.0);
        assert_eq!(grass.len(), 72);
        for vertex in grass.chunks_exact(12) {
            assert_eq!(&vertex[6..10], &[1.0, 1.0, 1.0, 1.0]);
        }
    }

    #[test]
    fn empty_generated_materials_have_distinct_fallback_visuals() {
        let materials = TerrainMaterials {
            surface: String::new(),
            subsurface: String::new(),
            rock: String::new(),
            water: String::new(),
        };
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_SURFACE_MATERIAL, &materials),
            ("", [1.0, 1.0, 1.0])
        );
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_SUBSURFACE_MATERIAL, &materials),
            ("", [0.46, 0.31, 0.16])
        );
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_ROCK_MATERIAL, &materials),
            ("", [0.48, 0.48, 0.46])
        );
    }

    #[test]
    fn procedural_grass_is_deterministic_and_respects_enabled_density() {
        let config = TerrainConfig::default();
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 32, 16],
            },
        )
        .unwrap();
        let TerrainData::Voxel(grid) = &result.data else {
            panic!("expected voxel Terrain");
        };

        let first = build_voxel_grass(
            grid,
            config.plains.water_level,
            config.grass,
            &config.materials,
            config.plains.seed,
        );
        let second = build_voxel_grass(
            grid,
            config.plains.water_level,
            config.grass,
            &config.materials,
            config.plains.seed,
        );
        assert_eq!(first, second);
        assert!(!first.is_empty());

        let mut disabled = config.grass;
        disabled.enabled = false;
        assert!(
            build_voxel_grass(
                grid,
                config.plains.water_level,
                disabled,
                &config.materials,
                config.plains.seed,
            )
            .is_empty()
        );

        let mut zero_density = config.grass;
        zero_density.density = 0.0;
        assert!(
            build_voxel_grass(
                grid,
                config.plains.water_level,
                zero_density,
                &config.materials,
                config.plains.seed,
            )
            .is_empty()
        );
    }

    #[test]
    fn procedural_grass_requires_surface_material_and_stays_above_water() {
        let materials = TerrainMaterials::default();
        let settings = TerrainGrassSettings::default();

        let rock_grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [1, 1, 1],
            vec!["grass_1".into(), "tile_24".into()],
            vec![crate::world::SmoothVoxel::from_unit_occupancy(1, 1.0)],
        )
        .unwrap();
        assert!(build_voxel_grass(&rock_grid, -10.0, settings, &materials, 1337,).is_empty());

        let surface_grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [1, 1, 1],
            vec!["grass_1".into()],
            vec![crate::world::SmoothVoxel::from_unit_occupancy(0, 1.0)],
        )
        .unwrap();
        assert!(build_voxel_grass(&surface_grid, 10.0, settings, &materials, 1337,).is_empty());
    }

    #[test]
    fn grass_density_safety_ceiling_preserves_normal_regions_and_bounds_wide_regions() {
        let normal_columns = 64_u64 * 64_u64;
        let normal_target = (MAX_PROCEDURAL_GRASS_CLUMPS as f32 / normal_columns as f32)
            .min(MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN as f32);
        assert_eq!(normal_target, MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN as f32);

        let wide_columns = 1024_u64 * 1024_u64;
        let wide_target = (MAX_PROCEDURAL_GRASS_CLUMPS as f32 / wide_columns as f32)
            .min(MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN as f32);
        assert!(wide_target < 1.0);
        assert!((wide_target * wide_columns as f32) <= MAX_PROCEDURAL_GRASS_CLUMPS as f32 + 1.0);
    }

    #[test]
    fn higher_grass_density_produces_at_least_as_much_geometry() {
        let config = TerrainConfig::default();
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 32, 16],
            },
        )
        .unwrap();
        let TerrainData::Voxel(grid) = &result.data else {
            panic!("expected voxel Terrain");
        };

        let mut low = config.grass;
        low.density = 0.1;
        let mut high = config.grass;
        high.density = 0.9;

        let low_vertices = build_voxel_grass(
            grid,
            config.plains.water_level,
            low,
            &config.materials,
            config.plains.seed,
        );
        let high_vertices = build_voxel_grass(
            grid,
            config.plains.water_level,
            high,
            &config.materials,
            config.plains.seed,
        );
        assert!(high_vertices.len() >= low_vertices.len());
    }

    #[test]
    fn parked_shoreline_changes_wave_settings_without_rebuilding_water_mesh() {
        assert!(!ADVANCED_SHORELINE_ENABLED);
        assert!(!WET_SAND_MATERIAL_ENABLED);
        let baseline = TerrainConfig::default();
        let key = terrain_render_config_key(&baseline);
        let mut changed = baseline.clone();
        changed.water.wave_height += 2.0;
        changed.water.wave_speed += 1.0;
        assert_eq!(terrain_render_config_key(&changed), key);
        assert_eq!(geometry_wave_parameters(&changed), (0.0, 0.0));
        changed.ocean_enabled = !baseline.ocean_enabled;
        assert_ne!(terrain_render_config_key(&changed), key);
    }

    #[test]
    fn grass_render_config_key_changes_with_density_length_and_enabled_state() {
        let config = TerrainConfig::default();
        let base = terrain_render_config_key(&config);

        let mut changed = config.clone();
        changed.grass.density = 0.9;
        assert_ne!(base, terrain_render_config_key(&changed));

        let mut changed = config.clone();
        changed.grass.length_studs = 2.0;
        assert_ne!(base, terrain_render_config_key(&changed));

        let mut changed = config.clone();
        changed.grass.enabled = false;
        assert_ne!(base, terrain_render_config_key(&changed));
    }

    #[test]
    fn sculpting_updates_logical_signature_and_mesh() {
        let mut world = World::new();
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 32, 8],
            })
            .unwrap();
        let initial_sig = logical_signature(world.terrain.authored.as_ref().unwrap());

        world.sculpt_authored_terrain(&crate::world::terrain::TerrainSculptRequest {
            tool: crate::world::terrain::SculptTool::Raise,
            center: [4.5, 0.0, 4.5],
            radius: 3.0,
            strength: 2.0,
            flatten_target: None,
        });

        let sculpted_sig = logical_signature(world.terrain.authored.as_ref().unwrap());
        assert_ne!(initial_sig, sculpted_sig);

        let mesh = build_terrain_mesh(
            world.terrain.authored.as_ref().unwrap(),
            -10.0,
            world.terrain.config.voxel_layers.slope_shading_strength,
        );
        assert_eq!(mesh.source_signature, sculpted_sig);
    }

    #[test]
    fn slope_shading_keeps_flats_bright_and_darkens_steep_faces() {
        assert!((terrain_slope_shade(Vec3::Y, 0.25) - 1.0).abs() < f32::EPSILON);
        assert!((terrain_slope_shade(Vec3::X, 0.25) - 0.75).abs() < 0.001);
        let diagonal = terrain_slope_shade(Vec3::new(1.0, 1.0, 0.0), 0.25);
        assert!(diagonal > 0.75 && diagonal < 1.0);
        assert_eq!(terrain_slope_shade(Vec3::X, 0.0), 1.0);
    }

    #[test]
    fn parked_shoreline_never_rebuilds_ocean_mesh_for_wave_controls() {
        let mut config = TerrainConfig::default();
        assert!(!ADVANCED_SHORELINE_ENABLED);
        assert!(!WET_SAND_MATERIAL_ENABLED);
        assert_eq!(geometry_wave_parameters(&config), (0.0, 0.0));
        let before = terrain_render_config_key(&config);
        config.water.wave_height = 3.0;
        config.water.wave_speed = 2.7;
        assert_eq!(
            before,
            terrain_render_config_key(&config),
            "wave controls belong to GPU uniforms, not a terrain rebuild"
        );
        assert!(WATER_VERTEX_SHADER.contains("ocean_wave_field(world.xz,u_time)"));
        assert!(WATER_VERTEX_SHADER.contains("u_wake_count"));
        assert!(WATER_VERTEX_SHADER.contains("0.15*sin(u_time*0.013)"));
    }

    #[test]
    fn terrain_render_config_key_changes_for_visual_settings_without_logical_hashing() {
        let config = TerrainConfig::default();
        let base = terrain_render_config_key(&config);

        let mut changed = config.clone();
        changed.voxel_layers.slope_shading_strength = 0.31;
        assert_ne!(base, terrain_render_config_key(&changed));

        let mut changed = config.clone();
        changed.grass.density = 0.77;
        assert_ne!(base, terrain_render_config_key(&changed));

        let mut changed = config.clone();
        changed.materials.surface = "custom_surface".into();
        assert_ne!(base, terrain_render_config_key(&changed));
    }

    #[test]
    fn water_chunk_frustum_bounds_include_sea_level_above_solid_terrain() {
        let mut world = World::new();
        world
            .terrain
            .generate_authored(crate::world::terrain::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 24, 16],
            })
            .unwrap();
        let result = world.terrain.effective().unwrap();
        let (solid_min, solid_max) = terrain_render_chunk_aabb(result, [0, 0]).unwrap();
        let water_above = solid_max.y + 40.0;
        let (water_min, water_max) = terrain_water_chunk_aabb(result, [0, 0], water_above).unwrap();
        assert_eq!(water_min, solid_min);
        assert_eq!(water_max.y, water_above);
        assert_eq!(water_max.x, solid_max.x);
        let below_floor = solid_min.y - 12.0;
        let (water_min, water_max) = terrain_water_chunk_aabb(result, [0, 0], below_floor).unwrap();
        assert_eq!(water_min.y, below_floor);
        assert_eq!(water_max.y, solid_max.y);
    }

    #[test]
    fn terrain_render_chunks_track_world_space_target_across_resolutions() {
        assert_eq!(terrain_render_chunk_samples(1.0), 64);
        assert_eq!(terrain_render_chunk_samples(2.0), 32);
        assert_eq!(terrain_render_chunk_samples(4.0), 16);
    }

    #[test]
    fn terrain_render_distance_rejects_far_chunk_bounds() {
        let near_min = Vec3::new(0.0, 0.0, 0.0);
        let near_max = Vec3::new(64.0, 32.0, 64.0);
        let far_min = Vec3::new(512.0, 0.0, 512.0);
        let far_max = Vec3::new(576.0, 32.0, 576.0);

        assert!(terrain_chunk_within_render_distance(
            near_min,
            near_max,
            Vec3::ZERO,
            4,
        ));
        assert!(!terrain_chunk_within_render_distance(
            far_min,
            far_max,
            Vec3::ZERO,
            4,
        ));
        assert!(terrain_chunk_within_render_distance(
            far_min,
            far_max,
            Vec3::ZERO,
            0,
        ));
    }

    #[test]
    fn terrain_frustum_rejects_offscreen_chunk_bounds() {
        let frustum = CameraFrustum::from_view_projection(Mat4::IDENTITY);
        assert!(frustum.intersects_aabb(Vec3::splat(-0.5), Vec3::splat(0.5),));
        assert!(!frustum.intersects_aabb(Vec3::new(4.0, -0.5, -0.5), Vec3::new(5.0, 0.5, 0.5),));
    }

    #[test]
    fn independent_water_batches_keep_material_instances_and_river_flow() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut config = TerrainConfig::default();
        config.ocean_enabled = false;
        config.grass.enabled = false;
        let mut lake = WaterBody::new(71, WaterBodyKind::Lake, [16.0, 16.0], 10.0);
        let mut river = WaterBody::new(72, WaterBodyKind::River, [38.0, 38.0], 11.0);
        lake.appearance.as_mut().unwrap().wave_height = 0.1;
        river.appearance.as_mut().unwrap().wave_height = 2.0;
        river.points[0].flow_speed = 7.0;
        river.points[1].flow_speed = 3.0;
        config.water_bodies = vec![lake, river];
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [64, 8, 64],
            },
        )
        .unwrap();
        let batches = terrain_render_chunk_keys(&result)
            .into_iter()
            .flat_map(|key| build_terrain_mesh_chunk(&result, &config, key).batches)
            .filter(|b| b.material == TerrainMaterialSlot::Water)
            .collect::<Vec<_>>();
        assert!(batches.iter().any(|b| b.water_body_id == Some(71)));
        assert!(batches.iter().any(|b| b.water_body_id == Some(72)));
        assert!(batches.iter().all(|b| b.water_body_id.is_some()));
        assert!(
            batches
                .iter()
                .filter(|b| b.water_body_id == Some(72))
                .any(|b| b
                    .vertices
                    .chunks_exact(12)
                    .any(|v| v[10].abs() + v[11].abs() > 0.1))
        );
        let before = terrain_render_config_key(&config);
        config.water_bodies[0]
            .appearance
            .as_mut()
            .unwrap()
            .wave_height = 2.8;
        assert_eq!(
            before,
            terrain_render_config_key(&config),
            "appearance changes must not rebuild water geometry"
        );
        config.water_bodies[0].material_asset = Some("stable-project-material".into());
        assert_eq!(
            before,
            terrain_render_config_key(&config),
            "Material assignment must not rebuild geometry"
        );
        config.water_bodies[1].points[0].flow_speed = 11.0;
        assert_ne!(
            before,
            terrain_render_config_key(&config),
            "current vector changes require the cached vertex attributes to refresh"
        );
    }

    #[test]
    fn lake_and_river_generate_visible_geometry_without_ocean() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut config = TerrainConfig::default();
        config.ocean_enabled = false;
        config.grass.enabled = false;
        config
            .water_bodies
            .push(WaterBody::new(1, WaterBodyKind::Lake, [16.0, 16.0], 10.0));
        config
            .water_bodies
            .push(WaterBody::new(2, WaterBodyKind::River, [38.0, 38.0], 11.0));
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [64, 8, 64],
            },
        )
        .unwrap();
        let keys = terrain_render_chunk_keys(&result);
        let mut water = 0usize;
        for key in keys {
            let mesh = build_terrain_mesh_chunk(&result, &config, key);
            water += mesh
                .batches
                .iter()
                .filter(|b| b.material == TerrainMaterialSlot::Water)
                .map(|b| b.vertices.len())
                .sum::<usize>();
        }
        assert!(
            water > 0,
            "creator water bodies must emit real water triangles"
        );
        assert_eq!(water % 36, 0);
        config
            .water_bodies
            .iter_mut()
            .for_each(|b| b.visible = false);
        let empty = terrain_render_chunk_keys(&result)
            .into_iter()
            .flat_map(|key| build_terrain_mesh_chunk(&result, &config, key).batches)
            .filter(|b| b.material == TerrainMaterialSlot::Water)
            .map(|b| b.vertices.len())
            .sum::<usize>();
        assert_eq!(
            empty, 0,
            "hidden bodies and disabled ocean must draw no water"
        );
    }

    #[test]
    fn independent_lake_body_seams_sample_the_same_world_positions() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let lake = WaterBody::new(7, WaterBodyKind::Lake, [32.0, 16.0], 12.0);
        let a = build_water_body_region(&lake, [0.0, 0.0], [16, 16], 2.0, |_, _| Some(0.0));
        let b = build_water_body_region(&lake, [32.0, 0.0], [16, 16], 2.0, |_, _| Some(0.0));
        assert!(!a.is_empty() && !b.is_empty());
        let edge = |vs: &Vec<f32>| {
            let mut out: Vec<(u32, u32, u32)> = vs
                .chunks_exact(12)
                .filter(|v| (v[0] - 32.0).abs() < 0.0001)
                .map(|v| (v[2].to_bits(), v[6].to_bits(), v[8].to_bits()))
                .collect();
            out.sort_unstable();
            out.dedup();
            out
        };
        assert!(!edge(&a).is_empty());
        assert_eq!(edge(&a), edge(&b), "cross-chunk lake surface must match");
    }

    #[test]
    fn ocean_visibility_controls_real_render_geometry_and_underwater_state() {
        let mut config = TerrainConfig::default();
        config.plains.base_height = 3.0;
        config.plains.height_variation = 0.0;
        config.plains.water_level = 11.0;
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 24, 16],
            },
        )
        .unwrap();
        let first = build_terrain_mesh_chunk(&result, &config, [0, 0]);
        assert!(
            first
                .batches
                .iter()
                .any(|b| b.material == TerrainMaterialSlot::Water && !b.vertices.is_empty())
        );
        let cache_before = terrain_render_config_key(&config);
        config.ocean_enabled = false;
        assert_ne!(cache_before, terrain_render_config_key(&config));
        let without = build_terrain_mesh_chunk(&result, &config, [0, 0]);
        assert!(
            !without
                .batches
                .iter()
                .any(|b| b.material == TerrainMaterialSlot::Water)
        );
        let mut world = World::new();
        world.terrain.config = config;
        world.terrain.replace_authored(Some(result));
        assert!(super::underwater_state(&world, Vec3::new(8.0, 8.0, 8.0)).is_none());
    }

    #[test]
    fn native_polygon_chunks_have_one_landscape_representation() {
        let mut config = TerrainConfig::default();
        config.grass.enabled = false;
        config.plains.water_level = -100.0;
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [136, 24, 136],
            },
        )
        .unwrap();
        let keys = terrain_render_chunk_keys(&result);
        assert!(keys.len() > 1);
        let chunks: Vec<_> = keys
            .iter()
            .map(|key| build_terrain_mesh_chunk(&result, &config, *key))
            .collect();
        let terrain_vertices: usize = chunks
            .iter()
            .flat_map(|chunk| {
                chunk.batches.iter().filter(|batch| {
                    !matches!(
                        batch.material,
                        TerrainMaterialSlot::Water | TerrainMaterialSlot::Grass
                    )
                })
            })
            .map(|batch| batch.vertices.len())
            .sum();
        assert!(
            terrain_vertices > 0,
            "native polygon landscape surface must render"
        );
        assert_eq!(terrain_vertices % 36, 0);
    }
}

#[cfg(test)]
mod water_material_ocean_tests {
    use super::*;
    use crate::project::material::*;
    #[test]
    fn water_material_ocean_optics_follow_shared_asset_and_preserve_geometry_key() {
        let mut look = crate::world::terrain::TerrainWaterSettings::default();
        look.underwater_fog_density = 0.27;
        look.underwater_fog_color = [0.1, 0.2, 0.3];
        let doc = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(look),
        });
        let root = std::env::temp_dir().join(&doc.id);
        std::fs::create_dir_all(&root).unwrap();
        save(&root, "ocean", &doc, None).unwrap();
        let catalog = MaterialCatalog::load(&root);
        let mut world = World::new();
        world.terrain.config.plains.base_height = 3.0;
        world.terrain.config.plains.height_variation = 0.0;
        world.terrain.config.plains.water_level = 9.0;
        world
            .generate_authored_terrain(crate::world::terrain::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 16, 16],
            })
            .unwrap();
        let key = terrain_render_config_key(&world.terrain.config);
        world.terrain.config.water_material_asset = Some(doc.id.clone());
        assert_eq!(key, terrain_render_config_key(&world.terrain.config));
        let state =
            underwater_state_with_material(&world, Vec3::new(8.0, 7.0, 8.0), &catalog).unwrap();
        assert_eq!(state.density, look.underwater_fog_density);
        assert_eq!(state.color, Vec3::from_array(look.underwater_fog_color));
        assert_eq!(world.terrain.config.resolved_water(&catalog), look);
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod background_project_terrain_tests {
    use super::*;
    #[test]
    fn flatten_radius_two_edits_only_local_terrain_and_times_mesh_build() {
        use std::time::Instant;
        let mut world = World::new();
        world.terrain.config.grass.enabled = false;
        world.terrain.config.ocean_enabled = false;
        world.terrain.config.plains.base_height = 20.0;
        world.terrain.config.plains.height_variation = 6.0;
        let generated = Instant::now();
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [128, 48, 128],
            })
            .unwrap();
        let generate_ms = generated.elapsed().as_secs_f64() * 1000.0;
        let before_revision = world.terrain.render_revision();
        let before_full = world.terrain.full_render_revision();
        let center = [45.25, 0.0, 55.25];
        let original_height = world
            .terrain
            .surface_height_at(center[0], center[2])
            .unwrap();
        let request = crate::world::terrain::TerrainSculptRequest {
            tool: crate::world::terrain::SculptTool::Flatten,
            center: [center[0], original_height, center[2]],
            radius: 2.0,
            strength: 1.0,
            flatten_target: Some(original_height + 2.0),
        };
        let mut edit_max_ms = 0.0_f64;
        for _ in 0..5 {
            let start = Instant::now();
            assert!(world.sculpt_authored_terrain(&request));
            edit_max_ms = edit_max_ms.max(start.elapsed().as_secs_f64() * 1000.0);
        }
        let keys = terrain_render_chunk_keys(world.terrain.effective().unwrap());
        let dirty: Vec<_> = keys
            .into_iter()
            .filter(|&key| world.terrain.render_chunk_revision(key) > before_revision)
            .collect();
        assert!(
            !dirty.is_empty(),
            "flatten radius 2 must dirty render chunks"
        );
        assert!(
            dirty.len() <= 4,
            "radius 2 dirtied {} render chunks",
            dirty.len()
        );
        assert_eq!(
            world.terrain.full_render_revision(),
            before_full,
            "small local brush must never invalidate the whole terrain"
        );
        let mut mesh_total_ms = 0.0_f64;
        let result = world.terrain.effective().unwrap();
        for key in &dirty {
            let started = Instant::now();
            let mesh = build_terrain_mesh_chunk(result, &world.terrain.config, *key);
            assert!(!mesh.batches.is_empty());
            mesh_total_ms += started.elapsed().as_secs_f64() * 1000.0;
        }
        eprintln!(
            "FLATTEN radius=2 generation_ms={generate_ms:.3}, edit_max_ms={edit_max_ms:.3}, dirty_chunks={}, mesh_cpu_ms={mesh_total_ms:.3}",
            dirty.len()
        );
    }

    #[test]
    fn background_project_cpu_terrain_preserves_meshes_and_obeys_cancellation() {
        let mut world = World::new();
        world.terrain.config.grass.enabled = false;
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [32, 8, 32],
            })
            .unwrap();
        let prepared = prepare_project_terrain(&world, &|| true).unwrap().unwrap();
        assert_eq!(prepared.revision, world.terrain.render_revision());
        assert_eq!(
            prepared.config_key,
            terrain_render_config_key(&world.terrain.config)
        );
        let result = world.terrain.effective().unwrap();
        assert_eq!(
            prepared.chunks.len(),
            terrain_render_chunk_keys(result).len()
        );
        for (key, mesh) in prepared.chunks {
            let expected = build_terrain_mesh_chunk(result, &world.terrain.config, key);
            assert_eq!(mesh.batches.len(), expected.batches.len());
            for (actual, expected) in mesh.batches.iter().zip(&expected.batches) {
                assert_eq!(actual.material, expected.material);
                assert_eq!(actual.water_body_id, expected.water_body_id);
                assert_eq!(actual.vertices, expected.vertices);
            }
        }
        assert!(prepare_project_terrain(&world, &|| false).is_err());
    }
}

#[cfg(test)]
mod generated_heightmap_chunk_isolation_regression {
    use super::*;
    use crate::world::{
        HeightfieldData, TerrainData, TerrainGenerationRequest, TerrainGeneratorType,
        TerrainResult, World,
    };

    #[test]
    fn generated_plains_and_heightmap_have_geometry_without_gap_triangles() {
        let mut world = World::new();
        world.terrain.config.ocean_enabled = false;
        world.terrain.config.grass.enabled = false;
        let id = world
            .terrain
            .add_generation_layer(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [24, 40, 24],
            })
            .unwrap();
        {
            let layer = &mut world.terrain.config.generation_layers[0];
            layer.settings.base_height = 10.0;
            layer.settings.relief = 0.0;
            layer.settings.water_level = 0.0;
        }
        world.terrain.generate_selected_layer(id).unwrap();
        let request = TerrainGenerationRequest {
            position: [40, 0, 0],
            size: [24, 40, 24],
        };
        world
            .import_heightmap_layer_with_water_level(
                TerrainResult {
                    request,
                    generator: TerrainGeneratorType::Programmatic,
                    generator_implementation_version: 1,
                    data: TerrainData::Heightfield(HeightfieldData {
                        origin: request.position,
                        size: request.size.map(|n| n as u32),
                        heights: vec![22.0; 24 * 24],
                    }),
                },
                0.0,
            )
            .unwrap();
        let result = world.terrain.effective().unwrap();
        let mut plains = 0usize;
        let mut imported = 0usize;
        let mut gap = 0usize;
        for key in terrain_render_chunk_keys(result) {
            let mesh = build_terrain_mesh_chunk(result, &world.terrain.config, key);
            for batch in mesh.batches {
                if matches!(
                    batch.material,
                    TerrainMaterialSlot::Water | TerrainMaterialSlot::Grass
                ) {
                    continue;
                }
                for triangle in batch.vertices.chunks_exact(36) {
                    let x = (triangle[0] + triangle[12] + triangle[24]) / 3.0;
                    let z = (triangle[2] + triangle[14] + triangle[26]) / 3.0;
                    if !(z > 5.0 && z < 19.0) {
                        continue;
                    }
                    if x > 8.0 && x < 16.0 {
                        plains += 1;
                    }
                    if x > 48.0 && x < 56.0 {
                        imported += 1;
                    }
                    if x > 29.0 && x < 35.0 {
                        gap += 1;
                    }
                }
            }
        }
        println!(
            "MESH_LAYER_GEOMETRY plains={} imported={} phantom_gap={}",
            plains, imported, gap
        );
        assert!(
            plains > 0,
            "Plains must have real triangles after importing a heightmap"
        );
        assert!(
            imported > 0,
            "The imported heightmap must have real triangles"
        );
        assert_eq!(
            gap, 0,
            "Unowned gap must not be rendered as a phantom minimum-height sheet"
        );
    }
}
