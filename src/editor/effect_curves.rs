// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::project::fx_authoring::{CurveChannel, CurveKey, EffectNode};
#[derive(Clone, Debug)]
pub struct CurveUiState {
    pub channel: CurveChannel,
    pub zoom: f32,
    pub scroll: f32,
    pub selected_key: Option<usize>,
}
impl Default for CurveUiState {
    fn default() -> Self {
        Self {
            channel: CurveChannel::Size,
            zoom: 1.0,
            scroll: 0.0,
            selected_key: None,
        }
    }
}
pub struct CurveChange {
    pub channel: CurveChannel,
    pub keys: Vec<CurveKey>,
}
fn range(channel: CurveChannel) -> f32 {
    match channel {
        CurveChannel::Size => 10.0,
        CurveChannel::Opacity | CurveChannel::Red | CurveChannel::Green | CurveChannel::Blue => 1.0,
    }
}
fn editable_keys(node: &EffectNode, channel: CurveChannel) -> Vec<CurveKey> {
    let keys = match channel {
        CurveChannel::Size => &node.size,
        CurveChannel::Opacity => &node.opacity,
        CurveChannel::Red => &node.color_r,
        CurveChannel::Green => &node.color_g,
        CurveChannel::Blue => &node.color_b,
    };
    if keys.is_empty() {
        match channel {
            CurveChannel::Size => vec![
                CurveKey {
                    time: 0.0,
                    value: 1.0,
                },
                CurveKey {
                    time: 1.0,
                    value: 1.0,
                },
            ],
            CurveChannel::Red
            | CurveChannel::Green
            | CurveChannel::Blue
            | CurveChannel::Opacity => vec![
                CurveKey {
                    time: 0.0,
                    value: 1.0,
                },
                CurveKey {
                    time: 1.0,
                    value: 1.0,
                },
            ],
        }
    } else {
        keys.clone()
    }
}
pub fn draw(ui: &mut egui::Ui, node: &EffectNode, state: &mut CurveUiState) -> Option<CurveChange> {
    ui.horizontal(|ui| {
        ui.selectable_value(&mut state.channel, CurveChannel::Size, "Size");
        ui.selectable_value(&mut state.channel, CurveChannel::Opacity, "Opacity");
        ui.selectable_value(&mut state.channel, CurveChannel::Red, "R");
        ui.selectable_value(&mut state.channel, CurveChannel::Green, "G");
        ui.selectable_value(&mut state.channel, CurveChannel::Blue, "B");
        ui.separator();
        ui.label("Zoom");
        ui.add(egui::Slider::new(&mut state.zoom, 1.0..=12.0).show_value(false));
        let visible = 1.0 / state.zoom;
        state.scroll = state.scroll.clamp(0.0, 1.0 - visible);
        ui.add(egui::Slider::new(&mut state.scroll, 0.0..=(1.0 - visible)).show_value(false));
    });
    let mut keys = editable_keys(node, state.channel);
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().max(120.0), 105.0),
        egui::Sense::click_and_drag(),
    );
    let paint = ui.painter_at(rect);
    let color = ui.visuals().selection.stroke.color;
    paint.rect_stroke(rect, 4.0, ui.visuals().widgets.noninteractive.bg_stroke);
    let value_max = range(state.channel);
    let start = state.scroll;
    let end = start + 1.0 / state.zoom;
    let to_point = |key: &CurveKey| {
        egui::pos2(
            rect.left() + ((key.time - start) / (end - start)) * rect.width(),
            rect.bottom() - (key.value / value_max).clamp(0.0, 1.0) * rect.height(),
        )
    };
    let to_key = |p: egui::Pos2| CurveKey {
        time: (start + ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0) * (end - start))
            .clamp(0.0, 1.0),
        value: (((rect.bottom() - p.y) / rect.height()).clamp(0.0, 1.0) * value_max),
    };
    for pair in keys.windows(2) {
        paint.line_segment(
            [to_point(&pair[0]), to_point(&pair[1])],
            egui::Stroke::new(1.8, color),
        );
    }
    for (index, key) in keys.iter().enumerate() {
        if key.time >= start && key.time <= end {
            let point = to_point(key);
            paint.circle_filled(
                point,
                if state.selected_key == Some(index) {
                    5.5
                } else {
                    4.0
                },
                color,
            );
        }
    }
    if response.drag_started() || response.clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            state.selected_key = keys
                .iter()
                .enumerate()
                .filter(|(_, key)| key.time >= start && key.time <= end)
                .map(|(i, key)| (i, to_point(key).distance(pointer)))
                .filter(|(_, dist)| *dist <= 15.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
        }
    }
    if response.dragged() {
        if let (Some(index), Some(pointer)) = (state.selected_key, response.interact_pointer_pos())
        {
            if index < keys.len() {
                let mut key = to_key(pointer);
                let lower = if index > 0 {
                    keys[index - 1].time + 0.0001
                } else {
                    0.0
                };
                let upper = if index + 1 < keys.len() {
                    keys[index + 1].time - 0.0001
                } else {
                    1.0
                };
                if lower <= upper {
                    key.time = key.time.clamp(lower, upper);
                    if keys[index] != key {
                        keys[index] = key;
                        return Some(CurveChange {
                            channel: state.channel,
                            keys,
                        });
                    }
                }
            }
        }
    }
    if response.double_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            if keys.len() < 256 {
                let key = to_key(pointer);
                if !keys
                    .iter()
                    .any(|existing| (existing.time - key.time).abs() < 0.0001)
                {
                    keys.push(key.clone());
                    keys.sort_by(|a, b| a.time.total_cmp(&b.time));
                    state.selected_key = keys.iter().position(|k| *k == key);
                    return Some(CurveChange {
                        channel: state.channel,
                        keys,
                    });
                }
            }
        }
    }
    if response.secondary_clicked() {
        if let Some(pointer) = response.interact_pointer_pos() {
            let target = keys
                .iter()
                .enumerate()
                .map(|(i, key)| (i, to_point(key).distance(pointer)))
                .filter(|(_, dist)| *dist <= 15.0)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(i, _)| i);
            if let Some(index) = target {
                if keys.len() > 2 {
                    keys.remove(index);
                    state.selected_key = None;
                    return Some(CurveChange {
                        channel: state.channel,
                        keys,
                    });
                }
            }
        }
    }
    None
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn default_key_edit_is_independent_from_node() {
        let node = EffectNode::new(1, "Fire");
        let a = editable_keys(&node, CurveChannel::Size);
        let b = editable_keys(&node, CurveChannel::Opacity);
        assert_eq!(a.len(), 2);
        assert_eq!(b.len(), 2);
        assert!(node.size.is_empty() && node.opacity.is_empty());
    }
}
