// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::CharacterSystem;
use crate::character::animation::{AnimationState, TargetAnimation};
use crate::character::character::Character;
use crate::engine::entity::{EntityId, EntityManager};
use glam::{Mat4, Quat, Vec2, Vec3};

impl CharacterSystem {
    pub fn configure_authored_collision(
        &mut self,
        entity_id: EntityId,
        actor: &crate::world::Actor,
    ) {
        let Some(c) = self.get_character_for_entity_mut(entity_id) else {
            return;
        };
        c.transform.scale = actor.scale;
        c.transform.rotation = Quat::from_euler(
            glam::EulerRot::XYZ,
            actor.rotation.x,
            actor.rotation.y,
            actor.rotation.z,
        );
        c.collision.radius =
            c.package.collision.radius * actor.scale.abs().x.max(actor.scale.abs().z);
        c.collision.height = c.package.collision.height * actor.scale.abs().y;
        c.uses_authored_collider = true;
        c.collision_solid = actor.physics.solid;
        c.collision_enabled = actor.physics.enabled
            && actor.physics.collision_enabled
            && actor.physics.collision_mode
                != crate::engine::collision_geometry::CollisionMode::Disabled;
        c.collision_geometry = crate::engine::collision_geometry::character_actor_collider(actor);
        c.collision_enabled &= c.collision_geometry.is_some();
        c.refresh_collision_profile();
    }

    pub(crate) fn sync_entity_collision_flags(
        &mut self,
        id: EntityId,
        p: &crate::engine::entity::EntityPhysicsState,
    ) {
        let Some(c) = self.get_character_for_entity_mut(id) else {
            return;
        };
        c.collision_enabled = p.collision_enabled && (!c.uses_authored_collider || p.enabled);
        c.collision_solid = p.solid;
        let authored = c.uses_authored_collider;
        if authored {
            self.set_entity_kinematic(id, p.anchored || !p.enabled);
        }
    }
    pub(crate) fn set_entity_scale(&mut self, id: EntityId, scale: Vec3) {
        if let Some(c) = self.get_character_for_entity_mut(id) {
            c.transform.scale = scale;
            if !c.uses_authored_collider {
                c.collision.radius = c.package.collision.radius * scale.abs().x.max(scale.abs().z);
                c.collision.height = c.package.collision.height * scale.abs().y;
            }
            c.refresh_collision_profile();
        }
    }

    pub fn associate_entity(&mut self, entity_id: EntityId, character_id: u64) {
        self.entity_to_character.insert(entity_id, character_id);

        self.character_to_entity.insert(character_id, entity_id);
    }

    pub fn get_character_for_entity(&self, entity_id: EntityId) -> Option<&Character> {
        let character_id = self.entity_to_character.get(&entity_id)?;

        self.characters.get(character_id)
    }

    pub fn resolve_attachment_point(&self, entity_id: EntityId, point_name: &str) -> Option<Mat4> {
        let character = self.get_character_for_entity(entity_id)?;
        let joint_index = character.package.attachment_joint_index(point_name)?;
        let joint_world_in_character = *character.current_pose.matrices.get(joint_index)?;
        let (joint_scale, joint_rotation, joint_translation) =
            joint_world_in_character.to_scale_rotation_translation();
        if !joint_scale.is_finite() || !joint_rotation.is_finite() || !joint_translation.is_finite()
        {
            return None;
        }
        let attachment_pose = Mat4::from_rotation_translation(joint_rotation, joint_translation)
            * Mat4::from_quat(character.package.attachment_rotation(point_name));
        let character_world = Mat4::from_scale_rotation_translation(
            character.transform.scale,
            character.transform.rotation,
            character.transform.position,
        );

        Some(character_world * attachment_pose)
    }

    pub fn get_character_for_entity_mut(&mut self, entity_id: EntityId) -> Option<&mut Character> {
        let character_id = self.entity_to_character.get(&entity_id)?;

        self.characters.get_mut(character_id)
    }

    pub fn get_character_id_for_entity(&self, entity_id: EntityId) -> Option<u64> {
        self.entity_to_character.get(&entity_id).copied()
    }

    pub fn get_entity_for_character(&self, character_id: u64) -> Option<EntityId> {
        self.character_to_entity.get(&character_id).copied()
    }

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

    pub fn set_entity_velocity(&mut self, entity_id: EntityId, velocity: Vec3) -> bool {
        self.cancel_entity_navigation(entity_id);
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.movement.velocity = velocity;
        true
    }

    pub fn set_entity_gravity_scale(&mut self, entity_id: EntityId, scale: f32) -> bool {
        if !scale.is_finite() || !(-100.0..=100.0).contains(&scale) {
            return false;
        }
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };
        character.movement.gravity_scale = scale;
        true
    }

    pub fn get_entity_gravity_scale(&self, entity_id: EntityId) -> Option<f32> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.movement.gravity_scale)
    }

    pub fn set_entity_horizontal_velocity(&mut self, entity_id: EntityId, x: f32, z: f32) -> bool {
        self.cancel_entity_navigation(entity_id);
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.movement.velocity.x = x;
        character.movement.velocity.z = z;

        true
    }

    pub fn get_entity_velocity(&self, entity_id: EntityId) -> Option<Vec3> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.movement.velocity)
    }

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

    pub fn get_entity_facing(&self, entity_id: EntityId) -> Option<Vec2> {
        self.get_character_for_entity(entity_id).map(|character| {
            let forward = character.transform.rotation * Vec3::Z;
            let horizontal = Vec2::new(forward.x, forward.z);
            if horizontal.length_squared() > 0.000001 {
                horizontal.normalize()
            } else {
                character.facing
            }
        })
    }

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

    pub fn is_entity_grounded(&self, entity_id: EntityId) -> Option<bool> {
        self.get_character_for_entity(entity_id)
            .map(|character| character.movement.is_grounded)
    }

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

    pub fn apply_entity_vertical_impulse(&mut self, entity_id: EntityId, impulse: f32) -> bool {
        let Some(character) = self.get_character_for_entity_mut(entity_id) else {
            return false;
        };

        character.movement.velocity.y = impulse;
        character.movement.is_grounded = false;

        true
    }

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

    pub fn sync_entity_positions(&self, entity_manager: &mut EntityManager) {
        for (&entity_id, &character_id) in &self.entity_to_character {
            if let Some(character) = self.characters.get(&character_id) {
                entity_manager.set_position(entity_id, character.transform.position);
                if let Some(e) = entity_manager.get_entity_mut(entity_id) {
                    let (x, y, z) = character.transform.rotation.to_euler(glam::EulerRot::XYZ);
                    e.rotation = Vec3::new(x, y, z);
                    e.scale = character.transform.scale;
                    if character.uses_authored_collider {
                        e.physics.collider = character.active_collider();
                        e.physics.collision_enabled =
                            character.collision_enabled && e.physics.collider.is_some();
                        e.physics.solid = character.collision_solid;
                        if let Some((a, b)) = e
                            .physics
                            .collider
                            .as_ref()
                            .and_then(crate::engine::collision_geometry::envelope)
                        {
                            e.physics.size = b - a;
                            e.physics.offset = (a + b) * 0.5;
                        }
                    }
                }
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

    pub fn destroy_entity(&mut self, entity_id: EntityId) -> bool {
        let Some(character_id) = self.entity_to_character.remove(&entity_id) else {
            self.kinematic_entities.remove(&entity_id);
            return false;
        };

        self.kinematic_entities.remove(&entity_id);

        self.character_to_entity.remove(&character_id);
        self.navigation_followers.remove(&character_id);
        self.navigation_pending.remove(&entity_id);
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
