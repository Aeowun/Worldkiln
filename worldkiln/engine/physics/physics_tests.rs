#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::character::CharacterSystem;
    use crate::engine::entity::{EntityContactPair, EntityManager};
    use crate::world::{CellType, World, WorldCoord};
    use glam::Vec3;
    use std::collections::HashSet;

    #[test]
    fn test_physics_clock_accumulation() {
        let mut clock = PhysicsClock::new();
        let mut steps = 0;
        clock.update(SIMULATION_DT, |_| steps += 1);
        assert_eq!(steps, 1);
        assert!(clock.accumulator < 0.0001);
        clock.update(SIMULATION_DT / 2.0, |_| steps += 1);
        assert_eq!(steps, 1);
        clock.update(SIMULATION_DT / 2.0, |_| steps += 1);
        assert_eq!(steps, 2);
    }

    #[test]
    fn test_physics_clock_cap() {
        let mut clock = PhysicsClock::new();
        let mut steps = 0;
        clock.update(1.0, |_| steps += 1);
        assert_eq!(steps, MAX_PHYSICS_STEPS);
        assert_eq!(clock.accumulator, 0.0);
    }

    #[test]
    fn character_owned_actor_sync_produces_contact_at_visible_position() {
        let physics = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let player_entity = entities.create_entity("Player");
        let sword_entity = entities.create_entity("Sword");
        let mut characters = CharacterSystem::new();
        let player_character = characters.spawn_character(Vec3::ZERO, Some("Rogue"));
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:25 -05:00
        // Contact/synchronization is independent of optional item art.
        let sword_character = characters.spawn_character(Vec3::new(0.5, 0.0, 0.0), Some("Rogue"));
        characters.associate_entity(player_entity, player_character);
        characters.associate_entity(sword_entity, sword_character);

        let sword_physics = entities.get_physics_mut(sword_entity).unwrap();
        sword_physics.enabled = true;
        sword_physics.collision_enabled = true;
        sword_physics.solid = true;
        sword_physics.anchored = false;

        // CharacterSystem owns the visible package transform. Publishing it
        // before contact evaluation is what makes the touching Actor and its
        // script Entity refer to the same place.
        characters.sync_entity_positions(&mut entities);
        let contacts =
            physics.process_entity_contacts_and_collisions(&mut entities, Some(&mut characters));
        assert!(
            contacts
                .current
                .contains(&EntityContactPair::new(player_entity, sword_entity)),
            "the visible Player/Sword overlap must dispatch an Entity contact"
        );

        characters.set_entity_position(sword_entity, Vec3::new(5.0, 0.0, 0.0));
        characters.sync_entity_positions(&mut entities);
        let contacts =
            physics.process_entity_contacts_and_collisions(&mut entities, Some(&mut characters));
        assert!(
            !contacts
                .current
                .contains(&EntityContactPair::new(player_entity, sword_entity))
        );

        let excluded: HashSet<_> = characters.associated_entity_ids().collect();
        let before = entities.get_position(sword_entity).unwrap();
        physics.step_entities_excluding(&mut entities, Vec3::new(0.0, -9.81, 0.0), 1.0, &excluded);
        assert_eq!(entities.get_position(sword_entity), Some(before));
    }

    #[test]
    fn test_physics_body_id_uniqueness() {
        let mut id_gen = PhysicsIdGenerator::new();
        assert_ne!(id_gen.next(), id_gen.next());
    }

    #[test]
    fn test_world_to_physics_registration() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let c1 = WorldCoord::new(0, 0, 0);
        world.set_cell(c1, CellType::Block);
        if let Some(cell) = world.get_mut(c1) {
            cell.anchored = false;
            cell.texture = "brick".to_string();
        }
        let c2 = WorldCoord::new(1, 0, 0);
        world.set_cell(c2, CellType::Block);
        if let Some(cell) = world.get_mut(c2) {
            cell.anchored = true;
        }
        p_world.register_from_world(&world);
        assert_eq!(p_world.bodies.len(), 1);
        assert_eq!(p_world.bodies[0].texture, "brick");
    }

    #[test]
    fn test_app_mode_transition_logic() {
        use crate::engine::EditorMode;
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let mut last_mode = EditorMode::Editor;
        let coord = WorldCoord::new(0, 5, 0);
        world.set_cell(coord, CellType::Block);
        if let Some(cell) = world.get_mut(coord) {
            cell.anchored = false;
        }
        let simulate_update =
            |mode: EditorMode, last: &mut EditorMode, p: &mut PhysicsWorld, w: &World| {
                if mode == EditorMode::Play && *last == EditorMode::Editor {
                    p.register_from_world(w);
                }
                *last = mode;
            };
        simulate_update(EditorMode::Play, &mut last_mode, &mut p_world, &world);
        assert_eq!(p_world.bodies.len(), 1);
        simulate_update(EditorMode::Play, &mut last_mode, &mut p_world, &world);
        assert_eq!(p_world.bodies.len(), 1);
    }

    #[test]
    fn test_physics_gravity_acceleration() {
        let mut p_world = PhysicsWorld::new();
        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE));
        let gravity = Vec3::new(0.0, -9.81, 0.0);
        p_world.apply_gravity(gravity, SIMULATION_DT);
        let expected = Vec3::new(0.0, -9.81 * SIMULATION_DT, 0.0);
        assert!((p_world.bodies[0].velocity.y - expected.y).abs() < 1e-5);
    }

    #[test]
    fn test_physics_gravity_anchored_ignored() {
        let mut p_world = PhysicsWorld::new();
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        body.anchored = true;
        p_world.bodies.push(body);
        p_world.apply_gravity(Vec3::new(0.0, -10.0, 0.0), 0.1);
        assert_eq!(p_world.bodies[0].velocity, Vec3::ZERO);
    }

    #[test]
    fn test_physics_gravity_participation_gating() {
        let mut p_world = PhysicsWorld::new();
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        body.gravity_participation = false;
        p_world.bodies.push(body);
        p_world.apply_gravity(Vec3::new(0.0, -10.0, 0.0), 0.1);
        assert_eq!(p_world.bodies[0].velocity, Vec3::ZERO);
    }

    #[test]
    fn test_physics_gravity_3d_vector() {
        let mut p_world = PhysicsWorld::new();
        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE));
        let gravity = Vec3::new(1.0, 2.0, -3.0);
        p_world.apply_gravity(gravity, 0.5);
        assert!((p_world.bodies[0].velocity - Vec3::new(0.5, 1.0, -1.5)).length() < 1e-5);
    }

    #[test]
    fn test_physics_position_integration() {
        let mut p_world = PhysicsWorld::new();
        let mut body =
            PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(10.0, 10.0, 10.0), Vec3::ONE);
        body.velocity = Vec3::new(1.0, 2.0, 3.0);
        p_world.bodies.push(body);
        p_world.integrate_positions(0.1);
        assert!((p_world.bodies[0].position - Vec3::new(10.1, 10.2, 10.3)).length() < 1e-5);
    }

    #[test]
    fn test_physics_integration_anchored_ignored() {
        let mut p_world = PhysicsWorld::new();
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        body.anchored = true;
        body.velocity = Vec3::ONE;
        p_world.bodies.push(body);
        p_world.integrate_positions(0.1);
        assert_eq!(p_world.bodies[0].position, Vec3::ZERO);
    }

    #[test]
    fn test_physics_gravity_integration_ordering() {
        let mut p_world = PhysicsWorld::new();
        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE));
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        let dt = 0.1;
        p_world.apply_gravity(gravity, dt);
        p_world.integrate_positions(dt);
        assert!((p_world.bodies[0].velocity.y - (-1.0)).abs() < 1e-5);
        assert!((p_world.bodies[0].position.y - (-0.1)).abs() < 1e-5);
    }

    #[test]
    fn test_static_collider_inclusion_rules() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let c1 = WorldCoord::new(0, 0, 0);
        world.set_cell(c1, CellType::Block);
        if let Some(cell) = world.get_mut(c1) {
            cell.anchored = true;
            cell.solid = true;
        }
        p_world.register_from_world(&world);
        assert!(
            p_world
                .static_colliders
                .iter()
                .any(|(_, pos)| *pos == Vec3::new(c1.x as f32, c1.y as f32, c1.z as f32))
        );
    }

    #[test]
    fn test_collision_detection() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        p_world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(1),
            0,
            Vec3::new(0.5, 0.5, 0.5),
            Vec3::ONE,
        ));
        assert_eq!(p_world.check_static_collisions().len(), 1);
    }

    #[test]
    fn test_physics_floor_collision_resolution() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 0.9, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(0.0, -10.0, 0.0);
        p_world.bodies.push(body);
        p_world.resolve_static_collisions();
        assert!(p_world.bodies[0].position.y >= 1.0);
        assert_eq!(p_world.bodies[0].velocity.y, 0.0);
    }

    #[test]
    fn test_physics_tangential_velocity_preserved() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 0.9, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(5.0, -10.0, 0.0);
        p_world.bodies.push(body);
        p_world.resolve_static_collisions();
        assert_eq!(p_world.bodies[0].velocity.y, 0.0);
        assert_eq!(p_world.bodies[0].velocity.x, 5.0);
    }

    #[test]
    fn test_physics_wall_collision_resolution() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(-0.1, 0.0, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(10.0, 0.0, 0.0);
        p_world.bodies.push(body);
        p_world.resolve_static_collisions();
        assert!((p_world.bodies[0].position.x - (-1.0)).abs() < 0.001);
        assert_eq!(p_world.bodies[0].velocity.x, 0.0);
    }

    #[test]
    fn test_physics_non_solid_ignored() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let c = WorldCoord::new(0, 0, 0);
        world.set_cell(c, CellType::Block);
        if let Some(cell) = world.get_mut(c) {
            cell.anchored = true;
            cell.solid = false;
        }
        p_world.register_from_world(&world);
        p_world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(1),
            0,
            Vec3::new(0.0, 0.9, 0.0),
            Vec3::ONE,
        ));
        p_world.resolve_static_collisions();
        assert_eq!(p_world.bodies[0].position.y, 0.9);
    }

    #[test]
    fn test_physics_touching_no_push() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        p_world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(1),
            0,
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::ONE,
        ));
        p_world.resolve_static_collisions();
        assert_eq!(p_world.bodies[0].position.y, 1.0);
    }

    #[test]
    fn test_physics_adjacent_floors_stable() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.5, 0.9, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(0.0, -10.0, 0.0);
        p_world.bodies.push(body);
        p_world.resolve_static_collisions();
        assert!((p_world.bodies[0].position.y - 1.0).abs() < 0.001);
        assert_eq!(p_world.bodies[0].velocity.y, 0.0);
    }

    #[test]
    fn test_physics_touching_adjacent_regression() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let pos = Vec3::new(0.0, 0.0, -1.0);
        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(1), 0, pos, Vec3::ONE));
        p_world.resolve_static_collisions();
        assert_eq!(p_world.bodies[0].position, pos);
    }

    #[test]
    fn test_physics_resting_contact_stability() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        let dt = SIMULATION_DT;
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 0.9, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(0.0, -10.0, 0.0);
        p_world.bodies.push(body);
        p_world.apply_gravity(gravity, dt);
        p_world.integrate_positions(dt);
        p_world.resolve_static_collisions();
        assert!(p_world.bodies[0].static_contact_normal.is_some());
        let pos_after_res = p_world.bodies[0].position;
        assert_eq!(p_world.bodies[0].velocity.y, 0.0);
        p_world.apply_gravity(gravity, dt);
        assert_eq!(p_world.bodies[0].velocity.y, 0.0);
        p_world.integrate_positions(dt);
        assert_eq!(p_world.bodies[0].position, pos_after_res);
        p_world.resolve_static_collisions();
        assert_eq!(p_world.bodies[0].position, pos_after_res);
    }

    #[test]
    fn test_physics_tangential_sliding_while_resting() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(5.0, 0.0, 0.0);
        body.static_contact_normal = Some(Vec3::Y);
        p_world.bodies.push(body);
        p_world.apply_gravity(Vec3::new(0.0, -10.0, 0.0), 0.1);
        p_world.integrate_positions(0.1);
        assert_eq!(p_world.bodies[0].velocity.y, 0.0);
        assert!((p_world.bodies[0].position.x - 0.5).abs() < 1e-5);
    }

    #[test]
    fn test_physics_contact_clearing_when_moving_away() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(0.0, 10.0, 0.0);
        body.static_contact_normal = Some(Vec3::Y);
        p_world.bodies.push(body);
        p_world.integrate_positions(0.1);
        p_world.resolve_static_collisions();
        assert!(p_world.bodies[0].static_contact_normal.is_none());
    }

    #[test]
    fn test_physics_arbitrary_gravity_contact() {
        let mut p_world = PhysicsWorld::new();
        p_world.add_static_collider(0, Vec3::ZERO);
        let body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(-0.15, 0.0, 0.0), Vec3::ONE);
        let gravity = Vec3::new(10.0, 0.0, 0.0);
        p_world.bodies.push(body);
        p_world.apply_gravity(gravity, 0.1);
        p_world.integrate_positions(0.1);
        p_world.resolve_static_collisions();
        assert_eq!(p_world.bodies[0].static_contact_normal, Some(Vec3::NEG_X));
        p_world.apply_gravity(gravity, 0.1);
        assert_eq!(p_world.bodies[0].velocity.x, 0.0);
    }

    #[test]
    fn test_physics_unsupported_no_sleep() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE));
        for _ in 0..100 {
            p_world.apply_gravity(gravity, SIMULATION_DT);
            p_world.integrate_positions(SIMULATION_DT);
            p_world.update_sleeping(gravity);
        }
        assert!(!p_world.bodies[0].is_sleeping);
        assert!(p_world.bodies[0].velocity.y < -5.0);
    }

    #[test]
    fn test_physics_static_floor_sleep() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        p_world.add_static_collider(0, Vec3::ZERO);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        body.static_contact_normal = Some(Vec3::Y);
        p_world.bodies.push(body);
        for _ in 0..60 {
            p_world.update_sleeping(gravity);
        }
        assert!(p_world.bodies[0].is_sleeping);
    }

    #[test]
    fn test_physics_dynamic_stack_sleep() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        let mut b1 = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        b1.is_sleeping = true;
        let mut b2 = PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::Y, Vec3::ONE);
        b2.dynamic_contact = Some((b1.id, Vec3::Y));
        p_world.bodies.push(b1);
        p_world.bodies.push(b2);
        for _ in 0..60 {
            p_world.update_sleeping(gravity);
        }
        assert!(p_world.bodies[1].is_sleeping);
    }

    #[test]
    fn test_physics_wake_on_support_wake() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        let b1 = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        let mut b2 = PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::Y, Vec3::ONE);
        b2.is_sleeping = true;
        b2.dynamic_contact = Some((b1.id, Vec3::Y));
        p_world.bodies.push(b1);
        p_world.bodies.push(b2);
        p_world.update_sleeping(gravity);
        assert!(!p_world.bodies[1].is_sleeping);
    }

    #[test]
    fn test_physics_wake_on_support_loss() {
        let mut p_world = PhysicsWorld::new();
        let mut b1 = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        b1.is_sleeping = true;
        let mut b2 = PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::Y, Vec3::ONE);
        b2.is_sleeping = true;
        b2.dynamic_contact = Some((b1.id, Vec3::Y));
        p_world.bodies.push(b1);
        p_world.bodies.push(b2);
        p_world.bodies[0].position.x += 10.0;
        p_world.refresh_dynamic_support();
        p_world.update_sleeping(Vec3::new(0.0, -10.0, 0.0));
        assert!(!p_world.bodies[1].is_sleeping);
    }

    #[test]
    fn test_physics_side_contact_no_support() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        body.static_contact_normal = Some(Vec3::X);
        p_world.bodies.push(body);
        for _ in 0..100 {
            p_world.update_sleeping(gravity);
        }
        assert!(!p_world.bodies[0].is_sleeping);
    }

    #[test]
    fn test_physics_arbitrary_gravity_sleep() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(10.0, 0.0, 0.0);
        let mut body = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE);
        body.static_contact_normal = Some(Vec3::NEG_X);
        p_world.bodies.push(body);
        for _ in 0..60 {
            p_world.update_sleeping(gravity);
        }
        assert!(p_world.bodies[0].is_sleeping);
    }

    #[test]
    fn test_physics_falling_stack_integration() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        let dt = SIMULATION_DT;

        p_world.add_static_collider(0, Vec3::ZERO);
        p_world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(1),
            0,
            Vec3::new(0.0, 5.0, 0.0),
            Vec3::ONE,
        ));
        p_world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(2),
            0,
            Vec3::new(0.0, 10.0, 0.0),
            Vec3::ONE,
        ));

        for _ in 0..300 {
            p_world.apply_gravity(gravity, dt);
            p_world.integrate_positions(dt);
            p_world.resolve_static_collisions();
            p_world.resolve_dynamic_collisions();
            p_world.refresh_dynamic_support();
            p_world.update_sleeping(gravity);
        }

        assert!(p_world.bodies[0].static_contact_normal.is_some());
        assert!(p_world.bodies[1].dynamic_contact.is_some());
        assert_eq!(
            p_world.bodies[1].dynamic_contact.unwrap().0,
            p_world.bodies[0].id
        );

        assert!(p_world.bodies[0].is_sleeping);
        assert!(p_world.bodies[1].is_sleeping);
    }

    #[test]
    fn test_physics_exact_touching_support() {
        let mut p_world = PhysicsWorld::new();
        let b1 = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        let b2 = PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::new(0.0, 1.0, 0.0), Vec3::ONE);
        p_world.bodies.push(b1);
        p_world.bodies.push(b2);

        let min_a = p_world.bodies[0].min_corner();
        let max_a = p_world.bodies[0].max_corner();
        let min_b = p_world.bodies[1].min_corner();
        let max_b = p_world.bodies[1].max_corner();
        assert!(!PhysicsWorld::aabb_overlap_static(
            min_a, max_a, min_b, max_b
        ));

        p_world.refresh_dynamic_support();
        assert!(p_world.bodies[1].dynamic_contact.is_some());
        let (support_id, normal) = p_world.bodies[1].dynamic_contact.unwrap();
        assert_eq!(support_id, p_world.bodies[0].id);
        assert_eq!(normal, Vec3::Y);
    }

    #[test]
    fn test_physics_x_axis_support() {
        let mut p_world = PhysicsWorld::new();
        let b1 = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        let b2 = PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::new(1.0, 0.0, 0.0), Vec3::ONE);
        p_world.bodies.push(b1);
        p_world.bodies.push(b2);
        p_world.refresh_dynamic_support();
        assert_eq!(
            p_world.bodies[1].dynamic_contact,
            Some((p_world.bodies[0].id, Vec3::X))
        );
    }

    #[test]
    fn test_physics_sideways_stack_sleep() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(10.0, 0.0, 0.0);
        p_world.add_static_collider(0, Vec3::ZERO);

        let mut b1 = PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::new(1.0, 0.0, 0.0), Vec3::ONE);
        b1.static_contact_normal = Some(Vec3::NEG_X);
        b1.is_sleeping = true;

        let mut b2 = PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::new(0.0, 0.0, 0.0), Vec3::ONE);
        b2.dynamic_contact = Some((b1.id, Vec3::NEG_X));

        p_world.bodies.push(b1);
        p_world.bodies.push(b2);

        for _ in 0..60 {
            p_world.update_sleeping(gravity);
        }

        assert!(
            p_world.bodies[1].is_sleeping,
            "Body should sleep when supported sideways against sideways gravity"
        );
    }

    #[test]
    fn test_physics_wake_on_support_movement_simulated() {
        let mut p_world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);

        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(1), 0, Vec3::ZERO, Vec3::ONE));
        p_world
            .bodies
            .push(PhysicsBody::new(PhysicsBodyId(2), 0, Vec3::Y, Vec3::ONE));
        p_world.bodies[0].is_sleeping = true;
        p_world.bodies[1].is_sleeping = true;
        p_world.bodies[1].dynamic_contact = Some((p_world.bodies[0].id, Vec3::Y));

        p_world.bodies[0].position.x += 5.0;

        p_world.refresh_dynamic_support();
        p_world.update_sleeping(gravity);

        assert!(
            !p_world.bodies[1].is_sleeping,
            "Upper body must wake when lower support is no longer touching"
        );
    }

    #[test]
    fn test_light_physics_exclusion() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Light);

        p_world.register_from_world(&world);

        assert_eq!(p_world.bodies.len(), 0);
        assert_eq!(p_world.static_colliders.len(), 0);
    }

    #[test]
    fn test_physics_body_captures_cell_color() {
        let mut world = World::new();
        let coord = WorldCoord::new(2, 5, 3);

        world.set_cell(coord, CellType::Block);

        let authored_color = Vec3::new(1.0, 0.0, 0.25);
        if let Some(cell) = world.get_mut(coord) {
            cell.anchored = false;
            cell.color_rgb = authored_color;
        }

        let mut p_world = PhysicsWorld::new();
        p_world.register_from_world(&world);

        assert_eq!(p_world.bodies.len(), 1);
        assert_eq!(p_world.bodies[0].color_rgb, authored_color);
    }

    #[test]
    fn test_physics_body_independence_after_registration() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);

        let original_color = Vec3::new(1.0, 1.0, 1.0);
        if let Some(cell) = world.get_mut(coord) {
            cell.anchored = false;
            cell.color_rgb = original_color;
            cell.visible = true;
        }

        let mut p_world = PhysicsWorld::new();
        p_world.register_from_world(&world);

        if let Some(cell) = world.get_mut(coord) {
            cell.color_rgb = Vec3::ZERO;
            cell.visible = false;
        }
        world.set_cell(coord, CellType::Empty);

        assert_eq!(p_world.bodies.len(), 1);
        assert_eq!(p_world.bodies[0].color_rgb, original_color);
        assert!(p_world.bodies[0].visible);
    }

    #[test]
    fn test_sync_no_dirty_no_scan() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        p_world.register_from_world(&world);

        world.physics_dirty_cells.clear();

        let initial_count = p_world.static_colliders.len();
        p_world.sync_with_world(&mut world);

        assert_eq!(p_world.static_colliders.len(), initial_count);
    }

    #[test]
    fn test_sync_offset_movement() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(10, 10, 10);
        world.set_cell(coord, CellType::Block);
        p_world.register_from_world(&world);

        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 5.0, 0.0));
        p_world.sync_with_world(&mut world);

        let (_, pos) = p_world.static_colliders[0];
        assert_eq!(pos, Vec3::new(10.0, 15.0, 10.0));
    }

    #[test]
    fn test_sync_offset_reset() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);
        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 10.0, 0.0));
        p_world.register_from_world(&world);

        world.clear_runtime_state();
        p_world.sync_with_world(&mut world);

        let (_, pos) = p_world.static_colliders[0];
        assert_eq!(pos, Vec3::ZERO);
    }

    #[test]
    fn test_sync_solid_toggle() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);
        p_world.register_from_world(&world);
        assert_eq!(p_world.static_colliders.len(), 1);

        world.set_cell_solid_runtime(coord, false);
        p_world.sync_with_world(&mut world);
        assert_eq!(p_world.static_colliders.len(), 0);

        world.set_cell_solid_runtime(coord, true);
        p_world.sync_with_world(&mut world);
        assert_eq!(p_world.static_colliders.len(), 1);
    }

    #[test]
    fn test_sync_anchored_toggle() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);
        p_world.register_from_world(&world);
        assert_eq!(p_world.static_colliders.len(), 1);
        assert_eq!(p_world.bodies.len(), 0);

        world.set_cell_anchored_runtime(coord, false);
        p_world.sync_with_world(&mut world);
        assert_eq!(p_world.static_colliders.len(), 0);
        assert_eq!(p_world.bodies.len(), 1);

        world.set_cell_anchored_runtime(coord, true);
        p_world.sync_with_world(&mut world);
        assert_eq!(p_world.static_colliders.len(), 1);
        assert_eq!(p_world.bodies.len(), 0);
    }

    #[test]
    fn test_sync_multiple_mutations() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);
        p_world.register_from_world(&world);

        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 1.0, 0.0));
        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 2.0, 0.0));
        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 3.0, 0.0));

        p_world.sync_with_world(&mut world);
        assert_eq!(p_world.static_colliders[0].1, Vec3::new(0.0, 3.0, 0.0));
    }

    #[test]
    fn test_sync_deleted_cell() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);
        let cell_id = world.get(coord).unwrap().id;
        p_world.register_from_world(&world);
        assert_eq!(p_world.static_colliders.len(), 1);

        world.mark_physics_dirty(cell_id);
        world.set_cell(coord, CellType::Empty);

        p_world.sync_with_world(&mut world);

        assert_eq!(p_world.static_colliders.len(), 0);
    }

    #[test]
    fn test_sync_adding_cell() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();

        p_world.register_from_world(&world);
        assert_eq!(p_world.static_colliders.len(), 0);

        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::Block);

        p_world.sync_with_world(&mut world);

        assert_eq!(p_world.static_colliders.len(), 1);
    }

    #[test]
    fn test_sync_anchor_toggle_preserves_color() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(4, 5, 6);
        let color = Vec3::new(0.3, 0.7, 0.2);

        world.set_cell(coord, CellType::Block);
        if let Some(cell) = world.get_mut(coord) {
            cell.color_rgb = color;
        }

        p_world.register_from_world(&world);

        world.set_cell_anchored_runtime(coord, false);
        p_world.sync_with_world(&mut world);

        assert_eq!(p_world.bodies.len(), 1);
        assert_eq!(p_world.bodies[0].color_rgb, color);
    }

    #[test]
    fn test_sync_runtime_appearance_visible_state() {
        let mut world = World::new();
        let mut p_world = PhysicsWorld::new();
        let coord = WorldCoord::new(2, 3, 4);

        world.set_cell(coord, CellType::Block);
        if let Some(cell) = world.get_mut(coord) {
            cell.anchored = false;
            cell.visible = true;
        }

        p_world.register_from_world(&world);

        world.set_cell_visible_runtime(coord, false);
        p_world.sync_with_world(&mut world);

        assert_eq!(p_world.bodies.len(), 1);
        assert!(!p_world.bodies[0].visible);
    }

    #[test]
    fn runtime_authored_actor_bootstrap_copies_collider_size_and_offset() {
        let mut actor = crate::world::Actor::new(1, "Buggy", Vec3::ZERO);
        actor.physics.enabled = true;
        actor.physics.collision_size = Vec3::new(3.0, 1.5, 5.0);
        actor.physics.collision_offset = Vec3::new(0.0, 0.75, -0.2);

        let mut entities = EntityManager::new();
        let entity_id = entities.create_entity("Buggy");

        if let Some(entity) = entities.get_entity_mut(entity_id) {
            entity.physics.size = actor.physics.collision_size;
            entity.physics.offset = actor.physics.collision_offset;
        }

        let physics = entities.get_physics(entity_id).unwrap();
        assert_eq!(physics.size, Vec3::new(3.0, 1.5, 5.0));
        assert_eq!(physics.offset, Vec3::new(0.0, 0.75, -0.2));
    }

    #[test]
    fn entity_collision_uses_collision_size() {
        let p_world = PhysicsWorld::new();
        let mut entities = EntityManager::new();

        let e1 = entities.create_entity("BigBox");
        let e2 = entities.create_entity("SmallBox");

        entities.set_position(e1, Vec3::ZERO);
        entities.set_position(e2, Vec3::new(2.5, 0.0, 0.0));

        {
            let p1 = entities.get_physics_mut(e1).unwrap();
            p1.enabled = true;
            p1.collision_enabled = true;
            p1.solid = true;
            p1.size = Vec3::new(4.0, 2.0, 2.0);
        }

        {
            let p2 = entities.get_physics_mut(e2).unwrap();
            p2.enabled = true;
            p2.collision_enabled = true;
            p2.solid = true;
            p2.size = Vec3::new(2.0, 2.0, 2.0);
        }

        let contacts = p_world.process_entity_contacts_and_collisions(&mut entities, None);
        assert!(
            contacts.current.contains(&EntityContactPair::new(e1, e2)),
            "Entities with size 4.0 and 2.0 at distance 2.5 must overlap"
        );
    }

    #[test]
    fn entity_collision_offset_changes_actual_contact_position() {
        let p_world = PhysicsWorld::new();
        let mut entities = EntityManager::new();

        let e1 = entities.create_entity("OffsetActor");
        let e2 = entities.create_entity("TestCellActor");

        entities.set_position(e1, Vec3::ZERO);
        {
            let p1 = entities.get_physics_mut(e1).unwrap();
            p1.enabled = true;
            p1.collision_enabled = true;
            p1.solid = true;
            p1.size = Vec3::new(1.0, 2.0, 1.0);
            p1.offset = Vec3::new(0.0, 1.0, 0.0);
        }

        entities.set_position(e2, Vec3::new(0.0, -0.8, 0.0));
        {
            let p2 = entities.get_physics_mut(e2).unwrap();
            p2.enabled = true;
            p2.collision_enabled = true;
            p2.solid = true;
            p2.size = Vec3::ONE;
            p2.offset = Vec3::ZERO;
        }

        let contacts = p_world.process_entity_contacts_and_collisions(&mut entities, None);
        assert!(
            !contacts.current.contains(&EntityContactPair::new(e1, e2)),
            "Actor 2 at Y=-0.8 should NOT overlap Actor 1 collider (Y: 0..2)"
        );

        entities.set_position(e2, Vec3::new(0.0, 0.5, 0.0));
        let contacts2 = p_world.process_entity_contacts_and_collisions(&mut entities, None);
        assert!(
            contacts2.current.contains(&EntityContactPair::new(e1, e2)),
            "Actor 2 at Y=0.5 MUST overlap Actor 1 offset collider (Y: 0..2)"
        );
    }

    #[test]
    fn anchored_entity_collider_blocks_character() {
        let p_world = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let mut characters = CharacterSystem::new();

        let actor_entity = entities.create_entity("SolidActor");
        entities.set_position(actor_entity, Vec3::ZERO);
        {
            let physics = entities.get_physics_mut(actor_entity).unwrap();
            physics.enabled = true;
            physics.collision_enabled = true;
            physics.solid = true;
            physics.anchored = true;
            physics.size = Vec3::new(2.0, 2.0, 2.0);
            physics.offset = Vec3::new(0.0, 1.0, 0.0);
        }

        let player_character = characters.spawn_character(Vec3::new(0.8, 0.0, 0.0), Some("Rogue"));
        let player_entity = entities.create_entity("Player");
        characters.associate_entity(player_entity, player_character);

        let contacts =
            p_world.process_entity_contacts_and_collisions(&mut entities, Some(&mut characters));

        assert!(
            contacts
                .current
                .contains(&EntityContactPair::new(actor_entity, player_entity)),
            "contact must be detected between Actor and Player"
        );

        let player = characters.get_character_for_entity(player_entity).unwrap();

        assert!(
            player.transform.position.x > 0.8,
            "solid anchored Actor must push the Player out (got X={})",
            player.transform.position.x
        );
    }

    #[test]
    fn nonsolid_entity_reports_contact_without_blocking_character() {
        let p_world = PhysicsWorld::new();
        let mut entities = EntityManager::new();
        let mut characters = CharacterSystem::new();

        let actor_entity = entities.create_entity("TriggerActor");
        entities.set_position(actor_entity, Vec3::ZERO);
        {
            let physics = entities.get_physics_mut(actor_entity).unwrap();
            physics.enabled = true;
            physics.collision_enabled = true;
            physics.solid = false;
            physics.anchored = true;
            physics.size = Vec3::new(2.0, 2.0, 2.0);
            physics.offset = Vec3::new(0.0, 1.0, 0.0);
        }

        let player_character = characters.spawn_character(Vec3::new(0.8, 0.0, 0.0), Some("Rogue"));
        let player_entity = entities.create_entity("Player");
        characters.associate_entity(player_entity, player_character);

        let initial_x = 0.8;

        let contacts =
            p_world.process_entity_contacts_and_collisions(&mut entities, Some(&mut characters));

        assert!(
            contacts
                .current
                .contains(&EntityContactPair::new(actor_entity, player_entity)),
            "contact must be reported for trigger Actor"
        );

        let player = characters.get_character_for_entity(player_entity).unwrap();

        assert_eq!(
            player.transform.position.x, initial_x,
            "non-solid Actor must not change the Player position"
        );
    }

    #[test]
    fn staggered_10x10_fall_blocks_do_not_tunnel_through_platform() {
        use crate::character::CharacterSystem;
        use crate::engine::attachment::AttachmentSystem;
        use crate::engine::entity::EntityManager;
        use crate::engine::mouse::MouseController;
        use crate::engine::physics::{PhysicsWorld, SIMULATION_DT};
        use crate::engine::ui::RuntimeUi;
        use crate::renderer::camera::GameplayCamera;
        use crate::scripting::api::HostContext;
        use crate::scripting::host::ScriptHostBridge;
        use crate::scripting::scene::tests::create_scene_with_path;
        use crate::scripting::value::{DynamicProperty, HandleKind};
        use crate::world::{CellType, World, WorldCoord};
        use glam::Vec3;
        use std::collections::{BTreeMap, HashMap, HashSet};
        use std::sync::Arc;

        let mut world = World::new();
        let mut physics_world = PhysicsWorld::new();
        let mut character_system = CharacterSystem::new();
        let mut attachment_system = AttachmentSystem::default();
        let mut actor_system = crate::actor::ActorSystem::new();
        let mut entity_manager = EntityManager::new();
        let mut gameplay_camera = GameplayCamera::new();
        let mut mouse = MouseController::default();
        let mut runtime_ui = RuntimeUi::new();

        let mut script_dynamic_properties: HashMap<
            (HandleKind, u64),
            BTreeMap<String, DynamicProperty>,
        > = HashMap::new();
        let mut script_pending_events = Vec::new();
        let mut script_test_results = BTreeMap::new();
        let mut script_pending_enable = Vec::new();
        let mut script_pending_disable = Vec::new();
        let mut script_pending_spawns = Vec::new();
        let mut script_pending_destroys = Vec::new();

        // 1. Build a 10x10 anchored platform at y = 0
        for x in 0..10 {
            for z in 0..10 {
                let coord = WorldCoord::new(x, 0, z);
                world.set_cell(coord, CellType::Block);
                let cell = world.get_mut(coord).unwrap();
                cell.anchored = true;
                cell.solid = true;
            }
        }

        // 2. Build 100 anchored fall_blocks at y = 4
        let mut faller_coords = Vec::new();
        let mut faller_cell_ids = Vec::new();
        for x in 0..10 {
            for z in 0..10 {
                let coord = WorldCoord::new(x, 4, z);
                let cell_id = world.set_cell(coord, CellType::Block);
                let cell = world.get_mut(coord).unwrap();
                cell.anchored = true;
                cell.solid = true;
                cell.entity_identity = Some("fall_blocks".to_string());
                cell.collision_events_enabled = true;
                faller_coords.push((x, z));
                faller_cell_ids.push(cell_id);
            }
        }

        physics_world.register_from_world(&world);
        world.physics_dirty_cells.clear();

        // 3. Spawn player character & associate player entity
        let player_char_id =
            character_system.spawn_character(Vec3::new(-100.0, 0.0, -100.0), Some("Rogue"));
        let player_entity = entity_manager.create_entity("Player");
        character_system.associate_entity(player_entity, player_char_id);

        // 4. Create and start AeoScript scene with exact script
        let source = r#"
            const fallers = find("fall_blocks")
            count = 0

            for fallblock in fallers {
                fallblock.on_touch = fn(thing) {
                    fallblock.anchored = false
                }
            }
        "#;

        let mut script_scene = {
            let mut bridge = ScriptHostBridge {
                entity_manager: &mut entity_manager,
                attachment_system: Some(&mut attachment_system),
                world: &mut world,
                mouse: &mut mouse,
                dynamic_properties: &mut script_dynamic_properties,
                pending_events: &mut script_pending_events,
                test_results: &mut script_test_results,
                pending_enable_scripts: &mut script_pending_enable,
                pending_disable_scripts: &mut script_pending_disable,
                runtime_ui: &mut runtime_ui,
                viewport_size: [1280.0, 720.0],
                keys_down: None,
                move_input: glam::Vec2::ZERO,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: Some(&mut character_system),
                actor_system: Some(&mut actor_system),
                gameplay_camera: Some(&mut gameplay_camera),
                valid_entity_declarations: Some(Arc::new(HashSet::new())),
                pending_spawns: &mut script_pending_spawns,
                pending_destroys: &mut script_pending_destroys,
                project_path: None,
                save_state: None,
            };
            let mut context = HostContext {
                delta_time: SIMULATION_DT as f64,
                engine: &mut bridge,
            };
            let mut scene = create_scene_with_path(source, "scripts/main.aeo", &mut context);
            scene.start(&mut context).expect("Scene should start");
            scene
                .update(0.0, &mut context)
                .expect("Scene initial update should succeed");
            scene
        };

        // Verify find("fall_blocks") found all 100 blocks
        let found_fallers = {
            let bridge = ScriptHostBridge {
                entity_manager: &mut entity_manager,
                attachment_system: Some(&mut attachment_system),
                world: &mut world,
                mouse: &mut mouse,
                dynamic_properties: &mut script_dynamic_properties,
                pending_events: &mut script_pending_events,
                test_results: &mut script_test_results,
                pending_enable_scripts: &mut script_pending_enable,
                pending_disable_scripts: &mut script_pending_disable,
                runtime_ui: &mut runtime_ui,
                viewport_size: [1280.0, 720.0],
                keys_down: None,
                move_input: glam::Vec2::ZERO,
                jump_requested: false,
                orbit_delta: [0.0, 0.0],
                character_system: Some(&mut character_system),
                actor_system: Some(&mut actor_system),
                gameplay_camera: Some(&mut gameplay_camera),
                valid_entity_declarations: Some(script_scene.valid_entity_declarations.clone()),
                pending_spawns: &mut script_pending_spawns,
                pending_destroys: &mut script_pending_destroys,
                project_path: None,
                save_state: None,
            };
            bridge.find_objects("fall_blocks")
        };
        assert_eq!(
            found_fallers.len(),
            100,
            "find('fall_blocks') must find all 100 authored Blocks"
        );

        // 5. Simulation loop using the EXACT Play runtime tick
        let total_fallers = 100;
        let steps_per_touch = 12; // 0.2s @ 60Hz
        let settle_steps = 600; // 10s extra for all blocks to fall and settle
        let total_steps = total_fallers * steps_per_touch + settle_steps;

        let dt = SIMULATION_DT;
        let mut first_invalid_captured = false;

        for step in 0..total_steps {
            // Touch trigger every 12 frames (0.2s apart) via real character contact
            let touch_index = step / steps_per_touch;
            if step % steps_per_touch == 0 && touch_index < total_fallers {
                let (fx, fz) = faller_coords[touch_index];
                if let Some(char) = character_system.get_character_for_entity_mut(player_entity) {
                    char.transform.position = Vec3::new(fx as f32 + 0.5, 4.0, fz as f32 + 0.5);
                }
            } else {
                if let Some(char) = character_system.get_character_for_entity_mut(player_entity) {
                    char.transform.position = Vec3::new(-100.0, -100.0, -100.0);
                }
            }

            // --- Play Runtime Tick ---
            // Step 1: Sync player entity position from character
            character_system.sync_entity_positions(&mut entity_manager);

            // Step 2: Top-level scripting update
            {
                let mut bridge = ScriptHostBridge {
                    entity_manager: &mut entity_manager,
                    attachment_system: Some(&mut attachment_system),
                    world: &mut world,
                    mouse: &mut mouse,
                    dynamic_properties: &mut script_dynamic_properties,
                    pending_events: &mut script_pending_events,
                    test_results: &mut script_test_results,
                    pending_enable_scripts: &mut script_pending_enable,
                    pending_disable_scripts: &mut script_pending_disable,
                    runtime_ui: &mut runtime_ui,
                    viewport_size: [1280.0, 720.0],
                    keys_down: None,
                    move_input: glam::Vec2::ZERO,
                    jump_requested: false,
                    orbit_delta: [0.0, 0.0],
                    character_system: Some(&mut character_system),
                    actor_system: Some(&mut actor_system),
                    gameplay_camera: Some(&mut gameplay_camera),
                    valid_entity_declarations: Some(script_scene.valid_entity_declarations.clone()),
                    pending_spawns: &mut script_pending_spawns,
                    pending_destroys: &mut script_pending_destroys,
                    project_path: None,
                    save_state: None,
                };
                let mut context = HostContext {
                    delta_time: dt as f64,
                    engine: &mut bridge,
                };
                let _ = script_scene.update(dt, &mut context);
            }

            // Step 3: Reconcile dirty cells into physics_world
            physics_world.sync_with_world(&mut world);

            // Step 4: Fixed-step physics
            let gravity = world.gravity;
            let bodies_before: Vec<(u64, Vec3, Vec3)> = physics_world
                .bodies
                .iter()
                .map(|b| (b.cell_id, b.position, b.velocity))
                .collect();

            physics_world.step_simulation(gravity, dt);

            let character_entities: HashSet<_> = character_system.associated_entity_ids().collect();
            physics_world.step_entities_excluding(
                &mut entity_manager,
                gravity,
                dt,
                &character_entities,
            );
            entity_manager.update_animations(dt);

            let attached_children: HashSet<_> =
                attachment_system.attached_children().into_iter().collect();
            let contacted = character_system.update_scripted_excluding(
                &world,
                &mut physics_world,
                dt,
                &attached_children,
            );

            character_system.sync_entity_positions(&mut entity_manager);

            let entity_contacts = physics_world.process_entity_contacts_and_collisions(
                &mut entity_manager,
                Some(&mut character_system),
            );

            {
                let mut bridge = ScriptHostBridge {
                    entity_manager: &mut entity_manager,
                    attachment_system: Some(&mut attachment_system),
                    world: &mut world,
                    mouse: &mut mouse,
                    dynamic_properties: &mut script_dynamic_properties,
                    pending_events: &mut script_pending_events,
                    test_results: &mut script_test_results,
                    pending_enable_scripts: &mut script_pending_enable,
                    pending_disable_scripts: &mut script_pending_disable,
                    runtime_ui: &mut runtime_ui,
                    viewport_size: [1280.0, 720.0],
                    keys_down: None,
                    move_input: glam::Vec2::ZERO,
                    jump_requested: false,
                    orbit_delta: [0.0, 0.0],
                    character_system: Some(&mut character_system),
                    actor_system: Some(&mut actor_system),
                    gameplay_camera: Some(&mut gameplay_camera),
                    valid_entity_declarations: Some(script_scene.valid_entity_declarations.clone()),
                    pending_spawns: &mut script_pending_spawns,
                    pending_destroys: &mut script_pending_destroys,
                    project_path: None,
                    save_state: None,
                };
                let mut context = HostContext {
                    delta_time: dt as f64,
                    engine: &mut bridge,
                };
                let _ = script_scene.on_entity_contacts(&entity_contacts, &mut context);
            }

            attachment_system.update(&mut entity_manager, &mut character_system);
            character_system.sync_entity_positions(&mut entity_manager);

            // Step 5: Real contact dispatch to AeoScript
            if !contacted.is_empty() {
                let mut bridge = ScriptHostBridge {
                    entity_manager: &mut entity_manager,
                    attachment_system: Some(&mut attachment_system),
                    world: &mut world,
                    mouse: &mut mouse,
                    dynamic_properties: &mut script_dynamic_properties,
                    pending_events: &mut script_pending_events,
                    test_results: &mut script_test_results,
                    pending_enable_scripts: &mut script_pending_enable,
                    pending_disable_scripts: &mut script_pending_disable,
                    runtime_ui: &mut runtime_ui,
                    viewport_size: [1280.0, 720.0],
                    keys_down: None,
                    move_input: glam::Vec2::ZERO,
                    jump_requested: false,
                    orbit_delta: [0.0, 0.0],
                    character_system: Some(&mut character_system),
                    actor_system: Some(&mut actor_system),
                    gameplay_camera: Some(&mut gameplay_camera),
                    valid_entity_declarations: Some(script_scene.valid_entity_declarations.clone()),
                    pending_spawns: &mut script_pending_spawns,
                    pending_destroys: &mut script_pending_destroys,
                    project_path: None,
                    save_state: None,
                };
                let mut context = HostContext {
                    delta_time: dt as f64,
                    engine: &mut bridge,
                };
                let _ = script_scene.on_character_contact(&contacted, &mut context);
            }

            // --- Instrumentation & First Invalid Transition Check ---
            for body in &physics_world.bodies {
                let (exp_x, exp_z) = if let Some(coord) = world.resolve_cell_id(body.cell_id) {
                    (coord.x as f32, coord.z as f32)
                } else {
                    (body.position.x, body.position.z)
                };

                let invalid_y = body.position.y < 0.99;
                let invalid_xz = (body.position.x - exp_x).abs() > 0.05
                    || (body.position.z - exp_z).abs() > 0.05;

                if (invalid_y || invalid_xz) && !first_invalid_captured {
                    first_invalid_captured = true;
                    let before = bodies_before
                        .iter()
                        .find(|(cid, _, _)| *cid == body.cell_id);

                    println!("========== FIRST INVALID TRANSITION CAPTURED ==========");
                    println!("Physics step / frame: {}", step);
                    println!("Cell ID: {}", body.cell_id);
                    if let Some(coord) = world.resolve_cell_id(body.cell_id) {
                        println!(
                            "Authored WorldCoord: ({}, {}, {})",
                            coord.x, coord.y, coord.z
                        );
                    }
                    println!(
                        "Runtime position (World): {:?}",
                        world
                            .resolve_cell_id(body.cell_id)
                            .map(|c| Vec3::new(c.x as f32, c.y as f32, c.z as f32))
                    );
                    println!("PhysicsBody position AFTER step: {:?}", body.position);
                    println!("Velocity AFTER step: {:?}", body.velocity);
                    println!("Anchored state: {}", body.anchored);
                    println!("Static contact normal: {:?}", body.static_contact_normal);
                    println!("Dynamic contact support: {:?}", body.dynamic_contact);
                    if let Some((_, pos_before, vel_before)) = before {
                        println!("Position BEFORE step: {:?}", pos_before);
                        println!("Velocity BEFORE step: {:?}", vel_before);
                        println!("Position delta in step: {:?}", body.position - *pos_before);
                    }
                    println!(
                        "Trigger condition: invalid_y={}, invalid_xz={}",
                        invalid_y, invalid_xz
                    );
                    println!("=======================================================");
                }
            }
        }

        // Final assertion: verify every faller landed on top of the platform at y = 1.0 (within epsilon)
        for (idx, cell_id) in faller_cell_ids.iter().enumerate() {
            let (x, z) = faller_coords[idx];
            let body = physics_world.bodies.iter().find(|b| b.cell_id == *cell_id);
            let pos = body
                .map(|b| b.position)
                .unwrap_or_else(|| Vec3::new(x as f32, 4.0, z as f32));

            assert!(
                pos.y >= 0.99,
                "faller {} at grid ({}, {}) fell through platform: pos={:?}",
                cell_id,
                x,
                z,
                pos
            );
            assert!(
                (pos.x - x as f32).abs() < 0.05 && (pos.z - z as f32).abs() < 0.05,
                "faller {} at grid ({}, {}) drifted laterally: pos={:?}",
                cell_id,
                x,
                z,
                pos
            );
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.5-RF-11 | 2026-10-05 21:44 -05:00
    // Exercise the shared runtime step with focused single-body, swept-crossing, matrix, and stack cases.
    fn step_cell_physics_like_runtime(world: &mut PhysicsWorld, gravity: Vec3) {
        world.step_simulation(gravity, SIMULATION_DT);
    }

    #[test]
    fn rf11_single_falling_body_remains_on_floor_without_lateral_drift() {
        let mut world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        world.add_static_collider(1, Vec3::ZERO);
        world.bodies.push(PhysicsBody::new(
            PhysicsBodyId(1),
            2,
            Vec3::new(0.0, 4.0, 0.0),
            Vec3::ONE,
        ));

        for _ in 0..1_200 {
            step_cell_physics_like_runtime(&mut world, gravity);
        }

        let body = &world.bodies[0];
        assert!(
            body.position.y >= 0.999,
            "single faller crossed the floor: {body:?}"
        );
        assert!(
            body.position.x.abs() < 0.001 && body.position.z.abs() < 0.001,
            "single faller drifted laterally: {body:?}"
        );
    }

    #[test]
    fn rf11_single_body_cannot_cross_floor_between_fixed_steps() {
        let mut world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        world.add_static_collider(1, Vec3::ZERO);

        let mut body = PhysicsBody::new(PhysicsBodyId(1), 2, Vec3::new(0.0, 2.0, 0.0), Vec3::ONE);
        body.velocity = Vec3::new(0.0, -180.0, 0.0);
        world.bodies.push(body);

        let before = (world.bodies[0].position, world.bodies[0].velocity);
        step_cell_physics_like_runtime(&mut world, gravity);
        let after = &world.bodies[0];

        assert!(
            after.position.y >= 0.999,
            "body crossed the anchored floor between fixed steps; before={before:?}, after_pos={:?}, after_vel={:?}, static_contact={:?}",
            after.position,
            after.velocity,
            after.static_contact_normal
        );
        assert!(after.position.x.abs() < 0.001 && after.position.z.abs() < 0.001);
    }

    #[test]
    fn rf11_staggered_10x10_solver_matrix_stays_above_platform() {
        let mut world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);

        for x in 0..10 {
            for z in 0..10 {
                world.add_static_collider(
                    (x * 10 + z + 1) as u64,
                    Vec3::new(x as f32, 0.0, z as f32),
                );
            }
        }

        let mut next_body = 0;
        for step in 0..1_800 {
            if step % 12 == 0 && next_body < 100 {
                let x = next_body / 10;
                let z = next_body % 10;
                world.bodies.push(PhysicsBody::new(
                    PhysicsBodyId((next_body + 1) as u64),
                    (next_body + 101) as u64,
                    Vec3::new(x as f32, 4.0, z as f32),
                    Vec3::ONE,
                ));
                next_body += 1;
            }

            step_cell_physics_like_runtime(&mut world, gravity);

            for body in &world.bodies {
                let authored_index = (body.id.0 - 1) as usize;
                let authored_x = (authored_index / 10) as f32;
                let authored_z = (authored_index % 10) as f32;
                assert!(
                    body.position.y >= 0.999,
                    "faller {} crossed the floor at step {step}: {body:?}",
                    body.id.0
                );
                assert!(
                    (body.position.x - authored_x).abs() < 0.001
                        && (body.position.z - authored_z).abs() < 0.001,
                    "faller {} drifted from ({authored_x}, {authored_z}) at step {step}: {body:?}",
                    body.id.0
                );
            }
        }
    }

    #[test]
    fn rf11_contact_heavy_stack_never_drives_bottom_body_through_floor() {
        let mut world = PhysicsWorld::new();
        let gravity = Vec3::new(0.0, -10.0, 0.0);
        world.add_static_collider(1, Vec3::ZERO);

        for index in (0..10).rev() {
            world.bodies.push(PhysicsBody::new(
                PhysicsBodyId((index + 1) as u64),
                (index + 2) as u64,
                Vec3::new(0.0, 4.0 + index as f32, 0.0),
                Vec3::ONE,
            ));
        }

        for step in 0..1_200 {
            let before: Vec<(Vec3, Vec3)> = world
                .bodies
                .iter()
                .map(|body| (body.position, body.velocity))
                .collect();
            step_cell_physics_like_runtime(&mut world, gravity);

            for (index, body) in world.bodies.iter().enumerate() {
                assert!(
                    body.position.y >= 0.999,
                    "stack body {} first crossed the floor at step {step}; before={:?}, after_pos={:?}, after_vel={:?}, static_contact={:?}, dynamic_contact={:?}",
                    body.id.0,
                    before[index],
                    body.position,
                    body.velocity,
                    body.static_contact_normal,
                    body.dynamic_contact
                );
                assert!(
                    body.position.x.abs() < 0.001 && body.position.z.abs() < 0.001,
                    "stack body {} drifted laterally at step {step}: {body:?}",
                    body.id.0
                );
            }
        }

        for body in &world.bodies {
            assert!(
                body.is_sleeping,
                "settled stack body did not sleep: {body:?}"
            );
            assert!(
                (body.position.y - body.id.0 as f32).abs() < 0.001,
                "settled stack body is not on its support level: {body:?}"
            );
        }
    }
}
