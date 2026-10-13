use glam::Vec3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MovementState {
    Idle,
    Walk,
    Run,
}

pub const MOVE_SPEED: f32 = 4.0;
pub const JUMP_IMPULSE: f32 = 5.0;

#[derive(Clone, Debug, PartialEq)]
pub struct CharacterMovement {
    pub velocity: Vec3,
    pub is_grounded: bool,
    pub state: MovementState,
}

impl Default for CharacterMovement {
    fn default() -> Self {
        Self {
            velocity: Vec3::ZERO,
            is_grounded: false,
            state: MovementState::Idle,
        }
    }
}

impl CharacterMovement {
    pub fn new() -> Self {
        Self::default()
    }
}
