// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::engine::ui::{RuntimeUi, UiElement};
use std::path::Path;

struct DrawItem {
    id: u32,
    element: UiElement,
    rect: egui::Rect,
    enabled: bool,
}

fn rgba(value: [f32; 4]) -> egui::Color32 {
    egui::Color32::from_rgba_unmultiplied(
        (value[0] * 255.0).clamp(0.0, 255.0) as u8,
        (value[1] * 255.0).clamp(0.0, 255.0) as u8,
        (value[2] * 255.0).clamp(0.0, 255.0) as u8,
        (value[3] * 255.0).clamp(0.0, 255.0) as u8,
    )
}

fn button_under_pointer(items: &[DrawItem], pointer: egui::Pos2) -> Option<u32> {
    for item in items.iter().rev() {
        if !item.rect.contains(pointer) {
            continue;
        }
        match &item.element {
            UiElement::Button(button) => {
                if button.common.background_color[3] > 0.001 || button.common.color[3] > 0.001 {
                    return item.enabled.then_some(item.id);
                }
            }
            UiElement::Panel(panel) if panel.color[3] > 0.001 => return None,
            UiElement::Image(media) | UiElement::Video(media) if media.common.color[3] > 0.001 => {
                return None;
            }
            _ => {}
        }
    }
    None
}

pub fn paint(
    ctx: &egui::Context,
    runtime: &mut RuntimeUi,
    viewport: egui::Rect,
    ui_scale: f32,
    project_path: Option<&Path>,
) {
    if viewport.width() <= 1.0 || viewport.height() <= 1.0 {
        return;
    }
    let scale = ui_scale.max(0.01);
    let viewport_px = [viewport.width() * scale, viewport.height() * scale];
    let mut items = Vec::new();

    for id in runtime.paint_order(viewport_px) {
        let Some(element) = runtime.elements.get(&id).cloned() else {
            continue;
        };
        let Ok(layout) = runtime.resolve_layout_in(id as u64, viewport_px) else {
            continue;
        };
        if !layout.visible {
            continue;
        }
        let position = egui::pos2(
            viewport.min.x + layout.position[0] / scale,
            viewport.min.y + layout.position[1] / scale,
        );
        let size = egui::vec2(layout.size[0] / scale, layout.size[1] / scale);
        items.push(DrawItem {
            id,
            element,
            rect: egui::Rect::from_min_size(position, size),
            enabled: layout.enabled,
        });
    }

    let clickable = ctx
        .input(|input| input.pointer.interact_pos())
        .and_then(|pointer| button_under_pointer(&items, pointer));
    let mut clicks = Vec::new();
    egui::Area::new(egui::Id::new("worldkiln_runtime_ui_canvas"))
        .fixed_pos(viewport.min)
        .order(egui::Order::Foreground)
        .movable(false)
        .interactable(clickable.is_some())
        .show(ctx, |ui| {
            ui.set_min_size(viewport.size());
            ui.set_clip_rect(viewport);
            for item in &items {
                let rect = item.rect;
                let common = item.element.common();
                let color = rgba(common.color);
                let border = rgba(common.border_color);
                match &item.element {
                    UiElement::Image(media) | UiElement::Video(media) => {
                        if matches!(&item.element, UiElement::Video(_)) {
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
                                project_path,
                            );
                        } else {
                            crate::engine::ui_media::paint_media_frame(
                                ui.painter(),
                                ctx,
                                rect,
                                &media.source,
                                media.fit,
                                media.common.color,
                                project_path,
                            );
                        }
                    }
                    UiElement::Panel(panel) => {
                        ui.painter()
                            .rect_filled(rect, panel.corner_radius / scale, color);
                        if panel.border_width > 0.0 {
                            ui.painter().rect_stroke(
                                rect,
                                panel.corner_radius / scale,
                                egui::Stroke::new(panel.border_width / scale, border),
                            );
                        }
                    }
                    UiElement::Text(text) => {
                        crate::engine::ui::paint_ui_text_styled(
                            ui.painter(),
                            ctx,
                            rect,
                            &text.text,
                            text.font_size / scale,
                            text.text_scaled,
                            text.horizontal_align,
                            text.vertical_align,
                            text.wrap,
                            [
                                text.padding[0] / scale,
                                text.padding[1] / scale,
                                text.padding[2] / scale,
                                text.padding[3] / scale,
                            ],
                            text.line_spacing / scale,
                            color,
                            text.font_family,
                            text.bold,
                            text.italic,
                            text.rich_text,
                        );
                    }
                    UiElement::Button(button) => {
                        let response = ui.interact(
                            rect,
                            ui.id().with(("runtime_button", item.id)),
                            if clickable == Some(item.id) {
                                egui::Sense::click()
                            } else {
                                egui::Sense::hover()
                            },
                        );
                        ui.painter().rect_filled(
                            rect,
                            button.common.corner_radius / scale,
                            rgba(button.common.background_color),
                        );
                        if button.common.border_width > 0.0 {
                            ui.painter().rect_stroke(
                                rect,
                                button.common.corner_radius / scale,
                                egui::Stroke::new(button.common.border_width / scale, border),
                            );
                        }
                        crate::engine::ui::paint_ui_text_styled(
                            ui.painter(),
                            ctx,
                            rect,
                            &button.text,
                            button.font_size / scale,
                            false,
                            button.horizontal_align,
                            button.vertical_align,
                            button.wrap,
                            [
                                button.padding[0] / scale,
                                button.padding[1] / scale,
                                button.padding[2] / scale,
                                button.padding[3] / scale,
                            ],
                            button.line_spacing / scale,
                            color,
                            button.font_family,
                            button.bold,
                            button.italic,
                            button.rich_text,
                        );
                        if response.clicked() && item.enabled && clickable == Some(item.id) {
                            clicks.push(item.id);
                        }
                    }
                }
            }
        });

    runtime.pending_clicks.extend(clicks);
    #[cfg(windows)]
    crate::engine::ui_media::finish_video_ui_frame();
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::ui::UiZIndexBehavior;
    use crate::scripting::value::Value;

    #[test]
    fn decorative_fullscreen_ui_does_not_capture_gameplay_mouse() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let mut runtime = RuntimeUi::new();
        let _hud = runtime.allocate("Panel").unwrap();
        for _ in 0..3 {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(viewport),
                    events: vec![egui::Event::PointerMoved(egui::pos2(400.0, 300.0))],
                    ..Default::default()
                },
                |ctx| {
                    paint(ctx, &mut runtime, viewport, 1.0, None);
                },
            );
        }
        assert!(
            !ctx.wants_pointer_input(),
            "A decorative fullscreen UI canvas must not intercept sword clicks"
        );
    }

    #[test]
    fn overlay_blocks_clicks_and_repeated_play_is_independent_of_creation_order() {
        for run in 0..3 {
            let mut ui = RuntimeUi::new();
            ui.z_index_behavior = UiZIndexBehavior::Sibling;
            let menu = ui.allocate("Panel").unwrap();
            let button = ui.allocate("Button").unwrap();
            let black = ui.allocate("Panel").unwrap();
            ui.set_property(button, "parent", Value::Number(menu as f64))
                .unwrap();
            ui.set_property(menu, "z_index", Value::Number(300.0))
                .unwrap();
            ui.set_property(black, "z_index", Value::Number(200.0))
                .unwrap();
            let front = ui.paint_order([100.0, 100.0]);
            assert_eq!(
                front,
                vec![black as u32, menu as u32, button as u32],
                "run {run}"
            );
            let items = front
                .into_iter()
                .map(|id| DrawItem {
                    id,
                    element: ui.elements[&id].clone(),
                    rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0)),
                    enabled: true,
                })
                .collect::<Vec<_>>();
            assert_eq!(
                button_under_pointer(&items, egui::pos2(10.0, 10.0)),
                Some(button as u32)
            );
            ui.set_property(black, "z_index", Value::Number(1000.0))
                .unwrap();
            let items = ui
                .paint_order([100.0, 100.0])
                .into_iter()
                .map(|id| DrawItem {
                    id,
                    element: ui.elements[&id].clone(),
                    rect: egui::Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(100.0, 50.0)),
                    enabled: true,
                })
                .collect::<Vec<_>>();
            assert_eq!(button_under_pointer(&items, egui::pos2(10.0, 10.0)), None);
        }
    }

    #[test]
    fn draw_order_is_stable_across_egui_context_reuse() {
        let ctx = egui::Context::default();
        let viewport = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(800.0, 600.0));
        let input = || egui::RawInput {
            screen_rect: Some(viewport),
            ..Default::default()
        };
        for run in 0..3 {
            let mut ui = RuntimeUi::new();
            ui.z_index_behavior = UiZIndexBehavior::Global;
            let menu = ui.allocate("Panel").unwrap();
            let black = ui.allocate("Panel").unwrap();
            ui.set_property(menu, "z_index", Value::Number(300.0))
                .unwrap();
            ui.set_property(black, "z_index", Value::Number(200.0))
                .unwrap();
            ui.elements
                .get_mut(&(menu as u32))
                .unwrap()
                .common_mut()
                .color = [1.0, 0.0, 0.0, 1.0];
            ui.elements
                .get_mut(&(black as u32))
                .unwrap()
                .common_mut()
                .color = [0.0, 0.0, 0.0, 1.0];
            ui.elements
                .get_mut(&(menu as u32))
                .unwrap()
                .common_mut()
                .border_width = 0.0;
            ui.elements
                .get_mut(&(black as u32))
                .unwrap()
                .common_mut()
                .border_width = 0.0;
            ctx.run(input(), |ctx| paint(ctx, &mut ui, viewport, 1.0, None));
            let output = ctx.run(input(), |ctx| {
                paint(ctx, &mut ui, viewport, 1.0, None);
            });
            assert!(!output.shapes.is_empty(), "no draw output on play {run}");
            assert_eq!(
                ui.paint_order([800.0, 600.0]),
                vec![black as u32, menu as u32],
                "Z ordering changed on play session {run}"
            );
            ctx.run(input(), |_ctx| {});
        }
    }
}
