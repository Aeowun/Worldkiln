// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{
    Editor, EditorTool,
    transform_gizmo::{self, TransformMode},
};
use crate::world::{World, WorldCoord};
use egui::{Pos2, Rect};
use glam::Vec3;

const DRAG_THRESHOLD_SQ: f32 = 25.0;

#[derive(Clone, Copy, Debug)]
pub struct BoxSelection {
    pub start: Pos2,
    pub end: Pos2,
}

impl BoxSelection {
    pub fn new(start: Pos2) -> Self {
        Self { start, end: start }
    }

    pub fn is_drag(&self) -> bool {
        self.start.distance_sq(self.end) >= DRAG_THRESHOLD_SQ
    }

    pub fn rectangle(&self, viewport: Rect) -> Rect {
        Rect::from_two_pos(self.start, self.end).intersect(viewport)
    }
}

pub(crate) fn permitted(editor: &Editor) -> bool {
    editor.mode == crate::engine::EditorMode::Editor
        && editor.current_tool == EditorTool::Select
        && editor.transform_mode == TransformMode::Select
        && !editor.ui_mode
        && !editor.show_script_workspace
        && !editor.effect_visible
        && !editor.navigation.surface.is_settings()
        && !editor.asset_manager.open
        && editor.sculpt_tool == crate::world::terrain::SculptTool::None
        && !editor.actor_brush_enabled
        && editor.viewport_rect.is_positive()
}

fn projected_bounds(editor: &Editor, min: Vec3, max: Vec3) -> Option<Rect> {
    let mut projected = Rect::NOTHING;
    let mut visible = false;
    for x in [min.x, max.x] {
        for y in [min.y, max.y] {
            for z in [min.z, max.z] {
                if let Some(pos) = transform_gizmo::project(
                    &editor.camera,
                    editor.viewport_rect,
                    Vec3::new(x, y, z),
                ) {
                    projected.extend_with(pos);
                    visible = true;
                }
            }
        }
    }
    visible.then_some(projected)
}

#[derive(Default, Debug, PartialEq, Eq)]
pub struct BoxSelectionHits {
    pub actors: Vec<u64>,
    pub cells: Vec<WorldCoord>,
}

pub(crate) fn gather_hits(editor: &Editor, world: &World, rectangle: Rect) -> BoxSelectionHits {
    let mut results = BoxSelectionHits::default();
    if !rectangle.is_positive() || !editor.viewport_rect.is_positive() {
        return results;
    }
    for (coord, cell) in world.iter_authored_cells() {
        if !cell.visible {
            continue;
        }
        let min = Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32);
        if projected_bounds(editor, min, min + Vec3::ONE)
            .is_some_and(|bounds| bounds.intersects(rectangle))
        {
            results.cells.push(coord);
        }
    }
    for actor in world.actors.values() {
        if !actor.visible || !actor.enabled {
            continue;
        }
        let (min, max) = crate::editor::grid::picking::actor_bounds(actor);
        if projected_bounds(editor, min, max).is_some_and(|bounds| bounds.intersects(rectangle)) {
            results.actors.push(actor.id);
        }
    }
    results.cells.sort_by_key(|c| (c.x, c.y, c.z));
    results.actors.sort_unstable();
    results
}

impl Editor {
    pub(crate) fn draw_box_selection_overlay(&self, ctx: &egui::Context) {
        let Some(gesture) = self.box_selection else {
            return;
        };
        if !permitted(self) || !gesture.is_drag() {
            return;
        }
        let rectangle = gesture.rectangle(self.viewport_rect);
        if !rectangle.is_positive() {
            return;
        }
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("viewport_box_selection"),
            ))
            .with_clip_rect(self.viewport_rect);
        let accent = ctx.style().visuals.selection.stroke.color;
        painter.rect_filled(rectangle, 0.0, accent.gamma_multiply(0.14));
        painter.rect_stroke(rectangle, 0.0, egui::Stroke::new(1.5, accent));
    }

    pub(crate) fn finish_box_selection(&mut self, world: &World, end: Pos2) -> bool {
        let Some(mut gesture) = self.box_selection.take() else {
            return false;
        };
        gesture.end = end;
        if !gesture.is_drag() || !permitted(self) {
            return false;
        }
        let rectangle = gesture.rectangle(self.viewport_rect);
        let hits = gather_hits(self, world, rectangle);
        self.clear_selection();
        self.selected_project_nodes.clear();
        self.selected_project_node_id = None;
        self.selected_coords = hits.cells;
        self.selected_coord = self.selected_coords.last().copied();
        self.selected_actor_id = hits.actors.last().copied();
        for id in hits.actors {
            if let Some(node_id) = self.project_hierarchy.find_actor_node(id) {
                self.selected_project_nodes.push(node_id);
            }
        }
        if self.selected_actor_id.is_some() {
            self.selected_project_node_id = self.selected_project_nodes.last().copied();
        } else if let Some(coord) = self.selected_coord {
            if let Some(cell) = world.get(coord) {
                self.selected_project_node_id = self.project_hierarchy.find_cell_node(cell.id);
            }
        }
        if self.selected_coord.is_some() || self.selected_actor_id.is_some() {
            self.ensure_scene_home_workspace();
            self.reveal_selected_properties();
        }
        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{Actor, CellType};
    #[test]
    fn selection_threshold_and_clamping() {
        let viewport = Rect::from_min_max(egui::pos2(20., 30.), egui::pos2(420., 330.));
        let mut selection = BoxSelection::new(egui::pos2(100., 100.));
        selection.end = egui::pos2(103., 103.);
        assert!(!selection.is_drag());
        selection.end = egui::pos2(500., -50.);
        assert!(selection.is_drag());
        assert_eq!(selection.rectangle(viewport).min, egui::pos2(100., 30.));
        assert_eq!(selection.rectangle(viewport).max, egui::pos2(420., 100.));
    }

    #[test]
    fn select_mixed_authored_cells_and_actors_only_on_release() {
        let mut editor = Editor::new();
        let mut world = World::new();
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200., 700.));
        editor.camera.target = Vec3::new(0.5, 0.5, 0.5);
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        world.set_cell(WorldCoord::new(0, 0, 1), CellType::Block);
        let actor_id = world.add_actor("Test1", Vec3::new(2.5, 0.5, 0.5));
        let second_actor_id = world.add_actor("Test2", Vec3::new(2.5, 0.5, 0.5));
        editor
            .project_hierarchy
            .create_actor_node(None, actor_id)
            .unwrap();
        editor
            .project_hierarchy
            .create_actor_node(None, second_actor_id)
            .unwrap();
        let rect = editor.viewport_rect;
        let all = gather_hits(&editor, &world, rect);
        assert!(all.cells.contains(&WorldCoord::new(0, 0, 0)));
        assert!(all.cells.contains(&WorldCoord::new(0, 0, 1)));
        assert!(all.actors.contains(&actor_id));
        assert!(all.actors.contains(&second_actor_id));
        editor.box_selection = Some(BoxSelection {
            start: rect.left_top(),
            end: rect.right_bottom(),
        });
        let initial_world = world.clone();
        assert!(editor.finish_box_selection(&world, rect.right_bottom()));
        assert!(editor.selected_coords.contains(&WorldCoord::new(0, 0, 0)));
        assert!(editor.selected_coords.contains(&WorldCoord::new(0, 0, 1)));
        assert_eq!(editor.selected_actor_ids(), vec![actor_id, second_actor_id]);
        assert_eq!(editor.selected_project_nodes.len(), 2);
        assert_eq!(editor.needs_save, false);
        assert_eq!(world.actors, initial_world.actors);
    }

    #[test]
    fn excludes_invisible_and_behind_camera_actors() {
        let mut editor = Editor::new();
        let mut world = World::new();
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1200., 700.));
        editor.camera.target = Vec3::ZERO;
        let hidden_id = world.add_actor("Hidden", Vec3::ZERO);
        world.actors.get_mut(&hidden_id).unwrap().visible = false;
        let eye = editor.camera.get_position();
        let behind = eye + (eye - editor.camera.target).normalize() * 50.0;
        let behind_id = world.add_actor("BehindCamera", behind);
        let hits = gather_hits(&editor, &world, editor.viewport_rect);
        assert!(!hits.actors.contains(&hidden_id));
        assert!(!hits.actors.contains(&behind_id));
    }

    #[test]
    fn click_without_drag_keeps_selection_unchanged() {
        let mut editor = Editor::new();
        let world = World::new();
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1000., 700.));
        let start = egui::pos2(50., 50.);
        editor.box_selection = Some(BoxSelection { start, end: start });
        assert!(!editor.finish_box_selection(&world, egui::pos2(51., 51.)));
        assert!(editor.box_selection.is_none());
    }

    #[test]
    fn boxes_disabled_while_transforming() {
        let mut editor = Editor::new();
        editor.viewport_rect = Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800., 600.));
        assert!(permitted(&editor));
        editor.transform_mode = TransformMode::Move;
        assert!(!permitted(&editor));
        editor.transform_mode = TransformMode::Rotate;
        assert!(!permitted(&editor));
        editor.transform_mode = TransformMode::Scale;
        assert!(!permitted(&editor));
        editor.transform_mode = TransformMode::Select;
        editor.current_tool = EditorTool::Build;
        assert!(!permitted(&editor));
    }
}
