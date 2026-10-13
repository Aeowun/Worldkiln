//! Worldkiln Block Terrain character collision.
//! Worldkiln 0.8.5 | WO 0.8.7-57 | 2026-10-09
//!
//! Block geometry is a solid voxel volume, not a height-only floor. Resolve
//! swept X/Z sides before Y floors/ceilings using ONLY neighboring Terrain
//! samples; no per-frame full-world collider mesh or physics registration.

use super::character::Character;
use crate::world::World;
use crate::world::terrain::{SMOOTH_VOXEL_ISO_LEVEL, TerrainData, TerrainMode};
use glam::Vec3;

const EPS: f32 = 0.0001;

#[derive(Clone, Copy)]
struct SolidBox {
    min: Vec3,
    max: Vec3,
}

fn sample_range(
    start: f32,
    end: f32,
    origin: f32,
    spacing: f32,
    count: u32,
) -> Option<std::ops::RangeInclusive<u32>> {
    if count == 0
        || spacing <= 0.0
        || !spacing.is_finite()
        || !start.is_finite()
        || !end.is_finite()
    {
        return None;
    }
    let min = ((start - origin) / spacing).floor() as i64;
    let max = ((end - origin) / spacing).floor() as i64;
    if max < 0 || min >= count as i64 {
        return None;
    }
    Some(min.max(0) as u32..=max.min(count as i64 - 1) as u32)
}

/// Produce only nearby **visible Block mode** solid boxes. Uses Terrain's
/// effective authored/layer/runtime payload, not raw authored voxels; works
/// for original HeightfieldData worlds and new SmoothVoxelGrid Terrain.
fn for_solid_boxes(world: &World, min: Vec3, max: Vec3, mut f: impl FnMut(SolidBox)) {
    let Some(result) = world.terrain.effective() else {
        return;
    };
    if world.terrain.config.mode != TerrainMode::Block {
        return;
    }
    match &result.data {
        TerrainData::Voxel(grid) => {
            let spacing = grid.spacing_studs;
            let [ox, oy, oz] = grid.origin.map(|x| x as f32);
            let (Some(xs), Some(ys), Some(zs)) = (
                sample_range(min.x, max.x, ox, spacing, grid.size[0]),
                sample_range(min.y, max.y, oy, spacing, grid.size[1]),
                sample_range(min.z, max.z, oz, spacing, grid.size[2]),
            ) else {
                return;
            };
            for z in zs {
                for y in ys.clone() {
                    for x in xs.clone() {
                        if !grid
                            .voxel(x, y, z)
                            .is_some_and(|voxel| voxel.occupancy_unit() >= SMOOTH_VOXEL_ISO_LEVEL)
                        {
                            continue;
                        }
                        let lower = Vec3::new(
                            ox + x as f32 * spacing,
                            oy + y as f32 * spacing,
                            oz + z as f32 * spacing,
                        );
                        f(SolidBox {
                            min: lower,
                            max: lower + Vec3::splat(spacing),
                        });
                    }
                }
            }
        }
        TerrainData::Heightfield(grid) => {
            let [ox, oy, oz] = grid.origin.map(|x| x as f32);
            let (Some(xs), Some(zs)) = (
                sample_range(min.x, max.x, ox, 1.0, grid.size[0]),
                sample_range(min.z, max.z, oz, 1.0, grid.size[2]),
            ) else {
                return;
            };
            for z in zs {
                for x in xs.clone() {
                    let Some(top) = grid.height(x, z).map(f32::round) else {
                        continue;
                    };
                    if top <= oy || top <= min.y || oy >= max.y {
                        continue;
                    }
                    f(SolidBox {
                        min: Vec3::new(ox + x as f32, oy, oz + z as f32),
                        max: Vec3::new(ox + x as f32 + 1.0, top, oz + z as f32 + 1.0),
                    });
                }
            }
        }
    }
}

fn overlaps(a_min: f32, a_max: f32, b_min: f32, b_max: f32) -> bool {
    a_max > b_min + EPS && a_min < b_max - EPS
}

/// Swept axis separation catches a wall even when the character moves beyond
/// its entire thickness in one frame. Ordering X then Z allows wall sliding.
fn horizontal_axis(
    world: &World,
    pos: Vec3,
    wanted: f32,
    axis: usize,
    radius: f32,
    height: f32,
) -> f32 {
    let from = pos[axis];
    let delta = wanted - from;
    if delta.abs() <= EPS {
        return wanted;
    }
    let mut query_min = pos - Vec3::new(radius, 0.0, radius);
    let mut query_max = pos + Vec3::new(radius, height, radius);
    query_min[axis] = from.min(wanted) - radius;
    query_max[axis] = from.max(wanted) + radius;
    let orthogonal = if axis == 0 { 2 } else { 0 };
    let mut resolved = wanted;
    for_solid_boxes(world, query_min, query_max, |solid| {
        if !overlaps(pos.y, pos.y + height, solid.min.y, solid.max.y)
            || !overlaps(
                pos[orthogonal] - radius,
                pos[orthogonal] + radius,
                solid.min[orthogonal],
                solid.max[orthogonal],
            )
        {
            return;
        }
        if delta > 0.0 {
            // Only faces approached from outside constrain this sweep.
            if from + radius <= solid.min[axis] + EPS && wanted + radius > solid.min[axis] {
                resolved = resolved.min(solid.min[axis] - radius);
            }
        } else if from - radius >= solid.max[axis] - EPS && wanted - radius < solid.max[axis] {
            resolved = resolved.max(solid.max[axis] + radius);
        }
    });
    resolved
}

pub(super) fn resolve_block_terrain_collision(
    character: &mut Character,
    world: &World,
    previous: Vec3,
) {
    let radius = character.collision.radius;
    let height = character.collision.height;
    let desired = character.transform.position;
    if !radius.is_finite()
        || radius <= 0.0
        || !height.is_finite()
        || height <= 0.0
        || !previous.is_finite()
        || !desired.is_finite()
    {
        return;
    }

    // Use the high end of the foot's vertical sweep for lateral blocking.
    // Standing on the same-height floor must not look like hitting its side
    // merely because gravity moved the foot infinitesimally downward.
    let horizontal_y = previous.y.max(desired.y);
    let start = Vec3::new(previous.x, horizontal_y, previous.z);
    let x = horizontal_axis(world, start, desired.x, 0, radius, height);
    let after_x = Vec3::new(x, horizontal_y, previous.z);
    let z = horizontal_axis(world, after_x, desired.z, 2, radius, height);
    let mut y = desired.y;
    let mut grounded = false;

    // Floor and ceiling sweeps use the post-wall-slide X/Z position.
    let horizontal_min = Vec3::new(x - radius, previous.y.min(desired.y), z - radius);
    let horizontal_max = Vec3::new(x + radius, previous.y.max(desired.y) + height, z + radius);
    for_solid_boxes(world, horizontal_min, horizontal_max, |solid| {
        if !overlaps(x - radius, x + radius, solid.min.x, solid.max.x)
            || !overlaps(z - radius, z + radius, solid.min.z, solid.max.z)
        {
            return;
        }
        if desired.y < previous.y - EPS
            && previous.y >= solid.max.y - EPS
            && desired.y < solid.max.y
        {
            let top = solid.max.y;
            if y < top {
                y = top;
                grounded = true;
            }
        } else if desired.y > previous.y + EPS
            && previous.y + height <= solid.min.y + EPS
            && desired.y + height > solid.min.y
        {
            y = y.min(solid.min.y - height);
        }
    });
    character.transform.position = Vec3::new(x, y, z);
    if (x - desired.x).abs() > EPS {
        character.movement.velocity.x = 0.0;
    }
    if (z - desired.z).abs() > EPS {
        character.movement.velocity.z = 0.0;
    }
    if grounded {
        character.movement.velocity.y = 0.0;
        character.movement.is_grounded = true;
    } else if (y - desired.y).abs() > EPS {
        character.movement.velocity.y = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::terrain::{
        SmoothVoxel, SmoothVoxelGrid, TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    };
    fn block_world(cells: &[(u32, u32, u32)]) -> World {
        let mut world = World::new();
        world.terrain.config.mode = TerrainMode::Block;
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [0, 0, 0],
            [10, 6, 10],
            2.0,
            vec!["tile_24".into()],
        )
        .unwrap();
        for &(x, y, z) in cells {
            grid.set_voxel(
                x,
                y,
                z,
                SmoothVoxel {
                    material: 0,
                    occupancy: 255,
                },
            );
        }
        world.terrain.replace_authored(Some(TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [20, 12, 20],
            },
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Voxel(grid),
        }));
        world
    }
    fn moving(from: Vec3, to: Vec3, velocity: Vec3, world: &World) -> Character {
        let mut c = Character::new(1, from);
        c.transform.position = to;
        c.movement.velocity = velocity;
        resolve_block_terrain_collision(&mut c, world, from);
        c
    }

    #[test]
    fn blocks_wall_from_both_x_directions_including_large_frame_motion() {
        let world = block_world(&[(3, 0, 2), (3, 1, 2)]);
        let west = moving(
            Vec3::new(5.0, 0.0, 5.0),
            Vec3::new(12.0, 0.0, 5.0),
            Vec3::new(7.0, 0.0, 0.0),
            &world,
        );
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:26 -05:00
        // The wall face is x=6; use the actual imported collider radius.
        assert!((west.transform.position.x - (6.0 - west.collision.radius)).abs() < 0.001);
        assert_eq!(west.movement.velocity.x, 0.0);
        let east = moving(
            Vec3::new(10.0, 0.0, 5.0),
            Vec3::new(1.0, 0.0, 5.0),
            Vec3::new(-9.0, 0.0, 0.0),
            &world,
        );
        assert!((east.transform.position.x - (8.0 + east.collision.radius)).abs() < 0.001);
    }

    #[test]
    fn blocks_z_walls_and_diagonal_motion_slides_along_free_axis() {
        let world = block_world(&[(2, 0, 3), (2, 1, 3)]);
        let c = moving(
            Vec3::new(5.0, 0.0, 5.0),
            Vec3::new(5.7, 0.0, 10.0),
            Vec3::new(0.7, 0.0, 5.0),
            &world,
        );
        assert!((c.transform.position.z - (6.0 - c.collision.radius)).abs() < 0.001);
        assert!((c.transform.position.x - 5.7).abs() < 0.001);
    }

    #[test]
    fn floor_landing_and_ceiling_jump_block_without_snapping_to_walls() {
        let floor = block_world(&[(2, 0, 2)]);
        let landed = moving(
            Vec3::new(5.0, 3.0, 5.0),
            Vec3::new(5.0, 1.0, 5.0),
            Vec3::new(0.0, -2.0, 0.0),
            &floor,
        );
        assert!((landed.transform.position.y - 2.0).abs() < 0.001);
        assert!(landed.movement.is_grounded);

        let ceiling = block_world(&[(2, 2, 2)]);
        let jumped = moving(
            // Start below the ceiling even with Rogue's full-height collider.
            Vec3::new(5.0, 0.5, 5.0),
            Vec3::new(5.0, 3.5, 5.0),
            Vec3::new(0.0, 2.0, 0.0),
            &ceiling,
        );
        assert!((jumped.transform.position.y - (4.0 - jumped.collision.height)).abs() < 0.001);
        assert_eq!(jumped.movement.velocity.y, 0.0);
    }

    // Worldkiln 0.8.5 | WO 0.8.7-57 | 2026-10-09
    #[test]
    fn full_character_system_update_stops_at_block_cliff_without_new_physics_bodies() {
        use crate::character::system::CharacterSystem;
        use glam::Vec2;
        let mut world = block_world(&[(3, 0, 2), (3, 1, 2)]);
        world.gravity = Vec3::ZERO;
        let mut controller = CharacterSystem::new();
        controller
            .characters
            .insert(1, Character::new(1, Vec3::new(5.0, 0.5, 5.0)));
        controller.active_player_id = Some(1);
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        physics.register_from_world(&world);
        for _ in 0..50 {
            controller.update(&world, &mut physics, 1.0 / 60.0, Vec2::X, false);
        }
        let avatar = controller.get_active_player().unwrap();
        assert!(
            avatar.transform.position.x <= 5.601,
            "runtime CharacterSystem let player pass through voxel wall: {:?}",
            avatar.transform.position
        );
        assert_eq!(avatar.movement.velocity.x, 0.0);
        controller.update(&world, &mut physics, 1.0 / 60.0, -Vec2::X, false);
        assert!(
            controller.get_active_player().unwrap().transform.position.x < 5.6,
            "player must be able to step away from blocked wall"
        );
    }

    #[test]
    fn older_heightfield_block_terrain_still_has_solid_sides() {
        use crate::world::terrain::HeightfieldData;
        let mut world = World::new();
        world.terrain.config.mode = TerrainMode::Block;
        let mut heights = vec![0.0; 8 * 8];
        heights[2 * 8 + 2] = 4.0;
        let data = HeightfieldData {
            origin: [0, 0, 0],
            size: [8, 6, 8],
            heights,
        };
        world.terrain.replace_authored(Some(TerrainResult {
            request: TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 6, 8],
            },
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Heightfield(data),
        }));
        let avatar = moving(
            Vec3::new(1.0, 1.0, 2.5),
            Vec3::new(5.0, 1.0, 2.5),
            Vec3::new(4.0, 0.0, 0.0),
            &world,
        );
        assert!((avatar.transform.position.x - (2.0 - avatar.collision.radius)).abs() < 0.001);
    }

    #[test]
    fn non_solid_or_outside_cells_do_not_block() {
        let world = block_world(&[]);
        let c = moving(
            Vec3::new(5.0, 0.0, 5.0),
            Vec3::new(12.0, 0.0, 5.0),
            Vec3::new(7.0, 0.0, 0.0),
            &world,
        );
        assert_eq!(c.transform.position.x, 12.0);
    }
}
