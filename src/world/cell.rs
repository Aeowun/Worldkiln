// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use glam::Vec3;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

pub const DEFAULT_SURFACE_FRICTION: f32 = 1.8;
pub const DEFAULT_AUDIO_MIN_DISTANCE: f32 = 10.0;
pub const DEFAULT_AUDIO_MAX_DISTANCE: f32 = 100.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", content = "value")]
pub enum AttributeValue {
    Number(f64),
    Bool(bool),
    String(String),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Hash)]
pub enum CellType {
    #[default]
    Empty,
    Block,
    FxBlock,
    Player,
    Light,
    SpawnPoint,
    AudioEmitter,
    ParticleEmitter,
}

pub const CHUNK_SIZE: i32 = 16;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ChunkCoord {
    pub x: i32,
    pub y: i32,
    pub z: i32,
}

impl ChunkCoord {
    pub fn new(x: i32, y: i32, z: i32) -> Self {
        Self { x, y, z }
    }

    pub fn from_world_coord(coord: crate::world::WorldCoord) -> Self {
        Self {
            x: coord.x.div_euclid(CHUNK_SIZE),
            y: coord.y.div_euclid(CHUNK_SIZE),
            z: coord.z.div_euclid(CHUNK_SIZE),
        }
    }

    pub fn local_offset(coord: crate::world::WorldCoord) -> (i32, i32, i32) {
        (
            coord.x.rem_euclid(CHUNK_SIZE),
            coord.y.rem_euclid(CHUNK_SIZE),
            coord.z.rem_euclid(CHUNK_SIZE),
        )
    }

    pub fn world_origin(&self) -> Vec3 {
        Vec3::new(
            (self.x * CHUNK_SIZE) as f32,
            (self.y * CHUNK_SIZE) as f32,
            (self.z * CHUNK_SIZE) as f32,
        )
    }

    pub fn aabb_min_max(&self) -> (Vec3, Vec3) {
        let min = self.world_origin();
        let max = min + Vec3::splat(CHUNK_SIZE as f32);
        (min, max)
    }
}

impl CellType {
    pub fn is_entity(self) -> bool {
        matches!(self, Self::Player)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub id: u64,
    pub cell_type: CellType,
    pub tags: BTreeSet<String>,
    pub visible: bool,
    pub solid: bool,
    pub anchored: bool,
    pub texture: String,
    pub color_rgb: Vec3,

    pub particles: crate::engine::particles::EmitterSettings,

    pub audio: String,
    pub playing: bool,
    pub looped: bool,
    pub loop_interval: f32,
    pub volume: f32,
    pub audio_min_distance: f32,
    pub audio_max_distance: f32,

    pub collision_events_enabled: bool,

    pub light_color: Vec3,
    pub light_intensity: f32,
    pub light_range: f32,
    pub light_shadows: bool,
    pub light_enabled: bool,

    pub entity_identity: Option<String>,

    pub attributes: BTreeMap<String, AttributeValue>,
}

#[derive(Clone, Debug, Default)]
pub struct RuntimeCellState {
    pub source_position: Option<Vec3>,
    pub light_enabled: Option<bool>,
    pub light_color: Option<Vec3>,
    pub light_intensity: Option<f32>,
    pub light_range: Option<f32>,
    pub light_shadows: Option<bool>,
    pub visible: Option<bool>,
    pub color_rgb: Option<Vec3>,
    pub solid: Option<bool>,
    pub anchored: Option<bool>,
    pub collision_events_enabled: Option<bool>,
    pub visual_offset: Option<Vec3>,
    pub attribute_overrides: BTreeMap<String, AttributeValue>,
    pub is_deleted: bool,
    pub tag_overrides: Option<BTreeSet<String>>,

    pub audio_path: Option<String>,
    pub audio_playing: Option<bool>,
    pub audio_paused: Option<bool>,
    pub audio_looped: Option<bool>,
    pub audio_volume: Option<f32>,
    pub audio_pitch: Option<f32>,
    pub audio_play_serial: u64,
    pub audio_min_distance: Option<f32>,
    pub audio_max_distance: Option<f32>,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            id: 0,
            cell_type: CellType::Empty,
            tags: BTreeSet::new(),
            visible: true,
            solid: true,
            anchored: true,
            texture: "None".to_string(),
            color_rgb: Vec3::new(0.5, 0.5, 0.5),
            particles: crate::engine::particles::EmitterSettings::default(),
            audio: String::new(),
            playing: false,
            looped: false,
            loop_interval: 0.0,
            volume: 1.0,
            audio_min_distance: DEFAULT_AUDIO_MIN_DISTANCE,
            audio_max_distance: DEFAULT_AUDIO_MAX_DISTANCE,
            collision_events_enabled: true,
            light_color: Vec3::ONE,
            light_intensity: 5.0,
            light_range: 10.0,
            light_shadows: true,
            light_enabled: true,
            entity_identity: None,
            attributes: BTreeMap::new(),
        }
    }
}

impl Cell {
    pub fn friction(&self) -> f32 {
        match self.attributes.get("friction") {
            Some(AttributeValue::Number(value)) if value.is_finite() && *value >= 0.0 => {
                (*value as f32).min(100.0)
            }
            _ => DEFAULT_SURFACE_FRICTION,
        }
    }

    pub fn set_friction(&mut self, value: f32) -> Result<(), String> {
        if !value.is_finite() || !(0.0..=100.0).contains(&value) {
            return Err("Cell friction must be finite and between 0 and 100".into());
        }
        self.attributes
            .insert("friction".into(), AttributeValue::Number(value as f64));
        Ok(())
    }

    pub fn new_block() -> Self {
        Self {
            id: 0,
            cell_type: CellType::Block,
            visible: true,
            solid: true,
            anchored: true,
            collision_events_enabled: true,
            texture: "Block_tx".to_string(),
            ..Default::default()
        }
    }

    pub fn new_light() -> Self {
        Self {
            id: 0,
            cell_type: CellType::Light,
            tags: BTreeSet::new(),
            visible: false,
            solid: false,
            anchored: true,
            collision_events_enabled: true,
            texture: "Light_tx".to_string(),
            color_rgb: Vec3::new(1.0, 1.0, 0.0),
            light_color: Vec3::ONE,
            light_intensity: 5.0,
            light_range: 10.0,
            light_shadows: true,
            light_enabled: true,
            entity_identity: None,
            attributes: BTreeMap::new(),
            particles: crate::engine::particles::EmitterSettings::default(),
            audio: String::new(),
            playing: false,
            looped: false,
            loop_interval: 0.0,
            volume: 1.0,
            audio_min_distance: DEFAULT_AUDIO_MIN_DISTANCE,
            audio_max_distance: DEFAULT_AUDIO_MAX_DISTANCE,
        }
    }

    pub fn new_audio_emitter() -> Self {
        Self {
            id: 0,
            cell_type: CellType::AudioEmitter,
            visible: false,
            solid: false,
            anchored: true,
            collision_events_enabled: true,
            texture: "speaker".to_string(),
            color_rgb: Vec3::ONE,
            particles: crate::engine::particles::EmitterSettings::default(),
            audio: String::new(),
            playing: false,
            looped: false,
            loop_interval: 0.0,
            volume: 1.0,
            audio_min_distance: DEFAULT_AUDIO_MIN_DISTANCE,
            audio_max_distance: DEFAULT_AUDIO_MAX_DISTANCE,
            ..Default::default()
        }
    }

    pub fn new_particle_emitter(preset: crate::engine::particles::EmitterPreset) -> Self {
        Self {
            cell_type: CellType::ParticleEmitter,
            visible: false,
            solid: false,
            collision_events_enabled: false,
            particles: crate::engine::particles::EmitterSettings {
                preset,
                ..Default::default()
            },
            ..Default::default()
        }
    }

    pub fn new_spawn_point() -> Self {
        Self {
            id: 0,
            cell_type: CellType::SpawnPoint,
            visible: true,
            solid: true,
            anchored: true,
            collision_events_enabled: true,
            texture: "Block_tx".to_string(),
            color_rgb: Vec3::new(0.5, 0.5, 0.5),
            ..Default::default()
        }
    }
}
