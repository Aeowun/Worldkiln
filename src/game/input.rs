// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

use super::runtime::GameRuntime;
use crate::scripting::value::Value;
use winit::keyboard::KeyCode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GameMouseButton {
    Left,
    Right,
    Middle,
}
impl GameMouseButton {
    fn state(self) -> (u8, &'static str) {
        match self {
            Self::Left => (1, "Left"),
            Self::Right => (2, "Right"),
            Self::Middle => (4, "Middle"),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum GameInputEvent {
    Resize {
        width: u32,
        height: u32,
    },
    Key {
        code: KeyCode,
        pressed: bool,
        repeat: bool,
        captured_by_ui: bool,
    },
    FocusLost,
    MouseButton {
        button: GameMouseButton,
        pressed: bool,
        captured_by_ui: bool,
    },
    PointerMotion {
        dx: f64,
        dy: f64,
    },
    GamepadButton {
        device: u32,
        button: u8,
        pressed: bool,
    },
    GamepadAxis {
        device: u32,
        axis: u8,
        value: f32,
    },
    GamepadDisconnected {
        device: u32,
    },
}

fn accept_mouse_press(pressed: bool, already_owned: bool, captured: bool) -> bool {
    pressed && !already_owned && !captured
}

impl GameRuntime {
    pub fn on_input(&mut self, event: GameInputEvent) {
        match event {
            GameInputEvent::Resize { width, height } => {
                self.renderer.resize(width as f32, height as f32);
            }
            GameInputEvent::Key {
                code,
                pressed,
                repeat,
                captured_by_ui,
            } => {
                self.input_actions.set_key(code, pressed, captured_by_ui);
                if pressed {
                    if captured_by_ui {
                        return;
                    }
                    let was_already_down = !self.keys_down.insert(code);
                    if !repeat && !was_already_down && self.script_scene.is_some() {
                        self.script_pending_events.push((
                            "keypress".to_string(),
                            vec![
                                Value::String(crate::scripting::host::key_code_name(code)),
                                Value::Bool(true),
                            ],
                        ));
                    }
                    if code == KeyCode::Space {
                        self.jump_requested = true;
                    }
                } else {
                    let was_down = self.keys_down.remove(&code);
                    if was_down && self.script_scene.is_some() {
                        self.script_pending_events.push((
                            "keypress".to_string(),
                            vec![
                                Value::String(crate::scripting::host::key_code_name(code)),
                                Value::Bool(false),
                            ],
                        ));
                    }
                }
            }
            GameInputEvent::FocusLost => {
                self.input_actions.focus_lost();
                let held_keys: Vec<_> = self.keys_down.drain().collect();
                if self.script_scene.is_some() {
                    for key in held_keys {
                        self.script_pending_events.push((
                            "keypress".to_string(),
                            vec![
                                Value::String(crate::scripting::host::key_code_name(key)),
                                Value::Bool(false),
                            ],
                        ));
                    }
                    for (bit, name) in [(1, "Left"), (2, "Right"), (4, "Middle")] {
                        if self.gameplay_mouse_buttons & bit != 0 {
                            self.script_pending_events.push((
                                "mousepress".to_string(),
                                vec![Value::String(name.into()), Value::Bool(false)],
                            ));
                        }
                    }
                }
                self.gameplay_mouse_buttons = 0;
                self.jump_requested = false;
                self.orbit_delta = [0.0, 0.0];
            }
            GameInputEvent::MouseButton {
                button,
                pressed,
                captured_by_ui,
            } => {
                let (bit, name) = button.state();
                self.input_actions.set_mouse_button(bit, pressed, captured_by_ui);
                let was_owned = self.gameplay_mouse_buttons & bit != 0;
                if accept_mouse_press(pressed, was_owned, captured_by_ui) {
                    self.gameplay_mouse_buttons |= bit;
                    if self.script_scene.is_some() {
                        self.script_pending_events.push((
                            "mousepress".to_string(),
                            vec![Value::String(name.into()), Value::Bool(true)],
                        ));
                    }
                } else if !pressed && was_owned {
                    self.gameplay_mouse_buttons &= !bit;
                    if self.script_scene.is_some() {
                        self.script_pending_events.push((
                            "mousepress".to_string(),
                            vec![Value::String(name.into()), Value::Bool(false)],
                        ));
                    }
                }
            }
            GameInputEvent::PointerMotion { dx, dy } => {
                if self.mouse.mode().uses_relative_motion() {
                    self.orbit_delta[0] += dx as f32;
                    self.orbit_delta[1] += dy as f32;
                }
            }
            GameInputEvent::GamepadButton { device, button, pressed } => {
                self.input_actions.set_gamepad_button(device, button, pressed);
            }
            GameInputEvent::GamepadAxis { device, axis, value } => {
                self.input_actions.set_gamepad_axis(device, axis, value);
            }
            GameInputEvent::GamepadDisconnected { device } => {
                self.input_actions.disconnect_gamepad(device);
            }
        }
    }
}
