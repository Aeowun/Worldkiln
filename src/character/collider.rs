// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::Character;
use crate::engine::{
    collision_geometry::{self, ColliderInstance, CollisionShape},
    physics::PhysicsWorld,
};
use glam::Vec3;
use std::{collections::HashSet, sync::Arc};

impl Character {
    pub(crate) fn active_collider(&self) -> Option<ColliderInstance> {
        if !self.collision_enabled {
            return None;
        }
        let mut shape = self.collision_geometry.clone()?;
        let (x, y, z) = self.transform.rotation.to_euler(glam::EulerRot::XYZ);
        shape.rotation = Vec3::new(x, y, z);
        shape.scale = self.transform.scale;
        Some(shape)
    }
    pub(crate) fn contact_collider(&self) -> Option<ColliderInstance> {
        if !self.collision_enabled {
            return None;
        }
        self.active_collider().or_else(|| {
            if self.uses_authored_collider {
                return None;
            }
            Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Box {
                    min: [-self.collision.radius, 0.0, -self.collision.radius],
                    max: [
                        self.collision.radius,
                        self.collision.height,
                        self.collision.radius,
                    ],
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            })
        })
    }
    pub(crate) fn collision_bounds(&self) -> (Vec3, Vec3) {
        if let Some(body) = self
            .active_collider()
            .and_then(|b| collision_geometry::envelope(&b))
        {
            return (
                self.transform.position + body.0,
                self.transform.position + body.1,
            );
        }
        (
            self.transform.position - Vec3::new(self.collision.radius, 0.0, self.collision.radius),
            self.transform.position
                + Vec3::new(
                    self.collision.radius,
                    self.collision.height,
                    self.collision.radius,
                ),
        )
    }
    pub(crate) fn refresh_collision_profile(&mut self) {
        let Some(mut body) = self.active_collider() else {
            return;
        };
        let Some((lo, hi)) = collision_geometry::envelope(&body) else {
            return;
        };
        self.collision.height = (hi.y - lo.y).max(0.01);
        body.rotation = Vec3::ZERO;
        if let Some((a, b)) = collision_geometry::envelope(&body) {
            let x = a.x.abs().max(b.x.abs());
            let z = a.z.abs().max(b.z.abs());
            self.collision.radius = (x * x + z * z).sqrt().max(0.01);
        }
    }
    pub(crate) fn navigation_bottom_offset(&self) -> f32 {
        self.active_collider()
            .and_then(|b| collision_geometry::envelope(&b))
            .map_or(0.0, |(a, _)| a.y)
    }
}

pub(crate) fn resolve_active_static_cells(
    character: &mut Character,
    physics: &PhysicsWorld,
    touched: &mut HashSet<u64>,
    from: Vec3,
    max_walkable_slope_degrees: f32,
) {
    if !character.collision_enabled || !character.collision_solid {
        return;
    }
    let Some(body) = character.contact_collider() else {
        return;
    };
    let Some((lo, hi)) = collision_geometry::envelope(&body) else {
        return;
    };
    let to = character.transform.position;
    let motion = to - from;
    if !motion.is_finite() {
        character.transform.position = from;
        return;
    }
    let faces = physics.anchored_faces_in_aabb(
        from.min(to) + lo - Vec3::splat(0.01),
        from.max(to) + hi + Vec3::splat(0.01),
    );
    let steps = (motion.length() / 0.1).ceil().clamp(1.0, 256.0) as usize;
    let increment = motion / steps as f32;
    let mut position = from;
    let previously_grounded = character.movement.is_grounded;
    character.movement.is_grounded = false;
    for _ in 0..steps {
        let mut next = position + increment;
        for _ in 0..16 {
            let mut best = None;
            for f in &faces {
                let min = f.min - f.normal.max(Vec3::ZERO);
                let max = f.max - f.normal.min(Vec3::ZERO);
                let Some(hit) = collision_geometry::against_box(&body, next, min, max) else {
                    continue;
                };
                let alignment = hit.normal.dot(f.normal);
                if alignment < 0.5 || hit.depth <= 0.00001 {
                    continue;
                }
                let depth = hit.depth / alignment;
                if best.is_none_or(|(_, d, _)| depth > d) {
                    best = Some((f.normal, depth, f.cell_id));
                }
            }
            let Some((normal, depth, id)) = best else {
                break;
            };
            next += normal * (depth + 0.00001);
            touched.insert(id);
            let into = character.movement.velocity.dot(normal).min(0.0);
            character.movement.velocity -= normal * into;
            let slide = character.movement.slope_slide_velocity.dot(normal).min(0.0);
            character.movement.slope_slide_velocity -= normal * slide;
            if super::collision::contact_is_walkable(normal, max_walkable_slope_degrees, previously_grounded) {
                character.movement.is_grounded = true;
            }
        }
        position = next;
    }
    character.transform.position = position;
    if character.movement.velocity.y <= 0.0 {
        for f in faces.iter().filter(|f| super::collision::contact_is_walkable(f.normal, max_walkable_slope_degrees, previously_grounded)) {
            let min = f.min - f.normal.max(Vec3::ZERO);
            let max = f.max - f.normal.min(Vec3::ZERO);
            if collision_geometry::against_box(&body, position - Vec3::Y * 0.002, min, max)
                .is_some_and(|h| super::collision::contact_is_walkable(h.normal, max_walkable_slope_degrees, previously_grounded))
            {
                character.movement.is_grounded = true;
                touched.insert(f.cell_id);
                break;
            }
        }
    }
}

pub(crate) fn sweep_terrain(
    physics: &PhysicsWorld,
    world: &crate::world::World,
    c: &Character,
    from: Vec3,
    to: Vec3,
) -> Option<(
    Vec3,
    Vec<crate::engine::physics::triangle_collider::CapsuleContact>,
)> {
    if !c.collision_enabled || !c.collision_solid {
        return Some((to, Vec::new()));
    }
    if c.uses_authored_collider && c.active_collider().is_none() {
        return Some((to, Vec::new()));
    }
    if let Some(body) = c.contact_collider() {
        physics.sweep_terrain_geometry(
            world,
            from,
            to,
            c.collision.radius,
            c.collision.height,
            &body,
        )
    } else {
        physics.sweep_terrain(world, from, to, c.collision.radius, c.collision.height)
    }
}

pub(crate) fn resolve_active_dynamic_cells(
    c: &mut Character,
    physics: &mut PhysicsWorld,
    touched: &mut HashSet<u64>,
    from: Vec3,
    max_walkable_slope_degrees: f32,
) {
    let Some(body) = c.contact_collider() else {
        return;
    };
    if physics.dynamic_bodies_by_cell.is_empty() && !physics.bodies.is_empty() {
        physics.rebuild_dynamic_body_index();
    }
    let Some((lo, hi)) = collision_geometry::envelope(&body) else {
        return;
    };
    let to = c.transform.position;
    let motion = to - from;
    let ids = physics.query_dynamic_bodies_in_aabb(
        from.min(to) + lo - Vec3::splat(0.01),
        from.max(to) + hi + Vec3::splat(0.01),
    );
    if ids.is_empty() {
        return;
    }
    let steps = (motion.length() / 0.1).ceil().clamp(1.0, 256.0) as usize;
    let step = motion / steps as f32;
    let mut at = from;
    for _ in 0..steps {
        at += step;
        for _ in 0..16 {
            let mut hit = None;
            for &id in &ids {
                let b = &physics.bodies[id];
                if b.anchored || !b.solid {
                    continue;
                }
                if let Some(h) =
                    collision_geometry::against_box(&body, at, b.min_corner(), b.max_corner())
                {
                    if h.depth > 0.00001
                        && hit.is_none_or(|(_, old): (_, collision_geometry::Contact)| {
                            h.depth > old.depth
                        })
                    {
                        hit = Some((id, h));
                    }
                }
            }
            let Some((id, h)) = hit else { break };
            use crate::engine::physics::rigid_contact::{Motion, resolve_with_friction};
            let b = &mut physics.bodies[id];
            let mass = b.get_mass();
            let mut mover = Motion {
                position: at,
                velocity: c.movement.velocity,
                inverse_mass: 1.0,
            };
            let mut cell = Motion {
                position: b.position,
                velocity: b.velocity,
                inverse_mass: if mass.is_finite() && mass > 0.0 {
                    1.0 / mass
                } else {
                    0.0
                },
            };
            let was_rising = c.movement.velocity.y > 0.0;
            if resolve_with_friction(&mut mover, &mut cell, h.normal, h.depth, b.friction) {
                at = mover.position;
                c.movement.velocity = mover.velocity;
                if h.normal.y > 0.0
                    && !super::collision::contact_is_walkable(h.normal, max_walkable_slope_degrees, false)
                    && !was_rising
                {
                    c.movement.velocity.y = c.movement.velocity.y.min(0.0);
                }
                b.position = cell.position;
                b.velocity = cell.velocity;
                if super::collision::contact_is_walkable(h.normal, max_walkable_slope_degrees, c.movement.is_grounded) {
                    c.movement.is_grounded = true;
                }
                b.wake();
                if b.cell_id != 0 {
                    touched.insert(b.cell_id);
                }
            }
        }
    }
    c.transform.position = at;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        character::CharacterSystem,
        engine::entity::{EntityId, EntityManager},
        world::{Actor, CellType, World, WorldCoord},
    };
    fn custom_actor() -> Actor {
        let mut a = Actor::new(1, "scaled body", Vec3::new(3.0, 1.0, 3.0));
        a.physics.anchored = false;
        a.physics.collision_mode_explicit = true;
        a.physics.collision_size = Vec3::new(2.0, 4.0, 2.0);
        a.physics.collision_offset = Vec3::new(0.0, 2.0, 0.0);
        a
    }
    #[test]
    fn character_box_default_and_runtime_sync_preserve_the_box_contract() {
        let mut system = CharacterSystem::new();
        let entity = EntityId(9299);
        let id = system.spawn_character(Vec3::ZERO, None);
        system.associate_entity(entity, id);
        let original = system
            .get_character_for_entity(entity)
            .unwrap()
            .collision
            .radius;
        for _ in 0..3 {
            let character = system.get_character_for_entity_mut(entity).unwrap();
            character.refresh_collision_profile();
            let body = character.contact_collider().unwrap();
            assert!(matches!(body.shapes[0], CollisionShape::Box { .. }));
            assert_eq!(character.collision.radius, original);
        }
        let mut actor = custom_actor();
        actor.physics.collision_mode = collision_geometry::CollisionMode::Auto;
        actor.package = Some("unavailable_visual_mesh".into());
        system.configure_authored_collision(entity, &actor);
        let body = system
            .get_character_for_entity(entity)
            .unwrap()
            .contact_collider()
            .unwrap();
        assert!(matches!(body.shapes[0], CollisionShape::Box { .. }));
        let mut entities = EntityManager::new();
        let runtime = entities.create_entity("Character Box");
        system.associate_entity(runtime, id);
        system.sync_entity_positions(&mut entities);
        assert_eq!(
            entities.get_physics(runtime).unwrap().collider.as_ref(),
            Some(&body)
        );
    }
    #[test]
    fn active_character_scale_matches_only_selected_green_body_once() {
        let mut system = CharacterSystem::new();
        let entity = EntityId(9200);
        let id = system.spawn_character(Vec3::new(3.0, 1.0, 3.0), None);
        system.associate_entity(entity, id);
        for scale in [0.25, 0.5, 1.0, 2.0] {
            let mut a = custom_actor();
            a.scale = Vec3::splat(scale);
            for _ in 0..2 {
                system.configure_authored_collision(entity, &a);
                let c = system.get_character_for_entity(entity).unwrap();
                let body = c.active_collider().unwrap();
                assert_eq!(
                    Some(body.clone()),
                    collision_geometry::preview_actor_collider(&a)
                );
                let (lo, hi) = collision_geometry::envelope(&body).unwrap();
                assert!(lo.distance(Vec3::new(-scale, 0.0, -scale)) < 0.0001);
                assert!(hi.distance(Vec3::new(scale, 4.0 * scale, scale)) < 0.0001);
                assert_eq!(
                    body.shapes.len(),
                    1,
                    "only selected box, no package capsule"
                );
            }
        }
        let mut manager = EntityManager::new();
        let runtime = manager.create_entity("same body");
        let char_id = system.spawn_character(Vec3::new(3.0, 1.0, 3.0), None);
        system.associate_entity(runtime, char_id);
        let mut a = custom_actor();
        a.scale = Vec3::splat(0.5);
        system.configure_authored_collision(runtime, &a);
        system.sync_entity_positions(&mut manager);
        assert_eq!(
            manager.get_physics(runtime).unwrap().collider,
            system
                .get_character_for_entity(runtime)
                .unwrap()
                .active_collider()
        );
    }
    #[test]
    fn active_character_geometry_has_no_hidden_package_radius_separation() {
        let mut system = CharacterSystem::new();
        for (i, x) in [(1, 0.0), (2, 0.6)] {
            let entity = EntityId(i);
            let id = system.spawn_character(Vec3::new(x, 0.0, 0.0), None);
            system.associate_entity(entity, id);
            let mut a = custom_actor();
            a.position = Vec3::new(x, 0.0, 0.0);
            a.scale = Vec3::splat(0.5);
            a.physics.collision_size = Vec3::new(0.4, 2.0, 0.4);
            a.physics.collision_offset = Vec3::new(0.0, 1.0, 0.0);
            system.configure_authored_collision(entity, &a);
            system
                .get_character_for_entity_mut(entity)
                .unwrap()
                .collision
                .radius = 10.0;
        }
        super::super::collision::resolve_character_collisions(&mut system.characters);
        assert_eq!(
            system
                .get_character_for_entity(EntityId(1))
                .unwrap()
                .transform
                .position,
            Vec3::ZERO
        );
        assert_eq!(
            system
                .get_character_for_entity(EntityId(2))
                .unwrap()
                .transform
                .position,
            Vec3::new(0.6, 0.0, 0.0)
        );
    }
    #[test]
    fn active_character_half_scale_box_and_asset_capsule_cannot_be_pushed_through_cells() {
        for capsule in [false, true] {
            let mut world = World::new();
            for x in 0..9 {
                for z in 0..7 {
                    world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
                }
            }
            for y in 1..5 {
                for z in 0..7 {
                    world.set_cell(WorldCoord::new(4, y, z), CellType::Block);
                }
            }
            let mut physics = PhysicsWorld::new();
            physics.register_from_world(&world);
            let mut system = CharacterSystem::new();
            for (entity, x) in [(EntityId(9301), 2.7), (EntityId(9302), 3.59)] {
                let id = system.spawn_character(Vec3::new(x, 1.0, 3.5), None);
                system.associate_entity(entity, id);
                let mut a = custom_actor();
                a.scale = Vec3::splat(0.5);
                a.physics.collision_size = Vec3::new(1.6, 3.6, 1.6);
                a.physics.collision_offset = Vec3::new(0.0, 1.8, 0.0);
                system.configure_authored_collision(entity, &a);
                if capsule {
                    system
                        .get_character_for_entity_mut(entity)
                        .unwrap()
                        .collision_geometry = Some(ColliderInstance {
                        shapes: Arc::new(vec![CollisionShape::Capsule {
                            start: [0.0, 0.8, 0.0],
                            end: [0.0, 2.8, 0.0],
                            radius: 0.8,
                        }]),
                        rotation: Vec3::ZERO,
                        scale: Vec3::splat(0.5),
                    });
                }
            }
            for frame in 0..360 {
                system
                    .get_character_for_entity_mut(EntityId(9301))
                    .unwrap()
                    .movement
                    .velocity
                    .x = 6.0;
                system
                    .get_character_for_entity_mut(EntityId(9302))
                    .unwrap()
                    .movement
                    .velocity
                    .x = 0.0;
                system.update_scripted(&world, &mut physics, 1.0 / 60.0);
                for entity in [EntityId(9301), EntityId(9302)] {
                    let c = system.get_character_for_entity(entity).unwrap();
                    let (lo, hi) = c.collision_bounds();
                    assert!(
                        hi.x <= 4.001,
                        "active capsule={capsule} frame={frame} body crossed wall: {hi:?}"
                    );
                    assert!(
                        (lo.y - 1.0).abs() < 0.005,
                        "body, not origin proxy, must stay on floor: {lo:?}"
                    );
                }
            }
            let a = system.get_character_for_entity(EntityId(9301)).unwrap();
            let b = system.get_character_for_entity(EntityId(9302)).unwrap();
            assert!(
                b.transform.position.x - a.transform.position.x >= 0.798,
                "pusher must stop when neighbor is pinned"
            );
        }
    }
    #[test]
    fn active_character_no_collision_does_not_reintroduce_package_body() {
        let mut system = CharacterSystem::new();
        let entity = EntityId(9401);
        let id = system.spawn_character(Vec3::ZERO, None);
        system.associate_entity(entity, id);
        let mut a = custom_actor();
        a.physics.collision_mode = collision_geometry::CollisionMode::Disabled;
        system.configure_authored_collision(entity, &a);
        let c = system.get_character_for_entity_mut(entity).unwrap();
        assert!(c.contact_collider().is_none());
        c.collision_enabled = true;
        assert!(
            c.contact_collider().is_none(),
            "missing selected body must never enable an invisible fallback"
        );
    }
}

#[cfg(test)]
mod dynamic_contact_contracts {
    use super::*;
    use crate::{
        character::CharacterSystem,
        engine::entity::EntityManager,
        world::{Actor, CellType, World, WorldCoord},
    };
    #[test]
    fn active_character_and_dynamic_entity_resolve_once_in_authoritative_systems() {
        let mut world = World::new();
        for x in 0..5 {
            for z in 0..5 {
                world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
            }
        }
        let mut physics = PhysicsWorld::new();
        physics.register_from_world(&world);
        let mut entities = EntityManager::new();
        let id = entities.create_entity("character");
        let prop = entities.create_entity("dynamic prop");
        let mut system = CharacterSystem::new();
        let c = system.spawn_character(Vec3::new(1.0, 1.0, 2.5), None);
        system.associate_entity(id, c);
        let mut actor = Actor::new(1, "custom", Vec3::new(1.0, 1.0, 2.5));
        actor.physics.anchored = false;
        actor.physics.collision_mode_explicit = true;
        actor.physics.collision_size = Vec3::new(0.8, 1.8, 0.8);
        actor.physics.collision_offset = Vec3::new(0.0, 0.9, 0.0);
        system.configure_authored_collision(id, &actor);
        for entity in [id, prop] {
            let e = entities.get_entity_mut(entity).unwrap();
            e.physics.enabled = true;
            e.physics.anchored = false;
        }
        entities.get_entity_mut(prop).unwrap().position = Vec3::new(1.5, 1.5, 2.5);
        system.sync_entity_positions(&mut entities);
        physics.process_entity_contacts_and_collisions_with_world(
            &mut entities,
            Some(&mut system),
            &world,
        );
        let c = system.get_character_for_entity(id).unwrap();
        let e = entities.get_entity(prop).unwrap();
        assert!(
            c.transform.position.x < 0.99,
            "Character correction must reach CharacterSystem"
        );
        assert!(
            e.position.x > 1.51,
            "dynamic props must still respond to Character contacts"
        );
        assert!(c.collision_bounds().1.x <= e.position.x - e.physics.size.x * 0.5 + 0.001);
        assert!(c.collision_bounds().0.y >= 0.999);
    }
}
