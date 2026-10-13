use std::cmp::Ordering;

use super::runtime::GameRuntime;

impl GameRuntime {
    pub fn render(&self) {
        self.renderer.render_game(
            &self.world,
            &self.physics_world,
            &self.entity_manager,
            &self.character_system,
            &self.gameplay_camera,
        );
    }

    pub fn render_runtime_ui(&mut self, ctx: &egui::Context) {
        let width = self.renderer.width();
        let height = self.renderer.height();

        if width <= 1.0 || height <= 1.0 {
            return;
        }

        let mut render_order = Vec::with_capacity(self.runtime_ui.elements.len());

        for &id in self.runtime_ui.elements.keys() {
            if let Ok(layout) = self.runtime_ui.resolve_layout_in(
                id as u64,
                [
                    width / ctx.pixels_per_point(),
                    height / ctx.pixels_per_point(),
                ],
            ) {
                render_order.push((id, layout.z_index));
            }
        }

        render_order.sort_by(|(left_id, left_z), (right_id, right_z)| {
            left_z
                .partial_cmp(right_z)
                .unwrap_or(Ordering::Equal)
                .then_with(|| left_id.cmp(right_id))
        });

        for (id, _) in render_order {
            let Some(element) = self.runtime_ui.elements.get(&id).cloned() else {
                continue;
            };

            let Ok(layout) = self.runtime_ui.resolve_layout_in(
                id as u64,
                [
                    width / ctx.pixels_per_point(),
                    height / ctx.pixels_per_point(),
                ],
            ) else {
                continue;
            };

            if !layout.visible {
                continue;
            }

            let position = egui::pos2(layout.position[0], layout.position[1]);
            let size = egui::vec2(layout.size[0], layout.size[1]);
            let elem_color = element.common().color;

            let color = egui::Color32::from_rgba_unmultiplied(
                (elem_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                (elem_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                (elem_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                (elem_color[3] * 255.0).clamp(0.0, 255.0) as u8,
            );

            // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:27 -05:00
            if let crate::engine::ui::UiElement::Image(media)
            | crate::engine::ui::UiElement::Video(media) = &element
            {
                egui::Area::new(format!("runtime_media_{id}").into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(position)
                    // WO 0.8.7-61: media surfaces are display-only.
                    .interactable(false)
                    .show(ctx, |ui| {
                        ui.set_clip_rect(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        ));
                        let rect = egui::Rect::from_min_size(position, size);
                        match element {
                            crate::engine::ui::UiElement::Video(_) => {
                                crate::engine::ui_media::paint_video_frame(
                                    ui.painter(),
                                    ctx,
                                    rect,
                                    &media.source,
                                    media.fit,
                                    media.common.color,
                                    media.playing,
                                    media.looped,
                                    media.playback_rate,
                                    self.project_path.as_deref(),
                                )
                            }
                            _ => crate::engine::ui_media::paint_media_frame(
                                ui.painter(),
                                ctx,
                                rect,
                                &media.source,
                                media.fit,
                                media.common.color,
                                self.project_path.as_deref(),
                            ),
                        }
                    });
                continue;
            }
            match element {
                crate::engine::ui::UiElement::Panel(panel) => {
                    egui::Area::new(format!("runtime_panel_{id}").into())
                        .order(egui::Order::Foreground)
                        .fixed_pos(position)
                        // WO 0.8.7-61: display layers must not click-raise.
                        .interactable(false)
                        .show(ctx, |ui| {
                            let rect = egui::Rect::from_min_size(position, size);

                            ui.set_clip_rect(egui::Rect::from_min_size(
                                egui::Pos2::new(0.0, 0.0),
                                egui::vec2(width, height),
                            ));

                            ui.painter().rect_filled(rect, panel.corner_radius, color);

                            if panel.border_width > 0.0 {
                                let border_color = egui::Color32::from_rgba_unmultiplied(
                                    (panel.border_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                                    (panel.border_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                                    (panel.border_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                                    (panel.border_color[3] * 255.0).clamp(0.0, 255.0) as u8,
                                );

                                ui.painter().rect_stroke(
                                    rect,
                                    panel.corner_radius,
                                    egui::Stroke::new(panel.border_width, border_color),
                                );
                            }
                        });
                }

                crate::engine::ui::UiElement::Text(text) => {
                    egui::Area::new(format!("runtime_text_{id}").into())
                        .order(egui::Order::Foreground)
                        .fixed_pos(position)
                        // WO 0.8.7-61: display layers must not click-raise.
                        .interactable(false)
                        .show(ctx, |ui| {
                            ui.set_clip_rect(egui::Rect::from_min_size(
                                egui::Pos2::new(0.0, 0.0),
                                egui::vec2(width, height),
                            ));

                            let rect = egui::Rect::from_min_size(position, size);

                            crate::engine::ui::paint_ui_text(
                                ui.painter(),
                                ctx,
                                rect,
                                &text.text,
                                text.font_size,
                                text.horizontal_align,
                                text.vertical_align,
                                text.wrap,
                                text.padding,
                                text.line_spacing,
                                color,
                            );
                        });
                }

                crate::engine::ui::UiElement::Button(button) => {
                    let id_val = id as u64;

                    egui::Area::new(format!("runtime_button_{id}").into())
                        .order(egui::Order::Foreground)
                        .fixed_pos(position)
                        .show(ctx, |ui| {
                            ui.set_clip_rect(egui::Rect::from_min_size(
                                egui::Pos2::new(0.0, 0.0),
                                egui::vec2(width, height),
                            ));

                            let rect = egui::Rect::from_min_size(position, size);
                            let response = ui.allocate_rect(rect, egui::Sense::click());
                            let is_hovered = response.hovered();

                            let display_color = if is_hovered {
                                egui::Color32::from_rgba_unmultiplied(
                                    (elem_color[0] * 280.0).clamp(0.0, 255.0) as u8,
                                    (elem_color[1] * 280.0).clamp(0.0, 255.0) as u8,
                                    (elem_color[2] * 280.0).clamp(0.0, 255.0) as u8,
                                    (elem_color[3] * 255.0).clamp(0.0, 255.0) as u8,
                                )
                            } else {
                                color
                            };

                            ui.painter().rect_filled(
                                rect,
                                button.common.corner_radius,
                                display_color,
                            );

                            if button.common.border_width > 0.0 {
                                let border_color = egui::Color32::from_rgba_unmultiplied(
                                    (button.common.border_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                                    (button.common.border_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                                    (button.common.border_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                                    (button.common.border_color[3] * 255.0).clamp(0.0, 255.0) as u8,
                                );

                                ui.painter().rect_stroke(
                                    rect,
                                    button.common.corner_radius,
                                    egui::Stroke::new(button.common.border_width, border_color),
                                );
                            }

                            crate::engine::ui::paint_ui_text(
                                ui.painter(),
                                ctx,
                                rect,
                                &button.text,
                                button.font_size,
                                button.horizontal_align,
                                button.vertical_align,
                                button.wrap,
                                button.padding,
                                button.line_spacing,
                                egui::Color32::WHITE,
                            );

                            if response.clicked() && layout.enabled {
                                self.runtime_ui.pending_clicks.push(id);
                                let _ = id_val;
                            }
                        });
                }
                crate::engine::ui::UiElement::Image(_) | crate::engine::ui::UiElement::Video(_) => {
                    unreachable!()
                }
            }
        }
        // WO 0.8.7-61: skip removes any playing soundtrack.
        #[cfg(windows)]
        crate::engine::ui_media::finish_video_ui_frame();
    }
}
