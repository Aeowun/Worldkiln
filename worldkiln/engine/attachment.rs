use crate::character::{CharacterMovement, CharacterSystem};
use crate::engine::entity::{EntityId, EntityManager, EntityPhysicsState};
use glam::{EulerRot, Mat4, Vec3};

#[derive(Clone, Debug)]
pub struct Attachment {
    pub child: EntityId,
    pub child_point: String,
    pub parent: EntityId,
    pub parent_point: String,
    child_point_local: Mat4,
    saved_physics: EntityPhysicsState,
    saved_character_movement: Option<CharacterMovement>,
}

/// Owns transient mount relationships for one runtime session.
#[derive(Clone, Debug, Default)]
pub struct AttachmentSystem {
    attachments: Vec<Attachment>,
}

impl AttachmentSystem {
    pub fn attach(
        &mut self,
        child: EntityId,
        child_point: &str,
        parent: EntityId,
        parent_point: &str,
        entities: &mut EntityManager,
        characters: &mut CharacterSystem,
    ) -> Result<(), String> {
        if child == parent {
            return Err("an Entity cannot attach to itself".to_string());
        }
        if !entities.validate_handle(child.0) || !entities.validate_handle(parent.0) {
            return Err("attachment requires two live runtime Entities".to_string());
        }
        if self.would_create_cycle(child, parent) {
            return Err("attachment would create a parent cycle".to_string());
        }

        if let Some(existing) = self.attachments.iter().find(|item| item.child == child) {
            if existing.parent == parent
                && existing.child_point.eq_ignore_ascii_case(child_point)
                && existing.parent_point.eq_ignore_ascii_case(parent_point)
            {
                return Ok(());
            }
        }

        let child_point_world = characters
            .resolve_attachment_point(child, child_point)
            .ok_or_else(|| {
                format!(
                    "attachment point '{}' was not found on child Entity {}",
                    child_point, child.0
                )
            })?;
        characters
            .resolve_attachment_point(parent, parent_point)
            .ok_or_else(|| {
                format!(
                    "attachment point '{}' was not found on parent Entity {}",
                    parent_point, parent.0
                )
            })?;

        let child_root = character_world_transform(characters, child)
            .ok_or_else(|| format!("child Entity {} has no rig transform", child.0))?;
        if child_root.determinant().abs() < 1.0e-8 {
            return Err(format!(
                "child Entity {} has a singular world transform",
                child.0
            ));
        }
        let child_point_local = child_root.inverse() * child_point_world;
        if child_point_local.determinant().abs() < 1.0e-8 {
            return Err(format!(
                "attachment point '{}' on child Entity {} has a singular transform",
                child_point, child.0
            ));
        }

        // Replacing an existing mount first restores its saved runtime state.
        self.detach(child, entities, characters);

        let saved_physics = entities
            .get_physics(child)
            .cloned()
            .ok_or_else(|| format!("child Entity {} has no physics state", child.0))?;
        let saved_character_movement = characters
            .get_character_for_entity(child)
            .map(|character| character.movement.clone());

        if let Some(physics) = entities.get_physics_mut(child) {
            physics.enabled = false;
            physics.solid = false;
            physics.anchored = true;
            physics.collision_enabled = false;
            physics.stop();
            physics.grounded = false;
        }
        if let Some(character) = characters.get_character_for_entity_mut(child) {
            character.movement.velocity = Vec3::ZERO;
            character.movement.is_grounded = false;
        }

        self.attachments.push(Attachment {
            child,
            child_point: child_point.to_string(),
            parent,
            parent_point: parent_point.to_string(),
            child_point_local,
            saved_physics,
            saved_character_movement,
        });

        Ok(())
    }

    pub fn detach(
        &mut self,
        child: EntityId,
        entities: &mut EntityManager,
        characters: &mut CharacterSystem,
    ) -> bool {
        let Some(index) = self.attachments.iter().position(|item| item.child == child) else {
            return false;
        };
        let attachment = self.attachments.remove(index);

        if let Some(physics) = entities.get_physics_mut(child) {
            *physics = attachment.saved_physics;
        }
        if let (Some(saved), Some(character)) = (
            attachment.saved_character_movement,
            characters.get_character_for_entity_mut(child),
        ) {
            character.movement = saved;
        }

        true
    }

    /// Applies mounted transforms after CharacterSystem has evaluated its poses.
    pub fn update(&mut self, entities: &mut EntityManager, characters: &mut CharacterSystem) {
        let mut invalid_children = Vec::new();

        for attachment in &self.attachments {
            let Some(parent_point_world) =
                characters.resolve_attachment_point(attachment.parent, &attachment.parent_point)
            else {
                invalid_children.push(attachment.child);
                continue;
            };
            if characters
                .get_character_for_entity(attachment.child)
                .is_none()
            {
                invalid_children.push(attachment.child);
                continue;
            }

            let child_world = parent_point_world * attachment.child_point_local.inverse();
            let (scale, rotation, translation) = child_world.to_scale_rotation_translation();
            if !scale.is_finite() || !rotation.is_finite() || !translation.is_finite() {
                invalid_children.push(attachment.child);
                continue;
            }

            if let Some(character) = characters.get_character_for_entity_mut(attachment.child) {
                character.transform.scale = scale;
                character.transform.rotation = rotation;
                character.transform.position = translation;
            }

            entities.set_position(attachment.child, translation);
            let (rotation_x, rotation_y, rotation_z) = rotation.to_euler(EulerRot::XYZ);
            entities.set_rotation(
                attachment.child,
                Vec3::new(rotation_x, rotation_y, rotation_z),
            );
            entities.set_scale(attachment.child, scale);
        }

        invalid_children.sort_unstable();
        invalid_children.dedup();
        for child in invalid_children {
            self.detach(child, entities, characters);
        }
    }

    pub fn attached_children(&self) -> Vec<EntityId> {
        self.attachments.iter().map(|item| item.child).collect()
    }

    pub fn remove_entity(
        &mut self,
        entity: EntityId,
        entities: &mut EntityManager,
        characters: &mut CharacterSystem,
    ) {
        let affected: Vec<_> = self
            .attachments
            .iter()
            .filter(|item| item.child == entity || item.parent == entity)
            .map(|item| item.child)
            .collect();
        for child in affected {
            self.detach(child, entities, characters);
        }
    }

    pub fn clear(&mut self, entities: &mut EntityManager, characters: &mut CharacterSystem) {
        let children = self.attached_children();
        for child in children {
            self.detach(child, entities, characters);
        }
    }

    fn would_create_cycle(&self, child: EntityId, parent: EntityId) -> bool {
        let mut current = parent;
        loop {
            if current == child {
                return true;
            }
            let Some(existing) = self.attachments.iter().find(|item| item.child == current) else {
                return false;
            };
            current = existing.parent;
        }
    }
}

fn character_world_transform(characters: &CharacterSystem, entity: EntityId) -> Option<Mat4> {
    let character = characters.get_character_for_entity(entity)?;
    Some(Mat4::from_scale_rotation_translation(
        character.transform.scale,
        character.transform.rotation,
        character.transform.position,
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    #[test]
    fn mount_solves_named_points_and_restores_child_physics_on_detach() {
        let mut entities = EntityManager::new();
        let child = entities.create_entity("Sword");
        let parent = entities.create_entity("Player");
        let child_physics_before = entities.get_physics(child).unwrap().clone();
        let mut characters = CharacterSystem::new();
        let child_character = characters.spawn_character(Vec3::new(4.0, 1.0, 0.0), None);
        let parent_character = characters.spawn_character(Vec3::new(0.0, 2.0, 0.0), None);
        characters.associate_entity(child, child_character);
        characters.associate_entity(parent, parent_character);
        let mount = characters
            .get_character_for_entity(parent)
            .unwrap()
            .package
            .skeleton
            .joints[0]
            .name
            .clone();
        let mut attachments = AttachmentSystem::default();

        attachments
            .attach(
                child,
                &mount,
                parent,
                &mount,
                &mut entities,
                &mut characters,
            )
            .unwrap();
        assert!(!entities.get_physics(child).unwrap().enabled);
        assert!(!entities.get_physics(child).unwrap().collision_enabled);

        attachments.update(&mut entities, &mut characters);
        let parent_point = characters.resolve_attachment_point(parent, &mount).unwrap();
        let child_point = characters.resolve_attachment_point(child, &mount).unwrap();
        for (a, b) in parent_point
            .to_cols_array()
            .iter()
            .zip(child_point.to_cols_array())
        {
            assert!((a - b).abs() < 1.0e-4);
        }

        assert!(attachments.detach(child, &mut entities, &mut characters));
        assert_eq!(entities.get_physics(child).unwrap(), &child_physics_before);
    }

    #[test]
    fn sword_grip_follows_rogue_right_hand_during_walk_and_attack_layer() {
        let mut entities = EntityManager::new();
        let sword = entities.create_entity("Sword");
        let player = entities.create_entity("Player");
        let mut characters = CharacterSystem::new();
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:33 -05:00
        // Attachment transform tests should not depend on optional sword art.
        // Use the verified Rogue GLB for both test entities, then give the
        // child a test-only "Grip" alias for its actual Fist.R joint.
        let sword_character = characters.spawn_character(Vec3::new(2.0, 0.0, 0.0), Some("Rogue"));
        {
            let sword = characters.characters.get_mut(&sword_character).unwrap();
            let mut fixture = (*sword.package).clone();
            fixture
                .metadata
                .attachment_points
                .insert("Grip".into(), "Fist.R".into());
            sword.package = std::sync::Arc::new(fixture);
        }
        let player_character = characters.spawn_character(Vec3::ZERO, Some("Rogue"));
        characters.associate_entity(sword, sword_character);
        characters.associate_entity(player, player_character);

        let sword_physics_before = {
            let physics = entities.get_physics_mut(sword).unwrap();
            physics.enabled = true;
            physics.solid = true;
            physics.anchored = false;
            physics.collision_enabled = true;
            physics.clone()
        };

        {
            let player_character = characters.get_character_for_entity_mut(player).unwrap();
            player_character
                .animation_controller
                .select_clip_name("walk");
            player_character
                .animation_controller
                .play_layer("upper_body", "Dagger_Attack", "Torso")
                .unwrap();
            player_character.animation_controller.update(0.25);
            player_character.current_pose = player_character.animation_controller.evaluate_pose();
        }

        let mut attachments = AttachmentSystem::default();
        attachments
            .attach(
                sword,
                "Grip",
                player,
                "RightHand",
                &mut entities,
                &mut characters,
            )
            .unwrap();
        attachments.update(&mut entities, &mut characters);

        let first_sword_position = entities.get_position(sword).unwrap();
        let parent_point = characters
            .resolve_attachment_point(player, "RightHand")
            .unwrap();
        let child_point = characters.resolve_attachment_point(sword, "Grip").unwrap();
        assert_transforms_near(parent_point, child_point);

        {
            let player_character = characters.get_character_for_entity_mut(player).unwrap();
            player_character.transform.position = Vec3::new(1.0, 0.0, 0.5);
            player_character.animation_controller.update(0.1);
            player_character.current_pose = player_character.animation_controller.evaluate_pose();
        }
        attachments.update(&mut entities, &mut characters);

        let moved_sword_position = entities.get_position(sword).unwrap();
        assert_ne!(first_sword_position, moved_sword_position);
        let parent_point = characters
            .resolve_attachment_point(player, "RightHand")
            .unwrap();
        let child_point = characters.resolve_attachment_point(sword, "Grip").unwrap();
        assert_transforms_near(parent_point, child_point);

        assert!(attachments.detach(sword, &mut entities, &mut characters));
        assert_eq!(entities.get_physics(sword), Some(&sword_physics_before));
    }

    fn assert_transforms_near(left: Mat4, right: Mat4) {
        for (a, b) in left.to_cols_array().iter().zip(right.to_cols_array()) {
            assert!((a - b).abs() < 1.0e-4, "{a} != {b}");
        }
    }

    #[test]
    fn attachment_rejects_cycles_without_mutating_existing_relationships() {
        let mut entities = EntityManager::new();
        let first = entities.create_entity("First");
        let second = entities.create_entity("Second");
        let mut characters = CharacterSystem::new();
        let first_character = characters.spawn_character(Vec3::ZERO, None);
        let second_character = characters.spawn_character(Vec3::ZERO, None);
        characters.associate_entity(first, first_character);
        characters.associate_entity(second, second_character);
        let joint = characters
            .get_character_for_entity(first)
            .unwrap()
            .package
            .skeleton
            .joints[0]
            .name
            .clone();
        let mut attachments = AttachmentSystem::default();
        attachments
            .attach(
                first,
                &joint,
                second,
                &joint,
                &mut entities,
                &mut characters,
            )
            .unwrap();

        assert!(
            attachments
                .attach(
                    second,
                    &joint,
                    first,
                    &joint,
                    &mut entities,
                    &mut characters
                )
                .is_err()
        );
        assert_eq!(attachments.attached_children(), vec![first]);
    }
}
