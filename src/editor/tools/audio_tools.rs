// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::asset_search::{AssetBrowserState, AssetEntry, AssetOrigin, ranked_asset_matches};
use egui::Ui;
use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

const AUDIO_BROWSER_ID: &str = "worldkiln_audio_browser";
const AUDIO_CATALOG_ID: &str = "worldkiln_audio_catalog";

#[derive(Clone, Default)]
struct AudioCatalog {
    entries: Vec<AssetEntry>,
}

fn scan_directory_wavs(
    current_dir: &Path,
    root_dir: &Path,
    origin: AssetOrigin,
    entries: &mut Vec<AssetEntry>,
) {
    let found = match crate::project::fs_walk::scan(current_dir, 64, 250_000) {
        Ok(found) => found,
        Err(error) => {
            eprintln!(
                "[EDITOR][AUDIO] Failed to scan {}: {error}",
                current_dir.display()
            );
            return;
        }
    };
    for (path, is_dir) in found {
        if is_dir {
            continue;
        }
        if !path
            .extension()
            .and_then(|ext| ext.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case("wav") || ext.eq_ignore_ascii_case("mp3"))
        {
            continue;
        }
        if let Ok(relative) = path.strip_prefix(root_dir) {
            entries.push(AssetEntry::new(
                relative.to_string_lossy().replace('\\', "/"),
                origin,
            ));
        }
    }
}

fn scan_audio_entries(project_path: &Option<PathBuf>) -> Vec<AssetEntry> {
    let mut discovered = Vec::new();

    let builtin_dir = crate::project::engine_assets_dir().join("audio");

    if builtin_dir.exists() {
        scan_directory_wavs(
            &builtin_dir,
            &builtin_dir,
            AssetOrigin::BuiltIn,
            &mut discovered,
        );
    }

    if let Some(project) = project_path {
        let project_dir = project.join(".assets").join("audio");

        if project_dir.exists() {
            scan_directory_wavs(
                &project_dir,
                &project_dir,
                AssetOrigin::Project,
                &mut discovered,
            );
        }
    }

    let mut by_path: BTreeMap<String, AssetEntry> = BTreeMap::new();
    for entry in discovered {
        match by_path.get(&entry.path) {
            Some(existing) if existing.origin == AssetOrigin::Project => {}
            _ => {
                by_path.insert(entry.path.clone(), entry);
            }
        }
    }

    by_path.into_values().collect()
}

fn load_audio_catalog(project_path: &Option<PathBuf>) -> AudioCatalog {
    AudioCatalog {
        entries: scan_audio_entries(project_path),
    }
}

fn audio_catalog_id(project_path: &Option<PathBuf>) -> egui::Id {
    egui::Id::new((
        AUDIO_CATALOG_ID,
        project_path
            .as_ref()
            .map(|path| path.to_string_lossy().into_owned()),
    ))
}

fn get_audio_catalog(ctx: &egui::Context, project_path: &Option<PathBuf>) -> AudioCatalog {
    let id = audio_catalog_id(project_path);

    if let Some(catalog) = ctx.data(|data| data.get_temp::<AudioCatalog>(id)) {
        return catalog;
    }

    let catalog = load_audio_catalog(project_path);

    ctx.data_mut(|data| {
        data.insert_temp(id, catalog.clone());
    });

    catalog
}

pub(crate) fn refresh_audio_catalog(ctx: &egui::Context, project_path: &Option<PathBuf>) {
    let catalog = load_audio_catalog(project_path);

    ctx.data_mut(|data| {
        data.insert_temp(audio_catalog_id(project_path), catalog);
    });
}

fn open_audio_browser(ctx: &egui::Context, project_path: &Option<PathBuf>, target_id: &str) {
    let _ = project_path;
    let id = egui::Id::new(AUDIO_BROWSER_ID);

    ctx.data_mut(|data| {
        let mut state = data.get_temp::<AssetBrowserState>(id).unwrap_or_default();

        state.open_for(target_id);

        data.insert_temp(id, state);
    });
}

fn audio_matches(catalog: &AudioCatalog, audio: &str) -> Vec<String> {
    ranked_asset_matches(
        catalog.entries.iter().map(|entry| entry.path.as_str()),
        audio,
        6,
    )
}

pub fn draw_audio_edit(
    ui: &mut Ui,
    audio: &mut String,
    project_path: &Option<PathBuf>,
    _target_id: &str,
) {
    crate::editor::editor::asset_browser::draw_project_asset_drop(
        ui,
        project_path.as_deref(),
        audio,
        crate::editor::editor::asset_browser::AssetDropKind::Audio,
    );
}

pub(crate) fn draw_audio_browser(
    ctx: &egui::Context,
    target_id: &str,
    selected_audio: &mut String,
    project_path: &Option<PathBuf>,
) {
    let id = egui::Id::new(AUDIO_BROWSER_ID);

    let mut state = ctx
        .data(|data| data.get_temp::<AssetBrowserState>(id))
        .unwrap_or_default();

    if !state.is_open_for(target_id) {
        return;
    }

    let mut open = state.open;
    let mut chosen_audio_name = None;

    egui::Window::new("Audio Browser")
        .open(&mut open)
        .default_size(egui::vec2(600.0, 400.0))
        .min_size(egui::vec2(360.0, 280.0))
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");

                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .desired_width(240.0)
                        .hint_text("Filter audio assets..."),
                );

                if ui.button("Refresh").clicked() {
                    refresh_audio_catalog(ctx, project_path);
                }
            });

            ui.separator();

            let catalog = get_audio_catalog(ctx, project_path);

            let search = state.search.trim().to_lowercase();

            let entries: Vec<&AssetEntry> = catalog
                .entries
                .iter()
                .filter(|entry| {
                    search.is_empty()
                        || entry.path.to_lowercase().contains(&search)
                        || entry.folder.to_lowercase().contains(&search)
                        || entry.origin.label().to_lowercase().contains(&search)
                })
                .collect();

            if entries.is_empty() {
                ui.vertical_centered(|ui| {
                    ui.add_space(24.0);

                    if catalog.entries.is_empty() {
                        ui.label("No imported WAV or MP3 audio assets found.");
                    } else {
                        ui.label("No audio assets match the search.");
                    }
                });

                return;
            }

            let mut groups: BTreeMap<String, Vec<&AssetEntry>> = BTreeMap::new();
            for entry in entries {
                groups.entry(entry.folder.clone()).or_default().push(entry);
            }

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (folder, entries) in groups {
                        egui::CollapsingHeader::new(format!("{} ({})", folder, entries.len()))
                            .default_open(true)
                            .show(ui, |ui| {
                                for entry in entries {
                                    let selected = selected_audio == &entry.path;

                                    ui.horizontal(|ui| {
                                        if ui.selectable_label(selected, &entry.path).clicked() {
                                            chosen_audio_name = Some(entry.path.clone());
                                        }

                                        ui.with_layout(
                                            egui::Layout::right_to_left(egui::Align::Center),
                                            |ui| {
                                                ui.label(
                                                    egui::RichText::new(entry.origin.label())
                                                        .monospace()
                                                        .small()
                                                        .weak(),
                                                );
                                            },
                                        );
                                    });
                                }
                            });
                    }
                });
        });

    if let Some(name) = chosen_audio_name {
        *selected_audio = name;
        open = false;
    }

    state.open = open;

    ctx.data_mut(|data| {
        data.insert_temp(id, state);
    });
}
