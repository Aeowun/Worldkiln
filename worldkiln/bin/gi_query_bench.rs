//! CPU research proxy for static voxel ray traversal. Not a GPU GI implementation.

use std::collections::{HashMap, HashSet};
use std::hint::black_box;
use std::path::Path;
use std::time::{Duration, Instant};

use Worldkiln::world::persistence::load_world;
use Worldkiln::world::{CHUNK_SIZE, CellType, ChunkCoord, World, WorldCoord};
use glam::Vec3;
use rand::{Rng, SeedableRng, rngs::StdRng};

const RAY_COUNT: usize = 50_000;
const MAX_DISTANCE: f64 = 512.0;
const START_EPSILON: f64 = 0.0000001;

#[derive(Clone, Copy)]
struct Ray {
    origin: Vec3,
    dir: Vec3,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Hit {
    coord: WorldCoord,
}

#[derive(Clone, Copy, Default)]
struct TraceStats {
    cell_steps: u64,
    chunk_steps: u64,
    hits: u64,
    checksum: i64,
}

struct Occupancy {
    flat: HashSet<WorldCoord>,
    chunks: HashMap<ChunkCoord, [u64; 64]>,
    coords: Vec<WorldCoord>,
    shifted_cells: usize,
}

#[derive(Clone, Copy)]
struct BvhNode {
    min: Vec3,
    max: Vec3,
    left: u32,
    right: u32,
    chunk: Option<ChunkCoord>,
}

struct ChunkBvh {
    nodes: Vec<BvhNode>,
    root: u32,
}

impl ChunkBvh {
    fn build(occupancy: &Occupancy) -> Self {
        let mut chunks: Vec<_> = occupancy.chunks.keys().copied().collect();
        let mut nodes = Vec::with_capacity(chunks.len().saturating_mul(2).saturating_sub(1));
        let root = build_bvh_node(&mut nodes, &mut chunks);
        Self { nodes, root }
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

impl Occupancy {
    fn from_world(world: &World) -> Self {
        let mut eligible = Vec::new();
        let mut shifted_cells = 0;
        for chunk in world.iter_active_chunks() {
            let Some(coords) = world.get_active_coords_in_chunk(chunk) else {
                continue;
            };
            for &coord in coords {
                let Some(cell) = world.get_effective_cell(coord) else {
                    continue;
                };
                if !matches!(
                    cell.cell_type,
                    CellType::Block | CellType::SpawnPoint | CellType::Light
                ) || !world.is_cell_visible(coord)
                    || !world.is_cell_anchored(coord)
                {
                    continue;
                }
                if world.get_visual_offset(coord).length_squared() > 0.000001 {
                    shifted_cells += 1;
                }
                eligible.push(coord);
            }
        }
        let mut occupancy = Self::from_coords(eligible);
        occupancy.shifted_cells = shifted_cells;
        occupancy
    }

    fn from_coords(mut coords: Vec<WorldCoord>) -> Self {
        let mut occupancy = Self {
            flat: HashSet::with_capacity(coords.len()),
            chunks: HashMap::new(),
            coords: Vec::new(),
            shifted_cells: 0,
        };
        for &coord in &coords {
            occupancy.flat.insert(coord);
            let chunk = ChunkCoord::from_world_coord(coord);
            let (x, y, z) = ChunkCoord::local_offset(coord);
            let index = x as usize | ((y as usize) << 4) | ((z as usize) << 8);
            occupancy.chunks.entry(chunk).or_insert([0; 64])[index / 64] |= 1 << (index % 64);
        }
        coords.sort_unstable_by_key(|coord| (coord.x, coord.y, coord.z));
        occupancy.coords = coords;
        occupancy
    }

    fn occupied(&self, coord: WorldCoord) -> bool {
        let chunk = ChunkCoord::from_world_coord(coord);
        let Some(bits) = self.chunks.get(&chunk) else {
            return false;
        };
        let (x, y, z) = ChunkCoord::local_offset(coord);
        let index = x as usize | ((y as usize) << 4) | ((z as usize) << 8);
        bits[index / 64] & (1 << (index % 64)) != 0
    }

    fn set_occupied(&mut self, coord: WorldCoord, occupied: bool) {
        let chunk = ChunkCoord::from_world_coord(coord);
        let (x, y, z) = ChunkCoord::local_offset(coord);
        let index = x as usize | ((y as usize) << 4) | ((z as usize) << 8);
        let mask = 1u64 << (index % 64);
        if occupied {
            self.flat.insert(coord);
            self.chunks.entry(chunk).or_insert([0; 64])[index / 64] |= mask;
        } else {
            self.flat.remove(&coord);
            if let Some(bits) = self.chunks.get_mut(&chunk) {
                bits[index / 64] &= !mask;
                if bits.iter().all(|word| *word == 0) {
                    self.chunks.remove(&chunk);
                }
            }
        }
    }
}

fn axis_setup(position: f64, direction: f64, coord: i32, size: f64) -> (i32, f64, f64) {
    if direction > 0.0 {
        let boundary = (coord as f64 + 1.0) * size;
        (1, (boundary - position) / direction, size / direction)
    } else if direction < 0.0 {
        let boundary = coord as f64 * size;
        (-1, (boundary - position) / direction, -size / direction)
    } else {
        (0, f64::INFINITY, f64::INFINITY)
    }
}

fn trace_grid<F>(ray: Ray, start: f64, end: f64, size: f64, mut visit: F) -> (Option<Hit>, u64)
where
    F: FnMut(WorldCoord, f64, f64) -> Option<Hit>,
{
    if start > end {
        return (None, 0);
    }
    let origin = ray.origin.as_dvec3();
    let dir = ray.dir.as_dvec3();
    let position = origin + dir * (start + START_EPSILON);
    let mut coord = WorldCoord::new(
        (position.x / size).floor() as i32,
        (position.y / size).floor() as i32,
        (position.z / size).floor() as i32,
    );
    let (sx, mut tx, dx) = axis_setup(origin.x, dir.x, coord.x, size);
    let (sy, mut ty, dy) = axis_setup(origin.y, dir.y, coord.y, size);
    let (sz, mut tz, dz) = axis_setup(origin.z, dir.z, coord.z, size);
    let mut t = start;
    let mut steps = 0;
    while t <= end {
        let next = tx.min(ty).min(tz);
        steps += 1;
        if let Some(hit) = visit(coord, t, next.min(end)) {
            return (Some(hit), steps);
        }
        if !next.is_finite() || next > end {
            break;
        }
        t = next;
        // Crossing tied faces moves along every tied axis. Zero-width cells
        // touched only at a corner do not count as intersections.
        if tx == next {
            coord.x += sx;
            tx += dx;
        }
        if ty == next {
            coord.y += sy;
            ty += dy;
        }
        if tz == next {
            coord.z += sz;
            tz += dz;
        }
    }
    (None, steps)
}

fn trace_flat(occupancy: &Occupancy, ray: Ray) -> (Option<Hit>, u64) {
    trace_grid(ray, 0.0, MAX_DISTANCE, 1.0, |coord, _, _| {
        occupancy.flat.contains(&coord).then_some(Hit { coord })
    })
}

fn trace_chunked(occupancy: &Occupancy, ray: Ray) -> (Option<Hit>, u64, u64) {
    let mut cell_steps = 0;
    let (hit, chunk_steps) = trace_grid(
        ray,
        0.0,
        MAX_DISTANCE,
        CHUNK_SIZE as f64,
        |coord, start, end| {
            let chunk = ChunkCoord::new(coord.x, coord.y, coord.z);
            if !occupancy.chunks.contains_key(&chunk) {
                return None;
            }
            let (hit, steps) = trace_grid(ray, start, end, 1.0, |cell, _, _| {
                occupancy.occupied(cell).then_some(Hit { coord: cell })
            });
            cell_steps += steps;
            hit
        },
    );
    (hit, chunk_steps, cell_steps)
}

fn ray_aabb(ray: Ray, min: Vec3, max: Vec3) -> Option<(f64, f64)> {
    let origin = ray.origin.as_dvec3();
    let direction = ray.dir.as_dvec3();
    let min = min.as_dvec3();
    let max = max.as_dvec3();
    let mut enter: f64 = 0.0;
    let mut exit: f64 = MAX_DISTANCE;

    for axis in 0..3 {
        let origin_axis = origin[axis];
        let direction_axis = direction[axis];
        if direction_axis.abs() < 1.0e-12 {
            if origin_axis < min[axis] || origin_axis > max[axis] {
                return None;
            }
            continue;
        }
        let inverse = 1.0 / direction_axis;
        let mut near = (min[axis] - origin_axis) * inverse;
        let mut far = (max[axis] - origin_axis) * inverse;
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

fn trace_bvh(occupancy: &Occupancy, bvh: &ChunkBvh, ray: Ray) -> (Option<Hit>, u64, u64) {
    let root = bvh.nodes[bvh.root as usize];
    let Some((root_enter, root_exit)) = ray_aabb(ray, root.min, root.max) else {
        return (None, 1, 0);
    };

    let mut stack = vec![(bvh.root, root_enter, root_exit)];
    let mut best_hit = None;
    let mut best_distance = f64::INFINITY;
    let mut node_tests = 1;
    let mut cell_steps = 0;

    while let Some((node_index, enter, exit)) = stack.pop() {
        if enter > best_distance {
            continue;
        }
        let node = bvh.nodes[node_index as usize];
        if let Some(chunk) = node.chunk {
            let (hit, steps) =
                trace_grid(ray, enter, exit.min(best_distance), 1.0, |cell, _, _| {
                    (ChunkCoord::from_world_coord(cell) == chunk && occupancy.occupied(cell))
                        .then_some(Hit { coord: cell })
                });
            cell_steps += steps;
            if let Some(hit) = hit {
                let cell_min =
                    Vec3::new(hit.coord.x as f32, hit.coord.y as f32, hit.coord.z as f32);
                let hit_distance = ray_aabb(ray, cell_min, cell_min + Vec3::ONE)
                    .map(|bounds| bounds.0)
                    .unwrap_or(enter);
                if hit_distance < best_distance {
                    best_distance = hit_distance;
                    best_hit = Some(hit);
                }
            }
            continue;
        }

        let left = bvh.nodes[node.left as usize];
        let right = bvh.nodes[node.right as usize];
        let left_hit = ray_aabb(ray, left.min, left.max);
        let right_hit = ray_aabb(ray, right.min, right.max);
        node_tests += 2;

        match (left_hit, right_hit) {
            (Some(left_bounds), Some(right_bounds)) => {
                // LIFO: push the farther child first so the nearer child is
                // likely to tighten best_distance before it is revisited.
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

    (best_hit, node_tests, cell_steps)
}

fn rays_for(occupancy: &Occupancy) -> Vec<Ray> {
    let mut rng = StdRng::seed_from_u64(0xAE0_2026);
    let min = occupancy
        .coords
        .iter()
        .fold(Vec3::splat(f32::INFINITY), |min, coord| {
            min.min(Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32))
        });
    let max = occupancy
        .coords
        .iter()
        .fold(Vec3::splat(f32::NEG_INFINITY), |max, coord| {
            max.max(Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32))
        });
    let center = (min + max + Vec3::ONE) * 0.5;
    let radius = ((max - min).length() * 0.6 + 8.0).min(140.0);
    (0..RAY_COUNT)
        .map(|index| {
            let outward = Vec3::new(
                rng.gen_range(-1.0..1.0),
                rng.gen_range(-0.5..0.5),
                rng.gen_range(-1.0..1.0),
            )
            .normalize_or_zero();
            let origin = center + outward * radius;
            let target = if index % 3 == 0 {
                center
                    + Vec3::new(
                        rng.gen_range(-radius..radius),
                        rng.gen_range(-radius..radius),
                        rng.gen_range(-radius..radius),
                    )
            } else {
                let coord = occupancy.coords[rng.gen_range(0..occupancy.coords.len())];
                Vec3::new(
                    coord.x as f32 + 0.5,
                    coord.y as f32 + 0.5,
                    coord.z as f32 + 0.5,
                )
            };
            Ray {
                origin,
                dir: (target - origin).normalize(),
            }
        })
        .collect()
}

fn measure<F>(rays: &[Ray], mut trace: F) -> (Duration, TraceStats, Vec<Option<Hit>>)
where
    F: FnMut(Ray) -> (Option<Hit>, u64, u64),
{
    let start = Instant::now();
    let mut stats = TraceStats::default();
    let mut hits = Vec::with_capacity(rays.len());
    for &ray in rays {
        let (hit, chunks, cells) = trace(black_box(ray));
        stats.chunk_steps += chunks;
        stats.cell_steps += cells;
        if let Some(hit) = hit {
            stats.hits += 1;
            stats.checksum = stats.checksum.wrapping_add(
                hit.coord.x as i64 * 31 + hit.coord.y as i64 * 17 + hit.coord.z as i64,
            );
        }
        hits.push(black_box(hit));
    }
    (start.elapsed(), stats, hits)
}

fn run_occupancy(
    name: &str,
    mut occupancy: Occupancy,
    build_time: Duration,
) -> Result<(), Box<dyn std::error::Error>> {
    if occupancy.coords.is_empty() {
        return Err(format!("{name} has no eligible static cells").into());
    }
    let bvh_build_start = Instant::now();
    let bvh = ChunkBvh::build(&occupancy);
    let bvh_build_time = bvh_build_start.elapsed();
    let rays = rays_for(&occupancy);
    let mut flat_trials = Vec::new();
    let mut chunk_trials = Vec::new();
    let mut bvh_trials = Vec::new();
    let mut chunk_mismatches = 0;
    let mut bvh_mismatches = 0;
    for trial in 0..4 {
        let (flat_time, flat_stats, flat_hits) = measure(&rays, |ray| {
            let (hit, cells) = trace_flat(&occupancy, ray);
            (hit, 0, cells)
        });
        let (chunk_time, chunk_stats, chunk_hits) = measure(&rays, |ray| {
            let (hit, chunks, cells) = trace_chunked(&occupancy, ray);
            (hit, chunks, cells)
        });
        let (bvh_time, bvh_stats, bvh_hits) =
            measure(&rays, |ray| trace_bvh(&occupancy, &bvh, ray));
        let trial_chunk_mismatches = flat_hits
            .iter()
            .zip(&chunk_hits)
            .filter(|(a, b)| a != b)
            .count();
        let trial_bvh_mismatches = flat_hits
            .iter()
            .zip(&bvh_hits)
            .filter(|(a, b)| a != b)
            .count();
        chunk_mismatches += trial_chunk_mismatches;
        bvh_mismatches += trial_bvh_mismatches;
        if trial_bvh_mismatches != 0 && trial == 0 {
            for (index, (flat_hit, chunk_hit)) in flat_hits
                .iter()
                .zip(&bvh_hits)
                .enumerate()
                .filter(|(_, (a, b))| a != b)
                .take(5)
            {
                eprintln!(
                    "BVH mismatch ray={index} origin={:?} dir={:?} flat={flat_hit:?} bvh={chunk_hit:?}",
                    rays[index].origin, rays[index].dir
                );
            }
        }
        if trial > 0 {
            flat_trials.push((flat_time, flat_stats));
            chunk_trials.push((chunk_time, chunk_stats));
            bvh_trials.push((bvh_time, bvh_stats));
        }
    }
    flat_trials.sort_unstable_by_key(|(duration, _)| *duration);
    chunk_trials.sort_unstable_by_key(|(duration, _)| *duration);
    bvh_trials.sort_unstable_by_key(|(duration, _)| *duration);
    let (flat_time, flat) = flat_trials[1];
    let (chunk_time, chunked) = chunk_trials[1];
    let (bvh_time, bvh_stats) = bvh_trials[1];
    println!(
        "world={} cells={} chunks={} shifted={} build_ms={:.3} bitset_payload_bytes={} rays={} max_distance={}",
        name,
        occupancy.coords.len(),
        occupancy.chunks.len(),
        occupancy.shifted_cells,
        build_time.as_secs_f64() * 1000.0,
        occupancy.chunks.len() * 512,
        rays.len(),
        MAX_DISTANCE
    );
    println!(
        "bvh_build_ms={:.3} bvh_nodes={} bvh_node_payload_bytes={}",
        bvh_build_time.as_secs_f64() * 1000.0,
        bvh.nodes.len(),
        bvh.nodes.len() * std::mem::size_of::<BvhNode>()
    );
    println!(
        "flat_ms={:.3} flat_cell_steps={} hits={} checksum={}",
        flat_time.as_secs_f64() * 1000.0,
        flat.cell_steps,
        flat.hits,
        flat.checksum
    );
    println!(
        "chunk_ms={:.3} chunk_steps={} chunk_cell_steps={} hits={} checksum={} mismatches={}",
        chunk_time.as_secs_f64() * 1000.0,
        chunked.chunk_steps,
        chunked.cell_steps,
        chunked.hits,
        chunked.checksum,
        chunk_mismatches
    );
    println!(
        "bvh_ms={:.3} bvh_node_tests={} bvh_cell_steps={} hits={} checksum={} mismatches={}",
        bvh_time.as_secs_f64() * 1000.0,
        bvh_stats.chunk_steps,
        bvh_stats.cell_steps,
        bvh_stats.hits,
        bvh_stats.checksum,
        bvh_mismatches
    );
    let edit_count = 100.min(occupancy.coords.len());
    let edit_start = Instant::now();
    for index in 0..edit_count {
        let coord = occupancy.coords[index * occupancy.coords.len() / edit_count];
        occupancy.set_occupied(coord, false);
        black_box(occupancy.occupied(coord));
        occupancy.set_occupied(coord, true);
        black_box(occupancy.occupied(coord));
    }
    println!(
        "local_edit_us_per_toggle={:.3} edits={} restored_cells={}",
        edit_start.elapsed().as_secs_f64() * 1_000_000.0 / (edit_count * 2) as f64,
        edit_count * 2,
        occupancy.flat.len()
    );
    if chunk_mismatches != 0 || bvh_mismatches != 0 {
        return Err(format!(
            "first-hit mismatches: chunk={} BVH={}",
            chunk_mismatches, bvh_mismatches
        )
        .into());
    }
    Ok(())
}

fn run_world(path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    let mut world = World::new();
    load_world(&mut world, &path.join("world.dat"))?;
    let build_start = Instant::now();
    let occupancy = Occupancy::from_world(&world);
    run_occupancy(
        &path.display().to_string(),
        occupancy,
        build_start.elapsed(),
    )
}

fn synthetic_sparse() -> Vec<WorldCoord> {
    let mut coords = Vec::with_capacity(16 * 16);
    for x in -8..8 {
        for z in -8..8 {
            coords.push(WorldCoord::new(x * CHUNK_SIZE * 4, 0, z * CHUNK_SIZE * 4));
        }
    }
    coords
}

fn synthetic_dense() -> Vec<WorldCoord> {
    let mut coords = Vec::with_capacity(64 * 64 * 64);
    for x in -32..32 {
        for y in -32..32 {
            for z in -32..32 {
                coords.push(WorldCoord::new(x, y, z));
            }
        }
    }
    coords
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let paths: Vec<_> = std::env::args().skip(1).collect();
    if paths.is_empty() {
        eprintln!(
            "Usage: gi-query-bench <project-directory|--synthetic-sparse|--synthetic-dense> [...]"
        );
        std::process::exit(2);
    }
    for path in paths {
        match path.as_str() {
            "--synthetic-sparse" | "--synthetic-dense" => {
                let build_start = Instant::now();
                let coords = if path == "--synthetic-sparse" {
                    synthetic_sparse()
                } else {
                    synthetic_dense()
                };
                let occupancy = Occupancy::from_coords(coords);
                run_occupancy(&path, occupancy, build_start.elapsed())?;
            }
            _ => run_world(Path::new(&path))?,
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn chunked_matches_flat_at_negative_chunk_boundaries() {
        let occupancy = Occupancy::from_coords(vec![
            WorldCoord::new(-17, 0, 0),
            WorldCoord::new(-16, 0, 0),
            WorldCoord::new(-1, 0, 0),
            WorldCoord::new(0, 0, 0),
            WorldCoord::new(16, 0, 0),
        ]);
        let bvh = ChunkBvh::build(&occupancy);
        for ray in [
            Ray {
                origin: Vec3::new(-30.5, 0.5, 0.5),
                dir: Vec3::X,
            },
            Ray {
                origin: Vec3::new(30.5, 0.5, 0.5),
                dir: -Vec3::X,
            },
            Ray {
                origin: Vec3::new(-16.0, 0.5, 0.5),
                dir: Vec3::X,
            },
            Ray {
                origin: Vec3::new(0.0, 0.5, 0.5),
                dir: -Vec3::X,
            },
            Ray {
                origin: Vec3::new(0.5, -10.0, 0.5),
                dir: Vec3::Y,
            },
        ] {
            assert_eq!(
                trace_flat(&occupancy, ray).0,
                trace_chunked(&occupancy, ray).0
            );
            assert_eq!(
                trace_flat(&occupancy, ray).0,
                trace_bvh(&occupancy, &bvh, ray).0
            );
        }
        let mut occupancy = occupancy;
        let coord = WorldCoord::new(-17, 0, 0);
        occupancy.set_occupied(coord, false);
        assert!(!occupancy.occupied(coord));
        occupancy.set_occupied(coord, true);
        assert!(occupancy.occupied(coord));
    }
}
