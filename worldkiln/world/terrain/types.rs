use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt;

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
use super::voxel::{SmoothVoxelGrid, VoxelLayerSettings};

// Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
pub const MAX_GENERATION_VOLUME: u64 = 16_777_216;
pub const PLAINS_GENERATOR_IMPLEMENTATION_VERSION: u32 = 1;
pub const TERRAIN_CHUNK_SIZE: u32 = 32;
// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
pub const TERRAIN_RENDER_CHUNK_TARGET_STUDS: f32 = 64.0;
pub type TerrainRenderChunkKey = [u32; 2];

pub(crate) fn terrain_render_chunk_samples(spacing_studs: f32) -> u32 {
    if !spacing_studs.is_finite() || spacing_studs <= 0.0 {
        return 16;
    }
    (TERRAIN_RENDER_CHUNK_TARGET_STUDS / spacing_studs)
        .ceil()
        .clamp(1.0, u32::MAX as f32) as u32
}

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
/// Upper bound of the existing LANDSCAPING Strength control. Keep preview
/// normalization tied to the authoring control so the inner ring remains
/// meaningful across the full selectable range.
pub const SCULPT_STRENGTH_UI_MAX: f32 = 5.0;

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 11:32 -05:00
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum TerrainMode {
    #[default]
    Smooth,
    Block,
}

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum SculptTool {
    #[default]
    None,
    Raise,
    Lower,
    Flatten,
    // Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
    FlattenRaiseOnly,
    FlattenCutOnly,
    Smooth,
    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    Paint,
}

// Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
impl SculptTool {
    pub fn is_flatten(self) -> bool {
        matches!(
            self,
            Self::Flatten | Self::FlattenRaiseOnly | Self::FlattenCutOnly
        )
    }

    pub fn allows_flatten_direction(self, current: f32, target: f32) -> bool {
        match self {
            Self::Flatten => true,
            Self::FlattenRaiseOnly => target > current,
            Self::FlattenCutOnly => target < current,
            _ => false,
        }
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum TerrainPaintMaterialSlot {
    #[default]
    Surface,
    Subsurface,
    Rock,
    Sand,
    // Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
    Dirt,
}

// Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
impl TerrainPaintMaterialSlot {
    /// Direct Dirt and Sand are stable built-in textures, not configurable layer roles.
    pub fn resolve_material(self, materials: &TerrainMaterials) -> &str {
        match self {
            Self::Surface => &materials.surface,
            Self::Subsurface => &materials.subsurface,
            Self::Rock => &materials.rock,
            Self::Sand => "sand",
            Self::Dirt => "dirt",
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainSculptRequest {
    pub tool: SculptTool,
    pub center: [f32; 3],
    pub radius: f32,
    pub strength: f32,
    pub flatten_target: Option<f32>,
}

// Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
#[derive(Clone, Debug, PartialEq)]
pub struct TerrainPaintRequest {
    pub center: [f32; 3],
    pub radius: f32,
    pub material: String,
}

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
pub fn sculpt_preview_radii(radius: f32, strength: f32) -> (f32, f32) {
    let outer_radius = if radius.is_finite() && radius > 0.0 {
        radius
    } else {
        0.01
    };
    let normalized_strength = if strength.is_finite() {
        (strength / SCULPT_STRENGTH_UI_MAX).clamp(0.0, 1.0)
    } else {
        0.0
    };
    let inner_radius = outer_radius * normalized_strength;
    (outer_radius, inner_radius)
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum TerrainGeneratorType {
    #[default]
    Plains,
    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:45 -05:00
    Programmatic,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct PlainsSettings {
    pub seed: i32,
    pub base_height: f32,
    pub height_variation: f32,
    pub noise_scale: f32,
    pub water_level: f32,
    pub destroy_cells: bool,
}

impl PlainsSettings {
    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        validate_finite("Base Height", self.base_height)?;
        validate_finite("Height Variation", self.height_variation)?;
        validate_finite("Noise Scale", self.noise_scale)?;
        validate_finite("Water Level", self.water_level)?;

        if self.height_variation < 0.0 {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Height Variation",
                reason: "must be zero or greater",
            });
        }
        if self.noise_scale <= 0.0 {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Noise Scale",
                reason: "must be greater than zero",
            });
        }
        if !(self.noise_scale * 4.0).is_finite()
            || !(self.base_height - self.height_variation).is_finite()
            || !(self.base_height + self.height_variation).is_finite()
        {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Plains",
                reason: "derived generator values must remain finite",
            });
        }

        Ok(())
    }
}

impl Default for PlainsSettings {
    fn default() -> Self {
        Self {
            seed: 1337,
            base_height: 16.0,
            height_variation: 8.0,
            noise_scale: 0.02,
            water_level: 12.0,
            destroy_cells: false,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TerrainMaterials {
    pub surface: String,
    pub subsurface: String,
    pub rock: String,
    pub water: String,
}

// Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
impl Default for TerrainMaterials {
    fn default() -> Self {
        Self {
            surface: "grass_1".into(),
            subsurface: "dirt".into(),
            rock: "tile_24".into(),
            water: "water".into(),
        }
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainGrassSettings {
    pub enabled: bool,
    pub density: f32,
    pub length_studs: f32,
}

impl TerrainGrassSettings {
    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        if !self.density.is_finite() || !(0.0..=1.0).contains(&self.density) {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Grass Density",
                reason: "must be finite and between 0 and 1",
            });
        }
        if !self.length_studs.is_finite() || !(0.25..=4.0).contains(&self.length_studs) {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Grass Length",
                reason: "must be finite and between 0.25 and 4 studs",
            });
        }
        Ok(())
    }
}

impl Default for TerrainGrassSettings {
    fn default() -> Self {
        Self {
            enabled: true,
            density: 0.45,
            length_studs: 1.25,
        }
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
/// Persisted, live-rendered water controls. These are shader parameters only:
/// changing them never rebuilds or rewrites authored Terrain geometry.
#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainWaterSettings {
    pub wave_height: f32,
    pub wave_speed: f32,
    pub foam_amount: f32,
    pub foam_scale: f32,
}

impl TerrainWaterSettings {
    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        for (setting, value, range) in [
            ("Wave Height", self.wave_height, 0.0..=3.0),
            ("Wave Speed", self.wave_speed, 0.0..=3.0),
            ("Foam Amount", self.foam_amount, 0.0..=2.0),
            ("Foam Scale", self.foam_scale, 0.25..=4.0),
        ] {
            if !value.is_finite() || !range.contains(&value) {
                return Err(TerrainGenerationError::InvalidSetting {
                    setting,
                    reason: "is outside the supported range",
                });
            }
        }
        Ok(())
    }
}

impl Default for TerrainWaterSettings {
    fn default() -> Self {
        Self {
            wave_height: 1.0,
            wave_speed: 1.0,
            foam_amount: 1.0,
            foam_scale: 1.0,
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:45 -05:00
pub struct TerrainConfig {
    pub mode: TerrainMode,
    pub generator: TerrainGeneratorType,
    pub plains: PlainsSettings,
    pub materials: TerrainMaterials,
    #[serde(default)]
    pub voxel_layers: VoxelLayerSettings,
    // Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
    #[serde(default)]
    pub grass: TerrainGrassSettings,
    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    #[serde(default)]
    pub water: TerrainWaterSettings,
    pub generator_implementation_version: u32,
    // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
    #[serde(default)]
    pub generation_layers: Vec<super::layers::GenerationLayer>,
}

impl TerrainConfig {
    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        if self.generator == TerrainGeneratorType::Plains
            && self.generator_implementation_version != PLAINS_GENERATOR_IMPLEMENTATION_VERSION
        {
            return Err(TerrainGenerationError::UnsupportedGeneratorVersion {
                generator: self.generator,
                version: self.generator_implementation_version,
            });
        }
        self.plains.validate()?;
        self.voxel_layers
            .validate()
            .map_err(|error| TerrainGenerationError::VoxelData(error.to_string()))?;
        // Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
        self.grass.validate()?;
        self.water.validate()?;
        Ok(())
    }
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            mode: TerrainMode::Smooth,
            generator: TerrainGeneratorType::Plains,
            plains: PlainsSettings::default(),
            materials: TerrainMaterials::default(),
            voxel_layers: VoxelLayerSettings::default(),
            // Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
            grass: TerrainGrassSettings::default(),
            water: TerrainWaterSettings::default(),
            generator_implementation_version: PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
            // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
            generation_layers: Vec::new(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Eq, Serialize)]
pub struct TerrainGenerationRequest {
    pub position: [i32; 3],
    pub size: [i32; 3],
}

impl TerrainGenerationRequest {
    pub fn volume(&self) -> Result<u64, TerrainGenerationError> {
        let [size_x, size_y, size_z] = self.size;
        for (axis, value) in [("x", size_x), ("y", size_y), ("z", size_z)] {
            if value <= 0 {
                return Err(TerrainGenerationError::NonPositiveSize { axis, value });
            }
        }

        let volume = (size_x as u64)
            .checked_mul(size_y as u64)
            .and_then(|value| value.checked_mul(size_z as u64))
            .ok_or(TerrainGenerationError::VolumeOverflow)?;
        if volume > MAX_GENERATION_VOLUME {
            return Err(TerrainGenerationError::VolumeLimitExceeded { volume });
        }
        Ok(volume)
    }

    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        self.volume()?;
        self.inclusive_max()?;
        Ok(())
    }

    pub fn inclusive_max(&self) -> Result<[i32; 3], TerrainGenerationError> {
        let mut maximum = [0; 3];
        for (index, (axis, position, size)) in [
            ("x", self.position[0], self.size[0]),
            ("y", self.position[1], self.size[1]),
            ("z", self.position[2], self.size[2]),
        ]
        .into_iter()
        .enumerate()
        {
            maximum[index] = position
                .checked_add(size - 1)
                .ok_or(TerrainGenerationError::BoundsOverflow { axis })?;
        }
        Ok(maximum)
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainResult {
    pub request: TerrainGenerationRequest,
    pub generator: TerrainGeneratorType,
    pub generator_implementation_version: u32,
    pub data: TerrainData,
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum TerrainData {
    Heightfield(HeightfieldData),
    /// Versioned volumetric Smooth Terrain payload. Generation/rendering remains
    /// behind the WO 0.9.3-03 compatibility boundary until Phase 3 lands.
    Voxel(SmoothVoxelGrid),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HeightfieldData {
    pub origin: [i32; 3],
    pub size: [u32; 3],
    /// Row-major surface heights, with X varying fastest and then Z.
    pub heights: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct TerrainState {
    pub config: TerrainConfig,
    pub authored: Option<TerrainResult>,
    pub runtime: Option<TerrainResult>,
    // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
    pub(crate) layered_authored: Option<TerrainResult>,
    // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
    render_revision: u64,
    full_render_revision: u64,
    render_chunk_revisions: HashMap<TerrainRenderChunkKey, u64>,
}

impl Default for TerrainState {
    fn default() -> Self {
        Self {
            config: TerrainConfig::default(),
            authored: None,
            runtime: None,
            // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
            layered_authored: None,
            render_revision: 0,
            full_render_revision: 0,
            render_chunk_revisions: HashMap::new(),
        }
    }
}

impl PartialEq for TerrainState {
    fn eq(&self, other: &Self) -> bool {
        self.config == other.config
            && self.authored == other.authored
            && self.runtime == other.runtime
    }
}

impl TerrainState {
    pub fn effective(&self) -> Option<&TerrainResult> {
        // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
        self.runtime
            .as_ref()
            .or(self.layered_authored.as_ref())
            .or(self.authored.as_ref())
    }

    // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
    pub(crate) fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub(crate) fn full_render_revision(&self) -> u64 {
        self.full_render_revision
    }

    pub(crate) fn render_chunk_revision(&self, key: TerrainRenderChunkKey) -> u64 {
        self.render_chunk_revisions.get(&key).copied().unwrap_or(0)
    }

    fn next_render_revision(&mut self) -> u64 {
        self.render_revision = self.render_revision.wrapping_add(1).max(1);
        self.render_revision
    }

    pub(crate) fn mark_full_render_dirty(&mut self) {
        let revision = self.next_render_revision();
        self.full_render_revision = revision;
        self.render_chunk_revisions.clear();
    }

    pub(crate) fn replace_authored(&mut self, authored: Option<TerrainResult>) {
        self.authored = authored;
        // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
        if self
            .config
            .generation_layers
            .iter()
            .any(|layer| layer.applied.is_some())
        {
            if self.rebuild_generation_layers().is_err() {
                self.layered_authored = None;
            }
        } else {
            self.layered_authored = None;
        }
        self.mark_full_render_dirty();
    }

    pub(crate) fn replace_runtime(&mut self, runtime: Option<TerrainResult>) {
        self.runtime = runtime;
        self.mark_full_render_dirty();
    }

    // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
    pub(crate) fn from_persisted(config: TerrainConfig, authored: Option<TerrainResult>) -> Self {
        let mut state = Self {
            config,
            authored,
            runtime: None,
            // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
            layered_authored: None,
            render_revision: 0,
            full_render_revision: 0,
            render_chunk_revisions: HashMap::new(),
        };
        if state.authored.is_some() {
            state.mark_full_render_dirty();
        }
        // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
        if let Err(error) = state.rebuild_generation_layers() {
            eprintln!(
                "Terrain layers failed to compose; original authored data remains intact: {error}"
            );
        }
        state
    }

    pub(crate) fn mark_authored_brush_dirty(&mut self, center: [f32; 3], radius: f32) {
        // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
        // Layer-local brushes dirty only covered render chunks, like base
        // sculpt, rather than invalidating the entire composed Terrain.
        let Some(authored) = self.layered_authored.as_ref().or(self.authored.as_ref()) else {
            return;
        };
        let TerrainData::Voxel(grid) = &authored.data else {
            self.mark_full_render_dirty();
            return;
        };
        if !radius.is_finite() || radius <= 0.0 {
            self.mark_full_render_dirty();
            return;
        }

        let voxel_size = grid.spacing_studs;
        let border = voxel_size;
        let min_world_x = center[0] - radius - border;
        let max_world_x = center[0] + radius + border;
        let min_world_z = center[2] - radius - border;
        let max_world_z = center[2] + radius + border;
        let origin_x = grid.origin[0] as f32;
        let origin_z = grid.origin[2] as f32;

        let min_x = (((min_world_x - origin_x) / voxel_size).floor() as i32)
            .clamp(0, grid.size[0].saturating_sub(1) as i32) as u32;
        let max_x = (((max_world_x - origin_x) / voxel_size).floor() as i32)
            .clamp(0, grid.size[0].saturating_sub(1) as i32) as u32;
        let min_z = (((min_world_z - origin_z) / voxel_size).floor() as i32)
            .clamp(0, grid.size[2].saturating_sub(1) as i32) as u32;
        let max_z = (((max_world_z - origin_z) / voxel_size).floor() as i32)
            .clamp(0, grid.size[2].saturating_sub(1) as i32) as u32;
        let chunk_samples = terrain_render_chunk_samples(grid.spacing_studs);

        let revision = self.next_render_revision();
        for chunk_z in (min_z / chunk_samples)..=(max_z / chunk_samples) {
            for chunk_x in (min_x / chunk_samples)..=(max_x / chunk_samples) {
                self.render_chunk_revisions
                    .insert([chunk_x, chunk_z], revision);
            }
        }
    }
}

impl HeightfieldData {
    pub fn height(&self, local_x: u32, local_z: u32) -> Option<f32> {
        if local_x >= self.size[0] || local_z >= self.size[2] {
            return None;
        }
        let index = local_z as usize * self.size[0] as usize + local_x as usize;
        self.heights.get(index).copied()
    }

    pub fn surface_height_at(&self, world_x: f32, world_z: f32, mode: TerrainMode) -> Option<f32> {
        let local_x = world_x - self.origin[0] as f32;
        let local_z = world_z - self.origin[2] as f32;
        if local_x < 0.0
            || local_z < 0.0
            || local_x >= self.size[0] as f32
            || local_z >= self.size[2] as f32
        {
            return None;
        }

        if mode == TerrainMode::Block {
            return self
                .height(local_x.floor() as u32, local_z.floor() as u32)
                .map(f32::round);
        }

        let sample_x = (local_x - 0.5).clamp(0.0, self.size[0].saturating_sub(1) as f32);
        let sample_z = (local_z - 0.5).clamp(0.0, self.size[2].saturating_sub(1) as f32);
        let x0 = sample_x.floor() as u32;
        let z0 = sample_z.floor() as u32;
        let x1 = (x0 + 1).min(self.size[0] - 1);
        let z1 = (z0 + 1).min(self.size[2] - 1);
        let tx = sample_x - x0 as f32;
        let tz = sample_z - z0 as f32;
        let north = self.height(x0, z0)? * (1.0 - tx) + self.height(x1, z0)? * tx;
        let south = self.height(x0, z1)? * (1.0 - tx) + self.height(x1, z1)? * tx;
        Some(north * (1.0 - tz) + south * tz)
    }

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    /// Returns whether a grid cell overlaps the generated solid heightfield.
    /// Water is intentionally excluded because it is a rendered surface only.
    pub fn solid_occupies_cell(&self, coord: crate::world::WorldCoord) -> bool {
        let local_x = coord.x - self.origin[0];
        let local_z = coord.z - self.origin[2];
        if local_x < 0
            || local_z < 0
            || local_x >= self.size[0] as i32
            || local_z >= self.size[2] as i32
            || coord.y < self.origin[1]
        {
            return false;
        }

        self.height(local_x as u32, local_z as u32)
            .is_some_and(|height| (coord.y as f32) < height)
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    pub fn apply_sculpt(&mut self, request: &TerrainSculptRequest, min_y: f32, max_y: f32) -> bool {
        if request.tool == SculptTool::None || self.size[0] == 0 || self.size[2] == 0 {
            return false;
        }
        if request.radius <= 0.0
            || !request.radius.is_finite()
            || request.strength <= 0.0
            || !request.strength.is_finite()
            || !request.center[0].is_finite()
            || !request.center[1].is_finite()
            || !request.center[2].is_finite()
        {
            return false;
        }
        if request.tool.is_flatten()
            && (request.flatten_target.is_none() || !request.flatten_target.unwrap().is_finite())
        {
            return false;
        }

        let radius = request.radius;
        let radius_sq = radius * radius;
        let local_center_x = request.center[0] - self.origin[0] as f32 - 0.5;
        let local_center_z = request.center[2] - self.origin[2] as f32 - 0.5;
        let max_x_index = self.size[0] as i32 - 1;
        let max_z_index = self.size[2] as i32 - 1;
        let min_x = ((local_center_x - radius).floor() as i32).clamp(0, max_x_index);
        let max_x = ((local_center_x + radius).ceil() as i32).clamp(0, max_x_index);
        let min_z = ((local_center_z - radius).floor() as i32).clamp(0, max_z_index);
        let max_z = ((local_center_z + radius).ceil() as i32).clamp(0, max_z_index);

        // Smooth needs stable pre-step values so its result is independent of
        // iteration order. Snapshot only the brush-local dependency rectangle
        // plus the one-sample border used by the 3x3 averaging kernel.
        let smooth_snapshot = if request.tool == SculptTool::Smooth {
            let snapshot_min_x = (min_x - 1).max(0);
            let snapshot_max_x = (max_x + 1).min(max_x_index);
            let snapshot_min_z = (min_z - 1).max(0);
            let snapshot_max_z = (max_z + 1).min(max_z_index);
            let snapshot_width = (snapshot_max_x - snapshot_min_x + 1) as usize;
            let snapshot_depth = (snapshot_max_z - snapshot_min_z + 1) as usize;
            let mut values = Vec::with_capacity(snapshot_width * snapshot_depth);
            for z in snapshot_min_z..=snapshot_max_z {
                let row_start = z as usize * self.size[0] as usize + snapshot_min_x as usize;
                let row_end = row_start + snapshot_width;
                values.extend_from_slice(&self.heights[row_start..row_end]);
            }
            Some((
                snapshot_min_x,
                snapshot_min_z,
                snapshot_width,
                snapshot_max_x,
                snapshot_max_z,
                values,
            ))
        } else {
            None
        };

        let mut changed = false;
        for z in min_z..=max_z {
            let z = z as u32;
            let sample_z = self.origin[2] as f32 + z as f32 + 0.5;
            let dz = sample_z - request.center[2];
            let dz_sq = dz * dz;
            if dz_sq > radius_sq {
                continue;
            }

            for x in min_x..=max_x {
                let x = x as u32;
                let sample_x = self.origin[0] as f32 + x as f32 + 0.5;
                let dx = sample_x - request.center[0];
                let dist_sq = dx * dx + dz_sq;
                if dist_sq > radius_sq {
                    continue;
                }

                let dist = dist_sq.sqrt();
                let falloff = (1.0 - (dist / radius) * (dist / radius)).max(0.0);
                if falloff <= 0.0 {
                    continue;
                }

                let idx = z as usize * self.size[0] as usize + x as usize;
                let current_height = self.heights[idx];

                let target_height = match request.tool {
                    SculptTool::None => current_height,
                    SculptTool::Raise => current_height + request.strength * falloff,
                    SculptTool::Lower => current_height - request.strength * falloff,
                    // Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
                    SculptTool::Flatten
                    | SculptTool::FlattenRaiseOnly
                    | SculptTool::FlattenCutOnly => {
                        let target = request.flatten_target.unwrap();
                        if request
                            .tool
                            .allows_flatten_direction(current_height, target)
                        {
                            current_height
                                + (target - current_height) * (request.strength * falloff).min(1.0)
                        } else {
                            current_height
                        }
                    }
                    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
                    SculptTool::Paint => current_height,
                    SculptTool::Smooth => {
                        let (
                            snapshot_min_x,
                            snapshot_min_z,
                            snapshot_width,
                            snapshot_max_x,
                            snapshot_max_z,
                            values,
                        ) = smooth_snapshot.as_ref().unwrap();
                        let mut sum = 0.0;
                        let mut count = 0.0;
                        for n_dz in -1..=1 {
                            for n_dx in -1..=1 {
                                let nx = x as i32 + n_dx;
                                let nz = z as i32 + n_dz;
                                if nx >= *snapshot_min_x
                                    && nx <= *snapshot_max_x
                                    && nz >= *snapshot_min_z
                                    && nz <= *snapshot_max_z
                                {
                                    let local_x = (nx - *snapshot_min_x) as usize;
                                    let local_z = (nz - *snapshot_min_z) as usize;
                                    sum += values[local_z * *snapshot_width + local_x];
                                    count += 1.0;
                                }
                            }
                        }
                        let avg = sum / count;
                        let diff = avg - current_height;
                        current_height + diff * (request.strength * falloff).min(1.0)
                    }
                };

                let clamped = target_height.clamp(min_y, max_y);
                if (clamped - current_height).abs() > 1e-6 {
                    self.heights[idx] = clamped;
                    changed = true;
                }
            }
        }

        changed
    }
}

impl TerrainResult {
    pub fn solid_occupies_cell(&self, coord: crate::world::WorldCoord) -> bool {
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:25 -05:00
        let within_request = [coord.x, coord.y, coord.z]
            .into_iter()
            .zip(self.request.position)
            .zip(self.request.size)
            .all(|((value, origin), size)| value >= origin && value < origin.saturating_add(size));
        if !within_request {
            return false;
        }
        match &self.data {
            TerrainData::Heightfield(heightfield) => heightfield.solid_occupies_cell(coord),
            TerrainData::Voxel(grid) => grid.solid_occupies_cell(coord),
        }
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    pub fn apply_sculpt(&mut self, request: &TerrainSculptRequest) -> bool {
        if request.tool == SculptTool::Paint {
            return false;
        }
        let min_y = self.request.position[1] as f32;
        let max_y = (self.request.position[1] + self.request.size[1] - 1) as f32;
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:45 -05:00
        match &mut self.data {
            TerrainData::Heightfield(heightfield) => {
                heightfield.apply_sculpt(request, min_y, max_y)
            }
            TerrainData::Voxel(grid) => grid.apply_surface_sculpt(request, min_y, max_y),
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    pub fn apply_paint(&mut self, request: &TerrainPaintRequest) -> bool {
        match &mut self.data {
            TerrainData::Heightfield(_) => false,
            TerrainData::Voxel(grid) => grid.paint_surface_material(request),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerrainGenerationError {
    NonPositiveSize {
        axis: &'static str,
        value: i32,
    },
    VolumeOverflow,
    VolumeLimitExceeded {
        volume: u64,
    },
    BoundsOverflow {
        axis: &'static str,
    },
    InvalidSetting {
        setting: &'static str,
        reason: &'static str,
    },
    UnsupportedGeneratorVersion {
        generator: TerrainGeneratorType,
        version: u32,
    },
    VoxelData(String),
}

impl fmt::Display for TerrainGenerationError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::NonPositiveSize { axis, value } => {
                write!(
                    formatter,
                    "Terrain size {axis} must be greater than zero; received {value}"
                )
            }
            Self::VolumeOverflow => write!(formatter, "Terrain generation volume overflowed"),
            Self::VolumeLimitExceeded { volume } => write!(
                formatter,
                "Terrain volume {volume} exceeds the safety limit of {MAX_GENERATION_VOLUME}"
            ),
            Self::BoundsOverflow { axis } => {
                write!(
                    formatter,
                    "Terrain generation bounds overflow on the {axis} axis"
                )
            }
            Self::InvalidSetting { setting, reason } => {
                write!(formatter, "Invalid Terrain {setting}: {reason}")
            }
            Self::UnsupportedGeneratorVersion { generator, version } => write!(
                formatter,
                "Unsupported {generator:?} Terrain generator implementation version {version}"
            ),
            Self::VoxelData(reason) => write!(formatter, "Invalid smooth voxel Terrain: {reason}"),
        }
    }
}

impl std::error::Error for TerrainGenerationError {}

fn validate_finite(setting: &'static str, value: f32) -> Result<(), TerrainGenerationError> {
    if value.is_finite() {
        Ok(())
    } else {
        Err(TerrainGenerationError::InvalidSetting {
            setting,
            reason: "must be finite",
        })
    }
}
