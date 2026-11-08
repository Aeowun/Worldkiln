// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::renderer::camera::CameraController;
use crate::world::{Actor, World, WorldCoord};
use egui::{Color32, Context, Pos2, Rect, Stroke};
use glam::{EulerRot, Quat, Vec2, Vec3, Vec4};

use super::{Editor, EditorTool};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TransformMode {
    #[default]
    Select,
    Move,
    Rotate,
    Scale,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TransformAxis {
    X,
    Y,
    Z,
    Center,
}
impl TransformAxis {
    pub const WORLD_AXES: [Self; 3] = [Self::X, Self::Y, Self::Z];
    pub fn vector(self) -> Vec3 {
        match self {
            Self::X => Vec3::X,
            Self::Y => Vec3::Y,
            Self::Z => Vec3::Z,
            Self::Center => Vec3::ZERO,
        }
    }
    fn label(self) -> &'static str {
        match self {
            Self::X => "X",
            Self::Y => "Y",
            Self::Z => "Z",
            Self::Center => "ALL",
        }
    }
}
#[derive(Clone, Debug)]
pub struct ActorTransformPreview {
    pub actor_id: u64,
    pub mode: TransformMode,
    pub axis: TransformAxis,
    pub start_mouse: Pos2,
    pub start_position: Vec3,
    pub start_rotation: Vec3,
    pub start_scale: Vec3,
    pub local_center: Vec3,
    pub current_position: Vec3,
    pub current_rotation: Vec3,
    pub current_scale: Vec3,
}
impl ActorTransformPreview {
    pub fn new(actor: &Actor, mode: TransformMode, axis: TransformAxis, start_mouse: Pos2) -> Self {
        Self {
            actor_id: actor.id,
            mode,
            axis,
            start_mouse,
            start_position: actor.position,
            start_rotation: actor.rotation,
            start_scale: actor.scale,
            local_center: crate::editor::grid::picking::actor_visual_local_center(actor),
            current_position: actor.position,
            current_rotation: actor.rotation,
            current_scale: actor.scale,
        }
    }
    fn start_visual_pivot(&self) -> Vec3 {
        crate::editor::grid::picking::actor_visual_world_pivot(
            self.start_position,
            self.start_rotation,
            self.start_scale,
            self.local_center,
        )
    }

    pub fn changed(&self) -> bool {
        self.current_position.distance_squared(self.start_position) > 1e-8
            || self.current_rotation.distance_squared(self.start_rotation) > 1e-8
            || self.current_scale.distance_squared(self.start_scale) > 1e-8
    }
    pub fn update(&mut self, current_mouse: Pos2, camera: &CameraController, viewport: Rect) {
        if !viewport.is_positive() {
            return;
        }
        let delta = Vec2::new(
            current_mouse.x - self.start_mouse.x,
            current_mouse.y - self.start_mouse.y,
        );
        if !delta.is_finite() {
            return;
        }
        if delta.length_squared() < 9.0 {
            self.current_position = self.start_position;
            self.current_rotation = self.start_rotation;
            self.current_scale = self.start_scale;
            return;
        }
        match self.mode {
            TransformMode::Move => {
                if let Some(position) = axis_move_position(
                    camera,
                    viewport,
                    self.start_visual_pivot(),
                    self.start_mouse,
                    current_mouse,
                    self.axis,
                ) {
                    self.current_position =
                        self.start_position + position - self.start_visual_pivot();
                }
            }
            TransformMode::Rotate => {
                let radians = match self.axis {
                    TransformAxis::X => -delta.y * 0.01,
                    TransformAxis::Y => delta.x * 0.01,
                    TransformAxis::Z => (delta.x + delta.y) * 0.007,
                    TransformAxis::Center => delta.x * 0.01,
                }
                .clamp(-std::f32::consts::TAU * 16.0, std::f32::consts::TAU * 16.0);
                let axis = if self.axis == TransformAxis::Center {
                    Vec3::Y
                } else {
                    self.axis.vector()
                };
                let start = Quat::from_euler(
                    EulerRot::XYZ,
                    self.start_rotation.x,
                    self.start_rotation.y,
                    self.start_rotation.z,
                );
                let rotated = (Quat::from_axis_angle(axis, radians) * start).normalize();
                let (x, y, z) = rotated.to_euler(EulerRot::XYZ);
                self.current_rotation = Vec3::new(x, y, z);
                self.current_position =
                    self.start_visual_pivot() - rotated * (self.start_scale * self.local_center);
            }
            TransformMode::Scale => {
                let amount = ((delta.x - delta.y) * 0.009).clamp(-8.0, 8.0).exp();
                let factor = if self.axis == TransformAxis::Center {
                    Vec3::splat(amount)
                } else {
                    Vec3::ONE + (self.axis.vector() * (amount - 1.0))
                };
                self.current_scale =
                    (self.start_scale * factor).clamp(Vec3::splat(0.01), Vec3::splat(1000.0));
                let start = Quat::from_euler(
                    EulerRot::XYZ,
                    self.start_rotation.x,
                    self.start_rotation.y,
                    self.start_rotation.z,
                );
                self.current_position =
                    self.start_visual_pivot() - start * (self.current_scale * self.local_center);
            }
            TransformMode::Select => {}
        }
    }
}
fn axis_move_position(
    camera: &CameraController,
    viewport: Rect,
    start: Vec3,
    start_mouse: Pos2,
    current_mouse: Pos2,
    axis: TransformAxis,
) -> Option<Vec3> {
    if axis == TransformAxis::Center || !viewport.is_positive() {
        return None;
    }
    let delta = Vec2::new(
        current_mouse.x - start_mouse.x,
        current_mouse.y - start_mouse.y,
    );
    if !delta.is_finite() {
        return None;
    }
    if delta.length_squared() < 9.0 {
        return Some(start);
    }
    let origin = project(camera, viewport, start)?;
    let end = project(camera, viewport, start + axis.vector())?;
    let screen_axis = Vec2::new(end.x - origin.x, end.y - origin.y);
    let sq = screen_axis.length_squared();
    if sq < 9.0 {
        return None;
    }
    let units = (delta.dot(screen_axis) / sq).clamp(-1000.0, 1000.0);
    Some(start + axis.vector() * units)
}

#[derive(Clone, Debug)]
pub struct CellMovePreview {
    pub axis: TransformAxis,
    start_position: Vec3,
    start_mouse: Pos2,
}
impl CellMovePreview {
    pub fn new(start_position: Vec3, axis: TransformAxis, start_mouse: Pos2) -> Self {
        Self {
            axis,
            start_position,
            start_mouse,
        }
    }
    pub fn delta(
        &self,
        mouse: Pos2,
        camera: &CameraController,
        viewport: Rect,
    ) -> Option<WorldCoord> {
        let position = axis_move_position(
            camera,
            viewport,
            self.start_position,
            self.start_mouse,
            mouse,
            self.axis,
        )?;
        let delta = (position - self.start_position).round();
        Some(WorldCoord::new(
            delta.x as i32,
            delta.y as i32,
            delta.z as i32,
        ))
    }
}

pub(crate) fn project(camera: &CameraController, viewport: Rect, position: Vec3) -> Option<Pos2> {
    if !position.is_finite() || !viewport.is_positive() {
        return None;
    }
    let aspect = viewport.width() / viewport.height();
    if !aspect.is_finite() || aspect < 0.01 {
        return None;
    }
    let projection =
        glam::camera::rh::proj::opengl::perspective(60.0_f32.to_radians(), aspect, 0.1, 1000.0);
    let view = camera.get_view_matrix();
    let clip = (projection * view) * Vec4::new(position.x, position.y, position.z, 1.0);
    if !clip.is_finite() || clip.w < 0.01 {
        return None;
    }
    let ndc = clip.truncate() / clip.w;
    if !ndc.is_finite() || ndc.z < -1.0 || ndc.z > 1.0 {
        return None;
    }
    Some(egui::pos2(
        viewport.left() + (ndc.x + 1.0) * viewport.width() * 0.5,
        viewport.top() + (1.0 - ndc.y) * viewport.height() * 0.5,
    ))
}
#[derive(Clone, Copy, Debug)]
pub struct GizmoAxisHandle {
    pub axis: TransformAxis,
    pub end: Pos2,
    pub start: Pos2,
}
fn handles(
    camera: &CameraController,
    viewport: Rect,
    position: Vec3,
) -> Option<(Pos2, [GizmoAxisHandle; 3])> {
    let center = project(camera, viewport, position)?;
    if !viewport.contains(center) {
        return None;
    }
    let mut result = [GizmoAxisHandle {
        axis: TransformAxis::X,
        start: center,
        end: center,
    }; 3];
    for (i, axis) in TransformAxis::WORLD_AXES.into_iter().enumerate() {
        let projected = project(camera, viewport, position + axis.vector())?;
        let direction = Vec2::new(projected.x - center.x, projected.y - center.y);
        if direction.length_squared() < 1e-6 {
            continue;
        }
        let dir = direction.normalize();
        result[i] = GizmoAxisHandle {
            axis,
            start: egui::pos2(center.x + dir.x * 14.0, center.y + dir.y * 14.0),
            end: egui::pos2(center.x + dir.x * 48.0, center.y + dir.y * 48.0),
        };
    }
    Some((center, result))
}
fn distance_to_segment(pos: Pos2, start: Pos2, end: Pos2) -> f32 {
    let seg = Vec2::new(end.x - start.x, end.y - start.y);
    let point = Vec2::new(pos.x - start.x, pos.y - start.y);
    let denominator = seg.length_squared();
    if denominator < 0.001 {
        return f32::MAX;
    }
    let t = (point.dot(seg) / denominator).clamp(0.0, 1.0);
    (point - seg * t).length()
}
pub fn hit_test(editor: &Editor, actor: &Actor, pointer: Pos2) -> Option<TransformAxis> {
    let local_center = crate::editor::grid::picking::actor_visual_local_center(actor);
    let pivot = crate::editor::grid::picking::actor_visual_world_pivot(
        actor.position,
        actor.rotation,
        actor.scale,
        local_center,
    );
    hit_test_at_position(editor, pivot, pointer)
}

pub fn hit_test_cells(editor: &Editor, world: &World, pointer: Pos2) -> Option<TransformAxis> {
    if editor.transform_mode != TransformMode::Move {
        return None;
    }
    let position = editor.cell_move_pivot(world)?;
    hit_test_at_position(editor, position, pointer)
}

fn hit_test_at_position(editor: &Editor, position: Vec3, pointer: Pos2) -> Option<TransformAxis> {
    if editor.transform_mode == TransformMode::Select
        || editor.current_tool != EditorTool::Select
        || editor.ui_mode
        || editor.show_script_workspace
        || !editor.viewport_rect.contains(pointer)
    {
        return None;
    }
    let (center, lines) = handles(&editor.camera, editor.viewport_rect, position)?;
    if editor.transform_mode == TransformMode::Scale && center.distance(pointer) < 13.0 {
        return Some(TransformAxis::Center);
    }
    if let Some((_, axis)) = lines
        .iter()
        .filter(|line| line.start != line.end)
        .filter_map(|line| {
            let d = pointer.distance(line.end);
            (d <= 12.0).then_some((d, line.axis))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
    {
        return Some(axis);
    }
    lines
        .into_iter()
        .filter(|line| line.start != line.end)
        .filter_map(|line| {
            let d = distance_to_segment(pointer, line.start, line.end);
            (d <= 11.0).then_some((d, line.axis))
        })
        .min_by(|a, b| a.0.total_cmp(&b.0))
        .map(|(_, axis)| axis)
}
impl Editor {
    pub fn transform_mode_enabled(&self, mode: TransformMode) -> bool {
        !(matches!(mode, TransformMode::Rotate | TransformMode::Scale)
            && (self.selected_coord.is_some() || !self.selected_coords.is_empty()))
    }
    pub(crate) fn enforce_cell_transform_mode(&mut self) {
        if !self.transform_mode_enabled(self.transform_mode) {
            self.transform_mode = TransformMode::Move;
            self.actor_transform_preview = None;
        }
    }
    pub(crate) fn cancel_cell_move_preview(&mut self) {
        if self.cell_move_preview.take().is_some() {
            self.grab_state = None;
        }
    }
    pub fn selected_cell_move_coords(&self, world: &World) -> Vec<WorldCoord> {
        if self.selected_actor_id.is_some() || self.selected_source_id.is_some() {
            return Vec::new();
        }
        let mut seen = std::collections::HashSet::new();
        self.selected_coords
            .iter()
            .copied()
            .chain(self.selected_coord)
            .filter(|c| world.get(*c).is_some() && seen.insert(*c))
            .collect()
    }
    pub fn cell_move_pivot(&self, world: &World) -> Option<Vec3> {
        let coords = self.selected_cell_move_coords(world);
        let first = coords.first()?;
        let mut low = Vec3::new(first.x as f32, first.y as f32, first.z as f32);
        let mut high = low;
        for c in coords {
            let p = Vec3::new(c.x as f32, c.y as f32, c.z as f32);
            low = low.min(p);
            high = high.max(p);
        }
        Some((low + high + Vec3::ONE) * 0.5)
    }
    pub fn activate_transform_mode(&mut self, mode: TransformMode) {
        if !self.transform_mode_enabled(mode) {
            return;
        }
        self.activate_editor_tool(EditorTool::Select);
        self.transform_mode = mode;
        self.actor_transform_preview = None;
    }
    pub(crate) fn draw_actor_transform_gizmo(&self, ctx: &Context, world: &World) {
        if self.mode != crate::engine::EditorMode::Editor
            || self.ui_mode
            || self.show_script_workspace
            || self.navigation.surface.is_settings()
            || self.current_tool != EditorTool::Select
            || self.transform_mode == TransformMode::Select
        {
            return;
        }
        if !self.transform_mode_enabled(self.transform_mode) {
            return;
        }
        let position =
            if let Some(actor) = self.selected_actor_id.and_then(|id| world.actors.get(&id)) {
                let local_center = crate::editor::grid::picking::actor_visual_local_center(actor);
                let preview = self
                    .actor_transform_preview
                    .as_ref()
                    .filter(|p| p.actor_id == actor.id);
                let (position, rotation, scale) = preview
                    .map(|p| (p.current_position, p.current_rotation, p.current_scale))
                    .unwrap_or((actor.position, actor.rotation, actor.scale));
                crate::editor::grid::picking::actor_visual_world_pivot(
                    position,
                    rotation,
                    scale,
                    local_center,
                )
            } else if self.transform_mode == TransformMode::Move {
                let Some(mut position) = self.cell_move_pivot(world) else {
                    return;
                };
                if let Some(grab) = self.grab_state.as_ref() {
                    position += Vec3::new(
                        grab.delta.x as f32,
                        grab.delta.y as f32,
                        grab.delta.z as f32,
                    );
                }
                position
            } else {
                return;
            };
        let Some((center, lines)) = handles(&self.camera, self.viewport_rect, position) else {
            return;
        };
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("worldkiln_actor_gizmo"),
            ))
            .with_clip_rect(self.viewport_rect);
        let style = ctx.style();
        let foreground = style.visuals.text_color();
        let accent = style.visuals.selection.stroke.color;
        painter.circle_stroke(center, 7.0, Stroke::new(1.4, accent));
        if self.transform_mode == TransformMode::Scale {
            painter.circle_filled(center, 5.0, accent);
        }
        for line in lines {
            if line.end == line.start {
                continue;
            }
            let hovered = ctx
                .input(|i| i.pointer.hover_pos())
                .is_some_and(|p| distance_to_segment(p, line.start, line.end) < 11.0);
            let stroke = Stroke::new(
                if hovered { 3.1 } else { 2.0 },
                if hovered { accent } else { foreground },
            );
            painter.line_segment([line.start, line.end], stroke);
            if self.transform_mode == TransformMode::Scale {
                painter.rect_filled(
                    Rect::from_center_size(line.end, egui::vec2(8.0, 8.0)),
                    1.5,
                    if hovered { accent } else { foreground },
                );
            } else {
                painter.circle_filled(
                    line.end,
                    if hovered { 6.5 } else { 5.0 },
                    if hovered { accent } else { foreground },
                );
            }
            painter.text(
                line.end + egui::vec2(8.0, -8.0),
                egui::Align2::LEFT_BOTTOM,
                line.axis.label(),
                egui::TextStyle::Body.resolve(&style),
                if hovered { accent } else { foreground },
            );
        }
        let gesture = self
            .actor_transform_preview
            .as_ref()
            .map(|p| (p.mode, p.axis))
            .or_else(|| {
                self.cell_move_preview
                    .as_ref()
                    .map(|p| (TransformMode::Move, p.axis))
            });
        if let Some((mode, axis)) = gesture {
            let action = match mode {
                TransformMode::Move => "Move",
                TransformMode::Rotate => "Rotate",
                TransformMode::Scale => "Scale",
                TransformMode::Select => "Select",
            };
            painter.text(
                center + egui::vec2(0.0, 64.0),
                egui::Align2::CENTER_TOP,
                format!("{action} {}", axis.label()),
                egui::TextStyle::Small.resolve(&style),
                foreground,
            );
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn axis_move_projects_and_changes_only_one_coordinate() {
        let camera = CameraController::new();
        let viewport = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 700.0));
        let actor = Actor::new(7, "Actor", Vec3::ZERO);
        let start = egui::pos2(600.0, 350.0);
        let mut drag =
            ActorTransformPreview::new(&actor, TransformMode::Move, TransformAxis::X, start);
        let projected_start = project(&camera, viewport, Vec3::ZERO).unwrap();
        let projected_end = project(&camera, viewport, Vec3::X).unwrap();
        let delta = projected_end - projected_start;
        drag.update(start + delta * 2.0, &camera, viewport);
        assert!((drag.current_position.x - 2.0).abs() < 0.02);
        assert_eq!(drag.current_position.y, 0.0);
        assert_eq!(drag.current_position.z, 0.0);
        assert!(drag.changed());
    }
    #[test]
    fn rotate_and_scale_write_transform_values_but_not_position() {
        let actor = Actor::new(9, "Actor", Vec3::new(3.0, 2.0, 1.0));
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let camera = CameraController::new();
        let start = egui::pos2(400.0, 350.0);
        let mut rotate =
            ActorTransformPreview::new(&actor, TransformMode::Rotate, TransformAxis::Y, start);
        rotate.update(start + egui::vec2(100.0, 0.0), &camera, rect);
        assert!((rotate.current_rotation.y - 1.0).abs() < 0.01);
        assert_eq!(rotate.current_position, actor.position);
        let mut scale =
            ActorTransformPreview::new(&actor, TransformMode::Scale, TransformAxis::Center, start);
        scale.update(start + egui::vec2(100.0, 0.0), &camera, rect);
        assert!(scale.current_scale.min_element() > actor.scale.max_element());
        assert_eq!(scale.current_position, actor.position);
    }
    #[test]
    fn selected_actor_axis_handles_are_real_pointer_hit_targets() {
        let mut editor = Editor::new();
        editor.activate_transform_mode(TransformMode::Move);
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200.0, 700.0));
        let actor = Actor::new(22, "Test", Vec3::ZERO);
        let (_, lines) = handles(&editor.camera, editor.viewport_rect, actor.position).unwrap();
        let axis_x = lines
            .iter()
            .find(|line| line.axis == TransformAxis::X)
            .unwrap();
        assert_eq!(
            hit_test(&editor, &actor, axis_x.end),
            Some(TransformAxis::X)
        );
        for line in lines.iter().filter(|line| line.end != line.start) {
            assert_eq!(
                hit_test(&editor, &actor, line.end),
                Some(line.axis),
                "the displayed axis endpoint must target its own transform axis",
            );
        }
        editor.activate_transform_mode(TransformMode::Select);
        assert_eq!(hit_test(&editor, &actor, axis_x.end), None);
    }
    #[test]
    fn imported_offset_mesh_rotates_about_visible_center_on_world_axis() {
        let mut actor = Actor::new(15, "Offset", Vec3::new(3.0, 2.0, 8.0));
        actor.rotation = Vec3::new(0.3, 0.4, 0.5);
        actor.scale = Vec3::new(2.0, 1.0, 3.0);
        let camera = CameraController::new();
        let viewport = Rect::from_min_size(Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let start = egui::pos2(400.0, 350.0);
        let mut preview =
            ActorTransformPreview::new(&actor, TransformMode::Rotate, TransformAxis::X, start);
        preview.local_center = Vec3::new(4.0, 1.0, -2.0);
        let center = preview.start_visual_pivot();
        preview.update(start + egui::vec2(0.0, -100.0), &camera, viewport);
        let rotated = Quat::from_euler(
            EulerRot::XYZ,
            preview.current_rotation.x,
            preview.current_rotation.y,
            preview.current_rotation.z,
        );
        let original = Quat::from_euler(
            EulerRot::XYZ,
            actor.rotation.x,
            actor.rotation.y,
            actor.rotation.z,
        );
        let expected = Quat::from_axis_angle(Vec3::X, 1.0) * original;
        assert!(rotated.dot(expected).abs() > 0.9999);
        let moved_center = crate::editor::grid::picking::actor_visual_world_pivot(
            preview.current_position,
            preview.current_rotation,
            preview.current_scale,
            preview.local_center,
        );
        assert!(center.distance(moved_center) < 0.0005);
    }

    #[test]
    fn click_without_drag_is_not_a_world_mutation() {
        let actor = Actor::new(10, "Test", Vec3::ZERO);
        let camera = CameraController::new();
        let rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000.0, 700.0));
        let mut preview = ActorTransformPreview::new(
            &actor,
            TransformMode::Rotate,
            TransformAxis::Z,
            egui::pos2(300.0, 300.0),
        );
        preview.update(egui::pos2(301.0, 300.0), &camera, rect);
        assert!(!preview.changed());
    }
}
