// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::CharacterSystem;
use crate::character::animation::TargetAnimation;
use crate::character::character::Character;
use crate::engine::entity::EntityId;
use crate::world::World;
use glam::{Quat, Vec2, Vec3};
use std::collections::HashMap;

const INTERMEDIATE_WAYPOINT_TOLERANCE: f32 = 0.12;
const STUCK_TIMEOUT_SECONDS: f32 = 1.0;

#[derive(Clone, Debug)]
pub(crate) struct PendingNavigationRequest {
    previous_horizontal_velocity: Vec2,
    target: Vec3,
    speed: f32,
    stop_distance: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NavigationFollowState {
    waypoints: Vec<Vec3>,
    next_waypoint: usize,
    speed: f32,
    stop_distance: f32,
    requested_target: Vec3,
    previous_horizontal_velocity: Vec2,
    best_waypoint_distance: f32,
    stalled_seconds: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationFailureReason {
    Stuck,
    NoPath(&'static str),
}

impl NavigationFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stuck => "stuck",
            Self::NoPath(reason) => reason,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum NavigationFollowError {
    NotACharacter,
    InvalidSpeed,
    InvalidStopDistance,
    NoPath(&'static str),
}

impl std::fmt::Display for NavigationFollowError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotACharacter => write!(formatter, "Entity is not backed by a Character"),
            Self::InvalidSpeed => write!(formatter, "navigation speed must be finite and positive"),
            Self::InvalidStopDistance => {
                write!(formatter, "stop distance must be finite and non-negative")
            }
            Self::NoPath(reason) => {
                write!(formatter, "no navigation path reaches the target: {reason}")
            }
        }
    }
}

impl std::error::Error for NavigationFollowError {}

impl CharacterSystem {
    pub fn begin_entity_navigation(
        &mut self,
        entity_id: EntityId,
        world: &World,
        target: Vec3,
        speed: f32,
        stop_distance: f32,
    ) -> Result<(), NavigationFollowError> {
        if !speed.is_finite() || speed <= 0.0 {
            return Err(NavigationFollowError::InvalidSpeed);
        }
        if !stop_distance.is_finite() || stop_distance < 0.0 {
            return Err(NavigationFollowError::InvalidStopDistance);
        }
        let character_id = self
            .get_character_id_for_entity(entity_id)
            .ok_or(NavigationFollowError::NotACharacter)?;
        let pending_velocity = self
            .navigation_pending
            .get(&entity_id)
            .map(|r| r.previous_horizontal_velocity);
        let result =
            crate::engine::development_test_run::timed_navigation("navigate_to_total", || {
                self.find_path_for_entity_with_stop_range(entity_id, world, target, stop_distance)
            });
        let waypoints = match result {
            Ok(waypoints) => {
                self.navigation_pending.remove(&entity_id);
                waypoints
            }
            Err(super::pathfinding::NavigationPathQueryError::Pending) => {
                let previous_horizontal_velocity = self
                    .navigation_pending
                    .get(&entity_id)
                    .map(|r| r.previous_horizontal_velocity)
                    .or_else(|| {
                        self.navigation_followers
                            .get(&character_id)
                            .map(|r| r.previous_horizontal_velocity)
                    })
                    .unwrap_or_else(|| {
                        let c = &self.characters[&character_id];
                        Vec2::new(c.movement.velocity.x, c.movement.velocity.z)
                    });
                self.cancel_entity_navigation(entity_id);
                if let Some(character) = self.characters.get_mut(&character_id) {
                    character.movement.velocity.x = 0.0;
                    character.movement.velocity.z = 0.0;
                    character.animation_override = None;
                }
                self.navigation_pending.insert(
                    entity_id,
                    PendingNavigationRequest {
                        previous_horizontal_velocity,
                        target,
                        speed,
                        stop_distance,
                    },
                );
                return Ok(());
            }
            Err(error) => return Err(NavigationFollowError::NoPath(error.reason_code())),
        };

        self.navigation_finished
            .retain(|finished| *finished != entity_id);
        self.navigation_failed
            .retain(|(failed, _)| *failed != entity_id);
        if waypoints.is_empty() {
            self.navigation_followers.remove(&character_id);
            if let Some(character) = self.characters.get_mut(&character_id) {
                character.movement.velocity.x = 0.0;
                character.movement.velocity.z = 0.0;
                character.animation_override = None;
            }
            self.navigation_finished.push(entity_id);
            return Ok(());
        }

        if let Some(character) = self.characters.get_mut(&character_id) {
            character.animation_override = None;
        }
        let previous_horizontal_velocity = self
            .navigation_followers
            .get(&character_id)
            .map(|state| state.previous_horizontal_velocity)
            .or(pending_velocity)
            .or_else(|| {
                self.characters.get(&character_id).map(|character| {
                    Vec2::new(character.movement.velocity.x, character.movement.velocity.z)
                })
            })
            .unwrap_or(Vec2::ZERO);
        self.navigation_followers.insert(
            character_id,
            NavigationFollowState {
                waypoints,
                next_waypoint: 0,
                speed,
                stop_distance,
                requested_target: target,
                previous_horizontal_velocity,
                best_waypoint_distance: f32::INFINITY,
                stalled_seconds: 0.0,
            },
        );
        Ok(())
    }

    pub fn cancel_entity_navigation(&mut self, entity_id: EntityId) -> bool {
        let Some(character_id) = self.get_character_id_for_entity(entity_id) else {
            return false;
        };
        let pending = self.navigation_pending.remove(&entity_id);
        if let Some(request) = &pending
            && let Some(character) = self.characters.get_mut(&character_id)
        {
            character.movement.velocity.x = request.previous_horizontal_velocity.x;
            character.movement.velocity.z = request.previous_horizontal_velocity.y;
            character.animation_override = None;
        }
        let removed = self.navigation_followers.remove(&character_id);
        if let Some(state) = &removed
            && let Some(character) = self.characters.get_mut(&character_id)
        {
            character.movement.velocity.x = state.previous_horizontal_velocity.x;
            character.movement.velocity.z = state.previous_horizontal_velocity.y;
            character.animation_override = None;
        }
        self.navigation_finished
            .retain(|finished| *finished != entity_id);
        self.navigation_failed
            .retain(|(failed, _)| *failed != entity_id);
        removed.is_some() || pending.is_some()
    }

    pub fn is_entity_navigating(&self, entity_id: EntityId) -> bool {
        self.navigation_pending.contains_key(&entity_id)
            || self
                .get_character_id_for_entity(entity_id)
                .is_some_and(|character_id| self.navigation_followers.contains_key(&character_id))
    }

    pub(crate) fn has_pending_navigation_work(&self) -> bool {
        !self.navigation_pending.is_empty() || self.navigation_builds.borrow().has_work()
    }

    pub(crate) fn tick_navigation_builds(&mut self, world: &World) {
        {
            let mut cache = self.navigation_cache.borrow_mut();
            let policy = super::pathfinding::navigation_policy(world);
            if cache.as_ref().is_some_and(|c| c.policy != policy) {
                *cache = Some(super::CachedNavigationGrid {
                    policy,
                    world_revision: world.render_revision(),
                    terrain_revision: world.terrain.render_revision(),
                    profiles: Default::default(),
                });
            }
            if let Some(cache) = cache.as_mut() {
                self.navigation_builds.borrow_mut().tick(world, cache);
            }
        }
        let pending = std::mem::take(&mut self.navigation_pending);
        for (entity_id, request) in pending {
            if self.get_character_id_for_entity(entity_id).is_none() {
                continue;
            }
            if let Err(error) = self.begin_entity_navigation(
                entity_id,
                world,
                request.target,
                request.speed,
                request.stop_distance,
            ) {
                let reason = match error {
                    NavigationFollowError::NoPath(reason) => reason,
                    _ => "navigation_unavailable",
                };
                self.navigation_failed
                    .push((entity_id, NavigationFailureReason::NoPath(reason)));
            }
            if let Some(waiting) = self.navigation_pending.get_mut(&entity_id) {
                waiting.previous_horizontal_velocity = request.previous_horizontal_velocity;
            }
            if let Some(id) = self.get_character_id_for_entity(entity_id)
                && let Some(state) = self.navigation_followers.get_mut(&id)
            {
                state.previous_horizontal_velocity = request.previous_horizontal_velocity;
            }
        }
    }

    pub fn drain_navigation_finished(&mut self) -> Vec<EntityId> {
        std::mem::take(&mut self.navigation_finished)
    }

    pub fn drain_navigation_failed(&mut self) -> Vec<(EntityId, NavigationFailureReason)> {
        std::mem::take(&mut self.navigation_failed)
    }
}

pub(super) fn apply_navigation_intent(
    character_id: u64,
    character: &mut Character,
    dt: f32,
    character_to_entity: &HashMap<u64, EntityId>,
    followers: &mut HashMap<u64, NavigationFollowState>,
    finished: &mut Vec<EntityId>,
    failed: &mut Vec<(EntityId, NavigationFailureReason)>,
) {
    let Some(state) = followers.get_mut(&character_id) else {
        return;
    };

    loop {
        let Some(target) = state.waypoints.get(state.next_waypoint).copied() else {
            character.movement.velocity.x = 0.0;
            character.movement.velocity.z = 0.0;
            character.animation_override = None;
            followers.remove(&character_id);
            if let Some(entity_id) = character_to_entity.get(&character_id).copied() {
                finished.push(entity_id);
            }
            return;
        };

        let offset = Vec2::new(
            target.x - character.transform.position.x,
            target.z - character.transform.position.z,
        );
        let distance = offset.length();
        let is_final = state.next_waypoint + 1 == state.waypoints.len();
        let reached = if is_final {
            Vec2::new(
                state.requested_target.x - character.transform.position.x,
                state.requested_target.z - character.transform.position.z,
            )
            .length()
                <= state.stop_distance.max(INTERMEDIATE_WAYPOINT_TOLERANCE)
        } else {
            distance <= INTERMEDIATE_WAYPOINT_TOLERANCE
        };
        if reached {
            state.next_waypoint += 1;
            state.best_waypoint_distance = f32::INFINITY;
            state.stalled_seconds = 0.0;
            continue;
        }

        let progress_epsilon =
            (state.speed * dt.max(0.0) * 0.25).max(f32::EPSILON * distance.max(1.0) * 4.0);
        if distance + progress_epsilon < state.best_waypoint_distance {
            state.best_waypoint_distance = distance;
            state.stalled_seconds = 0.0;
        } else {
            state.stalled_seconds += dt.max(0.0);
            if state.stalled_seconds >= STUCK_TIMEOUT_SECONDS {
                character.movement.velocity.x = 0.0;
                character.movement.velocity.z = 0.0;
                character.animation_override = None;
                followers.remove(&character_id);
                if let Some(entity_id) = character_to_entity.get(&character_id).copied() {
                    failed.push((entity_id, NavigationFailureReason::Stuck));
                }
                return;
            }
        }

        let direction = offset / distance;
        let frame_limited_speed = if dt > 0.0 {
            (distance / dt).min(state.speed)
        } else {
            0.0
        };
        character.movement.velocity.x = direction.x * frame_limited_speed;
        character.movement.velocity.z = direction.y * frame_limited_speed;
        character.facing = direction;
        character.transform.rotation = Quat::from_rotation_y(f32::atan2(direction.x, direction.y));
        character.animation_override = Some(TargetAnimation::Walk);
        return;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::entity::EntityManager;
    use crate::engine::physics::PhysicsWorld;
    use crate::world::{CellType, WorldCoord};

    fn finish_navigation_build(system: &mut CharacterSystem, world: &World) {
        for _ in 0..4096 {
            system.tick_navigation_builds(world);
            if !system.has_pending_navigation_work() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        panic!("navigation build did not complete");
    }

    fn flat_world() -> World {
        let mut world = World::new();
        world.navmesh_settings.global_navigation = true;
        for x in 0..=8 {
            for z in 0..=2 {
                world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
            }
        }
        world
    }

    fn obstacle_world() -> World {
        let mut world = World::new();
        world.navmesh_settings.global_navigation = true;
        for x in 0..=8 {
            for z in 0..=6 {
                world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
            }
        }
        for z in 0..=4 {
            world.set_cell(WorldCoord::new(4, 1, z), CellType::Block);
            world.set_cell(WorldCoord::new(4, 2, z), CellType::Block);
        }
        world
    }

    #[test]
    fn engine_owned_navigation_reaches_target_and_reports_completion() {
        let world = flat_world();
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        let mut system = CharacterSystem::new();
        let entity_id = EntityId(41);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 1.5), None);
        system.associate_entity(entity_id, character_id);

        system
            .begin_entity_navigation(entity_id, &world, Vec3::new(7.5, 1.0, 1.5), 3.0, 0.05)
            .unwrap();
        assert!(system.is_entity_navigating(entity_id));
        finish_navigation_build(&mut system, &world);

        for _ in 0..240 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
            if !system.is_entity_navigating(entity_id) {
                break;
            }
        }

        let character = system.get_character_for_entity(entity_id).unwrap();
        assert!((character.transform.position.x - 7.5).abs() < 0.13);
        assert!((character.transform.position.z - 1.5).abs() < 0.13);
        assert_eq!(system.drain_navigation_finished(), vec![entity_id]);
        assert!(system.drain_navigation_finished().is_empty());
    }

    #[test]
    fn direct_velocity_control_cancels_engine_navigation() {
        let world = flat_world();
        let mut system = CharacterSystem::new();
        let entity_id = EntityId(42);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 1.5), None);
        system.associate_entity(entity_id, character_id);
        system
            .begin_entity_navigation(entity_id, &world, Vec3::new(7.5, 1.0, 1.5), 3.0, 0.05)
            .unwrap();

        assert!(system.set_entity_horizontal_velocity(entity_id, -1.0, 0.0));
        assert!(!system.is_entity_navigating(entity_id));
        assert_eq!(system.get_entity_velocity(entity_id).unwrap().x, -1.0);
    }

    #[test]
    fn runtime_navigation_keeps_collision_snapshot_between_script_and_movement_ticks() {
        let mut world = flat_world();
        let prop_position = Vec3::new(4.5, 1.5, 5.5);
        let authored_prop = world.add_actor("static_prop", prop_position);
        let actor = world.actors.get_mut(&authored_prop).unwrap();
        actor.physics.enabled = true;
        actor.physics.anchored = true;
        actor.physics.collision_size = Vec3::ONE;

        let mut entities = EntityManager::new();
        let runtime_prop = entities.create_entity("static_prop");
        let prop = entities.get_entity_mut(runtime_prop).unwrap();
        prop.position = prop_position;
        prop.physics.enabled = true;
        prop.physics.anchored = true;
        prop.physics.size = Vec3::ONE;
        assert_ne!(authored_prop, runtime_prop.0);

        let entity_id = entities.create_entity("walker");
        let mut system = CharacterSystem::new();
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 1.5), None);
        system.associate_entity(entity_id, character_id);
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        {
            let _geometry =
                crate::navigation::runtime_collision_context(&world, &entities, &system);
            system
                .begin_entity_navigation(entity_id, &world, Vec3::new(7.5, 1.0, 1.5), 3.0, 0.05)
                .unwrap();
        }
        for _ in 0..1200 {
            {
                let _geometry =
                    crate::navigation::runtime_collision_context(&world, &entities, &system);
                system.tick_navigation_builds(&world);
            }
            system.update_scripted_runtime(
                &world,
                &entities,
                &mut physics,
                1.0 / 60.0,
                &Default::default(),
            );
            system.sync_entity_positions(&mut entities);
            if !system.is_entity_navigating(entity_id) {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
        let position = system
            .get_character_for_entity(entity_id)
            .unwrap()
            .transform
            .position;
        assert!(
            (position.x - 7.5).abs() < 0.15,
            "navigation did not reach its target: {position:?}"
        );
        assert_eq!(system.drain_navigation_finished(), vec![entity_id]);
        assert!(system.drain_navigation_failed().is_empty());
        assert!(world.navigation_runtime_geometry.lock().unwrap().is_none());
    }

    #[test]
    fn explicit_cancellation_restores_pre_navigation_horizontal_velocity() {
        let world = flat_world();
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        let mut system = CharacterSystem::new();
        let entity_id = EntityId(45);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 1.5), None);
        system.associate_entity(entity_id, character_id);
        assert!(system.set_entity_horizontal_velocity(entity_id, -1.0, 0.5));
        system
            .begin_entity_navigation(entity_id, &world, Vec3::new(7.5, 1.0, 1.5), 3.0, 0.05)
            .unwrap();
        system.update_scripted(&world, &mut physics, 1.0 / 60.0);

        assert!(system.cancel_entity_navigation(entity_id));
        let velocity = system.get_entity_velocity(entity_id).unwrap();
        assert_eq!(Vec2::new(velocity.x, velocity.z), Vec2::new(-1.0, 0.5));
    }

    #[test]
    fn engine_owned_navigation_physically_follows_route_around_wall() {
        let world = obstacle_world();
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        let mut system = CharacterSystem::new();
        let entity_id = EntityId(43);
        let character_id = system.spawn_character(Vec3::new(1.5, 1.0, 2.5), None);
        system.associate_entity(entity_id, character_id);

        system
            .begin_entity_navigation(entity_id, &world, Vec3::new(7.5, 1.0, 2.5), 3.0, 0.05)
            .unwrap();

        finish_navigation_build(&mut system, &world);
        let mut max_z = 2.5_f32;
        for _ in 0..600 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
            let position = system
                .get_character_for_entity(entity_id)
                .unwrap()
                .transform
                .position;
            max_z = max_z.max(position.z);
            if !system.is_entity_navigating(entity_id) {
                break;
            }
        }

        let character = system.get_character_for_entity(entity_id).unwrap();
        assert!(max_z > 5.25, "route must pass through the wall opening");
        assert!((character.transform.position.x - 7.5).abs() < 0.13);
        assert!((character.transform.position.z - 2.5).abs() < 0.13);
        assert_eq!(system.drain_navigation_finished(), vec![entity_id]);
    }

    #[test]
    fn navigation_stops_and_reports_when_route_becomes_blocked() {
        let mut world = flat_world();
        let mut system = CharacterSystem::new();
        let entity_id = EntityId(44);
        let character_id = system.spawn_character(Vec3::new(0.5, 1.0, 1.5), None);
        system.associate_entity(entity_id, character_id);
        system
            .begin_entity_navigation(entity_id, &world, Vec3::new(7.5, 1.0, 1.5), 3.0, 0.05)
            .unwrap();

        finish_navigation_build(&mut system, &world);
        for z in 0..=2 {
            world.set_cell(WorldCoord::new(4, 1, z), CellType::Block);
            world.set_cell(WorldCoord::new(4, 2, z), CellType::Block);
        }
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);

        for _ in 0..240 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
            if !system.is_entity_navigating(entity_id) {
                break;
            }
        }

        assert!(!system.is_entity_navigating(entity_id));
        assert!(system.drain_navigation_finished().is_empty());
        assert_eq!(
            system.drain_navigation_failed(),
            vec![(entity_id, NavigationFailureReason::Stuck)]
        );
        let character = system.get_character_for_entity(entity_id).unwrap();
        assert_eq!(character.movement.velocity.x, 0.0);
        assert_eq!(character.movement.velocity.z, 0.0);
        assert_eq!(character.animation_override, None);
    }
}
