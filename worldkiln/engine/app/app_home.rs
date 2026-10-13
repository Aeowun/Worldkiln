// Worldkiln 0.8.5 | WO 0.8.5-DOC-03 | 2026-10-05 00:37 -05:00
// Branding assets relocated to assets/branding.
// Worldkiln Home launcher screen implementation.
// Adheres to WO 0.8.5-RF-06 & AEOWUN brand guidelines:
// 1. Inherits the active editor Settings theme (no independent theme overrides).
// 2. Fixed AEOWUN brand fire treatment is restricted to brand identity and emblem.
// 3. Implements approved composition with header, nav rail, heading, recent projects, recent builds, and footer.
// 4. Glowing Volcanic Forge A Emblem logo and 2% padded black frame with orange stroke.

use crate::engine::audio::EngineSound;
use egui::RichText;
use std::process::Command;

use super::{App, COLOR_ACCENT_ORANGE, COLOR_ACCENT_RED, View};

const EMBLEM_PNG_BYTES: &[u8] = include_bytes!("../../../assets/branding/worldkiln-emblem.png");

#[derive(Clone)]
pub(crate) enum ManagementTarget {
    Project(std::path::PathBuf),
    Build(crate::project::project::BuiltGame),
}

impl ManagementTarget {
    fn name(&self) -> String {
        match self {
            Self::Project(path) => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Self::Build(game) => game.project_name.clone(),
        }
    }
}

pub(crate) struct HomeManagement {
    target: ManagementTarget,
    rename: bool,
    name: String,
}

// Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 17:28 -05:00
// Merge canonical recent entries and ordinary read_dir paths by their
// physical Windows identity, preserving recency order without duplicates.
fn merge_recent_project_paths(
    mut recent: Vec<std::path::PathBuf>,
    mut discovered: Vec<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    discovered.sort();
    for path in discovered {
        if !recent
            .iter()
            .any(|existing| crate::project::project::same_path(existing, &path))
        {
            recent.push(path);
        }
    }
    let mut unique: Vec<std::path::PathBuf> = Vec::new();
    for path in recent {
        if !unique
            .iter()
            .any(|existing| crate::project::project::same_path(existing, &path))
        {
            unique.push(path);
        }
    }
    unique
}

impl App {
    /// Renders the Worldkiln splash screen before transitioning to Home.
    pub(crate) fn draw_splash_screen(&mut self, ctx: &egui::Context) {
        let elapsed = self.splash_started.elapsed().as_secs_f32();

        crate::engine::install_ui::draw_worldkiln_splash(ctx, elapsed, self.development_build);

        if elapsed >= 7.0 {
            self.view = View::Home;
        }
    }

    /// Primary Home screen draw call. Inherits the active editor theme set by `apply_editor_theme`.
    pub(crate) fn draw_home_screen(&mut self, ctx: &egui::Context, next_view: &mut Option<View>) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| {
                // Background surface with faint AEOWUN grid texture and 2% padded black frame with orange stroke.
                self.draw_home_background(ui);

                let available = ui.available_rect_before_wrap();
                let pad_x = available.width() * 0.02;
                let pad_y = available.height() * 0.02;
                let frame_rect = available.shrink2(egui::vec2(pad_x, pad_y));

                // All Home content fits INSIDE of the 2% padded black frame.
                let outer = frame_rect.shrink2(egui::vec2(14.0, 10.0));

                // 1. Header across top inside frame.
                let header_height = 36.0;
                let header_rect = egui::Rect::from_min_max(
                    outer.left_top(),
                    egui::pos2(outer.right(), outer.top() + header_height),
                );
                ui.allocate_ui_at_rect(header_rect, |ui| {
                    self.draw_home_header(ui);
                });

                // 2. Quiet Footer across bottom inside frame.
                let footer_height = 22.0;
                let footer_rect = egui::Rect::from_min_max(
                    egui::pos2(outer.left(), outer.bottom() - footer_height),
                    outer.right_bottom(),
                );
                ui.allocate_ui_at_rect(footer_rect, |ui| {
                    self.draw_home_footer(ui);
                });

                // 3. Middle content workspace (nav rail + main area) inside frame.
                let content_top = header_rect.bottom() + 8.0;
                let content_bottom = footer_rect.top() - 6.0;

                if content_bottom <= content_top {
                    return;
                }

                let content_rect = egui::Rect::from_min_max(
                    egui::pos2(outer.left(), content_top),
                    egui::pos2(outer.right(), content_bottom),
                );

                // Narrow Navigation Rail on the left (56px).
                let rail_width = 56.0;
                let rail_rect = egui::Rect::from_min_max(
                    content_rect.left_top(),
                    egui::pos2(content_rect.left() + rail_width, content_rect.bottom()),
                );

                ui.allocate_ui_at_rect(rail_rect, |ui| {
                    self.draw_home_nav_rail(ui);
                });

                // Vertical separator between rail and main content.
                let main_left = rail_rect.right() + 16.0;
                ui.painter().line_segment(
                    [
                        egui::pos2(rail_rect.right() + 8.0, content_rect.top()),
                        egui::pos2(rail_rect.right() + 8.0, content_rect.bottom()),
                    ],
                    ui.visuals().widgets.noninteractive.bg_stroke,
                );

                // Main Content Workspace.
                let main_rect = egui::Rect::from_min_max(
                    egui::pos2(main_left, content_rect.top()),
                    content_rect.right_bottom(),
                );

                ui.allocate_ui_at_rect(main_rect, |ui| {
                    self.draw_home_main_content(ui, next_view);
                });
            });

        // Dialog overlays (New Project, Open Project, Exit Confirmation).
        self.draw_home_dialogs(ctx, next_view);
        self.draw_home_management_dialogs(ctx);
    }

    /// Draws the background grid and the 2% padded black frame with orange stroke.
    pub(crate) fn draw_home_background(&self, ui: &mut egui::Ui) {
        let rect = ui.max_rect();
        let painter = ui.painter();

        // 1. Dark surface fill from active Settings theme for outer margin
        painter.rect_filled(rect, 0.0, ui.visuals().panel_fill);

        // 2. Faint 64px grid lines (alpha ~5/255) behind the black frame
        let grid_size = 64.0;
        let line_stroke =
            egui::Stroke::new(1.0, egui::Color32::from_rgba_unmultiplied(255, 255, 255, 5));

        let mut x = rect.left();
        while x <= rect.right() {
            painter.line_segment(
                [egui::pos2(x, rect.top()), egui::pos2(x, rect.bottom())],
                line_stroke,
            );
            x += grid_size;
        }

        let mut y = rect.top();
        while y <= rect.bottom() {
            painter.line_segment(
                [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
                line_stroke,
            );
            y += grid_size;
        }

        // 3. Black frame with 2% padding around top, bottom, right, and left of screen with ORANGE STROKE
        let pad_x = rect.width() * 0.02;
        let pad_y = rect.height() * 0.02;
        let frame_rect = rect.shrink2(egui::vec2(pad_x, pad_y));

        painter.rect_filled(frame_rect, 4.0, egui::Color32::BLACK);
        painter.rect_stroke(frame_rect, 4.0, egui::Stroke::new(1.5, COLOR_ACCENT_ORANGE));
    }

    /// Home header carrying the Glowing Volcanic Forge A Emblem and AEOWUN fire identity on the left,
    /// and theme-derived utility buttons on the right.
    pub(crate) fn draw_home_header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 10.0;

            // Load Glowing Volcanic Forge A Emblem texture if not already loaded.
            if self.home_emblem_texture.is_none() {
                if let Ok(img) = image::load_from_memory(EMBLEM_PNG_BYTES) {
                    let rgba = img.to_rgba8();
                    let color_image = egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    );
                    self.home_emblem_texture = Some(ui.ctx().load_texture(
                        "aeowun_volcanic_emblem",
                        color_image,
                        egui::TextureOptions::LINEAR,
                    ));
                }
            }

            // Render Glowing Volcanic Forge A Emblem logo
            let emblem_size = egui::vec2(28.0, 28.0);
            if let Some(texture) = &self.home_emblem_texture {
                ui.add(egui::Image::new(texture).fit_to_exact_size(emblem_size));
            } else {
                ui.allocate_exact_size(emblem_size, egui::Sense::hover());
            }

            // Wordmark & Version
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;

                ui.label(
                    RichText::new("AEOWUN // WORLDKILN")
                        .monospace()
                        .strong()
                        .color(COLOR_ACCENT_ORANGE)
                        .size(14.0),
                );

                ui.label(
                    RichText::new(env!("CARGO_PKG_VERSION"))
                        .monospace()
                        .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                        .size(11.0),
                );
            });

            // Right side utility controls following Settings theme.
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let exit_btn = egui::Button::new(
                    RichText::new("×")
                        .strong()
                        .size(16.0)
                        .color(egui::Color32::WHITE),
                )
                .min_size(egui::vec2(28.0, 26.0))
                .fill(COLOR_ACCENT_RED);

                if ui.add(exit_btn).clicked() {
                    self.show_exit_confirmation_dialog = true;
                }

                ui.add_space(8.0);

                if ui.button("SETTINGS").clicked() {
                    self.editor.open_engine_settings(self.editor.settings_page);
                    self.play_engine_sound(EngineSound::Select);
                }

                ui.add_space(4.0);

                if ui.button("DOCS").clicked() {
                    ui.ctx()
                        .open_url(egui::OpenUrl::new_tab("https://www.aeowun.com/docs/"));
                }
            });
        });

        // Subdued header divider
        ui.add_space(4.0);
        ui.separator();
    }

    /// Narrow left navigation rail containing the active HOME destination.
    pub(crate) fn draw_home_nav_rail(&mut self, ui: &mut egui::Ui) {
        ui.vertical_centered(|ui| {
            ui.add_space(4.0);

            let label = RichText::new("HOME")
                .monospace()
                .strong()
                .size(11.0)
                .color(ui.visuals().selection.stroke.color);

            let home_btn = egui::Button::new(label)
                .fill(ui.visuals().selection.bg_fill)
                .min_size(egui::vec2(48.0, 32.0));

            ui.add(home_btn);
        });
    }

    /// Main content area containing heading, primary/secondary action buttons,
    /// and Recent Projects + Recent Builds lists.
    pub(crate) fn draw_home_main_content(
        &mut self,
        ui: &mut egui::Ui,
        next_view: &mut Option<View>,
    ) {
        // Page Heading + Action Buttons
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading(RichText::new("PROJECTS").monospace().strong().size(20.0));
                ui.label(
                    RichText::new("Your Worldkiln projects")
                        .color(ui.visuals().text_color())
                        .size(12.0),
                );
                ui.label(
                    RichText::new("Open an existing project or create one.")
                        .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                        .size(11.0),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                // Secondary action: OPEN PROJECT
                let open_button = egui::Button::new(
                    RichText::new("OPEN PROJECT")
                        .monospace()
                        .strong()
                        .size(12.0),
                )
                .min_size(egui::vec2(120.0, 32.0));

                if ui.add(open_button).clicked() {
                    self.show_open_project_dialog = true;
                    self.show_new_project_dialog = false;
                }

                // Primary action: + NEW PROJECT (Uses active editor Accent)
                let accent_fill = ui.visuals().selection.bg_fill;
                let accent_stroke_color = ui.visuals().selection.stroke.color;

                let new_button = egui::Button::new(
                    RichText::new("+ NEW PROJECT")
                        .monospace()
                        .strong()
                        .color(accent_stroke_color)
                        .size(12.0),
                )
                .fill(accent_fill)
                .min_size(egui::vec2(130.0, 32.0));

                if ui.add(new_button).clicked() {
                    self.show_new_project_dialog = true;
                    self.show_open_project_dialog = false;
                }
            });
        });

        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        // Responsive arrangement: Side-by-side if wide (>= 600px), stacked if narrow.
        let available_width = ui.available_width();
        let is_wide = available_width >= 600.0;

        if is_wide {
            let gap = 16.0;
            let projects_width = (available_width - gap) * 0.58;
            let builds_width = available_width - gap - projects_width;

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(projects_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.draw_home_recent_projects(ui, next_view);
                    },
                );

                ui.add_space(gap);

                ui.allocate_ui_with_layout(
                    egui::vec2(builds_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        self.draw_home_recent_builds(ui);
                    },
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("home_main_scroll")
                .show(ui, |ui| {
                    self.draw_home_recent_projects(ui, next_view);
                    ui.add_space(16.0);
                    ui.separator();
                    ui.add_space(16.0);
                    self.draw_home_recent_builds(ui);
                });
        }
    }

    /// Renders the list of recent projects.
    pub(crate) fn draw_home_recent_projects(
        &mut self,
        ui: &mut egui::Ui,
        next_view: &mut Option<View>,
    ) {
        ui.label(
            RichText::new("RECENT PROJECTS")
                .monospace()
                .strong()
                .size(13.0),
        );
        ui.add_space(8.0);

        let recent = merge_recent_project_paths(
            self.project_manager.recent_projects.clone(),
            self.project_manager.list_projects(),
        );

        if recent.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("NO RECENT PROJECTS")
                    .monospace()
                    .strong()
                    .size(11.0),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Create a new Worldkiln project to get started.")
                    .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                    .size(11.0),
            );
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("home_recent_projects_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                for (index, path) in recent.into_iter().enumerate() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let path_str = path.to_string_lossy();

                    let row_width = ui.available_width();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(row_width, 54.0), egui::Sense::click());

                    let hovered = response.hovered();

                    let fill = if hovered {
                        ui.visuals().widgets.hovered.bg_fill
                    } else {
                        ui.visuals().widgets.inactive.bg_fill
                    };

                    let stroke = if hovered {
                        ui.visuals().widgets.hovered.bg_stroke
                    } else {
                        ui.visuals().widgets.inactive.bg_stroke
                    };

                    ui.painter()
                        .rect_filled(rect, ui.visuals().widgets.inactive.rounding, fill);
                    ui.painter()
                        .rect_stroke(rect, ui.visuals().widgets.inactive.rounding, stroke);

                    let num_text = format!("{:02}", index + 1);
                    let text_left = rect.left() + 10.0;

                    // Number
                    ui.painter().text(
                        egui::pos2(text_left, rect.top() + 10.0),
                        egui::Align2::LEFT_TOP,
                        &num_text,
                        egui::FontId::monospace(11.0),
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    // Project Name
                    let name_x = text_left + 28.0;
                    let title =
                        egui::WidgetText::from(RichText::new(name.as_ref()).monospace().size(12.0))
                            .into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                (rect.right() - name_x - 152.0).max(20.0),
                                egui::TextStyle::Body,
                            );
                    ui.painter().galley(
                        egui::pos2(name_x, rect.top() + 8.0),
                        title,
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    // Subtitle
                    ui.painter().text(
                        egui::pos2(name_x, rect.top() + 25.0),
                        egui::Align2::LEFT_TOP,
                        "Local project",
                        egui::FontId::proportional(10.0),
                        ui.visuals().widgets.noninteractive.fg_stroke.color,
                    );

                    // Truncated Path
                    let max_path_width = (rect.right() - name_x - 152.0).max(40.0);
                    let truncated_path = egui::WidgetText::from(path_str.as_ref()).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        max_path_width,
                        egui::TextStyle::Small,
                    );
                    ui.painter().galley(
                        egui::pos2(name_x, rect.top() + 38.0),
                        truncated_path,
                        ui.visuals().widgets.noninteractive.fg_stroke.color,
                    );

                    // OPEN -> affordance
                    ui.painter().text(
                        egui::pos2(rect.right() - 84.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        "OPEN →",
                        egui::FontId::monospace(11.0),
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    let manage_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 76.0, rect.top() + 12.0),
                        egui::pos2(rect.right() - 8.0, rect.bottom() - 12.0),
                    );
                    self.draw_home_manage_menu(
                        ui,
                        manage_rect,
                        ManagementTarget::Project(path.clone()),
                    );
                    if response.clicked() && !ui.rect_contains_pointer(manage_rect) {
                        if self.project_manager.open_project(path) {
                            self.load_project();
                            self.play_engine_sound(EngineSound::ProjectOpened);
                            *next_view = Some(View::Editor);
                        } else {
                            self.play_engine_sound(EngineSound::Rejected);
                            self.project_manager.load_recent();
                        }
                    }

                    ui.add_space(6.0);
                }
            });
    }

    /// Renders the list of recent builds.
    pub(crate) fn draw_home_recent_builds(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("RECENT BUILDS")
                .monospace()
                .strong()
                .size(13.0),
        );
        ui.add_space(8.0);

        let built_games = self.project_manager.list_built_games();

        if built_games.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("NO RECENT BUILDS")
                    .monospace()
                    .strong()
                    .size(11.0),
            );
            ui.add_space(4.0);
            ui.label(
                RichText::new("Built games will appear here.")
                    .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                    .size(11.0),
            );
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("home_recent_builds_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                for (index, game) in built_games.into_iter().enumerate() {
                    let row_width = ui.available_width();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(row_width, 48.0), egui::Sense::click());

                    let hovered = response.hovered();

                    let fill = if hovered {
                        ui.visuals().widgets.hovered.bg_fill
                    } else {
                        ui.visuals().widgets.inactive.bg_fill
                    };

                    let stroke = if hovered {
                        ui.visuals().widgets.hovered.bg_stroke
                    } else {
                        ui.visuals().widgets.inactive.bg_stroke
                    };

                    ui.painter()
                        .rect_filled(rect, ui.visuals().widgets.inactive.rounding, fill);
                    ui.painter()
                        .rect_stroke(rect, ui.visuals().widgets.inactive.rounding, stroke);

                    let num_text = format!("{:02}", index + 1);
                    let text_left = rect.left() + 10.0;

                    // Number
                    ui.painter().text(
                        egui::pos2(text_left, rect.top() + 10.0),
                        egui::Align2::LEFT_TOP,
                        &num_text,
                        egui::FontId::monospace(11.0),
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    // Project Name
                    let name_x = text_left + 28.0;
                    let title = egui::WidgetText::from(
                        RichText::new(&game.project_name).monospace().size(12.0),
                    )
                    .into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        (rect.right() - name_x - 152.0).max(20.0),
                        egui::TextStyle::Body,
                    );
                    ui.painter().galley(
                        egui::pos2(name_x, rect.top() + 8.0),
                        title,
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    // Version string
                    ui.painter().text(
                        egui::pos2(name_x, rect.top() + 26.0),
                        egui::Align2::LEFT_TOP,
                        &game.version,
                        egui::FontId::monospace(10.0),
                        ui.visuals().widgets.noninteractive.fg_stroke.color,
                    );

                    // PLAY -> button/affordance on right
                    let play_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 144.0, rect.top() + 10.0),
                        egui::pos2(rect.right() - 90.0, rect.bottom() - 10.0),
                    );

                    let play_hovered = ui.rect_contains_pointer(play_rect);

                    ui.painter().rect_filled(
                        play_rect,
                        ui.visuals().widgets.inactive.rounding,
                        if play_hovered {
                            ui.visuals().selection.bg_fill
                        } else {
                            ui.visuals().widgets.active.bg_fill
                        },
                    );

                    ui.painter().text(
                        play_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "PLAY →",
                        egui::FontId::monospace(10.0),
                        if play_hovered {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().widgets.active.fg_stroke.color
                        },
                    );

                    let manage_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 76.0, rect.top() + 10.0),
                        egui::pos2(rect.right() - 8.0, rect.bottom() - 10.0),
                    );
                    self.draw_home_manage_menu(
                        ui,
                        manage_rect,
                        ManagementTarget::Build(game.clone()),
                    );
                    if response.clicked() && !ui.rect_contains_pointer(manage_rect) {
                        match Command::new(&game.executable)
                            .current_dir(
                                game.executable
                                    .parent()
                                    .unwrap_or_else(|| std::path::Path::new(".")),
                            )
                            .spawn()
                        {
                            Ok(_) => {
                                self.editor.terminal_output.push_str(&format!(
                                    "[GAME] Launched {} ({})\n",
                                    game.project_name, game.version
                                ));
                            }
                            Err(error) => {
                                self.home_management_message = Some(format!(
                                    "Failed to launch {}: {}",
                                    game.executable.display(),
                                    error
                                ));
                                self.editor.terminal_output.push_str(&format!(
                                    "[GAME ERROR] Failed to launch {}: {}\n",
                                    game.executable.display(),
                                    error
                                ));
                            }
                        }
                        self.editor.terminal_auto_scroll = true;
                    }

                    ui.add_space(6.0);
                }
            });
    }

    fn draw_home_manage_menu(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        target: ManagementTarget,
    ) {
        ui.push_id(match &target {
            ManagementTarget::Project(path) => path.clone(),
            ManagementTarget::Build(game) => game.executable.clone(),
        }, |ui| {
            ui.allocate_ui_at_rect(rect, |ui| {
                ui.menu_button("MANAGE", |ui| {
                    for action in ["RENAME", "DUPLICATE", "MOVE", "DELETE"] {
                        if ui.button(action).clicked() {
                            ui.close_menu();
                            match action {
                                "RENAME" | "DELETE" => {
                                    self.home_management = Some(HomeManagement {
                                        name: target.name(), target: target.clone(), rename: action == "RENAME",
                                    });
                                }
                                "DUPLICATE" => {
                                    let result = match &target {
                                        ManagementTarget::Project(path) => self.project_manager.duplicate_project(path),
                                        ManagementTarget::Build(game) => self.project_manager.duplicate_built_game(game),
                                    };
                                    self.home_management_message = Some(match result {
                                        Ok(path) => format!("Duplicated to {}", path.display()),
                                        Err(error) => format!("DUPLICATE failed: {error}"),
                                    });
                                }
                                "MOVE" => {
                                    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
                                        .map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("C:\\Users\\Public\\Documents"));
                                    if let Some(folder) = rfd::FileDialog::new()
                                        .set_title("MOVE — export a copy (original stays in Worldkiln)")
                                        .set_directory(home).pick_folder() {
                                        let result = match &target {
                                            ManagementTarget::Project(path) => self.project_manager.export_project(path, &folder),
                                            ManagementTarget::Build(game) => self.project_manager.export_built_game(game, &folder),
                                        };
                                        self.home_management_message = Some(match result {
                                            Ok(path) => format!("Exported to {}. Original preserved.", path.display()),
                                            Err(error) => format!("MOVE failed: {error}"),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    ui.separator();
                    ui.label("MOVE exports a copy; the original stays.");
                });
            });
        });
    }

    fn draw_home_management_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(mut pending) = self.home_management.take() {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new(if pending.rename {
                "Rename"
            } else {
                "Delete complete package?"
            })
            .id(egui::Id::new("home_management_confirmation"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(pending.target.name());
                if pending.rename {
                    ui.text_edit_singleline(&mut pending.name);
                } else {
                    ui.label(
                        "This permanently deletes the complete selected folder and its content.",
                    );
                }
                ui.horizontal(|ui| {
                    confirm = ui
                        .button(if pending.rename { "RENAME" } else { "DELETE" })
                        .clicked();
                    cancel = ui.button("CANCEL").clicked();
                });
            });
            if confirm {
                let result = match &pending.target {
                    ManagementTarget::Project(path) if pending.rename => self
                        .project_manager
                        .rename_project(path, &pending.name)
                        .map(|_| ()),
                    ManagementTarget::Build(game) if pending.rename => self
                        .project_manager
                        .rename_built_game(game, &pending.name)
                        .map(|_| ()),
                    ManagementTarget::Project(path) => self.project_manager.delete_project(path),
                    ManagementTarget::Build(game) => self.project_manager.delete_built_game(game),
                };
                self.home_management_message = Some(match result {
                    Ok(()) => if pending.rename {
                        "Renamed successfully."
                    } else {
                        "Deleted selected folder."
                    }
                    .to_string(),
                    Err(error) => format!("Management failed: {error}"),
                });
            } else if !cancel {
                self.home_management = Some(pending);
            }
        }
        if let Some(message) = self.home_management_message.clone() {
            egui::Window::new("Project / Built Game management")
                .id(egui::Id::new("home_management_result"))
                .collapsible(false)
                .resizable(false)
                .max_width(520.0)
                .show(ctx, |ui| {
                    ui.label(message);
                    if ui.button("OK").clicked() {
                        self.home_management_message = None;
                    }
                });
        }
    }

    /// Quiet footer at the bottom of the Home launcher.
    pub(crate) fn draw_home_footer(&self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            // BETA badge (small brand accent)
            ui.label(
                RichText::new("BETA")
                    .monospace()
                    .strong()
                    .color(COLOR_ACCENT_ORANGE)
                    .size(10.0),
            );

            // Worldkiln 0.8.7 | WO 0.8.7-11 | 2026-10-05 21:44 -05:00
            // BETA status describes feature maturity, not a withdrawn release-version boundary.
            ui.label(
                RichText::new(
                    "Features marked BETA are still in development and may change as they mature.",
                )
                .size(10.0)
                .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
            );

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new("AEOWUN ONLINE — COMING SOON")
                        .monospace()
                        .strong()
                        .size(10.0)
                        .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
                );
            });
        });
    }

    /// Renders modal dialog overlays on top of the Home launcher.
    pub(crate) fn draw_home_dialogs(&mut self, ctx: &egui::Context, next_view: &mut Option<View>) {
        if self.show_new_project_dialog {
            egui::Window::new("New Project")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .frame(egui::Frame::window(&ctx.style()))
                .show(ctx, |ui| {
                    ui.add_space(8.0);

                    ui.label(
                        RichText::new("PROJECT NAME")
                            .monospace()
                            .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                            .size(10.0),
                    );

                    ui.add_space(6.0);

                    ui.add_sized(
                        egui::vec2(320.0, 28.0),
                        egui::TextEdit::singleline(&mut self.new_project_name),
                    );

                    ui.add_space(16.0);

                    ui.horizontal(|ui| {
                        let create_btn = egui::Button::new(
                            RichText::new("CREATE PROJECT")
                                .monospace()
                                .strong()
                                .color(ui.visuals().selection.stroke.color),
                        )
                        .min_size(egui::vec2(140.0, 30.0))
                        .fill(ui.visuals().selection.bg_fill);

                        if ui.add(create_btn).clicked() {
                            if self
                                .project_manager
                                .create_project(&self.new_project_name)
                                .is_some()
                            {
                                self.load_project();
                                self.play_engine_sound(EngineSound::ProjectCreated);
                                *next_view = Some(View::Editor);
                                self.show_new_project_dialog = false;
                                self.new_project_name.clear();
                            } else {
                                self.play_engine_sound(EngineSound::Rejected);
                            }
                        }

                        if ui
                            .add(
                                egui::Button::new(RichText::new("CANCEL").monospace())
                                    .min_size(egui::vec2(90.0, 30.0)),
                            )
                            .clicked()
                        {
                            self.show_new_project_dialog = false;
                        }
                    });

                    ui.add_space(8.0);
                });
        }

        if self.show_open_project_dialog {
            egui::Window::new("Open Project")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .frame(egui::Frame::window(&ctx.style()))
                .show(ctx, |ui| {
                    ui.add_space(8.0);

                    let projects = self.project_manager.list_projects();

                    if projects.is_empty() {
                        ui.label(
                            RichText::new("No projects found in UserData.")
                                .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                                .size(12.0),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for path in projects {
                                    let name =
                                        path.file_name().unwrap_or_default().to_string_lossy();

                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(name.to_string()).strong(),
                                            )
                                            .min_size(egui::vec2(320.0, 32.0)),
                                        )
                                        .clicked()
                                    {
                                        if self.project_manager.open_project(path) {
                                            self.load_project();
                                            self.play_engine_sound(EngineSound::ProjectOpened);
                                            *next_view = Some(View::Editor);
                                            self.show_open_project_dialog = false;
                                        } else {
                                            self.play_engine_sound(EngineSound::Rejected);
                                        }
                                    }

                                    ui.add_space(6.0);
                                }
                            });
                    }

                    ui.add_space(16.0);

                    if ui
                        .add(
                            egui::Button::new(RichText::new("CANCEL").monospace())
                                .min_size(egui::vec2(90.0, 28.0)),
                        )
                        .clicked()
                    {
                        self.show_open_project_dialog = false;
                    }

                    ui.add_space(8.0);
                });
        }

        if self.show_exit_confirmation_dialog {
            egui::Window::new("Exit Worldkiln?")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .frame(egui::Frame::window(&ctx.style()))
                .show(ctx, |ui| {
                    ui.label(RichText::new("Are you sure you want to exit?").monospace());

                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        let yes_btn = egui::Button::new(
                            RichText::new("YES")
                                .monospace()
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .min_size(egui::vec2(70.0, 28.0))
                        .fill(COLOR_ACCENT_ORANGE);

                        if ui.add(yes_btn).clicked() {
                            self.show_exit_confirmation_dialog = false;
                            self.exit_requested = true;
                        }

                        if ui
                            .add(
                                egui::Button::new(RichText::new("NO").monospace())
                                    .min_size(egui::vec2(70.0, 28.0)),
                            )
                            .clicked()
                        {
                            self.show_exit_confirmation_dialog = false;
                        }
                    });
                });
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn text_center(output: &egui::FullOutput, text: &str) -> egui::Pos2 {
        output
            .shapes
            .iter()
            .find_map(|shape| {
                if let egui::epaint::Shape::Text(shape) = &shape.shape {
                    if shape.galley.text() == text {
                        return Some(shape.pos + shape.galley.size() * 0.5);
                    }
                }
                None
            })
            .unwrap_or_else(|| panic!("Missing visible {text}"))
    }

    #[test]
    fn management_menu_does_not_open_project_and_delete_waits_for_confirmation() {
        for (width, height) in [
            (1280.0, 720.0),
            (1600.0, 900.0),
            (1920.0, 1080.0),
            (1000.0, 650.0),
        ] {
            let root = std::env::temp_dir().join(format!(
                "Worldkiln_HomeManagement_{}_{width}",
                std::process::id()
            ));
            let project = root.join("Selected");
            std::fs::create_dir_all(&project).unwrap();
            std::fs::write(project.join("project.json"), b"fixture").unwrap();
            let mut app = App::new_for_tests(width, height);
            app.project_manager = crate::project::ProjectManager {
                user_data_dir: root.clone(),
                current_project: None,
                recent_projects: vec![project.clone()],
            };
            let ctx = egui::Context::default();
            let mut next_view = None;
            let mut frame = |events| {
                ctx.run(
                    egui::RawInput {
                        screen_rect: Some(egui::Rect::from_min_size(
                            egui::Pos2::ZERO,
                            egui::vec2(width, height),
                        )),
                        events,
                        ..Default::default()
                    },
                    |ctx| app.draw_home_screen(ctx, &mut next_view),
                )
            };
            frame(vec![]);
            let output = frame(vec![]);
            let manage = text_center(&output, "MANAGE");
            let click = |pos, pressed| {
                vec![
                    egui::Event::PointerMoved(pos),
                    egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed,
                        modifiers: egui::Modifiers::NONE,
                    },
                ]
            };
            frame(click(manage, true));
            frame(click(manage, false));
            let output = frame(vec![]);
            for action in ["RENAME", "DUPLICATE", "MOVE", "DELETE"] {
                text_center(&output, action);
            }
            let delete = text_center(&output, "DELETE");
            frame(click(delete, true));
            frame(click(delete, false));
            let output = frame(vec![]);
            assert!(
                project.exists(),
                "Opening delete confirmation must preserve content"
            );
            let cancel = text_center(&output, "CANCEL");
            frame(click(cancel, true));
            frame(click(cancel, false));
            assert_eq!(next_view, None);
            assert!(app.home_management.is_none());
            assert!(project.join("project.json").exists());
            std::fs::remove_dir_all(root).unwrap();
        }
    }

    #[test]
    fn test_home_screen_renders_with_custom_editor_accent() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let ctx = egui::Context::default();

        // Change accent to Cool to verify Home inherits theme accent without error.
        app.editor.theme_settings.accent = crate::engine::preferences::AccentTheme::Cool;
        app.apply_editor_theme(&ctx);

        let mut next_view = None;
        let output = ctx.run(Default::default(), |ctx| {
            app.draw_home_screen(ctx, &mut next_view);
        });

        assert_eq!(next_view, None);
        assert!(!output.shapes.is_empty());
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 17:29 -05:00
    #[test]
    fn home_recent_merge_deduplicates_windows_canonical_and_normal_paths() {
        let plain = std::path::PathBuf::from(r"C:\Users\fixit\AppData\Local\Aeowun\UserData\Hello");
        let canonical =
            std::path::PathBuf::from(r"\\?\C:\Users\fixit\AppData\Local\Aeowun\UserData\Hello");
        let second =
            std::path::PathBuf::from(r"C:\Users\fixit\AppData\Local\Aeowun\UserData\Second");
        let merged = merge_recent_project_paths(
            vec![canonical.clone(), canonical.clone()],
            vec![plain, second],
        );
        assert_eq!(merged.len(), 2);
        assert_eq!(merged[0], canonical);
    }

    #[test]
    fn test_home_screen_empty_states() {
        let mut app = App::new_for_tests(1280.0, 720.0);
        let ctx = egui::Context::default();

        app.project_manager.recent_projects.clear();

        let mut next_view = None;
        let output = ctx.run(Default::default(), |ctx| {
            app.draw_home_screen(ctx, &mut next_view);
        });

        assert!(!output.shapes.is_empty());
    }
}
