// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
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
    fn reserve_application_shell(&self, ctx: &egui::Context) {
        let margin = 4.0 / ctx.zoom_factor();
        let frame = egui::Frame::none().fill(ctx.style().visuals.panel_fill);
        egui::TopBottomPanel::top("shell_top_edge")
            .exact_height(margin)
            .frame(frame)
            .show(ctx, |_| {});
        egui::TopBottomPanel::bottom("shell_bottom_edge")
            .exact_height(margin)
            .frame(frame)
            .show(ctx, |_| {});
        egui::SidePanel::left("shell_left_edge")
            .exact_width(margin)
            .resizable(false)
            .frame(frame)
            .show(ctx, |_| {});
        egui::SidePanel::right("shell_right_edge")
            .exact_width(margin)
            .resizable(false)
            .frame(frame)
            .show(ctx, |_| {});
    }

    pub fn draw_application_border(&self, ctx: &egui::Context, maximized: bool) {
        let zoom = ctx.zoom_factor();
        let width = 2.0 / zoom;
        let radius = if maximized { 0.0 } else { 14.0 / zoom };
        let rect = ctx.screen_rect().shrink(width * 0.5);
        ctx.layer_painter(egui::LayerId::new(
            egui::Order::Tooltip,
            egui::Id::new("worldkiln_application_border"),
        ))
        .rect_stroke(
            rect,
            radius,
            egui::Stroke::new(width, self.editor.theme_settings.accent.color()),
        );
    }

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
            let result =
                if let Some(result) = self.editor.ai_chat.undo_generated_asset(root.as_deref()) {
                    result
                } else {
                    root.as_deref()
                        .ok_or_else(|| "No open project.".to_string())
                        .and_then(|root| self.editor.undo_agent_change(&mut self.world, root))
                        .and_then(|result| self.save_project_checked().map(|_| result))
                };
            self.editor
                .ai_chat
                .record_tool_result("Undo", &result.unwrap_or_else(|e| format!("ERROR: {e}")));
        }
        if let Some(tool) = self.editor.ai_chat.next_tool(&root) {
            if !self.project_load_failed
                && self.editor.mode == EditorMode::Editor
                && self.editor.ai_chat.tool_permitted(&tool)
                && crate::editor::editor::ai_generation::is_generation_tool(&tool.name)
            {
                self.editor.ai_chat.start_generation(ctx.clone(), tool);
                return;
            }
            let result = if self.project_load_failed {
                Err(
                    "Project load failed; agent operations are blocked until a successful reload."
                        .into(),
                )
            } else if !self.editor.ai_chat.tool_permitted(&tool) {
                Err("Tool not authorized or request cancelled.".into())
            } else if tool.name == "read_runtime_state" {
                Ok((serde_json::json!({"editor_mode":format!("{:?}",self.editor.mode),
                    "last_processed_mode":format!("{:?}",self.last_mode),
                    "script_runtime_available":self.script_scene.is_some(),
                    "player_position":self.character_system.get_active_player().map(|player|player.transform.position.to_array()),
                    "unsaved_scripts_prompt":self.show_unsaved_scripts_dialog,
                    "runtime_test_results":self.script_test_results,
                    "evidence":"Observed app state only; no gameplay interaction is implied."}).to_string(),false))
            } else if tool.name == "undo_agent_change" && self.editor.ai_chat.has_generated_undo() {
                self.editor
                    .ai_chat
                    .undo_generated_asset(root.as_deref())
                    .unwrap()
                    .map(|result| (result, false))
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
            self.editor
                .ai_chat
                .record_tool_result_with_args(&tool.name, &tool.args, &result);
            if !result.starts_with("ERROR:")
                && matches!(
                    tool.name.as_str(),
                    "import_project_assets"
                        | "create_character_asset"
                        | "create_texture_from_image"
                        | "create_material_from_textures"
                )
            {
                if let Some(root) = root.as_deref() {
                    crate::editor::tools::refresh_asset_catalogs(ctx, root);
                }
            }
            let _ = tool.reply.send(result);
            ctx.request_repaint();
        }
    }
    fn draw_packaging_workspace(&mut self, ctx: &egui::Context) {
        use crate::engine::build::{GameBuildConfiguration, GameBuildFormat};
        use crate::engine::build_platform::TargetPlatform;
        let running = self.game_build_job.is_some();
        let previous_options = self.packaging_options.clone();
        let mut start = false;
        let mut run_after = false;
        egui::SidePanel::left("worldkiln_packaging_workspace")
            .default_width(365.0)
            .min_width(320.0)
            .resizable(true)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("PACKAGING");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("CLOSE").clicked() {
                            self.packaging_open = false;
                        }
                    });
                });
                egui::ScrollArea::vertical()
                    .id_salt("worldkiln_packaging_options_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                ui.label("WORLDKILN / BUILD GAME");
                ui.separator();
                ui.label("TARGET PLATFORM");
                ui.add_enabled_ui(!running, |ui| {
                    for target in [
                        TargetPlatform::Windows,
                        TargetPlatform::Linux,
                        TargetPlatform::Macos,
                        TargetPlatform::Android,
                        TargetPlatform::Ios,
                        TargetPlatform::Xbox,
                    ] {
                        let descriptor = target.capabilities();
                        ui.add_enabled_ui(descriptor.native_adapter, |ui| {
                            ui.radio_value(
                                &mut self.packaging_options.target,
                                target,
                                descriptor.name,
                            );
                        });
                    }
                });
                ui.label("CONFIGURATION");
                ui.add_enabled_ui(!running, |ui| {
                    ui.radio_value(
                        &mut self.packaging_options.configuration,
                        GameBuildConfiguration::Development,
                        "Development",
                    );
                    ui.radio_value(
                        &mut self.packaging_options.configuration,
                        GameBuildConfiguration::Shipping,
                        "Shipping",
                    );
                });
                ui.separator();
                ui.label("DISTRIBUTION FORMAT");
                ui.add_enabled_ui(!running, |ui| {
                    ui.radio_value(
                        &mut self.packaging_options.format,
                        GameBuildFormat::PlayableFolder,
                        "Playable Folder",
                    );
                    ui.radio_value(
                        &mut self.packaging_options.format,
                        GameBuildFormat::ZipArchive,
                        "ZIP",
                    );
                });
                ui.separator();
                ui.label("OUTPUT DIRECTORY");
                ui.label(
                    self.packaging_options
                        .output_parent
                        .as_ref()
                        .map(|p| p.display().to_string())
                        .unwrap_or_else(|| "Default Builds folder".into()),
                );
                if ui
                    .add_enabled(!running, egui::Button::new("Browse..."))
                    .clicked()
                {
                    if let Some(folder) = rfd::FileDialog::new().pick_folder() {
                        self.packaging_options.output_parent = Some(folder);
                    }
                }
                ui.separator();
                ui.label("GAME SETTINGS");
                ui.add_enabled_ui(!running, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("GAME TITLE");
                        ui.text_edit_singleline(&mut self.packaging_options.title);
                    });
                    ui.horizontal(|ui| {
                        ui.label("VERSION");
                        ui.text_edit_singleline(&mut self.packaging_options.game_version);
                    });
                    ui.horizontal(|ui| {
                        ui.label("EXECUTABLE NAME");
                        ui.text_edit_singleline(&mut self.packaging_options.executable_name);
                    });
                    ui.label("WINDOW MODE");
                    ui.radio_value(
                        &mut self.packaging_options.window_mode,
                        "windowed".into(),
                        "Windowed",
                    );
                    ui.radio_value(
                        &mut self.packaging_options.window_mode,
                        "borderless_fullscreen".into(),
                        "Borderless Fullscreen",
                    );
                    ui.radio_value(
                        &mut self.packaging_options.window_mode,
                        "fullscreen".into(),
                        "Fullscreen",
                    );
                    ui.label("STARTUP RESOLUTION");
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::DragValue::new(&mut self.packaging_options.resolution[0])
                                .clamp_range(320..=16384),
                        );
                        ui.label("x");
                        ui.add(
                            egui::DragValue::new(&mut self.packaging_options.resolution[1])
                                .clamp_range(320..=16384),
                        );
                    });
                });
                ui.label("GAME ICON");
                ui.horizontal(|ui| {
                    ui.label(
                        self.packaging_options
                            .icon
                            .as_ref()
                            .and_then(|p| p.file_name())
                            .map(|s| s.to_string_lossy().into_owned())
                            .unwrap_or_else(|| "Default Worldkiln icon".into()),
                    );
                    if ui
                        .add_enabled(!running, egui::Button::new("Browse..."))
                        .clicked()
                    {
                        if let Some(icon) = rfd::FileDialog::new()
                            .add_filter("Game icon", &["ico", "png"])
                            .pick_file()
                        {
                            self.packaging_options.icon = Some(icon);
                        }
                    }
                    if ui
                        .add_enabled(
                            !running && self.packaging_options.icon.is_some(),
                            egui::Button::new("Default"),
                        )
                        .clicked()
                    {
                        self.packaging_options.icon = None;
                    }
                });
                ui.separator();
                ui.label("BUILD STATUS");
                ui.label(&self.packaging_status);
                if running {
                    ui.spinner();
                    if ui.button("CANCEL BUILD").clicked() {
                        self.packaging_cancel
                            .store(true, std::sync::atomic::Ordering::Relaxed);
                        self.packaging_status = "Canceling ? finishing current stage".into();
                    }
                }
                ui.separator();
                ui.add_enabled_ui(!running, |ui| {
                    if ui.button("BUILD GAME").clicked() {
                        start = true;
                    }
                    if ui.button("BUILD & RUN").clicked() {
                        start = true;
                        run_after = true;
                    }
                });
                ui.separator();
                ui.label(egui::RichText::new("PUBLISH TO AEOWUN").strong());
                use crate::engine::member_services::{MemberDisplay, PublishStatus};
                if let MemberDisplay::SignedIn(member) = self.home_member.display.clone() {
                    ui.label(format!("Signed in as @{}", member.handle));
                    let archive = self.last_built_zip.clone();
                    let eligible = archive.as_ref().is_some_and(|path| path.is_file())
                        && self.packaging_options.target == TargetPlatform::Windows
                        && self.packaging_options.configuration == GameBuildConfiguration::Shipping
                        && self.packaging_options.format == GameBuildFormat::ZipArchive;
                    if let Some(path) = archive.as_ref().filter(|_| eligible) {
                        ui.label(format!("Shipping archive: {}", path.display()));
                        ui.label("PUBLIC GAME DESCRIPTION");
                        ui.add(egui::TextEdit::multiline(&mut self.publish_description)
                            .desired_rows(3)
                            .hint_text("Describe what players can expect."));
                        ui.checkbox(&mut self.publish_rights, "I have the right to distribute this game and its content.");
                        ui.checkbox(&mut self.publish_safety, "I have checked the build for private data and unsafe files.");
                        ui.checkbox(&mut self.publish_publication, "I understand publication requires review and makes the approved ZIP publicly downloadable.");
                        let uploading = matches!(self.home_member.publishing, PublishStatus::Uploading { .. });
                        let valid = !running && !uploading
                            && self.publish_rights && self.publish_safety && self.publish_publication
                            && !self.publish_description.trim().is_empty()
                            && !self.packaging_options.title.trim().is_empty()
                            && !self.packaging_options.game_version.trim().is_empty();
                        if ui.add_enabled(valid, egui::Button::new("SUBMIT FOR REVIEW")).clicked() {
                            self.home_member.publish(
                                path.clone(),
                                self.packaging_options.title.clone(),
                                self.packaging_options.game_version.clone(),
                                self.publish_description.clone()
                            );
                        }
                    } else {
                        ui.label("Build a Windows Shipping ZIP to enable publishing.");
                    }
                    match &self.home_member.publishing {
                        PublishStatus::Idle => {}
                        PublishStatus::Uploading { percent, phase } => {
                            ui.label(format!("{phase}: {percent}%"));
                            ui.add(egui::ProgressBar::new(*percent as f32 / 100.0));
                            if ui.button("CANCEL UPLOAD").clicked() {
                                self.home_member.cancel_publish();
                            }
                        }
                        PublishStatus::Submitted(id) => {
                            ui.label(format!("Submitted for private review ({id})."));
                            ui.hyperlink_to("View your profile", format!(
                                "https://aeowun.com/members/profile/?handle={}", member.handle));
                        }
                        PublishStatus::Failed(message) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 115, 100), message);
                        }
                    }
                } else {
                    ui.label("Sign in to publish games to your member profile.");
                    if ui.button("OPEN MEMBER ACCOUNT").clicked() {
                        self.home_member.dialog_open = true;
                    }
                }
                    });
            });
        if self.home_member.dialog_open {
            self.draw_home_member_dialog(ctx);
        }
        if !running && self.packaging_options != previous_options {
            self.last_built_zip = None;
            if let Some(project) = &self.project_manager.current_project {
                if let Err(error) =
                    crate::engine::build::save_packaging_options(project, &self.packaging_options)
                {
                    self.packaging_status = format!("Could not save Packaging settings: {error}");
                }
            }
        }
        if start {
            self.packaging_run_after = run_after;
            self.build_game();
        }
    }

    fn draw_window_chrome(&mut self, ctx: &egui::Context) {
        if self.home_emblem_texture.is_none() {
            if let Ok(image) = image::load_from_memory(include_bytes!(
                "../../../assets/branding/worldkiln-emblem.png"
            )) {
                let rgba = image.to_rgba8();
                self.home_emblem_texture = Some(ctx.load_texture(
                    "aeowun_volcanic_emblem",
                    egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    ),
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        egui::TopBottomPanel::top("worldkiln_native_window_chrome")
            .exact_height(38.0)
            .frame(
                egui::Frame::none()
                    .fill(ctx.style().visuals.panel_fill)
                    .inner_margin(egui::Margin::symmetric(12.0, 5.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    if let Some(texture) = &self.home_emblem_texture {
                        ui.add(egui::Image::new(texture).fit_to_exact_size(egui::vec2(26.0, 26.0)));
                    }
                    ui.label(
                        egui::RichText::new("AEOWUN // WORLDKILN")
                            .strong()
                            .color(crate::engine::app::COLOR_ACCENT_ORANGE),
                    );
                    ui.label(egui::RichText::new(env!("CARGO_PKG_VERSION")).small());
                    let reserved = 249.0;
                    let room = (ui.available_width() - reserved).max(6.0);
                    let (_, drag) = ui
                        .allocate_exact_size(egui::vec2(room, 27.0), egui::Sense::click_and_drag());
                    if drag.drag_started() {
                        self.window_action = Some(super::WindowAction::Drag);
                    }
                    if drag.double_clicked() {
                        self.window_action = Some(super::WindowAction::MaximizeRestore);
                    }
                    if ui.button("DOCS").clicked() {
                        ui.ctx()
                            .open_url(egui::OpenUrl::new_tab("https://www.aeowun.com/docs/"));
                    }
                    if ui.button("SETTINGS").clicked() {
                        self.editor.open_engine_settings(self.editor.settings_page);
                    }
                    if ui
                        .add_sized([30.0, 26.0], egui::Button::new("—"))
                        .on_hover_text("Minimize window")
                        .clicked()
                    {
                        self.window_action = Some(super::WindowAction::Minimize);
                    }
                    if ui
                        .add_sized([30.0, 26.0], egui::Button::new("□"))
                        .on_hover_text("Maximize or restore window")
                        .clicked()
                    {
                        self.window_action = Some(super::WindowAction::MaximizeRestore);
                    }
                    if ui
                        .add_sized(
                            [30.0, 26.0],
                            egui::Button::new(
                                egui::RichText::new("×")
                                    .color(self.editor.theme_settings.accent.color()),
                            ),
                        )
                        .on_hover_text("Close window")
                        .clicked()
                    {
                        self.window_action = Some(super::WindowAction::Close);
                    }
                });
            });
    }

    pub fn update_ui(&mut self, ctx: &egui::Context) {
        #[cfg(feature = "kilngraph")]
        let _kg_scope = crate::kilngraph::Scope::new(50, file!(), line!());

        self.prepare_development_test_run_ui();
        let capture_started = (self.development_capture.borrow().state()
            == crate::engine::development_test_run::CaptureState::Running)
            .then(std::time::Instant::now);
        self.service_ai_agent(ctx);
        self.editor.collect_diagnostics();
        self.apply_editor_theme(ctx);
        let assets_started = capture_started.map(|_| std::time::Instant::now());
        self.service_asset_pack_jobs(ctx);
        self.poll_game_build();
        self.service_recovery();
        if let Some(started) = assets_started {
            self.development_capture.borrow_mut().record(
                "asset",
                "asset_pack_jobs_cpu",
                started.elapsed(),
            );
        }
        self.reserve_application_shell(ctx);

        let mut next_view = None;
        if self.view != View::Splash {
            self.draw_window_chrome(ctx);
        }

        if self.view == View::Splash {
            self.draw_splash_screen(ctx);
        } else if self.view == View::Home {
            if self.project_loading() {
                let elapsed = self
                    .project_loading_screen
                    .as_ref()
                    .map(|(started, _)| started.elapsed().as_secs_f32())
                    .unwrap_or_default();
                let project_name = self
                    .project_loading_screen
                    .as_ref()
                    .map(|(_, name)| name.as_str())
                    .unwrap_or("Project");
                let progress = self
                    .project_loader
                    .as_ref()
                    .map(|loader| loader.progress())
                    .unwrap_or_else(|| super::background_project::ProjectLoadProgress {
                        completed: 0,
                        total: 9,
                        current: "Preparing project",
                    });
                crate::engine::install_ui::draw_project_loading_screen(
                    ctx,
                    elapsed,
                    project_name,
                    progress.completed,
                    progress.total,
                    progress.current,
                );
            } else if self.editor.navigation.surface.is_settings() {
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
            let editor_started = capture_started.map(|_| std::time::Instant::now());
            self.editor.development_ui_timing_enabled = editor_started.is_some();
            self.editor.development_ui_spans.clear();
            self.editor.show_ui(
                ctx,
                &mut self.world,
                &self.project_manager.current_project,
                &self.renderer,
            );
            if let Some(magnitude) = self.editor.image_heightmap_import.take_erosion_sound() {
                self.audio_system
                    .play_erosion_edit(magnitude, &self.editor.engine_audio_settings);
            }
            if editor_started.is_some() {
                let mut recorder = self.development_capture.borrow_mut();
                for (operation, duration) in self.editor.development_ui_spans.drain(..) {
                    recorder.record("ui", operation, duration);
                }
            }
            if let Some(started) = editor_started {
                self.development_capture.borrow_mut().record(
                    "ui",
                    "editor_workspace_cpu",
                    started.elapsed(),
                );
            }

            self.draw_asset_pack_manager_dialog(ctx);

            if crate::editor::tools::texture_tools::take_gpu_texture_reload_request(ctx) {
                let reload_started = capture_started.map(|_| std::time::Instant::now());
                self.renderer.reload_material_assets();
                self.renderer.invalidate_texture_cache();
                if let Some(started) = reload_started {
                    self.development_capture.borrow_mut().record(
                        "asset",
                        "material_catalog_reload_cpu",
                        started.elapsed(),
                    );
                }
            }

            if std::mem::take(&mut self.editor.save_requested) {
                self.save_all_authored();
            }
            if self.editor.needs_build {
                self.editor.needs_build = false;
                if let Some(project) = &self.project_manager.current_project {
                    self.packaging_options = crate::engine::build::load_packaging_options(project);
                }
                self.packaging_open = true;
                let build_started = capture_started.map(|_| std::time::Instant::now());
                if let Some(started) = build_started {
                    self.development_capture.borrow_mut().record(
                        "project",
                        "build_game_cpu",
                        started.elapsed(),
                    );
                }
            }

            if self.packaging_open {
                self.draw_packaging_workspace(ctx);
            }
            self.draw_first_run_controls_overlay(ctx);

            if self.editor.mode == EditorMode::Editor
                && !self.editor.show_script_workspace
                && !self.editor.navigation.surface.is_settings()
                && !self.editor.ui_mode
                && self.editor.viewport_rect.is_finite()
                && self.editor.viewport_rect.is_positive()
            {
                let viewport = self.editor.viewport_rect;
                self.draw_editor_plane_hud(ctx);
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
                                if self.editor.script_editor.save_all_checked().is_ok() {
                                    self.editor.commit_pending_inspector_history();
                                    self.editor.mode = EditorMode::Play;
                                    self.show_unsaved_scripts_dialog = false;
                                }
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
        self.handle_development_test_run_action();
        if let Some(started) = capture_started {
            self.development_capture.borrow_mut().record(
                "ui",
                "application_update_cpu",
                started.elapsed(),
            );
        }
    }

    fn prepare_development_test_run_ui(&mut self) {
        use crate::editor::editor::SettingsPage;

        if self
            .development_save_task
            .as_ref()
            .is_some_and(|task| task.is_finished())
        {
            let task = self.development_save_task.take().expect("finished task");
            self.development_report_message = Some(match self.finish_test_run_save(task) {
                Ok(path) => format!("Saved report: {}", path.display()),
                Err(error) => format!("Save failed: {error}"),
            });
        }

        if self
            .development_load_task
            .as_ref()
            .is_some_and(|(_, task)| task.is_finished())
        {
            let (comparison, task) = self.development_load_task.take().expect("finished task");
            match task
                .join()
                .unwrap_or_else(|_| Err("Capture reader failed".into()))
            {
                Ok(capture) => {
                    let view = &mut self.editor.development_test_run_ui.investigation;
                    if comparison {
                        view.comparison = Some(std::sync::Arc::new(capture));
                    } else {
                        view.saved = Some(std::sync::Arc::new(capture));
                        view.selection.scope =
                            crate::engine::development_capture::Scope::WholeCapture;
                    }
                    view.refresh = true;
                    self.development_report_message = None;
                }
                Err(error) => {
                    self.development_report_message = Some(format!("Open failed: {error}"))
                }
            }
        }
        if self.development_capture.borrow().state()
            == crate::engine::development_test_run::CaptureState::Running
        {
            let (samples, dropped) = crate::engine::development_capture::drain_engine();
            self.development_capture
                .borrow_mut()
                .collect_spans(samples, dropped);
        }
        let development_settings = self.editor.settings_page == SettingsPage::Development
            && self.editor.navigation.surface.is_settings();
        let diagnostic_dock = self.editor.shell.dock_expanded
            && self.editor.shell.dock == crate::editor::editor::shell::DockTab::Diagnostics;
        if (!development_settings && !diagnostic_dock) || self.view == View::Splash {
            return;
        }
        if self.editor.development_test_run_ui.investigation.refresh
            || self.development_summary_cache.is_none()
            || self.development_summary_refreshed.elapsed() >= std::time::Duration::from_millis(250)
        {
            let recorder = self.development_capture.borrow();
            self.development_summary_cache = Some(recorder.summary());
            let view = &mut self.editor.development_test_run_ui.investigation;
            if !recorder.display_paused() || view.refresh || view.saved.is_some() {
                let started = std::time::Instant::now();
                view.snapshot = if let Some(saved) = &view.saved {
                    saved.inspect(&view.selection, view.selected.as_ref())
                } else {
                    recorder
                        .presentation_capture()
                        .inspect(&view.selection, view.selected.as_ref())
                };
                view.snapshot.warnings.push(format!(
                    "Workspace aggregation: {:.3} ms",
                    started.elapsed().as_secs_f64() * 1000.0
                ));
                view.refresh = false;
            }
            self.development_summary_refreshed = std::time::Instant::now();
        }
        let unsaved = self.development_capture.borrow().has_unsaved_capture();
        let has_project = self.development_session_project.borrow().is_some();
        let can_export = self.last_test_run_log_directory().is_some();
        let view = &mut self.editor.development_test_run_ui;
        view.loading = self.development_load_task.is_some();
        view.summary = self.development_summary_cache.clone();
        view.unsaved = unsaved;
        view.saving = self.development_save_task.is_some();
        view.has_project = has_project;
        view.can_export = can_export;
        view.confirm_replace = self.development_confirm_replace;
        view.message = self.development_report_message.clone();
        view.action = None;
    }

    fn handle_development_test_run_action(&mut self) {
        use crate::editor::editor::DevelopmentTestRunAction as Action;
        use crate::engine::development_test_run::CaptureState;
        let Some(action) = self.editor.development_test_run_ui.action.take() else {
            return;
        };
        match action {
            Action::Start => {
                self.editor.development_test_run_ui.investigation.saved = None;
                self.development_capture
                    .borrow_mut()
                    .set_config(self.editor.development_test_run_ui.config);
                let needs_confirmation = {
                    let recorder = self.development_capture.borrow();
                    recorder.state() == CaptureState::Stopped && recorder.has_unsaved_capture()
                };
                if needs_confirmation {
                    self.development_confirm_replace = true;
                } else {
                    self.start_test_run(false);
                }
            }
            Action::Pause => self.pause_test_run(),
            Action::Stop => self.stop_test_run(),
            Action::Save | Action::Export => match self.save_test_run_async() {
                Ok(task) => {
                    self.development_save_task = Some(task);
                    self.development_report_message = Some("Saving report...".into());
                }
                Err(error) => {
                    self.development_report_message = Some(format!("Save failed: {error}"));
                }
            },
            Action::Open | Action::Compare => {
                if self.development_load_task.is_none() {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("Worldkiln capture", &["json"])
                        .pick_file()
                    {
                        let comparison = action == Action::Compare;
                        self.development_load_task = Some((
                            comparison,
                            std::thread::spawn(move || {
                                crate::engine::development_capture::Capture::read(&path)
                                    .map_err(|e| e.to_string())
                            }),
                        ));
                    }
                }
            }
            Action::DiscardAndStart => {
                self.development_capture
                    .borrow_mut()
                    .set_config(self.editor.development_test_run_ui.config);
                self.start_test_run(true);
                self.development_confirm_replace = false;
            }
            Action::CancelDiscard => self.development_confirm_replace = false,
        }
        self.development_summary_cache = None;
        self.editor.development_test_run_ui.investigation.refresh = true;
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

    fn draw_editor_plane_hud(&self, ctx: &egui::Context) {
        let vp = self.editor.viewport_rect;
        if vp.width() < 280.0 || vp.height() < 140.0 {
            return;
        }
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("wk_editor_orientation_compass"),
        ));
        let origin = egui::pos2(vp.right() - 201.0, vp.top() + 48.0);
        let view = self.editor.camera.get_view_matrix();
        let axes = [
            (glam::Vec3::X, egui::Color32::from_rgb(255, 64, 64), "X"),
            (glam::Vec3::Y, egui::Color32::from_rgb(70, 250, 90), "Y"),
            (glam::Vec3::Z, egui::Color32::from_rgb(65, 125, 255), "Z"),
        ];
        for (axis, color, label) in axes {
            let rotated = view.transform_vector3(axis);
            let delta = egui::vec2(rotated.x, -rotated.y) * 24.0;
            let end = origin + delta;
            painter.line_segment([origin, end], egui::Stroke::new(2.7, color));
            painter.circle_filled(end, 2.5, color);
            let label_offset = if delta.length_sq() > 0.01 {
                delta.normalized() * 6.0
            } else {
                egui::vec2(0.0, -6.0)
            };
            painter.text(
                end + label_offset,
                egui::Align2::CENTER_CENTER,
                label,
                egui::FontId::monospace(10.0),
                color,
            );
        }
        painter.circle_filled(origin, 2.5, egui::Color32::from_gray(185));

        if self.editor.current_tool != crate::editor::EditorTool::Build
            || !self.editor.plane_picking
        {
            return;
        }
        let camera = &self.editor.camera;
        let plane = self.editor.plane_drag.as_ref().map_or_else(
            || crate::editor::grid::select_grid_plane(camera.get_position(), camera.target),
            |drag| drag.plane,
        );
        let ppp = self.editor.viewport_ppp.max(0.5);
        let pixel_size = glam::Vec2::new(vp.width() * ppp, vp.height() * ppp);
        let Some((globe, tip, _, _)) = crate::editor::grid::picking::plane_handle_positions(
            camera,
            self.editor.anchor,
            plane,
            pixel_size,
            ppp,
        ) else {
            return;
        };
        let globe = vp.min + egui::vec2(globe.x / ppp, globe.y / ppp);
        let tip = vp.min + egui::vec2(tip.x / ppp, tip.y / ppp);
        if !vp.contains(globe) {
            return;
        }
        let pointer = ctx.input(|i| i.pointer.hover_pos());
        let hovered = pointer.and_then(|pos| {
            crate::editor::grid::picking::plane_handle_hit(
                glam::Vec2::new((pos.x - vp.left()) * ppp, (pos.y - vp.top()) * ppp),
                glam::Vec2::new((globe.x - vp.left()) * ppp, (globe.y - vp.top()) * ppp),
                glam::Vec2::new((tip.x - vp.left()) * ppp, (tip.y - vp.top()) * ppp),
                ppp,
            )
        });
        let active = self.editor.plane_drag.map(|d| d.kind).or(hovered);
        let accent = self.editor.theme_settings.accent.color();
        let dim = accent.gamma_multiply(0.70);
        if vp.contains(tip) {
            painter.line_segment([globe, tip], egui::Stroke::new(1.7, dim));
            painter.circle_filled(
                tip,
                if active == Some(crate::editor::grid::picking::PlaneGrabKind::Normal) {
                    6.0
                } else {
                    4.5
                },
                if active == Some(crate::editor::grid::picking::PlaneGrabKind::Normal) {
                    accent
                } else {
                    dim
                },
            );
        }
        painter.circle_stroke(
            globe,
            if active == Some(crate::editor::grid::picking::PlaneGrabKind::Slide) {
                10.0
            } else {
                7.0
            },
            egui::Stroke::new(1.6, accent),
        );
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
                            RichText::new(self.editor.theme_settings.play_hint())
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
        let rect = self.editor.viewport_rect;
        if rect.width() <= 1.0 || rect.height() <= 1.0 {
            return;
        }
        crate::engine::ui_compositor::paint(
            ctx,
            &mut self.runtime_ui,
            rect,
            1.0,
            self.project_manager.current_project.as_deref(),
        );
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

        let accent_luma = 0.299 * accent_color.r() as f32
            + 0.587 * accent_color.g() as f32
            + 0.114 * accent_color.b() as f32;
        let selection_foreground = if accent_luma >= 150.0 {
            egui::Color32::from_rgb(12, 14, 16)
        } else {
            egui::Color32::WHITE
        };

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
    fn application_shell_border_tracks_accent_and_window_bounds() {
        use crate::engine::preferences::AccentTheme;
        let mut app = App::new_for_tests(1280.0, 720.0);
        let ctx = egui::Context::default();
        for accent in [AccentTheme::Cool, AccentTheme::Cyan] {
            app.editor.theme_settings.accent = accent;
            for maximized in [false, true] {
                let output = ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(1280.0, 720.0),
                        )),
                        ..Default::default()
                    },
                    |ctx| {
                        app.apply_editor_theme(ctx);
                        app.reserve_application_shell(ctx);
                        app.draw_window_chrome(ctx);
                        app.draw_application_border(ctx, maximized);
                    },
                );
                let border = output
                    .shapes
                    .iter()
                    .filter_map(|shape| {
                        if let egui::Shape::Rect(rect) = &shape.shape {
                            (rect.stroke.color == accent.color() && rect.rect.width() > 1000.0)
                                .then_some(rect)
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>();
                assert_eq!(border.len(), 1, "exactly one full-window accent outline");
                assert!(border[0].rect.min.x >= border[0].stroke.width * 0.5);
                assert_eq!(border[0].rounding.nw == 0.0, maximized);
                assert!(
                    output.shapes.iter().any(|shape| {
                        if let egui::Shape::Text(text) = &shape.shape {
                            text.galley.text() == "×"
                                && text.galley.rows.iter().any(|row| {
                                    row.visuals
                                        .mesh
                                        .vertices
                                        .iter()
                                        .any(|vertex| vertex.color == accent.color())
                                })
                        } else {
                            false
                        }
                    }),
                    "Close inherits the same accent"
                );
            }
        }
    }

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
