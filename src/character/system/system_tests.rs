// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
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
    fn project_walk_speed_measures_actual_distance_across_hilly_landscape() {
        use crate::world::terrain::{
            HeightfieldData, PLAINS_GENERATOR_IMPLEMENTATION_VERSION, TerrainData,
            TerrainGenerationRequest, TerrainGeneratorType, TerrainResult,
        };

        for terrain in 0..5 {
            let measure = |speed: f32, hill: f32, label: &str| -> (f32, f32, f32) {
                let mut world = World::new();
                world.character_walk_speed = speed;
                world.max_walkable_slope_degrees = 15.0;
                world.terrain.config.ocean_enabled = false;
                let height_at = |x: f32| {
                    let phase = terrain as f32 * 0.19;
                    3.0 + hill
                        * ((x * (0.85 + phase * 0.15)).sin() * 0.8 + (x * 1.41 + phase).sin() * 0.2)
                };
                world.terrain.authored = Some(TerrainResult {
                    request: TerrainGenerationRequest {
                        position: [0, 0, 0],
                        size: [12, 12, 12],
                    },
                    generator: TerrainGeneratorType::Plains,
                    generator_implementation_version: PLAINS_GENERATOR_IMPLEMENTATION_VERSION,
                    data: TerrainData::Heightfield(HeightfieldData {
                        origin: [0, 0, 0],
                        size: [12, 12, 12],
                        heights: (0..144).map(|i| height_at((i % 12) as f32)).collect(),
                    }),
                });
                let mut physics = crate::engine::physics::PhysicsWorld::new();
                physics.register_from_world(&world);
                let mut system = CharacterSystem::new();
                let (x, z) = (2.5, 5.5);
                let y = crate::character::terrain_motion::surface_contact(&world, x, z)
                    .expect("terrain contact")
                    .height;
                let id = system.spawn_character(Vec3::new(x, y, z), None);
                system.active_player_id = Some(id);
                system.get_active_player_mut().unwrap().movement.is_grounded = true;
                let initial = system.get_active_player().unwrap().transform.position;
                let mut previous = initial;
                let mut path = 0.0_f32;
                let mut max_step = 0.0_f32;
                for _ in 0..120 {
                    system.update(&world, &mut physics, 1.0 / 60.0, Vec2::new(1.0, 0.0), false);
                    let current = system.get_active_player().unwrap().transform.position;
                    let step = current.distance(previous);
                    path += step;
                    max_step = max_step.max(step);
                    previous = current;
                }
                let direct = previous.distance(initial);
                eprintln!(
                    "{label}: speed={speed:.2}, hill amplitude={hill:.2}, A-B={direct:.4} studs, actual path={path:.4} studs, max frame step={max_step:.4}"
                );
                (direct, path, max_step)
            };

            let original = measure(2.0, 0.24, "hill @ 2");
            let changed = measure(3.0, 0.24, "hill @ 3");
            assert!(
                changed.1 > original.1 + 1.0,
                "project Walk Speed change did not increase actual distance"
            );

            let too_steep = measure(2.0, 1.0, "over 15 degrees");
            let flat = measure(2.0, 0.0, "flat @ 2");
            let hill = measure(2.0, 0.24, "hill @ 2 comparison");
            eprintln!(
                "COMPARISON: flat={:.4}, hill={:.4}, hill faster? {}, over-limit={:.4}",
                flat.1,
                hill.1,
                hill.1 > flat.1,
                too_steep.1,
            );
            assert!(
                hill.1 < flat.1,
                "hill traveled FARTHER than flat: hill={}, flat={}",
                hill.1,
                flat.1
            );
            assert!(
                hill.2 < 0.12 && changed.2 < 0.15,
                "hilly terrain produced a position launch"
            );
            assert!(
                too_steep.1 < flat.1,
                "terrain {terrain}: over-limit slope did not reduce reachable distance: steep={}, flat={}",
                too_steep.1,
                flat.1
            );
            eprintln!("TERRAIN {terrain}: PASS");
        }
    }

    #[test]
    fn wk_swim_actual_lake_surface_stays_level_for_downward_aim() {
        use crate::character::movement::SwimMode;
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut world = World::new();
        world.terrain.config.water_bodies.push(WaterBody::new(
            1,
            WaterBodyKind::Lake,
            [0.0, 0.0],
            2.0,
        ));
        let mut system = CharacterSystem::new();
        let id = system.spawn_character(Vec3::new(0.0, 0.0, 0.0), Some("Rogue"));
        system.active_player_id = Some(id);
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        for _ in 0..540 {
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        }
        let player = system.get_active_player().unwrap();
        assert_eq!(
            player.movement.swim_mode,
            SwimMode::Surface,
            "body never reached actual lake surface"
        );
        let expected = 2.0 - player.collision.height * 0.78;
        assert!(
            (player.transform.position.y - expected).abs() < 0.03,
            "surface drift: {}",
            player.transform.position.y
        );
        system.set_active_swim_look_direction(-0.62, Vec2::new(0.0, 1.0));
        for _ in 0..35 {
            system.get_active_player_mut().unwrap().movement.velocity.z = 4.0;
            system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        }
        let player = system.get_active_player().unwrap();
        assert_eq!(
            player.movement.swim_mode,
            SwimMode::Surface,
            "aim must not unlock surface swimming"
        );
        assert!(
            (player.transform.position.y - expected).abs() < 0.03,
            "downward camera angle changed bobbing surface height"
        );
    }

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
    fn river_current_drifts_character_only_when_submerged() {
        use crate::world::terrain::{WaterBody, WaterBodyKind};
        let mut world = World::new();
        world.gravity = Vec3::ZERO;
        let mut river = WaterBody::new(45, WaterBodyKind::River, [0.0, 0.0], 10.0);
        for point in &mut river.points {
            point.flow_speed = 4.0;
        }
        world.terrain.config.water_bodies.push(river);
        let mut system = CharacterSystem::new();
        system
            .characters
            .insert(1, Character::new(1, Vec3::new(0.0, 9.0, 0.0)));
        system
            .characters
            .insert(2, Character::new(2, Vec3::new(0.0, 11.0, 0.0)));
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        physics.register_from_world(&world);
        system.update_scripted(&world, &mut physics, 0.25);
        let submerged = system.characters.get(&1).unwrap();
        let dry = system.characters.get(&2).unwrap();
        assert!(
            (submerged.transform.position.x - 1.0).abs() < 0.01,
            "4 studs/s x 0.25s must displace a submerged Character"
        );
        assert!(
            dry.transform.position.x.abs() < 0.01,
            "a Character above the surface must not drift"
        );
        assert_eq!(
            submerged.movement.velocity.x, 0.0,
            "river drift must not overwrite scripted movement velocity"
        );
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

        assert_eq!(updated.animation.current_state, AnimationState::Run);
    }

    #[cfg(windows)]
    #[test]
    fn play_scripted_path_lands_on_underlying_cell_and_holds_36000_frames() {
        let mut world = World::new();
        for x in 0..7 {
            for z in 0..7 {
                world.set_cell(WorldCoord::new(x, 0, z), CellType::Block);
            }
        }
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        physics.register_from_world(&world);
        world.physics_dirty_cells.clear();
        let mut characters = CharacterSystem::new();
        let id = characters.spawn_character(Vec3::new(3.5, 6.0, 3.5), None);
        let mut landed = false;
        for frame in 0..36_000 {
            physics.sync_with_world(&mut world);
            characters.update_scripted(&world, &mut physics, 1.0 / 60.0);
            let character = &characters.characters[&id];
            assert!(
                character.transform.position.y >= 0.99,
                "Play character fell through Cells at frame {frame}: {:?}; colliders={}",
                character.transform.position,
                physics.static_colliders.len()
            );
            assert!(
                (character.transform.position.x - 3.5).abs() < 0.005
                    && (character.transform.position.z - 3.5).abs() < 0.005,
                "Play character ejected from Cell: {:?}",
                character.transform.position
            );
            if character.movement.is_grounded {
                landed = true;
            }
            if landed {
                assert!(
                    character.movement.is_grounded,
                    "Play character lost Cell support at frame {frame}"
                );
                assert!((character.transform.position.y - 1.0).abs() < 0.01);
            }
        }
        assert!(landed);
    }

    #[test]
    fn sunken_character_recovers_on_cell_dock_and_keeps_walking() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();
        for x in 0..5 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        physics.register_from_world(&world);

        let id = system.spawn_character(Vec3::new(1.5, 0.76, 0.5), None);
        system.active_player_id = Some(id);
        let mut last_x = 1.5;
        for frame in 0..90 {
            let input = if frame < 30 {
                Vec2::new(1.0, 0.0)
            } else {
                Vec2::ZERO
            };
            system.update(&world, &mut physics, 1.0 / 60.0, input, false);
            let actor = system.get_active_player().unwrap();
            assert!(
                (actor.transform.position.y - 1.0).abs() < 0.025,
                "dock floor recovery failed at frame {frame}: {:?}",
                actor.transform.position
            );
            assert!(
                actor.transform.position.x >= last_x - 0.01,
                "connected dock pushed actor backward at frame {frame}"
            );
            assert!(
                (actor.transform.position.z - 0.5).abs() < 0.01,
                "connected dock ejected actor sideways at frame {frame}"
            );
            last_x = actor.transform.position.x;
        }
        assert!(
            last_x > 2.0 && last_x < 4.6,
            "player must walk across the dock without teleporting"
        );
    }

    #[test]
    fn walking_across_adjacent_cell_tops_never_ejects_character() {
        let mut system = CharacterSystem::new();
        let mut world = World::new();
        for x in 0..4 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        let mut physics = crate::engine::physics::PhysicsWorld::new();
        physics.register_from_world(&world);

        let id = system.spawn_character(Vec3::new(0.5, 1.0, 0.5), None);
        system.active_player_id = Some(id);
        let mut last_x = 0.5;
        for frame in 0..90 {
            let input = if frame < 20 {
                Vec2::new(1.0, 0.0)
            } else {
                Vec2::ZERO
            };
            system.update(&world, &mut physics, 1.0 / 60.0, input, false);
            let player = system.get_active_player().unwrap();
            assert!(
                player.transform.position.x >= last_x - 0.01,
                "Cell pushed the Character backward at frame {frame}"
            );
            assert!(
                (player.transform.position.y - 1.0).abs() < 0.02,
                "Cell floor shifted Character height at frame {frame}"
            );
            assert!(
                player.movement.is_grounded,
                "lost Cell floor contact at frame {frame}"
            );
            last_x = player.transform.position.x;
        }
        assert!(
            last_x > 1.5 && last_x < 3.5,
            "Character should cross multiple Cell joins: {last_x}"
        );
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

    #[test]
    fn character_grounds_on_generated_landscape() {
        {
            let mut system = CharacterSystem::new();
            let mut world = World::new();
            world.terrain.config.ocean_enabled = false;
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

    #[test]
    fn character_grounding_uses_sculpted_terrain_surface() {
        {
            let mut system = CharacterSystem::new();
            let mut world = World::new();
            world.terrain.config.ocean_enabled = false;
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
            let contact = crate::character::terrain_motion::surface_contact(
                &world,
                character.transform.position.x,
                character.transform.position.z,
            )
            .expect("sculpted native terrain must retain collision");
            assert!(
                (character.transform.position.y - contact.height).abs() < 0.01,
                "feet must track the visible terrain triangles"
            );
            let spawn_ground =
                crate::character::terrain_motion::surface_contact(&world, 3.5, 3.5).unwrap();
            if (1.0 - spawn_ground.normal.y * spawn_ground.normal.y).sqrt() < 0.588 {
                assert!(
                    (character.transform.position.x - 3.5).abs() < 0.05
                        && (character.transform.position.z - 3.5).abs() < 0.05,
                    "gentle sculpted hill should hold a stationary actor"
                );
            } else {
                assert!(
                    character.transform.position.x < 3.5 && character.transform.position.z < 3.5,
                    "steep sculpted ground should still slide downhill"
                );
            }
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
        let Some(package) = crate::character::package::get_package("Rogue") else {
            return;
        };
        let mut character =
            Character::from_package(73, Vec3::new(10.0, 2.0, -4.0), package.clone());
        character.transform.rotation = Quat::from_rotation_y(0.7);
        character.transform.scale = Vec3::splat(1.5);

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
        world.navmesh_settings.global_navigation = true;

        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);

        world.set_cell(WorldCoord::new(2, 0, 0), CellType::Block);

        let entity_id = EntityId(100);

        let character_id = system.spawn_character(Vec3::new(0.0, 1.0, 0.0), None);

        system.associate_entity(entity_id, character_id);

        let mut path = system.find_path_for_entity(entity_id, &world, Vec3::new(2.0, 1.0, 0.0));
        for _ in 0..4096 {
            if path.is_some() {
                break;
            }
            system.tick_navigation_builds(&world);
            path = system.find_path_for_entity(entity_id, &world, Vec3::new(2.0, 1.0, 0.0));
        }

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

#[test]
fn character_separation_cannot_push_a_neighbor_through_solid_cell_wall() {
    use crate::character::system::CharacterSystem;
    use crate::engine::{entity::EntityId, physics::PhysicsWorld};
    use crate::world::{CellType, World, WorldCoord};
    use glam::Vec3;
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
    for (entity, pos) in [
        (EntityId(9101), Vec3::new(2.7, 1.0, 3.5)),
        (EntityId(9102), Vec3::new(3.59, 1.0, 3.5)),
    ] {
        let id = system.spawn_character(pos, None);
        system.associate_entity(entity, id);
        let c = system.get_character_for_entity_mut(entity).unwrap();
        c.collision.radius = 0.4;
        c.collision.height = 1.8;
    }
    for frame in 0..360 {
        system
            .get_character_for_entity_mut(EntityId(9101))
            .unwrap()
            .movement
            .velocity
            .x = 6.0;
        system
            .get_character_for_entity_mut(EntityId(9102))
            .unwrap()
            .movement
            .velocity
            .x = 0.0;
        system.update_scripted(&world, &mut physics, 1.0 / 60.0);
        for entity in [EntityId(9101), EntityId(9102)] {
            let c = system.get_character_for_entity(entity).unwrap();
            assert!(
                c.transform.position.x + c.collision.radius <= 4.001,
                "frame {frame}: pair separation must not move either body into or through the wall: {:?}",
                c.transform.position
            );
            assert!((c.transform.position.y - 1.0).abs() < 0.01);
        }
    }
    let a = system.get_character_for_entity(EntityId(9101)).unwrap();
    let b = system.get_character_for_entity(EntityId(9102)).unwrap();
    assert!(
        a.transform.position.distance(b.transform.position) >= 0.798,
        "wall pinning must also stop the pusher, not leave hidden overlap"
    );
}
