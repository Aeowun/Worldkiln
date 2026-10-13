use serde::{Deserialize, Serialize};

// The explicit enum definitions below keep the default choice obvious at the
// declaration site and give the Settings Workspace stable values to bind to.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingMethod {
    #[default]
    Legacy,
    TwoLevelRadianceCache,
    SplitRadianceCascades,
}

impl LightingMethod {
    pub const ALL: &'static [Self] = &[
        Self::Legacy,
        Self::TwoLevelRadianceCache,
        Self::SplitRadianceCascades,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Legacy => "Legacy",
            Self::TwoLevelRadianceCache => "Two-Level Radiance Cache",
            Self::SplitRadianceCascades => "Split Radiance Cascades",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ShadowMethod {
    Off,
    #[default]
    LegacyShadowMap,
    CachedDirectionalClipmaps,
}

impl ShadowMethod {
    pub const ALL: &'static [Self] = &[
        Self::Off,
        Self::LegacyShadowMap,
        Self::CachedDirectionalClipmaps,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::LegacyShadowMap => "Legacy Shadow Map",
            Self::CachedDirectionalClipmaps => "Cached Directional Clipmaps",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightingQuality {
    Performance,
    #[default]
    Balanced,
    Quality,
    Custom,
}

impl LightingQuality {
    pub const ALL: &'static [Self] = &[
        Self::Performance,
        Self::Balanced,
        Self::Quality,
        Self::Custom,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance",
            Self::Balanced => "Balanced",
            Self::Quality => "Quality",
            Self::Custom => "Custom",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum IndirectDiffuse {
    #[default]
    Off,
    On,
}

impl IndirectDiffuse {
    pub const ALL: &'static [Self] = &[Self::Off, Self::On];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum BounceLighting {
    #[default]
    SingleBounce,
    MultipleBounce,
}

impl BounceLighting {
    pub const ALL: &'static [Self] = &[Self::SingleBounce, Self::MultipleBounce];
    pub const fn label(self) -> &'static str {
        match self {
            Self::SingleBounce => "Single Bounce",
            Self::MultipleBounce => "Multiple Bounce",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum TemporalReuse {
    #[default]
    Off,
    On,
}

impl TemporalReuse {
    pub const ALL: &'static [Self] = &[Self::Off, Self::On];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::On => "On",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DynamicGeometryGi {
    #[default]
    Off,
    BodiesOnly,
    BodiesActorsCharacters,
}

impl DynamicGeometryGi {
    pub const ALL: &'static [Self] = &[Self::Off, Self::BodiesOnly, Self::BodiesActorsCharacters];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::BodiesOnly => "Bodies Only",
            Self::BodiesActorsCharacters => "Bodies + Actors + Characters",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GiResolution {
    Quarter,
    #[default]
    Half,
    Full,
}

impl GiResolution {
    pub const ALL: &'static [Self] = &[Self::Quarter, Self::Half, Self::Full];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Quarter => "Quarter",
            Self::Half => "Half",
            Self::Full => "Full",
        }
    }

    pub const fn divisor(self) -> u32 {
        match self {
            Self::Quarter => 4,
            Self::Half => 2,
            Self::Full => 1,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SceneQueryBackend {
    #[default]
    Automatic,
    ChunkGridDda,
    OccupiedChunkBvh,
}

impl SceneQueryBackend {
    pub const ALL: &'static [Self] = &[Self::Automatic, Self::ChunkGridDda, Self::OccupiedChunkBvh];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Automatic => "Automatic",
            Self::ChunkGridDda => "Chunk Grid DDA",
            Self::OccupiedChunkBvh => "Occupied-Chunk BVH",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum CacheUpdateRate {
    Low,
    #[default]
    Medium,
    High,
    EveryFrame,
}

impl CacheUpdateRate {
    pub const ALL: &'static [Self] = &[Self::Low, Self::Medium, Self::High, Self::EveryFrame];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Low => "Low",
            Self::Medium => "Medium",
            Self::High => "High",
            Self::EveryFrame => "Every Frame",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Denoising {
    #[default]
    Off,
    Spatial,
    SpatialTemporal,
}

impl Denoising {
    pub const ALL: &'static [Self] = &[Self::Off, Self::Spatial, Self::SpatialTemporal];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Spatial => "Spatial",
            Self::SpatialTemporal => "Spatial + Temporal",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[repr(i32)]
pub enum DebugView {
    #[default]
    FinalLighting = 0,
    DirectOnly = 1,
    IndirectOnly = 2,
    Albedo = 3,
    Normals = 4,
    Depth = 5,
    ScreenProbes = 6,
    WorldCache = 7,
    RayHits = 8,
    CacheInvalidations = 9,
}

impl DebugView {
    pub const ALL: &'static [Self] = &[
        Self::FinalLighting,
        Self::DirectOnly,
        Self::IndirectOnly,
        Self::Albedo,
        Self::Normals,
        Self::Depth,
        Self::ScreenProbes,
        Self::WorldCache,
        Self::RayHits,
        Self::CacheInvalidations,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::FinalLighting => "Final Lighting",
            Self::DirectOnly => "Direct Only",
            Self::IndirectOnly => "Indirect Only",
            Self::Albedo => "Albedo",
            Self::Normals => "Normals",
            Self::Depth => "Depth",
            Self::ScreenProbes => "Screen Probes",
            Self::WorldCache => "World Cache",
            Self::RayHits => "Ray Hits",
            Self::CacheInvalidations => "Cache Invalidations",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum PerformanceDisplay {
    #[default]
    Off,
    FrameTime,
    PassTimings,
    RaysAndCacheStatistics,
}

impl PerformanceDisplay {
    pub const ALL: &'static [Self] = &[
        Self::Off,
        Self::FrameTime,
        Self::PassTimings,
        Self::RaysAndCacheStatistics,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::FrameTime => "Frame Time",
            Self::PassTimings => "Pass Timings",
            Self::RaysAndCacheStatistics => "Rays and Cache Statistics",
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ApplyBehavior {
    #[default]
    ApplyNowWhenSupported,
    RestartRequired,
}

impl ApplyBehavior {
    pub const ALL: &'static [Self] = &[Self::ApplyNowWhenSupported, Self::RestartRequired];
    pub const fn label(self) -> &'static str {
        match self {
            Self::ApplyNowWhenSupported => "Apply Now when supported",
            Self::RestartRequired => "Restart Required",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct RendererLightingSettings {
    pub lighting_method: LightingMethod,
    pub shadow_method: ShadowMethod,
    pub lighting_quality: LightingQuality,
    pub indirect_diffuse: IndirectDiffuse,
    pub bounce_lighting: BounceLighting,
    pub temporal_reuse: TemporalReuse,
    pub dynamic_geometry_gi: DynamicGeometryGi,
    pub gi_resolution: GiResolution,
    pub scene_query_backend: SceneQueryBackend,
    pub cache_update_rate: CacheUpdateRate,
    pub denoising: Denoising,
    pub debug_view: DebugView,
    pub performance_display: PerformanceDisplay,
    pub apply_behavior: ApplyBehavior,
}

impl Default for RendererLightingSettings {
    fn default() -> Self {
        Self {
            lighting_method: LightingMethod::Legacy,
            shadow_method: ShadowMethod::LegacyShadowMap,
            lighting_quality: LightingQuality::Balanced,
            indirect_diffuse: IndirectDiffuse::Off,
            bounce_lighting: BounceLighting::SingleBounce,
            temporal_reuse: TemporalReuse::Off,
            dynamic_geometry_gi: DynamicGeometryGi::Off,
            gi_resolution: GiResolution::Half,
            scene_query_backend: SceneQueryBackend::Automatic,
            cache_update_rate: CacheUpdateRate::Medium,
            denoising: Denoising::Off,
            debug_view: DebugView::FinalLighting,
            performance_display: PerformanceDisplay::Off,
            apply_behavior: ApplyBehavior::ApplyNowWhenSupported,
        }
    }
}

impl RendererLightingSettings {
    pub fn unsupported_reason(self) -> Option<&'static str> {
        if self.lighting_method == LightingMethod::SplitRadianceCascades {
            return Some(
                "Split Radiance Cascades is exposed for the beta research plan but is not implemented in this build.",
            );
        }
        if self.shadow_method == ShadowMethod::CachedDirectionalClipmaps {
            return Some(
                "Cached Directional Clipmaps is exposed for the beta research plan but is not implemented in this build.",
            );
        }
        if self.dynamic_geometry_gi == DynamicGeometryGi::BodiesActorsCharacters {
            return Some(
                "Actor and Character GI is not implemented yet. This build supports Off and Bodies Only.",
            );
        }
        if matches!(
            self.debug_view,
            DebugView::ScreenProbes | DebugView::RayHits | DebugView::CacheInvalidations
        ) {
            return Some(
                "The selected lighting debug view does not have a renderer output in this build.",
            );
        }
        if self.performance_display == PerformanceDisplay::PassTimings {
            return Some("The in-app GPU pass timing display is not implemented yet.");
        }
        None
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RendererLightingState {
    pub active: RendererLightingSettings,
    pub requested: RendererLightingSettings,
    pub restart_pending: bool,
    pub last_error: Option<String>,
}

impl Default for RendererLightingState {
    fn default() -> Self {
        let settings = RendererLightingSettings::default();
        Self {
            active: settings,
            requested: settings,
            restart_pending: false,
            last_error: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ApplyResult {
    Applied,
    RestartRequired,
    Unsupported(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_preserve_the_legacy_renderer() {
        let settings = RendererLightingSettings::default();
        assert_eq!(settings.lighting_method, LightingMethod::Legacy);
        assert_eq!(settings.shadow_method, ShadowMethod::LegacyShadowMap);
        assert_eq!(settings.indirect_diffuse, IndirectDiffuse::Off);
        assert_eq!(settings.debug_view, DebugView::FinalLighting);
    }

    #[test]
    fn every_menu_has_the_expected_number_of_options() {
        assert_eq!(LightingMethod::ALL.len(), 3);
        assert_eq!(ShadowMethod::ALL.len(), 3);
        assert_eq!(LightingQuality::ALL.len(), 4);
        assert_eq!(DebugView::ALL.len(), 10);
    }

    #[test]
    fn completed_beta_slice_accepts_supported_combinations() {
        let settings = RendererLightingSettings {
            lighting_method: LightingMethod::TwoLevelRadianceCache,
            indirect_diffuse: IndirectDiffuse::On,
            scene_query_backend: SceneQueryBackend::OccupiedChunkBvh,
            debug_view: DebugView::IndirectOnly,
            performance_display: PerformanceDisplay::RaysAndCacheStatistics,
            ..RendererLightingSettings::default()
        };
        assert_eq!(settings.unsupported_reason(), None);
    }

    #[test]
    fn unfinished_beta_methods_are_rejected_with_specific_reasons() {
        let split = RendererLightingSettings {
            lighting_method: LightingMethod::SplitRadianceCascades,
            ..RendererLightingSettings::default()
        };
        assert!(
            split
                .unsupported_reason()
                .unwrap()
                .contains("not implemented")
        );

        let clipmaps = RendererLightingSettings {
            shadow_method: ShadowMethod::CachedDirectionalClipmaps,
            ..RendererLightingSettings::default()
        };
        assert!(
            clipmaps
                .unsupported_reason()
                .unwrap()
                .contains("not implemented")
        );
    }
}
