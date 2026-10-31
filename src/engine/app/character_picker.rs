// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::project::character_offerings::{self, Offering, Setup};
use std::{
    collections::{HashMap, HashSet},
    sync::mpsc::{self, Receiver},
    time::Duration,
};

#[derive(Default)]
pub(crate) struct CharacterPicker {
    pub selected: Option<String>,
    pub entries: Vec<Offering>,
    loading: Option<Receiver<(Vec<Offering>, HashSet<String>, Option<String>)>>,
    installed_ids: HashSet<String>,
    pub error: Option<String>,
    loaded: bool,
    preview_jobs: HashMap<String, Receiver<Option<egui::ColorImage>>>,
    previews: HashMap<String, egui::TextureHandle>,
    preview_failed: HashSet<String>,
    pub request: Option<(Offering, Setup)>,
}
impl CharacterPicker {
    pub fn load(&mut self) {
        if self.loaded {
            return;
        }
        self.loaded = true;
        let (tx, rx) = mpsc::channel();
        self.loading = Some(rx);
        std::thread::spawn(move || {
            let root = character_offerings::cache_root();
            let mut entries = character_offerings::local(&root);
            let installed_ids = entries.iter().map(|c| c.id.clone()).collect();
            let mut error = None;
            match character_offerings::fetch_catalog() {
                Ok(catalog) => {
                    for c in catalog.characters {
                        if !entries.iter().any(|old| old.id == c.id) {
                            entries.push(c);
                        }
                    }
                }
                Err(e) => error = Some(e),
            }
            entries.sort_by(|a, b| {
                a.name
                    .to_ascii_lowercase()
                    .cmp(&b.name.to_ascii_lowercase())
            });
            let _ = tx.send((entries, installed_ids, error));
        });
    }
    pub fn poll(&mut self, ctx: &egui::Context) {
        if let Some(rx) = &self.loading {
            match rx.try_recv() {
                Ok((entries, installed_ids, error)) => {
                    self.entries = entries;
                    self.installed_ids = installed_ids;
                    self.error = error;
                    self.loading = None;
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.loading = None;
                    self.error = Some("Character catalogue worker failed".into());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        let mut done = Vec::new();
        for (id, rx) in &self.preview_jobs {
            match rx.try_recv() {
                Ok(Some(img)) => {
                    self.previews.insert(
                        id.clone(),
                        ctx.load_texture(
                            format!("worldkiln_character_{id}"),
                            img,
                            egui::TextureOptions::LINEAR,
                        ),
                    );
                    done.push(id.clone());
                }
                Ok(None) | Err(mpsc::TryRecvError::Disconnected) => {
                    self.preview_failed.insert(id.clone());
                    done.push(id.clone());
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        for id in done {
            self.preview_jobs.remove(&id);
        }
        if self.loading.is_some() || !self.preview_jobs.is_empty() {
            ctx.request_repaint_after(Duration::from_millis(150));
        }
    }
    pub fn refresh(&mut self) {
        self.loaded = false;
        self.error = None;
        self.load();
    }
    pub(super) fn is_installed(&self, id: &str) -> bool {
        self.installed_ids.contains(id)
    }
    pub fn mark_installed(&mut self, id: &str) {
        self.installed_ids.insert(id.to_owned());
        self.preview_failed.remove(id);
    }
    pub fn selected_offering(&self) -> Option<Offering> {
        self.selected
            .as_ref()
            .and_then(|id| self.entries.iter().find(|c| &c.id == id))
            .cloned()
    }
    fn ensure_preview(&mut self, entry: &Offering) {
        if self.previews.contains_key(&entry.id)
            || self.preview_jobs.contains_key(&entry.id)
            || self.preview_failed.contains(&entry.id)
        {
            return;
        }
        if self.preview_jobs.len() >= 4 {
            return;
        }
        let (tx, rx) = mpsc::channel();
        self.preview_jobs.insert(entry.id.clone(), rx);
        let c = entry.clone();
        std::thread::spawn(move || {
            let bytes = character_offerings::preview_bytes(&c).ok();
            let image = bytes
                .and_then(|bytes| {
                    image::load_from_memory_with_format(&bytes, image::ImageFormat::Png).ok()
                })
                .map(|img| {
                    let rgba = img.to_rgba8();
                    egui::ColorImage::from_rgba_unmultiplied(
                        [rgba.width() as usize, rgba.height() as usize],
                        rgba.as_raw(),
                    )
                });
            let _ = tx.send(image);
        });
    }
    pub(super) fn image(&mut self, ui: &mut egui::Ui, c: &Offering, size: f32) {
        self.ensure_preview(c);
        if let Some(texture) = self.previews.get(&c.id) {
            ui.image((texture.id(), egui::vec2(size, size)));
        } else {
            ui.allocate_ui(egui::vec2(size, size), |ui| {
                ui.small("Loading preview");
            });
        }
    }
    fn status(&self, entry: &Offering) -> String {
        if self.installed_ids.contains(&entry.id) {
            "Installed".into()
        } else {
            format!(
                "Download ({:.1} MB)",
                entry.model_bytes as f64 / 1_000_000.0
            )
        }
    }
    pub fn choose(&mut self, ui: &mut egui::Ui) {
        if self.loading.is_some() {
            ui.spinner();
            ui.weak("Loading Characters...");
        }
        if self.entries.is_empty() {
            if self.loading.is_none() {
                ui.weak("No individual Characters are available.");
            }
            if let Some(error) = &self.error {
                ui.small(error);
            }
            return;
        }
        let title = self
            .selected_offering()
            .map(|c| c.name)
            .unwrap_or_else(|| "Choose Character".into());
        ui.menu_button(title, |ui| {
            ui.set_min_width(370.0);
            egui::ScrollArea::vertical()
                .max_height(300.0)
                .show(ui, |ui| {
                    for c in self.entries.clone() {
                        ui.horizontal(|ui| {
                            self.image(ui, &c, 48.0);
                            let text = format!("{} — {}", c.name, self.status(&c));
                            if ui
                                .selectable_label(self.selected.as_deref() == Some(&c.id), text)
                                .clicked()
                            {
                                self.selected = Some(c.id.clone());
                                ui.close_menu();
                            }
                        });
                    }
                });
        });
        if let Some(c) = self.selected_offering() {
            ui.label(self.status(&c));
        }
    }
}
