// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
pub mod animation;
pub mod character;
pub(crate) mod collider;
pub mod collision;
pub mod geometry;
pub mod movement;
pub mod package;
pub mod rig;
pub mod spawning;
pub mod system;
mod terrain_motion;
pub mod transform;

pub use animation::{
    AnimationClip, AnimationState, CharacterAnimation, CharacterAnimationController, Keyframe,
    TargetAnimation, TransformTrack,
};
pub use character::{AppearanceCustomization, Character, MaterialSlot, generate_character_mesh};
pub use collision::CharacterCollision;
pub use movement::{CharacterMovement, MovementState};
pub use package::{CharacterPackage, CharacterPackageRegistry};
pub use rig::EvaluatedPose;
pub use spawning::spawn_at_random_point;
pub use system::CharacterSystem;
pub use transform::CharacterTransform;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{CellType, World, WorldCoord};
    use glam::{Quat, Vec3};

    #[test]
    fn test_character_transform_defaults() {
        let t = CharacterTransform::default();
        assert_eq!(t.position, Vec3::ZERO);
        assert_eq!(t.rotation, Quat::IDENTITY);
        assert_eq!(t.scale, Vec3::ONE);
    }

    #[test]
    fn test_character_transform_construction() {
        let pos = Vec3::new(1.0, 2.0, 3.0);
        let t = CharacterTransform::new(pos);
        assert_eq!(t.position, pos);
    }

    #[test]
    fn test_character_movement_defaults() {
        let m = CharacterMovement::default();
        assert_eq!(m.velocity, Vec3::ZERO);
        assert!(!m.is_grounded);
        assert_eq!(m.state, MovementState::Idle);
    }

    #[test]
    fn test_character_collision_defaults() {
        let c = CharacterCollision::default();
        assert_eq!(c.radius, 0.4);
        assert_eq!(c.height, 1.8);
    }

    #[test]
    fn test_character_animation_defaults() {
        let a = CharacterAnimation::default();
        assert_eq!(a.current_state, AnimationState::Idle);
    }


    #[test]
    fn test_spawn_no_points_returns_none() {
        let world = World::new();
        let result = spawn_at_random_point(&world, 1, None);
        assert!(result.is_none());
    }

    #[test]
    fn test_spawn_valid_solid_spawn_point() {
        let mut world = World::new();
        let coord = WorldCoord::new(10, 0, 10);
        world.set_cell(coord, CellType::SpawnPoint);

        let id = 123;
        let character = spawn_at_random_point(&world, id, None).expect("Should spawn character");

        assert_eq!(character.id, id);
        assert_eq!(character.transform.position, Vec3::new(10.0, 1.0, 10.0));
        assert_eq!(character.collision, character.package.collision);
    }

    #[test]
    fn spawn_rejects_missing_explicit_package_without_panicking_or_mutating_world() {
        let mut world = World::new();
        let coord = WorldCoord::new(1, 0, 1);
        world.set_cell(coord, CellType::SpawnPoint);
        let original = world.get(coord).unwrap().clone();
        assert!(spawn_at_random_point(&world, 7, Some("NotInstalledCharacter")).is_none());
        assert_eq!(world.get(coord).unwrap(), &original);
    }

    #[test]
    fn test_spawn_non_solid_spawn_point() {
        let mut world = World::new();
        let coord = WorldCoord::new(5, 5, 5);
        world.set_cell(coord, CellType::SpawnPoint);
        if let Some(cell) = world.get_mut(coord) {
            cell.solid = false;
        }

        let character = spawn_at_random_point(&world, 1, None).expect("Should spawn character");
        assert_eq!(character.transform.position, Vec3::new(5.0, 5.0, 5.0));
    }


    #[test]
    fn test_spawn_blocked_searches_nearby() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::SpawnPoint);

        world.set_cell(WorldCoord::new(0, 1, 0), CellType::Block);

        let character =
            spawn_at_random_point(&world, 1, None).expect("Should find alternative spawn");
        assert_ne!(character.transform.position, Vec3::new(0.0, 1.0, 0.0));
    }


    #[test]
    fn test_gameplay_camera_follows_character() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();
        let character = Character::new(1, Vec3::new(10.0, 0.0, 20.0));
        let world = World::new();

        camera.update(&character, &world);

        assert_eq!(
            camera.current_target,
            Vec3::new(10.0, camera.look_height, 20.0)
        );

        assert!(
            camera
                .current_position
                .distance(character.transform.position)
                > camera.distance - 0.1
        );
    }

    #[test]
    fn test_gameplay_camera_collision() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();
        let character = Character::new(1, Vec3::new(0.0, 0.0, 0.0));
        let mut world = World::new();

        camera.height = 1.0;
        camera.pitch = 0.0;
        camera.distance = 10.0;

        world.set_cell(WorldCoord::new(2, 1, 2), CellType::Block);

        camera.update(&character, &world);

        let actual_dist = camera.current_position.distance(camera.current_target);
        assert!(actual_dist < 9.5);
    }

    #[test]
    fn test_camera_relative_movement_vectors() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();

        camera.yaw = 0.0;
        let (fwd, right) = camera.get_horizontal_basis();
        assert!((fwd - Vec3::new(0.0, 0.0, -1.0)).length() < 0.001);
        assert!((right - Vec3::new(1.0, 0.0, 0.0)).length() < 0.001);

        camera.yaw = std::f32::consts::PI;
        let (fwd2, right2) = camera.get_horizontal_basis();
        assert!((fwd2 - Vec3::new(0.0, 0.0, 1.0)).length() < 0.001);
        assert!((right2 - Vec3::new(-1.0, 0.0, 0.0)).length() < 0.001);
    }

    #[test]
    fn test_camera_pitch_horizontal_isolation() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();

        camera.pitch = 80.0_f32.to_radians();
        let (fwd, _) = camera.get_horizontal_basis();
        assert_eq!(fwd.y, 0.0);

        camera.pitch = -80.0_f32.to_radians();
        let (fwd2, _) = camera.get_horizontal_basis();
        assert_eq!(fwd2.y, 0.0);
    }
}
