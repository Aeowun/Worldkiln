use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::ChunkCoord;
use super::actor::Actor;
use super::cell::{AttributeValue, Cell, CellType};
use super::coordinate::WorldCoord;

const CHUNK_FILE_VERSION: u32 = 1;
const CHUNK_EXTENSION: &str = "chunk";

#[derive(Clone, Debug)]
pub struct WorldStorage {
    root: PathBuf,
}

impl WorldStorage {
    pub fn new(project_root: impl AsRef<Path>) -> io::Result<Self> {
        let root = project_root.as_ref().join("chunks");
        fs::create_dir_all(&root)?;
        Ok(Self { root })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn chunk_path(&self, chunk_coord: ChunkCoord) -> PathBuf {
        self.root.join(format!(
            "{}_{}_{}.{}",
            chunk_coord.x, chunk_coord.y, chunk_coord.z, CHUNK_EXTENSION
        ))
    }

    pub fn list_chunks(&self) -> io::Result<Vec<ChunkCoord>> {
        let mut chunks = Vec::new();

        if !self.root.exists() {
            return Ok(chunks);
        }

        for entry in fs::read_dir(&self.root)? {
            let entry = entry?;
            let path = entry.path();

            if !path.is_file() {
                continue;
            }

            if path.extension().and_then(|value| value.to_str()) != Some(CHUNK_EXTENSION) {
                continue;
            }

            let Some(stem) = path.file_stem().and_then(|value| value.to_str()) else {
                continue;
            };

            let parts: Vec<&str> = stem.split('_').collect();

            if parts.len() != 3 {
                continue;
            }

            let Ok(x) = parts[0].parse::<i32>() else {
                continue;
            };

            let Ok(y) = parts[1].parse::<i32>() else {
                continue;
            };

            let Ok(z) = parts[2].parse::<i32>() else {
                continue;
            };

            chunks.push(ChunkCoord::new(x, y, z));
        }

        chunks.sort_unstable();
        Ok(chunks)
    }

    pub fn chunk_exists(&self, chunk_coord: ChunkCoord) -> bool {
        self.chunk_path(chunk_coord).is_file()
    }

    pub fn load_chunk(&self, chunk_coord: ChunkCoord) -> io::Result<Option<StoredChunk>> {
        let path = self.chunk_path(chunk_coord);

        if !path.exists() {
            return Ok(None);
        }

        let bytes = fs::read(&path)?;

        let chunk: StoredChunk = serde_json::from_slice(&bytes).map_err(|error| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Failed to decode chunk ({}, {}, {}): {}",
                    chunk_coord.x, chunk_coord.y, chunk_coord.z, error
                ),
            )
        })?;

        if chunk.version != CHUNK_FILE_VERSION {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Unsupported chunk version {} for ({}, {}, {}); expected {}",
                    chunk.version, chunk_coord.x, chunk_coord.y, chunk_coord.z, CHUNK_FILE_VERSION
                ),
            ));
        }

        if chunk.coord != chunk_coord {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Chunk coordinate mismatch: requested ({}, {}, {}), file contains ({}, {}, {})",
                    chunk_coord.x,
                    chunk_coord.y,
                    chunk_coord.z,
                    chunk.coord.x,
                    chunk.coord.y,
                    chunk.coord.z
                ),
            ));
        }

        for cell in &chunk.cells {
            if cell.cell_type == StoredCellType::ParticleEmitter {
                if let Some(settings) = &cell.particles {
                    settings.validate().map_err(|error| {
                        io::Error::new(
                            io::ErrorKind::InvalidData,
                            format!(
                                "ParticleEmitter Cell {} in chunk {:?}: {error}",
                                cell.id, chunk_coord
                            ),
                        )
                    })?;
                }
            }
        }
        Ok(Some(chunk))
    }

    pub fn save_chunk(&self, chunk: &StoredChunk) -> io::Result<()> {
        fs::create_dir_all(&self.root)?;

        let path = self.chunk_path(chunk.coord);

        let bytes = serde_json::to_vec(chunk).map_err(|error| {
            io::Error::new(
                io::ErrorKind::Other,
                format!(
                    "Failed to encode chunk ({}, {}, {}): {}",
                    chunk.coord.x, chunk.coord.y, chunk.coord.z, error
                ),
            )
        })?;

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
        crate::project::atomic_file::write_atomically(&path, &bytes)?;

        Ok(())
    }

    pub fn delete_chunk(&self, chunk_coord: ChunkCoord) -> io::Result<()> {
        let path = self.chunk_path(chunk_coord);

        if path.exists() {
            fs::remove_file(path)?;
        }

        Ok(())
    }

    pub fn create_stored_chunk(
        chunk_coord: ChunkCoord,
        cells: impl IntoIterator<Item = (WorldCoord, Cell)>,
    ) -> io::Result<StoredChunk> {
        let mut stored_cells = Vec::new();

        for (coord, cell) in cells {
            let expected_chunk = ChunkCoord::from_world_coord(coord);

            if expected_chunk != chunk_coord {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "Cell ({}, {}, {}) does not belong to chunk ({}, {}, {})",
                        coord.x, coord.y, coord.z, chunk_coord.x, chunk_coord.y, chunk_coord.z
                    ),
                ));
            }

            stored_cells.push(StoredCell::from_cell(coord, &cell));
        }

        Ok(StoredChunk {
            version: CHUNK_FILE_VERSION,
            coord: chunk_coord,
            cells: stored_cells,
        })
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredChunk {
    pub version: u32,
    pub coord: ChunkCoord,
    pub cells: Vec<StoredCell>,
}

impl StoredChunk {
    pub fn new(coord: ChunkCoord) -> Self {
        Self {
            version: CHUNK_FILE_VERSION,
            coord,
            cells: Vec::new(),
        }
    }

    /// Returns current Cells only.
    ///
    /// Legacy Actor records are intentionally excluded. Use
    /// `into_cells_and_legacy_actors()` during world loading so old Actor
    /// Cells can be migrated into World.actors.
    pub fn into_cells(self) -> Vec<(WorldCoord, Cell)> {
        self.cells
            .into_iter()
            .filter_map(|cell| {
                if cell.cell_type.is_legacy_actor() {
                    None
                } else {
                    Some(cell.into_cell(self.coord))
                }
            })
            .collect()
    }

    /// Splits current Cell records from legacy Actor-as-Cell records.
    pub fn into_cells_and_legacy_actors(self) -> (Vec<(WorldCoord, Cell)>, Vec<Actor>) {
        let mut cells = Vec::new();
        let mut actors = Vec::new();

        for stored_cell in self.cells {
            if stored_cell.cell_type.is_legacy_actor() {
                actors.push(stored_cell.into_legacy_actor(self.coord));
            } else {
                cells.push(stored_cell.into_cell(self.coord));
            }
        }

        (cells, actors)
    }
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredCell {
    pub local_x: i32,
    pub local_y: i32,
    pub local_z: i32,

    pub id: u64,
    pub cell_type: StoredCellType,

    pub visible: bool,
    pub solid: bool,
    pub anchored: bool,

    pub texture: String,
    pub color_rgb: [f32; 3],

    // Missing in older chunks; defaults preserve their load contract.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub particles: Option<crate::engine::particles::EmitterSettings>,

    pub audio: String,
    pub playing: bool,
    pub looped: bool,
    #[serde(default)]
    pub loop_interval: f32,
    pub volume: f32,

    pub collision_events_enabled: bool,

    pub light_color: [f32; 3],
    pub light_intensity: f32,
    pub light_range: f32,
    pub light_shadows: bool,
    pub light_enabled: bool,

    pub entity_identity: Option<String>,

    /// Legacy Actor package data.
    ///
    /// This exists only so old Actor-as-Cell chunk files can be migrated.
    /// New Cell data never writes this field.
    #[serde(default)]
    pub actor_package: Option<String>,

    pub attributes: BTreeMap<String, AttributeValue>,
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredSource {
    pub position: [f32; 3],
    pub cell: StoredCell,
}

impl StoredSource {
    pub fn from_source(source: &crate::world::AuthoredSource) -> Self {
        Self {
            position: [source.position.x, source.position.y, source.position.z],
            cell: StoredCell::from_cell(WorldCoord::new(0, 0, 0), &source.cell),
        }
    }

    pub fn into_source(self) -> crate::world::AuthoredSource {
        let (_, cell) = self.cell.into_cell(ChunkCoord::new(0, 0, 0));
        crate::world::AuthoredSource {
            position: Vec3::from_array(self.position),
            cell,
        }
    }
}

impl StoredCell {
    pub fn from_cell(coord: WorldCoord, cell: &Cell) -> Self {
        Self {
            local_x: coord.x.rem_euclid(super::CHUNK_SIZE),
            local_y: coord.y.rem_euclid(super::CHUNK_SIZE),
            local_z: coord.z.rem_euclid(super::CHUNK_SIZE),

            id: cell.id,
            cell_type: StoredCellType::from(cell.cell_type),

            visible: cell.visible,
            solid: cell.solid,
            anchored: cell.anchored,

            texture: cell.texture.clone(),
            color_rgb: [cell.color_rgb.x, cell.color_rgb.y, cell.color_rgb.z],

            particles: (cell.cell_type == CellType::ParticleEmitter)
                .then(|| cell.particles.clone()),
            audio: cell.audio.clone(),
            playing: cell.playing,
            looped: cell.looped,
            loop_interval: cell.loop_interval,
            volume: cell.volume,

            collision_events_enabled: cell.collision_events_enabled,

            light_color: [cell.light_color.x, cell.light_color.y, cell.light_color.z],
            light_intensity: cell.light_intensity,
            light_range: cell.light_range,
            light_shadows: cell.light_shadows,
            light_enabled: cell.light_enabled,

            entity_identity: cell.entity_identity.clone(),
            actor_package: None,

            attributes: cell.attributes.clone(),
        }
    }

    pub fn into_cell(self, chunk_coord: ChunkCoord) -> (WorldCoord, Cell) {
        let coord = WorldCoord::new(
            chunk_coord.x * super::CHUNK_SIZE + self.local_x,
            chunk_coord.y * super::CHUNK_SIZE + self.local_y,
            chunk_coord.z * super::CHUNK_SIZE + self.local_z,
        );

        let cell_type = match self.cell_type {
            StoredCellType::Block => CellType::Block,
            StoredCellType::FxBlock => CellType::FxBlock,
            StoredCellType::Player => CellType::Player,
            StoredCellType::Light => CellType::Light,
            StoredCellType::SpawnPoint => CellType::SpawnPoint,
            StoredCellType::AudioEmitter => CellType::AudioEmitter,
            StoredCellType::ParticleEmitter => CellType::ParticleEmitter,
            StoredCellType::LegacyActor => {
                panic!("Legacy Actor storage must be migrated before into_cell()")
            }
        };

        let cell = Cell {
            id: self.id,
            cell_type,

            visible: self.visible,
            solid: self.solid,
            anchored: self.anchored,

            texture: self.texture,
            color_rgb: Vec3::new(self.color_rgb[0], self.color_rgb[1], self.color_rgb[2]),

            particles: self.particles.unwrap_or_default(),
            audio: self.audio,
            playing: self.playing,
            looped: self.looped,
            loop_interval: if self.loop_interval.is_finite() {
                self.loop_interval.max(0.0)
            } else {
                0.0
            },
            volume: self.volume,

            collision_events_enabled: self.collision_events_enabled,

            light_color: Vec3::new(
                self.light_color[0],
                self.light_color[1],
                self.light_color[2],
            ),
            light_intensity: self.light_intensity,
            light_range: self.light_range,
            light_shadows: self.light_shadows,
            light_enabled: self.light_enabled,

            entity_identity: self.entity_identity,
            attributes: self.attributes,
        };

        (coord, cell)
    }

    pub fn into_legacy_actor(self, chunk_coord: ChunkCoord) -> Actor {
        let coord = WorldCoord::new(
            chunk_coord.x * super::CHUNK_SIZE + self.local_x,
            chunk_coord.y * super::CHUNK_SIZE + self.local_y,
            chunk_coord.z * super::CHUNK_SIZE + self.local_z,
        );

        let identity = self
            .entity_identity
            .filter(|value| !value.trim().is_empty())
            .unwrap_or_else(|| format!("Actor_{}", self.id));

        let mut actor = Actor::new(
            self.id,
            identity,
            Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32),
        );

        actor.visible = self.visible;
        actor.enabled = true;
        actor.package = self.actor_package;

        actor.physics.enabled = true;
        actor.physics.solid = self.solid;
        actor.physics.anchored = self.anchored;
        actor.physics.collision_enabled = self.collision_events_enabled;
        actor.physics.mass = 1.0;

        actor.attributes = self.attributes;

        actor
    }
}

fn default_actor_collision_size() -> [f32; 3] {
    [1.0, 1.0, 1.0]
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct StoredActor {
    pub id: u64,
    pub identity: String,
    pub position: [f32; 3],
    pub rotation: [f32; 3],
    pub scale: [f32; 3],
    pub visible: bool,
    pub enabled: bool,
    pub package: Option<String>,
    pub physics_enabled: bool,
    pub physics_solid: bool,
    pub physics_anchored: bool,
    pub physics_collision_enabled: bool,
    pub physics_mass: f32,

    #[serde(default = "default_actor_collision_size")]
    pub physics_collision_size: [f32; 3],

    #[serde(default)]
    pub physics_collision_offset: [f32; 3],

    pub attributes: BTreeMap<String, AttributeValue>,
}

impl StoredActor {
    pub fn from_actor(actor: &Actor) -> Self {
        Self {
            id: actor.id,
            identity: actor.identity.clone(),
            position: [actor.position.x, actor.position.y, actor.position.z],
            rotation: [actor.rotation.x, actor.rotation.y, actor.rotation.z],
            scale: [actor.scale.x, actor.scale.y, actor.scale.z],
            visible: actor.visible,
            enabled: actor.enabled,
            package: actor.package.clone(),
            physics_enabled: actor.physics.enabled,
            physics_solid: actor.physics.solid,
            physics_anchored: actor.physics.anchored,
            physics_collision_enabled: actor.physics.collision_enabled,
            physics_mass: actor.physics.mass,
            physics_collision_size: [
                actor.physics.collision_size.x,
                actor.physics.collision_size.y,
                actor.physics.collision_size.z,
            ],
            physics_collision_offset: [
                actor.physics.collision_offset.x,
                actor.physics.collision_offset.y,
                actor.physics.collision_offset.z,
            ],
            attributes: actor.attributes.clone(),
        }
    }

    pub fn into_actor(self) -> Actor {
        Actor {
            id: self.id,
            identity: self.identity,
            position: Vec3::new(self.position[0], self.position[1], self.position[2]),
            rotation: Vec3::new(self.rotation[0], self.rotation[1], self.rotation[2]),
            scale: Vec3::new(self.scale[0], self.scale[1], self.scale[2]),
            visible: self.visible,
            enabled: self.enabled,
            package: self.package,
            physics: super::actor::ActorPhysicsConfig {
                enabled: self.physics_enabled,
                solid: self.physics_solid,
                anchored: self.physics_anchored,
                collision_enabled: self.physics_collision_enabled,
                mass: self.physics_mass,
                collision_size: Vec3::new(
                    self.physics_collision_size[0],
                    self.physics_collision_size[1],
                    self.physics_collision_size[2],
                ),
                collision_offset: Vec3::new(
                    self.physics_collision_offset[0],
                    self.physics_collision_offset[1],
                    self.physics_collision_offset[2],
                ),
            },
            attributes: self.attributes,
        }
    }
}

/// Stable serialized representation of CellType.
///
/// `LegacyActor` exists only for backwards-compatible decoding of old chunk
/// files where Actors were stored as Cells. New chunks never write it.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub enum StoredCellType {
    Block,
    FxBlock,
    Player,
    #[serde(rename = "Actor")]
    LegacyActor,
    Light,
    SpawnPoint,
    AudioEmitter,
    ParticleEmitter,
}

impl StoredCellType {
    pub fn is_legacy_actor(&self) -> bool {
        matches!(self, Self::LegacyActor)
    }
}

impl From<CellType> for StoredCellType {
    fn from(value: CellType) -> Self {
        match value {
            CellType::Block => Self::Block,
            CellType::FxBlock => Self::FxBlock,
            CellType::Player => Self::Player,
            CellType::Light => Self::Light,
            CellType::SpawnPoint => Self::SpawnPoint,
            CellType::AudioEmitter => Self::AudioEmitter,
            CellType::ParticleEmitter => Self::ParticleEmitter,
            CellType::Empty => {
                panic!("Empty cells must not be written to chunk storage")
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn unique_test_directory() -> PathBuf {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();

        std::env::temp_dir().join(format!(
            "worldkiln_world_storage_test_{}_{}",
            std::process::id(),
            timestamp
        ))
    }

    #[test]
    fn stored_cell_round_trip_preserves_data() {
        let coord = WorldCoord::new(17, -3, 34);
        let chunk_coord = ChunkCoord::from_world_coord(coord);

        let mut cell = Cell::new_block();
        cell.id = 12345;
        cell.color_rgb = Vec3::new(0.1, 0.2, 0.3);
        cell.texture = "brick".to_string();
        cell.entity_identity = Some("TestEntity".to_string());
        cell.loop_interval = 2.5;

        cell.attributes
            .insert("health".to_string(), AttributeValue::Number(100.0));

        let stored = StoredCell::from_cell(coord, &cell);
        let (loaded_coord, loaded_cell) = stored.into_cell(chunk_coord);

        assert_eq!(coord, loaded_coord);
        assert_eq!(cell.id, loaded_cell.id);
        assert_eq!(cell.cell_type, loaded_cell.cell_type);
        assert_eq!(cell.visible, loaded_cell.visible);
        assert_eq!(cell.solid, loaded_cell.solid);
        assert_eq!(cell.anchored, loaded_cell.anchored);
        assert_eq!(cell.texture, loaded_cell.texture);
        assert_eq!(cell.color_rgb, loaded_cell.color_rgb);
        assert_eq!(cell.entity_identity, loaded_cell.entity_identity);
        assert_eq!(cell.loop_interval, loaded_cell.loop_interval);
        assert_eq!(cell.attributes, loaded_cell.attributes);
    }

    #[test]
    fn legacy_audio_emitter_without_loop_interval_defaults_to_zero() {
        let coord = WorldCoord::new(4, 5, 6);
        let mut cell = Cell::new_audio_emitter();
        cell.id = 88;
        cell.looped = true;
        cell.loop_interval = 3.0;

        let stored = StoredCell::from_cell(coord, &cell);
        let mut json = serde_json::to_value(stored).unwrap();
        json.as_object_mut().unwrap().remove("loop_interval");

        let legacy: StoredCell = serde_json::from_value(json).unwrap();
        assert_eq!(legacy.loop_interval, 0.0);

        let chunk_coord = ChunkCoord::from_world_coord(coord);
        let (_, loaded) = legacy.into_cell(chunk_coord);
        assert_eq!(loaded.loop_interval, 0.0);
    }

    #[test]
    fn legacy_actor_cell_migrates_to_actor() {
        let coord = WorldCoord::new(5, 5, 5);
        let chunk_coord = ChunkCoord::from_world_coord(coord);

        let mut stored = StoredCell {
            local_x: 5,
            local_y: 5,
            local_z: 5,
            id: 999,
            cell_type: StoredCellType::LegacyActor,
            visible: false,
            solid: false,
            anchored: true,
            texture: "None".to_string(),
            color_rgb: [0.5, 0.5, 0.5],
            particles: None,
            audio: String::new(),
            playing: false,
            looped: false,
            loop_interval: 0.0,
            volume: 1.0,
            collision_events_enabled: true,
            light_color: [1.0, 1.0, 1.0],
            light_intensity: 5.0,
            light_range: 10.0,
            light_shadows: true,
            light_enabled: true,
            entity_identity: Some("Guard01".to_string()),
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
            actor_package: Some("Rogue".to_string()),
            attributes: BTreeMap::new(),
        };

        stored.attributes.insert(
            "faction".to_string(),
            AttributeValue::String("guard".to_string()),
        );

        let actor = stored.into_legacy_actor(chunk_coord);

        assert_eq!(actor.id, 999);
        assert_eq!(actor.identity, "Guard01");
        assert_eq!(actor.position, Vec3::new(5.0, 5.0, 5.0));
        assert_eq!(actor.package.as_deref(), Some("Rogue"));
        assert!(!actor.physics.solid);
        assert!(actor.physics.anchored);
        assert_eq!(
            actor.attributes.get("faction"),
            Some(&AttributeValue::String("guard".to_string()))
        );
    }

    #[test]
    fn stored_actor_round_trip_preserves_data() {
        let actor = Actor {
            id: 42,
            identity: "Door".to_string(),
            position: Vec3::new(1.0, 2.0, 3.0),
            rotation: Vec3::new(0.1, 0.2, 0.3),
            scale: Vec3::new(2.0, 2.0, 2.0),
            visible: true,
            enabled: true,
            package: Some("custom_door".to_string()),
            physics: crate::world::actor::ActorPhysicsConfig {
                enabled: true,
                solid: false,
                anchored: false,
                collision_enabled: true,
                mass: 3.5,
                collision_size: Vec3::new(1.0, 1.0, 1.0),
                collision_offset: Vec3::ZERO,
            },
            attributes: {
                let mut map = BTreeMap::new();
                map.insert(
                    "state".to_string(),
                    AttributeValue::String("closed".to_string()),
                );
                map
            },
        };

        let stored = StoredActor::from_actor(&actor);
        let loaded = stored.into_actor();

        assert_eq!(actor, loaded);
    }

    #[test]
    fn stored_actor_round_trip_preserves_transform_and_collider() {
        let mut actor = Actor::new(101, "Buggy", Vec3::new(5.0, 0.0, -10.0));
        actor.rotation = Vec3::new(0.1, 0.2, 0.3);
        actor.scale = Vec3::new(2.0, 3.0, 4.0);
        actor.physics.collision_size = Vec3::new(3.0, 1.5, 5.0);
        actor.physics.collision_offset = Vec3::new(0.0, 0.75, -0.2);

        let stored = StoredActor::from_actor(&actor);
        let loaded = stored.into_actor();

        assert_eq!(actor, loaded);
        assert_eq!(loaded.rotation, Vec3::new(0.1, 0.2, 0.3));
        assert_eq!(loaded.scale, Vec3::new(2.0, 3.0, 4.0));
        assert_eq!(loaded.physics.collision_size, Vec3::new(3.0, 1.5, 5.0));
        assert_eq!(loaded.physics.collision_offset, Vec3::new(0.0, 0.75, -0.2));
    }

    #[test]
    fn storage_saves_and_loads_chunk() {
        let root = unique_test_directory();
        let storage = WorldStorage::new(&root).unwrap();

        let coord = WorldCoord::new(1, 2, 3);
        let chunk_coord = ChunkCoord::from_world_coord(coord);

        let mut cell = Cell::new_block();
        cell.id = 42;

        let chunk = WorldStorage::create_stored_chunk(chunk_coord, [(coord, cell)]).unwrap();

        storage.save_chunk(&chunk).unwrap();

        assert!(storage.chunk_exists(chunk_coord));

        let loaded = storage
            .load_chunk(chunk_coord)
            .unwrap()
            .expect("chunk should exist");

        assert_eq!(loaded.version, CHUNK_FILE_VERSION);
        assert_eq!(loaded.coord, chunk_coord);
        assert_eq!(loaded.cells.len(), 1);
        assert_eq!(loaded.cells[0].id, 42);

        storage.delete_chunk(chunk_coord).unwrap();

        assert!(!storage.chunk_exists(chunk_coord));

        let _ = fs::remove_dir_all(root);
    }

    #[test]
    fn missing_chunk_returns_none() {
        let root = unique_test_directory();
        let storage = WorldStorage::new(&root).unwrap();

        let coord = ChunkCoord::new(100, 200, -50);

        assert!(storage.load_chunk(coord).unwrap().is_none());

        let _ = fs::remove_dir_all(root);
    }
}
