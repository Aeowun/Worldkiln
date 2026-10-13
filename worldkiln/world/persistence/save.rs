use std::collections::BTreeMap;
use std::io::Write;
use std::path::Path;

use super::WORLD_FORMAT_VERSION;
use crate::world::cell::{Cell, ChunkCoord};
use crate::world::coordinate::WorldCoord;
use crate::world::storage::{StoredActor, StoredSource, WorldStorage};
use crate::world::terrain::storage::TerrainStorage;
use crate::world::world::World;

/// We save the world to a simple text format. This is easier to debug and
/// version than a binary format for now.
pub fn save_world(world: &World, path: &Path) -> std::io::Result<()> {
    let project_root = path.parent().unwrap_or_else(|| Path::new("."));
    let storage = WorldStorage::new(project_root)?;
    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
    let terrain_storage = TerrainStorage::new(project_root);

    let mut file = Vec::new();

    writeln!(file, "WORLD_FORMAT {}", WORLD_FORMAT_VERSION)?;
    writeln!(file, "NEXT_CELL_ID {}", world.next_cell_id)?;
    writeln!(file, "NEXT_ACTOR_ID {}", world.next_actor_id)?;
    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
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
        "GRAVITY {} {} {}",
        world.gravity.x, world.gravity.y, world.gravity.z
    )?;

    writeln!(
        file,
        "LIGHTING {} {} {} {} {} {} {} {} {} {}",
        world.lighting.shadows_enabled,
        world.lighting.global_light_enabled,
        world.lighting.global_light_direction.x,
        world.lighting.global_light_direction.y,
        world.lighting.global_light_direction.z,
        world.lighting.global_light_color.x,
        world.lighting.global_light_color.y,
        world.lighting.global_light_color.z,
        world.lighting.global_light_intensity,
        world.lighting.ambient_intensity
    )?;

    writeln!(
        file,
        "SKY {} {}",
        world.sky.enabled,
        if world.sky.texture.is_empty() {
            "None"
        } else {
            &world.sky.texture
        }
    )?;

    // Worldkiln 0.9.3 | WO 0.9.3-18 | 2026-10-07 13:31 -05:00
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

    writeln!(
        file,
        "MOUSE {} {}",
        world.cursor_visible, world.screen_locked
    )?;

    for binding in &world.script_bindings {
        writeln!(
            file,
            "SCRIPT_BINDING {} {} {}",
            binding.target_identity, binding.script_path, binding.enabled
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
                "None"
            } else {
                stored.identity.trim()
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
            stored.package.as_deref().unwrap_or("None"),
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
            binding.actor_id, binding.script_path, binding.enabled
        )?;
    }

    for path in &world.disabled_scripts {
        writeln!(file, "DISABLED_SCRIPT {}", path)?;
    }

    // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 18:13 -05:00
    // A blank selection is the explicit no-Character setting; persist a
    // token because the legacy World text parser splits on whitespace.
    let character_selection = if world.selected_character.is_empty() {
        "None"
    } else {
        world.selected_character.as_str()
    };
    writeln!(file, "SELECTED_CHARACTER {character_selection}")?;

    writeln!(file, "SELECTED_CONTROLLER {}", world.selected_controller)?;

    writeln!(file, "SELECTED_CAMERA {}", world.selected_camera)?;

    // Group all authored Cells by their owning chunk.
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

    // Remove chunk files that no longer have any authored Cells.
    let existing_chunks = storage.list_chunks()?;

    for chunk_coord in existing_chunks {
        if !cells_by_chunk.contains_key(&chunk_coord) {
            storage.delete_chunk(chunk_coord)?;
        }
    }

    // Write current Cell chunks.
    for (chunk_coord, cells) in &cells_by_chunk {
        let stored_chunk = WorldStorage::create_stored_chunk(*chunk_coord, cells.clone())?;

        storage.save_chunk(&stored_chunk)?;
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:53 -05:00
    // The manifest lets loads distinguish an intentionally empty World from a
    // project whose referenced chunk file was lost or moved.
    writeln!(file, "CHUNK_MANIFEST 1")?;
    for chunk_coord in cells_by_chunk.keys() {
        writeln!(
            file,
            "CHUNK_COORD {} {} {}",
            chunk_coord.x, chunk_coord.y, chunk_coord.z
        )?;
    }

    terrain_storage.save(&world.terrain)?;

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
    crate::project::atomic_file::write_atomically(path, &file)?;

    Ok(())
}
