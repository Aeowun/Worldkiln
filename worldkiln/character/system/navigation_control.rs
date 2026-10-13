use super::CharacterSystem;
use crate::character::animation::TargetAnimation;
use crate::character::character::Character;
use crate::engine::entity::EntityId;
use crate::world::World;
use glam::{Quat, Vec2, Vec3};
use std::collections::HashMap;

const INTERMEDIATE_WAYPOINT_TOLERANCE: f32 = 0.12;
const STUCK_TIMEOUT_SECONDS: f32 = 1.0;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct NavigationFollowState {
    waypoints: Vec<Vec3>,
    next_waypoint: usize,
    speed: f32,
    stop_distance: f32,
    previous_horizontal_velocity: Vec2,
    best_waypoint_distance: f32,
    stalled_seconds: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavigationFailureReason {
    Stuck,
}

impl NavigationFailureReason {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Stuck => "stuck",
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
    /// Starts engine-owned horizontal path following for a Character Entity.
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
        let waypoints = self
            .find_path_for_entity_detailed(entity_id, world, target)
            .map_err(|error| NavigationFollowError::NoPath(error.reason_code()))?;

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
        removed.is_some()
    }

    pub fn is_entity_navigating(&self, entity_id: EntityId) -> bool {
        self.get_character_id_for_entity(entity_id)
            .is_some_and(|character_id| self.navigation_followers.contains_key(&character_id))
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
        let tolerance = if is_final {
            state.stop_distance.max(INTERMEDIATE_WAYPOINT_TOLERANCE)
        } else {
            INTERMEDIATE_WAYPOINT_TOLERANCE
        };
        if distance <= tolerance {
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
    use crate::engine::physics::PhysicsWorld;
    use crate::world::{CellType, WorldCoord};

    fn flat_world() -> World {
        let mut world = World::new();
        for x in 0..=8 {
            for z in 0..=2 {
                world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
            }
        }
        world
    }

    fn obstacle_world() -> World {
        let mut world = World::new();
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

        // The route was valid when requested. Seal the corridor afterward to
        // model a world change or dynamic obstruction without live replanning.
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
