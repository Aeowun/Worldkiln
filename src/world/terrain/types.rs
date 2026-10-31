// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fmt;

use super::voxel::{SmoothVoxelGrid, VoxelLayerSettings};

pub const MAX_GENERATION_VOLUME: u64 = 16_777_216;
pub const PLAINS_GENERATOR_IMPLEMENTATION_VERSION: u32 = 1;
pub const TERRAIN_CHUNK_SIZE: u32 = 32;
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

pub const SCULPT_STRENGTH_UI_MAX: f32 = 5.0;

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum SculptTool {
    #[default]
    None,
    Raise,
    Lower,
    Flatten,
    FlattenRaiseOnly,
    FlattenCutOnly,
    Smooth,
    Paint,
}

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

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
pub enum TerrainPaintMaterialSlot {
    #[default]
    Surface,
    Subsurface,
    Rock,
    Sand,
    Dirt,
}

impl TerrainPaintMaterialSlot {
    pub fn resolve_material(self, materials: &TerrainMaterials) -> &str {
        match self {
            Self::Surface => &materials.surface,
            Self::Subsurface => &materials.subsurface,
            Self::Rock => &materials.rock,
            Self::Sand => "PBR:Sand",
            Self::Dirt => "PBR:Dirt",
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

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainPaintRequest {
    pub center: [f32; 3],
    pub radius: f32,
    pub material: String,
    #[serde(default = "default_paint_strength")]
    pub strength: f32,
    #[serde(default)]
    pub falloff: Option<f32>,
}

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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TerrainMaterialBlend {
    pub rock_start: f32,
    pub rock_end: f32,
    pub dirt_strength: f32,
    pub beach_width: f32,
}
impl Default for TerrainMaterialBlend {
    fn default() -> Self {
        Self {
            rock_start: 0.48,
            rock_end: 0.84,
            dirt_strength: 0.03,
            beach_width: 3.5,
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

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct WaterMaterial {
    pub base_color: String,
    pub normal_gl: String,
    pub roughness: String,
    pub displacement: String,
    pub ambient_occlusion: String,
}

impl Default for TerrainMaterials {
    fn default() -> Self {
        Self {
            surface: String::new(),
            subsurface: String::new(),
            rock: String::new(),
            water: "Water".into(),
        }
    }
}

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

#[derive(Clone, Copy, Debug, Deserialize, PartialEq, Serialize)]
#[serde(default)]
pub struct TerrainWaterSettings {
    pub wave_height: f32,
    pub wave_speed: f32,
    pub wave_length: f32,
    pub wave_direction_degrees: f32,
    pub wave_steepness: f32,
    pub absorption_color: [f32; 3],
    pub absorption_strength: f32,
    pub scattering_color: [f32; 3],
    pub scattering_strength: f32,
    pub foam_amount: f32,
    pub foam_scale: f32,
    pub shallow_color: [f32; 3],
    pub deep_color: [f32; 3],
    pub foam_color: [f32; 3],
    pub underwater_color: [f32; 3],
    pub opacity: f32,
    pub underwater_opacity: f32,
    pub reflection_strength: f32,
    pub specular_strength: f32,
    pub underwater_effects_enabled: bool,
    pub underwater_fog_color: [f32; 3],
    pub underwater_fog_density: f32,
    pub underwater_depth_fog: f32,
    pub underwater_depth_fog_limit: f32,
    pub underwater_fog_limit: f32,
    pub underwater_sky_tint: f32,
}

impl TerrainWaterSettings {
    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        for (setting, value, range) in [
            ("Wave Height", self.wave_height, 0.0..=10.0),
            ("Wave Speed", self.wave_speed, 0.0..=3.0),
            ("Wave Length", self.wave_length, 1.0..=500.0),
            (
                "Wave Direction",
                self.wave_direction_degrees,
                -360.0..=360.0,
            ),
            ("Wave Steepness", self.wave_steepness, 0.0..=1.0),
            ("Absorption Strength", self.absorption_strength, 0.0..=5.0),
            ("Scattering Strength", self.scattering_strength, 0.0..=5.0),
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
        for (setting, rgb) in [
            ("Shallow Color", self.shallow_color),
            ("Deep Color", self.deep_color),
            ("Absorption Color", self.absorption_color),
            ("Scattering Color", self.scattering_color),
            ("Foam Color", self.foam_color),
            ("Underwater Color", self.underwater_color),
        ] {
            if rgb
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
            {
                return Err(TerrainGenerationError::InvalidSetting {
                    setting,
                    reason: "RGB must be finite and between 0 and 1",
                });
            }
        }
        for (setting, value, max) in [
            ("Water Opacity", self.opacity, 1.0),
            ("Underwater Opacity", self.underwater_opacity, 1.0),
            ("Reflection Strength", self.reflection_strength, 2.0),
            ("Sun Highlight", self.specular_strength, 2.0),
        ] {
            if !value.is_finite() || !(0.0..=max).contains(&value) {
                return Err(TerrainGenerationError::InvalidSetting {
                    setting,
                    reason: "is outside the supported range",
                });
            }
        }
        if self
            .underwater_fog_color
            .iter()
            .any(|v| !v.is_finite() || !(0.0..=1.0).contains(v))
        {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Underwater Fog Color",
                reason: "RGB must be finite and between 0 and 1",
            });
        }
        for (setting, value, max) in [
            ("Underwater Fog Density", self.underwater_fog_density, 0.5),
            ("Underwater Depth Fog", self.underwater_depth_fog, 0.1),
            (
                "Underwater Depth Fog Limit",
                self.underwater_depth_fog_limit,
                1.0,
            ),
            ("Underwater Fog Limit", self.underwater_fog_limit, 1.0),
            ("Underwater Sky Tint", self.underwater_sky_tint, 1.0),
        ] {
            if !value.is_finite() || !(0.0..=max).contains(&value) {
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
            wave_length: 50.0,
            wave_direction_degrees: 0.0,
            wave_steepness: 0.0,
            absorption_color: [0.018, 0.12, 0.22],
            absorption_strength: 0.0,
            scattering_color: [0.085, 0.47, 0.50],
            scattering_strength: 0.0,
            foam_amount: 1.0,
            foam_scale: 1.0,
            shallow_color: [0.085, 0.47, 0.50],
            deep_color: [0.018, 0.12, 0.22],
            foam_color: [0.93, 0.97, 0.94],
            underwater_color: [0.025, 0.24, 0.31],
            opacity: 0.78,
            underwater_opacity: 0.84,
            reflection_strength: 1.0,
            specular_strength: 1.0,
            underwater_effects_enabled: true,
            underwater_fog_color: [0.055, 0.24, 0.32],
            underwater_fog_density: 0.065,
            underwater_depth_fog: 0.018,
            underwater_depth_fog_limit: 0.32,
            underwater_fog_limit: 0.94,
            underwater_sky_tint: 0.88,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum OceanOpticsMode {
    #[default]
    Ray,
    Height,
    Off,
}
impl OceanOpticsMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Ray => "Ray",
            Self::Height => "Height",
            Self::Off => "Off",
        }
    }
    pub const fn shader_id(self) -> i32 {
        match self {
            Self::Off => 0,
            Self::Height => 1,
            Self::Ray => 2,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WaterRealismMode {
    Basic,
    #[default]
    Legacy,
    Realistic,
}
impl WaterRealismMode {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Basic => "Basic",
            Self::Legacy => "Legacy",
            Self::Realistic => "Realistic",
        }
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainConfig {
    #[serde(default)]
    pub material_layers: Vec<super::material_layers::TerrainMaterialLayer>,
    #[serde(default = "super::material_layers::legacy_shore_layer")]
    pub shore_material_layer: String,
    #[serde(default)]
    pub material_role_replacements: std::collections::BTreeMap<String, String>,
    pub generator: TerrainGeneratorType,
    pub plains: PlainsSettings,
    pub materials: TerrainMaterials,
    #[serde(default)]
    pub material_blend: TerrainMaterialBlend,
    #[serde(default = "default_paint_falloff")]
    pub paint_falloff: f32,
    #[serde(default)]
    pub native_base_paint_strokes: Vec<TerrainPaintRequest>,
    #[serde(default)]
    pub voxel_layers: VoxelLayerSettings,
    #[serde(default)]
    pub grass: TerrainGrassSettings,
    #[serde(default)]
    pub water: TerrainWaterSettings,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water_custom_preset: Option<TerrainWaterSettings>,
    #[serde(default)]
    pub infinite_ocean: bool,
    #[serde(default)]
    pub water_realism: WaterRealismMode,
    #[serde(default)]
    pub ocean_optics_mode: OceanOpticsMode,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub water_material_asset: Option<String>,
    #[serde(default = "ocean_default_enabled")]
    pub ocean_enabled: bool,
    #[serde(default)]
    pub water_bodies: Vec<super::water_bodies::WaterBody>,
    pub generator_implementation_version: u32,
    #[serde(default)]
    pub generation_layers: Vec<super::layers::GenerationLayer>,
}

impl TerrainConfig {
    pub fn resolved_water(
        &self,
        catalog: &crate::project::material::MaterialCatalog,
    ) -> TerrainWaterSettings {
        self.water_material_asset
            .as_deref()
            .and_then(|id| catalog.last_known_water(id))
            .unwrap_or(self.water)
    }
    pub fn validate(&self) -> Result<(), TerrainGenerationError> {
        let mut layer_ids = std::collections::HashSet::new();
        if self
            .material_layers
            .iter()
            .any(|l| l.id.is_empty() || !layer_ids.insert(&l.id))
        {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Materials",
                reason: "empty or duplicate layer identity",
            });
        }
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
        self.grass.validate()?;
        self.water.validate()?;
        if let Some(custom) = &self.water_custom_preset {
            custom.validate()?;
        }
        let mut seen = std::collections::HashSet::new();
        if self.water_bodies.len() > 256 {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Water Bodies",
                reason: "too many water bodies",
            });
        }
        for body in &self.water_bodies {
            if body.validate().is_err() || !seen.insert(body.id) {
                return Err(TerrainGenerationError::InvalidSetting {
                    setting: "Water Bodies",
                    reason: "invalid geometry or duplicate ID",
                });
            }
        }
        Ok(())
    }
}

fn ocean_default_enabled() -> bool {
    true
}
pub(crate) fn default_paint_strength() -> f32 {
    1.0
}
fn default_paint_falloff() -> f32 {
    0.22
}

impl Default for TerrainConfig {
    fn default() -> Self {
        Self {
            material_layers: Vec::new(),
            shore_material_layer: super::material_layers::default_shore_layer(),
            material_role_replacements: Default::default(),
            generator: TerrainGeneratorType::Plains,
            plains: PlainsSettings::default(),
            materials: TerrainMaterials::default(),
            material_blend: TerrainMaterialBlend::default(),
            paint_falloff: default_paint_falloff(),
            native_base_paint_strokes: Vec::new(),
            voxel_layers: VoxelLayerSettings::default(),
            grass: TerrainGrassSettings::default(),
            water: TerrainWaterSettings::default(),
            water_custom_preset: None,
            infinite_ocean: false,
            water_realism: WaterRealismMode::Legacy,
            ocean_optics_mode: OceanOpticsMode::Ray,
            water_material_asset: None,
            ocean_enabled: true,
            water_bodies: Vec::new(),
            generator_implementation_version: PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
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

#[cfg(test)]
mod generation_volume_policy_tests {
    use super::*;
    #[test]
    fn landscape_generation_exceeding_old_benchmark_volume_is_valid() {
        let request = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [1024, 64, 1024],
        };
        assert_eq!(request.volume().unwrap(), 67_108_864);
        assert!(request.validate().is_ok());
    }
    #[test]
    fn landscape_generation_still_rejects_coordinate_overflow() {
        let request = TerrainGenerationRequest {
            position: [i32::MAX, 0, 0],
            size: [8, 4, 8],
        };
        assert!(matches!(
            request.validate(),
            Err(TerrainGenerationError::BoundsOverflow { axis: "x" })
        ));
    }
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct TerrainResult {
    pub request: TerrainGenerationRequest,
    pub generator: TerrainGeneratorType,
    pub generator_implementation_version: u32,
    pub data: TerrainData,
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub enum TerrainData {
    Heightfield(HeightfieldData),
    Voxel(SmoothVoxelGrid),
}

#[derive(Clone, Debug, Deserialize, PartialEq, Serialize)]
pub struct HeightfieldData {
    pub origin: [i32; 3],
    pub size: [u32; 3],
    pub heights: Vec<f32>,
}

#[derive(Clone, Debug)]
pub struct TerrainState {
    pub config: TerrainConfig,
    pub authored: Option<TerrainResult>,
    pub runtime: Option<TerrainResult>,
    pub(crate) layered_authored: Option<TerrainResult>,
    render_revision: u64,
    full_render_revision: u64,
    render_chunk_revisions: HashMap<TerrainRenderChunkKey, u64>,
    render_dirty_history: std::collections::VecDeque<(u64, Vec<TerrainRenderChunkKey>)>,
    render_dirty_floor: u64,
    pub paint_weights: crate::landscape::material_weights::MaterialWeightAtlas,
    pub paint_pending: std::cell::RefCell<Vec<crate::landscape::material_weights::DirtyUpload>>,
    pub paint_epoch: u64,
    pub paint_falloff: f32,
}

impl Default for TerrainState {
    fn default() -> Self {
        Self {
            config: TerrainConfig::default(),
            authored: None,
            runtime: None,
            layered_authored: None,
            render_revision: 0,
            full_render_revision: 0,
            render_chunk_revisions: HashMap::new(),
            render_dirty_history: Default::default(),
            render_dirty_floor: 0,
            paint_weights: Default::default(),
            paint_pending: Default::default(),
            paint_epoch: 0,
            paint_falloff: 0.22,
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
        self.runtime
            .as_ref()
            .or(self.layered_authored.as_ref())
            .or(self.authored.as_ref())
    }

    pub fn paint_native(&mut self, request: &TerrainPaintRequest) -> bool {
        use crate::landscape::{
            material_weights::{MaterialWeightPage, PAGE_DIM, PageId},
            paint_mask::CircularPaintBrush,
        };
        let Some(result) = self.effective() else {
            return false;
        };
        let origin = [
            result.request.position[0] as f32,
            result.request.position[2] as f32,
        ];
        let brush = CircularPaintBrush {
            center: request.center,
            radius: request.radius,
            edge_softness: request
                .falloff
                .unwrap_or(self.paint_falloff)
                .clamp(0.0, 1.0),
        };
        if brush.validated().is_none() {
            return false;
        }
        if !request.strength.is_finite() || !(0.0..=1.0).contains(&request.strength) {
            return false;
        }
        let dynamic_id = request.material.strip_prefix("layer:");
        let legacy_channel = if dynamic_id.is_none() {
            match request
                .material
                .trim_start_matches("PBR:")
                .to_ascii_lowercase()
                .as_str()
            {
                "grass" | "surface" | "grass_1" => Some(0),
                "dirt" | "subsurface" => Some(1),
                "rock" | "tile_24" => Some(2),
                "sand" | "beach" => Some(3),
                _ => None,
            }
        } else {
            None
        };
        let layer_id = if let Some(id) = dynamic_id {
            Some(id.to_owned())
        } else if legacy_channel.is_none() {
            self.config.paint_layer_id(&request.material)
        } else {
            None
        };
        if legacy_channel.is_none() && layer_id.is_none() {
            return false;
        }
        const PITCH: f32 = 0.25;
        let span = PITCH * PAGE_DIM as f32;
        let x0 = ((request.center[0] - request.radius - origin[0]) / span).floor() as i32;
        let x1 = ((request.center[0] + request.radius - origin[0]) / span).floor() as i32;
        let z0 = ((request.center[2] - request.radius - origin[1]) / span).floor() as i32;
        let z1 = ((request.center[2] + request.radius - origin[1]) / span).floor() as i32;
        if (x1 as i64 - x0 as i64 + 1) * (z1 as i64 - z0 as i64 + 1) > 256 {
            return false;
        }
        let mut affected_pages = Vec::new();
        for z in z0..=z1 {
            for x in x0..=x1 {
                let id = PageId { chart: 1, x, y: z };
                affected_pages.push(id);
                let page_origin = [origin[0] + x as f32 * span, origin[1] + z as f32 * span];
                if let std::collections::btree_map::Entry::Vacant(slot) =
                    self.paint_weights.pages.entry(id)
                {
                    if let Some(page) = MaterialWeightPage::new(id, page_origin, PITCH) {
                        slot.insert(page);
                    }
                }
            }
        }
        if let Some(id) = layer_id {
            let changed = self.paint_weights.stroke_layer_on_pages(
                &affected_pages,
                &brush,
                &id,
                request.strength,
            );
            let changed_count = changed.len();
            self.paint_pending.borrow_mut().extend(changed);
            return changed_count > 0;
        }
        let transaction = self.paint_weights.stroke_indexed(
            1,
            origin,
            PITCH,
            &brush,
            legacy_channel.unwrap(),
            request.strength,
        );
        if transaction.pages.is_empty() {
            return false;
        }
        let uploads = self.paint_weights.extract_dirty_uploads(&transaction);
        self.paint_pending.borrow_mut().extend(uploads);
        true
    }
    pub fn paint_base_and_record(&mut self, request: &TerrainPaintRequest) -> bool {
        if !self.config.generation_layers.is_empty() {
            return false;
        }
        if self.paint_native(request) {
            self.config.native_base_paint_strokes.push(request.clone());
            true
        } else {
            false
        }
    }
    pub(crate) fn rebuild_paint_from_layer_strokes(&mut self) {
        self.paint_falloff = self.config.paint_falloff;
        self.paint_weights = Default::default();
        self.paint_pending.borrow_mut().clear();
        self.paint_epoch = self.paint_epoch.wrapping_add(1);
        let base_paints = self.config.native_base_paint_strokes.clone();
        for request in &base_paints {
            self.paint_native(request);
        }
        let paints: Vec<_> = self
            .config
            .generation_layers
            .iter()
            .filter(|l| l.visible && l.applied.is_some())
            .flat_map(|l| l.brush_edits.iter())
            .filter_map(|edit| match edit {
                super::LayerBrushEdit::Paint {
                    center,
                    radius,
                    material,
                    strength,
                    falloff,
                } => Some(TerrainPaintRequest {
                    strength: *strength,
                    falloff: *falloff,
                    center: *center,
                    radius: *radius,
                    material: material.clone(),
                }),
                _ => None,
            })
            .collect();
        for request in &paints {
            self.paint_native(request);
        }
        self.paint_pending.borrow_mut().clear();
    }

    pub(crate) fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub(crate) fn full_render_revision(&self) -> u64 {
        self.full_render_revision
    }

    pub(crate) fn render_chunk_revision(&self, key: TerrainRenderChunkKey) -> u64 {
        self.render_chunk_revisions.get(&key).copied().unwrap_or(0)
    }

    pub(crate) fn render_dirty_chunks_since(
        &self,
        revision: u64,
    ) -> Option<Vec<TerrainRenderChunkKey>> {
        if revision < self.render_dirty_floor || revision > self.render_revision {
            return None;
        }
        let mut keys = std::collections::BTreeSet::new();
        for (changed, region) in self.render_dirty_history.iter().rev() {
            if *changed <= revision {
                break;
            }
            keys.extend(region.iter().copied());
        }
        Some(keys.into_iter().collect())
    }

    pub(crate) fn mark_render_chunks_dirty(&mut self, mut keys: Vec<TerrainRenderChunkKey>) {
        keys.sort_unstable();
        keys.dedup();
        if keys.is_empty() {
            return;
        }
        let revision = self.next_render_revision();
        for &key in &keys {
            self.render_chunk_revisions.insert(key, revision);
        }
        self.render_dirty_history.push_back((revision, keys));
        if self.render_dirty_history.len() > 128 {
            self.render_dirty_floor = self.render_dirty_history.pop_front().unwrap().0;
        }
    }

    fn next_render_revision(&mut self) -> u64 {
        self.render_revision = self.render_revision.wrapping_add(1).max(1);
        self.render_revision
    }

    pub(crate) fn mark_full_render_dirty(&mut self) {
        let revision = self.next_render_revision();
        self.full_render_revision = revision;
        self.render_chunk_revisions.clear();
        self.render_dirty_history.clear();
        self.render_dirty_floor = revision;
    }

    pub(crate) fn replace_authored(&mut self, authored: Option<TerrainResult>) {
        self.authored = authored;
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
        if self.runtime.is_none() && runtime.is_none() {
            return;
        }
        self.runtime = runtime;
        self.mark_full_render_dirty();
    }

    pub(crate) fn from_persisted(
        mut config: TerrainConfig,
        authored: Option<TerrainResult>,
    ) -> Self {
        let mut authored = authored;
        let migrating = config.generation_layers.is_empty()
            && matches!(
                authored.as_ref().map(|r| &r.data),
                Some(TerrainData::Heightfield(_))
            );
        if migrating {
            if let Some(TerrainResult {
                request,
                data: TerrainData::Heightfield(field),
                ..
            }) = authored.as_ref()
            {
                let mut layer = super::layers::GenerationLayer::new(1, *request);
                layer.name = Some("Imported Heightmap".into());
                layer.settings.water_level = config.plains.water_level;
                layer.applied = Some(layer.settings.clone());
                layer.heightmap = Some(field.clone());
                config.generation_layers.push(layer);
            }
        }
        if (config.material_layers.is_empty() && config.shore_material_layer=="legacy-sand")
            || config.native_base_paint_strokes.iter().any(|r|r.material.starts_with("Material:"))
            || config.generation_layers.iter().flat_map(|l|&l.brush_edits).any(|e|matches!(e,super::LayerBrushEdit::Paint{material,..} if material.starts_with("Material:"))) {
            config.migrate_material_layers();
        }
        let mut state = Self {
            config,
            authored,
            runtime: None,
            layered_authored: None,
            render_revision: 0,
            full_render_revision: 0,
            render_chunk_revisions: HashMap::new(),
            render_dirty_history: Default::default(),
            render_dirty_floor: 0,
            paint_weights: Default::default(),
            paint_pending: Default::default(),
            paint_epoch: 0,
            paint_falloff: 0.22,
        };
        if state.authored.is_some() {
            state.mark_full_render_dirty();
        }
        if let Err(error) = state.rebuild_generation_layers_cached(true) {
            if migrating {
                state.config.generation_layers.clear();
                state.layered_authored = None;
            }
            eprintln!(
                "Terrain layers failed to compose; original authored data remains intact: {error}"
            );
        } else if migrating {
            state.authored = None;
        }
        state
    }

    pub(crate) fn mark_authored_heightfield_samples_dirty(&mut self, indices: &[usize]) {
        let Some(crate::world::TerrainResult {
            data: TerrainData::Heightfield(field),
            ..
        }) = self.authored.as_ref()
        else {
            return;
        };
        let width = field.size[0] as usize;
        let depth = field.size[2] as usize;
        let chunk = terrain_render_chunk_samples(1.0) as usize;
        let mut keys = HashSet::new();
        for &i in indices {
            if i >= width * depth {
                continue;
            }
            let x = i % width;
            let z = i / width;
            let x0 = x.saturating_sub(1) / chunk;
            let x1 = (x + 1).min(width - 1) / chunk;
            let z0 = z.saturating_sub(1) / chunk;
            let z1 = (z + 1).min(depth - 1) / chunk;
            for cz in z0..=z1 {
                for cx in x0..=x1 {
                    keys.insert([cx as u32, cz as u32]);
                }
            }
        }
        if keys.is_empty() {
            return;
        }
        self.mark_render_chunks_dirty(keys.into_iter().collect());
    }
    pub(crate) fn mark_authored_brush_dirty(&mut self, center: [f32; 3], radius: f32) {
        let Some(authored) = self.layered_authored.as_ref().or(self.authored.as_ref()) else {
            return;
        };
        if let TerrainData::Heightfield(field) = &authored.data {
            if !radius.is_finite() || radius <= 0.0 {
                return;
            }
            let x = center[0] - field.origin[0] as f32;
            let z = center[2] - field.origin[2] as f32;
            let border = 1.0;
            let max_x = field.size[0].saturating_sub(1) as f32;
            let max_z = field.size[2].saturating_sub(1) as f32;
            let x0 = ((x - radius - border).floor().max(0.0).min(max_x) as u32);
            let x1 = ((x + radius + border).ceil().max(0.0).min(max_x) as u32);
            let z0 = ((z - radius - border).floor().max(0.0).min(max_z) as u32);
            let z1 = ((z + radius + border).ceil().max(0.0).min(max_z) as u32);
            let chunk = terrain_render_chunk_samples(1.0);
            let affected = (z0 / chunk..=z1 / chunk)
                .flat_map(|cz| (x0 / chunk..=x1 / chunk).map(move |cx| [cx, cz]))
                .collect::<Vec<_>>();
            self.mark_render_chunks_dirty(affected);
            return;
        }
        let TerrainData::Voxel(grid) = &authored.data else {
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

        let mut keys = Vec::new();
        for chunk_z in (min_z / chunk_samples)..=(max_z / chunk_samples) {
            for chunk_x in (min_x / chunk_samples)..=(max_x / chunk_samples) {
                keys.push([chunk_x, chunk_z]);
            }
        }
        self.mark_render_chunks_dirty(keys);
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

    pub fn surface_height_at(&self, world_x: f32, world_z: f32) -> Option<f32> {
        let local_x = world_x - self.origin[0] as f32;
        let local_z = world_z - self.origin[2] as f32;
        if local_x < 0.0
            || local_z < 0.0
            || local_x >= self.size[0] as f32
            || local_z >= self.size[2] as f32
        {
            return None;
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

    pub fn apply_sculpt(&mut self, request: &TerrainSculptRequest) -> bool {
        if request.tool == SculptTool::Paint {
            return false;
        }
        let min_y = self.request.position[1] as f32;
        let max_y = (self.request.position[1] + self.request.size[1] - 1) as f32;
        match &mut self.data {
            TerrainData::Heightfield(heightfield) => {
                heightfield.apply_sculpt(request, min_y, max_y)
            }
            TerrainData::Voxel(grid) => grid.apply_surface_sculpt(request, min_y, max_y),
        }
    }

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

#[cfg(test)]
mod water_appearance_tests {
    #[test]
    fn water_realism_default_and_save_roundtrip_preserve_old_projects() {
        use super::*;
        let mut original = TerrainConfig::default();
        assert_eq!(original.water_realism, WaterRealismMode::Legacy);
        let legacy = serde_json::to_value(&original).unwrap();
        let mut old = legacy.as_object().unwrap().clone();
        old.remove("water_realism");
        let restored: TerrainConfig =
            serde_json::from_value(serde_json::Value::Object(old)).unwrap();
        assert_eq!(restored.water_realism, WaterRealismMode::Legacy);
        original.water_realism = WaterRealismMode::Basic;
        let roundtrip: TerrainConfig =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(roundtrip.water_realism, WaterRealismMode::Basic);
        assert_eq!(roundtrip.water, original.water);
    }

    use super::TerrainWaterSettings;
    #[test]
    fn old_four_field_water_serialization_keeps_original_appearance() {
        let json = r#"{"wave_height":0.5,"wave_speed":1.2,"foam_amount":0.7,"foam_scale":1.1}"#;
        let restored: TerrainWaterSettings = serde_json::from_str(json).unwrap();
        assert_eq!(restored.wave_height, 0.5);
        assert_eq!(restored.opacity, 0.78);
        assert_eq!(restored.shallow_color, [0.085, 0.47, 0.50]);
        restored.validate().unwrap();
    }
    #[test]
    fn user_colors_opacity_and_reflection_round_trip() {
        let sample = TerrainWaterSettings {
            shallow_color: [0.9, 0.3, 0.4],
            deep_color: [0.1, 0.2, 0.3],
            opacity: 0.35,
            reflection_strength: 1.7,
            ..TerrainWaterSettings::default()
        };
        let restored: TerrainWaterSettings =
            serde_json::from_str(&serde_json::to_string(&sample).unwrap()).unwrap();
        assert_eq!(restored, sample);
        restored.validate().unwrap();
        assert!(
            TerrainWaterSettings {
                opacity: f32::NAN,
                ..sample
            }
            .validate()
            .is_err()
        );
    }
}

#[cfg(test)]
mod terrain_mode_retirement_tests {
    use super::TerrainConfig;

    #[test]
    fn historical_block_and_smooth_config_values_load_without_restoring_mode() {
        for historical_mode in ["Block", "Smooth"] {
            let original = TerrainConfig::default();
            let mut json = serde_json::to_value(&original).unwrap();
            let fields = json.as_object_mut().unwrap();
            fields.insert("mode".to_owned(), serde_json::json!(historical_mode));
            let restored: TerrainConfig = serde_json::from_value(json).unwrap();
            assert_eq!(
                restored, original,
                "legacy mode must not alter authored generator or material configuration"
            );
            let rewritten = serde_json::to_value(&restored).unwrap();
            assert!(
                !rewritten.as_object().unwrap().contains_key("mode"),
                "retired mode must not be written to newly saved projects"
            );
        }
    }
}

#[cfg(test)]
mod ocean_optics_mode_tests {
    use super::{OceanOpticsMode, TerrainConfig};

    #[test]
    fn old_project_terrain_metadata_defaults_to_ray() {
        let mut json = serde_json::to_value(TerrainConfig::default()).unwrap();
        json.as_object_mut().unwrap().remove("ocean_optics_mode");
        let restored: TerrainConfig = serde_json::from_value(json).unwrap();
        assert_eq!(restored.ocean_optics_mode, OceanOpticsMode::Ray);
    }

    #[test]
    fn project_mode_roundtrips_and_has_distinct_shader_paths() {
        for (mode, id, name) in [
            (OceanOpticsMode::Ray, 2, "ray"),
            (OceanOpticsMode::Height, 1, "height"),
            (OceanOpticsMode::Off, 0, "off"),
        ] {
            let mut project = TerrainConfig::default();
            project.ocean_optics_mode = mode;
            let value = serde_json::to_value(&project).unwrap();
            assert_eq!(value["ocean_optics_mode"], name);
            let restored: TerrainConfig = serde_json::from_value(value).unwrap();
            assert_eq!(restored.ocean_optics_mode, mode);
            assert_eq!(restored.ocean_optics_mode.shader_id(), id);
            assert_eq!(
                restored.water, project.water,
                "project optics mode must not modify Water Material controls"
            );
        }
    }
}

#[cfg(test)]
mod underwater_postprocess_tests {
    use super::TerrainWaterSettings;

    #[test]
    fn legacy_water_json_inherits_historical_underwater_appearance() {
        let historical =
            r#"{"wave_height":1.0,"wave_speed":1.0,"foam_amount":1.0,"foam_scale":1.0}"#;
        let restored: TerrainWaterSettings = serde_json::from_str(historical).unwrap();
        assert!(restored.underwater_effects_enabled);
        assert_eq!(restored.underwater_fog_color, [0.055, 0.24, 0.32]);
        assert_eq!(restored.underwater_fog_density, 0.065);
        assert_eq!(restored.underwater_depth_fog, 0.018);
        assert_eq!(restored.underwater_depth_fog_limit, 0.32);
        assert_eq!(restored.underwater_fog_limit, 0.94);
        assert_eq!(restored.underwater_sky_tint, 0.88);
        restored.validate().unwrap();
    }

    #[test]
    fn fully_custom_underwater_postprocess_roundtrips_and_validates() {
        let changed = TerrainWaterSettings {
            underwater_effects_enabled: false,
            underwater_fog_color: [0.85, 0.1, 0.15],
            underwater_fog_density: 0.33,
            underwater_depth_fog: 0.06,
            underwater_depth_fog_limit: 0.4,
            underwater_fog_limit: 0.6,
            underwater_sky_tint: 0.25,
            ..TerrainWaterSettings::default()
        };
        let serialized = serde_json::to_string(&changed).unwrap();
        let restored: TerrainWaterSettings = serde_json::from_str(&serialized).unwrap();
        assert_eq!(restored, changed);
        restored.validate().unwrap();
        assert!(
            TerrainWaterSettings {
                underwater_fog_density: f32::INFINITY,
                ..changed
            }
            .validate()
            .is_err()
        );
        assert!(
            TerrainWaterSettings {
                underwater_fog_color: [-0.1, 0.5, 0.5],
                ..changed
            }
            .validate()
            .is_err()
        );
        assert!(
            TerrainWaterSettings {
                underwater_fog_limit: 1.2,
                ..changed
            }
            .validate()
            .is_err()
        );
    }
}

#[cfg(test)]
mod water_wave_optics_tests {
    use super::TerrainWaterSettings;

    #[test]
    fn gerstner_and_optical_settings_preserve_legacy_defaults() {
        let old = r#"{"wave_height":1.0,"wave_speed":1.0,"foam_amount":1.0,"foam_scale":1.0}"#;
        let restored: TerrainWaterSettings = serde_json::from_str(old).unwrap();
        assert_eq!(restored.wave_length, 50.0);
        assert_eq!(restored.wave_direction_degrees, 0.0);
        assert_eq!(restored.wave_steepness, 0.0);
        assert_eq!(restored.absorption_strength, 0.0);
        assert_eq!(restored.scattering_strength, 0.0);
        restored.validate().unwrap();
    }

    #[test]
    fn separate_water_wave_optics_settings_roundtrip_and_validate() {
        let original = TerrainWaterSettings {
            wave_length: 12.0,
            wave_direction_degrees: 60.0,
            wave_steepness: 0.8,
            absorption_color: [0.2, 0.4, 0.6],
            absorption_strength: 1.5,
            scattering_color: [0.7, 0.6, 0.5],
            scattering_strength: 1.1,
            ..TerrainWaterSettings::default()
        };
        original.validate().unwrap();
        let loaded: TerrainWaterSettings =
            serde_json::from_str(&serde_json::to_string(&original).unwrap()).unwrap();
        assert_eq!(loaded, original);
        let bad = TerrainWaterSettings {
            wave_length: 0.0,
            ..original
        };
        assert!(bad.validate().is_err());
        let bad = TerrainWaterSettings {
            wave_steepness: f32::NAN,
            ..original
        };
        assert!(bad.validate().is_err());
    }
}

#[cfg(test)]
mod grass_tint_retirement_tests {
    use super::TerrainMaterialBlend;

    #[test]
    fn old_grass_green_settings_are_ignored_and_no_longer_serialized() {
        let old = r#"{"rock_start":0.48,"rock_end":0.84,"dirt_strength":0.03,
                     "beach_width":3.5,"grass_green":1.0}"#;
        let blend: TerrainMaterialBlend = serde_json::from_str(old).unwrap();
        assert_eq!(blend, TerrainMaterialBlend::default());
        let serialized = serde_json::to_string(&blend).unwrap();
        assert!(!serialized.contains("grass_green"));
    }
}

#[cfg(test)]
mod terrain_edit_dirty_history_tests {
    use super::*;
    #[test]
    fn terrain_edit_perf_dirty_history_is_local_coalesced_and_bounded() {
        let mut terrain = TerrainState::default();
        terrain.mark_full_render_dirty();
        let initial = terrain.render_revision();
        terrain.mark_render_chunks_dirty(vec![[20000, 40000], [20000, 40000]]);
        terrain.mark_render_chunks_dirty(vec![[20001, 40000]]);
        assert_eq!(
            terrain.render_dirty_chunks_since(initial).unwrap(),
            vec![[20000, 40000], [20001, 40000]]
        );
        let recent = terrain.render_revision();
        for _ in 0..129 {
            terrain.mark_render_chunks_dirty(vec![[7, 9]]);
        }
        assert_eq!(terrain.render_dirty_history.len(), 128);
        assert!(terrain.render_dirty_chunks_since(recent).is_none());
        let previous = terrain.render_revision();
        terrain.mark_render_chunks_dirty(vec![[8, 9]]);
        assert_eq!(
            terrain.render_dirty_chunks_since(previous).unwrap(),
            vec![[8, 9]]
        );
        terrain.mark_full_render_dirty();
        assert!(terrain.render_dirty_chunks_since(previous).is_none());
        assert_eq!(
            terrain.render_dirty_chunks_since(terrain.render_revision()),
            Some(vec![])
        );
    }
}
