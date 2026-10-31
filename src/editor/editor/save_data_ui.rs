// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::save_data_discovery::{ScriptSaveField, discover_script_save_fields};
use crate::engine::save_state::{
    PreviousSaveSlot, SaveState, SaveValue, previous_saves_for_project,
};
use egui::{Color32, RichText};
use std::collections::BTreeMap;
use std::path::PathBuf;

const MAX_VISIBLE_FIELDS: usize = 80;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum FieldType {
    #[default]
    Number,
    Boolean,
    Text,
}

impl FieldType {
    const ALL: [Self; 3] = [Self::Number, Self::Boolean, Self::Text];
    fn label(self) -> &'static str {
        match self {
            Self::Number => "Number",
            Self::Boolean => "Boolean",
            Self::Text => "Text",
        }
    }
    fn value(self) -> SaveValue {
        match self {
            Self::Number => SaveValue::Number(0.0),
            Self::Boolean => SaveValue::Bool(false),
            Self::Text => SaveValue::String(String::new()),
        }
    }
    fn from_value(v: &SaveValue) -> Self {
        match v {
            SaveValue::Number(_) => Self::Number,
            SaveValue::Bool(_) => Self::Boolean,
            SaveValue::String(_) => Self::Text,
        }
    }
}

#[derive(Default)]
pub(super) struct SaveDataEditor {
    project: Option<PathBuf>,
    state: Option<SaveState>,
    baseline: BTreeMap<String, SaveValue>,
    baseline_defaults: BTreeMap<String, SaveValue>,
    script_fields: Vec<ScriptSaveField>,
    previous_slots: Vec<PreviousSaveSlot>,
    search: String,
    new_key: String,
    new_type: FieldType,
    rename_key: Option<String>,
    rename_to: String,
    delete_key: Option<String>,
    confirm_reload: bool,
    status: Option<(String, bool)>,
}

impl SaveDataEditor {
    fn load(&mut self, project: &PathBuf) {
        self.project = Some(project.clone());
        self.state = None;
        self.baseline.clear();
        self.baseline_defaults.clear();
        self.script_fields = discover_script_save_fields(project);
        self.previous_slots = match previous_saves_for_project(project) {
            Ok(slots) => slots,
            Err(error) => {
                self.status = Some((format!("Previous Save Data scan failed: {error}"), false));
                Vec::new()
            }
        };
        self.rename_key = None;
        self.delete_key = None;
        self.confirm_reload = false;
        match SaveState::load(project) {
            Ok(state) => {
                self.baseline = state.values().clone();
                self.baseline_defaults = state.defaults().clone();
                self.state = Some(state);
                self.status = None;
            }
            Err(error) => {
                self.status = Some((
                    format!(
                        "Could not read project Save Data: {error}. Existing file was not modified."
                    ),
                    false,
                ));
            }
        }
    }

    fn save(&mut self) {
        let (Some(project), Some(state)) = (&self.project, &mut self.state) else {
            return;
        };
        let disk = match SaveState::load(project) {
            Ok(state) => state,
            Err(error) => {
                self.status = Some((
                    format!("Save blocked: cannot read current file: {error}"),
                    false,
                ));
                return;
            }
        };
        match commit_checked(state, &self.baseline, &self.baseline_defaults, &disk) {
            Ok(()) => {
                self.baseline = state.values().clone();
                self.baseline_defaults = state.defaults().clone();
                self.status = Some((
                    "Save Data committed. AeoScript will read these values on the next Play."
                        .into(),
                    true,
                ));
            }
            Err(error) => self.status = Some((error, false)),
        }
    }

    fn import_previous(&mut self, index: usize) {
        let Some(candidate) = self.previous_slots.get(index) else {
            return;
        };
        let Some(state) = self.state.as_mut() else {
            return;
        };
        let mut added = 0;
        for (key, value) in &candidate.values {
            if state.has(key) {
                continue;
            }
            match state.set(key, value.clone()) {
                Ok(()) => added += 1,
                Err(error) => {
                    self.status = Some((error, false));
                    return;
                }
            }
        }
        self.status = Some((
            format!(
                "Staged {added} previous saved values. Review them, then click Save Changes. Original save untouched."
            ),
            true,
        ));
    }

    fn apply_values(&mut self, key: &str, value: SaveValue, default: SaveValue) {
        if let Some(state) = self.state.as_mut() {
            if let Err(error) = state.set_with_default(key, value, Some(default)) {
                self.status = Some((error, false));
            } else {
                self.status = None;
            }
        }
    }

    fn add_field(&mut self) {
        let name = self.new_key.trim().to_owned();
        let Some(state) = self.state.as_mut() else {
            return;
        };
        if name.is_empty() {
            self.status = Some(("Enter a field key first.".into(), false));
        } else if state.has(&name) {
            self.status = Some((
                format!("'{name}' already exists. Choose a different key."),
                false,
            ));
        } else {
            match state.set(&name, self.new_type.value()) {
                Ok(()) => {
                    self.new_key.clear();
                    self.search.clear();
                    self.status =
                        Some((format!("Added '{name}'. Save Changes to persist it."), true));
                }
                Err(error) => self.status = Some((error, false)),
            }
        }
    }

    fn rename_field(&mut self, key: &str) {
        let destination = self.rename_to.trim().to_owned();
        let Some(state) = self.state.as_mut() else {
            return;
        };
        match state.rename(key, &destination) {
            Ok(()) => {
                self.rename_key = None;
                self.rename_to.clear();
                self.status = Some((
                    format!("Renamed '{key}' to '{destination}'. Update script keys if necessary."),
                    true,
                ));
            }
            Err(error) => self.status = Some((error, false)),
        }
    }

    pub(super) fn render(
        &mut self,
        ui: &mut egui::Ui,
        project_path: &Option<PathBuf>,
        editable: bool,
    ) {
        egui::CollapsingHeader::new("SAVE DATA")
            .default_open(true)
            .show(ui,|ui|{
                let Some(project)=project_path else {
                    ui.small("Open a project to inspect and edit its saved gameplay fields.");
                    return;
                };

                if self.project.as_ref()!=Some(project) {
                    if self.state.as_ref().is_some_and(SaveState::is_dirty) {
                        ui.colored_label(Color32::YELLOW,"The previous project has unsaved Save Data.");
                        ui.horizontal(|ui|{
                            if ui.button("Save Previous").clicked() {
                                self.save();
                                if self.state.as_ref().is_some_and(|s|!s.is_dirty()) {
                                    self.load(project);
                                }
                            }
                            if ui.button("Discard Previous").clicked() {
                                self.load(project);
                            }
                        });
                        self.show_status(ui);
                        return;
                    }
                    self.load(project);
                }
                if self.state.is_none() {
                    ui.label("This project's Save Data could not be loaded.");
                    if ui.button("Retry Load").clicked() {self.load(project);}
                    self.show_status(ui);
                    return;
                }

                let count=self.state.as_ref().map(|s|s.values().len()).unwrap_or(0);
                ui.label(RichText::new(format!("{count} fields · default slot")).strong());
                ui.small("Gameplay progress in AppData, separate from World/project files.");
                if !editable {
                    ui.colored_label(Color32::YELLOW,
                        "Stop Play to edit. Reload after Play to see newly committed script values.");
                }
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui|{
                    let dirty=self.state.as_ref().is_some_and(SaveState::is_dirty);
                    if dirty {ui.colored_label(Color32::YELLOW,"Unsaved changes");}
                    if ui.add_enabled(editable && dirty,egui::Button::new("Save Changes")).clicked(){
                        self.save();
                    }
                    if ui.button("Reload").clicked() {
                        if dirty {self.confirm_reload=true;}
                        else {self.load(project);}
                    }
                });
                if self.confirm_reload {
                    ui.colored_label(Color32::YELLOW,"Discard unsaved changes and reload from disk?");
                    ui.horizontal(|ui|{
                        if ui.button("Discard & Reload").clicked() {self.load(project);}
                        if ui.button("Cancel").clicked() {self.confirm_reload=false;}
                    });
                }
                self.render_previous_saves(ui,editable);
                self.render_script_references(ui,editable);
                ui.separator();
                ui.label(RichText::new("ADD FIELD").strong());
                ui.add_enabled_ui(editable,|ui|{
                    ui.add(egui::TextEdit::singleline(&mut self.new_key)
                        .hint_text("Field key (e.g. player.health)")
                        .desired_width(ui.available_width().max(80.0)));
                    ui.horizontal(|ui|{
                        egui::ComboBox::from_id_salt("save_data_new_field_type")
                            .selected_text(self.new_type.label())
                            .show_ui(ui,|ui|{
                                for kind in FieldType::ALL {
                                    ui.selectable_value(&mut self.new_type,kind,kind.label());
                                }
                            });
                        if ui.button("+ Add Field").clicked() {self.add_field();}
                    });
                });

                ui.separator();
                ui.label(RichText::new("FIELDS").strong());
                ui.add(egui::TextEdit::singleline(&mut self.search)
                    .hint_text("Search keys or namespaces...")
                    .desired_width(ui.available_width().max(80.0)));
                let query=self.search.trim().to_ascii_lowercase();
                let (keys,total) = {
                    let Some(state)=self.state.as_ref() else {return;};
                    let mut found=Vec::new();
                    let mut total=0;
                    for key in state.values().keys() {
                        if !key.to_ascii_lowercase().contains(&query) {continue;}
                        total+=1;
                        if found.len()<MAX_VISIBLE_FIELDS {found.push(key.clone());}
                    }
                    (found,total)
                };
                if total>MAX_VISIBLE_FIELDS {
                    ui.small(format!("Showing first {MAX_VISIBLE_FIELDS} of {total} matches. Filter to find others."));
                } else if total==0 {
                    ui.small("No fields match the current search.");
                }
                egui::Grid::new("save_data_fields_grid")
                    .num_columns(5)
                    .striped(true)
                    .spacing([12.0, 4.0])
                    .show(ui, |ui| {
                        ui.strong("Name");
                        ui.strong("Type");
                        ui.strong("Current");
                        ui.strong("Default");
                        ui.label("");
                        ui.end_row();
                        for key in keys {
                            self.draw_field(ui,&key,editable);
                        }
                    });
                self.show_status(ui);
                if self.state.as_ref().is_some_and(|s| !s.values().is_empty()) {
                    ui.small("Use save.get(key), save.set(key, value), and save.commit() in AeoScript.");
                }
            });
    }

    fn render_previous_saves(&mut self, ui: &mut egui::Ui, editable: bool) {
        if self.previous_slots.is_empty() {
            return;
        }
        ui.separator();
        egui::CollapsingHeader::new(format!(
            "PREVIOUS SAVES ({})",self.previous_slots.len()
        )).default_open(true).show(ui,|ui|{
            ui.small("The project path has changed, but earlier save slots with the same project name exist. Preview before importing; same-name projects may be unrelated.");
            for index in 0..self.previous_slots.len() {
                let candidate=&self.previous_slots[index];
                let count=candidate.values.len();
                let file=candidate.path.to_string_lossy().to_string();
                let mut import_clicked=false;
                egui::CollapsingHeader::new(format!("{count} saved fields · Previous slot {}",index+1))
                    .id_salt(("previous_save",index)).show(ui,|ui|{
                        ui.small(&file);
                        for (key,value) in candidate.values.iter().take(30){
                            let exists=self.state.as_ref().is_some_and(|s|s.has(key));
                            ui.horizontal_wrapped(|ui|{
                                ui.monospace(key);
                                ui.small(if exists {"Already saved in current slot"} else {"Available to import"});
                                ui.small(format!("{value:?}"));
                            });
                        }
                        if count>30 {ui.small(format!("Showing 30 of {count} keys."));}
                        if ui.add_enabled(editable,
                            egui::Button::new("Import Missing Values")).clicked(){
                            import_clicked=true;
                        }
                    });
                if import_clicked {self.import_previous(index);}
            }
        });
    }

    fn render_script_references(&mut self, ui: &mut egui::Ui, editable: bool) {
        ui.separator();
        let total = self.script_fields.len();
        egui::CollapsingHeader::new(format!("REFERENCED IN AEOSCRIPT ({total})"))
            .default_open(true).show(ui,|ui|{
                ui.small("Keys found in project AeoScript source. A reference does not mean a value has been saved. Dynamic/computed keys cannot be discovered statically.");
                if total==0 {ui.small("No literal save.get/set/has/remove keys found.");}
                let filter=self.search.trim().to_ascii_lowercase();
                let mut visible=0;
                for item in &self.script_fields {
                    if !item.key.to_ascii_lowercase().contains(&filter){continue;}
                    if visible>=MAX_VISIBLE_FIELDS {break;}
                    visible+=1;
                    let saved=self.state.as_ref().is_some_and(|s|s.has(&item.key));
                    let name=&item.key;
                    let source=item.first_source.to_string_lossy();
                    ui.horizontal_wrapped(|ui|{
                        ui.monospace(name).on_hover_text(format!(
                            "{} · {} references ({} writes)",source,item.references,item.writes
                        ));
                        ui.small(if saved {"Saved"} else {"Not saved"});
                        if !saved && ui.add_enabled(editable,egui::Button::new("Set up")).clicked(){
                            self.new_key=item.key.clone();
                            self.new_type=item.suggested.as_ref().map(FieldType::from_value)
                                .unwrap_or_default();
                            self.status=Some((
                                format!("Field '{}' is referenced but not saved. Select the correct type/value in Add Field above; do not treat script code as persisted progress.",name),true
                            ));
                        }
                    });
                }
                if visible==MAX_VISIBLE_FIELDS && total>visible {
                    ui.small("Showing first 80 matching keys. Filter to find others.");
                }
            });
    }

    fn draw_field(&mut self, ui: &mut egui::Ui, key: &str, editable: bool) {
        let Some(mut value) = self.state.as_ref().and_then(|s| s.get(key)).cloned() else {
            return;
        };
        let mut default = self
            .state
            .as_ref()
            .and_then(|s| s.get_default(key))
            .cloned()
            .unwrap_or_else(|| FieldType::from_value(&value).value());
        let mut kind = FieldType::from_value(&value);
        let mut change = false;
        let mut default_change = false;
        let mut rename_now = false;
        let mut delete_now = false;
        ui.monospace(key).on_hover_text(key);
        ui.add_enabled_ui(editable, |ui| {
            egui::ComboBox::from_id_salt(("save_data_kind", key))
                .selected_text(kind.label())
                .width(84.0)
                .show_ui(ui, |ui| {
                    for choice in FieldType::ALL {
                        ui.selectable_value(&mut kind, choice, choice.label());
                    }
                });
        });
        if kind != FieldType::from_value(&value) {
            value = kind.value();
            default = kind.value();
            change = true;
            default_change = true;
        }
        ui.add_enabled_ui(editable, |ui| match &mut value {
            SaveValue::Bool(value) => change |= ui.checkbox(value, "").changed(),
            SaveValue::Number(value) => {
                change |= ui.add(egui::DragValue::new(value).speed(1.0)).changed();
            }
            SaveValue::String(value) => {
                change |= ui
                    .add(egui::TextEdit::singleline(value).desired_width(180.0))
                    .changed();
            }
        });
        ui.add_enabled_ui(editable, |ui| match &mut default {
            SaveValue::Bool(v) => default_change |= ui.checkbox(v, "").changed(),
            SaveValue::Number(v) => {
                default_change |= ui.add(egui::DragValue::new(v).speed(1.0)).changed()
            }
            SaveValue::String(v) => {
                default_change |= ui
                    .add(egui::TextEdit::singleline(v).desired_width(130.0))
                    .changed()
            }
        });
        ui.horizontal(|ui| {
            if ui.small_button("Copy").clicked() {
                ui.ctx().copy_text(format!(
                    "save.get({})",
                    serde_json::to_string(key).unwrap_or_default()
                ));
            }
            if editable && ui.small_button("Rename").clicked() {
                self.rename_key = Some(key.to_owned());
                self.rename_to = key.to_owned();
                self.delete_key = None;
            }
            if editable && ui.small_button("Delete").clicked() {
                self.delete_key = Some(key.to_owned());
                self.rename_key = None;
            }
        });
        ui.end_row();
        if self.rename_key.as_deref() == Some(key) {
            ui.label("");
            ui.label("");
            ui.label("");
            ui.add(egui::TextEdit::singleline(&mut self.rename_to).desired_width(180.0));
            ui.horizontal(|ui| {
                rename_now = ui
                    .add_enabled(editable, egui::Button::new("Apply Rename"))
                    .clicked();
                if ui.small_button("Cancel").clicked() {
                    self.rename_key = None;
                }
            });
            ui.end_row();
        }
        if self.delete_key.as_deref() == Some(key) {
            ui.label("");
            ui.label("");
            ui.label("");
            ui.colored_label(Color32::YELLOW, "Remove this saved value?");
            ui.horizontal(|ui| {
                delete_now = ui
                    .add_enabled(editable, egui::Button::new("Confirm Delete"))
                    .clicked();
                if ui.small_button("Cancel").clicked() {
                    self.delete_key = None;
                }
            });
            ui.end_row();
        }
        if change || default_change {
            self.apply_values(key, value, default);
        }
        if rename_now {
            self.rename_field(key);
        }
        if delete_now {
            if let Some(state) = self.state.as_mut() {
                match state.remove(key) {
                    Ok(true) => {
                        self.status =
                            Some((format!("Deleted '{key}'. Save Changes to persist."), true))
                    }
                    Ok(false) => self.status = Some(("Field no longer exists.".into(), false)),
                    Err(error) => self.status = Some((error, false)),
                }
            }
            self.delete_key = None;
        }
    }

    fn show_status(&self, ui: &mut egui::Ui) {
        if let Some((message, success)) = &self.status {
            let color = if *success {
                ui.visuals().strong_text_color()
            } else {
                ui.visuals().error_fg_color
            };
            ui.colored_label(color, message);
        }
    }
}

fn commit_checked(
    state: &mut SaveState,
    baseline: &BTreeMap<String, SaveValue>,
    baseline_defaults: &BTreeMap<String, SaveValue>,
    disk: &SaveState,
) -> Result<(), String> {
    if disk.values() != baseline || disk.defaults() != baseline_defaults {
        return Err("Save blocked: gameplay or another editor changed this file. Reload to review the latest values before saving.".into());
    }
    state
        .commit()
        .map_err(|err| format!("Could not commit Save Data: {err}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    #[test]
    fn editor_save_rejects_external_play_changes_and_preserves_existing_values() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_save_data_ui_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let project = root.join("TestProject");
        fs::create_dir_all(&project).unwrap();
        let save_root = root.join("saves");
        let mut editor_state = SaveState::load_from_root(&project, &save_root).unwrap();
        let baseline = editor_state.values().clone();
        let editor_state_defaults = editor_state.defaults().clone();
        editor_state
            .set("progress.level", SaveValue::Number(3.0))
            .unwrap();
        let disk = SaveState::load_from_root(&project, &save_root).unwrap();
        commit_checked(&mut editor_state, &baseline, &editor_state_defaults, &disk).unwrap();
        let saved = SaveState::load_from_root(&project, &save_root).unwrap();
        assert_eq!(saved.get("progress.level"), Some(&SaveValue::Number(3.0)));

        let mut game_state = SaveState::load_from_root(&project, &save_root).unwrap();
        game_state
            .set("progress.level", SaveValue::Number(4.0))
            .unwrap();
        game_state.commit().unwrap();

        editor_state
            .set("progress.level", SaveValue::Number(9.0))
            .unwrap();
        let fresh_disk = SaveState::load_from_root(&project, &save_root).unwrap();
        assert!(
            commit_checked(
                &mut editor_state,
                saved.values(),
                saved.defaults(),
                &fresh_disk
            )
            .is_err()
        );
        assert_eq!(
            SaveState::load_from_root(&project, &save_root)
                .unwrap()
                .get("progress.level"),
            Some(&SaveValue::Number(4.0))
        );
        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn default_add_field_types_match_existing_aeoscript_save_contract() {
        assert_eq!(FieldType::Number.value(), SaveValue::Number(0.0));
        assert_eq!(FieldType::Boolean.value(), SaveValue::Bool(false));
        assert_eq!(FieldType::Text.value(), SaveValue::String(String::new()));
    }
}
