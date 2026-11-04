// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.

use crate::engine::audio::EngineSound;
use egui::RichText;
use std::process::Command;

use super::{App, COLOR_ACCENT_ORANGE, COLOR_ACCENT_RED, View};

const EMBLEM_PNG_BYTES: &[u8] = include_bytes!("../../../assets/branding/worldkiln-emblem.png");

#[derive(Clone)]
pub(crate) enum ManagementTarget {
    Project(std::path::PathBuf),
    Build(crate::project::project::BuiltGame),
}

impl ManagementTarget {
    fn name(&self) -> String {
        match self {
            Self::Project(path) => path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned(),
            Self::Build(game) => game.project_name.clone(),
        }
    }
}

pub(crate) struct HomeManagement {
    target: ManagementTarget,
    rename: bool,
    name: String,
}

fn merge_recent_project_paths(
    mut recent: Vec<std::path::PathBuf>,
    mut discovered: Vec<std::path::PathBuf>,
) -> Vec<std::path::PathBuf> {
    discovered.sort();
    for path in discovered {
        if !recent
            .iter()
            .any(|existing| crate::project::project::same_path(existing, &path))
        {
            recent.push(path);
        }
    }
    let mut unique: Vec<std::path::PathBuf> = Vec::new();
    for path in recent {
        if !unique
            .iter()
            .any(|existing| crate::project::project::same_path(existing, &path))
        {
            unique.push(path);
        }
    }
    unique
}

#[derive(Default)]
pub(crate) struct NewProjectPacks {
    catalog: Option<crate::project::asset_packs::Catalog>,
    selected: std::collections::BTreeSet<String>,
    catalog_job:
        Option<std::sync::mpsc::Receiver<Result<crate::project::asset_packs::Catalog, String>>>,
    closing: bool,
    project: Option<std::path::PathBuf>,
    error: Option<String>,
    creation_error: Option<String>,
    catalog_error: Option<String>,
    catalog_started: Option<std::time::Instant>,
    search: String,
    category: String,
    focused: Option<String>,
    preview_textures: std::collections::HashMap<String, egui::TextureHandle>,
    preview_jobs:
        std::collections::HashMap<String, std::sync::mpsc::Receiver<Option<egui::ColorImage>>>,
    preview_failed: std::collections::HashSet<String>,
    preview_ready: std::collections::HashMap<String, egui::ColorImage>,
    pub(super) characters: super::character_picker::CharacterPicker,
    installed_status: std::collections::HashMap<String, bool>,
    installed_job: Option<std::sync::mpsc::Receiver<std::collections::HashMap<String, bool>>>,
}

fn project_name_entered_for_packs(name: &str) -> bool {
    !name.trim().is_empty()
}

fn catalogue_size(bytes: u64) -> String {
    if bytes < 1_000 {
        format!("{bytes} B")
    } else if bytes < 1_000_000 {
        format!("{:.0} KB", bytes as f64 / 1_000.0)
    } else if bytes < 100_000_000 {
        format!("{:.1} MB", bytes as f64 / 1_000_000.0)
    } else {
        format!("{:.0} MB", bytes as f64 / 1_000_000.0)
    }
}
fn catalogue_category(pack: &crate::project::asset_packs::Pack) -> &str {
    if pack.category.eq_ignore_ascii_case("characters") {
        "Actors"
    } else {
        &pack.category
    }
}
fn catalogue_kind_icon(category: &str) -> &'static str {
    match category.to_ascii_lowercase().as_str() {
        "actors" | "entities" => "3D",
        "characters" => "NPC",
        "materials" => "MAT",
        "textures" => "PNG",
        "audio" => "SFX",
        "media" | "video" => "VID",
        _ => "PKG",
    }
}
fn catalogue_artwork(pack: &crate::project::asset_packs::Pack) -> Option<&'static str> {
    match pack.id.as_str() {
        "kenney-pirate-kit" => Some("pirate_hero.png"),
        "kenney-survival-kit" => Some("survival_hero.png"),
        "pirate-props" => Some("pirate_props.png"),
        "kenney-blocky-characters" => Some("catalogue_kenney-blocky-characters.png"),
        "kenney-nature-kit" => Some("catalogue_kenney-nature-kit.png"),
        "furniture-complete" => Some("catalogue_furniture-complete.png"),
        "furniture-chairs" => Some("catalogue_furniture-chairs.png"),
        "nature-trees" => Some("catalogue_nature-trees.png"),
        "nature-plants" => Some("catalogue_nature-plants.png"),
        "nature-rocks" => Some("catalogue_nature-rocks.png"),
        "nature-props" => Some("catalogue_nature-props.png"),
        "pirate-ships" => Some("catalogue_pirate-ships.png"),
        "survival-tools" => Some("catalogue_survival-tools.png"),
        "pbr-style" => Some("catalogue_pbr-style.png"),
        "pbr-4k" => Some("catalogue_pbr-4k.png"),
        _ => None,
    }
}
fn catalogue_generated_preview(
    ui: &egui::Ui,
    pack: &crate::project::asset_packs::Pack,
    rect: egui::Rect,
) {
    let painter = ui.painter();
    let area = rect.shrink(4.0);
    let seed = pack.id.bytes().fold(2166136261u32, |h, b| {
        (h ^ u32::from(b)).wrapping_mul(16777619)
    });
    let category = catalogue_category(pack).to_ascii_lowercase();
    if matches!(category.as_str(), "materials" | "textures") {
        let swatches = [
            egui::Color32::from_rgb(86, 149, 67),
            egui::Color32::from_rgb(111, 113, 121),
            egui::Color32::from_rgb(220, 227, 240),
            egui::Color32::from_rgb(98, 70, 51),
            egui::Color32::from_rgb(216, 178, 107),
            egui::Color32::from_rgb(154, 97, 61),
        ];
        let gap = 2.0;
        let cell_w = (area.width() - gap * 2.0) / 3.0;
        let cell_h = (area.height() - gap) / 2.0;
        for i in 0..6usize {
            let x = i % 3;
            let y = i / 3;
            let tile = egui::Rect::from_min_size(
                egui::pos2(
                    area.min.x + x as f32 * (cell_w + gap),
                    area.min.y + y as f32 * (cell_h + gap),
                ),
                egui::vec2(cell_w, cell_h),
            );
            painter.rect_filled(tile, 2.0, swatches[(i + seed as usize % 6) % 6]);
            for j in 0..4u32 {
                let n = seed
                    .wrapping_add((i as u32).wrapping_mul(41))
                    .wrapping_add(j.wrapping_mul(997));
                painter.circle_filled(
                    egui::pos2(
                        tile.left() + tile.width() * (0.14 + (n & 127) as f32 / 173.0),
                        tile.top() + tile.height() * (0.14 + ((n >> 8) & 127) as f32 / 173.0),
                    ),
                    (tile.width() * 0.035).max(0.7),
                    egui::Color32::from_rgba_unmultiplied(255, 255, 255, 70),
                );
            }
        }
    } else {
        let color = egui::Color32::from_rgb(
            47 + (seed & 63) as u8,
            68 + ((seed >> 8) & 63) as u8,
            92 + ((seed >> 16) & 63) as u8,
        );
        painter.rect_filled(area, 3.0, color);
        if category == "audio" {
            for i in 0..11u32 {
                let height =
                    area.height() * (0.15 + (seed.wrapping_add(i * 43) % 75) as f32 / 100.0);
                let x = area.left() + area.width() * (i as f32 + 1.0) / 12.0;
                painter.line_segment(
                    [
                        egui::pos2(x, area.center().y - height * 0.5),
                        egui::pos2(x, area.center().y + height * 0.5),
                    ],
                    egui::Stroke::new(
                        (area.width() / 35.0).max(1.0),
                        egui::Color32::from_rgb(218, 231, 244),
                    ),
                );
            }
        } else {
            for i in 0..3usize {
                let inset = 0.12 + i as f32 * 0.14;
                painter.rect_filled(
                    egui::Rect::from_min_size(
                        egui::pos2(
                            area.left() + area.width() * inset,
                            area.top() + area.height() * (0.16 + i as f32 * 0.14),
                        ),
                        egui::vec2(area.width() * 0.51, area.height() * 0.29),
                    ),
                    2.0,
                    egui::Color32::from_rgba_unmultiplied(225, 237, 250, 160),
                );
            }
        }
    }
    let label = egui::Rect::from_min_max(
        egui::pos2(area.left(), area.bottom() - area.height() * 0.25),
        area.right_bottom(),
    );
    painter.rect_filled(
        label,
        1.0,
        egui::Color32::from_rgba_unmultiplied(18, 22, 27, 195),
    );
    painter.text(
        label.center(),
        egui::Align2::CENTER_CENTER,
        catalogue_kind_icon(catalogue_category(pack)),
        egui::FontId::proportional((rect.height() * 0.14).clamp(9.0, 16.0)),
        egui::Color32::WHITE,
    );
}

fn catalogue_preview(
    ui: &mut egui::Ui,
    pack: &crate::project::asset_packs::Pack,
    size: egui::Vec2,
    packs: &mut NewProjectPacks,
) {
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let visuals = ui.visuals();
    ui.painter().rect_filled(
        rect,
        visuals.widgets.inactive.rounding,
        visuals.widgets.inactive.bg_fill,
    );
    ui.painter().rect_stroke(
        rect,
        visuals.widgets.inactive.rounding,
        visuals.widgets.inactive.bg_stroke,
    );
    catalogue_generated_preview(ui, pack, rect);
    let legacy = catalogue_artwork(pack);
    let filename = legacy
        .map(str::to_owned)
        .unwrap_or_else(|| format!("catalogue_{}.png", pack.id));
    let release = legacy
        .map(|_| "asset-packs-v1.3.0")
        .unwrap_or(pack.tag.as_str());
    let repository = if release.starts_with("asset-packs-v") {
        "Worldkiln-AssetPacks"
    } else {
        "Worldkiln"
    };
    let key = format!("{release}-{filename}");
    {
        if !packs.preview_textures.contains_key(&key) {
            if let Some(colors) = packs.preview_ready.remove(&key) {
                let texture = ui.ctx().load_texture(
                    format!("pack-artwork-{key}"),
                    colors,
                    egui::TextureOptions::LINEAR,
                );
                packs.preview_textures.insert(key.clone(), texture);
            } else if !packs.preview_failed.contains(&key)
                && !packs.preview_jobs.contains_key(&key)
                && packs.preview_jobs.len() < 3
            {
                let url = format!(
                    "https://github.com/Aeowun/{repository}/releases/download/{release}/{filename}"
                );
                let (tx, rx) = std::sync::mpsc::channel();
                packs.preview_jobs.insert(key.clone(), rx);
                let ctx = ui.ctx().clone();
                let cache_key = key.clone();
                std::thread::spawn(move || {
                    let cache = crate::project::asset_packs::cache_root()
                        .join("catalogue-artwork")
                        .join(cache_key);
                    let valid_png = |bytes: &Vec<u8>| {
                        bytes.len() <= 3_000_000 && bytes.starts_with(b"\x89PNG\r\n\x1a\n")
                    };
                    let cached = std::fs::read(&cache).ok().filter(valid_png);
                    let result = cached.or_else(|| {
                        let bytes = reqwest::blocking::Client::builder()
                            .user_agent("Worldkiln-asset-catalogue-previews")
                            .timeout(std::time::Duration::from_secs(12))
                            .build()
                            .ok()?
                            .get(&url)
                            .send()
                            .ok()?
                            .error_for_status()
                            .ok()?
                            .bytes()
                            .ok()?
                            .to_vec();
                        if !valid_png(&bytes) {
                            return None;
                        }
                        if let Some(dir) = cache.parent() {
                            if std::fs::create_dir_all(dir).is_ok() {
                                let _ = std::fs::write(&cache, &bytes);
                            }
                        }
                        Some(bytes)
                    });
                    let prepared = result.and_then(|bytes| {
                        let image = image::load_from_memory(&bytes).ok()?;
                        let image = image.thumbnail(560, 380).to_rgba8();
                        Some(egui::ColorImage::from_rgba_unmultiplied(
                            [image.width() as usize, image.height() as usize],
                            image.as_raw(),
                        ))
                    });
                    let _ = tx.send(prepared);
                    ctx.request_repaint();
                });
            }
        }
        if let Some(texture) = packs.preview_textures.get(&key) {
            let image_size = texture.size_vec2();
            let zoom =
                (rect.width() / image_size.x.max(1.0)).max(rect.height() / image_size.y.max(1.0));
            let visible = egui::vec2(
                rect.width() / (image_size.x * zoom).max(1.0),
                rect.height() / (image_size.y * zoom).max(1.0),
            );
            let uv = egui::Rect::from_center_size(egui::pos2(0.5, 0.5), visible);
            ui.painter()
                .image(texture.id(), rect, uv, egui::Color32::WHITE);
            return;
        }
    }
    ui.painter().text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        catalogue_kind_icon(catalogue_category(pack)),
        egui::FontId::proportional((size.y * 0.28).clamp(15.0, 34.0)),
        visuals.text_color(),
    );
}

fn catalogue_visible(
    pack: &crate::project::asset_packs::Pack,
    category: &str,
    query: &str,
) -> bool {
    (category.is_empty() || catalogue_category(pack) == category)
        && (query.is_empty()
            || format!(
                "{} {} {} {}",
                pack.name,
                catalogue_category(pack),
                pack.description,
                pack.license
            )
            .to_ascii_lowercase()
            .contains(query))
}
#[derive(Clone)]
enum CatalogueItem {
    Pack(crate::project::asset_packs::Pack),
    Character(crate::project::character_offerings::Offering),
}
impl CatalogueItem {
    fn focus_id(&self) -> String {
        match self {
            Self::Pack(pack) => pack.id.clone(),
            Self::Character(character) => format!("character:{}", character.id),
        }
    }
}
fn catalogue_items(packs: &NewProjectPacks, query: &str) -> Vec<CatalogueItem> {
    let mut items = Vec::new();
    if let Some(catalog) = &packs.catalog {
        items.extend(
            catalog
                .packs
                .iter()
                .filter(|pack| catalogue_visible(pack, &packs.category, query))
                .cloned()
                .map(CatalogueItem::Pack),
        );
    }
    if packs.category.is_empty() || packs.category == "Characters" {
        items.extend(
            packs
                .characters
                .entries
                .iter()
                .filter(|character| {
                    query.is_empty()
                        || format!(
                            "{} {} {} Characters",
                            character.name, character.attribution, character.license
                        )
                        .to_ascii_lowercase()
                        .contains(query)
                })
                .cloned()
                .map(CatalogueItem::Character),
        );
    }
    items
}
fn catalogue_row(
    ui: &mut egui::Ui,
    item: &CatalogueItem,
    packs: &mut NewProjectPacks,
    working: bool,
    width: f32,
) -> egui::Response {
    let id = item.focus_id();
    let focused = packs.focused.as_deref() == Some(id.as_str());
    let visuals = ui.visuals().clone();
    let bg = if focused {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let stroke = if focused {
        egui::Stroke::new(1.4, visuals.selection.bg_fill)
    } else {
        visuals.widgets.inactive.bg_stroke
    };
    let frame = egui::Frame::group(ui.style())
        .fill(bg)
        .stroke(stroke)
        .rounding(visuals.widgets.inactive.rounding)
        .inner_margin(egui::Margin::same(9.0));
    let response = ui.scope_builder(
        egui::UiBuilder::new()
            .id_salt(("wk_catalogue_card", &id))
            .sense(egui::Sense::click()),
        |ui| {
            frame.show(ui, |ui| {
                ui.set_min_width((width - 32.0).max(155.0));
                ui.set_min_height(110.0);
                ui.horizontal(|ui| {
                    if let CatalogueItem::Pack(pack) = item {
                        let mut checked = packs.selected.contains(&pack.id);
                        if ui.add_enabled(!working, egui::Checkbox::without_text(&mut checked)).changed() {
                            packs.set_selected(&pack.id, checked);
                        }
                        catalogue_preview(ui, pack, egui::vec2(108.0, 72.0), packs);
                    } else if let CatalogueItem::Character(character) = item {
                        ui.add_space(18.0);
                        packs.characters.image(ui, character, 72.0);
                        ui.add_space(36.0);
                    }
                    ui.vertical(|ui| {
                        ui.set_max_width((width - 155.0).max(100.0));
                        ui.spacing_mut().item_spacing.y = 5.0;
                        let (name, description, category, bytes, license, installed) = match item {
                            CatalogueItem::Pack(pack) => (
                                pack.name.as_str(), pack.description.as_str(),
                                catalogue_category(pack), pack.bytes, pack.license.as_str(),
                                packs.installed_status.get(&pack.id) == Some(&true),
                            ),
                            CatalogueItem::Character(character) => (
                                character.name.as_str(), "Animated Character with optional controller and camera setup",
                                "Characters", character.model_bytes, character.license.as_str(),
                                packs.characters.is_installed(&character.id),
                            ),
                        };
                        ui.add(egui::Label::new(egui::RichText::new(name).strong())
                            .truncate().selectable(false)).on_hover_text(name);
                        let short = description.chars().take(76).collect::<String>();
                        ui.add(egui::Label::new(
                            egui::RichText::new(short).size(16.8).color(visuals.weak_text_color())
                        ).truncate().selectable(false)).on_hover_text(description);
                        ui.add(egui::Label::new(
                            egui::RichText::new(format!("{}  |  {}  |  {}{}",
                                category, catalogue_size(bytes), license,
                                if installed { "  |  Installed" } else { "" }))
                                .size(15.4).color(visuals.weak_text_color())
                        ).truncate().selectable(false));
                    });
                });
            })
        },
    ).response;
    if response.clicked() {
        packs.focused = Some(id);
        if !working && response.double_clicked() {
            if let CatalogueItem::Pack(pack) = item {
                packs.set_selected(&pack.id, !packs.selected.contains(&pack.id));
            }
        }
    }
    ui.add_space(5.0);
    response
}
fn catalogue_details(ui: &mut egui::Ui, packs: &mut NewProjectPacks, working: bool, h: f32) {
    let visuals = ui.visuals().clone();
    egui::Frame::group(ui.style())
        .fill(visuals.window_fill)
        .inner_margin(egui::Margin::same(12.0))
        .show(ui, |ui| {
            ui.set_min_height((h - 16.0).max(120.0));
            ui.heading(
                if packs.category == "Characters"
                    || (packs.category.is_empty()
                        && packs
                            .focused
                            .as_deref()
                            .is_some_and(|id| id.starts_with("character:")))
                {
                    "Character Details"
                } else {
                    "Pack Details"
                },
            );
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("wk_pack_details_scroll")
                .max_height((h - 66.0).max(95.0))
                .show(ui, |ui| {
                    let query = packs.search.trim().to_ascii_lowercase();
                    let visible = catalogue_items(packs, &query);
                    let selected = visible
                        .iter()
                        .find(|item| packs.focused.as_deref() == Some(item.focus_id().as_str()))
                        .or_else(|| visible.first())
                        .cloned();
                    if let Some(CatalogueItem::Character(character)) = selected {
                        egui::Frame::group(ui.style())
                            .fill(visuals.widgets.inactive.bg_fill)
                            .inner_margin(egui::Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(7.0);
                                    packs.characters.image(ui, &character, 210.0);
                                    ui.add_space(7.0);
                                    ui.label(
                                        egui::RichText::new("CHARACTER")
                                            .size(15.4)
                                            .color(visuals.weak_text_color()),
                                    );
                                    ui.add_space(5.0);
                                });
                            });
                        ui.add_space(10.0);
                        ui.heading(&character.name);
                        ui.add_space(4.0);
                        ui.label("Animated Character with editable camera and controller setup.");
                        ui.add_space(8.0);
                        ui.separator();
                        egui::Grid::new("wk_character_details_metadata")
                            .num_columns(2)
                            .spacing(egui::vec2(14.0, 9.0))
                            .show(ui, |ui| {
                                ui.weak("Category");
                                ui.label("Characters");
                                ui.end_row();
                                ui.weak("Version");
                                ui.label(&character.version);
                                ui.end_row();
                                ui.weak("Download");
                                ui.label(catalogue_size(character.model_bytes));
                                ui.end_row();
                                ui.weak("License");
                                ui.label(&character.license);
                                ui.end_row();
                            });
                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(&character.attribution);
                        ui.add_space(12.0);
                        if packs.project.is_some() {
                            for (mode, label) in [
                                (
                                    crate::project::character_offerings::Setup::TopDown,
                                    "Top Down",
                                ),
                                (
                                    crate::project::character_offerings::Setup::ThirdPerson,
                                    "Third Person",
                                ),
                                (crate::project::character_offerings::Setup::Custom, "Custom"),
                            ] {
                                if ui
                                    .add_enabled(
                                        !working,
                                        egui::Button::new(label).min_size(egui::vec2(142.0, 34.0)),
                                    )
                                    .clicked()
                                {
                                    packs.characters.request = Some((character.clone(), mode));
                                }
                            }
                        } else {
                            ui.weak("Choose a starting Character in New Project setup.");
                        }
                    } else if let Some(CatalogueItem::Pack(pack)) = selected {
                        egui::Frame::group(ui.style())
                            .fill(visuals.widgets.inactive.bg_fill)
                            .inner_margin(egui::Margin::same(12.0))
                            .show(ui, |ui| {
                                ui.vertical_centered(|ui| {
                                    ui.add_space(7.0);
                                    catalogue_preview(ui, &pack, egui::vec2(210.0, 129.0), packs);
                                    ui.add_space(7.0);
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{} ASSET PACK",
                                            catalogue_category(&pack).to_ascii_uppercase()
                                        ))
                                        .size(15.4)
                                        .color(visuals.weak_text_color()),
                                    );
                                    ui.add_space(5.0);
                                });
                            });
                        ui.add_space(10.0);
                        ui.heading(&pack.name);
                        ui.add_space(4.0);
                        ui.label(&pack.description);
                        ui.add_space(8.0);
                        ui.separator();
                        egui::Grid::new("wk_pack_details_metadata")
                            .num_columns(2)
                            .spacing(egui::vec2(14.0, 9.0))
                            .show(ui, |ui| {
                                ui.weak("Category");
                                ui.label(catalogue_category(&pack));
                                ui.end_row();
                                ui.weak("Version");
                                ui.label(&pack.version);
                                ui.end_row();
                                ui.weak("Download");
                                ui.label(catalogue_size(pack.bytes));
                                ui.end_row();
                                ui.weak("License");
                                ui.label(&pack.license);
                                ui.end_row();
                            });
                        ui.add_space(8.0);
                        ui.separator();
                        ui.label(format!(
                            "{} files   |   {} materials",
                            pack.entries.len(),
                            pack.materials.len()
                        ));
                        if !pack.attribution.is_empty() {
                            ui.add(
                                egui::Label::new(
                                    egui::RichText::new(&pack.attribution)
                                        .size(16.8)
                                        .color(visuals.weak_text_color()),
                                )
                                .wrap(),
                            );
                        }
                        ui.add_space(12.0);
                        let checked = packs.selected.contains(&pack.id);
                        let label = if checked {
                            "Remove Selection"
                        } else {
                            "Select Pack"
                        };
                        if ui
                            .add_enabled(
                                !working,
                                egui::Button::new(label).min_size(egui::vec2(142.0, 34.0)),
                            )
                            .clicked()
                        {
                            packs.set_selected(&pack.id, !checked);
                        }
                    } else {
                        ui.add_space(26.0);
                        ui.label("Select an asset to view its details.");
                    }
                });
        });
}
fn asset_catalogue_body(ui: &mut egui::Ui, packs: &mut NewProjectPacks) {
    ui.scope(|ui| catalogue_body_scaled(ui, packs));
}

fn catalogue_body_scaled(ui: &mut egui::Ui, packs: &mut NewProjectPacks) {
    packs.characters.load();
    let working = false;
    for style in [
        egui::TextStyle::Body,
        egui::TextStyle::Small,
        egui::TextStyle::Heading,
        egui::TextStyle::Button,
    ] {
        if let Some(font) = ui.style_mut().text_styles.get_mut(&style) {
            font.size *= 1.4;
        }
    }
    let visuals = ui.visuals().clone();
    ui.horizontal(|ui| {
        ui.heading("Asset Catalogue");
        ui.add_space(10.0);
        let search_width = (ui.available_width() - 108.0).clamp(100.0, 460.0);
        ui.add_sized(
            [search_width, 30.0],
            egui::TextEdit::singleline(&mut packs.search).hint_text("Search assets..."),
        );
        if ui
            .add_enabled(
                !working && packs.catalog_job.is_none(),
                egui::Button::new("Refresh").min_size(egui::vec2(80.0, 30.0)),
            )
            .clicked()
        {
            packs.refresh();
            packs.characters.refresh();
        }
    });
    if packs.catalog_job.is_some() && packs.catalog.is_none() {
        ui.label("Loading asset catalog...");
    }
    if let Some(error) = packs.catalog_error.clone() {
        ui.horizontal_wrapped(|ui| {
            ui.colored_label(
                visuals.warn_fg_color,
                if packs.catalog.is_some() {
                    format!("Using cached asset catalogue. {error}")
                } else {
                    format!("Could not load asset catalogue: {error}")
                },
            );
            if ui
                .add_enabled(
                    !working && packs.catalog_job.is_none(),
                    egui::Button::new("Retry"),
                )
                .clicked()
            {
                packs.refresh();
            }
        });
    }
    ui.add_space(6.0);
    let size = ui.available_size();
    let h = (size.y - 41.0).max(185.0);
    let side = (size.x * 0.17).clamp(130.0, 200.0);
    let detail =
        ((size.x * 0.31).clamp(235.0, 370.0) * 1.3).min((size.x - side - 290.0).max(200.0));
    let list = (size.x - side - detail - 20.0).max(220.0);
    let query = packs.search.trim().to_ascii_lowercase();
    let items = catalogue_items(packs, &query);
    let visible_count = items.len();
    ui.horizontal_top(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(side, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::group(ui.style())
                    .fill(visuals.window_fill)
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_min_height((h - 16.0).max(120.0));
                        ui.heading("Categories");
                        ui.add_space(6.0);
                        if ui
                            .add_sized(
                                [side - 28.0, 34.0],
                                egui::SelectableLabel::new(packs.category.is_empty(), "All Assets"),
                            )
                            .clicked()
                        {
                            packs.category.clear();
                        }
                        let mut categories = std::collections::BTreeSet::new();
                        if let Some(c) = &packs.catalog {
                            for p in &c.packs {
                                categories.insert(catalogue_category(p).to_owned());
                            }
                        }
                        categories.insert("Characters".to_owned());
                        for kind in categories {
                            if ui
                                .add_sized(
                                    [side - 28.0, 34.0],
                                    egui::SelectableLabel::new(packs.category == kind, &kind),
                                )
                                .clicked()
                            {
                                packs.category = kind;
                            }
                        }
                    });
            },
        );
        ui.add_space(8.0);
        ui.allocate_ui_with_layout(
            egui::vec2(list, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                egui::Frame::group(ui.style())
                    .fill(visuals.window_fill)
                    .inner_margin(egui::Margin::same(10.0))
                    .show(ui, |ui| {
                        ui.set_min_height((h - 16.0).max(120.0));
                        ui.horizontal(|ui| {
                            ui.heading(if packs.category == "Characters" {
                                "Available Characters"
                            } else {
                                "Available Packs"
                            });
                            ui.with_layout(
                                egui::Layout::right_to_left(egui::Align::Center),
                                |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "{visible_count} {}",
                                            if packs.category == "Characters" {
                                                "characters"
                                            } else {
                                                "assets"
                                            }
                                        ))
                                        .color(visuals.weak_text_color()),
                                    );
                                },
                            );
                        });
                        ui.separator();
                        if items.is_empty() {
                            ui.weak("No assets match your filters.");
                            if packs.category == "Characters" && packs.characters.error.is_some() {
                                ui.label(packs.characters.error.as_deref().unwrap_or_default());
                            }
                        } else {
                            egui::ScrollArea::vertical()
                                .id_salt("wk_catalogue_tiles")
                                .max_height((h - 62.0).max(100.0))
                                .auto_shrink([false, false])
                                .show_rows(ui, 136.0, items.len(), |ui, rows| {
                                    for row in rows {
                                        catalogue_row(ui, &items[row], packs, working, list - 20.0);
                                    }
                                });
                        }
                    });
            },
        );
        ui.add_space(8.0);
        ui.allocate_ui_with_layout(
            egui::vec2(detail, h),
            egui::Layout::top_down(egui::Align::Min),
            |ui| {
                catalogue_details(ui, packs, working, h);
            },
        );
    });
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.strong(format!("{} selected", packs.selected.len()));
        ui.separator();
        ui.weak("All packs are optional");
        if let Some(error) = packs.creation_error.as_ref().or(packs.error.as_ref()) {
            ui.colored_label(visuals.error_fg_color, error);
        }
    });
}

impl NewProjectPacks {
    fn set_selected(&mut self, id: &str, selected: bool) {
        if selected {
            self.selected.insert(id.to_owned());
        } else {
            self.selected.remove(id);
        }
        self.focused = Some(id.to_owned());
    }

    fn start_installed_check(&mut self) {
        if self.catalog.is_none()
            || self.installed_job.is_some()
            || !self.installed_status.is_empty()
        {
            return;
        }
        let references = self
            .catalog
            .as_ref()
            .unwrap()
            .packs
            .iter()
            .map(|pack| (pack.id.clone(), pack.reference()))
            .collect::<Vec<_>>();
        if references.is_empty() {
            return;
        }
        let (sender, receiver) = std::sync::mpsc::channel();
        self.installed_job = Some(receiver);
        std::thread::spawn(move || {
            let root = crate::project::asset_packs::cache_root();
            let checked = references
                .into_iter()
                .map(|(id, reference)| {
                    (
                        id,
                        crate::project::asset_packs::installed(&root, &reference),
                    )
                })
                .collect();
            let _ = sender.send(checked);
        });
    }
    fn load(&mut self) {
        if self.catalog.is_some() || self.catalog_job.is_some() || self.catalog_error.is_some() {
            return;
        }
        let root = crate::project::asset_packs::cache_root();
        if let Ok(cached) = crate::project::asset_packs::cached_catalog(&root) {
            self.catalog = Some(cached);
        }
        self.refresh();
    }
    fn refresh(&mut self) {
        if self.catalog_job.is_some() {
            return;
        }
        self.catalog_error = None;
        self.catalog_started = Some(std::time::Instant::now());
        let root = crate::project::asset_packs::cache_root();
        let (sender, receiver) = std::sync::mpsc::channel();
        self.catalog_job = Some(receiver);
        std::thread::spawn(move || {
            let _ = sender.send(crate::project::asset_packs::fetch_catalog(&root));
        });
    }
    fn poll(&mut self) {
        let complete = self
            .preview_jobs
            .iter()
            .filter_map(|(key, rx)| match rx.try_recv() {
                Ok(result) => Some((key.clone(), result)),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => Some((key.clone(), None)),
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            })
            .collect::<Vec<_>>();
        for (key, result) in complete {
            self.preview_jobs.remove(&key);
            if let Some(image) = result {
                self.preview_ready.insert(key, image);
            } else {
                self.preview_failed.insert(key);
            }
        }
        if let Some(receiver) = &self.installed_job {
            match receiver.try_recv() {
                Ok(status) => {
                    self.installed_status = status;
                    self.installed_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.installed_job = None;
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        let result = self
            .catalog_job
            .as_ref()
            .and_then(|rx| match rx.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Catalogue worker stopped unexpectedly".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => self
                    .catalog_started
                    .filter(|t| t.elapsed() > std::time::Duration::from_secs(75))
                    .map(|_| Err("Catalogue request timed out".into())),
            });
        if let Some(result) = result {
            self.catalog_job = None;
            self.catalog_started = None;
            match result {
                Ok(c) => {
                    self.catalog = Some(c);
                    self.catalog_error = None;
                    self.installed_status.clear();
                    self.installed_job = None;
                }
                Err(e) => self.catalog_error = Some(e),
            }
        }
        self.start_installed_check();
    }
}

impl App {
    pub(crate) fn service_asset_pack_jobs(&mut self, ctx: &egui::Context) {
        self.new_project_packs.poll();
        self.asset_manager_packs.poll();
        self.new_project_packs.characters.poll(ctx);
        self.asset_manager_packs.characters.poll(ctx);
        self.service_character_setup_jobs(ctx);

        let loaded = self.loaded_project_path.clone();
        let mut refresh_active = false;
        for job in &mut self.background_asset_pack_jobs {
            job.poll();
            if job.refresh_pending
                && loaded
                    .as_deref()
                    .is_some_and(|path| crate::project::project::same_path(path, &job.project))
            {
                refresh_active = true;
                job.refresh_pending = false;
            }
        }
        if refresh_active {
            if let Some(project) = loaded.as_deref() {
                self.editor.invalidate_asset_pack_catalog();
                crate::editor::tools::refresh_asset_catalogs(ctx, project);
                self.renderer.reload_material_assets();
                self.renderer.invalidate_texture_cache();
            }
        }
        if let Some(job) = self
            .background_asset_pack_jobs
            .iter()
            .find(|job| !job.finished)
        {
            let name = job
                .project
                .file_name()
                .unwrap_or_default()
                .to_string_lossy();
            let detail = format!("{name}: {}", job.status);
            if crate::engine::progress_widget::draw_cancellable_progress(
                ctx,
                "wk_asset_packs_notification",
                "Asset Packs",
                Some(job.fraction().min(0.99)),
                &detail,
            ) {
                job.cancel();
            }
        }
        let mut retry = None;
        let mut dismiss = None;
        for (index, job) in self.background_asset_pack_jobs.iter().enumerate() {
            if let Some(error) = job.error.as_deref() {
                let project = job
                    .project
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy();
                egui::Window::new("Asset Pack Installation")
                    .id(egui::Id::new(("wk_pack_install_error", index)))
                    .collapsible(false)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.label(format!("{}: {}", project, error));
                        ui.horizontal(|ui| {
                            if ui.button("Retry").clicked() {
                                retry = Some(index);
                            }
                            if ui.button("Dismiss").clicked() {
                                dismiss = Some(index);
                            }
                        });
                    });
            }
        }
        if let Some(index) = retry {
            self.background_asset_pack_jobs[index].retry();
        }
        if let Some(index) = dismiss {
            self.background_asset_pack_jobs[index].error = None;
        }
        self.background_asset_pack_jobs
            .retain(|job| !job.finished || job.error.is_some() || job.refresh_pending);
        if self
            .background_asset_pack_jobs
            .iter()
            .any(|job| !job.finished)
            || self.new_project_packs.catalog_job.is_some()
            || self.asset_manager_packs.catalog_job.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
    }

    fn pack_job_running(&self, project: &std::path::Path) -> bool {
        self.background_asset_pack_jobs
            .iter()
            .any(|job| !job.finished && crate::project::project::same_path(&job.project, project))
    }

    fn create_from_project_setup(&mut self, with_asset_packs: bool) {
        let selected_character = self.new_project_packs.characters.selected_offering();
        if let Err(error) = crate::project::character_offerings::validate_selection(
            self.new_project_type,
            selected_character.as_ref(),
        ) {
            self.new_project_packs.creation_error = Some(error);
            self.play_engine_sound(EngineSound::Rejected);
            return;
        }
        if with_asset_packs {
            let result = self
                .new_project_packs
                .catalog
                .as_ref()
                .ok_or_else(|| "Asset catalogue is unavailable".to_owned())
                .and_then(|catalog| catalog.selected(&self.new_project_packs.selected));
            if let Err(error) = result {
                self.new_project_packs.creation_error = Some(error);
                return;
            }
        }
        match self
            .project_manager
            .create_project_checked(&self.new_project_name)
        {
            Ok(project) => {
                self.play_engine_sound(EngineSound::ProjectCreated);
                if self.new_project_type != crate::project::character_offerings::Setup::Blank {
                    if let Some(character) = selected_character {
                        self.background_character_jobs.push(
                            super::new_project_character::CharacterSetupJob::start(
                                project.clone(),
                                character,
                                self.new_project_type,
                            ),
                        );
                    }
                }
                if with_asset_packs && !self.new_project_packs.selected.is_empty() {
                    let result = super::pack_install_queue::PackInstallQueue::start(
                        self.new_project_packs.catalog.as_ref().unwrap(),
                        &self.new_project_packs.selected,
                        crate::project::asset_packs::cache_root(),
                        project,
                    );
                    match result {
                        Ok(job) => self.background_asset_pack_jobs.push(job),
                        Err(error) => crate::engine::diagnostics::report("ERROR", "Assets", error),
                    }
                }
                self.load_project();
                self.show_new_project_dialog = false;
                self.new_project_catalog_open = false;
                self.new_project_name.clear();
                self.new_project_name_warning = None;
                self.new_project_type = crate::project::character_offerings::Setup::Blank;
                self.new_project_packs = NewProjectPacks::default();
            }
            Err(error) => {
                self.new_project_packs.creation_error = Some(error);
                self.play_engine_sound(EngineSound::Rejected);
            }
        }
    }

    pub(crate) fn draw_splash_screen(&mut self, ctx: &egui::Context) {
        let elapsed = self.splash_started.elapsed().as_secs_f32();

        crate::engine::install_ui::draw_worldkiln_splash(ctx, elapsed, self.development_build);

        if elapsed >= 7.0 {
            self.view = View::Home;
        }
    }

    fn home_recent_card<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
        let frame = egui::Frame::none()
            .inner_margin(egui::Margin::same(12.0))
            .rounding(egui::Rounding::same(12.0))
            .fill(egui::Color32::TRANSPARENT)
            .stroke(ui.visuals().widgets.noninteractive.bg_stroke);
        frame
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width().max(160.0));
                add(ui)
            })
            .inner
    }

    pub(crate) fn draw_home_screen(&mut self, ctx: &egui::Context, next_view: &mut Option<View>) {
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(ctx.style().visuals.panel_fill))
            .show(ctx, |ui| {
                let available = ui.available_rect_before_wrap();
                let pad_x = available.width() * 0.02;
                let pad_y = available.height() * 0.02;
                let frame_rect = available.shrink2(egui::vec2(pad_x, pad_y));

                let outer = frame_rect.shrink2(egui::vec2(14.0, 10.0));

                let footer_height = 22.0;
                let footer_rect = egui::Rect::from_min_max(
                    egui::pos2(outer.left(), outer.bottom() - footer_height),
                    outer.right_bottom(),
                );
                ui.allocate_ui_at_rect(footer_rect, |ui| {
                    self.draw_home_footer(ui);
                });

                let content_top = outer.top() + 8.0;
                let content_bottom = footer_rect.top() - 6.0;

                if content_bottom <= content_top {
                    return;
                }

                let content_rect = egui::Rect::from_min_max(
                    egui::pos2(outer.left(), content_top),
                    egui::pos2(outer.right(), content_bottom),
                );

                let main_left = content_rect.left();

                let main_rect = egui::Rect::from_min_max(
                    egui::pos2(main_left, content_rect.top()),
                    content_rect.right_bottom(),
                );

                ui.allocate_ui_at_rect(main_rect, |ui| {
                    self.draw_home_main_content(ui, next_view);
                });
            });

        self.draw_home_dialogs(ctx, next_view);
        self.draw_home_management_dialogs(ctx);
        self.draw_home_publish_dialog(ctx);
        self.draw_home_member_dialog(ctx);
    }

    pub(crate) fn draw_home_main_content(
        &mut self,
        ui: &mut egui::Ui,
        next_view: &mut Option<View>,
    ) {
        if self.home_recovery.is_some() {
            self.draw_home_recovery_content(ui);
            return;
        }
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.heading(RichText::new("PROJECTS").monospace().strong().size(20.0));
                ui.label(
                    RichText::new("Your Worldkiln projects")
                        .color(ui.visuals().text_color())
                        .size(12.0),
                );
                ui.label(
                    RichText::new("Open an existing project or create one.")
                        .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                        .size(11.0),
                );
            });

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 8.0;

                let open_button = egui::Button::new(
                    RichText::new("OPEN PROJECT")
                        .monospace()
                        .strong()
                        .size(12.0),
                )
                .min_size(egui::vec2(120.0, 32.0));

                if ui.add(open_button).clicked() {
                    self.show_open_project_dialog = true;
                    self.show_new_project_dialog = false;
                }

                let accent_fill = ui.visuals().selection.bg_fill;
                let accent_stroke_color = ui.visuals().selection.stroke.color;

                let new_button = egui::Button::new(
                    RichText::new("+ NEW PROJECT")
                        .monospace()
                        .strong()
                        .color(accent_stroke_color)
                        .size(12.0),
                )
                .fill(accent_fill)
                .min_size(egui::vec2(130.0, 32.0));

                if ui.add(new_button).clicked() {
                    self.show_new_project_dialog = true;
                    self.show_open_project_dialog = false;
                }
            });
        });

        if let Some(error) = self.project_load_error.clone() {
            ui.add_space(8.0);
            ui.group(|ui| {
                ui.horizontal(|ui| {
                    ui.colored_label(
                        egui::Color32::from_rgb(230, 115, 100),
                        egui::RichText::new("PROJECT OPEN FAILED").strong(),
                    );
                    if ui.small_button("Copy error").clicked() {
                        ui.output_mut(|output| output.copied_text = error.clone());
                    }
                    if ui.small_button("Dismiss").clicked() {
                        self.project_load_error = None;
                    }
                });
                ui.label(&error);
            });
        }
        ui.add_space(10.0);
        ui.separator();
        ui.add_space(10.0);

        let available_width = ui.available_width();
        let is_wide = available_width >= 600.0;

        if is_wide {
            let gap = 16.0;
            let projects_width = (available_width - gap) * 0.58;
            let builds_width = available_width - gap - projects_width;

            ui.horizontal_top(|ui| {
                ui.allocate_ui_with_layout(
                    egui::vec2(projects_width, ui.available_height()),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        Self::home_recent_card(ui, |ui| {
                            self.draw_home_recent_projects(ui, next_view)
                        });
                    },
                );

                ui.add_space(gap);

                let column_height = ui.available_height();
                let builds_height = (column_height * 0.48).max(96.0);
                ui.allocate_ui_with_layout(
                    egui::vec2(builds_width, column_height),
                    egui::Layout::top_down(egui::Align::Min),
                    |ui| {
                        ui.allocate_ui_with_layout(
                            egui::vec2(builds_width, builds_height),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                Self::home_recent_card(ui, |ui| self.draw_home_recent_builds(ui));
                            },
                        );
                        ui.add_space(10.0);
                        ui.allocate_ui_with_layout(
                            egui::vec2(builds_width, (column_height - builds_height - 10.0).max(96.0)),
                            egui::Layout::top_down(egui::Align::Min),
                            |ui| {
                                Self::home_recent_card(ui, |ui| self.draw_home_developer_notes(ui));
                            },
                        );
                    },
                );
            });
        } else {
            egui::ScrollArea::vertical()
                .id_salt("home_main_scroll")
                .show(ui, |ui| {
                    Self::home_recent_card(ui, |ui| self.draw_home_recent_projects(ui, next_view));
                    ui.add_space(16.0);
                    Self::home_recent_card(ui, |ui| self.draw_home_recent_builds(ui));
                    ui.add_space(16.0);
                    Self::home_recent_card(ui, |ui| self.draw_home_developer_notes(ui));
                });
        }
    }

    pub(crate) fn draw_home_developer_notes(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("DEVELOPER NOTES")
                .monospace()
                .strong()
                .size(13.0),
        );
        ui.label(
            RichText::new("Latest Worldkiln updates")
                .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                .size(11.0),
        );
        if self.home_dev_notes.unavailable && !self.home_dev_notes.notes.is_empty() {
            ui.label(
                RichText::new("Offline — showing saved notes when available")
                    .size(10.0)
                    .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
            );
        }
        if let Some(last_updated) = self.home_dev_notes.last_updated.as_deref() {
            if let Ok(date) = chrono::DateTime::parse_from_rfc3339(last_updated) {
                ui.label(
                    RichText::new(format!(
                        "Updated {}",
                        date.with_timezone(&chrono::Local).format("%b %-d, %Y %I:%M %p")
                    ))
                    .size(10.0)
                    .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
                );
            }
        }
        ui.add_space(6.0);

        if self.home_dev_notes.notes.is_empty() {
            let message = if self.home_dev_notes.unavailable {
                "Development notes temporarily unavailable"
            } else if self.home_dev_notes.loading() {
                "Loading updates..."
            } else {
                "No published developer notes."
            };
            ui.label(RichText::new(message).size(11.0));
        } else {
            egui::ScrollArea::vertical()
                .id_salt("home_developer_notes_scroll")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    for note in self.home_dev_notes.notes.iter().take(10) {
                        ui.label(
                            RichText::new(note.category.to_uppercase())
                                .monospace()
                                .size(10.0)
                                .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
                        );
                        ui.label(RichText::new(&note.title).strong().size(12.0));
                        if !note.summary.is_empty() {
                            ui.label(RichText::new(&note.summary).size(11.0));
                        }
                        if let Some(link) = note.link_url.as_deref() {
                            ui.hyperlink_to("Read more", link);
                        }
                        ui.add_space(6.0);
                        ui.separator();
                        ui.add_space(6.0);
                    }
                });
        }
    }

    pub(crate) fn draw_home_recent_projects(
        &mut self,
        ui: &mut egui::Ui,
        next_view: &mut Option<View>,
    ) {
        ui.label(
            RichText::new("RECENT PROJECTS")
                .monospace()
                .strong()
                .size(13.0),
        );
        ui.add_space(8.0);

        let recent = merge_recent_project_paths(
            self.project_manager.recent_projects.clone(),
            self.project_manager.list_projects(),
        );

        if recent.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("NO RECENT PROJECTS")
                    .monospace()
                    .strong()
                    .size(11.0),
            );
            ui.add_space(4.0);
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("home_recent_projects_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                for (index, path) in recent.into_iter().enumerate() {
                    let name = path.file_name().unwrap_or_default().to_string_lossy();
                    let path_str = path.to_string_lossy();

                    let row_width = ui.available_width();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(row_width, 54.0), egui::Sense::click());

                    let hovered = response.hovered();

                    let fill = if hovered {
                        ui.visuals().widgets.hovered.bg_fill
                    } else {
                        ui.visuals().widgets.inactive.bg_fill
                    };

                    let stroke = if hovered {
                        ui.visuals().widgets.hovered.bg_stroke
                    } else {
                        ui.visuals().widgets.inactive.bg_stroke
                    };

                    ui.painter()
                        .rect_filled(rect, ui.visuals().widgets.inactive.rounding, fill);
                    ui.painter()
                        .rect_stroke(rect, ui.visuals().widgets.inactive.rounding, stroke);

                    let num_text = format!("{:02}", index + 1);
                    let text_left = rect.left() + 10.0;

                    ui.painter().text(
                        egui::pos2(text_left, rect.top() + 10.0),
                        egui::Align2::LEFT_TOP,
                        &num_text,
                        egui::FontId::monospace(11.0),
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    let name_x = text_left + 28.0;
                    let title =
                        egui::WidgetText::from(RichText::new(name.as_ref()).monospace().size(12.0))
                            .into_galley(
                                ui,
                                Some(egui::TextWrapMode::Truncate),
                                (rect.right() - name_x - 152.0).max(20.0),
                                egui::TextStyle::Body,
                            );
                    ui.painter().galley(
                        egui::pos2(name_x, rect.top() + 8.0),
                        title,
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    ui.painter().text(
                        egui::pos2(name_x, rect.top() + 25.0),
                        egui::Align2::LEFT_TOP,
                        "Local project",
                        egui::FontId::proportional(10.0),
                        ui.visuals().widgets.noninteractive.fg_stroke.color,
                    );

                    let max_path_width = (rect.right() - name_x - 152.0).max(40.0);
                    let truncated_path = egui::WidgetText::from(path_str.as_ref()).into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        max_path_width,
                        egui::TextStyle::Small,
                    );
                    ui.painter().galley(
                        egui::pos2(name_x, rect.top() + 38.0),
                        truncated_path,
                        ui.visuals().widgets.noninteractive.fg_stroke.color,
                    );

                    ui.painter().text(
                        egui::pos2(rect.right() - 84.0, rect.center().y),
                        egui::Align2::RIGHT_CENTER,
                        "OPEN",
                        egui::FontId::monospace(11.0),
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    let manage_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 76.0, rect.top() + 12.0),
                        egui::pos2(rect.right() - 8.0, rect.bottom() - 12.0),
                    );
                    self.draw_home_manage_menu(
                        ui,
                        manage_rect,
                        ManagementTarget::Project(path.clone()),
                    );
                    if response.clicked() && !ui.rect_contains_pointer(manage_rect) {
                        self.request_open_project(path);
                    }

                    ui.add_space(6.0);
                }
            });
    }

    pub(crate) fn draw_home_recent_builds(&mut self, ui: &mut egui::Ui) {
        ui.label(
            RichText::new("RECENT BUILDS")
                .monospace()
                .strong()
                .size(13.0),
        );
        ui.add_space(8.0);

        let built_games = self.project_manager.list_built_games();

        if built_games.is_empty() {
            ui.add_space(12.0);
            ui.label(
                RichText::new("NO RECENT BUILDS")
                    .monospace()
                    .strong()
                    .size(11.0),
            );
            ui.add_space(4.0);
            return;
        }

        egui::ScrollArea::vertical()
            .id_salt("home_recent_builds_scroll")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.set_min_width(ui.available_width());

                for (index, game) in built_games.into_iter().enumerate() {
                    let row_width = ui.available_width();
                    let (rect, response) =
                        ui.allocate_exact_size(egui::vec2(row_width, 79.0), egui::Sense::click());

                    let hovered = response.hovered();

                    let fill = if hovered {
                        ui.visuals().widgets.hovered.bg_fill
                    } else {
                        ui.visuals().widgets.inactive.bg_fill
                    };

                    let stroke = if hovered {
                        ui.visuals().widgets.hovered.bg_stroke
                    } else {
                        ui.visuals().widgets.inactive.bg_stroke
                    };

                    ui.painter()
                        .rect_filled(rect, ui.visuals().widgets.inactive.rounding, fill);
                    ui.painter()
                        .rect_stroke(rect, ui.visuals().widgets.inactive.rounding, stroke);

                    let num_text = format!("{:02}", index + 1);
                    let text_left = rect.left() + 10.0;

                    ui.painter().text(
                        egui::pos2(text_left, rect.top() + 10.0),
                        egui::Align2::LEFT_TOP,
                        &num_text,
                        egui::FontId::monospace(11.0),
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    let name_x = text_left + 28.0;
                    let title = egui::WidgetText::from(
                        RichText::new(&game.project_name).monospace().size(12.0),
                    )
                    .into_galley(
                        ui,
                        Some(egui::TextWrapMode::Truncate),
                        (rect.right() - name_x - 152.0).max(20.0),
                        egui::TextStyle::Body,
                    );
                    ui.painter().galley(
                        egui::pos2(name_x, rect.top() + 8.0),
                        title,
                        if hovered {
                            ui.visuals().widgets.hovered.fg_stroke.color
                        } else {
                            ui.visuals().widgets.noninteractive.fg_stroke.color
                        },
                    );

                    ui.painter().text(
                        egui::pos2(name_x, rect.top() + 26.0),
                        egui::Align2::LEFT_TOP,
                        &game.version,
                        egui::FontId::monospace(10.0),
                        ui.visuals().widgets.noninteractive.fg_stroke.color,
                    );

                    let play_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 144.0, rect.top() + 7.0),
                        egui::pos2(rect.right() - 90.0, rect.top() + 34.0),
                    );

                    let play_hovered = ui.rect_contains_pointer(play_rect);

                    ui.painter().rect_filled(
                        play_rect,
                        ui.visuals().widgets.inactive.rounding,
                        if play_hovered {
                            ui.visuals().selection.bg_fill
                        } else {
                            ui.visuals().widgets.active.bg_fill
                        },
                    );

                    ui.painter().text(
                        play_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "PLAY",
                        egui::FontId::monospace(10.0),
                        if play_hovered {
                            ui.visuals().selection.stroke.color
                        } else {
                            ui.visuals().widgets.active.fg_stroke.color
                        },
                    );

                    let manage_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 76.0, rect.top() + 7.0),
                        egui::pos2(rect.right() - 8.0, rect.top() + 34.0),
                    );
                    self.draw_home_manage_menu(
                        ui,
                        manage_rect,
                        ManagementTarget::Build(game.clone()),
                    );
                    let publish_rect = egui::Rect::from_min_max(
                        egui::pos2(rect.right() - 152.0, rect.top() + 43.0),
                        egui::pos2(rect.right() - 8.0, rect.bottom() - 7.0),
                    );
                    ui.allocate_ui_at_rect(publish_rect, |ui| {
                        if ui.add_sized(
                            publish_rect.size(),
                            egui::Button::new(
                                RichText::new("PUBLISH TO AEOWUN").monospace().size(10.0),
                            ),
                        ).clicked() {
                            self.home_publish_build = Some(game.clone());
                            self.publish_description.clear();
                            self.publish_rights = false;
                            self.publish_safety = false;
                            self.publish_publication = false;
                        }
                    });
                    if response.clicked()
                        && !ui.rect_contains_pointer(manage_rect)
                        && !ui.rect_contains_pointer(publish_rect)
                    {
                        match Command::new(&game.executable)
                            .current_dir(
                                game.executable
                                    .parent()
                                    .unwrap_or_else(|| std::path::Path::new(".")),
                            )
                            .spawn()
                        {
                            Ok(_) => {
                                self.editor.terminal_output.push_str(&format!(
                                    "[GAME] Launched {} ({})\n",
                                    game.project_name, game.version
                                ));
                            }
                            Err(error) => {
                                self.home_management_message = Some(format!(
                                    "Failed to launch {}: {}",
                                    game.executable.display(),
                                    error
                                ));
                                self.editor.terminal_output.push_str(&format!(
                                    "[GAME ERROR] Failed to launch {}: {}\n",
                                    game.executable.display(),
                                    error
                                ));
                            }
                        }
                        self.editor.terminal_auto_scroll = true;
                    }

                    ui.add_space(6.0);
                }
            });
    }

    fn draw_home_publish_dialog(&mut self, ctx: &egui::Context) {
        let Some(game) = self.home_publish_build.clone() else {
            return;
        };
        use crate::engine::member_services::{MemberDisplay, PublishStatus};
        let mut open = true;
        egui::Window::new("PUBLISH TO AEOWUN")
            .id(egui::Id::new("worldkiln_home_publish_game"))
            .collapsible(false)
            .resizable(true)
            .default_width(420.0)
            .open(&mut open)
            .show(ctx, |ui| {
                ui.label(RichText::new(&game.project_name).strong());
                ui.label(format!("Version {}", game.version));
                ui.add_space(6.0);
                ui.label("Worldkiln will package this existing game into a ZIP for submission.");
                ui.label("Your local build stays unchanged. Public downloads require approval.");
                ui.add_space(10.0);
                if let MemberDisplay::SignedIn(member) = self.home_member.display.clone() {
                    ui.label(format!("Signed in as @{}", member.handle));
                    let folder = game.package_dir().ok().map(std::path::Path::to_path_buf);
                    if !game.executable.is_file() || folder.as_ref().is_none_or(|p| !p.is_dir()) {
                        ui.colored_label(
                            egui::Color32::from_rgb(230, 115, 100),
                            "This built game is missing. Rebuild the game first.",
                        );
                        return;
                    }
                    ui.add_space(6.0);
                    ui.label("PUBLIC GAME DESCRIPTION");
                    ui.add(
                        egui::TextEdit::multiline(&mut self.publish_description)
                            .desired_rows(3)
                            .hint_text("Describe what players can expect."),
                    );
                    ui.checkbox(
                        &mut self.publish_rights,
                        "I have the right to distribute this game and its content.",
                    );
                    ui.checkbox(
                        &mut self.publish_safety,
                        "I have checked this game for private data and unsafe files.",
                    );
                    ui.checkbox(
                        &mut self.publish_publication,
                        "I understand approved games become publicly downloadable.",
                    );
                    let status = self.home_member.publishing.clone();
                    let uploading = matches!(status, PublishStatus::Uploading { .. });
                    let valid = !uploading
                        && self.publish_rights
                        && self.publish_safety
                        && self.publish_publication
                        && !self.publish_description.trim().is_empty();
                    if ui
                        .add_enabled(valid, egui::Button::new("SUBMIT FOR REVIEW"))
                        .clicked()
                    {
                        if let Some(folder) = folder {
                            self.home_member.publish_built_game(
                                folder,
                                game.project_name.clone(),
                                game.version.clone(),
                                self.publish_description.clone(),
                            );
                        }
                    }
                    match status {
                        PublishStatus::Idle => {}
                        PublishStatus::Uploading { percent, phase } => {
                            ui.label(format!("{phase}: {percent}%"));
                            ui.add(egui::ProgressBar::new(percent as f32 / 100.0));
                            if ui.button("CANCEL UPLOAD").clicked() {
                                self.home_member.cancel_publish();
                            }
                        }
                        PublishStatus::Submitted(id) => {
                            ui.label(format!("Submitted for private review ({id})."));
                            ui.hyperlink_to(
                                "View your Aeowun profile",
                                format!(
                                    "https://aeowun.com/members/profile/?handle={}",
                                    member.handle
                                ),
                            );
                        }
                        PublishStatus::Failed(message) => {
                            ui.colored_label(egui::Color32::from_rgb(230, 115, 100), message);
                        }
                    }
                } else {
                    ui.label("Sign in to AEOWUN to publish this game.");
                    if ui.button("SIGN IN TO AEOWUN").clicked() {
                        self.home_member.dialog_open = true;
                    }
                }
            });
        if !open {
            self.home_publish_build = None;
        }
    }

    fn draw_home_manage_menu(
        &mut self,
        ui: &mut egui::Ui,
        rect: egui::Rect,
        target: ManagementTarget,
    ) {
        ui.push_id(match &target {
            ManagementTarget::Project(path) => path.clone(),
            ManagementTarget::Build(game) => game.executable.clone(),
        }, |ui| {
            ui.allocate_ui_at_rect(rect, |ui| {
                ui.menu_button("MANAGE", |ui| {
                    if let ManagementTarget::Project(path) = &target {
                        let recoveries = if self.home_recovery_job.is_none() {
                            super::recovery::recovery_candidates(path).unwrap_or_default()
                        } else {
                            Vec::new()
                        };
                        if !recoveries.is_empty() && ui.button("RECOVERY...").clicked() {
                            self.home_recovery = Some((path.clone(), recoveries, 0));
                            ui.close_menu();
                        }
                    }
                    for action in ["RENAME", "DUPLICATE", "MOVE", "DELETE"] {
                        if ui.button(action).clicked() {
                            ui.close_menu();
                            match action {
                                "RENAME" | "DELETE" => {
                                    self.home_management = Some(HomeManagement {
                                        name: target.name(), target: target.clone(), rename: action == "RENAME",
                                    });
                                }
                                "DUPLICATE" => {
                                    let result = match &target {
                                        ManagementTarget::Project(path) => self.project_manager.duplicate_project(path),
                                        ManagementTarget::Build(game) => self.project_manager.duplicate_built_game(game),
                                    };
                                    self.home_management_message = Some(match result {
                                        Ok(path) => format!("Duplicated to {}", path.display()),
                                        Err(error) => format!("DUPLICATE failed: {error}"),
                                    });
                                }
                                "MOVE" => {
                                    let home = std::env::var_os("USERPROFILE").or_else(|| std::env::var_os("HOME"))
                                        .map(std::path::PathBuf::from).unwrap_or_else(|| std::path::PathBuf::from("C:\\Users\\Public\\Documents"));
                                    if let Some(folder) = rfd::FileDialog::new()
                                        .set_title("MOVE - export a copy (original stays in Worldkiln)")
                                        .set_directory(home).pick_folder() {
                                        let result = match &target {
                                            ManagementTarget::Project(path) => self.project_manager.export_project(path, &folder),
                                            ManagementTarget::Build(game) => self.project_manager.export_built_game(game, &folder),
                                        };
                                        self.home_management_message = Some(match result {
                                            Ok(path) => format!("Exported to {}. Original preserved.", path.display()),
                                            Err(error) => format!("MOVE failed: {error}"),
                                        });
                                    }
                                }
                                _ => {}
                            }
                        }
                    }
                    ui.separator();
                    ui.label("MOVE exports a copy; the original stays.");
                });
            });
        });
    }

    fn draw_home_recovery_content(&mut self, ui: &mut egui::Ui) {
        if let Some((project, candidates, mut selected)) = self.home_recovery.take() {
            let mut keep_open = true;
            let mut action: Option<&'static str> = None;
            ui.vertical(|ui| {
                ui.heading(RichText::new("RECOVER PROJECT").monospace().strong());
                ui.separator();
                ui.label("Recovery snapshots");
                for (i, candidate) in candidates.iter().enumerate() {
                    let kind = if candidate.includes_world {
                        if candidate.script_count > 0 {
                            "WORLD + SCRIPTS"
                        } else {
                            "WORLD"
                        }
                    } else {
                        "SCRIPTS ONLY"
                    };
                    let timestamp = candidate
                        .name
                        .strip_prefix("snapshot-")
                        .and_then(|rest| rest.split('-').next())
                        .and_then(|value| value.parse::<i64>().ok())
                        .and_then(chrono::DateTime::<chrono::Utc>::from_timestamp_millis)
                        .map(|value| {
                            value
                                .with_timezone(&chrono::Local)
                                .format("%b %e, %I:%M %p")
                                .to_string()
                        })
                        .unwrap_or_else(|| candidate.name.clone());
                    let description =
                        format!("{timestamp} - {kind} ({} scripts)", candidate.script_count);
                    ui.radio_value(&mut selected, i, description);
                }
                ui.label(
                    "Recovery creates a separate project copy. The original project is preserved.",
                );
                ui.horizontal(|ui| {
                    if ui.button("OPEN ORIGINAL").clicked() {
                        action = Some("original");
                    }
                    if ui.button("OPEN RECOVERED COPY").clicked() {
                        action = Some("restore");
                    }
                    if ui.button("CANCEL").clicked() {
                        action = Some("cancel");
                    }
                });
            });
            match action {
                Some("original") => {
                    keep_open = false;
                    self.request_open_project(project.clone());
                }
                Some("restore") => {
                    if let Some(candidate) = candidates.get(selected) {
                        let parent = project.parent().unwrap_or(&project);
                        let base = project.file_name().unwrap_or_default().to_string_lossy();
                        let destination = (1..=1000)
                            .map(|n| parent.join(format!("{base} (Recovered {})", n)))
                            .find(|path| !path.exists());
                        match destination {
                            Some(destination) => {
                                let (sender, receiver) = std::sync::mpsc::channel();
                                self.home_recovery_job = Some(receiver);
                                let project = project.clone();
                                let name = candidate.name.clone();
                                std::thread::spawn(move || {
                                    let result = super::recovery::restore_recovery_to_new_project(
                                        &project,
                                        &name,
                                        &destination,
                                    )
                                    .map(|_| destination);
                                    let _ = sender.send(result);
                                });
                                keep_open = false;
                            }
                            None => {
                                self.home_management_message =
                                    Some("Recovery failed: no available destination name".into())
                            }
                        }
                    }
                }
                Some("cancel") => keep_open = false,
                _ => {}
            }
            if keep_open {
                self.home_recovery = Some((project, candidates, selected));
            }
        }
    }

    fn draw_home_management_dialogs(&mut self, ctx: &egui::Context) {
        if let Some(job) = self.home_recovery_job.as_ref() {
            match job.try_recv() {
                Ok(Ok(path)) => {
                    self.home_recovery_job = None;
                    self.request_open_project(path);
                }
                Ok(Err(error)) => {
                    self.home_recovery_job = None;
                    self.home_management_message = Some(format!("Recovery failed: {error}"));
                }
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    self.home_recovery_job = None;
                    self.home_management_message =
                        Some("Recovery failed: copy worker stopped".into());
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(mut pending) = self.home_management.take() {
            let mut confirm = false;
            let mut cancel = false;
            egui::Window::new(if pending.rename {
                "Rename"
            } else {
                "Delete complete package?"
            })
            .id(egui::Id::new("home_management_confirmation"))
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
            .show(ctx, |ui| {
                ui.label(pending.target.name());
                if pending.rename {
                    ui.text_edit_singleline(&mut pending.name);
                } else {
                    ui.label(
                        "This permanently deletes the complete selected folder and its content.",
                    );
                }
                ui.horizontal(|ui| {
                    confirm = ui
                        .button(if pending.rename { "RENAME" } else { "DELETE" })
                        .clicked();
                    cancel = ui.button("CANCEL").clicked();
                });
            });
            if confirm {
                let result = match &pending.target {
                    ManagementTarget::Project(path) if pending.rename => self
                        .project_manager
                        .rename_project(path, &pending.name)
                        .map(|_| ()),
                    ManagementTarget::Build(game) if pending.rename => self
                        .project_manager
                        .rename_built_game(game, &pending.name)
                        .map(|_| ()),
                    ManagementTarget::Project(path) => self.project_manager.delete_project(path),
                    ManagementTarget::Build(game) => self.project_manager.delete_built_game(game),
                };
                self.home_management_message = Some(match result {
                    Ok(()) => if pending.rename {
                        "Renamed successfully."
                    } else {
                        "Deleted selected folder."
                    }
                    .to_string(),
                    Err(error) => format!("Management failed: {error}"),
                });
            } else if !cancel {
                self.home_management = Some(pending);
            }
        }
        if let Some(message) = self.home_management_message.clone() {
            egui::Window::new("Project / Built Game management")
                .id(egui::Id::new("home_management_result"))
                .collapsible(false)
                .resizable(false)
                .max_width(520.0)
                .show(ctx, |ui| {
                    ui.label(message);
                    if ui.button("OK").clicked() {
                        self.home_management_message = None;
                    }
                });
        }
    }

    pub(crate) fn draw_home_member_dialog(&mut self, ctx: &egui::Context) {
        if !self.home_member.dialog_open {
            return;
        }

        let mut open = true;
        egui::Window::new("AEOWUN MEMBER")
            .id(egui::Id::new("worldkiln_member_account"))
            .collapsible(false)
            .resizable(false)
            .default_width(380.0)
            .max_width(480.0)
            .open(&mut open)
            .show(ctx, |ui| {
                use crate::engine::member_services::MemberDisplay;
                match self.home_member.display.clone() {
                    MemberDisplay::SignedOut => {
                        ui.label("Worldkiln works without signing in.");
                        ui.add_space(10.0);
                        if let Some(message) = self.home_member.error {
                            ui.colored_label(egui::Color32::from_rgb(230, 115, 100), message);
                            ui.add_space(6.0);
                        }
                        ui.horizontal(|ui| {
                            let title = if self.home_member.error.is_some() {
                                "Try again"
                            } else {
                                "Sign in"
                            };
                            if ui.button(title).clicked() {
                                self.home_member.sign_in();
                            }
                            if ui.button("Create account").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(
                                    "https://aeowun.com/members/signup/".to_owned(),
                                ));
                            }
                        });
                    }
                    MemberDisplay::Starting => {
                        ui.horizontal(|ui| {
                            ui.spinner();
                            ui.label("Sign in");
                        });
                        ui.add_space(8.0);
                        if ui.button("Cancel").clicked() {
                            self.home_member.cancel();
                        }
                    }
                    MemberDisplay::AwaitingApproval(prompt) => {
                        ui.hyperlink_to(
                            "Approve this device in your browser.",
                            &prompt.verification_uri,
                        );
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(&prompt.user_code)
                                .monospace()
                                .strong()
                                .size(22.0),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Copy code").clicked() {
                                ui.output_mut(|output| {
                                    output.copied_text = prompt.user_code.clone();
                                });
                            }
                            if ui.button("Cancel").clicked() {
                                self.home_member.cancel();
                            }
                        });
                    }
                    MemberDisplay::SignedIn(member) => {
                        ui.label(RichText::new(&member.display_name).strong());
                        ui.label(
                            RichText::new(format!("@{}", member.handle))
                                .monospace()
                                .size(12.0),
                        );
                        ui.add_space(10.0);
                        ui.horizontal(|ui| {
                            if ui.button("Manage account").clicked() {
                                ctx.open_url(egui::OpenUrl::new_tab(
                                    "https://aeowun.com/members/".to_owned(),
                                ));
                            }
                            if ui.button("Switch account").clicked() {
                                self.home_member.switch_account();
                            }
                            if ui.button("Sign out").clicked() {
                                self.home_member.sign_out();
                            }
                        });
                    }
                }
            });
        self.home_member.dialog_open = open;
    }

    pub(crate) fn draw_home_footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;

            ui.label(
                RichText::new("BETA")
                    .monospace()
                    .strong()
                    .color(COLOR_ACCENT_ORANGE)
                    .size(10.0),
            );

            if ui.available_width() >= 600.0 {
                ui.label(
                    RichText::new(
                        "Features marked BETA are still in development and may change as they mature.",
                    )
                    .size(10.0)
                    .color(ui.visuals().widgets.noninteractive.fg_stroke.color),
                );
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .small_button(RichText::new("AEOWUN MEMBER").monospace().size(10.0))
                    .clicked()
                {
                    self.home_member.dialog_open = true;
                }
            });
        });
    }

    pub(crate) fn draw_asset_pack_manager_dialog(&mut self, ctx: &egui::Context) {
        let signal = egui::Id::new("wk_open_project_asset_packs");
        if ctx.data(|data| data.get_temp::<bool>(signal).unwrap_or(false)) {
            ctx.data_mut(|data| data.insert_temp(signal, false));
            self.asset_manager_packs_open = true;
            if self.asset_manager_packs.project != self.project_manager.current_project {
                self.asset_manager_packs = NewProjectPacks::default();
                self.asset_manager_packs.project = self.project_manager.current_project.clone();
            }
            self.asset_manager_packs.selected.clear();
        }
        if !self.asset_manager_packs_open {
            return;
        }
        self.asset_manager_packs.load();
        let mut open = true;
        let mut install = false;
        let rect = ctx.available_rect().shrink2(egui::vec2(14.0, 14.0));
        egui::Window::new("Asset Catalogue")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .fixed_pos(rect.min)
            .fixed_size(rect.size())
            .show(ctx, |ui| {
                ui.set_min_size(rect.size() - egui::vec2(24.0, 48.0));
                asset_catalogue_body(ui, &mut self.asset_manager_packs);
                ui.horizontal(|ui| {
                    if ui
                        .add_enabled(
                            self.asset_manager_packs.catalog.is_some()
                                && !self.asset_manager_packs.selected.is_empty(),
                            egui::Button::new("Install Selected"),
                        )
                        .clicked()
                    {
                        install = true;
                    }
                    if ui.button("Skip").clicked() {
                        self.asset_manager_packs_open = false;
                    }
                });
            });
        if !open {
            self.asset_manager_packs_open = false;
        }
        if let Some((character, mode)) = self.asset_manager_packs.characters.request.take() {
            if let Some(project) = self.asset_manager_packs.project.clone() {
                self.background_character_jobs.push(
                    super::new_project_character::CharacterSetupJob::start(
                        project, character, mode,
                    ),
                );
                self.asset_manager_packs_open = false;
            }
        }
        if install {
            if let Some(project) = self.asset_manager_packs.project.clone() {
                if self.pack_job_running(&project) {
                    self.asset_manager_packs.error =
                        Some("Asset packs are already installing for this project.".into());
                } else if let Some(catalog) = &self.asset_manager_packs.catalog {
                    match super::pack_install_queue::PackInstallQueue::start(
                        catalog,
                        &self.asset_manager_packs.selected,
                        crate::project::asset_packs::cache_root(),
                        project,
                    ) {
                        Ok(job) => {
                            self.background_asset_pack_jobs.push(job);
                            self.asset_manager_packs.error = None;
                            self.asset_manager_packs_open = false;
                        }
                        Err(error) => self.asset_manager_packs.error = Some(error),
                    }
                }
            }
        }
    }

    pub(crate) fn draw_home_dialogs(&mut self, ctx: &egui::Context, next_view: &mut Option<View>) {
        if self.show_new_project_dialog {
            let mut cancel = false;
            let mut create = None;
            if self.new_project_catalog_open {
                self.new_project_packs.load();
                let rect = ctx.available_rect().shrink2(egui::vec2(14.0, 14.0));
                egui::Window::new("New Project - Asset Catalogue")
                    .collapsible(false)
                    .resizable(false)
                    .fixed_pos(rect.min)
                    .fixed_size(rect.size())
                    .show(ctx, |ui| {
                        ui.set_min_size(rect.size() - egui::vec2(24.0, 48.0));
                        ui.label(format!("Project: {}", self.new_project_name));
                        asset_catalogue_body(ui, &mut self.new_project_packs);
                        ui.horizontal(|ui| {
                            if ui.button("Back to Setup").clicked() {
                                self.new_project_catalog_open = false;
                            }
                            let accent = ui.visuals().selection.bg_fill;
                            let text = ui.visuals().selection.stroke.color;
                            if ui
                                .add_enabled(
                                    self.new_project_packs.catalog.is_some(),
                                    egui::Button::new(
                                        RichText::new("Create Project").strong().color(text),
                                    )
                                    .fill(accent)
                                    .min_size(egui::vec2(144.0, 32.0)),
                                )
                                .clicked()
                            {
                                create = Some(true);
                            }
                        });
                    });
            } else {
                self.new_project_packs.characters.load();
                egui::Window::new("New Project")
                    .collapsible(false)
                    .resizable(false)
                    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                    .fixed_size(egui::vec2(550.0, 316.0))
                    .show(ctx, |ui| {
                        ui.add_space(8.0);
                        ui.strong("PROJECT NAME");
                        if ui
                            .add_sized(
                                [ui.available_width().min(500.0), 30.0],
                                egui::TextEdit::singleline(&mut self.new_project_name),
                            )
                            .changed()
                        {
                            self.new_project_packs.creation_error = None;
                            self.new_project_name_warning = None;
                        }
                        ui.add_space(10.0);
                        ui.strong("PROJECT TYPE");
                        ui.horizontal(|ui| {
                            use crate::project::character_offerings::Setup;
                            ui.selectable_value(&mut self.new_project_type, Setup::Blank, "Blank");
                            ui.selectable_value(
                                &mut self.new_project_type,
                                Setup::ThirdPerson,
                                "Third Person",
                            );
                            ui.selectable_value(
                                &mut self.new_project_type,
                                Setup::TopDown,
                                "Top Down",
                            );
                        });
                        ui.add_space(10.0);
                        ui.strong("STARTING CHARACTER");
                        if self.new_project_type
                            == crate::project::character_offerings::Setup::Blank
                        {
                            ui.weak("Not required for a Blank project.");
                        } else {
                            self.new_project_packs.characters.choose(ui);
                        }
                        ui.add_space(8.0);
                        if let Some(error) = &self.new_project_packs.creation_error {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        if self
                            .new_project_name_warning
                            .is_some_and(|time| time.elapsed() < std::time::Duration::from_secs(3))
                        {
                            ui.colored_label(
                                ui.visuals().warn_fg_color,
                                "Enter a project name before selecting asset packs.",
                            );
                            ctx.request_repaint_after(std::time::Duration::from_millis(100));
                        } else {
                            self.new_project_name_warning = None;
                        }
                        ui.separator();
                        ui.horizontal(|ui| {
                            if ui.button("Cancel").clicked() {
                                cancel = true;
                            }
                            let named = project_name_entered_for_packs(&self.new_project_name);
                            let mut button = egui::Button::new("Asset Packs");
                            if !named {
                                button = button.fill(ui.visuals().widgets.noninteractive.bg_fill);
                            }
                            if ui.add(button).clicked() {
                                if named {
                                    self.new_project_catalog_open = true;
                                } else {
                                    self.new_project_name_warning = Some(std::time::Instant::now());
                                }
                            }
                            let accent = ui.visuals().selection.bg_fill;
                            let text = ui.visuals().selection.stroke.color;
                            if ui
                                .add(
                                    egui::Button::new(
                                        RichText::new("Create Project").strong().color(text),
                                    )
                                    .fill(accent),
                                )
                                .clicked()
                            {
                                create = Some(false);
                            }
                        });
                    });
            }
            if cancel {
                self.show_new_project_dialog = false;
                self.new_project_catalog_open = false;
                self.new_project_packs = NewProjectPacks::default();
                self.new_project_name.clear();
                self.new_project_type = crate::project::character_offerings::Setup::Blank;
                self.new_project_name_warning = None;
            }
            if let Some(with_packs) = create {
                self.create_from_project_setup(with_packs);
            }
        }

        if self.show_open_project_dialog {
            egui::Window::new("Open Project")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .frame(egui::Frame::window(&ctx.style()))
                .show(ctx, |ui| {
                    ui.add_space(8.0);

                    let projects = self.project_manager.list_projects();

                    if projects.is_empty() {
                        ui.label(
                            RichText::new("No projects found in UserData.")
                                .color(ui.visuals().widgets.noninteractive.fg_stroke.color)
                                .size(12.0),
                        );
                    } else {
                        egui::ScrollArea::vertical()
                            .max_height(300.0)
                            .show(ui, |ui| {
                                for path in projects {
                                    let name =
                                        path.file_name().unwrap_or_default().to_string_lossy();

                                    if ui
                                        .add(
                                            egui::Button::new(
                                                RichText::new(name.to_string()).strong(),
                                            )
                                            .min_size(egui::vec2(320.0, 32.0)),
                                        )
                                        .clicked()
                                    {
                                        self.request_open_project(path);
                                        self.show_open_project_dialog = false;
                                    }

                                    ui.add_space(6.0);
                                }
                            });
                    }

                    ui.add_space(16.0);

                    if ui
                        .add(
                            egui::Button::new(RichText::new("CANCEL").monospace())
                                .min_size(egui::vec2(90.0, 28.0)),
                        )
                        .clicked()
                    {
                        self.show_open_project_dialog = false;
                    }

                    ui.add_space(8.0);
                });
        }

        if self.show_exit_confirmation_dialog {
            egui::Window::new("Exit Worldkiln?")
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .collapsible(false)
                .resizable(false)
                .frame(egui::Frame::window(&ctx.style()))
                .show(ctx, |ui| {
                    ui.label(RichText::new("Are you sure you want to exit?").monospace());

                    ui.add_space(14.0);

                    ui.horizontal(|ui| {
                        let yes_btn = egui::Button::new(
                            RichText::new("YES")
                                .monospace()
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .min_size(egui::vec2(70.0, 28.0))
                        .fill(COLOR_ACCENT_ORANGE);

                        if ui.add(yes_btn).clicked() {
                            self.show_exit_confirmation_dialog = false;
                            self.exit_requested = true;
                        }

                        if ui
                            .add(
                                egui::Button::new(RichText::new("NO").monospace())
                                    .min_size(egui::vec2(70.0, 28.0)),
                            )
                            .clicked()
                        {
                            self.show_exit_confirmation_dialog = false;
                        }
                    });
                });
        }
    }
}

#[cfg(test)]
mod new_project_setup_tests {
    use super::*;

    struct Scratch(std::path::PathBuf);
    impl Scratch {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "wk-new-project-queue-{:032x}",
                rand::random::<u128>()
            ));
            std::fs::create_dir(&path).unwrap();
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn asset_packs_reject_unnamed_projects_before_opening_catalogue() {
        assert!(!super::project_name_entered_for_packs(""));
        assert!(!super::project_name_entered_for_packs("   "));
        assert!(!super::project_name_entered_for_packs("\t\n"));
        assert!(super::project_name_entered_for_packs("My New Game"));
    }

    #[test]
    fn blank_project_starts_core_loader_without_catalogue_or_pack_queue() {
        let scratch = Scratch::new();
        let mut app = App::new_for_tests(640.0, 480.0);
        app.project_manager.user_data_dir = scratch.0.clone();
        app.new_project_name = "Blank Project".into();
        app.show_new_project_dialog = true;
        app.create_from_project_setup(false);
        assert!(!app.show_new_project_dialog);
        assert!(!app.new_project_catalog_open);
        assert!(app.project_loading(), "Core loader must start immediately");
        assert!(app.background_asset_pack_jobs.is_empty());
        let project = app.project_manager.current_project.as_ref().unwrap();
        assert!(project.join("world.dat").is_file());
        assert!(project.join("scripts").is_dir());
        assert!(project.join("playerscripts").is_dir());
    }
}
