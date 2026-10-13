pub mod actor;
pub mod actor_hierarchy;
pub mod actor_modules;
pub mod block;
pub mod cell;
pub mod coordinate;
pub mod persistence;
pub mod source;
pub mod storage;
// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 11:32 -05:00
pub mod terrain;
pub mod world;

pub use actor::{Actor, ActorPhysicsConfig};
pub use actor_hierarchy::{ActorDirectory, ActorHierarchy};
pub use actor_modules::{ActorModuleIndex, ActorModuleRef};
pub use cell::{AttributeValue, CHUNK_SIZE, Cell, CellType, ChunkCoord};
pub use coordinate::WorldCoord;
pub use source::AuthoredSource;
pub use storage::{StoredChunk, StoredSource, WorldStorage};
// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 23:05 -05:00
// Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
// Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
// Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
pub use terrain::{
    DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS, HeightfieldData, LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
    MAX_GENERATION_VOLUME, PLAINS_GENERATOR_IMPLEMENTATION_VERSION, PlainsSettings,
    SMOOTH_VOXEL_ISO_LEVEL, SMOOTH_VOXEL_OCCUPANCY_MAX, SculptTool, SmoothVoxel, SmoothVoxelGrid,
    SmoothVoxelGridError, TERRAIN_CHUNK_SIZE, TERRAIN_RENDER_CHUNK_TARGET_STUDS, TerrainConfig,
    TerrainData, TerrainGenerationError, TerrainGenerationRequest, TerrainGeneratorType,
    TerrainGrassSettings, TerrainMaterials, TerrainMode, TerrainPaintMaterialSlot,
    TerrainPaintRequest, TerrainRenderChunkKey, TerrainResult, TerrainSculptRequest, TerrainState,
    TerrainWaterSettings, VoxelLayerSettings, apply_surface_color_map, apply_surface_material_map,
    generate_terrain, sculpt_preview_radii, voxelize_heightfield_layers,
    voxelize_heightfield_layers_seeded, voxelize_heightmap_layers,
};
pub use world::{
    ChunkCellIndex, DirtyReason, LightingSettings, RenderChange, RenderChanges, SkySettings, World,
    WorldSpatialIndex,
};
