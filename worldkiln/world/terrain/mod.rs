//! World-level Terrain contracts and generator dispatch.
//!
//! Terrain is independent from Cells and rendering. Generator output is a
//! representation-neutral logical payload consumed by later systems.

mod plains;
// Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
mod layers;
// Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
pub use layers::{GenerationLayer, GeneratorSettings, Landform, LayerBrushEdit};
pub mod storage;
mod types;
// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:08 -05:00
mod voxel;

// Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
pub(crate) use types::terrain_render_chunk_samples;
pub use types::{
    HeightfieldData, MAX_GENERATION_VOLUME, PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
    PlainsSettings, SCULPT_STRENGTH_UI_MAX, SculptTool, TERRAIN_CHUNK_SIZE,
    TERRAIN_RENDER_CHUNK_TARGET_STUDS, TerrainConfig, TerrainData, TerrainGenerationError,
    TerrainGenerationRequest, TerrainGeneratorType, TerrainGrassSettings, TerrainMaterials,
    TerrainMode, TerrainPaintMaterialSlot, TerrainPaintRequest, TerrainRenderChunkKey,
    TerrainResult, TerrainSculptRequest, TerrainState, TerrainWaterSettings, sculpt_preview_radii,
};

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:08 -05:00
pub use voxel::{
    DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS, LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
    SMOOTH_VOXEL_ISO_LEVEL, SMOOTH_VOXEL_OCCUPANCY_MAX, SmoothVoxel, SmoothVoxelGrid,
    SmoothVoxelGridError, VoxelLayerSettings, apply_surface_color_map, apply_surface_material_map,
    voxelize_heightfield_layers, voxelize_heightfield_layers_seeded, voxelize_heightmap_layers,
};
// Worldkiln 0.9.3 | WO 0.9.3-06 | 2026-10-06 23:14 -05:00
pub(crate) use voxel::{
    BEACH_ABOVE_WATER_STUDS, TERRAIN_FALLBACK_ROCK_MATERIAL, TERRAIN_FALLBACK_SUBSURFACE_MATERIAL,
    TERRAIN_FALLBACK_SURFACE_MATERIAL, apply_waterline_beach_material,
};

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 11:32 -05:00
/// Generates logical Terrain through the selected generator implementation.
pub fn generate_terrain(
    config: &TerrainConfig,
    request: TerrainGenerationRequest,
) -> Result<TerrainResult, TerrainGenerationError> {
    request.validate()?;
    config.validate()?;

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:50 -05:00
    let generated = match config.generator {
        TerrainGeneratorType::Plains => plains::generate(&config.plains, request)?,
        TerrainGeneratorType::Programmatic => {
            return Err(TerrainGenerationError::InvalidSetting {
                setting: "Generator",
                reason: "Programmatic Terrain must be supplied as voxel data",
            });
        }
    };

    let mut generated = generated;
    // Worldkiln 0.9.3 | WO 0.9.3-09 | 2026-10-07 00:24 -05:00
    let mut grid = match &generated.data {
        TerrainData::Heightfield(heightfield) => voxelize_heightfield_layers_seeded(
            heightfield,
            &config.materials,
            config.voxel_layers,
            config.plains.seed,
        )
        .map_err(|error| TerrainGenerationError::VoxelData(error.to_string()))?,
        TerrainData::Voxel(_) => return Ok(generated),
    };
    // Worldkiln 0.9.3 | WO 0.9.3-17 | 2026-10-07
    // Sand replaces only Surface/Subsurface roles for shoreline and submerged
    // columns. Rock remains untouched.
    let surface_material = if config.materials.surface.trim().is_empty() {
        TERRAIN_FALLBACK_SURFACE_MATERIAL
    } else {
        config.materials.surface.as_str()
    };
    let subsurface_material = if config.materials.subsurface.trim().is_empty() {
        TERRAIN_FALLBACK_SUBSURFACE_MATERIAL
    } else {
        config.materials.subsurface.as_str()
    };
    let rock_material = if config.materials.rock.trim().is_empty() {
        TERRAIN_FALLBACK_ROCK_MATERIAL
    } else {
        config.materials.rock.as_str()
    };
    apply_waterline_beach_material(
        &mut grid,
        config.plains.water_level,
        "sand",
        surface_material,
        subsurface_material,
        rock_material,
    );
    generated.data = TerrainData::Voxel(grid);
    Ok(generated)
}

impl TerrainState {
    pub fn generate_authored(
        &mut self,
        request: TerrainGenerationRequest,
    ) -> Result<(), TerrainGenerationError> {
        let generated = generate_terrain(&self.config, request)?;
        // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
        self.replace_authored(Some(generated));
        Ok(())
    }

    pub fn generate_runtime(
        &mut self,
        request: TerrainGenerationRequest,
    ) -> Result<(), TerrainGenerationError> {
        let generated = generate_terrain(&self.config, request)?;
        // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
        self.replace_runtime(Some(generated));
        Ok(())
    }

    pub fn rebuild_representation(&mut self, mode: TerrainMode) {
        self.config.mode = mode;
    }

    pub fn surface_height_at(&self, world_x: f32, world_z: f32) -> Option<f32> {
        let result = self.effective()?;
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:50 -05:00
        let min_x = result.request.position[0] as f32;
        let min_z = result.request.position[2] as f32;
        let max_x = min_x + result.request.size[0] as f32;
        let max_z = min_z + result.request.size[2] as f32;
        if world_x < min_x || world_x >= max_x || world_z < min_z || world_z >= max_z {
            return None;
        }
        match &result.data {
            TerrainData::Heightfield(heightfield) => {
                heightfield.surface_height_at(world_x, world_z, self.config.mode)
            }
            TerrainData::Voxel(grid) => grid.surface_height_at(world_x, world_z, self.config.mode),
        }
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    pub fn sculpt_authored(&mut self, request: &TerrainSculptRequest) -> bool {
        // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09
        // Generated layers have a separate edit API; never send their brushes
        // to the legacy authored base without an explicit layer selection.
        if self
            .config
            .generation_layers
            .iter()
            .any(|layer| layer.applied.is_some())
        {
            return false;
        }
        let changed = if let Some(ref mut authored) = self.authored {
            authored.apply_sculpt(request)
        } else {
            false
        };
        if changed {
            // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:20 -05:00
            // Re-evaluate procedural slope/altitude only around the brush,
            // without replacing custom-painted voxel materials.
            if let Some(TerrainResult {
                data: TerrainData::Voxel(grid),
                ..
            }) = self.authored.as_mut()
            {
                grid.refresh_local_slope_materials(
                    request.center,
                    request.radius,
                    &self.config.materials,
                    self.config.voxel_layers,
                    self.config.plains.seed,
                    self.config.plains.water_level,
                );
            }
            // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
            if self
                .config
                .generation_layers
                .iter()
                .any(|layer| layer.applied.is_some())
            {
                let _ = self.rebuild_generation_layers();
            } else {
                self.mark_authored_brush_dirty(request.center, request.radius);
            }
        }
        changed
    }

    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    pub fn paint_authored(&mut self, request: &TerrainPaintRequest) -> bool {
        // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09
        if self
            .config
            .generation_layers
            .iter()
            .any(|layer| layer.applied.is_some())
        {
            return false;
        }
        let changed = if let Some(ref mut authored) = self.authored {
            authored.apply_paint(request)
        } else {
            false
        };
        if changed {
            // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:45 -05:00
            if self
                .config
                .generation_layers
                .iter()
                .any(|layer| layer.applied.is_some())
            {
                let _ = self.rebuild_generation_layers();
            } else {
                self.mark_authored_brush_dirty(request.center, request.radius);
            }
        }
        changed
    }
}

// Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:50 -05:00
fn programmatic_voxel_result(
    origin: [i32; 3],
    size: [u32; 3],
    spacing_studs: f32,
    materials: Vec<String>,
    voxels: Vec<SmoothVoxel>,
) -> Result<TerrainResult, TerrainGenerationError> {
    let grid =
        SmoothVoxelGrid::from_dense_with_spacing(origin, size, spacing_studs, materials, voxels)
            .map_err(|error| TerrainGenerationError::VoxelData(error.to_string()))?;
    let mut request_size = [0_i32; 3];
    for axis in 0..3 {
        let studs = size[axis] as f64 * spacing_studs as f64;
        if !studs.is_finite() || studs > i32::MAX as f64 {
            return Err(TerrainGenerationError::VolumeOverflow);
        }
        request_size[axis] = studs.ceil() as i32;
    }
    let request = TerrainGenerationRequest {
        position: origin,
        size: request_size,
    };
    request.validate()?;
    Ok(TerrainResult {
        request,
        generator: TerrainGeneratorType::Programmatic,
        generator_implementation_version: 1,
        data: TerrainData::Voxel(grid),
    })
}

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
impl crate::world::World {
    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    pub fn sculpt_authored_terrain(&mut self, request: &TerrainSculptRequest) -> bool {
        self.terrain.sculpt_authored(request)
    }

    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    pub fn paint_authored_terrain(&mut self, request: &TerrainPaintRequest) -> bool {
        self.terrain.paint_authored(request)
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:50 -05:00
    pub fn set_runtime_voxel_terrain(
        &mut self,
        origin: [i32; 3],
        size: [u32; 3],
        materials: Vec<String>,
        voxels: Vec<SmoothVoxel>,
    ) -> Result<(), TerrainGenerationError> {
        self.set_runtime_voxel_terrain_with_spacing(
            origin,
            size,
            LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
            materials,
            voxels,
        )
    }

    pub fn set_runtime_voxel_terrain_with_spacing(
        &mut self,
        origin: [i32; 3],
        size: [u32; 3],
        spacing_studs: f32,
        materials: Vec<String>,
        voxels: Vec<SmoothVoxel>,
    ) -> Result<(), TerrainGenerationError> {
        let result = programmatic_voxel_result(origin, size, spacing_studs, materials, voxels)?;
        if self.terrain.config.plains.destroy_cells {
            let overlapping_ids: Vec<_> = self
                .iter_active_effective_coords()
                .filter(|&coord| result.solid_occupies_cell(coord))
                .filter_map(|coord| self.get_effective_cell(coord).map(|cell| cell.id))
                .collect();
            for id in overlapping_ids {
                self.delete_cell_runtime(id);
            }
        }
        self.terrain.replace_runtime(Some(result));
        Ok(())
    }

    pub fn set_authored_voxel_terrain(
        &mut self,
        origin: [i32; 3],
        size: [u32; 3],
        materials: Vec<String>,
        voxels: Vec<SmoothVoxel>,
    ) -> Result<(), TerrainGenerationError> {
        self.set_authored_voxel_terrain_with_spacing(
            origin,
            size,
            LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
            materials,
            voxels,
        )
    }

    pub fn set_authored_voxel_terrain_with_spacing(
        &mut self,
        origin: [i32; 3],
        size: [u32; 3],
        spacing_studs: f32,
        materials: Vec<String>,
        voxels: Vec<SmoothVoxel>,
    ) -> Result<(), TerrainGenerationError> {
        let result = programmatic_voxel_result(origin, size, spacing_studs, materials, voxels)?;
        if self.terrain.config.plains.destroy_cells {
            let overlapping: Vec<_> = self
                .iter_active_effective_coords()
                .filter(|&coord| self.get(coord).is_some() && result.solid_occupies_cell(coord))
                .collect();
            for coord in overlapping {
                self.set_cell(coord, crate::world::CellType::Empty);
            }
        }
        self.terrain.replace_authored(Some(result));
        Ok(())
    }
    /// Generates authored Terrain and applies optional overlap deletion through
    /// the normal authored Cell mutation path.
    pub fn generate_authored_terrain(
        &mut self,
        request: TerrainGenerationRequest,
    ) -> Result<(), TerrainGenerationError> {
        let generated = generate_terrain(&self.terrain.config, request)?;
        if self.terrain.config.plains.destroy_cells {
            let overlapping: Vec<_> = self
                .iter_active_effective_coords()
                .filter(|&coord| self.get(coord).is_some() && generated.solid_occupies_cell(coord))
                .collect();
            for coord in overlapping {
                self.set_cell(coord, crate::world::CellType::Empty);
            }
        }
        self.terrain.replace_authored(Some(generated));
        Ok(())
    }

    /// Generates session-only Terrain and applies overlap deletion only as
    /// runtime overrides, preserving authored Cells for the next session.
    pub fn generate_runtime_terrain(
        &mut self,
        request: TerrainGenerationRequest,
    ) -> Result<(), TerrainGenerationError> {
        let generated = generate_terrain(&self.terrain.config, request)?;
        if self.terrain.config.plains.destroy_cells {
            let overlapping_ids: Vec<_> = self
                .iter_active_effective_coords()
                .filter(|&coord| generated.solid_occupies_cell(coord))
                .filter_map(|coord| self.get_effective_cell(coord).map(|cell| cell.id))
                .collect();
            for id in overlapping_ids {
                self.delete_cell_runtime(id);
            }
        }
        self.terrain.replace_runtime(Some(generated));
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{CellType, World, WorldCoord};

    fn request(position: [i32; 3], size: [i32; 3]) -> TerrainGenerationRequest {
        TerrainGenerationRequest { position, size }
    }

    fn generated_heightfield(seed: i32) -> HeightfieldData {
        let config = TerrainConfig {
            plains: PlainsSettings {
                seed,
                base_height: 12.0,
                height_variation: 7.0,
                noise_scale: 0.025,
                water_level: 8.0,
                destroy_cells: false,
            },
            ..TerrainConfig::default()
        };

        let result = plains::generate(&config.plains, request([-8, -4, 5], [9, 32, 7])).unwrap();
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
        let TerrainData::Heightfield(heightfield) = result.data else {
            panic!("expected heightfield Terrain in legacy regression");
        };
        heightfield
    }

    fn overlap_test_world(destroy_cells: bool) -> World {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 4.0;
        world.terrain.config.plains.height_variation = 0.0;
        world.terrain.config.plains.destroy_cells = destroy_cells;
        world.set_cell(WorldCoord::new(1, 1, 1), CellType::Block);
        world.set_cell(WorldCoord::new(1, 5, 1), CellType::Block);
        world.set_cell(WorldCoord::new(9, 1, 1), CellType::Block);
        world
    }

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    // Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
    #[test]
    fn direct_dirt_paint_uses_builtin_dirt_even_with_custom_subsurface() {
        let mut materials = TerrainMaterials::default();
        assert_eq!(
            TerrainPaintMaterialSlot::Dirt.resolve_material(&materials),
            "dirt"
        );
        materials.subsurface = "player_custom_soil".into();
        assert_eq!(
            TerrainPaintMaterialSlot::Dirt.resolve_material(&materials),
            "dirt"
        );
        assert_eq!(
            TerrainPaintMaterialSlot::Subsurface.resolve_material(&materials),
            "player_custom_soil"
        );
        assert_eq!(
            TerrainPaintMaterialSlot::Sand.resolve_material(&materials),
            "sand"
        );
    }

    #[test]
    fn authored_terrain_paint_changes_material_without_shape() {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 8.0;
        world.terrain.config.plains.height_variation = 0.0;
        world
            .generate_authored_terrain(request([0, 0, 0], [16, 16, 16]))
            .unwrap();

        let before_height = world.terrain.surface_height_at(6.0, 6.0).unwrap();
        assert!(world.paint_authored_terrain(&TerrainPaintRequest {
            center: [6.0, before_height, 6.0],
            radius: 3.0,
            material: "tile_24".into(),
        }));
        assert_eq!(
            before_height,
            world.terrain.surface_height_at(6.0, 6.0).unwrap()
        );

        let TerrainData::Voxel(grid) = &world.terrain.authored.as_ref().unwrap().data else {
            panic!("expected voxel Terrain");
        };
        let rock = grid
            .materials
            .iter()
            .position(|name| name == "tile_24")
            .unwrap() as u16;
        let painted = (0..grid.size[1])
            .rev()
            .find_map(|y| grid.voxel(1, y, 1).filter(|voxel| !voxel.is_empty()))
            .unwrap();
        assert_eq!(painted.material, rock);
    }

    // Worldkiln 0.9.3 | WO 0.9.3-12 | 2026-10-07 02:05 -05:00
    #[test]
    fn terrain_revision_is_o1_and_brush_edits_dirty_local_render_chunks() {
        let mut world = World::new();
        world.terrain.config.plains.base_height = 8.0;
        world.terrain.config.plains.height_variation = 0.0;
        world
            .generate_authored_terrain(request([0, 0, 0], [192, 32, 192]))
            .unwrap();

        let generated_revision = world.terrain.render_revision();
        assert!(generated_revision > 0);
        assert_eq!(world.terrain.full_render_revision(), generated_revision);

        assert!(world.paint_authored_terrain(&TerrainPaintRequest {
            center: [70.0, 8.0, 70.0],
            radius: 3.0,
            material: "tile_24".into(),
        }));
        let edited_revision = world.terrain.render_revision();
        assert!(edited_revision > generated_revision);
        assert_eq!(world.terrain.full_render_revision(), generated_revision);

        let chunk_samples = terrain_render_chunk_samples(DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS);
        let edited_key = [
            (70.0_f32 / DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS).floor() as u32 / chunk_samples,
            (70.0_f32 / DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS).floor() as u32 / chunk_samples,
        ];
        assert_eq!(
            world.terrain.render_chunk_revision(edited_key),
            edited_revision
        );
        assert_eq!(world.terrain.render_chunk_revision([2, 2]), 0);
    }

    #[test]
    fn terrain_replacement_marks_full_renderer_revision() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 0, 0], [16, 16, 16]))
            .unwrap();
        let first = world.terrain.render_revision();

        world
            .generate_authored_terrain(request([32, 0, 32], [16, 16, 16]))
            .unwrap();
        assert!(world.terrain.render_revision() > first);
        assert_eq!(
            world.terrain.full_render_revision(),
            world.terrain.render_revision()
        );
    }

    #[test]
    fn destroy_cells_off_preserves_overlapping_authored_cells() {
        let mut world = overlap_test_world(false);
        world
            .generate_authored_terrain(request([0, 0, 0], [4, 8, 4]))
            .unwrap();

        assert!(world.get(WorldCoord::new(1, 1, 1)).is_some());
        assert_eq!(world.active_effective_blocks().len(), 3);
    }

    #[test]
    fn destroy_cells_on_removes_only_solid_authored_overlaps() {
        let mut world = overlap_test_world(true);
        world
            .generate_authored_terrain(request([0, 0, 0], [4, 8, 4]))
            .unwrap();

        assert!(world.get(WorldCoord::new(1, 1, 1)).is_none());
        assert!(world.get(WorldCoord::new(1, 5, 1)).is_some());
        assert!(world.get(WorldCoord::new(9, 1, 1)).is_some());
    }

    #[test]
    fn runtime_destroy_cells_restores_authored_cells_after_session_clear() {
        let mut world = overlap_test_world(true);
        let overlap = WorldCoord::new(1, 1, 1);
        world
            .generate_runtime_terrain(request([0, 0, 0], [4, 8, 4]))
            .unwrap();

        assert!(world.get(overlap).is_some());
        assert!(world.get_effective_cell(overlap).is_none());
        world.clear_runtime_state();
        assert!(world.get_effective_cell(overlap).is_some());
        assert!(world.terrain.runtime.is_none());
    }

    #[test]
    fn rejects_zero_and_negative_sizes() {
        for size in [[0, 1, 1], [1, 0, 1], [1, 1, 0], [-1, 1, 1]] {
            let error = request([0, 0, 0], size).validate().unwrap_err();
            assert!(matches!(
                error,
                TerrainGenerationError::NonPositiveSize { .. }
            ));
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-07 | 2026-10-07 00:02 -05:00
    #[test]
    fn accepts_volume_limit_and_rejects_larger_or_overflowing_requests() {
        assert_eq!(request([0, 0, 0], [256, 256, 256]).volume(), Ok(16_777_216));
        assert!(matches!(
            request([0, 0, 0], [257, 256, 256]).volume(),
            Err(TerrainGenerationError::VolumeLimitExceeded { volume: 16_842_752 })
        ));
        assert!(matches!(
            request([0, 0, 0], [i32::MAX, i32::MAX, i32::MAX]).volume(),
            Err(TerrainGenerationError::VolumeOverflow)
        ));
    }

    #[test]
    fn default_terrain_materials_match_slice_closeout_profile() {
        assert_eq!(
            TerrainMaterials::default(),
            TerrainMaterials {
                surface: "grass_1".into(),
                subsurface: "dirt".into(),
                rock: "tile_24".into(),
                water: "water".into(),
            }
        );
    }

    // Worldkiln 0.9.3 | WO 0.9.3-08 | 2026-10-07 11:00 -05:00
    #[test]
    fn procedural_grass_defaults_and_missing_serialized_field_are_stable() {
        let defaults = TerrainGrassSettings::default();
        assert!(defaults.enabled);
        assert!((defaults.density - 0.45).abs() < f32::EPSILON);
        assert!((defaults.length_studs - 1.25).abs() < f32::EPSILON);
        defaults.validate().unwrap();

        let mut value = serde_json::to_value(TerrainConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("grass");
        let decoded: TerrainConfig = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.grass, defaults);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    #[test]
    fn water_controls_default_when_loading_legacy_terrain_config() {
        let defaults = TerrainWaterSettings::default();
        let mut value = serde_json::to_value(TerrainConfig::default()).unwrap();
        value.as_object_mut().unwrap().remove("water");
        let decoded: TerrainConfig = serde_json::from_value(value).unwrap();
        assert_eq!(decoded.water, defaults);
        defaults.validate().unwrap();
    }

    #[test]
    fn water_controls_reject_non_finite_and_out_of_range_values() {
        for water in [
            TerrainWaterSettings {
                wave_height: -0.01,
                ..TerrainWaterSettings::default()
            },
            TerrainWaterSettings {
                wave_speed: f32::NAN,
                ..TerrainWaterSettings::default()
            },
            TerrainWaterSettings {
                foam_amount: 2.01,
                ..TerrainWaterSettings::default()
            },
            TerrainWaterSettings {
                foam_scale: 0.24,
                ..TerrainWaterSettings::default()
            },
        ] {
            let config = TerrainConfig {
                water,
                ..TerrainConfig::default()
            };
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn rejects_invalid_procedural_grass_settings() {
        for grass in [
            TerrainGrassSettings {
                density: -0.01,
                ..TerrainGrassSettings::default()
            },
            TerrainGrassSettings {
                density: 1.01,
                ..TerrainGrassSettings::default()
            },
            TerrainGrassSettings {
                length_studs: 0.0,
                ..TerrainGrassSettings::default()
            },
            TerrainGrassSettings {
                length_studs: 4.01,
                ..TerrainGrassSettings::default()
            },
        ] {
            let config = TerrainConfig {
                grass,
                ..TerrainConfig::default()
            };
            assert!(config.validate().is_err());
        }
    }

    #[test]
    fn rejects_coordinate_bounds_overflow() {
        assert!(matches!(
            request([i32::MAX, 0, 0], [2, 1, 1]).validate(),
            Err(TerrainGenerationError::BoundsOverflow { axis: "x" })
        ));
    }

    #[test]
    fn rejects_non_finite_and_invalid_plains_settings() {
        let invalid_settings = [
            PlainsSettings {
                base_height: f32::NAN,
                ..PlainsSettings::default()
            },
            PlainsSettings {
                height_variation: -1.0,
                ..PlainsSettings::default()
            },
            PlainsSettings {
                noise_scale: 0.0,
                ..PlainsSettings::default()
            },
            PlainsSettings {
                water_level: f32::INFINITY,
                ..PlainsSettings::default()
            },
        ];

        for settings in invalid_settings {
            let config = TerrainConfig {
                plains: settings,
                ..TerrainConfig::default()
            };
            assert!(generate_terrain(&config, request([0, 0, 0], [2, 2, 2])).is_err());
        }
    }

    // Worldkiln 0.9.3 | WO 0.9.3-09 | 2026-10-07 00:24 -05:00
    #[test]
    fn generated_plains_applies_beach_material_at_waterline() {
        let mut config = TerrainConfig::default();
        config.plains.base_height = 8.0;
        config.plains.height_variation = 0.0;
        config.plains.water_level = 8.0;
        config.voxel_layers.rock_slope_degrees = 90.0;
        config.voxel_layers.rock_slope_blend_degrees = 0.0;
        config.voxel_layers.material_noise_strength_studs = 0.0;

        let result = generate_terrain(
            &config,
            TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 16, 8],
            },
        )
        .unwrap();
        let TerrainData::Voxel(grid) = result.data else {
            panic!("expected voxel Terrain");
        };
        // Worldkiln 0.8.5 | WO MAT-051 | 2026-10-10 16:36 -05:00
        // Generation intentionally replaces surface and soil near waterline
        // with beach sand (see apply_waterline_beach_material). Test both
        // the named palette role and the world's actual generated top voxel.
        let beach = grid
            .materials
            .iter()
            .position(|name| name == "sand")
            .unwrap() as u16;
        let top = (0..grid.size[1])
            .rev()
            .find_map(|y| grid.voxel(0, y, 0).filter(|voxel| !voxel.is_empty()))
            .unwrap();
        assert_eq!(top.material, beach);
        assert!(top.occupancy > 0);
    }

    #[test]
    fn identical_inputs_produce_identical_values() {
        let first = generated_heightfield(481_516);
        let second = generated_heightfield(481_516);

        assert_eq!(first, second);
        assert_eq!(first.heights.len(), 63);
        assert_eq!(
            &first.heights[..6],
            &[
                8.255156, 8.625885, 9.371435, 10.253357, 10.980685, 11.347645
            ]
        );
    }

    #[test]
    fn changing_seed_changes_logical_values() {
        assert_ne!(
            generated_heightfield(481_516).heights,
            generated_heightfield(481_517).heights
        );
    }

    #[test]
    fn overlapping_requests_sample_the_same_world_coordinates() {
        let config = TerrainConfig::default();
        let larger = plains::generate(&config.plains, request([-4, 0, -4], [8, 32, 8])).unwrap();
        let inset = plains::generate(&config.plains, request([-2, 0, -1], [3, 32, 2])).unwrap();
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
        let TerrainData::Heightfield(larger) = larger.data else {
            panic!("expected heightfield Terrain in legacy regression");
        };
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
        let TerrainData::Heightfield(inset) = inset.data else {
            panic!("expected heightfield Terrain in legacy regression");
        };

        for local_z in 0..inset.size[2] {
            for local_x in 0..inset.size[0] {
                assert_eq!(
                    inset.height(local_x, local_z),
                    larger.height(local_x + 2, local_z + 3)
                );
            }
        }
    }

    #[test]
    fn generated_heights_stay_inside_requested_y_bounds() {
        let config = TerrainConfig {
            plains: PlainsSettings {
                base_height: 500.0,
                height_variation: 500.0,
                ..PlainsSettings::default()
            },
            ..TerrainConfig::default()
        };
        let result = plains::generate(&config.plains, request([0, -3, 0], [16, 5, 16])).unwrap();
        // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:25 -05:00
        let TerrainData::Heightfield(heightfield) = result.data else {
            panic!("expected heightfield Terrain in legacy regression");
        };

        assert!(
            heightfield
                .heights
                .iter()
                .all(|height| (-3.0..=1.0).contains(height))
        );
    }

    #[test]
    fn serialized_contract_retains_generator_version_and_materials() {
        let config = TerrainConfig {
            materials: TerrainMaterials {
                surface: "grass".to_string(),
                subsurface: "soil".to_string(),
                rock: "stone".to_string(),
                water: "water".to_string(),
            },
            ..TerrainConfig::default()
        };
        let encoded = serde_json::to_string(&config).unwrap();
        let decoded: TerrainConfig = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, config);
        assert_eq!(decoded.generator_implementation_version, 1);
        assert_eq!(decoded.materials.rock, "stone");
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    #[test]
    fn sculpt_raise_affects_only_samples_in_footprint() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 0, 0], [16, 32, 16]))
            .unwrap();
        let unedited_center_height = world.terrain.surface_height_at(8.5, 8.5).unwrap();
        let unedited_far_height = world.terrain.surface_height_at(0.5, 0.5).unwrap();

        let changed = world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.5, 0.0, 8.5],
            radius: 3.0,
            strength: 2.0,
            flatten_target: None,
        });

        assert!(changed);
        assert!(world.terrain.surface_height_at(8.5, 8.5).unwrap() > unedited_center_height);
        assert_eq!(
            world.terrain.surface_height_at(0.5, 0.5).unwrap(),
            unedited_far_height
        );
    }

    #[test]
    fn sculpt_lower_affects_only_samples_in_footprint() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 0, 0], [16, 32, 16]))
            .unwrap();
        let unedited_center_height = world.terrain.surface_height_at(8.5, 8.5).unwrap();

        let changed = world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Lower,
            center: [8.5, 0.0, 8.5],
            radius: 3.0,
            strength: 2.0,
            flatten_target: None,
        });

        assert!(changed);
        assert!(world.terrain.surface_height_at(8.5, 8.5).unwrap() < unedited_center_height);
    }

    #[test]
    fn sculpt_clamps_to_requested_y_bounds() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 10, 0], [8, 5, 8]))
            .unwrap();

        world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [4.5, 0.0, 4.5],
            radius: 8.0,
            strength: 100.0,
            flatten_target: None,
        });

        assert!(world.terrain.surface_height_at(4.0, 4.0).unwrap() <= 14.01);

        world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Lower,
            center: [4.5, 0.0, 4.5],
            radius: 8.0,
            strength: 100.0,
            flatten_target: None,
        });

        assert!(world.terrain.surface_height_at(4.0, 4.0).unwrap_or(10.0) >= 9.99);
    }

    #[test]
    fn sculpt_flatten_moves_samples_toward_target() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 0, 0], [16, 32, 16]))
            .unwrap();
        let target = 25.0;

        world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Flatten,
            center: [8.5, 0.0, 8.5],
            radius: 4.0,
            strength: 1.0,
            flatten_target: Some(target),
        });

        let actual = world.terrain.surface_height_at(8.5, 8.5).unwrap();
        assert!(
            (actual - target).abs() < 0.1,
            "Flatten center {actual} must reach target {target}"
        );
    }

    // Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
    #[test]
    fn flatten_modes_obey_direction_in_authored_voxel_terrain() {
        for (tool, offset, expected_change) in [
            (SculptTool::FlattenRaiseOnly, 3.0, true),
            (SculptTool::FlattenRaiseOnly, -3.0, false),
            (SculptTool::FlattenCutOnly, -3.0, true),
            (SculptTool::FlattenCutOnly, 3.0, false),
            (SculptTool::Flatten, -3.0, true),
            (SculptTool::Flatten, 3.0, true),
        ] {
            let mut world = World::new();
            world
                .generate_authored_terrain(request([0, 0, 0], [16, 64, 16]))
                .unwrap();
            let before = world.terrain.surface_height_at(8.5, 8.5).unwrap();
            let target = before + offset;
            let changed = world.sculpt_authored_terrain(&TerrainSculptRequest {
                tool,
                center: [8.5, before, 8.5],
                radius: 4.0,
                strength: 1.0,
                flatten_target: Some(target),
            });
            assert_eq!(changed, expected_change, "{tool:?}, target offset {offset}");
            let after = world.terrain.surface_height_at(8.5, 8.5).unwrap();
            if !expected_change {
                assert!((after - before).abs() < 0.001);
            } else if offset > 0.0 {
                assert!(after > before, "{tool:?} failed to raise");
            } else {
                assert!(after < before, "{tool:?} failed to cut");
            }
        }
    }

    #[test]
    fn flatten_directional_modes_require_target_height() {
        assert!(SculptTool::Flatten.is_flatten());
        assert!(SculptTool::FlattenRaiseOnly.is_flatten());
        assert!(SculptTool::FlattenCutOnly.is_flatten());
        assert!(!SculptTool::Raise.is_flatten());
        assert!(SculptTool::FlattenRaiseOnly.allows_flatten_direction(2.0, 3.0));
        assert!(!SculptTool::FlattenRaiseOnly.allows_flatten_direction(4.0, 3.0));
        assert!(SculptTool::FlattenCutOnly.allows_flatten_direction(4.0, 3.0));
        assert!(!SculptTool::FlattenCutOnly.allows_flatten_direction(2.0, 3.0));
    }

    #[test]
    fn sculpt_smooth_reduces_discontinuity() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 0, 0], [8, 32, 8]))
            .unwrap();

        let center = [6.0, 0.0, 6.0];
        assert!(world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center,
            radius: 3.0,
            strength: 8.0,
            flatten_target: None,
        }));
        let spiked = world
            .terrain
            .surface_height_at(center[0], center[2])
            .unwrap();

        assert!(world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Smooth,
            center,
            radius: 4.5,
            strength: 1.0,
            flatten_target: None,
        }));
        assert!(
            world
                .terrain
                .surface_height_at(center[0], center[2])
                .unwrap()
                < spiked
        );
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    #[test]
    fn sculpt_strength_scales_edit_intensity() {
        let mut weak = World::new();
        weak.generate_authored_terrain(request([0, 0, 0], [16, 32, 16]))
            .unwrap();
        let mut strong = weak.clone();
        let before = weak.terrain.surface_height_at(8.5, 8.5).unwrap();

        assert!(weak.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.5, 0.0, 8.5],
            radius: 4.0,
            strength: 0.25,
            flatten_target: None,
        }));
        assert!(strong.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.5, 0.0, 8.5],
            radius: 4.0,
            strength: 1.0,
            flatten_target: None,
        }));

        let weak_delta = weak.terrain.surface_height_at(8.5, 8.5).unwrap() - before;
        let strong_delta = strong.terrain.surface_height_at(8.5, 8.5).unwrap() - before;
        assert!(weak_delta > 0.0);
        assert!(strong_delta > weak_delta);
    }

    #[test]
    fn larger_radius_affects_samples_outside_smaller_radius() {
        let mut small = World::new();
        small
            .generate_authored_terrain(request([0, 0, 0], [16, 32, 16]))
            .unwrap();
        let mut large = small.clone();
        let probe_before = small.terrain.surface_height_at(12.5, 8.5).unwrap();

        small.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.5, 0.0, 8.5],
            radius: 2.0,
            strength: 1.0,
            flatten_target: None,
        });
        large.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.5, 0.0, 8.5],
            radius: 5.0,
            strength: 1.0,
            flatten_target: None,
        });

        assert_eq!(
            small.terrain.surface_height_at(12.5, 8.5).unwrap(),
            probe_before
        );
        assert!(large.terrain.surface_height_at(12.5, 8.5).unwrap() > probe_before);
    }

    #[test]
    fn smooth_is_local_and_deterministic() {
        let mut first = World::new();
        first
            .generate_authored_terrain(request([0, 0, 0], [16, 32, 16]))
            .unwrap();
        assert!(first.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [10.0, 0.0, 10.0],
            radius: 3.0,
            strength: 8.0,
            flatten_target: None,
        }));
        let mut second = first.clone();
        let far_before = first.terrain.surface_height_at(0.5, 0.5).unwrap();
        let sculpt = TerrainSculptRequest {
            tool: SculptTool::Smooth,
            center: [8.5, 0.0, 8.5],
            radius: 2.5,
            strength: 0.8,
            flatten_target: None,
        };

        assert!(first.sculpt_authored_terrain(&sculpt));
        assert!(second.sculpt_authored_terrain(&sculpt));
        assert_eq!(first.terrain.authored, second.terrain.authored);
        assert_eq!(
            first.terrain.surface_height_at(0.5, 0.5).unwrap(),
            far_before
        );
    }

    // Worldkiln 0.8.7 | WO 0.8.7-27 | 2026-10-06 15:03 -05:00
    #[test]
    fn authored_sculpting_does_not_mutate_runtime_terrain() {
        let mut world = World::new();

        // The default Plains base height is 16. A 0..15 generation range clamps
        // this fixture to the upper bound, so Raise correctly reports no change.
        // Give the regression headroom so it actually exercises authored mutation
        // while runtime Terrain is present.
        let terrain_request = request([0, 0, 0], [8, 32, 8]);
        world.generate_authored_terrain(terrain_request).unwrap();
        world.generate_runtime_terrain(terrain_request).unwrap();

        let authored_before = world.terrain.authored.clone();
        let runtime_before = world.terrain.runtime.clone();
        let config_before = world.terrain.config.clone();

        assert!(world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [4.5, 0.0, 4.5],
            radius: 2.0,
            strength: 1.0,
            flatten_target: None,
        }));

        assert_ne!(world.terrain.authored, authored_before);
        assert_eq!(world.terrain.runtime, runtime_before);
        assert_eq!(world.terrain.config, config_before);
    }

    #[test]
    fn sculpting_does_not_change_generator_settings_and_regeneration_replaces_edits() {
        let mut world = World::new();
        world.terrain.config.plains.seed = 4242;
        world.terrain.config.plains.base_height = 11.0;
        world.terrain.config.plains.height_variation = 3.0;
        world.terrain.config.plains.noise_scale = 0.035;
        let request = request([0, 0, 0], [16, 32, 16]);
        world.generate_authored_terrain(request).unwrap();
        let config_before = world.terrain.config.clone();
        let generated_before = world.terrain.authored.clone();

        assert!(world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.5, 0.0, 8.5],
            radius: 4.0,
            strength: 2.0,
            flatten_target: None,
        }));
        assert_eq!(world.terrain.config, config_before);
        assert_ne!(world.terrain.authored, generated_before);

        world.generate_authored_terrain(request).unwrap();
        assert_eq!(world.terrain.config, config_before);
        assert_eq!(world.terrain.authored, generated_before);
    }

    // Worldkiln 0.9.3 | WO 0.9.3-03 | 2026-10-06 22:50 -05:00
    #[test]
    fn plains_generation_now_produces_persistent_voxel_logical_data() {
        let config = TerrainConfig::default();
        let result = generate_terrain(&config, request([0, 0, 0], [16, 24, 16])).unwrap();
        let TerrainData::Voxel(grid) = result.data else {
            panic!("Smooth voxel Terrain should be the generated logical payload");
        };
        assert_eq!(grid.size, [8, 12, 8]);
        assert!(grid.filled_voxel_count() > 0);
    }

    #[test]
    fn programmatic_runtime_voxels_use_same_queries_and_request_limits() {
        let mut world = World::new();
        world
            .set_runtime_voxel_terrain(
                [0, 0, 0],
                [1, 1, 1],
                vec!["stone".into()],
                vec![SmoothVoxel::from_unit_occupancy(0, 1.0)],
            )
            .unwrap();
        assert!(matches!(
            world
                .terrain
                .runtime
                .as_ref()
                .map(|result| result.generator),
            Some(TerrainGeneratorType::Programmatic)
        ));
        assert!(world.terrain.surface_height_at(2.0, 2.0).is_some());
        assert!(
            world
                .terrain
                .runtime
                .as_ref()
                .unwrap()
                .solid_occupies_cell(WorldCoord::new(1, 1, 1))
        );
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    #[test]
    fn sculpt_preview_radii_calculates_outer_and_inner_radii_with_clamped_strength() {
        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
        let (outer, inner) = sculpt_preview_radii(10.0, 1.0);
        assert_eq!(outer, 10.0);
        assert_eq!(inner, 2.0);

        let (outer_zero, inner_zero) = sculpt_preview_radii(8.0, 0.0);
        assert_eq!(outer_zero, 8.0);
        assert_eq!(inner_zero, 0.0);

        let (outer_full, inner_full) = sculpt_preview_radii(6.0, SCULPT_STRENGTH_UI_MAX);
        assert_eq!(outer_full, 6.0);
        assert_eq!(inner_full, 6.0);

        let (outer_clamped, inner_clamped) = sculpt_preview_radii(6.0, 10.0);
        assert_eq!(outer_clamped, 6.0);
        assert_eq!(inner_clamped, 6.0);

        let (outer_invalid, inner_invalid) = sculpt_preview_radii(f32::NAN, f32::NAN);
        assert!(outer_invalid.is_finite());
        assert!(inner_invalid.is_finite());
        assert_eq!(inner_invalid, 0.0);
    }

    #[test]
    fn sculpt_rejects_invalid_inputs() {
        let mut world = World::new();
        world
            .generate_authored_terrain(request([0, 0, 0], [8, 8, 8]))
            .unwrap();

        assert!(!world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::None,
            center: [0.0, 0.0, 0.0],
            radius: 2.0,
            strength: 1.0,
            flatten_target: None,
        }));

        assert!(!world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [0.0, 0.0, 0.0],
            radius: -1.0,
            strength: 1.0,
            flatten_target: None,
        }));

        assert!(!world.sculpt_authored_terrain(&TerrainSculptRequest {
            tool: SculptTool::Flatten,
            center: [0.0, 0.0, 0.0],
            radius: 2.0,
            strength: 1.0,
            flatten_target: None,
        }));

        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
        for request in [
            TerrainSculptRequest {
                tool: SculptTool::Raise,
                center: [0.0, 0.0, 0.0],
                radius: f32::NAN,
                strength: 1.0,
                flatten_target: None,
            },
            TerrainSculptRequest {
                tool: SculptTool::Raise,
                center: [0.0, 0.0, 0.0],
                radius: 2.0,
                strength: f32::INFINITY,
                flatten_target: None,
            },
            TerrainSculptRequest {
                tool: SculptTool::Raise,
                center: [f32::NAN, 0.0, 0.0],
                radius: 2.0,
                strength: 1.0,
                flatten_target: None,
            },
            TerrainSculptRequest {
                tool: SculptTool::Flatten,
                center: [0.0, 0.0, 0.0],
                radius: 2.0,
                strength: 1.0,
                flatten_target: Some(f32::NAN),
            },
        ] {
            assert!(!world.sculpt_authored_terrain(&request));
        }
    }
}
