use std::collections::HashSet;

use glam::Vec3Swizzles;
use rand::rngs::StdRng;
use rand::{Rng, SeedableRng};
use winit::event::{ElementState, MouseButton, MouseScrollDelta, WindowEvent};
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::editor::EditorTool;
// Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
use crate::editor::editor::transform_gizmo::{
    ActorTransformPreview, TransformAxis, TransformMode, hit_test,
};
use crate::editor::grid::picking::{SceneHit, SurfaceHit};
use crate::scripting::value::Value;
use crate::world::{CellType, WorldCoord};

use super::{
    App, EditorMode, RMB_GUARD_EDGE_SCROLL_SPEED, RMB_GUARD_INNER_RADIUS_FACTOR,
    RMB_GUARD_MAX_RADIUS_FACTOR, View,
};

// -----------------------------------------------------------------------------
// Empty-world Build placement fallback
// -----------------------------------------------------------------------------
//
// Build normally places against authored scene geometry. A completely empty
// World has no surface for that ray to hit, so Plane OFF used to leave Build
// with no placement target at all. Keep the fallback intentionally short and
// predictable while we dogfood the exact feel of the distance.
const EMPTY_WORLD_PLACEMENT_DISTANCE: f32 = 12.0;
// Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:10 -05:00
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

// Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
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
    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
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

// Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
fn actor_brush_mesh_bottom_y(
    mesh_bounds: Option<(glam::Vec3, glam::Vec3)>,
    scale: f32,
    rotation: glam::Vec3,
) -> f32 {
    let Some((min, max)) = mesh_bounds else {
        return 0.0;
    };
    let orientation =
        glam::Quat::from_euler(glam::EulerRot::XYZ, rotation.x, rotation.y, rotation.z);
    let mut bottom = f32::INFINITY;
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                bottom = bottom.min((orientation * glam::Vec3::new(x, y, z) * scale).y);
            }
        }
    }
    bottom
}

// Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
fn actor_brush_placed_y(
    terrain_y: f32,
    mesh_bounds: Option<(glam::Vec3, glam::Vec3)>,
    scale: f32,
    rotation: glam::Vec3,
    y_offset: f32,
) -> f32 {
    terrain_y - actor_brush_mesh_bottom_y(mesh_bounds, scale, rotation) + y_offset
}

fn empty_world_placement_target(origin: glam::Vec3, direction: glam::Vec3) -> WorldCoord {
    let point = origin + direction * EMPTY_WORLD_PLACEMENT_DISTANCE;

    WorldCoord::new(
        point.x.floor() as i32,
        point.y.floor() as i32,
        point.z.floor() as i32,
    )
}

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 16:16 -05:00
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

fn terrain_build_target(hit: &SurfaceHit) -> WorldCoord {
    // Terrain is a heightfield-backed solid surface. Build placement belongs in
    // the first whole voxel at or above that surface, never inside/beyond it.
    // Ray refinement can land a fraction above an exact integer surface, so
    // snap near-integer Y values before applying ceil.
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

// Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:54 -05:00
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

fn home_shortcut_is_available(view: View, mode: EditorMode) -> bool {
    view != View::Editor || mode != EditorMode::Play
}

// -----------------------------------------------------------------------------
// Editor-camera input contract
// -----------------------------------------------------------------------------
//
// The editor camera remains the authoritative camera in normal Editor mode and
// in simulation-only Play when no explicit player exists. Keep this predicate
// shared so keyboard, mouse-look, orbit, and wheel controls cannot drift apart.
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

// Authoring shortcuts are intentionally stricter than camera controls.
// Simulation-only Play may navigate with the editor camera, but must never
// change Build/Select/Erase/Navigate tool state while the game is running.
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
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    // A brush's immediate preview operates on the effective terrain. On
    // release/focus loss, recompose from each independently edited source
    // so overlapping unrelated layers are restored deterministically.
    fn finish_landscaping_stroke(&mut self) {
        // Worldkiln 0.8.5 | WO MAT-052 | 2026-10-10 16:54 -05:00
        // The sole visible generated layer is already updated in-place and
        // its brush edit journal is authoritative. Do not re-voxelize it
        // after every stroke; composite worlds must still rebuild.
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

    // Route all viewport brush input through the selected layer. The authored
    // base cannot accidentally change while generated layers are present.
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
        if self.world.terrain.config.generation_layers.is_empty() {
            return self.world.paint_authored_terrain(request);
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

            WindowEvent::KeyboardInput {
                event: key_event, ..
            } => {
                if let PhysicalKey::Code(key) = key_event.physical_key {
                    let was_pressed = key_event.state == ElementState::Pressed;

                    if was_pressed {
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

                    let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                        || self.keys_down.contains(&KeyCode::ControlRight);
                    if egui_capture_blocks_engine_key(key, ctrl, egui_ctx.wants_keyboard_input()) {
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
                            || self.keys_down.contains(&KeyCode::ControlRight);

                        match key {
                            KeyCode::F5 => {
                                if self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                    && !self.editor.ui_mode
                                {
                                    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
                                    self.editor.commit_pending_inspector_history();
                                    self.editor.mode = EditorMode::Play;
                                }
                            }

                            // During Play, G is gameplay input (the Dungeon uses it
                            // for detach/drop). Editor navigation must not consume it
                            // or replace the running project with the Home screen.
                            KeyCode::KeyG
                                if home_shortcut_is_available(self.view, self.editor.mode) =>
                            {
                                self.view = if self.view == View::Home {
                                    View::Editor
                                } else {
                                    View::Home
                                };
                            }

                            KeyCode::KeyS => {
                                if ctrl && self.view == View::Editor {
                                    if self.editor.show_script_workspace {
                                        let shift = self.keys_down.contains(&KeyCode::ShiftLeft)
                                            || self.keys_down.contains(&KeyCode::ShiftRight);

                                        if shift {
                                            self.editor.script_editor.save_all();
                                        } else {
                                            self.editor.script_editor.save_active();
                                        }
                                    } else {
                                        self.save_project();
                                    }
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

                            KeyCode::KeyZ => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                {
                                    let shift = self.keys_down.contains(&KeyCode::ShiftLeft)
                                        || self.keys_down.contains(&KeyCode::ShiftRight);
                                    self.dispatch_undo_redo(shift);
                                }
                            }

                            KeyCode::KeyY => {
                                if ctrl
                                    && self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                {
                                    self.dispatch_undo_redo(true);
                                }
                            }

                            key if self.editor.theme_settings.bind_delete.matches_key(key) => {
                                if self.view == View::Editor
                                    && self.editor.mode == EditorMode::Editor
                                    && !self.editor.ui_mode
                                    && !self.editor.project_explorer_focused
                                {
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

                            // Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:10 -05:00
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
                                // WO69: Escape cancels an in-progress Actor gizmo drag.
                                // Preview has not touched World or undo history.
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
                                } else if self.editor.mode == EditorMode::Play {
                                    self.editor.mode = EditorMode::Editor;
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
                    && self.editor.ui_mode
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
                    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
                    // Always finish an existing stroke on release, even if UI or
                    // workspace ownership changed after the stroke began.
                    if *state == ElementState::Released && self.editor.sculpt_stroke_active {
                        self.is_left_mouse_down = false;
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
                                // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:19 -05:00
                                // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
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
                                self.editor.sculpt_stroke_mutated = false;
                                self.editor.sculpt_stroke_snapshot = Some(snapshot);
                                // Worldkiln 0.8.7 | WO 0.8.7-42 | 2026-10-08 21:13 -05:00
                                if self.editor.sculpt_tool.is_flatten() {
                                    self.editor.sculpt_flatten_target = Some(hit.point.y);
                                } else {
                                    self.editor.sculpt_flatten_target = None;
                                }

                                // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
                                // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:19 -05:00
                                // Capture only as-yet-unseen brush columns before mutation.
                                // Subsequent drag samples keep the first value of a column.
                                self.editor.capture_brush_columns(
                                    &self.world,
                                    [hit.point.x, hit.point.y, hit.point.z],
                                    self.editor.sculpt_brush_radius,
                                );
                                let changed = if self.editor.sculpt_tool
                                    == crate::world::terrain::SculptTool::Paint
                                {
                                    // Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
                                    // Direct Dirt ignores the configurable Subsurface role.
                                    let material = self
                                        .editor
                                        .terrain_paint_material_slot
                                        .resolve_material(&self.world.terrain.config.materials)
                                        .to_string();
                                    self.apply_landscaping_paint(
                                        &crate::world::terrain::TerrainPaintRequest {
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

                        self.is_left_mouse_down = true;

                        // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
                        // Handle selected Actor gizmo lines before scene raycasts:
                        // a handle can lie outside the Actor's collision shape.
                        let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                            || self.keys_down.contains(&KeyCode::ControlRight);
                        if self.editor.current_tool == EditorTool::Select && !ctrl {
                            let pointer = mouse_logical;
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
                                // Freeform sources have position only. Their existing
                                // Move action remains available; Rotate/Scale never
                                // incorrectly translate a source.
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
                                    // Clicking an Actor selects it in every transform mode.
                                    // Dragging in Rotate/Scale also manipulates that Actor:
                                    // Y for Rotate and uniform for Scale; X/Y/Z line
                                    // handles choose a specific axis on subsequent drags.
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
                                    self.editor.select_actor(actor_id);
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
                                if self.editor.selected_coords.contains(&hovered) {
                                    // An ordinary drag is always a single-object
                                    // manipulation. Stale Ctrl multi-selection
                                    // must not absorb a later unmodified drag.
                                    self.editor.select_cell(hovered);
                                    self.pending_grab = Some(PendingGrab {
                                        start_mouse: self.mouse_pos,
                                        anchor_coord: hovered,
                                        source_coords: vec![hovered],
                                    });

                                    self.drag_start_coord = None;

                                    return;
                                }
                            }
                        }

                        self.begin_tool_pointer_action(ctrl);
                    } else {
                        // WO69: commit only on release, with a single existing
                        // authored World undo snapshot; simple clicks do nothing.
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
                            if grab.current_position != grab.start_position {
                                self.push_undo_snapshot();

                                if let Some(actor) = self.world.actors.get_mut(&grab.actor_id) {
                                    actor.position = grab.current_position;
                                    self.editor.needs_save = true;
                                }
                            }

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
                    && self.editor.ui_mode
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

                // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
                // Drag continues across the viewport edge until release, but
                // never into Play/UI/Script mode.
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

                if in_viewport
                    && self.view == View::Editor
                    && self.editor.mode == EditorMode::Editor
                    && !self.editor.ui_mode
                {
                    self.update_hover();

                    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
                    let sculpt_input_allowed = self
                        .editor
                        .sculpt_viewport_input_allowed(in_viewport, egui_ctx.wants_pointer_input());
                    if self.editor.sculpt_tool != crate::world::terrain::SculptTool::None
                        && !sculpt_input_allowed
                    {
                        self.editor.hovered_terrain_hit = None;
                    }

                    if self.editor.sculpt_stroke_active
                        && self.is_left_mouse_down
                        && sculpt_input_allowed
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
                            // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
                            // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:19 -05:00
                            // Capture only as-yet-unseen brush columns before mutation.
                            // Subsequent drag samples keep the first value of a column.
                            self.editor.capture_brush_columns(
                                &self.world,
                                [hit.point.x, hit.point.y, hit.point.z],
                                self.editor.sculpt_brush_radius,
                            );
                            let changed = if self.editor.sculpt_tool
                                == crate::world::terrain::SculptTool::Paint
                            {
                                // Worldkiln 0.8.7 | WO 0.8.7-43 | 2026-10-08 21:14 -05:00
                                // Direct Dirt ignores the configurable Subsurface role.
                                let material = self
                                    .editor
                                    .terrain_paint_material_slot
                                    .resolve_material(&self.world.terrain.config.materials)
                                    .to_string();
                                self.apply_landscaping_paint(
                                    &crate::world::terrain::TerrainPaintRequest {
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
                            if let Some(target) = self.actor_drag_target() {
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
                        if let Some(target) = self.actor_drag_target() {
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
                    && self.editor.ui_mode
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

                    // Hover is authoring state; simulation-only Play gets camera
                    // zoom without re-enabling Build/Select/Erase hover behavior.
                    if self.editor.mode == EditorMode::Editor {
                        self.update_hover();
                    }
                }
            }

            WindowEvent::Focused(false) => {
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

                // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
                // Focus loss terminates a changed stroke through the same normal
                // history/dirty-state path as an LMB release.
                self.finish_landscaping_stroke();
                self.editor.clear_hover();

                self.pending_grab = None;
                self.editor.grab_state = None;
                self.pending_actor_grab = None;
                self.editor.actor_grab_state = None;
                self.editor.actor_transform_preview = None;
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

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
    /// Route the global shortcut to the active authoring history owner.
    fn dispatch_undo_redo(&mut self, redo: bool) {
        if self.view != View::Editor || self.editor.mode != EditorMode::Editor {
            return;
        }
        if self.editor.show_script_workspace {
            // The focused Script Editor text widget owns its document undo stack.
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
    // Destructive World reset includes Terrain, unlike ordinary scene edits.
    pub(crate) fn push_full_undo_snapshot(&mut self) {
        let snapshot = self.editor.create_snapshot(&self.world);
        self.editor.history.push_undo_entry(snapshot);
        self.editor.history.redo_stack.clear();
    }

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
    // Only the active editor mode receives its own history command.
    fn undo_editor_history(&mut self) {
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
        // Commit an active Inspector edit before replaying World history.
        self.editor.commit_pending_inspector_history();
        // Agent history includes saved file contents as well as the scene.
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
    fn redo_editor_history(&mut self) {
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
        self.editor.commit_pending_inspector_history();
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
    // Keep selections for restored objects so undo/redo does not strand the Inspector.
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

    // Worldkiln 0.8.5 | WO MAT-003 | 2026-10-10 14:16 -05:00
    // Preserve sub-1.0 Windows DPI scales: clamping to 1.0 offsets viewport picking.
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

        crate::editor::grid::picking::raycast_scene(
            mouse_x,
            mouse_y,
            viewport_width,
            viewport_height,
            &self.editor.camera,
            &self.world,
            true,
        )
    }

    fn actor_drag_target(&self) -> Option<glam::Vec3> {
        let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();

        crate::editor::grid::picking::raycast_world_surface(
            mouse_x,
            mouse_y,
            viewport_width,
            viewport_height,
            &self.editor.camera,
            &self.world,
            false,
        )
        .map(|hit| hit.point)
    }

    pub(crate) fn calculate_placement_target(&self) -> Option<WorldCoord> {
        let scene_hit = self.current_scene_hit();

        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 16:16 -05:00
        // Plane-OFF Build placement must treat Terrain as authored scene
        // geometry. Choose the nearest Terrain/scene blocker so the empty-world
        // fallback can never project a placement through Terrain.
        if self.editor.current_tool == EditorTool::Build && !self.editor.plane_picking {
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

        // Ctrl+Build targets the hit Cell itself. Ordinary Build retains the
        // hit-face placement target below, including all six raycast normals.
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

        // ---------------------------------------------------------------------
        // Empty-world Build placement fallback
        // ---------------------------------------------------------------------
        //
        // Surface placement remains authoritative whenever the ray hits authored
        // geometry. This path exists only for Build + Plane OFF + no scene hit.
        // It does not create a floor, does not clamp world coordinates, and can
        // therefore place into negative coordinates just like ordinary World
        // authoring.
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

    // Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:08 -05:00
    pub(crate) fn copy_selection(&mut self) {
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

    // Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:08 -05:00
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
                        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
                        self.editor.ensure_scene_home_workspace();
                        self.editor.reveal_selected_properties();
                        self.editor.needs_save = true;
                    }
                    Err(_error) => {
                        self.editor.history.undo_stack.pop();
                    }
                }
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

    // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:03 -05:00
    // Wrapper captures picker calls even with early returns.
    pub(crate) fn update_hover(&mut self) {
        let start = std::time::Instant::now();
        self.update_hover_inner();
        self.renderer
            .record_editor_hover_call(start.elapsed().as_secs_f64() * 1000.0);
    }

    fn update_hover_inner(&mut self) {
        if self.editor.actor_transform_preview.is_some() {
            self.editor.clear_hover();
            return;
        }
        let (mouse_x, mouse_y, viewport_width, viewport_height) = self.viewport_mouse();

        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
        // Only Landscaping sculpting owns the cached Terrain hover hit. Standard
        // tools avoid an otherwise unused Terrain raymarch on every mouse move.
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

        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 16:16 -05:00
        // Build/Plane OFF owns a surface-placement ghost. Resolve it before the
        // ordinary object-hover path so nearer Terrain can block a Cell/Actor/
        // source behind it just as authored voxel geometry does.
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
                // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:10 -05:00
                // Terrain picking is independent from Cell/Actor/source picking.
                // Preserve the Terrain hit when no ordinary scene object is hit.
                self.editor.clear_scene_object_hover();

                // -------------------------------------------------------------
                // Empty-world Build ghost
                // -------------------------------------------------------------
                //
                // Keep the fallback visible before click. Without this companion
                // hover path, calculate_placement_target() could resolve a valid
                // empty-world target while the creator still saw no Build ghost.
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

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
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
            // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
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
                actor.package = Some(package_name.clone());
                actor.scale = glam::Vec3::splat(candidate.scale);
                actor.rotation = candidate.rotation;
                if let Some((min, max)) = mesh_bounds {
                    actor.physics.collision_size = (max - min).max(glam::Vec3::splat(0.01));
                    actor.physics.collision_offset = (min + max) * 0.5;
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

    // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
    // Preview values are not written into World until release, so Esc/focus
    // loss are true cancellations and we need only one undo history entry.
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
        self.push_undo_snapshot();
        let Some(actor) = self.world.actors.get_mut(&preview.actor_id) else {
            return false;
        };
        actor.position = preview.current_position;
        actor.rotation = preview.current_rotation;
        actor.scale = preview.current_scale;
        self.editor.needs_save = true;
        true
    }

    /// Shared mouse-down path after Actor/source manipulation has been handled.
    fn begin_tool_pointer_action(&mut self, ctrl: bool) {
        if matches!(
            self.editor.current_tool,
            EditorTool::Navigate | EditorTool::Select
        ) {
            self.on_click();
            // Only Ctrl+Select arms a cell range; ordinary clicks remain single-object actions.
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

    /// Shared mouse-up path for Build, Erase, and Ctrl cell-range selection.
    fn finish_tool_pointer_action(&mut self) {
        if matches!(
            self.editor.current_tool,
            EditorTool::Build | EditorTool::Erase | EditorTool::Select
        ) {
            if let (Some(start), Some(end)) = (self.drag_start_coord, self.editor.hovered_cell) {
                // A stationary Ctrl-click already toggled on press; do not re-add it on release.
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
                    self.editor.select_source(source_id);
                    return;
                }

                if let Some(actor_id) = self.editor.hovered_actor_id {
                    self.editor.select_actor(actor_id);

                    return;
                }

                let Some(coord) = self.editor.hovered_cell else {
                    self.editor.clear_selection();
                    return;
                };

                let ctrl = self.keys_down.contains(&KeyCode::ControlLeft)
                    || self.keys_down.contains(&KeyCode::ControlRight);

                if ctrl {
                    self.editor.selected_actor_id = None;

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

                // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:11 -05:00
                if self.editor.selected_coord.is_some()
                    || self.editor.selected_actor_id.is_some()
                    || self.editor.selected_source_id.is_some()
                {
                    // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
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

    fn erase_source(&mut self, source_id: u64) {
        if self.world.source(source_id).is_none() {
            return;
        }

        self.push_undo_snapshot();
        self.world.remove_source(source_id);
        self.editor.clear_selection();
        self.editor.clear_hover();
        self.editor.needs_save = true;
    }

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:57 -05:00
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
                            if crate::editor::grid::picking::actor_intersects_cell(
                                &self.world,
                                coord,
                            ) {
                                continue;
                            }

                            if self.editor.build_is_actor {
                                if !history_pushed {
                                    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
                                    self.push_undo_snapshot();
                                    history_pushed = true;
                                }
                                let position = glam::Vec3::new(
                                    coord.x as f32 + 0.5,
                                    coord.y as f32,
                                    coord.z as f32 + 0.5,
                                );

                                let pkg = self.editor.build_actor_package.clone();

                                let identity = pkg.as_deref().unwrap_or("Actor").to_string();

                                let actor_id = self.world.add_actor(identity, position);

                                if let Some(actor) = self.world.actors.get_mut(&actor_id) {
                                    actor.package = pkg;
                                    if let Some(package_name) = actor.package.as_deref()
                                        && let Some(package) =
                                            crate::engine::entity_package::get_package(package_name)
                                        && let Some((min, max)) = package.triangle_mesh_bounds()
                                    {
                                        actor.physics.collision_size =
                                            (max - min).max(glam::Vec3::splat(0.01));
                                        actor.physics.collision_offset = (min + max) * 0.5;
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
                                        // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
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
                                        // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:54 -05:00
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
                                    // Worldkiln 0.9.3 | WO 0.9.3-09 | 2026-10-07 00:24 -05:00
                                    // set_cell() dirtied the default replacement Cell. The final
                                    // Build template assignment happens afterward, so publish a
                                    // second revision after the authored texture/color/flags are
                                    // actually installed. Persistent renderer consumers must not
                                    // keep the pre-template chunk material.
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
                                    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
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
                // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
                self.editor.ensure_scene_home_workspace();
                self.editor.reveal_selected_properties();
            }
        }

        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
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
                    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
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

// Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
fn egui_capture_blocks_engine_key(key: KeyCode, ctrl: bool, wants_keyboard: bool) -> bool {
    wants_keyboard && !(ctrl && key == KeyCode::KeyS) && key != KeyCode::KeyG
}

#[cfg(test)]
mod tests {
    use super::*;

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
    #[test]
    fn focused_egui_controls_keep_undo_redo_ownership() {
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyZ, true, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyY, true, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyX, true, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyZ, false, true));
        assert!(egui_capture_blocks_engine_key(KeyCode::KeyZ, true, true));
    }

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-003 | 2026-10-10 14:16 -05:00
    #[test]
    fn viewport_mouse_preserves_subunit_dpi_scale_and_panel_offset() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.editor.viewport_rect =
            egui::Rect::from_min_max(egui::pos2(240.0, 120.0), egui::pos2(1040.0, 720.0));
        for scale in [0.75_f32, 1.0, 1.5] {
            app.editor.viewport_ppp = scale;
            // Physical cursor sits at the center of an offset logical viewport.
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
        // Invalid DPI must not produce NaN coordinates or corrupt pointer handling.
        app.editor.viewport_ppp = f32::NAN;
        app.mouse_pos = (640.0, 420.0);
        assert_eq!(app.viewport_mouse(), (400.0, 300.0, 800.0, 600.0));
    }

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
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

    // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:36 -05:00
    #[test]
    fn placing_new_block_from_another_workspace_activates_home_and_selects_block() {
        use crate::editor::editor::feature_nav::EditorSurface;
        use crate::editor::editor::shell::Workspace;
        use crate::editor::editor::{LeftPanelTab, RightPanelTab};
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.view = View::Editor;
        app.editor.activate_shell_workspace(Workspace::Terrain);
        // Simulate stale Build ownership from a prior UI shortcut: even if
        // the mode is changed without first calling activate_editor_tool,
        // successful authored placement must return to Home.
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

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    #[test]
    fn actor_brush_candidates_honor_density_radius_and_random_scale_bounds() {
        let center = glam::Vec3::new(10.0, 5.0, -4.0);
        // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
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

    // Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:16 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:57 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:57 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:58 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:54 -05:00
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

    // Worldkiln 0.9.3 | WO 0.9.3-09 | 2026-10-07 00:24 -05:00
    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:54 -05:00
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

    // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
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
        let target = glam::Vec3::new(1.75, 3.125, 3.5);
        assert_eq!(target, glam::Vec3::new(1.75, 3.125, 3.5));
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
            // Cursor movement updates the hovered endpoint while the button and Ctrl stay down.
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

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 16:16 -05:00
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
