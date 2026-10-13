use super::{
    HeightfieldData, LayerBrushEdit, SmoothVoxel, SmoothVoxelGrid, TERRAIN_CHUNK_SIZE,
    TerrainConfig, TerrainData, TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    TerrainState,
};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
const LEGACY_TERRAIN_STORAGE_VERSION: u32 = 1;
const TERRAIN_STORAGE_VERSION: u32 = 2;
// Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
// Reopening authored voxels must not apply the smaller procedural allocation cap.
const MAX_STORED_VOXEL_VOLUME: u64 = super::MAX_GENERATION_VOLUME * 16;
const METADATA_FILE: &str = "terrain.json";
const CHUNK_EXTENSION: &str = "terrainchunk";
// Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
const LAYER_EDIT_CHUNK_EXTENSION: &str = "layerchunk";
const LAYER_EDIT_CHUNK_SIZE: usize = 128;

fn legacy_voxel_spacing_studs() -> f32 {
    super::LEGACY_TERRAIN_SAMPLE_SPACING_STUDS
}

#[derive(Clone, Debug)]
pub struct TerrainStorage {
    root: PathBuf,
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
#[derive(Clone, Debug, Deserialize)]
struct StoredTerrainMetadataV1 {
    version: u32,
    config: TerrainConfig,
    result: Option<StoredHeightfieldResultV1>,
}

#[derive(Clone, Debug, Deserialize)]
struct StoredHeightfieldResultV1 {
    request: TerrainGenerationRequest,
    generator: TerrainGeneratorType,
    generator_implementation_version: u32,
    chunks: Vec<[u32; 2]>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredTerrainMetadata {
    version: u32,
    config: TerrainConfig,
    result: Option<StoredTerrainResult>,
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    #[serde(default)]
    layer_edit_chunks: Vec<StoredLayerEditManifest>,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredLayerEditManifest {
    id: u64,
    chunks: u32,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredLayerEditChunk {
    id: u64,
    index: u32,
    edits: Vec<LayerBrushEdit>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "payload", rename_all = "snake_case")]
enum StoredTerrainResult {
    Heightfield {
        request: TerrainGenerationRequest,
        generator: TerrainGeneratorType,
        generator_implementation_version: u32,
        chunks: Vec<[u32; 2]>,
    },
    Voxel {
        request: TerrainGenerationRequest,
        generator: TerrainGeneratorType,
        generator_implementation_version: u32,
        origin: [i32; 3],
        size: [u32; 3],
        #[serde(default = "legacy_voxel_spacing_studs")]
        spacing_studs: f32,
        materials: Vec<String>,
        chunks: Vec<[u32; 3]>,
    },
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredHeightfieldChunk {
    version: u32,
    offset: [u32; 2],
    size: [u32; 2],
    heights: Vec<f32>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
struct StoredVoxelChunk {
    version: u32,
    offset: [u32; 3],
    size: [u32; 3],
    materials: Vec<u16>,
    occupancies: Vec<u8>,
}

impl TerrainStorage {
    pub fn new(project_root: impl AsRef<Path>) -> Self {
        Self {
            root: project_root.as_ref().join("terrain"),
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
    pub fn save(&self, state: &TerrainState) -> io::Result<()> {
        if state.authored.is_none()
            && state.config == TerrainConfig::default()
            && !self.root.join(METADATA_FILE).exists()
        {
            return Ok(());
        }

        fs::create_dir_all(&self.root)?;
        let mut desired_chunks = HashSet::new();
        // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
        // Per-layer edits are stored in bounded segments. A new dab only
        // updates the final edit segment; older segments remain byte-identical.
        let mut layer_edit_chunks = Vec::new();
        for layer in &state.config.generation_layers {
            if layer.brush_edits.is_empty() {
                continue;
            }
            let count = layer.brush_edits.len().div_ceil(LAYER_EDIT_CHUNK_SIZE);
            if count > u32::MAX as usize {
                return Err(invalid_data("Too many layer brush edit chunks"));
            }
            for (index, edits) in layer.brush_edits.chunks(LAYER_EDIT_CHUNK_SIZE).enumerate() {
                for edit in edits {
                    edit.validate().map_err(invalid_data)?;
                }
                let path = self.layer_edit_chunk_path(layer.id, index as u32);
                write_json_if_changed(
                    &path,
                    &StoredLayerEditChunk {
                        id: layer.id,
                        index: index as u32,
                        edits: edits.to_vec(),
                    },
                )?;
                desired_chunks.insert(path);
            }
            layer_edit_chunks.push(StoredLayerEditManifest {
                id: layer.id,
                chunks: count as u32,
            });
        }

        let stored_result = match &state.authored {
            Some(result) => Some(match &result.data {
                TerrainData::Heightfield(heightfield) => {
                    let mut chunks = Vec::new();
                    for offset_z in (0..heightfield.size[2]).step_by(TERRAIN_CHUNK_SIZE as usize) {
                        for offset_x in
                            (0..heightfield.size[0]).step_by(TERRAIN_CHUNK_SIZE as usize)
                        {
                            let key =
                                [offset_x / TERRAIN_CHUNK_SIZE, offset_z / TERRAIN_CHUNK_SIZE];
                            let stored = StoredHeightfieldChunk::from_heightfield(
                                heightfield,
                                [offset_x, offset_z],
                                TERRAIN_STORAGE_VERSION,
                            );
                            let path = self.heightfield_chunk_path(key);
                            write_json_if_changed(&path, &stored)?;
                            desired_chunks.insert(path);
                            chunks.push(key);
                        }
                    }
                    StoredTerrainResult::Heightfield {
                        request: result.request,
                        generator: result.generator,
                        generator_implementation_version: result.generator_implementation_version,
                        chunks,
                    }
                }
                TerrainData::Voxel(grid) => {
                    grid.validate().map_err(|error| {
                        invalid_data(format!("Invalid smooth voxel Terrain payload: {error}"))
                    })?;
                    let mut chunks = Vec::new();
                    for offset_y in (0..grid.size[1]).step_by(TERRAIN_CHUNK_SIZE as usize) {
                        for offset_z in (0..grid.size[2]).step_by(TERRAIN_CHUNK_SIZE as usize) {
                            for offset_x in (0..grid.size[0]).step_by(TERRAIN_CHUNK_SIZE as usize) {
                                let key = [
                                    offset_x / TERRAIN_CHUNK_SIZE,
                                    offset_y / TERRAIN_CHUNK_SIZE,
                                    offset_z / TERRAIN_CHUNK_SIZE,
                                ];
                                let stored = StoredVoxelChunk::from_grid(
                                    grid,
                                    [offset_x, offset_y, offset_z],
                                );
                                let path = self.voxel_chunk_path(key);
                                write_json_if_changed(&path, &stored)?;
                                desired_chunks.insert(path);
                                chunks.push(key);
                            }
                        }
                    }
                    StoredTerrainResult::Voxel {
                        request: result.request,
                        generator: result.generator,
                        generator_implementation_version: result.generator_implementation_version,
                        origin: grid.origin,
                        size: grid.size,
                        spacing_studs: grid.spacing_studs,
                        materials: grid.materials.clone(),
                        chunks,
                    }
                }
            }),
            None => None,
        };

        // Commit metadata before deleting outdated chunks. Never remove a
        // chunk referenced by the previous metadata after a failed write.
        write_json_if_changed(
            &self.root.join(METADATA_FILE),
            &StoredTerrainMetadata {
                version: TERRAIN_STORAGE_VERSION,
                config: state.config.clone(),
                result: stored_result,
                layer_edit_chunks,
            },
        )?;

        for entry in fs::read_dir(&self.root)? {
            let path = entry?.path();
            let extension = path.extension().and_then(|value| value.to_str());
            if (extension == Some(CHUNK_EXTENSION) || extension == Some(LAYER_EDIT_CHUNK_EXTENSION))
                && !desired_chunks.contains(&path)
            {
                fs::remove_file(path)?;
            }
        }
        Ok(())
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
    pub fn load(&self) -> io::Result<Option<TerrainState>> {
        let metadata_path = self.root.join(METADATA_FILE);
        if !metadata_path.is_file() {
            return Ok(None);
        }

        let value = read_json_value(&metadata_path)?;
        let version = value
            .get("version")
            .and_then(serde_json::Value::as_u64)
            .ok_or_else(|| invalid_data("Terrain metadata is missing a numeric storage version"))?;

        if version == LEGACY_TERRAIN_STORAGE_VERSION as u64 {
            let metadata: StoredTerrainMetadataV1 =
                serde_json::from_value(value).map_err(|error| {
                    invalid_data(format!(
                        "Failed to decode {}: {error}",
                        metadata_path.display()
                    ))
                })?;
            self.load_v1(metadata)
        } else if version == TERRAIN_STORAGE_VERSION as u64 {
            let metadata: StoredTerrainMetadata =
                serde_json::from_value(value).map_err(|error| {
                    invalid_data(format!(
                        "Failed to decode {}: {error}",
                        metadata_path.display()
                    ))
                })?;
            self.load_v2(metadata)
        } else {
            Err(invalid_data(format!(
                "Unsupported Terrain storage version {version}; expected {LEGACY_TERRAIN_STORAGE_VERSION} or {TERRAIN_STORAGE_VERSION}"
            )))
        }
    }

    fn load_v1(&self, metadata: StoredTerrainMetadataV1) -> io::Result<Option<TerrainState>> {
        debug_assert_eq!(metadata.version, LEGACY_TERRAIN_STORAGE_VERSION);
        let authored = match metadata.result {
            Some(result) => Some(self.load_heightfield_result(
                result.request,
                result.generator,
                result.generator_implementation_version,
                &result.chunks,
                LEGACY_TERRAIN_STORAGE_VERSION,
            )?),
            None => None,
        };
        // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
        Ok(Some(TerrainState::from_persisted(
            metadata.config,
            authored,
        )))
    }

    fn load_v2(&self, mut metadata: StoredTerrainMetadata) -> io::Result<Option<TerrainState>> {
        // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
        // Read each layer edit chunk separately before constructing the
        // effective composite; older v2 files omit this manifest entirely.
        let mut loaded_layer_ids = HashSet::new();
        for entry in &metadata.layer_edit_chunks {
            if !loaded_layer_ids.insert(entry.id) {
                return Err(invalid_data("Duplicate Terrain layer edit manifest"));
            }
            let layer = metadata
                .config
                .generation_layers
                .iter_mut()
                .find(|l| l.id == entry.id)
                .ok_or_else(|| invalid_data("Terrain edit chunk references unknown layer"))?;
            if entry.chunks > 1_000_000 {
                return Err(invalid_data("Too many Terrain edit chunks"));
            }
            for index in 0..entry.chunks {
                let chunk: StoredLayerEditChunk =
                    read_json(&self.layer_edit_chunk_path(entry.id, index))?;
                if chunk.id != entry.id
                    || chunk.index != index
                    || chunk.edits.is_empty()
                    || chunk.edits.len() > LAYER_EDIT_CHUNK_SIZE
                {
                    return Err(invalid_data("Invalid Terrain layer edit chunk"));
                }
                for edit in &chunk.edits {
                    edit.validate().map_err(invalid_data)?;
                }
                layer.brush_edits.extend(chunk.edits);
            }
        }

        debug_assert_eq!(metadata.version, TERRAIN_STORAGE_VERSION);
        let authored = match metadata.result {
            Some(StoredTerrainResult::Heightfield {
                request,
                generator,
                generator_implementation_version,
                chunks,
            }) => Some(self.load_heightfield_result(
                request,
                generator,
                generator_implementation_version,
                &chunks,
                TERRAIN_STORAGE_VERSION,
            )?),
            Some(StoredTerrainResult::Voxel {
                request,
                generator,
                generator_implementation_version,
                origin,
                size,
                spacing_studs,
                materials,
                chunks,
            }) => {
                // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
                validate_stored_voxel_request(request)?;
                let empty = SmoothVoxelGrid::empty_with_spacing(
                    origin,
                    size,
                    spacing_studs,
                    materials.clone(),
                )
                .map_err(|error| {
                    invalid_data(format!("Invalid stored smooth voxel Terrain: {error}"))
                })?;
                // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
                // Validate each expected key exactly once, then stream individual
                // files into the final dense allocation. No additional full-volume
                // coverage bitmap is needed (previously 1 byte per voxel).
                let dimensions = size.map(|n| n.div_ceil(TERRAIN_CHUNK_SIZE));
                let expected = dimensions
                    .iter()
                    .try_fold(1_u64, |n, axis| n.checked_mul(*axis as u64))
                    .ok_or_else(|| invalid_data("Stored Terrain chunk count overflow"))?;
                if chunks.len() as u64 != expected {
                    return Err(invalid_data(
                        "Smooth voxel Terrain chunk coverage is incomplete",
                    ));
                }
                let mut seen = HashSet::with_capacity(chunks.len());
                let mut voxels = empty.voxels;
                for key in &chunks {
                    if key.iter().zip(dimensions).any(|(k, max)| *k >= max) || !seen.insert(*key) {
                        return Err(invalid_data(
                            "Unexpected or duplicate smooth voxel Terrain chunk",
                        ));
                    }
                    let chunk: StoredVoxelChunk = read_json(&self.voxel_chunk_path(*key))?;
                    let offset = key.map(|v| v * TERRAIN_CHUNK_SIZE);
                    let expected_size =
                        [0, 1, 2].map(|axis| TERRAIN_CHUNK_SIZE.min(size[axis] - offset[axis]));
                    if chunk.offset != offset || chunk.size != expected_size {
                        return Err(invalid_data(
                            "Smooth voxel Terrain chunk is stored at wrong offset/size",
                        ));
                    }
                    chunk.copy_into(size, &mut voxels)?;
                }
                let grid = SmoothVoxelGrid::from_dense_with_spacing(
                    origin,
                    size,
                    spacing_studs,
                    materials,
                    voxels,
                )
                .map_err(|error| {
                    invalid_data(format!("Invalid stored smooth voxel Terrain: {error}"))
                })?;
                Some(TerrainResult {
                    request,
                    generator,
                    generator_implementation_version,
                    data: TerrainData::Voxel(grid),
                })
            }
            None => None,
        };

        // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
        Ok(Some(TerrainState::from_persisted(
            metadata.config,
            authored,
        )))
    }

    fn load_heightfield_result(
        &self,
        request: TerrainGenerationRequest,
        generator: TerrainGeneratorType,
        generator_implementation_version: u32,
        chunks: &[[u32; 2]],
        expected_chunk_version: u32,
    ) -> io::Result<TerrainResult> {
        request
            .validate()
            .map_err(|error| invalid_data(format!("Invalid stored Terrain request: {error}")))?;
        let width = request.size[0] as u32;
        let depth = request.size[2] as u32;
        let mut heights = vec![f32::NAN; width as usize * depth as usize];
        for key in chunks {
            let chunk: StoredHeightfieldChunk = read_json(&self.heightfield_chunk_path(*key))?;
            chunk.copy_into(width, depth, &mut heights, expected_chunk_version)?;
        }
        if heights.iter().any(|height| !height.is_finite()) {
            return Err(invalid_data("Terrain chunk coverage is incomplete"));
        }

        Ok(TerrainResult {
            request,
            generator,
            generator_implementation_version,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: request.position,
                size: request.size.map(|value| value as u32),
                heights,
            }),
        })
    }

    fn heightfield_chunk_path(&self, key: [u32; 2]) -> PathBuf {
        self.root
            .join(format!("{}_{}.{}", key[0], key[1], CHUNK_EXTENSION))
    }

    fn layer_edit_chunk_path(&self, id: u64, index: u32) -> PathBuf {
        self.root
            .join(format!("layer_{id}_{index}.{LAYER_EDIT_CHUNK_EXTENSION}"))
    }

    fn voxel_chunk_path(&self, key: [u32; 3]) -> PathBuf {
        self.root.join(format!(
            "v_{}_{}_{}.{}",
            key[0], key[1], key[2], CHUNK_EXTENSION
        ))
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
impl StoredHeightfieldChunk {
    fn from_heightfield(heightfield: &HeightfieldData, offset: [u32; 2], version: u32) -> Self {
        let width = TERRAIN_CHUNK_SIZE.min(heightfield.size[0] - offset[0]);
        let depth = TERRAIN_CHUNK_SIZE.min(heightfield.size[2] - offset[1]);
        let mut heights = Vec::with_capacity(width as usize * depth as usize);
        for z in 0..depth {
            for x in 0..width {
                heights.push(heightfield.height(offset[0] + x, offset[1] + z).unwrap());
            }
        }
        Self {
            version,
            offset,
            size: [width, depth],
            heights,
        }
    }

    fn copy_into(
        &self,
        width: u32,
        depth: u32,
        target: &mut [f32],
        expected_version: u32,
    ) -> io::Result<()> {
        if self.version != expected_version
            || self.offset[0].saturating_add(self.size[0]) > width
            || self.offset[1].saturating_add(self.size[1]) > depth
            || self.heights.len() != self.size[0] as usize * self.size[1] as usize
        {
            return Err(invalid_data("Invalid Terrain chunk dimensions"));
        }
        for z in 0..self.size[1] {
            let source_start = z as usize * self.size[0] as usize;
            let target_start =
                (self.offset[1] + z) as usize * width as usize + self.offset[0] as usize;
            target[target_start..target_start + self.size[0] as usize]
                .copy_from_slice(&self.heights[source_start..source_start + self.size[0] as usize]);
        }
        Ok(())
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
impl StoredVoxelChunk {
    fn from_grid(grid: &SmoothVoxelGrid, offset: [u32; 3]) -> Self {
        let size = [
            TERRAIN_CHUNK_SIZE.min(grid.size[0] - offset[0]),
            TERRAIN_CHUNK_SIZE.min(grid.size[1] - offset[1]),
            TERRAIN_CHUNK_SIZE.min(grid.size[2] - offset[2]),
        ];
        let volume = size[0] as usize * size[1] as usize * size[2] as usize;
        let mut materials = Vec::with_capacity(volume);
        let mut occupancies = Vec::with_capacity(volume);
        for y in 0..size[1] {
            for z in 0..size[2] {
                for x in 0..size[0] {
                    let voxel = grid
                        .voxel(offset[0] + x, offset[1] + y, offset[2] + z)
                        .expect("validated voxel chunk bounds must resolve");
                    materials.push(voxel.material);
                    occupancies.push(voxel.occupancy);
                }
            }
        }
        Self {
            version: TERRAIN_STORAGE_VERSION,
            offset,
            size,
            materials,
            occupancies,
        }
    }

    fn copy_into(&self, grid_size: [u32; 3], target: &mut [SmoothVoxel]) -> io::Result<()> {
        let volume = self.size[0] as usize * self.size[1] as usize * self.size[2] as usize;
        if self.version != TERRAIN_STORAGE_VERSION
            || self.offset[0].saturating_add(self.size[0]) > grid_size[0]
            || self.offset[1].saturating_add(self.size[1]) > grid_size[1]
            || self.offset[2].saturating_add(self.size[2]) > grid_size[2]
            || self.materials.len() != volume
            || self.occupancies.len() != volume
        {
            return Err(invalid_data(
                "Invalid smooth voxel Terrain chunk dimensions",
            ));
        }

        for y in 0..self.size[1] {
            for z in 0..self.size[2] {
                for x in 0..self.size[0] {
                    let source = ((y as usize * self.size[2] as usize + z as usize)
                        * self.size[0] as usize)
                        + x as usize;
                    let target_index = (((self.offset[1] + y) as usize * grid_size[2] as usize
                        + (self.offset[2] + z) as usize)
                        * grid_size[0] as usize)
                        + (self.offset[0] + x) as usize;
                    target[target_index] = SmoothVoxel {
                        material: self.materials[source],
                        occupancy: self.occupancies[source],
                    };
                }
            }
        }
        Ok(())
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
fn validate_stored_voxel_request(request: TerrainGenerationRequest) -> io::Result<()> {
    let mut volume = 1_u64;
    for (axis, size) in ["x", "y", "z"].into_iter().zip(request.size) {
        if size <= 0 {
            return Err(invalid_data(format!(
                "Invalid stored Terrain request: {axis} size {size} must be positive"
            )));
        }
        volume = volume
            .checked_mul(size as u64)
            .ok_or_else(|| invalid_data("Invalid stored Terrain request: volume overflow"))?;
    }
    if volume > MAX_STORED_VOXEL_VOLUME {
        return Err(invalid_data(format!(
            "Invalid stored Terrain request: volume {volume} exceeds the authored voxel limit of {MAX_STORED_VOXEL_VOLUME}"
        )));
    }
    request
        .inclusive_max()
        .map_err(|error| invalid_data(format!("Invalid stored Terrain request: {error}")))?;
    Ok(())
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path) -> io::Result<T> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid_data(format!("Failed to decode {}: {error}", path.display())))
}

fn read_json_value(path: &Path) -> io::Result<serde_json::Value> {
    let bytes = fs::read(path)?;
    serde_json::from_slice(&bytes)
        .map_err(|error| invalid_data(format!("Failed to decode {}: {error}", path.display())))
}

// Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
// Both heightfield and voxel chunks already have bounded payloads. Compare
// individual chunk bytes to skip rewrites, rather than touching every chunk
// on every project save. Existing v1/v2 projects remain load-compatible.
fn write_json_if_changed<T: Serialize>(path: &Path, value: &T) -> io::Result<()> {
    let bytes = serde_json::to_vec(value)
        .map_err(|error| invalid_data(format!("Failed to encode {}: {error}", path.display())))?;
    if path.exists() {
        let old = fs::read(path)?;
        if old == bytes {
            return Ok(());
        }
    }
    write_bytes_atomically(path, &bytes)
}
fn write_bytes_atomically(path: &Path, bytes: &[u8]) -> io::Result<()> {
    let temporary = path.with_extension(format!("{CHUNK_EXTENSION}.tmp"));
    fs::write(&temporary, bytes)?;
    if !path.exists() {
        return fs::rename(temporary, path);
    }
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    // Preserve the previous chunk while replacing on Windows. A failed rename
    // is rolled back instead of deleting the only valid saved copy.
    let backup = path.with_extension(format!("{CHUNK_EXTENSION}.bak"));
    if backup.exists() {
        return Err(invalid_data(format!(
            "Terrain recovery backup already exists; resolve before saving: {}",
            backup.display()
        )));
    }
    fs::rename(path, &backup)?;
    match fs::rename(&temporary, path) {
        Ok(()) => fs::remove_file(backup),
        Err(error) => {
            let _ = fs::rename(backup, path);
            Err(error)
        }
    }
}
fn invalid_data(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

// Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
#[cfg(test)]
mod incremental_tests {
    use super::*;

    #[test]
    fn unchanged_voxel_chunks_are_not_rewritten_on_save() {
        let dir = std::env::temp_dir().join(format!(
            "worldkiln_wo56_incremental_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = TerrainStorage::new(&dir);
        let mut state = TerrainState::default();
        let request = TerrainGenerationRequest {
            position: [0, 0, 0],
            // 2-stud samples, 32-sample chunk: cover more than one chunk.
            size: [96, 64, 64],
        };
        state.generate_authored(request).unwrap();
        storage.save(&state).unwrap();
        let unchanged = storage.root.join("v_0_0_0.terrainchunk");
        let changed = storage.root.join("v_1_0_0.terrainchunk");
        let old_unchanged = std::fs::read(&unchanged).unwrap();
        let old_modified = std::fs::metadata(&unchanged).unwrap().modified().unwrap();
        let old_changed = std::fs::read(&changed).unwrap();

        let TerrainData::Voxel(ref mut grid) = state.authored.as_mut().unwrap().data else {
            panic!("generated smooth terrain should use voxels");
        };
        let original = grid.voxel(TERRAIN_CHUNK_SIZE + 1, 0, 0).unwrap();
        grid.set_voxel(
            TERRAIN_CHUNK_SIZE + 1,
            0,
            0,
            SmoothVoxel {
                material: original.material,
                occupancy: if original.occupancy == 0 { 255 } else { 0 },
            },
        );
        storage.save(&state).unwrap();
        assert_eq!(std::fs::read(&unchanged).unwrap(), old_unchanged);
        assert_eq!(
            std::fs::metadata(&unchanged).unwrap().modified().unwrap(),
            old_modified,
            "Unchanged voxel chunk timestamp must remain intact"
        );
        assert_ne!(std::fs::read(&changed).unwrap(), old_changed);
        let loaded = storage.load().unwrap().unwrap();
        assert_eq!(loaded.authored, state.authored);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn missing_and_duplicate_voxel_chunks_fail_without_partial_world_load() {
        let dir = std::env::temp_dir().join(format!(
            "worldkiln_wo56_invalid_chunks_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let storage = TerrainStorage::new(&dir);
        let mut state = TerrainState::default();
        state
            .generate_authored(TerrainGenerationRequest {
                position: [0, 0, 0],
                // 2-stud samples, 32-sample chunk: require two X chunks.
                size: [96, 64, 64],
            })
            .unwrap();
        storage.save(&state).unwrap();
        let metadata = storage.root.join(METADATA_FILE);
        let original = std::fs::read(&metadata).unwrap();
        let mut json: serde_json::Value = serde_json::from_slice(&original).unwrap();
        let chunks = json["result"]["chunks"].as_array_mut().unwrap();
        assert!(chunks.len() > 1);
        chunks[1] = chunks[0].clone();
        std::fs::write(&metadata, serde_json::to_vec(&json).unwrap()).unwrap();
        assert!(
            storage.load().is_err(),
            "Duplicate chunk manifest must be rejected"
        );
        std::fs::write(&metadata, original).unwrap();
        std::fs::remove_file(storage.voxel_chunk_path([0, 0, 0])).unwrap();
        assert!(
            storage.load().is_err(),
            "Missing referenced chunk must be rejected"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }
}
