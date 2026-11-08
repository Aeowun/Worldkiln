// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::MouseMode;
use std::time::{Duration, Instant};
use winit::window::{CursorGrabMode, Window};

trait CursorPlatform {
    fn set_grab(&self, mode: CursorGrabMode) -> Result<(), String>;
    fn set_visible(&self, visible: bool);
}

impl CursorPlatform for Window {
    fn set_grab(&self, mode: CursorGrabMode) -> Result<(), String> {
        self.set_cursor_grab(mode)
            .map_err(|error| error.to_string())
    }

    fn set_visible(&self, visible: bool) {
        self.set_cursor_visible(visible);
    }
}

/// Sole owner of OS cursor capture. An authored mouse request never changes on focus loss.
/// A failed native grab never masquerades as an active captured cursor.
#[derive(Debug, Default)]
pub struct WindowMouseCapture {
    focused: bool,
    applied_request: Option<MouseMode>,
    retry_after: Option<Instant>,
    last_error: Option<String>,
}

impl WindowMouseCapture {
    pub fn focused(&self) -> bool {
        self.focused
    }

    pub fn focus_changed(&mut self, focused: bool) {
        self.focused = focused;
        self.applied_request = None;
        self.retry_after = None;
        self.last_error = None;
    }

    pub fn effective_mode(&self, desired: MouseMode) -> MouseMode {
        if self.focused {
            desired
        } else {
            MouseMode::Visible
        }
    }

    pub fn is_relative(&self, desired: MouseMode) -> bool {
        self.focused
            && desired == MouseMode::Captured
            && self.applied_request == Some(MouseMode::Captured)
    }

    pub fn apply(&mut self, window: &Window, desired: MouseMode) -> Result<(), String> {
        self.apply_to(window, desired)
    }

    fn apply_to(&mut self, window: &impl CursorPlatform, desired: MouseMode) -> Result<(), String> {
        let effective = self.effective_mode(desired);
        if self.applied_request == Some(effective) {
            return Ok(());
        }
        if self.retry_after.is_some_and(|retry| Instant::now() < retry) {
            return Ok(());
        }
        let grab = match effective {
            MouseMode::Visible | MouseMode::Hidden => CursorGrabMode::None,
            MouseMode::Captured => CursorGrabMode::Locked,
            MouseMode::Confined | MouseMode::ConfinedHidden => CursorGrabMode::Confined,
        };
        match window.set_grab(grab) {
            Ok(()) => {
                window.set_visible(effective.cursor_visible());
                self.applied_request = Some(effective);
                self.retry_after = None;
                self.last_error = None;
                Ok(())
            }
            Err(error) => {
                let _ = window.set_grab(CursorGrabMode::None);
                window.set_visible(true);
                self.applied_request = None;
                self.retry_after = Some(Instant::now() + Duration::from_millis(150));
                let message = format!("Could not apply mouse mode {}: {error}", effective.as_str());
                if self.last_error.as_deref() == Some(&message) {
                    Ok(())
                } else {
                    self.last_error = Some(message.clone());
                    Err(message)
                }
            }
        }
    }

    pub fn release(&mut self, window: &Window) {
        self.release_to(window);
    }

    fn release_to(&mut self, window: &impl CursorPlatform) {
        let _ = window.set_grab(CursorGrabMode::None);
        window.set_visible(true);
        self.applied_request = None;
        self.retry_after = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::{Cell, RefCell};

    #[derive(Default)]
    struct MockCursor {
        grabs: RefCell<Vec<CursorGrabMode>>,
        visible: Cell<bool>,
        fail_capture: Cell<bool>,
    }

    impl CursorPlatform for MockCursor {
        fn set_grab(&self, mode: CursorGrabMode) -> Result<(), String> {
            self.grabs.borrow_mut().push(mode);
            if self.fail_capture.get() && mode == CursorGrabMode::Locked {
                Err("native capture rejected".into())
            } else {
                Ok(())
            }
        }

        fn set_visible(&self, visible: bool) {
            self.visible.set(visible);
        }
    }

    #[test]
    fn capture_reapplies_after_focus_cycle_without_mutating_game_request() {
        let mut manager = WindowMouseCapture::default();
        let native = MockCursor::default();
        let desired = MouseMode::Captured;
        manager.focus_changed(true);
        assert!(!manager.is_relative(desired));
        manager.apply_to(&native, desired).unwrap();
        assert!(manager.is_relative(desired));
        manager.focus_changed(false);
        manager.release_to(&native);
        assert!(!manager.is_relative(desired));
        assert_eq!(manager.effective_mode(desired), MouseMode::Visible);
        manager.focus_changed(true);
        manager.apply_to(&native, desired).unwrap();
        assert!(manager.is_relative(desired));
        assert_eq!(
            native.grabs.borrow().as_slice(),
            &[
                CursorGrabMode::Locked,
                CursorGrabMode::None,
                CursorGrabMode::Locked
            ]
        );
    }

    #[test]
    fn visible_menu_is_never_captured_even_after_window_refocus() {
        let mut manager = WindowMouseCapture::default();
        let native = MockCursor::default();
        manager.focus_changed(true);
        manager.apply_to(&native, MouseMode::Visible).unwrap();
        manager.focus_changed(false);
        manager.release_to(&native);
        manager.focus_changed(true);
        manager.apply_to(&native, MouseMode::Visible).unwrap();
        assert!(native.visible.get());
        assert!(!manager.is_relative(MouseMode::Visible));
        assert!(
            native
                .grabs
                .borrow()
                .iter()
                .all(|grab| *grab == CursorGrabMode::None)
        );
    }

    #[test]
    fn rejected_grab_cannot_activate_relative_gameplay_motion() {
        let mut manager = WindowMouseCapture::default();
        let native = MockCursor::default();
        native.fail_capture.set(true);
        manager.focus_changed(true);
        let error = manager.apply_to(&native, MouseMode::Captured).unwrap_err();
        assert!(error.contains("native capture rejected"));
        assert!(native.visible.get());
        assert!(!manager.is_relative(MouseMode::Captured));
        assert_eq!(native.grabs.borrow().last(), Some(&CursorGrabMode::None));
    }

    #[test]
    fn captured_mode_alone_receives_relative_input() {
        let mut manager = WindowMouseCapture::default();
        let native = MockCursor::default();
        manager.focus_changed(true);
        manager.apply_to(&native, MouseMode::Captured).unwrap();
        assert!(manager.is_relative(MouseMode::Captured));
        assert!(!manager.is_relative(MouseMode::Hidden));
        assert!(!manager.is_relative(MouseMode::Confined));
    }
}
