// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
pub mod actor;
pub mod actor_hierarchy;
pub mod actor_modules;
pub mod block;
pub mod cell;
pub mod coordinate;
pub mod persistence;
pub mod source;
pub mod storage;
pub mod terrain;
pub mod world;

pub use actor::{Actor, ActorPhysicsConfig};
pub use actor_hierarchy::{ActorDirectory, ActorHierarchy};
pub use actor_modules::{ActorModuleIndex, ActorModuleRef};
pub use cell::{AttributeValue, CHUNK_SIZE, Cell, CellType, ChunkCoord};
pub use coordinate::WorldCoord;
pub use source::AuthoredSource;
pub use storage::{StoredChunk, StoredSource, WorldStorage};
pub use terrain::{
    DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS, HeightfieldData, LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
    MAX_GENERATION_VOLUME, OceanOpticsMode, PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
    PlainsSettings, SMOOTH_VOXEL_ISO_LEVEL, SMOOTH_VOXEL_OCCUPANCY_MAX, SculptTool, SmoothVoxel,
    SmoothVoxelGrid, SmoothVoxelGridError, TERRAIN_CHUNK_SIZE, TERRAIN_RENDER_CHUNK_TARGET_STUDS,
    TerrainConfig, TerrainData, TerrainGenerationError, TerrainGenerationRequest,
    TerrainGeneratorType, TerrainGrassSettings, TerrainMaterials, TerrainPaintMaterialSlot,
    TerrainPaintRequest, TerrainRenderChunkKey, TerrainResult, TerrainSculptRequest, TerrainState,
    TerrainWaterSettings, VoxelLayerSettings, apply_surface_color_map, apply_surface_material_map,
    generate_terrain, sculpt_preview_radii, voxelize_heightfield_layers,
    voxelize_heightfield_layers_seeded, voxelize_heightmap_layers,
};
pub use world::{
    ChunkCellIndex, DirtyReason, LightingSettings, RenderChange, RenderChanges, SkySettings, World,
    WorldSpatialIndex,
};
