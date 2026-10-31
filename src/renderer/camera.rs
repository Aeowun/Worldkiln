// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use glam::{Mat4, Vec3};

use crate::world::{World, WorldCoord};

const ORBIT_SENSITIVITY: f32 = 0.005;

const EDITOR_MIN_PITCH: f32 = -85.0_f32.to_radians();
const EDITOR_MAX_PITCH: f32 = 85.0_f32.to_radians();

const GAMEPLAY_MIN_PITCH: f32 = -85.0_f32.to_radians();
const GAMEPLAY_MAX_PITCH: f32 = 85.0_f32.to_radians();

pub const DEFAULT_DISTANCE: f32 = 6.0;
pub const DEFAULT_HEIGHT: f32 = 3.0;
pub const LOOK_HEIGHT: f32 = 1.0;

const CAMERA_COLLISION_STEP: f32 = 0.2;
const CAMERA_COLLISION_MARGIN: f32 = 0.1;
const FIRST_PERSON_HEIGHT: f32 = 1.5;

#[derive(Clone)]
pub struct CameraController {
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub target: Vec3,
}

impl Default for CameraController {
    fn default() -> Self {
        Self::new()
    }
}

impl CameraController {
    pub fn new() -> Self {
        Self {
            yaw: 45.0_f32.to_radians(),
            pitch: 35.0_f32.to_radians(),
            distance: 12.0,
            target: Vec3::ZERO,
        }
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        self.yaw -= dx * ORBIT_SENSITIVITY;
        self.pitch += dy * ORBIT_SENSITIVITY;
        self.pitch = self.pitch.clamp(EDITOR_MIN_PITCH, EDITOR_MAX_PITCH);
    }

    pub fn look(&mut self, dx: f32, dy: f32) {
        let position = self.get_position();

        self.yaw -= dx * ORBIT_SENSITIVITY;
        self.pitch += dy * ORBIT_SENSITIVITY;
        self.pitch = self.pitch.clamp(EDITOR_MIN_PITCH, EDITOR_MAX_PITCH);

        self.target = position - orbit_offset(self.distance, self.yaw, self.pitch);
    }

    pub fn zoom(&mut self, delta: f32) {
        self.distance = (self.distance - delta * 1.5).clamp(2.0, 100.0);
    }

    pub fn get_position(&self) -> Vec3 {
        self.target + orbit_offset(self.distance, self.yaw, self.pitch)
    }

    pub fn get_view_matrix(&self) -> Mat4 {
        let position = self.get_position();
        let (_, up, _) = self.get_basis();

        safe_camera_view(position, self.target, self.yaw, self.pitch)
    }

    pub fn get_basis(&self) -> (Vec3, Vec3, Vec3) {
        camera_basis(self.yaw, self.pitch)
    }

    pub fn move_target(&mut self, dx: f32, dy: f32) {
        let (right, up, _) = self.get_basis();
        let speed = self.distance * 0.01;

        self.target += right * dx * speed + up * dy * speed;
    }

    pub fn translate_free(&mut self, movement: Vec3) {
        let (right, up, forward) = self.get_basis();

        self.target += right * movement.x + up * movement.y + forward * movement.z;
    }

    pub fn frame_bounds(&mut self, min: Vec3, max: Vec3) {
        let center = (min + max) * 0.5;
        self.target = center;

        let extents = (max - min) * 0.5;
        let radius = extents.length().max(0.5);

        const FOV_HALF_ANGLE_RAD: f32 = 30.0 * std::f32::consts::PI / 180.0;
        const MARGIN_FACTOR: f32 = 1.25;

        let distance = (radius / FOV_HALF_ANGLE_RAD.sin()) * MARGIN_FACTOR;
        self.distance = distance.clamp(2.0, 100.0);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CameraMode {
    ThirdPerson,
    FirstPerson,
}

pub struct GameplayCamera {
    pub mode: CameraMode,
    pub distance: f32,
    pub height: f32,
    pub look_height: f32,

    pub yaw: f32,
    pub pitch: f32,

    pub current_position: Vec3,
    pub current_target: Vec3,
    pub spring_arm: super::camera_spring_arm::SpringArmState,
}

impl Default for GameplayCamera {
    fn default() -> Self {
        Self::new()
    }
}

impl GameplayCamera {
    pub fn new() -> Self {
        Self {
            mode: CameraMode::ThirdPerson,
            distance: DEFAULT_DISTANCE,
            height: DEFAULT_HEIGHT,
            look_height: LOOK_HEIGHT,

            yaw: 45.0_f32.to_radians(),
            pitch: 20.0_f32.to_radians(),

            current_position: Vec3::ZERO,
            current_target: Vec3::ZERO,
            spring_arm: Default::default(),
        }
    }

    pub fn set_mode_from_name(&mut self, name: &str) {
        self.mode = match name {
            "firstPerson" => CameraMode::FirstPerson,
            _ => CameraMode::ThirdPerson,
        };
    }
    pub fn swim_aim_y(&self) -> f32 {
        match self.mode {
            CameraMode::FirstPerson => self.pitch.sin(),
            CameraMode::ThirdPerson => (20.0_f32.to_radians() - self.pitch).sin(),
        }
    }

    pub fn orbit(&mut self, dx: f32, dy: f32) {
        if !dx.is_finite() || !dy.is_finite() {
            return;
        }
        self.yaw = (self.yaw - dx * ORBIT_SENSITIVITY).rem_euclid(std::f32::consts::TAU);
        self.pitch =
            (self.pitch + dy * ORBIT_SENSITIVITY).clamp(GAMEPLAY_MIN_PITCH, GAMEPLAY_MAX_PITCH);
    }

    pub fn get_horizontal_basis(&self) -> (Vec3, Vec3) {
        let sin_yaw = self.yaw.sin();
        let cos_yaw = self.yaw.cos();

        let forward = Vec3::new(-sin_yaw, 0.0, -cos_yaw);
        let right = Vec3::new(cos_yaw, 0.0, -sin_yaw);

        (forward, right)
    }

    pub fn update(&mut self, character: &crate::character::Character, world: &World) {
        let character_position = character.transform.position;

        match self.mode {
            CameraMode::ThirdPerson => {
                self.current_target = character_position + Vec3::Y * self.look_height;

                let desired_position = self.current_target
                    + orbit_offset(self.distance, self.yaw, self.pitch)
                    + Vec3::Y * (self.height - self.look_height);

                self.current_position =
                    self.resolve_collision(self.current_target, desired_position, world);
            }

            CameraMode::FirstPerson => {
                self.current_position = character_position + Vec3::Y * FIRST_PERSON_HEIGHT;

                let forward = first_person_forward(self.yaw, self.pitch);
                self.current_target = self.current_position + forward;
            }
        }
    }

    pub fn resolve_collision(&self, target: Vec3, desired: Vec3, world: &World) -> Vec3 {
        let offset = desired - target;
        let distance = offset.length();

        if distance <= f32::EPSILON {
            return desired;
        }

        let direction = offset / distance;
        let mut current_distance = CAMERA_COLLISION_STEP;

        while current_distance < distance {
            let point = target + direction * current_distance;

            let coord = WorldCoord::new(
                point.x.floor() as i32,
                point.y.floor() as i32,
                point.z.floor() as i32,
            );

            if world.get(coord).is_some() && world.is_cell_solid(coord) {
                return target + direction * (current_distance - CAMERA_COLLISION_MARGIN).max(0.0);
            }

            current_distance += CAMERA_COLLISION_STEP;
        }

        desired
    }

    pub fn get_view_matrix(&self) -> Mat4 {
        safe_camera_view(
            self.current_position,
            self.current_target,
            self.yaw,
            self.pitch,
        )
    }

    pub fn get_basis(&self) -> (Vec3, Vec3, Vec3) {
        camera_basis(self.yaw, self.pitch)
    }
}

fn safe_camera_view(eye: Vec3, target: Vec3, yaw: f32, pitch: f32) -> Mat4 {
    let eye = if eye.is_finite() { eye } else { Vec3::ZERO };
    let direction = target - eye;
    let fallback = if yaw.is_finite() && pitch.is_finite() {
        first_person_forward(yaw, pitch)
    } else {
        Vec3::NEG_Z
    };
    let forward = if direction.is_finite() && direction.length_squared() > 1.0e-10 {
        direction.normalize()
    } else {
        fallback
    };
    let forward = if forward.is_finite() && forward.length_squared() > 0.9 {
        forward
    } else {
        Vec3::NEG_Z
    };
    let up = if forward.dot(Vec3::Y).abs() > 0.999 {
        Vec3::Z
    } else {
        Vec3::Y
    };
    Mat4::look_at_rh(eye, eye + forward, up)
}

fn orbit_offset(distance: f32, yaw: f32, pitch: f32) -> Vec3 {
    let cos_pitch = pitch.cos();

    Vec3::new(
        distance * cos_pitch * yaw.sin(),
        distance * pitch.sin(),
        distance * cos_pitch * yaw.cos(),
    )
}

fn camera_basis(yaw: f32, pitch: f32) -> (Vec3, Vec3, Vec3) {
    let sin_yaw = yaw.sin();
    let cos_yaw = yaw.cos();
    let sin_pitch = pitch.sin();
    let cos_pitch = pitch.cos();

    let forward = Vec3::new(-cos_pitch * sin_yaw, -sin_pitch, -cos_pitch * cos_yaw).normalize();

    let right = Vec3::new(cos_yaw, 0.0, -sin_yaw).normalize();
    let up = right.cross(forward).normalize();

    (right, up, forward)
}

fn first_person_forward(yaw: f32, pitch: f32) -> Vec3 {
    let cos_pitch = pitch.cos();
    let sin_pitch = pitch.sin();

    Vec3::new(-cos_pitch * yaw.sin(), sin_pitch, -cos_pitch * yaw.cos()).normalize()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn coincident_camera_target_has_finite_view_without_moving_eye() {
        let mut camera = GameplayCamera::new();
        camera.current_position = Vec3::new(3.0, 2.0, -8.0);
        camera.current_target = camera.current_position;
        camera.yaw = 1.25;
        camera.pitch = 0.3;
        let view = camera.get_view_matrix();
        assert!(view.is_finite());
        assert_eq!(camera.current_position, camera.current_target);
        assert!(view.transform_point3(camera.current_position).length() < 0.001);
    }

    #[test]
    fn wk_swim_chase_camera_default_is_neutral_and_aim_controls_dive() {
        let mut camera = GameplayCamera::new();
        assert_eq!(camera.mode, CameraMode::ThirdPerson);
        assert!(camera.swim_aim_y().abs() < 1.0e-6);
        camera.pitch = 50.0_f32.to_radians();
        assert!(camera.swim_aim_y() < -0.258_819_05);
        camera.pitch = 0.0;
        assert!(camera.swim_aim_y() > 0.0);
        camera.mode = CameraMode::FirstPerson;
        camera.pitch = (-30.0_f32).to_radians();
        assert!(camera.swim_aim_y() < -0.25);
        camera.pitch = 30.0_f32.to_radians();
        assert!(camera.swim_aim_y() > 0.25);
    }

    #[test]
    fn gameplay_orbit_discards_invalid_input_and_bounds_angles() {
        let mut camera = GameplayCamera::new();
        let original = (camera.yaw, camera.pitch);
        for (dx, dy) in [
            (f32::NAN, 0.0),
            (0.0, f32::INFINITY),
            (f32::NEG_INFINITY, 5.0),
        ] {
            camera.orbit(dx, dy);
            assert_eq!((camera.yaw, camera.pitch), original);
        }
        for (dx, dy) in [(1_000_000.0, 500_000.0), (-1_000_000.0, -500_000.0)] {
            camera.orbit(dx, dy);
            assert!(camera.yaw.is_finite() && camera.yaw >= 0.0);
            assert!(camera.yaw < std::f32::consts::TAU);
            assert!(camera.pitch >= GAMEPLAY_MIN_PITCH && camera.pitch <= GAMEPLAY_MAX_PITCH);
            let (right, up, forward) = camera.get_basis();
            assert!(right.is_finite() && up.is_finite() && forward.is_finite());
        }
    }

    #[test]
    fn test_frame_bounds_single_cell() {
        let mut camera = CameraController::new();
        let initial_yaw = camera.yaw;
        let initial_pitch = camera.pitch;

        let min = Vec3::new(0.0, 0.0, 0.0);
        let max = Vec3::new(1.0, 1.0, 1.0);

        camera.frame_bounds(min, max);

        assert_eq!(camera.target, Vec3::new(0.5, 0.5, 0.5));
        assert_eq!(camera.yaw, initial_yaw);
        assert_eq!(camera.pitch, initial_pitch);
        assert!(camera.distance > 2.0 && camera.distance < 5.0);
    }

    #[test]
    fn test_frame_bounds_scales_with_size() {
        let mut camera_small = CameraController::new();
        camera_small.frame_bounds(Vec3::ZERO, Vec3::ONE);

        let mut camera_large = CameraController::new();
        camera_large.frame_bounds(Vec3::ZERO, Vec3::new(20.0, 10.0, 20.0));

        assert!(camera_large.distance > camera_small.distance);
        assert_eq!(camera_large.target, Vec3::new(10.0, 5.0, 10.0));
    }

    #[test]
    fn test_frame_bounds_clamping() {
        let mut camera_tiny = CameraController::new();
        camera_tiny.frame_bounds(Vec3::ZERO, Vec3::splat(0.001));
        assert!(camera_tiny.distance >= 2.0);

        let mut camera_huge = CameraController::new();
        camera_huge.frame_bounds(Vec3::ZERO, Vec3::splat(1000.0));
        assert!(camera_huge.distance <= 100.0);
    }
}
