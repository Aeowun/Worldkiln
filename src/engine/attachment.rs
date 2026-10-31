// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
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
    root_mount: bool,
    offset: Vec3,
    saved_physics: EntityPhysicsState,
    saved_character_movement: Option<CharacterMovement>,
}

#[derive(Clone, Debug, Default)]
pub struct AttachmentSystem {
    attachments: Vec<Attachment>,
    spatial: Vec<SpatialAttachment>,
}

#[derive(Clone, Debug)]
struct SpatialAttachment {
    child: u64,
    parent: EntityId,
    joint: Option<String>,
    offset: Vec3,
}

impl AttachmentSystem {
    pub fn attach_spatial(
        &mut self,
        child: u64,
        parent: EntityId,
        joint: Option<String>,
        offset: Vec3,
    ) {
        self.spatial.retain(|item| item.child != child);
        self.spatial.push(SpatialAttachment {
            child,
            parent,
            joint,
            offset,
        });
    }

    pub fn update_spatial(
        &mut self,
        world: &mut crate::world::World,
        entities: &EntityManager,
        characters: &CharacterSystem,
    ) {
        let mut orphaned = Vec::new();
        self.spatial.retain(|item| {
            let position = item
                .joint
                .as_deref()
                .and_then(|joint| characters.resolve_attachment_point(item.parent, joint))
                .map(|matrix| matrix.transform_point3(item.offset))
                .or_else(|| {
                    entities.get_entity(item.parent).map(|entity| {
                        let q = glam::Quat::from_euler(
                            EulerRot::XYZ,
                            entity.rotation.x,
                            entity.rotation.y,
                            entity.rotation.z,
                        );
                        entity.position + q * (item.offset * entity.scale)
                    })
                });
            let valid = position.is_some_and(|position| {
                world
                    .set_runtime_cell_position(item.child, position)
                    .is_ok()
            });
            if !valid {
                orphaned.push(item.child);
            }
            valid
        });
        for child in orphaned {
            if world.runtime_cells.contains_key(&child) {
                world.delete_cell_runtime(child);
            }
        }
    }

    pub fn spatial_children(&self, parent: EntityId) -> Vec<u64> {
        self.spatial
            .iter()
            .filter(|item| item.parent == parent)
            .map(|item| item.child)
            .collect()
    }

    pub fn detach_spatial(&mut self, child: u64) -> bool {
        let before = self.spatial.len();
        self.spatial.retain(|item| item.child != child);
        before != self.spatial.len()
    }

    pub fn set_spatial_offset(&mut self, child: u64, offset: Vec3) -> Result<(), String> {
        if !offset.is_finite() {
            return Err("attachment offset must be finite".into());
        }
        let item = self
            .spatial
            .iter_mut()
            .find(|item| item.child == child)
            .ok_or_else(|| "spatial attachment not found".to_string())?;
        item.offset = offset;
        Ok(())
    }

    pub fn spatial_parent(&self, child: u64) -> Option<EntityId> {
        self.spatial
            .iter()
            .find(|item| item.child == child)
            .map(|item| item.parent)
    }

    pub fn update_authored_sources(
        hierarchy: &crate::project::ProjectHierarchy,
        world: &mut crate::world::World,
        actors: &crate::actor::ActorSystem,
        entities: &EntityManager,
        characters: &CharacterSystem,
    ) {
        let bindings: Vec<_> = hierarchy
            .nodes
            .values()
            .filter_map(|node| {
                let crate::project::ProjectNodeKind::Cell {
                    cell_id,
                    attachment: Some(binding),
                    ..
                } = &node.kind
                else {
                    return None;
                };
                Some((*cell_id, binding.clone()))
            })
            .collect();
        for (cell_id, binding) in bindings {
            let Some(parent_entity) = actors.entity_for_authored_actor(binding.parent_actor_id)
            else {
                continue;
            };
            let offset = Vec3::from_array(binding.local_origin_offset);
            let position = binding
                .joint_target
                .as_deref()
                .and_then(|joint| characters.resolve_attachment_point(parent_entity, joint))
                .map(|transform| transform.transform_point3(offset))
                .or_else(|| {
                    entities.get_entity(parent_entity).map(|entity| {
                        let rotation = glam::Quat::from_euler(
                            EulerRot::XYZ,
                            entity.rotation.x,
                            entity.rotation.y,
                            entity.rotation.z,
                        );
                        entity.position + rotation * (offset * entity.scale)
                    })
                });
            if let Some(position) = position {
                let _ = world.set_attachment_source_position(cell_id, position);
            }
        }
    }
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

        let root_mount = child_point.eq_ignore_ascii_case("Origin");
        if root_mount {
            let child_world = character_world_transform(characters, child)
                .or_else(|| {
                    entities
                        .get_entity(child)
                        .map(|e| Mat4::from_translation(e.position))
                })
                .ok_or_else(|| "missing child transform".to_string())?;
            let parent_world = if parent_point.eq_ignore_ascii_case("Origin") {
                character_world_transform(characters, parent).or_else(|| {
                    entities
                        .get_entity(parent)
                        .map(|e| Mat4::from_translation(e.position))
                })
            } else {
                characters.resolve_attachment_point(parent, parent_point)
            }
            .ok_or_else(|| "missing parent transform".to_string())?;
            if parent_world.determinant().abs() < 1.0e-8 {
                return Err("parent transform is not invertible".to_string());
            }
            let relative = if parent_point.eq_ignore_ascii_case("Origin") {
                parent_world.inverse() * child_world
            } else {
                let rotation = entities
                    .get_entity(child)
                    .map(|e| {
                        glam::Quat::from_euler(
                            EulerRot::XYZ,
                            e.rotation.x,
                            e.rotation.y,
                            e.rotation.z,
                        )
                    })
                    .unwrap_or(glam::Quat::IDENTITY);
                Mat4::from_quat(rotation)
            };
            self.detach(child, entities, characters);
            let saved_physics = entities
                .get_physics(child)
                .cloned()
                .ok_or_else(|| "child has no physics state".to_string())?;
            let saved_character_movement = characters
                .get_character_for_entity(child)
                .map(|c| c.movement.clone());
            if let Some(physics) = entities.get_physics_mut(child) {
                physics.enabled = false;
                physics.solid = false;
                physics.anchored = true;
                physics.collision_enabled = false;
                physics.stop();
            }
            self.attachments.push(Attachment {
                child,
                child_point: child_point.to_owned(),
                parent,
                parent_point: parent_point.to_owned(),
                child_point_local: relative,
                root_mount: true,
                offset: Vec3::ZERO,
                saved_physics,
                saved_character_movement,
            });
            return Ok(());
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
            root_mount: false,
            offset: Vec3::ZERO,
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

    pub fn update(&mut self, entities: &mut EntityManager, characters: &mut CharacterSystem) {
        let mut invalid_children = Vec::new();

        for attachment in &self.attachments {
            let _attachment_span = crate::engine::development_capture::operation(
                "attachment",
                "mounted_transform",
                true,
            )
            .map(|span| {
                span.entity(attachment.child.0)
                    .diagnostic(serde_json::json!({
                        "child_entity": attachment.child.0, "parent_entity": attachment.parent.0,
                    }))
            });
            if attachment.root_mount {
                let parent_world = if attachment.parent_point.eq_ignore_ascii_case("Origin") {
                    character_world_transform(characters, attachment.parent).or_else(|| {
                        entities
                            .get_entity(attachment.parent)
                            .map(|e| Mat4::from_translation(e.position))
                    })
                } else {
                    characters.resolve_attachment_point(attachment.parent, &attachment.parent_point)
                };
                let Some(parent_world) = parent_world else {
                    invalid_children.push(attachment.child);
                    continue;
                };
                let new_world = parent_world
                    * Mat4::from_translation(attachment.offset)
                    * attachment.child_point_local;
                let (_, rotation, pos) = new_world.to_scale_rotation_translation();
                if !pos.is_finite()
                    || !rotation.is_finite()
                    || !entities.validate_handle(attachment.child.0)
                {
                    invalid_children.push(attachment.child);
                    continue;
                }
                if let Some(character) = characters.get_character_for_entity_mut(attachment.child) {
                    character.transform.position = pos;
                    character.transform.rotation = rotation;
                }
                entities.set_position(attachment.child, pos);
                let (rx, ry, rz) = rotation.to_euler(EulerRot::XYZ);
                entities.set_rotation(attachment.child, Vec3::new(rx, ry, rz));
                continue;
            }
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

    pub fn set_offset(&mut self, child: EntityId, offset: Vec3) -> Result<(), String> {
        if !offset.is_finite() {
            return Err("attachment offset must contain finite values".to_string());
        }
        let mount = self
            .attachments
            .iter_mut()
            .find(|mount| mount.child == child)
            .ok_or_else(|| "attachment not found".to_string())?;
        mount.offset = offset;
        Ok(())
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
