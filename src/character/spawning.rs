// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::character::Character;
use super::collision::CharacterCollision;
use super::package::{self, DEFAULT_CHARACTER_PACKAGE};
use super::system::CharacterSystem;
use crate::engine::entity::EntityManager;
use crate::world::{CellType, World, WorldCoord};

pub fn runtime_character_spawn_is_clear(
    world: &World,
    entities: &EntityManager,
    characters: &CharacterSystem,
    position: Vec3,
    collision: &CharacterCollision,
) -> bool {
    let radius = collision.radius;
    let height = collision.height;
    if !position.is_finite()
        || !radius.is_finite()
        || !height.is_finite()
        || radius <= 0.0
        || height <= 0.0
    {
        return false;
    }
    let min = position + Vec3::new(-radius, 0.025, -radius);
    let max = position + Vec3::new(radius, height, radius);
    for x in (min.x.floor() as i32)..(max.x.ceil() as i32) {
        for y in (min.y.floor() as i32)..(max.y.ceil() as i32) {
            for z in (min.z.floor() as i32)..(max.z.ceil() as i32) {
                let coord = WorldCoord::new(x, y, z);
                if !world.is_cell_solid(coord) {
                    continue;
                }
                let cell_min = Vec3::new(x as f32, y as f32, z as f32);
                let cell_max = cell_min + Vec3::ONE;
                if min.cmplt(cell_max).all() && max.cmpgt(cell_min).all() {
                    return false;
                }
            }
        }
    }

    for entity in entities.iter() {
        if !entity.enabled
            || !entity.physics.enabled
            || !entity.physics.collision_enabled
            || !entity.physics.solid
        {
            continue;
        }
        let physics = &entity.physics;
        let center = entity.position + physics.offset;
        let entity_min = center - physics.size * 0.5;
        let entity_max = center + physics.size * 0.5;
        if entity_min.cmplt(max).all() && entity_max.cmpgt(min).all() {
            if let Some(collider) = physics.collider.as_ref() {
                if crate::engine::collision_geometry::against_box(collider, center, min, max)
                    .is_some()
                {
                    return false;
                }
            } else {
                return false;
            }
        }
    }

    for existing in characters.get_active_characters() {
        let other = existing.transform.position;
        if !other.is_finite() {
            continue;
        }
        let vertical_overlap =
            position.y + height > other.y && other.y + existing.collision.height > position.y;
        let separation = Vec3::new(position.x - other.x, 0.0, position.z - other.z);
        if vertical_overlap
            && separation.length_squared() < (radius + existing.collision.radius).powi(2)
        {
            return false;
        }
    }
    true
}

use glam::Vec3;

impl CharacterSystem {
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
    let requested = package_name.unwrap_or(DEFAULT_CHARACTER_PACKAGE);
    let package = package::resolve_package(requested)?;
    spawn_at_random_point_with_package(world, next_id, package)
}

pub(crate) fn spawn_at_random_point_with_package(
    world: &World,
    next_id: u64,
    package: std::sync::Arc<package::CharacterPackage>,
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

    let scale = if package.source_actor.is_some() {
        package.scale
    } else {
        world.player_character_scale
    };
    let mut collision = package.collision.clone();
    collision.radius *= scale.x.max(scale.z);
    collision.height *= scale.y;

    let make_character = |position| {
        let mut character = Character::from_package(next_id, position, package.clone());
        character.transform.scale = scale;
        character.collision = collision.clone();
        character
    };

    let base_position = Vec3::new(coord.x as f32, spawn_y, coord.z as f32);

    if has_character_clearance(world, base_position, &collision) {
        return Some(make_character(base_position));
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
                    return Some(make_character(candidate));
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

#[cfg(test)]
mod runtime_spawn_clearance_tests {
    use super::*;
    use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
    use std::sync::Arc;

    #[test]
    fn tree_collision_geometry_rejects_trunk_but_not_empty_canopy_envelope() {
        let world = World::new();
        let mut entities = EntityManager::new();
        let characters = CharacterSystem::new();
        let tree = entities.create_entity("Tree");
        let entity = entities.get_entity_mut(tree).unwrap();
        entity.position = Vec3::new(5.0, 0.0, 5.0);
        entity.physics.enabled = true;
        entity.physics.collision_enabled = true;
        entity.physics.solid = true;
        entity.physics.anchored = true;
        entity.physics.size = Vec3::new(8.0, 4.0, 8.0);
        entity.physics.collider = Some(ColliderInstance {
            shapes: Arc::new(vec![CollisionShape::Box {
                min: [-0.3, 0.0, -0.3],
                max: [0.3, 2.0, 0.3],
            }]),
            rotation: Vec3::ZERO,
            scale: Vec3::ONE,
        });
        let character = CharacterCollision::new_with_size(0.4, 1.8);
        assert!(!runtime_character_spawn_is_clear(
            &world,
            &entities,
            &characters,
            Vec3::new(5.0, 0.0, 5.0),
            &character
        ));
        assert!(runtime_character_spawn_is_clear(
            &world,
            &entities,
            &characters,
            Vec3::new(7.0, 0.0, 5.0),
            &character
        ));
    }

    #[test]
    fn solid_cells_and_other_characters_prevent_overlapping_spawns() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(3, 0, 3), CellType::Block);
        let entities = EntityManager::new();
        let mut characters = CharacterSystem::new();
        let collision = CharacterCollision::new_with_size(0.4, 1.8);
        assert!(!runtime_character_spawn_is_clear(
            &world,
            &entities,
            &characters,
            Vec3::new(3.5, 0.0, 3.5),
            &collision
        ));
        assert!(runtime_character_spawn_is_clear(
            &world,
            &entities,
            &characters,
            Vec3::new(3.5, 1.0, 3.5),
            &collision
        ));
        characters.spawn_character(Vec3::new(8.0, 1.0, 8.0), None);
        assert!(!runtime_character_spawn_is_clear(
            &world,
            &entities,
            &characters,
            Vec3::new(8.2, 1.0, 8.2),
            &collision
        ));
        assert!(runtime_character_spawn_is_clear(
            &world,
            &entities,
            &characters,
            Vec3::new(10.0, 1.0, 8.0),
            &collision
        ));
    }
}

#[cfg(test)]
mod player_scale_tests {
    use super::*;
    #[test]
    fn wkasset010_spawn_applies_three_axis_player_scale_and_collision() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(8, 0, 8), CellType::SpawnPoint);
        world.player_character_scale = Vec3::new(1.5, 2.0, 0.5);
        let spawned = spawn_at_random_point(&world, 42, None)
            .expect("default installed Character must spawn");
        assert_eq!(spawned.transform.scale, world.player_character_scale);
        let base = &spawned.package.collision;
        assert!((spawned.collision.radius - base.radius * 1.5).abs() < 0.0001);
        assert!((spawned.collision.height - base.height * 2.0).abs() < 0.0001);
    }
}
