// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
mod plains;
mod sampling;
pub use sampling::TerrainSurfaceSample;
mod layer_cache;
mod layers;
pub use layers::{GenerationLayer, GeneratorSettings, Landform, LayerBrushEdit};
pub use water_bodies::{WaterBody, WaterBodyKind, WaterControlPoint};
mod material_layers;
pub mod storage;
mod types;
pub use material_layers::TerrainMaterialLayer;
mod water_bodies;
pub fn ocean_wave_time_seconds() -> f32 {
    use std::sync::OnceLock;
    use std::time::Instant;
    static START: OnceLock<Instant> = OnceLock::new();
    START.get_or_init(Instant::now).elapsed().as_secs_f32()
}

pub fn ocean_tide_offset(time: f32) -> f32 {
    if time.is_finite() {
        0.15 * (time * 0.013).sin()
    } else {
        0.0
    }
}

pub fn ocean_height_at(x: f32, z: f32, time: f32, config: &TerrainConfig) -> f32 {
    ocean_height_with_appearance(x, z, time, config, &config.water)
}
pub fn ocean_height_with_appearance(
    x: f32,
    z: f32,
    time: f32,
    config: &TerrainConfig,
    w: &TerrainWaterSettings,
) -> f32 {
    if !(x.is_finite() && z.is_finite() && time.is_finite()) {
        return 0.0;
    }
    let hash = |n: f32| {
        let v = (n * 17.197 + 13.123).sin() * 43758.5453;
        v - v.floor()
    };
    let seed = (config.plains.seed.rem_euclid(65536)) as f32;
    let hs = hash(seed + 0.123);
    let h7 = hash(seed + 7.0);
    let h5 = hash(seed + 5.0);
    let angle = w.wave_direction_degrees.to_radians();
    let k = std::f32::consts::TAU / w.wave_length.max(1.0);
    let projection = |theta: f32| x * theta.cos() + z * theta.sin();
    let a = -projection(angle) * k + time * w.wave_speed;
    let b = -projection(angle + 0.43 + hs * 0.47) * k * 1.71
        + time * w.wave_speed * 1.18
        + hs * std::f32::consts::TAU;
    let c = -projection(angle - 0.91 + h7 * 0.39) * k * 0.58
        + time * w.wave_speed * 0.74
        + h5 * std::f32::consts::TAU;
    w.wave_height * (a.sin() * 0.72 + b.sin() * 0.19 + c.sin() * 0.09)
}

mod voxel;

pub(crate) use types::terrain_render_chunk_samples;
pub use types::{
    HeightfieldData, MAX_GENERATION_VOLUME, OceanOpticsMode,
    PLAINS_GENERATOR_IMPLEMENTATION_VERSION, PlainsSettings, SCULPT_STRENGTH_UI_MAX, SculptTool,
    TERRAIN_CHUNK_SIZE, TERRAIN_RENDER_CHUNK_TARGET_STUDS, TerrainConfig, TerrainData,
    TerrainGenerationError, TerrainGenerationRequest, TerrainGeneratorType, TerrainGrassSettings,
    TerrainMaterials, TerrainPaintMaterialSlot, TerrainPaintRequest, TerrainRenderChunkKey,
    TerrainResult, TerrainSculptRequest, TerrainState, TerrainWaterSettings, WaterRealismMode,
    sculpt_preview_radii,
};

pub(crate) use voxel::{
    BEACH_ABOVE_WATER_STUDS, TERRAIN_FALLBACK_ROCK_MATERIAL, TERRAIN_FALLBACK_SUBSURFACE_MATERIAL,
    TERRAIN_FALLBACK_SURFACE_MATERIAL, apply_waterline_beach_material,
};
pub use voxel::{
    DEFAULT_TERRAIN_SAMPLE_SPACING_STUDS, LEGACY_TERRAIN_SAMPLE_SPACING_STUDS,
    SMOOTH_VOXEL_ISO_LEVEL, SMOOTH_VOXEL_OCCUPANCY_MAX, SmoothVoxel, SmoothVoxelGrid,
    SmoothVoxelGridError, VoxelLayerSettings, apply_surface_color_map, apply_surface_material_map,
    voxelize_heightfield_layers, voxelize_heightfield_layers_seeded, voxelize_heightmap_layers,
};

pub fn generate_terrain(
    config: &TerrainConfig,
    request: TerrainGenerationRequest,
) -> Result<TerrainResult, TerrainGenerationError> {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("terrain", "generate", false);
    request.validate()?;
    config.validate()?;

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
    let generation_materials = config.geometry_material_roles();
    let mut grid = match &generated.data {
        TerrainData::Heightfield(heightfield) => voxelize_heightfield_layers_seeded(
            heightfield,
            &generation_materials,
            config.voxel_layers,
            config.plains.seed,
        )
        .map_err(|error| TerrainGenerationError::VoxelData(error.to_string()))?,
        TerrainData::Voxel(_) => return Ok(generated),
    };
    let surface_material = if generation_materials.surface.trim().is_empty() {
        TERRAIN_FALLBACK_SURFACE_MATERIAL
    } else {
        generation_materials.surface.as_str()
    };
    let subsurface_material = if generation_materials.subsurface.trim().is_empty() {
        TERRAIN_FALLBACK_SUBSURFACE_MATERIAL
    } else {
        generation_materials.subsurface.as_str()
    };
    let rock_material = if generation_materials.rock.trim().is_empty() {
        TERRAIN_FALLBACK_ROCK_MATERIAL
    } else {
        generation_materials.rock.as_str()
    };
    apply_waterline_beach_material(
        &mut grid,
        config.plains.water_level,
        if config.shore_material_layer == "legacy-sand" {
            "sand"
        } else {
            subsurface_material
        },
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
        self.replace_authored(Some(generated));
        Ok(())
    }

    pub fn generate_runtime(
        &mut self,
        request: TerrainGenerationRequest,
    ) -> Result<(), TerrainGenerationError> {
        let generated = generate_terrain(&self.config, request)?;
        self.replace_runtime(Some(generated));
        Ok(())
    }

    pub fn surface_height_at(&self, world_x: f32, world_z: f32) -> Option<f32> {
        let result = self.effective()?;
        let min_x = result.request.position[0] as f32;
        let min_z = result.request.position[2] as f32;
        let max_x = min_x + result.request.size[0] as f32;
        let max_z = min_z + result.request.size[2] as f32;
        if world_x < min_x || world_x >= max_x || world_z < min_z || world_z >= max_z {
            return None;
        }
        match &result.data {
            TerrainData::Heightfield(heightfield) => {
                heightfield.surface_height_at(world_x, world_z)
            }
            TerrainData::Voxel(grid) => grid.surface_height_at(world_x, world_z),
        }
    }

    pub fn sculpt_authored(&mut self, request: &TerrainSculptRequest) -> bool {
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

    pub fn paint_authored(&mut self, request: &TerrainPaintRequest) -> bool {
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

impl crate::world::World {
    pub fn sculpt_authored_terrain(&mut self, request: &TerrainSculptRequest) -> bool {
        self.terrain.sculpt_authored(request)
    }

    pub fn paint_authored_terrain(&mut self, request: &TerrainPaintRequest) -> bool {
        self.terrain.paint_authored(request)
    }

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

    #[test]
    fn direct_dirt_paint_uses_builtin_dirt_even_with_custom_subsurface() {
        let mut materials = TerrainMaterials::default();
        assert_eq!(
            TerrainPaintMaterialSlot::Dirt.resolve_material(&materials),
            "PBR:Dirt"
        );
        materials.subsurface = "player_custom_soil".into();
        assert_eq!(
            TerrainPaintMaterialSlot::Dirt.resolve_material(&materials),
            "PBR:Dirt"
        );
        assert_eq!(
            TerrainPaintMaterialSlot::Subsurface.resolve_material(&materials),
            "player_custom_soil"
        );
        assert_eq!(
            TerrainPaintMaterialSlot::Sand.resolve_material(&materials),
            "PBR:Sand"
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
            strength: 1.0,
            falloff: None,
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
            strength: 1.0,
            falloff: None,
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
    fn play_stop_only_invalidates_terrain_when_runtime_geometry_existed() {
        let mut world = World::new();
        let authored = request([0, 0, 0], [16, 16, 16]);
        world.generate_authored_terrain(authored).unwrap();
        let revision = world.terrain.render_revision();
        let full_revision = world.terrain.full_render_revision();

        for _ in 0..5 {
            world.clear_runtime_state();
            assert_eq!(world.terrain.render_revision(), revision);
            assert_eq!(world.terrain.full_render_revision(), full_revision);
        }

        world
            .generate_runtime_terrain(request([32, 0, 32], [16, 16, 16]))
            .unwrap();
        let runtime_revision = world.terrain.render_revision();
        assert!(runtime_revision > revision);
        world.clear_runtime_state();
        assert!(world.terrain.runtime.is_none());
        assert!(world.terrain.render_revision() > runtime_revision);
        assert_eq!(
            world.terrain.full_render_revision(),
            world.terrain.render_revision()
        );
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

    #[test]
    fn default_terrain_materials_are_optional_and_unassigned() {
        assert_eq!(
            TerrainMaterials::default(),
            TerrainMaterials {
                surface: String::new(),
                subsurface: String::new(),
                rock: String::new(),
                water: "Water".into(),
            }
        );
    }

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

    #[test]
    fn generated_plains_uses_subsurface_at_waterline_without_sand() {
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
        let beach = grid
            .materials
            .iter()
            .position(|name| name == TERRAIN_FALLBACK_SUBSURFACE_MATERIAL)
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
        let TerrainData::Heightfield(larger) = larger.data else {
            panic!("expected heightfield Terrain in legacy regression");
        };
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

    #[test]
    fn authored_sculpting_does_not_mutate_runtime_terrain() {
        let mut world = World::new();

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

    #[test]
    fn sculpt_preview_radii_calculates_outer_and_inner_radii_with_clamped_strength() {
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

impl crate::world::World {
    pub fn import_authored_heightfield(&mut self, result: TerrainResult) -> Result<(), String> {
        self.import_heightmap_layer_with_water_level(result, self.terrain.config.plains.water_level)
    }
    pub fn import_heightmap_layer_with_water_level(
        &mut self,
        result: TerrainResult,
        sea_level: f32,
    ) -> Result<(), String> {
        if !sea_level.is_finite() {
            return Err("Invalid sea level".into());
        }
        result.request.validate().map_err(|e| e.to_string())?;
        let TerrainData::Heightfield(ref source) = result.data else {
            return Err("Image import requires a heightfield, never voxels".into());
        };
        if source.origin != result.request.position
            || source.size != result.request.size.map(|s| s as u32)
            || source.heights.len() != (source.size[0] as usize) * (source.size[2] as usize)
        {
            return Err("Image heightfield dimensions and bounds do not match".into());
        }
        let min_y = result.request.position[1] as f32;
        let max_y = (result.request.position[1] + result.request.size[1] - 1) as f32;
        if !source
            .heights
            .iter()
            .all(|v| v.is_finite() && *v >= min_y && *v <= max_y)
        {
            return Err("Image contains heights outside the selected world bounds".into());
        }
        if self.terrain.runtime.is_some() {
            return Err("Stop Play mode before importing terrain".into());
        }
        let mut replacement = self.terrain.clone();
        replacement.config.plains.water_level = sea_level;
        let id = replacement.add_generation_layer(result.request)?;
        let layer = replacement
            .config
            .generation_layers
            .iter_mut()
            .find(|l| l.id == id)
            .expect("new independent image layer");
        layer.name = Some("Imported Heightmap".into());
        layer.settings.water_level = sea_level;
        layer.applied = Some(layer.settings.clone());
        layer.heightmap = Some(source.clone());
        replacement.rebuild_generation_layers()?;
        self.terrain = replacement;
        Ok(())
    }
}

#[cfg(test)]
mod image_import_integration_tests {
    use super::*;
    #[test]
    fn imported_heightmap_is_layered_rendered_and_edited_after_reopen() {
        let mut world = crate::world::World::new();
        let req = TerrainGenerationRequest {
            position: [0, -10, 0],
            size: [16, 100, 16],
        };
        let source = HeightfieldData {
            origin: req.position,
            size: req.size.map(|v| v as u32),
            heights: (0..256).map(|i| 12.0 + (i % 16) as f32 * 0.2).collect(),
        };
        let original = TerrainResult {
            request: req,
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(source.clone()),
        };
        world
            .import_heightmap_layer_with_water_level(original, 4.5)
            .unwrap();
        assert_eq!(world.terrain.config.plains.water_level, 4.5);
        assert_eq!(
            world.terrain.config.generation_layers[0]
                .applied
                .as_ref()
                .unwrap()
                .water_level,
            4.5
        );
        assert!(world.terrain.authored.is_none());
        assert_eq!(world.terrain.config.generation_layers.len(), 1);
        let layer = &world.terrain.config.generation_layers[0];
        assert_eq!(layer.name.as_deref(), Some("Imported Heightmap"));
        assert_eq!(layer.heightmap.as_ref(), Some(&source));
        assert!(layer.applied.is_some());
        assert!(
            matches!(
                world.terrain.effective().unwrap().data,
                TerrainData::Voxel(_)
            ),
            "Image must use the same voxel render pipeline as generated layers"
        );
        let temp = std::env::temp_dir().join(format!(
            "worldkiln_import_layer_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = storage::TerrainStorage::new(&temp);
        storage.save(&world.terrain).unwrap();
        let mut loaded = storage.load().unwrap().unwrap();
        assert!(loaded.authored.is_none());
        assert_eq!(
            loaded.config.generation_layers[0].heightmap.as_ref(),
            Some(&source)
        );
        assert!(matches!(
            loaded.effective().unwrap().data,
            TerrainData::Voxel(_)
        ));
        let id = loaded.config.generation_layers[0].id;
        let brush = TerrainSculptRequest {
            tool: SculptTool::Raise,
            center: [8.0, 12.0, 8.0],
            radius: 3.0,
            strength: 2.0,
            flatten_target: None,
        };
        assert!(
            loaded
                .apply_generation_layer_brush(id, LayerBrushEdit::Sculpt(brush))
                .unwrap()
        );
        loaded.rebuild_generation_layers().unwrap();
        assert_eq!(loaded.config.generation_layers[0].brush_edits.len(), 1);
        storage.save(&loaded).unwrap();
        let mut reopened = storage.load().unwrap().unwrap();
        assert_eq!(
            reopened.config.generation_layers[0].heightmap.as_ref(),
            Some(&source)
        );
        assert_eq!(reopened.config.generation_layers[0].brush_edits.len(), 1);
        assert_eq!(reopened.effective(), loaded.effective());
        let imported_id = reopened.config.generation_layers[0].id;
        reopened
            .set_generation_layer_visible(imported_id, false)
            .unwrap();
        assert!(reopened.effective().is_none());
        reopened
            .set_generation_layer_visible(imported_id, true)
            .unwrap();
        assert_eq!(reopened.effective(), loaded.effective());
        let another = reopened
            .add_generation_layer(TerrainGenerationRequest {
                position: [16, -10, 0],
                size: [16, 100, 16],
            })
            .unwrap();
        reopened.generate_selected_layer(another).unwrap();
        assert_eq!(reopened.config.generation_layers.len(), 2);
        assert_eq!(
            reopened.config.generation_layers[0].heightmap.as_ref(),
            Some(&source)
        );
        assert!(matches!(
            reopened.effective().unwrap().data,
            TerrainData::Voxel(_)
        ));
        std::fs::remove_dir_all(temp).unwrap();
    }
    #[test]
    fn legacy_unlayered_heightmap_opens_as_imported_layer_without_disk_changes() {
        let req = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [8, 24, 8],
        };
        let field = HeightfieldData {
            origin: req.position,
            size: req.size.map(|v| v as u32),
            heights: vec![9.0; 64],
        };
        let original = TerrainResult {
            request: req,
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(field.clone()),
        };
        let loaded = TerrainState::from_persisted(TerrainConfig::default(), Some(original));
        assert!(loaded.authored.is_none());
        assert_eq!(
            loaded.config.generation_layers[0].heightmap.as_ref(),
            Some(&field)
        );
        assert!(matches!(
            loaded.effective().unwrap().data,
            TerrainData::Voxel(_)
        ));
    }
    #[test]
    fn rejects_bad_samples_without_replacing_user_terrain() {
        let mut world = crate::world::World::new();
        let req = TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [2, 3, 2],
        };
        let result = TerrainResult {
            request: req,
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: req.position,
                size: req.size.map(|v| v as u32),
                heights: vec![0.0, f32::NAN, 0.0, 0.0],
            }),
        };
        assert!(world.import_authored_heightfield(result).is_err());
        assert!(world.terrain.authored.is_none());
    }
}

#[cfg(test)]
mod wk_swim_wave_tests {
    use super::*;
    #[test]
    fn wk_swim_material_wave_appearance_controls_sampled_vertical_surface() {
        let config = TerrainConfig::default();
        let mut material = config.water;
        let sample = ocean_height_with_appearance(12.3, 42.1, 5.0, &config, &material);
        material.wave_height = 0.0;
        let flat = ocean_height_with_appearance(12.3, 42.1, 5.0, &config, &material);
        assert!(sample.is_finite() && sample.abs() > 1.0e-6);
        assert_eq!(flat, 0.0);
    }
    #[test]
    fn ocean_wave_height_is_seeded_finite_and_bounded() {
        let mut config = TerrainConfig::default();
        config.water.wave_height = 1.5;
        config.water.wave_length = 32.0;
        config.plains.seed = 129;
        for t in [0.0, 0.15, 1.0, 17.0, 100.0] {
            let h = ocean_height_at(35.0, -41.0, t, &config);
            assert!(h.is_finite());
            assert!(h.abs() <= 1.5 + 1e-5);
        }
        assert_ne!(
            ocean_height_at(35.0, -41.0, 0.0, &config),
            ocean_height_at(35.0, -41.0, 1.0, &config)
        );
        config.water.wave_height = 0.0;
        assert_eq!(ocean_height_at(35.0, -41.0, 1.0, &config), 0.0);
    }
}

#[cfg(test)]
mod generated_then_imported_independent_layer_regression {
    use super::*;
    #[test]
    fn generate_plains_then_import_keeps_both_layers_and_independent_surface_regions() {
        let mut world = crate::world::World::new();
        let plains_id = world
            .terrain
            .add_generation_layer(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [24, 40, 24],
            })
            .unwrap();
        {
            let layer = world
                .terrain
                .config
                .generation_layers
                .iter_mut()
                .find(|l| l.id == plains_id)
                .unwrap();
            layer.settings.base_height = 10.0;
            layer.settings.relief = 0.0;
            layer.settings.water_level = 0.0;
        }
        world.terrain.generate_selected_layer(plains_id).unwrap();
        let original = world.terrain.surface_height_at(12.5, 12.5).unwrap();
        let imported_req = TerrainGenerationRequest {
            position: [40, 0, 0],
            size: [24, 40, 24],
        };
        let imported = TerrainResult {
            request: imported_req,
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: imported_req.position,
                size: imported_req.size.map(|n| n as u32),
                heights: vec![22.0; 24 * 24],
            }),
        };
        world
            .import_heightmap_layer_with_water_level(imported, 0.0)
            .unwrap();
        let layers = &world.terrain.config.generation_layers;
        assert_eq!(layers.len(), 2, "import must not replace generated Plains");
        assert_eq!(layers[0].id, plains_id);
        assert!(layers[0].heightmap.is_none());
        assert!(layers[1].heightmap.is_some());
        assert_ne!(layers[0].id, layers[1].id);
        let surface0 = world.terrain.surface_height_at(12.5, 12.5).unwrap();
        let surface1 = world.terrain.surface_height_at(52.5, 12.5).unwrap();
        println!("GENERATED_AND_IMPORTED surfaces_plains={surface0} imported={surface1}");
        assert!((surface0 - original).abs() < 0.1);
        assert!((surface1 - 22.0).abs() < 3.0);
        let imported_id = layers[1].id;
        world
            .terrain
            .set_generation_layer_visible(imported_id, false)
            .unwrap();
        assert!(world.terrain.surface_height_at(12.5, 12.5).is_some());
        assert!(world.terrain.surface_height_at(52.5, 12.5).is_none());
    }
}

#[cfg(test)]
mod generated_and_imported_roundtrip_regression {
    use super::*;
    #[test]
    fn two_layer_generation_brush_journals_survive_save_reopen() {
        let mut world = crate::world::World::new();
        world.terrain.config.ocean_enabled = false;
        let id = world
            .terrain
            .add_generation_layer(TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [24, 40, 24],
            })
            .unwrap();
        {
            let layer = &mut world.terrain.config.generation_layers[0];
            layer.settings.base_height = 10.0;
            layer.settings.relief = 0.0;
            layer.settings.water_level = 0.0;
        }
        world.terrain.generate_selected_layer(id).unwrap();
        let req = TerrainGenerationRequest {
            position: [40, 0, 0],
            size: [24, 40, 24],
        };
        world
            .import_heightmap_layer_with_water_level(
                TerrainResult {
                    request: req,
                    generator: TerrainGeneratorType::Programmatic,
                    generator_implementation_version: 1,
                    data: TerrainData::Heightfield(HeightfieldData {
                        origin: req.position,
                        size: req.size.map(|v| v as u32),
                        heights: vec![22.0; 24 * 24],
                    }),
                },
                0.0,
            )
            .unwrap();
        let second = world.terrain.config.generation_layers[1].id;
        for (layer_id, center) in [(id, [12.5, 10.0, 12.5]), (second, [52.5, 22.0, 12.5])] {
            let edited = world
                .terrain
                .apply_generation_layer_brush(
                    layer_id,
                    LayerBrushEdit::Sculpt(TerrainSculptRequest {
                        tool: SculptTool::Raise,
                        center,
                        radius: 3.0,
                        strength: 2.0,
                        flatten_target: None,
                    }),
                )
                .unwrap();
            assert!(
                edited,
                "the requested layer must accept its own brush edits"
            );
        }
        world.terrain.rebuild_generation_layers().unwrap();
        let expected = world.terrain.clone();
        let dir = std::env::temp_dir().join(format!(
            "wk_disjoint_layer_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let storage = storage::TerrainStorage::new(&dir);
        storage.save(&world.terrain).unwrap();
        let restored = storage.load().unwrap().unwrap();
        assert_eq!(restored.config.generation_layers.len(), 2);
        assert_eq!(restored.config.generation_layers[0].id, id);
        assert_eq!(restored.config.generation_layers[1].id, second);
        assert_eq!(restored.config.generation_layers[0].brush_edits.len(), 1);
        assert_eq!(restored.config.generation_layers[1].brush_edits.len(), 1);
        assert_eq!(
            restored.config.generation_layers[1].heightmap,
            expected.config.generation_layers[1].heightmap
        );
        for (x, z) in [(12.5, 12.5), (52.5, 12.5)] {
            let a = expected.surface_height_at(x, z).unwrap();
            let b = restored.surface_height_at(x, z).unwrap();
            assert!(
                (a - b).abs() < 0.1,
                "independent sculpt journal must survive reopen"
            );
        }
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
