#[cfg(test)]
mod tests {
    use super::super::CharacterSystem;
    use crate::character::animation::AnimationState;
    use crate::character::character::Character;
    use crate::character::movement::{self, JUMP_IMPULSE};
    use crate::engine::entity::{EntityId, EntityManager};
    use crate::world::{CellType, World, WorldCoord};
    use glam::{Mat4, Quat, Vec2, Vec3};

    #[test]
    fn test_character_runtime_starts_empty() {
        let system = CharacterSystem::new();

        assert!(!system.has_characters());
        assert_eq!(system.get_active_characters().count(), 0);
        assert!(system.get_active_player().is_none());
    }

    #[test]
    fn explicit_active_player_never_falls_back_to_an_arbitrary_actor() {
        let mut system = CharacterSystem::new();
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:25 -05:00
        // Ownership semantics do not depend on an unrelated optional sword package.
        let item_id = system.spawn_character(Vec3::ZERO, Some("Rogue"));

        assert!(system.get_active_player().is_none());
        assert!(system.get_active_player_mut().is_none());
        assert!(!system.set_active_position(Vec3::ONE));
        assert_eq!(system.explicit_active_player_id(), None);

        system.active_player_id = Some(item_id);
        assert_eq!(system.explicit_active_player_id(), Some(item_id));
        system.active_player_id = Some(item_id + 1);
        assert!(system.get_active_player().is_none());
        assert!(system.get_active_player_mut().is_none());
    }

    #[test]
    fn test_adding_character_stores_it() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        system.spawn_player(&world, None);

        assert!(system.has_characters());
        assert_eq!(system.get_active_characters().count(), 1);
        assert!(system.get_active_player().is_some());
    }

    #[test]
    fn test_runtime_ids_remain_unique() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        world.set_cell(WorldCoord::new(5, 0, 5), CellType::SpawnPoint);

        system.spawn_player(&world, None);

        let id1 = system.get_active_characters().next().unwrap().id;

        system.spawn_player(&world, None);

        let id2 = system
            .get_active_characters()
            .find(|character| character.id != id1)
            .unwrap()
            .id;

        assert_ne!(id1, id2);
    }

    #[test]
    fn test_clear_removes_all_characters() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        system.spawn_player(&world, None);

        assert!(system.has_characters());

        system.clear();

        assert!(!system.has_characters());
        assert!(system.get_active_player().is_none());
        assert_eq!(system.next_id, 1);
    }

    #[test]
    fn test_character_teleport() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::SpawnPoint);

        system.spawn_player(&world, None);

        let player = system.get_active_player_mut().unwrap();

        player.movement.velocity = Vec3::new(4.0, -8.0, 2.0);

        player.movement.is_grounded = true;

        let target = Vec3::new(10.0, 20.0, 30.0);

        assert!(system.set_active_position(target));

        let player = system.get_active_player().unwrap();

        assert_eq!(player.transform.position, target);

        assert_eq!(player.movement.velocity, Vec3::ZERO);

        assert!(!player.movement.is_grounded);
    }

    #[test]
    fn test_character_gravity() {
        let mut system = CharacterSystem::new();
        let world = World::new();

        let character = Character::new(1, Vec3::new(0.0, 10.0, 0.0));

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        for _ in 0..60 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!(updated.transform.position.y < 10.0);

        assert!(updated.movement.velocity.y < 0.0);
    }

    #[test]
    fn test_character_horizontal_movement() {
        let mut system = CharacterSystem::new();
        let world = World::new();

        let character = Character::new(1, Vec3::ZERO);

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::new(1.0, 0.0), false);

        let updated = system.get_active_player().unwrap();

        assert!(updated.transform.position.x > 0.0);

        assert_eq!(updated.animation.current_state, AnimationState::Walk);

        assert!(updated.animation_controller.blend_weight() > 0.0);
    }

    #[test]
    fn test_scripted_horizontal_movement_preserves_velocity() {
        let mut system = CharacterSystem::new();
        let world = World::new();

        let character = Character::new(1, Vec3::ZERO);

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        {
            let player = system.get_active_player_mut().unwrap();

            player.movement.velocity.x = 5.0;

            player.movement.velocity.z = -2.0;
        }

        let _ = system.update_scripted(&world, &mut physics_world, 0.1);

        let updated = system.get_active_player().unwrap();

        assert!(updated.transform.position.x > 0.49);

        assert!(updated.transform.position.z < -0.19);

        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:31 -05:00
        // Rogue has a Run clip and a speed-5 threshold: sqrt(5²+2²) runs.
        assert_eq!(updated.animation.current_state, AnimationState::Run);
    }

    #[test]
    fn test_character_ground_collision() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        let character = Character::new(1, Vec3::new(0.0, 1.5, 0.0));

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        for _ in 0..60 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!((updated.transform.position.y - 1.0).abs() < 0.01);

        assert!(updated.movement.is_grounded);
    }

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
    #[test]
    fn character_grounds_on_generated_smooth_and_block_terrain() {
        for mode in [
            crate::world::TerrainMode::Smooth,
            crate::world::TerrainMode::Block,
        ] {
            let mut system = CharacterSystem::new();
            let mut world = World::new();
            world.terrain.config.mode = mode;
            world.terrain.config.plains.base_height = 4.0;
            world.terrain.config.plains.height_variation = 0.0;
            world
                .terrain
                .generate_authored(crate::world::TerrainGenerationRequest {
                    position: [0, 0, 0],
                    size: [8, 12, 8],
                })
                .unwrap();
            let mut physics_world = crate::engine::physics::PhysicsWorld::new();
            physics_world.register_from_world(&world);
            system
                .characters
                .insert(1, Character::new(1, Vec3::new(3.5, 6.0, 3.5)));
            system.active_player_id = Some(1);

            for _ in 0..120 {
                let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
            }

            let character = system.get_active_player().unwrap();
            assert!((character.transform.position.y - 4.0).abs() < 0.01);
            assert!(character.movement.is_grounded);
        }
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    #[test]
    fn character_grounding_uses_sculpted_terrain_surface_in_both_modes() {
        for mode in [
            crate::world::TerrainMode::Smooth,
            crate::world::TerrainMode::Block,
        ] {
            let mut system = CharacterSystem::new();
            let mut world = World::new();
            world.terrain.config.mode = mode;
            world.terrain.config.plains.base_height = 4.0;
            world.terrain.config.plains.height_variation = 0.0;
            world
                .terrain
                .generate_authored(crate::world::TerrainGenerationRequest {
                    position: [0, 0, 0],
                    size: [8, 12, 8],
                })
                .unwrap();
            assert!(
                world.sculpt_authored_terrain(&crate::world::terrain::TerrainSculptRequest {
                    tool: crate::world::terrain::SculptTool::Raise,
                    center: [3.5, 0.0, 3.5],
                    radius: 2.0,
                    strength: 2.0,
                    flatten_target: None,
                })
            );
            let expected_surface = world.terrain.surface_height_at(3.5, 3.5).unwrap();
            assert!(expected_surface > 4.0);

            let mut physics_world = crate::engine::physics::PhysicsWorld::new();
            physics_world.register_from_world(&world);
            system.characters.insert(
                1,
                Character::new(1, Vec3::new(3.5, expected_surface + 3.0, 3.5)),
            );
            system.active_player_id = Some(1);

            for _ in 0..180 {
                let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
            }

            let character = system.get_active_player().unwrap();
            assert!((character.transform.position.y - expected_surface).abs() < 0.01);
            assert!(character.movement.is_grounded);
        }
    }

    #[test]
    fn test_animation_state_switching() {
        let mut system = CharacterSystem::new();
        let world = World::new();

        let character = Character::new(1, Vec3::ZERO);

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::ZERO, false);

        assert_eq!(
            system.get_active_player().unwrap().animation.current_state,
            AnimationState::Idle
        );

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::X, false);

        assert_eq!(
            system.get_active_player().unwrap().animation.current_state,
            AnimationState::Walk
        );
    }

    #[test]
    fn test_character_facing_direction() {
        let mut system = CharacterSystem::new();
        let world = World::new();

        let character = Character::new(1, Vec3::ZERO);

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::new(1.0, 0.0), false);

        let rotation_right = system.get_active_player().unwrap().transform.rotation;

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::new(-1.0, 0.0), false);

        let rotation_left = system.get_active_player().unwrap().transform.rotation;

        assert_ne!(rotation_right, rotation_left);

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::ZERO, false);

        let rotation_idle = system.get_active_player().unwrap().transform.rotation;

        assert_eq!(rotation_left, rotation_idle);
    }

    #[test]
    fn test_character_jumping() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        physics_world.register_from_world(&world);

        let character = Character::new(1, Vec3::new(0.0, 1.0, 0.0));

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);

        assert!(system.get_active_player().unwrap().movement.is_grounded);

        let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, true);

        let updated = system.get_active_player().unwrap();

        let expected_velocity = JUMP_IMPULSE + world.gravity.y * (1.0 / 60.0);

        assert!((updated.movement.velocity.y - expected_velocity).abs() < 0.001);

        assert!(!updated.movement.is_grounded);

        let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, true);

        assert!(system.get_active_player().unwrap().movement.velocity.y < movement::JUMP_IMPULSE);
    }

    #[test]
    fn test_character_collision_with_dynamic_bodies() {
        use crate::engine::physics::{PhysicsBody, PhysicsBodyId, PhysicsWorld};

        let mut system = CharacterSystem::new();

        let world = World::new();

        let mut physics_world = PhysicsWorld::new();

        let mut character = Character::new(1, Vec3::ZERO);

        character.collision.radius = 0.5;

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        let body_id = PhysicsBodyId(1);

        let mut body = PhysicsBody::new(body_id, 0, Vec3::new(0.6, 0.0, -0.5), Vec3::ONE);

        body.solid = true;
        body.anchored = false;

        physics_world.bodies.push(body);

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::new(1.0, 0.0), false);

        let updated = system.get_active_player().unwrap();

        assert!(
            updated.transform.position.x < 0.4,
            "Character should be blocked by awake body"
        );

        assert!((updated.transform.position.x - 0.1).abs() < 0.01);

        physics_world.bodies[0].is_sleeping = true;

        physics_world.bodies[0].position = Vec3::new(0.6, 0.0, -0.5);

        system.get_active_player_mut().unwrap().transform.position = Vec3::ZERO;

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::new(1.0, 0.0), false);

        let updated = system.get_active_player().unwrap();

        assert!(updated.transform.position.x < 0.4);

        assert!(!physics_world.bodies[0].is_sleeping);

        physics_world.bodies[0].solid = false;

        system.get_active_player_mut().unwrap().transform.position = Vec3::ZERO;

        let _ = system.update(&world, &mut physics_world, 0.1, Vec2::new(1.0, 0.0), false);

        let updated = system.get_active_player().unwrap();

        assert!((updated.transform.position.x - 0.4).abs() < 0.01);
    }

    #[test]
    fn test_character_collision_with_other_characters() {
        let mut system = CharacterSystem::new();

        let world = World::new();

        let mut physics_world = crate::engine::physics::PhysicsWorld::new();

        let mut character_a = Character::new(1, Vec3::new(0.0, 0.0, 0.0));

        let mut character_b = Character::new(2, Vec3::new(0.5, 0.0, 0.0));

        character_a.movement.velocity = Vec3::new(2.0, 0.0, 0.0);

        character_b.movement.velocity = Vec3::new(-2.0, 0.0, 0.0);

        system.characters.insert(1, character_a);

        system.characters.insert(2, character_b);

        let _ = system.update_scripted(&world, &mut physics_world, 1.0 / 60.0);

        let a = system.characters.get(&1).unwrap();

        let b = system.characters.get(&2).unwrap();

        let dx = b.transform.position.x - a.transform.position.x;

        let dz = b.transform.position.z - a.transform.position.z;

        let horizontal_distance = (dx * dx + dz * dz).sqrt();

        let minimum_distance = a.collision.radius + b.collision.radius;

        assert!(
            horizontal_distance >= minimum_distance - 0.001,
            "Characters still overlap: distance={}, minimum={}",
            horizontal_distance,
            minimum_distance
        );
    }

    #[test]
    fn test_entity_character_relationship_and_position_sync() {
        let mut system = CharacterSystem::new();

        let mut entity_manager = EntityManager::new();

        let pos = Vec3::new(12.0, 3.0, 8.0);

        let char_id = system.spawn_character(pos, None);

        let entity_id = entity_manager.create_entity("Villager");

        entity_manager.set_position(entity_id, pos);

        system.associate_entity(entity_id, char_id);

        assert_eq!(system.get_character_id_for_entity(entity_id), Some(char_id));

        assert_eq!(system.get_entity_for_character(char_id), Some(entity_id));

        let character = system.get_character_for_entity(entity_id).unwrap();

        assert_eq!(character.transform.position, pos);

        let new_pos = Vec3::new(20.0, 3.0, 30.0);

        system.set_entity_position(entity_id, new_pos);

        system.sync_entity_positions(&mut entity_manager);

        assert_eq!(entity_manager.get_position(entity_id), Some(new_pos));
    }

    #[test]
    fn attachment_point_resolves_named_joint_from_current_pose_in_world_space() {
        // Worldkiln 0.8.5 | WO 0.8.7-82 | 2026-10-10 01:10 -05:00
        // Bundled character assets are optional. Run the rig assertion only when
        // the fixture is installed; missing assets must not fail the test suite.
        let Some(package) = crate::character::package::get_package("Rogue") else {
            return;
        };
        let mut character =
            Character::from_package(73, Vec3::new(10.0, 2.0, -4.0), package.clone());
        character.transform.rotation = Quat::from_rotation_y(0.7);
        character.transform.scale = Vec3::splat(1.5);

        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:25 -05:00
        // Imported Rogue aliases RightHand to Fist.R and provides a grip rotation.
        let joint_index = package
            .attachment_joint_index("RightHand")
            .expect("Rogue package should expose RightHand attachment");
        let mut local_transforms: Vec<_> = package
            .skeleton
            .joints
            .iter()
            .map(|joint| joint.local_transform)
            .collect();
        local_transforms[joint_index].translation += Vec3::new(0.25, 0.5, -0.75);
        character.current_pose = package.skeleton.evaluate_pose(&local_transforms);

        let joint_matrix = character.current_pose.matrices[joint_index];
        let (_joint_scale, joint_rotation, joint_translation) =
            joint_matrix.to_scale_rotation_translation();
        // Match the authored grip-frame correction and deliberately discard
        // the imported rig's 100x internal scale.
        let expected = Mat4::from_scale_rotation_translation(
            character.transform.scale,
            character.transform.rotation,
            character.transform.position,
        ) * Mat4::from_rotation_translation(joint_rotation, joint_translation)
            * Mat4::from_quat(package.attachment_rotation("RightHand"));

        let mut system = CharacterSystem::new();
        let entity_id = EntityId(900);
        system.characters.insert(character.id, character);
        system.associate_entity(entity_id, 73);

        let actual = system
            .resolve_attachment_point(entity_id, "rightHAND")
            .expect("joint lookup should be case-insensitive");
        for (actual, expected) in actual
            .to_cols_array()
            .into_iter()
            .zip(expected.to_cols_array())
        {
            assert!((actual - expected).abs() < 0.0001);
        }

        assert!(
            system
                .resolve_attachment_point(entity_id, "MissingPoint")
                .is_none()
        );
        assert!(
            system
                .resolve_attachment_point(EntityId(901), "RightHand")
                .is_none()
        );
    }

    #[test]
    fn test_multiple_spawned_characters() {
        let mut system = CharacterSystem::new();

        let id1 = system.spawn_character(Vec3::new(10.0, 1.0, 5.0), None);

        let id2 = system.spawn_character(Vec3::new(14.0, 1.0, 5.0), None);

        let id3 = system.spawn_character(Vec3::new(20.0, 1.0, 8.0), None);

        assert_ne!(id1, id2);
        assert_ne!(id2, id3);

        assert_eq!(system.get_active_characters().count(), 3);
    }

    #[test]
    fn test_character_runtime_controls() {
        let mut system = CharacterSystem::new();

        let entity_id = EntityId(100);

        let character_id = system.spawn_character(Vec3::new(0.0, 1.0, 0.0), None);

        system.associate_entity(entity_id, character_id);

        assert!(system.set_entity_velocity(entity_id, Vec3::new(1.0, 2.0, 3.0,)));

        assert_eq!(
            system.get_entity_velocity(entity_id),
            Some(Vec3::new(1.0, 2.0, 3.0,))
        );

        assert!(system.set_entity_facing_direction(entity_id, 1.0, 0.0,));

        let facing = system.get_entity_facing(entity_id).unwrap();

        assert!((facing.x - 1.0).abs() < 0.001);

        assert!(facing.y.abs() < 0.001);

        assert!(system.set_entity_animation(entity_id, "Walk",).unwrap());
    }

    #[test]
    fn test_character_health_controls() {
        let mut system = CharacterSystem::new();

        let entity_id = EntityId(100);

        let character_id = system.spawn_character(Vec3::ZERO, None);

        system.associate_entity(entity_id, character_id);

        assert_eq!(system.get_entity_health(entity_id), Some(100.0));

        assert_eq!(system.damage_entity(entity_id, 25.0), Some(75.0));

        assert_eq!(system.heal_entity(entity_id, 10.0), Some(85.0));

        assert!(system.set_entity_health(entity_id, 0.0));

        assert_eq!(system.is_entity_alive(entity_id), Some(false));
    }

    #[test]
    fn test_character_destroy() {
        let mut system = CharacterSystem::new();

        let entity_id = EntityId(100);

        let character_id = system.spawn_character(Vec3::ZERO, None);

        system.associate_entity(entity_id, character_id);

        assert!(system.destroy_entity(entity_id));

        assert!(system.get_character_for_entity(entity_id).is_none());

        assert!(system.get_entity_for_character(character_id).is_none());
    }

    #[test]
    fn test_character_pathfinding() {
        let mut system = CharacterSystem::new();

        let mut world = World::new();

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);

        world.set_cell(WorldCoord::new(2, 0, 0), CellType::Block);

        let entity_id = EntityId(100);

        let character_id = system.spawn_character(Vec3::new(0.0, 1.0, 0.0), None);

        system.associate_entity(entity_id, character_id);

        let path = system.find_path_for_entity(entity_id, &world, Vec3::new(2.0, 1.0, 0.0));

        assert!(path.is_some());
        assert!(!path.unwrap().is_empty());
    }

    #[test]
    fn test_anchored_cell_runtime_movement_collision() {
        use crate::engine::physics::PhysicsWorld;

        let mut system = CharacterSystem::new();

        let mut world = World::new();

        let mut physics_world = PhysicsWorld::new();

        let coord = WorldCoord::new(0, 0, 0);

        world.set_cell(coord, CellType::Block);

        let cell_id = world.get(coord).unwrap().id;

        physics_world.register_from_world(&world);

        assert_eq!(physics_world.static_colliders.len(), 1);

        let character = Character::new(1, Vec3::new(0.0, 1.5, 0.0));

        system.characters.insert(1, character);

        system.active_player_id = Some(1);

        for _ in 0..60 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!((updated.transform.position.y - 1.0).abs() < 0.01);

        assert!(updated.movement.is_grounded);

        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 2.0, 0.0));

        physics_world.sync_with_world(&mut world);

        system.get_active_player_mut().unwrap().transform.position = Vec3::new(0.0, 1.5, 0.0);

        system.get_active_player_mut().unwrap().movement.is_grounded = false;

        system.get_active_player_mut().unwrap().movement.velocity = Vec3::ZERO;

        for _ in 0..10 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!(updated.transform.position.y < 1.5);

        assert!(!updated.movement.is_grounded);

        system.get_active_player_mut().unwrap().transform.position = Vec3::new(0.0, 3.5, 0.0);

        for _ in 0..60 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!((updated.transform.position.y - 3.0).abs() < 0.01);

        assert!(updated.movement.is_grounded);

        world.set_visual_offset_runtime(coord, Vec3::new(0.0, 4.0, 0.0));

        physics_world.sync_with_world(&mut world);

        system.get_active_player_mut().unwrap().transform.position = Vec3::new(0.0, 5.5, 0.0);

        for _ in 0..60 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!((updated.transform.position.y - 5.0).abs() < 0.01);

        assert!(updated.movement.is_grounded);

        world.clear_runtime_state();

        physics_world.sync_with_world(&mut world);

        system.get_active_player_mut().unwrap().transform.position = Vec3::new(0.0, 1.5, 0.0);

        for _ in 0..60 {
            let _ = system.update(&world, &mut physics_world, 1.0 / 60.0, Vec2::ZERO, false);
        }

        let updated = system.get_active_player().unwrap();

        assert!((updated.transform.position.y - 1.0).abs() < 0.01);

        assert!(world.get(coord).is_some());

        assert_eq!(world.get(coord).unwrap().id, cell_id);
    }
}
