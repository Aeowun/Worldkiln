use std::collections::{BTreeMap, hash_map::DefaultHasher};
use std::hash::{Hash, Hasher};

use glam::{Mat4, Vec3};

use crate::world::{
    CHUNK_SIZE, HeightfieldData, SMOOTH_VOXEL_ISO_LEVEL, SmoothVoxelGrid, TerrainConfig,
    TerrainData, TerrainGrassSettings, TerrainMaterials, TerrainMode, TerrainRenderChunkKey,
    TerrainResult, World,
};
// Worldkiln 0.9.3 | WO 0.9.3-06 | 2026-10-06 23:14 -05:00
use crate::world::terrain::{
    TERRAIN_FALLBACK_ROCK_MATERIAL, TERRAIN_FALLBACK_SUBSURFACE_MATERIAL,
    TERRAIN_FALLBACK_SURFACE_MATERIAL, terrain_render_chunk_samples,
};

use super::Renderer;
use super::chunk::CameraFrustum;
use super::mesh::{add_block_quad, upload_block_vertices_3d};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerrainMaterialSlot {
    Surface,
    Subsurface,
    Rock,
    Water,
    // Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
    Grass,
    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:55 -05:00
    Palette(u16),
}

#[derive(Clone, Copy, Debug, Default)]
pub(super) struct TerrainDrawStats {
    pub draws: u64,
    pub triangles: u64,
    pub chunks_drawn: u64,
    pub chunks_culled: u64,
    // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
    // Optional, bounded CPU attribution inside WO46's coarse Terrain stage.
    pub texture_lookup_ns: u64,
    pub draw_submission_ns: u64,
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
}

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
pub(super) struct TerrainGpuMesh {
    revision: u64,
    config_key: u64,
    chunks: BTreeMap<TerrainRenderChunkKey, TerrainGpuChunk>,
}

struct TerrainGpuChunk {
    batches: Vec<TerrainGpuBatch>,
}

struct TerrainGpuBatch {
    material: TerrainMaterialSlot,
    vao: u32,
    vbo: u32,
    vertex_count: i32,
}

// Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
pub(super) const UNDERWATER_VOLUME_COLOR: Vec3 = Vec3::new(0.055, 0.24, 0.32);
pub(super) const UNDERWATER_VOLUME_DENSITY: f32 = 0.065;
pub(super) const UNDERWATER_SKY_BLEND: f32 = 0.88;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct UnderwaterState {
    pub color: Vec3,
    pub density: f32,
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

pub(super) fn underwater_state(world: &World, camera_position: Vec3) -> Option<UnderwaterState> {
    let result = world.terrain.effective()?;
    let water_level = world.terrain.config.plains.water_level;
    let volume = underwater_volume_for_result(result, water_level)?;
    if !volume.contains(camera_position) {
        return None;
    }

    // The volume is bounded like an Unreal post-process box, but activation
    // still follows the actual fluid column. High ground below Water Level's
    // Y plane is not water, and camera positions inside solid Terrain are not
    // treated as submerged.
    let terrain_surface = world
        .terrain
        .surface_height_at(camera_position.x, camera_position.z)?;
    if terrain_surface >= water_level - 0.01 || camera_position.y <= terrain_surface + 0.01 {
        return None;
    }

    Some(UnderwaterState {
        color: UNDERWATER_VOLUME_COLOR,
        density: UNDERWATER_VOLUME_DENSITY,
        surface_y: volume.max.y,
    })
}

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
// Worldkiln 0.9.3 | WO 0.9.3-05 | 2026-10-06 23:14 -05:00
pub fn build_terrain_mesh(
    result: &TerrainResult,
    mode: TerrainMode,
    water_level: f32,
    slope_shading_strength: f32,
) -> TerrainMesh {
    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
    let mut mesh = match &result.data {
        TerrainData::Heightfield(heightfield) => {
            let mut mesh = match mode {
                TerrainMode::Smooth => build_smooth(heightfield),
                TerrainMode::Block => build_block(heightfield),
            };
            let water = build_water(heightfield, water_level);
            if !water.is_empty() {
                mesh.batches.push(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Water,
                    vertices: water,
                });
            }
            mesh
        }
        // Phase 2 makes the payload persistent without activating it in the
        // renderer. Phase 3 installs occupancy contouring before generation can
        // produce TerrainData::Voxel.
        TerrainData::Voxel(grid) => {
            let mut mesh = match mode {
                TerrainMode::Smooth => build_voxel_smooth(grid, slope_shading_strength),
                TerrainMode::Block => build_voxel_block(grid, slope_shading_strength),
            };
            let water = build_voxel_water(grid, water_level);
            if !water.is_empty() {
                mesh.batches.push(TerrainMeshBatch {
                    material: TerrainMaterialSlot::Water,
                    vertices: water,
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
        }],
        source_signature: 0,
    }
}

fn build_block(heightfield: &HeightfieldData) -> TerrainMesh {
    let mut surface = Vec::new();
    let mut subsurface = Vec::new();
    let mut rock = Vec::new();
    let min_y = heightfield.origin[1] as f32;

    for z in 0..heightfield.size[2] {
        for x in 0..heightfield.size[0] {
            let world_x = heightfield.origin[0] as f32 + x as f32;
            let world_z = heightfield.origin[2] as f32 + z as f32;
            let top = heightfield.height(x, z).unwrap().round();
            add_block_quad(
                &mut surface,
                [world_x, top, world_z],
                [world_x, top, world_z + 1.0],
                [world_x + 1.0, top, world_z + 1.0],
                [world_x + 1.0, top, world_z],
                [1.0; 4],
                [0.0, 1.0, 0.0],
            );

            let west = neighbor_top(heightfield, x as i32 - 1, z as i32).unwrap_or(min_y);
            let east = neighbor_top(heightfield, x as i32 + 1, z as i32).unwrap_or(min_y);
            let north = neighbor_top(heightfield, x as i32, z as i32 - 1).unwrap_or(min_y);
            let south = neighbor_top(heightfield, x as i32, z as i32 + 1).unwrap_or(min_y);
            add_side(&mut subsurface, &mut rock, top, west, world_x, world_z, 0);
            add_side(&mut subsurface, &mut rock, top, east, world_x, world_z, 1);
            add_side(&mut subsurface, &mut rock, top, north, world_x, world_z, 2);
            add_side(&mut subsurface, &mut rock, top, south, world_x, world_z, 3);
        }
    }

    TerrainMesh {
        batches: vec![
            TerrainMeshBatch {
                material: TerrainMaterialSlot::Surface,
                vertices: surface,
            },
            TerrainMeshBatch {
                material: TerrainMaterialSlot::Subsurface,
                vertices: subsurface,
            },
            TerrainMeshBatch {
                material: TerrainMaterialSlot::Rock,
                vertices: rock,
            },
        ],
        source_signature: 0,
    }
}

fn add_side(
    subsurface: &mut Vec<f32>,
    rock: &mut Vec<f32>,
    top: f32,
    neighbor: f32,
    x: f32,
    z: f32,
    direction: u8,
) {
    let bottom = neighbor.round();
    if bottom >= top {
        return;
    }
    let band_bottom = (top - 2.0).max(bottom);
    add_vertical_quad(subsurface, x, z, band_bottom, top, direction);
    if bottom < band_bottom {
        add_vertical_quad(rock, x, z, bottom, band_bottom, direction);
    }
}

fn add_vertical_quad(vertices: &mut Vec<f32>, x: f32, z: f32, low: f32, high: f32, side: u8) {
    let (points, normal) = match side {
        0 => (
            [
                [x, high, z + 1.0],
                [x, high, z],
                [x, low, z],
                [x, low, z + 1.0],
            ],
            [-1.0, 0.0, 0.0],
        ),
        1 => (
            [
                [x + 1.0, high, z],
                [x + 1.0, high, z + 1.0],
                [x + 1.0, low, z + 1.0],
                [x + 1.0, low, z],
            ],
            [1.0, 0.0, 0.0],
        ),
        2 => (
            [
                [x, high, z],
                [x + 1.0, high, z],
                [x + 1.0, low, z],
                [x, low, z],
            ],
            [0.0, 0.0, -1.0],
        ),
        _ => (
            [
                [x + 1.0, high, z + 1.0],
                [x, high, z + 1.0],
                [x, low, z + 1.0],
                [x + 1.0, low, z + 1.0],
            ],
            [0.0, 0.0, 1.0],
        ),
    };
    add_block_quad(
        vertices, points[0], points[1], points[2], points[3], [1.0; 4], normal,
    );
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:55 -05:00
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

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
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
                    polygonize_tetrahedron(samples, &mut batches, slope_shading_strength);
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

fn polygonize_tetrahedron(
    samples: [DensitySample; 4],
    batches: &mut [Vec<f32>],
    slope_shading_strength: f32,
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

    // Worldkiln 0.8.5 | WO MAT-053 | 2026-10-10 17:31 -05:00
    // Use the nearest inside sample to the visible iso-surface, NOT the
    // densest interior voxel. Deep rock/grass previously won over a freshly
    // painted partially occupied beach/top voxel, creating fragmented
    // triangular patches that ignored the brush's apparent footprint.
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
        push_terrain_triangle(vertices, a, b, d, slope_shading_strength);
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

fn build_voxel_block(grid: &SmoothVoxelGrid, slope_shading_strength: f32) -> TerrainMesh {
    build_voxel_block_region(
        grid,
        slope_shading_strength,
        0,
        grid.size[0],
        0,
        grid.size[2],
    )
}

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
fn build_voxel_block_region(
    grid: &SmoothVoxelGrid,
    slope_shading_strength: f32,
    x_start: u32,
    x_end: u32,
    z_start: u32,
    z_end: u32,
) -> TerrainMesh {
    let mut batches = vec![Vec::new(); grid.materials.len()];
    for y in 0..grid.size[1] {
        for z in z_start..z_end {
            for x in x_start..x_end {
                let Some(voxel) = grid.voxel(x, y, z) else {
                    continue;
                };
                if voxel.occupancy_unit() < SMOOTH_VOXEL_ISO_LEVEL {
                    continue;
                }
                let Some(vertices) = batches.get_mut(voxel.material as usize) else {
                    continue;
                };
                let min = Vec3::new(
                    grid.origin[0] as f32 + x as f32 * grid.spacing_studs,
                    grid.origin[1] as f32 + y as f32 * grid.spacing_studs,
                    grid.origin[2] as f32 + z as f32 * grid.spacing_studs,
                );
                let max = min + Vec3::splat(grid.spacing_studs);
                let exposed = |nx: i32, ny: i32, nz: i32| {
                    if nx < 0 || ny < 0 || nz < 0 {
                        return true;
                    }
                    grid.voxel(nx as u32, ny as u32, nz as u32)
                        .is_none_or(|neighbor| neighbor.occupancy_unit() < SMOOTH_VOXEL_ISO_LEVEL)
                };

                if exposed(x as i32 - 1, y as i32, z as i32) {
                    add_block_quad(
                        vertices,
                        [min.x, max.y, max.z],
                        [min.x, max.y, min.z],
                        [min.x, min.y, min.z],
                        [min.x, min.y, max.z],
                        terrain_slope_color([-1.0, 0.0, 0.0], slope_shading_strength),
                        [-1.0, 0.0, 0.0],
                    );
                }
                if exposed(x as i32 + 1, y as i32, z as i32) {
                    add_block_quad(
                        vertices,
                        [max.x, max.y, min.z],
                        [max.x, max.y, max.z],
                        [max.x, min.y, max.z],
                        [max.x, min.y, min.z],
                        terrain_slope_color([1.0, 0.0, 0.0], slope_shading_strength),
                        [1.0, 0.0, 0.0],
                    );
                }
                if exposed(x as i32, y as i32 - 1, z as i32) {
                    add_block_quad(
                        vertices,
                        [min.x, min.y, min.z],
                        [max.x, min.y, min.z],
                        [max.x, min.y, max.z],
                        [min.x, min.y, max.z],
                        terrain_slope_color([0.0, -1.0, 0.0], slope_shading_strength),
                        [0.0, -1.0, 0.0],
                    );
                }
                if exposed(x as i32, y as i32 + 1, z as i32) {
                    add_block_quad(
                        vertices,
                        [min.x, max.y, max.z],
                        [max.x, max.y, max.z],
                        [max.x, max.y, min.z],
                        [min.x, max.y, min.z],
                        terrain_slope_color([0.0, 1.0, 0.0], slope_shading_strength),
                        [0.0, 1.0, 0.0],
                    );
                }
                if exposed(x as i32, y as i32, z as i32 - 1) {
                    add_block_quad(
                        vertices,
                        [min.x, max.y, min.z],
                        [max.x, max.y, min.z],
                        [max.x, min.y, min.z],
                        [min.x, min.y, min.z],
                        terrain_slope_color([0.0, 0.0, -1.0], slope_shading_strength),
                        [0.0, 0.0, -1.0],
                    );
                }
                if exposed(x as i32, y as i32, z as i32 + 1) {
                    add_block_quad(
                        vertices,
                        [max.x, max.y, max.z],
                        [min.x, max.y, max.z],
                        [min.x, min.y, max.z],
                        [max.x, min.y, max.z],
                        terrain_slope_color([0.0, 0.0, 1.0], slope_shading_strength),
                        [0.0, 0.0, 1.0],
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
                })
            })
            .collect(),
        source_signature: 0,
    }
}

fn build_voxel_water(grid: &SmoothVoxelGrid, water_level: f32) -> Vec<f32> {
    build_voxel_water_region(grid, water_level, 0, grid.size[0], 0, grid.size[2])
}

// Worldkiln 0.9.3 | WO 0.9.3-15 | 2026-10-07
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
        // Keep shoreline foam transitions local. Half-stud bands avoid smearing
        // the high-contrast foam mask across a merged patch.
        ((depth / 0.5).floor() as u8).min(4)
    } else {
        // Once foam has faded out, depth only affects broad color/opacity.
        // Treat deep water as one merge class and average depth per patch.
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

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
// Worldkiln 0.9.3 | WO 0.9.3-15 | 2026-10-07
fn build_voxel_water_region(
    grid: &SmoothVoxelGrid,
    water_level: f32,
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

    // The render chunk remains the wave-energy and rebuild domain. We classify
    // each logical column once, then greedily merge compatible water cells into
    // larger flat patches. This keeps dry holes/shorelines exact while removing
    // the one-quad-per-sample geometry explosion introduced by denser Terrain.
    let width = x_end - x_start;
    let depth = z_end - z_start;
    let mut cells = vec![None; width as usize * depth as usize];
    let mut water_cell_count = 0_usize;
    let mut depth_sum = 0.0_f32;

    // Worldkiln 0.8.7 | WO 0.8.7-38 | 2026-10-08 19:35 -05:00
    // Cache one halo of terrain-center depths. Vertices on neighboring
    // render chunks consult the same four global columns, eliminating the
    // visible rectangular color seams from one averaged depth per patch.
    let halo_width = width as usize + 2;
    let mut halo_depths = vec![0.0_f32; halo_width * (depth as usize + 2)];
    for hz in 0..depth + 2 {
        let z = (z_start as i64 + hz as i64 - 1).clamp(0, grid.size[2] as i64 - 1) as u32;
        for hx in 0..width + 2 {
            let x = (x_start as i64 + hx as i64 - 1).clamp(0, grid.size[0] as i64 - 1) as u32;
            let center_x = min[0] + (x as f32 + 0.5) * grid.spacing_studs;
            let center_z = min[2] + (z as f32 + 0.5) * grid.spacing_studs;
            let surface = grid
                .surface_height_at(center_x, center_z, TerrainMode::Smooth)
                .unwrap_or(min[1]);
            let column_depth = (water_level - surface).max(0.0);
            halo_depths[hz as usize * halo_width + hx as usize] = column_depth;

            if hz > 0 && hz <= depth && hx > 0 && hx <= width && surface < water_level {
                let cell_index = (hz - 1) as usize * width as usize + (hx - 1) as usize;
                cells[cell_index] = Some(WaterCell {
                    depth: column_depth,
                    class: water_merge_class(column_depth),
                });
                water_cell_count += 1;
                depth_sum += column_depth;
            }
        }
    }

    if water_cell_count == 0 {
        return vertices;
    }

    let total_cells = (width * depth) as usize;
    let average_depth = depth_sum / water_cell_count as f32;
    let wave_energy = water_chunk_wave_energy(water_cell_count, total_cells, average_depth);

    for patch in greedy_water_patches(&cells, width, depth) {
        // A bounded 4x4 subgrid leaves large open-water areas greedy-meshed:
        // at most 16 quads per 16x16 water chunk instead of 256 voxel quads.
        // Shared vertex values are derived from global terrain samples rather
        // than the patch's average depth.
        let mut z = patch.z0;
        while z < patch.z1 {
            let next_z = (z + 4).min(patch.z1);
            let mut x = patch.x0;
            while x < patch.x1 {
                let next_x = (x + 4).min(patch.x1);
                let corner = |cx: u32, cz: u32| -> f32 {
                    let i = cz as usize * halo_width + cx as usize;
                    (halo_depths[i]
                        + halo_depths[i + 1]
                        + halo_depths[i + halo_width]
                        + halo_depths[i + halo_width + 1])
                        * 0.25
                };
                let x0 = min[0] + (x_start + x) as f32 * grid.spacing_studs;
                let z0 = min[2] + (z_start + z) as f32 * grid.spacing_studs;
                let x1 = min[0] + (x_start + next_x) as f32 * grid.spacing_studs;
                let z1 = min[2] + (z_start + next_z) as f32 * grid.spacing_studs;
                push_water_gradient_quad(
                    &mut vertices,
                    [
                        [x0, water_level, z0],
                        [x0, water_level, z1],
                        [x1, water_level, z1],
                        [x1, water_level, z0],
                    ],
                    [
                        corner(x, z),
                        corner(x, next_z),
                        corner(next_x, next_z),
                        corner(next_x, z),
                    ],
                    wave_energy,
                );
                x = next_x;
            }
            z = next_z;
        }
    }

    vertices
}

// Worldkiln 0.8.7 | WO 0.8.7-38 | 2026-10-08 19:35 -05:00
// Water tint and shoreline foam are interpolated separately at each corner.
// Keep the existing renderer's 12-float vertex layout: position, normal,
// water metadata in RGBA, UV. Quad winding matches add_block_quad.
fn push_water_gradient_quad(
    vertices: &mut Vec<f32>,
    points: [[f32; 3]; 4],
    depths: [f32; 4],
    wave_energy: f32,
) {
    let metadata = depths.map(|depth| water_surface_attributes(depth, wave_energy));
    for i in [0, 1, 2, 0, 2, 3] {
        let p = points[i];
        let c = metadata[i];
        vertices.extend_from_slice(&[
            p[0], p[1], p[2], 0.0, 1.0, 0.0, c[0], c[1], c[2], c[3], p[0], p[2],
        ]);
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
const MAX_GRASS_CLUMPS_PER_VOXEL_COLUMN: u32 = 8;
const MAX_PROCEDURAL_GRASS_CLUMPS: u32 = 65_536;

fn configured_surface_material_name(materials: &TerrainMaterials) -> &str {
    if materials.surface.trim().is_empty() {
        TERRAIN_FALLBACK_SURFACE_MATERIAL
    } else {
        materials.surface.as_str()
    }
}

fn top_voxel_material(grid: &SmoothVoxelGrid, x: u32, z: u32, mode: TerrainMode) -> Option<u16> {
    for y in (0..grid.size[1]).rev() {
        let voxel = grid.voxel(x, y, z)?;
        let visible = match mode {
            TerrainMode::Smooth => !voxel.is_empty(),
            TerrainMode::Block => voxel.occupancy_unit() >= SMOOTH_VOXEL_ISO_LEVEL,
        };
        if visible {
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

fn push_grass_clump(vertices: &mut Vec<f32>, center: Vec3, length: f32, angle: f32, shade: f32) {
    let half_width = (length * 0.09).clamp(0.04, 0.18);

    for plane_angle in [angle, angle + std::f32::consts::FRAC_PI_2] {
        let axis = Vec3::new(plane_angle.cos(), 0.0, plane_angle.sin());
        let normal = Vec3::new(-axis.z, 0.0, axis.x);
        let left = center - axis * half_width;
        let right = center + axis * half_width;
        let tip = center + Vec3::Y * length;

        for (point, uv) in [(left, [0.0, 0.0]), (right, [1.0, 0.0]), (tip, [0.5, 1.0])] {
            vertices.extend_from_slice(&[
                point.x, point.y, point.z, normal.x, normal.y, normal.z, shade, shade, shade, 1.0,
                uv[0], uv[1],
            ]);
        }
    }
}

fn build_voxel_grass(
    grid: &SmoothVoxelGrid,
    mode: TerrainMode,
    water_level: f32,
    settings: TerrainGrassSettings,
    materials: &TerrainMaterials,
    seed: i32,
) -> Vec<f32> {
    build_voxel_grass_region(
        grid,
        mode,
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

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
fn build_voxel_grass_region(
    grid: &SmoothVoxelGrid,
    mode: TerrainMode,
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
    // Preserve the historical density as a world-area density rather than a
    // clumps-per-logical-column density. The old 4-stud column covered 16 stud².
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
            let Some(material) = top_voxel_material(grid, x, z, mode) else {
                continue;
            };
            if grid.material_name(material) != Some(surface_material) {
                continue;
            }

            // Hash logical columns rather than truncating non-integer world spacing.
            // Origin keeps placement stable when the same Terrain is moved.
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
                let Some(surface_y) = grid.surface_height_at(world_x, world_z, mode) else {
                    continue;
                };
                if surface_y <= water_level + 0.05 {
                    continue;
                }

                let angle = grass_random(seed, cell_world_x, cell_world_z, slot, 0x3)
                    * std::f32::consts::TAU;
                let length_variation =
                    0.85 + grass_random(seed, cell_world_x, cell_world_z, slot, 0x4) * 0.30;
                let shade = 0.82 + grass_random(seed, cell_world_x, cell_world_z, slot, 0x5) * 0.18;
                push_grass_clump(
                    &mut vertices,
                    Vec3::new(world_x, surface_y + 0.02, world_z),
                    length_studs * length_variation,
                    angle,
                    shade,
                );
                generated_clumps += 1;
            }
        }
    }

    vertices
}

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
fn terrain_render_chunk_keys(result: &TerrainResult) -> Vec<TerrainRenderChunkKey> {
    match &result.data {
        TerrainData::Heightfield(_) => vec![[0, 0]],
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
            if key != [0, 0] {
                return None;
            }
            let min = Vec3::new(
                heightfield.origin[0] as f32,
                heightfield.origin[1] as f32,
                heightfield.origin[2] as f32,
            );
            let max = min
                + Vec3::new(
                    heightfield.size[0] as f32,
                    heightfield.size[1] as f32,
                    heightfield.size[2] as f32,
                );
            Some((min, max))
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

// Worldkiln 0.8.5 | WO MAT-055 | 2026-10-10 16:58 -05:00
// Water can sit above (or below) the voxel volume. Culling it using only the
// solid Terrain Y bounds makes water vanish except at a few near-camera spots.
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

fn build_terrain_mesh_chunk(
    result: &TerrainResult,
    config: &TerrainConfig,
    key: TerrainRenderChunkKey,
) -> TerrainMesh {
    let TerrainData::Voxel(grid) = &result.data else {
        return if key == [0, 0] {
            build_terrain_mesh_with_grass(result, config)
        } else {
            TerrainMesh::default()
        };
    };
    let Some((x_start, x_end, z_start, z_end)) = terrain_render_chunk_bounds(grid, key) else {
        return TerrainMesh::default();
    };

    let mut mesh = match config.mode {
        TerrainMode::Smooth => build_voxel_smooth_region(
            grid,
            config.voxel_layers.slope_shading_strength,
            x_start,
            x_end,
            z_start,
            z_end,
        ),
        TerrainMode::Block => build_voxel_block_region(
            grid,
            config.voxel_layers.slope_shading_strength,
            x_start,
            x_end,
            z_start,
            z_end,
        ),
    };

    let water = build_voxel_water_region(
        grid,
        config.plains.water_level,
        x_start,
        x_end,
        z_start,
        z_end,
    );
    if !water.is_empty() {
        mesh.batches.push(TerrainMeshBatch {
            material: TerrainMaterialSlot::Water,
            vertices: water,
        });
    }

    let grass = build_voxel_grass_region(
        grid,
        config.mode,
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
        });
    }
    mesh
}

fn build_terrain_mesh_with_grass(result: &TerrainResult, config: &TerrainConfig) -> TerrainMesh {
    let mut mesh = build_terrain_mesh(
        result,
        config.mode,
        config.plains.water_level,
        config.voxel_layers.slope_shading_strength,
    );
    if let TerrainData::Voxel(grid) = &result.data {
        let grass = build_voxel_grass(
            grid,
            config.mode,
            config.plains.water_level,
            config.grass,
            &config.materials,
            config.plains.seed,
        );
        if !grass.is_empty() {
            mesh.batches.push(TerrainMeshBatch {
                material: TerrainMaterialSlot::Grass,
                vertices: grass,
            });
        }
    }
    mesh
}

// Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
fn water_surface_attributes(depth: f32, wave_energy: f32) -> [f32; 4] {
    let depth = depth.max(0.0);
    // WO 0.9.3-20: broader, smoother shoreline metadata for alpha/foam/waves.
    let foam = 1.0 - smoothstep_cpu(0.20, 3.50, depth);
    let depth_normalized = smoothstep_cpu(0.0, 16.0, depth);
    [foam, depth_normalized, wave_energy.clamp(0.05, 1.0), 1.0]
}

fn smoothstep_cpu(edge0: f32, edge1: f32, value: f32) -> f32 {
    if edge1 <= edge0 {
        return if value >= edge1 { 1.0 } else { 0.0 };
    }
    let t = ((value - edge0) / (edge1 - edge0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

// Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
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
    let mut vertices = Vec::new();
    if !water_level.is_finite() {
        return vertices;
    }

    let mut water_tiles = Vec::new();
    let mut depth_sum = 0.0;
    for z in 0..heightfield.size[2] {
        for x in 0..heightfield.size[0] {
            let ground_height = heightfield.height(x, z).unwrap();
            if ground_height >= water_level {
                continue;
            }
            let depth = water_level - ground_height;
            depth_sum += depth;
            water_tiles.push((x, z, depth));
        }
    }
    let wave_energy = water_chunk_wave_energy(
        water_tiles.len(),
        (heightfield.size[0] * heightfield.size[2]) as usize,
        if water_tiles.is_empty() {
            0.0
        } else {
            depth_sum / water_tiles.len() as f32
        },
    );

    for (x, z, depth) in water_tiles {
        let water_attributes = water_surface_attributes(depth, wave_energy);
        let wx = heightfield.origin[0] as f32 + x as f32;
        let wz = heightfield.origin[2] as f32 + z as f32;
        add_block_quad(
            &mut vertices,
            [wx, water_level, wz],
            [wx, water_level, wz + 1.0],
            [wx + 1.0, water_level, wz + 1.0],
            [wx + 1.0, water_level, wz],
            water_attributes,
            [0.0, 1.0, 0.0],
        );
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

// Worldkiln 0.9.3 | WO 0.9.3-05 | 2026-10-06 23:14 -05:00
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

// Worldkiln 0.9.3 | WO 0.9.3-06 | 2026-10-06 23:14 -05:00
fn terrain_palette_visual<'a>(
    name: &'a str,
    materials: &'a TerrainMaterials,
) -> (&'a str, [f32; 3]) {
    if name == TERRAIN_FALLBACK_SURFACE_MATERIAL {
        ("", [0.52, 0.72, 0.32])
    } else if name == TERRAIN_FALLBACK_SUBSURFACE_MATERIAL {
        ("", [0.46, 0.31, 0.16])
    } else if name == TERRAIN_FALLBACK_ROCK_MATERIAL {
        ("", [0.48, 0.48, 0.46])
    } else if name == materials.surface.as_str() {
        (name, [0.52, 0.72, 0.32])
    } else if name == materials.subsurface.as_str() {
        (name, [0.46, 0.31, 0.16])
    } else if name == materials.rock.as_str() {
        (name, [0.48, 0.48, 0.46])
    } else {
        (name, [0.62, 0.62, 0.62])
    }
}

fn terrain_blend_role(name: &str, materials: &TerrainMaterials) -> Option<i32> {
    if name == TERRAIN_FALLBACK_SURFACE_MATERIAL || name == materials.surface.as_str() {
        Some(0)
    } else if name == TERRAIN_FALLBACK_SUBSURFACE_MATERIAL || name == materials.subsurface.as_str()
    {
        Some(1)
    } else if name == TERRAIN_FALLBACK_ROCK_MATERIAL || name == materials.rock.as_str() {
        Some(2)
    } else if name == "sand" {
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
    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
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

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
fn terrain_render_config_key(config: &TerrainConfig) -> u64 {
    let mut hasher = DefaultHasher::new();
    (config.mode as u8).hash(&mut hasher);
    config.plains.water_level.to_bits().hash(&mut hasher);
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

// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
impl TerrainGpuChunk {
    fn upload(mesh: TerrainMesh) -> Self {
        let batches = mesh
            .batches
            .into_iter()
            .filter(|batch| !batch.vertices.is_empty())
            .map(|batch| {
                let (vao, vbo, vertex_count) = upload_block_vertices_3d(&batch.vertices);
                TerrainGpuBatch {
                    material: batch.material,
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
            chunks,
        }
    }

    pub(super) fn free(&mut self) {
        for chunk in self.chunks.values_mut() {
            chunk.free();
        }
        self.chunks.clear();
    }
}

impl Renderer {
    fn ensure_terrain_cache(&self, world: &World) {
        let Some(result) = world.terrain.effective() else {
            if let Some(mut cached) = self.terrain_cache.borrow_mut().take() {
                cached.free();
            }
            return;
        };

        // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
        let revision = world.terrain.render_revision();
        let config_key = terrain_render_config_key(&world.terrain.config);
        let full_dirty_revision = world.terrain.full_render_revision();

        let needs_full_rebuild = self.terrain_cache.borrow().as_ref().is_none_or(|cache| {
            cache.config_key != config_key || full_dirty_revision > cache.revision
        });

        if needs_full_rebuild {
            let replacement =
                TerrainGpuMesh::build_full(result, &world.terrain.config, revision, config_key);
            if let Some(mut old) = self.terrain_cache.borrow_mut().replace(replacement) {
                old.free();
            }
            return;
        }

        let cached_revision = self
            .terrain_cache
            .borrow()
            .as_ref()
            .map(|cache| cache.revision)
            .unwrap_or(0);
        if cached_revision == revision {
            return;
        }

        let dirty_keys: Vec<_> = terrain_render_chunk_keys(result)
            .into_iter()
            .filter(|key| world.terrain.render_chunk_revision(*key) > cached_revision)
            .collect();

        let mut cache = self.terrain_cache.borrow_mut();
        let Some(cache) = cache.as_mut() else {
            return;
        };
        for key in dirty_keys {
            let replacement = TerrainGpuChunk::upload(build_terrain_mesh_chunk(
                result,
                &world.terrain.config,
                key,
            ));
            if let Some(mut old) = cache.chunks.insert(key, replacement) {
                old.free();
            }
        }
        cache.revision = revision;
    }

    // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
    pub(super) fn draw_terrain(
        &self,
        world: &World,
        camera_frustum: &CameraFrustum,
        chunk_focus: Vec3,
        chunk_radius: i32,
        profile_draws: bool,
    ) -> TerrainDrawStats {
        self.ensure_terrain_cache(world);
        let Some(result) = world.terrain.effective() else {
            return TerrainDrawStats::default();
        };
        let materials = &world.terrain.config.materials;
        let cache = self.terrain_cache.borrow();
        let Some(cache) = cache.as_ref() else {
            return TerrainDrawStats::default();
        };
        let mut stats = TerrainDrawStats::default();

        unsafe {
            gl::UniformMatrix4fv(
                self.grid_model_location,
                1,
                gl::FALSE,
                Mat4::IDENTITY.to_cols_array().as_ptr(),
            );
            gl::Uniform1i(self.grid_use_texture_location, 1);

            let smooth_blending = world.terrain.config.mode == TerrainMode::Smooth;
            if smooth_blending {
                gl::ActiveTexture(gl::TEXTURE3);
                gl::BindTexture(gl::TEXTURE_2D, self.get_texture(materials.surface.as_str()));
                gl::ActiveTexture(gl::TEXTURE4);
                gl::BindTexture(
                    gl::TEXTURE_2D,
                    self.get_texture(materials.subsurface.as_str()),
                );
                gl::ActiveTexture(gl::TEXTURE5);
                gl::BindTexture(gl::TEXTURE_2D, self.get_texture(materials.rock.as_str()));
                gl::ActiveTexture(gl::TEXTURE6);
                gl::BindTexture(gl::TEXTURE_2D, self.get_texture("sand"));
                gl::Uniform1f(
                    self.grid_terrain_water_level_location,
                    world.terrain.config.plains.water_level,
                );
            }
            gl::ActiveTexture(gl::TEXTURE1);
            // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 07:54 -05:00
            // Local to the opaque pass: other passes may legitimately rebind.
            let mut last_texture: Option<u32> = None;
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

                for batch in &chunk.batches {
                    if matches!(batch.material, TerrainMaterialSlot::Water) {
                        continue;
                    }
                    let (texture, color, blend_role): (&str, [f32; 3], Option<i32>) =
                        match batch.material {
                            TerrainMaterialSlot::Surface => {
                                (materials.surface.as_str(), [0.52, 0.72, 0.32], Some(0))
                            }
                            TerrainMaterialSlot::Subsurface => {
                                (materials.subsurface.as_str(), [0.46, 0.31, 0.16], Some(1))
                            }
                            TerrainMaterialSlot::Rock => {
                                (materials.rock.as_str(), [0.48, 0.48, 0.46], Some(2))
                            }
                            TerrainMaterialSlot::Grass => ("", [0.30, 0.72, 0.18], None),
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
                    let blend_role = smooth_blending.then_some(blend_role).flatten();
                    gl::Uniform1i(
                        self.grid_terrain_blend_enabled_location,
                        i32::from(blend_role.is_some()),
                    );
                    gl::Uniform1i(
                        self.grid_terrain_source_role_location,
                        blend_role.unwrap_or(0),
                    );
                    let draw_color = if blend_role.is_some() {
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
                    gl::Uniform1i(self.grid_use_texture_location, if is_grass { 0 } else { 1 });
                    if is_grass {
                        gl::Disable(gl::CULL_FACE);
                        if super::resources::texture_binding_changed(&mut last_texture, 0) {
                            gl::BindTexture(gl::TEXTURE_2D, 0);
                        }
                    } else {
                        let started = profile_draws.then(std::time::Instant::now);
                        let resolved = self.get_texture(texture);
                        if let Some(started) = started {
                            stats.texture_lookup_ns = stats
                                .texture_lookup_ns
                                .saturating_add(started.elapsed().as_nanos() as u64);
                        }
                        // Lazy texture loading may alter GL_TEXTURE_2D on the active unit.
                        gl::ActiveTexture(gl::TEXTURE1);
                        if super::resources::texture_binding_changed(&mut last_texture, resolved) {
                            gl::BindTexture(gl::TEXTURE_2D, resolved);
                        }
                    }
                    gl::BindVertexArray(batch.vao);
                    let started = profile_draws.then(std::time::Instant::now);
                    gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
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

            gl::Uniform1i(self.grid_terrain_blend_enabled_location, 0);
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
        self.ensure_terrain_cache(world);
        if self.water_program == 0 {
            return TerrainDrawStats::default();
        }
        let Some(result) = world.terrain.effective() else {
            return TerrainDrawStats::default();
        };
        let cache = self.terrain_cache.borrow();
        let Some(cache) = cache.as_ref() else {
            return TerrainDrawStats::default();
        };
        let mut stats = TerrainDrawStats::default();

        unsafe {
            gl::UseProgram(self.water_program);
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
                self.water_started.elapsed().as_secs_f32(),
            );
            gl::Uniform1i(
                self.water_camera_underwater_location,
                i32::from(camera_underwater),
            );
            // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
            let water = world.terrain.config.water;
            gl::Uniform1f(self.water_wave_height_location, water.wave_height);
            gl::Uniform1f(self.water_wave_speed_location, water.wave_speed);
            gl::Uniform1f(self.water_foam_amount_location, water.foam_amount);
            gl::Uniform1f(self.water_foam_scale_location, water.foam_scale);
            let atmosphere_enabled = world.sky.atmosphere_enabled && !camera_underwater;
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
            gl::DepthMask(gl::FALSE);
            for (key, chunk) in &cache.chunks {
                // Water must use bounds extended to the shared sea level.
                if !terrain_chunk_visible(
                    result,
                    *key,
                    camera_frustum,
                    chunk_focus,
                    chunk_radius,
                    Some(world.terrain.config.plains.water_level),
                ) {
                    stats.chunks_culled += 1;
                    continue;
                }
                stats.chunks_drawn += 1;
                for batch in &chunk.batches {
                    if !matches!(batch.material, TerrainMaterialSlot::Water) {
                        continue;
                    }
                    gl::BindVertexArray(batch.vao);
                    gl::DrawArrays(gl::TRIANGLES, 0, batch.vertex_count);
                    stats.draws += 1;
                    stats.triangles += batch.vertex_count as u64 / 3;
                }
            }
            gl::DepthMask(gl::TRUE);
            gl::Enable(gl::CULL_FACE);
            gl::UseProgram(self.grid_program);
        }
        stats
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
pub(super) const WATER_VERTEX_SHADER: &str = r#"
#version 330 core

layout (location = 0) in vec3 a_position;
layout (location = 2) in vec4 a_color;

uniform mat4 u_view_projection;
uniform float u_time;
uniform float u_wave_height;
uniform float u_wave_speed;

out vec3 v_world_pos;
out float v_foam;
out float v_depth;
out float v_wave_energy;

float wave_height(vec2 p, float t, float depth, float wave_energy) {
    float shore_scale = mix(0.08, 1.0, smoothstep(0.03, 0.48, depth));
    float energy = clamp(wave_energy, 0.05, 1.0);
    // Keep shore water almost flat while retaining open-water movement.
    float geometry_energy = mix(0.35, 1.0, energy);
    float scaled_time = t * u_wave_speed;
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
    world.y += wave_height(world.xz, u_time, v_depth, v_wave_energy);
    v_world_pos = world;
    gl_Position = u_view_projection * vec4(world, 1.0);
}
"#;

pub(super) const WATER_FRAGMENT_SHADER: &str = r#"
#version 330 core

in vec3 v_world_pos;
in float v_foam;
in float v_depth;
in float v_wave_energy;

uniform vec3 u_camera_position;
uniform float u_time;
uniform bool u_camera_underwater;
uniform float u_wave_height;
uniform float u_wave_speed;
uniform float u_foam_amount;
uniform float u_foam_scale;
uniform samplerCube u_sky;
// Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
uniform bool u_atmosphere_enabled;
uniform vec3 u_fog_color;
uniform float u_fog_start;
uniform float u_fog_end;

out vec4 FragColor;

vec3 water_normal(vec2 p, float t, float depth, float wave_energy) {
    float shore_scale = mix(0.10, 1.0, smoothstep(0.03, 0.42, depth));
    float energy = clamp(wave_energy, 0.05, 1.0);
    // WO 0.9.3-15: keep two dominant normal waves. The third micro-wave was
    // visually subtle but added two more trig evaluations per water fragment.
    float scaled_time = t * u_wave_speed;
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
    return normalize(vec3(-dx * u_wave_height, 1.0, -dz * u_wave_height));
}

void main() {
    vec3 n = water_normal(v_world_pos.xz, u_time, v_depth, v_wave_energy);
    if (u_camera_underwater || !gl_FrontFacing) {
        n = -n;
    }

    vec3 view_dir = normalize(u_camera_position - v_world_pos);
    float facing = clamp(abs(dot(n, view_dir)), 0.0, 1.0);
    float one_minus_facing = 1.0 - facing;
    float fresnel5 = one_minus_facing * one_minus_facing;
    fresnel5 *= fresnel5 * one_minus_facing;
    float fresnel = 0.04 + 0.96 * fresnel5;

    vec3 reflected = texture(u_sky, reflect(-view_dir, n)).rgb;

    vec3 shallow = vec3(0.085, 0.47, 0.50);
    vec3 deep = vec3(0.018, 0.12, 0.22);
    float depth_mix = smoothstep(0.08, 0.82, v_depth);
    vec3 body = mix(shallow, deep, depth_mix);

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    // Two warped, non-parallel fields replace the single diagonal sine that
    // read as evenly spaced straight lines across broad shore water.
    vec2 foam_p = v_world_pos.xz * u_foam_scale;
    float foam_time = u_time * u_wave_speed;
    float foam_warp =
        sin(foam_p.x * 0.37 - foam_time * 0.31) * 0.75 +
        sin(foam_p.y * 0.29 + foam_time * 0.23) * 0.55;
    float foam_noise =
        0.74 +
        0.16 * sin(foam_p.x * 1.31 + foam_p.y * 1.87 + foam_warp + foam_time * 1.13) +
        0.10 * sin(foam_p.x * -2.17 + foam_p.y * 0.83 - foam_warp * 0.7 - foam_time * 0.79);
    float crest = smoothstep(0.965, 1.0, n.y) * 0.08 * v_wave_energy;
    float shore_foam = smoothstep(0.08, 0.92, v_foam * foam_noise);
    float foam = clamp((shore_foam + crest) * u_foam_amount, 0.0, 1.0);

    vec3 light_dir = normalize(vec3(0.35, 0.86, 0.28));
    vec3 half_dir = normalize(light_dir + view_dir);
    // Approximate x^96 with multiplies instead of a general-purpose pow().
    float spec_base = max(dot(n, half_dir), 0.0);
    float spec2 = spec_base * spec_base;
    float spec4 = spec2 * spec2;
    float spec8 = spec4 * spec4;
    float spec16 = spec8 * spec8;
    float spec32 = spec16 * spec16;
    float spec64 = spec32 * spec32;
    float specular = spec64 * spec32 * 1.4;

    vec3 color;
    float alpha;
    if (u_camera_underwater) {
        vec3 underside = vec3(0.025, 0.24, 0.31);
        color = mix(underside, reflected, fresnel * 0.20);
        color += vec3(0.10, 0.18, 0.19) * specular * 0.35;
        color = mix(color, vec3(0.72, 0.88, 0.88), foam * 0.45);
        alpha = 0.84;
    } else {
        float edge = 1.0 - smoothstep(0.02, 0.22, v_depth);
        float reflection_weight = mix(0.10, 0.22 + fresnel * 0.68, 1.0 - edge);
        color = mix(body, reflected, reflection_weight);
        color += vec3(1.0, 0.96, 0.82) * specular * mix(0.35, 1.0, 1.0 - edge);
        color = mix(color, vec3(0.93, 0.97, 0.94), foam * 0.72);
        // Worldkiln 0.8.7 | WO 0.8.7-38 | 2026-10-08 20:12 -05:00
        // DIAGNOSTIC: retain full water color, waves, foam and Fresnel,
        // but remove depth-based alpha to isolate water transparency issues.
        alpha = 0.78;
    }

    if (u_atmosphere_enabled && !u_camera_underwater) {
        float distance_to_camera = length(v_world_pos - u_camera_position);
        float fog_amount = smoothstep(
            max(u_fog_start, 0.0),
            max(u_fog_end, u_fog_start + 1.0),
            distance_to_camera
        );
        color = mix(color, u_fog_color, fog_amount);
    }

    FragColor = vec4(color, clamp(alpha, 0.0, 0.96));
}
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{TerrainConfig, TerrainGenerationRequest, generate_terrain};

    // Worldkiln 0.8.5 | WO MAT-055 | 2026-10-10 18:06 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-053 | 2026-10-10 17:31 -05:00
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

    // Worldkiln 0.9.3 | WO 0.9.3-20
    #[test]
    fn smooth_terrain_blend_roles_cover_core_layers_only() {
        let materials = TerrainMaterials::default();
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

    // Worldkiln 0.9.3 | WO 0.9.3-10 | 2026-10-07 00:41 -05:00
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
        assert!(underwater_state(&world, Vec3::new(4.0, 8.0, 4.0)).is_none());
        assert!(underwater_state(&world, Vec3::new(4.0, 3.0, 4.0)).is_none());
        assert!(underwater_state(&world, Vec3::new(20.0, 6.0, 4.0)).is_none());
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

    // Worldkiln 0.9.3 | WO 0.9.3-06 | 2026-10-06 23:14 -05:00
    // Worldkiln 0.9.3 | WO 0.9.3-13 | 2026-10-07 02:32 -05:00
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
    fn chunk_wave_energy_keeps_puddles_calmer_than_open_deep_water() {
        let puddle = water_chunk_wave_energy(4, 256, 0.8);
        let pond = water_chunk_wave_energy(96, 256, 3.0);
        let ocean = water_chunk_wave_energy(256, 256, 12.0);

        assert!(puddle < pond);
        assert!(pond < ocean);
        assert!(puddle < 0.25);
        assert!(ocean > 0.9);
    }

    // Worldkiln 0.9.3 | WO 0.9.3-15

    // Worldkiln 0.8.7 | WO 0.8.7-38 | 2026-10-08 19:38 -05:00
    // Worldkiln 0.8.7 | WO 0.8.7-38 | 2026-10-08 20:12 -05:00
    #[test]
    fn water_fixed_alpha_preview_retains_normal_shading_without_depth_alpha() {
        let frag = WATER_FRAGMENT_SHADER;
        assert!(frag.contains("vec3 body = mix(shallow, deep, depth_mix);"));
        assert!(frag.contains("color = mix(body, reflected, reflection_weight);"));
        assert!(frag.contains("color = mix(color, vec3(0.93, 0.97, 0.94), foam * 0.72);"));
        assert!(frag.contains("alpha = 0.78;"));
        assert!(!frag.contains("alpha = mix(0.20, 0.82"));
        assert!(!frag.contains("alpha = max(alpha, foam * 0.58)"));
        assert!(!frag.contains("FragColor = vec4(1.0, 0.0, 1.0, 1.0);"));
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    #[test]
    fn water_controls_keep_fixed_alpha_and_replace_single_line_foam_pattern() {
        let vertex = WATER_VERTEX_SHADER;
        let fragment = WATER_FRAGMENT_SHADER;
        for uniform in ["u_wave_height", "u_wave_speed"] {
            assert!(vertex.contains(uniform));
            assert!(fragment.contains(uniform));
        }
        for uniform in ["u_foam_amount", "u_foam_scale"] {
            assert!(fragment.contains(uniform));
        }
        assert!(fragment.contains("float foam_warp"));
        assert!(fragment.contains("foam_p.x * -2.17 + foam_p.y * 0.83"));
        assert!(!fragment.contains("v_world_pos.x * 1.9"));
        assert!(fragment.contains("alpha = 0.78;"));
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
        // Both depths are in the same greedy-merge class (>2.25).
        assert_eq!(water_merge_class(6.0), water_merge_class(18.0));
        let data = build_voxel_water_region(&grid, 30.0, 0, 16, 0, 16);
        // 16 subquads, rather than one 16x16 quad or 256 voxel quads.
        assert_eq!(data.len() / 12, 16 * 6);
        let colors: Vec<f32> = data.chunks_exact(12).map(|vertex| vertex[7]).collect();
        let min = colors.iter().copied().fold(f32::INFINITY, f32::min);
        let max = colors.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        assert!(
            max - min > 0.2,
            "Greedy patch must interpolate actual depth, not patch average: {min}..{max}"
        );
    }

    // Worldkiln 0.8.7 | WO 0.8.7-38 | 2026-10-08 19:38 -05:00
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
    fn empty_generated_materials_have_distinct_fallback_visuals() {
        // Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
        let materials = TerrainMaterials {
            surface: String::new(),
            subsurface: String::new(),
            rock: String::new(),
            water: String::new(),
        };
        assert_eq!(
            terrain_palette_visual(TERRAIN_FALLBACK_SURFACE_MATERIAL, &materials),
            ("", [0.52, 0.72, 0.32])
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

    // Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
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
            config.mode,
            config.plains.water_level,
            config.grass,
            &config.materials,
            config.plains.seed,
        );
        let second = build_voxel_grass(
            grid,
            config.mode,
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
                config.mode,
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
                config.mode,
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
        assert!(
            build_voxel_grass(
                &rock_grid,
                TerrainMode::Smooth,
                -10.0,
                settings,
                &materials,
                1337,
            )
            .is_empty()
        );

        let surface_grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [1, 1, 1],
            vec!["grass_1".into()],
            vec![crate::world::SmoothVoxel::from_unit_occupancy(0, 1.0)],
        )
        .unwrap();
        assert!(
            build_voxel_grass(
                &surface_grid,
                TerrainMode::Smooth,
                10.0,
                settings,
                &materials,
                1337,
            )
            .is_empty()
        );
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
            config.mode,
            config.plains.water_level,
            low,
            &config.materials,
            config.plains.seed,
        );
        let high_vertices = build_voxel_grass(
            grid,
            config.mode,
            config.plains.water_level,
            high,
            &config.materials,
            config.plains.seed,
        );
        assert!(high_vertices.len() >= low_vertices.len());
    }

    // Worldkiln 0.8.7 | WO 0.8.7-36 | 2026-10-08 18:25 -05:00
    // The O(1) revision cache uses the configuration key directly; the
    // historical result-scanning helper was removed in WO 0.9.3-12.
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
    fn smooth_and_block_consumers_share_the_same_logical_source() {
        let config = TerrainConfig::default();
        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 24, 8],
            },
        )
        .unwrap();
        let smooth = build_terrain_mesh(
            &result,
            TerrainMode::Smooth,
            -10.0,
            config.voxel_layers.slope_shading_strength,
        );
        let block = build_terrain_mesh(
            &result,
            TerrainMode::Block,
            -10.0,
            config.voxel_layers.slope_shading_strength,
        );

        assert!(!smooth.batches[0].vertices.is_empty());
        assert!(!block.batches[0].vertices.is_empty());
        assert_ne!(smooth.source_signature, 0);
        assert_eq!(smooth.source_signature, block.source_signature);
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:55 -05:00
        let TerrainData::Voxel(data) = &result.data else {
            panic!("expected smooth voxel Terrain after WO 0.9.3-03 generation");
        };
        // Worldkiln 0.8.5 | WO MAT-051 | 2026-10-10 16:34 -05:00
        // Current authored generation uses two-stud samples; the four-stud
        // mapping is retained only for legacy saved voxel data.
        assert_eq!(
            data.spacing_studs,
            crate::world::terrain::DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS
        );
        assert_eq!(data.size, [4, 12, 4]);
        assert_eq!(data.world_bounds().1, [8.0, 24.0, 8.0]);
        assert!(
            smooth
                .batches
                .iter()
                .any(|batch| matches!(batch.material, TerrainMaterialSlot::Palette(_)))
        );
    }

    #[test]
    fn rebuilding_mode_preserves_generated_logical_data() {
        let mut world = World::new();
        world
            .terrain
            .generate_authored(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [4, 16, 4],
            })
            .unwrap();
        let before = world.terrain.authored.clone();
        world.terrain.rebuild_representation(TerrainMode::Block);
        assert_eq!(world.terrain.authored, before);
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    #[test]
    fn smooth_block_switch_preserves_sculpted_logical_data() {
        let mut world = World::new();
        world
            .generate_authored_terrain(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 24, 8],
            })
            .unwrap();
        assert!(
            world.sculpt_authored_terrain(&crate::world::terrain::TerrainSculptRequest {
                tool: crate::world::terrain::SculptTool::Raise,
                center: [4.5, 0.0, 4.5],
                radius: 3.0,
                strength: 2.0,
                flatten_target: None,
            })
        );
        let sculpted = world.terrain.authored.clone().unwrap();

        let smooth = build_terrain_mesh(
            &sculpted,
            TerrainMode::Smooth,
            -10.0,
            world.terrain.config.voxel_layers.slope_shading_strength,
        );
        world.terrain.rebuild_representation(TerrainMode::Block);
        assert_eq!(world.terrain.authored.as_ref(), Some(&sculpted));
        let block = build_terrain_mesh(
            world.terrain.authored.as_ref().unwrap(),
            TerrainMode::Block,
            -10.0,
            world.terrain.config.voxel_layers.slope_shading_strength,
        );
        world.terrain.rebuild_representation(TerrainMode::Smooth);

        assert_eq!(world.terrain.authored.as_ref(), Some(&sculpted));
        assert_eq!(smooth.source_signature, block.source_signature);
        assert_ne!(smooth.source_signature, 0);
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
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
            TerrainMode::Smooth,
            -10.0,
            world.terrain.config.voxel_layers.slope_shading_strength,
        );
        assert_eq!(mesh.source_signature, sculpted_sig);
    }

    // Worldkiln 0.9.3 | WO 0.9.3-05 | 2026-10-06 23:14 -05:00
    #[test]
    fn slope_shading_keeps_flats_bright_and_darkens_steep_faces() {
        assert!((terrain_slope_shade(Vec3::Y, 0.25) - 1.0).abs() < f32::EPSILON);
        assert!((terrain_slope_shade(Vec3::X, 0.25) - 0.75).abs() < 0.001);
        let diagonal = terrain_slope_shade(Vec3::new(1.0, 1.0, 0.0), 0.25);
        assert!(diagonal > 0.75 && diagonal < 1.0);
        assert_eq!(terrain_slope_shade(Vec3::X, 0.0), 1.0);
    }

    #[test]
    fn voxel_block_mesh_writes_slope_shading_into_vertex_color() {
        let grid = SmoothVoxelGrid::from_dense(
            [0, 0, 0],
            [1, 1, 1],
            vec!["rock".into()],
            vec![crate::world::SmoothVoxel::from_unit_occupancy(0, 1.0)],
        )
        .unwrap();
        let mesh = build_voxel_block(&grid, 0.25);
        let colors: Vec<f32> = mesh.batches[0]
            .vertices
            .chunks_exact(12)
            .map(|vertex| vertex[6])
            .collect();
        assert!(colors.iter().any(|value| (*value - 1.0).abs() < 0.001));
        assert!(colors.iter().any(|value| (*value - 0.75).abs() < 0.001));
    }

    // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
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

    // Worldkiln 0.9.3 | WO 0.9.3-16
    // Worldkiln 0.8.5 | WO MAT-055 | 2026-10-10 16:59 -05:00
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
    fn chunked_voxel_meshing_matches_whole_mesh_vertex_totals() {
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

        for mode in [TerrainMode::Smooth, TerrainMode::Block] {
            config.mode = mode;
            let whole = build_terrain_mesh_with_grass(&result, &config);
            let whole_vertices: usize =
                whole.batches.iter().map(|batch| batch.vertices.len()).sum();

            let chunk_vertices: usize = keys
                .iter()
                .map(|key| build_terrain_mesh_chunk(&result, &config, *key))
                .flat_map(|mesh| mesh.batches)
                .map(|batch| batch.vertices.len())
                .sum();

            assert_eq!(
                chunk_vertices, whole_vertices,
                "chunked {mode:?} meshing must partition the same logical surface"
            );
        }
    }
}
