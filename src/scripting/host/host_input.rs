// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use winit::keyboard::KeyCode;

use super::ScriptHostBridge;

pub fn key_code_name(key: KeyCode) -> String {
    match key {
        KeyCode::KeyA => "A",
        KeyCode::KeyB => "B",
        KeyCode::KeyC => "C",
        KeyCode::KeyD => "D",
        KeyCode::KeyE => "E",
        KeyCode::KeyF => "F",
        KeyCode::KeyG => "G",
        KeyCode::KeyH => "H",
        KeyCode::KeyI => "I",
        KeyCode::KeyJ => "J",
        KeyCode::KeyK => "K",
        KeyCode::KeyL => "L",
        KeyCode::KeyM => "M",
        KeyCode::KeyN => "N",
        KeyCode::KeyO => "O",
        KeyCode::KeyP => "P",
        KeyCode::KeyQ => "Q",
        KeyCode::KeyR => "R",
        KeyCode::KeyS => "S",
        KeyCode::KeyT => "T",
        KeyCode::KeyU => "U",
        KeyCode::KeyV => "V",
        KeyCode::KeyW => "W",
        KeyCode::KeyX => "X",
        KeyCode::KeyY => "Y",
        KeyCode::KeyZ => "Z",
        KeyCode::Digit0 => "0",
        KeyCode::Digit1 => "1",
        KeyCode::Digit2 => "2",
        KeyCode::Digit3 => "3",
        KeyCode::Digit4 => "4",
        KeyCode::Digit5 => "5",
        KeyCode::Digit6 => "6",
        KeyCode::Digit7 => "7",
        KeyCode::Digit8 => "8",
        KeyCode::Digit9 => "9",
        _ => return format!("{key:?}"),
    }
    .to_string()
}

fn normalized_move_input(input: glam::Vec2) -> glam::Vec2 {
    let length_squared = input.length_squared();
    if length_squared > 1.0 {
        input / length_squared.sqrt()
    } else {
        input
    }
}

impl<'a> ScriptHostBridge<'a> {
    pub fn get_input_move_vector(&self) -> [f32; 2] {
        normalized_move_input(self.move_input).to_array()
    }

    pub fn is_input_jump_pressed(&self) -> bool {
        self.jump_requested
    }

    pub fn get_input_orbit_delta(&self) -> [f32; 2] {
        self.orbit_delta
    }

    pub fn is_input_key_down(&self, key_name: &str) -> bool {
        self.keys_down.map_or(false, |keys| {
            keys.iter().any(|key| key_code_name(*key) == key_name)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    #[test]
    fn input_diagonal_has_cardinal_speed_and_unit_components() {
        for x in [-1.0f32, 1.0] {
            for y in [-1.0f32, 1.0] {
                let normalized = normalized_move_input(glam::Vec2::new(x, y));
                assert!((normalized.length() - 1.0).abs() < 1e-6);
                assert!((normalized.x.abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
                assert!((normalized.y.abs() - std::f32::consts::FRAC_1_SQRT_2).abs() < 1e-6);
                assert!(((normalized * 5.0).length() - 5.0).abs() < 1e-5);
            }
        }
    }

    #[test]
    fn cardinal_and_analog_movement_preserve_input_magnitude() {
        for input in [
            glam::Vec2::ZERO,
            glam::Vec2::X,
            -glam::Vec2::X,
            glam::Vec2::Y,
            -glam::Vec2::Y,
            glam::Vec2::new(0.3, 0.4),
        ] {
            assert_eq!(normalized_move_input(input), input);
        }
    }

    #[test]
    fn key_names_use_simple_letters_and_digits() {
        assert_eq!(key_code_name(KeyCode::KeyF), "F");
        assert_eq!(key_code_name(KeyCode::KeyW), "W");
        assert_eq!(key_code_name(KeyCode::Digit1), "1");
        assert_eq!(key_code_name(KeyCode::Digit9), "9");
    }

    #[test]
    fn key_names_keep_special_key_names() {
        assert_eq!(key_code_name(KeyCode::Space), "Space");
        assert_eq!(key_code_name(KeyCode::ArrowUp), "ArrowUp");
        assert_eq!(key_code_name(KeyCode::Escape), "Escape");
    }

    #[test]
    fn polling_uses_canonical_key_names() {
        let mut keys = HashSet::new();
        keys.insert(KeyCode::KeyF);
        keys.insert(KeyCode::Space);

        assert!(keys.iter().any(|key| key_code_name(*key) == "F"));
        assert!(keys.iter().any(|key| key_code_name(*key) == "Space"));
        assert!(!keys.iter().any(|key| key_code_name(*key) == "Q"));
    }
}
