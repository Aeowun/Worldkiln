// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{Heightmap, HeightmapError};
use crate::world::{TerrainData, TerrainResult};

fn sample_legacy_height(result: &TerrainResult, x: f32, z: f32) -> Option<f32> {
    match &result.data {
        TerrainData::Voxel(grid) => grid.surface_height_at(x, z),
        TerrainData::Heightfield(data) => data.surface_height_at(x, z),
    }
}

pub fn import_legacy_surface_tile(
    result: &TerrainResult,
    start_x: usize,
    end_x: usize,
    start_z: usize,
    end_z: usize,
) -> Result<Heightmap, HeightmapError> {
    import_legacy_surface_tile_with_validity(result, start_x, end_x, start_z, end_z)
        .map(|(source, _)| source)
}

pub fn import_legacy_surface_tile_with_validity(
    result: &TerrainResult,
    start_x: usize,
    end_x: usize,
    start_z: usize,
    end_z: usize,
) -> Result<(Heightmap, Vec<bool>), HeightmapError> {
    let (size_x, size_z, spacing) = match &result.data {
        TerrainData::Voxel(grid) => (
            grid.size[0] as usize,
            grid.size[2] as usize,
            f64::from(grid.spacing_studs),
        ),
        TerrainData::Heightfield(grid) => (grid.size[0] as usize, grid.size[2] as usize, 1.0),
    };
    if start_x >= end_x
        || start_z >= end_z
        || end_x > size_x
        || end_z > size_z
        || !spacing.is_finite()
        || spacing <= 0.0
    {
        return Err(HeightmapError::InvalidBounds);
    }
    let cols = end_x
        .checked_sub(start_x)
        .and_then(|n| n.checked_add(3))
        .ok_or(HeightmapError::InvalidDimensions)?;
    let rows = end_z
        .checked_sub(start_z)
        .and_then(|n| n.checked_add(3))
        .ok_or(HeightmapError::InvalidDimensions)?;
    let count = cols
        .checked_mul(rows)
        .ok_or(HeightmapError::InvalidDimensions)?;
    let min_x = result.request.position[0] as f64;
    let min_z = result.request.position[2] as f64;
    let max_x = min_x + f64::from(result.request.size[0]);
    let max_z = min_z + f64::from(result.request.size[2]);
    let bottom = result.request.position[1] as f64;
    let start_global_x = start_x as f64 - 1.0;
    let start_global_z = start_z as f64 - 1.0;
    let mut heights = Vec::with_capacity(count);
    let mut occupied = Vec::with_capacity(count);
    for z in 0..rows {
        for x in 0..cols {
            let sample_x =
                (min_x + (start_global_x + x as f64) * spacing).clamp(min_x, max_x - 0.0005);
            let sample_z =
                (min_z + (start_global_z + z as f64) * spacing).clamp(min_z, max_z - 0.0005);
            let found = sample_legacy_height(result, sample_x as f32, sample_z as f32);
            let height = found.map(f64::from).unwrap_or(bottom);
            if !height.is_finite() {
                return Err(HeightmapError::InvalidSamples);
            }
            heights.push(height);
            occupied.push(found.is_some());
        }
    }
    Ok((
        Heightmap::new(
            cols,
            rows,
            [
                min_x + start_global_x * spacing,
                0.0,
                min_z + start_global_z * spacing,
            ],
            spacing,
            heights,
        )?,
        occupied,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{HeightfieldData, TerrainGenerationRequest, TerrainGeneratorType};

    #[test]
    fn legacy_import_tiles_share_exact_seams_and_normals() {
        let heights: Vec<f32> = (0..6)
            .flat_map(|z| (0..6).map(move |x| (x * x + z * 2) as f32))
            .collect();
        let historical = TerrainResult {
            request: TerrainGenerationRequest {
                position: [4, 0, 9],
                size: [6, 40, 6],
            },
            generator: TerrainGeneratorType::Plains,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: [4, 0, 9],
                size: [6, 40, 6],
                heights,
            }),
        };
        let a = import_legacy_surface_tile(&historical, 0, 3, 0, 6).unwrap();
        let b = import_legacy_surface_tile(&historical, 3, 6, 0, 6).unwrap();
        let am = a.tile_mesh(1, 1, 3, 6, 1).unwrap();
        let bm = b.tile_mesh(1, 1, 3, 6, 1).unwrap();
        for z in 0..=6 {
            let left = &am.vertices[z * 4 + 3];
            let right = &bm.vertices[z * 4];
            assert_eq!(left.position, right.position);
            assert_eq!(left.normal, right.normal);
        }
    }

    #[test]
    fn legacy_import_rejects_out_of_range_tile_without_touching_source() {
        let historical = TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [4, 4, 4],
            },
            generator: TerrainGeneratorType::Plains,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: [0; 3],
                size: [4, 4, 4],
                heights: vec![0.0; 16],
            }),
        };
        let untouched = historical.clone();
        assert_eq!(
            import_legacy_surface_tile(&historical, 0, 8, 0, 4),
            Err(HeightmapError::InvalidBounds),
        );
        assert_eq!(historical, untouched);
    }
}

pub fn import_legacy_surface(result: &TerrainResult) -> Result<Heightmap, HeightmapError> {
    let (size_x, size_z, spacing) = match &result.data {
        TerrainData::Voxel(grid) => (
            grid.size[0] as usize,
            grid.size[2] as usize,
            f64::from(grid.spacing_studs),
        ),
        TerrainData::Heightfield(grid) => (grid.size[0] as usize, grid.size[2] as usize, 1.0),
    };
    if size_x < 2
        || size_z < 2
        || size_x > 4096
        || size_z > 4096
        || !spacing.is_finite()
        || spacing <= 0.0
    {
        return Err(HeightmapError::InvalidDimensions);
    }
    let columns = size_x
        .checked_add(1)
        .ok_or(HeightmapError::InvalidDimensions)?;
    let rows = size_z
        .checked_add(1)
        .ok_or(HeightmapError::InvalidDimensions)?;
    let count = columns
        .checked_mul(rows)
        .ok_or(HeightmapError::InvalidDimensions)?;
    let min_x = f64::from(result.request.position[0]);
    let min_z = f64::from(result.request.position[2]);
    let max_x = min_x + f64::from(result.request.size[0]);
    let max_z = min_z + f64::from(result.request.size[2]);
    let bottom = result.request.position[1] as f64;
    let mut heights = Vec::with_capacity(count);
    for z in 0..rows {
        for x in 0..columns {
            let world_x = (min_x + x as f64 * spacing).min(max_x - 0.0005);
            let world_z = (min_z + z as f64 * spacing).min(max_z - 0.0005);
            let h = sample_legacy_height(result, world_x as f32, world_z as f32)
                .map(f64::from)
                .unwrap_or(bottom);
            if !h.is_finite() {
                return Err(HeightmapError::InvalidSamples);
            }
            heights.push(h);
        }
    }
    Heightmap::new(columns, rows, [min_x, 0.0, min_z], spacing, heights)
}
