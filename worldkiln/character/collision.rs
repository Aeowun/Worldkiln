use super::character::Character;
use crate::engine::physics::PhysicsWorld;
use crate::world::World;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

const CHARACTER_BROADPHASE_CELL_SIZE: f32 = 1.0;

fn character_grid_coord(value: f32) -> i32 {
    (value / CHARACTER_BROADPHASE_CELL_SIZE).floor() as i32
}

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterCollision {
    pub radius: f32,
    pub height: f32,
}

impl Default for CharacterCollision {
    fn default() -> Self {
        Self {
            radius: 0.4,
            height: 1.8,
        }
    }
}

impl CharacterCollision {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn new_with_size(radius: f32, height: f32) -> Self {
        Self { radius, height }
    }
}

pub fn resolve_static_voxel_collisions(
    character: &mut Character,
    physics_world: &PhysicsWorld,
    contacted_cells: &mut HashSet<u64>,
    previous_position: Vec3,
) {
    let radius = character.collision.radius;
    let height = character.collision.height;

    character.movement.is_grounded = false;

    let min_pos = character.transform.position - Vec3::new(radius + 1.0, 0.5, radius + 1.0);
    let max_pos =
        character.transform.position + Vec3::new(radius + 1.0, height + 1.0, radius + 1.0);

    let candidates = physics_world.query_static_colliders_in_aabb(min_pos, max_pos);

    for (cell_id, v_min) in &candidates {
        let v_max = *v_min + Vec3::ONE;

        let overlap_x = (character.transform.position.x + radius).min(v_max.x)
            - (character.transform.position.x - radius).max(v_min.x);

        let overlap_y = (character.transform.position.y + height).min(v_max.y)
            - character.transform.position.y.max(v_min.y);

        let overlap_z = (character.transform.position.z + radius).min(v_max.z)
            - (character.transform.position.z - radius).max(v_min.z);

        if overlap_x <= 0.001 || overlap_y <= 0.001 || overlap_z <= 0.001 {
            continue;
        }

        contacted_cells.insert(*cell_id);

        let previous_bottom = previous_position.y;

        let previous_top = previous_position.y + height;

        let current_bottom = character.transform.position.y;

        let current_top = character.transform.position.y + height;

        let moving_down = character.movement.velocity.y <= 0.0;

        let moving_up = character.movement.velocity.y >= 0.0;

        let crossed_floor =
            moving_down && previous_bottom >= v_max.y - 0.001 && current_bottom < v_max.y;

        let crossed_ceiling = moving_up && previous_top <= v_min.y + 0.001 && current_top > v_min.y;

        if crossed_floor {
            character.transform.position.y = v_max.y;

            character.movement.velocity.y = 0.0;

            character.movement.is_grounded = true;

            continue;
        }

        if crossed_ceiling {
            character.transform.position.y = v_min.y - height;

            character.movement.velocity.y = 0.0;

            continue;
        }

        if overlap_x < overlap_z {
            if character.transform.position.x < v_min.x {
                character.transform.position.x -= overlap_x;
            } else {
                character.transform.position.x += overlap_x;
            }

            character.movement.velocity.x = 0.0;
        } else {
            if character.transform.position.z < v_min.z {
                character.transform.position.z -= overlap_z;
            } else {
                character.transform.position.z += overlap_z;
            }

            character.movement.velocity.z = 0.0;
        }
    }
}

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
pub fn resolve_terrain_collision(
    character: &mut Character,
    world: &World,
    previous_position: Vec3,
) {
    // Worldkiln 0.8.5 | WO 0.8.7-57 | 2026-10-09
    // Block Terrain's occupied voxels need sides, ceilings, floors, and
    // swept horizontal contact. Height-at-XZ is only suitable for Smooth
    // Terrain grounding and let characters pass straight through cliffs.
    if world.terrain.config.mode == crate::world::terrain::TerrainMode::Block {
        super::terrain_block_collision::resolve_block_terrain_collision(
            character,
            world,
            previous_position,
        );
        return;
    }
    if character.movement.velocity.y > 0.0 {
        return;
    }
    let Some(surface) = world.terrain.surface_height_at(
        character.transform.position.x,
        character.transform.position.z,
    ) else {
        return;
    };

    let current_bottom = character.transform.position.y;
    let previous_bottom = previous_position.y;
    if current_bottom <= surface && previous_bottom >= surface - 1.0 {
        character.transform.position.y = surface;
        character.movement.velocity.y = 0.0;
        character.movement.is_grounded = true;
    }
}

pub fn resolve_dynamic_body_collisions(
    character: &mut Character,
    physics_world: &mut PhysicsWorld,
    contacted_cells: &mut HashSet<u64>,
    previous_position: Vec3,
) {
    if physics_world.dynamic_bodies_by_cell.is_empty() && !physics_world.bodies.is_empty() {
        physics_world.rebuild_dynamic_body_index();
    }

    let radius = character.collision.radius;
    let height = character.collision.height;

    let min_pos = character.transform.position - Vec3::new(radius + 1.0, 0.5, radius + 1.0);
    let max_pos =
        character.transform.position + Vec3::new(radius + 1.0, height + 1.0, radius + 1.0);

    let candidates = physics_world.query_dynamic_bodies_in_aabb(min_pos, max_pos);

    for body_index in candidates {
        let Some(body) = physics_world.bodies.get_mut(body_index) else {
            continue;
        };

        if body.anchored || !body.solid {
            continue;
        }

        let v_min = body.min_corner();

        let v_max = body.max_corner();

        let overlap_x = (character.transform.position.x + radius).min(v_max.x)
            - (character.transform.position.x - radius).max(v_min.x);

        let overlap_y = (character.transform.position.y + height).min(v_max.y)
            - character.transform.position.y.max(v_min.y);

        let overlap_z = (character.transform.position.z + radius).min(v_max.z)
            - (character.transform.position.z - radius).max(v_min.z);

        if overlap_x <= 0.001 || overlap_y <= 0.001 || overlap_z <= 0.001 {
            continue;
        }

        if body.cell_id != 0 {
            contacted_cells.insert(body.cell_id);
        }

        let previous_bottom = previous_position.y;

        let previous_top = previous_position.y + height;

        let current_bottom = character.transform.position.y;

        let current_top = character.transform.position.y + height;

        let moving_down = character.movement.velocity.y <= 0.0;

        let moving_up = character.movement.velocity.y >= 0.0;

        let crossed_floor =
            moving_down && previous_bottom >= v_max.y - 0.001 && current_bottom < v_max.y;

        let crossed_ceiling = moving_up && previous_top <= v_min.y + 0.001 && current_top > v_min.y;

        if crossed_floor {
            character.transform.position.y = v_max.y;

            character.movement.velocity.y = 0.0;

            character.movement.is_grounded = true;

            body.wake();

            continue;
        }

        if crossed_ceiling {
            character.transform.position.y = v_min.y - height;

            character.movement.velocity.y = 0.0;

            body.wake();

            continue;
        }

        if overlap_x < overlap_z {
            if character.transform.position.x < v_min.x {
                character.transform.position.x -= overlap_x;
            } else {
                character.transform.position.x += overlap_x;
            }

            character.movement.velocity.x = 0.0;
        } else {
            if character.transform.position.z < v_min.z {
                character.transform.position.z -= overlap_z;
            } else {
                character.transform.position.z += overlap_z;
            }

            character.movement.velocity.z = 0.0;
        }

        body.wake();
    }
}

pub fn resolve_character_collisions(characters: &mut HashMap<u64, Character>) {
    resolve_character_collisions_excluding(characters, &HashSet::new());
}

pub fn resolve_character_collisions_excluding(
    characters: &mut HashMap<u64, Character>,
    excluded_characters: &HashSet<u64>,
) {
    const EPSILON: f32 = 0.001;

    for _ in 0..2 {
        let snapshots: Vec<(u64, Vec3, f32, f32, Vec3)> = characters
            .values()
            .filter(|character| !excluded_characters.contains(&character.id))
            .map(|character| {
                (
                    character.id,
                    character.transform.position,
                    character.collision.radius,
                    character.collision.height,
                    character.movement.velocity,
                )
            })
            .collect();

        if snapshots.len() < 2 {
            return;
        }

        let max_radius = snapshots
            .iter()
            .map(|(_, _, radius, _, _)| *radius)
            .fold(0.0_f32, f32::max);

        let max_height = snapshots
            .iter()
            .map(|(_, _, _, height, _)| *height)
            .fold(0.0_f32, f32::max);

        let mut spatial_grid: HashMap<(i32, i32, i32), Vec<usize>> = HashMap::new();

        for (index, (_, position, _, _, _)) in snapshots.iter().enumerate() {
            let key = (
                character_grid_coord(position.x),
                character_grid_coord(position.y),
                character_grid_coord(position.z),
            );

            spatial_grid.entry(key).or_default().push(index);
        }

        let mut position_corrections: HashMap<u64, Vec3> = HashMap::new();
        let mut velocity_corrections: HashMap<u64, Vec3> = HashMap::new();

        for i in 0..snapshots.len() {
            let (id_a, position_a, radius_a, height_a, velocity_a) = snapshots[i];

            let grid_x = character_grid_coord(position_a.x);
            let grid_y = character_grid_coord(position_a.y);
            let grid_z = character_grid_coord(position_a.z);

            let horizontal_range =
                ((radius_a + max_radius) / CHARACTER_BROADPHASE_CELL_SIZE).ceil() as i32;

            let vertical_range =
                (height_a.max(max_height) / CHARACTER_BROADPHASE_CELL_SIZE).ceil() as i32;

            for gx in (grid_x - horizontal_range)..=(grid_x + horizontal_range) {
                for gy in (grid_y - vertical_range)..=(grid_y + vertical_range) {
                    for gz in (grid_z - horizontal_range)..=(grid_z + horizontal_range) {
                        let Some(candidates) = spatial_grid.get(&(gx, gy, gz)) else {
                            continue;
                        };

                        for &j in candidates {
                            if j <= i {
                                continue;
                            }

                            let (id_b, position_b, radius_b, height_b, velocity_b) = snapshots[j];

                            let vertical_overlap = (position_a.y + height_a)
                                .min(position_b.y + height_b)
                                - position_a.y.max(position_b.y);

                            if vertical_overlap <= EPSILON {
                                continue;
                            }

                            let delta_x = position_b.x - position_a.x;
                            let delta_z = position_b.z - position_a.z;

                            let distance_squared = delta_x * delta_x + delta_z * delta_z;
                            let combined_radius = radius_a + radius_b;

                            if distance_squared >= combined_radius * combined_radius {
                                continue;
                            }

                            let (normal_x, normal_z, distance) = if distance_squared > 0.000001 {
                                let distance = distance_squared.sqrt();
                                (delta_x / distance, delta_z / distance, distance)
                            } else if id_a < id_b {
                                (1.0, 0.0, 0.0)
                            } else {
                                (-1.0, 0.0, 0.0)
                            };

                            let penetration = combined_radius - distance;

                            if penetration <= EPSILON {
                                continue;
                            }

                            let correction =
                                Vec3::new(normal_x, 0.0, normal_z) * (penetration * 0.5);

                            position_corrections
                                .entry(id_a)
                                .and_modify(|value| {
                                    *value -= correction;
                                })
                                .or_insert(-correction);

                            position_corrections
                                .entry(id_b)
                                .and_modify(|value| {
                                    *value += correction;
                                })
                                .or_insert(correction);

                            let velocity_a_normal =
                                velocity_a.x * normal_x + velocity_a.z * normal_z;

                            if velocity_a_normal > 0.0 {
                                let correction_velocity = Vec3::new(
                                    -normal_x * velocity_a_normal,
                                    0.0,
                                    -normal_z * velocity_a_normal,
                                );

                                velocity_corrections
                                    .entry(id_a)
                                    .and_modify(|value| {
                                        *value += correction_velocity;
                                    })
                                    .or_insert(correction_velocity);
                            }

                            let velocity_b_normal =
                                velocity_b.x * normal_x + velocity_b.z * normal_z;

                            if velocity_b_normal < 0.0 {
                                let correction_velocity = Vec3::new(
                                    -normal_x * velocity_b_normal,
                                    0.0,
                                    -normal_z * velocity_b_normal,
                                );

                                velocity_corrections
                                    .entry(id_b)
                                    .and_modify(|value| {
                                        *value += correction_velocity;
                                    })
                                    .or_insert(correction_velocity);
                            }
                        }
                    }
                }
            }
        }

        for (id, correction) in position_corrections {
            if let Some(character) = characters.get_mut(&id) {
                character.transform.position += correction;
            }
        }

        for (id, correction) in velocity_corrections {
            if let Some(character) = characters.get_mut(&id) {
                character.movement.velocity += correction;
            }
        }
    }
}
