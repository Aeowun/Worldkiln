// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::runtime::GameRuntime;

impl GameRuntime {
    pub fn render(&self) {
        self.renderer.render_game(
            &self.world,
            &self.physics_world,
            &self.entity_manager,
            &self.character_system,
            &self.gameplay_camera,
            &self.actor_system,
        );
    }

    pub fn render_runtime_ui(&mut self, ctx: &egui::Context) {
        let width = self.renderer.width();
        let height = self.renderer.height();
        if width <= 1.0 || height <= 1.0 {
            return;
        }
        let rect = egui::Rect::from_min_size(
            egui::Pos2::ZERO,
            egui::vec2(
                width / ctx.pixels_per_point(),
                height / ctx.pixels_per_point(),
            ),
        );
        crate::engine::ui_compositor::paint(
            ctx,
            &mut self.runtime_ui,
            rect,
            1.0,
            self.project_path.as_deref(),
        );
    }
}
