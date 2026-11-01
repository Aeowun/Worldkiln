// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::HashSet;

use glam::Vec3Swizzles;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::editor::EditorTool;
use crate::editor::actor_placement::{
    mesh_bottom_y as actor_brush_mesh_bottom_y, surface_placement_y as actor_brush_placed_y,
};
use crate::editor::editor::transform_gizmo::{
    ActorTransformPreview, TransformAxis, TransformMode, hit_test, hit_test_cells,
};
use crate::editor::grid::picking::{SceneHit, SurfaceHit};
use crate::scripting::value::Value;
use crate::world::{CellType, WorldCoord};

use super::{
    App, EditorMode, RMB_GUARD_EDGE_SCROLL_SPEED, RMB_GUARD_INNER_RADIUS_FACTOR,
    RMB_GUARD_MAX_RADIUS_FACTOR, View,
};

const EMPTY_WORLD_PLACEMENT_DISTANCE: f32 = 12.0;
fn ribbon_digit_for_key(key: KeyCode) -> Option<u8> {
    match key {
        KeyCode::Digit1 | KeyCode::Numpad1 => Some(1),
        KeyCode::Digit2 | KeyCode::Numpad2 => Some(2),
        KeyCode::Digit3 | KeyCode::Numpad3 => Some(3),
        KeyCode::Digit4 | KeyCode::Numpad4 => Some(4),
        KeyCode::Digit5 | KeyCode::Numpad5 => Some(5),
        KeyCode::Digit6 | KeyCode::Numpad6 => Some(6),
        _ => None,
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct ActorBrushCandidate {
    xz: glam::Vec2,
    scale: f32,
    rotation: glam::Vec3,
}

fn actor_brush_candidates(
    center: glam::Vec3,
    radius: f32,
    density: f32,
    scale_min: f32,
    scale_max: f32,
    tilt_min_degrees: f32,
    tilt_max_degrees: f32,
    random_y_rotation: bool,
    seed: u64,
) -> Vec<ActorBrushCandidate> {
    let radius = radius.clamp(0.5, 64.0);
    let density = density.clamp(0.1, 12.0);
    let scale_min = scale_min.clamp(0.05, 20.0);
    let scale_max = scale_max.clamp(scale_min, 20.0);
    let tilt_min_degrees = tilt_min_degrees.clamp(0.0, 90.0);
    let tilt_max_degrees = tilt_max_degrees.clamp(tilt_min_degrees, 90.0);
    let count =
        ((std::f32::consts::PI * radius * radius * density / 100.0).round() as usize).clamp(1, 256);
    let mut rng = StdRng::seed_from_u64(seed);
    (0..count)
        .map(|_| {
            let angle = rng.gen_range(0.0..std::f32::consts::TAU);
            let distance = radius * rng.r#gen::<f32>().sqrt();
            let xz = glam::Vec2::new(
                center.x + angle.cos() * distance,
                center.z + angle.sin() * distance,
            );
            let scale = rng.gen_range(scale_min..=scale_max);
            let yaw = if random_y_rotation {
                rng.gen_range(0.0..std::f32::consts::TAU)
            } else {
                0.0
            };
            let tilt = rng
                .gen_range(tilt_min_degrees..=tilt_max_degrees)
                .to_radians();
            let tilt_direction = rng.gen_range(0.0..std::f32::consts::TAU);
            ActorBrushCandidate {
                xz,
                scale,
                rotation: glam::Vec3::new(
                    tilt * tilt_direction.cos(),
                    yaw,
                    tilt * tilt_direction.sin(),
                ),
            }
        })
        .collect()
}

fn snap_actor_stud(value: f32, step: f32) -> f32 {
    let step = step.max(0.001);
    ((value - 0.5) / step).round() * step + 0.5
}

fn empty_world_placement_target(origin: glam::Vec3, direction: glam::Vec3) -> WorldCoord {
    let point = origin + direction * EMPTY_WORLD_PLACEMENT_DISTANCE;

    WorldCoord::new(
        point.x.floor() as i32,
        point.y.floor() as i32,
        point.z.floor() as i32,
    )
}

fn scene_hit_distance(hit: &SceneHit) -> f32 {
    match hit {
        SceneHit::Cell { distance, .. }
        | SceneHit::Actor { distance, .. }
        | SceneHit::Source { distance, .. } => *distance,
    }
}

fn terrain_is_nearest_build_surface(
    terrain_hit: &SurfaceHit,
    scene_hit: Option<&SceneHit>,
) -> bool {
    scene_hit
        .map(|hit| terrain_hit.distance <= scene_hit_distance(hit))
        .unwrap_or(true)
}

fn scene_hit_visible_above_terrain(
    scene_hit: Option<SceneHit>,
    terrain_hit: Option<&SurfaceHit>,
) -> Option<SceneHit> {
    scene_hit.filter(|hit| {
        terrain_hit.is_none_or(|terrain| scene_hit_distance(hit) < terrain.distance - 0.001)
    })
}

fn nearest_drag_surface(
    terrain: Option<SurfaceHit>,
    cell: Option<SurfaceHit>,
) -> Option<SurfaceHit> {
    match (terrain, cell) {
        (Some(terrain), Some(cell)) => Some(if terrain.distance <= cell.distance {
            terrain
        } else {
            cell
        }),
        (Some(terrain), None) => Some(terrain),
        (None, Some(cell)) => Some(cell),
        (None, None) => None,
    }
}

fn terrain_build_target(hit: &SurfaceHit) -> WorldCoord {
    let rounded_y = hit.point.y.round();
    let surface_y = if (hit.point.y - rounded_y).abs() <= 0.001 {
        rounded_y
    } else {
        hit.point.y
    };

    WorldCoord::new(
        hit.point.x.floor() as i32,
        surface_y.ceil() as i32,
        hit.point.z.floor() as i32,
    )
}

fn cell_build_target(coord: WorldCoord, normal: glam::Vec3, replace: bool) -> WorldCoord {
    if replace {
        coord
    } else {
        WorldCoord::new(
            coord.x + normal.x as i32,
            coord.y + normal.y as i32,
            coord.z + normal.z as i32,
        )
    }
}

fn editor_camera_controls_available(
    view: View,
    mode: EditorMode,
    ui_mode: bool,
    has_active_player: bool,
) -> bool {
    view == View::Editor
        && !ui_mode
        && (mode == EditorMode::Editor || (mode == EditorMode::Play && !has_active_player))
}

fn editor_authoring_shortcuts_available(view: View, mode: EditorMode, ui_mode: bool) -> bool {
    view == View::Editor && mode == EditorMode::Editor && !ui_mode
}

#[derive(Clone, Debug)]
pub struct PendingGrab {
    pub start_mouse: (f64, f64),
    pub anchor_coord: WorldCoord,
    pub source_coords: Vec<WorldCoord>,
}

#[derive(Clone, Debug)]
pub struct PendingActorGrab {
    pub actor_id: u64,
    pub start_mouse: (f64, f64),
    pub start_position: glam::Vec3,
}

#[derive(Clone, Debug)]
pub struct PendingSourceGrab {
    pub source_id: u64,
    pub start_mouse: (f64, f64),
    pub start_position: glam::Vec3,
}

impl App {
    pub(crate) fn update_continuous_sculpt(&mut self, egui_ctx: &egui::Context) {
        if !self.sculpt_pointer_pending {
            return;
        }
        self.sculpt_pointer_pending = false;
        if self.view != View::Editor || self.editor.mode != EditorMode::Editor {
            return;
        }
        let ppp = egui_ctx.pixels_per_point();
        let mouse_logical =
            egui::pos2(self.mouse_pos.0 as f32 / ppp, self.mouse_pos.1 as f32 / ppp);
        let in_viewport = self.editor.viewport_rect.contains(mouse_logical);
        let sculpt_input_allowed = self
            .editor
            .sculpt_viewport_input_allowed(in_viewport, egui_ctx.wants_pointer_input());
        if !sculpt_input_allowed {
            self.editor.hovered_terrain_hit = None;
            return;
        }
        if self.editor.sculpt_stroke_active && self.is_left_mouse_down && sculpt_input_allowed {
            let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();
            if let Some(hit) = crate::editor::grid::picking::raycast_sculpt_terrain_surface(
                mouse_x,
                mouse_y,
                viewport_width,
                viewport_height,
                &self.editor.camera,
                &self.world,
            ) {
                self.editor.hovered_terrain_hit = Some(hit);
                self.editor.capture_brush_columns(
                    &self.world,
                    [hit.point.x, hit.point.y, hit.point.z],
                    self.editor.sculpt_brush_radius,
                );
                let changed = if self.editor.sculpt_tool == crate::world::terrain::SculptTool::Paint
                {
                    let material = format!("layer:{}", self.editor.terrain_paint_layer_id);
                    let radius = self.editor.sculpt_brush_radius;
                    let mut positions = Vec::new();
                    self.editor.paint_stroke_resampler.sample(
                        [hit.point.x, hit.point.y, hit.point.z],
                        radius,
                        |point| positions.push(point),
                    );
                    let mut changed = false;
                    for center in positions {
                        self.editor
                            .capture_brush_columns(&self.world, center, radius);
                        changed |= self.apply_landscaping_paint(
                            &crate::world::terrain::TerrainPaintRequest {
                                strength: self.editor.sculpt_strength.clamp(0.0, 1.0),
                                falloff: Some(self.editor.terrain_paint_falloff),
                                center,
                                radius,
                                material: material.clone(),
                            },
                        );
                    }
                    changed
                } else {
                    let request = crate::world::terrain::TerrainSculptRequest {
                        tool: self.editor.sculpt_tool,
                        center: [hit.point.x, hit.point.y, hit.point.z],
                        radius: self.editor.sculpt_brush_radius,
                        strength: self.editor.sculpt_strength,
                        flatten_target: self.editor.sculpt_flatten_target,
                    };
                    self.apply_landscaping_sculpt(&request)
                };
                if changed {
                    self.editor.sculpt_stroke_mutated = true;
                }
            }
        }
    }

    fn finish_landscaping_stroke(&mut self) {
        self.editor.paint_stroke_resampler.end();
        let regenerate = self.editor.sculpt_stroke_active
            && self.editor.sculpt_stroke_mutated
            && !self.world.terrain.config.generation_layers.is_empty()
            && self.world.terrain.brush_recompose_required();
        self.editor.finish_sculpt_stroke();
        if regenerate {
            match self.world.terrain.rebuild_generation_layers() {
                Ok(()) => self.editor.terrain_generation_error = None,
                Err(error) => self.editor.terrain_generation_error = Some(error),
            }
        }
    }

    fn apply_landscaping_sculpt(
        &mut self,
        request: &crate::world::terrain::TerrainSculptRequest,
    ) -> bool {
        if self.world.terrain.config.generation_layers.is_empty() {
            return self.world.sculpt_authored_terrain(request);
        }
        let Some(id) = self.editor.terrain_selected_generation_layer else {
            return false;
        };
        match self.world.terrain.apply_generation_layer_brush(
            id,
            crate::world::terrain::LayerBrushEdit::Sculpt(*request),
        ) {
            Ok(value) => value,
            Err(error) => {
                self.editor.terrain_generation_error = Some(error);
                false
            }
        }
    }

    fn apply_landscaping_paint(
        &mut self,
        request: &crate::world::terrain::TerrainPaintRequest,
    ) -> bool {
        self.world.terrain.paint_falloff = self.editor.terrain_paint_falloff;
        self.world.terrain.config.paint_falloff = self.editor.terrain_paint_falloff;
        if self.world.terrain.config.generation_layers.is_empty() {
            return self.world.terrain.paint_base_and_record(request);
        }
        let Some(id) = self.editor.terrain_selected_generation_layer else {
            return false;
        };
        match self.world.terrain.apply_generation_layer_brush(
            id,
            crate::world::terrain::LayerBrushEdit::Paint {
                center: request.center,
                radius: request.radius,
                material: request.material.clone(),
                strength: request.strength,
                falloff: request.falloff,
            },
        ) {
            Ok(value) => value,
            Err(error) => {
                self.editor.terrain_generation_error = Some(error);
                false
            }
        }
    }

    pub(crate) fn editor_camera_controls_active(&self) -> bool {
        editor_camera_controls_available(
            self.view,
            self.editor.mode,
            self.editor.ui_mode,
            self.character_system.get_active_player().is_some(),
        )
    }

    pub(crate) fn handle_play_shortcut(&mut self, event: &WindowEvent) -> bool {
        if self.view != View::Editor {
            return false;
        }
        let WindowEvent::KeyboardInput { event, .. } = event else {
            return false;
        };
        let PhysicalKey::Code(key) = event.physical_key else {
            return false;
        };
        self.route_play_shortcut(key, event.state == ElementState::Pressed, event.repeat)
    }

    fn route_play_shortcut(&mut self, key: KeyCode, pressed: bool, repeat: bool) -> bool {
        if self.view != View::Editor {
            return false;
        }
        let (play, tray) = self.editor.theme_settings.play_bindings();
        let playing = self.editor.mode == EditorMode::Play;
        if key != play.key()
            && !(playing && key == tray.key())
            && !self.editor_shortcut_keys.contains(&key)
        {
            return false;
        }
        let fresh = if pressed {
            self.editor_shortcut_keys.insert(key) && !repeat
        } else {
            self.editor_shortcut_keys.remove(&key);
            false
        };
        if fresh {
            if key == play.key() {
                self.editor.commit_pending_inspector_history();
                self.editor.mode = if playing {
                    EditorMode::Editor
                } else {
                    EditorMode::Play
                };
            } else {
                self.editor.shell.play_tray_hidden = !self.editor.shell.play_tray_hidden;
            }
        }
        true
    }

    pub fn on_window_event(
        &mut self,
        event: &WindowEvent,
        egui_ctx: &egui::Context,
        window: &winit::window::Window,
    ) {
        match event {
            WindowEvent::Resized(size) => {
                self.renderer.resize(size.width as f32, size.height as f32);
            }

            WindowEvent::Focused(false) => {
                let gameplay_active = self.view == View::Editor
                    && self.editor.mode == EditorMode::Play
                    && self.script_scene.is_some();
                for key in self.keys_down.drain() {
                    if gameplay_active {
                        self.script_pending_events.push((
                            "keypress".to_string(),
                            vec![
                                Value::String(crate::scripting::host::key_code_name(key)),
                                Value::Bool(false),
                            ],
                        ));
                    }
                }
                if self.gameplay_left_mouse_captured && gameplay_active {
                    self.script_pending_events.push((
                        "mousepress".to_string(),
                        vec![Value::String("Left".into()), Value::Bool(false)],
                    ));
                }
                self.gameplay_left_mouse_captured = false;
                self.editor_shortcut_keys.clear();
                self.is_right_mouse_down = false;
                self.rmb_edge_scroll_velocity = glam::Vec2::ZERO;
                self.last_cursor_pos = None;
            }

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if let PhysicalKey::Code(key) = key_event.physical_key {
                    let was_pressed = key_event.state == ElementState::Pressed;

                    let wants_keyboard = egui_ctx.wants_keyboard_input();
                    let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                        || self.keys_down.contains(&KeyCode::ControlRight)
                        || egui_ctx.input(|input| input.modifiers.ctrl);
                    if was_pressed {
                        if !wants_keyboard {
                            let was_already_down = !self.keys_down.insert(key);
                            if !key_event.repeat
                                && !was_already_down
                                && self.view == View::Editor
                                && self.editor.mode == EditorMode::Play
                                && self.script_scene.is_some()
                            {
                                self.script_pending_events.push((
                                    "keypress".to_string(),
                                    vec![
                                        Value::String(crate::scripting::host::key_code_name(key)),
                                        Value::Bool(true),
                                    ],
                                ));
                            }
                        }
                    } else {
                        let was_down = self.keys_down.remove(&key);
                        if was_down
                            && self.view == View::Editor
                            && self.editor.mode == EditorMode::Play
                            && self.script_scene.is_some()
                        {
                            self.script_pending_events.push((
                                "keypress".to_string(),
                                vec![
                                    Value::String(crate::scripting::host::key_code_name(key)),
                                    Value::Bool(false),
                                ],
                            ));
                        }
                    }
                    if egui_capture_blocks_engine_key(key, ctrl, wants_keyboard) {
                        return;
                    }

                    if (key == KeyCode::ShiftLeft || key == KeyCode::ShiftRight)
                        && self.view == View::Editor
                        && self.editor.mode == EditorMode::Editor
                        && !self.editor.ui_mode
                    {
                        let pixels_per_point = egui_ctx.pixels_per_point();

                        let mouse_logical = egui::pos2(
                            self.mouse_pos.0 as f32 / pixels_per_point,
                            self.mouse_pos.1 as f32 / pixels_per_point,
                        );

                        let in_viewport = self.editor.viewport_rect.contains(mouse_logical);

                        if in_viewport && !egui_ctx.wants_pointer_input() {
                            self.update_hover();
                        }
                    }

                    if was_pressed {
                        let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                            || self.keys_down.contains(&KeyCode::ControlRight)
                            || egui_ctx.input(|input| input.modifiers.ctrl);

                        match key {
                            KeyCode::KeyS => {
                                if ctrl && self.view == View::Editor {
                                    self.save_all_authored();
                                }
                            }

                            KeyCode::KeyN => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.show_script_workspace
                                {
                                    self.editor.script_editor.show_new_script_dialog = true;
                                }
                            }

                            KeyCode::KeyC => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                    && !self.editor.ui_mode
                                {
                                    self.copy_selection();
                                }
                            }

                            KeyCode::KeyV => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                    && !self.editor.ui_mode
                                {
                                    self.paste_clipboard();
                                }
                            }

                            KeyCode::KeyW | KeyCode::KeyE | KeyCode::KeyR
                                if self.editor.water_selected_body.is_some()
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                    && !ctrl
                                    && !self.editor.ui_mode
                                    && self.editor.current_tool == EditorTool::Select
                                    && !self.is_right_mouse_down =>
                            {
                                self.editor.transform_mode = match key {
                                    KeyCode::KeyW => TransformMode::Move,
                                    KeyCode::KeyE => TransformMode::Rotate,
                                    _ => TransformMode::Scale,
                                };
                            }
                            KeyCode::KeyZ => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                {
                                    let shift = self.keys_down.contains(&KeyCode::ShiftLeft)
                                        || self.keys_down.contains(&KeyCode::ShiftRight);
                                    if !self.editor.water_undo_redo(&mut self.world, shift) {
                                        self.dispatch_undo_redo(shift);
                                    }
                                }
                            }

                            KeyCode::KeyY => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                {
                                    if !self.editor.water_undo_redo(&mut self.world, true) {
                                        self.dispatch_undo_redo(true);
                                    }
                                }
                            }

                            key if self.editor.theme_settings.bind_delete.matches_key(key) => {
                                if self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                    && !self.editor.ui_mode
                                    && !self.editor.project_explorer_focused
                                {
                                    if self.editor.water_delete_selected(&mut self.world) {
                                        return;
                                    }
                                    if self.delete_selected_scene_source() {
                                        return;
                                    }
                                    if let Some(actor_id) = self.editor.selected_actor_id {
                                        if self.world.actors.contains_key(&actor_id) {
                                            self.push_undo_snapshot();

                                            self.world.remove_actor(actor_id);

                                            self.editor.clear_selection();
                                            self.editor.clear_hover();

                                            self.editor.needs_save = true;
                                        } else {
                                            self.editor.selected_actor_id = None;
                                        }
                                    }

                                    if self.editor.selected_actor_id.is_none()
                                        && !self.editor.selected_coords.is_empty()
                                    {
                                        self.push_undo_snapshot();

                                        for coord in &self.editor.selected_coords {
                                            self.world.set_cell(*coord, CellType::Empty);
                                        }

                                        self.editor.clear_selection();

                                        self.editor.needs_save = true;
                                    }
                                }
                            }

                            KeyCode::KeyF => {
                                if self.view == View::Editor && !self.editor.ui_mode {
                                    let bounds =
                                        if let Some(actor_id) = self.editor.selected_actor_id {
                                            self.world
                                                .actors
                                                .get(&actor_id)
                                                .map(crate::editor::grid::picking::actor_bounds)
                                        } else if !self.editor.selected_coords.is_empty() {
                                            let mut min = glam::Vec3::splat(f32::MAX);
                                            let mut max = glam::Vec3::splat(f32::MIN);

                                            for coord in &self.editor.selected_coords {
                                                let cell_min = glam::Vec3::new(
                                                    coord.x as f32,
                                                    coord.y as f32,
                                                    coord.z as f32,
                                                );
                                                let cell_max = cell_min + glam::Vec3::ONE;

                                                min = min.min(cell_min);
                                                max = max.max(cell_max);
                                            }

                                            Some((min, max))
                                        } else {
                                            None
                                        };

                                    if let Some((min, max)) = bounds {
                                        self.editor.camera.frame_bounds(min, max);

                                        let center = self.editor.camera.target;

                                        self.editor.anchor = WorldCoord::new(
                                            center.x.round() as i32,
                                            center.y.round() as i32,
                                            center.z.round() as i32,
                                        );
                                    }
                                }
                            }

                            KeyCode::Digit1
                            | KeyCode::Numpad1
                            | KeyCode::Digit2
                            | KeyCode::Numpad2
                            | KeyCode::Digit3
                            | KeyCode::Numpad3
                            | KeyCode::Digit4
                            | KeyCode::Numpad4
                            | KeyCode::Digit5
                            | KeyCode::Numpad5
                            | KeyCode::Digit6
                            | KeyCode::Numpad6 => {
                                if editor_authoring_shortcuts_available(
                                    self.view,
                                    self.editor.mode,
                                    self.editor.ui_mode,
                                ) && !self.editor.show_script_workspace
                                    && !self.editor.navigation.surface.is_settings()
                                {
                                    if let Some(digit) = ribbon_digit_for_key(key) {
                                        self.editor.activate_ribbon_shortcut(digit);
                                    }
                                }
                            }

                            KeyCode::Escape => {
                                if self.view == View::Editor && self.editor.mode == EditorMode::Play
                                {
                                    return;
                                }
                                if self.editor.navigation.surface.is_settings() {
                                    self.editor.close_settings();
                                    return;
                                }
                                if self.editor.water_spline_drag.is_some() {
                                    self.editor.water_cancel_drag(&mut self.world);
                                    self.is_left_mouse_down = false;
                                    return;
                                }
                                if self.editor.cell_move_preview.is_some() {
                                    self.editor.cancel_cell_move_preview();
                                    self.is_left_mouse_down = false;
                                    self.pending_grab = None;
                                    self.drag_start_coord = None;
                                    self.editor.clear_hover();
                                    return;
                                }
                                if self.editor.actor_transform_preview.take().is_some() {
                                    self.is_left_mouse_down = false;
                                    self.pending_actor_grab = None;
                                    self.drag_start_coord = None;
                                    self.editor.clear_hover();
                                    return;
                                }
                                if self.editor.ui_mode {
                                    self.editor.exit_ui_mode();
                                    return;
                                }

                                if self.editor.grab_state.is_some()
                                    || self.pending_grab.is_some()
                                    || self.editor.actor_grab_state.is_some()
                                    || self.pending_actor_grab.is_some()
                                    || self.editor.source_grab_state.is_some()
                                    || self.pending_source_grab.is_some()
                                {
                                    self.editor.grab_state = None;
                                    self.pending_grab = None;
                                    self.editor.actor_grab_state = None;
                                    self.pending_actor_grab = None;
                                    self.editor.source_grab_state = None;
                                    self.pending_source_grab = None;
                                    self.editor.clear_hover();
                                } else if self.view == View::Splash {
                                    self.view = View::Home;
                                } else if self.view == View::Home {
                                    self.show_exit_confirmation_dialog = true;
                                } else if self.view == View::Editor {
                                    if self.is_left_mouse_down {
                                        self.is_left_mouse_down = false;
                                        self.drag_start_coord = None;
                                    } else if self.editor.selected_actor_id.is_some()
                                        || self.editor.selected_source_id.is_some()
                                        || !self.editor.selected_coords.is_empty()
                                        || self.editor.selected_coord.is_some()
                                    {
                                        self.editor.clear_selection();
                                    } else {
                                        self.exit_to_home();
                                    }
                                }
                            }

                            KeyCode::Enter => {
                                if self.view == View::Splash {
                                    self.view = View::Home;
                                }
                            }

                            KeyCode::Space => {
                                if self.view == View::Editor && self.editor.mode == EditorMode::Play
                                {
                                    self.jump_requested = true;
                                }
                            }

                            _ => {}
                        }
                    }
                }
            }

            WindowEvent::MouseInput { state, button, .. } => {
                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && (self.editor.ui_mode || self.editor.effect_visible)
                {
                    return;
                }

                let pixels_per_point = egui_ctx.pixels_per_point();

                let mouse_logical = egui::pos2(
                    self.mouse_pos.0 as f32 / pixels_per_point,
                    self.mouse_pos.1 as f32 / pixels_per_point,
                );

                let in_viewport = self.editor.viewport_rect.contains(mouse_logical);

                let wants_pointer = egui_ctx.wants_pointer_input();
                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Play
                    && self.script_scene.is_some()
                {
                    let down = *state == ElementState::Pressed;
                    let acquired = in_viewport && !wants_pointer;
                    let is_left = *button == MouseButton::Left;
                    let captured_release = is_left && !down && self.gameplay_left_mouse_captured;
                    if (down && acquired) || (captured_release) || (!down && acquired && !is_left) {
                        let name = match button {
                            MouseButton::Left => Some("Left"),
                            MouseButton::Right => Some("Right"),
                            MouseButton::Middle => Some("Middle"),
                            _ => None,
                        };
                        if let Some(name) = name {
                            if is_left {
                                self.gameplay_left_mouse_captured = down;
                            }
                            self.script_pending_events.push((
                                "mousepress".to_string(),
                                vec![Value::String(name.into()), Value::Bool(down)],
                            ));
                        }
                    }
                }
                if *button == MouseButton::Right
                    && *state == ElementState::Pressed
                    && in_viewport
                    && !wants_pointer
                    && self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && self.editor.water_context_down(&self.world, mouse_logical)
                {
                    return;
                }

                if self.editor_camera_controls_active()
                    && self
                        .editor
                        .theme_settings
                        .bind_free_look
                        .matches_button(*button)
                {
                    if *state == ElementState::Pressed {
                        if !in_viewport || wants_pointer {
                            return;
                        }

                        self.is_right_mouse_down = true;
                    } else {
                        self.is_right_mouse_down = false;
                        self.rmb_edge_scroll_velocity = glam::Vec2::ZERO;
                        self.last_cursor_pos = None;
                    }

                    return;
                }

                if self.editor_camera_controls_active()
                    && self
                        .editor
                        .theme_settings
                        .bind_orbit_camera
                        .matches_button(*button)
                {
                    if *state == ElementState::Pressed {
                        if !in_viewport || wants_pointer {
                            return;
                        }

                        self.is_middle_mouse_down = true;
                    } else {
                        self.is_middle_mouse_down = false;
                        self.last_cursor_pos = None;
                    }

                    return;
                }

                if self
                    .editor
                    .theme_settings
                    .bind_select
                    .matches_button(*button)
                {
                    if *state == ElementState::Released && self.editor.box_selection.is_some() {
                        let was_drag = self.editor.box_selection.as_ref().is_some_and(|gesture| {
                            gesture.is_drag() || gesture.start.distance_sq(mouse_logical) >= 25.0
                        });
                        self.editor.finish_box_selection(&self.world, mouse_logical);
                        if !was_drag && in_viewport {
                            self.update_hover();
                            self.on_click();
                        }
                        self.is_left_mouse_down = false;
                        self.drag_start_coord = None;
                        self.editor.clear_hover();
                        return;
                    }
                    if *state == ElementState::Released {
                        if let Some(drag) = self.editor.plane_drag.take() {
                            if self.editor.anchor != drag.start_anchor {
                                self.editor.needs_save = true;
                            }
                            self.editor.plane_grab_hover = false;
                            self.is_left_mouse_down = false;
                            self.drag_start_coord = None;
                            self.editor.clear_hover();
                            return;
                        }
                    }
                    if *state == ElementState::Pressed
                        && in_viewport
                        && !wants_pointer
                        && self.view == View::Editor
                        && self.editor.mode == EditorMode::Editor
                        && !self.editor.ui_mode
                        && !self.editor.show_script_workspace
                        && self.editor.current_tool == EditorTool::Build
                        && self.editor.plane_picking
                    {
                        let (x, y, width, height) = self.viewport_mouse();
                        if let Some(grab) = crate::editor::grid::picking::begin_plane_drag(
                            glam::Vec2::new(x, y),
                            glam::Vec2::new(width, height),
                            self.editor.viewport_ppp,
                            &self.editor.camera,
                            self.editor.anchor,
                        ) {
                            self.editor.plane_drag = Some(grab);
                            self.is_left_mouse_down = true;
                            self.drag_start_coord = None;
                            self.editor.clear_hover();
                            return;
                        }
                    }
                    if *state == ElementState::Released && self.editor.water_spline_drag.is_some() {
                        self.editor.water_finish_drag(&mut self.world);
                        self.is_left_mouse_down = false;
                        self.editor.clear_hover();
                        return;
                    }
                    if *state == ElementState::Released && self.editor.sculpt_stroke_active {
                        self.update_continuous_sculpt(egui_ctx);
                        self.is_left_mouse_down = false;
                        self.sculpt_pointer_pending = false;
                        self.finish_landscaping_stroke();
                        return;
                    }
                    if *state == ElementState::Released && self.editor.actor_brush_stroke_active {
                        self.is_left_mouse_down = false;
                        self.editor.finish_actor_brush_stroke();
                        return;
                    }

                    if self.editor.actor_brush_enabled
                        && self.view == View::Editor
                        && self.editor.mode == EditorMode::Editor
                        && !self.editor.ui_mode
                        && !self.editor.show_script_workspace
                    {
                        if *state == ElementState::Pressed {
                            if !self
                                .editor
                                .actor_brush_viewport_input_allowed(in_viewport, wants_pointer)
                            {
                                return;
                            }
                            self.is_left_mouse_down = true;
                            let (mouse_x, mouse_y, viewport_width, viewport_height) =
                                self.viewport_mouse();
                            if let Some(hit) =
                                crate::editor::grid::picking::raycast_sculpt_terrain_surface(
                                    mouse_x,
                                    mouse_y,
                                    viewport_width,
                                    viewport_height,
                                    &self.editor.camera,
                                    &self.world,
                                )
                            {
                                self.editor.actor_brush_stroke_active = true;
                                self.editor.actor_brush_stroke_mutated = false;
                                self.editor.actor_brush_stroke_snapshot =
                                    Some(self.editor.create_snapshot(&self.world));
                                self.editor.actor_brush_last_center = None;
                                self.editor.actor_brush_stroke_positions.clear();
                                self.apply_actor_brush_dab(hit.point);
                            }
                        } else {
                            self.is_left_mouse_down = false;
                        }
                        return;
                    }

                    if self.editor.sculpt_tool != crate::world::terrain::SculptTool::None
                        && self.view == View::Editor
                        && self.editor.mode == EditorMode::Editor
                        && !self.editor.ui_mode
                        && !self.editor.show_script_workspace
                    {
                        if *state == ElementState::Pressed {
                            if !self
                                .editor
                                .sculpt_viewport_input_allowed(in_viewport, wants_pointer)
                            {
                                return;
                            }

                            self.is_left_mouse_down = true;
                            let (mouse_x, mouse_y, viewport_width, viewport_height) =
                                self.viewport_mouse();
                            if let Some(hit) =
                                crate::editor::grid::picking::raycast_sculpt_terrain_surface(
                                    mouse_x,
                                    mouse_y,
                                    viewport_width,
                                    viewport_height,
                                    &self.editor.camera,
                                    &self.world,
                                )
                            {
                                let snapshot =
                                    if !self.world.terrain.config.generation_layers.is_empty() {
                                        if let Some(id) =
                                            self.editor.terrain_selected_generation_layer
                                        {
                                            self.editor.create_layer_brush_snapshot(&self.world, id)
                                        } else {
                                            self.editor.create_brush_snapshot(&self.world)
                                        }
                                    } else {
                                        self.editor.create_brush_snapshot(&self.world)
                                    };
                                self.editor.sculpt_stroke_active = true;
                                self.sculpt_pointer_pending = false;
                                if self.editor.sculpt_tool
                                    == crate::world::terrain::SculptTool::Paint
                                {
                                    self.editor.paint_stroke_resampler.begin([
                                        hit.point.x,
                                        hit.point.y,
                                        hit.point.z,
                                    ]);
                                }

                                self.editor.sculpt_stroke_mutated = false;
                                self.editor.sculpt_stroke_snapshot = Some(snapshot);
                                if self.editor.sculpt_tool.is_flatten() {
                                    self.editor.sculpt_flatten_target = Some(hit.point.y);
                                } else {
                                    self.editor.sculpt_flatten_target = None;
                                }

                                self.editor.capture_brush_columns(
                                    &self.world,
                                    [hit.point.x, hit.point.y, hit.point.z],
                                    self.editor.sculpt_brush_radius,
                                );
                                let changed = if self.editor.sculpt_tool
                                    == crate::world::terrain::SculptTool::Paint
                                {
                                    let material =
                                        format!("layer:{}", self.editor.terrain_paint_layer_id);
                                    self.apply_landscaping_paint(
                                        &crate::world::terrain::TerrainPaintRequest {
                                            strength: self.editor.sculpt_strength.clamp(0.0, 1.0),
                                            falloff: Some(self.editor.terrain_paint_falloff),
                                            center: [hit.point.x, hit.point.y, hit.point.z],
                                            radius: self.editor.sculpt_brush_radius,
                                            material,
                                        },
                                    )
                                } else {
                                    let request = crate::world::terrain::TerrainSculptRequest {
                                        tool: self.editor.sculpt_tool,
                                        center: [hit.point.x, hit.point.y, hit.point.z],
                                        radius: self.editor.sculpt_brush_radius,
                                        strength: self.editor.sculpt_strength,
                                        flatten_target: self.editor.sculpt_flatten_target,
                                    };
                                    self.apply_landscaping_sculpt(&request)
                                };
                                if changed {
                                    self.editor.sculpt_stroke_mutated = true;
                                }
                            }
                        } else {
                            self.is_left_mouse_down = false;
                        }
                        return;
                    }

                    if *state == ElementState::Pressed {
                        if !in_viewport || wants_pointer {
                            return;
                        }

                        if self.editor.current_tool == EditorTool::Select
                            && self.editor.sculpt_tool == crate::world::terrain::SculptTool::None
                        {
                            let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                                || self.keys_down.contains(&KeyCode::ControlRight);
                            let alt = self.keys_down.contains(&KeyCode::AltLeft)
                                || self.keys_down.contains(&KeyCode::AltRight);
                            if self.editor.water_pointer_down(
                                &mut self.world,
                                mouse_logical,
                                ctrl,
                                alt,
                            ) {
                                self.is_left_mouse_down = true;
                                self.editor.clear_hover();
                                return;
                            }
                        }
                        self.is_left_mouse_down = true;

                        let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                            || self.keys_down.contains(&KeyCode::ControlRight);
                        if self.editor.current_tool == EditorTool::Select && !ctrl {
                            let pointer = mouse_logical;
                            if let Some(axis) = hit_test_cells(&self.editor, &self.world, pointer) {
                                if self.begin_cell_move_preview(axis, pointer) {
                                    return;
                                }
                            }
                            let selected = self.editor.selected_actor_id;
                            if let Some(actor) = selected.and_then(|id| self.world.actors.get(&id))
                            {
                                if let Some(axis) = hit_test(&self.editor, actor, pointer) {
                                    let preview = ActorTransformPreview::new(
                                        actor,
                                        self.editor.transform_mode,
                                        axis,
                                        pointer,
                                    );
                                    self.editor.actor_transform_preview = Some(preview);
                                    self.pending_actor_grab = None;
                                    self.drag_start_coord = None;
                                    return;
                                }
                            }
                        }

                        self.update_hover();

                        if crate::editor::editor::box_selection::permitted(&self.editor)
                            && !ctrl
                            && !self.keys_down.contains(&KeyCode::AltLeft)
                            && !self.keys_down.contains(&KeyCode::AltRight)
                            && self.editor.hovered_actor_id.is_none()
                            && self.editor.hovered_source_id.is_none()
                            && self
                                .editor
                                .hovered_cell
                                .is_none_or(|coord| self.world.get(coord).is_none())
                        {
                            self.editor.box_selection =
                                Some(crate::editor::editor::BoxSelection::new(mouse_logical));
                            self.drag_start_coord = None;
                            return;
                        }

                        if self.editor.current_tool == EditorTool::Erase {
                            if let Some(source_id) = self.editor.hovered_source_id {
                                self.erase_source(source_id);
                                self.is_left_mouse_down = false;
                                self.drag_start_coord = None;
                                return;
                            }
                            if let Some(actor_id) = self.editor.hovered_actor_id {
                                self.erase_actor(actor_id);

                                self.is_left_mouse_down = false;
                                self.drag_start_coord = None;

                                return;
                            }
                        }

                        if self.editor.current_tool == EditorTool::Build
                            && (self.editor.hovered_actor_id.is_some()
                                || self.editor.hovered_source_id.is_some())
                        {
                            self.is_left_mouse_down = false;
                            self.drag_start_coord = None;

                            return;
                        }

                        let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                            || self.keys_down.contains(&KeyCode::ControlRight);

                        if self.editor.current_tool == EditorTool::Select && !ctrl {
                            if let Some(source_id) = self.editor.hovered_source_id {
                                if !matches!(
                                    self.editor.transform_mode,
                                    TransformMode::Select | TransformMode::Move
                                ) {
                                    self.editor.select_source(source_id);
                                    self.drag_start_coord = None;
                                    return;
                                }
                                if let Some(source) = self.world.source(source_id) {
                                    let start_position = source.position;
                                    self.editor.select_source(source_id);
                                    self.pending_source_grab = Some(PendingSourceGrab {
                                        source_id,
                                        start_mouse: self.mouse_pos,
                                        start_position,
                                    });
                                    self.drag_start_coord = None;
                                    return;
                                }
                            }

                            if let Some(actor_id) = self.editor.hovered_actor_id {
                                if let Some(actor) = self.world.actors.get(&actor_id) {
                                    let position = actor.position;
                                    let preview = match self.editor.transform_mode {
                                        TransformMode::Rotate => Some(ActorTransformPreview::new(
                                            actor,
                                            TransformMode::Rotate,
                                            TransformAxis::Y,
                                            mouse_logical,
                                        )),
                                        TransformMode::Scale => Some(ActorTransformPreview::new(
                                            actor,
                                            TransformMode::Scale,
                                            TransformAxis::Center,
                                            mouse_logical,
                                        )),
                                        _ => None,
                                    };
                                    if ctrl {
                                        self.editor.toggle_actor_selection(actor_id);
                                        self.pending_actor_grab = None;
                                        self.drag_start_coord = None;
                                        return;
                                    }
                                    if !self.editor.selected_actor_ids().contains(&actor_id) {
                                        self.editor.select_actor(actor_id);
                                    }
                                    if let Some(preview) = preview {
                                        self.editor.actor_transform_preview = Some(preview);
                                        self.pending_actor_grab = None;
                                    } else {
                                        self.pending_actor_grab = Some(PendingActorGrab {
                                            actor_id,
                                            start_mouse: self.mouse_pos,
                                            start_position: position,
                                        });
                                    }
                                    self.drag_start_coord = None;
                                    return;
                                }
                            }
                        }

                        if self.editor.current_tool == EditorTool::Select
                            && self.editor.hovered_actor_id.is_none()
                            && !ctrl
                        {
                            if let Some(hovered) = self.editor.hovered_cell {
                                let move_tool = self.editor.transform_mode == TransformMode::Move;
                                if self.editor.selected_coords.contains(&hovered)
                                    || (move_tool && self.world.get(hovered).is_some())
                                {
                                    if !move_tool || !self.editor.selected_coords.contains(&hovered)
                                    {
                                        self.editor.select_cell(hovered);
                                    }
                                    let source_coords = if move_tool {
                                        self.editor.selected_cell_move_coords(&self.world)
                                    } else {
                                        vec![hovered]
                                    };
                                    self.pending_grab = Some(PendingGrab {
                                        start_mouse: self.mouse_pos,
                                        anchor_coord: hovered,
                                        source_coords,
                                    });

                                    self.drag_start_coord = None;

                                    return;
                                }
                            }
                        }

                        self.begin_tool_pointer_action(ctrl);
                    } else {
                        if self.editor.cell_move_preview.is_some() {
                            self.commit_cell_move_preview();
                            self.pending_grab = None;
                            self.is_left_mouse_down = false;
                            self.drag_start_coord = None;
                            self.editor.clear_hover();
                            return;
                        }
                        if self.editor.actor_transform_preview.is_some() {
                            self.commit_actor_transform_preview();
                            self.pending_actor_grab = None;
                            self.is_left_mouse_down = false;
                            self.drag_start_coord = None;
                            self.editor.clear_hover();
                            return;
                        }
                        if let Some(grab) = self.editor.source_grab_state.take() {
                            if grab.current_position != grab.start_position {
                                self.push_undo_snapshot();
                                if self
                                    .world
                                    .set_source_position(grab.source_id, grab.current_position)
                                    .is_ok()
                                {
                                    self.editor.needs_save = true;
                                }
                            }
                            self.pending_source_grab = None;
                            self.editor.clear_hover();
                        } else if let Some(_pending_source) = self.pending_source_grab.take() {
                            self.editor.clear_hover();
                        } else if let Some(grab) = self.editor.actor_grab_state.take() {
                            self.commit_actor_surface_drag(grab);

                            self.pending_actor_grab = None;
                            self.editor.clear_hover();
                        } else if let Some(_pending_actor) = self.pending_actor_grab.take() {
                            self.editor.clear_hover();
                        } else if let Some(grab) = self.editor.grab_state.take() {
                            if grab.valid && grab.delta != WorldCoord::new(0, 0, 0) {
                                self.push_undo_snapshot();

                                if let Ok(()) =
                                    self.world.move_cells(&grab.source_coords, grab.delta)
                                {
                                    self.editor.selected_coords = grab
                                        .source_coords
                                        .iter()
                                        .map(|c| {
                                            WorldCoord::new(
                                                c.x + grab.delta.x,
                                                c.y + grab.delta.y,
                                                c.z + grab.delta.z,
                                            )
                                        })
                                        .collect();

                                    if let Some(sel) = self.editor.selected_coord {
                                        self.editor.selected_coord = Some(WorldCoord::new(
                                            sel.x + grab.delta.x,
                                            sel.y + grab.delta.y,
                                            sel.z + grab.delta.z,
                                        ));
                                    }

                                    self.editor.needs_save = true;
                                }
                            }

                            self.pending_grab = None;
                        } else if let Some(_pending) = self.pending_grab.take() {
                        } else if self.is_left_mouse_down {
                            self.finish_tool_pointer_action();
                        }

                        self.is_left_mouse_down = false;
                        self.drag_start_coord = None;
                    }

                    return;
                }
            }

            WindowEvent::CursorMoved { position, .. } => {
                let dx = position.x - self.mouse_pos.0;
                let dy = position.y - self.mouse_pos.1;

                self.mouse_pos = (position.x, position.y);

                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && (self.editor.ui_mode || self.editor.effect_visible)
                {
                    self.editor.clear_hover();

                    self.rmb_edge_scroll_velocity = glam::Vec2::ZERO;

                    return;
                }

                let pixels_per_point = egui_ctx.pixels_per_point();

                let mouse_logical = egui::pos2(
                    position.x as f32 / pixels_per_point,
                    position.y as f32 / pixels_per_point,
                );

                let in_viewport = self.editor.viewport_rect.contains(mouse_logical);

                if let Some(selection) = self.editor.box_selection.as_mut() {
                    selection.end = mouse_logical;
                    self.editor.clear_hover();
                    return;
                }

                self.editor.plane_grab_hover = false;
                if in_viewport
                    && self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && self.editor.current_tool == EditorTool::Build
                    && self.editor.plane_picking
                    && !self.editor.ui_mode
                    && !self.editor.show_script_workspace
                {
                    let (mx, my, vw, vh) = self.viewport_mouse();
                    let camera = &self.editor.camera;
                    let plane = self.editor.plane_drag.as_ref().map_or_else(
                        || {
                            crate::editor::grid::select_grid_plane(
                                camera.get_position(),
                                camera.target,
                            )
                        },
                        |drag| drag.plane,
                    );
                    if let Some((globe, tip, _, _)) =
                        crate::editor::grid::picking::plane_handle_positions(
                            camera,
                            self.editor.anchor,
                            plane,
                            glam::Vec2::new(vw, vh),
                            self.editor.viewport_ppp,
                        )
                    {
                        self.editor.plane_grab_hover =
                            crate::editor::grid::picking::plane_handle_hit(
                                glam::Vec2::new(mx, my),
                                globe,
                                tip,
                                self.editor.viewport_ppp,
                            )
                            .is_some();
                    }
                }

                if let Some(grab) = self.editor.plane_drag {
                    if self.view == View::Editor
                        && self.editor.mode == EditorMode::Editor
                        && self.editor.current_tool == EditorTool::Build
                        && self.editor.plane_picking
                        && !self.editor.ui_mode
                        && !self.editor.show_script_workspace
                    {
                        let (x, y, w, h) = self.viewport_mouse();
                        if let Some(new_anchor) = crate::editor::grid::picking::update_plane_drag(
                            &grab,
                            glam::Vec2::new(x, y),
                            glam::Vec2::new(w, h),
                            &self.editor.camera,
                        ) {
                            self.editor.anchor = new_anchor;
                        }
                    }
                    self.editor.clear_hover();
                    return;
                }
                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && !self.editor.ui_mode
                    && !self.editor.show_script_workspace
                    && self.editor.cell_move_preview.is_some()
                {
                    self.update_cell_move_preview(mouse_logical);
                    self.editor.clear_hover();
                    return;
                }
                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && !self.editor.ui_mode
                    && !self.editor.show_script_workspace
                    && self.editor.actor_transform_preview.is_some()
                {
                    let camera = self.editor.camera.clone();
                    let viewport = self.editor.viewport_rect;
                    if let Some(drag) = self.editor.actor_transform_preview.as_mut() {
                        drag.update(mouse_logical, &camera, viewport);
                    }
                    self.editor.clear_hover();
                    return;
                }

                if self.view == View::Editor && self.editor.water_spline_drag.is_some() {
                    self.editor
                        .water_pointer_move(&mut self.world, mouse_logical);
                    self.editor.clear_hover();
                    return;
                }
                if in_viewport
                    && self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && !self.editor.ui_mode
                {
                    if self.editor.sculpt_stroke_active && self.is_left_mouse_down {
                        self.sculpt_pointer_pending = true;
                        return;
                    }
                    self.update_hover();

                    let sculpt_input_allowed = self
                        .editor
                        .sculpt_viewport_input_allowed(in_viewport, egui_ctx.wants_pointer_input());
                    if self.editor.sculpt_tool != crate::world::terrain::SculptTool::None
                        && !sculpt_input_allowed
                    {
                        self.editor.hovered_terrain_hit = None;
                    }

                    if self.editor.actor_brush_stroke_active
                        && self.is_left_mouse_down
                        && self.editor.actor_brush_viewport_input_allowed(
                            in_viewport,
                            egui_ctx.wants_pointer_input(),
                        )
                    {
                        let (mouse_x, mouse_y, viewport_width, viewport_height) =
                            self.viewport_mouse();
                        if let Some(hit) =
                            crate::editor::grid::picking::raycast_sculpt_terrain_surface(
                                mouse_x,
                                mouse_y,
                                viewport_width,
                                viewport_height,
                                &self.editor.camera,
                                &self.world,
                            )
                        {
                            self.apply_actor_brush_dab(hit.point);
                        }
                    }

                    if let Some(pending) = self.pending_source_grab.clone() {
                        let pdx = position.x - pending.start_mouse.0;
                        let pdy = position.y - pending.start_mouse.1;
                        if pdx * pdx + pdy * pdy >= 25.0 {
                            if let Some(target) = self.actor_drag_target() {
                                self.editor.source_grab_state =
                                    Some(crate::editor::editor::SourceGrabState {
                                        source_id: pending.source_id,
                                        start_position: pending.start_position,
                                        current_position: target,
                                    });
                                self.pending_source_grab = None;
                                self.editor.clear_hover();
                            }
                        }
                    } else if self.editor.source_grab_state.is_some() {
                        if let Some(target) = self.actor_drag_target() {
                            if let Some(grab) = self.editor.source_grab_state.as_mut() {
                                grab.current_position = target;
                            }
                        }
                        self.editor.clear_hover();
                    } else if let Some(pending) = self.pending_actor_grab.clone() {
                        let pdx = position.x - pending.start_mouse.0;
                        let pdy = position.y - pending.start_mouse.1;

                        if pdx * pdx + pdy * pdy >= 25.0 {
                            if let Some(target) = self.actor_surface_drag_target(pending.actor_id) {
                                self.editor.actor_grab_state =
                                    Some(crate::editor::ActorGrabState {
                                        actor_id: pending.actor_id,
                                        start_position: pending.start_position,
                                        current_position: target,
                                    });
                                self.pending_actor_grab = None;
                                self.editor.hovered_actor_id = None;
                                self.editor.hovered_cell = None;
                            }
                        }
                    } else if self.editor.actor_grab_state.is_some() {
                        let actor_id = self.editor.actor_grab_state.as_ref().unwrap().actor_id;
                        if let Some(target) = self.actor_surface_drag_target(actor_id) {
                            if let Some(grab) = self.editor.actor_grab_state.as_mut() {
                                grab.current_position = target;
                            }

                            self.editor.hovered_actor_id = None;
                            self.editor.hovered_cell = None;
                        }
                    } else if let Some(ref pending) = self.pending_grab.clone() {
                        let pdx = position.x - pending.start_mouse.0;
                        let pdy = position.y - pending.start_mouse.1;

                        if pdx * pdx + pdy * pdy >= 25.0 {
                            let target = self
                                .calculate_placement_target()
                                .unwrap_or(pending.anchor_coord);

                            let delta = WorldCoord::new(
                                target.x - pending.anchor_coord.x,
                                target.y - pending.anchor_coord.y,
                                target.z - pending.anchor_coord.z,
                            );

                            let valid =
                                self.validate_grab_destination(&pending.source_coords, delta);

                            self.editor.grab_state = Some(crate::editor::GrabState {
                                source_coords: pending.source_coords.clone(),
                                anchor_coord: pending.anchor_coord,
                                current_target: target,
                                delta,
                                valid,
                            });

                            self.pending_grab = None;
                        }
                    } else if self.editor.grab_state.is_some() {
                        let target = self.calculate_placement_target().unwrap_or_else(|| {
                            self.editor.grab_state.as_ref().unwrap().anchor_coord
                        });

                        let (delta, source_coords) = {
                            let grab = self.editor.grab_state.as_ref().unwrap();

                            let delta = WorldCoord::new(
                                target.x - grab.anchor_coord.x,
                                target.y - grab.anchor_coord.y,
                                target.z - grab.anchor_coord.z,
                            );

                            (delta, grab.source_coords.clone())
                        };

                        let valid = self.validate_grab_destination(&source_coords, delta);

                        if let Some(ref mut grab) = self.editor.grab_state {
                            grab.current_target = target;
                            grab.delta = delta;
                            grab.valid = valid;
                        }
                    }
                } else {
                    self.editor.clear_hover();
                }

                if self.editor_camera_controls_active() && self.is_right_mouse_down {
                    if let Some((last_x, last_y)) = self.last_set_cursor_pos {
                        if (position.x - last_x).abs() < 0.1 && (position.y - last_y).abs() < 0.1 {
                            self.last_set_cursor_pos = None;
                            return;
                        }
                    }

                    let pixels_per_point = egui_ctx.pixels_per_point();

                    let viewport_rect = self.editor.viewport_rect;

                    let center_logical = viewport_rect.center();

                    let center_physical = egui::pos2(
                        center_logical.x * pixels_per_point,
                        center_logical.y * pixels_per_point,
                    );

                    let min_side =
                        viewport_rect.width().min(viewport_rect.height()) * pixels_per_point;

                    let max_radius = min_side * RMB_GUARD_MAX_RADIUS_FACTOR;

                    let inner_radius = min_side * RMB_GUARD_INNER_RADIUS_FACTOR;

                    let offset = egui::pos2(
                        position.x as f32 - center_physical.x,
                        position.y as f32 - center_physical.y,
                    );

                    let distance = (offset.x * offset.x + offset.y * offset.y).sqrt();

                    let sens = self.editor.camera_sensitivity();
                    self.editor.camera.look(dx as f32 * sens, dy as f32 * sens);

                    if distance > inner_radius {
                        let denominator = (max_radius - inner_radius).max(0.001);

                        let strength = ((distance - inner_radius) / denominator).clamp(0.0, 1.0);

                        let direction = egui::vec2(offset.x / distance, offset.y / distance);

                        self.rmb_edge_scroll_velocity = glam::Vec2::new(direction.x, direction.y)
                            * strength
                            * RMB_GUARD_EDGE_SCROLL_SPEED;
                    } else {
                        self.rmb_edge_scroll_velocity = glam::Vec2::ZERO;
                    }

                    if distance > max_radius {
                        let clamped_offset = egui::vec2(
                            offset.x / distance * max_radius,
                            offset.y / distance * max_radius,
                        );

                        let clamped_pos = egui::pos2(
                            center_physical.x + clamped_offset.x,
                            center_physical.y + clamped_offset.y,
                        );

                        let _ = window.set_cursor_position(winit::dpi::PhysicalPosition::new(
                            clamped_pos.x as f64,
                            clamped_pos.y as f64,
                        ));

                        self.last_set_cursor_pos =
                            Some((clamped_pos.x as f64, clamped_pos.y as f64));

                        self.mouse_pos = (clamped_pos.x as f64, clamped_pos.y as f64);
                    }
                } else if self.is_middle_mouse_down && self.editor_camera_controls_active() {
                    let sens = self.editor.camera_sensitivity();
                    self.editor.camera.orbit(dx as f32 * sens, dy as f32 * sens);
                } else {
                    self.rmb_edge_scroll_velocity = glam::Vec2::ZERO;
                }
            }

            WindowEvent::MouseWheel { delta, .. } => {
                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && (self.editor.ui_mode || self.editor.effect_visible)
                {
                    return;
                }

                let pixels_per_point = egui_ctx.pixels_per_point();

                let mouse_logical = egui::pos2(
                    self.mouse_pos.0 as f32 / pixels_per_point,
                    self.mouse_pos.1 as f32 / pixels_per_point,
                );

                let in_viewport = self.editor.viewport_rect.contains(mouse_logical);

                if !in_viewport || egui_ctx.wants_pointer_input() {
                    return;
                }

                let y = match delta {
                    MouseScrollDelta::LineDelta(_, y) => *y,
                    MouseScrollDelta::PixelDelta(pos) => (pos.y / 100.0) as f32,
                };

                if self.editor_camera_controls_active() {
                    self.editor.camera.zoom(y);

                    if self.editor.mode == EditorMode::Editor {
                        self.update_hover();
                    }
                }
            }

            WindowEvent::Focused(false) => {
                self.editor_shortcut_keys.clear();
                let released_keys: Vec<KeyCode> = self.keys_down.drain().collect();

                if self.view == View::Editor
                    && self.editor.mode == EditorMode::Play
                    && self.script_scene.is_some()
                {
                    for key in released_keys {
                        self.script_pending_events.push((
                            "keypress".to_string(),
                            vec![
                                Value::String(crate::scripting::host::key_code_name(key)),
                                Value::Bool(false),
                            ],
                        ));
                    }
                }

                self.jump_requested = false;

                self.orbit_delta = [0.0, 0.0];

                self.is_right_mouse_down = false;
                self.is_middle_mouse_down = false;
                self.is_left_mouse_down = false;

                self.finish_landscaping_stroke();
                self.editor.clear_hover();

                self.pending_grab = None;
                self.editor.grab_state = None;
                self.pending_actor_grab = None;
                self.editor.actor_grab_state = None;
                self.editor.actor_transform_preview = None;
                self.editor.cell_move_preview = None;
                self.pending_source_grab = None;
                self.editor.source_grab_state = None;
            }

            _ => {}
        }
    }

    pub fn on_mouse_motion(&mut self, dx: f64, dy: f64) {
        if self.view == View::Editor
            && self.editor.mode == EditorMode::Play
            && self.character_system.get_active_player().is_some()
        {
            self.orbit_delta = [dx as f32, dy as f32];

            self.gameplay_camera.orbit(dx as f32, dy as f32);
        }
    }

    fn dispatch_undo_redo(&mut self, redo: bool) {
        if self.view != View::Editor || self.editor.mode != EditorMode::Editor {
            return;
        }
        if self.editor.show_script_workspace {
            return;
        }
        if self.editor.ui_mode {
            if redo {
                self.editor.redo_ui_document();
            } else {
                self.editor.undo_ui_document();
            }
        } else if redo {
            self.redo_editor_history();
        } else {
            self.undo_editor_history();
        }
    }

    pub(crate) fn push_undo_snapshot(&mut self) {
        let snapshot = self.editor.create_snapshot_without_terrain(&self.world);
        self.editor.history.push_undo_entry(snapshot);
        self.editor.history.redo_stack.clear();
    }

    pub(crate) fn push_full_undo_snapshot(&mut self) {
        let snapshot = self.editor.create_snapshot(&self.world);
        self.editor.history.push_undo_entry(snapshot);
        self.editor.history.redo_stack.clear();
    }

    fn undo_editor_history(&mut self) {
        self.editor.commit_pending_inspector_history();
        self.editor
            .finish_heightfield_erosion_for_history(&mut self.world);
        if self.editor.agent_change_is_latest(&self.world) {
            if let Some(root) = self.project_manager.current_project.as_deref() {
                if let Err(error) = self.editor.undo_agent_change(&mut self.world, root) {
                    self.editor
                        .script_editor
                        .push_output(format!("AI undo: {error}"));
                }
            }
            return;
        }

        if self.editor.undo_heightfield_history(&mut self.world) {
            return;
        }

        let Some((
            prev_cells,
            prev_sources,
            prev_bindings,
            prev_actors,
            prev_actor_bindings,
            prev_project_hierarchy,
            prev_terrain,
        )) = self.editor.history.undo_stack.pop()
        else {
            return;
        };

        let previous_actor_ids = self.world.actors.keys().copied().collect::<HashSet<_>>();
        let previous_source_ids = self.world.sources.keys().copied().collect::<HashSet<_>>();
        let redo_cells = self.world.swap_authored_cells_for_history(prev_cells);
        let previous_coord_set = redo_cells.keys().copied().collect::<HashSet<_>>();
        let redo_entry = (
            redo_cells,
            std::mem::replace(&mut self.world.sources, prev_sources),
            std::mem::replace(&mut self.world.script_bindings, prev_bindings),
            std::mem::replace(&mut self.world.actors, prev_actors),
            std::mem::replace(&mut self.world.actor_script_bindings, prev_actor_bindings),
            std::mem::replace(&mut self.editor.project_hierarchy, prev_project_hierarchy),
            prev_terrain.counterpart(&self.world.terrain),
        );
        self.editor.history.redo_stack.push(redo_entry);
        if self.editor.history.redo_stack.len() > self.editor.history.limit {
            let excess = self.editor.history.redo_stack.len() - self.editor.history.limit;
            self.editor.history.redo_stack.drain(0..excess);
        }
        self.world.rebuild_actor_module_index();
        prev_terrain.apply(&mut self.world.terrain);

        self.reconcile_editor_selection_after_history(
            &previous_coord_set,
            &previous_actor_ids,
            &previous_source_ids,
        );
        self.editor.clear_hover();
        self.editor.sanitize_project_explorer_selection();
        self.editor.needs_save = true;
    }

    fn redo_editor_history(&mut self) {
        self.editor.commit_pending_inspector_history();
        self.editor
            .finish_heightfield_erosion_for_history(&mut self.world);
        if self.editor.agent_redo_is_latest(&self.world) {
            if let Some(root) = self.project_manager.current_project.as_deref() {
                if let Err(error) = self.editor.redo_agent_change(&mut self.world, root) {
                    self.editor
                        .script_editor
                        .push_output(format!("AI redo: {error}"));
                }
            }
            return;
        }

        if self.editor.redo_heightfield_history(&mut self.world) {
            return;
        }

        let Some((
            next_cells,
            next_sources,
            next_bindings,
            next_actors,
            next_actor_bindings,
            next_project_hierarchy,
            next_terrain,
        )) = self.editor.history.redo_stack.pop()
        else {
            return;
        };

        let previous_actor_ids = self.world.actors.keys().copied().collect::<HashSet<_>>();
        let previous_source_ids = self.world.sources.keys().copied().collect::<HashSet<_>>();
        let undo_cells = self.world.swap_authored_cells_for_history(next_cells);
        let previous_coord_set = undo_cells.keys().copied().collect::<HashSet<_>>();
        let undo_entry = (
            undo_cells,
            std::mem::replace(&mut self.world.sources, next_sources),
            std::mem::replace(&mut self.world.script_bindings, next_bindings),
            std::mem::replace(&mut self.world.actors, next_actors),
            std::mem::replace(&mut self.world.actor_script_bindings, next_actor_bindings),
            std::mem::replace(&mut self.editor.project_hierarchy, next_project_hierarchy),
            next_terrain.counterpart(&self.world.terrain),
        );
        self.editor.history.push_undo_entry(undo_entry);
        self.world.rebuild_actor_module_index();
        next_terrain.apply(&mut self.world.terrain);

        self.reconcile_editor_selection_after_history(
            &previous_coord_set,
            &previous_actor_ids,
            &previous_source_ids,
        );
        self.editor.clear_hover();
        self.editor.sanitize_project_explorer_selection();
        self.editor.needs_save = true;
    }

    fn reconcile_editor_selection_after_history(
        &mut self,
        previous_coords: &HashSet<WorldCoord>,
        previous_actor_ids: &HashSet<u64>,
        previous_source_ids: &HashSet<u64>,
    ) {
        self.editor
            .selected_coords
            .retain(|coord| self.world.cells.contains_key(coord));
        self.editor.selected_coord = self
            .editor
            .selected_coord
            .filter(|coord| self.world.cells.contains_key(coord));
        if self.editor.selected_coord.is_none() {
            self.editor.selected_coord = self.editor.selected_coords.last().copied();
        }
        if self
            .editor
            .selected_actor_id
            .is_some_and(|id| !self.world.actors.contains_key(&id))
        {
            self.editor.selected_actor_id = None;
        }
        if self
            .editor
            .selected_source_id
            .is_some_and(|id| self.world.source(id).is_none())
        {
            self.editor.selected_source_id = None;
        }
        if self.editor.selected_coord.is_some() || !self.editor.selected_coords.is_empty() {
            self.editor.selected_actor_id = None;
            self.editor.selected_source_id = None;
        } else if self.editor.selected_actor_id.is_some() {
            self.editor.selected_source_id = None;
        }
        if self.editor.selected_coord.is_none()
            && self.editor.selected_actor_id.is_none()
            && self.editor.selected_source_id.is_none()
        {
            let restored_coords = self
                .world
                .cells
                .keys()
                .filter(|coord| !previous_coords.contains(coord))
                .copied()
                .collect::<Vec<_>>();
            if !restored_coords.is_empty() {
                self.editor.selected_coords = restored_coords;
                self.editor.selected_coord = self.editor.selected_coords.last().copied();
            } else if let Some(id) = self
                .world
                .actors
                .keys()
                .find(|id| !previous_actor_ids.contains(id))
                .copied()
            {
                self.editor.selected_actor_id = Some(id);
            } else if let Some(id) = self
                .world
                .sources
                .keys()
                .find(|id| !previous_source_ids.contains(id))
                .copied()
            {
                self.editor.selected_source_id = Some(id);
            }
        }
    }

    fn viewport_mouse(&self) -> (f32, f32, f32, f32) {
        let pixels_per_point =
            if self.editor.viewport_ppp.is_finite() && self.editor.viewport_ppp > 0.0 {
                self.editor.viewport_ppp
            } else {
                1.0
            };

        let rect = self.editor.viewport_rect;

        if rect.width() > 1.0 && rect.height() > 1.0 {
            let viewport_x = rect.min.x * pixels_per_point;

            let viewport_y = rect.min.y * pixels_per_point;

            let viewport_width = rect.width() * pixels_per_point;

            let viewport_height = rect.height() * pixels_per_point;

            (
                self.mouse_pos.0 as f32 - viewport_x,
                self.mouse_pos.1 as f32 - viewport_y,
                viewport_width,
                viewport_height,
            )
        } else {
            (
                self.mouse_pos.0 as f32,
                self.mouse_pos.1 as f32,
                self.renderer.width(),
                self.renderer.height(),
            )
        }
    }

    fn current_scene_hit(&self) -> Option<SceneHit> {
        let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();
        let scene = crate::editor::grid::picking::raycast_scene(
            mouse_x,
            mouse_y,
            viewport_width,
            viewport_height,
            &self.editor.camera,
            &self.world,
            true,
        );
        let terrain = scene.and_then(|_| {
            crate::editor::grid::picking::raycast_terrain_surface(
                mouse_x,
                mouse_y,
                viewport_width,
                viewport_height,
                &self.editor.camera,
                &self.world,
            )
        });
        scene_hit_visible_above_terrain(scene, terrain.as_ref())
    }

    fn actor_build_preview_at_cursor(&self) -> Option<crate::editor::editor::ActorBuildPreview> {
        let snap = self.editor.actor_build_snap.max(0.001);
        let selected = self.editor.build_actor_package.as_deref();
        let model_bounds = selected
            .and_then(crate::engine::entity_package::get_package)
            .and_then(|package| package.triangle_mesh_bounds());
        let (bounds_min, bounds_max) = model_bounds.unwrap_or((
            glam::Vec3::new(-0.5, 0.0, -0.5),
            glam::Vec3::new(0.5, 1.0, 0.5),
        ));
        let yaw = self.editor.actor_build_yaw_degrees.to_radians();
        let rotation = glam::Vec3::new(0.0, yaw, 0.0);
        let (point, terrain_surface) = if self.editor.actor_build_surface {
            let (mx, my, w, h) = self.viewport_mouse();
            let terrain = crate::editor::grid::picking::raycast_terrain_surface(
                mx,
                my,
                w,
                h,
                &self.editor.camera,
                &self.world,
            );
            let cell = crate::editor::grid::picking::raycast_world_surface(
                mx,
                my,
                w,
                h,
                &self.editor.camera,
                &self.world,
                false,
            );
            let on_terrain = terrain.as_ref().is_some_and(|terrain| {
                cell.as_ref()
                    .is_none_or(|cell| terrain.distance <= cell.distance)
            });
            if let Some(hit) = nearest_drag_surface(terrain, cell) {
                if hit.normal.y < 0.5 {
                    return None;
                }
                (hit.point, on_terrain)
            } else {
                let target = self.calculate_placement_target()?;
                (
                    glam::Vec3::new(
                        target.x as f32 + 0.5,
                        target.y as f32,
                        target.z as f32 + 0.5,
                    ),
                    false,
                )
            }
        } else {
            let (mx, my, w, h) = self.viewport_mouse();
            let (near, direction) =
                crate::editor::grid::picking::screen_ray(mx, my, w, h, &self.editor.camera)?;
            if direction.y.abs() < 1e-5 {
                return None;
            }
            let plane_y = self.editor.anchor.y as f32;
            let distance = (plane_y - near.y) / direction.y;
            if distance < 0.0 {
                return None;
            }
            (near + direction * distance, false)
        };
        let x = snap_actor_stud(point.x, snap);
        let z = snap_actor_stud(point.z, snap);
        let height = if terrain_surface {
            self.world
                .terrain
                .surface_height_at(x, z)
                .unwrap_or(point.y)
        } else {
            point.y
        };
        let y = actor_brush_placed_y(height, Some((bounds_min, bounds_max)), 1.0, rotation, 0.0);
        let position = glam::Vec3::new(x, y, z);
        let orientation = glam::Quat::from_rotation_y(yaw);
        let mut aabb_min = glam::Vec3::splat(f32::INFINITY);
        let mut aabb_max = glam::Vec3::splat(f32::NEG_INFINITY);
        for bx in [bounds_min.x, bounds_max.x] {
            for by in [bounds_min.y, bounds_max.y] {
                for bz in [bounds_min.z, bounds_max.z] {
                    let corner = position + orientation * glam::Vec3::new(bx, by, bz);
                    aabb_min = aabb_min.min(corner);
                    aabb_max = aabb_max.max(corner);
                }
            }
        }
        let mut valid = true;
        if self.editor.actor_build_check_overlap {
            valid = !self.world.actors.values().any(|other| {
                if !other.physics.collision_enabled {
                    return false;
                }
                let half = other.physics.collision_size * other.scale * 0.5;
                let center = other.position + other.physics.collision_offset;
                let other_min = center - half;
                let other_max = center + half;
                aabb_min.x < other_max.x - 0.001
                    && aabb_max.x > other_min.x + 0.001
                    && aabb_min.y < other_max.y - 0.001
                    && aabb_max.y > other_min.y + 0.001
                    && aabb_min.z < other_max.z - 0.001
                    && aabb_max.z > other_min.z + 0.001
            });
        }
        Some(crate::editor::editor::ActorBuildPreview {
            position,
            rotation,
            bounds_min,
            bounds_max,
            valid,
        })
    }

    fn actor_surface_drag_target(&self, actor_id: u64) -> Option<glam::Vec3> {
        let surface = self.actor_drag_target()?;
        let actor = self.world.actors.get(&actor_id)?;
        let bounds = actor
            .package
            .as_deref()
            .and_then(crate::engine::entity_package::get_package)
            .and_then(|package| package.triangle_mesh_bounds());
        Some(crate::editor::actor_placement::surface_position(
            surface,
            bounds,
            actor.scale,
            actor.rotation,
        ))
    }

    fn commit_actor_surface_drag(&mut self, grab: crate::editor::ActorGrabState) {
        if grab.current_position == grab.start_position
            || !self.world.actors.contains_key(&grab.actor_id)
        {
            return;
        }
        self.push_undo_snapshot();
        let selected = self.editor.selected_actor_ids();
        if selected.len() > 1 && selected.contains(&grab.actor_id) {
            let delta = grab.current_position - grab.start_position;
            for id in selected {
                if let Some(actor) = self.world.actors.get_mut(&id) {
                    actor.position += delta;
                }
            }
        } else if let Some(actor) = self.world.actors.get_mut(&grab.actor_id) {
            actor.position = grab.current_position;
        }
        self.editor.needs_save = true;
    }

    fn actor_drag_target(&self) -> Option<glam::Vec3> {
        let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();
        let terrain = crate::editor::grid::picking::raycast_terrain_surface(
            mouse_x,
            mouse_y,
            viewport_width,
            viewport_height,
            &self.editor.camera,
            &self.world,
        );
        let cell = crate::editor::grid::picking::raycast_world_surface(
            mouse_x,
            mouse_y,
            viewport_width,
            viewport_height,
            &self.editor.camera,
            &self.world,
            false,
        );
        nearest_drag_surface(terrain, cell).map(|hit| hit.point)
    }

    pub(crate) fn calculate_placement_target(&self) -> Option<WorldCoord> {
        let scene_hit = self.current_scene_hit();

        if !self.editor.plane_picking {
            let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();
            if let Some(terrain_hit) = crate::editor::grid::picking::raycast_terrain_surface(
                mouse_x,
                mouse_y,
                viewport_width,
                viewport_height,
                &self.editor.camera,
                &self.world,
            ) {
                if terrain_is_nearest_build_surface(&terrain_hit, scene_hit.as_ref()) {
                    return Some(terrain_build_target(&terrain_hit));
                }
            }
        }

        let replace_mode = self.keys_down.contains(&KeyCode::ControlLeft)
            || self.keys_down.contains(&KeyCode::ControlRight);
        if self.editor.current_tool == EditorTool::Build
            && replace_mode
            && let Some(SceneHit::Cell { coord, normal, .. }) = scene_hit
        {
            return Some(cell_build_target(coord, normal, true));
        }

        if let Some(hit) = scene_hit {
            match hit {
                SceneHit::Actor { .. } => {
                    return None;
                }

                SceneHit::Source { .. } => {
                    return None;
                }

                SceneHit::Cell { coord, normal, .. } => {
                    if !self.editor.plane_picking {
                        return Some(cell_build_target(coord, normal, false));
                    }
                }
            }
        }

        if self.editor.current_tool == EditorTool::Build && !self.editor.plane_picking {
            let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();

            if let Some((ray_origin, ray_direction)) = crate::editor::grid::picking::screen_ray(
                mouse_x,
                mouse_y,
                viewport_width,
                viewport_height,
                &self.editor.camera,
            ) {
                return Some(empty_world_placement_target(ray_origin, ray_direction));
            }
        }

        let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();

        crate::editor::grid::picking::update_hover(
            mouse_x,
            mouse_y,
            viewport_width,
            viewport_height,
            &self.editor.camera,
            self.editor.anchor,
        )
    }

    pub(crate) fn validate_grab_destination(
        &self,
        source_coords: &[WorldCoord],
        delta: WorldCoord,
    ) -> bool {
        let source_set: HashSet<_> = source_coords.iter().copied().collect();

        for &src in source_coords {
            let dest = WorldCoord::new(src.x + delta.x, src.y + delta.y, src.z + delta.z);

            if self.world.get(dest).is_some() && !source_set.contains(&dest) {
                return false;
            }
            if self
                .world
                .terrain
                .effective()
                .is_some_and(|terrain| terrain.solid_occupies_cell(dest))
            {
                return false;
            }
        }

        true
    }

    pub(crate) fn find_copy_pivot(&self, coords: &[WorldCoord]) -> WorldCoord {
        if let Some(hovered) = self.editor.hovered_cell {
            if coords.contains(&hovered) {
                return hovered;
            }
        }

        let mut best = coords[0];

        for &c in &coords[1..] {
            if c.y < best.y
                || (c.y == best.y && c.x < best.x)
                || (c.y == best.y && c.x == best.x && c.z < best.z)
            {
                best = c;
            }
        }

        best
    }

    pub(crate) fn copy_selection(&mut self) {
        let actor_ids = self.editor.selected_actor_ids();
        if actor_ids.len() > 1 {
            let mut actors = Vec::new();
            for id in actor_ids {
                if let Some(actor) = self.world.actors.get(&id).cloned() {
                    let bindings = self
                        .world
                        .actor_script_bindings
                        .iter()
                        .filter(|binding| binding.actor_id == id)
                        .cloned()
                        .collect();
                    actors.push((actor, bindings));
                }
            }
            if let Some((first, _)) = actors.first() {
                let pivot = first.position;
                self.editor.clipboard =
                    Some(crate::editor::EditorClipboard::Actors { pivot, actors });
            }
            return;
        }
        if let Some(actor_id) = self.editor.selected_actor_id {
            if let Some(actor) = self.world.actors.get(&actor_id).cloned() {
                let actor_bindings = self
                    .world
                    .actor_script_bindings
                    .iter()
                    .filter(|binding| binding.actor_id == actor_id)
                    .cloned()
                    .collect();
                self.editor.clipboard = Some(crate::editor::EditorClipboard::Actor {
                    actor,
                    actor_bindings,
                });
            }
            return;
        }

        if let Some(source_id) = self.editor.selected_source_id {
            if let Some(source) = self.world.source(source_id).cloned() {
                let script_binding = self
                    .world
                    .script_bindings
                    .iter()
                    .find(|binding| binding.target_identity == source_id)
                    .cloned();
                self.editor.clipboard = Some(crate::editor::EditorClipboard::Source {
                    source,
                    script_binding,
                });
            }
            return;
        }

        if self.editor.selected_coords.is_empty() && self.editor.selected_coord.is_none() {
            return;
        }

        let mut coords = Vec::new();
        for &coord in &self.editor.selected_coords {
            if !coords.contains(&coord) && self.world.get(coord).is_some() {
                coords.push(coord);
            }
        }
        if coords.is_empty() {
            if let Some(coord) = self.editor.selected_coord {
                if self.world.get(coord).is_some() {
                    coords.push(coord);
                }
            }
        }
        if coords.is_empty() {
            return;
        }

        let pivot = self.find_copy_pivot(&coords);
        let mut cells = Vec::new();
        for coord in &coords {
            if let Some(cell) = self.world.get(*coord) {
                let offset =
                    WorldCoord::new(coord.x - pivot.x, coord.y - pivot.y, coord.z - pivot.z);
                let script_binding = self
                    .world
                    .script_bindings
                    .iter()
                    .find(|binding| binding.target_identity == cell.id)
                    .cloned();
                cells.push(crate::editor::ClipboardCell {
                    offset,
                    cell: cell.clone(),
                    script_binding,
                });
            }
        }

        if !cells.is_empty() {
            self.editor.clipboard = Some(crate::editor::EditorClipboard::Cells {
                pivot_coord: pivot,
                cells,
            });
        }
    }

    pub(crate) fn paste_clipboard(&mut self) {
        let Some(clipboard) = self.editor.clipboard.clone() else {
            return;
        };

        match clipboard {
            crate::editor::EditorClipboard::Cells {
                pivot_coord: _,
                cells,
            } => {
                if cells.is_empty() {
                    return;
                }
                let Some(target_pivot) = self
                    .calculate_placement_target()
                    .or(self.editor.hovered_cell)
                else {
                    return;
                };

                self.push_undo_snapshot();
                match self.world.paste_cells(&cells, target_pivot) {
                    Ok(pasted_coords) => {
                        self.editor.selected_actor_id = None;
                        self.editor.selected_source_id = None;
                        self.editor.selected_coords = pasted_coords;
                        self.editor.selected_coord = Some(target_pivot);
                        self.editor.ensure_scene_home_workspace();
                        self.editor.reveal_selected_properties();
                        self.editor.needs_save = true;
                    }
                    Err(_error) => {
                        self.editor.history.undo_stack.pop();
                    }
                }
            }
            crate::editor::EditorClipboard::Actors { pivot, actors } => {
                let Some(target) = self.actor_drag_target() else {
                    return;
                };
                if actors.is_empty() {
                    return;
                }
                self.push_undo_snapshot();
                let mut pasted_nodes = Vec::new();
                let mut last_actor = None;
                for (actor, bindings) in actors {
                    let position = target + (actor.position - pivot);
                    let id = self.world.paste_actor(&actor, &bindings, position);
                    match self.editor.project_hierarchy.create_actor_node(None, id) {
                        Ok(node) => pasted_nodes.push(node),
                        Err(error) => crate::engine::diagnostics::report(
                            "ERROR",
                            "Project",
                            format!("Pasted Actor {id} node failed: {error}"),
                        ),
                    }
                    last_actor = Some(id);
                }
                if let Some(id) = last_actor {
                    self.editor.select_actor(id);
                    self.editor.selected_project_nodes = pasted_nodes;
                    self.editor.selected_project_node_id =
                        self.editor.selected_project_nodes.last().copied();
                }
                self.editor.needs_save = true;
            }
            crate::editor::EditorClipboard::Actor {
                actor,
                actor_bindings,
            } => {
                let Some(target) = self.actor_drag_target() else {
                    return;
                };

                self.push_undo_snapshot();
                let actor_id = self.world.paste_actor(&actor, &actor_bindings, target);
                if self
                    .editor
                    .project_hierarchy
                    .find_actor_node(actor_id)
                    .is_none()
                {
                    if let Err(error) = self
                        .editor
                        .project_hierarchy
                        .create_actor_node(None, actor_id)
                    {
                        crate::engine::diagnostics::report(
                            "ERROR",
                            "Project",
                            format!(
                                "Pasted Actor {} but failed to create its Project Explorer node: {}",
                                actor_id, error
                            ),
                        );
                    }
                }
                self.editor.select_actor(actor_id);
                self.editor.needs_save = true;
            }
            crate::editor::EditorClipboard::Source {
                source,
                script_binding,
            } => {
                let Some(target) = self.actor_drag_target() else {
                    return;
                };

                self.push_undo_snapshot();
                let source_id = self
                    .world
                    .paste_source(&source, script_binding.as_ref(), target);
                self.editor.select_source(source_id);
                self.editor.needs_save = true;
            }
        }
    }

    pub(crate) fn update_hover(&mut self) {
        let start = std::time::Instant::now();
        self.update_hover_inner();
        self.renderer
            .record_editor_hover_call(start.elapsed().as_secs_f64() * 1000.0);
    }

    fn update_hover_inner(&mut self) {
        if self.editor.actor_transform_preview.is_some() || self.editor.cell_move_preview.is_some()
        {
            self.editor.clear_hover();
            return;
        }
        let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();

        self.editor.hovered_terrain_hit = if (self.editor.sculpt_tool
            != crate::world::terrain::SculptTool::None
            || self.editor.actor_brush_enabled)
            && self.editor.mode == EditorMode::Editor
            && !self.editor.ui_mode
            && !self.editor.show_script_workspace
        {
            crate::editor::grid::picking::raycast_sculpt_terrain_surface(
                mouse_x,
                mouse_y,
                viewport_width,
                viewport_height,
                &self.editor.camera,
                &self.world,
            )
        } else {
            None
        };

        if self.editor.sculpt_tool != crate::world::terrain::SculptTool::None
            || self.editor.actor_brush_enabled
        {
            self.editor.clear_scene_object_hover();
            return;
        }

        if self.editor.source_grab_state.is_some() {
            self.editor.clear_hover();
            return;
        }

        if self.editor.actor_grab_state.is_some() {
            self.editor.hovered_actor_id = None;
            self.editor.hovered_cell = None;

            return;
        }

        if self.editor.grab_state.is_some() {
            let target = self.calculate_placement_target();

            self.editor.hovered_actor_id = None;

            self.editor.hovered_cell = target;

            return;
        }

        if self.editor.current_tool == EditorTool::Build && self.editor.build_is_actor {
            let preview = self.actor_build_preview_at_cursor();
            self.editor.actor_build_preview = preview;
            self.editor.clear_scene_object_hover();
            self.editor.hovered_cell = preview.map(|preview| {
                WorldCoord::new(
                    preview.position.x.floor() as i32,
                    preview.position.y.floor() as i32,
                    preview.position.z.floor() as i32,
                )
            });
            return;
        }
        self.editor.actor_build_preview = None;

        if self.editor.current_tool == EditorTool::Build && !self.editor.plane_picking {
            if let Some(target) = self.calculate_placement_target() {
                self.editor.clear_scene_object_hover();
                self.editor.hovered_cell = Some(target);
                return;
            }
        }

        let hit = self.current_scene_hit();

        match hit {
            Some(SceneHit::Source { source_id, .. }) => {
                self.editor.hovered_source_id = Some(source_id);
                self.editor.hovered_actor_id = None;
                self.editor.hovered_cell = None;
            }

            Some(SceneHit::Actor { actor_id, .. }) => {
                self.editor.hovered_source_id = None;
                self.editor.hovered_actor_id = Some(actor_id);

                self.editor.hovered_cell = None;
            }

            Some(SceneHit::Cell { coord, .. }) => {
                self.editor.hovered_source_id = None;
                self.editor.hovered_actor_id = None;

                self.editor.hovered_cell = Some(coord);

                if self.editor.current_tool == EditorTool::Build {
                    self.editor.hovered_cell = self.calculate_placement_target();
                }
            }

            None => {
                self.editor.clear_scene_object_hover();

                if self.editor.current_tool == EditorTool::Build && !self.editor.plane_picking {
                    self.editor.hovered_cell = self.calculate_placement_target();
                } else if self.editor.plane_picking
                    && self.editor.current_tool != EditorTool::Select
                    && self.editor.current_tool != EditorTool::Erase
                {
                    let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();

                    self.editor.hovered_cell = crate::editor::grid::picking::update_hover(
                        mouse_x,
                        mouse_y,
                        viewport_width,
                        viewport_height,
                        &self.editor.camera,
                        self.editor.anchor,
                    );
                }
            }
        }
    }

    fn apply_actor_brush_dab(&mut self, center: glam::Vec3) {
        let Some(package_name) = self.editor.actor_brush_package.clone() else {
            return;
        };
        let min_spacing = (100.0 / self.editor.actor_brush_density.max(0.1))
            .sqrt()
            .max(0.5);
        if self.editor.actor_brush_last_center.is_some_and(|previous| {
            previous.xz().distance(center.xz()) < (min_spacing * 0.45).max(0.5)
        }) {
            return;
        }
        self.editor.actor_brush_last_center = Some(center);
        self.editor.actor_brush_seed = self.editor.actor_brush_seed.wrapping_add(1);
        let candidates = actor_brush_candidates(
            center,
            self.editor.sculpt_brush_radius,
            self.editor.actor_brush_density,
            self.editor.actor_brush_scale_min,
            self.editor.actor_brush_scale_max,
            self.editor.actor_brush_tilt_min_degrees,
            self.editor.actor_brush_tilt_max_degrees,
            self.editor.actor_brush_random_y_rotation,
            self.editor.actor_brush_seed,
        );
        let package = crate::engine::entity_package::get_package(&package_name);
        let mesh_bounds = package
            .as_ref()
            .and_then(|package| package.triangle_mesh_bounds());

        for candidate in candidates {
            let Some(surface_y) = self
                .world
                .terrain
                .surface_height_at(candidate.xz.x, candidate.xz.y)
            else {
                continue;
            };
            let placed_y = actor_brush_placed_y(
                surface_y,
                mesh_bounds,
                candidate.scale,
                candidate.rotation,
                self.editor.actor_brush_y_offset,
            );
            let position = glam::Vec3::new(candidate.xz.x, placed_y, candidate.xz.y);
            let too_close_to_stroke = self
                .editor
                .actor_brush_stroke_positions
                .iter()
                .any(|placed| placed.xz().distance(position.xz()) < min_spacing);
            let too_close_to_existing = self
                .world
                .actors
                .values()
                .any(|actor| actor.position.xz().distance(position.xz()) < min_spacing);
            if too_close_to_stroke || too_close_to_existing {
                continue;
            }

            let actor_id = self.world.add_actor(package_name.clone(), position);
            if let Some(actor) = self.world.actors.get_mut(&actor_id) {
                actor.package = Some(
                    crate::engine::entity_package::asset_id_for_package_name(&package_name)
                        .unwrap_or_else(|| package_name.clone()),
                );
                actor.scale = glam::Vec3::splat(candidate.scale);
                actor.rotation = candidate.rotation;
                if let Some((min, max)) = mesh_bounds {
                    if let Some(pkg) = crate::engine::entity_package::get_package(&package_name) {
                        crate::engine::collision_geometry::set_actor_package_collision(actor, &pkg);
                    }
                }
            }
            if let Err(error) = self
                .editor
                .project_hierarchy
                .create_actor_node(None, actor_id)
            {
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Actor Brush",
                    format!("Actor {actor_id} was placed but its Project node failed: {error}"),
                );
            }
            self.editor.actor_brush_stroke_positions.push(position);
            self.editor.actor_brush_stroke_mutated = true;
        }
    }

    fn commit_actor_transform_preview(&mut self) -> bool {
        let Some(preview) = self.editor.actor_transform_preview.take() else {
            return false;
        };
        if !preview.changed() || !self.world.actors.contains_key(&preview.actor_id) {
            return false;
        }
        if !preview.current_position.is_finite()
            || !preview.current_rotation.is_finite()
            || !preview.current_scale.is_finite()
            || preview.current_scale.min_element() < 0.01
        {
            return false;
        }
        let selected = self.editor.selected_actor_ids();
        let group = selected.len() > 1 && selected.contains(&preview.actor_id);
        self.push_undo_snapshot();
        if group {
            let translation = preview.current_position - preview.start_position;
            let delta_rotation = preview.current_rotation - preview.start_rotation;
            let rotation = glam::Quat::from_euler(
                glam::EulerRot::XYZ,
                delta_rotation.x,
                delta_rotation.y,
                delta_rotation.z,
            );
            let scale_ratio =
                preview.current_scale / preview.start_scale.max(glam::Vec3::splat(0.01));
            for actor_id in selected {
                if let Some(actor) = self.world.actors.get_mut(&actor_id) {
                    match preview.mode {
                        TransformMode::Move => actor.position += translation,
                        TransformMode::Rotate => {
                            actor.position = preview.start_position
                                + rotation * (actor.position - preview.start_position);
                            actor.rotation += delta_rotation;
                        }
                        TransformMode::Scale => {
                            actor.position = preview.start_position
                                + (actor.position - preview.start_position) * scale_ratio;
                            actor.scale = (actor.scale * scale_ratio).max(glam::Vec3::splat(0.01));
                        }
                        TransformMode::Select => {}
                    }
                }
            }
        } else {
            let Some(actor) = self.world.actors.get_mut(&preview.actor_id) else {
                return false;
            };
            actor.position = preview.current_position;
            actor.rotation = preview.current_rotation;
            actor.scale = preview.current_scale;
        }
        self.editor.needs_save = true;
        true
    }

    fn begin_tool_pointer_action(&mut self, ctrl: bool) {
        if matches!(
            self.editor.current_tool,
            EditorTool::Navigate | EditorTool::Select
        ) {
            self.on_click();
            self.drag_start_coord = if self.editor.current_tool == EditorTool::Select
                && ctrl
                && self.editor.hovered_actor_id.is_none()
                && self.editor.hovered_source_id.is_none()
            {
                self.editor.hovered_cell
            } else {
                None
            };
        } else {
            self.drag_start_coord = self.editor.hovered_cell;
        }
    }

    fn finish_tool_pointer_action(&mut self) {
        if matches!(
            self.editor.current_tool,
            EditorTool::Build | EditorTool::Erase | EditorTool::Select
        ) {
            if let (Some(start), Some(end)) = (self.drag_start_coord, self.editor.hovered_cell) {
                if self.editor.current_tool != EditorTool::Select || start != end {
                    self.apply_tool_to_range(start, end);
                }
            }
        }
    }

    fn on_click(&mut self) {
        if self.view != View::Editor
            || self.editor.mode != EditorMode::Editor
            || self.editor.ui_mode
        {
            return;
        }

        match self.editor.current_tool {
            EditorTool::Navigate => {
                if self.editor.hovered_actor_id.is_some() {
                    return;
                }

                if let Some(coord) = self.editor.hovered_cell {
                    self.editor.set_anchor(coord);
                }
            }

            EditorTool::Select => {
                if let Some(source_id) = self.editor.hovered_source_id {
                    self.editor.selected_project_nodes.clear();
                    self.editor.selected_project_node_id = None;
                    self.editor.select_source(source_id);
                    return;
                }

                if let Some(actor_id) = self.editor.hovered_actor_id {
                    let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                        || self.keys_down.contains(&KeyCode::ControlRight);
                    if ctrl {
                        self.editor.toggle_actor_selection(actor_id);
                    } else {
                        self.editor.select_actor(actor_id);
                    }
                    return;
                }

                let Some(coord) = self.editor.hovered_cell else {
                    self.editor.clear_selection();
                    self.editor.selected_project_nodes.clear();
                    self.editor.selected_project_node_id = None;
                    return;
                };

                let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                    || self.keys_down.contains(&KeyCode::ControlRight);
                self.editor.selected_project_nodes.clear();
                self.editor.selected_project_node_id = None;

                if ctrl {
                    self.editor.selected_actor_id = None;
                    self.editor.selected_source_id = None;

                    if self.editor.selected_coords.contains(&coord) {
                        self.editor.selected_coords.retain(|c| *c != coord);

                        if self.editor.selected_coord == Some(coord) {
                            self.editor.selected_coord =
                                self.editor.selected_coords.last().copied();
                        }
                    } else if self.world.get(coord).is_some() {
                        self.editor.selected_coords.push(coord);

                        self.editor.selected_coord = Some(coord);
                    }
                } else if self.world.get(coord).is_some() {
                    self.editor.select_cell(coord);
                } else {
                    self.editor.clear_selection();
                }

                self.editor.enforce_cell_transform_mode();
                if self.editor.selected_coord.is_some()
                    || self.editor.selected_actor_id.is_some()
                    || self.editor.selected_source_id.is_some()
                {
                    self.editor.ensure_scene_home_workspace();
                    self.editor.reveal_selected_properties();
                }
            }

            _ => {}
        }
    }

    fn erase_actor(&mut self, actor_id: u64) {
        if !self.world.actors.contains_key(&actor_id) {
            return;
        }

        self.push_undo_snapshot();

        self.world.remove_actor(actor_id);

        self.editor.clear_selection();
        self.editor.clear_hover();

        self.editor.needs_save = true;
    }

    fn delete_selected_scene_source(&mut self) -> bool {
        let Some(source_id) = self.editor.selected_source_id else {
            return false;
        };
        if self.world.source(source_id).is_none() {
            self.editor.selected_source_id = None;
            return false;
        }
        self.erase_source(source_id);
        true
    }

    fn erase_source(&mut self, source_id: u64) {
        if self.world.source(source_id).is_none() {
            return;
        }

        self.push_undo_snapshot();
        self.world.remove_source(source_id);
        if let Some(node_id) = self.editor.project_hierarchy.find_cell_node(source_id) {
            self.editor.project_hierarchy.remove_node(node_id);
        }
        self.editor.clear_selection();
        self.editor.clear_hover();
        self.editor.needs_save = true;
    }

    pub(crate) fn apply_tool_to_range(&mut self, start: WorldCoord, end: WorldCoord) {
        if self.view != View::Editor
            || self.editor.mode != EditorMode::Editor
            || self.editor.ui_mode
        {
            return;
        }

        let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
            || self.keys_down.contains(&KeyCode::ControlRight);

        if self.editor.current_tool == EditorTool::Select {
            self.editor.selected_actor_id = None;
            self.editor.selected_source_id = None;

            if !ctrl {
                if self.world.get(end).is_some() {
                    self.editor.select_cell(end);
                } else {
                    self.editor.clear_selection();
                }
                return;
            }
        }

        let x_min = start.x.min(end.x);
        let x_max = start.x.max(end.x);

        let y_min = start.y.min(end.y);
        let y_max = start.y.max(end.y);

        let z_min = start.z.min(end.z);
        let z_max = start.z.max(end.z);

        let mut placed_actor_ids = Vec::new();
        let mut placed_source_ids = Vec::new();
        let mut placed_cells = Vec::new();
        let mut erased_cells = Vec::new();
        let mut history_pushed = false;

        for x in x_min..=x_max {
            for y in y_min..=y_max {
                for z in z_min..=z_max {
                    let coord = WorldCoord::new(x, y, z);

                    match self.editor.current_tool {
                        EditorTool::Build => {
                            if !self.editor.build_is_actor
                                && crate::editor::grid::picking::actor_intersects_cell(
                                    &self.world,
                                    coord,
                                )
                            {
                                continue;
                            }

                            if self.editor.build_is_actor {
                                if coord != end {
                                    continue;
                                }
                                let preview = self.editor.actor_build_preview;
                                if !history_pushed {
                                    self.push_undo_snapshot();
                                    history_pushed = true;
                                }
                                let position = preview.map(|preview| preview.position).unwrap_or(
                                    glam::Vec3::new(
                                        coord.x as f32 + 0.5,
                                        coord.y as f32,
                                        coord.z as f32 + 0.5,
                                    ),
                                );

                                let pkg = self.editor.build_actor_package.clone();

                                let identity = pkg
                                    .as_deref()
                                    .and_then(crate::engine::entity_package::get_package)
                                    .map(|asset| asset.metadata.name.clone())
                                    .unwrap_or_else(|| {
                                        pkg.clone().unwrap_or_else(|| "Actor".into())
                                    });

                                let actor_id = self.world.add_actor(identity, position);

                                if let Some(actor) = self.world.actors.get_mut(&actor_id) {
                                    if let Some(preview) = preview {
                                        actor.rotation = preview.rotation;
                                    }
                                    actor.package = pkg.map(|name| {
                                        crate::engine::entity_package::asset_id_for_package_name(
                                            &name,
                                        )
                                        .unwrap_or(name)
                                    });
                                    if let Some(package_name) = actor.package.as_deref()
                                        && let Some(package) =
                                            crate::engine::entity_package::get_package(package_name)
                                        && let Some((min, max)) = package.triangle_mesh_bounds()
                                    {
                                        crate::engine::collision_geometry::set_actor_package_collision(actor,&package);
                                    }
                                }

                                placed_actor_ids.push(actor_id);
                            } else {
                                let cell = self.editor.build_template.clone();
                                if crate::world::AuthoredSource::supports(cell.cell_type) {
                                    if !placed_source_ids.is_empty() {
                                        continue;
                                    }
                                    if !history_pushed {
                                        self.push_undo_snapshot();
                                        history_pushed = true;
                                    }
                                    let position = self.actor_drag_target().unwrap_or_else(|| {
                                        glam::Vec3::new(
                                            coord.x as f32 + 0.5,
                                            coord.y as f32 + 0.5,
                                            coord.z as f32 + 0.5,
                                        )
                                    });
                                    let id = self.world.add_source(cell.cell_type, position);
                                    if let Some(source) = self.world.source_mut(id) {
                                        source.cell = cell;
                                        source.cell.id = id;
                                    }
                                    placed_source_ids.push(id);
                                } else {
                                    if self.world.get(coord).is_some() && !ctrl {
                                        continue;
                                    }
                                    if !history_pushed {
                                        self.push_undo_snapshot();
                                        history_pushed = true;
                                    }
                                    self.world.set_cell(coord, cell.cell_type);

                                    if let Some(target) = self.world.get_mut(coord) {
                                        let id = target.id;

                                        *target = cell;

                                        target.id = id;

                                        placed_cells.push(coord);
                                    }
                                    self.world.bump_render_revision();
                                    self.world.mark_render_dirty(
                                        coord,
                                        crate::world::DirtyReason::Geometry,
                                    );
                                }
                            }
                        }

                        EditorTool::Erase => {
                            if self.world.get(coord).is_some() {
                                if !history_pushed {
                                    self.push_undo_snapshot();
                                    history_pushed = true;
                                }
                                self.world.set_cell(coord, CellType::Empty);
                                erased_cells.push(coord);
                            }
                        }

                        EditorTool::Select => {
                            if self.world.get(coord).is_some() {
                                if !self.editor.selected_coords.contains(&coord) {
                                    self.editor.selected_coords.push(coord);
                                }

                                self.editor.selected_coord = Some(coord);
                                self.editor.enforce_cell_transform_mode();
                            }
                        }

                        _ => {}
                    }
                }
            }
        }

        if self.editor.current_tool == EditorTool::Build
            && (!placed_actor_ids.is_empty()
                || !placed_source_ids.is_empty()
                || !placed_cells.is_empty())
        {
            self.editor.needs_save = true;

            if let Some(&actor_id) = placed_actor_ids.last() {
                self.editor.select_actor(actor_id);

                let actor_node_id = if let Some(node_id) =
                    self.editor.project_hierarchy.find_actor_node(actor_id)
                {
                    Some(node_id)
                } else {
                    match self
                        .editor
                        .project_hierarchy
                        .create_actor_node(None, actor_id)
                    {
                        Ok(node_id) => Some(node_id),
                        Err(error) => {
                            crate::engine::diagnostics::report(
                                "ERROR",
                                "Project",
                                format!(
                                    "Failed to create Project Explorer node for newly placed Actor {}: {}",
                                    actor_id, error
                                ),
                            );
                            None
                        }
                    }
                };

                if let Some(node_id) = actor_node_id {
                    self.editor.selected_project_node_id = Some(node_id);
                    self.editor.selected_project_nodes = vec![node_id];
                }
            } else if let Some(&source_id) = placed_source_ids.last() {
                self.editor.select_source(source_id);
            } else {
                self.editor.selected_actor_id = None;
                self.editor.selected_coords = placed_cells;
                self.editor.selected_coord = self.editor.selected_coords.last().copied();
                self.editor.ensure_scene_home_workspace();
                self.editor.reveal_selected_properties();
            }
        }

        if self.editor.current_tool == EditorTool::Select && self.editor.selected_coord.is_some() {
            self.editor.ensure_scene_home_workspace();
            self.editor.reveal_selected_properties();
        }

        if !erased_cells.is_empty() {
            self.editor.needs_save = true;
            if erased_cells
                .iter()
                .any(|coord| self.editor.selected_coords.contains(coord))
            {
                self.editor.clear_selection();
            }
            if self
                .editor
                .hovered_cell
                .is_some_and(|coord| erased_cells.contains(&coord))
            {
                self.editor.clear_scene_object_hover();
            }
        }

        if self.editor.current_tool == EditorTool::Erase {
            let mut actors_to_remove = Vec::new();

            for actor in self.world.actors.values() {
                if !actor.visible || !actor.enabled {
                    continue;
                }

                let actor_coord = WorldCoord::new(
                    actor.position.x.floor() as i32,
                    actor.position.y.floor() as i32,
                    actor.position.z.floor() as i32,
                );

                if actor_coord.x >= x_min
                    && actor_coord.x <= x_max
                    && actor_coord.y >= y_min
                    && actor_coord.y <= y_max
                    && actor_coord.z >= z_min
                    && actor_coord.z <= z_max
                {
                    actors_to_remove.push(actor.id);
                }
            }

            for &actor_id in &actors_to_remove {
                if !history_pushed {
                    self.push_undo_snapshot();
                    history_pushed = true;
                }
                self.world.remove_actor(actor_id);
            }

            if !actors_to_remove.is_empty() {
                self.editor.clear_selection();
                self.editor.clear_hover();
                self.editor.needs_save = true;
            }
        }
    }
}

fn egui_capture_blocks_engine_key(key: KeyCode, ctrl: bool, wants_keyboard: bool) -> bool {
    wants_keyboard && !(ctrl && key == KeyCode::KeyS)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn play_shortcuts_consume_edges_without_gameplay_or_repeat_toggles() {
        let mut app = App::new_for_tests(800.0, 600.0);
        app.view = View::Editor;
        assert!(app.route_play_shortcut(KeyCode::F5, true, false));
        assert_eq!(app.editor.mode, EditorMode::Play);
        assert!(app.route_play_shortcut(KeyCode::F5, true, true));
        assert_eq!(app.editor.mode, EditorMode::Play);
        assert!(app.route_play_shortcut(KeyCode::F5, false, false));
        assert!(app.route_play_shortcut(KeyCode::F2, true, false));
        assert!(app.editor.shell.play_tray_hidden);
        assert!(!app.route_play_shortcut(KeyCode::Escape, true, false));
        assert!(app.keys_down.is_empty());
        assert!(app.script_pending_events.is_empty());
        app.route_play_shortcut(KeyCode::F2, false, false);
        app.route_play_shortcut(KeyCode::F5, true, false);
        assert_eq!(app.editor.mode, EditorMode::Editor);
        assert!(!app.route_play_shortcut(KeyCode::F2, true, false));
        app.view = View::Home;
        assert!(!app.route_play_shortcut(KeyCode::F5, true, false));
    }

    #[test]
    fn play_shortcuts_follow_preferences_and_preserve_tray_geometry() {
        use crate::engine::preferences::PlayShortcut;
        let mut app = App::new_for_tests(800.0, 600.0);
        app.view = View::Editor;
        app.editor.theme_settings.bind_play = PlayShortcut::F8;
        app.editor.theme_settings.bind_play_tray = PlayShortcut::F9;
        app.editor.shell.dock_height = 287.0;
        app.editor.shell.dock = crate::editor::editor::shell::DockTab::Diagnostics;
        assert!(!app.route_play_shortcut(KeyCode::F5, true, false));
        app.route_play_shortcut(KeyCode::F8, true, false);
        for _ in 0..2 {
            app.route_play_shortcut(KeyCode::F9, true, false);
            app.route_play_shortcut(KeyCode::F9, false, false);
        }
        assert!(!app.editor.shell.play_tray_hidden);
        assert_eq!(app.editor.shell.dock_height, 287.0);
        assert_eq!(
            app.editor.shell.dock,
            crate::editor::editor::shell::DockTab::Diagnostics
        );
        assert_eq!(
            app.editor.theme_settings.play_hint(),
            "Press F8 to stop Play · F9: Show/hide bottom tray"
        );
    }

    #[test]
    fn flatten_radius_two_continuous_drag_coalesces_high_rate_mouse_events() {
        use std::time::Instant;
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.viewport_rect =
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0));
        app.world.terrain.config.grass.enabled = false;
        app.world.terrain.config.ocean_enabled = false;
        app.world.terrain.config.plains.base_height = 20.0;
        app.world.terrain.config.plains.height_variation = 6.0;
        app.world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [128, 48, 128],
            })
            .unwrap();
        let y = app.world.terrain.surface_height_at(64.0, 64.0).unwrap();
        app.editor.camera.target = glam::Vec3::new(64.0, y, 64.0);
        app.editor.camera.pitch = 0.9;
        app.editor.camera.yaw = 0.0;
        app.editor.camera.distance = 30.0;
        app.editor.sculpt_tool = crate::world::terrain::SculptTool::Flatten;
        app.editor.sculpt_brush_radius = 2.0;
        app.editor.sculpt_strength = 0.35;
        app.editor.sculpt_flatten_target = Some(y + 2.0);
        app.editor.sculpt_stroke_active = true;
        app.is_left_mouse_down = true;
        let start_revision = app.world.terrain.render_revision();
        let ctx = egui::Context::default();
        let mut costs = Vec::new();
        const FRAMES: usize = 90;
        const MOUSE_EVENTS_PER_FRAME: usize = 16;
        for frame in 0..FRAMES {
            for tick in 0..MOUSE_EVENTS_PER_FRAME {
                let t = (frame * MOUSE_EVENTS_PER_FRAME + tick) as f64
                    / (FRAMES * MOUSE_EVENTS_PER_FRAME) as f64;
                app.mouse_pos = (640.0 + (t * 70.0) - 35.0, 360.0 + ((t * 9.0).sin() * 5.0));
                app.sculpt_pointer_pending = true;
            }
            let began = Instant::now();
            app.update_continuous_sculpt(&ctx);
            costs.push(began.elapsed().as_secs_f64() * 1000.0);
            assert!(!app.sculpt_pointer_pending);
        }
        assert!(
            app.world.terrain.render_revision() > start_revision,
            "The benchmark must actually deform generated terrain"
        );
        costs.sort_by(f64::total_cmp);
        let p95 = costs[FRAMES * 95 / 100];
        let peak = costs[FRAMES - 1];
        let avg = costs.iter().sum::<f64>() / FRAMES as f64;
        eprintln!(
            "HELD-FLATTEN radius=2, 90 frames, 1440 mouse events: \
            cpu_stroke_avg_ms={avg:.3} p95_ms={p95:.3} max_ms={peak:.3}; \
            render/GPU not included"
        );
        assert!(p95 < 30.0, "sustained stroke CPU p95 {p95:.3} ms");
    }

    #[test]
    fn focused_egui_controls_keep_undo_redo_ownership() {
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyZ, true, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyY, true, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyX, true, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyZ, false, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyZ, true, true));
    }

    #[test]
    fn build_and_erase_undo_redo_refresh_editor_render_cache() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;
        let coord = WorldCoord::new(2, 0, -3);
        let chunk = crate::world::ChunkCoord::from_world_coord(coord);

        app.apply_tool_to_range(coord, coord);
        assert!(app.world.get(coord).is_some());
        app.world.drain_render_dirty_cells();
        *app.renderer.last_render_mode.borrow_mut() = Some(crate::renderer::RenderMode::Editor);
        *app.renderer.last_render_revision.borrow_mut() = app.world.render_revision();
        app.renderer.chunk_cache.borrow_mut().insert(
            chunk,
            crate::renderer::chunk::ChunkMesh {
                chunk_coord: chunk,
                main_vao: 0,
                main_vbo: 0,
                main_texture_batches: Vec::new(),
                shadow_vao: 0,
                shadow_vbo: 0,
                shadow_vertex_count: 0,
            },
        );
        assert!(app.renderer.chunk_cache.borrow().contains_key(&chunk));

        let revision_before_undo = app.world.render_revision();
        app.undo_editor_history();
        assert!(app.world.get(coord).is_none());
        assert!(app.world.render_revision() > revision_before_undo);
        assert_eq!(
            app.world
                .render_changes_since(revision_before_undo)
                .changes
                .get(&coord),
            Some(&crate::world::DirtyReason::Geometry)
        );
        app.renderer
            .update_chunk_cache(&app.world, crate::renderer::RenderMode::Editor);
        assert!(!app.renderer.chunk_cache.borrow().contains_key(&chunk));

        let revision_before_redo = app.world.render_revision();
        app.redo_editor_history();
        assert!(app.world.get(coord).is_some());
        assert!(app.world.render_revision() > revision_before_redo);
        assert_eq!(
            app.world
                .render_changes_since(revision_before_redo)
                .changes
                .get(&coord),
            Some(&crate::world::DirtyReason::Geometry)
        );

        app.world.drain_render_dirty_cells();
        *app.renderer.last_render_revision.borrow_mut() = app.world.render_revision();
        app.renderer.chunk_cache.borrow_mut().insert(
            chunk,
            crate::renderer::chunk::ChunkMesh {
                chunk_coord: chunk,
                main_vao: 0,
                main_vbo: 0,
                main_texture_batches: Vec::new(),
                shadow_vao: 0,
                shadow_vbo: 0,
                shadow_vertex_count: 0,
            },
        );
        app.editor.current_tool = EditorTool::Erase;
        app.apply_tool_to_range(coord, coord);
        assert!(app.world.get(coord).is_none());

        let revision_before_erase_undo = app.world.render_revision();
        app.undo_editor_history();
        assert!(app.world.get(coord).is_some());
        assert_eq!(
            app.world
                .render_changes_since(revision_before_erase_undo)
                .changes
                .get(&coord),
            Some(&crate::world::DirtyReason::Geometry)
        );

        app.world.drain_render_dirty_cells();
        *app.renderer.last_render_revision.borrow_mut() = app.world.render_revision();
        app.renderer.chunk_cache.borrow_mut().insert(
            chunk,
            crate::renderer::chunk::ChunkMesh {
                chunk_coord: chunk,
                main_vao: 0,
                main_vbo: 0,
                main_texture_batches: Vec::new(),
                shadow_vao: 0,
                shadow_vbo: 0,
                shadow_vertex_count: 0,
            },
        );
        let revision_before_erase_redo = app.world.render_revision();
        app.redo_editor_history();
        assert!(app.world.get(coord).is_none());
        assert_eq!(
            app.world
                .render_changes_since(revision_before_erase_redo)
                .changes
                .get(&coord),
            Some(&crate::world::DirtyReason::Geometry)
        );
        app.renderer
            .update_chunk_cache(&app.world, crate::renderer::RenderMode::Editor);
        assert!(!app.renderer.chunk_cache.borrow().contains_key(&chunk));
    }

    #[test]
    fn viewport_mouse_preserves_subunit_dpi_scale_and_panel_offset() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.editor.viewport_rect =
            egui::Rect::from_min_max(egui::pos2(240.0, 120.0), egui::pos2(1040.0, 720.0));
        for scale in [0.75_f32, 1.0, 1.5] {
            app.editor.viewport_ppp = scale;
            app.mouse_pos = (640.0 * f64::from(scale), 420.0 * f64::from(scale));
            let actual = app.viewport_mouse();
            let expected = (400.0 * scale, 300.0 * scale, 800.0 * scale, 600.0 * scale);
            for (actual, expected) in [
                (actual.0, expected.0),
                (actual.1, expected.1),
                (actual.2, expected.2),
                (actual.3, expected.3),
            ] {
                assert!(
                    (actual - expected).abs() < 0.001,
                    "DPI scale {scale}: {actual} != {expected}"
                );
            }
        }
        app.editor.viewport_ppp = f32::NAN;
        app.mouse_pos = (640.0, 420.0);
        assert_eq!(app.viewport_mouse(), (400.0, 300.0, 800.0, 600.0));
    }

    #[test]
    fn inspector_property_edit_undo_redo_restores_authored_cell() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        let coord = WorldCoord::new(6, 2, -4);
        app.world.set_cell(coord, CellType::Block);
        let original = app.world.get(coord).unwrap().clone();
        app.editor.select_cell(coord);

        app.editor.apply_property_changes(
            &mut app.world,
            &[crate::editor::editor::inspector::PropertyChange::Texture(
                "edited.png".into(),
            )],
        );
        assert_eq!(app.world.get(coord).unwrap().texture, "edited.png");
        assert!(app.editor.pending_inspector_history.is_some());

        app.undo_editor_history();
        assert_eq!(app.world.get(coord).unwrap(), &original);
        assert_eq!(app.editor.selected_coord, Some(coord));

        app.redo_editor_history();
        assert_eq!(app.world.get(coord).unwrap().texture, "edited.png");
        assert_eq!(app.editor.selected_coord, Some(coord));
    }

    #[test]
    fn placing_new_block_from_another_workspace_activates_home_and_selects_block() {
        use crate::editor::editor::feature_nav::EditorSurface;
        use crate::editor::editor::shell::Workspace;
        use crate::editor::editor::{LeftPanelTab, RightPanelTab};
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.activate_shell_workspace(Workspace::Terrain);
        app.editor.current_tool = EditorTool::Build;
        app.editor.shell.properties_visible = false;
        let coord = WorldCoord::new(4, 2, 3);
        app.apply_tool_to_range(coord, coord);
        assert!(app.world.get(coord).is_some());
        assert_eq!(app.editor.selected_coord, Some(coord));
        assert_eq!(app.editor.shell.workspace, Workspace::Home);
        assert_eq!(app.editor.navigation.surface, EditorSurface::Workspace);
        assert_eq!(app.editor.left_panel_tab, LeftPanelTab::Project);
        assert_eq!(app.editor.right_panel_tab, RightPanelTab::Inspector);
        assert!(app.editor.shell.explorer_visible && app.editor.shell.properties_visible);
        assert!(app.editor.needs_save);
    }

    #[test]
    fn actor_brush_candidates_honor_density_radius_and_random_scale_bounds() {
        let center = glam::Vec3::new(10.0, 5.0, -4.0);
        let sparse = actor_brush_candidates(center, 10.0, 1.0, 0.5, 1.5, 5.0, 12.0, true, 7);
        let dense = actor_brush_candidates(center, 10.0, 4.0, 0.5, 1.5, 5.0, 12.0, true, 7);
        assert!(dense.len() > sparse.len());
        assert_eq!(
            dense,
            actor_brush_candidates(center, 10.0, 4.0, 0.5, 1.5, 5.0, 12.0, true, 7)
        );
        assert!(dense.iter().any(|candidate| candidate.rotation.y > 0.0));
        for candidate in dense {
            assert!(candidate.xz.distance(center.xz()) <= 10.0 + f32::EPSILON);
            assert!((0.5..=1.5).contains(&candidate.scale));
            assert!((0.0..=std::f32::consts::TAU).contains(&candidate.rotation.y));
            let tilt_degrees = candidate
                .rotation
                .x
                .hypot(candidate.rotation.z)
                .to_degrees();
            assert!((5.0 - 0.001..=12.0 + 0.001).contains(&tilt_degrees));
        }
    }

    #[test]
    fn actor_brush_can_keep_y_rotation_fixed_and_ground_a_tilted_mesh() {
        let candidates =
            actor_brush_candidates(glam::Vec3::ZERO, 4.0, 2.0, 1.0, 1.0, 30.0, 30.0, false, 9);
        assert!(
            candidates
                .iter()
                .all(|candidate| candidate.rotation.y == 0.0)
        );
        assert!(candidates.iter().all(|candidate| {
            (candidate
                .rotation
                .x
                .hypot(candidate.rotation.z)
                .to_degrees()
                - 30.0)
                .abs()
                < 0.001
        }));

        let bounds = Some((
            glam::Vec3::new(-1.0, 0.0, -1.0),
            glam::Vec3::new(1.0, 2.0, 1.0),
        ));
        assert_eq!(
            actor_brush_mesh_bottom_y(bounds, 1.0, glam::Vec3::ZERO),
            0.0
        );
        let tilted_bottom = actor_brush_mesh_bottom_y(
            bounds,
            1.0,
            glam::Vec3::new(30.0_f32.to_radians(), 0.0, 0.0),
        );
        assert!((tilted_bottom + 0.5).abs() < 0.001);
        assert!(
            (actor_brush_placed_y(10.0, bounds, 1.0, glam::Vec3::ZERO, -0.3) - 9.7).abs() < 0.001
        );
        assert!(
            (actor_brush_placed_y(10.0, bounds, 1.0, glam::Vec3::ZERO, 1.0) - 11.0).abs() < 0.001
        );
    }

    #[test]
    fn digit_shortcut_keymap_matches_six_visible_tools_without_navigation() {
        for (digit, numpad, index) in [
            (KeyCode::Digit1, KeyCode::Numpad1, 1),
            (KeyCode::Digit2, KeyCode::Numpad2, 2),
            (KeyCode::Digit3, KeyCode::Numpad3, 3),
            (KeyCode::Digit4, KeyCode::Numpad4, 4),
            (KeyCode::Digit5, KeyCode::Numpad5, 5),
            (KeyCode::Digit6, KeyCode::Numpad6, 6),
        ] {
            assert_eq!(ribbon_digit_for_key(digit), Some(index));
            assert_eq!(ribbon_digit_for_key(numpad), Some(index));
        }
        assert_eq!(ribbon_digit_for_key(KeyCode::Digit7), None);
        assert_eq!(ribbon_digit_for_key(KeyCode::KeyN), None);
    }
    #[test]
    fn editor_authoring_shortcuts_stay_disabled_during_play() {
        assert!(editor_authoring_shortcuts_available(
            View::Editor,
            EditorMode::Editor,
            false,
        ));
        assert!(!editor_authoring_shortcuts_available(
            View::Editor,
            EditorMode::Play,
            false,
        ));
        assert!(!editor_authoring_shortcuts_available(
            View::Editor,
            EditorMode::Editor,
            true,
        ));
    }

    #[test]
    fn editor_camera_controls_remain_available_in_simulation_only_play() {
        assert!(editor_camera_controls_available(
            View::Editor,
            EditorMode::Editor,
            false,
            false,
        ));
        assert!(editor_camera_controls_available(
            View::Editor,
            EditorMode::Play,
            false,
            false,
        ));
        assert!(!editor_camera_controls_available(
            View::Editor,
            EditorMode::Play,
            false,
            true,
        ));
        assert!(!editor_camera_controls_available(
            View::Editor,
            EditorMode::Play,
            true,
            false,
        ));
        assert!(!editor_camera_controls_available(
            View::Home,
            EditorMode::Editor,
            false,
            false,
        ));
    }

    #[test]
    fn build_cell_auto_selects_new_content() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;

        let coord = WorldCoord::new(3, 2, -4);
        app.apply_tool_to_range(coord, coord);

        assert_eq!(app.editor.selected_coord, Some(coord));
        assert_eq!(app.editor.selected_coords, vec![coord]);
        assert!(app.editor.selected_actor_id.is_none());
        assert!(app.editor.show_properties_window);
        assert!(app.editor.needs_save);
    }

    #[test]
    fn build_preserves_zero_negative_and_safe_large_world_coordinates() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;
        let coords = [
            WorldCoord::new(0, 0, 0),
            WorldCoord::new(-17, -3, -29),
            WorldCoord::new(1_000_000, -750_000, 500_000),
        ];
        let mut ids = Vec::new();

        for coord in coords {
            app.apply_tool_to_range(coord, coord);
            ids.push(app.world.get(coord).expect("built Cell exists").id);
        }

        assert!(ids.iter().all(|id| *id != 0));
        assert_eq!(
            ids.iter()
                .copied()
                .collect::<std::collections::HashSet<_>>()
                .len(),
            3
        );
        for (coord, id) in coords.into_iter().zip(ids) {
            assert_eq!(
                app.world.get(coord).expect("exact coordinate stored").id,
                id
            );
        }
    }

    #[test]
    fn no_op_build_and_erase_do_not_add_undo_or_render_changes() {
        let coord = WorldCoord::new(0, 0, 0);
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;
        let actor_id = app
            .world
            .add_actor("Blocker", glam::Vec3::new(0.5, 0.0, 0.5));
        let revision = app.world.render_revision();

        app.apply_tool_to_range(coord, coord);

        assert!(app.world.get(coord).is_none());
        assert!(app.world.actors.contains_key(&actor_id));
        assert!(app.editor.history.undo_stack.is_empty());
        assert_eq!(app.world.render_revision(), revision);

        app.editor.current_tool = EditorTool::Erase;
        let empty_coord = WorldCoord::new(10, 5, 10);
        app.apply_tool_to_range(empty_coord, empty_coord);

        assert!(app.editor.history.undo_stack.is_empty());
        assert_eq!(app.world.render_revision(), revision);
    }

    #[test]
    fn erase_cell_marks_project_dirty_and_clears_matching_selection_and_hover() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Erase;
        let coord = WorldCoord::new(-2, 3, 5);
        app.world.set_cell(coord, CellType::Block);
        app.editor.select_cell(coord);
        app.editor.hovered_cell = Some(coord);

        app.apply_tool_to_range(coord, coord);

        assert!(app.world.get(coord).is_none());
        assert!(app.editor.needs_save);
        assert_eq!(app.editor.history.undo_stack.len(), 1);
        assert!(app.editor.selected_coord.is_none());
        assert!(app.editor.selected_coords.is_empty());
        assert!(app.editor.hovered_cell.is_none());
    }

    #[test]
    fn build_uses_raycast_face_and_ctrl_targets_hit_cell() {
        let coord = WorldCoord::new(0, 0, 0);
        for (yaw, pitch, expected_normal) in [
            (0.0, 0.0, glam::Vec3::Z),
            (std::f32::consts::FRAC_PI_2, 0.0, glam::Vec3::X),
            (std::f32::consts::PI, 0.0, -glam::Vec3::Z),
            (-std::f32::consts::FRAC_PI_2, 0.0, -glam::Vec3::X),
            (0.0, std::f32::consts::FRAC_PI_2, glam::Vec3::Y),
            (0.0, -std::f32::consts::FRAC_PI_2, -glam::Vec3::Y),
        ] {
            let mut app = App::new_for_tests(1280.0, 720.0);
            app.view = View::Editor;
            app.editor.current_tool = EditorTool::Build;
            app.editor.camera.target = glam::Vec3::splat(0.5);
            app.editor.camera.yaw = yaw;
            app.editor.camera.pitch = pitch;
            app.mouse_pos = (640.0, 360.0);
            app.world.set_cell(coord, CellType::Block);

            let hit = app.current_scene_hit().expect("center ray hits Cell");
            let SceneHit::Cell {
                coord: hit_coord,
                normal,
                ..
            } = hit
            else {
                panic!("expected Cell hit, got {hit:?}");
            };
            assert!((normal - expected_normal).length() < 0.001, "{normal:?}");
            assert_eq!(
                app.calculate_placement_target(),
                Some(cell_build_target(hit_coord, normal, false))
            );

            app.keys_down.insert(KeyCode::ControlLeft);
            assert_eq!(app.calculate_placement_target(), Some(hit_coord));
        }
    }

    #[test]
    fn build_only_replaces_occupied_cell_when_ctrl_is_held() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;
        app.editor.build_template = crate::world::Cell::new_block();
        app.editor.build_template.texture = "grass_1".into();
        let coord = WorldCoord::new(4, 2, -3);
        let old_id = app.world.set_cell(coord, CellType::Block);
        let before = app.world.render_revision();

        app.apply_tool_to_range(coord, coord);

        assert_eq!(app.world.get(coord).unwrap().id, old_id);
        assert_eq!(app.world.render_revision(), before);
        assert!(app.editor.history.undo_stack.is_empty());
        assert!(!app.editor.needs_save);

        app.keys_down.insert(KeyCode::ControlLeft);
        app.apply_tool_to_range(coord, coord);

        let replacement = app.world.get(coord).expect("replacement Cell exists");
        assert_ne!(replacement.id, old_id);
        assert_eq!(replacement.texture, "grass_1");
        assert_eq!(app.editor.history.undo_stack.len(), 1);
        assert!(app.world.render_revision() > before);
        assert!(app.editor.needs_save);
    }

    #[test]
    fn build_replacement_publishes_final_template_texture_revision() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;
        app.editor.build_template = crate::world::Cell::new_block();
        app.editor.build_template.texture = "grass_1".into();
        app.keys_down.insert(KeyCode::ControlLeft);

        let coord = WorldCoord::new(2, 0, 3);
        app.world.set_cell(coord, CellType::Block);
        app.world.get_mut(coord).unwrap().texture = "dirt".into();
        let before = app.world.render_revision();

        app.apply_tool_to_range(coord, coord);

        assert_eq!(app.world.get(coord).unwrap().texture, "grass_1");
        assert!(app.world.render_revision() > before);
        let changes = app.world.render_changes_since(before);
        assert_eq!(
            changes.changes.get(&coord),
            Some(&crate::world::DirtyReason::Geometry)
        );
    }

    #[test]
    fn actor_build_stud_snapping_and_placement_contract() {
        use crate::editor::editor::ActorBuildPreview;
        assert_eq!(snap_actor_stud(1.74, 1.0), 1.5);
        assert_eq!(snap_actor_stud(1.74, 0.25), 1.75);
        assert_eq!(snap_actor_stud(-0.51, 1.0), -0.5);
        assert_eq!(snap_actor_stud(2.38, 0.5), 2.5);

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = true;
        let bounds_min = glam::Vec3::new(-0.5, 0.0, -0.5);
        let bounds_max = glam::Vec3::new(0.5, 1.0, 0.5);
        let rotation = glam::Vec3::new(0.0, std::f32::consts::FRAC_PI_2, 0.0);
        let position = glam::Vec3::new(1.5, 3.25, 2.5);
        app.editor.actor_build_preview = Some(ActorBuildPreview {
            position,
            rotation,
            bounds_min,
            bounds_max,
            valid: true,
        });
        let coord = WorldCoord::new(1, 3, 2);
        app.apply_tool_to_range(coord, coord);
        assert_eq!(app.world.actors.len(), 1, "placed one Actor");
        let actor = app.world.actors.values().next().unwrap();
        assert_eq!(actor.position, position);
        assert_eq!(actor.rotation, rotation);

        assert!(crate::editor::grid::picking::actor_intersects_cell(
            &app.world, coord
        ));
        app.editor.actor_build_preview = Some(ActorBuildPreview {
            position,
            rotation,
            bounds_min,
            bounds_max,
            valid: false,
        });
        app.apply_tool_to_range(coord, coord);
        assert_eq!(app.world.actors.len(), 2);
        assert_eq!(
            app.world
                .actors
                .values()
                .filter(|a| a.position == position)
                .count(),
            2
        );

        app.editor.actor_build_preview = Some(ActorBuildPreview {
            position: glam::Vec3::new(4.5, 3.25, 4.5),
            rotation,
            bounds_min,
            bounds_max,
            valid: true,
        });
        app.apply_tool_to_range(WorldCoord::new(3, 3, 3), WorldCoord::new(4, 3, 4));
        assert_eq!(app.world.actors.len(), 3);
    }

    #[test]
    fn build_actor_auto_selects_and_syncs_project_explorer() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = true;
        app.editor.build_actor_package = None;

        let coord = WorldCoord::new(1, 0, 2);
        app.apply_tool_to_range(coord, coord);

        let actor_id = app.editor.selected_actor_id.expect("new Actor selected");
        assert!(app.world.actors.contains_key(&actor_id));

        let node_id = app
            .editor
            .project_hierarchy
            .find_actor_node(actor_id)
            .expect("new Actor represented in Project Explorer");

        assert_eq!(app.editor.selected_project_node_id, Some(node_id));
        assert_eq!(app.editor.selected_project_nodes, vec![node_id]);
        assert!(app.editor.show_properties_window);
        assert!(app.editor.needs_save);
    }

    #[test]
    fn fx_003_viewport_selected_emitter_deletes_and_undo_restores() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        let emitter = app
            .world
            .add_source(CellType::ParticleEmitter, glam::Vec3::new(3.5, 4.5, 5.5));
        let explorer_node = app
            .editor
            .project_hierarchy
            .create_cell_node(None, emitter)
            .expect("emitter in Explorer");
        app.editor.select_source(emitter);

        assert!(app.delete_selected_scene_source());
        assert!(app.world.source(emitter).is_none());
        assert!(!app.world.particle_emitter_ids.contains(&emitter));
        assert!(app.editor.project_hierarchy.node(explorer_node).is_none());
        assert!(app.editor.selected_source_id.is_none());
        assert!(app.editor.needs_save);
        assert!(!app.delete_selected_scene_source());

        app.undo_editor_history();
        assert!(app.world.source(emitter).is_some());
        assert!(
            app.editor
                .project_hierarchy
                .find_cell_node(emitter)
                .is_some()
        );
    }

    #[test]
    fn fx_003_delete_no_selected_emitter_does_not_remove_other_sources() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let emitter = app
            .world
            .add_source(CellType::ParticleEmitter, glam::Vec3::ZERO);
        assert!(!app.delete_selected_scene_source());
        assert!(app.world.source(emitter).is_some());
    }

    #[test]
    fn build_source_creates_non_voxel_freeform_object_and_selects_it() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Build;
        app.editor.build_is_actor = false;
        app.editor.build_template = crate::world::Cell::new_light();
        let coord = WorldCoord::new(2, 4, 6);

        app.apply_tool_to_range(coord, coord);

        let source_id = app.editor.selected_source_id.expect("new source selected");
        assert_eq!(
            app.world.source_position(source_id),
            Some(glam::Vec3::new(2.5, 4.5, 6.5))
        );
        assert!(app.world.get(coord).is_none());
        assert!(app.editor.needs_save);
    }

    #[test]
    fn actor_rotate_preview_commits_once_to_real_world_and_marks_dirty() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let id = app.world.add_actor("Test", glam::Vec3::ZERO);
        let start = egui::pos2(400.0, 300.0);
        let actor = app.world.actors.get(&id).unwrap();
        let mut preview =
            ActorTransformPreview::new(actor, TransformMode::Rotate, TransformAxis::Y, start);
        preview.update(
            start + egui::vec2(90.0, 0.0),
            &app.editor.camera,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0)),
        );
        app.editor.actor_transform_preview = Some(preview);
        let old = app.world.actors[&id].rotation;
        assert_eq!(
            old,
            glam::Vec3::ZERO,
            "preview must not mutate authored actor"
        );
        assert!(app.commit_actor_transform_preview());
        assert!((app.world.actors[&id].rotation.y - 0.9).abs() < 0.01);
        assert!(app.editor.needs_save);
        assert!(app.editor.actor_transform_preview.is_none());
        assert!(!app.commit_actor_transform_preview());
    }
    #[test]
    fn wk_select_001_copy_preserves_selected_actor_layout() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let a = app.world.add_actor("A", glam::Vec3::new(2.0, 0.0, 1.0));
        let b = app.world.add_actor("B", glam::Vec3::new(9.0, 2.0, 1.0));
        app.editor
            .project_hierarchy
            .create_actor_node(None, a)
            .unwrap();
        app.editor
            .project_hierarchy
            .create_actor_node(None, b)
            .unwrap();
        app.editor.select_actor(a);
        app.editor.toggle_actor_selection(b);
        app.copy_selection();
        match app.editor.clipboard.as_ref().unwrap() {
            crate::editor::EditorClipboard::Actors { pivot, actors } => {
                assert_eq!(actors.len(), 2);
                assert_eq!(*pivot, actors[0].0.position);
                assert_eq!(
                    actors[1].0.position - *pivot,
                    glam::Vec3::new(7.0, 2.0, 0.0)
                );
            }
            _ => panic!("Expected group clipboard"),
        }
    }

    #[test]
    fn wk_select_001_group_transform_preserves_offsets_and_one_undo() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let first = app.world.add_actor("First", glam::Vec3::new(2.0, 0.0, 0.0));
        let second = app
            .world
            .add_actor("Second", glam::Vec3::new(8.0, 0.0, 0.0));
        app.editor
            .project_hierarchy
            .create_actor_node(None, first)
            .unwrap();
        app.editor
            .project_hierarchy
            .create_actor_node(None, second)
            .unwrap();
        app.editor.select_actor(first);
        app.editor.toggle_actor_selection(second);
        let mut preview = ActorTransformPreview::new(
            &app.world.actors[&second],
            TransformMode::Move,
            TransformAxis::X,
            egui::pos2(0.0, 0.0),
        );
        preview.current_position.x += 3.0;
        app.editor.actor_transform_preview = Some(preview);
        assert!(app.commit_actor_transform_preview());
        assert_eq!(app.world.actors[&first].position.x, 5.0);
        assert_eq!(app.world.actors[&second].position.x, 11.0);
        assert_eq!(app.editor.history.undo_stack.len(), 1);
    }

    #[test]
    fn actor_scale_click_cancels_without_world_edit() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let id = app.world.add_actor("Test", glam::Vec3::ZERO);
        let start = egui::pos2(200.0, 200.0);
        let mut preview = ActorTransformPreview::new(
            &app.world.actors[&id],
            TransformMode::Scale,
            TransformAxis::Center,
            start,
        );
        preview.update(
            start + egui::vec2(1.0, 1.0),
            &app.editor.camera,
            egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 720.0)),
        );
        app.editor.actor_transform_preview = Some(preview);
        assert!(!app.commit_actor_transform_preview());
        assert_eq!(app.world.actors[&id].scale, glam::Vec3::ONE);
        assert!(!app.editor.needs_save);
    }
    #[test]
    fn actor_drag_commit_preserves_fractional_position() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let start = glam::Vec3::new(0.0, 4.0, 0.0);
        let id = app.world.add_actor("Independent centered mesh", start);
        let surface = glam::Vec3::new(1.75, 3.125, 3.5);
        let scale = glam::Vec3::new(2.0, 3.0, 4.0);
        let rotation = glam::Vec3::new(0.0, 0.0, std::f32::consts::FRAC_PI_2);
        let target = crate::editor::actor_placement::surface_position(
            surface,
            Some((glam::Vec3::splat(-0.5), glam::Vec3::splat(0.5))),
            scale,
            rotation,
        );
        {
            let actor = app.world.actors.get_mut(&id).unwrap();
            actor.scale = scale;
            actor.rotation = rotation;
        }
        app.commit_actor_surface_drag(crate::editor::ActorGrabState {
            actor_id: id,
            start_position: start,
            current_position: target,
        });
        let actor = &app.world.actors[&id];
        assert!((actor.position.y - 4.125).abs() < 0.0001);
        assert_eq!((actor.position.x, actor.position.z), (surface.x, surface.z));
        assert_eq!((actor.scale, actor.rotation), (scale, rotation));
        assert_eq!(app.editor.history.undo_stack.len(), 1);
        assert_eq!(app.editor.history.undo_stack[0].3[&id].position, start);
        assert!(app.editor.needs_save);
    }

    #[test]
    fn actor_surface_drag_preserves_group_offsets_and_one_undo() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let start = glam::Vec3::new(0.25, 2.5, -1.0);
        let id = app.world.add_actor("First", start);
        let offset = glam::Vec3::new(2.0, 1.0, 3.0);
        let second = app.world.add_actor("Second", start + offset);
        app.editor
            .project_hierarchy
            .create_actor_node(None, id)
            .unwrap();
        app.editor
            .project_hierarchy
            .create_actor_node(None, second)
            .unwrap();
        app.editor.select_actor(id);
        app.editor.toggle_actor_selection(second);
        let target = glam::Vec3::new(1.75, 3.125, 3.5);
        app.commit_actor_surface_drag(crate::editor::ActorGrabState {
            actor_id: id,
            start_position: start,
            current_position: target,
        });
        assert_eq!(app.world.actors[&id].position, target);
        assert_eq!(app.world.actors[&second].position - target, offset);
        assert_eq!(app.editor.history.undo_stack.len(), 1);
    }

    #[test]
    fn ordinary_click_replaces_stale_multi_selection() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Select;
        let first = WorldCoord::new(1, 0, 0);
        let second = WorldCoord::new(2, 0, 0);
        app.world.set_cell(first, CellType::Block);
        app.world.set_cell(second, CellType::Block);
        app.editor.selected_coord = Some(second);
        app.editor.selected_coords = vec![first, second];
        app.editor.hovered_cell = Some(first);

        app.on_click();

        assert_eq!(app.editor.selected_coord, Some(first));
        assert_eq!(app.editor.selected_coords, vec![first]);
    }

    #[test]
    fn ctrl_click_is_the_only_cell_multi_selection_toggle() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Select;
        let first = WorldCoord::new(1, 0, 0);
        let second = WorldCoord::new(2, 0, 0);
        app.world.set_cell(first, CellType::Block);
        app.world.set_cell(second, CellType::Block);
        app.editor.select_cell(first);
        app.keys_down.insert(KeyCode::ControlLeft);

        app.editor.hovered_cell = Some(second);
        app.on_click();
        assert_eq!(app.editor.selected_coords, vec![first, second]);

        app.editor.hovered_cell = Some(first);
        app.on_click();
        assert_eq!(app.editor.selected_coord, Some(second));
        assert_eq!(app.editor.selected_coords, vec![second]);
    }

    #[test]
    fn ctrl_drag_mouse_sequence_selects_range_and_preserves_prior_selection() {
        for ctrl in [KeyCode::ControlLeft, KeyCode::ControlRight] {
            let mut app = App::new_for_tests(1280.0, 720.0);
            app.view = View::Editor;
            app.editor.current_tool = EditorTool::Select;
            let start = WorldCoord::new(0, 0, 0);
            let end = WorldCoord::new(3, 0, 0);
            let previous = WorldCoord::new(9, 0, 0);
            for x in [0, 1, 2, 3, 9] {
                app.world
                    .set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
            }
            app.editor.select_cell(previous);
            app.keys_down.insert(ctrl);
            app.editor.hovered_cell = Some(start);
            app.is_left_mouse_down = true;
            app.begin_tool_pointer_action(true);
            app.editor.hovered_cell = Some(end);
            app.finish_tool_pointer_action();
            assert!(app.editor.selected_coords.contains(&previous));
            for x in 0..=3 {
                assert!(
                    app.editor
                        .selected_coords
                        .contains(&WorldCoord::new(x, 0, 0))
                );
            }
            assert_eq!(app.editor.selected_coords.len(), 5);
            assert!(
                !app.editor.needs_save,
                "Selection must not edit authored content"
            );
        }
    }

    #[test]
    fn ctrl_click_release_does_not_undo_selection_toggle() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Select;
        let coord = WorldCoord::new(0, 0, 0);
        app.world.set_cell(coord, CellType::Block);
        app.editor.select_cell(coord);
        app.keys_down.insert(KeyCode::ControlLeft);
        app.editor.hovered_cell = Some(coord);
        app.begin_tool_pointer_action(true);
        app.finish_tool_pointer_action();
        assert!(app.editor.selected_coords.is_empty());
        app.begin_tool_pointer_action(true);
        app.finish_tool_pointer_action();
        assert_eq!(app.editor.selected_coords, vec![coord]);
    }

    #[test]
    fn ordinary_select_press_and_release_do_not_arm_a_range() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Select;
        let start = WorldCoord::new(0, 0, 0);
        let end = WorldCoord::new(3, 0, 0);
        for x in 0..=3 {
            app.world
                .set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        app.editor.hovered_cell = Some(start);
        app.begin_tool_pointer_action(false);
        assert_eq!(app.drag_start_coord, None);
        app.editor.hovered_cell = Some(end);
        app.finish_tool_pointer_action();
        assert_eq!(app.editor.selected_coords, vec![start]);
    }

    #[test]
    fn unmodified_select_range_remains_single_selection() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.current_tool = EditorTool::Select;
        let start = WorldCoord::new(0, 0, 0);
        let end = WorldCoord::new(3, 0, 0);
        for x in start.x..=end.x {
            app.world
                .set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }

        app.apply_tool_to_range(start, end);

        assert_eq!(app.editor.selected_coord, Some(end));
        assert_eq!(app.editor.selected_coords, vec![end]);
    }

    #[test]
    fn ordinary_scene_tools_do_not_pick_through_nearer_terrain() {
        let terrain = SurfaceHit {
            coord: WorldCoord::new(4, 6, 8),
            point: glam::Vec3::new(4.5, 6.2, 8.5),
            normal: glam::Vec3::Y,
            distance: 4.0,
        };
        let farther = SceneHit::Actor {
            actor_id: 99,
            distance: 5.0,
        };
        let closer = SceneHit::Cell {
            coord: WorldCoord::new(4, 8, 8),
            normal: -glam::Vec3::Z,
            distance: 3.0,
        };
        assert_eq!(
            scene_hit_visible_above_terrain(Some(farther), Some(&terrain)),
            None
        );
        assert_eq!(
            scene_hit_visible_above_terrain(Some(closer), Some(&terrain)),
            Some(closer)
        );
        assert_eq!(
            scene_hit_visible_above_terrain(Some(farther), None),
            Some(farther)
        );
        assert_eq!(nearest_drag_surface(Some(terrain), None), Some(terrain));
        assert_eq!(
            nearest_drag_surface(
                Some(terrain),
                Some(SurfaceHit {
                    distance: 7.0,
                    ..terrain
                })
            ),
            Some(terrain)
        );
    }

    #[test]
    fn terrain_build_target_places_above_fractional_surface() {
        let hit = SurfaceHit {
            coord: WorldCoord::new(4, 12, -3),
            point: glam::Vec3::new(4.25, 12.3, -2.4),
            normal: glam::Vec3::Y,
            distance: 7.0,
        };

        assert_eq!(terrain_build_target(&hit), WorldCoord::new(4, 13, -3));
    }

    #[test]
    fn terrain_build_target_keeps_exact_integer_surface_level() {
        let hit = SurfaceHit {
            coord: WorldCoord::new(4, 12, -3),
            point: glam::Vec3::new(4.25, 12.0002, -2.4),
            normal: glam::Vec3::Y,
            distance: 7.0,
        };

        assert_eq!(terrain_build_target(&hit), WorldCoord::new(4, 12, -3));
    }

    #[test]
    fn nearer_terrain_blocks_farther_scene_geometry_for_build() {
        let terrain_hit = SurfaceHit {
            coord: WorldCoord::new(0, 3, 0),
            point: glam::Vec3::new(0.5, 3.25, 0.5),
            normal: glam::Vec3::Y,
            distance: 5.0,
        };
        let farther_cell = SceneHit::Cell {
            coord: WorldCoord::new(0, 3, 4),
            normal: -glam::Vec3::Z,
            distance: 9.0,
        };
        let nearer_cell = SceneHit::Cell {
            coord: WorldCoord::new(0, 3, 1),
            normal: -glam::Vec3::Z,
            distance: 3.0,
        };

        assert!(terrain_is_nearest_build_surface(
            &terrain_hit,
            Some(&farther_cell)
        ));
        assert!(!terrain_is_nearest_build_surface(
            &terrain_hit,
            Some(&nearer_cell)
        ));
        assert!(terrain_is_nearest_build_surface(&terrain_hit, None));
    }

    #[test]
    fn empty_world_placement_target_uses_fixed_distance_and_floor_snapping() {
        let origin = glam::Vec3::new(0.25, 1.75, -0.25);
        let direction = glam::Vec3::new(0.0, 0.0, -1.0);

        let target = empty_world_placement_target(origin, direction);

        assert_eq!(target, WorldCoord::new(0, 1, -13));
    }
}

#[cfg(test)]
#[path = "cell_tool_slice_tests.rs"]
mod cell_tool_slice_tests;

#[path = "cell_move.rs"]
mod cell_move;
