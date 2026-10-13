use winit::event::{ElementState, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::scripting::value::Value;

use super::runtime::GameRuntime;

impl GameRuntime {
    pub fn on_window_event(
        &mut self,
        event: &WindowEvent,
        _egui_ctx: &egui::Context,
        _window: &winit::window::Window,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                self.renderer.resize(size.width as f32, size.height as f32);
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if let PhysicalKey::Code(key) = key_event.physical_key {
                    let was_pressed = key_event.state == ElementState::Pressed;

                    if was_pressed {
                        // Escape closes the standalone built game.
                        if key == KeyCode::Escape {
                            std::process::exit(0);
                        }

                        let was_already_down = !self.keys_down.insert(key);

                        if !key_event.repeat && !was_already_down && self.script_scene.is_some() {
                            self.script_pending_events.push((
                                "keypress".to_string(),
                                vec![
                                    Value::String(crate::scripting::host::key_code_name(key)),
                                    Value::Bool(true),
                                ],
                            ));
                        }

                        if key == KeyCode::Space {
                            self.jump_requested = true;
                        }
                    } else {
                        let was_down = self.keys_down.remove(&key);

                        if was_down && self.script_scene.is_some() {
                            self.script_pending_events.push((
                                "keypress".to_string(),
                                vec![
                                    Value::String(crate::scripting::host::key_code_name(key)),
                                    Value::Bool(false),
                                ],
                            ));
                        }
                    }
                }
            }

            WindowEvent::Focused(false) => {
                let held_keys: Vec<_> = self.keys_down.drain().collect();

                for key in held_keys {
                    if self.script_scene.is_some() {
                        self.script_pending_events.push((
                            "keypress".to_string(),
                            vec![
                                Value::String(crate::scripting::host::key_code_name(key)),
                                Value::Bool(false),
                            ],
                        ));
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if *button == winit::event::MouseButton::Right {
                    if *state == ElementState::Pressed {
                        self.mouse.screen_locked = true;
                        self.mouse.cursor_visible = false;
                    } else {
                        self.mouse.screen_locked = false;
                        self.mouse.cursor_visible = true;
                    }
                }
            }

            _ => {}
        }
    }

    pub fn on_mouse_motion(&mut self, dx: f64, dy: f64) {
        if self.mouse.screen_locked {
            self.orbit_delta[0] += dx as f32;
            self.orbit_delta[1] += dy as f32;
        }
    }
}
