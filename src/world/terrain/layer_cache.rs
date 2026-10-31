// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{
    GeneratorSettings, HeightfieldData, LayerBrushEdit, SmoothVoxel, SmoothVoxelGrid,
    TerrainConfig, TerrainData, TerrainGeneratorType, TerrainResult,
};
use serde::{Deserialize, Serialize};
use std::{
    hash::{Hash, Hasher},
    io::Write,
    path::PathBuf,
};
const MAGIC: &[u8; 8] = b"WK09LC04";
#[derive(Serialize, Deserialize)]
struct Header {
    origin: [i32; 3],
    size: [u32; 3],
    spacing: f32,
    materials: Vec<String>,
    sculpt_count: usize,
    sculpt_prefix: u64,
}
fn path(
    settings: &GeneratorSettings,
    edits: &[LayerBrushEdit],
    cfg: &TerrainConfig,
    source: Option<&HeightfieldData>,
) -> Option<PathBuf> {
    let _ = edits;
    let key = serde_json::to_vec(&(MAGIC, settings, &cfg.materials, &cfg.voxel_layers)).ok()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hash);
    if let Some(source) = source {
        b"worldkiln-imported-heightfield-v1".hash(&mut hash);
        source.origin.hash(&mut hash);
        source.size.hash(&mut hash);
        for height in &source.heights {
            height.to_bits().hash(&mut hash);
        }
    }
    cfg.shore_material_layer.hash(&mut hash);
    let root = std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Aeowun")
        .join("Worldkiln")
        .join("cache")
        .join("landscape-v2");
    Some(root.join(format!("{:016x}.wkgrid", hash.finish())))
}
fn sculpt_prefix_hash(edits: &[LayerBrushEdit], count: usize) -> Option<u64> {
    let sculptures: Vec<_> = edits
        .iter()
        .filter(|e| matches!(e, LayerBrushEdit::Sculpt(_)))
        .take(count)
        .collect();
    if sculptures.len() != count {
        return None;
    }
    let encoded = serde_json::to_vec(&sculptures).ok()?;
    let mut hash = std::collections::hash_map::DefaultHasher::new();
    encoded.hash(&mut hash);
    Some(hash.finish())
}
pub(super) fn load(
    settings: &GeneratorSettings,
    edits: &[LayerBrushEdit],
    cfg: &TerrainConfig,
    source: Option<&HeightfieldData>,
) -> Option<(TerrainResult, usize)> {
    let file = std::fs::read(path(settings, edits, cfg, source)?).ok()?;
    if file.len() < 12 || &file[..8] != MAGIC {
        return None;
    }
    let len = u32::from_le_bytes(file[8..12].try_into().ok()?) as usize;
    if len > 2_000_000 || 12 + len > file.len() {
        return None;
    }
    let header: Header = serde_json::from_slice(&file[12..12 + len]).ok()?;
    if sculpt_prefix_hash(edits, header.sculpt_count)? != header.sculpt_prefix {
        return None;
    }
    let count = header
        .size
        .iter()
        .try_fold(1usize, |a, b| a.checked_mul(*b as usize))?;
    if count > 40_000_000 || file.len() != 12 + len + count * 3 {
        return None;
    }
    if header.origin != settings.request.position
        || !header.spacing.is_finite()
        || header.spacing <= 0.0
    {
        return None;
    }
    let bytes = &file[12 + len..];
    let voxels = bytes
        .chunks_exact(3)
        .map(|b| SmoothVoxel {
            material: u16::from_le_bytes([b[0], b[1]]),
            occupancy: b[2],
        })
        .collect::<Vec<_>>();
    if voxels
        .iter()
        .any(|v| v.material as usize >= header.materials.len() && v.occupancy != 0)
    {
        return None;
    }
    let grid = SmoothVoxelGrid {
        origin: header.origin,
        size: header.size,
        spacing_studs: header.spacing,
        materials: header.materials,
        voxels,
    };
    Some((
        TerrainResult {
            request: settings.request,
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Voxel(grid),
        },
        header.sculpt_count,
    ))
}
pub(super) fn save(
    settings: &GeneratorSettings,
    edits: &[LayerBrushEdit],
    cfg: &TerrainConfig,
    result: &TerrainResult,
    source: Option<&HeightfieldData>,
) {
    let TerrainData::Voxel(grid) = &result.data else {
        return;
    };
    let Some(path) = path(settings, edits, cfg, source) else {
        return;
    };
    let sculpt_count = edits
        .iter()
        .filter(|e| matches!(e, LayerBrushEdit::Sculpt(_)))
        .count();
    let Some(sculpt_prefix) = sculpt_prefix_hash(edits, sculpt_count) else {
        return;
    };
    let header = Header {
        origin: grid.origin,
        size: grid.size,
        spacing: grid.spacing_studs,
        materials: grid.materials.clone(),
        sculpt_count,
        sculpt_prefix,
    };
    let Ok(header) = serde_json::to_vec(&header) else {
        return;
    };
    let Ok(header_len) = u32::try_from(header.len()) else {
        return;
    };
    if std::fs::create_dir_all(path.parent().unwrap()).is_err() {
        return;
    }
    let partial = path.with_extension("wkgrid.tmp");
    let outcome = (|| -> std::io::Result<()> {
        let mut file = std::fs::File::create(&partial)?;
        file.write_all(MAGIC)?;
        file.write_all(&header_len.to_le_bytes())?;
        file.write_all(&header)?;
        let mut buffer = Vec::with_capacity(3 * 16384);
        for slice in grid.voxels.chunks(16384) {
            buffer.clear();
            for voxel in slice {
                buffer.extend_from_slice(&voxel.material.to_le_bytes());
                buffer.push(voxel.occupancy);
            }
            file.write_all(&buffer)?;
        }
        file.sync_all()?;
        std::fs::rename(&partial, &path)?;
        Ok(())
    })();
    if outcome.is_err() {
        let _ = std::fs::remove_file(partial);
    }
}

#[cfg(test)]
mod imported_source_cache_tests {
    use super::*;
    #[test]
    fn imported_cache_identity_tracks_pixels_extent_and_geometry_settings() {
        let settings = GeneratorSettings::default();
        let config = TerrainConfig::default();
        let mut src = HeightfieldData {
            origin: settings.request.position,
            size: settings.request.size.map(|x| x as u32),
            heights: vec![
                3.0;
                settings.request.size[0] as usize * settings.request.size[2] as usize
            ],
        };
        let origin_key = path(&settings, &[], &config, Some(&src)).unwrap();
        let procedural = path(&settings, &[], &config, None).unwrap();
        assert_ne!(origin_key, procedural);
        src.heights[0] += 1.0;
        assert_ne!(
            origin_key,
            path(&settings, &[], &config, Some(&src)).unwrap()
        );
        src.heights[0] -= 1.0;
        src.origin[0] += 1;
        assert_ne!(
            origin_key,
            path(&settings, &[], &config, Some(&src)).unwrap()
        );
        src.origin[0] -= 1;
        let mut changed = config.clone();
        changed.shore_material_layer = "different-shore".to_string();
        assert_ne!(
            origin_key,
            path(&settings, &[], &changed, Some(&src)).unwrap()
        );
    }
}
