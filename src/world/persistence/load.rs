// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

use glam::Vec3;
use serde_json;

use super::WORLD_FORMAT_VERSION;
use crate::scripting::binding::{ActorScriptBinding, ScriptBinding};
use crate::world::ChunkCoord;
use crate::world::cell::{Cell, CellType};
use crate::world::coordinate::WorldCoord;
use crate::world::storage::{StoredSource, WorldStorage};
use crate::world::terrain::storage::TerrainStorage;
use crate::world::world::World;

fn parse_block_properties(cell: &mut Cell, parts: &[&str], offset: usize) {
    cell.visible = parts[offset + 4].parse::<bool>().unwrap_or(true);
    cell.solid = parts[offset + 5].parse::<bool>().unwrap_or(true);
    cell.anchored = parts[offset + 6].parse::<bool>().unwrap_or(true);
    cell.texture = parts[offset + 7].to_string();

    let r = parts[offset + 8].parse::<f32>().unwrap_or(0.5);
    let g = parts[offset + 9].parse::<f32>().unwrap_or(0.5);
    let b = parts[offset + 10].parse::<f32>().unwrap_or(0.5);

    cell.color_rgb = Vec3::new(r, g, b);
}

fn parse_actor_record(
    world: &mut World,
    parts: &[&str],
    format_version: u32,
) -> std::io::Result<bool> {
    if parts[0] != "ACTOR" {
        return Ok(false);
    }
    let strict = format_version >= WORLD_FORMAT_VERSION;
    let invalid = |field: &str| {
        std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            format!("Invalid ACTOR {field} in world format {format_version}"),
        )
    };
    if parts.len() < 20 {
        return if strict {
            Err(invalid("record length"))
        } else {
            Ok(false)
        };
    }
    let id = match parts[1].parse::<u64>() {
        Ok(value) => value,
        Err(_) if !strict => return Ok(true),
        Err(_) => return Err(invalid("ID")),
    };
    let number = |column: usize, default: f32| -> std::io::Result<f32> {
        match parts[column].parse::<f32>() {
            Ok(value) if value.is_finite() => Ok(value),
            _ if !strict => Ok(default),
            _ => Err(invalid(&format!("numeric column {column} for Actor {id}"))),
        }
    };
    let identity = if parts[2] == "None" {
        String::new()
    } else {
        super::decode_token(parts[2])
    };
    let position = Vec3::new(number(3, 0.0)?, number(4, 0.0)?, number(5, 0.0)?);
    let rotation = Vec3::new(number(6, 0.0)?, number(7, 0.0)?, number(8, 0.0)?);
    let scale = Vec3::new(number(9, 1.0)?, number(10, 1.0)?, number(11, 1.0)?);

    let mut actor = crate::world::Actor::new(id, identity, position);
    actor.rotation = rotation;
    actor.scale = scale;
    actor.visible = parts[12].parse::<bool>().unwrap_or(true);
    actor.enabled = parts[13].parse::<bool>().unwrap_or(true);
    actor.package = if parts[14] == "None" {
        None
    } else {
        Some(super::decode_token(parts[14]))
    };
    actor.physics.enabled = parts[15].parse::<bool>().unwrap_or(true);
    actor.physics.solid = parts[16].parse::<bool>().unwrap_or(true);
    actor.physics.anchored = parts[17].parse::<bool>().unwrap_or(true);
    actor.physics.collision_enabled = parts[18].parse::<bool>().unwrap_or(true);
    actor.physics.mass = number(19, 1.0)?;

    if parts.len() >= 26 {
        actor.physics.collision_size =
            Vec3::new(number(20, 1.0)?, number(21, 1.0)?, number(22, 1.0)?);
        actor.physics.collision_offset =
            Vec3::new(number(23, 0.0)?, number(24, 0.0)?, number(25, 0.0)?);
    } else if strict && parts.len() > 20 {
        return Err(invalid("partial collision tail"));
    }

    world.observe_authored_actor_id(id);
    world.actors.insert(id, actor);
    Ok(true)
}

pub fn load_world(world: &mut World, path: &Path) -> std::io::Result<()> {
    let mut staged_world = World::new();
    load_world_into(&mut staged_world, path, true)?;
    *world = staged_world;
    Ok(())
}

pub(crate) fn load_scene_without_terrain(world: &mut World, path: &Path) -> std::io::Result<()> {
    let mut staged = World::new();
    load_world_into(&mut staged, path, false)?;
    *world = staged;
    Ok(())
}

fn load_world_into(world: &mut World, path: &Path, load_terrain: bool) -> std::io::Result<()> {
    *world = World::new();

    let mut has_sky_line = false;
    let mut world_format_version = 1;
    let mut expected_chunks = None::<std::collections::BTreeSet<ChunkCoord>>;
    let mut storage_generation = None::<String>;

    if !path.exists() {
        return Ok(());
    }

    let file = File::open(path)?;
    let reader = BufReader::new(file);

    let mut legacy_bindings = Vec::new();

    for line in reader.lines() {
        let line = line?;

        if let Some(json) = line.strip_prefix("SOURCE ") {
            let stored: StoredSource = serde_json::from_str(json).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid authored source record: {error}"),
                )
            })?;
            world.insert_source(stored.into_source());
            continue;
        }

        let parts: Vec<&str> = line.split_whitespace().collect();

        if parts.is_empty() {
            continue;
        }

        if parts[0] == "WORLD_FORMAT" {
            if parts.len() != 2 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid WORLD_FORMAT record: expected one version number",
                ));
            }
            world_format_version = parts[1].parse::<u32>().map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid WORLD_FORMAT version '{}': {error}", parts[1]),
                )
            })?;
            if world_format_version > WORLD_FORMAT_VERSION {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "Unsupported world format version {world_format_version}; maximum supported is {WORLD_FORMAT_VERSION}"
                    ),
                ));
            }
            continue;
        }

        if parts[0] == "STORAGE_GENERATION" {
            if parts.len() != 2
                || storage_generation.is_some()
                || parts[1].is_empty()
                || !parts[1]
                    .bytes()
                    .all(|byte| byte.is_ascii_hexdigit() || byte == b'-')
            {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid STORAGE_GENERATION record",
                ));
            }
            storage_generation = Some(parts[1].to_owned());
            continue;
        }

        if parts[0] == "CHUNK_MANIFEST" {
            if parts.len() != 2 || parts[1] != "1" || expected_chunks.is_some() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid or duplicate CHUNK_MANIFEST record",
                ));
            }
            expected_chunks = Some(std::collections::BTreeSet::new());
            continue;
        }
        if parts[0] == "CHUNK_COORD" {
            if parts.len() != 4 {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Invalid CHUNK_COORD record: expected x, y, and z",
                ));
            }
            let coord = ChunkCoord::new(
                parts[1]
                    .parse::<i32>()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
                parts[2]
                    .parse::<i32>()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
                parts[3]
                    .parse::<i32>()
                    .map_err(|error| std::io::Error::new(std::io::ErrorKind::InvalidData, error))?,
            );
            let Some(expected) = expected_chunks.as_mut() else {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "CHUNK_COORD appeared before CHUNK_MANIFEST",
                ));
            };
            if !expected.insert(coord) {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Duplicate chunk coordinate in manifest: {coord:?}"),
                ));
            }
            continue;
        }

        if let Some(json) = line.strip_prefix("NAVMESH_SETTINGS ") {
            world.navmesh_settings = serde_json::from_str(json).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid NavMesh settings: {error}"),
                )
            })?;
            continue;
        }
        if let Some(json) = line.strip_prefix("NAVMESH_VOLUME ") {
            let volume = serde_json::from_str(json).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Invalid NavMesh volume: {error}"),
                )
            })?;
            world.navmesh_volumes.push(volume);
            continue;
        }
        if parts[0] == "NEXT_CELL_ID" && parts.len() >= 2 {
            if let Ok(next_id) = parts[1].parse::<u64>() {
                world.next_cell_id = next_id;
            }
            continue;
        }

        if parts[0] == "NEXT_ACTOR_ID" && parts.len() >= 2 {
            if let Ok(next_id) = parts[1].parse::<u64>() {
                world.next_actor_id = next_id;
            }
            continue;
        }

        if parts[0] == "RENDER_DISTANCE" && parts.len() >= 3 {
            world.render_distance.editor_chunks = parts[1].parse::<i32>().unwrap_or(0).max(0);
            world.render_distance.play_chunks = parts[2].parse::<i32>().unwrap_or(0).max(0);
            continue;
        }

        if parts[0] == "ANIMATION_BUDGET" && parts.len() >= 3 {
            if let Ok(enabled) = parts[1].parse::<bool>() {
                world.animation_budget_enabled = enabled;
            }
            if let Ok(ms) = parts[2].parse::<f32>() {
                if ms.is_finite() {
                    world.animation_pose_budget_ms = ms.clamp(0.25, 8.0);
                }
            }
            continue;
        }
        if parts[0] == "CHARACTER_WALK_SPEED" && parts.len() >= 2 {
            if let Ok(speed) = parts[1].parse::<f32>() {
                if speed.is_finite() {
                    world.character_walk_speed = speed.clamp(0.0, 50.0);
                }
            }
            continue;
        }
        if parts[0] == "MAX_WALKABLE_SLOPE" && parts.len() >= 2 {
            if let Ok(angle) = parts[1].parse::<f32>() {
                if angle.is_finite() {
                    world.max_walkable_slope_degrees = angle.clamp(0.0, 89.0);
                }
            }
            continue;
        }
        if parts[0] == "GRAVITY" && parts.len() >= 4 {
            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[1].parse::<f32>(),
                parts[2].parse::<f32>(),
                parts[3].parse::<f32>(),
            ) {
                world.gravity = Vec3::new(x, y, z);
            }
            continue;
        }

        if parts[0] == "LIGHTING" && parts.len() >= 11 {
            world.lighting.shadows_enabled = parts[1].parse::<bool>().unwrap_or(true);
            world.lighting.global_light_enabled = parts[2].parse::<bool>().unwrap_or(true);

            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[3].parse::<f32>(),
                parts[4].parse::<f32>(),
                parts[5].parse::<f32>(),
            ) {
                world.lighting.global_light_direction = Vec3::new(x, y, z);
            }

            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[6].parse::<f32>(),
                parts[7].parse::<f32>(),
                parts[8].parse::<f32>(),
            ) {
                world.lighting.global_light_color = Vec3::new(r, g, b);
            }

            world.lighting.global_light_intensity = parts[9].parse::<f32>().unwrap_or(1.0);
            world.lighting.ambient_intensity = parts[10].parse::<f32>().unwrap_or(0.2);
            if let Some(distance) = parts.get(11).and_then(|value| value.parse::<f32>().ok()) {
                if distance.is_finite() && distance >= 0.0 {
                    world.lighting.point_shadow_camera_distance = distance;
                }
            }

            continue;
        }

        if parts[0] == "MOUSE" && parts.len() >= 3 {
            world.cursor_visible = parts[1].parse::<bool>().unwrap_or(false);
            world.screen_locked = parts[2].parse::<bool>().unwrap_or(true);
            continue;
        }

        if parts[0] == "SKY" && parts.len() >= 2 {
            has_sky_line = true;

            world.sky.enabled = parts[1].parse::<bool>().unwrap_or(false);

            world.sky.texture = if parts.len() >= 3 && parts[2] != "None" {
                super::decode_token(parts[2])
            } else {
                String::new()
            };

            if world.sky.texture.contains("Tropical") {
                world.sky.preset = "Tropical".to_string();
            } else if world.sky.texture.contains("Desert") {
                world.sky.preset = "Desert".to_string();
            } else if world.sky.texture.contains("Snowy") {
                world.sky.preset = "Snowy".to_string();
            } else if world.sky.texture.contains("Mars") {
                world.sky.preset = "Mars".to_string();
            } else {
                world.sky.preset = "Temperate".to_string();
            }

            continue;
        }

        if parts[0] == "SKY_MATERIAL" && parts.len() >= 2 {
            world.sky.material_asset = if parts[1] == "None" {
                None
            } else {
                Some(super::decode_token(parts[1]))
            };
            continue;
        }

        if parts[0] == "ATMOSPHERE" && parts.len() >= 8 {
            world.sky.atmosphere_enabled = parts[1].parse::<bool>().unwrap_or(true);
            world.sky.fog_start_studs = parts[2].parse::<f32>().unwrap_or(96.0).max(0.0);
            world.sky.fog_end_studs = parts[3]
                .parse::<f32>()
                .unwrap_or(320.0)
                .max(world.sky.fog_start_studs + 1.0);
            if let (Ok(r), Ok(g), Ok(b)) = (
                parts[4].parse::<f32>(),
                parts[5].parse::<f32>(),
                parts[6].parse::<f32>(),
            ) {
                world.sky.fog_color = Vec3::new(r, g, b).clamp(Vec3::ZERO, Vec3::ONE);
            }
            world.sky.horizon_strength = parts[7].parse::<f32>().unwrap_or(0.38).clamp(0.0, 1.0);
            continue;
        }

        if parts[0] == "HORIZON_MIRAGE" && parts.len() >= 2 {
            world.sky.horizon_mirage_enabled = parts[1].parse::<bool>().unwrap_or(false);
            continue;
        }
        if parts[0] == "HORIZON_MIRAGE_STRENGTH" && parts.len() >= 2 {
            world.sky.horizon_mirage_strength = parts[1]
                .parse::<f32>()
                .ok()
                .filter(|strength| strength.is_finite())
                .unwrap_or(1.0)
                .clamp(0.0, 2.0);
            continue;
        }

        if parts[0] == "SCRIPT_BINDING" && parts.len() >= 3 {
            let target_str = parts[1];
            let script_path = super::decode_token(parts[2]);

            if let Ok(id) = target_str.parse::<u64>() {
                let mut binding = ScriptBinding::new(id, script_path);

                if parts.len() >= 4 {
                    binding.enabled = parts[3].parse::<bool>().unwrap_or(true);
                }

                world.script_bindings.push(binding);
            } else {
                legacy_bindings.push((target_str.to_string(), script_path));
            }

            continue;
        }

        if parts[0] == "ACTOR" {
            parse_actor_record(world, &parts, world_format_version)?;
            continue;
        }

        if parts[0] == "ACTOR_COLLISION_MODE" && parts.len() == 3 {
            if let Ok(id) = parts[1].parse::<u64>() {
                if let Some(actor) = world.actors.get_mut(&id) {
                    actor.physics.collision_mode_explicit = true;
                    actor.physics.collision_mode = match parts[2] {
                        "auto" => crate::engine::collision_geometry::CollisionMode::Auto,
                        "disabled" => crate::engine::collision_geometry::CollisionMode::Disabled,
                        _ => crate::engine::collision_geometry::CollisionMode::Box,
                    };
                }
            }
            continue;
        }
        if parts[0] == "ACTOR_TAGS" && parts.len() >= 3 {
            if let Ok(id) = parts[1].parse::<u64>() {
                let json = line.splitn(3, ' ').nth(2).unwrap_or("");
                if let (Some(actor), Ok(tags)) = (
                    world.actors.get_mut(&id),
                    serde_json::from_str::<std::collections::BTreeSet<String>>(json),
                ) {
                    actor.tags = tags;
                }
            }
            continue;
        }

        if parts[0] == "ACTOR_ATTRIBUTES" && parts.len() >= 3 {
            let id = parts[1].parse::<u64>().unwrap_or(0);
            let json = line.splitn(3, ' ').nth(2).unwrap_or("");

            if let Some(actor) = world.actors.get_mut(&id) {
                if let Ok(attrs) = serde_json::from_str(json) {
                    actor.attributes = attrs;
                }
            }

            continue;
        }

        if parts[0] == "ACTOR_SCRIPT_BINDING" && parts.len() >= 3 {
            if let Ok(actor_id) = parts[1].parse::<u64>() {
                let enabled = if parts.len() >= 4 {
                    parts[3].parse::<bool>().unwrap_or(true)
                } else {
                    true
                };

                world.actor_script_bindings.push(ActorScriptBinding {
                    actor_id,
                    script_path: super::decode_token(parts[2]),
                    enabled,
                });
            }

            continue;
        }

        if parts[0] == "DISABLED_SCRIPT" && parts.len() >= 2 {
            world.disabled_scripts.push(super::decode_token(parts[1]));
            continue;
        }

        if parts[0] == "PLAYER_CHARACTER_SCALE" && parts.len() == 4 {
            if let (Ok(x), Ok(y), Ok(z)) = (
                parts[1].parse::<f32>(),
                parts[2].parse::<f32>(),
                parts[3].parse::<f32>(),
            ) {
                let candidate = Vec3::new(x, y, z);
                if candidate.is_finite()
                    && candidate.min_element() >= 0.01
                    && candidate.max_element() <= 100.0
                {
                    world.player_character_scale = candidate;
                }
            }
            continue;
        }

        if parts[0] == "SELECTED_CHARACTER" && parts.len() >= 2 {
            world.selected_character = match super::decode_token(parts[1]).as_str() {
                "None" => String::new(),
                "custom" | "custom_soldier" => crate::character::package::available_package_names()
                    .into_iter()
                    .next()
                    .unwrap_or_default(),
                other => super::decode_token(other),
            };
            continue;
        }

        if parts[0] == "SELECTED_CONTROLLER" {
            world.selected_controller = super::decode_token(parts.get(1).copied().unwrap_or(""));
            continue;
        }

        if parts[0] == "SELECTED_CAMERA" {
            world.selected_camera = super::decode_token(parts.get(1).copied().unwrap_or(""));
            continue;
        }

        if parts[0] == "ATTRIBUTES" && parts.len() >= 3 {
            let id = parts[1].parse::<u64>().unwrap_or(0);
            let json = line.splitn(3, ' ').nth(2).unwrap_or("");

            if let Some(coord) = world.resolve_cell_id(id) {
                if let Some(cell) = world.get_mut(coord) {
                    if let Ok(attrs) = serde_json::from_str(json) {
                        cell.attributes = attrs;
                    }
                }
            }

            continue;
        }

        if parts.len() < 4 {
            continue;
        }

        let block_type = parts[0];

        let has_id = match block_type {
            "BLOCK" | "SPAWN_POINT" | "LIGHT" | "PLAYER" | "FX_BLOCK" | "AUDIO_EMITTER"
            | "PARTICLE_EMITTER" => {
                let legacy_len = match block_type {
                    "BLOCK" | "SPAWN_POINT" | "LIGHT" => 11,
                    "PLAYER" | "FX_BLOCK" => 12,
                    "AUDIO_EMITTER" | "PARTICLE_EMITTER" => 13,
                    _ => 0,
                };

                parts.len() > legacy_len
            }
            _ => false,
        };

        let offset = if has_id { 1 } else { 0 };

        let (Ok(x), Ok(y), Ok(z)) = (
            parts[offset + 1].parse::<i32>(),
            parts[offset + 2].parse::<i32>(),
            parts[offset + 3].parse::<i32>(),
        ) else {
            continue;
        };

        let coord = WorldCoord::new(x, y, z);

        let id = if has_id {
            parts[1].parse::<u64>().unwrap_or(0)
        } else {
            0
        };

        let is_legacy = id == 0;

        let old_id = match block_type {
            "BLOCK" => world.set_cell(coord, CellType::Block),
            "FX_BLOCK" => world.set_cell(coord, CellType::FxBlock),
            "SPAWN_POINT" => world.set_cell(coord, CellType::SpawnPoint),
            "PLAYER" => world.set_cell(coord, CellType::Player),
            "LIGHT" => world.set_cell(coord, CellType::Light),
            "AUDIO_EMITTER" => world.set_cell(coord, CellType::AudioEmitter),
            "PARTICLE_EMITTER" => world.set_cell(coord, CellType::ParticleEmitter),
            _ => 0,
        };

        if !is_legacy {
            world.update_id_mapping(old_id, id, coord);
        }

        if let Some(cell) = world.get_mut(coord) {
            if !is_legacy {
                cell.id = id;
            }

            if block_type == "LIGHT" {
                cell.light_color = Vec3::new(
                    parts[offset + 4].parse::<f32>().unwrap_or(1.0),
                    parts[offset + 5].parse::<f32>().unwrap_or(1.0),
                    parts[offset + 6].parse::<f32>().unwrap_or(1.0),
                );

                cell.light_intensity = parts[offset + 7].parse::<f32>().unwrap_or(5.0);
                cell.light_range = parts[offset + 8].parse::<f32>().unwrap_or(10.0);
                cell.light_shadows = parts[offset + 9].parse::<bool>().unwrap_or(true);

                if parts.len() >= offset + 11 {
                    cell.light_enabled = parts[offset + 10].parse::<bool>().unwrap_or(true);
                }

                if parts.len() >= offset + 12 {
                    let identity = parts[offset + 11];
                    if identity != "None" {
                        cell.entity_identity = Some(identity.to_string());
                    }
                }

                if parts.len() >= offset + 13 {
                    cell.collision_events_enabled =
                        parts[offset + 12].parse::<bool>().unwrap_or(true);
                }
            } else {
                parse_block_properties(cell, &parts, offset);

                if parts.len() >= offset + 12 {
                    let identity = parts[offset + 11];
                    if identity != "None" {
                        cell.entity_identity = Some(identity.to_string());
                    }
                }

                if parts.len() >= offset + 13 {
                    cell.collision_events_enabled =
                        parts[offset + 12].parse::<bool>().unwrap_or(true);
                }
            }
        }

        let source_type = match block_type {
            "LIGHT" => Some(CellType::Light),
            "AUDIO_EMITTER" => Some(CellType::AudioEmitter),
            "PARTICLE_EMITTER" => Some(CellType::ParticleEmitter),
            _ => None,
        };
        if source_type.is_some() {
            if let Some(cell) = world.cells.remove(&coord) {
                world.insert_source(crate::world::AuthoredSource {
                    position: Vec3::new(
                        coord.x as f32 + 0.5,
                        coord.y as f32 + 0.5,
                        coord.z as f32 + 0.5,
                    ),
                    cell,
                });
                world.update_spatial_index_at(coord);
            }
        }
    }

    let project_root = path.parent().unwrap_or_else(|| Path::new("."));
    let generation_root = storage_generation
        .as_deref()
        .map(|generation| project_root.join(".world_generations").join(generation));
    let storage_root = generation_root.as_deref().unwrap_or(project_root);
    if load_terrain {
        if let Some(terrain) = TerrainStorage::for_generation(storage_root, project_root).load()? {
            world.terrain = terrain;
        }
    }

    if world_format_version >= 2 {
        let storage = WorldStorage::new(storage_root)?;
        let strict_ids = expected_chunks.is_some();
        let existing_chunks = storage.list_chunks()?;
        let chunk_coords = if let Some(expected) = expected_chunks {
            let actual: std::collections::BTreeSet<_> = existing_chunks.iter().copied().collect();
            if let Some(missing) = expected.difference(&actual).next() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotFound,
                    format!(
                        "Missing World chunk {}",
                        storage.chunk_path(*missing).display()
                    ),
                ));
            }
            if let Some(unlisted) = actual.difference(&expected).next() {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!(
                        "World chunk is not listed in world.dat: {}",
                        storage.chunk_path(*unlisted).display()
                    ),
                ));
            }
            expected.into_iter().collect()
        } else {
            existing_chunks
        };

        for chunk_coord in chunk_coords {
            let Some(stored_chunk) = storage.load_chunk(chunk_coord)? else {
                continue;
            };

            let (cells, legacy_actors) = stored_chunk.into_cells_and_legacy_actors();

            for (coord, cell) in cells {
                world.observe_authored_id(cell.id);
                if crate::world::AuthoredSource::supports(cell.cell_type) {
                    world.insert_source(crate::world::AuthoredSource {
                        position: Vec3::new(
                            coord.x as f32 + 0.5,
                            coord.y as f32 + 0.5,
                            coord.z as f32 + 0.5,
                        ),
                        cell,
                    });
                } else {
                    world.cells.insert(coord, cell);
                }
            }

            for actor in legacy_actors {
                world.observe_authored_actor_id(actor.id);
                world.actors.insert(actor.id, actor);
            }
        }

        if strict_ids {
            let mut cell_ids = std::collections::HashSet::new();
            for (coord, cell) in &world.cells {
                let chunk_path = storage.chunk_path(ChunkCoord::from_world_coord(*coord));
                if cell.id == 0 || !cell_ids.insert(cell.id) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "Invalid or duplicate Cell ID {} in {}",
                            cell.id,
                            chunk_path.display()
                        ),
                    ));
                }
            }
            for source in world.iter_sources() {
                if source.cell.id == 0 || !cell_ids.insert(source.cell.id) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "Invalid or duplicate authored source Cell ID {} in {}",
                            source.cell.id,
                            path.display()
                        ),
                    ));
                }
            }

            let mut actor_ids = std::collections::HashSet::new();
            for actor in world.actors.values() {
                if actor.id == 0 || !actor_ids.insert(actor.id) {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::InvalidData,
                        format!(
                            "Invalid or duplicate Actor ID {} in {}",
                            actor.id,
                            path.display()
                        ),
                    ));
                }
            }
        }

        world.rebuild_id_mapping();

        let mut remaining_bindings = Vec::new();

        for binding in std::mem::take(&mut world.script_bindings) {
            if world.actors.contains_key(&binding.target_identity) {
                world.actor_script_bindings.push(ActorScriptBinding {
                    actor_id: binding.target_identity,
                    script_path: binding.script_path,
                    enabled: binding.enabled,
                });
            } else {
                remaining_bindings.push(binding);
            }
        }

        world.script_bindings = remaining_bindings;

        if !has_sky_line {
            world.sky.enabled = false;
        }

        world.rebuild_actor_module_index();

        return Ok(());
    }

    world.rebuild_id_mapping();

    for (name, script_path) in legacy_bindings {
        let cell_matches: Vec<u64> = world
            .cells
            .values()
            .filter_map(|cell| {
                (cell.entity_identity.as_deref() == Some(name.as_str())).then_some(cell.id)
            })
            .collect();
        let source_matches: Vec<u64> = world
            .iter_sources()
            .filter_map(|source| {
                (source.cell.entity_identity.as_deref() == Some(name.as_str()))
                    .then_some(source.cell.id)
            })
            .collect();
        let mut cell_matches = cell_matches;
        cell_matches.extend(source_matches);

        let actor_matches: Vec<u64> = world
            .actors
            .values()
            .filter_map(|actor| (actor.identity == name).then_some(actor.id))
            .collect();

        if actor_matches.len() == 1 && cell_matches.is_empty() {
            world.actor_script_bindings.push(ActorScriptBinding {
                actor_id: actor_matches[0],
                script_path,
                enabled: true,
            });
        } else if actor_matches.is_empty() && cell_matches.len() == 1 {
            world
                .script_bindings
                .push(ScriptBinding::new(cell_matches[0], script_path));
        } else if actor_matches.is_empty() && cell_matches.is_empty() {
            println!("Stale legacy script binding found for identity '{}'", name);
        } else {
            eprintln!(
                "Migration ambiguity: multiple objects found for legacy script binding identity '{}'.",
                name
            );
        }
    }

    world.rebuild_actor_module_index();

    if !has_sky_line {
        world.sky.enabled = false;
    }

    Ok(())
}
#[cfg(test)]
mod actor_binding_compatibility_tests {
    use super::*;
    use crate::world::World;
    use glam::Vec3;
    use std::fs;

    #[test]
    fn current_actor_numeric_corruption_is_rejected_without_mutating_world() {
        let row =
            "ACTOR 42 Hero broken 2 3 0 0 0 1 1 1 true true None true true true true 1 1 1 1 0 0 0";
        let fields: Vec<_> = row.split_whitespace().collect();
        let mut world = World::new();
        assert!(parse_actor_record(&mut world, &fields, WORLD_FORMAT_VERSION).is_err());
        assert!(!world.actors.contains_key(&42));
        let mut legacy = World::new();
        assert!(parse_actor_record(&mut legacy, &fields, 1).unwrap());
        assert_eq!(legacy.actors.get(&42).unwrap().position.x, 0.0);
    }

    #[test]
    fn current_actor_collision_tail_rejects_nan_and_preserves_absent_legacy_tail() {
        let row =
            "ACTOR 42 Hero 1 2 3 0 0 0 1 1 1 true true None true true true true 1 1 NaN 1 0 0 0";
        let fields: Vec<_> = row.split_whitespace().collect();
        let mut world = World::new();
        assert!(parse_actor_record(&mut world, &fields, WORLD_FORMAT_VERSION).is_err());
        assert!(parse_actor_record(&mut world, &fields[..20], WORLD_FORMAT_VERSION).unwrap());
        assert_eq!(
            world.actors.get(&42).unwrap().physics.collision_size,
            Vec3::ONE
        );
    }

    #[test]
    fn actor_binding_round_trip_preserves_actor_id_and_binding() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_actor_binding_roundtrip_{}",
            std::process::id()
        ));

        if root.exists() {
            fs::remove_dir_all(&root).ok();
        }

        fs::create_dir_all(&root).unwrap();

        let world_path = root.join("world.dat");

        let mut world = World::new();

        let actor_id = world.add_actor("Door", Vec3::new(1.0, 2.0, 3.0));

        world.actor_script_bindings.push(ActorScriptBinding::new(
            actor_id,
            "scripts/Dungeon/doors.aeo",
        ));

        crate::world::persistence::save_world(&world, &world_path).expect("World should save");

        let mut loaded = World::new();

        load_world(&mut loaded, &world_path).expect("World should load");

        assert!(loaded.actors.contains_key(&actor_id));

        assert_eq!(
            loaded
                .actor_script_bindings
                .iter()
                .find(|binding| binding.actor_id == actor_id)
                .map(|binding| binding.script_path.as_str()),
            Some("scripts/Dungeon/doors.aeo")
        );

        fs::remove_dir_all(&root).ok();
    }
}
