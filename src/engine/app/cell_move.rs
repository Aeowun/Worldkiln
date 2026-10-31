// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::*;
use crate::editor::editor::transform_gizmo::CellMovePreview;

impl App {
    pub(super) fn begin_cell_move_preview(
        &mut self,
        axis: TransformAxis,
        pointer: egui::Pos2,
    ) -> bool {
        if self.editor.transform_mode != TransformMode::Move || axis == TransformAxis::Center {
            return false;
        }
        let coords = self.editor.selected_cell_move_coords(&self.world);
        let Some(pivot) = self.editor.cell_move_pivot(&self.world) else {
            return false;
        };
        let anchor = self
            .editor
            .selected_coord
            .filter(|c| coords.contains(c))
            .unwrap_or(coords[0]);
        self.editor.cell_move_preview = Some(CellMovePreview::new(pivot, axis, pointer));
        self.editor.grab_state = Some(crate::editor::GrabState {
            source_coords: coords,
            anchor_coord: anchor,
            current_target: anchor,
            delta: WorldCoord::new(0, 0, 0),
            valid: true,
        });
        self.pending_grab = None;
        self.pending_actor_grab = None;
        self.drag_start_coord = None;
        self.editor.clear_hover();
        true
    }

    pub(super) fn update_cell_move_preview(&mut self, pointer: egui::Pos2) {
        let Some(preview) = self.editor.cell_move_preview.as_ref() else {
            return;
        };
        let Some(delta) = preview.delta(pointer, &self.editor.camera, self.editor.viewport_rect)
        else {
            return;
        };
        let Some(grab) = self.editor.grab_state.as_ref() else {
            return;
        };
        let representable = grab.source_coords.iter().all(|c| {
            c.x.checked_add(delta.x).is_some()
                && c.y.checked_add(delta.y).is_some()
                && c.z.checked_add(delta.z).is_some()
        });
        if !representable {
            return;
        }
        let valid = grab
            .source_coords
            .iter()
            .all(|c| self.world.get(*c).is_some())
            && self.validate_grab_destination(&grab.source_coords, delta);
        let grab = self.editor.grab_state.as_mut().unwrap();
        grab.delta = delta;
        grab.current_target = WorldCoord::new(
            grab.anchor_coord.x + delta.x,
            grab.anchor_coord.y + delta.y,
            grab.anchor_coord.z + delta.z,
        );
        grab.valid = valid;
    }

    pub(super) fn commit_cell_move_preview(&mut self) -> bool {
        if self.editor.cell_move_preview.take().is_none() {
            return false;
        }
        let Some(grab) = self.editor.grab_state.take() else {
            return false;
        };
        if !grab.valid
            || grab.delta == WorldCoord::new(0, 0, 0)
            || grab
                .source_coords
                .iter()
                .any(|c| self.world.get(*c).is_none())
            || !self.validate_grab_destination(&grab.source_coords, grab.delta)
        {
            return false;
        }
        let snapshot = self.editor.create_snapshot_without_terrain(&self.world);
        if self
            .world
            .move_cells(&grab.source_coords, grab.delta)
            .is_err()
        {
            return false;
        }
        self.editor.history.push_undo_entry(snapshot);
        self.editor.history.redo_stack.clear();
        self.editor.selected_coords = grab
            .source_coords
            .iter()
            .map(|c| WorldCoord::new(c.x + grab.delta.x, c.y + grab.delta.y, c.z + grab.delta.z))
            .collect();
        self.editor.selected_coord = Some(grab.current_target);
        self.editor.needs_save = true;
        true
    }
}
