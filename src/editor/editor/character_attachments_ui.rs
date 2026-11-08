// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::project::character_attachment_manifest::{
    AttachmentComponent as Component, CharacterAttachmentTemplate as Row, CharacterManifest,
};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
struct ManifestRowDrag {
    id: String,
}

#[derive(Default)]
pub struct CharacterManifestEditor {
    pub project: Option<PathBuf>,
    pub document: CharacterManifest,
    pub draft: Option<(Option<usize>, Row)>,
    pub pending_remove: Option<usize>,
    pub error: Option<String>,
    pub load_failed: bool,
}

impl super::Editor {
    pub(super) fn show_character_manifest(
        &mut self,
        ui: &mut egui::Ui,
        project: &Path,
        selected_character: &str,
    ) {
        let state = &mut self.character_manifest_editor;
        if state.project.as_deref() != Some(project) {
            state.project = Some(project.to_path_buf());
            state.draft = None;
            state.pending_remove = None;
            match crate::project::character_attachment_manifest::load(project) {
                Ok(doc) => {
                    state.document = doc;
                    state.error = None;
                    state.load_failed = false;
                }
                Err(err) => {
                    state.error = Some(err);
                    state.load_failed = true;
                    state.document = CharacterManifest::default();
                }
            }
        }
        egui::CollapsingHeader::new("ATTACHMENTS")
            .default_open(true)
            .show(ui, |ui| {
                ui.small("Default components created for the Player before game scripts start.");
                if let Some(err) = &state.error {
                    ui.colored_label(ui.visuals().error_fg_color, err);
                    if state.load_failed {
                        if ui.button("Retry Load").clicked() {
                            match crate::project::character_attachment_manifest::load(project) {
                                Ok(doc) => {
                                    state.document = doc;
                                    state.error = None;
                                    state.load_failed = false;
                                }
                                Err(err) => state.error = Some(err),
                            }
                        }
                        return;
                    }
                }
                if state.document.attachments.is_empty() {
                    ui.small("No default attachments configured.");
                } else {
                    ui.horizontal(|ui| {
                        ui.strong("Name");
                        ui.separator();
                        ui.strong("Type / Joint Target");
                    });
                }
                let mut reorder = None;
                for (index, row) in state.document.attachments.iter().enumerate() {
                    ui.push_id(row.id.clone(), |ui| {
                        let row_response = ui.horizontal_wrapped(|ui| {
                            let grip = ui.add(egui::Label::new("≡").sense(egui::Sense::drag()));
                            grip.dnd_set_drag_payload(ManifestRowDrag { id: row.id.clone() });
                            ui.label(&row.name);
                            let kind = match row.component {
                                Component::PointLight { .. } => "Point Light",
                                Component::SpatialAudio { .. } => "Spatial Audio",
                                Component::ParticleEmitter { .. } => "Particle Emitter",
                            };
                            ui.small(format!(
                                "{} / {}",
                                kind,
                                row.joint_target.as_deref().unwrap_or("Actor Origin")
                            ));
                            if !row.enabled {
                                ui.small("(Disabled)");
                            }
                            if ui.small_button("Edit").clicked() {
                                state.draft = Some((Some(index), row.clone()));
                            }
                            if ui.small_button("Remove").clicked() {
                                state.pending_remove = Some(index);
                            }
                            if index > 0 && ui.small_button("↑").clicked() {
                                reorder = Some((index, index - 1));
                            }
                            if index + 1 < state.document.attachments.len()
                                && ui.small_button("↓").clicked()
                            {
                                reorder = Some((index, index + 1));
                            }
                        });
                        if let Some(drag) = row_response
                            .response
                            .dnd_release_payload::<ManifestRowDrag>()
                        {
                            if let Some(from) = state
                                .document
                                .attachments
                                .iter()
                                .position(|candidate| candidate.id == drag.id)
                            {
                                if from != index {
                                    reorder = Some((from, index));
                                }
                            }
                        }
                    });
                }
                if let Some((from, to)) = reorder {
                    let mut next = state.document.clone();
                    next.attachments.swap(from, to);
                    match crate::project::character_attachment_manifest::save(project, &next) {
                        Ok(()) => state.document = next,
                        Err(err) => state.error = Some(err),
                    }
                }
                if let Some(index) = state.pending_remove {
                    let name = &state.document.attachments[index].name;
                    ui.label(format!(
                        "Remove attachment \"{}\" from the Character Manifest?",
                        name
                    ));
                    ui.horizontal(|ui| {
                        if ui.button("Remove").clicked() {
                            let mut next = state.document.clone();
                            next.attachments.remove(index);
                            match crate::project::character_attachment_manifest::save(
                                project, &next,
                            ) {
                                Ok(()) => state.document = next,
                                Err(err) => state.error = Some(err),
                            }
                            state.pending_remove = None;
                        }
                        if ui.button("Cancel").clicked() {
                            state.pending_remove = None;
                        }
                    });
                }
                if state.draft.is_none() && ui.button("Add Attachment").clicked() {
                    state.draft = Some((None, Row::point_light("New Attachment", "Actor Origin")));
                }
                if let Some((index, draft)) = state.draft.as_mut() {
                    ui.separator();
                    ui.label("Name");
                    ui.text_edit_singleline(&mut draft.name);
                    let mut kind = match draft.component {
                        Component::PointLight { .. } => 0,
                        Component::SpatialAudio { .. } => 1,
                        Component::ParticleEmitter { .. } => 2,
                    };
                    egui::ComboBox::from_label("Type")
                        .selected_text(["Point Light", "Spatial Audio", "Particle Emitter"][kind])
                        .show_ui(ui, |ui| {
                            for (i, label) in ["Point Light", "Spatial Audio", "Particle Emitter"]
                                .iter()
                                .enumerate()
                            {
                                ui.selectable_value(&mut kind, i, *label);
                            }
                        });
                    let old_kind = match draft.component {
                        Component::PointLight { .. } => 0,
                        Component::SpatialAudio { .. } => 1,
                        Component::ParticleEmitter { .. } => 2,
                    };
                    if old_kind != kind {
                        draft.component = match kind {
                            0 => Component::PointLight {
                                color: [1.0; 3],
                                intensity: 5.0,
                                range: 10.0,
                                shadows: true,
                            },
                            1 => Component::SpatialAudio {
                                audio: None,
                                play_on_start: false,
                                looped: false,
                                volume: 1.0,
                                min_distance: 10.0,
                                max_distance: 100.0,
                            },
                            _ => Component::ParticleEmitter {
                                effect: None,
                                preset: "Fire".into(),
                                rate: 40.0,
                                scale: 1.0,
                                radius: 0.2,
                            },
                        };
                    }
                    let joints: Vec<String> =
                        crate::character::package::get_package(selected_character)
                            .map(|pkg| {
                                pkg.skeleton
                                    .joints
                                    .iter()
                                    .map(|joint| joint.name.clone())
                                    .collect()
                            })
                            .unwrap_or_default();
                    let current = draft
                        .joint_target
                        .as_deref()
                        .unwrap_or("Actor Origin")
                        .to_owned();
                    egui::ComboBox::from_label("Joint Target")
                        .selected_text(&current)
                        .show_ui(ui, |ui| {
                            if ui
                                .selectable_label(draft.joint_target.is_none(), "Actor Origin")
                                .clicked()
                            {
                                draft.joint_target = None;
                            }
                            for joint in &joints {
                                if ui
                                    .selectable_label(
                                        draft.joint_target.as_deref() == Some(joint.as_str()),
                                        joint,
                                    )
                                    .clicked()
                                {
                                    draft.joint_target = Some(joint.clone());
                                }
                            }
                        });
                    if joints.is_empty() {
                        ui.small("No rigged Character selected; attach at Actor Origin.");
                    }
                    if let Some(missing) = draft.joint_target.as_deref() {
                        if !joints.iter().any(|name| name == missing) {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                format!("Missing: {missing}"),
                            );
                        }
                    }
                    ui.label("Local Origin Offset (studs)");
                    ui.horizontal(|ui| {
                        for value in &mut draft.local_origin_offset {
                            ui.add(egui::DragValue::new(value).speed(0.05));
                        }
                    });
                    let quat = glam::Quat::from_xyzw(
                        draft.local_rotation[0],
                        draft.local_rotation[1],
                        draft.local_rotation[2],
                        draft.local_rotation[3],
                    );
                    let quat = if quat.is_finite() && quat.length_squared() > 0.00001 {
                        quat.normalize()
                    } else {
                        glam::Quat::IDENTITY
                    };
                    let (x, y, z) = quat.to_euler(glam::EulerRot::XYZ);
                    let mut angles = [x.to_degrees(), y.to_degrees(), z.to_degrees()];
                    ui.label("Local Rotation (degrees)");
                    ui.horizontal(|ui| {
                        for value in &mut angles {
                            ui.add(egui::DragValue::new(value).speed(1.0));
                        }
                    });
                    let newq = glam::Quat::from_euler(
                        glam::EulerRot::XYZ,
                        angles[0].to_radians(),
                        angles[1].to_radians(),
                        angles[2].to_radians(),
                    );
                    draft.local_rotation = newq.to_array();
                    ui.checkbox(&mut draft.enabled, "Enabled");
                    match &mut draft.component {
                        Component::PointLight {
                            color,
                            intensity,
                            range,
                            shadows,
                        } => {
                            ui.strong("POINT LIGHT");
                            ui.color_edit_button_rgb(color);
                            ui.horizontal(|ui| {
                                ui.label("Intensity");
                                ui.add(egui::DragValue::new(intensity).range(0.0..=10000.0));
                                ui.label("Range");
                                ui.add(egui::DragValue::new(range).range(0.0..=100000.0));
                            });
                            ui.checkbox(shadows, "Shadows");
                        }
                        Component::SpatialAudio {
                            audio,
                            play_on_start,
                            looped,
                            volume,
                            min_distance,
                            max_distance,
                        } => {
                            ui.strong("SPATIAL AUDIO");
                            let mut path = audio.clone().unwrap_or_default();
                            super::asset_browser::draw_project_asset_drop(
                                ui,
                                Some(project),
                                &mut path,
                                super::asset_browser::AssetDropKind::Audio,
                            );
                            *audio = (!path.is_empty()).then_some(path);
                            ui.checkbox(play_on_start, "Play on Start");
                            ui.checkbox(looped, "Looped");
                            ui.horizontal(|ui| {
                                ui.label("Volume");
                                ui.add(egui::DragValue::new(volume).range(0.0..=10.0));
                            });
                            ui.horizontal(|ui| {
                                ui.label("Min Distance");
                                ui.add(egui::DragValue::new(min_distance).range(0.0..=100000.0));
                                ui.label("Max Distance");
                                ui.add(egui::DragValue::new(max_distance).range(0.0..=100000.0));
                            });
                        }
                        Component::ParticleEmitter {
                            effect,
                            preset,
                            rate,
                            scale,
                            radius,
                        } => {
                            ui.strong("PARTICLE EMITTER");
                            ui.label("Effect asset");
                            let (rect, response) = ui.allocate_exact_size(
                                super::asset_browser::compact_asset_drop_size(ui.available_width()),
                                egui::Sense::hover(),
                            );
                            ui.painter().rect(
                                rect,
                                4.0,
                                ui.visuals().widgets.inactive.bg_fill,
                                ui.visuals().widgets.inactive.bg_stroke,
                            );
                            ui.painter().text(
                                rect.center(),
                                egui::Align2::CENTER_CENTER,
                                effect.as_deref().unwrap_or("Drop Effect asset here"),
                                egui::TextStyle::Body.resolve(ui.style()),
                                ui.visuals().text_color(),
                            );
                            if let Some(payload) =
                                response.dnd_release_payload::<super::shell::FxContentDragPayload>()
                            {
                                if payload.project_root == project {
                                    *effect = Some(payload.asset_id.clone());
                                }
                            }
                            ui.horizontal(|ui| {
                                ui.label("Preset");
                                ui.text_edit_singleline(preset);
                            });
                            ui.horizontal(|ui| {
                                ui.label("Rate");
                                ui.add(egui::DragValue::new(rate).range(0.0..=100000.0));
                                ui.label("Scale");
                                ui.add(egui::DragValue::new(scale).range(0.001..=10000.0));
                                ui.label("Radius");
                                ui.add(egui::DragValue::new(radius).range(0.0..=100000.0));
                            });
                        }
                    }
                    let mut save_draft = false;
                    let mut cancel_draft = false;
                    ui.horizontal(|ui| {
                        if ui.button("Save").clicked() {
                            save_draft = true;
                        }
                        if ui.button("Cancel").clicked() {
                            cancel_draft = true;
                        }
                    });
                    if save_draft {
                        let mut next = state.document.clone();
                        if let Some(index) = *index {
                            next.attachments[index] = draft.clone();
                        } else {
                            next.attachments.push(draft.clone());
                        }
                        match crate::project::character_attachment_manifest::save(project, &next) {
                            Ok(()) => {
                                state.document = next;
                                state.draft = None;
                            }
                            Err(err) => state.error = Some(err),
                        }
                    } else if cancel_draft {
                        state.draft = None;
                    }
                }
            });
    }
}
