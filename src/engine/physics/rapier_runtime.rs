use super::physics_world::PhysicsWorld;
use super::rapier_backend::{BodyShape, RigidSolver};
use crate::engine::collision_geometry::CollisionShape;
use crate::engine::entity::{EntityId, EntityManager, EntityPhysicsState};
use crate::world::World;
use glam::{EulerRot, Quat, Vec3};
use rapier3d::prelude::{ColliderBuilder, Vector};
use std::collections::HashSet;

const ACTOR_NAMESPACE: u64 = 1 << 63;
fn actor_key(id: EntityId) -> u64 {
    id.0 | ACTOR_NAMESPACE
}
fn cell_key(id: u64) -> u64 {
    id & !ACTOR_NAMESPACE
}
fn axis(v: Vec3) -> Vector {
    Vector::new(v.x, v.y, v.z)
}
fn orientation(rot: Vec3) -> Quat {
    Quat::from_euler(EulerRot::XYZ, rot.x, rot.y, rot.z)
}
fn source_actor_collider(
    physics: &EntityPhysicsState,
    rotation: Quat,
) -> (Vec<ColliderBuilder>, f32) {
    if !physics.solid || !physics.collision_enabled {
        return (Vec::new(), 1.0);
    }
    let mut colliders = Vec::new();
    let mut total_volume = 0.0_f32;
    if let Some(instance) = physics.collider.as_ref().filter(|c| !c.shapes.is_empty()) {
        let inst_rotation = orientation(instance.rotation);
        let local_rotation = rotation.inverse() * inst_rotation;
        let scale = instance.scale;
        let radius_scale = scale.abs().max_element();
        for shape in instance.shapes.iter() {
            let local_center: Vec3;
            let mut relative_rotation = local_rotation;
            let (builder, volume) = match shape {
                CollisionShape::Sphere { center, radius } => {
                    let r = radius.abs() * radius_scale;
                    if r <= 0.0001 {
                        continue;
                    }
                    local_center = Vec3::from_array(*center) * scale;
                    (
                        ColliderBuilder::ball(r),
                        4.0 * std::f32::consts::PI * r.powi(3) / 3.0,
                    )
                }
                CollisionShape::Box { min, max } => {
                    let a = Vec3::from_array(*min) * scale;
                    let b = Vec3::from_array(*max) * scale;
                    local_center = (a + b) * 0.5;
                    let half = (b - a).abs() * 0.5;
                    if half.min_element() <= 0.0001 {
                        continue;
                    }
                    (
                        ColliderBuilder::cuboid(half.x, half.y, half.z),
                        8.0 * half.x * half.y * half.z,
                    )
                }
                CollisionShape::Capsule { start, end, radius } => {
                    let a = Vec3::from_array(*start) * scale;
                    let b = Vec3::from_array(*end) * scale;
                    local_center = (a + b) * 0.5;
                    let r = radius.abs() * radius_scale;
                    let length = (b - a).length();
                    if r <= 0.0001 {
                        continue;
                    }
                    if length > 0.0001 {
                        relative_rotation =
                            local_rotation * Quat::from_rotation_arc(Vec3::Y, (b - a) / length);
                    }
                    (
                        ColliderBuilder::capsule_y(length * 0.5, r),
                        std::f32::consts::PI * r * r * length
                            + 4.0 * std::f32::consts::PI * r.powi(3) / 3.0,
                    )
                }
                CollisionShape::Triangles { vertices } => {
                    let mut points = Vec::with_capacity(vertices.len() * 3);
                    for face in vertices {
                        for point in face {
                            let p = Vec3::from_array(*point) * scale;
                            if p.is_finite() {
                                points.push(axis(p));
                            }
                        }
                    }
                    if points.len() < 4 {
                        continue;
                    }
                    local_center = Vec3::ZERO;
                    let Some(builder) = ColliderBuilder::convex_hull(&points) else {
                        continue;
                    };
                    (builder, physics.size.x * physics.size.y * physics.size.z)
                }
            };
            let center_world = inst_rotation * local_center - physics.offset;
            let relative_translation = rotation.inverse() * center_world;
            let ang = relative_rotation.to_scaled_axis();
            colliders.push(
                builder
                    .translation(axis(relative_translation))
                    .rotation(axis(ang)),
            );
            total_volume += volume.max(0.0);
        }
    }
    if colliders.is_empty() {
        let half = physics.size.abs() * 0.5;
        if half.min_element() > 0.0001 {
            colliders.push(ColliderBuilder::cuboid(half.x, half.y, half.z));
            total_volume = 8.0 * half.x * half.y * half.z;
        }
    }
    (colliders, total_volume.max(0.001))
}
impl PhysicsWorld {
    pub fn step_unified_rapier(
        &mut self,
        manager: &mut EntityManager,
        gravity: Vec3,
        dt: f32,
        character_entities: &HashSet<EntityId>,
        characters: Option<&crate::character::CharacterSystem>,
        terrain: Option<&World>,
    ) {
        if !dt.is_finite() || dt <= 0.0 {
            return;
        }
        let mut solver = if let Some(solver) = self.rapier_runtime.take() {
            solver
        } else {
            let mut solver = RigidSolver::new(gravity, dt);
            for &(id, pos) in &self.static_colliders {
                let friction = self
                    .static_friction
                    .get(&id)
                    .copied()
                    .unwrap_or(crate::world::cell::DEFAULT_SURFACE_FRICTION);
                solver.add_static(
                    BodyShape::Box {
                        half_extents: Vec3::splat(0.5),
                    },
                    pos + Vec3::splat(0.5),
                    friction,
                );
            }
            if let Some(mesh) = self.native_terrain_collider() {
                solver.add_static_triangles(
                    &mesh.triangle_vertices(),
                    crate::world::cell::DEFAULT_SURFACE_FRICTION,
                );
            }
            solver
        };
        let mut wanted = HashSet::new();

        for body in &self.bodies {
            if body.anchored || !body.solid {
                continue;
            }
            let key = cell_key(body.id.0);
            wanted.insert(key);
            if !solver.contains(key) {
                let density = body.density.max(0.001);
                solver.add_dynamic(
                    key,
                    BodyShape::Box {
                        half_extents: body.size.abs() * 0.5,
                    },
                    body.center(),
                    density,
                    body.friction,
                    body.velocity,
                    body.rotation,
                    true,
                );
            } else {
                solver.apply_motion(
                    key,
                    body.center(),
                    body.rotation,
                    body.velocity,
                    body.angular_velocity,
                    Vec3::ZERO,
                    Vec3::ZERO,
                    Vec3::ZERO,
                    self.rapier_published.get(&key).copied(),
                );
            }
        }

        for entity in manager.iter() {
            let p = &entity.physics;
            if !entity.enabled {
                continue;
            }
            let active_character = if character_entities.contains(&entity.id) {
                characters
                    .and_then(|cs| cs.get_character_for_entity(entity.id))
                    .filter(|ch| {
                        ch.collision_enabled
                            && ch.collision_solid
                            && ch.contact_collider().is_some()
                    })
            } else {
                None
            };
            let kinematic = active_character.is_some();
            if character_entities.contains(&entity.id) && !kinematic {
                continue;
            }
            if !kinematic && (!p.enabled || (!p.anchored && (!p.mass.is_finite() || p.mass <= 0.0)))
            {
                continue;
            }
            let id = actor_key(entity.id);
            wanted.insert(id);
            let q = active_character
                .map_or_else(|| orientation(entity.rotation), |ch| ch.transform.rotation);
            let center =
                active_character.map_or(entity.position + p.offset, |ch| ch.transform.position);
            let dynamic = !p.anchored && !kinematic;
            if !solver.contains(id) {
                let (parts, volume) = if let Some(ch) = active_character {
                    let mut state = p.clone();
                    state.solid = true;
                    state.collision_enabled = true;
                    state.collider = ch.contact_collider();
                    state.offset = Vec3::ZERO;
                    source_actor_collider(&state, q)
                } else {
                    source_actor_collider(p, q)
                };
                let mass = if p.mass.is_finite() && p.mass > 0.0 {
                    p.mass
                } else {
                    1.0
                };
                solver.add_body(
                    id,
                    parts,
                    center,
                    q,
                    mass,
                    mass / volume,
                    crate::world::cell::DEFAULT_SURFACE_FRICTION,
                    p.velocity,
                    dynamic,
                    kinematic,
                );
            }
            solver.set_body_mode(id, dynamic, kinematic);
            if kinematic {
                solver.sync_kinematic_actor(id, center, q);
            } else if p.anchored {
                solver.sync_fixed_actor(id, center, q);
            } else {
                solver.apply_motion(
                    id,
                    center,
                    q,
                    p.velocity,
                    p.angular_velocity,
                    p.accumulated_force,
                    p.accumulated_torque,
                    gravity * (p.gravity_scale - 1.0),
                    self.rapier_published.get(&id).copied(),
                );
            }
        }
        solver.retain_dynamic(&wanted);

        if let Some(world) = terrain {
            let mut new_patches = Vec::new();
            for body in &self.bodies {
                if !body.anchored && body.solid {
                    let c = body.center();
                    for dx in -2..=2 {
                        for dz in -2..=2 {
                            new_patches.push((c.x.floor() as i32 + dx, c.z.floor() as i32 + dz));
                        }
                    }
                }
            }
            for entity in manager.iter() {
                if !entity.enabled
                    || (!entity.physics.enabled && !character_entities.contains(&entity.id))
                {
                    continue;
                }
                let c = entity.position + entity.physics.offset;
                for dx in -2..=2 {
                    for dz in -2..=2 {
                        new_patches.push((c.x.floor() as i32 + dx, c.z.floor() as i32 + dz));
                    }
                }
            }
            let wanted_patches: HashSet<_> = new_patches.into_iter().collect();
            self.rapier_terrain_patches.retain(|key, handle| {
                if wanted_patches.contains(key) {
                    true
                } else {
                    solver.remove_terrain_patch(*handle);
                    false
                }
            });
            for (x, z) in wanted_patches {
                if self.rapier_terrain_patches.contains_key(&(x, z)) {
                    continue;
                }
                let corners = [(0, 0), (1, 0), (1, 1), (0, 1)].map(|(dx, dz)| {
                    let xx = (x + dx) as f32;
                    let zz = (z + dz) as f32;
                    world
                        .terrain
                        .surface_height_at(xx, zz)
                        .map(|h| Vec3::new(xx, h, zz))
                });
                if let [Some(a), Some(b), Some(c), Some(d)] = corners {
                    let friction = world
                        .terrain
                        .surface_friction_at(x as f32 + 0.5, z as f32 + 0.5);
                    if let Some(handle) = solver.add_terrain_patch([a, b, c, d], friction) {
                        self.rapier_terrain_patches.insert((x, z), handle);
                    }
                }
            }
        }

        solver.step(gravity);
        self.rapier_published.clear();
        for body in &mut self.bodies {
            let key = cell_key(body.id.0);
            if !wanted.contains(&key) {
                continue;
            }
            if let Some(state) = solver.state(key) {
                body.position = state.translation - body.size * 0.5;
                body.rotation = state.rotation;
                body.velocity = state.linear_velocity;
                body.angular_velocity = state.angular_velocity;
                body.is_sleeping = state.sleeping;
                self.rapier_published.insert(key, state);
            }
        }
        for entity in manager.iter_mut() {
            if !entity.enabled
                || !entity.physics.enabled
                || entity.physics.anchored
                || character_entities.contains(&entity.id)
            {
                continue;
            }
            let key = actor_key(entity.id);
            let Some(state) = solver.state(key) else {
                continue;
            };
            let (_, _, _) = state.rotation.to_euler(EulerRot::XYZ);
            let (x, y, z) = state.rotation.to_euler(EulerRot::XYZ);
            entity.rotation = Vec3::new(x, y, z);
            if let Some(collider) = entity.physics.collider.as_mut() {
                collider.rotation = entity.rotation;
                if let Some((lo, hi)) = crate::engine::collision_geometry::envelope(collider) {
                    entity.physics.size = (hi - lo).max(Vec3::splat(0.01));
                    entity.physics.offset = (hi + lo) * 0.5;
                }
            }
            entity.position = state.translation - entity.physics.offset;
            entity.physics.velocity = state.linear_velocity;
            entity.physics.angular_velocity = state.angular_velocity;
            entity.physics.grounded = solver.supported(key, gravity);
            entity.physics.accumulated_force = Vec3::ZERO;
            entity.physics.accumulated_torque = Vec3::ZERO;
            self.rapier_published.insert(key, state);
        }
        self.rapier_runtime = Some(solver);
        self.rebuild_dynamic_body_index();
    }
}

#[cfg(test)]
mod unified_runtime_tests {
    use super::*;
    use crate::engine::collision_geometry::{ColliderInstance, CollisionShape};
    use crate::engine::physics::{PhysicsBody, PhysicsBodyId};
    use std::sync::Arc;

    #[test]
    fn shared_runtime_cell_settles_on_anchored_support() {
        let mut physics = PhysicsWorld::new();
        physics.add_static_collider(1, Vec3::ZERO);
        physics.bodies.push(PhysicsBody::new(
            PhysicsBodyId(7),
            7,
            Vec3::new(0.0, 2.0, 0.0),
            Vec3::ONE,
        ));
        let mut entities = EntityManager::new();
        for _ in 0..180 {
            physics.step_unified_rapier(
                &mut entities,
                Vec3::NEG_Y * 9.81,
                1.0 / 60.0,
                &HashSet::new(),
                None,
                None,
            );
        }
        let body = &physics.bodies[0];
        assert!(
            body.position.y > 0.95 && body.position.y < 1.1,
            "runtime Cell remains on anchored support: {:?}",
            body.position
        );
        assert!(body.velocity.length() < 0.05);
    }

    #[test]
    fn shared_runtime_sphere_actor_transfers_momentum_to_cell() {
        let mut physics = PhysicsWorld::new();
        physics.bodies.push(PhysicsBody::new(
            PhysicsBodyId(7),
            7,
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::ONE,
        ));
        let mut entities = EntityManager::new();
        let id = entities.create_entity("DynamicSphere");
        {
            let e = entities.get_entity_mut(id).unwrap();
            e.position = Vec3::new(-1.0, 0.5, 0.5);
            e.physics.enabled = true;
            e.physics.anchored = false;
            e.physics.solid = true;
            e.physics.collision_enabled = true;
            e.physics.velocity = Vec3::X * 7.0;
            e.physics.collider = Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Sphere {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.5,
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            });
        }
        for _ in 0..45 {
            physics.step_unified_rapier(
                &mut entities,
                Vec3::ZERO,
                1.0 / 60.0,
                &HashSet::new(),
                None,
                None,
            );
        }
        assert!(
            physics.bodies[0].velocity.x > 0.3,
            "dynamic Actor must transfer momentum into the same Cell solver"
        );
        assert!(entities.get_entity(id).unwrap().position.is_finite());
    }

    #[test]
    fn shared_runtime_ccd_prevents_sphere_wall_tunneling() {
        let mut physics = PhysicsWorld::new();
        physics.add_static_collider(1, Vec3::new(2.0, 0.0, 0.0));
        let mut entities = EntityManager::new();
        let id = entities.create_entity("FastSphere");
        {
            let e = entities.get_entity_mut(id).unwrap();
            e.position = Vec3::new(0.0, 0.5, 0.5);
            e.physics.enabled = true;
            e.physics.anchored = false;
            e.physics.velocity = Vec3::X * 250.0;
            e.physics.collider = Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Sphere {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.5,
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            });
        }
        for _ in 0..10 {
            physics.step_unified_rapier(
                &mut entities,
                Vec3::ZERO,
                1.0 / 60.0,
                &HashSet::new(),
                None,
                None,
            );
        }
        let x = entities.get_entity(id).unwrap().position.x;
        assert!(x <= 1.52, "sphere crossed static wall: x={x}");
    }
    #[test]
    fn active_character_without_actor_physics_pushes_only_dynamic_actors() {
        let mut physics = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let char_id = entities.create_entity("CharacterControlled");
        let dynamic_id = entities.create_entity("DynamicActor");
        {
            let e = entities.get_entity_mut(char_id).unwrap();
            e.physics.enabled = false;
        }
        {
            let e = entities.get_entity_mut(dynamic_id).unwrap();
            e.position = Vec3::new(0.85, 0.7, 0.0);
            e.physics.enabled = true;
            e.physics.anchored = false;
            e.physics.solid = true;
            e.physics.collision_enabled = true;
            e.physics.mass = 1.0;
            e.physics.size = Vec3::ONE;
        }
        let mut characters = crate::character::CharacterSystem::new();
        let character_id = characters.spawn_character(Vec3::new(-1.0, 0.0, 0.0), None);
        characters.associate_entity(char_id, character_id);
        let ids = HashSet::from([char_id]);
        // Actor physics remains disabled, but its active solid Character
        // must have a kinematic contact shape and move dynamic bodies.
        for i in 0..50 {
            let c = characters.get_character_for_entity_mut(char_id).unwrap();
            c.transform.position.x = -1.0 + i as f32 * 0.06;
            physics.step_unified_rapier(
                &mut entities,
                Vec3::ZERO,
                1.0 / 60.0,
                &ids,
                Some(&characters),
                None,
            );
        }
        assert!(
            physics
                .rapier_runtime
                .as_ref()
                .is_some_and(|r| r.contains(actor_key(char_id))),
            "active Character without Actor physics needs a Rapier collider"
        );
        let dynamic = &entities.get_entity(dynamic_id).unwrap().physics;
        assert!(
            dynamic.velocity.x > 0.05,
            "kinematic Character motion must apply an impulse to the dynamic Actor"
        );
    }

    #[test]
    fn non_solid_character_is_not_registered_with_rapier() {
        let mut physics = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let id = entities.create_entity("NonSolidCharacter");
        let mut characters = crate::character::CharacterSystem::new();
        let c = characters.spawn_character(Vec3::ZERO, None);
        characters.associate_entity(id, c);
        characters
            .get_character_for_entity_mut(id)
            .unwrap()
            .collision_solid = false;
        physics.step_unified_rapier(
            &mut entities,
            Vec3::ZERO,
            1.0 / 60.0,
            &HashSet::from([id]),
            Some(&characters),
            None,
        );
        assert!(
            physics
                .rapier_runtime
                .as_ref()
                .is_some_and(|r| !r.contains(actor_key(id)))
        );
    }

    #[test]
    fn actor_contact_events_are_derived_from_manifolds() {
        let mut physics = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let moving = entities.create_entity("MovingSphere");
        let wall = entities.create_entity("FixedSphere");
        for (id, pos, anchored, speed) in [
            (moving, Vec3::new(0.0, 0.0, 0.0), false, 2.0),
            (wall, Vec3::new(0.85, 0.0, 0.0), true, 0.0),
        ] {
            let e = entities.get_entity_mut(id).unwrap();
            e.position = pos;
            e.physics.enabled = true;
            e.physics.anchored = anchored;
            e.physics.collision_enabled = true;
            e.physics.velocity = Vec3::X * speed;
            e.physics.collider = Some(ColliderInstance {
                shapes: Arc::new(vec![CollisionShape::Sphere {
                    center: [0.0, 0.0, 0.0],
                    radius: 0.5,
                }]),
                rotation: Vec3::ZERO,
                scale: Vec3::ONE,
            });
        }
        let mut seen = false;
        for _ in 0..15 {
            physics.step_unified_rapier(
                &mut entities,
                Vec3::ZERO,
                1.0 / 60.0,
                &HashSet::new(),
                None,
                None,
            );
            let events = physics.process_entity_contacts_and_collisions(&mut entities, None);
            if events
                .current
                .contains(&crate::engine::entity::EntityContactPair::new(moving, wall))
            {
                seen = true;
                break;
            }
        }
        assert!(
            seen,
            "Rapier contacts must remain observable through existing script contact events"
        );
    }

    #[test]
    fn explicitly_disabled_actor_collision_stays_disabled() {
        let mut physics = PhysicsWorld::new();
        physics.add_static_collider(1, Vec3::new(2.0, 0.0, 0.0));
        let mut entities = EntityManager::new();
        let id = entities.create_entity("Ghost");
        {
            let e = entities.get_entity_mut(id).unwrap();
            e.position = Vec3::new(0.0, 0.5, 0.5);
            e.physics.enabled = true;
            e.physics.anchored = false;
            e.physics.collision_enabled = false;
            e.physics.velocity = Vec3::X * 5.0;
        }
        for _ in 0..80 {
            physics.step_unified_rapier(
                &mut entities,
                Vec3::ZERO,
                1.0 / 60.0,
                &HashSet::new(),
                None,
                None,
            );
        }
        let x = entities.get_entity(id).unwrap().position.x;
        assert!(x > 4.0, "explicitly disabled collider blocked: x={x}");
    }
}
