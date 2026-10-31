// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub mod character_state;
pub mod entity_control;
mod navigation_build;
pub mod navigation_control;
pub mod pathfinding;

#[cfg(test)]
mod system_tests;

use super::animation::AnimationState;
use super::animation::TargetAnimation;
use super::character::Character;
use super::collision::{
    resolve_character_collisions_with_world, resolve_dynamic_body_collisions,
    resolve_static_voxel_collisions,
};
use super::movement::{JUMP_IMPULSE, MovementState, SwimMode};
use super::terrain_motion::{resolve_terrain_movement, slide_velocity_with_limit, surface_contact};
use crate::engine::entity::EntityId;
use crate::navigation::{NavigationAgent, NavigationGrid};
use crate::world::World;
use glam::{Quat, Vec2};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(crate) struct CachedNavigationGrid {
    pub(crate) policy: String,
    pub(crate) world_revision: u64,
    pub(crate) terrain_revision: u64,
    pub(crate) profiles: std::collections::BTreeMap<(u32, u32), NavigationGrid>,
}

#[derive(Default)]
pub struct CharacterSystem {
    pub(crate) characters: HashMap<u64, Character>,
    animation_budget_frame: u64,

    pub(crate) entity_to_character: HashMap<EntityId, u64>,

    pub(crate) character_to_entity: HashMap<u64, EntityId>,

    pub(crate) collision_step_origins: HashMap<u64, glam::Vec3>,

    pub(crate) kinematic_entities: HashSet<EntityId>,

    pub(crate) active_player_id: Option<u64>,
    ocean_appearance: Option<crate::world::terrain::TerrainWaterSettings>,

    pub(crate) next_id: u64,

    pub(crate) navigation_cache: RefCell<Option<CachedNavigationGrid>>,
    pub(crate) navigation_builds: RefCell<navigation_build::NavigationBuildService>,
    pub(crate) navigation_pending: HashMap<EntityId, navigation_control::PendingNavigationRequest>,

    pub(crate) navigation_followers: HashMap<u64, navigation_control::NavigationFollowState>,

    pub(crate) navigation_finished: Vec<EntityId>,

    pub(crate) navigation_failed: Vec<(EntityId, navigation_control::NavigationFailureReason)>,

    pending_animation_events: Vec<CharacterAnimationEvent>,
}

#[derive(Clone, Debug)]
pub struct CharacterAnimationEvent {
    pub character_id: u64,
    pub entity_id: Option<EntityId>,
    pub name: String,
    pub payload: std::collections::BTreeMap<String, crate::animation::EventValue>,
}

fn buoyancy_acceleration(gravity: glam::Vec3, immersed: f32, relative_density: f32) -> glam::Vec3 {
    -gravity * (immersed.clamp(0.0, 1.0) / relative_density.clamp(0.3, 5.0))
}
const SWIM_SURFACE_FEET_OFFSET: f32 = 0.78;

fn swim_mode_step(
    previous: SwimMode,
    surface: Option<f32>,
    foot_y: f32,
    height: f32,
    immersion: f32,
    can_stand: bool,
    look_y: f32,
    signed_forward_speed: f32,
    vertical_velocity: f32,
) -> SwimMode {
    let Some(surface_y) = surface else {
        return SwimMode::Dry;
    };
    if !surface_y.is_finite() || can_stand || immersion <= 0.15 {
        return SwimMode::Dry;
    }
    let _ = (look_y, signed_forward_speed);
    if previous == SwimMode::Surface {
        return SwimMode::Surface;
    }
    let eye_y = foot_y + height * 0.83;
    if previous == SwimMode::Underwater && eye_y >= surface_y - 0.09 && vertical_velocity >= -0.35 {
        SwimMode::Surface
    } else {
        SwimMode::Underwater
    }
}
fn surface_lock_position(
    current_foot_y: f32,
    surface_y: f32,
    height: f32,
    dt: f32,
    blend: &mut f32,
) -> f32 {
    let target = surface_y - height.max(0.1) * SWIM_SURFACE_FEET_OFFSET;
    *blend = (*blend + dt.max(0.0) * 12.0).clamp(0.0, 1.0);
    current_foot_y + (target - current_foot_y) * (*blend)
}

impl CharacterSystem {
    pub fn drain_animation_events(&mut self) -> impl Iterator<Item = CharacterAnimationEvent> + '_ {
        self.pending_animation_events.drain(..)
    }

    pub fn is_player_controller_enabled(&self) -> bool {
        self.get_active_player()
            .is_none_or(|c| c.movement.controller_enabled)
    }

    pub fn set_ocean_appearance(
        &mut self,
        appearance: crate::world::terrain::TerrainWaterSettings,
    ) {
        self.ocean_appearance = Some(appearance);
    }
    pub fn try_surface_jump(&mut self, impulse: f32) -> bool {
        let Some(player) = self.get_active_player_mut() else {
            return false;
        };
        if player.movement.swim_mode != SwimMode::Surface || !impulse.is_finite() || impulse <= 0.0
        {
            return false;
        }
        player.movement.velocity.y = impulse;
        player.movement.is_grounded = false;
        player.movement.surface_jump_remaining = 0.52;
        player.movement.swim_mode = SwimMode::Dry;
        player.movement.is_swimming = false;
        player.movement.surface_lock_blend = 0.0;
        true
    }
    pub fn new() -> Self {
        Self {
            animation_budget_frame: 0,
            characters: HashMap::new(),
            entity_to_character: HashMap::new(),
            character_to_entity: HashMap::new(),
            collision_step_origins: HashMap::new(),
            kinematic_entities: HashSet::new(),
            active_player_id: None,
            ocean_appearance: None,
            next_id: 1,
            navigation_cache: RefCell::new(None),
            navigation_builds: RefCell::default(),
            navigation_pending: HashMap::new(),
            navigation_followers: HashMap::new(),
            navigation_finished: Vec::new(),
            navigation_failed: Vec::new(),
            pending_animation_events: Vec::new(),
        }
    }

    pub fn update(
        &mut self,
        world: &World,
        physics_world: &mut crate::engine::physics::PhysicsWorld,
        dt: f32,
        input: Vec2,
        jump_requested: bool,
    ) -> HashSet<u64> {
        self.update_internal(
            world,
            physics_world,
            dt,
            Some(input),
            jump_requested,
            false,
            &HashSet::new(),
        )
        .into_iter()
        .map(|(_, cell_id)| cell_id)
        .collect()
    }

    pub fn set_active_swim_look_direction(&mut self, forward_y: f32, forward_xz: glam::Vec2) {
        if let Some(p) = self.get_active_player_mut() {
            p.movement.swim_look_y = if forward_y.is_finite() {
                forward_y.clamp(-1.0, 1.0)
            } else {
                0.0
            };
            p.movement.swim_camera_forward =
                if forward_xz.is_finite() && forward_xz.length_squared() > 0.0001 {
                    forward_xz.normalize()
                } else {
                    glam::Vec2::ZERO
                };
        }
    }

    pub fn update_scripted(
        &mut self,
        world: &World,
        physics_world: &mut crate::engine::physics::PhysicsWorld,
        dt: f32,
    ) -> HashSet<(EntityId, u64)> {
        self.update_scripted_excluding(world, physics_world, dt, &HashSet::new())
    }

    pub fn update_scripted_excluding(
        &mut self,
        world: &World,
        physics_world: &mut crate::engine::physics::PhysicsWorld,
        dt: f32,
        externally_transformed: &HashSet<EntityId>,
    ) -> HashSet<(EntityId, u64)> {
        let excluded_characters: HashSet<u64> = self
            .entity_to_character
            .iter()
            .filter_map(|(entity_id, character_id)| {
                (self.kinematic_entities.contains(entity_id)
                    || externally_transformed.contains(entity_id))
                .then_some(*character_id)
            })
            .collect();
        self.update_internal(
            world,
            physics_world,
            dt,
            None,
            false,
            true,
            &excluded_characters,
        )
    }

    fn update_internal(
        &mut self,
        world: &World,
        physics_world: &mut crate::engine::physics::PhysicsWorld,
        dt: f32,
        input: Option<Vec2>,
        jump_requested: bool,
        scripted: bool,
        externally_transformed: &HashSet<u64>,
    ) -> HashSet<(EntityId, u64)> {
        if self.has_pending_navigation_work() {
            self.tick_navigation_builds(world);
        }
        let mut contacts = HashSet::new();
        let _update_span =
            crate::engine::development_capture::operation("character", "system_update", true);
        self.collision_step_origins.clear();
        for c in self.characters.values_mut() {
            c.refresh_collision_profile();
        }
        let look = self.ocean_appearance.as_ref();

        let allocator_span =
            crate::engine::development_capture::operation("animation", "pose_allocator", true);
        self.animation_budget_frame = self.animation_budget_frame.wrapping_add(1).max(1);
        let frame = self.animation_budget_frame;
        let player_pos = self
            .active_player_id
            .and_then(|id| self.characters.get(&id).map(|c| c.transform.position));
        let mut ranked: Vec<(u64, f32)> = self
            .characters
            .iter()
            .map(|(&id, c)| {
                let distance2 = player_pos
                    .map(|p| (c.transform.position - p).length_squared())
                    .unwrap_or(0.0);
                (id, distance2)
            })
            .collect();
        ranked.sort_by(|a, b| {
            (a.0 != self.active_player_id.unwrap_or(u64::MAX))
                .cmp(&(b.0 != self.active_player_id.unwrap_or(u64::MAX)))
                .then_with(|| a.1.total_cmp(&b.1))
                .then_with(|| a.0.cmp(&b.0))
        });
        let budget_ns =
            ((world.animation_pose_budget_ms.clamp(0.25, 8.0) as f64) * 1_000_000.0) as u64;
        let mut predicted_ns = 0u64;
        let mut evaluate_pose = HashSet::with_capacity(self.characters.len());
        for (id, distance2) in ranked {
            let c = &self.characters[&id];
            let age = frame.saturating_sub(c.pose_last_frame);
            let important = self.active_player_id == Some(id) || distance2 <= 144.0;
            let interval = if distance2 <= 625.0 {
                2
            } else if distance2 <= 2500.0 {
                4
            } else {
                6
            };
            let due = c.pose_last_frame == 0 || age >= interval;
            let urgent = age >= 12;
            if !world.animation_budget_enabled
                || important
                || urgent
                || (due && predicted_ns.saturating_add(c.pose_estimate_ns) <= budget_ns)
            {
                evaluate_pose.insert(id);
                predicted_ns = predicted_ns.saturating_add(c.pose_estimate_ns);
            }
        }

        drop(allocator_span.map(|span| {
            span.diagnostic(serde_json::json!({
                "characters": self.characters.len(), "poses_selected": evaluate_pose.len(),
                "budget_enabled": world.animation_budget_enabled, "budget_ns": budget_ns,
                "predicted_ns": predicted_ns, "simulation_tick": frame,
            }))
        }));
        for (&character_id, character) in self.characters.iter_mut() {
            let _character_span = crate::engine::development_capture::operation(
                "character",
                "runtime_entity_total",
                true,
            )
            .map(|span| {
                span.character_identity(
                    character_id,
                    self.character_to_entity.get(&character_id).map(|id| id.0),
                )
            });
            let movement_span = crate::engine::development_capture::operation(
                "character",
                "runtime_entity_movement_collision",
                true,
            )
            .map(|span| {
                span.character_identity(
                    character_id,
                    self.character_to_entity.get(&character_id).map(|id| id.0),
                )
            });
            let mut character_contacted_cells = HashSet::new();
            let externally_transformed = externally_transformed.contains(&character_id);
            if !externally_transformed {
                self.collision_step_origins
                    .insert(character_id, character.transform.position);
            }

            if scripted && !externally_transformed {
                navigation_control::apply_navigation_intent(
                    character_id,
                    character,
                    dt,
                    &self.character_to_entity,
                    &mut self.navigation_followers,
                    &mut self.navigation_finished,
                    &mut self.navigation_failed,
                );
            }

            if !scripted && !externally_transformed {
                let horizontal_velocity = match input {
                    Some(input) if input.length_squared() > 0.001 => {
                        input.normalize() * world.character_walk_speed
                    }

                    _ => Vec2::ZERO,
                };

                character.movement.velocity.x = horizontal_velocity.x;

                character.movement.velocity.z = horizontal_velocity.y;

                if jump_requested
                    && (character.movement.is_grounded
                        || character.movement.swim_mode == SwimMode::Surface)
                {
                    character.movement.velocity.y = JUMP_IMPULSE;
                    character.movement.is_grounded = false;
                    character.movement.slope_slide_velocity = glam::Vec3::ZERO;
                    if character.movement.swim_mode == SwimMode::Surface {
                        character.movement.surface_jump_remaining = 0.52;
                        character.movement.swim_mode = SwimMode::Dry;
                        character.movement.is_swimming = false;
                        character.movement.surface_lock_blend = 0.0;
                    }
                }

                if horizontal_velocity.length_squared() > 0.001 {
                    let facing = horizontal_velocity.normalize();

                    character.facing = facing;

                    let angle = f32::atan2(facing.x, facing.y);

                    character.transform.rotation = Quat::from_rotation_y(angle);
                }
            }

            if !externally_transformed {
                let was_grounded = character.movement.is_grounded;
                let previous_position = character.transform.position;
                let surface = surface_contact(world, previous_position.x, previous_position.z);
                let near_ground = surface.is_some_and(|contact| {
                    (previous_position.y - contact.height).abs()
                        <= character.collision.radius + 0.35
                });
                let on_anchored_floor = was_grounded
                    && physics_world.anchored_floor_at(
                        previous_position,
                        character.collision.radius,
                        0.12,
                    );
                let supported =
                    if character.movement.velocity.y <= 0.0 && near_ground && !on_anchored_floor {
                        surface
                    } else {
                        None
                    };
                character.movement.surface_jump_remaining =
                    (character.movement.surface_jump_remaining - dt.max(0.0)).max(0.0);
                let wave_time = crate::world::terrain::ocean_wave_time_seconds();
                let liquid_surface = if character.movement.surface_jump_remaining > 0.0 {
                    None
                } else {
                    world.swimming_surface_with_appearance(
                        previous_position.x,
                        previous_position.z,
                        wave_time,
                        look,
                    )
                };
                let immersed = liquid_surface
                    .map(|surface| {
                        ((surface - previous_position.y) / character.collision.height.max(0.1))
                            .clamp(0.0, 1.0)
                    })
                    .unwrap_or(0.0);
                let can_stand = liquid_surface.zip(surface).is_some_and(|(water, ground)| {
                    (water - ground.height) <= character.collision.height * 0.86
                        && (previous_position.y - ground.height).abs()
                            <= character.collision.height * 0.85
                        && ground.is_walkable_at(world.max_walkable_slope_degrees, was_grounded)
                });
                let signed_forward =
                    glam::Vec2::new(character.movement.velocity.x, character.movement.velocity.z)
                        .dot(character.movement.swim_camera_forward);
                let previous_mode = character.movement.swim_mode;
                let candidate = swim_mode_step(
                    previous_mode,
                    liquid_surface,
                    previous_position.y,
                    character.collision.height,
                    immersed,
                    can_stand,
                    character.movement.swim_look_y,
                    signed_forward,
                    character.movement.velocity.y,
                );
                character.movement.swim_mode = if previous_mode == SwimMode::Dry
                    && candidate != SwimMode::Dry
                    && immersed <= 0.46
                {
                    SwimMode::Dry
                } else {
                    candidate
                };
                character.movement.is_swimming = character.movement.swim_mode != SwimMode::Dry;
                if character.movement.swim_mode != SwimMode::Surface {
                    character.movement.surface_lock_blend = 0.0;
                }
                character.movement.slope_slide_velocity = if character.movement.is_swimming {
                    glam::Vec3::ZERO
                } else {
                    slide_velocity_with_limit(
                        character.movement.slope_slide_velocity,
                        supported,
                        world.gravity * character.movement.gravity_scale,
                        dt,
                        world.max_walkable_slope_degrees,
                    )
                };
                if !scripted && input.is_some_and(|direction| direction.length_squared() > 0.001) {
                    character.movement.slope_slide_velocity = glam::Vec3::ZERO;
                }
                let buoyancy = buoyancy_acceleration(
                    world.gravity * character.movement.gravity_scale,
                    immersed,
                    character.movement.relative_density,
                );
                character.movement.velocity +=
                    (world.gravity * character.movement.gravity_scale + buoyancy) * dt;
                if character.movement.is_swimming {
                    character.movement.is_grounded = false;
                    character.movement.velocity *= (-1.7 * dt).exp();
                    if character.movement.swim_mode == SwimMode::Surface {
                        character.movement.velocity.y = 0.0;
                    }
                }

                let river_current = world.water_current_at(previous_position);
                let move_velocity = character.movement.velocity
                    + character.movement.slope_slide_velocity
                    + river_current;
                let rise = if character.movement.velocity.y <= 0.0 && (was_grounded || near_ground)
                {
                    supported
                        .filter(|face| {
                            face.is_walkable_at(world.max_walkable_slope_degrees, was_grounded)
                        })
                        .map_or(0.0, |face| {
                            -(face.normal.x * move_velocity.x + face.normal.z * move_velocity.z)
                                / face.normal.y.max(0.01)
                                * dt
                        })
                } else {
                    0.0
                };
                let horizontal_speed = glam::Vec2::new(move_velocity.x, move_velocity.z).length();
                let slope_vertical_speed = if dt > 0.0 { rise / dt } else { 0.0 };
                let tangent_speed = horizontal_speed.hypot(slope_vertical_speed);
                let slope_angle = if horizontal_speed > 1e-6 {
                    slope_vertical_speed.abs().atan2(horizontal_speed)
                } else {
                    0.0
                };
                let slope_speed_factor = slope_angle.cos();
                let walk_scale = if tangent_speed > 1e-6 {
                    (horizontal_speed / tangent_speed) * slope_speed_factor
                } else {
                    1.0
                };
                character.transform.position += move_velocity * dt;
                if rise != 0.0 {
                    character.transform.position.x -= move_velocity.x * dt * (1.0 - walk_scale);
                    character.transform.position.z -= move_velocity.z * dt * (1.0 - walk_scale);
                    character.transform.position.y += rise * walk_scale;
                }

                let static_span = crate::engine::development_capture::operation(
                    "character",
                    "static_cell_collision",
                    true,
                );
                resolve_static_voxel_collisions(
                    character,
                    physics_world,
                    &mut character_contacted_cells,
                    previous_position,
                );

                drop(static_span);
                let dynamic_span = crate::engine::development_capture::operation(
                    "character",
                    "dynamic_cell_collision",
                    true,
                );
                resolve_dynamic_body_collisions(
                    character,
                    physics_world,
                    &mut character_contacted_cells,
                    previous_position,
                    was_grounded,
                );

                drop(dynamic_span);
                if character.movement.is_grounded {
                    character.movement.slope_slide_velocity = glam::Vec3::ZERO;
                }
                let supported_by_cell = character.movement.is_grounded;
                let terrain_span = crate::engine::development_capture::operation(
                    "character",
                    "terrain_movement",
                    true,
                );
                if let Some((position, contacts)) = crate::character::collider::sweep_terrain(
                    physics_world,
                    world,
                    character,
                    previous_position,
                    character.transform.position,
                ) {
                    character.transform.position = position;
                    let descending = character.movement.velocity.y <= 0.0;
                    for contact in contacts {
                        let normal = contact.normal;
                        if normal.y
                            >= world
                                .max_walkable_slope_degrees
                                .clamp(0.0, 89.0)
                                .to_radians()
                                .cos()
                            && descending
                        {
                            character.movement.is_grounded = true;
                            character.movement.velocity.y = 0.0;
                        } else {
                            let into = character.movement.velocity.dot(normal).min(0.0);
                            character.movement.velocity -= normal * into;
                            if descending && character.movement.velocity.y > 0.0 {
                                character.movement.velocity.y = 0.0;
                            }
                            let slide_into =
                                character.movement.slope_slide_velocity.dot(normal).min(0.0);
                            character.movement.slope_slide_velocity -= normal * slide_into;
                        }
                    }
                    if character.movement.is_grounded
                        && !supported_by_cell
                        && physics_world.native_terrain_collider().is_none()
                    {
                        if let Some(ground) = surface_contact(
                            world,
                            character.transform.position.x,
                            character.transform.position.z,
                        ) {
                            if ground.is_walkable_at(world.max_walkable_slope_degrees, was_grounded)
                            {
                                character.transform.position.y =
                                    ground.height - character.navigation_bottom_offset();
                            }
                        }
                    }
                } else {
                    resolve_terrain_movement(character, world, previous_position, was_grounded);
                }

                drop(terrain_span);
                let resolved_water = if character.movement.surface_jump_remaining > 0.0 {
                    None
                } else {
                    world.swimming_surface_with_appearance(
                        character.transform.position.x,
                        character.transform.position.z,
                        crate::world::terrain::ocean_wave_time_seconds(),
                        look,
                    )
                };
                let resolved_ground = surface_contact(
                    world,
                    character.transform.position.x,
                    character.transform.position.z,
                );
                let resolved_can_stand =
                    resolved_water
                        .zip(resolved_ground)
                        .is_some_and(|(water, ground)| {
                            water - ground.height <= character.collision.height * 0.86
                                && (character.transform.position.y - ground.height).abs()
                                    <= character.collision.height * 0.85
                                && ground
                                    .is_walkable_at(world.max_walkable_slope_degrees, was_grounded)
                        });
                let resolved_immersion = resolved_water
                    .map(|water| {
                        ((water - character.transform.position.y)
                            / character.collision.height.max(0.1))
                        .clamp(0.0, 1.0)
                    })
                    .unwrap_or(0.0);
                let resolved_mode = swim_mode_step(
                    character.movement.swim_mode,
                    resolved_water,
                    character.transform.position.y,
                    character.collision.height,
                    resolved_immersion,
                    resolved_can_stand,
                    character.movement.swim_look_y,
                    signed_forward,
                    character.movement.velocity.y,
                );
                character.movement.swim_mode = resolved_mode;
                character.movement.is_swimming = resolved_mode != SwimMode::Dry;
                if resolved_mode == SwimMode::Surface {
                    if let Some(wave_y) = resolved_water {
                        character.transform.position.y = surface_lock_position(
                            character.transform.position.y,
                            wave_y,
                            character.collision.height,
                            dt,
                            &mut character.movement.surface_lock_blend,
                        );
                        character.movement.velocity.y = 0.0;
                        character.movement.is_grounded = false;
                    }
                } else {
                    character.movement.surface_lock_blend = 0.0;
                }

                if !scripted
                    && !character.movement.is_swimming
                    && input.is_some_and(|direction| direction.length_squared() > 0.001)
                {
                    if let Some(ground) =
                        surface_contact(world, previous_position.x, previous_position.z)
                    {
                        let normal = ground.normal;
                        let slope_cos = normal.y.clamp(0.0, 1.0);
                        let horizontal_input = glam::Vec3::new(
                            character.movement.velocity.x,
                            0.0,
                            character.movement.velocity.z,
                        );
                        let uphill =
                            horizontal_input.dot(glam::Vec3::new(-normal.x, 0.0, -normal.z)) > 0.0;
                        let walkable =
                            ground.is_walkable_at(world.max_walkable_slope_degrees, was_grounded);
                        let permitted = if !walkable && uphill {
                            0.0
                        } else {
                            world.character_walk_speed * dt * slope_cos
                        };
                        let travelled = character.transform.position - previous_position;
                        let distance = travelled.length();
                        if distance > permitted && distance > 1e-6 {
                            character.transform.position =
                                previous_position + travelled * (permitted / distance);
                        }
                    }
                }
                if let Some(entity_id) = self.character_to_entity.get(&character_id).copied() {
                    for cell_id in character_contacted_cells {
                        contacts.insert((entity_id, cell_id));
                    }
                }
            }

            drop(movement_span);
            let horizontal_speed =
                Vec2::new(character.movement.velocity.x, character.movement.velocity.z).length();

            let target_animation = if character.movement.is_swimming {
                if horizontal_speed > 0.25 {
                    TargetAnimation::Named("Swim".to_owned())
                } else {
                    TargetAnimation::Named("SwimIdle".to_owned())
                }
            } else {
                character.animation_override.clone().unwrap_or_else(|| {
                    if character.package.blend.run_clip.is_some()
                        && horizontal_speed >= character.package.blend.run_threshold
                    {
                        TargetAnimation::Run
                    } else if horizontal_speed > 0.1 {
                        TargetAnimation::Walk
                    } else {
                        TargetAnimation::Idle
                    }
                })
            };

            match &target_animation {
                TargetAnimation::Walk => {
                    character.animation.current_state = AnimationState::Walk;
                    character.movement.state = MovementState::Walk;
                }
                TargetAnimation::Run => {
                    character.animation.current_state = AnimationState::Run;
                    character.movement.state = MovementState::Run;
                }
                TargetAnimation::Idle => {
                    character.animation.current_state = AnimationState::Idle;
                    character.movement.state = MovementState::Idle;
                }
                TargetAnimation::Named(_) => {}
            }

            character
                .animation_controller
                .select_animation(target_animation);

            let animation_clock_span = crate::engine::development_capture::operation(
                "animation",
                "clock_and_events",
                true,
            );
            character.animation_controller.update(dt);

            let entity_id = self.character_to_entity.get(&character.id).copied();
            self.pending_animation_events.extend(
                character.animation_controller.drain_events().map(|event| {
                    CharacterAnimationEvent {
                        character_id: character.id,
                        entity_id,
                        name: event.name,
                        payload: event.payload,
                    }
                }),
            );

            drop(animation_clock_span);
            if evaluate_pose.contains(&character_id) {
                let pose_span = crate::engine::development_capture::operation(
                    "animation",
                    "skeletal_pose_evaluation",
                    true,
                )
                .map(|span| {
                    span.character_identity(
                        character_id,
                        self.character_to_entity.get(&character_id).map(|id| id.0),
                    )
                });
                let started = std::time::Instant::now();
                character.current_pose = character.animation_controller.evaluate_pose();
                let nanos = started.elapsed().as_nanos().min(u64::MAX as u128) as u64;
                character.pose_estimate_ns = character
                    .pose_estimate_ns
                    .saturating_mul(7)
                    .saturating_add(nanos)
                    .saturating_div(8)
                    .max(1);
                character.pose_last_frame = frame;
                drop(pose_span);
            }
        }

        let _separation_span = crate::engine::development_capture::operation(
            "character",
            "character_separation",
            true,
        );
        resolve_character_collisions_with_world(
            &mut self.characters,
            externally_transformed,
            physics_world,
            world,
        );

        contacts
    }
}

#[cfg(test)]
mod wk_script_owned_motion_tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn zero_gravity_script_motion_then_restore_world_gravity() {
        let world = World::new();
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        let mut system = CharacterSystem::new();
        let id = system.spawn_character(Vec3::new(0.0, 100.0, 0.0), Some("Rogue"));
        system.active_player_id = Some(id);
        let character = system.get_active_player_mut().unwrap();
        assert_eq!(character.movement.gravity_scale, 1.0);
        assert!(character.movement.controller_enabled);
        character.movement.gravity_scale = 0.0;
        character.movement.controller_enabled = false;
        character.movement.velocity = Vec3::new(0.0, 2.5, 0.0);
        assert!(!system.is_player_controller_enabled());
        for _ in 0..30 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        }
        let raised = system.get_active_player().unwrap().transform.position.y;
        assert!(
            raised > 101.0,
            "no gravity, movement must gain height: {raised}"
        );
        let c = system.get_active_player_mut().unwrap();
        c.movement.velocity = Vec3::ZERO;
        c.movement.gravity_scale = 1.0;
        c.movement.controller_enabled = true;
        assert!(system.is_player_controller_enabled());
        for _ in 0..20 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        }
        assert!(system.get_active_player().unwrap().transform.position.y < raised);
    }
}

#[cfg(test)]
mod wk_swim_physics_tests {
    use super::*;
    #[test]
    fn wk_swim_scripted_old_controller_jump_works_without_script_modification() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut world = World::new();
        world.terrain.config.water_bodies.push(WaterBody::new(
            1,
            WaterBodyKind::Lake,
            [0.0, 0.0],
            2.0,
        ));
        let mut system = CharacterSystem::new();
        let id = system.spawn_character(glam::Vec3::ZERO, Some("Rogue"));
        system.active_player_id = Some(id);
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        for _ in 0..540 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        }
        assert_eq!(
            system.get_active_player().unwrap().movement.swim_mode,
            SwimMode::Surface
        );
        let y = system.get_active_player().unwrap().transform.position.y;
        assert!(system.try_surface_jump(6.5));
        assert!(!system.try_surface_jump(6.5), "one keypress, one jump");
        for _ in 0..12 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        }
        let player = system.get_active_player().unwrap();
        assert!(
            player.transform.position.y > y + 0.2,
            "swim jump must stay airborne"
        );
        assert!(!player.movement.is_swimming);
    }
    #[test]
    fn wk_swim_surface_jump_is_not_blocked_or_instantly_snapped_back() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut world = World::new();
        world.terrain.config.water_bodies.push(WaterBody::new(
            1,
            WaterBodyKind::Lake,
            [0.0, 0.0],
            2.0,
        ));
        let mut system = CharacterSystem::new();
        let cid = system.spawn_character(glam::Vec3::new(0.0, 0.0, 0.0), Some("Rogue"));
        system.active_player_id = Some(cid);
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        for _ in 0..540 {
            system.update(&world, &mut physics, 1.0 / 60.0, glam::Vec2::ZERO, false);
        }
        assert_eq!(
            system.get_active_player().unwrap().movement.swim_mode,
            SwimMode::Surface
        );
        let old_y = system.get_active_player().unwrap().transform.position.y;
        system.update(&world, &mut physics, 1.0 / 60.0, glam::Vec2::ZERO, true);
        let p = system.get_active_player().unwrap();
        assert!(
            p.movement.velocity.y > 1.0,
            "Water surface must permit a real upward jump impulse"
        );
        assert!(
            p.transform.position.y > old_y,
            "Water surface lock must not cancel a jump"
        );
        for _ in 0..12 {
            system.update(&world, &mut physics, 1.0 / 60.0, glam::Vec2::ZERO, false);
        }
        let p = system.get_active_player().unwrap();
        assert!(
            p.transform.position.y > old_y + 0.2,
            "Character should stay airborne temporarily, not snap to water"
        );
    }
    #[test]
    fn surface_lock_tracks_rising_and_falling_waves_with_zero_or_negative_velocity() {
        let height = 1.8;
        let mut feet = -1.5;
        let mut blend = 0.0;
        for frame in 0..150 {
            let time = frame as f32 / 60.0;
            let moving_wave = 2.0 + 0.45 * (time * 3.8).sin();
            feet = surface_lock_position(feet, moving_wave, height, 1.0 / 60.0, &mut blend);
            if frame > 6 {
                let target = moving_wave - height * SWIM_SURFACE_FEET_OFFSET;
                assert!(
                    (feet - target).abs() < 1.0e-5,
                    "surface lock drift on frame {frame}"
                );
            }
        }
        assert!((blend - 1.0).abs() < 1.0e-6);
    }
    #[test]
    fn wk_swim_surface_remains_level_regardless_of_camera_pitch_or_wasd() {
        let h = 1.8;
        let y = 2.0 - h * SWIM_SURFACE_FEET_OFFSET;
        let surface = Some(2.0);
        let mode = swim_mode_step(
            SwimMode::Underwater,
            surface,
            y,
            h,
            0.78,
            false,
            0.2,
            2.0,
            0.2,
        );
        assert_eq!(mode, SwimMode::Surface);
        for (look, speed) in [
            (-0.9, 4.0),
            (-0.8, -4.0),
            (-0.8, 0.0),
            (0.8, 4.0),
            (0.0, 3.0),
        ] {
            assert_eq!(
                swim_mode_step(mode, surface, y, h, 0.78, false, look, speed, 0.0),
                SwimMode::Surface
            );
        }
        assert_eq!(
            swim_mode_step(mode, surface, y, h, 0.78, true, 0.0, 0.0, 0.0),
            SwimMode::Dry
        );
        assert_eq!(
            swim_mode_step(mode, None, y, h, 0.78, false, 0.0, 0.0, 0.0),
            SwimMode::Dry
        );
    }
    #[test]
    fn gravity_keeps_direction_and_buoyancy_opposes_displaced_water_weight() {
        let gravity = glam::Vec3::new(0.0, -9.81, 0.0);
        assert_eq!(buoyancy_acceleration(gravity, 0.0, 0.93), glam::Vec3::ZERO);
        let float = gravity + buoyancy_acceleration(gravity, 1.0, 0.93);
        let sink = gravity + buoyancy_acceleration(gravity, 1.0, 1.2);
        assert!(float.y > 0.0, "less dense swimmer floats");
        assert!(sink.y < 0.0, "denser body sinks");
        assert!((gravity + buoyancy_acceleration(gravity, 0.93, 0.93)).length() < 0.00001);
        assert!(
            float.y < 1.0,
            "Slow buoyancy must not catapult a swimmer upward"
        );
    }
}
