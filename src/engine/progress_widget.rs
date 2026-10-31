// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
pub(crate) fn draw_progress(
    ctx: &egui::Context,
    id: &str,
    name: &str,
    ratio: Option<f32>,
    detail: &str,
) {
    egui::Window::new("Import progress")
        .id(egui::Id::new(id))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
        .fixed_size(egui::vec2(252.0, 66.0))
        .show(ctx, |ui| {
            ui.label(egui::RichText::new(name).strong());
            ui.horizontal(|ui| {
                ui.spinner();
                if let Some(ratio) = ratio {
                    ui.add(egui::ProgressBar::new(ratio.clamp(0.0, 1.0)).show_percentage());
                } else {
                    let seconds = ui.ctx().input(|input| input.time);
                    let pulse = 0.18 + 0.10 * ((seconds * 4.0) as f32).sin().abs();
                    ui.add(egui::ProgressBar::new(pulse).desired_width(180.0));
                }
            });
            if !detail.is_empty() {
                ui.small(detail);
            }
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(75));
}

pub(crate) fn draw_cancellable_progress(
    ctx: &egui::Context,
    id: &str,
    name: &str,
    ratio: Option<f32>,
    detail: &str,
) -> bool {
    let mut cancel = false;
    egui::Window::new("Import progress")
        .id(egui::Id::new(id))
        .title_bar(false)
        .resizable(false)
        .collapsible(false)
        .order(egui::Order::Foreground)
        .anchor(egui::Align2::RIGHT_BOTTOM, egui::vec2(-18.0, -18.0))
        .fixed_size(egui::vec2(340.0, 92.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(name).strong());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Cancel Download").clicked() {
                        cancel = true;
                    }
                });
            });
            ui.horizontal(|ui| {
                ui.spinner();
                if let Some(ratio) = ratio {
                    ui.add(egui::ProgressBar::new(ratio.clamp(0.0, 1.0)).show_percentage());
                } else {
                    ui.add(egui::ProgressBar::new(0.0).desired_width(180.0));
                }
            });
            ui.add(egui::Label::new(detail).truncate());
        });
    ctx.request_repaint_after(std::time::Duration::from_millis(75));
    cancel
}
