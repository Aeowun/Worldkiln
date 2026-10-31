// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::character::Character;
use crate::engine::physics::PhysicsWorld;
use crate::world::World;
use glam::Vec3;
use std::collections::{HashMap, HashSet};

const CHARACTER_BROADPHASE_CELL_SIZE: f32 = 1.0;

fn cell_support_contact(
    previous_bottom: f32,
    current_bottom: f32,
    cell_top: f32,
    moving_down: bool,
    previously_grounded: bool,
) -> bool {
    if !moving_down || !previous_bottom.is_finite() || !current_bottom.is_finite() {
        return false;
    }
    let crossed_from_above = previous_bottom >= cell_top - 0.001 && current_bottom < cell_top;
    let stayed_on_top = previously_grounded
        && previous_bottom >= cell_top - 0.12
        && previous_bottom <= cell_top + 0.12
        && current_bottom >= cell_top - 0.20
        && current_bottom <= cell_top + 0.02;
    let recover_floor = previous_bottom >= cell_top - 0.45
        && previous_bottom <= cell_top + 0.12
        && current_bottom >= cell_top - 0.55
        && current_bottom < cell_top;
    crossed_from_above || stayed_on_top || recover_floor
}

fn resolve_cell_wall_contact(
    character: &mut Character,
    previous_position: Vec3,
    min: &Vec3,
    max: Vec3,
    overlap_x: f32,
    overlap_z: f32,
) {
    let radius = character.collision.radius;
    let from_left = previous_position.x < min.x - 0.001;
    let from_right = previous_position.x > max.x + 0.001;
    let from_front = previous_position.z < min.z - 0.001;
    let from_back = previous_position.z > max.z + 0.001;
    let x_entry = from_left || from_right;
    let z_entry = from_front || from_back;
    let into_x = (from_left && character.transform.position.x > previous_position.x)
        || (from_right && character.transform.position.x < previous_position.x);
    let into_z = (from_front && character.transform.position.z > previous_position.z)
        || (from_back && character.transform.position.z < previous_position.z);

    if into_x && (!into_z || overlap_x <= overlap_z) {
        let boundary = if from_left {
            min.x - radius
        } else {
            max.x + radius
        };
        character.transform.position.x = if from_left {
            boundary.max(previous_position.x)
        } else {
            boundary.min(previous_position.x)
        };
        character.movement.velocity.x = 0.0;
        character.movement.slope_slide_velocity.x = 0.0;
    } else if into_z {
        let boundary = if from_front {
            min.z - radius
        } else {
            max.z + radius
        };
        character.transform.position.z = if from_front {
            boundary.max(previous_position.z)
        } else {
            boundary.min(previous_position.z)
        };
        character.movement.velocity.z = 0.0;
        character.movement.slope_slide_velocity.z = 0.0;
    } else if !x_entry && !z_entry {
        character.transform.position.x = previous_position.x;
        character.transform.position.z = previous_position.z;
        character.movement.velocity.x = 0.0;
        character.movement.velocity.z = 0.0;
        character.movement.slope_slide_velocity.x = 0.0;
        character.movement.slope_slide_velocity.z = 0.0;
    }
}

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
    if !character.collision_enabled || !character.collision_solid {
        return;
    }
    if character.uses_authored_collider || character.contact_collider().is_some() {
        super::collider::resolve_active_static_cells(
            character,
            physics_world,
            contacted_cells,
            previous_position,
        );
        return;
    }
    let radius = character.collision.radius;
    let height = character.collision.height;
    let previously_grounded = character.movement.is_grounded;
    let descending = character.movement.velocity.y <= 0.0;
    let ascending = character.movement.velocity.y >= 0.0;
    character.movement.is_grounded = false;
    let foot = character.transform.position;
    let faces = physics_world.anchored_faces_in_aabb(
        foot.min(previous_position) - Vec3::new(radius + 0.01, 0.5, radius + 0.01),
        (foot + Vec3::new(0.0, height, 0.0)).max(previous_position + Vec3::new(0.0, height, 0.0))
            + Vec3::new(radius + 0.01, 0.5, radius + 0.01),
    );
    let overlaps_xz = |min: Vec3, max: Vec3, at: Vec3| {
        at.x + radius > min.x + 0.001
            && at.x - radius < max.x - 0.001
            && at.z + radius > min.z + 0.001
            && at.z - radius < max.z - 0.001
    };
    let mut supported_top: Option<f32> = None;
    for face in &faces {
        if face.normal.y > 0.5
            && overlaps_xz(face.min, face.max, foot)
            && cell_support_contact(
                previous_position.y,
                foot.y,
                face.min.y,
                descending,
                previously_grounded,
            )
        {
            supported_top = Some(supported_top.map_or(face.min.y, |h| h.max(face.min.y)));
        }
    }
    if let Some(y) = supported_top {
        character.transform.position.y = y;
        character.movement.velocity.y = 0.0;
        character.movement.is_grounded = true;
    }
    let contact_foot = character.transform.position;
    for (id, min) in physics_world.query_static_colliders_in_aabb(
        contact_foot - Vec3::new(radius, 0.2, radius),
        contact_foot + Vec3::new(radius, height + 0.2, radius),
    ) {
        let max = min + Vec3::ONE;
        if overlaps_xz(min, max, contact_foot)
            && character.movement.is_grounded
            && (max.y - contact_foot.y).abs() < 0.002
        {
            contacted_cells.insert(id);
        }
    }
    if ascending {
        let previous_top = previous_position.y + height;
        let top = character.transform.position.y + height;
        for face in &faces {
            if face.normal.y < -0.5
                && overlaps_xz(face.min, face.max, character.transform.position)
                && previous_top <= face.min.y + 0.001
                && top > face.min.y
            {
                character.transform.position.y = face.min.y - height;
                character.movement.velocity.y = 0.0;
                contacted_cells.insert(face.cell_id);
                break;
            }
        }
    }
    for face in &faces {
        let bottom = character.transform.position.y;
        let top = bottom + height;
        if bottom >= face.max.y - 0.001 || top <= face.min.y + 0.001 {
            continue;
        }
        let x = character.transform.position.x;
        let z = character.transform.position.z;
        if face.normal.x < -0.5 {
            let plane = face.min.x;
            if z + radius > face.min.z
                && z - radius < face.max.z
                && previous_position.x + radius <= plane + 0.001
                && x + radius > plane
            {
                character.transform.position.x = plane - radius;
                character.movement.velocity.x = 0.0;
                character.movement.slope_slide_velocity.x = 0.0;
                contacted_cells.insert(face.cell_id);
            }
        } else if face.normal.x > 0.5 {
            let plane = face.min.x;
            if z + radius > face.min.z
                && z - radius < face.max.z
                && previous_position.x - radius >= plane - 0.001
                && x - radius < plane
            {
                character.transform.position.x = plane + radius;
                character.movement.velocity.x = 0.0;
                character.movement.slope_slide_velocity.x = 0.0;
                contacted_cells.insert(face.cell_id);
            }
        } else if face.normal.z < -0.5 {
            let plane = face.min.z;
            if x + radius > face.min.x
                && x - radius < face.max.x
                && previous_position.z + radius <= plane + 0.001
                && z + radius > plane
            {
                character.transform.position.z = plane - radius;
                character.movement.velocity.z = 0.0;
                character.movement.slope_slide_velocity.z = 0.0;
                contacted_cells.insert(face.cell_id);
            }
        } else if face.normal.z > 0.5 {
            let plane = face.min.z;
            if x + radius > face.min.x
                && x - radius < face.max.x
                && previous_position.z - radius >= plane - 0.001
                && z - radius < plane
            {
                character.transform.position.z = plane + radius;
                character.movement.velocity.z = 0.0;
                character.movement.slope_slide_velocity.z = 0.0;
                contacted_cells.insert(face.cell_id);
            }
        }
    }
}

pub fn resolve_terrain_collision(
    character: &mut Character,
    world: &World,
    previous_position: Vec3,
) {
    let was_grounded = character.movement.is_grounded;
    super::terrain_motion::resolve_terrain_movement(
        character,
        world,
        previous_position,
        was_grounded,
    );
}

pub fn resolve_dynamic_body_collisions(
    character: &mut Character,
    physics_world: &mut PhysicsWorld,
    contacted_cells: &mut HashSet<u64>,
    previous_position: Vec3,
    previously_grounded: bool,
) {
    if !character.collision_enabled || !character.collision_solid {
        return;
    }
    if character.uses_authored_collider || character.contact_collider().is_some() {
        super::collider::resolve_active_dynamic_cells(
            character,
            physics_world,
            contacted_cells,
            previous_position,
        );
        return;
    }
    if physics_world.dynamic_bodies_by_cell.is_empty() && !physics_world.bodies.is_empty() {
        physics_world.rebuild_dynamic_body_index();
    }

    let radius = character.collision.radius;
    let height = character.collision.height;

    let min_pos = character.transform.position - Vec3::new(radius + 1.0, 0.5, radius + 1.0);
    let max_pos =
        character.transform.position + Vec3::new(radius + 1.0, height + 1.0, radius + 1.0);

    let candidates = physics_world.query_dynamic_bodies_in_aabb(min_pos, max_pos);

    let mut supporting_top: Option<f32> = None;
    let mut supporting_bodies = Vec::new();
    for &index in &candidates {
        let Some(body) = physics_world.bodies.get(index) else {
            continue;
        };
        if body.anchored || !body.solid {
            continue;
        }
        let min = body.min_corner();
        let max = body.max_corner();
        let foot = character.transform.position;
        if foot.x + radius <= min.x + 0.001
            || foot.x - radius >= max.x - 0.001
            || foot.z + radius <= min.z + 0.001
            || foot.z - radius >= max.z - 0.001
        {
            continue;
        }
        if cell_support_contact(
            previous_position.y,
            foot.y,
            max.y,
            character.movement.velocity.y <= 0.0,
            previously_grounded,
        ) {
            if body.cell_id != 0 {
                contacted_cells.insert(body.cell_id);
            }
            supporting_bodies.push(index);
            supporting_top = Some(supporting_top.map_or(max.y, |top| top.max(max.y)));
        }
    }
    if let Some(top) = supporting_top {
        character.transform.position.y = top;
        character.movement.velocity.y = 0.0;
        character.movement.is_grounded = true;
    }

    for index in supporting_bodies {
        if let Some(body) = physics_world.bodies.get_mut(index) {
            body.wake();
        }
    }

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

        let crossed_floor = cell_support_contact(
            previous_bottom,
            current_bottom,
            v_max.y,
            moving_down,
            previously_grounded,
        );

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

        resolve_cell_wall_contact(
            character,
            previous_position,
            &v_min,
            v_max,
            overlap_x,
            overlap_z,
        );

        body.wake();
    }
}

pub fn resolve_character_collisions(characters: &mut HashMap<u64, Character>) {
    resolve_character_collisions_excluding(characters, &HashSet::new());
}

pub fn resolve_character_collisions_excluding(
    characters: &mut HashMap<u64, Character>,
    excluded: &HashSet<u64>,
) {
    resolve_character_pairs(characters, excluded, None);
}

pub(crate) fn resolve_character_collisions_with_world(
    characters: &mut HashMap<u64, Character>,
    excluded: &HashSet<u64>,
    physics: &PhysicsWorld,
    world: &World,
) {
    resolve_character_pairs(characters, excluded, Some((physics, world)));
}

fn resolve_character_pairs(
    characters: &mut HashMap<u64, Character>,
    excluded: &HashSet<u64>,
    constraints: Option<(&PhysicsWorld, &World)>,
) {
    use crate::engine::collision_geometry;
    for _ in 0..8 {
        let mut snapshots = characters
            .values()
            .filter(|c| !excluded.contains(&c.id) && c.collision_enabled)
            .filter_map(|c| {
                let (lo, hi) = c.collision_bounds();
                Some((c.id, c.transform.position, lo, hi, c.contact_collider()?))
            })
            .collect::<Vec<_>>();
        snapshots.sort_by_key(|s| s.0);
        if snapshots.len() < 2 {
            return;
        }
        let radius = |p: Vec3, lo: Vec3, hi: Vec3| {
            (lo.x - p.x)
                .abs()
                .max((hi.x - p.x).abs())
                .max((lo.z - p.z).abs())
                .max((hi.z - p.z).abs())
        };
        let max_radius = snapshots
            .iter()
            .map(|s| radius(s.1, s.2, s.3))
            .fold(0.0, f32::max);
        let mut grid: HashMap<(i32, i32), Vec<usize>> = HashMap::new();
        for (i, s) in snapshots.iter().enumerate() {
            grid.entry((character_grid_coord(s.1.x), character_grid_coord(s.1.z)))
                .or_default()
                .push(i);
        }
        let mut corrections: HashMap<u64, Vec3> = HashMap::new();
        let mut velocity_corrections: HashMap<u64, Vec3> = HashMap::new();
        for (i, a) in snapshots.iter().enumerate() {
            let range = ((radius(a.1, a.2, a.3) + max_radius) / CHARACTER_BROADPHASE_CELL_SIZE)
                .ceil() as i32;
            let (gx, gz) = (character_grid_coord(a.1.x), character_grid_coord(a.1.z));
            for x in gx - range..=gx + range {
                for z in gz - range..=gz + range {
                    let Some(ids) = grid.get(&(x, z)) else {
                        continue;
                    };
                    for &j in ids {
                        if j <= i {
                            continue;
                        }
                        let b = &snapshots[j];
                        if a.2.cmpge(b.3).any() || b.2.cmpge(a.3).any() {
                            continue;
                        }
                        let Some(hit) = collision_geometry::contact(
                            Some(&a.4),
                            a.1,
                            a.2,
                            a.3,
                            Some(&b.4),
                            b.1,
                            b.2,
                            b.3,
                        ) else {
                            continue;
                        };
                        if hit.depth <= 0.00001
                            || !characters[&a.0].collision_solid
                            || !characters[&b.0].collision_solid
                        {
                            continue;
                        }
                        let correction = hit.normal * ((hit.depth + 0.0001) * 0.5);
                        *corrections.entry(a.0).or_default() += correction;
                        *corrections.entry(b.0).or_default() -= correction;
                        let va = characters[&a.0].movement.velocity;
                        let vb = characters[&b.0].movement.velocity;
                        *velocity_corrections.entry(a.0).or_default() -=
                            hit.normal * va.dot(hit.normal).min(0.0);
                        *velocity_corrections.entry(b.0).or_default() -=
                            hit.normal * vb.dot(hit.normal).max(0.0);
                    }
                }
            }
        }
        if corrections.is_empty() {
            return;
        }
        for (id, correction) in corrections {
            let c = characters.get_mut(&id).unwrap();
            let previous = c.transform.position;
            c.transform.position += correction;
            if let Some((physics, world)) = constraints {
                let mut touched = HashSet::new();
                resolve_static_voxel_collisions(c, physics, &mut touched, previous);
                let from = previous;
                let to = c.transform.position;
                if let Some((position, _)) =
                    super::collider::sweep_terrain(physics, world, c, from, to)
                {
                    c.transform.position = position;
                    resolve_static_voxel_collisions(c, physics, &mut touched, from);
                }
            }
        }
        for (id, correction) in velocity_corrections {
            if let Some(c) = characters.get_mut(&id) {
                c.movement.velocity += correction;
            }
        }
    }
}

#[cfg(test)]
mod terrain_side_collision_tests {
    use super::*;
    use crate::world::terrain::{
        HeightfieldData, PLAINS_GENERATOR_IMPLEMENTATION_VERSION, TerrainData,
        TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    };

    fn make_world(steep: bool) -> World {
        let mut world = World::new();
        let heights: Vec<f32> = (0..12)
            .flat_map(|_| {
                (0..12).map(|x| {
                    if steep && x >= 6 {
                        8.0
                    } else if steep {
                        0.0
                    } else {
                        x as f32 * 0.02
                    }
                })
            })
            .collect();
        world.terrain.authored = Some(TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [12, 12, 12],
            },
            generator: TerrainGeneratorType::Plains,
            generator_implementation_version: PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
            data: TerrainData::Heightfield(HeightfieldData {
                origin: [0, 0, 0],
                size: [12, 12, 12],
                heights,
            }),
        });
        world
    }

    #[test]
    fn smooth_terrain_tall_cliff_blocks_horizontal_actor_motion() {
        let world = make_world(true);
        let start = Vec3::new(4.0, 1.0, 6.0);
        let mut actor = Character::new(418, start);
        actor.transform.position = Vec3::new(7.5, 1.0, 6.0);
        actor.movement.velocity = Vec3::new(6.0, 0.0, 0.0);
        resolve_terrain_collision(&mut actor, &world, start);
        assert!(actor.transform.position.x > start.x);
        assert!(
            actor.transform.position.x < 6.0,
            "actor must stop before cliff"
        );
        assert!(actor.movement.velocity.x.abs() < 0.01);
    }

    #[test]
    fn steep_cliff_preserves_sideways_glancing_motion() {
        let world = make_world(true);
        let start = Vec3::new(4.0, 1.0, 4.0);
        let mut actor = Character::new(521, start);
        actor.transform.position = Vec3::new(8.0, 1.0, 8.0);
        actor.movement.velocity = Vec3::new(8.0, 0.0, 8.0);
        resolve_terrain_collision(&mut actor, &world, start);
        assert!(
            actor.transform.position.x < 6.0,
            "steep wall must block uphill"
        );
        assert!(
            actor.transform.position.z > 4.5,
            "sideways glide must continue"
        );
        assert!(
            actor.movement.velocity.z > 7.0,
            "tangent velocity should survive"
        );
    }

    #[test]
    fn high_speed_terrain_sweep_stops_at_first_cliff_contact() {
        let world = make_world(true);
        let start = Vec3::new(1.0, 1.0, 6.0);
        let mut actor = Character::new(522, start);
        actor.transform.position = Vec3::new(11.0, 1.0, 6.0);
        actor.movement.velocity = Vec3::new(140.0, 0.0, 0.0);
        resolve_terrain_collision(&mut actor, &world, start);
        assert!(
            actor.transform.position.x > 1.0 && actor.transform.position.x < 6.0,
            "fast actor must reach, but not pass through, cliff boundary"
        );
        assert!(actor.movement.velocity.x.abs() < 0.01);
    }

    #[test]
    fn smooth_terrain_walks_up_and_down_a_real_ramp() {
        let mut world = make_world(false);
        if let Some(result) = world.terrain.authored.as_mut() {
            if let TerrainData::Heightfield(heightfield) = &mut result.data {
                for z in 0..12 {
                    for x in 0..12 {
                        heightfield.heights[z * 12 + x] = x as f32 * 0.65;
                    }
                }
            }
        }
        let start = Vec3::new(3.5, world.terrain.surface_height_at(3.5, 6.0).unwrap(), 6.0);
        let mut uphill = Character::new(420, start);
        uphill.transform.position.x = 5.0;
        uphill.movement.velocity = Vec3::new(4.0, 0.0, 0.0);
        resolve_terrain_collision(&mut uphill, &world, start);
        assert!(
            uphill.transform.position.x > 4.9,
            "walkable 33-degree slope blocked"
        );
        assert!(
            uphill.transform.position.y > start.y,
            "uphill elevation did not rise"
        );
        let down_start = uphill.transform.position;
        uphill.transform.position.x = 3.5;
        uphill.movement.velocity = Vec3::new(-4.0, 0.0, 0.0);
        resolve_terrain_collision(&mut uphill, &world, down_start);
        assert!(uphill.transform.position.x < 3.6, "downhill slope blocked");
    }

    #[test]
    fn smooth_terrain_shallow_slope_remains_traversable() {
        let world = make_world(false);
        let start = Vec3::new(4.0, 1.0, 6.0);
        let mut actor = Character::new(419, start);
        actor.transform.position = Vec3::new(7.0, 1.0, 6.0);
        resolve_terrain_collision(&mut actor, &world, start);
        assert!(actor.transform.position.x > 6.0);
    }
}

#[cfg(test)]
mod cell_collision_stability_tests {
    use super::*;
    use crate::engine::physics::{PhysicsBody, PhysicsBodyId};
    use crate::world::{CellType, WorldCoord};

    fn anchored_floor(width: i32) -> PhysicsWorld {
        let mut world = World::new();
        for x in 0..width {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        physics
    }

    #[test]
    fn overlapping_neighbor_cells_cannot_push_idle_capsule_to_the_dock_edge() {
        let physics = anchored_floor(5);
        let previous = Vec3::new(2.45, 0.1, 0.5);
        let mut actor = Character::new(53, previous);
        actor.collision.radius = 0.65;
        actor.movement.velocity = Vec3::ZERO;
        actor.transform.position = previous;
        resolve_static_voxel_collisions(&mut actor, &physics, &mut HashSet::new(), previous);
        assert!(
            (actor.transform.position.x - previous.x).abs() < 0.001
                && (actor.transform.position.z - previous.z).abs() < 0.001,
            "neighboring dock Cells ejected an idle, overlapping Character"
        );
    }

    #[test]
    fn dock_floor_recovers_moderate_sink_without_launching_sideways() {
        let physics = anchored_floor(5);
        let previous = Vec3::new(2.45, 0.76, 0.5);
        let mut actor = Character::new(51, previous);
        actor.movement.velocity = Vec3::new(0.0, -1.0, 0.0);
        actor.transform.position = Vec3::new(2.45, 0.73, 0.5);
        let mut touched = HashSet::new();
        resolve_static_voxel_collisions(&mut actor, &physics, &mut touched, previous);
        assert!(
            (actor.transform.position.y - 1.0).abs() < 0.001,
            "sunk dock landing should be resolved upward, not toward edges"
        );
        assert!(
            (actor.transform.position.x - previous.x).abs() < 0.001
                && (actor.transform.position.z - previous.z).abs() < 0.001,
            "overlapping dock Cells launched actor sideways: {:?}",
            actor.transform.position
        );
        assert!(actor.movement.is_grounded);
    }

    #[test]
    fn cell_wall_does_not_push_an_already_embedded_actor_across_the_dock() {
        let physics = anchored_floor(5);
        let previous = Vec3::new(2.45, 0.1, 0.5);
        let mut actor = Character::new(52, previous);
        actor.movement.velocity = Vec3::ZERO;
        actor.transform.position = previous;
        resolve_static_voxel_collisions(&mut actor, &physics, &mut HashSet::new(), previous);
        assert!(
            (actor.transform.position.x - previous.x).abs() < 0.001,
            "ambiguous embedded contact must not cascade across dock Cells"
        );
        assert!(
            (actor.transform.position.z - previous.z).abs() < 0.001,
            "ambiguous embedded contact must not choose a random escape direction"
        );
    }

    #[test]
    fn shallow_cell_top_overlap_snaps_up_without_horizontal_ejection() {
        let physics = anchored_floor(1);
        let previous = Vec3::new(0.5, 0.94, 0.5);
        let mut character = Character::new(1, previous);
        character.movement.is_grounded = true;
        character.movement.velocity.y = -1.0;
        character.transform.position = Vec3::new(0.5, 0.89, 0.5);
        let mut contacts = HashSet::new();
        resolve_static_voxel_collisions(&mut character, &physics, &mut contacts, previous);
        assert!((character.transform.position.y - 1.0).abs() < 0.0001);
        assert!((character.transform.position.x - 0.5).abs() < 0.0001);
        assert!((character.transform.position.z - 0.5).abs() < 0.0001);
        assert!(character.movement.is_grounded);
        assert!(
            !contacts.is_empty(),
            "supported Cell retains its touch event"
        );
    }

    #[test]
    fn adjacent_same_height_cells_never_kick_the_character_sideways() {
        let physics = anchored_floor(2);
        let previous = Vec3::new(0.92, 0.97, 0.5);
        let mut character = Character::new(2, previous);
        character.movement.is_grounded = true;
        character.movement.velocity = Vec3::new(2.0, -0.3, 0.0);
        character.transform.position = Vec3::new(1.08, 0.93, 0.5);
        let mut contacts = HashSet::new();
        resolve_static_voxel_collisions(&mut character, &physics, &mut contacts, previous);
        assert!((character.transform.position.x - 1.08).abs() < 0.0001);
        assert!((character.transform.position.y - 1.0).abs() < 0.0001);
        assert!(character.movement.is_grounded);
    }

    #[test]
    fn touching_the_side_of_a_tall_cell_remains_blocked() {
        let physics = anchored_floor(1);
        let previous = Vec3::new(-0.65, 0.1, 0.5);
        let mut character = Character::new(3, previous);
        character.movement.velocity = Vec3::new(4.0, 0.0, 0.0);
        character.transform.position.x = -0.2;
        let mut contacts = HashSet::new();
        resolve_static_voxel_collisions(&mut character, &physics, &mut contacts, previous);
        assert!(character.transform.position.x <= -0.399);
        assert!((character.transform.position.y - 0.1).abs() < 0.0001);
        assert!(!character.movement.is_grounded);
    }

    #[test]
    fn jumping_into_cell_ceiling_stops_vertical_movement() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(0, 2, 0), CellType::Block);
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        let mut character = Character::new(4, Vec3::ZERO);
        let height = character.collision.height;
        let previous = Vec3::new(0.5, 2.0 - height - 0.2, 0.5);
        character.transform.position = previous + Vec3::new(0.0, 0.4, 0.0);
        character.movement.velocity.y = 3.0;
        resolve_static_voxel_collisions(&mut character, &physics, &mut HashSet::new(), previous);
        assert!((character.transform.position.y - (2.0 - height)).abs() < 0.0001);
        assert_eq!(character.movement.velocity.y, 0.0);
        assert!(!character.movement.is_grounded);
    }

    #[test]
    fn dynamic_cell_platform_support_does_not_eject_character() {
        let mut physics = PhysicsWorld::new();
        let mut platform =
            PhysicsBody::new(PhysicsBodyId(8), 88, Vec3::new(0.0, 0.05, 0.0), Vec3::ONE);
        platform.solid = true;
        platform.anchored = false;
        physics.bodies.push(platform);
        let previous = Vec3::new(0.5, 1.02, 0.5);
        let mut character = Character::new(5, previous);
        character.movement.is_grounded = true;
        character.movement.velocity.y = -0.4;
        character.transform.position.y = 0.99;
        let mut touched = HashSet::new();
        resolve_dynamic_body_collisions(&mut character, &mut physics, &mut touched, previous, true);
        let platform = &physics.bodies[0];
        assert!(
            (character.transform.position.y - platform.max_corner().y).abs() < 0.005,
            "grounded Character must follow the moved platform's surface"
        );
        assert!(
            platform.position.y < 0.05,
            "unanchored platform must respond to impact"
        );
        assert!(
            platform.velocity.y < 0.0,
            "downward momentum must transfer to platform"
        );
        assert!((character.transform.position.x - 0.5).abs() < 0.0001);
        assert!(character.movement.is_grounded);
        assert!(touched.contains(&88));
    }

    #[test]
    fn moving_cell_platform_supports_a_character_between_frames() {
        let mut physics = PhysicsWorld::new();
        physics.bodies.push(PhysicsBody::new(
            PhysicsBodyId(10),
            91,
            Vec3::ZERO,
            Vec3::ONE,
        ));
        let previous = Vec3::new(0.5, 0.98, 0.5);
        let mut character = Character::new(7, previous);
        character.movement.is_grounded = true;
        character.movement.velocity.y = -0.3;
        character.transform.position.y = 0.95;
        resolve_dynamic_body_collisions(
            &mut character,
            &mut physics,
            &mut HashSet::new(),
            previous,
            true,
        );
        let first_top = physics.bodies[0].max_corner().y;
        assert!(
            (character.transform.position.y - first_top).abs() < 0.005,
            "Character must remain supported by the dynamic platform"
        );

        physics.bodies[0].position.y += 0.08;
        physics.rebuild_dynamic_body_index();
        let previous = character.transform.position;
        character.movement.velocity.y = -0.3;
        character.transform.position.y -= 0.015;
        resolve_dynamic_body_collisions(
            &mut character,
            &mut physics,
            &mut HashSet::new(),
            previous,
            true,
        );
        assert!(
            (character.transform.position.y - physics.bodies[0].max_corner().y).abs() < 0.005,
            "Character must rest on the platform after relative motion"
        );
        assert!(character.transform.position.y > first_top);
        assert!((character.transform.position.x - 0.5).abs() < 0.0001);
        assert!(character.movement.is_grounded);
    }

    #[test]
    fn dynamic_cell_side_collision_still_blocks_walking_through_it() {
        let mut physics = PhysicsWorld::new();
        physics.bodies.push(PhysicsBody::new(
            PhysicsBodyId(9),
            90,
            Vec3::ZERO,
            Vec3::ONE,
        ));
        let previous = Vec3::new(-0.7, 0.1, 0.5);
        let mut character = Character::new(6, previous);
        character.movement.velocity.x = 3.0;
        character.transform.position.x = -0.2;
        resolve_dynamic_body_collisions(
            &mut character,
            &mut physics,
            &mut HashSet::new(),
            previous,
            false,
        );
        let cell = &physics.bodies[0];
        assert!(
            character.transform.position.x + character.collision.radius
                <= cell.min_corner().x + 0.002,
            "Character and moving Cell must not interpenetrate"
        );
        assert!(cell.position.x > 0.0, "dynamic Cell must move on contact");
        assert!(
            cell.velocity.x > 0.0,
            "impact must transfer forward momentum"
        );
        assert!(!character.movement.is_grounded);
    }
}
