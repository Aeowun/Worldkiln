use egui::RichText;
use std::cmp::Ordering;

use super::{
    App, COLOR_ACCENT_ORANGE, COLOR_ACCENT_RED, COLOR_BORDER, COLOR_BORDER_BRIGHT, COLOR_TEXT,
    COLOR_VOID, COLOR_VOID_ELEVATED, COLOR_VOID_PANEL, EditorMode, View,
};

pub(crate) fn runtime_ui_element_rect(
    viewport_rect: egui::Rect,
    layout: &crate::engine::ui::UiResolvedLayout,
) -> egui::Rect {
    runtime_ui_element_rect_scaled(viewport_rect, layout, 1.0)
}

fn runtime_ui_element_rect_scaled(
    viewport_rect: egui::Rect,
    layout: &crate::engine::ui::UiResolvedLayout,
    editor_ui_scale: f32,
) -> egui::Rect {
    let inverse_scale = 1.0 / editor_ui_scale.max(0.01);

    let position = egui::pos2(
        viewport_rect.min.x + layout.position[0] * inverse_scale,
        viewport_rect.min.y + layout.position[1] * inverse_scale,
    );

    egui::Rect::from_min_size(
        position,
        egui::vec2(
            layout.size[0] * inverse_scale,
            layout.size[1] * inverse_scale,
        ),
    )
}

impl App {
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    fn service_ai_agent(&mut self, ctx: &egui::Context) {
        let root = self.project_manager.current_project.clone();
        self.editor.reset_agent_project(&root);
        if let Some(change_id) = self.editor.ai_chat.revert_requested.take() {
            let result = root
                .as_deref()
                .ok_or_else(|| "No open project.".to_string())
                .and_then(|root| {
                    self.editor
                        .revert_agent_file(&mut self.world, root, change_id)
                })
                .and_then(|result| self.save_project_checked().map(|_| result));
            self.editor.ai_chat.record_tool_result(
                "Revert file",
                &result.unwrap_or_else(|e| format!("ERROR: {e}")),
            );
        }
        if std::mem::take(&mut self.editor.ai_chat.undo_requested) {
            let result = root
                .as_deref()
                .ok_or_else(|| "No open project.".to_string())
                .and_then(|root| self.editor.undo_agent_change(&mut self.world, root))
                .and_then(|result| self.save_project_checked().map(|_| result));
            self.editor
                .ai_chat
                .record_tool_result("Undo", &result.unwrap_or_else(|e| format!("ERROR: {e}")));
        }
        if let Some(tool) = self.editor.ai_chat.next_tool(&root) {
            let result = if self.project_load_failed {
                Err(
                    "Project load failed; agent operations are blocked until a successful reload."
                        .into(),
                )
            } else if !self.editor.ai_chat.tool_permitted(&tool) {
                Err("Tool not authorized or request cancelled.".into())
            } else {
                let settings = self.editor.ai_chat.agent_settings.clone();
                self.editor.execute_agent_tool(
                    &mut self.world,
                    &tool.project,
                    &settings,
                    true,
                    &tool.name,
                    &tool.args,
                )
            };
            let result = result
                .and_then(|(result, save)| {
                    if save {
                        self.save_project_checked()?;
                        self.editor.needs_save = false;
                    }
                    Ok(result)
                })
                .unwrap_or_else(|error| format!("ERROR: {error}"));
            self.editor.ai_chat.record_tool_result(&tool.name, &result);
            let _ = tool.reply.send(result);
            ctx.request_repaint();
        }
    }
    pub fn update_ui(&mut self, ctx: &egui::Context) {
        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
        // Service tools even while the AI dock is hidden or Settings is open.
        self.service_ai_agent(ctx);
        self.editor.collect_diagnostics();
        self.apply_editor_theme(ctx);

        let mut next_view = None;

        if self.view == View::Splash {
            self.draw_splash_screen(ctx);
        } else if self.view == View::Home {
            if self.editor.navigation.surface.is_settings() {
                // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
                self.editor.draw_settings_workspace(
                    ctx,
                    &self.renderer,
                    Some(&mut self.world),
                    &self.project_manager.current_project,
                );
            } else {
                self.draw_home_screen(ctx, &mut next_view);
            }
        } else if self.view == View::Editor {
            self.editor.show_ui(
                ctx,
                &mut self.world,
                &self.project_manager.current_project,
                &self.renderer,
            );

            // Worldkiln 0.8.7 | WO 0.8.7-51 | 2026-10-09 08:20 -05:00
            // A project texture import/refresh/rename/delete explicitly clears
            // GPU material resolutions. Normal rendering never polls disk.
            if crate::editor::tools::texture_tools::take_gpu_texture_reload_request(ctx) {
                self.renderer.invalidate_texture_cache();
            }

            if self.editor.needs_build {
                self.editor.needs_build = false;
                self.build_game();
            }

            self.draw_first_run_controls_overlay(ctx);

            // Worldkiln 0.8.7 | WO 0.8.7-46 | 2026-10-08 23:02 -05:00
            // Opt-in display: no project data or persistent settings are written.
            if let Some(report) = self.renderer.editor_profile_summary() {
                egui::Area::new("wo46_editor_profiler".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(310.0, 58.0))
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::none()
                            .fill(egui::Color32::from_rgba_unmultiplied(8, 10, 13, 220))
                            .inner_margin(egui::Margin::same(8.0))
                            .show(ui, |ui| {
                                ui.set_max_width(560.0);
                                ui.label(
                                    egui::RichText::new(report)
                                        .monospace()
                                        .size(10.0)
                                        .color(egui::Color32::LIGHT_GREEN),
                                );
                            });
                    });
            }

            if self.editor.mode == EditorMode::Editor
                && !self.editor.show_script_workspace
                && !self.editor.navigation.surface.is_settings()
                && !self.editor.ui_mode
                && self.editor.viewport_rect.is_finite()
                && self.editor.viewport_rect.is_positive()
            {
                let viewport = self.editor.viewport_rect;

                egui::Area::new("editor_navigation_help".into())
                    .order(egui::Order::Foreground)
                    .fixed_pos(egui::pos2(viewport.right() - 168.0, viewport.top() + 12.0))
                    .show(ctx, |ui| {
                        let frame_size = egui::vec2(168.0, 112.0);

                        let (frame_rect, _) =
                            ui.allocate_exact_size(frame_size, egui::Sense::hover());

                        ui.painter().rect_filled(
                            frame_rect.translate(egui::vec2(0.0, 3.0)),
                            4.0,
                            egui::Color32::from_rgba_unmultiplied(0, 0, 0, 120),
                        );

                        ui.painter().rect_filled(
                            frame_rect,
                            4.0,
                            egui::Color32::from_rgba_unmultiplied(7, 9, 12, 235),
                        );

                        ui.painter().rect_stroke(
                            frame_rect,
                            4.0,
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 28),
                            ),
                        );

                        ui.painter().line_segment(
                            [
                                frame_rect.left_top(),
                                egui::pos2(frame_rect.left() + 42.0, frame_rect.top()),
                            ],
                            egui::Stroke::new(2.0, COLOR_ACCENT_ORANGE),
                        );

                        let text_x = frame_rect.left() + 10.0;

                        let text_y = frame_rect.top() + 7.0;

                        let line_height = 23.0;

                        let font = egui::FontId::monospace(10.0);

                        ui.painter().text(
                            egui::pos2(text_x, text_y),
                            egui::Align2::LEFT_TOP,
                            "RMB + WASD   MOVE",
                            font.clone(),
                            COLOR_TEXT,
                        );

                        ui.painter().text(
                            egui::pos2(text_x, text_y + line_height),
                            egui::Align2::LEFT_TOP,
                            "RMB + Q/E    UP / DOWN",
                            font.clone(),
                            COLOR_TEXT,
                        );

                        ui.painter().text(
                            egui::pos2(text_x, text_y + line_height * 2.0),
                            egui::Align2::LEFT_TOP,
                            "MMB + MOUSE  ORBIT",
                            font.clone(),
                            COLOR_TEXT,
                        );

                        ui.painter().text(
                            egui::pos2(text_x, text_y + line_height * 3.0),
                            egui::Align2::LEFT_TOP,
                            "SCROLL       ZOOM",
                            font,
                            COLOR_TEXT,
                        );
                    });
            }

            self.render_runtime_ui(ctx);
            self.draw_play_mode_exit_hint(ctx);

            if self.show_unsaved_scripts_dialog {
                egui::Window::new("Unsaved Script Changes")
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .collapsible(false)
                    .frame(
                        egui::Frame::window(&ctx.style())
                            .fill(egui::Color32::from_rgb(7, 9, 12))
                            .stroke(egui::Stroke::new(1.0, COLOR_BORDER_BRIGHT))
                            .inner_margin(egui::Margin::symmetric(14.0, 12.0)),
                    )
                    .show(ctx, |ui| {
                        ui.label(
                            RichText::new("UNSAVED SCRIPT CHANGES")
                                .monospace()
                                .strong()
                                .color(COLOR_ACCENT_ORANGE)
                                .size(11.0),
                        );

                        ui.add_space(6.0);

                        ui.label(
                            RichText::new("You have unsaved changes in your scripts.")
                                .color(COLOR_TEXT)
                                .size(12.0),
                        );

                        ui.add_space(10.0);

                        ui.horizontal(|ui| {
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Save & Play")
                                            .monospace()
                                            .strong()
                                            .color(COLOR_VOID),
                                    )
                                    .fill(COLOR_ACCENT_ORANGE)
                                    .stroke(egui::Stroke::new(1.0, COLOR_ACCENT_ORANGE)),
                                )
                                .clicked()
                            {
                                self.editor.script_editor.save_all();

                                // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
                                self.editor.commit_pending_inspector_history();
                                self.editor.mode = EditorMode::Play;

                                self.show_unsaved_scripts_dialog = false;
                            }

                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Don't Save").monospace().color(COLOR_TEXT),
                                    )
                                    .fill(COLOR_VOID_PANEL)
                                    .stroke(egui::Stroke::new(1.0, COLOR_BORDER_BRIGHT)),
                                )
                                .clicked()
                            {
                                // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
                                self.editor.commit_pending_inspector_history();
                                self.editor.mode = EditorMode::Play;

                                self.show_unsaved_scripts_dialog = false;
                            }

                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Cancel").monospace().color(COLOR_TEXT),
                                    )
                                    .fill(COLOR_VOID_PANEL)
                                    .stroke(egui::Stroke::new(1.0, COLOR_BORDER)),
                                )
                                .clicked()
                            {
                                self.show_unsaved_scripts_dialog = false;
                            }
                        });
                    });
            }
        }

        let pending_sounds = std::mem::take(&mut self.editor.pending_engine_sounds);

        for sound in pending_sounds {
            self.play_engine_sound(sound);
        }

        if let Some(view) = next_view {
            self.view = view;
        }
    }

    fn draw_first_run_controls_overlay(&mut self, ctx: &egui::Context) {
        if self.editor.theme_settings.first_run_controls_dismissed
            || self.view != View::Editor
            || self.editor.mode != EditorMode::Editor
            || self.editor.show_script_workspace
            || self.editor.navigation.surface.is_settings()
            || self.editor.ui_mode
            || !self.editor.viewport_rect.is_finite()
            || !self.editor.viewport_rect.is_positive()
        {
            return;
        }

        let viewport = self.editor.viewport_rect;

        egui::Area::new("first_run_controls_overlay".into())
            .order(egui::Order::Foreground)
            .fixed_pos(viewport.center() - egui::vec2(220.0, 145.0))
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(7, 9, 12, 248))
                    .stroke(egui::Stroke::new(1.0, COLOR_BORDER_BRIGHT))
                    .rounding(egui::Rounding::same(8.0))
                    .inner_margin(egui::Margin::symmetric(18.0, 16.0))
                    .show(ui, |ui| {
                        ui.set_min_width(404.0);

                        ui.label(
                            RichText::new("WORLDKILN CONTROLS")
                                .monospace()
                                .strong()
                                .size(12.0)
                                .color(COLOR_ACCENT_ORANGE),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new("Move around the world before you start building.")
                                .size(13.0)
                                .color(COLOR_TEXT),
                        );
                        ui.add_space(12.0);

                        egui::Grid::new("first_run_controls_grid")
                            .num_columns(2)
                            .spacing([22.0, 8.0])
                            .show(ui, |ui| {
                                for (input, action) in [
                                    ("RMB + WASD", "Move"),
                                    ("RMB + Q / E", "Down / Up"),
                                    ("MMB + MOUSE", "Orbit"),
                                    ("SCROLL", "Zoom"),
                                    ("LEFT CLICK", "Use active tool"),
                                    ("ESC", "Clear selection / leave Play"),
                                ] {
                                    ui.label(
                                        RichText::new(input)
                                            .monospace()
                                            .strong()
                                            .color(egui::Color32::WHITE),
                                    );
                                    ui.label(RichText::new(action).color(COLOR_TEXT));
                                    ui.end_row();
                                }
                            });

                        ui.add_space(10.0);
                        ui.label(
                            RichText::new(
                                "Choose BUILD to place content, SELECT to inspect it, and PLAY to test it.",
                            )
                            .size(11.0)
                            .color(COLOR_TEXT),
                        );
                        ui.add_space(12.0);

                        if ui
                            .add_sized(
                                [112.0, 28.0],
                                egui::Button::new(
                                    RichText::new("GOT IT")
                                        .monospace()
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                )
                                .fill(COLOR_ACCENT_ORANGE),
                            )
                            .clicked()
                        {
                            self.editor.theme_settings.first_run_controls_dismissed = true;
                            crate::engine::preferences::save_editor_preferences(
                                &self.editor.theme_settings,
                            );
                            self.editor
                                .pending_engine_sounds
                                .push(crate::engine::audio::EngineSound::Click);
                        }
                    });
            });
    }

    fn draw_play_mode_exit_hint(&self, ctx: &egui::Context) {
        if self.view != View::Editor || self.editor.mode != EditorMode::Play {
            return;
        }

        let Some(started) = self.play_mode_hint_started else {
            return;
        };

        const HOLD_SECONDS: f32 = 3.0;
        const FADE_SECONDS: f32 = 1.0;

        let elapsed = started.elapsed().as_secs_f32();
        if elapsed >= HOLD_SECONDS + FADE_SECONDS {
            return;
        }

        let alpha = if elapsed <= HOLD_SECONDS {
            1.0
        } else {
            1.0 - ((elapsed - HOLD_SECONDS) / FADE_SECONDS).clamp(0.0, 1.0)
        };

        ctx.request_repaint();

        egui::Area::new("play_mode_exit_hint".into())
            .anchor(egui::Align2::RIGHT_TOP, egui::vec2(-18.0, 18.0))
            .order(egui::Order::Foreground)
            .interactable(false)
            .show(ctx, |ui| {
                egui::Frame::none()
                    .fill(egui::Color32::from_rgba_unmultiplied(
                        5,
                        7,
                        10,
                        (210.0 * alpha) as u8,
                    ))
                    .stroke(egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, (55.0 * alpha) as u8),
                    ))
                    .rounding(4.0)
                    .inner_margin(egui::Margin::symmetric(12.0, 8.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new("PRESS ESCAPE TO QUIT PLAY")
                                .monospace()
                                .strong()
                                .color(egui::Color32::from_rgba_unmultiplied(
                                    255,
                                    255,
                                    255,
                                    (255.0 * alpha) as u8,
                                )),
                        );
                    });
            });
    }

    pub(crate) fn render_runtime_ui(&mut self, ctx: &egui::Context) {
        if self.editor.mode != EditorMode::Play {
            return;
        }

        let viewport_rect = self.editor.viewport_rect;

        if viewport_rect.width() <= 1.0 || viewport_rect.height() <= 1.0 {
            return;
        }

        // The editor's Interface Scale is an editor preference, not a gameplay
        // UI transform. Runtime UI authored in project coordinates must render
        // at the same physical size in Play mode as it does in a standalone
        // game. egui's global zoom shrinks the logical viewport, so resolve the
        // runtime layout in unzoomed project-space and convert back only at the
        // final paint/input boundary.
        let editor_ui_scale = self.editor.theme_settings.interface_scale.factor();
        let inverse_editor_ui_scale = 1.0 / editor_ui_scale.max(0.01);
        let runtime_viewport_size = [
            viewport_rect.width() * editor_ui_scale,
            viewport_rect.height() * editor_ui_scale,
        ];

        let mut render_order = Vec::with_capacity(self.runtime_ui.elements.len());

        for &id in self.runtime_ui.elements.keys() {
            if let Ok(layout) = self
                .runtime_ui
                .resolve_layout_in(id as u64, runtime_viewport_size)
            {
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

            let Ok(layout) = self
                .runtime_ui
                .resolve_layout_in(id as u64, runtime_viewport_size)
            else {
                continue;
            };

            if !layout.visible {
                continue;
            }

            let position = egui::pos2(
                viewport_rect.min.x + layout.position[0] * inverse_editor_ui_scale,
                viewport_rect.min.y + layout.position[1] * inverse_editor_ui_scale,
            );

            let size = egui::vec2(
                layout.size[0] * inverse_editor_ui_scale,
                layout.size[1] * inverse_editor_ui_scale,
            );
            let element_rect =
                runtime_ui_element_rect_scaled(viewport_rect, &layout, editor_ui_scale);

            let common = element.common();

            let color = egui::Color32::from_rgba_unmultiplied(
                (common.color[0] * 255.0).clamp(0.0, 255.0) as u8,
                (common.color[1] * 255.0).clamp(0.0, 255.0) as u8,
                (common.color[2] * 255.0).clamp(0.0, 255.0) as u8,
                (common.color[3] * 255.0).clamp(0.0, 255.0) as u8,
            );

            let border_color = egui::Color32::from_rgba_unmultiplied(
                (common.border_color[0] * 255.0).clamp(0.0, 255.0) as u8,
                (common.border_color[1] * 255.0).clamp(0.0, 255.0) as u8,
                (common.border_color[2] * 255.0).clamp(0.0, 255.0) as u8,
                (common.border_color[3] * 255.0).clamp(0.0, 255.0) as u8,
            );

            let area_id = egui::Id::new("runtime_ui_element").with(id);

            // Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:27 -05:00
            if let crate::engine::ui::UiElement::Image(media)
            | crate::engine::ui::UiElement::Video(media) = &element
            {
                egui::Area::new(area_id)
                    .fixed_pos(position)
                    .order(egui::Order::Middle)
                    // WO 0.8.7-61: clicks must not promote video/image areas.
                    .interactable(false)
                    .show(ctx, |ui| {
                        ui.set_clip_rect(viewport_rect);
                        match element {
                            crate::engine::ui::UiElement::Video(_) => {
                                crate::engine::ui_media::paint_video_frame(
                                    ui.painter(),
                                    ctx,
                                    element_rect,
                                    &media.source,
                                    media.fit,
                                    media.common.color,
                                    media.playing,
                                    media.looped,
                                    media.playback_rate,
                                    self.project_manager.current_project.as_deref(),
                                )
                            }
                            _ => crate::engine::ui_media::paint_media_frame(
                                ui.painter(),
                                ctx,
                                element_rect,
                                &media.source,
                                media.fit,
                                media.common.color,
                                self.project_manager.current_project.as_deref(),
                            ),
                        }
                    });
                continue;
            }
            match element {
                crate::engine::ui::UiElement::Panel(panel) => {
                    egui::Area::new(area_id)
                        .fixed_pos(position)
                        .order(egui::Order::Middle)
                        // WO 0.8.7-61: backdrop/text cannot click-raise above movie.
                        .interactable(false)
                        .show(ctx, |ui| {
                            ui.set_clip_rect(viewport_rect);

                            let (response, _) = ui.allocate_exact_size(size, egui::Sense::hover());

                            ui.painter().rect_filled(
                                response.translate(egui::vec2(0.0, 3.0 * inverse_editor_ui_scale)),
                                panel.corner_radius * inverse_editor_ui_scale,
                                egui::Color32::from_rgba_unmultiplied(0, 0, 0, 90),
                            );

                            ui.painter().rect_filled(
                                response,
                                panel.corner_radius * inverse_editor_ui_scale,
                                color,
                            );

                            if panel.border_width > 0.0 {
                                ui.painter().rect_stroke(
                                    response,
                                    panel.corner_radius * inverse_editor_ui_scale,
                                    egui::Stroke::new(
                                        panel.border_width * inverse_editor_ui_scale,
                                        border_color,
                                    ),
                                );
                            }
                        });
                }

                crate::engine::ui::UiElement::Text(text) => {
                    egui::Area::new(area_id)
                        .fixed_pos(position)
                        .order(egui::Order::Middle)
                        // WO 0.8.7-61: backdrop/text cannot click-raise above movie.
                        .interactable(false)
                        .show(ctx, |ui| {
                            ui.set_clip_rect(viewport_rect);

                            crate::engine::ui::paint_ui_text(
                                ui.painter(),
                                ctx,
                                element_rect,
                                &text.text,
                                text.font_size * inverse_editor_ui_scale,
                                text.horizontal_align,
                                text.vertical_align,
                                text.wrap,
                                [
                                    text.padding[0] * inverse_editor_ui_scale,
                                    text.padding[1] * inverse_editor_ui_scale,
                                    text.padding[2] * inverse_editor_ui_scale,
                                    text.padding[3] * inverse_editor_ui_scale,
                                ],
                                text.line_spacing * inverse_editor_ui_scale,
                                color,
                            );
                        });
                }

                crate::engine::ui::UiElement::Button(button) => {
                    let mut clicked = false;

                    egui::Area::new(area_id)
                        .fixed_pos(position)
                        .order(egui::Order::Middle)
                        .show(ctx, |ui| {
                            ui.set_clip_rect(viewport_rect);

                            let response = ui.allocate_rect(element_rect, egui::Sense::click());

                            ui.painter().rect_filled(
                                element_rect,
                                button.common.corner_radius * inverse_editor_ui_scale,
                                egui::Color32::from_rgba_unmultiplied(8, 10, 13, 225),
                            );

                            if button.common.border_width > 0.0 {
                                ui.painter().rect_stroke(
                                    element_rect,
                                    button.common.corner_radius * inverse_editor_ui_scale,
                                    egui::Stroke::new(
                                        button.common.border_width * inverse_editor_ui_scale,
                                        border_color,
                                    ),
                                );
                            }

                            crate::engine::ui::paint_ui_text(
                                ui.painter(),
                                ctx,
                                element_rect,
                                &button.text,
                                button.font_size * inverse_editor_ui_scale,
                                button.horizontal_align,
                                button.vertical_align,
                                button.wrap,
                                [
                                    button.padding[0] * inverse_editor_ui_scale,
                                    button.padding[1] * inverse_editor_ui_scale,
                                    button.padding[2] * inverse_editor_ui_scale,
                                    button.padding[3] * inverse_editor_ui_scale,
                                ],
                                button.line_spacing * inverse_editor_ui_scale,
                                color,
                            );

                            if response.clicked() && layout.enabled {
                                clicked = true;
                            }
                        });

                    if clicked {
                        self.runtime_ui.pending_clicks.push(id);
                    }
                }
                crate::engine::ui::UiElement::Image(_) | crate::engine::ui::UiElement::Video(_) => {
                    unreachable!()
                }
            }
        }
        // WO 0.8.7-61: stop hidden movie audio/video after skip.
        #[cfg(windows)]
        crate::engine::ui_media::finish_video_ui_frame();
    }

    pub(crate) fn apply_editor_theme(&self, ctx: &egui::Context) {
        use crate::engine::preferences::*;

        let settings = &self.editor.theme_settings;

        let mut visuals = match settings.theme {
            EditorTheme::AeoDark
            | EditorTheme::Custom
            | EditorTheme::Graphite
            | EditorTheme::Midnight
            | EditorTheme::Forest => egui::Visuals::dark(),
            EditorTheme::Light | EditorTheme::Sandstone => egui::Visuals::light(),
        };

        let accent_color = settings.accent.color();

        // Selected controls use the accent as their fill, so their foreground
        // must be chosen independently. Otherwise the settings navigation can
        // become accent-on-accent and the active page label disappears.
        let accent_luma = 0.299 * accent_color.r() as f32
            + 0.587 * accent_color.g() as f32
            + 0.114 * accent_color.b() as f32;
        let selection_foreground = if accent_luma >= 150.0 {
            egui::Color32::from_rgb(12, 14, 16)
        } else {
            egui::Color32::WHITE
        };

        // Worldkiln 0.8.7 | WO 0.8.7-34 | 2026-10-07 21:57 -05:00
        let radius = settings.panel_rounding.radius();

        visuals.window_rounding = radius.into();
        visuals.widgets.noninteractive.rounding = radius.into();
        visuals.widgets.inactive.rounding = radius.into();
        visuals.widgets.hovered.rounding = radius.into();
        visuals.widgets.active.rounding = radius.into();
        visuals.widgets.open.rounding = radius.into();

        if matches!(settings.theme, EditorTheme::Light | EditorTheme::Sandstone) {
            visuals.selection.bg_fill = accent_color;
            visuals.selection.stroke = egui::Stroke::new(1.0, selection_foreground);
            visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent_color);
            visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, accent_color);
            visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent_color);
            visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, accent_color);
        } else {
            visuals.extreme_bg_color = COLOR_VOID;
            visuals.window_fill = COLOR_VOID_ELEVATED;
            visuals.panel_fill = COLOR_VOID;

            visuals.selection.bg_fill = accent_color;
            visuals.selection.stroke = egui::Stroke::new(1.0, selection_foreground);

            visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(7, 9, 12);
            visuals.widgets.noninteractive.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT);

            visuals.widgets.inactive.bg_fill = COLOR_VOID_PANEL;
            visuals.widgets.inactive.bg_stroke = egui::Stroke::new(
                1.0,
                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 18),
            );
            visuals.widgets.inactive.fg_stroke = egui::Stroke::new(1.0, COLOR_TEXT);

            visuals.widgets.hovered.bg_fill = COLOR_VOID_ELEVATED;
            visuals.widgets.hovered.bg_stroke = egui::Stroke::new(1.0, accent_color);
            visuals.widgets.hovered.fg_stroke = egui::Stroke::new(1.0, accent_color);

            visuals.widgets.active.bg_fill = COLOR_VOID_PANEL;
            visuals.widgets.active.bg_stroke = egui::Stroke::new(1.0, accent_color);
            visuals.widgets.active.fg_stroke = egui::Stroke::new(1.0, accent_color);
        }

        // Worldkiln 0.8.7 | WO 0.8.7-34 | 2026-10-07 21:57 -05:00
        // Theme palettes only change egui styling; no asset scans or new draw passes.
        let rgb = egui::Color32::from_rgb;
        let palette = match settings.theme {
            EditorTheme::Graphite => Some((
                [13, 15, 19],
                [24, 27, 32],
                [30, 34, 40],
                [18, 21, 26],
                [38, 42, 49],
                [49, 53, 61],
            )),
            EditorTheme::Midnight => Some((
                [8, 13, 25],
                [15, 23, 39],
                [24, 34, 53],
                [12, 19, 33],
                [31, 43, 62],
                [43, 57, 79],
            )),
            EditorTheme::Forest => Some((
                [10, 17, 15],
                [19, 31, 27],
                [28, 41, 35],
                [16, 26, 23],
                [35, 50, 43],
                [46, 64, 55],
            )),
            EditorTheme::Sandstone => Some((
                [220, 211, 194],
                [240, 234, 222],
                [250, 246, 236],
                [229, 221, 207],
                [230, 221, 207],
                [218, 206, 188],
            )),
            _ => None,
        };
        if let Some((extreme, panel, window, noninteractive, inactive, hovered)) = palette {
            visuals.extreme_bg_color = rgb(extreme[0], extreme[1], extreme[2]);
            visuals.panel_fill = rgb(panel[0], panel[1], panel[2]);
            visuals.window_fill = rgb(window[0], window[1], window[2]);
            visuals.widgets.noninteractive.bg_fill =
                rgb(noninteractive[0], noninteractive[1], noninteractive[2]);
            visuals.widgets.inactive.bg_fill = rgb(inactive[0], inactive[1], inactive[2]);
            visuals.widgets.hovered.bg_fill = rgb(hovered[0], hovered[1], hovered[2]);
            visuals.widgets.active.bg_fill = rgb(inactive[0], inactive[1], inactive[2]);
        }

        match settings.panel_contrast {
            PanelContrast::Low => {
                if !matches!(settings.theme, EditorTheme::Light | EditorTheme::Sandstone) {
                    visuals.panel_fill = egui::Color32::from_rgb(18, 22, 28);
                    visuals.window_fill = egui::Color32::from_rgb(22, 26, 32);
                    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                    );
                }
            }
            PanelContrast::Normal => {}
            PanelContrast::High => {
                if !matches!(settings.theme, EditorTheme::Light | EditorTheme::Sandstone) {
                    visuals.panel_fill = egui::Color32::from_rgb(4, 5, 8);
                    visuals.window_fill = egui::Color32::from_rgb(18, 22, 30);
                    visuals.widgets.inactive.bg_stroke = egui::Stroke::new(
                        1.0,
                        egui::Color32::from_rgba_unmultiplied(255, 255, 255, 35),
                    );
                }
            }
        }

        match settings.separator_visibility {
            SeparatorVisibility::Low => {
                visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(
                    0.5,
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 10),
                );
            }
            SeparatorVisibility::Normal => {}
            SeparatorVisibility::High => {
                visuals.widgets.noninteractive.bg_stroke = egui::Stroke::new(
                    1.5,
                    if matches!(settings.theme, EditorTheme::Light | EditorTheme::Sandstone) {
                        egui::Color32::from_rgb(160, 160, 160)
                    } else {
                        egui::Color32::from_rgb(80, 95, 110)
                    },
                );
            }
        }

        ctx.set_zoom_factor(settings.interface_scale.factor());

        let prop_scale = settings.font_size.scale();
        let mono_scale = settings.monospace_size.scale();

        let mut style = (*ctx.style()).clone();
        style.text_styles = [
            (
                egui::TextStyle::Heading,
                egui::FontId::new(18.0 * prop_scale, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Body,
                egui::FontId::new(14.0 * prop_scale, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Button,
                egui::FontId::new(14.0 * prop_scale, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Small,
                egui::FontId::new(10.0 * prop_scale, egui::FontFamily::Proportional),
            ),
            (
                egui::TextStyle::Monospace,
                egui::FontId::new(12.0 * mono_scale, egui::FontFamily::Monospace),
            ),
        ]
        .into();

        match settings.animation {
            UiAnimation::Off => {
                style.animation_time = 0.0;
            }
            UiAnimation::Reduced => {
                style.animation_time = 0.08;
            }
            UiAnimation::Full => {
                style.animation_time = 0.2;
            }
        }

        if !settings.tooltips {
            style.interaction.show_tooltips_only_when_still = false;
        }

        style.visuals = visuals;
        ctx.set_style(style);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ui::{RuntimeUi, UiAnchor, UiResolvedLayout};

    #[test]
    fn test_runtime_ui_element_rect_calculation() {
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1280.0, 720.0));

        let layout = UiResolvedLayout {
            position: [500.0, 13.0],
            size: [380.0, 35.0],
            visible: true,
            enabled: true,
            z_index: 0.0,
        };

        let element_rect = runtime_ui_element_rect(viewport, &layout);

        assert_eq!(element_rect.min, egui::pos2(600.0, 63.0));
        assert_eq!(element_rect.size(), egui::vec2(380.0, 35.0));
    }

    #[test]
    fn runtime_ui_rect_compensates_for_editor_interface_scale() {
        let viewport =
            egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1024.0, 576.0));

        // At 125% editor zoom, this is the authored/project-space layout that
        // corresponds to a 1280x720 runtime viewport.
        let layout = UiResolvedLayout {
            position: [500.0, 125.0],
            size: [250.0, 100.0],
            visible: true,
            enabled: true,
            z_index: 0.0,
        };

        let rect = runtime_ui_element_rect_scaled(viewport, &layout, 1.25);

        assert_eq!(rect.min, egui::pos2(500.0, 150.0));
        assert_eq!(rect.size(), egui::vec2(200.0, 80.0));
    }

    #[test]
    fn test_top_right_anchored_text_layout_bounds() {
        let mut ui = RuntimeUi::new();
        let text_id = ui.allocate("Text").unwrap();

        let elem = ui.elements.get_mut(&(text_id as u32)).unwrap();
        elem.common_mut().anchor = UiAnchor::TopRight;
        elem.common_mut().position = [-400.0, 13.0];
        elem.common_mut().size = [380.0, 35.0];

        let viewport_size = [1280.0, 720.0];
        let layout = ui.resolve_layout_in(text_id, viewport_size).unwrap();

        let viewport_rect =
            egui::Rect::from_min_size(egui::pos2(100.0, 50.0), egui::vec2(1280.0, 720.0));

        let element_rect = runtime_ui_element_rect(viewport_rect, &layout);

        assert_eq!(element_rect.min, egui::pos2(600.0, 63.0));
        assert_eq!(element_rect.size(), egui::vec2(380.0, 35.0));
    }
}
