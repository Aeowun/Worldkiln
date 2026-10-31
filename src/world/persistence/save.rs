// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use super::WORLD_FORMAT_VERSION;
use crate::world::cell::{Cell, ChunkCoord};
use crate::world::coordinate::WorldCoord;
use crate::world::storage::{StoredActor, StoredSource, WorldStorage};
use crate::world::terrain::storage::TerrainStorage;
use crate::world::world::World;

pub fn save_world(world: &World, path: &Path) -> std::io::Result<()> {
    let _diagnostic_operation =
        crate::engine::development_capture::operation("filesystem", "serialize_world", false);
    let project_root = path.parent().unwrap_or_else(|| Path::new("."));
    let generations = project_root.join(".world_generations");
    std::fs::create_dir_all(&generations)?;
    let mut generation = None;
    for attempt in 0..64u32 {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(std::io::Error::other)?
            .as_nanos();
        let id = format!("{nanos:x}-{:x}-{attempt:x}", std::process::id());
        let directory = generations.join(&id);
        match std::fs::create_dir(&directory) {
            Ok(()) => {
                generation = Some((id, directory));
                break;
            }
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    let (generation_id, generation_root) = generation.ok_or_else(|| {
        std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "Unable to reserve world generation",
        )
    })?;
    let storage = WorldStorage::new(&generation_root)?;
    let terrain_storage = TerrainStorage::for_generation(&generation_root, project_root);

    let mut file = Vec::new();

    writeln!(file, "WORLD_FORMAT {}", WORLD_FORMAT_VERSION)?;
    writeln!(file, "STORAGE_GENERATION {}", generation_id)?;
    writeln!(file, "NEXT_CELL_ID {}", world.next_cell_id)?;
    writeln!(file, "NEXT_ACTOR_ID {}", world.next_actor_id)?;
    writeln!(
        file,
        "NAVMESH_SETTINGS {}",
        serde_json::to_string(&world.navmesh_settings)?
    )?;
    for volume in &world.navmesh_volumes {
        writeln!(file, "NAVMESH_VOLUME {}", serde_json::to_string(volume)?)?;
    }
    writeln!(
        file,
        "RENDER_DISTANCE {} {}",
        world.render_distance.editor_chunks, world.render_distance.play_chunks
    )?;

    writeln!(
        file,
        "MAX_WALKABLE_SLOPE {}",
        world.max_walkable_slope_degrees
    )?;
    writeln!(file, "CHARACTER_WALK_SPEED {}", world.character_walk_speed)?;
    writeln!(
        file,
        "ANIMATION_BUDGET {} {}",
        world.animation_budget_enabled, world.animation_pose_budget_ms
    )?;
    writeln!(
        file,
        "GRAVITY {} {} {}",
        world.gravity.x, world.gravity.y, world.gravity.z
    )?;

    writeln!(
        file,
        "LIGHTING {} {} {} {} {} {} {} {} {} {} {}",
        world.lighting.shadows_enabled,
        world.lighting.global_light_enabled,
        world.lighting.global_light_direction.x,
        world.lighting.global_light_direction.y,
        world.lighting.global_light_direction.z,
        world.lighting.global_light_color.x,
        world.lighting.global_light_color.y,
        world.lighting.global_light_color.z,
        world.lighting.global_light_intensity,
        world.lighting.ambient_intensity,
        world.lighting.point_shadow_camera_distance
    )?;

    let sky_texture = if world.sky.texture.is_empty() {
        "None".to_string()
    } else {
        super::encode_token(&world.sky.texture)
    };
    writeln!(file, "SKY {} {}", world.sky.enabled, sky_texture)?;

    if let Some(material_id) = world.sky.material_asset.as_deref() {
        writeln!(file, "SKY_MATERIAL {}", super::encode_token(material_id))?;
    }

    writeln!(
        file,
        "ATMOSPHERE {} {} {} {} {} {} {}",
        world.sky.atmosphere_enabled,
        world.sky.fog_start_studs,
        world.sky.fog_end_studs,
        world.sky.fog_color.x,
        world.sky.fog_color.y,
        world.sky.fog_color.z,
        world.sky.horizon_strength
    )?;

    writeln!(file, "HORIZON_MIRAGE {}", world.sky.horizon_mirage_enabled)?;
    writeln!(
        file,
        "HORIZON_MIRAGE_STRENGTH {}",
        world.sky.effective_mirage_strength()
    )?;

    writeln!(
        file,
        "MOUSE {} {}",
        world.cursor_visible, world.screen_locked
    )?;

    for binding in &world.script_bindings {
        writeln!(
            file,
            "SCRIPT_BINDING {} {} {}",
            binding.target_identity,
            super::encode_token(&binding.script_path),
            binding.enabled
        )?;
    }

    let mut sources: Vec<_> = world.iter_sources().collect();
    sources.sort_by_key(|source| source.cell.id);
    for source in sources {
        let json = serde_json::to_string(&StoredSource::from_source(source)).map_err(|error| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                format!("Failed to serialize authored source: {error}"),
            )
        })?;
        writeln!(file, "SOURCE {json}")?;
    }

    for actor in world.actors.values() {
        let stored = StoredActor::from_actor(actor);

        writeln!(
            file,
            "ACTOR {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {} {}",
            stored.id,
            if stored.identity.trim().is_empty() {
                "None".to_string()
            } else {
                super::encode_token(stored.identity.trim())
            },
            stored.position[0],
            stored.position[1],
            stored.position[2],
            stored.rotation[0],
            stored.rotation[1],
            stored.rotation[2],
            stored.scale[0],
            stored.scale[1],
            stored.scale[2],
            stored.visible,
            stored.enabled,
            stored
                .package
                .as_deref()
                .map(super::encode_token)
                .unwrap_or_else(|| "None".to_string()),
            stored.physics_enabled,
            stored.physics_solid,
            stored.physics_anchored,
            stored.physics_collision_enabled,
            stored.physics_mass,
            stored.physics_collision_size[0],
            stored.physics_collision_size[1],
            stored.physics_collision_size[2],
            stored.physics_collision_offset[0],
            stored.physics_collision_offset[1],
            stored.physics_collision_offset[2],
        )?;

        if stored.physics_collision_mode_explicit
            || stored.physics_collision_mode
                != crate::engine::collision_geometry::CollisionMode::Box
        {
            writeln!(
                file,
                "ACTOR_COLLISION_MODE {} {}",
                stored.id,
                match stored.physics_collision_mode {
                    crate::engine::collision_geometry::CollisionMode::Box => "box",
                    crate::engine::collision_geometry::CollisionMode::Auto => "auto",
                    crate::engine::collision_geometry::CollisionMode::Disabled => "disabled",
                }
            )?;
        }
        if !stored.tags.is_empty() {
            let json_tags = serde_json::to_string(&stored.tags).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to serialize Actor tags: {error}"),
                )
            })?;
            writeln!(file, "ACTOR_TAGS {} {}", stored.id, json_tags)?;
        }

        if !stored.attributes.is_empty() {
            let json_attrs = serde_json::to_string(&stored.attributes).map_err(|error| {
                std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("Failed to serialize Actor attributes: {error}"),
                )
            })?;

            writeln!(file, "ACTOR_ATTRIBUTES {} {}", stored.id, json_attrs)?;
        }
    }

    for binding in &world.actor_script_bindings {
        writeln!(
            file,
            "ACTOR_SCRIPT_BINDING {} {} {}",
            binding.actor_id,
            super::encode_token(&binding.script_path),
            binding.enabled
        )?;
    }

    for path in &world.disabled_scripts {
        writeln!(file, "DISABLED_SCRIPT {}", super::encode_token(path))?;
    }

    let character_selection = if world.selected_character.is_empty() {
        "None"
    } else {
        world.selected_character.as_str()
    };
    writeln!(
        file,
        "SELECTED_CHARACTER {}",
        if character_selection == "None" {
            "None".to_string()
        } else {
            super::encode_token(character_selection)
        }
    )?;
    let scale = world.player_character_scale;
    writeln!(
        file,
        "PLAYER_CHARACTER_SCALE {} {} {}",
        scale.x, scale.y, scale.z
    )?;

    writeln!(
        file,
        "SELECTED_CONTROLLER {}",
        super::encode_token(&world.selected_controller)
    )?;

    writeln!(
        file,
        "SELECTED_CAMERA {}",
        super::encode_token(&world.selected_camera)
    )?;

    let mut cells_by_chunk: BTreeMap<ChunkCoord, Vec<(WorldCoord, Cell)>> = BTreeMap::new();

    for coord in world.active_blocks() {
        if let Some(cell) = world.get(coord) {
            let chunk_coord = ChunkCoord::from_world_coord(coord);

            cells_by_chunk
                .entry(chunk_coord)
                .or_default()
                .push((coord, cell.clone()));
        }
    }

    for (chunk_coord, cells) in &cells_by_chunk {
        let stored_chunk = WorldStorage::create_stored_chunk(*chunk_coord, cells.clone())?;

        storage.save_chunk(&stored_chunk)?;
    }

    writeln!(file, "CHUNK_MANIFEST 1")?;
    for chunk_coord in cells_by_chunk.keys() {
        writeln!(
            file,
            "CHUNK_COORD {} {} {}",
            chunk_coord.x, chunk_coord.y, chunk_coord.z
        )?;
    }

    terrain_storage.save(&world.terrain)?;

    crate::project::atomic_file::write_atomically(path, &file)?;

    Ok(())
}
