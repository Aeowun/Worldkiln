// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum MouseMode {
    #[default]
    Visible,
    Hidden,
    Captured,
    Confined,
    ConfinedHidden,
}

impl MouseMode {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Visible => "visible",
            Self::Hidden => "hidden",
            Self::Captured => "captured",
            Self::Confined => "confined",
            Self::ConfinedHidden => "confined_hidden",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.trim().to_ascii_lowercase().as_str() {
            "visible" => Ok(Self::Visible),
            "hidden" => Ok(Self::Hidden),
            "captured" => Ok(Self::Captured),
            "confined" => Ok(Self::Confined),
            "confined_hidden" | "confinedhidden" => Ok(Self::ConfinedHidden),
            _ => Err(format!("Unknown mouse mode: {value}")),
        }
    }

    pub fn cursor_visible(self) -> bool {
        matches!(self, Self::Visible | Self::Confined)
    }

    pub fn screen_locked(self) -> bool {
        matches!(self, Self::Captured | Self::Confined | Self::ConfinedHidden)
    }

    pub fn uses_relative_motion(self) -> bool {
        self == Self::Captured
    }

    pub fn from_legacy(cursor_visible: bool, screen_locked: bool) -> Self {
        match (cursor_visible, screen_locked) {
            (true, false) => Self::Visible,
            (false, false) => Self::Hidden,
            (false, true) => Self::Captured,
            (true, true) => Self::Confined,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct MouseController {
    base_mode: MouseMode,
    override_mode: Option<MouseMode>,
}

impl MouseController {
    pub fn mode(&self) -> MouseMode {
        self.override_mode.unwrap_or(self.base_mode)
    }

    pub fn base_mode(&self) -> MouseMode {
        self.base_mode
    }

    pub fn set_mode(&mut self, mode: MouseMode) {
        self.base_mode = mode;
    }

    pub fn override_mode(&mut self, mode: Option<MouseMode>) {
        self.override_mode = mode;
    }

    pub fn cursor_visible(&self) -> bool {
        self.mode().cursor_visible()
    }

    pub fn screen_locked(&self) -> bool {
        self.mode().screen_locked()
    }

    pub fn set_cursor_visible(&mut self, visible: bool) {
        let current = self.base_mode;
        self.base_mode = match (current, visible) {
            (MouseMode::Captured, true) => MouseMode::Confined,
            (MouseMode::ConfinedHidden, true) => MouseMode::Confined,
            (MouseMode::Confined, false) => MouseMode::ConfinedHidden,
            (MouseMode::Captured, false) => MouseMode::Captured,
            (MouseMode::Confined, true) => MouseMode::Confined,
            (MouseMode::ConfinedHidden, false) => MouseMode::ConfinedHidden,
            (_, true) => MouseMode::Visible,
            (_, false) => MouseMode::Hidden,
        };
    }

    pub fn set_screen_locked(&mut self, locked: bool) {
        self.base_mode = MouseMode::from_legacy(self.base_mode.cursor_visible(), locked);
    }

    pub fn apply_world_defaults(&mut self, cursor_visible: bool, screen_locked: bool) {
        self.base_mode = MouseMode::from_legacy(cursor_visible, screen_locked);
        self.override_mode = None;
    }

    pub fn set_play_defaults(&mut self) {
        self.set_mode(MouseMode::Captured);
        self.override_mode = None;
    }

    pub fn set_editor_defaults(&mut self) {
        self.set_mode(MouseMode::Visible);
        self.override_mode = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn modes_are_complete_and_round_trip() {
        for mode in [
            MouseMode::Visible,
            MouseMode::Hidden,
            MouseMode::Captured,
            MouseMode::Confined,
            MouseMode::ConfinedHidden,
        ] {
            assert_eq!(MouseMode::parse(mode.as_str()), Ok(mode));
        }
        assert!(MouseMode::parse("grabanything").is_err());
    }

    #[test]
    fn old_cursor_settings_translate_without_changing_authored_world_files() {
        assert_eq!(MouseMode::from_legacy(true, false), MouseMode::Visible);
        assert_eq!(MouseMode::from_legacy(false, false), MouseMode::Hidden);
        assert_eq!(MouseMode::from_legacy(false, true), MouseMode::Captured);
        assert_eq!(MouseMode::from_legacy(true, true), MouseMode::Confined);
    }

    #[test]
    fn sequential_legacy_script_properties_remain_compatible() {
        let mut mouse = MouseController::default();
        mouse.set_cursor_visible(false);
        mouse.set_screen_locked(true);
        assert_eq!(mouse.mode(), MouseMode::Captured);
        mouse.set_cursor_visible(true);
        mouse.set_screen_locked(false);
        assert_eq!(mouse.mode(), MouseMode::Visible);
    }

    #[test]
    fn ui_override_does_not_destroy_gameplay_mouse_intent() {
        let mut mouse = MouseController::default();
        mouse.set_mode(MouseMode::Captured);
        mouse.override_mode(Some(MouseMode::Visible));
        assert_eq!(mouse.mode(), MouseMode::Visible);
        assert_eq!(mouse.base_mode(), MouseMode::Captured);
        mouse.override_mode(None);
        assert_eq!(mouse.mode(), MouseMode::Captured);
    }

    #[test]
    fn captured_mouse_is_the_only_relative_pointer_mode() {
        for mode in [
            MouseMode::Visible,
            MouseMode::Hidden,
            MouseMode::Confined,
            MouseMode::ConfinedHidden,
        ] {
            assert!(!mode.uses_relative_motion());
        }
        assert!(MouseMode::Captured.uses_relative_motion());
    }
}
