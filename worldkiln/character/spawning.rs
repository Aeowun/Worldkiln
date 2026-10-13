use super::character::Character;
use super::collision::CharacterCollision;
use super::package::{self, DEFAULT_CHARACTER_PACKAGE};
use super::system::CharacterSystem;
use crate::world::{CellType, World, WorldCoord};
use glam::Vec3;

impl CharacterSystem {
    /// Spawns a player character at a valid world spawn point.
    pub fn spawn_player(&mut self, world: &World, package_name: Option<&str>) -> Option<u64> {
        let character = spawn_at_random_point(world, self.next_id, package_name)?;

        let id = character.id;

        self.characters.insert(id, character);

        self.active_player_id = Some(id);

        self.next_id += 1;

        Some(id)
    }
}

pub fn spawn_at_random_point(
    world: &World,
    next_id: u64,
    package_name: Option<&str>,
) -> Option<Character> {
    let spawn_points: Vec<WorldCoord> = world
        .active_blocks()
        .iter()
        .filter_map(|&coord| {
            world
                .get(coord)
                .filter(|cell| cell.cell_type == CellType::SpawnPoint)
                .map(|_| coord)
        })
        .collect();

    if spawn_points.is_empty() {
        return None;
    }

    let coord = spawn_points[0];

    let spawn_y = if let Some(_cell) = world.get(coord) {
        if world.is_cell_solid(coord) {
            coord.y as f32 + 1.0
        } else {
            coord.y as f32
        }
    } else {
        return None;
    };

    // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:24 -05:00
    // Resolve the actual CharacterPackage before testing spawn clearance.
    // The imported Rogue GLB has a taller collider than the old generic one.
    // Missing explicit packages must fail without constructing a partial player.
    let requested = package_name.unwrap_or(DEFAULT_CHARACTER_PACKAGE);
    let package = package::resolve_package(requested)?;
    let collision = package.collision.clone();

    let base_position = Vec3::new(coord.x as f32, spawn_y, coord.z as f32);

    if has_character_clearance(world, base_position, &collision) {
        return Some(Character::from_package(
            next_id,
            base_position,
            package.clone(),
        ));
    }

    for radius in 1..=10 {
        let radius = radius as i32;

        for x in -radius..=radius {
            for z in -radius..=radius {
                if x.abs() != radius && z.abs() != radius {
                    continue;
                }

                let candidate = Vec3::new(
                    coord.x as f32 + x as f32,
                    spawn_y,
                    coord.z as f32 + z as f32,
                );

                if has_character_clearance(world, candidate, &collision) {
                    return Some(Character::from_package(next_id, candidate, package.clone()));
                }
            }
        }
    }

    None
}

fn has_character_clearance(world: &World, position: Vec3, collision: &CharacterCollision) -> bool {
    let radius = collision.radius;
    let height = collision.height;

    let min_x = (position.x - radius).floor() as i32;
    let max_x = (position.x + radius).ceil() as i32;
    let min_y = position.y.floor() as i32;
    let max_y = (position.y + height).ceil() as i32;
    let min_z = (position.z - radius).floor() as i32;
    let max_z = (position.z + radius).ceil() as i32;

    for x in min_x..max_x {
        for y in min_y..max_y {
            for z in min_z..max_z {
                let coord = WorldCoord::new(x, y, z);

                if let Some(_cell) = world.get(coord) {
                    if world.is_cell_solid(coord) {
                        return false;
                    }
                }
            }
        }
    }

    true
}
