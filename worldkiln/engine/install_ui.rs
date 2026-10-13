use std::f32::consts::TAU;

use super::install::SUPPORT_URL;
use crate::engine::app::{COLOR_ACCENT_ORANGE, COLOR_TEXT_BRIGHT, COLOR_TEXT_DIM, COLOR_VOID};

pub(crate) const INSTALLER_SPLASH_COMPLETE_SECS: f32 = 1.60;

const STATUS_FADE_IN_SECS: f32 = 0.25;
const STATUS_FADE_OUT_SECS: f32 = 0.25;
const ACTIVITY_FRAME_SECS: f32 = 0.18;

const ACTIVITY_FRAMES: [&str; 6] = ["", " >", " > >", " > > >", " >>>", " >>>>>"];

pub(crate) fn draw_worldkiln_splash(
    ctx: &egui::Context,
    elapsed: f32,
    show_development_build: bool,
) {
    draw_splash(ctx, elapsed, show_development_build, false);
}

pub(crate) fn draw_installer_splash(ctx: &egui::Context, elapsed: f32) {
    draw_splash(ctx, elapsed, false, true);
}

fn draw_splash(ctx: &egui::Context, elapsed: f32, show_development_build: bool, installer: bool) {
    ctx.request_repaint();

    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    painter.rect_filled(rect, 0.0, COLOR_VOID);

    let presents_start = 0.85;
    let presents_fade_in = 0.35;
    let presents_fade_out_start = 1.15;
    let presents_fade_out = 0.45;

    let mut presents_alpha = 0.0;

    if elapsed >= presents_start {
        presents_alpha = ((elapsed - presents_start) / presents_fade_in).clamp(0.0, 1.0);
    }

    if elapsed >= presents_fade_out_start {
        presents_alpha *=
            (1.0 - (elapsed - presents_fade_out_start) / presents_fade_out).clamp(0.0, 1.0);
    }

    if !installer && presents_alpha > 0.0 {
        let color = egui::Color32::from_rgba_unmultiplied(
            COLOR_TEXT_DIM.r(),
            COLOR_TEXT_DIM.g(),
            COLOR_TEXT_DIM.b(),
            (presents_alpha * 255.0) as u8,
        );

        painter.text(
            rect.center() + egui::vec2(0.0, -18.0),
            egui::Align2::CENTER_CENTER,
            "AEOWUN PRESENTS...",
            egui::FontId::monospace(60.0),
            color,
        );
    }

    // Only the installer skips the presentation beat; editor startup keeps its existing timing.
    let title_start = if installer { 0.10 } else { 1.45 };
    let stagger = 0.12;
    let reveal_duration = 0.38;

    let title = "WORLDKILN";

    let font = egui::FontId::monospace(52.0);

    let char_width = painter
        .layout_no_wrap("M".to_string(), font.clone(), COLOR_TEXT_BRIGHT)
        .size()
        .x;

    let total_width = char_width * title.chars().count() as f32;

    let start_x = rect.center().x - total_width * 0.5 + char_width * 0.5;
    let base_y = rect.center().y + 20.0;

    for (index, character) in title.chars().enumerate() {
        let char_start = title_start + index as f32 * stagger;

        let progress = ((elapsed - char_start) / reveal_duration).clamp(0.0, 1.0);

        if progress <= 0.0 {
            continue;
        }

        let eased = 1.0 - (1.0 - progress).powi(3);

        let x = start_x + index as f32 * char_width - (1.0 - eased) * 28.0;
        let y = base_y;

        let alpha = (eased * 255.0) as u8;

        let title_color = if index == 0 {
            COLOR_ACCENT_ORANGE
        } else {
            COLOR_TEXT_BRIGHT
        };

        let color = egui::Color32::from_rgba_unmultiplied(
            title_color.r(),
            title_color.g(),
            title_color.b(),
            alpha,
        );

        painter.text(
            egui::pos2(x, y),
            egui::Align2::CENTER_CENTER,
            character.to_string(),
            font.clone(),
            color,
        );

        if progress < 1.0 {
            let reaction = eased.powf(0.75);

            for dust_index in 0..20 {
                let seed = index as f32 * 31.7 + dust_index as f32 * 17.3;

                let spread_x = (seed.sin() * 7.0) + (dust_index as f32 % 5.0 - 2.0) * 1.8;

                let spread_y = (seed.cos() * 5.0) + (dust_index as f32 % 4.0 - 1.5) * 2.0;

                let backward_distance = reaction * (10.0 + dust_index as f32 * 3.4);

                let upward_distance = reaction * reaction * (8.0 + dust_index as f32 * 2.1);

                let dust_x = x - backward_distance + spread_x;
                let dust_y = y - upward_distance + spread_y;

                let dust_alpha = ((1.0 - progress) * 80.0).clamp(0.0, 255.0) as u8;

                let dust_size = 0.35 + (dust_index % 3) as f32 * 0.15;

                painter.circle_filled(
                    egui::pos2(dust_x, dust_y),
                    dust_size,
                    egui::Color32::from_rgba_unmultiplied(210, 210, 210, dust_alpha),
                );
            }
        }
    }

    if show_development_build {
        let alpha = ((elapsed - title_start) / 0.45).clamp(0.0, 1.0);
        let color = egui::Color32::from_rgba_unmultiplied(
            COLOR_ACCENT_ORANGE.r(),
            COLOR_ACCENT_ORANGE.g(),
            COLOR_ACCENT_ORANGE.b(),
            (alpha * 255.0) as u8,
        );

        painter.text(
            egui::pos2(rect.center().x, base_y + 58.0),
            egui::Align2::CENTER_CENTER,
            concat!("DEVELOPMENT BUILD V", env!("CARGO_PKG_VERSION")),
            egui::FontId::monospace(15.0),
            color,
        );
    }
}

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:17 -05:00
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum InstallerUserAction {
    Launch,
    Close,
    Retry,
    Bypass,
}

pub(crate) fn draw_installer_screen(
    ctx: &egui::Context,
    elapsed: f32,
    status: &str,
    status_age: f32,
    status_active: bool,
    fade_out_age: Option<f32>,
    show_completion_prompt: bool,
    failure_detail: Option<&str>,
) -> Option<InstallerUserAction> {
    ctx.request_repaint();

    let rect = ctx.screen_rect();
    let painter = ctx.layer_painter(egui::LayerId::background());

    painter.rect_filled(rect, 0.0, COLOR_VOID);

    let center = rect.center();
    let mut user_action = None;

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:17 -05:00
    if show_completion_prompt {
        painter.text(
            center + egui::vec2(0.0, -82.0),
            egui::Align2::CENTER_CENTER,
            "WORLDKILN IS READY",
            egui::FontId::monospace(22.0),
            COLOR_TEXT_BRIGHT,
        );

        painter.text(
            center + egui::vec2(0.0, -42.0),
            egui::Align2::CENTER_CENTER,
            "Launch Worldkiln now?",
            egui::FontId::monospace(14.0),
            COLOR_TEXT_DIM,
        );

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.style_mut().spacing.item_spacing = egui::vec2(16.0, 0.0);

                let button_rect = egui::Rect::from_center_size(
                    center + egui::vec2(0.0, 24.0),
                    egui::vec2(320.0, 42.0),
                );

                ui.allocate_ui_at_rect(button_rect, |ui| {
                    ui.horizontal(|ui| {
                        let launch_btn = egui::Button::new(
                            egui::RichText::new("LAUNCH")
                                .monospace()
                                .strong()
                                .color(COLOR_VOID)
                                .size(13.0),
                        )
                        .min_size(egui::vec2(148.0, 38.0))
                        .fill(COLOR_ACCENT_ORANGE);

                        if ui.add(launch_btn).clicked() {
                            user_action = Some(InstallerUserAction::Launch);
                        }

                        let close_btn = egui::Button::new(
                            egui::RichText::new("CLOSE")
                                .monospace()
                                .color(COLOR_TEXT_BRIGHT)
                                .size(13.0),
                        )
                        .min_size(egui::vec2(148.0, 38.0))
                        .fill(egui::Color32::from_rgb(16, 18, 22))
                        .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 50, 60)));

                        if ui.add(close_btn).clicked() {
                            user_action = Some(InstallerUserAction::Close);
                        }
                    });
                });
            });
    } else if let Some(error) = failure_detail {
        painter.text(
            center + egui::vec2(0.0, -132.0),
            egui::Align2::CENTER_CENTER,
            "WORLDKILN",
            egui::FontId::monospace(22.0),
            COLOR_TEXT_BRIGHT,
        );

        painter.text(
            center + egui::vec2(0.0, -94.0),
            egui::Align2::CENTER_CENTER,
            "INSTALL FAILED",
            egui::FontId::monospace(18.0),
            egui::Color32::from_rgb(242, 112, 102),
        );

        painter.text(
            center + egui::vec2(0.0, -68.0),
            egui::Align2::CENTER_CENTER,
            env!("CARGO_PKG_VERSION"),
            egui::FontId::monospace(11.0),
            COLOR_ACCENT_ORANGE,
        );

        let card_rect = egui::Rect::from_center_size(
            center + egui::vec2(0.0, 20.0),
            egui::vec2(rect.width() - 80.0, 148.0),
        );
        painter.rect_filled(card_rect, 8.0, egui::Color32::from_rgb(16, 19, 25));
        painter.rect_stroke(
            card_rect,
            8.0,
            egui::Stroke::new(1.0, egui::Color32::from_rgb(52, 58, 68)),
        );

        egui::CentralPanel::default()
            .frame(egui::Frame::none())
            .show(ctx, |ui| {
                ui.allocate_ui_at_rect(card_rect.shrink(18.0), |ui| {
                    ui.vertical_centered(|ui| {
                        let (message, details) = error
                            .split_once(" Details: ")
                            .map(|(message, details)| (message, Some(details)))
                            .unwrap_or((error, None));
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(message)
                                .size(15.0)
                                .color(COLOR_TEXT_BRIGHT),
                        );
                        if let Some(details) = details {
                            ui.add_space(5.0);
                            ui.collapsing("TECHNICAL DETAILS", |ui| {
                                ui.label(
                                    egui::RichText::new(details)
                                        .monospace()
                                        .size(10.0)
                                        .color(COLOR_TEXT_DIM),
                                );
                            });
                        }
                        ui.add_space(5.0);
                        ui.label(
                            egui::RichText::new(
                                "Continuing may break features that need these UserData files.",
                            )
                            .size(12.0)
                            .color(COLOR_ACCENT_ORANGE),
                        );
                    });
                });
                let actions_rect = egui::Rect::from_center_size(
                    center + egui::vec2(0.0, 138.0),
                    egui::vec2(500.0, 44.0),
                );
                ui.allocate_ui_at_rect(actions_rect, |ui| {
                    ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                        let retry = egui::Button::new(
                            egui::RichText::new("RETRY")
                                .monospace()
                                .strong()
                                .color(COLOR_VOID)
                                .size(13.0),
                        )
                        .min_size(egui::vec2(116.0, 40.0))
                        .fill(COLOR_ACCENT_ORANGE);
                        if ui.add(retry).clicked() {
                            user_action = Some(InstallerUserAction::Retry);
                        }

                        ui.add_space(10.0);
                        let bypass = egui::Button::new(
                            egui::RichText::new("BYPASS & CONTINUE")
                                .monospace()
                                .strong()
                                .color(COLOR_TEXT_BRIGHT)
                                .size(11.0),
                        )
                        .min_size(egui::vec2(176.0, 40.0))
                        .fill(egui::Color32::from_rgb(75, 50, 24))
                        .stroke(egui::Stroke::new(1.0, COLOR_ACCENT_ORANGE));
                        if ui.add(bypass).clicked() {
                            user_action = Some(InstallerUserAction::Bypass);
                        }

                        ui.add_space(10.0);
                        ui.hyperlink_to(
                            egui::RichText::new("AEOWUN SUPPORT ↗")
                                .monospace()
                                .size(10.0),
                            SUPPORT_URL,
                        );
                    });
                });
            });
    } else {
        painter.text(
            center + egui::vec2(0.0, -92.0),
            egui::Align2::CENTER_CENTER,
            "WORLDKILN",
            egui::FontId::monospace(22.0),
            COLOR_TEXT_BRIGHT,
        );

        draw_installer_spinner(&painter, center + egui::vec2(0.0, -36.0), elapsed);

        painter.text(
            center + egui::vec2(0.0, 8.0),
            egui::Align2::CENTER_CENTER,
            "INSTALLING...",
            egui::FontId::monospace(17.0),
            COLOR_TEXT_BRIGHT,
        );

        painter.text(
            center + egui::vec2(0.0, 34.0),
            egui::Align2::CENTER_CENTER,
            env!("CARGO_PKG_VERSION"),
            egui::FontId::monospace(11.0),
            COLOR_ACCENT_ORANGE,
        );
    }

    if failure_detail.is_none() && !status.is_empty() {
        let alpha = installer_status_alpha(status_age, fade_out_age);

        if alpha > 0.0 {
            let suffix = if status_active {
                installer_activity_suffix(status_age)
            } else {
                ""
            };

            let text = format!("> {status}{suffix}");

            let color = egui::Color32::from_rgba_unmultiplied(
                COLOR_TEXT_DIM.r(),
                COLOR_TEXT_DIM.g(),
                COLOR_TEXT_DIM.b(),
                (alpha * 255.0) as u8,
            );

            painter.text(
                egui::pos2(center.x, rect.bottom() - 38.0),
                egui::Align2::CENTER_CENTER,
                text,
                egui::FontId::monospace(10.0),
                color,
            );
        }
    }

    user_action
}

fn draw_installer_spinner(painter: &egui::Painter, center: egui::Pos2, elapsed: f32) {
    let radius = 17.0;
    let start_angle = elapsed * 4.5;
    let arc_length = TAU * 0.78;
    let segment_count = 28;

    let mut points = Vec::with_capacity(segment_count + 1);

    for index in 0..=segment_count {
        let progress = index as f32 / segment_count as f32;
        let angle = start_angle + arc_length * progress;

        points.push(egui::pos2(
            center.x + angle.cos() * radius,
            center.y + angle.sin() * radius,
        ));
    }

    painter.add(egui::Shape::line(
        points,
        egui::Stroke::new(3.0, COLOR_ACCENT_ORANGE),
    ));
}

pub(crate) fn installer_activity_suffix(elapsed: f32) -> &'static str {
    let frame = ((elapsed / ACTIVITY_FRAME_SECS) as usize) % ACTIVITY_FRAMES.len();

    ACTIVITY_FRAMES[frame]
}

pub(crate) fn installer_status_alpha(status_age: f32, fade_out_age: Option<f32>) -> f32 {
    if let Some(fade_age) = fade_out_age {
        return (1.0 - fade_age / STATUS_FADE_OUT_SECS).clamp(0.0, 1.0);
    }

    (status_age / STATUS_FADE_IN_SECS).clamp(0.0, 1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn installer_splash_completion_matches_title_reveal() {
        let title_start = 0.10;
        let stagger = 0.12;
        let reveal_duration = 0.38;
        let final_character_index = 8.0;

        let expected = title_start + final_character_index * stagger + reveal_duration + 0.16;

        assert!((INSTALLER_SPLASH_COMPLETE_SECS - expected).abs() < 0.001);
    }

    #[test]
    fn installer_splash_skips_presents_and_keeps_title_and_dust() {
        let ctx = egui::Context::default();
        ctx.begin_pass(egui::RawInput::default());
        draw_installer_splash(&ctx, 1.1);
        let output = ctx.end_pass();
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "W")));
        assert!(
            output
                .shapes
                .iter()
                .any(|shape| matches!(&shape.shape, egui::Shape::Circle(_)))
        );
        assert!(!output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text().contains("PRESENTS"))));

        ctx.begin_pass(egui::RawInput::default());
        draw_worldkiln_splash(&ctx, 1.1, false);
        let output = ctx.end_pass();
        assert!(output.shapes.iter().any(|shape| matches!(&shape.shape,
            egui::Shape::Text(text) if text.galley.text() == "AEOWUN PRESENTS...")));
    }

    #[test]
    fn installer_activity_suffix_advances_frames() {
        assert_eq!(installer_activity_suffix(0.00), "");
        assert_eq!(installer_activity_suffix(0.19), " >");
        assert_eq!(installer_activity_suffix(0.37), " > >");
        assert_eq!(installer_activity_suffix(0.55), " > > >");
        assert_eq!(installer_activity_suffix(0.73), " >>>");
        assert_eq!(installer_activity_suffix(0.91), " >>>>>");
    }

    #[test]
    fn installer_status_alpha_fades_in_and_out() {
        assert_eq!(installer_status_alpha(0.0, None), 0.0);
        assert_eq!(installer_status_alpha(0.25, None), 1.0);

        assert_eq!(installer_status_alpha(1.0, Some(0.0)), 1.0);
        assert_eq!(installer_status_alpha(1.0, Some(0.25)), 0.0);
    }

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
    #[test]
    fn installer_failure_screen_shows_retry_bypass_warning_and_support_link() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(720.0, 420.0));
        let draw = |ctx: &egui::Context, events: Vec<egui::Event>| {
            ctx.begin_pass(egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            });
            let action = draw_installer_screen(
                ctx,
                3.0,
                "INSTALL FAILED",
                1.0,
                false,
                None,
                false,
                Some("Could not connect to GitHub. Check your internet connection and try again."),
            );
            let output = ctx.end_pass();
            (action, output)
        };

        let (_, output) = draw(&ctx, Vec::new());
        let text: Vec<_> = output
            .shapes
            .iter()
            .filter_map(|shape| match &shape.shape {
                egui::Shape::Text(text) => Some(text.galley.text().to_string()),
                _ => None,
            })
            .collect();

        assert!(text.iter().any(|text| text.contains("RETRY")));
        assert!(text.iter().any(|text| text.contains("BYPASS & CONTINUE")));
        assert!(text.iter().any(|text| text.contains("may break features")));
        assert!(text.iter().any(|text| text.contains("AEOWUN SUPPORT")));

        let retry_label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains("RETRY") => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("Retry button label should be rendered");

        let (action, _) = draw(
            &ctx,
            vec![
                egui::Event::PointerMoved(retry_label),
                egui::Event::PointerButton {
                    pos: retry_label,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(action, None);

        let (action, _) = draw(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: retry_label,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(action, Some(InstallerUserAction::Retry));

        let (_, output) = draw(&ctx, Vec::new());
        let bypass_label = output
            .shapes
            .iter()
            .find_map(|shape| match &shape.shape {
                egui::Shape::Text(text) if text.galley.text().contains("BYPASS & CONTINUE") => {
                    Some(text.pos + text.galley.size() * 0.5)
                }
                _ => None,
            })
            .expect("Bypass button label should be rendered");
        let (action, _) = draw(
            &ctx,
            vec![
                egui::Event::PointerMoved(bypass_label),
                egui::Event::PointerButton {
                    pos: bypass_label,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers: egui::Modifiers::NONE,
                },
            ],
        );
        assert_eq!(action, None);
        let (action, _) = draw(
            &ctx,
            vec![egui::Event::PointerButton {
                pos: bypass_label,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers: egui::Modifiers::NONE,
            }],
        );
        assert_eq!(action, Some(InstallerUserAction::Bypass));
    }
}
