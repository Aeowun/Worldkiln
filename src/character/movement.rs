// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementState {
    Idle,
    Walk,
    Run,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub enum SwimMode {
    #[default]
    Dry,
    Underwater,
    Surface,
}

pub const MOVE_SPEED: f32 = 4.0;
pub const JUMP_IMPULSE: f32 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterMovement {
    pub velocity: Vec3,
    pub gravity_scale: f32,
    pub controller_enabled: bool,
    pub slope_slide_velocity: Vec3,
    pub is_grounded: bool,
    pub is_swimming: bool,
    pub swim_mode: SwimMode,
    pub surface_lock_blend: f32,
    pub surface_jump_remaining: f32,
    pub swim_look_y: f32,
    pub swim_camera_forward: glam::Vec2,
    pub relative_density: f32,
    pub state: MovementState,
}

impl Default for CharacterMovement {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            gravity_scale: 1.0,
            controller_enabled: true,
            slope_slide_velocity: Vec3::ZERO,
            is_grounded: false,
            is_swimming: false,
            swim_mode: SwimMode::Dry,
            surface_lock_blend: 0.0,
            surface_jump_remaining: 0.0,
            swim_look_y: 0.0,
            swim_camera_forward: glam::Vec2::ZERO,
            relative_density: 0.93,
            state: MovementState::Idle,
        }
    }
}

impl CharacterMovement {
    pub fn new() -> Self {
        Self::default()
    }
}
