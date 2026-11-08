// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::project::material::{
    GraphOperation, MaterialCatalog, MaterialOutput, ParameterValue, PinType,
};
use crate::project::post_process_pipeline::{
    PostProcessCommand, PostProcessEffect, PostProcessMask, PostProcessPipeline,
    PostProcessRuntime, PostProcessStage,
};
use std::collections::BTreeMap;
use std::path::Path;

fn value(kind: PinType) -> ParameterValue {
    match kind {
        PinType::Scalar => ParameterValue::Scalar(0.0),
        PinType::Vec2 => ParameterValue::Vec2([0.0, 0.0]),
        PinType::Vec3 => ParameterValue::Color3([0.0; 3]),
        PinType::Color => ParameterValue::Color4([0.0, 0.0, 0.0, 1.0]),
    }
}

fn draw_parameter(ui: &mut egui::Ui, name: &str, parameter: &mut ParameterValue) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.label(name);
        match parameter {
            ParameterValue::Scalar(v) => {
                changed |= ui.add(egui::DragValue::new(v).speed(0.01)).changed();
            }
            ParameterValue::Boolean(v) => {
                changed |= ui.checkbox(v, "").changed();
            }
            ParameterValue::Vec2(v) => {
                for part in v.iter_mut() {
                    changed |= ui.add(egui::DragValue::new(part).speed(0.01)).changed();
                }
            }
            ParameterValue::Color3(v) => {
                changed |= ui.color_edit_button_rgb(v).changed();
            }
            ParameterValue::Color4(v) => {
                changed |= ui.color_edit_button_rgba_unmultiplied(v).changed();
            }
        }
        if ui.small_button("Reset").clicked() {
            *parameter = match parameter {
                ParameterValue::Scalar(_) => ParameterValue::Scalar(0.0),
                ParameterValue::Boolean(_) => ParameterValue::Boolean(false),
                ParameterValue::Vec2(_) => ParameterValue::Vec2([0.0, 0.0]),
                ParameterValue::Color3(_) => ParameterValue::Color3([0.0; 3]),
                ParameterValue::Color4(_) => ParameterValue::Color4([0.0, 0.0, 0.0, 1.0]),
            };
            changed = true;
        }
    });
    changed
}

pub(super) fn render_object_mask(
    ui: &mut egui::Ui,
    project: Option<&Path>,
    object: crate::project::post_process_pipeline::PostProcessObject,
    preview: Option<&mut PostProcessRuntime>,
) -> Option<PostProcessCommand> {
    let Some(project) = project else { return None };
    let mut pipeline = match PostProcessPipeline::load(project) {
        Ok(v) => v,
        Err(error) => {
            ui.colored_label(
                ui.visuals().error_fg_color,
                format!("Invalid Effect: {error}"),
            );
            return None;
        }
    };
    let mut channel = preview.as_ref().map_or_else(
        || pipeline.object_channel(object),
        |runtime| runtime.object_channel(&pipeline, object),
    );
    let before = channel;
    egui::ComboBox::from_label("Post-Process Mask")
        .selected_text(if channel == 0 {
            "None".to_owned()
        } else {
            channel.to_string()
        })
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut channel, 0, "None");
            for option in 1..=8u8 {
                ui.selectable_value(&mut channel, option, option.to_string());
            }
        });
    if before != channel {
        let result = if let Some(runtime) = preview {
            runtime.set_object_channel(object, channel)
        } else {
            pipeline
                .set_object_channel(object, channel)
                .and_then(|_| pipeline.save(project))
        };
        match result {
            Ok(()) => return Some(PostProcessCommand::SetObjectChannel { object, channel }),
            Err(error) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Invalid Effect: {error}"),
                );
            }
        }
    }
    None
}

impl super::Editor {
    pub(super) fn render_selected_post_process_mask(
        &mut self,
        ui: &mut egui::Ui,
        project: Option<&Path>,
        object: crate::project::post_process_pipeline::PostProcessObject,
    ) {
        let play = self.mode == crate::engine::EditorMode::Play;
        let mut preview = self.post_process_runtime_preview.borrow_mut();
        let command = render_object_mask(
            ui,
            project,
            object,
            if play { Some(&mut preview) } else { None },
        );
        drop(preview);
        if let Some(command) = command {
            if play {
                self.post_process_runtime_commands
                    .borrow_mut()
                    .push(command);
            } else {
                self.post_process_dirty.set(true);
            }
        }
    }
    pub(super) fn render_post_process_settings(
        &mut self,
        ui: &mut egui::Ui,
        project: Option<&Path>,
    ) {
        let Some(project) = project else {
            return;
        };
        let authored = match PostProcessPipeline::load(project) {
            Ok(v) => v,
            Err(error) => {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Invalid Effect: {error}"),
                );
                return;
            }
        };
        let play = self.mode == crate::engine::EditorMode::Play;
        let mut pipeline = authored.clone();
        if play {
            pipeline.passes = self
                .post_process_runtime_preview
                .borrow()
                .resolved(&authored);
        }
        let before = pipeline.clone();
        let catalog = MaterialCatalog::load(project);
        let available: Vec<(String, String)> = catalog
            .documents()
            .iter()
            .filter_map(|(name, document)| {
                (catalog.resolve(&document.id) == Ok(MaterialOutput::PostProcess))
                    .then(|| (name.clone(), document.id.clone()))
            })
            .collect();
        let selection_id = egui::Id::new("wk_post_process_add_material");
        let mut selected = ui
            .ctx()
            .data(|d| d.get_temp::<String>(selection_id))
            .unwrap_or_default();
        if !available.iter().any(|(_, id)| id == &selected) {
            selected = available
                .first()
                .map(|(_, id)| id.clone())
                .unwrap_or_default();
        }
        let selected_name = available
            .iter()
            .find(|(_, id)| id == &selected)
            .map(|(name, _)| name.as_str())
            .unwrap_or("Missing Material");
        let mut changed = false;
        let mut edit_material: Option<String> = None;
        egui::CollapsingHeader::new("Post Processing")
            .default_open(true)
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_label("Material")
                        .selected_text(selected_name)
                        .show_ui(ui, |ui| {
                            for (name, id) in &available {
                                ui.selectable_value(&mut selected, id.clone(), name);
                            }
                        });
                    if ui
                        .add_enabled(
                            !play && !selected.is_empty(),
                            egui::Button::new("Add Effect"),
                        )
                        .clicked()
                    {
                        let mut parameters = BTreeMap::new();
                        if let Ok(Some(graph)) = catalog.graph(&selected) {
                            for node in &graph.nodes {
                                if let GraphOperation::Parameter { name, value_type } =
                                    &node.operation
                                {
                                    parameters
                                        .entry(name.clone())
                                        .or_insert_with(|| value(*value_type));
                                }
                            }
                        }
                        pipeline.passes.push(PostProcessEffect {
                            id: format!("effect-{:032x}", rand::random::<u128>()),
                            material_id: selected.clone(),
                            enabled: true,
                            stage: PostProcessStage::AfterTonemapping,
                            mask: PostProcessMask::All,
                            parameters,
                        });
                        changed = true;
                    }
                });
                if pipeline.passes.is_empty() {
                    ui.label("No post-processing effects");
                }
                let mut move_to: Option<(usize, usize)> = None;
                let mut remove = None;
                let len = pipeline.passes.len();
                for i in 0..len {
                    let effect = &mut pipeline.passes[i];
                    ui.push_id(effect.id.clone(), |ui| {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                let name = available
                                    .iter()
                                    .find(|(_, id)| id == &effect.material_id)
                                    .map(|(name, _)| name.as_str())
                                    .unwrap_or("Missing Material");
                                ui.strong(name);
                                changed |= ui.checkbox(&mut effect.enabled, "Enabled").changed();
                                if ui
                                    .add_enabled(!play && i > 0, egui::Button::new("Move Up"))
                                    .clicked()
                                {
                                    move_to = Some((i, i - 1));
                                }
                                if ui
                                    .add_enabled(
                                        !play && i + 1 < len,
                                        egui::Button::new("Move Down"),
                                    )
                                    .clicked()
                                {
                                    move_to = Some((i, i + 1));
                                }
                                if ui.add_enabled(!play, egui::Button::new("Remove")).clicked() {
                                    remove = Some(i);
                                }
                                if ui
                                    .add_enabled(!play, egui::Button::new("Edit Material"))
                                    .clicked()
                                {
                                    edit_material = available
                                        .iter()
                                        .find(|(_, id)| id == &effect.material_id)
                                        .map(|(n, _)| n.clone());
                                }
                            });
                            egui::CollapsingHeader::new("Parameters").show(ui, |ui| {
                                for (name, value) in effect.parameters.iter_mut() {
                                    changed |= draw_parameter(ui, name, value);
                                }
                            });
                        });
                    });
                }
                if let Some((a, b)) = move_to {
                    pipeline.passes.swap(a, b);
                    changed = true;
                }
                if let Some(index) = remove {
                    pipeline.passes.remove(index);
                    changed = true;
                }
            });
        ui.ctx().data_mut(|d| d.insert_temp(selection_id, selected));
        if changed {
            if play {
                let mut preview = self.post_process_runtime_preview.borrow_mut();
                let mut commands = self.post_process_runtime_commands.borrow_mut();
                for (old, new) in before.passes.iter().zip(&pipeline.passes) {
                    if old.enabled != new.enabled {
                        match preview.set_enabled(&authored, &new.id, new.enabled) {
                            Ok(()) => commands.push(PostProcessCommand::SetEnabled {
                                pass_id: new.id.clone(),
                                enabled: new.enabled,
                            }),
                            Err(error) => {
                                ui.colored_label(ui.visuals().error_fg_color, error);
                            }
                        }
                    }
                    for (name, value) in &new.parameters {
                        if old.parameters.get(name) == Some(value) {
                            continue;
                        }
                        match preview.set_parameter(&authored, &new.id, name, value.clone()) {
                            Ok(()) => commands.push(PostProcessCommand::SetParameter {
                                pass_id: new.id.clone(),
                                name: name.clone(),
                                value: value.clone(),
                            }),
                            Err(error) => {
                                ui.colored_label(ui.visuals().error_fg_color, error);
                            }
                        }
                    }
                }
            } else if let Err(error) = pipeline.save(project) {
                ui.colored_label(
                    ui.visuals().error_fg_color,
                    format!("Invalid Effect: {error}"),
                );
            } else {
                self.post_process_dirty.set(true);
            }
        }
        if let Some(name) = edit_material {
            match crate::editor::asset_management::material_editor::SurfaceMaterialEditor::open(
                project, &name,
            ) {
                Ok(editor) => {
                    self.asset_manager.surface_material_editor = Some(editor);
                    self.asset_manager.surface_editor_active = true;
                    self.asset_manager.sky_editor_active = false;
                    self.close_settings();
                    self.asset_manager.open = true;
                }
                Err(error) => {
                    ui.colored_label(
                        ui.visuals().error_fg_color,
                        format!("Missing Material: {error}"),
                    );
                }
            }
        }
    }
}
