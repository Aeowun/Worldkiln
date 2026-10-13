use winit::keyboard::KeyCode;

use super::ScriptHostBridge;

/// Canonical AeoScript name for a physical keyboard key.
///
/// Letters and digits intentionally use compact names such as `F` and `1`.
/// Other keys use their stable `KeyCode` debug name, such as `Space` or
/// `ArrowUp`.
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

impl<'a> ScriptHostBridge<'a> {
    pub fn get_input_move_vector(&self) -> [f32; 2] {
        [self.move_input.x, self.move_input.y]
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
