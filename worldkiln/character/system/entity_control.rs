use super::CharacterSystem;
use crate::character::animation::{AnimationState, TargetAnimation};
use crate::character::character::Character;
use crate::engine::entity::{EntityId, EntityManager};
use glam::{Mat4, Quat, Vec2, Vec3};

impl CharacterSystem {
    /// Establishes an explicit mapping between a script EntityId and a
    /// runtime Character ID.
    pub fn associate_entity(&mut self, entity_id: EntityId, character_id: u64) {
        self.entity_to_character.insert(entity_id, character_id);

        self.character_to_entity.insert(character_id, entity_id);
    }

    /// Returns the character associated with a specific script EntityId.
    pub fn get_character_for_entity(&self, entity_id: EntityId) -> Option<&Character> {
        let character_id = self.entity_to_character.get(&entity_id)?;

        self.characters.get(character_id)
    }

    /// Resolves a named rig joint to its current world-space transform.
    ///
    /// The returned matrix composes the Character's world transform with the
    /// joint's transform from its latest evaluated pose. This works for any
    /// runtime Entity associated with a CharacterPackage, including Actors
    /// whose package is an item rig such as a sword.
    pub fn resolve_attachment_point(&self, entity_id: EntityId, point_name: &str) -> Option<Mat4> {
        let character = self.get_character_for_entity(entity_id)?;
        let joint_index = character.package.attachment_joint_index(point_name)?;
        let joint_world_in_character = *character.current_pose.matrices.get(joint_index)?;
        // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 17:24 -05:00
        // Imported Blender rigs commonly carry a 100x unit-conversion scale on
        // their root. It is already baked into the evaluated joint translation;
        // inheriting it again as an attachment scale turns hand-held props into
        // camera-filling geometry. Preserve the joint pose, but normalize its
        // rig-internal scale before applying the Character's authored scale.
        let (joint_scale, joint_rotation, joint_translation) =
            joint_world_in_character.to_scale_rotation_translation();
        if !joint_scale.is_finite() || !joint_rotation.is_finite() || !joint_translation.is_finite()
        {
            return None;
        }
        // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 18:02 -05:00
        // Package metadata may define a stable grip-frame correction where an
        // imported hand bone uses different local axes than Worldkiln props.
        let attachment_pose = Mat4::from_rotation_translation(joint_rotation, joint_translation)
            * Mat4::from_quat(character.package.attachment_rotation(point_name));
        let character_world = Mat4::from_scale_rotation_translation(
            character.transform.scale,
            character.transform.rotation,
            character.transform.position,
        );

        Some(character_world * attachment_pose)
    }

    /// Returns the character associated with a specific script EntityId
    /// mutably.
    pub fn get_character_for_entity_mut(&mut self, entity_id: EntityId) -> Option<&mut Character> {
        let character_id = self.entity_to_character.get(&entity_id)?;

        self.characters.get_mut(character_id)
    }

    /// Returns the character ID associated with a specific script EntityId.
    pub fn get_character_id_for_entity(&self, entity_id: EntityId) -> Option<u64> {
        self.entity_to_character.get(&entity_id).copied()
    }

    /// Returns the script EntityId associated with a specific character ID.
    pub fn get_entity_for_character(&self, character_id: u64) -> Option<EntityId> {
        self.character_to_entity.get(&character_id).copied()
    }

    /// Returns every script Entity currently backed by a runtime Character.
    ///
    /// These Entities mirror CharacterSystem transforms for scripting and
    /// contact dispatch; they must not also be integrated by generic Entity
    /// physics or the two systems will independently move the same Actor.
    pub fn associated_entity_ids(&self) -> impl Iterator<Item = EntityId> + '_ {
        self.entity_to_character.keys().copied()
    }

    pub fn set_entity_kinematic(&mut self, entity_id: EntityId, kinematic: bool) {
        if kinematic {
            self.kinematic_entities.insert(entity_id);
        } else {
            self.kinematic_entities.remove(&entity_id);
        }
    }

    /// Sets the complete runtime velocity of a character.
    pub fn set_entity_velocity(&mut self, entity_id: EntityId, velocity: Vec3) -> bool {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.movement.velocity = velocity;
        true
    }

    /// Sets only horizontal velocity.
    pub fn set_entity_horizontal_velocity(&mut self, entity_id: EntityId, x: f32, z: f32) -> bool {
        self.cancel_entity_navigation(entity_id);
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.movement.velocity.x = x;
        character.movement.velocity.z = z;

        true
    }

    /// Returns the current runtime velocity.
    pub fn get_entity_velocity(&self, entity_id: EntityId) -> Option<Vec3> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.movement.velocity)
    }

    /// Sets the character's horizontal facing direction.
    pub fn set_entity_facing_direction(&mut self, entity_id: EntityId, x: f32, z: f32) -> bool {
        let direction = Vec2::new(x, z);

        if direction.length_squared() <= 0.000001 {
            return false;
        }

        let direction = direction.normalize();

        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.facing = direction;

        let angle = f32::atan2(direction.x, direction.y);

        character.transform.rotation = Quat::from_rotation_y(angle);

        true
    }

    /// Returns the current horizontal facing direction.
    pub fn get_entity_facing(&self, entity_id: EntityId) -> Option<Vec2> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.facing)
    }

    /// Explicitly selects an animation.
    ///
    /// `Auto` returns control to movement-driven animation.
    pub fn set_entity_animation(
        &mut self,
        entity_id: EntityId,
        animation: &str,
    ) -> Result<bool, String> {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return Ok(false);
        };

        character.animation_override = match animation {
            "Auto" => None,
            "Idle" => Some(TargetAnimation::Idle),
            "Walk" => Some(TargetAnimation::Walk),
            "Run" => Some(TargetAnimation::Run),
            other => Some(TargetAnimation::Named(other.to_string())),
        };

        Ok(true)
    }

    /// Returns the currently active animation state.
    pub fn get_entity_animation(&self, entity_id: EntityId) -> Option<String> {
        let character = self.get_character_for_entity(entity_id)?;

        if let Some(animation) = &character.animation_override {
            return Some(match animation {
                TargetAnimation::Idle => "Idle".to_string(),
                TargetAnimation::Walk => "Walk".to_string(),
                TargetAnimation::Run => "Run".to_string(),
                TargetAnimation::Named(name) => name.clone(),
            });
        }

        Some(match character.animation.current_state {
            AnimationState::Idle => "Idle".to_string(),
            AnimationState::Walk => "Walk".to_string(),
            AnimationState::Run => "Run".to_string(),
        })
    }

    /// Returns whether the character is currently grounded.
    pub fn is_entity_grounded(&self, entity_id: EntityId) -> Option<bool> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.movement.is_grounded)
    }

    /// Applies a jump impulse only when grounded.
    pub fn jump_entity(&mut self, entity_id: EntityId, impulse: f32) -> bool {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        if !character.movement.is_grounded {
            return false;
        }

        character.movement.velocity.y = impulse;
        character.movement.is_grounded = false;

        true
    }

    /// Applies an explicit vertical velocity impulse to a Character.
    ///
    /// Unlike jump_entity(), this does not require the Character to already
    /// be grounded. This preserves the existing AeoScript Player behavior.
    pub fn apply_entity_vertical_impulse(&mut self, entity_id: EntityId, impulse: f32) -> bool {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.movement.velocity.y = impulse;
        character.movement.is_grounded = false;

        true
    }

    /// Explicitly sets the position of a character associated with a script
    /// EntityId.
    ///
    /// Returns true if an associated character was moved, or false if no
    /// mapping exists.
    pub fn set_entity_position(&mut self, entity_id: EntityId, position: Vec3) -> bool {
        self.cancel_entity_navigation(entity_id);
        if let Some(&character_id) = self.entity_to_character.get(&entity_id) {
            if let Some(character) = self.characters.get_mut(&character_id) {
                character.transform.position = position;

                character.movement.velocity = Vec3::ZERO;

                character.movement.is_grounded = false;

                return true;
            }
        }

        false
    }

    /// Synchronizes all runtime character transforms back to their
    /// corresponding EntityManager entities.
    pub fn sync_entity_positions(&self, entity_manager: &mut EntityManager) {
        for (&entity_id, &character_id) in &self.entity_to_character {
            if let Some(character) = self.characters.get(&character_id) {
                entity_manager.set_position(entity_id, character.transform.position);
            }
        }

        if let Some(player_id) = entity_manager.lookup_entity("Player") {
            if !self.entity_to_character.contains_key(&player_id) {
                if let Some(player) = self.get_active_player() {
                    entity_manager.set_position(player_id, player.transform.position);
                }
            }
        }
    }

    /// Destroys a mapped runtime character.
    ///
    /// EntityManager removal is handled by the scripting host because
    /// CharacterSystem does not own the EntityManager registry.
    pub fn destroy_entity(&mut self, entity_id: EntityId) -> bool {
        let Some(character_id) = self.entity_to_character.remove(&entity_id) else {
            self.kinematic_entities.remove(&entity_id);
            return false;
        };

        self.kinematic_entities.remove(&entity_id);

        self.character_to_entity.remove(&character_id);
        self.navigation_followers.remove(&character_id);
        self.navigation_finished
            .retain(|finished| *finished != entity_id);
        self.navigation_failed
            .retain(|(failed, _)| *failed != entity_id);

        if self.active_player_id == Some(character_id) {
            self.active_player_id = None;
        }

        self.characters.remove(&character_id).is_some()
    }
}
