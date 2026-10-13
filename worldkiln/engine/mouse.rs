#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MouseController {
    pub cursor_visible: bool,
    pub screen_locked: bool,
}

impl Default for MouseController {
    fn default() -> Self {
        Self {
            cursor_visible: true,
            screen_locked: false,
        }
    }
}

impl MouseController {
    pub fn apply_world_defaults(&mut self, cursor_visible: bool, screen_locked: bool) {
        self.cursor_visible = cursor_visible;
        self.screen_locked = screen_locked;
    }

    pub fn set_play_defaults(&mut self) {
        self.cursor_visible = false;
        self.screen_locked = true;
    }

    pub fn set_editor_defaults(&mut self) {
        self.cursor_visible = true;
        self.screen_locked = false;
    }
}
