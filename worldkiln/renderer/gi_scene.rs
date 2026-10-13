use std::collections::{HashMap, HashSet};

use glam::Vec3;

use crate::world::{CHUNK_SIZE, CellType, ChunkCoord, World, WorldCoord};

use super::settings::SceneQueryBackend;

const VOXELS_PER_CHUNK: usize = (CHUNK_SIZE * CHUNK_SIZE * CHUNK_SIZE) as usize;
const OCCUPANCY_WORDS: usize = VOXELS_PER_CHUNK / 64;

#[derive(Clone)]
pub struct GiVoxelChunk {
    pub occupancy: [u64; OCCUPANCY_WORDS],
    /// RGB8 albedo in bits 0..23. Bit 31 marks a light/emissive source.
    pub materials: Box<[u32; VOXELS_PER_CHUNK]>,
    pub occupied_count: u16,
}

impl GiVoxelChunk {
    fn new() -> Self {
        Self {
            occupancy: [0; OCCUPANCY_WORDS],
            materials: Box::new([0; VOXELS_PER_CHUNK]),
            occupied_count: 0,
        }
    }

    pub fn occupied(&self, index: usize) -> bool {
        self.occupancy[index / 64] & (1_u64 << (index % 64)) != 0
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct GiSceneUpdateStats {
    pub full_rebuild: bool,
    pub rebuilt_chunks: usize,
    pub occupied_voxels: usize,
    pub excluded_shifted_voxels: usize,
    pub payload_bytes: usize,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GiRayHit {
    pub coord: WorldCoord,
    pub distance: f32,
    pub normal: Vec3,
    pub packed_material: u32,
}

pub struct GiVoxelScene {
    pub chunks: HashMap<ChunkCoord, GiVoxelChunk>,
    bvh: Option<ChunkBvh>,
    revision: u64,
    excluded_shifted_voxels: usize,
}

impl Default for GiVoxelScene {
    fn default() -> Self {
        Self {
            chunks: HashMap::new(),
            bvh: None,
            revision: 0,
            excluded_shifted_voxels: 0,
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct BvhNode {
    min: Vec3,
    max: Vec3,
    left: u32,
    right: u32,
    chunk: Option<ChunkCoord>,
}

#[derive(Debug)]
struct ChunkBvh {
    nodes: Vec<BvhNode>,
    root: u32,
    bounds_volume_in_chunks: usize,
}

impl ChunkBvh {
    fn build(mut chunks: Vec<ChunkCoord>) -> Option<Self> {
        if chunks.is_empty() {
            return None;
        }
        let min_chunk = chunks.iter().fold(
            ChunkCoord::new(i32::MAX, i32::MAX, i32::MAX),
            |min, chunk| {
                ChunkCoord::new(min.x.min(chunk.x), min.y.min(chunk.y), min.z.min(chunk.z))
            },
        );
        let max_chunk = chunks.iter().fold(
            ChunkCoord::new(i32::MIN, i32::MIN, i32::MIN),
            |max, chunk| {
                ChunkCoord::new(max.x.max(chunk.x), max.y.max(chunk.y), max.z.max(chunk.z))
            },
        );
        let volume = (i64::from(max_chunk.x) - i64::from(min_chunk.x) + 1)
            .saturating_mul(i64::from(max_chunk.y) - i64::from(min_chunk.y) + 1)
            .saturating_mul(i64::from(max_chunk.z) - i64::from(min_chunk.z) + 1)
            .max(0) as usize;
        let mut nodes = Vec::with_capacity(chunks.len().saturating_mul(2).saturating_sub(1));
        let root = build_bvh_node(&mut nodes, &mut chunks);
        Some(Self {
            nodes,
            root,
            bounds_volume_in_chunks: volume,
        })
    }
}

fn chunk_bounds(chunk: ChunkCoord) -> (Vec3, Vec3) {
    let min = chunk.world_origin();
    (min, min + Vec3::splat(CHUNK_SIZE as f32))
}

fn build_bvh_node(nodes: &mut Vec<BvhNode>, chunks: &mut [ChunkCoord]) -> u32 {
    let mut min = Vec3::splat(f32::INFINITY);
    let mut max = Vec3::splat(f32::NEG_INFINITY);
    for &chunk in chunks.iter() {
        let (chunk_min, chunk_max) = chunk_bounds(chunk);
        min = min.min(chunk_min);
        max = max.max(chunk_max);
    }

    let node_index = nodes.len() as u32;
    nodes.push(BvhNode {
        min,
        max,
        left: 0,
        right: 0,
        chunk: None,
    });
    if chunks.len() == 1 {
        nodes[node_index as usize].chunk = Some(chunks[0]);
        return node_index;
    }

    let extent = max - min;
    let axis = if extent.x >= extent.y && extent.x >= extent.z {
        0
    } else if extent.y >= extent.z {
        1
    } else {
        2
    };
    chunks.sort_unstable_by_key(|chunk| match axis {
        0 => chunk.x,
        1 => chunk.y,
        _ => chunk.z,
    });
    let split = chunks.len() / 2;
    let (left_chunks, right_chunks) = chunks.split_at_mut(split);
    let left = build_bvh_node(nodes, left_chunks);
    let right = build_bvh_node(nodes, right_chunks);
    nodes[node_index as usize].left = left;
    nodes[node_index as usize].right = right;
    node_index
}

fn ray_aabb(
    origin: Vec3,
    direction: Vec3,
    min: Vec3,
    max: Vec3,
    max_distance: f32,
) -> Option<(f32, f32)> {
    let mut enter = 0.0_f32;
    let mut exit = max_distance;
    for axis in 0..3 {
        if direction[axis].abs() < 1.0e-8 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return None;
            }
            continue;
        }
        let inverse = direction[axis].recip();
        let mut near = (min[axis] - origin[axis]) * inverse;
        let mut far = (max[axis] - origin[axis]) * inverse;
        if near > far {
            std::mem::swap(&mut near, &mut far);
        }
        enter = enter.max(near);
        exit = exit.min(far);
        if enter > exit {
            return None;
        }
    }
    Some((enter, exit))
}

impl GiVoxelScene {
    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn update(&mut self, world: &World) -> GiSceneUpdateStats {
        if self.revision == world.render_revision() && !self.chunks.is_empty() {
            return self.stats(false, 0);
        }

        let changes = world.render_changes_since(self.revision);
        if self.chunks.is_empty() || changes.full_resync_required {
            self.rebuild_all(world);
            self.revision = changes.latest_revision;
            self.rebuild_bvh();
            return self.stats(true, self.chunks.len());
        }

        let dirty_chunks: HashSet<_> = changes
            .changes
            .keys()
            .map(|coord| ChunkCoord::from_world_coord(*coord))
            .collect();
        for &chunk in &dirty_chunks {
            self.rebuild_chunk(world, chunk);
        }
        self.revision = changes.latest_revision;
        self.recount_shifted_voxels(world);
        self.rebuild_bvh();
        self.stats(false, dirty_chunks.len())
    }

    pub fn occupied(&self, coord: WorldCoord) -> bool {
        let chunk_coord = ChunkCoord::from_world_coord(coord);
        let Some(chunk) = self.chunks.get(&chunk_coord) else {
            return false;
        };
        chunk.occupied(local_index(coord))
    }

    pub fn packed_material(&self, coord: WorldCoord) -> Option<u32> {
        let chunk_coord = ChunkCoord::from_world_coord(coord);
        let chunk = self.chunks.get(&chunk_coord)?;
        let index = local_index(coord);
        chunk.occupied(index).then_some(chunk.materials[index])
    }

    pub fn trace_chunk_grid(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
    ) -> Option<GiRayHit> {
        let direction = direction.normalize_or_zero();
        if direction == Vec3::ZERO || max_distance <= 0.0 {
            return None;
        }
        trace_grid(
            origin,
            direction,
            0.0,
            max_distance,
            CHUNK_SIZE as f32,
            |chunk_coord, chunk_enter, chunk_exit, _| {
                let chunk_coord = ChunkCoord::new(chunk_coord.x, chunk_coord.y, chunk_coord.z);
                if !self.chunks.contains_key(&chunk_coord) {
                    return None;
                }
                trace_grid(
                    origin,
                    direction,
                    chunk_enter,
                    chunk_exit,
                    1.0,
                    |coord, enter, _, normal| {
                        if ChunkCoord::from_world_coord(coord) != chunk_coord {
                            return None;
                        }
                        self.packed_material(coord).map(|packed_material| GiRayHit {
                            coord,
                            distance: enter,
                            normal: if normal == Vec3::ZERO {
                                initial_hit_normal(direction)
                            } else {
                                normal
                            },
                            packed_material,
                        })
                    },
                )
            },
        )
    }

    pub fn trace(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
        backend: SceneQueryBackend,
    ) -> Option<GiRayHit> {
        match backend {
            SceneQueryBackend::ChunkGridDda => {
                self.trace_chunk_grid(origin, direction, max_distance)
            }
            SceneQueryBackend::OccupiedChunkBvh => {
                self.trace_occupied_chunk_bvh(origin, direction, max_distance)
            }
            SceneQueryBackend::Automatic => {
                let use_bvh = self.bvh.as_ref().is_some_and(|bvh| {
                    self.chunks.len() >= 8
                        && bvh.bounds_volume_in_chunks > self.chunks.len().saturating_mul(8)
                });
                if use_bvh {
                    self.trace_occupied_chunk_bvh(origin, direction, max_distance)
                } else {
                    self.trace_chunk_grid(origin, direction, max_distance)
                }
            }
        }
    }

    pub fn trace_occupied_chunk_bvh(
        &self,
        origin: Vec3,
        direction: Vec3,
        max_distance: f32,
    ) -> Option<GiRayHit> {
        let direction = direction.normalize_or_zero();
        let bvh = self.bvh.as_ref()?;
        if direction == Vec3::ZERO || max_distance <= 0.0 {
            return None;
        }
        let root = bvh.nodes[bvh.root as usize];
        let (root_enter, root_exit) =
            ray_aabb(origin, direction, root.min, root.max, max_distance)?;
        let mut stack = vec![(bvh.root, root_enter, root_exit)];
        let mut best_hit = None;
        let mut best_distance = max_distance;

        while let Some((node_index, enter, exit)) = stack.pop() {
            if enter > best_distance {
                continue;
            }
            let node = bvh.nodes[node_index as usize];
            if let Some(chunk_coord) = node.chunk {
                if let Some(hit) = trace_grid(
                    origin,
                    direction,
                    enter.max(0.0),
                    exit.min(best_distance),
                    1.0,
                    |coord, cell_enter, _, normal| {
                        if ChunkCoord::from_world_coord(coord) != chunk_coord {
                            return None;
                        }
                        self.packed_material(coord).map(|packed_material| GiRayHit {
                            coord,
                            distance: cell_enter,
                            normal: if normal == Vec3::ZERO {
                                initial_hit_normal(direction)
                            } else {
                                normal
                            },
                            packed_material,
                        })
                    },
                ) {
                    best_distance = hit.distance;
                    best_hit = Some(hit);
                }
                continue;
            }

            let left = bvh.nodes[node.left as usize];
            let right = bvh.nodes[node.right as usize];
            let left_hit = ray_aabb(origin, direction, left.min, left.max, best_distance);
            let right_hit = ray_aabb(origin, direction, right.min, right.max, best_distance);
            match (left_hit, right_hit) {
                (Some(left_bounds), Some(right_bounds)) => {
                    if left_bounds.0 <= right_bounds.0 {
                        stack.push((node.right, right_bounds.0, right_bounds.1));
                        stack.push((node.left, left_bounds.0, left_bounds.1));
                    } else {
                        stack.push((node.left, left_bounds.0, left_bounds.1));
                        stack.push((node.right, right_bounds.0, right_bounds.1));
                    }
                }
                (Some(bounds), None) => stack.push((node.left, bounds.0, bounds.1)),
                (None, Some(bounds)) => stack.push((node.right, bounds.0, bounds.1)),
                (None, None) => {}
            }
        }
        best_hit
    }

    fn rebuild_all(&mut self, world: &World) {
        self.chunks.clear();
        for chunk in world.iter_active_chunks() {
            self.rebuild_chunk(world, chunk);
        }
        self.recount_shifted_voxels(world);
    }

    fn rebuild_chunk(&mut self, world: &World, chunk_coord: ChunkCoord) {
        let Some(coords) = world.get_active_coords_in_chunk(chunk_coord) else {
            self.chunks.remove(&chunk_coord);
            return;
        };
        let mut chunk = GiVoxelChunk::new();
        for &coord in coords {
            let Some(cell) = world.get_effective_cell(coord) else {
                continue;
            };
            if !matches!(
                cell.cell_type,
                CellType::Block | CellType::SpawnPoint | CellType::Light
            ) || !world.is_cell_visible(coord)
                || !world.is_cell_anchored(coord)
                || world.get_visual_offset(coord).length_squared() > 0.000_001
            {
                continue;
            }
            let index = local_index(coord);
            chunk.occupancy[index / 64] |= 1_u64 << (index % 64);
            let color = world.get_effective_color(coord);
            chunk.materials[index] = pack_rgb8(color)
                | if cell.cell_type == CellType::Light {
                    1 << 31
                } else {
                    0
                };
            chunk.occupied_count += 1;
        }
        if chunk.occupied_count == 0 {
            self.chunks.remove(&chunk_coord);
        } else {
            self.chunks.insert(chunk_coord, chunk);
        }
    }

    fn rebuild_bvh(&mut self) {
        self.bvh = ChunkBvh::build(self.chunks.keys().copied().collect());
    }

    fn recount_shifted_voxels(&mut self, world: &World) {
        self.excluded_shifted_voxels = world
            .iter_active_effective_coords()
            .filter(|&coord| {
                world.get_effective_cell(coord).is_some_and(|cell| {
                    matches!(
                        cell.cell_type,
                        CellType::Block | CellType::SpawnPoint | CellType::Light
                    ) && world.is_cell_visible(coord)
                        && world.is_cell_anchored(coord)
                        && world.get_visual_offset(coord).length_squared() > 0.000_001
                })
            })
            .count();
    }

    fn stats(&self, full_rebuild: bool, rebuilt_chunks: usize) -> GiSceneUpdateStats {
        let occupied_voxels = self
            .chunks
            .values()
            .map(|chunk| chunk.occupied_count as usize)
            .sum();
        GiSceneUpdateStats {
            full_rebuild,
            rebuilt_chunks,
            occupied_voxels,
            excluded_shifted_voxels: self.excluded_shifted_voxels,
            payload_bytes: self.chunks.len()
                * (OCCUPANCY_WORDS * std::mem::size_of::<u64>()
                    + VOXELS_PER_CHUNK * std::mem::size_of::<u32>()),
        }
    }
}

fn axis_setup(position: f32, direction: f32, coord: i32, size: f32) -> (i32, f32, f32) {
    if direction > 0.0 {
        let boundary = (coord as f32 + 1.0) * size;
        (1, (boundary - position) / direction, size / direction)
    } else if direction < 0.0 {
        let boundary = coord as f32 * size;
        (-1, (boundary - position) / direction, -size / direction)
    } else {
        (0, f32::INFINITY, f32::INFINITY)
    }
}

fn trace_grid<T>(
    origin: Vec3,
    direction: Vec3,
    start: f32,
    end: f32,
    size: f32,
    mut visit: impl FnMut(WorldCoord, f32, f32, Vec3) -> Option<T>,
) -> Option<T> {
    if start > end {
        return None;
    }
    let position = origin + direction * (start + 0.000_01);
    let mut coord = WorldCoord::new(
        (position.x / size).floor() as i32,
        (position.y / size).floor() as i32,
        (position.z / size).floor() as i32,
    );
    let (step_x, mut next_x, delta_x) = axis_setup(origin.x, direction.x, coord.x, size);
    let (step_y, mut next_y, delta_y) = axis_setup(origin.y, direction.y, coord.y, size);
    let (step_z, mut next_z, delta_z) = axis_setup(origin.z, direction.z, coord.z, size);
    let mut enter = start;
    let mut entry_normal = Vec3::ZERO;

    while enter <= end {
        let exit = next_x.min(next_y).min(next_z).min(end);
        if let Some(hit) = visit(coord, enter, exit, entry_normal) {
            return Some(hit);
        }
        let next = next_x.min(next_y).min(next_z);
        if !next.is_finite() || next > end {
            break;
        }

        enter = next;
        let mut crossed = Vec3::ZERO;
        if (next_x - next).abs() <= f32::EPSILON {
            coord.x += step_x;
            next_x += delta_x;
            crossed.x = -(step_x as f32);
        }
        if (next_y - next).abs() <= f32::EPSILON {
            coord.y += step_y;
            next_y += delta_y;
            crossed.y = -(step_y as f32);
        }
        if (next_z - next).abs() <= f32::EPSILON {
            coord.z += step_z;
            next_z += delta_z;
            crossed.z = -(step_z as f32);
        }
        entry_normal = crossed.normalize_or_zero();
    }
    None
}

fn initial_hit_normal(direction: Vec3) -> Vec3 {
    let absolute = direction.abs();
    if absolute.x >= absolute.y && absolute.x >= absolute.z {
        Vec3::new(-direction.x.signum(), 0.0, 0.0)
    } else if absolute.y >= absolute.z {
        Vec3::new(0.0, -direction.y.signum(), 0.0)
    } else {
        Vec3::new(0.0, 0.0, -direction.z.signum())
    }
}

pub fn unpack_rgb8(packed: u32) -> Vec3 {
    Vec3::new(
        (packed & 0xff) as f32,
        ((packed >> 8) & 0xff) as f32,
        ((packed >> 16) & 0xff) as f32,
    ) / 255.0
}

fn local_index(coord: WorldCoord) -> usize {
    let (x, y, z) = ChunkCoord::local_offset(coord);
    x as usize | ((y as usize) << 4) | ((z as usize) << 8)
}

fn pack_rgb8(color: Vec3) -> u32 {
    let color = color.clamp(Vec3::ZERO, Vec3::ONE) * 255.0 + Vec3::splat(0.5);
    color.x as u32 | ((color.y as u32) << 8) | ((color.z as u32) << 16)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scene_builds_packed_occupancy_and_materials() {
        let mut world = World::new();
        let coord = WorldCoord::new(-1, 2, 16);
        world.set_cell(coord, CellType::Block);
        world.get_mut(coord).unwrap().color_rgb = Vec3::new(1.0, 0.5, 0.0);

        let mut scene = GiVoxelScene::default();
        let stats = scene.update(&world);
        assert!(stats.full_rebuild);
        assert_eq!(stats.occupied_voxels, 1);
        assert!(scene.occupied(coord));
        assert_eq!(scene.packed_material(coord), Some(0x00_00_80_FF));
    }

    #[test]
    fn scene_updates_only_the_changed_chunk() {
        let mut world = World::new();
        let a = WorldCoord::new(0, 0, 0);
        let b = WorldCoord::new(CHUNK_SIZE, 0, 0);
        world.set_cell(a, CellType::Block);
        world.set_cell(b, CellType::Block);
        let mut scene = GiVoxelScene::default();
        scene.update(&world);

        world.set_cell(a, CellType::Empty);
        let stats = scene.update(&world);
        assert!(!stats.full_rebuild);
        assert_eq!(stats.rebuilt_chunks, 1);
        assert!(!scene.occupied(a));
        assert!(scene.occupied(b));
    }

    #[test]
    fn shifted_static_voxels_are_explicitly_excluded() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);
        world.set_visual_offset_runtime(coord, Vec3::new(0.25, 0.0, 0.0));

        let mut scene = GiVoxelScene::default();
        let stats = scene.update(&world);
        assert!(!scene.occupied(coord));
        assert_eq!(stats.excluded_shifted_voxels, 1);
    }

    #[test]
    fn chunk_grid_trace_reports_nearest_hit_and_face_normal() {
        let mut world = World::new();
        let near = WorldCoord::new(-16, 0, 0);
        let far = WorldCoord::new(1, 0, 0);
        world.set_cell(near, CellType::Block);
        world.set_cell(far, CellType::Block);
        let mut scene = GiVoxelScene::default();
        scene.update(&world);

        let hit = scene
            .trace_chunk_grid(Vec3::new(-20.5, 0.5, 0.5), Vec3::X, 64.0)
            .unwrap();
        assert_eq!(hit.coord, near);
        assert!((hit.distance - 4.5).abs() < 0.0001);
        assert_eq!(hit.normal, -Vec3::X);
    }

    #[test]
    fn chunk_grid_trace_handles_negative_direction_and_misses() {
        let mut world = World::new();
        let coord = WorldCoord::new(-17, -1, 16);
        world.set_cell(coord, CellType::Block);
        let mut scene = GiVoxelScene::default();
        scene.update(&world);

        let hit = scene
            .trace_chunk_grid(Vec3::new(-10.0, -0.5, 16.5), -Vec3::X, 32.0)
            .unwrap();
        assert_eq!(hit.coord, coord);
        assert_eq!(hit.normal, Vec3::X);
        assert!(
            scene
                .trace_chunk_grid(Vec3::new(0.0, 20.0, 0.0), Vec3::Y, 10.0)
                .is_none()
        );
    }

    #[test]
    fn occupied_chunk_bvh_matches_chunk_grid_first_hits() {
        let mut world = World::new();
        for coord in [
            WorldCoord::new(-33, 1, 0),
            WorldCoord::new(4, 1, 0),
            WorldCoord::new(80, 1, 0),
            WorldCoord::new(4, 1, 48),
        ] {
            world.set_cell(coord, CellType::Block);
        }
        let mut scene = GiVoxelScene::default();
        scene.update(&world);

        let rays = [
            (Vec3::new(-50.0, 1.5, 0.5), Vec3::X),
            (Vec3::new(100.0, 1.5, 0.5), Vec3::NEG_X),
            (Vec3::new(4.5, 1.5, 80.0), Vec3::NEG_Z),
            (Vec3::new(0.0, 50.0, 0.0), Vec3::Y),
        ];
        for (origin, direction) in rays {
            let grid = scene.trace_chunk_grid(origin, direction, 200.0);
            let bvh = scene.trace_occupied_chunk_bvh(origin, direction, 200.0);
            assert_eq!(bvh.map(|hit| hit.coord), grid.map(|hit| hit.coord));
        }
    }

    #[test]
    fn automatic_backend_uses_a_valid_query_path() {
        let mut world = World::new();
        let coord = WorldCoord::new(128, -1, -64);
        world.set_cell(coord, CellType::Block);
        let mut scene = GiVoxelScene::default();
        scene.update(&world);

        let hit = scene.trace(
            Vec3::new(100.0, -0.5, -63.5),
            Vec3::X,
            64.0,
            SceneQueryBackend::Automatic,
        );
        assert_eq!(hit.map(|hit| hit.coord), Some(coord));
    }
}
