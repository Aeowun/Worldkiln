pub mod animation;
pub mod character;
pub mod collision;
pub mod geometry;
pub mod movement;
pub mod package;
pub mod rig;
pub mod spawning;
pub mod system;
// Worldkiln 0.8.5 | WO 0.8.7-57 | 2026-10-09
mod terrain_block_collision;
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
        // Standard character capsule dimensions
        assert_eq!(c.radius, 0.4);
        assert_eq!(c.height, 1.8);
    }

    #[test]
    fn test_character_animation_defaults() {
        let a = CharacterAnimation::default();
        assert_eq!(a.current_state, AnimationState::Idle);
    }

    #[test]
    fn test_character_construction() {
        let id = 42;
        let pos = Vec3::new(10.0, 5.0, -2.0);
        let c = Character::new(id, pos);

        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:33 -05:00
        // The default must be the real CC0 imported GLB, not a legacy placeholder.
        assert_eq!(c.package_name, "Rogue");
        assert_eq!(c.package.metadata.mesh_type, "imported_glb");
        assert!(c.package.clip_named("Walk").is_some());
        assert_eq!(c.collision, c.package.collision);
        assert_eq!(c.id, id);
        assert_eq!(c.transform.position, pos);
        assert_eq!(c.movement.state, MovementState::Idle);
        assert_eq!(c.animation.current_state, AnimationState::Idle);

        // Custom character integration verification
        assert_eq!(c.animation_controller.blend_weight(), 0.0);
        assert_eq!(c.appearance.show_accessory, true);
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
        // Default spawn point is solid.

        let id = 123;
        let character = spawn_at_random_point(&world, id, None).expect("Should spawn character");

        assert_eq!(character.id, id);
        // Should be at (10, 1, 10) because block is at (10, 0, 10) and is solid.
        assert_eq!(character.transform.position, Vec3::new(10.0, 1.0, 10.0));
        // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:24 -05:00
        // Spawn uses the imported Character's own collider, not an old
        // generic/legacy size.
        assert_eq!(character.collision, character.package.collision);
    }

    // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:24 -05:00
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
        // Should be at (5, 5, 5) because block is non-solid.
        assert_eq!(character.transform.position, Vec3::new(5.0, 5.0, 5.0));
    }

    // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:33 -05:00
    #[test]
    fn spawn_clearance_uses_actual_tall_rogue_glb_bounds() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(10, 0, 10), CellType::SpawnPoint);
        // Generic 1.8-height fixture would clear this ceiling, but
        // the actual imported Rogue collider is taller than 2 studs.
        world.set_cell(WorldCoord::new(10, 3, 10), CellType::Block);
        let character = spawn_at_random_point(&world, 12, None)
            .expect("nearby clearance should permit a valid spawn");
        assert_eq!(character.id, 12);
        assert!(character.collision.height > 2.0);
        assert_ne!(character.transform.position, Vec3::new(10.0, 1.0, 10.0));
        assert!((character.transform.position.x - 10.0).abs() <= 10.0);
        assert!((character.transform.position.z - 10.0).abs() <= 10.0);
    }

    #[test]
    fn test_spawn_blocked_searches_nearby() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);
        world.set_cell(coord, CellType::SpawnPoint);

        // Place a solid block exactly where the character would spawn (0, 1, 0)
        world.set_cell(WorldCoord::new(0, 1, 0), CellType::Block);

        let character =
            spawn_at_random_point(&world, 1, None).expect("Should find alternative spawn");
        // Should not be at (0, 1, 0)
        assert_ne!(character.transform.position, Vec3::new(0.0, 1.0, 0.0));
        // It should have found a valid spot within the search radius.
        // (0, 1, 0) is blocked, so it would check radius 1.
        // e.g. (1, 1, 0) or (0, 1, 1) etc.
    }

    #[test]
    fn test_character_animation_controller_pose_evaluation() {
        let mut c = Character::new(1, Vec3::ZERO);

        // Initial pose should be evaluated in new()
        assert_eq!(c.current_pose.matrices.is_empty(), false);

        let initial_hips_mat = c.current_pose.matrices[1];

        // Advance and re-evaluate
        c.animation_controller.update(0.5);
        c.current_pose = c.animation_controller.evaluate_pose();

        let new_hips_mat = c.current_pose.matrices[1];
        assert_ne!(initial_hips_mat, new_hips_mat);
    }

    #[test]
    fn test_gameplay_camera_follows_character() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();
        let character = Character::new(1, Vec3::new(10.0, 0.0, 20.0));
        let world = World::new();

        camera.update(&character, &world);

        // Camera target should be character position + look height
        assert_eq!(
            camera.current_target,
            Vec3::new(10.0, camera.look_height, 20.0)
        );

        // Camera position should be offset from character
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

        // Configure camera to be at the same height as the look target (Y=1)
        // look_height is 1.0. If height is 1.0 and pitch is 0, camera Y is 1.0.
        camera.height = 1.0;
        camera.pitch = 0.0;
        camera.distance = 10.0;

        // Place a solid block in the line of sight (constant Y=1).
        world.set_cell(WorldCoord::new(2, 1, 2), CellType::Block);

        camera.update(&character, &world);

        // Camera should be shortened.
        let actual_dist = camera.current_position.distance(camera.current_target);
        assert!(actual_dist < 9.5);
    }

    #[test]
    fn test_camera_relative_movement_vectors() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();

        // Face North (yaw 0)
        camera.yaw = 0.0;
        let (fwd, right) = camera.get_horizontal_basis();
        // Forward should be -Z: (0, 0, -1)
        assert!((fwd - Vec3::new(0.0, 0.0, -1.0)).length() < 0.001);
        // Right should be +X: (1, 0, 0)
        assert!((right - Vec3::new(1.0, 0.0, 0.0)).length() < 0.001);

        // Rotate 180 degrees
        camera.yaw = std::f32::consts::PI;
        let (fwd2, right2) = camera.get_horizontal_basis();
        // Forward should be +Z: (0, 0, 1)
        assert!((fwd2 - Vec3::new(0.0, 0.0, 1.0)).length() < 0.001);
        // Right should be -X: (-1, 0, 0)
        assert!((right2 - Vec3::new(-1.0, 0.0, 0.0)).length() < 0.001);
    }

    #[test]
    fn test_camera_pitch_horizontal_isolation() {
        use crate::renderer::camera::GameplayCamera;
        let mut camera = GameplayCamera::new();

        // Extreme pitch should not affect horizontal basis.
        camera.pitch = 80.0_f32.to_radians();
        let (fwd, _) = camera.get_horizontal_basis();
        assert_eq!(fwd.y, 0.0);

        camera.pitch = -80.0_f32.to_radians();
        let (fwd2, _) = camera.get_horizontal_basis();
        assert_eq!(fwd2.y, 0.0);
    }
}
