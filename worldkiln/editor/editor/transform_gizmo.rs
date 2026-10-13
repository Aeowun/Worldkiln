// Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
// Editor-only viewport transform handles. The Renderer uses preview values,
// while app_input owns commit/undo. No mutation of authored Worlds on mouse move.
use crate::renderer::camera::CameraController;
use crate::world::{Actor, World};
use egui::{Color32, Context, Pos2, Rect, Stroke};
use glam::{Vec2, Vec3, Vec4};

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
            current_position: actor.position,
            current_rotation: actor.rotation,
            current_scale: actor.scale,
        }
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
        // No change until a meaningful pointer gesture (click selects only).
        if delta.length_squared() < 9.0 {
            self.current_position = self.start_position;
            self.current_rotation = self.start_rotation;
            self.current_scale = self.start_scale;
            return;
        }
        match self.mode {
            TransformMode::Move => {
                if self.axis == TransformAxis::Center {
                    // The Center handle is for Rotate/Scale. Surface-based
                    // free Move stays with Worldkiln's existing Actor grab path.
                    return;
                }
                let Some(origin) = project(camera, viewport, self.start_position) else {
                    return;
                };
                let Some(end) = project(camera, viewport, self.start_position + self.axis.vector())
                else {
                    return;
                };
                let screen_axis = Vec2::new(end.x - origin.x, end.y - origin.y);
                let sq = screen_axis.length_squared();
                if sq < 9.0 {
                    return;
                }
                let units = (delta.dot(screen_axis) / sq).clamp(-1000.0, 1000.0);
                self.current_position = self.start_position + self.axis.vector() * units;
            }
            TransformMode::Rotate => {
                // Mouse travel around the selected axis rotates the actual
                // Euler component, not the camera or a cosmetic gizmo.
                let radians = match self.axis {
                    TransformAxis::X => -delta.y * 0.01,
                    TransformAxis::Y => delta.x * 0.01,
                    TransformAxis::Z => (delta.x + delta.y) * 0.007,
                    TransformAxis::Center => delta.x * 0.01,
                }
                .clamp(-std::f32::consts::TAU * 16.0, std::f32::consts::TAU * 16.0);
                let v = if self.axis == TransformAxis::Center {
                    Vec3::Y
                } else {
                    self.axis.vector()
                };
                self.current_rotation = self.start_rotation + v * radians;
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
            }
            TransformMode::Select => {}
        }
    }
}
fn project(camera: &CameraController, viewport: Rect, position: Vec3) -> Option<Pos2> {
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
    if editor.transform_mode == TransformMode::Select
        || editor.current_tool != EditorTool::Select
        || editor.ui_mode
        || editor.show_script_workspace
        || !editor.viewport_rect.contains(pointer)
    {
        return None;
    }
    let (center, lines) = handles(&editor.camera, editor.viewport_rect, actor.position)?;
    if editor.transform_mode == TransformMode::Scale && center.distance(pointer) < 13.0 {
        return Some(TransformAxis::Center);
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
    pub fn activate_transform_mode(&mut self, mode: TransformMode) {
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
        let Some(actor) = self.selected_actor_id.and_then(|id| world.actors.get(&id)) else {
            return;
        };
        let position = self
            .actor_transform_preview
            .as_ref()
            .filter(|preview| preview.actor_id == actor.id)
            .map(|preview| preview.current_position)
            .unwrap_or(actor.position);
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
        // Axis names rather than hardcoded RGB: derived from user's ThemeSettings.
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
        if let Some(preview) = self.actor_transform_preview.as_ref() {
            let action = match preview.mode {
                TransformMode::Move => "Move",
                TransformMode::Rotate => "Rotate",
                TransformMode::Scale => "Scale",
                TransformMode::Select => "Select",
            };
            painter.text(
                center + egui::vec2(0.0, 64.0),
                egui::Align2::CENTER_TOP,
                format!("{action} {}", preview.axis.label()),
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
        editor.activate_transform_mode(TransformMode::Select);
        assert_eq!(hit_test(&editor, &actor, axis_x.end), None);
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
