// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::character::Character;
use crate::world::{TerrainData, World};
use glam::Vec3;

pub const WALKABLE_NORMAL_Y: f32 = std::f32::consts::FRAC_1_SQRT_2;
const GROUNDED_HYSTERESIS_Y: f32 = 0.669_131;

#[derive(Clone, Copy, Debug)]
pub struct TerrainContact {
    pub height: f32,
    pub normal: Vec3,
}

impl TerrainContact {
    pub fn is_walkable(self) -> bool {
        self.normal.y >= WALKABLE_NORMAL_Y
    }

    pub fn is_walkable_with_previous(self, previously_grounded: bool) -> bool {
        self.normal.y
            >= if previously_grounded {
                GROUNDED_HYSTERESIS_Y
            } else {
                WALKABLE_NORMAL_Y
            }
    }

    pub fn is_walkable_at(self, angle: f32, previously_grounded: bool) -> bool {
        super::collision::contact_is_walkable(self.normal, angle, previously_grounded)
    }

    pub fn downhill(self) -> Vec3 {
        let gravity = Vec3::NEG_Y;
        let tangent = gravity - self.normal * gravity.dot(self.normal);
        Vec3::new(tangent.x, 0.0, tangent.z)
    }
}

pub fn surface_contact(world: &World, world_x: f32, world_z: f32) -> Option<TerrainContact> {
    let result = world.terrain.effective()?;
    let origin = result.request.position;
    let end_x = origin[0] as f32 + result.request.size[0] as f32;
    let end_z = origin[2] as f32 + result.request.size[2] as f32;
    if !world_x.is_finite()
        || !world_z.is_finite()
        || world_x < origin[0] as f32
        || world_z < origin[2] as f32
        || world_x >= end_x
        || world_z >= end_z
    {
        return None;
    }
    let spacing = match &result.data {
        TerrainData::Voxel(grid) => grid.spacing_studs,
        TerrainData::Heightfield(_) => 1.0,
    };
    if !spacing.is_finite() || spacing <= 0.0 {
        return None;
    }
    let half = if matches!(&result.data, TerrainData::Heightfield(_)) {
        spacing * 0.5
    } else {
        0.0
    };
    let gx = ((world_x - origin[0] as f32 - half) / spacing).max(0.0);
    let gz = ((world_z - origin[2] as f32 - half) / spacing).max(0.0);
    let ix = gx.floor();
    let iz = gz.floor();
    let tx = (gx - ix).clamp(0.0, 1.0);
    let tz = (gz - iz).clamp(0.0, 1.0);
    let x0 = origin[0] as f32 + half + ix * spacing;
    let z0 = origin[2] as f32 + half + iz * spacing;
    let sample = |x: f32, z: f32| {
        world.terrain.surface_height_at(
            x.clamp(origin[0] as f32, end_x - 0.0005),
            z.clamp(origin[2] as f32, end_z - 0.0005),
        )
    };
    let h00 = sample(x0, z0)?;
    let h10 = sample(x0 + spacing, z0)?;
    let h01 = sample(x0, z0 + spacing)?;
    let h11 = sample(x0 + spacing, z0 + spacing)?;
    let (height, sx, sz) = if matches!(&result.data, TerrainData::Heightfield(_)) {
        if tx >= tz {
            let sx = (h10 - h00) / spacing;
            let sz = (h11 - h10) / spacing;
            (h00 + tx * (h10 - h00) + tz * (h11 - h10), sx, sz)
        } else {
            let sx = (h11 - h01) / spacing;
            let sz = (h01 - h00) / spacing;
            (h00 + tx * (h11 - h01) + tz * (h01 - h00), sx, sz)
        }
    } else if tx + tz <= 1.0 {
        let sx = (h10 - h00) / spacing;
        let sz = (h01 - h00) / spacing;
        (h00 + tx * (h10 - h00) + tz * (h01 - h00), sx, sz)
    } else {
        let sx = (h11 - h01) / spacing;
        let sz = (h11 - h10) / spacing;
        (
            h11 - (1.0 - tx) * (h11 - h01) - (1.0 - tz) * (h11 - h10),
            sx,
            sz,
        )
    };
    let normal = Vec3::new(-sx, 1.0, -sz).normalize_or_zero();
    if normal.y <= 0.0 || !height.is_finite() {
        return None;
    }
    Some(TerrainContact { height, normal })
}

pub fn slide_velocity(
    previous: Vec3,
    contact: Option<TerrainContact>,
    gravity: Vec3,
    dt: f32,
) -> Vec3 {
    slide_velocity_with_limit(previous, contact, gravity, dt, 45.0)
}

pub fn slide_velocity_with_limit(
    previous: Vec3,
    contact: Option<TerrainContact>,
    gravity: Vec3,
    dt: f32,
    max_walkable_slope_degrees: f32,
) -> Vec3 {
    if !dt.is_finite() || dt <= 0.0 {
        return previous;
    }
    let dt = dt.min(0.05);
    let Some(contact) = contact else {
        return previous * (-8.0 * dt).exp();
    };
    let slope_sine = (1.0 - contact.normal.y * contact.normal.y).max(0.0).sqrt();
    let friction = if contact.is_walkable_at(max_walkable_slope_degrees, false) {
        14.0
    } else {
        2.5
    };
    let mut slide = previous * (-friction * dt).exp();
    let t = ((slope_sine - 0.588) / (0.839 - 0.588)).clamp(0.0, 1.0);
    let slide_strength = t * t * (3.0 - 2.0 * t);
    if slide_strength == 0.0 || contact.normal.y < 0.15 {
        return if slide.length_squared() < 0.0004 {
            Vec3::ZERO
        } else {
            slide
        };
    }
    let downhill = contact.downhill().normalize_or_zero();
    let acceleration = gravity.length()
        * slope_sine
        * (0.65 + 0.65 * slope_sine)
        * slide_strength
        * ((contact.normal.y - 0.15) / 0.35).clamp(0.0, 1.0);
    slide += downhill * (acceleration * dt);
    let max_speed = if contact.is_walkable_at(max_walkable_slope_degrees, false) {
        1.8
    } else {
        12.0
    };
    slide.clamp_length_max(max_speed)
}

pub fn resolve_terrain_movement(
    character: &mut Character,
    world: &World,
    previous_position: Vec3,
    was_grounded: bool,
) {
    let current = character.transform.position;
    let delta = current - previous_position;
    let horizontal = Vec3::new(delta.x, 0.0, delta.z);
    let distance = horizontal.length();
    let mut resolved = current;

    if distance > 1e-6 {
        let direction = horizontal / distance;
        let lateral = Vec3::new(-direction.z, 0.0, direction.x);
        let radius = character.collision.radius.max(0.05);
        let steps = (distance / (radius * 0.35).max(0.035))
            .ceil()
            .clamp(1.0, 256.0) as usize;
        let mut accepted = previous_position;

        for i in 1..=steps {
            let t = i as f32 / steps as f32;
            let candidate = previous_position + delta * t;
            let previous = previous_position + delta * ((i - 1) as f32 / steps as f32);
            let mut blocking_normal = None;
            for offset in [
                Vec3::ZERO,
                lateral * (radius * 0.65),
                -lateral * (radius * 0.65),
            ] {
                let before = surface_contact(world, previous.x + offset.x, previous.z + offset.z);
                let after = surface_contact(world, candidate.x + offset.x, candidate.z + offset.z);
                let (Some(before), Some(after)) = (before, after) else {
                    continue;
                };

                let rise = after.height - before.height;
                let against = direction.dot(Vec3::new(after.normal.x, 0.0, after.normal.z));
                let contacts_side = after.height > previous.y + radius * 0.15;
                if !after.is_walkable_at(world.max_walkable_slope_degrees, was_grounded)
                    && rise > 0.0001
                    && against < -0.01
                {
                    blocking_normal = Some(after.normal);
                    break;
                }
            }
            if let Some(normal) = blocking_normal {
                let out = Vec3::new(normal.x, 0.0, normal.z).normalize_or_zero();
                let remaining = horizontal * (1.0 - (i - 1) as f32 / steps as f32);
                let tangent = remaining - out * remaining.dot(out).min(0.0);
                resolved.x = accepted.x + tangent.x;
                resolved.z = accepted.z + tangent.z;

                let velocity_into = character.movement.velocity.dot(out).min(0.0);
                character.movement.velocity -= out * velocity_into;
                break;
            }
            accepted = candidate;
        }
    }
    character.transform.position.x = resolved.x;
    character.transform.position.z = resolved.z;
    if character.movement.velocity.y > 0.0 {
        return;
    }
    let Some(surface) = surface_contact(
        world,
        character.transform.position.x,
        character.transform.position.z,
    ) else {
        return;
    };
    let walkable = surface.is_walkable_at(world.max_walkable_slope_degrees, was_grounded);

    let prev_contact = surface_contact(world, previous_position.x, previous_position.z);
    let near_previous_ground = prev_contact.is_some_and(|ground| {
        (previous_position.y - ground.height).abs() <= 0.35 + character.collision.radius
    });
    let current_bottom = character.transform.position.y;
    let snap_down = (0.1 + distance * 1.2 + character.collision.radius * 0.25).min(1.4);
    let crossed_surface =
        current_bottom <= surface.height && previous_position.y >= surface.height - 0.02;
    if current_bottom <= surface.height + snap_down
        && (was_grounded || near_previous_ground || crossed_surface)
        && previous_position.y >= surface.height - 1.4
        && (walkable || (surface.normal.y > 0.15 && near_previous_ground))
    {
        character.transform.position.y = surface.height;
        if walkable {
            character.movement.velocity.y = 0.0;
            character.movement.is_grounded = true;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::terrain::{
        HeightfieldData, PLAINS_GENERATOR_IMPLEMENTATION_VERSION, TerrainData,
        TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
    };

    fn ramp(slope: f32) -> World {
        let mut world = World::new();
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
                heights: (0..144).map(|i| i as f32 % 12.0 * slope).collect(),
            }),
        });
        world
    }

    #[test]
    fn triangle_normals_represent_ground_slope_not_shading() {
        let ground = ramp(0.65);
        let contact = surface_contact(&ground, 5.35, 6.4).unwrap();
        assert!((contact.normal.y - (1.0 / (1.0_f32 + 0.65 * 0.65).sqrt())).abs() < 0.001);
        assert!(contact.is_walkable());
        assert!(contact.downhill().x < 0.0);
        assert!(contact.downhill().z.abs() < 0.001);
        assert!(surface_contact(&ground, -0.1, 2.0).is_none());
    }

    #[test]
    fn gravity_increases_downhill_slide_as_slope_steepens() {
        let gentle = surface_contact(&ramp(0.35), 5.0, 6.0).unwrap();
        let steep = surface_contact(&ramp(1.25), 5.0, 6.0).unwrap();
        let mut slow = Vec3::ZERO;
        let mut fast = Vec3::ZERO;
        for _ in 0..45 {
            slow = slide_velocity(slow, Some(gentle), Vec3::new(0.0, -9.81, 0.0), 1.0 / 60.0);
            fast = slide_velocity(fast, Some(steep), Vec3::new(0.0, -9.81, 0.0), 1.0 / 60.0);
        }
        assert!(
            fast.x < slow.x - 0.8,
            "steep slope must accelerate downhill"
        );
        assert_eq!(slow, Vec3::ZERO, "gentle hills should not slide when idle");
        assert!(slow.y.abs() < 0.001 && fast.y.abs() < 0.001);
        let stopped = slide_velocity(fast, None, Vec3::NEG_Y, 0.05);
        assert!(stopped.length() < fast.length());
    }

    #[test]
    fn gentle_hills_hold_a_stationary_character_without_sliding() {
        let gravity = Vec3::new(0.0, -9.81, 0.0);
        for slope in [0.0, 0.2, 0.35, 0.65] {
            let ground = surface_contact(&ramp(slope), 5.0, 6.0).unwrap();
            let mut motion = Vec3::ZERO;
            for _ in 0..180 {
                motion = slide_velocity(motion, Some(ground), gravity, 1.0 / 60.0);
            }
            assert!(
                motion.length() < 0.0001,
                "gentle slope {slope} drifts when idle: {motion:?}"
            );
        }
    }

    #[test]
    fn downhill_slide_starts_gradually_on_steeper_terrain() {
        let gravity = Vec3::new(0.0, -9.81, 0.0);
        let just_above = surface_contact(&ramp(0.74), 5.0, 6.0).unwrap();
        let steep = surface_contact(&ramp(1.4), 5.0, 6.0).unwrap();
        let gentle = surface_contact(&ramp(0.35), 5.0, 6.0).unwrap();
        let mut small = Vec3::ZERO;
        let mut large = Vec3::ZERO;
        for _ in 0..90 {
            small = slide_velocity(small, Some(just_above), gravity, 1.0 / 60.0);
            large = slide_velocity(large, Some(steep), gravity, 1.0 / 60.0);
        }
        assert!(small.x < 0.0, "steeper hills should begin sliding");
        assert!(
            large.length() > small.length() + 0.6,
            "steep cliffs should slide more strongly"
        );
        for _ in 0..120 {
            large = slide_velocity(large, Some(gentle), gravity, 1.0 / 60.0);
        }
        assert!(
            large.length() < 0.001,
            "momentum should settle after returning to gentle ground"
        );
    }

    #[test]
    fn grounded_character_walks_continuously_up_and_down_a_slope() {
        let ground = ramp(0.65);
        let start_height = surface_contact(&ground, 3.5, 6.0).unwrap().height;
        let mut actor = Character::new(442, Vec3::new(3.5, start_height, 6.0));
        actor.movement.is_grounded = true;
        for direction in [1.0, -1.0] {
            for _ in 0..30 {
                let prev = actor.transform.position;
                actor.movement.is_grounded = false;
                actor.movement.velocity = Vec3::new(direction * 4.0, -0.16, 0.0);
                actor.transform.position += actor.movement.velocity / 60.0;
                resolve_terrain_movement(&mut actor, &ground, prev, true);
                assert!(
                    actor.movement.is_grounded,
                    "walkable slope should stay grounded"
                );
                assert!(
                    (actor.transform.position.x - prev.x).abs() > 0.05,
                    "the actor must not stop at small height changes"
                );
            }
        }
    }

    #[test]
    fn slope_hysteresis_prevents_walkable_boundary_teetering() {
        let between = surface_contact(&ramp(1.04), 4.2, 5.2).unwrap();
        assert!(!between.is_walkable());
        assert!(
            between.is_walkable_with_previous(true),
            "grounded character keeps contact through a small normal change"
        );
        let too_steep = surface_contact(&ramp(1.35), 4.2, 5.2).unwrap();
        assert!(!too_steep.is_walkable_with_previous(true));
    }

    #[test]
    fn steep_terrain_is_not_classified_as_walkable() {
        let contact = surface_contact(&ramp(1.7), 4.3, 4.6).unwrap();
        assert!(!contact.is_walkable());
        assert!(contact.downhill().x < 0.0);
        assert!(contact.downhill().length_squared() > 0.0);
    }

    #[test]
    fn sampled_collision_heights_respond_immediately_to_sculpted_geometry() {
        let mut ground = ramp(0.1);
        let first = surface_contact(&ground, 5.35, 6.4).unwrap().height;
        if let Some(result) = ground.terrain.authored.as_mut() {
            if let TerrainData::Heightfield(field) = &mut result.data {
                field.heights[6 * 12 + 5] += 2.0;
            }
        }
        let changed = surface_contact(&ground, 5.35, 6.4).unwrap().height;
        assert!(changed > first + 0.3);
    }
}
