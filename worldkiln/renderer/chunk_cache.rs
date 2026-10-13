use std::collections::HashMap;

use super::RenderMode;
use crate::world::{CellType, ChunkCoord, World, WorldCoord};

use super::Renderer;
use super::chunk::{
    ChunkMesh, GhostChunkMesh, TextureBatchRange, build_cpu_chunk_data, build_cpu_ghost_chunk_data,
    expand_dirty_coords_to_chunks, upload_position_only_vertices,
};
use super::mesh::{BLOCK_VERTEX_FLOATS, upload_block_vertices_3d};

impl Renderer {
    /// Clears and frees all GPU resources for cached voxel chunk meshes.
    ///
    /// Must be called when transitioning project boundaries or resetting the world
    /// to ensure stale project render state does not survive.
    pub fn clear_chunk_cache(&self) {
        let mut cache = self.chunk_cache.borrow_mut();

        for chunk_mesh in cache.values_mut() {
            chunk_mesh.free_gl_resources();
        }

        cache.clear();

        *self.last_render_mode.borrow_mut() = None;
        *self.last_render_revision.borrow_mut() = 0;

        let mut ghost_cache = self.ghost_cache.borrow_mut();

        for ghost_mesh in ghost_cache.values_mut() {
            ghost_mesh.free_gl_resources();
        }

        ghost_cache.clear();

        *self.last_ghost_revision.borrow_mut() = 0;
        *self.last_ghost_mode.borrow_mut() = None;
    }

    /// Updates the cached editor ghosts only when render-relevant world state
    /// or renderer mode changes.
    ///
    /// Unchanged frames must not rescan the World or rebuild ghost meshes.
    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    /// Updates editor ghost meshes from the non-destructive render journal.
    ///
    /// Local authored edits rebuild only affected resident chunks rather than
    /// scanning the entire authored Cell population.
    pub(super) fn update_ghost_cache(&self, world: &World, mode: RenderMode) {
        let revision = world.render_revision();
        let previous_revision = *self.last_ghost_revision.borrow();
        let previous_mode = *self.last_ghost_mode.borrow();

        if previous_revision == revision && previous_mode == Some(mode) {
            return;
        }

        let mode_changed = previous_mode != Some(mode);
        let changes = world.render_changes_since(previous_revision);
        let mut cache = self.ghost_cache.borrow_mut();

        if mode != RenderMode::Editor {
            for ghost_mesh in cache.values_mut() {
                ghost_mesh.free_gl_resources();
            }
            cache.clear();
        } else if mode_changed || changes.full_resync_required {
            for ghost_mesh in cache.values_mut() {
                ghost_mesh.free_gl_resources();
            }
            cache.clear();

            for chunk_coord in world.iter_resident_chunks() {
                let coords = world
                    .get_resident_coords_in_chunk(chunk_coord)
                    .map(|coords| coords.iter().copied().collect::<Vec<_>>())
                    .unwrap_or_default();
                self.rebuild_single_ghost_chunk(world, &mut cache, chunk_coord, &coords);
            }
        } else {
            let dirty_chunks = expand_dirty_coords_to_chunks(&changes.changes);
            for chunk_coord in dirty_chunks {
                let coords = world
                    .get_resident_coords_in_chunk(chunk_coord)
                    .map(|coords| coords.iter().copied().collect::<Vec<_>>())
                    .unwrap_or_default();
                self.rebuild_single_ghost_chunk(world, &mut cache, chunk_coord, &coords);
            }
        }

        *self.last_ghost_revision.borrow_mut() = revision;
        *self.last_ghost_mode.borrow_mut() = Some(mode);
    }

    fn rebuild_single_ghost_chunk(
        &self,
        world: &World,
        cache: &mut HashMap<ChunkCoord, GhostChunkMesh>,
        chunk_coord: ChunkCoord,
        coords_in_chunk: &[WorldCoord],
    ) {
        let cpu_data = build_cpu_ghost_chunk_data(world, chunk_coord, coords_in_chunk);

        if cpu_data.vertices.is_empty() {
            if let Some(mut old_mesh) = cache.remove(&chunk_coord) {
                old_mesh.free_gl_resources();
            }
            return;
        }

        #[cfg(test)]
        let (vao, vbo, vertex_count) =
            (0, 0, (cpu_data.vertices.len() / BLOCK_VERTEX_FLOATS) as i32);

        #[cfg(not(test))]
        let (vao, vbo, vertex_count) = upload_block_vertices_3d(&cpu_data.vertices);

        if let Some(mut old_mesh) = cache.remove(&chunk_coord) {
            old_mesh.free_gl_resources();
        }

        cache.insert(
            chunk_coord,
            GhostChunkMesh {
                chunk_coord,
                vao,
                vbo,
                vertex_count,
            },
        );
    }

    pub fn update_chunk_cache(&self, world: &World, mode: RenderMode) {
        self.update_ghost_cache(world, mode);

        let dirty_coords = world.drain_render_dirty_cells();

        let mode_changed = *self.last_render_mode.borrow() != Some(mode);

        let revision_reset = world.render_revision() < *self.last_render_revision.borrow();

        if dirty_coords.is_empty()
            && !mode_changed
            && !revision_reset
            && !self.chunk_cache.borrow().is_empty()
        {
            // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
            *self.last_render_revision.borrow_mut() = world.render_revision();
            return;
        }

        let mut cache = self.chunk_cache.borrow_mut();

        if mode_changed || revision_reset || cache.is_empty() {
            for chunk_mesh in cache.values_mut() {
                chunk_mesh.free_gl_resources();
            }

            cache.clear();

            for chunk_coord in world.iter_active_chunks() {
                if let Some(coords) = world.get_active_coords_in_chunk(chunk_coord) {
                    let coords_vec: Vec<WorldCoord> = coords.iter().copied().collect();

                    self.rebuild_single_chunk(world, &mut cache, chunk_coord, &coords_vec, mode);
                }
            }

            *self.last_render_mode.borrow_mut() = Some(mode);

            *self.last_render_revision.borrow_mut() = world.render_revision();

            return;
        }

        let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);

        for chunk_coord in dirty_chunks {
            let coords_vec: Vec<WorldCoord> = world
                .get_active_coords_in_chunk(chunk_coord)
                .map(|set| set.iter().copied().collect())
                .unwrap_or_default();

            self.rebuild_single_chunk(world, &mut cache, chunk_coord, &coords_vec, mode);
        }

        *self.last_render_revision.borrow_mut() = world.render_revision();
    }

    fn rebuild_single_chunk(
        &self,
        world: &World,
        cache: &mut HashMap<ChunkCoord, ChunkMesh>,
        chunk_coord: ChunkCoord,
        coords_in_chunk: &[WorldCoord],
        mode: RenderMode,
    ) {
        let cpu_data = build_cpu_chunk_data(world, chunk_coord, coords_in_chunk, mode);

        // A streamed chunk can become empty when it is evicted. Remove its
        // previous GPU mesh instead of leaving stale geometry visible.
        if cpu_data.main_texture_vertices.is_empty() && cpu_data.shadow_positions.is_empty() {
            if let Some(mut old_mesh) = cache.remove(&chunk_coord) {
                old_mesh.free_gl_resources();
            }

            return;
        }

        let mut main_vao = 0;
        let mut main_vbo = 0;
        let mut main_texture_batches = Vec::new();

        let mut combined_main_vertices = Vec::new();
        let mut current_vertex_offset = 0i32;

        let mut texture_ids: Vec<_> = cpu_data.main_texture_vertices.keys().cloned().collect();

        texture_ids.sort();

        for tex_id in texture_ids {
            if let Some(verts) = cpu_data.main_texture_vertices.get(&tex_id) {
                if verts.is_empty() {
                    continue;
                }

                let count = (verts.len() / BLOCK_VERTEX_FLOATS) as i32;

                main_texture_batches.push(TextureBatchRange {
                    texture_id: tex_id,
                    start_vertex: current_vertex_offset,
                    vertex_count: count,
                });

                combined_main_vertices.extend_from_slice(verts);

                current_vertex_offset += count;
            }
        }

        if !combined_main_vertices.is_empty() {
            let (vao, vbo, _) = upload_block_vertices_3d(&combined_main_vertices);

            main_vao = vao;
            main_vbo = vbo;
        }

        let mut shadow_vao = 0;
        let mut shadow_vbo = 0;
        let mut shadow_vertex_count = 0;

        if !cpu_data.shadow_positions.is_empty() {
            let (s_vao, s_vbo, count) = upload_position_only_vertices(&cpu_data.shadow_positions);

            shadow_vao = s_vao;
            shadow_vbo = s_vbo;
            shadow_vertex_count = count;
        }

        if main_vao != 0 || shadow_vao != 0 {
            if let Some(mut old_mesh) = cache.remove(&chunk_coord) {
                old_mesh.free_gl_resources();
            }

            let chunk_mesh = ChunkMesh {
                chunk_coord,
                main_vao,
                main_vbo,
                main_texture_batches,
                shadow_vao,
                shadow_vbo,
                shadow_vertex_count,
            };

            cache.insert(chunk_coord, chunk_mesh);
        } else if let Some(mut old_mesh) = cache.remove(&chunk_coord) {
            old_mesh.free_gl_resources();
        }
    }
}
#[cfg(test)]
mod tests {
    use super::RenderMode;
    use super::*;

    use crate::renderer::Renderer;
    use crate::world::DirtyReason;

    use std::hint::black_box;
    use std::time::{Duration, Instant};
    const WORLD_BENCHMARK_COUNTS: &[usize] = &[
        5_000, 10_000, 20_000, 30_000, 50_000, 75_000, 100_000, 200_000, 1_000_000,
    ];

    /// Safety ceiling for the number of chunks a benchmark case is allowed
    /// to create. This specifically prevents pathological 1-D layouts from
    /// allocating tens of thousands of chunk buckets.
    const MAX_ESTIMATED_CHUNKS: usize = 4_096;

    /// A single benchmark case must not monopolize the machine indefinitely.
    const MAX_WORLD_CASE_TIME: Duration = Duration::from_secs(3);

    /// A single spatial distribution gets a larger aggregate allowance while
    /// still guaranteeing that the benchmark will stop.
    const MAX_WORLD_SHAPE_TIME: Duration = Duration::from_secs(12);

    /// Absolute ceiling for the complete large-world benchmark.
    const MAX_WORLD_BENCHMARK_TIME: Duration = Duration::from_secs(30);

    /// A sudden timing explosion relative to the previous size is treated as
    /// suspicious and terminates that spatial distribution.
    const MAX_WORLD_TIME_MULTIPLIER: f64 = 6.0;

    const MARKER_COUNTS_PER_TYPE: &[usize] = &[5_000, 10_000, 25_000, 50_000, 100_000];

    const MAX_MARKER_CASE_CELLS: usize = 300_000;
    const MAX_MARKER_CASE_TIME: Duration = Duration::from_secs(3);
    const MAX_MARKER_TOTAL_TIME: Duration = Duration::from_secs(15);
    const MAX_MARKER_TIME_MULTIPLIER: f64 = 6.0;

    #[derive(Clone, Copy, Debug)]
    enum WorldBenchmarkShape {
        Cube,
        FlatPlane,
        XLine,
    }

    impl WorldBenchmarkShape {
        fn name(self) -> &'static str {
            match self {
                Self::Cube => "3D cube",
                Self::FlatPlane => "2D flat plane",
                Self::XLine => "1D X line",
            }
        }
    }

    fn ceil_div(value: usize, divisor: usize) -> usize {
        value.div_ceil(divisor)
    }

    fn estimated_chunk_count(block_count: usize, shape: WorldBenchmarkShape) -> usize {
        match shape {
            WorldBenchmarkShape::Cube => {
                let side = (block_count as f64).cbrt().ceil() as usize;
                let chunks_per_axis = ceil_div(side.max(1), 16);

                chunks_per_axis
                    .saturating_mul(chunks_per_axis)
                    .saturating_mul(chunks_per_axis)
            }
            WorldBenchmarkShape::FlatPlane => {
                let side = (block_count as f64).sqrt().ceil() as usize;
                let chunks_per_axis = ceil_div(side.max(1), 16);

                chunks_per_axis.saturating_mul(chunks_per_axis)
            }
            WorldBenchmarkShape::XLine => ceil_div(block_count.max(1), 16),
        }
    }

    fn populate_world_for_benchmark(
        world: &mut World,
        block_count: usize,
        shape: WorldBenchmarkShape,
    ) {
        match shape {
            WorldBenchmarkShape::Cube => {
                let side = (block_count as f64).cbrt().ceil() as i32;

                let mut count = 0usize;

                'outer: for x in 0..side {
                    for y in 0..side {
                        for z in 0..side {
                            world.set_cell(WorldCoord::new(x, y, z), CellType::Block);

                            count += 1;

                            if count >= block_count {
                                break 'outer;
                            }
                        }
                    }
                }
            }

            WorldBenchmarkShape::FlatPlane => {
                let side = (block_count as f64).sqrt().ceil() as i32;

                let mut count = 0usize;

                'outer: for x in 0..side {
                    for z in 0..side {
                        world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);

                        count += 1;

                        if count >= block_count {
                            break 'outer;
                        }
                    }
                }
            }

            WorldBenchmarkShape::XLine => {
                for x in 0..block_count {
                    world.set_cell(WorldCoord::new(x as i32, 0, 0), CellType::Block);
                }
            }
        }
    }

    fn run_world_shape_benchmark(shape: WorldBenchmarkShape, total_start: Instant) {
        let shape_start = Instant::now();
        let mut previous_full_build: Option<Duration> = None;

        for &block_count in WORLD_BENCHMARK_COUNTS {
            if total_start.elapsed() >= MAX_WORLD_BENCHMARK_TIME {
                println!(
                    "[BENCHMARK BREAK] total world benchmark cutoff reached after {:?}; stopping before {} blocks ({})",
                    total_start.elapsed(),
                    block_count,
                    shape.name()
                );
                break;
            }

            if shape_start.elapsed() >= MAX_WORLD_SHAPE_TIME {
                println!(
                    "[BENCHMARK BREAK] {} shape cutoff reached after {:?}; stopping before {} blocks",
                    shape.name(),
                    shape_start.elapsed(),
                    block_count
                );
                break;
            }

            let estimated_chunks = estimated_chunk_count(block_count, shape);

            if estimated_chunks > MAX_ESTIMATED_CHUNKS {
                println!(
                    "[BENCHMARK BREAK] {} would require approximately {} chunks for {} blocks; safety ceiling is {}",
                    shape.name(),
                    estimated_chunks,
                    block_count,
                    MAX_ESTIMATED_CHUNKS
                );
                break;
            }

            let case_start = Instant::now();

            let mut world = World::new();

            let population_start = Instant::now();

            populate_world_for_benchmark(&mut world, block_count, shape);

            let population_time = population_start.elapsed();

            if world.resident_authored_cell_count() != block_count {
                println!(
                    "[BENCHMARK BREAK] {} blocks requested but world contains {}; stopping on unexpected population result",
                    block_count,
                    world.resident_authored_cell_count()
                );
                break;
            }

            if case_start.elapsed() >= MAX_WORLD_CASE_TIME {
                println!(
                    "[BENCHMARK BREAK] {} {} case exceeded {:?} during world setup ({:?}); stopping this shape",
                    shape.name(),
                    block_count,
                    MAX_WORLD_CASE_TIME,
                    case_start.elapsed()
                );
                break;
            }

            // The population mutations intentionally populate render dirtiness.
            // Drain it before the full-build measurement so the benchmark only
            // measures the CPU chunk construction itself.
            let _ = world.drain_render_dirty_cells();

            let full_build_start = Instant::now();

            let mut total_batches = 0usize;
            let mut built_chunks = 0usize;

            for chunk_coord in world.iter_active_chunks() {
                if let Some(coords) = world.get_active_coords_in_chunk(chunk_coord) {
                    let coords_vec: Vec<WorldCoord> = coords.iter().copied().collect();

                    let cpu_data =
                        build_cpu_chunk_data(&world, chunk_coord, &coords_vec, RenderMode::Editor);

                    total_batches += cpu_data.main_texture_vertices.len();

                    built_chunks += 1;

                    if full_build_start.elapsed() >= MAX_WORLD_CASE_TIME {
                        println!(
                            "[BENCHMARK BREAK] {} {} CPU chunk build exceeded {:?} at chunk {} of approximately {}",
                            shape.name(),
                            block_count,
                            MAX_WORLD_CASE_TIME,
                            built_chunks,
                            estimated_chunks
                        );
                        return;
                    }
                }
            }

            let full_build_time = full_build_start.elapsed();

            if let Some(previous) = previous_full_build {
                if previous > Duration::from_millis(1)
                    && full_build_time.as_secs_f64()
                        > previous.as_secs_f64() * MAX_WORLD_TIME_MULTIPLIER
                {
                    println!(
                        "[BENCHMARK BREAK] {} {} produced suspicious full-build growth: {:?} after {:?}; multiplier limit is {:.1}x",
                        shape.name(),
                        block_count,
                        full_build_time,
                        previous,
                        MAX_WORLD_TIME_MULTIPLIER
                    );
                    break;
                }
            }

            previous_full_build = Some(full_build_time);

            if case_start.elapsed() >= MAX_WORLD_CASE_TIME {
                println!(
                    "[BENCHMARK BREAK] {} {} case exceeded {:?} after full build ({:?}); stopping this shape",
                    shape.name(),
                    block_count,
                    MAX_WORLD_CASE_TIME,
                    case_start.elapsed()
                );
                break;
            }

            // Measure one real selective edit after the initial dirty state has
            // been drained, matching the previous benchmark methodology.
            world.set_cell(WorldCoord::new(0, 0, 0), CellType::Empty);

            let selective_start = Instant::now();

            let dirty_coords = world.drain_render_dirty_cells();

            let dirty_chunks = expand_dirty_coords_to_chunks(&dirty_coords);

            let mut edit_batches = 0usize;

            for chunk_coord in dirty_chunks {
                let coords_vec: Vec<WorldCoord> = world
                    .get_active_coords_in_chunk(chunk_coord)
                    .map(|set| set.iter().copied().collect())
                    .unwrap_or_default();

                let cpu_data =
                    build_cpu_chunk_data(&world, chunk_coord, &coords_vec, RenderMode::Editor);

                edit_batches += cpu_data.main_texture_vertices.len();
            }

            let selective_edit_time = selective_start.elapsed();

            println!(
                "[BENCHMARK] Shape: {} | Blocks: {} | Estimated chunks: {} | Built chunks: {} | Population: {:?} | Full CPU build: {:?} | Selective edit rebuild: {:?} | Full batches: {} | Edit batches: {}",
                shape.name(),
                block_count,
                estimated_chunks,
                built_chunks,
                population_time,
                full_build_time,
                selective_edit_time,
                total_batches,
                edit_batches
            );
        }
    }

    #[test]
    fn test_renderer_chunk_cache_cleared_on_project_reset() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);

        let mock_mesh = ChunkMesh {
            chunk_coord: ChunkCoord::new(0, 0, 0),
            main_vao: 0,
            main_vbo: 0,
            main_texture_batches: Vec::new(),
            shadow_vao: 0,
            shadow_vbo: 0,
            shadow_vertex_count: 0,
        };

        renderer
            .chunk_cache
            .borrow_mut()
            .insert(ChunkCoord::new(0, 0, 0), mock_mesh);

        assert!(!renderer.chunk_cache.borrow().is_empty());

        renderer.clear_chunk_cache();

        assert!(
            renderer.chunk_cache.borrow().is_empty(),
            "Chunk cache must be completely empty after clear_chunk_cache()"
        );

        assert_eq!(*renderer.last_render_revision.borrow(), 0);

        assert_eq!(*renderer.last_render_mode.borrow(), None);

        assert!(
            renderer.ghost_cache.borrow().is_empty(),
            "Ghost cache must be cleared when the renderer cache is cleared"
        );

        assert_eq!(*renderer.last_ghost_revision.borrow(), 0);

        assert_eq!(*renderer.last_ghost_mode.borrow(), None);
    }

    #[test]
    fn test_renderer_chunk_cache_invalidates_on_world_revision_reset() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);

        let mock_mesh = ChunkMesh {
            chunk_coord: ChunkCoord::new(0, 0, 0),
            main_vao: 0,
            main_vbo: 0,
            main_texture_batches: Vec::new(),
            shadow_vao: 0,
            shadow_vbo: 0,
            shadow_vertex_count: 0,
        };

        renderer
            .chunk_cache
            .borrow_mut()
            .insert(ChunkCoord::new(0, 0, 0), mock_mesh);

        let new_blank_world = World::new();

        renderer.update_chunk_cache(&new_blank_world, RenderMode::Editor);

        assert!(
            renderer.chunk_cache.borrow().is_empty(),
            "Chunk cache must be empty for blank world with 0 blocks"
        );

        assert_eq!(
            *renderer.last_render_revision.borrow(),
            new_blank_world.render_revision()
        );
    }

    #[test]
    fn test_renderer_selective_chunk_rebuild_preserves_unmodified_chunks() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);

        let mut world = World::new();

        *renderer.last_render_mode.borrow_mut() = Some(RenderMode::Editor);
        *renderer.last_render_revision.borrow_mut() = world.render_revision();

        let distant_chunk = ChunkCoord::new(10, 10, 10);

        let mock_mesh = ChunkMesh {
            chunk_coord: distant_chunk,
            main_vao: 0,
            main_vbo: 0,
            main_texture_batches: Vec::new(),
            shadow_vao: 0,
            shadow_vbo: 0,
            shadow_vertex_count: 0,
        };

        renderer
            .chunk_cache
            .borrow_mut()
            .insert(distant_chunk, mock_mesh);

        world.mark_render_dirty(WorldCoord::new(0, 0, 0), DirtyReason::Geometry);

        world.bump_render_revision();

        renderer.update_chunk_cache(&world, RenderMode::Editor);

        let cache = renderer.chunk_cache.borrow();

        assert!(
            cache.contains_key(&distant_chunk),
            "Distant chunk mesh should remain cached when an unrelated chunk is dirtied"
        );
    }

    #[test]
    fn test_renderer_ghost_cache_rebuilds_only_when_render_state_changes() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);

        let mut world = World::new();

        let coord = WorldCoord::new(0, 0, 0);

        world.set_cell(coord, CellType::Block);
        world.set_cell_visible_runtime(coord, false);

        renderer.update_ghost_cache(&world, RenderMode::Editor);

        let first_revision = *renderer.last_ghost_revision.borrow();

        let first_cache_len = renderer.ghost_cache.borrow().len();
        let first_vertex_count = renderer
            .ghost_cache
            .borrow()
            .values()
            .next()
            .map(|mesh| mesh.vertex_count)
            .unwrap_or(0);

        assert_eq!(first_cache_len, 1);
        assert_eq!(first_vertex_count, 36);

        renderer.update_ghost_cache(&world, RenderMode::Editor);

        assert_eq!(*renderer.last_ghost_revision.borrow(), first_revision);

        assert_eq!(renderer.ghost_cache.borrow().len(), first_cache_len);

        assert_eq!(
            renderer
                .ghost_cache
                .borrow()
                .values()
                .next()
                .map(|mesh| mesh.vertex_count)
                .unwrap_or(0),
            first_vertex_count
        );
    }

    #[test]
    fn test_renderer_ghost_cache_updates_when_cell_render_state_changes() {
        use glam::Vec3;

        let renderer = Renderer::new_for_tests(800.0, 600.0);

        let mut world = World::new();

        let coord = WorldCoord::new(0, 0, 0);

        world.set_cell(coord, CellType::Block);
        world.set_cell_visible_runtime(coord, false);

        renderer.update_ghost_cache(&world, RenderMode::Editor);

        let first_revision = *renderer.last_ghost_revision.borrow();

        world.set_cell_color_runtime(coord, Vec3::new(1.0, 0.0, 0.0));

        renderer.update_ghost_cache(&world, RenderMode::Editor);

        assert_eq!(renderer.ghost_cache.borrow().len(), 1);

        assert!(
            *renderer.last_ghost_revision.borrow() > first_revision,
            "Ghost cache revision must advance after a render-relevant world change"
        );
    }

    #[test]
    fn test_renderer_ghost_cache_clears_outside_editor_mode() {
        let renderer = Renderer::new_for_tests(800.0, 600.0);

        let mut world = World::new();

        let coord = WorldCoord::new(0, 0, 0);

        world.set_cell(coord, CellType::Block);
        world.set_cell_visible_runtime(coord, false);

        renderer.update_ghost_cache(&world, RenderMode::Editor);

        assert_eq!(renderer.ghost_cache.borrow().len(), 1);

        renderer.update_ghost_cache(&world, RenderMode::Game);

        assert!(
            renderer.ghost_cache.borrow().is_empty(),
            "Editor ghost geometry must not remain cached in Play mode"
        );
    }

    #[test]
    #[ignore]
    fn test_benchmark_large_world_performance_cliff() {
        let total_start = Instant::now();

        for shape in [
            WorldBenchmarkShape::Cube,
            WorldBenchmarkShape::FlatPlane,
            WorldBenchmarkShape::XLine,
        ] {
            if total_start.elapsed() >= MAX_WORLD_BENCHMARK_TIME {
                println!(
                    "[BENCHMARK BREAK] global world benchmark cutoff reached after {:?}; stopping before {}",
                    total_start.elapsed(),
                    shape.name()
                );
                break;
            }

            println!("[BENCHMARK SHAPE START] {}", shape.name());

            run_world_shape_benchmark(shape, total_start);
        }
    }
}
