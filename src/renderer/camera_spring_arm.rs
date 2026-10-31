// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use glam::Vec3;

#[derive(Clone, Copy, Debug)]
pub struct SpringArmInput {
    pub target: Vec3,
    pub offset: Vec3,
    pub delta_time: f32,
    pub follow_speed: f32,
    pub rotation_speed: f32,
    pub recovery_speed: f32,
    pub look_ahead: Vec3,
    pub dead_zone: [f32; 2],
    pub collision: bool,
    pub collision_padding: f32,
    pub reset: bool,
}
impl Default for SpringArmInput {
    fn default() -> Self {
        Self {
            target: Vec3::ZERO,
            offset: Vec3::new(0.0, 2.0, 6.0),
            delta_time: 1.0 / 60.0,
            follow_speed: 8.0,
            rotation_speed: 12.0,
            recovery_speed: 10.0,
            look_ahead: Vec3::ZERO,
            dead_zone: [0.0, 0.0],
            collision: true,
            collision_padding: 0.1,
            reset: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct SpringArmOutput {
    pub position: Vec3,
    pub target: Vec3,
    pub obstructed: bool,
}
#[derive(Clone, Debug, Default)]
pub struct SpringArmState {
    last_target: Option<Vec3>,
    last_offset: Option<Vec3>,
    last_arm_length: Option<f32>,
}
impl SpringArmState {
    pub fn reset(&mut self) {
        self.last_target = None;
        self.last_offset = None;
        self.last_arm_length = None;
    }
    pub fn update(
        &mut self,
        input: SpringArmInput,
        viewport: [f32; 2],
    ) -> Result<SpringArmOutput, String> {
        let points = [input.target, input.offset, input.look_ahead];
        if points.iter().any(|p| !p.is_finite())
            || !input.delta_time.is_finite()
            || input.delta_time < 0.0
            || input.delta_time > 1.0
            || !input.follow_speed.is_finite()
            || !(0.0..=120.0).contains(&input.follow_speed)
            || !input.rotation_speed.is_finite()
            || !(0.0..=120.0).contains(&input.rotation_speed)
            || !input.recovery_speed.is_finite()
            || !(0.0..=120.0).contains(&input.recovery_speed)
            || input
                .dead_zone
                .iter()
                .any(|v| !v.is_finite() || !(0.0..=0.95).contains(v))
            || !input.collision_padding.is_finite()
            || !(0.0..=5.0).contains(&input.collision_padding)
            || input.offset.length_squared() < 1.0e-6
            || input.offset.length() > 5000.0
        {
            return Err(
                "camera.spring_arm received invalid, non-finite or out-of-range settings".into(),
            );
        }
        if input.reset {
            self.reset();
        }
        let desired = input.target + input.look_ahead;
        let mut pivot = self.last_target.unwrap_or(desired);
        let current_offset = self.last_offset.unwrap_or(input.offset);
        let forward = (-current_offset).normalize_or_zero();
        let right = forward.cross(Vec3::Y).normalize_or_zero();
        let right = if right.length_squared() > 0.5 {
            right
        } else {
            Vec3::X
        };
        let up = right.cross(forward).normalize_or_zero();
        let diff = desired - pivot;
        let depth = current_offset.length().max(0.05);
        let half_height = (30.0_f32.to_radians().tan() * depth).max(0.01);
        let aspect = if viewport[0].is_finite()
            && viewport[1].is_finite()
            && viewport[0] > 1.0
            && viewport[1] > 1.0
        {
            (viewport[0] / viewport[1]).clamp(0.25, 8.0)
        } else {
            16.0 / 9.0
        };
        let width_limit = input.dead_zone[0] * half_height * aspect;
        let height_limit = input.dead_zone[1] * half_height;
        let right_component = diff.dot(right);
        let up_component = diff.dot(up);
        let clip = |value: f32, limit: f32| value.signum() * (value.abs() - limit).max(0.0);
        let correction = right * clip(right_component, width_limit)
            + up * clip(up_component, height_limit)
            + forward * diff.dot(forward);
        let alpha = |speed: f32| {
            if speed == 0.0 {
                1.0
            } else {
                1.0 - (-speed * input.delta_time).exp()
            }
        };
        if self.last_target.is_some() {
            pivot += correction * alpha(input.follow_speed);
        } else {
            pivot = desired;
        }
        let offset = if self.last_offset.is_some() {
            current_offset.lerp(input.offset, alpha(input.rotation_speed))
        } else {
            input.offset
        };
        self.last_target = Some(pivot);
        self.last_offset = Some(offset);
        Ok(SpringArmOutput {
            position: pivot + offset,
            target: pivot,
            obstructed: false,
        })
    }

    pub fn finish_collision(
        &mut self,
        mut output: SpringArmOutput,
        safe: Vec3,
        input: SpringArmInput,
    ) -> SpringArmOutput {
        if !input.collision {
            self.last_arm_length = None;
            return output;
        }
        let desired_vector = output.position - output.target;
        let desired_length = desired_vector.length();
        if desired_length < 1.0e-5 {
            return output;
        }
        let blocked = safe.distance_squared(output.position) > 1.0e-6;
        let allowed = if blocked {
            (safe.distance(output.target) - input.collision_padding).max(0.0)
        } else {
            desired_length
        };
        let previous = self
            .last_arm_length
            .unwrap_or(desired_length)
            .min(desired_length);
        let extent = if allowed < previous {
            allowed
        } else if input.recovery_speed == 0.0 {
            allowed
        } else {
            allowed - (allowed - previous) * (-input.recovery_speed * input.delta_time).exp()
        };
        self.last_arm_length = Some(extent);
        output.position = output.target + desired_vector / desired_length * extent;
        output.obstructed = blocked;
        output
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn follows_with_frame_rate_independent_lag() {
        let mut a = SpringArmState::default();
        let mut b = SpringArmState::default();
        let mut inp = SpringArmInput::default();
        a.update(inp, [1600.0, 900.0]).unwrap();
        b.update(inp, [1600.0, 900.0]).unwrap();
        inp.target.x = 10.0;
        inp.delta_time = 1.0 / 60.0;
        for _ in 0..60 {
            a.update(inp, [1600.0, 900.0]).unwrap();
        }
        inp.delta_time = 1.0 / 30.0;
        for _ in 0..30 {
            b.update(inp, [1600.0, 900.0]).unwrap();
        }
        let x = a.last_target.unwrap().x;
        let y = b.last_target.unwrap().x;
        assert!(
            (x - y).abs() < 0.0005,
            "lag must be frame-rate independent: {x}, {y}"
        );
        assert!(x > 9.9 && x < 10.0);
    }
    #[test]
    fn dead_zone_and_look_ahead_do_not_need_a_player_character() {
        let mut arm = SpringArmState::default();
        let mut i = SpringArmInput::default();
        i.dead_zone = [0.3, 0.3];
        arm.update(i, [1200.0, 800.0]).unwrap();
        i.target.x = 0.05;
        let still = arm.update(i, [1200.0, 800.0]).unwrap();
        assert!(
            still.target.x.abs() < 0.001,
            "inside screen zone should not pan"
        );
        i.target.x = 15.0;
        let moved = arm.update(i, [1200.0, 800.0]).unwrap();
        assert!(moved.target.x > 0.0);
        i.look_ahead.x = 2.0;
        i.reset = true;
        let prediction = arm.update(i, [1200.0, 800.0]).unwrap();
        assert!((prediction.target.x - 17.0).abs() < 0.001);
    }
    #[test]
    fn collision_retracts_immediately_and_returns_smoothly() {
        let mut arm = SpringArmState::default();
        let input = SpringArmInput::default();
        let desired = arm.update(input, [1600.0, 900.0]).unwrap();
        let axis = (desired.position - desired.target).normalize();
        let blocked = arm.finish_collision(desired, desired.target + axis * 1.5, input);
        assert!(blocked.obstructed);
        assert!(blocked.position.distance(blocked.target) < 1.5);
        let raw = arm.update(input, [1600.0, 900.0]).unwrap();
        let recovered = arm.finish_collision(raw, raw.position, input);
        let blocked_distance = blocked.position.distance(blocked.target);
        let recovered_distance = recovered.position.distance(recovered.target);
        assert!(!recovered.obstructed);
        assert!(
            recovered_distance > blocked_distance,
            "distance must start extending"
        );
        assert!(
            recovered_distance < raw.position.distance(raw.target) - 0.1,
            "camera must not snap instantly through remaining distance"
        );
        let mut output = recovered;
        for _ in 0..300 {
            let raw = arm.update(input, [1600.0, 900.0]).unwrap();
            output = arm.finish_collision(raw, raw.position, input);
        }
        assert!((output.position.distance(output.target) - input.offset.length()).abs() < 0.001);
        let mut disabled = input;
        disabled.collision = false;
        let raw = arm.update(disabled, [1600.0, 900.0]).unwrap();
        let instant = arm.finish_collision(raw, raw.target, disabled);
        assert!(!instant.obstructed);
        assert_eq!(instant.position, raw.position);
    }
    #[test]
    fn rejects_invalid_input_and_resets() {
        let mut arm = SpringArmState::default();
        let i = SpringArmInput::default();
        arm.update(i, [800.0, 600.0]).unwrap();
        let mut bad = i;
        bad.follow_speed = f32::NAN;
        assert!(arm.update(bad, [800.0, 600.0]).is_err());
        arm.reset();
        assert!(arm.last_target.is_none());
        let mut reset = i;
        reset.target.x = 99.0;
        assert!((arm.update(reset, [800.0, 600.0]).unwrap().target.x - 99.0).abs() < 0.001);
    }
}
