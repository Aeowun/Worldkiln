// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{DevelopmentTestRunAction as Action, DevelopmentTestRunUi, Editor};
use crate::engine::development_capture::{Capture, Sample, Scope, Selection, Snapshot};
use crate::engine::development_test_run::CaptureState;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum View {
    Overview,
    Timeline,
    Scripts,
    Rendering,
    Resources,
    Findings,
}
impl View {
    const ALL: [Self; 6] = [
        Self::Overview,
        Self::Timeline,
        Self::Scripts,
        Self::Rendering,
        Self::Resources,
        Self::Findings,
    ];
    fn label(self) -> &'static str {
        match self {
            Self::Overview => "Overview",
            Self::Timeline => "Timeline",
            Self::Scripts => "Scripts",
            Self::Rendering => "Rendering",
            Self::Resources => "Resources",
            Self::Findings => "Findings",
        }
    }
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub tabs: Vec<View>,
    pub active: Option<View>,
    pub cards: [bool; 4],
    pub columns: [bool; 4],
    pub recent: bool,
    pub details: bool,
    pub graph_height: f32,
    #[serde(skip)]
    pub search: String,
}
impl Default for Layout {
    fn default() -> Self {
        Self {
            tabs: vec![View::Overview],
            active: Some(View::Overview),
            cards: [true; 4],
            columns: [false; 4],
            recent: false,
            details: false,
            graph_height: 75.0,
            search: String::new(),
        }
    }
}
impl Layout {
    pub fn normalize(&mut self) {
        let mut seen = Vec::new();
        self.tabs.retain(|v| {
            if seen.contains(v) {
                false
            } else {
                seen.push(*v);
                true
            }
        });
        if self.active.is_some_and(|v| !self.tabs.contains(&v)) {
            self.active = self.tabs.first().copied();
        }
        if !self.graph_height.is_finite() {
            self.graph_height = 75.0;
        }
        self.graph_height = self.graph_height.clamp(35.0, 240.0);
    }
    fn open(&mut self, v: View) {
        if !self.tabs.contains(&v) {
            self.tabs.push(v);
        }
        self.active = Some(v);
    }
    fn close(&mut self, v: View) {
        self.tabs.retain(|t| *t != v);
        if self.active == Some(v) {
            self.active = self.tabs.first().copied();
        }
    }
    fn move_before(&mut self, from: View, to: View) {
        if from == to {
            return;
        }
        self.tabs.retain(|v| *v != from);
        if let Some(i) = self.tabs.iter().position(|v| *v == to) {
            self.tabs.insert(i, from);
        }
    }
}
#[derive(Default)]
pub struct Investigation {
    pub selection: Selection,
    pub snapshot: Snapshot,
    pub saved: Option<std::sync::Arc<Capture>>,
    pub comparison: Option<std::sync::Arc<Capture>>,
    pub selected: Option<Sample>,
    pub refresh: bool,
}

pub(super) fn controls(ui: &mut egui::Ui, state: &mut DevelopmentTestRunUi) {
    let status = state
        .summary
        .as_ref()
        .map_or(CaptureState::Ready, |s| s.state);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!("Status: {status:?}"));
        for (label, action, enabled) in [
            (
                "Start",
                Action::Start,
                status != CaptureState::Running && !state.saving,
            ),
            ("Pause", Action::Pause, status == CaptureState::Running),
            (
                "Stop",
                Action::Stop,
                matches!(status, CaptureState::Running | CaptureState::Paused),
            ),
            (
                "Save",
                Action::Save,
                status == CaptureState::Stopped && !state.saving && state.has_project,
            ),
            (
                "Export",
                Action::Export,
                status == CaptureState::Stopped && !state.saving && state.has_project,
            ),
        ] {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                state.action = Some(action);
            }
        }
        if state.saving {
            ui.spinner();
        }
    });
    if status == CaptureState::Paused {
        ui.small("Display paused; recording continues. Start resumes the live display.");
    }
    if state.confirm_replace {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(ui.visuals().warn_fg_color, "Replace unsaved recording?");
            if ui.button("Discard and Start").clicked() {
                state.action = Some(Action::DiscardAndStart);
            }
            if ui.button("Cancel").clicked() {
                state.action = Some(Action::CancelDiscard);
            }
        });
    }
    if let Some(message) = &state.message {
        ui.small(message);
    }
    if matches!(status, CaptureState::Running | CaptureState::Paused) {
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(250));
    }
}

impl Editor {
    pub(super) fn draw_diagnostics_workspace(&mut self, ui: &mut egui::Ui) {
        controls(ui, &mut self.development_test_run_ui);
        let layout = &mut self.shell.diagnostics;
        let state = &mut self.development_test_run_ui;
        let investigation = &mut state.investigation;
        let mut changed = false;
        ui.horizontal_wrapped(|ui| {
            let mut close = None;
            let mut move_tab = None;
            for view in layout.tabs.clone() {
                let response = ui.add(
                    egui::Button::new(view.label())
                        .selected(layout.active == Some(view))
                        .sense(egui::Sense::click_and_drag()),
                );
                if response.clicked() {
                    layout.active = Some(view);
                    changed = true;
                }
                if response.drag_started() {
                    response.dnd_set_drag_payload(view);
                }
                if let Some(from) = response.dnd_release_payload::<View>() {
                    move_tab = Some((*from, view));
                }
                if ui
                    .small_button("×")
                    .on_hover_text(format!("Close {}", view.label()))
                    .clicked()
                {
                    close = Some(view);
                }
            }
            if let Some(view) = close {
                layout.close(view);
                changed = true;
            }
            if let Some((from, to)) = move_tab {
                layout.move_before(from, to);
                changed = true;
            }
            ui.menu_button("+", |ui| {
                ui.text_edit_singleline(&mut layout.search);
                for view in View::ALL {
                    if view
                        .label()
                        .to_lowercase()
                        .contains(&layout.search.to_lowercase())
                        && ui.button(view.label()).clicked()
                    {
                        layout.open(view);
                        changed = true;
                        ui.close_menu();
                    }
                }
            });
            if ui.button("Reset Layout").clicked() {
                *layout = Layout::default();
                changed = true;
            }
        });
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt("diagnostic_scope")
                .selected_text(investigation.selection.scope.label())
                .show_ui(ui, |ui| {
                    for scope in Scope::ALL {
                        if ui
                            .selectable_value(
                                &mut investigation.selection.scope,
                                scope,
                                scope.label(),
                            )
                            .changed()
                        {
                            investigation.refresh = true;
                        }
                    }
                });
            if investigation.selection.scope == Scope::SelectedFrame {
                if let Some(frame) = investigation.selection.first {
                    ui.label(format!("Frame {frame}"));
                }
            }
            if investigation.selection.scope == Scope::SelectedRange {
                let mut first = investigation
                    .selection
                    .first
                    .unwrap_or(investigation.snapshot.latest);
                let mut last = investigation.selection.last.unwrap_or(first);
                ui.label("Frames");
                if ui
                    .add(egui::DragValue::new(&mut first).range(1..=u64::MAX))
                    .changed()
                {
                    investigation.selection.first = Some(first);
                    investigation.refresh = true;
                }
                if ui
                    .add(egui::DragValue::new(&mut last).range(1..=u64::MAX))
                    .changed()
                {
                    investigation.selection.last = Some(last);
                    investigation.refresh = true;
                }
            }
            if ui.button("Open Capture").clicked() {
                state.action = Some(Action::Open);
            }
            if ui.button("Compare").clicked() {
                state.action = Some(Action::Compare);
            }
            if investigation.saved.is_some() {
                ui.label("Saved capture");
                if ui.button("Live").clicked() {
                    investigation.saved = None;
                    investigation.refresh = true;
                }
            }
            ui.menu_button("Customize", |ui| {
                for (i, label) in ["Frame time / FPS", "CPU", "GPU", "Scripts"]
                    .into_iter()
                    .enumerate()
                {
                    changed |= ui.checkbox(&mut layout.cards[i], label).changed();
                }
                ui.separator();
                for (i, label) in ["Self time", "Worst callback", "Entity", "Source location"]
                    .into_iter()
                    .enumerate()
                {
                    changed |= ui.checkbox(&mut layout.columns[i], label).changed();
                }
            });
        });
        if state.loading {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("Loading capture");
            });
        }
        if investigation.snapshot.dropped > 0 {
            ui.small(format!(
                "Dropped / evicted samples: {}",
                investigation.snapshot.dropped
            ));
        }
        let active = layout.active;
        let compact = ui.available_height() < 160.0;
        egui::ScrollArea::vertical().id_salt("diagnostics_workspace_scroll").auto_shrink([false,false]).show(ui,|ui| {
            if active==Some(View::Overview) {
                ui.small(&investigation.snapshot.aggregation);
                if compact {
                    ui.horizontal_wrapped(|ui| {
                        for (i,label,view) in [(0,"Frame time / FPS",View::Timeline),(1,"CPU",View::Timeline),(2,"GPU",View::Rendering),(3,"Scripts",View::Scripts)] {
                            if !layout.cards[i]{continue;}
                            let value=investigation.snapshot.current[i].map_or("Unavailable".into(),|v|format!("{v:.2} ms"));
                            if ui.small_button(format!("{label}: {value}")).clicked(){layout.open(view);changed=true;}
                        }
                    });
                } else {

                ui.horizontal_wrapped(|ui| {
                    for (i,label,view) in [(0,"Frame time / FPS",View::Timeline),(1,"CPU",View::Timeline),(2,"GPU",View::Rendering),(3,"Scripts",View::Scripts)] {
                        if !layout.cards[i]{continue;}
                        egui::Frame::group(ui.style()).show(ui,|ui| {
                            ui.set_min_width(130.0);
                            let enabled=match i {1=>state.config.cpu,2=>state.config.gpu,3=>state.config.scripts,_=>true};
                            let value=investigation.snapshot.current[i].map_or_else(||if !enabled {"Not captured".into()}else{"Unavailable".into()},|v| if i==0 && v>0.0 {format!("{v:.2} ms / {:.1} FPS",1000.0/v)} else {format!("{v:.2} ms")});
                            if ui.button(label).clicked(){layout.open(view);changed=true;}
                            ui.label(value).on_hover_text(if i==1 {"Current render CPU wall time; excludes update and waiting"}else{"Current completed frame; trend uses retained frames"});
                            sparkline(ui,&investigation.snapshot.trends[i],egui::vec2(130.0,20.0));
                        });
                    }
                });
                ui.label("Frame time (ms) — select a spike; Shift-click extends a range");
                frame_graph(ui,&investigation.snapshot.frames,&mut investigation.selection,&mut investigation.refresh,layout.graph_height);
                let (grip,response)=ui.allocate_exact_size(egui::vec2(ui.available_width(),5.0),egui::Sense::drag());
                ui.painter().line_segment([grip.left_center(),grip.right_center()],ui.visuals().widgets.inactive.bg_stroke);
                if response.dragged(){layout.graph_height=(layout.graph_height+ui.input(|i|i.pointer.delta().y)).clamp(35.0,240.0);}
                if response.drag_stopped(){changed=true;}
                let recent=egui::CollapsingHeader::new("Recent Events").open(Some(layout.recent)).show(ui,|ui| {
                    for sample in investigation.snapshot.timeline.iter().rev().filter(|s|s.duration_ns>=20_000_000).take(12) {
                        if ui.selectable_label(false,format!("Frame {} · {} · {:.2} ms",sample.frame,sample.operation,sample.duration_ns as f64/1e6)).clicked() {investigation.selected=Some(sample.clone());investigation.refresh=true;}
                    }
                    if investigation.snapshot.timeline.is_empty(){ui.weak("Not captured");}
                });
                if recent.header_response.clicked(){layout.recent=!layout.recent;changed=true;}
                }
            } else if active==Some(View::Timeline) {
                ui.small("Measured CPU threads and GPU passes; clocks are shown separately. Times in ms.");
                egui::Grid::new("diagnostics_timeline").striped(true).show(ui,|ui| {
                    for label in ["Frame","Thread","Operation","Start ms","Duration ms"] {ui.strong(label);}ui.end_row();
                    for s in &investigation.snapshot.timeline {
                        ui.label(format!("Frame {}",s.frame));ui.label(&s.thread);
                        if ui.selectable_label(investigation.selected.as_ref().is_some_and(|v|v.id==s.id),&s.operation).clicked(){investigation.selected=Some(s.clone());investigation.refresh=true;}
                        ui.label(format!("{:.3} ms",s.at_ns as f64/1e6));ui.label(format!("{:.3} ms",s.duration_ns as f64/1e6));ui.end_row();
                    }
                });
            } else if active==Some(View::Findings) {
                ui.strong("Hitches");
                for hitch in &investigation.snapshot.hitches {
                    if ui.button(format!("Frame {} — {:.2} ms",hitch.frame,hitch.duration_ns as f64/1e6)).clicked(){
                        investigation.selection=Selection{scope:Scope::SelectedFrame,first:Some(hitch.frame),last:Some(hitch.frame)};
                        investigation.selected=Some(hitch.clone());investigation.refresh=true;layout.open(View::Timeline);changed=true;
                    }
                }
                ui.strong("Measured expensive operations");
                for r in investigation.snapshot.rows.iter().take(8) {ui.label(format!("{} — total {:.2} ms; worst {:.2} ms",r.label,r.total_ns as f64/1e6,r.worst_ns as f64/1e6));}
                ui.strong("Suspected contributors");ui.small("Asset presence, size and triangle counts do not establish causation.");
                ui.strong("Controlled comparison");
                if let Some(other)=&investigation.comparison {
                    if let Some(current)=&investigation.saved {
                        let mismatches=current.mismatches(other);
                        if mismatches.is_empty(){ui.label("Recorded conditions match; repeat the same workload before confirming causation.");}
                        for mismatch in mismatches {ui.label(mismatch);}
                        let stats=|c:&Capture| {let mut frames:Vec<_>=c.samples.iter().filter(|s|s.category=="render"&&s.operation=="frame").map(|s|s.duration_ns).collect();frames.sort_unstable();frames.get(frames.len().saturating_sub(1)*95/100).copied()};
                        ui.label(format!("Frame P95: current {} · comparison {}",ms(stats(current)),ms(stats(other))));
                    } else {ui.label("Open a saved capture to compare retained runs.");}
                } else {ui.label("No controlled comparison captured");}
                for warning in &investigation.snapshot.warnings {ui.small(warning);}
            } else if let Some(view)=active {
                if view==View::Resources {ui.small("Memory: Unavailable (allocator totals are not captured)");}
                ui.small("Retained samples in selected scope. Total and average are inclusive wall time (ms); nested rows overlap.");
                egui::ScrollArea::horizontal().id_salt("diagnostics_columns").show(ui,|ui| {
                    egui::Grid::new("diagnostic_ranked_rows").striped(true).show(ui,|ui| {
                        ui.strong("Operation");ui.strong("Calls");ui.strong("Total ms");ui.strong("Average ms");
                        for (i,label) in ["Self ms","Worst ms","Entity","Source"].into_iter().enumerate(){if layout.columns[i]{ui.strong(label);}}ui.end_row();
                        let mut count=0;
                        for r in &investigation.snapshot.rows {
                            let include=match view {View::Scripts=>matches!(r.category.as_str(),"callback"|"native"|"navigation"|"script"),View::Rendering=>matches!(r.category.as_str(),"render"|"gpu"|"animation"|"fx"),View::Resources=>matches!(r.category.as_str(),"resources"|"asset"|"filesystem"|"autosave"),_=>true};
                            if !include{continue;}count+=1;
                            if ui.selectable_label(false,&r.label).clicked(){investigation.selected=r.sample.clone();investigation.refresh=true;layout.details=true;changed=true;}
                            ui.label(r.calls.to_string());ui.label(ms(Some(r.total_ns)));ui.label(ms(Some(r.total_ns/r.calls.max(1))));
                            if layout.columns[0]{ui.label(ms(r.self_ns));}if layout.columns[1]{ui.label(ms(Some(r.worst_ns)));}
                            if layout.columns[2]{ui.label(r.sample.as_ref().and_then(|s|s.entity.as_deref()).unwrap_or("Unattributed"));}
                            if layout.columns[3]{ui.label(r.sample.as_ref().and_then(|s|s.source.as_deref()).unwrap_or("Not captured"));}ui.end_row();
                        }
                        if count==0 {ui.weak("Not captured in this scope");ui.end_row();}
                    });
                });
            } else {ui.weak("Open an investigation view with +");}
            let details=egui::CollapsingHeader::new("Details").open(Some(layout.details)).show(ui,|ui| {
                if let Some(s)=investigation.selected.clone() {
                    ui.label(format!("Frame {} · {} · {}",s.frame,s.thread,s.operation));
                    ui.label(format!("Total {} · Self {}",ms(Some(s.duration_ns)),ms(s.self_ns)));
                    ui.label(format!("Script: {} · Entity: {}",s.script.as_deref().unwrap_or("Unattributed"),s.entity.as_deref().unwrap_or("Unattributed")));
                    ui.label(format!("Source: {}",s.source.as_deref().unwrap_or("Not captured")));
                    if let Some(asset)=&s.asset {ui.label(format!("Asset: {asset}"));}
                    if let Some(parent)=s.cause.or(s.parent) {
                        if let Some(caller)=investigation.snapshot.timeline.iter().find(|v|v.id==parent) {
                            if ui.button(format!("{}: {}",if s.cause.is_some(){"Async request"}else{"Caller"},caller.operation)).clicked(){investigation.selected=Some(caller.clone());investigation.refresh=true;}
                        } else {ui.label(format!("Caller span: {parent} — evicted or not captured"));}
                    }
                    for child in investigation.snapshot.timeline.iter().filter(|v|v.parent==Some(s.id)||v.cause==Some(s.id)) {
                        if ui.button(format!("{} — {}",child.operation,ms(Some(child.duration_ns)))).clicked(){investigation.selected=Some(child.clone());investigation.refresh=true;}
                    }
                } else {ui.weak("Select a frame or operation");}
            });
            if details.header_response.clicked(){layout.details=!layout.details;changed=true;}
        });
        if changed {
            self.shell.save();
        }
    }
}
fn ms(value: Option<u64>) -> String {
    value.map_or("Unavailable".into(), |v| format!("{:.3}", v as f64 / 1e6))
}
fn sparkline(ui: &mut egui::Ui, values: &[f32], size: egui::Vec2) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let max = values
        .iter()
        .copied()
        .filter(|v| v.is_finite())
        .fold(1.0_f32, f32::max);
    let points: Vec<_> = values
        .iter()
        .enumerate()
        .filter(|(_, v)| v.is_finite())
        .map(|(i, v)| {
            egui::pos2(
                rect.left()
                    + rect.width() * i as f32 / values.len().saturating_sub(1).max(1) as f32,
                rect.bottom() - rect.height() * v / max,
            )
        })
        .collect();
    if points.len() > 1 {
        ui.painter().add(egui::Shape::line(
            points,
            egui::Stroke::new(1.0, ui.visuals().selection.stroke.color),
        ));
    }
}
fn frame_graph(
    ui: &mut egui::Ui,
    frames: &[Sample],
    selection: &mut Selection,
    refresh: &mut bool,
    height: f32,
) {
    let values: Vec<f32> = frames.iter().map(|s| s.duration_ns as f32 / 1e6).collect();
    let start = ui.cursor().min;
    let width = ui.available_width().max(1.0);
    sparkline(ui, &values, egui::vec2(width, height));
    let rect = egui::Rect::from_min_size(start, egui::vec2(width, height));
    let response = ui.interact(rect, ui.id().with("frame_selection"), egui::Sense::click());
    if let Some(pos) = response.hover_pos() {
        if !frames.is_empty() {
            let i = (((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0)
                * (frames.len() - 1) as f32)
                .round() as usize;
            response.clone().on_hover_text(format!(
                "Frame {} · {:.3} ms",
                frames[i].frame,
                frames[i].duration_ns as f64 / 1e6
            ));
            if response.clicked() {
                if ui.input(|i| i.modifiers.shift) && selection.first.is_some() {
                    selection.scope = Scope::SelectedRange;
                    selection.last = Some(frames[i].frame);
                } else {
                    selection.scope = Scope::SelectedFrame;
                    selection.first = Some(frames[i].frame);
                    selection.last = selection.first;
                }
                *refresh = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn hidden_play_tray_ignores_errors_and_restores_geometry_and_selection() {
        let mut editor = Editor::new();
        editor.mode = crate::engine::EditorMode::Play;
        editor.shell.dock = super::super::shell::DockTab::Diagnostics;
        editor.shell.dock_height = 280.0;
        editor.shell.dock_expanded = true;
        editor.shell.play_tray_hidden = true;
        editor.append_output("[ERROR] test hidden error\n");
        let ctx = egui::Context::default();
        let draw = |editor: &mut Editor| {
            let _ = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    ..Default::default()
                },
                |ctx| editor.draw_shell_dock(ctx, &None),
            );
        };
        draw(&mut editor);
        assert!(editor.shell.play_tray_hidden);
        assert_eq!(editor.shell.dock_height, 280.0);
        editor.shell.play_tray_hidden = false;
        draw(&mut editor);
        assert_eq!(editor.shell.dock, super::super::shell::DockTab::Diagnostics);
        assert_eq!(editor.shell.dock_height, 280.0);
    }
    #[test]
    fn investigation_layout_survives_serialization_close_reopen_and_reorder() {
        let mut layout = Layout::default();
        layout.open(View::Scripts);
        layout.open(View::Timeline);
        layout.move_before(View::Timeline, View::Overview);
        layout.close(View::Overview);
        let mut restored: Layout =
            serde_json::from_slice(&serde_json::to_vec(&layout).unwrap()).unwrap();
        restored.normalize();
        assert_eq!(restored.tabs, vec![View::Timeline, View::Scripts]);
        restored.open(View::Overview);
        assert_eq!(restored.tabs.len(), 3);
        for v in View::ALL {
            restored.close(v);
        }
        assert!(restored.tabs.is_empty());
        assert_eq!(restored.active, None);
        restored = Layout::default();
        assert_eq!(restored.tabs, vec![View::Overview]);
    }
}
