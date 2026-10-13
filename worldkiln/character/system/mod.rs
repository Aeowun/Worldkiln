pub mod character_state;
pub mod entity_control;
pub mod health;
pub mod navigation_control;
pub mod pathfinding;

#[cfg(test)]
mod system_tests;

use super::animation::AnimationState;
use super::animation::TargetAnimation;
use super::character::Character;
use super::collision::{
    resolve_character_collisions_excluding, resolve_dynamic_body_collisions,
    resolve_static_voxel_collisions, resolve_terrain_collision,
};
use super::movement::{JUMP_IMPULSE, MOVE_SPEED, MovementState};
use crate::engine::entity::EntityId;
use crate::navigation::{NavigationAgent, NavigationGrid};
use crate::world::World;
use glam::{Quat, Vec2};
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

#[derive(Clone, Debug)]
pub(crate) struct CachedNavigationGrid {
    pub(crate) world_revision: u64,
    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
    pub(crate) terrain_revision: u64,
    pub(crate) agent: NavigationAgent,
    pub(crate) grid: NavigationGrid,
}

/// Owns all runtime character state.
///
/// CharacterSystem is authoritative for:
/// - character transform
/// - gravity
/// - movement velocity
/// - grounded state
/// - collision resolution
/// - animation state
/// - character health
///
/// AeoScript may control horizontal velocity, facing, animation selection,
/// jumping, health, damage, path queries, and explicit teleports through
/// the EngineHost bridge.
#[derive(Default)]
pub struct CharacterSystem {
    pub(crate) characters: HashMap<u64, Character>,

    /// Maps EntityId to CharacterId.
    pub(crate) entity_to_character: HashMap<EntityId, u64>,

    /// Maps CharacterId to EntityId.
    pub(crate) character_to_entity: HashMap<u64, EntityId>,

    /// Entities whose transforms are owned by authored kinematic state or an
    /// external runtime system such as AttachmentSystem.
    pub(crate) kinematic_entities: HashSet<EntityId>,

    /// Explicit active player identity.
    ///
    /// HashMap iteration order is not a gameplay contract, so runtime player
    /// access should use this ID whenever possible.
    pub(crate) active_player_id: Option<u64>,

    pub(crate) next_id: u64,

    /// Shared immutable navigation snapshot for script path queries. World
    /// geometry mutations advance render_revision, which invalidates this
    /// cache without coupling World ownership to navigation internals.
    pub(crate) navigation_cache: RefCell<Option<CachedNavigationGrid>>,

    /// Engine-owned path-following state, keyed by Character ID.
    pub(crate) navigation_followers: HashMap<u64, navigation_control::NavigationFollowState>,

    /// Entity completions waiting for the runtime/scripting layer to consume.
    pub(crate) navigation_finished: Vec<EntityId>,

    /// Entity failures waiting for the runtime/scripting layer to consume.
    pub(crate) navigation_failed: Vec<(EntityId, navigation_control::NavigationFailureReason)>,
}

impl CharacterSystem {
    pub fn new() -> Self {
        Self {
            characters: HashMap::new(),
            entity_to_character: HashMap::new(),
            character_to_entity: HashMap::new(),
            kinematic_entities: HashSet::new(),
            active_player_id: None,
            next_id: 1,
            navigation_cache: RefCell::new(None),
            navigation_followers: HashMap::new(),
            navigation_finished: Vec::new(),
            navigation_failed: Vec::new(),
        }
    }

    /// Legacy engine-controlled movement path.
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

    /// AeoScript-controlled movement path.
    ///
    /// Each returned contact preserves both sides of the interaction: `(entity_that_touched, touched_cell)`.
    ///
    /// The scripting layer uses the entity as the `on_touch(c)` argument.
    pub fn update_scripted(
        &mut self,
        world: &World,
        physics_world: &mut crate::engine::physics::PhysicsWorld,
        dt: f32,
    ) -> HashSet<(EntityId, u64)> {
        self.update_scripted_excluding(world, physics_world, dt, &HashSet::new())
    }

    /// AeoScript-controlled update while listed Entities are externally
    /// transformed. Their animation still evaluates, but CharacterSystem
    /// skips movement and collision ownership for them.
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
        let mut contacts = HashSet::new();

        for (&character_id, character) in self.characters.iter_mut() {
            let mut character_contacted_cells = HashSet::new();
            let externally_transformed = externally_transformed.contains(&character_id);

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
                    Some(input) if input.length_squared() > 0.001 => input.normalize() * MOVE_SPEED,

                    _ => Vec2::ZERO,
                };

                character.movement.velocity.x = horizontal_velocity.x;

                character.movement.velocity.z = horizontal_velocity.y;

                if jump_requested && character.movement.is_grounded {
                    character.movement.velocity.y = JUMP_IMPULSE;

                    character.movement.is_grounded = false;
                }

                // Engine-controlled movement owns facing.
                if horizontal_velocity.length_squared() > 0.001 {
                    let facing = horizontal_velocity.normalize();

                    character.facing = facing;

                    let angle = f32::atan2(facing.x, facing.y);

                    character.transform.rotation = Quat::from_rotation_y(angle);
                }
            }

            if !externally_transformed {
                // Gravity is always authoritative for simulated Characters.
                character.movement.velocity += world.gravity * dt;

                // Preserve the previous position so collision resolution can
                // distinguish vertical crossings from horizontal contacts.
                let previous_position = character.transform.position;

                character.transform.position += character.movement.velocity * dt;

                resolve_static_voxel_collisions(
                    character,
                    physics_world,
                    &mut character_contacted_cells,
                    previous_position,
                );

                resolve_dynamic_body_collisions(
                    character,
                    physics_world,
                    &mut character_contacted_cells,
                    previous_position,
                );

                resolve_terrain_collision(character, world, previous_position);

                // Preserve WHO touched the cell.
                //
                // CharacterSystem already knows which runtime EntityId owns
                // this character.
                if let Some(entity_id) = self.character_to_entity.get(&character_id).copied() {
                    for cell_id in character_contacted_cells {
                        contacts.insert((entity_id, cell_id));
                    }
                }
            }

            // Animation follows movement unless explicitly overridden.
            let horizontal_speed =
                Vec2::new(character.movement.velocity.x, character.movement.velocity.z).length();

            let target_animation = character.animation_override.clone().unwrap_or_else(|| {
                if character.package.blend.run_clip.is_some()
                    && horizontal_speed >= character.package.blend.run_threshold
                {
                    TargetAnimation::Run
                } else if horizontal_speed > 0.1 {
                    TargetAnimation::Walk
                } else {
                    TargetAnimation::Idle
                }
            });

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

            character.animation_controller.update(dt);

            character.current_pose = character.animation_controller.evaluate_pose();
        }

        resolve_character_collisions_excluding(&mut self.characters, externally_transformed);

        contacts
    }
}
