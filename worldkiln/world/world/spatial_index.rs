use super::World;
use crate::world::cell::{CellType, ChunkCoord};
use crate::world::coordinate::WorldCoord;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug, Default)]
pub struct ChunkCellIndex {
    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    pub resident_coords: HashSet<WorldCoord>,
    pub active_coords: HashSet<WorldCoord>,
    pub solid_coords: HashSet<WorldCoord>,
}

#[derive(Clone, Debug, Default)]
pub struct WorldSpatialIndex {
    pub chunks: HashMap<ChunkCoord, ChunkCellIndex>,
}

impl WorldSpatialIndex {
    pub fn clear(&mut self) {
        self.chunks.clear();
    }

    pub fn update_cell(&mut self, coord: WorldCoord, resident: bool, active: bool, solid: bool) {
        let chunk_coord = ChunkCoord::from_world_coord(coord);

        if resident {
            let chunk = self.chunks.entry(chunk_coord).or_default();
            chunk.resident_coords.insert(coord);

            if active {
                chunk.active_coords.insert(coord);
            } else {
                chunk.active_coords.remove(&coord);
            }

            if solid {
                chunk.solid_coords.insert(coord);
            } else {
                chunk.solid_coords.remove(&coord);
            }
        } else if let Some(chunk) = self.chunks.get_mut(&chunk_coord) {
            chunk.resident_coords.remove(&coord);
            chunk.active_coords.remove(&coord);
            chunk.solid_coords.remove(&coord);

            if chunk.resident_coords.is_empty() {
                self.chunks.remove(&chunk_coord);
            }
        }
    }
}

impl World {
    pub fn update_spatial_index_at(&mut self, coord: WorldCoord) {
        if let Some(cell) = self.get_effective_cell(coord) {
            // IMPORTANT:
            // Use the effective/runtime visibility instead of the authored
            // cell.visible value. AeoScript can override visibility at runtime.
            let resident = cell.cell_type != CellType::Empty;
            let active = resident && self.is_cell_visible(coord);
            let solid = active && self.is_cell_solid(coord);

            self.spatial_index
                .update_cell(coord, resident, active, solid);
        } else {
            self.spatial_index.update_cell(coord, false, false, false);
        }
    }

    pub fn rebuild_spatial_index(&mut self) {
        self.spatial_index.clear();

        let coords: Vec<WorldCoord> = self.cells.keys().copied().collect();

        for coord in coords {
            self.update_spatial_index_at(coord);
        }

        let runtime_coords: Vec<WorldCoord> = self.runtime_id_to_coord.values().copied().collect();

        for coord in runtime_coords {
            self.update_spatial_index_at(coord);
        }
    }

    pub fn iter_active_chunks(&self) -> impl Iterator<Item = ChunkCoord> + '_ {
        self.spatial_index
            .chunks
            .iter()
            .filter_map(|(coord, chunk)| (!chunk.active_coords.is_empty()).then_some(*coord))
    }

    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    pub fn iter_resident_chunks(&self) -> impl Iterator<Item = ChunkCoord> + '_ {
        self.spatial_index.chunks.keys().copied()
    }

    pub fn get_resident_coords_in_chunk(
        &self,
        chunk_coord: ChunkCoord,
    ) -> Option<&HashSet<WorldCoord>> {
        self.spatial_index
            .chunks
            .get(&chunk_coord)
            .map(|c| &c.resident_coords)
    }

    pub fn get_active_coords_in_chunk(
        &self,
        chunk_coord: ChunkCoord,
    ) -> Option<&HashSet<WorldCoord>> {
        self.spatial_index
            .chunks
            .get(&chunk_coord)
            .map(|c| &c.active_coords)
    }

    pub fn get_solid_coords_in_chunk(
        &self,
        chunk_coord: ChunkCoord,
    ) -> Option<&HashSet<WorldCoord>> {
        self.spatial_index
            .chunks
            .get(&chunk_coord)
            .map(|c| &c.solid_coords)
    }

    pub fn query_solid_coords_in_aabb(
        &self,
        min_coord: WorldCoord,
        max_coord: WorldCoord,
    ) -> Vec<(WorldCoord, Vec3)> {
        let min_chunk = ChunkCoord::from_world_coord(min_coord);
        let max_chunk = ChunkCoord::from_world_coord(max_coord);

        let mut results = Vec::new();

        for cx in min_chunk.x..=max_chunk.x {
            for cy in min_chunk.y..=max_chunk.y {
                for cz in min_chunk.z..=max_chunk.z {
                    let chunk_coord = ChunkCoord::new(cx, cy, cz);

                    if let Some(solid_coords) = self
                        .spatial_index
                        .chunks
                        .get(&chunk_coord)
                        .map(|c| &c.solid_coords)
                    {
                        for &coord in solid_coords {
                            if coord.x >= min_coord.x
                                && coord.x <= max_coord.x
                                && coord.y >= min_coord.y
                                && coord.y <= max_coord.y
                                && coord.z >= min_coord.z
                                && coord.z <= max_coord.z
                            {
                                let offset = self.get_visual_offset(coord);

                                let pos = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32)
                                    + offset;

                                results.push((coord, pos));
                            }
                        }
                    }
                }
            }
        }

        results
    }
}
