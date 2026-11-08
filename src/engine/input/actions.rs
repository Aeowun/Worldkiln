// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::collections::{BTreeMap, HashMap};
use winit::keyboard::KeyCode;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum AxisDirection {
    Positive,
    Negative,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum InputBinding {
    Key(KeyCode),
    MouseButton(u8),
    GamepadButton {
        button: u8,
        device: Option<u32>,
    },
    GamepadAxis {
        axis: u8,
        direction: AxisDirection,
        device: Option<u32>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Source {
    Key(KeyCode),
    MouseButton(u8),
    GamepadButton {
        button: u8,
        device: u32,
    },
    GamepadAxis {
        axis: u8,
        direction: AxisDirection,
        device: u32,
    },
}

impl InputBinding {
    fn matches(self, source: Source) -> bool {
        match (self, source) {
            (Self::Key(a), Source::Key(b)) => a == b,
            (Self::MouseButton(a), Source::MouseButton(b)) => a == b,
            (
                Self::GamepadButton {
                    button: a,
                    device: bind_device,
                },
                Source::GamepadButton { button: b, device },
            ) => a == b && bind_device.is_none_or(|id| id == device),
            (
                Self::GamepadAxis {
                    axis: a,
                    direction: da,
                    device: bind_device,
                },
                Source::GamepadAxis {
                    axis: b,
                    direction: db,
                    device,
                },
            ) => a == b && da == db && bind_device.is_none_or(|id| id == device),
            _ => false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct ActionState {
    pub pressed: bool,
    pub just_pressed: bool,
    pub just_released: bool,
    pub strength: f32,
    pub raw_strength: f32,
}

#[derive(Debug)]
struct Action {
    deadzone: f32,
    bindings: Vec<InputBinding>,
    state: ActionState,
}

#[derive(Debug, Default)]
pub struct InputActions {
    actions: BTreeMap<String, Action>,
    sources: HashMap<Source, f32>,
}

impl InputActions {
    pub fn define(&mut self, name: &str, deadzone: f32) -> Result<(), String> {
        if name.is_empty() || name.chars().any(char::is_whitespace) {
            return Err("An action needs a nonempty name without whitespace.".into());
        }
        if !deadzone.is_finite() || !(0.0..1.0).contains(&deadzone) {
            return Err("Action deadzone must be finite and between 0 and 1.".into());
        }
        match self.actions.get_mut(name) {
            Some(action) => action.deadzone = deadzone,
            None => {
                self.actions.insert(
                    name.into(),
                    Action {
                        deadzone,
                        bindings: Vec::new(),
                        state: ActionState::default(),
                    },
                );
            }
        }
        self.recompute();
        Ok(())
    }

    pub fn bind(&mut self, name: &str, binding: InputBinding) -> Result<(), String> {
        let action = self
            .actions
            .get_mut(name)
            .ok_or_else(|| format!("Unknown action: {name}"))?;
        if !action.bindings.contains(&binding) {
            action.bindings.push(binding);
        }
        self.recompute();
        Ok(())
    }

    pub fn unbind(&mut self, name: &str, binding: InputBinding) -> Result<(), String> {
        let action = self
            .actions
            .get_mut(name)
            .ok_or_else(|| format!("Unknown action: {name}"))?;
        action.bindings.retain(|old| *old != binding);
        self.recompute();
        Ok(())
    }

    pub fn state(&self, name: &str) -> Option<ActionState> {
        self.actions.get(name).map(|action| action.state)
    }

    pub fn pressed(&self, name: &str) -> bool {
        self.state(name).is_some_and(|action| action.pressed)
    }

    pub fn just_pressed(&self, name: &str) -> bool {
        self.state(name).is_some_and(|action| action.just_pressed)
    }

    pub fn just_released(&self, name: &str) -> bool {
        self.state(name).is_some_and(|action| action.just_released)
    }

    pub fn strength(&self, name: &str) -> f32 {
        self.state(name).map_or(0.0, |action| action.strength)
    }

    pub fn raw_strength(&self, name: &str) -> f32 {
        self.state(name).map_or(0.0, |action| action.raw_strength)
    }

    pub fn axis(&self, negative_action: &str, positive_action: &str) -> f32 {
        self.strength(positive_action) - self.strength(negative_action)
    }

    pub fn vector(
        &self,
        negative_x: &str,
        positive_x: &str,
        negative_y: &str,
        positive_y: &str,
        deadzone: f32,
    ) -> [f32; 2] {
        let x = self.axis(negative_x, positive_x);
        let y = self.axis(negative_y, positive_y);
        let length = x.hypot(y);
        let deadzone = deadzone.clamp(0.0, 0.99);
        if length <= deadzone {
            return [0.0, 0.0];
        }
        let adjusted_length = ((length - deadzone) / (1.0 - deadzone)).clamp(0.0, 1.0);
        [x / length * adjusted_length, y / length * adjusted_length]
    }

    pub fn set_key(&mut self, key: KeyCode, pressed: bool, ui_captured: bool) {
        self.set_source(Source::Key(key), pressed.then_some(1.0), ui_captured);
    }

    pub fn set_mouse_button(&mut self, button: u8, pressed: bool, ui_captured: bool) {
        self.set_source(
            Source::MouseButton(button),
            pressed.then_some(1.0),
            ui_captured,
        );
    }

    pub fn set_gamepad_button(&mut self, device: u32, button: u8, pressed: bool) {
        self.set_source(
            Source::GamepadButton { device, button },
            pressed.then_some(1.0),
            false,
        );
    }

    pub fn set_gamepad_axis(&mut self, device: u32, axis: u8, value: f32) {
        let value = if value.is_finite() {
            value.clamp(-1.0, 1.0)
        } else {
            0.0
        };
        self.set_source(
            Source::GamepadAxis {
                device,
                axis,
                direction: AxisDirection::Positive,
            },
            (value > 0.0).then_some(value),
            false,
        );
        self.set_source(
            Source::GamepadAxis {
                device,
                axis,
                direction: AxisDirection::Negative,
            },
            (value < 0.0).then_some(-value),
            false,
        );
    }

    pub fn disconnect_gamepad(&mut self, device: u32) {
        self.sources.retain(|source, _| {
            !matches!(source,
                Source::GamepadButton { device: id, .. } | Source::GamepadAxis { device: id, .. }
                    if *id == device
            )
        });
        self.recompute();
    }

    pub fn focus_lost(&mut self) {
        self.sources.clear();
        self.recompute();
    }

    /// End a simulation frame after consumers have observed edge events.
    pub fn finish_frame(&mut self) {
        for action in self.actions.values_mut() {
            action.state.just_pressed = false;
            action.state.just_released = false;
        }
    }

    fn set_source(&mut self, source: Source, value: Option<f32>, ui_captured: bool) {
        let new_value = value.unwrap_or_default();
        if new_value > 0.0 && ui_captured {
            return;
        }
        if new_value > 0.0 {
            self.sources.insert(source, new_value);
        } else {
            self.sources.remove(&source);
        }
        self.recompute();
    }

    fn recompute(&mut self) {
        for action in self.actions.values_mut() {
            let raw = self
                .sources
                .iter()
                .filter(|(source, _)| {
                    action
                        .bindings
                        .iter()
                        .any(|binding| binding.matches(**source))
                })
                .map(|(_, magnitude)| *magnitude)
                .fold(0.0f32, f32::max);
            let strength = if raw <= action.deadzone {
                0.0
            } else {
                ((raw - action.deadzone) / (1.0 - action.deadzone)).clamp(0.0, 1.0)
            };
            let pressed = strength > 0.0;
            if pressed && !action.state.pressed {
                action.state.just_pressed = true;
            }
            if !pressed && action.state.pressed {
                action.state.just_released = true;
            }
            action.state.pressed = pressed;
            action.state.strength = strength;
            action.state.raw_strength = raw;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn multiple_bindings_do_not_release_action_while_other_source_is_held() {
        let mut input = InputActions::default();
        input.define("jump", 0.2).unwrap();
        input
            .bind("jump", InputBinding::Key(KeyCode::Space))
            .unwrap();
        input.bind("jump", InputBinding::MouseButton(1)).unwrap();
        input.set_key(KeyCode::Space, true, false);
        assert!(input.pressed("jump") && input.just_pressed("jump"));
        input.finish_frame();
        input.set_mouse_button(1, true, false);
        input.set_key(KeyCode::Space, false, true);
        assert!(input.pressed("jump") && !input.just_released("jump"));
        input.set_mouse_button(1, false, true);
        assert!(!input.pressed("jump") && input.just_released("jump"));
    }

    #[test]
    fn captured_gui_input_is_not_assigned_to_gameplay() {
        let mut input = InputActions::default();
        input.define("fire", 0.1).unwrap();
        input.bind("fire", InputBinding::MouseButton(1)).unwrap();
        input.set_mouse_button(1, true, true);
        assert!(!input.pressed("fire"));
        input.set_mouse_button(1, false, true);
        assert!(!input.just_pressed("fire"));
    }

    #[test]
    fn gamepad_disconnect_releases_only_disconnected_device() {
        let mut input = InputActions::default();
        input.define("move", 0.2).unwrap();
        input
            .bind(
                "move",
                InputBinding::GamepadAxis {
                    axis: 0,
                    direction: AxisDirection::Positive,
                    device: None,
                },
            )
            .unwrap();
        input.set_gamepad_axis(7, 0, 0.9);
        input.set_gamepad_axis(8, 0, 0.5);
        input.finish_frame();
        input.disconnect_gamepad(7);
        assert!(input.pressed("move"));
        input.disconnect_gamepad(8);
        assert!(input.just_released("move"));
    }

    #[test]
    fn raw_analog_strength_and_deadzone_are_distinct() {
        let mut input = InputActions::default();
        input.define("throttle", 0.2).unwrap();
        input
            .bind(
                "throttle",
                InputBinding::GamepadAxis {
                    axis: 2,
                    direction: AxisDirection::Positive,
                    device: Some(3),
                },
            )
            .unwrap();
        input.set_gamepad_axis(3, 2, 0.1);
        assert_eq!(input.raw_strength("throttle"), 0.1);
        assert_eq!(input.strength("throttle"), 0.0);
        input.set_gamepad_axis(3, 2, 0.6);
        assert!((input.strength("throttle") - 0.5).abs() < 0.001);
        input.set_gamepad_axis(5, 2, 1.0);
        assert!((input.strength("throttle") - 0.5).abs() < 0.001);
    }

    #[test]
    fn focus_loss_clears_all_sources_with_release_events() {
        let mut input = InputActions::default();
        input.define("jump", 0.1).unwrap();
        input
            .bind("jump", InputBinding::Key(KeyCode::Space))
            .unwrap();
        input.set_key(KeyCode::Space, true, false);
        input.finish_frame();
        input.focus_lost();
        assert!(!input.pressed("jump"));
        assert!(input.just_released("jump"));
    }

    #[test]
    fn vectors_have_circular_deadzones_and_cannot_exceed_unit_length() {
        let mut input = InputActions::default();
        for name in ["left", "right", "up", "down"] {
            input.define(name, 0.0).unwrap();
        }
        input
            .bind("right", InputBinding::Key(KeyCode::KeyD))
            .unwrap();
        input
            .bind("down", InputBinding::Key(KeyCode::KeyS))
            .unwrap();
        input.set_key(KeyCode::KeyD, true, false);
        input.set_key(KeyCode::KeyS, true, false);
        let result = input.vector("left", "right", "up", "down", 0.2);
        assert!((result[0].hypot(result[1]) - 1.0).abs() < 0.001);
        assert!((result[0] - result[1]).abs() < 0.001);
    }

    #[test]
    fn invalid_action_definitions_are_rejected_without_partial_mutation() {
        let mut input = InputActions::default();
        assert!(input.define("move left", 0.2).is_err());
        assert!(input.define("move", f32::NAN).is_err());
        assert!(
            input
                .bind("missing", InputBinding::Key(KeyCode::Space))
                .is_err()
        );
        assert!(input.state("move").is_none());
    }
}
