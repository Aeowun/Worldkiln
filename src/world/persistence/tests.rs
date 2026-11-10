// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::scripting::binding::ScriptBinding;
use crate::world::cell::AttributeValue;
use crate::world::coordinate::WorldCoord;
use crate::world::world::World;
use crate::world::{
    Cell, CellType, ChunkCoord, HeightfieldData, SculptTool, TerrainData, TerrainGenerationRequest,
    TerrainGeneratorType, TerrainMaterials, TerrainResult, TerrainSculptRequest, WorldStorage,
};
use glam::Vec3;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;

fn unique_test_project(name: &str) -> PathBuf {
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();

    let project = std::env::temp_dir().join(format!(
        "worldkiln_persistence_{}_{}_{}",
        name,
        std::process::id(),
        timestamp
    ));

    fs::create_dir_all(&project).unwrap();

    project
}

fn test_world_path(name: &str) -> (PathBuf, PathBuf) {
    let project = unique_test_project(name);
    let path = project.join("world.dat");
    (project, path)
}

fn active_storage_root(project: &std::path::Path) -> PathBuf {
    let metadata = fs::read_to_string(project.join("world.dat")).unwrap_or_default();
    let generation = metadata
        .lines()
        .find_map(|line| line.strip_prefix("STORAGE_GENERATION "));
    match generation {
        Some(generation) => project.join(".world_generations").join(generation),
        None => project.to_path_buf(),
    }
}

#[test]
fn explicit_none_character_selection_survives_world_save_and_reopen() {
    let (project, path) = test_world_path("no_character_selected");
    let mut authored = World::new();
    authored.selected_character.clear();
    save_world(&authored, &path).unwrap();
    let contents = fs::read_to_string(&path).unwrap();
    assert!(contents.contains("SELECTED_CHARACTER None"));

    let mut reopened = World::new();
    reopened.selected_character = "PreviouslyChosen".into();
    load_world(&mut reopened, &path).unwrap();
    assert!(
        reopened.selected_character.is_empty(),
        "reloading a no-Character project must not introduce a Player"
    );
    fs::remove_dir_all(project).ok();
}

#[test]
fn wkasset010_player_scale_roundtrip_and_legacy_default() {
    let (project, path) = test_world_path("player_character_scale");
    let mut world = World::new();
    assert_eq!(world.player_character_scale, Vec3::ONE);
    world.player_character_scale = Vec3::new(0.5, 1.75, 2.25);
    save_world(&world, &path).unwrap();
    assert!(
        fs::read_to_string(&path)
            .unwrap()
            .contains("PLAYER_CHARACTER_SCALE 0.5 1.75 2.25")
    );
    let mut reopened = World::new();
    load_world(&mut reopened, &path).unwrap();
    assert_eq!(
        reopened.player_character_scale,
        world.player_character_scale
    );

    let legacy = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .filter(|line| !line.starts_with("PLAYER_CHARACTER_SCALE "))
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(&path, format!("{legacy}\n")).unwrap();
    load_world(&mut reopened, &path).unwrap();
    assert_eq!(reopened.player_character_scale, Vec3::ONE);

    let broken = format!("{legacy}\nPLAYER_CHARACTER_SCALE NaN 1 0\n");
    fs::write(&path, broken).unwrap();
    load_world(&mut reopened, &path).unwrap();
    assert_eq!(reopened.player_character_scale, Vec3::ONE);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn test_world_default_gravity() {
    let world = World::new();

    assert_eq!(world.gravity, Vec3::new(0.0, -9.81, 0.0));
}

#[test]
fn failed_world_load_preserves_the_existing_destination_world() {
    let (project, path) = test_world_path("failed_load_keeps_world");
    let mut disk_world = World::new();
    disk_world.set_cell(WorldCoord::new(1, 2, 3), CellType::Block);
    save_world(&disk_world, &path).unwrap();
    fs::create_dir_all(active_storage_root(&project).join("terrain")).unwrap();
    fs::write(
        active_storage_root(&project).join("terrain/terrain.json"),
        "{ invalid json",
    )
    .unwrap();

    let mut destination = World::new();
    destination.set_cell(WorldCoord::new(-7, 4, 12), CellType::Light);
    destination.gravity = Vec3::new(3.0, -4.0, 5.0);
    let previous_revision = destination.render_revision;

    assert!(load_world(&mut destination, &path).is_err());
    assert!(destination.get(WorldCoord::new(-7, 4, 12)).is_some());
    assert!(destination.get(WorldCoord::new(1, 2, 3)).is_none());
    assert_eq!(destination.gravity, Vec3::new(3.0, -4.0, 5.0));
    assert_eq!(destination.render_revision, previous_revision);

    fs::remove_dir_all(project).ok();
}

#[test]
fn unsupported_or_malformed_world_format_preserves_destination_world() {
    let (project, path) = test_world_path("unsupported_world_format");
    let cases = [
        "WORLD_FORMAT 999\n",
        "WORLD_FORMAT nope\n",
        "WORLD_FORMAT 2 extra\n",
    ];

    for content in cases {
        fs::write(&path, content).unwrap();
        let mut destination = World::new();
        let sentinel = WorldCoord::new(8, 9, 10);
        destination.set_cell(sentinel, CellType::Block);

        let error = load_world(&mut destination, &path).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(destination.get(sentinel).is_some());
    }

    fs::remove_dir_all(project).ok();
}

#[test]
fn terrain_config_and_logical_chunks_round_trip_without_regeneration() {
    let (project, path) = test_world_path("terrain_round_trip");
    let mut world = World::new();
    world.terrain.config.plains.seed = 918_221;
    world.terrain.config.materials = TerrainMaterials {
        surface: "project/grass".into(),
        subsurface: "project/soil".into(),
        rock: "engine/rock".into(),
        water: "engine/water".into(),
    };
    world
        .terrain
        .generate_authored(TerrainGenerationRequest {
            position: [-20, -5, 11],
            size: [70, 24, 45],
        })
        .unwrap();
    let expected = world.terrain.authored.clone();

    save_world(&world, &path).unwrap();
    assert!(
        active_storage_root(&project)
            .join("terrain")
            .join("terrain.json")
            .is_file()
    );
    assert!(
        fs::read_dir(active_storage_root(&project).join("terrain"))
            .unwrap()
            .filter_map(Result::ok)
            .any(
                |entry| entry.path().extension().and_then(|value| value.to_str())
                    == Some("terrainchunk")
            )
    );

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    assert_eq!(loaded.terrain.config, world.terrain.config);
    assert_eq!(loaded.terrain.authored, expected);
    assert!(loaded.terrain.runtime.is_none());

    fs::remove_dir_all(project).ok();
}

#[test]
fn sculpted_authored_terrain_survives_save_and_reopen_exact() {
    let (project, path) = test_world_path("sculpted_terrain_persistence");
    let mut world = World::new();
    world.terrain.config.plains.seed = 77_123;
    world.terrain.config.plains.base_height = 9.0;
    world.terrain.config.plains.height_variation = 4.0;
    world.terrain.config.plains.noise_scale = 0.031;
    world
        .generate_authored_terrain(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [32, 32, 32],
        })
        .unwrap();

    assert!(world.sculpt_authored_terrain(&TerrainSculptRequest {
        tool: SculptTool::Raise,
        center: [16.5, 0.0, 16.5],
        radius: 6.0,
        strength: 5.0,
        flatten_target: None,
    }));
    let expected_config = world.terrain.config.clone();
    let expected_authored = world.terrain.authored.clone().unwrap();

    world
        .generate_runtime_terrain(TerrainGenerationRequest {
            position: [4, 0, 4],
            size: [8, 16, 8],
        })
        .unwrap();
    assert!(world.terrain.runtime.is_some());

    save_world(&world, &path).unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert_eq!(loaded.terrain.config, expected_config);
    assert_eq!(loaded.terrain.authored, Some(expected_authored));
    assert!(loaded.terrain.runtime.is_none());

    fs::remove_dir_all(project).ok();
}

#[test]
fn legacy_world_without_terrain_loads_default_empty_state() {
    let (project, path) = test_world_path("legacy_no_terrain");
    fs::write(&path, "WORLD_FORMAT 2\nNEXT_CELL_ID 10000000\n").unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert_eq!(loaded.terrain.config, Default::default());
    assert!(loaded.terrain.authored.is_none());
    assert!(loaded.terrain.runtime.is_none());
    fs::remove_dir_all(project).ok();
}

#[test]
fn runtime_terrain_is_not_persisted() {
    let (project, path) = test_world_path("runtime_terrain");
    let mut world = World::new();
    world
        .terrain
        .generate_runtime(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [8, 8, 8],
        })
        .unwrap();
    save_world(&world, &path).unwrap();
    assert!(!active_storage_root(&project).join("terrain").exists());

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    assert!(loaded.terrain.authored.is_none());
    assert!(loaded.terrain.runtime.is_none());
    fs::remove_dir_all(project).ok();
}

#[test]
fn default_world_does_not_create_terrain_storage() {
    let (project, path) = test_world_path("default_without_terrain_storage");
    fs::create_dir(active_storage_root(&project).join("terrain")).unwrap();
    save_world(&World::new(), &path).unwrap();

    assert!(
        !active_storage_root(&project)
            .join("terrain")
            .join("terrain.json")
            .exists()
    );
    fs::remove_dir_all(project).ok();
}

#[test]
fn terrain_load_does_not_regenerate_from_seed_when_chunk_data_is_modified() {
    let (project, path) = test_world_path("terrain_no_regen");
    let mut world = World::new();
    world
        .terrain
        .generate_authored(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [64, 8, 64],
        })
        .unwrap();
    save_world(&world, &path).unwrap();

    let chunk_path = active_storage_root(&project)
        .join("terrain")
        .join("v_0_0_0.terrainchunk");
    let content = fs::read_to_string(&chunk_path).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&content).unwrap();
    json["occupancies"][0] = serde_json::json!(17);
    fs::write(&chunk_path, serde_json::to_string(&json).unwrap()).unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    let loaded_authored = loaded.terrain.authored.as_ref().unwrap();
    let TerrainData::Voxel(ref grid) = loaded_authored.data else {
        panic!("expected voxel Terrain after storage-v2 load");
    };
    assert_eq!(grid.voxels[0].occupancy, 17);

    fs::remove_dir_all(project).ok();
}

#[test]
fn storage_v2_without_spacing_keeps_historical_world_scale() {
    let (project, path) = test_world_path("terrain_legacy_spacing_v2");
    let mut world = World::new();
    world
        .set_authored_voxel_terrain(
            [0, 0, 0],
            [2, 1, 2],
            vec!["rock".into()],
            vec![crate::world::SmoothVoxel::from_unit_occupancy(0, 1.0); 4],
        )
        .unwrap();

    save_world(&world, &path).unwrap();

    let metadata_path = active_storage_root(&project)
        .join("terrain")
        .join("terrain.json");
    let mut metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
    metadata["result"]
        .as_object_mut()
        .expect("stored voxel result object")
        .remove("spacing_studs");
    fs::write(&metadata_path, serde_json::to_string(&metadata).unwrap()).unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    let TerrainData::Voxel(grid) = &loaded.terrain.authored.as_ref().unwrap().data else {
        panic!("expected voxel Terrain");
    };
    assert_eq!(
        grid.spacing_studs,
        crate::world::LEGACY_TERRAIN_SAMPLE_SPACING_STUDS
    );
    assert_eq!(grid.world_bounds().1, [8.0, 4.0, 8.0]);

    fs::remove_dir_all(project).ok();
}

#[test]
fn stale_terrain_chunks_are_cleaned_up_on_save() {
    let (project, path) = test_world_path("stale_terrain_chunks");
    let mut world = World::new();

    world
        .terrain
        .generate_authored(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [132, 8, 132],
        })
        .unwrap();
    save_world(&world, &path).unwrap();

    let terrain_dir = active_storage_root(&project).join("terrain");
    let chunk_files_count = |dir: &PathBuf| {
        fs::read_dir(dir)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry.path().extension().and_then(|ext| ext.to_str()) == Some("terrainchunk")
            })
            .count()
    };

    let TerrainData::Voxel(grid) = &world.terrain.authored.as_ref().unwrap().data else {
        panic!("expected generated voxel Terrain");
    };
    let expected_chunks = grid
        .size
        .map(|size| size.div_ceil(crate::world::TERRAIN_CHUNK_SIZE))
        .into_iter()
        .product::<u32>() as usize;
    assert_eq!(chunk_files_count(&terrain_dir), expected_chunks);

    world
        .terrain
        .generate_authored(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [32, 16, 32],
        })
        .unwrap();
    save_world(&world, &path).unwrap();

    let terrain_dir = active_storage_root(&project).join("terrain");
    assert_eq!(chunk_files_count(&terrain_dir), 1);

    fs::remove_dir_all(project).ok();
}

#[test]
fn wo53_expanded_voxel_load_retains_dimension_volume_and_bounds_validation() {
    let (project, path) = test_world_path("wo53_voxel_request_validation");
    let mut world = World::new();
    world
        .set_authored_voxel_terrain(
            [0, 0, 0],
            [1, 1, 1],
            vec!["sand".into()],
            vec![crate::world::SmoothVoxel::from_unit_occupancy(0, 1.0)],
        )
        .unwrap();
    save_world(&world, &path).unwrap();
    let metadata_path = active_storage_root(&project)
        .join("terrain")
        .join("terrain.json");
    let original: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
    for (position, size, message) in [
        ([0, 0, 0], [0, 1, 1], "must be positive"),
        ([0, 0, 0], [-1, 1, 1], "must be positive"),
        ([i32::MAX, 0, 0], [2, 1, 1], "bounds overflow"),
        ([0, 0, 0], [i32::MAX; 3], "volume overflow"),
    ] {
        let mut metadata = original.clone();
        metadata["result"]["request"]["position"] = serde_json::json!(position);
        metadata["result"]["request"]["size"] = serde_json::json!(size);
        fs::write(&metadata_path, serde_json::to_string(&metadata).unwrap()).unwrap();
        let error = load_world(&mut World::new(), &path).unwrap_err();
        assert!(error.to_string().contains(message), "{error}");
    }
    let mut metadata = original;
    metadata["result"]["request"]["size"] = serde_json::json!([2048.0, 96.0, 1280.0]);
    fs::write(&metadata_path, serde_json::to_string(&metadata).unwrap()).unwrap();
    assert!(load_world(&mut World::new(), &path).is_err());
    fs::remove_dir_all(project).ok();
}

#[test]
fn smooth_voxel_terrain_round_trips_exactly_through_storage_v2() {
    let (project, path) = test_world_path("smooth_voxel_terrain_v2");
    let mut world = World::new();
    world.terrain.config.materials = TerrainMaterials {
        surface: "grass".into(),
        subsurface: "dirt".into(),
        rock: "rock".into(),
        water: "water".into(),
    };
    world
        .terrain
        .generate_authored(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [16, 16, 16],
        })
        .unwrap();

    assert!(matches!(
        world.terrain.authored.as_ref().map(|result| &result.data),
        Some(TerrainData::Voxel(_))
    ));
    let expected = world.terrain.authored.clone();

    save_world(&world, &path).unwrap();
    assert!(
        active_storage_root(&project)
            .join("terrain")
            .join("v_0_0_0.terrainchunk")
            .is_file()
    );

    let metadata: serde_json::Value = serde_json::from_str(
        &fs::read_to_string(
            active_storage_root(&project)
                .join("terrain")
                .join("terrain.json"),
        )
        .unwrap(),
    )
    .unwrap();
    assert_eq!(metadata["version"], serde_json::json!(2));
    assert_eq!(metadata["result"]["payload"], serde_json::json!("voxel"));

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    assert_eq!(loaded.terrain.authored, expected);
    assert!(loaded.terrain.runtime.is_none());

    fs::remove_dir_all(project).ok();
}

#[test]
fn legacy_v1_heightfield_storage_loads_without_voxel_migration() {
    let (project, path) = test_world_path("legacy_v1_heightfield_terrain");
    let mut world = World::new();
    let request = TerrainGenerationRequest {
        position: [0, 0, 0],
        size: [16, 16, 16],
    };
    world.terrain.authored = Some(TerrainResult {
        request,
        generator: TerrainGeneratorType::Plains,
        generator_implementation_version: 1,
        data: TerrainData::Heightfield(HeightfieldData {
            origin: request.position,
            size: request.size.map(|value| value as u32),
            heights: vec![7.25; 16 * 16],
        }),
    });
    let expected = world.terrain.authored.clone();
    save_world(&world, &path).unwrap();

    let terrain_dir = active_storage_root(&project).join("terrain");
    let metadata_path = terrain_dir.join("terrain.json");
    let mut metadata: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&metadata_path).unwrap()).unwrap();
    let result = metadata["result"].clone();
    metadata["version"] = serde_json::json!(1);
    metadata["result"] = serde_json::json!({
        "request": result["request"].clone(),
        "generator": result["generator"].clone(),
        "generator_implementation_version": result["generator_implementation_version"].clone(),
        "chunks": result["chunks"].clone(),
    });
    fs::write(&metadata_path, serde_json::to_string(&metadata).unwrap()).unwrap();

    let chunk_path = terrain_dir.join("0_0.terrainchunk");
    let mut chunk: serde_json::Value =
        serde_json::from_str(&fs::read_to_string(&chunk_path).unwrap()).unwrap();
    chunk["version"] = serde_json::json!(1);
    fs::write(&chunk_path, serde_json::to_string(&chunk).unwrap()).unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    let expected_heightfield = match expected.as_ref().map(|result| &result.data) {
        Some(TerrainData::Heightfield(field)) => field,
        _ => unreachable!(),
    };
    assert!(loaded.terrain.authored.is_none());
    assert_eq!(loaded.terrain.config.generation_layers.len(), 1);
    assert_eq!(
        loaded.terrain.config.generation_layers[0]
            .heightmap
            .as_ref(),
        Some(expected_heightfield)
    );
    assert!(matches!(
        loaded
            .terrain
            .layered_authored
            .as_ref()
            .map(|result| &result.data),
        Some(TerrainData::Heightfield(_))
    ));

    fs::remove_dir_all(project).ok();
}

#[test]
fn corrupted_terrain_storage_version_returns_error() {
    let (project, path) = test_world_path("corrupted_terrain_version");
    let mut world = World::new();
    world
        .terrain
        .generate_authored(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [16, 16, 16],
        })
        .unwrap();
    save_world(&world, &path).unwrap();

    let meta_path = active_storage_root(&project)
        .join("terrain")
        .join("terrain.json");
    let content = fs::read_to_string(&meta_path).unwrap();
    let mut json: serde_json::Value = serde_json::from_str(&content).unwrap();
    json["version"] = serde_json::json!(999);
    fs::write(&meta_path, serde_json::to_string(&json).unwrap()).unwrap();

    let mut loaded = World::new();
    let result = load_world(&mut loaded, &path);
    assert!(result.is_err());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_world_gravity_persistence() {
    let mut world = World::new();
    let custom_gravity = Vec3::new(1.2, 3.4, -5.6);

    world.gravity = custom_gravity;

    let (project, path) = test_world_path("gravity");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert_eq!(loaded_world.gravity, custom_gravity);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_world_lighting_defaults() {
    let world = World::new();

    assert!(world.lighting.shadows_enabled);
    assert!(world.lighting.global_light_enabled);
    assert!((world.lighting.global_light_intensity - 1.0).abs() < 1e-5);
    assert!((world.lighting.ambient_intensity - 0.2).abs() < 1e-5);
}

#[test]
fn test_light_cell_construction() {
    let cell = Cell::new_light();

    assert_eq!(cell.cell_type, CellType::Light);
    assert!(!cell.visible);
    assert!(!cell.solid);
    assert_eq!(cell.light_intensity, 5.0);
}

#[test]
fn test_world_lighting_persistence() {
    let mut world = World::new();

    world.lighting.shadows_enabled = false;
    world.lighting.point_shadow_camera_distance = 93.5;
    world.lighting.global_light_enabled = false;
    world.lighting.global_light_direction = Vec3::new(0.0, 1.0, 0.0);
    world.lighting.global_light_color = Vec3::new(1.0, 0.5, 0.2);
    world.lighting.global_light_intensity = 4.2;
    world.lighting.ambient_intensity = 0.88;

    let (project, path) = test_world_path("lighting");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert!(!loaded_world.lighting.shadows_enabled);
    assert_eq!(loaded_world.lighting.point_shadow_camera_distance, 93.5);
    assert!(!loaded_world.lighting.global_light_enabled);

    assert_eq!(
        loaded_world.lighting.global_light_direction,
        Vec3::new(0.0, 1.0, 0.0)
    );

    assert_eq!(
        loaded_world.lighting.global_light_color,
        Vec3::new(1.0, 0.5, 0.2)
    );

    assert!((loaded_world.lighting.global_light_intensity - 4.2).abs() < 1e-5);

    assert!((loaded_world.lighting.ambient_intensity - 0.88).abs() < 1e-5);

    fs::remove_dir_all(project).ok();
}

#[test]
fn atmosphere_settings_round_trip() {
    let (project, path) = test_world_path("atmosphere");
    let mut world = World::new();
    world.sky.atmosphere_enabled = false;
    world.sky.fog_start_studs = 44.0;
    world.sky.fog_end_studs = 222.0;
    world.sky.fog_color = Vec3::new(0.2, 0.35, 0.6);
    world.sky.horizon_strength = 0.72;

    save_world(&world, &path).unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert!(!loaded.sky.atmosphere_enabled);
    assert!((loaded.sky.fog_start_studs - 44.0).abs() < 1e-5);
    assert!((loaded.sky.fog_end_studs - 222.0).abs() < 1e-5);
    assert_eq!(loaded.sky.fog_color, Vec3::new(0.2, 0.35, 0.6));
    assert!((loaded.sky.horizon_strength - 0.72).abs() < 1e-5);

    fs::remove_dir_all(project).ok();
}

#[test]
fn legacy_world_without_atmosphere_record_keeps_defaults() {
    let (project, path) = test_world_path("legacy_atmosphere_defaults");
    fs::write(
        &path,
        "WORLD_FORMAT 2\nNEXT_CELL_ID 10000000\nSKY true Cubemap_Temperate_01-512x512.png\n",
    )
    .unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert!(loaded.sky.atmosphere_enabled);
    assert!((loaded.sky.fog_start_studs - 96.0).abs() < 1e-5);
    assert!((loaded.sky.fog_end_studs - 320.0).abs() < 1e-5);
    assert_eq!(loaded.sky.fog_color, Vec3::new(0.70, 0.78, 0.84));
    assert!((loaded.sky.horizon_strength - 0.38).abs() < 1e-5);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_light_cell_persistence() {
    let mut world = World::new();

    let coord = WorldCoord::new(10, 20, 30);

    let id = world.set_cell(coord, CellType::Light);

    if let Some(cell) = world.get_mut(coord) {
        cell.light_color = Vec3::new(0.1, 0.2, 0.3);
        cell.light_intensity = 99.0;
        cell.light_range = 50.0;
        cell.light_shadows = false;
        cell.light_enabled = false;
    }

    let (project, path) = test_world_path("light_cell");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    let loaded_source = loaded_world.source(id).unwrap();
    let loaded_cell = &loaded_source.cell;
    assert_eq!(loaded_source.position, Vec3::new(10.5, 20.5, 30.5));

    assert_eq!(loaded_cell.cell_type, CellType::Light);
    assert_eq!(loaded_cell.light_color, Vec3::new(0.1, 0.2, 0.3));
    assert_eq!(loaded_cell.light_intensity, 99.0);
    assert_eq!(loaded_cell.light_range, 50.0);
    assert!(!loaded_cell.light_shadows);
    assert!(!loaded_cell.light_enabled);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_chunked_world_persistence() {
    let (project, path) = test_world_path("chunked_world");

    let mut world = World::new();

    let a = WorldCoord::new(0, 0, 0);
    let b = WorldCoord::new(16, 0, 0);
    let c = WorldCoord::new(-1, 0, 0);

    world.set_cell(a, CellType::Block);
    let light_id = world.set_cell(b, CellType::Light);
    world.set_cell(c, CellType::FxBlock);

    save_world(&world, &path).unwrap();

    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();

    assert!(storage.chunk_exists(ChunkCoord::new(0, 0, 0)));
    assert!(storage.chunk_exists(ChunkCoord::new(1, 0, 0)));
    assert!(storage.chunk_exists(ChunkCoord::new(-1, 0, 0)));

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert_eq!(loaded.get(a).unwrap().cell_type, CellType::Block);

    let light = loaded.source(light_id).unwrap();
    assert_eq!(light.cell.cell_type, CellType::Light);
    assert_eq!(
        light.position,
        Vec3::new(b.x as f32 + 0.5, b.y as f32 + 0.5, b.z as f32 + 0.5)
    );

    assert_eq!(loaded.get(c).unwrap().cell_type, CellType::FxBlock);

    fs::remove_dir_all(project).ok();
}

#[test]
fn missing_manifest_chunk_fails_without_replacing_destination_world() {
    let (project, path) = test_world_path("missing_manifest_chunk");
    let mut authored = World::new();
    let coord = WorldCoord::new(17, 2, -1);
    authored.set_cell(coord, CellType::Block);
    save_world(&authored, &path).unwrap();
    let original_world_file = fs::read(&path).unwrap();
    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();
    let missing = storage.chunk_path(ChunkCoord::from_world_coord(coord));
    fs::remove_file(&missing).unwrap();

    let mut destination = World::new();
    let sentinel = WorldCoord::new(-9, 4, 12);
    destination.set_cell(sentinel, CellType::Light);
    let error = load_world(&mut destination, &path).unwrap_err();

    assert_eq!(error.kind(), std::io::ErrorKind::NotFound);
    assert!(
        error
            .to_string()
            .contains(&missing.to_string_lossy().to_string())
    );
    assert!(destination.get(sentinel).is_some());
    assert_eq!(fs::read(&path).unwrap(), original_world_file);
    fs::remove_dir_all(project).ok();
}

#[test]
fn invalid_manifest_cell_ids_fail_without_replacing_destination_world() {
    let (project, path) = test_world_path("invalid_manifest_cell_ids");
    let mut authored = World::new();
    authored.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
    authored.set_cell(WorldCoord::new(1, 0, 0), CellType::FxBlock);
    save_world(&authored, &path).unwrap();
    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();
    let chunk_path = storage.chunk_path(ChunkCoord::new(0, 0, 0));
    let original_chunk: serde_json::Value =
        serde_json::from_slice(&fs::read(&chunk_path).unwrap()).unwrap();
    let first_id = original_chunk["cells"][0]["id"].as_u64().unwrap();

    for invalid_id in [0, first_id] {
        let mut corrupted = original_chunk.clone();
        corrupted["cells"][1]["id"] = serde_json::json!(invalid_id);
        fs::write(&chunk_path, serde_json::to_vec(&corrupted).unwrap()).unwrap();

        let mut destination = World::new();
        let sentinel = WorldCoord::new(-9, 4, 12);
        destination.set_cell(sentinel, CellType::Light);
        let error = load_world(&mut destination, &path).unwrap_err();

        assert_eq!(error.kind(), std::io::ErrorKind::InvalidData);
        assert!(
            error
                .to_string()
                .contains(&chunk_path.to_string_lossy().to_string())
        );
        assert!(destination.get(sentinel).is_some());
    }

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_chunked_world_file_contains_metadata_only() {
    let (project, path) = test_world_path("metadata_only");

    let mut world = World::new();

    world.set_cell(WorldCoord::new(123, 456, 789), CellType::Block);

    save_world(&world, &path).unwrap();

    let content = fs::read_to_string(&path).unwrap();

    assert!(content.contains("WORLD_FORMAT 2"));
    assert!(content.contains("GRAVITY"));
    assert!(content.contains("LIGHTING"));
    assert!(content.contains("SKY"));
    assert!(content.contains("ATMOSPHERE"));
    assert!(content.contains("MOUSE"));

    assert!(!content.contains("BLOCK "));
    assert!(!content.contains("FX_BLOCK "));
    assert!(!content.contains("LIGHT "));
    assert!(!content.contains("PLAYER "));
    assert!(!content.contains("AUDIO_EMITTER "));
    assert!(!content.contains("SPAWN_POINT "));
    assert!(!content.contains("ATTRIBUTES "));

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_chunk_coordinate_math() {
    assert_eq!(
        ChunkCoord::from_world_coord(WorldCoord::new(0, 0, 0)),
        ChunkCoord::new(0, 0, 0)
    );

    assert_eq!(
        ChunkCoord::from_world_coord(WorldCoord::new(15, 15, 15)),
        ChunkCoord::new(0, 0, 0)
    );

    assert_eq!(
        ChunkCoord::from_world_coord(WorldCoord::new(16, 16, 16)),
        ChunkCoord::new(1, 1, 1)
    );

    assert_eq!(
        ChunkCoord::from_world_coord(WorldCoord::new(-1, -1, -1)),
        ChunkCoord::new(-1, -1, -1)
    );

    assert_eq!(
        ChunkCoord::local_offset(WorldCoord::new(-1, -1, -1)),
        (15, 15, 15)
    );

    assert_eq!(
        ChunkCoord::local_offset(WorldCoord::new(-16, -16, -16)),
        (0, 0, 0)
    );

    assert_eq!(
        ChunkCoord::from_world_coord(WorldCoord::new(-17, -17, -17)),
        ChunkCoord::new(-2, -2, -2)
    );

    assert_eq!(
        ChunkCoord::local_offset(WorldCoord::new(-17, -17, -17)),
        (15, 15, 15)
    );
}

#[test]
fn test_negative_coordinate_chunk_round_trip() {
    let (project, path) = test_world_path("negative_coordinates");

    let mut world = World::new();

    let coord = WorldCoord::new(-17, 34, -49);

    world.set_cell(coord, CellType::Block);

    save_world(&world, &path).unwrap();

    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();

    assert!(storage.chunk_exists(ChunkCoord::new(-2, 2, -4)));

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert!(loaded.get(coord).is_some());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_full_cell_data_round_trip() {
    let (project, path) = test_world_path("full_cell");

    let coord = WorldCoord::new(-17, 18, -19);

    let mut world = World::new();
    let id = world.set_cell(coord, CellType::AudioEmitter);

    {
        let cell = world.get_mut(coord).unwrap();

        cell.visible = false;
        cell.solid = false;
        cell.anchored = false;
        cell.texture = "custom_texture".to_string();
        cell.color_rgb = Vec3::new(0.11, 0.22, 0.33);

        cell.audio = "audio/ambient/wind.wav".to_string();
        cell.playing = true;
        cell.looped = true;
        cell.volume = 0.37;

        cell.collision_events_enabled = false;

        cell.light_color = Vec3::new(0.44, 0.55, 0.66);
        cell.light_intensity = 12.5;
        cell.light_range = 31.0;
        cell.light_shadows = false;
        cell.light_enabled = false;

        cell.entity_identity = Some("FullRoundTrip".to_string());

        cell.attributes
            .insert("health".to_string(), AttributeValue::Number(123.45));
        cell.attributes
            .insert("active".to_string(), AttributeValue::Bool(true));
        cell.attributes.insert(
            "name".to_string(),
            AttributeValue::String("A value with spaces".to_string()),
        );
    }

    save_world(&world, &path).unwrap();

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    let cell = &loaded.source(id).unwrap().cell;

    assert_eq!(cell.id, id);
    assert_eq!(cell.cell_type, CellType::AudioEmitter);
    assert!(!cell.visible);
    assert!(!cell.solid);
    assert!(!cell.anchored);
    assert_eq!(cell.texture, "custom_texture");
    assert_eq!(cell.color_rgb, Vec3::new(0.11, 0.22, 0.33));

    assert_eq!(cell.audio, "audio/ambient/wind.wav");
    assert!(cell.playing);
    assert!(cell.looped);
    assert!((cell.volume - 0.37).abs() < 1e-6);

    assert!(!cell.collision_events_enabled);

    assert_eq!(cell.light_color, Vec3::new(0.44, 0.55, 0.66));
    assert_eq!(cell.light_intensity, 12.5);
    assert_eq!(cell.light_range, 31.0);
    assert!(!cell.light_shadows);
    assert!(!cell.light_enabled);

    assert_eq!(cell.entity_identity, Some("FullRoundTrip".to_string()));

    assert_eq!(
        cell.attributes.get("health"),
        Some(&AttributeValue::Number(123.45))
    );
    assert_eq!(
        cell.attributes.get("active"),
        Some(&AttributeValue::Bool(true))
    );
    assert_eq!(
        cell.attributes.get("name"),
        Some(&AttributeValue::String("A value with spaces".to_string()))
    );

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_stale_chunk_is_removed_on_save() {
    let (project, path) = test_world_path("stale_chunk");

    let first_coord = WorldCoord::new(0, 0, 0);
    let second_coord = WorldCoord::new(16, 0, 0);

    let mut world = World::new();

    world.set_cell(first_coord, CellType::Block);
    world.set_cell(second_coord, CellType::Block);

    save_world(&world, &path).unwrap();

    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();

    assert!(storage.chunk_exists(ChunkCoord::new(0, 0, 0)));
    assert!(storage.chunk_exists(ChunkCoord::new(1, 0, 0)));

    world.cells.remove(&second_coord);
    world.rebuild_id_mapping();

    save_world(&world, &path).unwrap();

    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();
    assert!(storage.chunk_exists(ChunkCoord::new(0, 0, 0)));
    assert!(!storage.chunk_exists(ChunkCoord::new(1, 0, 0)));

    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    assert!(loaded.get(first_coord).is_some());
    assert!(loaded.get(second_coord).is_none());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_empty_world_has_no_chunk_files() {
    let (project, path) = test_world_path("empty_world");

    let world = World::new();

    save_world(&world, &path).unwrap();

    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();

    assert!(storage.list_chunks().unwrap().is_empty());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_world_save_creates_expected_chunk_files() {
    let (project, path) = test_world_path("chunk_paths");

    let mut world = World::new();

    world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

    world.set_cell(WorldCoord::new(16, 0, 0), CellType::Block);

    world.set_cell(WorldCoord::new(-1, 0, 0), CellType::Block);

    save_world(&world, &path).unwrap();

    let storage = WorldStorage::new(active_storage_root(&project)).unwrap();

    assert_eq!(
        storage
            .chunk_path(ChunkCoord::new(0, 0, 0))
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        "0_0_0.chunk"
    );

    assert_eq!(
        storage
            .chunk_path(ChunkCoord::new(1, 0, 0))
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        "1_0_0.chunk"
    );

    assert_eq!(
        storage
            .chunk_path(ChunkCoord::new(-1, 0, 0))
            .file_name()
            .unwrap()
            .to_str()
            .unwrap(),
        "-1_0_0.chunk"
    );

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_light_default_enabled_legacy_load() {
    let (project, path) = test_world_path("legacy_light");

    {
        let mut file = File::create(&path).unwrap();

        writeln!(file, "LIGHT 5 5 5 1 1 1 5 10 true").unwrap();
    }

    let mut world = World::new();
    load_world(&mut world, &path).unwrap();

    let source = world.iter_sources().next().unwrap();
    let cell = &source.cell;

    assert_eq!(cell.cell_type, CellType::Light);
    assert!(cell.light_enabled);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_spawn_point_persistence() {
    let mut world = World::new();

    let coord = WorldCoord::new(4, 8, 12);

    world.set_cell(coord, CellType::SpawnPoint);

    if let Some(cell) = world.get_mut(coord) {
        cell.visible = false;
        cell.solid = true;
        cell.anchored = false;
        cell.texture = "Block_tx".to_string();
        cell.color_rgb = Vec3::new(0.2, 0.4, 0.8);
    }

    let (project, path) = test_world_path("spawn_point");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    let loaded_cell = loaded_world.get(coord).unwrap();

    assert_eq!(loaded_cell.cell_type, CellType::SpawnPoint);
    assert!(!loaded_cell.visible);
    assert!(loaded_cell.solid);
    assert!(!loaded_cell.anchored);
    assert_eq!(loaded_cell.texture, "Block_tx");
    assert_eq!(loaded_cell.color_rgb, Vec3::new(0.2, 0.4, 0.8));

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_empty_bindings_persistence() {
    let mut world = World::new();
    world.script_bindings.clear();

    let (project, path) = test_world_path("empty_bindings");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert!(loaded_world.script_bindings.is_empty());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_one_binding_persistence() {
    let mut world = World::new();

    world
        .script_bindings
        .push(ScriptBinding::new(12345678, "scripts/player.aeo"));

    let (project, path) = test_world_path("one_binding");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert_eq!(loaded_world.script_bindings.len(), 1);
    assert_eq!(loaded_world.script_bindings[0].target_identity, 12345678);
    assert_eq!(
        loaded_world.script_bindings[0].script_path,
        "scripts/player.aeo"
    );

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_multiple_bindings_persistence() {
    let mut world = World::new();

    world
        .script_bindings
        .push(ScriptBinding::new(10000001, "scripts/player.aeo"));
    world
        .script_bindings
        .push(ScriptBinding::new(10000002, "scripts/door.aeo"));
    world
        .script_bindings
        .push(ScriptBinding::new(10000003, "scripts/enemy.aeo"));

    let (project, path) = test_world_path("multiple_bindings");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert_eq!(loaded_world.script_bindings.len(), 3);

    assert_eq!(loaded_world.script_bindings[0].target_identity, 10000001);
    assert_eq!(loaded_world.script_bindings[1].target_identity, 10000002);
    assert_eq!(loaded_world.script_bindings[2].target_identity, 10000003);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_legacy_binding_migration() {
    let (project, path) = test_world_path("legacy_binding");

    let cell_id = 55555555;

    {
        let mut file = File::create(&path).unwrap();

        writeln!(file, "GRAVITY 0 -9.81 0").unwrap();

        writeln!(file, "LIGHTING true true 0.5 -1.0 0.5 1 1 1 1 0.2").unwrap();

        writeln!(file, "SCRIPT_BINDING PlayerOne scripts/player.aeo").unwrap();

        writeln!(
            file,
            "PLAYER {} 1 1 1 true true false Block_tx 0.5 0.5 0.5 PlayerOne",
            cell_id
        )
        .unwrap();
    }

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert_eq!(loaded_world.script_bindings.len(), 1);
    assert_eq!(loaded_world.script_bindings[0].target_identity, cell_id);
    assert_eq!(
        loaded_world.script_bindings[0].script_path,
        "scripts/player.aeo"
    );

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_existing_data_and_bindings_persistence() {
    let mut world = World::new();

    let coord = WorldCoord::new(1, 2, 3);
    let cell_id = world.set_cell(coord, CellType::Block);

    world
        .script_bindings
        .push(ScriptBinding::new(cell_id, "scripts/box.aeo"));

    let (project, path) = test_world_path("existing_data_bindings");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert!(loaded_world.get(coord).is_some());

    assert_eq!(loaded_world.script_bindings.len(), 1);

    assert_eq!(loaded_world.script_bindings[0].target_identity, cell_id);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_entity_identity_persistence() {
    let mut world = World::new();

    let p_coord = WorldCoord::new(1, 1, 1);
    let n_coord = WorldCoord::new(2, 2, 2);
    let b_coord = WorldCoord::new(3, 3, 3);

    world.set_cell(p_coord, CellType::Player);

    if let Some(cell) = world.get_mut(p_coord) {
        cell.entity_identity = Some("Hero".to_string());
    }

    world.set_cell(n_coord, CellType::FxBlock);

    if let Some(cell) = world.get_mut(n_coord) {
        cell.entity_identity = Some("Merchant".to_string());
    }

    world.set_cell(b_coord, CellType::Block);

    let (project, path) = test_world_path("entity_identity");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    let p_cell = loaded_world.get(p_coord).unwrap();

    assert_eq!(p_cell.cell_type, CellType::Player);

    assert_eq!(p_cell.entity_identity, Some("Hero".to_string()));

    let n_cell = loaded_world.get(n_coord).unwrap();

    assert_eq!(n_cell.cell_type, CellType::FxBlock);

    assert_eq!(n_cell.entity_identity, Some("Merchant".to_string()));

    let b_cell = loaded_world.get(b_coord).unwrap();

    assert_eq!(b_cell.cell_type, CellType::Block);

    assert_eq!(b_cell.entity_identity, None);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_identity_not_derived_dynamically() {
    let mut world = World::new();

    let coord = WorldCoord::new(10, 20, 30);

    world.set_cell(coord, CellType::FxBlock);

    if let Some(cell) = world.get_mut(coord) {
        cell.entity_identity = Some("SpecificGuard".to_string());
    }

    let new_coord = WorldCoord::new(40, 50, 60);

    let cell_id = world.get(coord).unwrap().id;

    let cell = world.cells.remove(&coord).unwrap();

    world.cells.insert(new_coord, cell);
    world.rebuild_id_mapping();

    assert_eq!(
        world.get(new_coord).unwrap().entity_identity,
        Some("SpecificGuard".to_string())
    );

    assert_eq!(world.resolve_cell_id(cell_id), Some(new_coord));

    if let Some(cell) = world.get_mut(new_coord) {
        cell.cell_type = CellType::Player;
    }

    assert_eq!(
        world.get(new_coord).unwrap().entity_identity,
        Some("SpecificGuard".to_string())
    );

    if let Some(cell) = world.get_mut(new_coord) {
        cell.cell_type = CellType::Block;
        cell.entity_identity = None;
    }

    assert_eq!(world.get(new_coord).unwrap().entity_identity, None);
}

#[test]
fn test_legacy_identity_load() {
    let (project, path) = test_world_path("legacy_identity");

    {
        let mut file = File::create(&path).unwrap();

        writeln!(file, "PLAYER 0 0 0 true true true Default 0.5 0.5 0.5").unwrap();
    }

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    let cell = loaded_world.get(WorldCoord::new(0, 0, 0)).unwrap();

    assert_eq!(cell.cell_type, CellType::Player);
    assert_eq!(cell.entity_identity, None);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_older_data_loadability() {
    let (project, path) = test_world_path("old_format");

    {
        let mut file = File::create(&path).unwrap();

        writeln!(file, "GRAVITY 0 -9.81 0").unwrap();

        writeln!(file, "BLOCK 0 0 0 true true true Default 0.5 0.5 0.5").unwrap();
    }

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert_eq!(loaded_world.gravity.y, -9.81);

    assert!(loaded_world.get(WorldCoord::new(0, 0, 0)).is_some());

    assert!(loaded_world.script_bindings.is_empty());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_build_path_id_preservation() {
    let mut world = World::new();
    let coord = WorldCoord::new(5, 5, 5);

    let template = Cell::new_block();

    assert_eq!(template.id, 0);

    world.set_cell(coord, template.cell_type);

    let generated_id = world.get(coord).unwrap().id;

    assert_ne!(generated_id, 0);

    if let Some(target) = world.get_mut(coord) {
        let id = target.id;

        *target = template.clone();

        target.id = id;
    }

    assert_eq!(world.get(coord).unwrap().id, generated_id);
}

#[test]
fn test_corrupted_id_migration() {
    let (project, path) = test_world_path("corrupted_ids");

    {
        let mut file = File::create(&path).unwrap();

        writeln!(file, "BLOCK 0 10 10 10 true true true Default 0.5 0.5 0.5").unwrap();

        writeln!(file, "LIGHT 0 20 20 20 1 1 1 5 10 true true").unwrap();

        writeln!(
            file,
            "BLOCK 88888888 30 30 30 true true true Default 0.1 0.2 0.3"
        )
        .unwrap();
    }

    let mut world = World::new();
    load_world(&mut world, &path).unwrap();

    let c1 = world.get(WorldCoord::new(10, 10, 10)).unwrap();

    let c2 = &world.iter_sources().next().unwrap().cell;

    let c3 = world.get(WorldCoord::new(30, 30, 30)).unwrap();

    assert_ne!(c1.id, 0);
    assert_ne!(c2.id, 0);

    assert_ne!(c1.id, c2.id);
    assert_ne!(c1.id, c3.id);
    assert_ne!(c2.id, c3.id);

    assert_eq!(c3.id, 88888888);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_cell_attributes_defaults() {
    let cell = Cell::default();
    assert!(cell.attributes.is_empty());

    let light = Cell::new_light();
    assert!(light.attributes.is_empty());
}

#[test]
fn test_cell_attributes_clone() {
    let mut cell = Cell::default();

    cell.attributes
        .insert("health".to_string(), AttributeValue::Number(100.0));

    cell.attributes.insert(
        "name".to_string(),
        AttributeValue::String("Player".to_string()),
    );

    let cloned = cell.clone();

    assert_eq!(cloned.attributes, cell.attributes);

    assert_eq!(
        cloned.attributes.get("health"),
        Some(&AttributeValue::Number(100.0))
    );
}

#[test]
fn test_world_attributes_persistence() {
    let mut world = World::new();

    let coord = WorldCoord::new(1, 1, 1);

    let id = world.set_cell(coord, CellType::Block);

    if let Some(cell) = world.get_mut(coord) {
        cell.attributes
            .insert("score".to_string(), AttributeValue::Number(123.45));

        cell.attributes
            .insert("is_active".to_string(), AttributeValue::Bool(true));

        cell.attributes.insert(
            "description".to_string(),
            AttributeValue::String("A block with spaces".to_string()),
        );
    }

    let (project, path) = test_world_path("attributes");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    let loaded_cell = loaded_world.get(coord).unwrap();

    assert_eq!(loaded_cell.id, id);

    assert_eq!(
        loaded_cell.attributes.get("score"),
        Some(&AttributeValue::Number(123.45))
    );

    assert_eq!(
        loaded_cell.attributes.get("is_active"),
        Some(&AttributeValue::Bool(true))
    );

    assert_eq!(
        loaded_cell.attributes.get("description"),
        Some(&AttributeValue::String("A block with spaces".to_string()))
    );

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_world_multiple_attributes_persistence() {
    let mut world = World::new();

    let c1 = WorldCoord::new(0, 0, 0);
    let c2 = WorldCoord::new(1, 1, 1);

    world.set_cell(c1, CellType::Player);

    if let Some(cell) = world.get_mut(c1) {
        cell.attributes
            .insert("speed".to_string(), AttributeValue::Number(5.0));
    }

    world.set_cell(c2, CellType::FxBlock);

    if let Some(cell) = world.get_mut(c2) {
        cell.attributes
            .insert("aggro".to_string(), AttributeValue::Bool(false));

        cell.attributes.insert(
            "greeting".to_string(),
            AttributeValue::String("Hello traveler".to_string()),
        );
    }

    let (project, path) = test_world_path("multiple_attributes");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert_eq!(
        loaded_world.get(c1).unwrap().attributes.get("speed"),
        Some(&AttributeValue::Number(5.0))
    );

    assert_eq!(
        loaded_world.get(c2).unwrap().attributes.get("aggro"),
        Some(&AttributeValue::Bool(false))
    );

    assert_eq!(
        loaded_world.get(c2).unwrap().attributes.get("greeting"),
        Some(&AttributeValue::String("Hello traveler".to_string()))
    );

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_legacy_load_empty_attributes() {
    let (project, path) = test_world_path("legacy_no_attributes");

    {
        let mut file = File::create(&path).unwrap();

        writeln!(file, "GRAVITY 0 -9.81 0").unwrap();

        writeln!(file, "BLOCK 12345 0 0 0 true true true Default 0.5 0.5 0.5").unwrap();
    }

    let mut world = World::new();
    load_world(&mut world, &path).unwrap();

    let cell = world.get(WorldCoord::new(0, 0, 0)).unwrap();

    assert!(cell.attributes.is_empty());

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_world_mouse_settings_persistence() {
    let mut world = World::new();

    world.cursor_visible = true;
    world.screen_locked = false;

    let (project, path) = test_world_path("mouse_settings");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert!(loaded_world.cursor_visible);
    assert!(!loaded_world.screen_locked);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_next_cell_id_persistence() {
    let (project, path) = test_world_path("next_cell_id");

    let mut world = World::new();

    let id1 = world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

    let id2 = world.set_cell(WorldCoord::new(16, 0, 0), CellType::Block);

    assert_eq!(id2, id1 + 1);

    save_world(&world, &path).unwrap();

    let saved = fs::read_to_string(&path).unwrap();

    assert!(saved.contains(&format!("NEXT_CELL_ID {}", world.next_cell_id)));

    let mut loaded = World::new();

    load_world(&mut loaded, &path).unwrap();

    assert_eq!(loaded.next_cell_id, world.next_cell_id);

    let new_id = loaded.set_cell(WorldCoord::new(32, 0, 0), CellType::Block);

    assert_eq!(new_id, world.next_cell_id);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_allocator_does_not_depend_on_resident_cells() {
    let mut world = World::new();

    let coord = WorldCoord::new(0, 0, 0);

    let id = world.set_cell(coord, CellType::Block);

    world.cells.remove(&coord);
    world.id_to_coord.remove(&id);

    let new_id = world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);

    assert_eq!(new_id, id + 1);
}

#[test]
fn test_empty_attributes_no_line() {
    let mut world = World::new();

    world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

    let (project, path) = test_world_path("empty_attributes");

    save_world(&world, &path).unwrap();

    let content = fs::read_to_string(&path).unwrap();

    assert!(!content.contains("ATTRIBUTES"));

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_persistence_firewall_regression() {
    let mut world = World::new();

    let auth_coord = WorldCoord::new(1, 1, 1);

    let auth_id = world.set_cell(auth_coord, CellType::Block);

    let runtime_id = world.create_runtime_cell(CellType::Block);

    let runtime_coord = WorldCoord::new(2, 2, 2);

    world.move_runtime_cell(runtime_id, runtime_coord).unwrap();

    if let Some(cell) = world.runtime_cells.get_mut(&runtime_id) {
        cell.color_rgb = Vec3::new(1.0, 0.0, 0.0);

        cell.attributes
            .insert("temp".to_string(), AttributeValue::Bool(true));
    }

    let (project, path) = test_world_path("firewall");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    assert!(loaded_world.resolve_cell_id(runtime_id).is_none());

    assert!(loaded_world.get(runtime_coord).is_none());

    assert!(loaded_world.resolve_cell_id(auth_id).is_some());

    assert!(loaded_world.get(auth_coord).is_some());

    world.clear_runtime_state();

    assert!(world.runtime_cells.is_empty());

    assert!(world.resolve_cell_id(runtime_id).is_none());

    assert!(world.get(auth_coord).is_some());

    assert_eq!(world.get(auth_coord).unwrap().id, auth_id);

    fs::remove_dir_all(project).ok();
}

#[test]
fn test_audio_emitter_persistence() {
    let mut world = World::new();

    let coord = WorldCoord::new(2, 4, 6);

    let id = world.set_cell(coord, CellType::AudioEmitter);

    if let Some(cell) = world.get_mut(coord) {
        cell.audio = "battle/sword-unsheathe.wav".to_string();

        cell.playing = true;
        cell.looped = true;
        cell.volume = 0.65;
    }

    let (project, path) = test_world_path("audio_emitter");

    save_world(&world, &path).unwrap();

    let mut loaded_world = World::new();
    load_world(&mut loaded_world, &path).unwrap();

    let loaded_cell = &loaded_world.source(id).unwrap().cell;

    assert_eq!(loaded_cell.id, id);
    assert_eq!(loaded_cell.cell_type, CellType::AudioEmitter);

    assert_eq!(loaded_cell.audio, "battle/sword-unsheathe.wav");

    assert!(loaded_cell.playing);
    assert!(loaded_cell.looped);
    assert!((loaded_cell.volume - 0.65).abs() < 1e-6);

    fs::remove_dir_all(project).ok();
}

#[test]
fn freeform_sources_round_trip_without_quantization() {
    let mut world = World::new();
    let expected = [
        (CellType::Light, Vec3::new(1.0, 11.2, 0.0)),
        (CellType::AudioEmitter, Vec3::new(2.5, 1.0, 55.0)),
        (CellType::ParticleEmitter, Vec3::new(-3.75, 4.125, 9.5)),
    ];
    let ids: Vec<_> = expected
        .iter()
        .map(|(cell_type, position)| world.add_source(*cell_type, *position))
        .collect();
    let (project, path) = test_world_path("freeform_sources");

    save_world(&world, &path).unwrap();
    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    for ((cell_type, position), id) in expected.into_iter().zip(ids) {
        let source = loaded
            .source(id)
            .expect("source should survive save/reopen");
        assert_eq!(source.cell.cell_type, cell_type);
        assert_eq!(source.position, position);
    }
    assert!(loaded.active_blocks().is_empty());
    fs::remove_dir_all(project).ok();
}

#[test]
fn legacy_grid_sources_migrate_to_voxel_centers() {
    let mut world = World::new();
    let legacy = [
        (CellType::Light, WorldCoord::new(1, 11, 0)),
        (CellType::AudioEmitter, WorldCoord::new(2, 1, 55)),
        (CellType::ParticleEmitter, WorldCoord::new(-4, 4, 9)),
    ];
    let ids: Vec<_> = legacy
        .iter()
        .map(|(cell_type, coord)| world.set_cell(*coord, *cell_type))
        .collect();
    let (project, path) = test_world_path("legacy_source_migration");

    save_world(&world, &path).unwrap();
    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();

    for ((_, coord), id) in legacy.into_iter().zip(ids) {
        let source = loaded.source(id).expect("legacy source should migrate");
        assert_eq!(
            source.position,
            Vec3::new(
                coord.x as f32 + 0.5,
                coord.y as f32 + 0.5,
                coord.z as f32 + 0.5
            )
        );
        assert!(loaded.get(coord).is_none());
    }
    fs::remove_dir_all(project).ok();
}

#[test]
fn independently_authored_water_bodies_and_ocean_setting_survive_save_reopen() {
    use crate::world::terrain::{WaterBody, WaterBodyKind};
    let (project, path) = test_world_path("water_bodies_round_trip");
    let mut world = World::new();
    world.terrain.config.ocean_enabled = false;
    world.terrain.config.water_bodies.push(WaterBody::new(
        41,
        WaterBodyKind::Lake,
        [16.0, 16.0],
        7.5,
    ));
    world.terrain.config.water_bodies.push(WaterBody::new(
        42,
        WaterBodyKind::River,
        [10.0, 14.0],
        7.0,
    ));
    world.terrain.config.water_bodies[1].points[1].width = 9.0;
    world
        .generate_authored_terrain(TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [32, 24, 32],
        })
        .unwrap();
    let authored_before = world.terrain.authored.clone();
    let bodies_before = world.terrain.config.water_bodies.clone();
    save_world(&world, &path).unwrap();
    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    assert!(!loaded.terrain.config.ocean_enabled);
    assert_eq!(loaded.terrain.config.water_bodies, bodies_before);
    assert_eq!(loaded.terrain.authored, authored_before);
    fs::remove_dir_all(project).ok();
}

#[test]
fn water_material_assignment_survives_real_world_save_and_cold_reopen() {
    use crate::project::material::*;
    use crate::world::terrain::{WaterBody, WaterBodyKind};
    let (project, path) = test_world_path("water_material_save_reopen");
    fs::create_dir_all(&project).unwrap();
    let doc = MaterialDocument::new(MaterialDefinition::Asset {
        material: MaterialOutput::Water(crate::world::terrain::TerrainWaterSettings::default()),
    });
    save(&project, "alpine_water", &doc, None).unwrap();
    let catalog = MaterialCatalog::load(&project);
    let mut world = World::new();
    let mut body = WaterBody::new(8, WaterBodyKind::River, [10.0, 14.0], 7.0);
    body.assign_material(&catalog, &doc.id).unwrap();
    world.terrain.config.water_bodies.push(body);
    world.terrain.config.water_material_asset = Some(doc.id.clone());
    save_world(&world, &path).unwrap();
    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    let reopened = MaterialCatalog::load(&project);
    assert_eq!(
        loaded.terrain.config.water_material_asset.as_deref(),
        Some(doc.id.as_str())
    );
    assert_eq!(
        loaded.terrain.config.resolved_water(&reopened),
        catalog.water(&doc.id).unwrap()
    );
    assert_eq!(
        loaded.terrain.config.water_bodies[0]
            .material_asset
            .as_deref(),
        Some(doc.id.as_str())
    );
    assert_eq!(
        loaded.terrain.config.water_bodies[0]
            .resolved_appearance(&reopened, &loaded.terrain.config.water),
        catalog.water(&doc.id).unwrap()
    );
    let expected = fs::read(file_path(&project, "alpine_water").unwrap()).unwrap();
    let mut edited = doc.clone();
    if let MaterialDefinition::Asset {
        material: MaterialOutput::Water(look),
    } = &mut edited.definition
    {
        look.wave_height = 1.7;
    }
    save(&project, "alpine_water", &edited, Some(&expected)).unwrap();
    save_world(&world, &path).unwrap();
    fs::remove_file(file_path(&project, "alpine_water").unwrap()).unwrap();
    let mut cold = World::new();
    load_world(&mut cold, &path).unwrap();
    assert_eq!(
        cold.terrain
            .config
            .resolved_water(&MaterialCatalog::default())
            .wave_height,
        1.7
    );
    assert_eq!(
        cold.terrain.config.water_bodies[0]
            .resolved_appearance(&MaterialCatalog::default(), &cold.terrain.config.water)
            .wave_height,
        1.7
    );
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn global_horizon_mirage_defaults_off_and_survives_reopen() {
    let (project, path) = test_world_path("global_horizon_mirage");
    let mut world = World::new();
    assert!(!world.sky.horizon_mirage_enabled);
    world.sky.horizon_mirage_enabled = true;
    save_world(&world, &path).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("HORIZON_MIRAGE true"));

    let mut reopened = World::new();
    load_world(&mut reopened, &path).unwrap();
    assert!(reopened.sky.horizon_mirage_enabled);

    fs::write(&path, saved.replace("HORIZON_MIRAGE true\n", "")).unwrap();
    let mut legacy = World::new();
    load_world(&mut legacy, &path).unwrap();
    assert!(!legacy.sky.horizon_mirage_enabled);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn horizon_mirage_strength_round_trip_with_safe_legacy_defaults() {
    let (project, path) = test_world_path("mirage_strength");
    let mut world = World::new();
    assert!(!world.sky.horizon_mirage_enabled);
    assert_eq!(world.sky.effective_mirage_strength(), 1.0);
    world.sky.horizon_mirage_enabled = true;
    world.sky.horizon_mirage_strength = 1.6;
    save_world(&world, &path).unwrap();
    let saved = fs::read_to_string(&path).unwrap();
    assert!(saved.contains("HORIZON_MIRAGE_STRENGTH 1.6"));

    let mut reopened = World::new();
    load_world(&mut reopened, &path).unwrap();
    assert!(reopened.sky.horizon_mirage_enabled);
    assert!((reopened.sky.horizon_mirage_strength - 1.6).abs() < 0.0001);

    fs::write(&path, saved.replace("HORIZON_MIRAGE_STRENGTH 1.6\n", "")).unwrap();
    let mut legacy = World::new();
    load_world(&mut legacy, &path).unwrap();
    assert!(legacy.sky.horizon_mirage_enabled);
    assert_eq!(legacy.sky.effective_mirage_strength(), 1.0);

    for (raw_strength, expected) in [("nan", 1.0), ("9999", 2.0), ("-3", 0.0)] {
        fs::write(
            &path,
            saved.replace(
                "HORIZON_MIRAGE_STRENGTH 1.6",
                &format!("HORIZON_MIRAGE_STRENGTH {raw_strength}"),
            ),
        )
        .unwrap();
        let mut loaded = World::new();
        load_world(&mut loaded, &path).unwrap();
        assert_eq!(loaded.sky.effective_mirage_strength(), expected);
    }
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn actor_tags_round_trip_and_play_overlays_never_persist() {
    let (project, path) = test_world_path("actor_tags_round_trip");
    let mut world = World::new();
    let id = world.add_actor("Chest", Vec3::new(1.0, 2.0, 3.0));
    world
        .actors
        .get_mut(&id)
        .unwrap()
        .tags
        .insert("loot".into());
    assert_eq!(world.set_runtime_actor_tag(id, "hidden", true), Some(true));
    save_world(&world, &path).unwrap();
    let source = fs::read_to_string(&path).unwrap();
    assert!(source.contains("ACTOR_TAGS "));
    assert!(!source.contains("hidden"));
    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    assert_eq!(loaded.actor_ids_with_tag("loot"), vec![id]);
    assert!(loaded.actor_ids_with_tag("hidden").is_empty());
    fs::write(
        &path,
        source
            .lines()
            .filter(|line| !line.starts_with("ACTOR_TAGS "))
            .collect::<Vec<_>>()
            .join("\n")
            + "\n",
    )
    .unwrap();
    let mut legacy = World::new();
    load_world(&mut legacy, &path).unwrap();
    assert!(legacy.actors.get(&id).unwrap().tags.is_empty());
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn spaced_names_and_paths_survive_world_roundtrip() {
    let (project, path) = test_world_path("spaced_fields");
    let mut world = World::new();
    world
        .actors
        .insert(21, crate::world::Actor::new(21, "Front Door", Vec3::ZERO));
    world.sky.texture = "Sky Textures/Sunny Horizon.png".into();
    world.sky.material_asset = Some("My Sky Material".into());
    world.selected_character = "Main Character".into();
    world.selected_controller = "Player One".into();
    world.selected_camera = "Main Camera".into();
    world
        .script_bindings
        .push(ScriptBinding::new(5, "scripts/My Door.aeo"));
    world.disabled_scripts.push("scripts/Old Door.aeo".into());
    save_world(&world, &path).unwrap();
    let mut loaded = World::new();
    load_world(&mut loaded, &path).unwrap();
    assert_eq!(loaded.actors.get(&21).unwrap().identity, "Front Door");
    assert_eq!(loaded.sky.texture, world.sky.texture);
    assert_eq!(loaded.sky.material_asset, world.sky.material_asset);
    assert_eq!(loaded.selected_character, world.selected_character);
    assert_eq!(loaded.selected_controller, world.selected_controller);
    assert_eq!(loaded.selected_camera, world.selected_camera);
    assert_eq!(loaded.script_bindings[0].script_path, "scripts/My Door.aeo");
    assert_eq!(loaded.disabled_scripts, world.disabled_scripts);
    fs::remove_dir_all(project).unwrap();
}

#[test]
fn interrupted_generation_does_not_corrupt_last_committed_world() {
    let (project, path) = test_world_path("generation_commit");
    let mut first = World::new();
    let retained = WorldCoord::new(1, 0, 1);
    first.set_cell(retained, CellType::Block);
    save_world(&first, &path).unwrap();
    let prior_manifest = fs::read(&path).unwrap();
    let prior_root = active_storage_root(&project);
    assert!(prior_root.join("chunks").is_dir());

    let mut replacement = World::new();
    let newer = WorldCoord::new(32, 0, 32);
    replacement.set_cell(newer, CellType::Block);
    save_world(&replacement, &path).unwrap();
    assert_ne!(prior_manifest, fs::read(&path).unwrap());

    fs::write(&path, &prior_manifest).unwrap();
    let mut reopened = World::new();
    load_world(&mut reopened, &path).unwrap();
    assert!(reopened.get(retained).is_some());
    assert!(reopened.get(newer).is_none());
    assert!(prior_root.join("chunks").is_dir());
    fs::remove_dir_all(project).ok();
}
