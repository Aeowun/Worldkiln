// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::editor::editor::shell::ActorContentDragPayload;
use crate::project::character_asset::{CHARACTER_ASSET_VERSION, CharacterAsset};
use egui::{RichText, Sense, Ui};
use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub(crate) struct CharacterEditorState {
    pub project: PathBuf,
    pub original_name: Option<String>,
    pub draft: CharacterAsset,
    pub status: Option<(String, bool)>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum CharacterEditorAction {
    None,
    Saved,
    Cancel,
}

impl CharacterEditorState {
    pub fn create(project: &Path, source_actor: Option<String>) -> Self {
        let source = source_actor.unwrap_or_default();
        let name = if source.is_empty() {
            "New Character".into()
        } else {
            source.clone()
        };
        Self {
            project: project.to_owned(),
            original_name: None,
            draft: CharacterAsset {
                version: CHARACTER_ASSET_VERSION,
                name,
                source_actor: source,
                scale: [1.0; 3],
                material: None,
                controller: None,
                camera: None,
            },
            status: None,
        }
    }

    pub fn open(project: &Path, name: &str) -> Result<Self, String> {
        Ok(Self {
            project: project.to_owned(),
            original_name: Some(name.to_owned()),
            draft: crate::project::character_asset::load(project, name)?,
            status: None,
        })
    }

    fn accept_actor_drop(&mut self, payload: &ActorContentDragPayload) -> bool {
        if payload.project_root != self.project {
            return false;
        }
        let actor = match crate::project::asset_registry::AssetRegistry::actor_directory(
            &self.project,
            &payload.package,
        ) {
            Ok(path) => path,
            Err(error) => {
                self.status = Some((error, false));
                return false;
            }
        };
        if !actor.join("character-package.json").is_file() || !actor.join("source.glb").is_file() {
            self.status = Some(("That Actor has no rigged Character data.".into(), false));
            return false;
        }
        self.draft.source_actor = payload.package.clone();
        if self.original_name.is_none() && self.draft.name == "New Character" {
            self.draft.name = actor
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();
        }
        self.status = None;
        true
    }
}

fn optional_combo(
    ui: &mut Ui,
    id: &'static str,
    label: &str,
    current: &mut Option<String>,
    values: Vec<String>,
) {
    ui.horizontal(|ui| {
        ui.label(label);
        egui::ComboBox::from_id_salt(id)
            .selected_text(current.as_deref().unwrap_or("None"))
            .show_ui(ui, |ui| {
                if ui.selectable_label(current.is_none(), "None").clicked() {
                    *current = None;
                }
                for value in values {
                    let selected = current.as_deref() == Some(value.as_str());
                    if ui.selectable_label(selected, &value).clicked() {
                        *current = Some(value);
                    }
                }
            });
    });
}

pub(crate) fn draw(ui: &mut Ui, editor: &mut CharacterEditorState) -> CharacterEditorAction {
    let mut action = CharacterEditorAction::None;
    ui.horizontal(|ui| {
        ui.heading(if editor.original_name.is_some() {
            "Edit Character"
        } else {
            "Create Character"
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .add_sized([36.0, 32.0], egui::Button::new("✕"))
                .on_hover_text("Close Character Editor")
                .clicked()
            {
                action = CharacterEditorAction::Cancel;
            }
        });
    });
    ui.separator();
    ui.horizontal(|ui| {
        ui.label("Character name");
        ui.text_edit_singleline(&mut editor.draft.name);
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Source Actor").strong());
    let (rect, response) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(560.0), 54.0),
        Sense::hover(),
    );
    let hover = response.dnd_hover_payload::<ActorContentDragPayload>();
    let valid = hover.as_ref().is_some_and(|payload| {
        payload.project_root == editor.project
            && crate::project::asset_registry::AssetRegistry::actor_directory(
                &editor.project,
                &payload.package,
            )
            .is_ok_and(|path| path.join("character-package.json").is_file())
    });
    let stroke = if valid {
        ui.visuals().selection.stroke
    } else {
        ui.visuals().widgets.inactive.bg_stroke
    };
    ui.painter()
        .rect(rect, 4.0, ui.visuals().widgets.inactive.bg_fill, stroke);
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        if editor.draft.source_actor.is_empty() {
            "Drag an Actor here"
        } else {
            &editor.draft.source_actor
        },
        egui::TextStyle::Button.resolve(ui.style()),
        ui.visuals().text_color(),
    );
    if let Some(payload) = response.dnd_release_payload::<ActorContentDragPayload>() {
        editor.accept_actor_drop(&payload);
    }

    ui.add_space(8.0);
    ui.label(RichText::new("Live GLB preview").strong());
    let (preview, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width().min(560.0), 260.0),
        Sense::hover(),
    );
    ui.painter()
        .rect_filled(preview, 4.0, ui.visuals().faint_bg_color);
    if editor.draft.source_actor.is_empty() {
        ui.painter().text(
            preview.center(),
            egui::Align2::CENTER_CENTER,
            "Choose a Source Actor",
            egui::TextStyle::Body.resolve(ui.style()),
            ui.visuals().weak_text_color(),
        );
    } else {
        crate::editor::actor_model_preview::paint_model(
            ui,
            preview,
            &crate::project::asset_registry::AssetRegistry::actor_directory(
                &editor.project,
                &editor.draft.source_actor,
            )
            .unwrap_or_default(),
        );
    }

    ui.horizontal(|ui| {
        ui.label("Character scale");
        for (axis, value) in ["X", "Y", "Z"]
            .into_iter()
            .zip(editor.draft.scale.iter_mut())
        {
            ui.label(axis);
            ui.add(
                egui::DragValue::new(value)
                    .speed(0.05)
                    .range(0.01..=100.0)
                    .fixed_decimals(2),
            );
        }
    });
    let catalog = crate::project::material::MaterialCatalog::cached(&editor.project);
    optional_combo(
        ui,
        "character_material",
        "Material",
        &mut editor.draft.material,
        catalog
            .documents()
            .keys()
            .filter(|name| !name.starts_with("pack/"))
            .cloned()
            .collect(),
    );
    optional_combo(
        ui,
        "character_controller",
        "Default Character Controller (.aeo)",
        &mut editor.draft.controller,
        crate::project::discover_controllers(&editor.project),
    );
    optional_combo(
        ui,
        "character_camera",
        "Default Camera Controller (.aeo)",
        &mut editor.draft.camera,
        crate::project::discover_cameras(&editor.project),
    );

    if !editor.draft.source_actor.is_empty() {
        let mut names = std::collections::BTreeSet::new();
        let actor = crate::project::asset_registry::AssetRegistry::actor_directory(
            &editor.project,
            &editor.draft.source_actor,
        )
        .unwrap_or_default();
        if let Ok(bytes) = std::fs::read(actor.join("import.json"))
            && let Ok(manifest) = serde_json::from_slice::<serde_json::Value>(&bytes)
            && let Some(clips) = manifest.get("clips").and_then(serde_json::Value::as_array)
        {
            for clip in clips.iter().filter_map(serde_json::Value::as_str) {
                names.insert(clip.to_owned());
            }
        }
        for (name, _) in crate::animation::asset::registered_clips_for_actor(
            &editor.project,
            &editor.draft.source_actor,
        ) {
            names.insert(name);
        }
        ui.separator();
        ui.label(RichText::new(format!("Animations ({})", names.len())).strong());
        egui::ScrollArea::vertical()
            .id_salt("character_animation_library")
            .max_height(104.0)
            .show(ui, |ui| {
                for name in names {
                    ui.monospace(name);
                }
            });
    }
    if let Some((message, ok)) = &editor.status {
        ui.colored_label(
            if *ok {
                ui.visuals().text_color()
            } else {
                ui.visuals().error_fg_color
            },
            message,
        );
    }
    ui.separator();
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        ui.spacing_mut().button_padding = egui::vec2(15.0, 8.0);
        let label = if editor.original_name.is_some() {
            "Save"
        } else {
            "Create"
        };
        if ui.button(label).clicked() {
            match crate::project::character_asset::save(
                &editor.project,
                editor.original_name.as_deref(),
                &editor.draft,
            ) {
                Ok(_) => {
                    editor.original_name = Some(editor.draft.name.clone());
                    editor.status = Some(("Character saved.".into(), true));
                    action = CharacterEditorAction::Saved;
                }
                Err(error) => editor.status = Some((error, false)),
            }
        }
        if ui
            .add_sized([95.0, 32.0], egui::Button::new("Close"))
            .clicked()
        {
            action = CharacterEditorAction::Cancel;
        }
    });
    action
}
