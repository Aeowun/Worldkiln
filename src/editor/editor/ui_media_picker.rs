// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui::{Align, Layout, RichText};

use super::Editor;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UiMediaPickerKind {
    Image,
    Video,
}

impl UiMediaPickerKind {
    fn title(self) -> &'static str {
        match self {
            Self::Image => "Select UI Image",
            Self::Video => "Select UI Video",
        }
    }

    fn accepts(self, extension: &str) -> bool {
        match self {
            Self::Image => matches!(extension, "png" | "jpg" | "jpeg"),
            Self::Video => extension == "mp4",
        }
    }

    fn categories(self) -> &'static [&'static str] {
        match self {
            Self::Image => &["Textures", "Media"],
            Self::Video => &["Media"],
        }
    }
}

#[derive(Clone, Debug)]
struct UiMediaAsset {
    category: &'static str,
    relative: String,
    source: String,
}

#[derive(Clone, Debug)]
pub struct UiMediaPickerState {
    node_id: u32,
    document_id: u32,
    project: PathBuf,
    kind: UiMediaPickerKind,
    category: &'static str,
    folder: String,
    search: String,
    selected: Option<String>,
    assets: Vec<UiMediaAsset>,
    error: Option<String>,
}

fn category_folder(category: &str) -> &'static str {
    match category {
        "Textures" => "textures",
        _ => "Media",
    }
}

fn imported_root(project: &Path, category: &str) -> PathBuf {
    super::asset_browser::physical_category_root(project, category)
}

impl UiMediaPickerState {
    pub fn open(
        project: &Path,
        document_id: u32,
        node_id: u32,
        kind: UiMediaPickerKind,
        current_source: &str,
    ) -> Self {
        let mut picker = Self {
            node_id,
            document_id,
            project: project.to_owned(),
            kind,
            category: kind.categories()[0],
            folder: String::new(),
            search: String::new(),
            selected: None,
            assets: Vec::new(),
            error: None,
        };
        picker.scan();
        if let Some(current) = picker
            .assets
            .iter()
            .find(|asset| asset.source == current_source)
        {
            picker.category = current.category;
            picker.folder = current
                .relative
                .rsplit_once('/')
                .map(|(parent, _)| parent.to_owned())
                .unwrap_or_default();
            picker.selected = Some(current.source.clone());
        }
        picker
    }

    fn scan(&mut self) {
        self.assets.clear();
        for &category in self.kind.categories() {
            let base = imported_root(&self.project, category);
            if !base.is_dir() || base.is_symlink() {
                continue;
            }
            let Ok(canonical_base) = base.canonicalize() else {
                continue;
            };
            let Ok(canonical_project) = self.project.canonicalize() else {
                continue;
            };
            if !canonical_base.starts_with(&canonical_project) {
                continue;
            }
            let mut pending = vec![(base, String::new(), 0u8)];
            while let Some((directory, prefix, depth)) = pending.pop() {
                if self.assets.len() >= 5000 {
                    self.error =
                        Some("Asset list capped at 5000 files. Narrow the project folder.".into());
                    break;
                }
                let Ok(read) = std::fs::read_dir(&directory) else {
                    continue;
                };
                let mut entries: Vec<_> = read.flatten().collect();
                entries
                    .sort_by_key(|entry| entry.file_name().to_string_lossy().to_ascii_lowercase());
                for entry in entries {
                    let Ok(kind) = entry.file_type() else {
                        continue;
                    };
                    if kind.is_symlink() {
                        continue;
                    }
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with('.') || name.contains('/') || name.contains('\\') {
                        continue;
                    }
                    let relative = if prefix.is_empty() {
                        name
                    } else {
                        format!("{prefix}/{name}")
                    };
                    let path = entry.path();
                    if kind.is_dir() {
                        if depth < 8 {
                            pending.push((path, relative, depth + 1));
                        }
                        continue;
                    }
                    if !kind.is_file() {
                        continue;
                    }
                    let ext = path
                        .extension()
                        .and_then(|ext| ext.to_str())
                        .unwrap_or("")
                        .to_ascii_lowercase();
                    if !self.kind.accepts(&ext) {
                        continue;
                    }
                    let Ok(canonical_path) = path.canonicalize() else {
                        continue;
                    };
                    if !canonical_path.starts_with(&canonical_base) {
                        continue;
                    }
                    let source = format!("{}/{}", category_folder(category), relative);
                    if crate::engine::ui_media::validate_ui_media_source(&source).is_ok() {
                        self.assets.push(UiMediaAsset {
                            category,
                            relative,
                            source,
                        });
                    }
                }
            }
        }
        self.assets.sort_by(|left, right| {
            left.category.cmp(right.category).then_with(|| {
                left.relative
                    .to_ascii_lowercase()
                    .cmp(&right.relative.to_ascii_lowercase())
            })
        });
    }

    fn visible(&self, item: &UiMediaAsset) -> bool {
        let query = self.search.trim().to_ascii_lowercase();
        if !query.is_empty() {
            return item.relative.to_ascii_lowercase().contains(&query);
        }
        if item.category != self.category {
            return false;
        }
        item.relative
            .rsplit_once('/')
            .map(|(parent, _)| parent)
            .unwrap_or("")
            == self.folder
    }

    fn folders(&self) -> Vec<String> {
        let mut folders = BTreeSet::new();
        for asset in self
            .assets
            .iter()
            .filter(|asset| asset.category == self.category)
        {
            let tail = if self.folder.is_empty() {
                asset.relative.as_str()
            } else if let Some(tail) = asset.relative.strip_prefix(&format!("{}/", self.folder)) {
                tail
            } else {
                continue;
            };
            if let Some((child, _)) = tail.split_once('/') {
                folders.insert(child.to_string());
            }
        }
        folders.into_iter().collect()
    }
}

impl Editor {
    pub(crate) fn draw_ui_media_picker(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<PathBuf>,
    ) {
        let Some(mut picker) = self.ui_media_picker.take() else {
            return;
        };
        if !self.ui_mode
            || project_path.as_deref() != Some(picker.project.as_path())
            || self.ui_workspace.active_document_id != picker.document_id
            || self.ui_document.node(picker.node_id).is_none()
        {
            return;
        }
        let mut is_open = true;
        let mut close = false;
        let mut choose = false;
        let mut open_asset_manager = false;
        egui::Window::new(picker.kind.title())
            .id(egui::Id::new("worldkiln_ui_imported_media_picker"))
            .open(&mut is_open)
            .collapsible(false)
            .resizable(true)
            .default_size(egui::vec2(700.0, 490.0))
            .min_width(520.0)
            .min_height(330.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Project Assets").strong());
                    ui.label("/");
                    ui.label(if picker.search.trim().is_empty() {
                        picker.category.to_string()
                    } else {
                        "Search Results".to_owned()
                    });
                    if !picker.folder.is_empty() && picker.search.trim().is_empty() {
                        ui.label("/");
                        ui.label(&picker.folder);
                    }
                });
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut picker.search)
                            .hint_text("Search imported assets...")
                            .desired_width(ui.available_width().max(180.0) - 10.0),
                    );
                });
                ui.separator();
                let height = ui.available_height().max(240.0) - 50.0;
                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(150.0, height),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            ui.strong("Folders");
                            ui.separator();
                            for &category in picker.kind.categories() {
                                if ui
                                    .selectable_label(
                                        picker.category == category && picker.folder.is_empty(),
                                        category,
                                    )
                                    .clicked()
                                {
                                    picker.category = category;
                                    picker.folder.clear();
                                    picker.search.clear();
                                }
                            }
                            if !picker.folder.is_empty() {
                                ui.separator();
                                if ui.button("Back").clicked() {
                                    picker.folder = picker
                                        .folder
                                        .rsplit_once('/')
                                        .map(|(parent, _)| parent.to_owned())
                                        .unwrap_or_default();
                                    picker.search.clear();
                                }
                            }
                        },
                    );
                    ui.separator();
                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), height),
                        Layout::top_down(Align::LEFT),
                        |ui| {
                            ui.strong(match picker.kind {
                                UiMediaPickerKind::Image => "Imported images",
                                UiMediaPickerKind::Video => "Imported videos",
                            });
                            ui.separator();
                            let folders = if picker.search.trim().is_empty() {
                                picker.folders()
                            } else {
                                Vec::new()
                            };
                            let visible: Vec<_> = picker
                                .assets
                                .iter()
                                .filter(|item| picker.visible(item))
                                .cloned()
                                .collect();
                            egui::ScrollArea::vertical()
                                .id_salt("worldkiln_ui_media_picker_scroll")
                                .auto_shrink([false, false])
                                .show(ui, |ui| {
                                    let tile_size = egui::vec2(140.0, 74.0);
                                    let columns =
                                        ((ui.available_width() / 150.0).floor() as usize).max(1);
                                    let mut index = 0;
                                    let mut tiles = Vec::new();
                                    for folder in &folders {
                                        tiles.push((
                                            format!("Folder\n{folder}"),
                                            Some(folder.clone()),
                                            None,
                                        ));
                                    }
                                    for item in &visible {
                                        let name = item
                                            .relative
                                            .rsplit('/')
                                            .next()
                                            .unwrap_or(&item.relative);
                                        let glyph = if picker.kind == UiMediaPickerKind::Video {
                                            "Video"
                                        } else {
                                            "Image"
                                        };
                                        tiles.push((
                                            format!("{glyph}\n{name}"),
                                            None,
                                            Some(item.source.clone()),
                                        ));
                                    }
                                    while index < tiles.len() {
                                        ui.horizontal(|ui| {
                                            for (label, folder, asset) in tiles
                                                [index..(index + columns).min(tiles.len())]
                                                .iter()
                                            {
                                                let selected =
                                                    asset.as_ref().is_some_and(|value| {
                                                        picker.selected.as_deref()
                                                            == Some(value.as_str())
                                                    });
                                                let response = ui.add_sized(
                                                    tile_size,
                                                    egui::SelectableLabel::new(
                                                        selected,
                                                        label.as_str(),
                                                    ),
                                                );
                                                if response.clicked() {
                                                    if let Some(folder) = folder {
                                                        picker.folder = if picker.folder.is_empty()
                                                        {
                                                            folder.clone()
                                                        } else {
                                                            format!("{}/{}", picker.folder, folder)
                                                        };
                                                    } else if let Some(asset) = asset {
                                                        picker.selected = Some(asset.clone());
                                                    }
                                                }
                                                if response.double_clicked() && asset.is_some() {
                                                    choose = true;
                                                }
                                            }
                                        });
                                        index += columns;
                                    }
                                    if tiles.is_empty() {
                                        ui.add_space(14.0);
                                        ui.label(match picker.kind {
                                            UiMediaPickerKind::Image => "No imported images found.",
                                            UiMediaPickerKind::Video => "No imported videos found.",
                                        });
                                        if ui.button("Open Asset Manager").clicked() {
                                            open_asset_manager = true;
                                            close = true;
                                        }
                                    }
                                });
                        },
                    );
                });
                if let Some(error) = &picker.error {
                    ui.colored_label(egui::Color32::LIGHT_RED, error);
                }
                if let Some(source) = &picker.selected {
                    ui.small(format!("Selected: {source}"));
                }
                ui.separator();
                ui.horizontal(|ui| {
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui
                            .add_enabled(
                                picker.selected.is_some(),
                                egui::Button::new("Select Asset"),
                            )
                            .clicked()
                        {
                            choose = true;
                        }
                        if ui.button("Cancel").clicked() {
                            close = true;
                        }
                    });
                });
            });
        if choose {
            if let Some(source) = picker.selected.clone() {
                let allowed = picker.assets.iter().any(|entry| entry.source == source);
                let validation = if !allowed {
                    Err("Selected asset is no longer in the project browser.".to_owned())
                } else {
                    crate::engine::ui_media::resolve_ui_media_path(&picker.project, &source)
                        .map(|_| ())
                };
                match validation {
                    Ok(()) => {
                        if let Some(node) = self.ui_document.node_mut(picker.node_id) {
                            if (picker.kind == UiMediaPickerKind::Image
                                && node.element_type == "Image")
                                || (picker.kind == UiMediaPickerKind::Video
                                    && node.element_type == "Video")
                            {
                                node.source = source;
                                self.needs_save = true;
                                close = true;
                            }
                        }
                    }
                    Err(error) => picker.error = Some(error),
                }
            }
        }
        if open_asset_manager {
            self.asset_manager.open = true;
        }
        if is_open && !close {
            self.ui_media_picker = Some(picker);
        }
    }
}
