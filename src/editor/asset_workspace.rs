// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{AssetManagerState, BROWSER_CATEGORIES, BrowserAsset, BrowserSort, format_asset_size};
use crate::editor::editor::asset_browser::{CONTENT_CATEGORIES, project_asset_snapshot};
use egui::{Align, Color32, Layout, Pos2, Rect, RichText, ScrollArea, Stroke, Ui};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

const IMPORTED_ACTORS: &str = "@imported-actors";
const TILE_W: f32 = 160.0;
const GRID_ROW_H: f32 = 166.0;
const LIST_ROW_H: f32 = 44.0;

#[derive(Clone, Debug, PartialEq, Eq)]
enum Entry {
    Category(&'static str, usize),
    Folder(String),
    File(usize),
    ActorPackage(usize),
    ActorLetter(char, usize),
}

fn is_package_category(category: &str) -> bool {
    matches!(category, "Actors" | "Characters")
}

fn root_packages(state: &AssetManagerState, category: &str) -> Vec<usize> {
    if state.catalog_loaded {
        return match category {
            "Actors" => state.actor_package_indices.clone(),
            "Characters" => state.character_package_indices.clone(),
            _ => Vec::new(),
        };
    }
    state
        .browser_files
        .iter()
        .enumerate()
        .filter(|(_, a)| a.is_directory && a.category == category)
        .map(|(i, _)| i)
        .collect()
}

fn category_count(state: &AssetManagerState, category: &str) -> usize {
    if state.catalog_loaded {
        if let Some(project) = state.catalog_project.as_deref() {
            return project_asset_snapshot(project)
                .assets
                .iter()
                .filter(|record| record.kind.category() == category)
                .count();
        }
        return state
            .browser_category_counts
            .get(category)
            .copied()
            .unwrap_or(0);
    }
    if is_package_category(category) {
        root_packages(state, category).len()
    } else {
        state
            .browser_files
            .iter()
            .filter(|a| a.category == category && !a.is_directory)
            .count()
    }
}

fn actor_label<'a>(state: &'a AssetManagerState, index: usize) -> &'a str {
    let asset = &state.browser_files[index];
    state
        .actor_display_names
        .get(&asset.path)
        .map(String::as_str)
        .unwrap_or_else(|| asset.name.rsplit('/').next().unwrap_or(&asset.name))
}

fn actor_initial(name: &str) -> char {
    name.chars()
        .next()
        .map(|c| c.to_ascii_uppercase())
        .filter(|c| c.is_ascii_alphanumeric())
        .unwrap_or('#')
}

const LEGACY_GROUP: &str = "Legacy";
fn actor_group<'a>(state: &'a AssetManagerState, actor: &str) -> &'a str {
    state
        .actor_import_groups
        .get(actor)
        .or_else(|| {
            state
                .actor_import_groups_folded
                .get(&actor.to_ascii_lowercase())
        })
        .map(String::as_str)
        .unwrap_or(LEGACY_GROUP)
}

fn actor_groups(state: &AssetManagerState, category: &str) -> Vec<(String, usize)> {
    let mut groups = std::collections::BTreeMap::<String, usize>::new();
    for i in root_packages(state, category) {
        *groups
            .entry(actor_group(state, &state.browser_files[i].name).to_string())
            .or_default() += 1;
    }
    let mut results: Vec<_> = groups.into_iter().collect();
    results.sort_by(|a, b| a.0.to_ascii_lowercase().cmp(&b.0.to_ascii_lowercase()));
    results
}

fn actor_letters(state: &AssetManagerState, group: &str) -> Vec<(char, usize)> {
    let mut map = std::collections::BTreeMap::<char, usize>::new();
    for i in root_packages(state, "Actors") {
        let asset = &state.browser_files[i];
        if actor_group(state, &asset.name) == group {
            *map.entry(actor_initial(actor_label(state, i))).or_default() += 1;
        }
    }
    map.into_iter().collect()
}

fn selected_actor_group(folder: &str) -> Option<&str> {
    let group = folder
        .strip_prefix(&format!("{IMPORTED_ACTORS}/"))?
        .split('/')
        .next()?;
    (!group.is_empty()).then_some(group)
}

fn actor_bucket(folder: &str) -> Option<char> {
    let remainder = folder.strip_prefix(&format!("{IMPORTED_ACTORS}/"))?;
    let (_, letter) = remainder.split_once('/')?;
    if letter.chars().count() != 1 {
        return None;
    }
    letter
        .chars()
        .next()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '#')
}

fn direct_folders(files: &[BrowserAsset], category: &str, folder: &str) -> Vec<String> {
    let mut result = BTreeSet::new();
    let actual_folder = if folder == IMPORTED_ACTORS {
        ""
    } else {
        folder
    };
    for asset in files.iter().filter(|a| a.category == category) {
        let remainder = if actual_folder.is_empty() {
            asset.name.as_str()
        } else if let Some(tail) = asset
            .name
            .strip_prefix(actual_folder)
            .and_then(|tail| tail.strip_prefix('/'))
        {
            tail
        } else {
            continue;
        };
        if let Some((segment, _)) = remainder.split_once('/') {
            if !segment.is_empty() {
                result.insert(segment.to_string());
            }
        } else if asset.is_directory && !remainder.is_empty() {
            result.insert(remainder.to_string());
        }
    }
    result.into_iter().collect()
}

fn entries(state: &AssetManagerState) -> Vec<Entry> {
    let query = state.search.trim().to_ascii_lowercase();
    let category = state.browser_category.as_str();
    let folder = state.browser_folder.as_str();
    let searching = !query.is_empty();

    if category.is_empty() && !searching {
        return CONTENT_CATEGORIES
            .iter()
            .map(|cat| (*cat, category_count(state, cat)))
            .map(|(cat, count)| Entry::Category(cat, count))
            .collect();
    }

    if is_package_category(category) {
        if !searching && folder.is_empty() {
            return actor_groups(state, category)
                .into_iter()
                .map(|(name, _)| Entry::Folder(name))
                .collect();
        }
        return root_packages(state, category)
            .into_iter()
            .filter(|i| {
                let asset = &state.browser_files[*i].name;
                if searching {
                    actor_label(state, *i).to_ascii_lowercase().contains(&query)
                        || actor_group(state, asset)
                            .to_ascii_lowercase()
                            .contains(&query)
                } else {
                    actor_group(state, asset) == folder
                }
            })
            .map(Entry::ActorPackage)
            .collect();
    }

    if category == "Animations" && !searching {
        if let Some(project) = state.catalog_project.as_deref() {
            let registry = project_asset_snapshot(project);
            let mut groups = std::collections::BTreeMap::<String, Vec<usize>>::new();
            for (index, asset) in state.browser_files.iter().enumerate() {
                if asset.category != "Animations" || asset.is_directory {
                    continue;
                }
                let record = registry.assets.iter().find(|record| {
                    project.join(&record.path) == asset.path
                        || project
                            .join(&record.path)
                            .join(crate::animation::asset::ANIMATION_FILE)
                            == asset.path
                });
                let owner = record
                    .and_then(|record| record.dependencies.iter().find_map(|id| registry.find(id)));
                let group = owner
                    .map(|owner| format!("{} [{}]", owner.name, owner.id))
                    .unwrap_or_else(|| "Unassigned".into());
                groups.entry(group).or_default().push(index);
            }
            if folder.is_empty() {
                return groups.into_keys().map(Entry::Folder).collect();
            }
            return groups
                .remove(folder)
                .unwrap_or_default()
                .into_iter()
                .map(Entry::File)
                .collect();
        }
    }
    let mut result = Vec::new();
    if !searching && !category.is_empty() {
        result.extend(
            direct_folders(&state.browser_files, category, folder)
                .into_iter()
                .map(Entry::Folder),
        );
    }
    if category == "Actors" && searching {
        result.extend(
            root_packages(state, "Actors")
                .into_iter()
                .filter(|i| {
                    let actor = &state.browser_files[*i].name;
                    actor.to_ascii_lowercase().contains(&query)
                        || actor_label(state, *i).to_ascii_lowercase().contains(&query)
                        || actor_group(state, actor)
                            .to_ascii_lowercase()
                            .contains(&query)
                })
                .map(Entry::ActorPackage),
        );
    } else {
        for (i, asset) in state.browser_files.iter().enumerate() {
            if asset.is_directory || (!category.is_empty() && asset.category != category) {
                continue;
            }
            if searching {
                if !asset.name.to_ascii_lowercase().contains(&query) {
                    continue;
                }
            } else {
                let parent = asset.name.rsplit_once('/').map(|(p, _)| p).unwrap_or("");
                if parent != folder {
                    continue;
                }
            }
            result.push(Entry::File(i));
        }
    }
    result.sort_by(|a, b| {
        let group = |e: &Entry| match e {
            Entry::Folder(_) | Entry::Category(_, _) | Entry::ActorLetter(_, _) => 0,
            _ => 1,
        };
        let name = |e: &Entry| match e {
            Entry::Folder(n) => n.to_ascii_lowercase(),
            Entry::Category(n, _) => n.to_ascii_lowercase(),
            Entry::ActorLetter(n, _) => n.to_string(),
            Entry::ActorPackage(i) => actor_label(state, *i).to_ascii_lowercase(),
            Entry::File(i) => state.browser_files[*i]
                .name
                .rsplit('/')
                .next()
                .unwrap_or("")
                .to_ascii_lowercase(),
        };
        group(a).cmp(&group(b)).then_with(|| {
            let kind = |e: &Entry| match e {
                Entry::ActorPackage(_) => "Actor package",
                Entry::File(i) => state.browser_files[*i]
                    .path
                    .extension()
                    .and_then(|v| v.to_str())
                    .unwrap_or(""),
                _ => "",
            };
            let size = |e: &Entry| match e {
                Entry::File(i) => state.browser_files[*i].bytes,
                _ => 0,
            };
            match state.browser_sort {
                BrowserSort::Name => name(a).cmp(&name(b)),
                BrowserSort::Category => kind(a).cmp(kind(b)).then_with(|| name(a).cmp(&name(b))),
                BrowserSort::Size => size(b).cmp(&size(a)).then_with(|| name(a).cmp(&name(b))),
            }
        })
    });
    result
}

fn short_label(name: &str, max: usize) -> String {
    let length = name.chars().count();
    if length <= max {
        return name.to_owned();
    }
    format!(
        "{}...",
        name.chars().take(max.saturating_sub(3)).collect::<String>()
    )
}

fn asset_name_label(name: &str) -> String {
    if name.chars().count() <= 13 {
        name.to_owned()
    } else {
        format!("{}…", name.chars().take(12).collect::<String>())
    }
}

fn fitted_label(ui: &Ui, label: &str, width_px: f32, font: &egui::FontId) -> String {
    let measure = |text: &str| {
        ui.painter()
            .layout_no_wrap(text.to_string(), font.clone(), Color32::WHITE)
            .rect
            .width()
    };
    if measure(label) <= width_px.max(1.0) {
        return label.to_string();
    }
    let chars: Vec<char> = label.chars().collect();
    let mut low = 0usize;
    let mut high = chars.len();
    while low < high {
        let mid = (low + high + 1) / 2;
        let candidate = format!("{}…", chars[..mid].iter().collect::<String>());
        if measure(&candidate) <= width_px.max(1.0) {
            low = mid;
        } else {
            high = mid - 1;
        }
    }
    format!("{}…", chars[..low].iter().collect::<String>())
}

fn parent_folder(folder: &str) -> String {
    if folder == IMPORTED_ACTORS {
        return String::new();
    }
    folder
        .rsplit_once('/')
        .map(|(p, _)| p.to_string())
        .unwrap_or_default()
}

fn open_folder(folder: &str, child: &str) -> String {
    if child == IMPORTED_ACTORS {
        return IMPORTED_ACTORS.into();
    }
    if folder == IMPORTED_ACTORS {
        return format!("{IMPORTED_ACTORS}/{child}");
    }
    if folder.is_empty() {
        child.to_string()
    } else {
        format!("{folder}/{child}")
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Symbol {
    Folder,
    Package,
    Image,
    Audio,
    Video,
    Script,
    Camera,
    Material,
    File,
}

fn category_symbol(category: &str) -> Symbol {
    match category {
        "Actors" | "Characters" | "Models" => Symbol::Package,
        "Textures" | "Images" => Symbol::Image,
        "Materials" => Symbol::Material,
        "Audio" => Symbol::Audio,
        "Video" => Symbol::Video,
        "Scripts" | "Controllers" => Symbol::Script,
        "Cameras" => Symbol::Camera,
        _ => Symbol::File,
    }
}

fn entry_symbol(state: &AssetManagerState, entry: &Entry) -> Symbol {
    match entry {
        Entry::Category(name, _) => category_symbol(name),
        Entry::Folder(_) | Entry::ActorLetter(_, _) => Symbol::Folder,
        Entry::ActorPackage(_) => Symbol::Package,
        Entry::File(i) => category_symbol(state.browser_files[*i].category),
    }
}

fn paint_symbol(ui: &Ui, symbol: Symbol, center: Pos2, scale: f32, color: Color32) {
    let painter = ui.painter();
    let s = scale;
    let stroke = Stroke::new((s * 0.075).clamp(1.4, 2.3), color);
    match symbol {
        Symbol::Folder => {
            let body =
                Rect::from_center_size(center + egui::vec2(0.0, s * 0.07), egui::vec2(s, s * 0.64));
            painter.rect_filled(body, 3.0, color.gamma_multiply(0.20));
            painter.rect_stroke(body, 3.0, stroke);
            let tab = Rect::from_min_size(
                egui::pos2(body.left() + s * 0.08, body.top() - s * 0.15),
                egui::vec2(s * 0.42, s * 0.17),
            );
            painter.rect_filled(tab, 2.0, color.gamma_multiply(0.65));
            painter.line_segment(
                [
                    egui::pos2(body.left() + s * 0.16, body.bottom() - s * 0.20),
                    egui::pos2(body.right() - s * 0.16, body.bottom() - s * 0.20),
                ],
                stroke,
            );
        }
        Symbol::Package => {
            let t = center + egui::vec2(0.0, -s * 0.39);
            let l = center + egui::vec2(-s * 0.43, -s * 0.20);
            let r = center + egui::vec2(s * 0.43, -s * 0.20);
            let m = center + egui::vec2(0.0, 0.0);
            let lb = l + egui::vec2(0.0, s * 0.55);
            let rb = r + egui::vec2(0.0, s * 0.55);
            let mb = m + egui::vec2(0.0, s * 0.55);
            for (a, b) in [
                (t, l),
                (t, r),
                (l, m),
                (m, r),
                (l, lb),
                (r, rb),
                (m, mb),
                (lb, mb),
                (mb, rb),
            ] {
                painter.line_segment([a, b], stroke);
            }
        }
        Symbol::Image => {
            let body = Rect::from_center_size(center, egui::vec2(s, s * 0.70));
            painter.rect_filled(body, 3.0, color.gamma_multiply(0.10));
            painter.rect_stroke(body, 3.0, stroke);
            painter.circle_filled(center + egui::vec2(s * 0.23, -s * 0.15), s * 0.09, color);
            painter.add(egui::Shape::line(
                vec![
                    egui::pos2(body.left() + s * 0.10, body.bottom() - s * 0.11),
                    egui::pos2(center.x - s * 0.14, center.y - s * 0.06),
                    egui::pos2(center.x + s * 0.06, center.y + s * 0.13),
                    egui::pos2(center.x + s * 0.18, center.y + s * 0.03),
                    egui::pos2(body.right() - s * 0.08, body.bottom() - s * 0.11),
                ],
                stroke,
            ));
        }
        Symbol::Audio => {
            for (i, h) in [0.26, 0.52, 0.80, 0.49, 0.27].into_iter().enumerate() {
                let x = center.x + (i as f32 - 2.0) * s * 0.21;
                painter.line_segment(
                    [
                        egui::pos2(x, center.y - s * h * 0.5),
                        egui::pos2(x, center.y + s * h * 0.5),
                    ],
                    Stroke::new(s * 0.085, color),
                );
            }
        }
        Symbol::Video => {
            let body = Rect::from_center_size(center, egui::vec2(s, s * 0.70));
            painter.rect_stroke(body, 3.0, stroke);
            painter.add(egui::Shape::convex_polygon(
                vec![
                    center + egui::vec2(-s * 0.13, -s * 0.18),
                    center + egui::vec2(s * 0.23, 0.0),
                    center + egui::vec2(-s * 0.13, s * 0.18),
                ],
                color,
                Stroke::NONE,
            ));
        }
        Symbol::Camera => {
            let body = Rect::from_center_size(center, egui::vec2(s, s * 0.63));
            painter.rect_stroke(body, 3.0, stroke);
            painter.circle_stroke(center, s * 0.20, stroke);
            painter.rect_filled(
                Rect::from_min_size(
                    body.min + egui::vec2(s * 0.12, -s * 0.11),
                    egui::vec2(s * 0.34, s * 0.13),
                ),
                2.0,
                color,
            );
        }
        Symbol::Material => {
            painter.circle_filled(center, s * 0.4, color.gamma_multiply(0.4));
            painter.circle_stroke(center, s * 0.4, stroke);
        }
        Symbol::Script | Symbol::File => {
            let body = Rect::from_center_size(center, egui::vec2(s * 0.70, s));
            painter.rect_stroke(body, 3.0, stroke);
            for i in 0..3 {
                let y = center.y - s * 0.20 + (i as f32) * s * 0.20;
                painter.line_segment(
                    [
                        egui::pos2(body.left() + s * 0.11, y),
                        egui::pos2(body.right() - s * 0.11, y),
                    ],
                    stroke,
                );
            }
        }
    }
}

fn navigation_row(
    ui: &mut Ui,
    label: &str,
    count: Option<usize>,
    selected: bool,
    symbol: Symbol,
    indent: f32,
) -> egui::Response {
    let width = ui.available_width().max(100.0);
    let (rect, response) = ui.allocate_exact_size(egui::vec2(width, 34.0), egui::Sense::click());
    let visuals = ui.visuals();
    let accent = visuals.selection.stroke.color;
    let hover = response.hovered();
    if selected {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(2.0, 1.0)),
            4.0,
            accent.gamma_multiply(0.18),
        );
        ui.painter().rect_filled(
            Rect::from_min_size(
                rect.min + egui::vec2(2.0, 5.0),
                egui::vec2(3.0, rect.height() - 10.0),
            ),
            2.0,
            accent,
        );
    } else if hover {
        ui.painter().rect_filled(
            rect.shrink2(egui::vec2(2.0, 2.0)),
            4.0,
            visuals.widgets.hovered.weak_bg_fill.gamma_multiply(0.45),
        );
    }
    let icon_center = egui::pos2(rect.left() + 18.0 + indent, rect.center().y);
    paint_symbol(
        ui,
        symbol,
        icon_center,
        15.0,
        if selected {
            accent
        } else {
            visuals.weak_text_color()
        },
    );
    let right_margin = if count.is_some() { 54.0 } else { 10.0 };
    let visible_label = fitted_label(
        ui,
        label,
        rect.width() - indent - 39.0 - right_margin,
        &egui::TextStyle::Body.resolve(ui.style()),
    );
    let truncated = visible_label != label;
    ui.painter().text(
        egui::pos2(icon_center.x + 18.0, rect.center().y),
        egui::Align2::LEFT_CENTER,
        visible_label,
        egui::TextStyle::Body.resolve(ui.style()),
        visuals.text_color(),
    );
    if let Some(n) = count {
        ui.painter().text(
            egui::pos2(rect.right() - 10.0, rect.center().y),
            egui::Align2::RIGHT_CENTER,
            n.to_string(),
            egui::TextStyle::Small.resolve(ui.style()),
            visuals.weak_text_color(),
        );
    }
    let response = response.on_hover_cursor(egui::CursorIcon::PointingHand);
    if truncated {
        response.on_hover_text(label)
    } else {
        response
    }
}

fn source_panel(ui: &mut Ui, state: &mut AssetManagerState) {
    ui.label(
        RichText::new("PROJECT LIBRARY")
            .strong()
            .size(11.0)
            .color(ui.visuals().weak_text_color()),
    );
    ui.add_space(10.0);
    ScrollArea::vertical()
        .id_salt("wk_sources_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            if navigation_row(
                ui,
                "All Assets",
                None,
                state.browser_category.is_empty(),
                Symbol::Folder,
                0.0,
            )
            .clicked()
            {
                state.browser_category.clear();
                state.browser_folder.clear();
                state.browser_selected = None;
            }
            ui.add_space(8.0);
            ui.label(
                RichText::new("ASSET TYPES")
                    .strong()
                    .size(10.0)
                    .color(ui.visuals().weak_text_color()),
            );
            ui.add_space(5.0);
            for &label in &CONTENT_CATEGORIES {
                let count = category_count(state, label);
                let selected = state.browser_category == label;
                if navigation_row(
                    ui,
                    label,
                    Some(count),
                    selected,
                    category_symbol(label),
                    0.0,
                )
                .clicked()
                {
                    state.browser_category = label.to_string();
                    state.browser_folder.clear();
                    state.browser_selected = None;
                }
            }
            if !state.browser_category.is_empty() {
                ui.add_space(13.0);
                ui.label(
                    RichText::new("FOLDERS")
                        .strong()
                        .size(10.0)
                        .color(ui.visuals().weak_text_color()),
                );
                ui.add_space(5.0);
                if navigation_row(
                    ui,
                    "Category root",
                    None,
                    state.browser_folder.is_empty(),
                    Symbol::Folder,
                    0.0,
                )
                .clicked()
                {
                    state.browser_folder.clear();
                    state.browser_selected = None;
                }
                let folder = state.browser_folder.clone();
                if !folder.is_empty() {
                    let mut parent = String::new();
                    let segments: Vec<&str> = folder.split('/').collect();
                    for (level, segment) in segments.iter().enumerate() {
                        parent = open_folder(&parent, segment);
                        if navigation_row(
                            ui,
                            if *segment == IMPORTED_ACTORS {
                                "Imported Actors"
                            } else {
                                segment
                            },
                            None,
                            parent == folder,
                            Symbol::Folder,
                            ((level + 1).min(3) as f32) * 10.0,
                        )
                        .clicked()
                        {
                            state.browser_folder = if state.browser_folder == parent {
                                parent_folder(&parent)
                            } else {
                                parent.clone()
                            };
                            state.browser_selected = None;
                        }
                    }
                }
                let children: Vec<(String, String, Option<usize>)> =
                    if is_package_category(&state.browser_category) {
                        if folder.is_empty() {
                            actor_groups(state, &state.browser_category)
                                .into_iter()
                                .map(|(group, count)| (group.clone(), group, Some(count)))
                                .collect()
                        } else {
                            Vec::new()
                        }
                    } else {
                        direct_folders(&state.browser_files, &state.browser_category, &folder)
                            .into_iter()
                            .map(|s| (s.clone(), s, None))
                            .collect()
                    };
                ScrollArea::vertical()
                    .id_salt("wk_folder_tree")
                    .max_height(280.0)
                    .show_rows(ui, 34.0, children.len(), |ui, rows| {
                        for row in rows {
                            let (key, label, count) = &children[row];
                            if navigation_row(ui, label, *count, false, Symbol::Folder, 10.0)
                                .clicked()
                            {
                                state.browser_folder = open_folder(&folder, key);
                                state.browser_selected = None;
                            }
                        }
                    });
            }
        });
}

fn entry_label(state: &AssetManagerState, entry: &Entry) -> (String, String) {
    match entry {
        Entry::Category(label, count) => (
            label.to_string(),
            format!(
                "{count} {}",
                if is_package_category(label) {
                    "packages"
                } else {
                    "assets"
                }
            ),
        ),
        Entry::Folder(folder) if folder == IMPORTED_ACTORS => (
            "Imported Actors".into(),
            format!("{} packages", category_count(state, "Actors")),
        ),
        Entry::Folder(folder) => (folder.clone(), "Folder".into()),
        Entry::ActorLetter(letter, count) => (format!("{letter}"), format!("{count} packages")),
        Entry::ActorPackage(i) => (
            actor_label(state, *i).to_string(),
            if state.browser_files[*i].category == "Characters" {
                "Character".into()
            } else {
                "Actor".into()
            },
        ),
        Entry::File(i) => {
            let a = &state.browser_files[*i];
            (
                if a.category == "Materials" {
                    a.name
                        .rsplit('/')
                        .next()
                        .unwrap_or("")
                        .strip_suffix(".json")
                        .unwrap_or(a.name.rsplit('/').next().unwrap_or(""))
                        .to_string()
                } else {
                    a.name.rsplit('/').next().unwrap_or("").to_string()
                },
                format!("{}  {}", a.category, format_asset_size(a.bytes)),
            )
        }
    }
}

fn draw_entry(
    ui: &mut Ui,
    width: f32,
    height: f32,
    title: &str,
    display_title: &str,
    subtitle: &str,
    selected: bool,
    tile: bool,
    symbol: Symbol,
    thumbnail: Option<(egui::TextureId, egui::Vec2)>,
) -> egui::Response {
    let (rect, response) =
        ui.allocate_exact_size(egui::vec2(width.max(80.0), height), egui::Sense::click());
    let v = ui.visuals();
    let stroke = if selected {
        v.selection.stroke
    } else if response.hovered() {
        v.widgets.hovered.bg_stroke
    } else {
        v.widgets.inactive.bg_stroke
    };
    let bg = if selected {
        v.selection.bg_fill.gamma_multiply(0.35)
    } else {
        v.widgets.inactive.bg_fill
    };
    let card = rect.shrink2(egui::vec2(3.0, if tile { 3.0 } else { 2.0 }));
    ui.painter().rect_filled(card, 5.0, bg);
    ui.painter().rect_stroke(card, 5.0, stroke);
    let accent = v.selection.stroke.color;
    let glyph = if selected || response.hovered() {
        accent
    } else {
        v.weak_text_color()
    };
    let font = egui::TextStyle::Body.resolve(ui.style());
    let small = egui::TextStyle::Small.resolve(ui.style());
    let text = v.text_color();
    let secondary = v.weak_text_color();
    if tile {
        let preview = Rect::from_min_max(
            card.min + egui::vec2(7.0, 7.0),
            egui::pos2(card.right() - 7.0, card.bottom() - 51.0),
        );
        ui.painter().rect_filled(
            preview,
            3.0,
            v.widgets.noninteractive.weak_bg_fill.gamma_multiply(0.48),
        );
        if let Some((id, size)) = thumbnail {
            let fit = (preview.width() / size.x.max(1.0))
                .min(preview.height() / size.y.max(1.0))
                .min(1.0);
            let image = Rect::from_center_size(preview.center(), size * fit);
            ui.painter().image(
                id,
                image,
                Rect::from_min_max(egui::Pos2::ZERO, egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        } else {
            paint_symbol(ui, symbol, preview.center(), 44.0, glyph);
        }
        ui.painter().text(
            egui::pos2(card.left() + 11.0, card.bottom() - 40.0),
            egui::Align2::LEFT_CENTER,
            fitted_label(ui, display_title, width - 28.0, &font),
            font.clone(),
            text,
        );
        ui.painter().text(
            egui::pos2(card.left() + 11.0, card.bottom() - 19.0),
            egui::Align2::LEFT_CENTER,
            fitted_label(ui, subtitle, width - 28.0, &small),
            small,
            secondary,
        );
    } else {
        paint_symbol(
            ui,
            symbol,
            egui::pos2(card.left() + 23.0, card.center().y),
            24.0,
            glyph,
        );
        let available = (width * 0.57 - 48.0).max(42.0);
        ui.painter().text(
            egui::pos2(card.left() + 47.0, card.center().y),
            egui::Align2::LEFT_CENTER,
            fitted_label(ui, display_title, available, &font),
            font,
            text,
        );
        if width > 240.0 {
            ui.painter().text(
                egui::pos2(card.right() - 13.0, card.center().y),
                egui::Align2::RIGHT_CENTER,
                fitted_label(ui, subtitle, width * 0.38, &small),
                small,
                secondary,
            );
        }
    }
    response
        .on_hover_cursor(egui::CursorIcon::PointingHand)
        .on_hover_text(format!("{title}\n{subtitle}"))
}

fn texture_source_image(asset: &BrowserAsset) -> bool {
    asset.category == "Images"
        && !asset.is_directory
        && asset.path.extension().is_some_and(|ext| {
            ext.eq_ignore_ascii_case("png")
                || ext.eq_ignore_ascii_case("jpg")
                || ext.eq_ignore_ascii_case("jpeg")
        })
}

pub(super) fn create_texture_from_image(
    project: &Path,
    asset: &BrowserAsset,
) -> Result<PathBuf, String> {
    if !texture_source_image(asset) {
        return Err("Create Texture requires a PNG, JPG or JPEG in Images".into());
    }
    let images_root = project.join(".assets/media");
    let relative = asset
        .path
        .strip_prefix(&images_root)
        .map_err(|_| "Image must belong to the current project")?;
    let relative_image = format!(
        ".assets/media/{}",
        relative.to_string_lossy().replace('\\', "/")
    );
    super::asset_lifecycle::checked(project, &relative_image)?;
    if !asset.path.is_file() {
        return Err("Image file is missing".into());
    }
    let stem = relative
        .file_stem()
        .and_then(|name| name.to_str())
        .ok_or("Invalid image filename")?;
    let parent = relative.parent().unwrap_or(Path::new(""));
    let source_is_png = asset
        .path
        .extension()
        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"));
    let decoded = if source_is_png {
        None
    } else {
        let reader = image::ImageReader::open(&asset.path)
            .map_err(|e| format!("Cannot read source Image: {e}"))?
            .with_guessed_format()
            .map_err(|e| format!("Cannot identify source Image: {e}"))?;
        Some(
            reader
                .decode()
                .map_err(|e| format!("Cannot decode source Image: {e}"))?,
        )
    };
    for number in 0..=9999 {
        let filename = if number == 0 {
            format!("{stem}.png")
        } else {
            format!("{stem}_{number}.png")
        };
        let destination_relative = Path::new(".assets/textures").join(parent).join(filename);
        let relative_text = destination_relative.to_string_lossy().replace('\\', "/");
        let destination = super::asset_lifecycle::checked(project, &relative_text)?;
        if destination.exists() {
            continue;
        }
        std::fs::create_dir_all(destination.parent().ok_or("Invalid Texture directory")?)
            .map_err(|e| format!("Cannot create Texture directory: {e}"))?;
        super::asset_lifecycle::checked(project, &relative_text)?;
        let mut output = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Cannot create Texture: {error}")),
        };
        let copy = (|| -> Result<(), String> {
            if let Some(decoded) = &decoded {
                decoded
                    .write_to(&mut output, image::ImageFormat::Png)
                    .map_err(|e| format!("Cannot encode JPEG as PNG Texture: {e}"))?;
            } else {
                let mut input = std::fs::File::open(&asset.path).map_err(|e| e.to_string())?;
                std::io::copy(&mut input, &mut output).map_err(|e| e.to_string())?;
            }
            output.sync_all().map_err(|e| e.to_string())
        })();
        if let Err(error) = copy {
            drop(output);
            let _ = std::fs::remove_file(&destination);
            return Err(format!("Cannot create Texture: {error}"));
        }
        crate::project::asset_registry::AssetRegistry::reconcile(project)?;
        return Ok(destination);
    }
    Err("No available Texture filename".into())
}

fn duplicate_standalone_asset(project: &Path, entry: &BrowserAsset) -> Result<PathBuf, String> {
    if entry.is_directory
        || matches!(
            entry.category,
            "Materials" | "Actors" | "Characters" | "Controllers" | "Cameras" | "Models"
        )
    {
        let relative = entry
            .path
            .strip_prefix(project)
            .map_err(|_| "Asset outside project")?
            .to_string_lossy()
            .replace('\\', "/");
        return super::asset_lifecycle::duplicate(project, &relative);
    }
    if !matches!(
        entry.category,
        "Textures" | "Images" | "Audio" | "Video" | "Other Media" | "Scripts"
    ) {
        return Err(format!(
            "{} assets require package-aware duplication.",
            entry.category
        ));
    }
    let root = if entry.category == "Scripts" {
        project.join("scripts")
    } else {
        let Some((_, folder)) = BROWSER_CATEGORIES
            .iter()
            .find(|(label, _)| *label == entry.category)
        else {
            return Err("Unsupported asset type".into());
        };
        project.join(".assets").join(folder)
    };
    let canonical_root = root
        .canonicalize()
        .map_err(|e| format!("Asset root inaccessible: {e}"))?;
    let source = entry
        .path
        .canonicalize()
        .map_err(|e| format!("Asset inaccessible: {e}"))?;
    if !source.starts_with(&canonical_root) || !source.is_file() {
        return Err("Asset is outside its project-owned category.".into());
    }
    let parent = source.parent().ok_or("Asset has no parent directory")?;
    let stem = source
        .file_stem()
        .and_then(|s| s.to_str())
        .ok_or("Invalid asset filename")?;
    let extension = source.extension().and_then(|s| s.to_str()).unwrap_or("");
    for suffix in 1..=10000 {
        let name = if suffix == 1 {
            format!("{stem}_Copy")
        } else {
            format!("{stem}_Copy_{suffix}")
        };
        let filename = if extension.is_empty() {
            name
        } else {
            format!("{name}.{extension}")
        };
        let destination = parent.join(filename);
        let mut output = match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination)
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(format!("Could not create copy: {error}")),
        };
        let copied = (|| -> std::io::Result<()> {
            let mut input = std::fs::File::open(&source)?;
            std::io::copy(&mut input, &mut output)?;
            output.sync_all()?;
            Ok(())
        })();
        if let Err(error) = copied {
            drop(output);
            let _ = std::fs::remove_file(&destination);
            return Err(format!("Copy failed; incomplete copy removed: {error}"));
        }
        return Ok(destination);
    }
    Err("No available duplicate filename after 10,000 attempts".into())
}

fn managed_identity(
    project: &Path,
    asset: &BrowserAsset,
) -> Option<(super::ManagedAssetKind, String)> {
    let category_root = if asset.category == "Scripts" {
        project.join("scripts")
    } else {
        project.join(".assets").join(
            BROWSER_CATEGORIES
                .iter()
                .find(|(category, _)| *category == asset.category)?
                .1,
        )
    };
    if !asset.path.starts_with(&category_root) {
        return None;
    }
    if asset.is_directory {
        let relative = asset
            .path
            .strip_prefix(project)
            .ok()?
            .to_string_lossy()
            .replace('\\', "/");
        super::asset_lifecycle::validate_relative(&relative).ok()?;
        return Some((super::ManagedAssetKind::Path, relative));
    }
    let kind = match asset.category {
        "Textures"
            if asset
                .path
                .extension()?
                .to_str()?
                .eq_ignore_ascii_case("png") =>
        {
            super::ManagedAssetKind::Texture
        }
        "Audio" => super::ManagedAssetKind::Audio,
        "Materials"
            if asset
                .path
                .extension()?
                .to_str()?
                .eq_ignore_ascii_case("json") =>
        {
            super::ManagedAssetKind::Material
        }
        _ => {
            let relative = asset
                .path
                .strip_prefix(project)
                .ok()?
                .to_string_lossy()
                .replace('\\', "/");
            super::asset_lifecycle::validate_relative(&relative).ok()?;
            return Some((super::ManagedAssetKind::Path, relative));
        }
    };
    let root = project.join(".assets").join(match kind {
        super::ManagedAssetKind::Texture => "textures",
        super::ManagedAssetKind::Audio => "audio",
        super::ManagedAssetKind::Material => "materials",
        super::ManagedAssetKind::Path => return None,
    });
    let relative = asset.path.strip_prefix(root).ok()?;
    let mut identifier = relative.to_string_lossy().replace('\\', "/");
    if kind != super::ManagedAssetKind::Audio {
        identifier = relative
            .with_extension("")
            .to_string_lossy()
            .replace('\\', "/");
    }
    Some((kind, identifier))
}

fn paint_material_cube(
    ui: &Ui,
    preview: Rect,
    project: &Path,
    path: &Path,
    renderer: &crate::renderer::Renderer,
    catalog: &crate::project::material::MaterialCatalog,
) -> bool {
    let Ok(relative) = path.strip_prefix(project.join(".assets/materials")) else {
        return false;
    };
    let id = relative
        .with_extension("")
        .to_string_lossy()
        .replace(char::from(92), "/");
    let Some(document) = catalog.documents().get(&id) else {
        return false;
    };
    let Ok(crate::project::material::MaterialOutput::SurfacePbr {
        base_color,
        roughness,
        metallic,
    }) = catalog.resolve(&document.id)
    else {
        return false;
    };
    let maps = catalog.texture_maps(&document.id).unwrap_or_default();
    let graph = catalog.graph(&document.id).ok().flatten();
    let base = crate::project::material::MaterialOutput::SurfacePbr {
        base_color,
        roughness,
        metallic,
    };
    let compiled = renderer.surface_material_preview(
        preview,
        base_color,
        roughness,
        metallic,
        maps.get("BaseColor").map(String::as_str),
        graph.map(|g| (g, &base, &maps)),
        false,
    );
    let callback = compiled.or_else(|_| {
        renderer.surface_material_preview(
            preview,
            base_color,
            roughness,
            metallic,
            maps.get("BaseColor").map(String::as_str),
            None,
            false,
        )
    });
    if let Ok(callback) = callback {
        ui.painter().add(callback);
        true
    } else {
        false
    }
}

fn asset_view(
    ui: &mut Ui,
    state: &mut AssetManagerState,
    entries: &[Entry],
    project_path: &Path,
    refresh: &mut bool,
    renderer: Option<&crate::renderer::Renderer>,
) {
    let material_catalog = renderer.map(|renderer| renderer.material_snapshot());
    let mut texture_request: Option<usize> = None;
    let selected_convertible_image = state.browser_selected.as_ref().and_then(|path| {
        state
            .browser_files
            .iter()
            .position(|asset| asset.path == *path && texture_source_image(asset))
    });
    let title = if state.browser_category.is_empty() {
        "All Assets".into()
    } else if is_package_category(&state.browser_category) {
        if state.browser_folder.is_empty() {
            state.browser_category.clone()
        } else {
            state.browser_folder.clone()
        }
    } else if state.browser_folder.is_empty() {
        state.browser_category.clone()
    } else if state.browser_folder == IMPORTED_ACTORS {
        "Imported Actors".into()
    } else if let Some(letter) = actor_bucket(&state.browser_folder) {
        format!(
            "Imported Actors / {} / {letter}",
            selected_actor_group(&state.browser_folder).unwrap_or(LEGACY_GROUP)
        )
    } else if state.browser_category == "Actors" {
        selected_actor_group(&state.browser_folder)
            .unwrap_or("Actors")
            .to_owned()
    } else {
        state.browser_folder.rsplit('/').next().unwrap_or("").into()
    };
    ui.horizontal(|ui| {
        ui.heading(short_label(&title, 38));
        if let Some(index) = selected_convertible_image {
            if ui
                .button("Create Texture")
                .on_hover_text("Create a PNG Texture from the selected Image; keep the original")
                .clicked()
            {
                texture_request = Some(index);
            }
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.small(format!("{} items", entries.len()));
        });
    });
    if state.browser_category.is_empty() && state.search.trim().is_empty() {
        ui.small("Browse by asset type. Select a collection to open its folders and files.");
        ui.add_space(10.0);
    }
    ui.separator();
    ui.add_space(6.0);
    let grid = state.browser_grid;
    let view_width = ui.available_width().max(90.0);
    let cols = if grid {
        ((view_width / TILE_W).floor() as usize).max(1)
    } else {
        1
    };
    let gap = ui.spacing().item_spacing.x;
    let column_width =
        ((view_width - gap * (cols.saturating_sub(1)) as f32) / cols as f32).max(80.0);
    let row_h = if grid { GRID_ROW_H } else { LIST_ROW_H };
    let rows = entries.len().div_ceil(cols);
    let mut action: Option<(Entry, bool)> = None;
    let mut duplicate_request: Option<usize> = None;
    let mut rename_request: Option<usize> = None;
    let mut delete_request: Option<usize> = None;
    let mut material_request: Option<usize> = None;
    let mut character_request: Option<usize> = None;
    let mut animation_create_request: Option<usize> = None;
    let mut animation_retarget_request: Option<usize> = None;
    let mut animation_open_request: Option<usize> = None;
    let mut animation_import_request: Option<(usize, String)> = None;
    let mut thumbnail_loads = 2usize;
    ScrollArea::vertical()
        .id_salt("wk_asset_view_scroll")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, rows, |ui, visible| {
            for row in visible {
                ui.horizontal(|ui| {
                    for col in 0..cols {
                        let Some(entry) = entries.get(row * cols + col) else {
                            break;
                        };
                        let (label, detail) = entry_label(state, entry);
                        let display_label = if matches!(entry, Entry::File(_)) {
                            asset_name_label(&label)
                        } else {
                            label.clone()
                        };
                        let selected = match entry {
                            Entry::File(i) | Entry::ActorPackage(i) => {
                                state.browser_selected.as_ref()
                                    == Some(&state.browser_files[*i].path)
                            }
                            _ => false,
                        };
                        let mut thumbnail = None;
                        if grid {
                            if let Entry::File(i) = entry {
                                let path = state.browser_files[*i].path.clone();
                                if matches!(
                                    path.extension()
                                        .and_then(|e| e.to_str())
                                        .map(|e| e.to_ascii_lowercase())
                                        .as_deref(),
                                    Some("png" | "jpg" | "jpeg")
                                ) {
                                    if !state.browser_thumbnails.contains_key(&path)
                                        && !state.browser_thumbnail_failed.contains(&path)
                                        && thumbnail_loads > 0
                                    {
                                        thumbnail_loads -= 1;
                                        match super::load_browser_thumbnail(ui.ctx(), &path) {
                                            super::ThumbnailLoad::Ready(texture) => {
                                            if state.browser_thumbnail_order.len() >= 128 {
                                                if let Some(old) =
                                                    state.browser_thumbnail_order.pop_front()
                                                {
                                                    state.browser_thumbnails.remove(&old);
                                                }
                                            }
                                            state.browser_thumbnail_order.push_back(path.clone());
                                            state.browser_thumbnails.insert(
                                                path.clone(),
                                                super::BrowserPreview(texture),
                                            );
                                            }
                                            super::ThumbnailLoad::Failed => {
                                                state.browser_thumbnail_failed.insert(path.clone());
                                            }
                                            super::ThumbnailLoad::Pending => {
                                                ui.ctx().request_repaint_after(
                                                    std::time::Duration::from_millis(250));
                                            }
                                        }
                                    }
                                    thumbnail = state
                                        .browser_thumbnails
                                        .get(&path)
                                        .map(|texture| (texture.0.id(), texture.0.size_vec2()));
                                }
                            }
                        }
                        let response = draw_entry(
                            ui,
                            column_width,
                            row_h - 4.0,
                            &label,
                            &display_label,
                            &detail,
                            selected,
                            grid,
                            entry_symbol(state, entry),
                            thumbnail,
                        );
                        if grid {
                            if let Entry::File(i) | Entry::ActorPackage(i) = entry {
                                let asset = &state.browser_files[*i];
                                let preview = egui::Rect::from_min_max(
                                    response.rect.min + egui::vec2(11.0, 11.0),
                                    egui::pos2(response.rect.right() - 11.0,
                                        response.rect.bottom() - 58.0),
                                );
                                if matches!(entry, Entry::ActorPackage(_)) {
                                    crate::editor::actor_model_preview::paint_model(
                                        ui, preview, &asset.path,
                                    );
                                } else if asset.category == "Materials"
                                    && asset.path.extension()
                                        .is_some_and(|ext| ext.eq_ignore_ascii_case("json"))
                                {
                                    if let (Some(renderer), Some(catalog)) =
                                        (renderer, material_catalog.as_ref())
                                    {
                                        paint_material_cube(
                                            ui, preview, project_path, &asset.path,
                                            renderer, catalog,
                                        );
                                    }
                                }
                            }
                        }
                        let is_double = response.double_clicked();
                        let was_clicked = response.clicked();
                        let folder_index = if let Entry::Folder(folder) = entry {
                            let name = open_folder(&state.browser_folder, folder);
                            state.browser_files.iter().position(|asset| {
                                asset.is_directory
                                    && asset.category == state.browser_category
                                    && asset.name == name
                            })
                        } else {
                            None
                        };
                        response.context_menu(|ui| {
                            if let Entry::Folder(folder) = entry {
                                if is_package_category(&state.browser_category) {
                                    if ui.button("Rename Folder (visual only)").clicked() {
                                        state.folder_rename_source = Some(folder.clone());
                                        state.folder_rename_draft = folder.clone();
                                        ui.close_menu();
                                    }
                                    if ui.button("Delete Entire Import Group...").clicked() {
                                        state.delete_import_group = Some((
                                            state.browser_category.clone(), folder.clone(),
                                        ));
                                        ui.close_menu();
                                    }
                                } else if state.browser_category != "Scripts" {
                                    if let Some((_, base)) = BROWSER_CATEGORIES.iter()
                                        .find(|(category, _)| *category == state.browser_category)
                                    {
                                        if ui.button("Delete Folder and Contents...").clicked() {
                                            let folder_path = open_folder(&state.browser_folder, folder);
                                            state.delete_asset = Some((
                                                super::ManagedAssetKind::Path,
                                                format!(".assets/{base}/{folder_path}"),
                                            ));
                                            ui.close_menu();
                                        }
                                    }
                                }
                            }
                            let index = match entry {
                                Entry::File(index) | Entry::ActorPackage(index) => Some(*index),
                                _ => folder_index,
                            };
                            if let Some(index) = index {
                                let asset = &state.browser_files[index];
                                let copyable = managed_identity(project_path, asset).is_some();
                                if ui
                                    .add_enabled(copyable, egui::Button::new("Duplicate"))
                                    .clicked()
                                {
                                    duplicate_request = Some(index);
                                    ui.close_menu();
                                }
                                if texture_source_image(asset) {
                                    if ui.button("Create Texture").clicked() {
                                        texture_request = Some(index);
                                        ui.close_menu();
                                    }
                                }
                                if asset.category == "Textures"
                                    && !asset.is_directory
                                    && asset
                                        .path
                                        .extension()
                                        .is_some_and(|ext| ext.eq_ignore_ascii_case("png"))
                                {
                                    if ui.button("Create Material").clicked() {
                                        material_request = Some(index);
                                        ui.close_menu();
                                    }
                                }
                                if asset.category == "Actors" && asset.is_directory
                                    && asset.path.join("character-package.json").is_file()
                                {
                                    if ui.button("Create Character").clicked() {
                                        character_request = Some(index);
                                        ui.close_menu();
                                    }
                                }
                                if matches!(asset.category, "Actors" | "Characters") && asset.is_directory {
                                    if ui.button("Add Animation...").clicked() {
                                        animation_create_request = Some(index);
                                        ui.close_menu();
                                    }
                                    if ui.button("Import Animations...").clicked() {
                                        animation_retarget_request = Some(index);
                                        ui.close_menu();
                                    }
                                    if let Ok(registry) = crate::project::asset_registry::AssetRegistry::load(project_path)
                                        && let Some(rig) = registry.assets.iter().find(|record| project_path.join(&record.path) == asset.path)
                                    {
                                        let clips = registry.assets.iter().filter(|record| {
                                            record.kind == crate::project::asset_registry::AssetType::Animation
                                                && record.dependencies.contains(&rig.id)
                                        }).collect::<Vec<_>>();
                                        ui.menu_button("Edit Animation", |ui| {
                                            let imported_root = if asset.category == "Characters" {
                                                std::fs::read(asset.path.join("package.json")).ok()
                                                    .and_then(|bytes| serde_json::from_slice::<crate::project::character_asset::CharacterAsset>(&bytes).ok())
                                                    .and_then(|character| crate::project::asset_registry::AssetRegistry::actor_directory(
                                                        project_path, &character.source_actor).ok())
                                                    .unwrap_or_else(|| asset.path.clone())
                                            } else { asset.path.clone() };
                                            let imported = std::fs::read(imported_root.join("import.json")).ok()
                                                .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                                                .and_then(|json| json.get("clips").and_then(serde_json::Value::as_array).cloned())
                                                .unwrap_or_default();
                                            if clips.is_empty() && imported.is_empty() {
                                                ui.add_enabled(false, egui::Button::new("No animations"));
                                            }
                                            for clip in &clips {
                                                if ui.button(&clip.name).clicked() {
                                                    animation_open_request = state.browser_files.iter().position(|candidate| {
                                                        candidate.category == "Animations"
                                                            && candidate.path == project_path.join(&clip.path)
                                                    });
                                                    ui.close_menu();
                                                }
                                            }
                                            if !imported.is_empty() && !clips.is_empty() { ui.separator(); }
                                            for clip in imported.iter().filter_map(serde_json::Value::as_str) {
                                                if ui.button(clip).clicked() {
                                                    animation_import_request = Some((index, clip.to_owned()));
                                                    ui.close_menu();
                                                }
                                            }
                                        });
                                        if ui.button("Manage Animations...").clicked() {
                                            state.browser_category = "Animations".into();
                                            state.browser_folder.clear();
                                            ui.close_menu();
                                        }
                                    }
                                }
                                if asset.category == "Animations" && asset.is_directory {
                                    if ui.button("Edit Animation").clicked() {
                                        animation_open_request = Some(index);
                                        ui.close_menu();
                                    }
                                }
                                if ui
                                    .add_enabled(
                                        managed_identity(project_path, asset).is_some(),
                                        egui::Button::new("Rename / Move..."),
                                    )
                                    .clicked()
                                {
                                    rename_request = Some(index);
                                    ui.close_menu();
                                }
                                if ui
                                    .add_enabled(
                                        managed_identity(project_path, asset).is_some(),
                                        egui::Button::new("Delete..."),
                                    )
                                    .clicked()
                                {
                                    delete_request = Some(index);
                                    ui.close_menu();
                                }
                                if asset.category == "Materials" && !asset.is_directory {
                                    if ui.button("Set as Sky Material").clicked() {
                                        state.material_assignment_request = Some((asset.path.clone(),true));
                                        ui.close_menu();
                                    }
                                    if ui.button("Set as Water Material").clicked() {
                                        state.material_assignment_request = Some((asset.path.clone(),false));
                                        ui.close_menu();
                                    }
                                }
                                if ui.button("Copy file path").clicked() {
                                    ui.ctx().copy_text(asset.path.display().to_string());
                                    ui.close_menu();
                                }
                                if !copyable {
                                    ui.small("Package assets require dependency-aware operations.");
                                }
                            } else {
                                ui.small("This collection is managed as a folder or package.");
                            }
                        });
                        if is_double || (was_clicked && !matches!(entry, Entry::Folder(_))) {
                            action = Some((entry.clone(), is_double));
                        }
                    }
                });
            }
        });
    if let Some(previous) = state.folder_rename_source.clone() {
        let mut dialog_open = true;
        let mut accept = false;
        let mut cancel = false;
        egui::Window::new("Rename Import Folder")
            .collapsible(false)
            .resizable(false)
            .open(&mut dialog_open)
            .show(ui.ctx(), |ui| {
                ui.label("Visual folder name. Asset IDs and paths will not change.");
                ui.text_edit_singleline(&mut state.folder_rename_draft);
                ui.horizontal(|ui| {
                    if ui.button("Rename").clicked() {
                        accept = true;
                    }
                    if ui.button("Cancel").clicked() {
                        cancel = true;
                    }
                });
            });
        if accept {
            let packages = root_packages(state, &state.browser_category)
                .into_iter()
                .filter(|i| actor_group(state, &state.browser_files[*i].name) == previous)
                .map(|i| state.browser_files[i].name.clone())
                .collect::<Vec<_>>();
            match super::rename_visual_import_group(
                project_path,
                &previous,
                &state.folder_rename_draft,
                &packages,
            ) {
                Ok(()) => {
                    if state.browser_folder == previous {
                        state.browser_folder = state.folder_rename_draft.trim().to_owned();
                    }
                    state.folder_rename_source = None;
                    *refresh = true;
                }
                Err(error) => state.status = Some((error, false)),
            }
        } else if cancel || !dialog_open {
            state.folder_rename_source = None;
        }
    }
    if rename_request.is_none() && ui.ctx().input(|input| input.key_pressed(egui::Key::F2)) {
        rename_request = state.browser_selected.as_ref().and_then(|path| {
            state.browser_files.iter().position(|asset| {
                asset.path == *path && managed_identity(project_path, asset).is_some()
            })
        });
    }
    if let Some(index) = rename_request {
        if let Some((kind, identifier)) =
            managed_identity(project_path, &state.browser_files[index])
        {
            state.rename_asset = Some((kind, identifier.clone()));
            state.rename_target = identifier;
            state.status = None;
        }
    }
    if let Some(index) = delete_request {
        if let Some((kind, identifier)) =
            managed_identity(project_path, &state.browser_files[index])
        {
            state.delete_asset = Some((kind, identifier));
            state.status = None;
        }
    }
    if let Some(index) = duplicate_request {
        let asset = &state.browser_files[index];
        match duplicate_standalone_asset(project_path, asset) {
            Ok(path) => {
                state.status = Some((format!("Duplicated '{}'", path.display()), true));
                state.browser_selected = Some(path);
                state.browser_preview_loaded = None;
                *refresh = true;
            }
            Err(error) => state.status = Some((format!("Duplicate failed: {error}"), false)),
        }
    }
    if let Some(index) = texture_request {
        match create_texture_from_image(project_path, &state.browser_files[index]) {
            Ok(path) => {
                state.browser_category = "Textures".into();
                state.browser_folder = path
                    .parent()
                    .and_then(|parent| {
                        parent
                            .strip_prefix(project_path.join(".assets/textures"))
                            .ok()
                    })
                    .map(|folder| folder.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                state.browser_selected = Some(path.clone());
                state.browser_preview_loaded = None;
                state.status = Some((format!("Created Texture '{}'", path.display()), true));
                *refresh = true;
                crate::editor::tools::texture_tools::refresh_texture_catalog(
                    ui.ctx(),
                    &Some(project_path.to_owned()),
                );
            }
            Err(error) => state.status = Some((format!("Create Texture failed: {error}"), false)),
        }
    }
    if let Some(index) = material_request {
        let asset = &state.browser_files[index];
        let result = (|| {
            let texture = asset
                .path
                .strip_prefix(project_path.join(".assets/textures"))
                .map_err(|_| "Texture outside current project")?
                .to_string_lossy()
                .replace('\\', "/");
            let stem = asset
                .path
                .file_stem()
                .and_then(|name| name.to_str())
                .ok_or("Invalid Texture name")?;
            for suffix in 0..10000 {
                let name = if suffix == 0 {
                    format!("{stem}.MAT")
                } else {
                    format!("{stem}_{suffix}.MAT")
                };
                let path = crate::project::material::file_path(project_path, &name)?;
                if path.exists() {
                    continue;
                }
                crate::project::texture_material::from_texture(project_path, &name, &texture)?;
                return Ok::<_, String>(path);
            }
            Err("No available Material filename".into())
        })();
        match result {
            Ok(path) => {
                state.browser_category = "Materials".into();
                state.browser_folder.clear();
                state.browser_selected = Some(path);
                state.browser_preview_loaded = None;
                *refresh = true;
                crate::editor::tools::texture_tools::refresh_texture_catalog(
                    ui.ctx(),
                    &Some(project_path.to_owned()),
                );
            }
            Err(error) => state.status = Some((error, false)),
        }
    }
    if let Some(index) = character_request {
        if let Some(actor) = state.browser_files[index]
            .path
            .file_name()
            .and_then(|name| name.to_str())
        {
            state.character_editor = Some(
                crate::editor::character_editor::CharacterEditorState::create(
                    project_path,
                    Some(actor.to_owned()),
                ),
            );
            state.character_editor_active = true;
            state.surface_editor_active = false;
            state.sky_editor_active = false;
            state.status = None;
        }
    }
    if let Some(index) = animation_create_request {
        let asset = &state.browser_files[index];
        let result = crate::project::asset_registry::AssetRegistry::load(project_path)
            .and_then(|registry| {
                registry
                    .assets
                    .iter()
                    .find(|record| project_path.join(&record.path) == asset.path)
                    .map(|record| record.id.clone())
                    .ok_or_else(|| "Selected rig is not registered".to_string())
            })
            .and_then(|rig_id| {
                crate::editor::animation_editor::AnimationEditorState::create(project_path, &rig_id)
            });
        match result {
            Ok(editor) => {
                state.animation_editor = Some(editor);
                state.animation_editor_active = true;
                state.character_editor_active = false;
                state.surface_editor_active = false;
                state.sky_editor_active = false;
                state.status = None;
            }
            Err(error) => state.status = Some((format!("Add Animation failed: {error}"), false)),
        }
    }
    if let Some(index) = animation_retarget_request {
        let asset = &state.browser_files[index];
        let result = crate::project::asset_registry::AssetRegistry::load(project_path)
            .and_then(|registry| {
                registry
                    .assets
                    .iter()
                    .find(|record| project_path.join(&record.path) == asset.path)
                    .map(|record| record.id.clone())
                    .ok_or_else(|| "Selected rig is not registered".to_owned())
            })
            .and_then(|rig_id| {
                crate::editor::animation_editor::AnimationEditorState::import(project_path, &rig_id)
            });
        match result {
            Ok(editor) => {
                state.animation_editor = Some(editor);
                state.animation_editor_active = true;
                state.character_editor_active = false;
                state.surface_editor_active = false;
                state.sky_editor_active = false;
                state.status = None;
            }
            Err(error) => {
                state.status = Some((format!("Import Animations failed: {error}"), false))
            }
        }
    }
    if let Some(index) = animation_open_request {
        match crate::editor::animation_editor::AnimationEditorState::open(
            project_path,
            &state.browser_files[index].path,
        ) {
            Ok(editor) => {
                state.animation_editor = Some(editor);
                state.animation_editor_active = true;
                state.character_editor_active = false;
                state.surface_editor_active = false;
                state.sky_editor_active = false;
                state.status = None;
            }
            Err(error) => state.status = Some((format!("Edit Animation failed: {error}"), false)),
        }
    }
    if let Some((index, clip)) = animation_import_request {
        let asset = &state.browser_files[index];
        let result = crate::project::asset_registry::AssetRegistry::load(project_path)
            .and_then(|registry| {
                registry
                    .assets
                    .iter()
                    .find(|record| project_path.join(&record.path) == asset.path)
                    .map(|record| record.id.clone())
                    .ok_or_else(|| "Selected rig is not registered".to_string())
            })
            .and_then(|rig_id| {
                crate::editor::animation_editor::AnimationEditorState::edit_imported(
                    project_path,
                    &rig_id,
                    &clip,
                )
            });
        match result {
            Ok(editor) => {
                state.animation_editor = Some(editor);
                state.animation_editor_active = true;
                state.character_editor_active = false;
                state.surface_editor_active = false;
                state.sky_editor_active = false;
                state.status = None;
            }
            Err(error) => state.status = Some((format!("Edit Animation failed: {error}"), false)),
        }
    }
    if let Some((entry, is_double)) = action {
        match entry {
            Entry::Category(category, _) => {
                state.browser_category = category.into();
                state.browser_folder.clear();
                state.browser_selected = None;
            }
            Entry::Folder(folder) => {
                state.browser_folder = open_folder(&state.browser_folder, &folder);
                state.browser_selected = None;
            }
            Entry::ActorLetter(letter, _) => {
                state.browser_folder = open_folder(&state.browser_folder, &letter.to_string());
                state.browser_selected = None;
            }
            Entry::File(i) | Entry::ActorPackage(i) => {
                let file = &state.browser_files[i];
                if is_double && file.category == "Animations" && file.is_directory {
                    match crate::editor::animation_editor::AnimationEditorState::open(
                        project_path,
                        &file.path,
                    ) {
                        Ok(editor) => {
                            state.animation_editor = Some(editor);
                            state.animation_editor_active = true;
                            state.character_editor_active = false;
                            state.surface_editor_active = false;
                            state.sky_editor_active = false;
                            state.status = None;
                        }
                        Err(error) => {
                            state.status = Some((format!("Edit Animation failed: {error}"), false))
                        }
                    }
                    return;
                }
                if file.category == "Characters" && file.is_directory {
                    if let Some(name) = file.path.file_name().and_then(|name| name.to_str()) {
                        match crate::editor::character_editor::CharacterEditorState::open(
                            project_path,
                            name,
                        ) {
                            Ok(editor) => {
                                state.character_editor = Some(editor);
                                state.character_editor_active = true;
                                state.surface_editor_active = false;
                                state.sky_editor_active = false;
                                state.status = None;
                            }
                            Err(error) => {
                                state.status =
                                    Some((format!("Open Character failed: {error}"), false))
                            }
                        }
                    }
                    return;
                }
                state.browser_selected = Some(file.path.clone());
                state.browser_preview_loaded = None;
                if super::openable_aeo_asset(project_path, file) {
                    state.open_script_request = Some(file.path.clone());
                }
                if is_double && file.category == "Materials" && !file.is_directory {
                    let same_name = file
                        .path
                        .strip_prefix(project_path.join(".assets/materials"))
                        .ok()
                        .map(|relative| {
                            relative
                                .with_extension("")
                                .to_string_lossy()
                                .replace('\\', "/")
                        });
                    if let Some(name) = same_name.as_deref() {
                        if state.surface_material_editor.as_ref().is_some_and(|draft| {
                            draft.name == name && draft.project == project_path
                        }) {
                            state.surface_editor_active = true;
                            state.sky_editor_active = false;
                            state.status = None;
                            return;
                        }
                        if state.sky_material_preview.as_ref().is_some_and(|draft| {
                            draft.name == name && draft.project.as_deref() == Some(project_path)
                        }) {
                            state.sky_editor_active = true;
                            state.surface_editor_active = false;
                            state.status = None;
                            return;
                        }
                    }
                    let result = (|| -> Result<_, String> {
                        let relative = file
                            .path
                            .strip_prefix(project_path.join(".assets/materials"))
                            .map_err(|_| "Only project-owned Sky Materials can be edited")?;
                        let name = relative
                            .with_extension("")
                            .to_string_lossy()
                            .replace('\\', "/");
                        if let Some(renderer) = renderer {
                            let snapshot = renderer.material_snapshot();
                            let sky = snapshot.documents().get(&name).is_some_and(|doc| {
                                matches!(
                                    &doc.definition,
                                    crate::project::material::MaterialDefinition::Asset {
                                        material: crate::project::material::MaterialOutput::Sky(_)
                                    }
                                )
                            });
                            if !sky {
                                return Err("Not a Sky Material".into());
                            }
                            super::material_editor::SkyMaterialPreview::open_with_snapshot(
                                project_path,
                                &name,
                                &snapshot,
                            )
                        } else {
                            super::material_editor::SkyMaterialPreview::open(project_path, &name)
                        }
                    })();
                    match result {
                        Ok(preview) => {
                            if state
                                .surface_material_editor
                                .as_ref()
                                .is_some_and(|draft| draft.dirty())
                            {
                                state.status = Some((
                                    "Save the open Surface Material before switching.".into(),
                                    false,
                                ));
                            } else if state
                                .sky_material_preview
                                .as_ref()
                                .is_some_and(|draft| draft.is_dirty())
                            {
                                state.status = Some((
                                    "Save the open Sky Material before switching.".into(),
                                    false,
                                ));
                            } else {
                                state.sky_material_preview = Some(preview);
                                state.sky_editor_active = true;
                                state.surface_editor_active = false;
                            }
                        }
                        Err(error) => {
                            let surface_name = file
                                .path
                                .strip_prefix(project_path.join(".assets/materials"))
                                .ok()
                                .map(|path| {
                                    path.with_extension("").to_string_lossy().replace('\\', "/")
                                });
                            let mut surface_error = None;
                            if let Some(name) = surface_name {
                                let opened = if let Some(renderer) = renderer {
                                    let snapshot = renderer.material_snapshot();
                                    super::material_editor::SurfaceMaterialEditor::open_with_snapshot(project_path,&name,&snapshot)
                                } else {
                                    super::material_editor::SurfaceMaterialEditor::open(
                                        project_path,
                                        &name,
                                    )
                                };
                                match opened {
                                    Ok(editor) => {
                                        if state
                                            .sky_material_preview
                                            .as_ref()
                                            .is_some_and(|draft| draft.is_dirty())
                                        {
                                            state.status = Some((
                                                "Save the open Sky Material before switching."
                                                    .into(),
                                                false,
                                            ));
                                        } else if state
                                            .surface_material_editor
                                            .as_ref()
                                            .is_some_and(|draft| draft.dirty())
                                        {
                                            state.status = Some(("Save the current Surface Material before switching.".into(), false));
                                        } else {
                                            state.surface_material_editor = Some(editor);
                                            state.surface_editor_active = true;
                                            state.sky_editor_active = false;
                                        }
                                        return;
                                    }
                                    Err(err) => surface_error = Some(err),
                                }
                            }
                            if let Ok(document) = crate::project::material::read(
                                project_path,
                                &file
                                    .path
                                    .strip_prefix(project_path.join(".assets/materials"))
                                    .unwrap_or(&file.path)
                                    .with_extension("")
                                    .to_string_lossy(),
                            ) {
                                if matches!(
                                    document.definition,
                                    crate::project::material::MaterialDefinition::Instance { .. }
                                        | crate::project::material::MaterialDefinition::Asset {
                                            material: crate::project::material::MaterialOutput::SurfacePbr { .. }
                                        }
                                ) {
                                    state.status = Some((
                                        format!("Open Surface Material failed: {}", surface_error.unwrap_or_else(|| "Unresolved Material".into())),
                                        false,
                                    ));
                                }
                            } else {
                                state.status =
                                    Some((format!("Open Sky Material failed: {error}"), false));
                            }
                        }
                    }
                }
            }
        }
    }
}

fn details_panel(ui: &mut Ui, state: &AssetManagerState) {
    ui.strong("DETAILS");
    ui.separator();
    if let Some(asset) = state
        .browser_selected
        .as_ref()
        .and_then(|path| state.browser_files.iter().find(|a| &a.path == path))
    {
        let filename = asset
            .path
            .file_name()
            .map(|x| x.to_string_lossy().to_string())
            .unwrap_or_default();
        let display_name = if asset.category == "Actors" && asset.is_directory {
            state
                .actor_display_names
                .get(&asset.path)
                .map(String::as_str)
                .unwrap_or(&filename)
        } else {
            &filename
        };
        ui.label(RichText::new(display_name).strong())
            .on_hover_text(&filename);
        if display_name != filename {
            ui.small("Package ID");
            ui.monospace(&filename)
                .on_hover_text("Stable name used by scenes and scripts");
        }
        ui.add_space(4.0);
        ui.label(format!("Type: {}", asset.category));
        if let Some(project) = state.catalog_project.as_deref() {
            let registry = project_asset_snapshot(project);
            if let Some(record) = registry
                .assets
                .iter()
                .find(|record| project.join(&record.path) == asset.path)
            {
                crate::editor::editor::asset_browser::draw_registered_asset_inspector(
                    ui, record, &registry, project,
                );
            }
        }
        if !asset.is_directory {
            ui.label(format!("Size: {}", format_asset_size(asset.bytes)));
        }
        ui.label("Project asset");
        if asset.is_directory && (asset.category == "Actors" || asset.category == "Characters") {
            if let Some(project) = state.catalog_project.as_deref() {
                let registry = project_asset_snapshot(project);
                if let Some(record) = registry
                    .assets
                    .iter()
                    .find(|entry| project.join(&entry.path) == asset.path)
                {
                    let names = crate::animation::asset::visible_clip_names(project, &record.id);
                    ui.separator();
                    ui.strong(format!("Animations ({})", names.len()));
                    if names.is_empty() {
                        ui.small("No animations imported. Right-click to Import Animations...");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("asset_animation_names")
                            .max_height(196.0)
                            .show(ui, |ui| {
                                for name in names {
                                    ui.horizontal(|ui| {
                                        ui.monospace(&name);
                                        if ui.small_button("Copy").clicked() {
                                            ui.ctx().copy_text(name.clone());
                                        }
                                    });
                                }
                            });
                    }
                }
            }
        }
        ui.separator();
        if let Some((_, preview)) = state
            .browser_preview
            .as_ref()
            .filter(|(p, _)| *p == asset.path)
        {
            let image_size = preview.0.size_vec2();
            let side = ui.available_width().min(240.0) / image_size.x.max(image_size.y).max(1.0);
            ui.image((preview.0.id(), image_size * side));
        } else {
            ui.small("No image preview available");
        }
        ui.separator();
        ui.label(RichText::new("Location").strong());
        ui.add(egui::Label::new(short_label(
            &asset.path.display().to_string(),
            39,
        )))
        .on_hover_text(asset.path.display().to_string());
    } else {
        ui.add_space(45.0);
        ui.vertical_centered(|ui| {
            let (rect, _) = ui.allocate_exact_size(egui::vec2(90.0, 84.0), egui::Sense::hover());
            paint_symbol(
                ui,
                Symbol::File,
                rect.center(),
                42.0,
                ui.visuals().weak_text_color().gamma_multiply(0.55),
            );
            ui.add_space(8.0);
            ui.strong("Nothing selected");
            ui.small("Choose an asset to see its details and preview.");
        });
    }
}

fn safe_folder_name(name: &str) -> bool {
    !name.is_empty()
        && name != "."
        && name != ".."
        && !name.ends_with('.')
        && !name.chars().any(|c| "\\/:*?\"<>|".contains(c))
}

fn folder_creation_root(project: &Path, category: &str, folder: &str) -> Option<PathBuf> {
    if category.is_empty()
        || folder == IMPORTED_ACTORS
        || matches!(
            category,
            "Actors" | "Characters" | "Controllers" | "Cameras"
        )
    {
        return None;
    }
    let (_, directory) = BROWSER_CATEGORIES
        .iter()
        .find(|(label, _)| *label == category)?;
    let base = if category == "Scripts" {
        project.join("scripts")
    } else {
        project.join(".assets").join(directory)
    };
    Some(base.join(folder))
}

pub(super) fn draw(
    ui: &mut Ui,
    state: &mut AssetManagerState,
    project_path: &Path,
    refresh: &mut bool,
    renderer: Option<&crate::renderer::Renderer>,
) {
    if !state.browser_view_initialized {
        state.browser_grid = true;
        state.browser_view_initialized = true;
    }
    if state.browser_group_category != state.browser_category {
        state.browser_group_filter.clear();
        state.browser_group_category = state.browser_category.clone();
    }
    if !state.surface_editor_active {
        if let Some(name) = state
            .surface_material_editor
            .as_ref()
            .filter(|edit| edit.project == project_path && edit.dirty())
            .map(|edit| edit.name.clone())
        {
            ui.horizontal(|ui| {
                ui.label(format!("Unsaved Material: {name}"));
                if ui.button("Resume Editing").clicked() {
                    state.surface_editor_active = true;
                    state.sky_editor_active = false;
                    state.status = None;
                    ui.ctx().request_repaint();
                }
            });
        }
    }
    if !state.sky_editor_active {
        if let Some(name) = state
            .sky_material_preview
            .as_ref()
            .filter(|edit| edit.project.as_deref() == Some(project_path) && edit.is_dirty())
            .map(|edit| edit.name.clone())
        {
            ui.horizontal(|ui| {
                ui.label(format!("Unsaved Sky Material: {name}"));
                if ui.button("Resume Editing").clicked() {
                    state.sky_editor_active = true;
                    state.surface_editor_active = false;
                    state.status = None;
                    ui.ctx().request_repaint();
                }
            });
        }
    }
    ui.horizontal_wrapped(|ui| {
        if ui.button("Refresh").clicked() {
            *refresh = true;
        }
        if state.browser_category == "Actors" && ui.button("Create Character").clicked() {
            state.character_editor = Some(
                crate::editor::character_editor::CharacterEditorState::create(project_path, None),
            );
            state.character_editor_active = true;
            state.surface_editor_active = false;
            state.sky_editor_active = false;
            state.status = None;
        }
        if state.browser_category == "Materials" && ui.button("Create Material").clicked() {
            let dialog_id = egui::Id::new("worldkiln_create_material_open");
            let open = ui
                .ctx()
                .data(|data| data.get_temp::<bool>(dialog_id))
                .unwrap_or(false);
            ui.ctx().data_mut(|data| data.insert_temp(dialog_id, !open));
        }
        let can_create =
            folder_creation_root(project_path, &state.browser_category, &state.browser_folder)
                .is_some();
        if ui
            .add_enabled(can_create, egui::Button::new("New Folder"))
            .clicked()
        {
            state.browser_folder_creator_open = !state.browser_folder_creator_open;
        }
        if ui.button("Up").on_hover_text("Parent folder").clicked() {
            state.browser_folder = parent_folder(&state.browser_folder);
        }
        if ui.selectable_label(state.browser_grid, "Tiles").clicked() {
            state.browser_grid = true;
        }
        if ui.selectable_label(!state.browser_grid, "List").clicked() {
            state.browser_grid = false;
        }
        egui::ComboBox::from_id_salt("wk_workspace_sort")
            .selected_text(match state.browser_sort {
                BrowserSort::Name => "Name",
                BrowserSort::Category => "Type",
                BrowserSort::Size => "Size",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut state.browser_sort, BrowserSort::Name, "Name");
                ui.selectable_value(&mut state.browser_sort, BrowserSort::Category, "Type");
                ui.selectable_value(&mut state.browser_sort, BrowserSort::Size, "Size");
            });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            if ui
                .selectable_label(!state.browser_details_collapsed, "Details")
                .clicked()
            {
                state.browser_details_collapsed = !state.browser_details_collapsed;
            }
        });
    });
    let material_dialog_id = egui::Id::new("worldkiln_create_material_open");
    if state.browser_category == "Materials"
        && ui
            .ctx()
            .data(|data| data.get_temp::<bool>(material_dialog_id))
            .unwrap_or(false)
    {
        let material_kind_id = egui::Id::new("worldkiln_create_material_type");
        let mut material_kind = ui
            .ctx()
            .data(|data| data.get_temp::<String>(material_kind_id))
            .unwrap_or_else(|| "Surface".to_owned());
        if !matches!(
            material_kind.as_str(),
            "Surface" | "Water" | "Sky" | "Post Process"
        ) {
            material_kind = "Surface".to_owned();
        }
        let mut create = false;
        ui.horizontal(|ui| {
            egui::ComboBox::from_label("Type")
                .selected_text(&material_kind)
                .show_ui(ui, |ui| {
                    for kind in ["Surface", "Water", "Sky", "Post Process"] {
                        ui.selectable_value(&mut material_kind, kind.to_owned(), kind);
                    }
                });
            create = ui.button("Create").clicked();
        });
        ui.ctx()
            .data_mut(|data| data.insert_temp(material_kind_id, material_kind.clone()));
        if create {
            if state
                .sky_material_preview
                .as_ref()
                .is_some_and(|edit| edit.is_dirty())
            {
                state.sky_editor_active = true;
                state.surface_editor_active = false;
                state.status = Some((
                    "Save the open Sky Material before creating another Material.".into(),
                    false,
                ));
            } else if state
                .surface_material_editor
                .as_ref()
                .is_some_and(|edit| edit.dirty())
            {
                state.surface_editor_active = true;
                state.sky_editor_active = false;
                state.status = Some((
                    "Save the open Material before creating another.".into(),
                    false,
                ));
            } else if material_kind == "Sky" {
                match super::material_editor::SkyMaterialPreview::for_project(project_path) {
                    Ok(editor) => {
                        state.browser_selected =
                            crate::project::material::file_path(project_path, &editor.name).ok();
                        state.sky_material_preview = Some(editor);
                        state.sky_editor_active = true;
                        state.surface_editor_active = false;
                        state.sky_choices_project = None;
                        state.status = None;
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(material_dialog_id, false));
                        *refresh = true;
                    }
                    Err(error) => {
                        state.status = Some((format!("Create Material failed: {error}"), false));
                    }
                }
            } else {
                let result = match material_kind.as_str() {
                    "Surface" => {
                        super::material_editor::SurfaceMaterialEditor::create(project_path)
                    }
                    "Water" => {
                        super::material_editor::SurfaceMaterialEditor::create_water(project_path)
                    }
                    "Post Process" => {
                        super::material_editor::SurfaceMaterialEditor::create_post_process(
                            project_path,
                        )
                    }
                    _ => unreachable!("material type validated above"),
                };
                match result {
                    Ok(editor) => {
                        state.browser_selected =
                            crate::project::material::file_path(project_path, &editor.name).ok();
                        state.surface_material_editor = Some(editor);
                        state.surface_editor_active = true;
                        state.sky_editor_active = false;
                        state.status = None;
                        ui.ctx()
                            .data_mut(|data| data.insert_temp(material_dialog_id, false));
                        *refresh = true;
                    }
                    Err(error) => {
                        state.status = Some((format!("Create Material failed: {error}"), false));
                    }
                }
            }
        }
    }
    if state.browser_folder_creator_open {
        ui.horizontal(|ui| {
            ui.label("Folder name");
            ui.add(egui::TextEdit::singleline(&mut state.new_folder_name).desired_width(200.0));
            if ui.button("Create").clicked() {
                let name = state.new_folder_name.trim().to_string();
                if !safe_folder_name(&name) {
                    state.status = Some(("Enter a valid folder name.".to_string(), false));
                } else if let Some(parent) = folder_creation_root(
                    project_path,
                    &state.browser_category,
                    &state.browser_folder,
                ) {
                    let result = std::fs::create_dir_all(&parent)
                        .and_then(|_| std::fs::create_dir(parent.join(&name)));
                    match result {
                        Ok(()) => {
                            state.status = Some((format!("Created folder '{name}'."), true));
                            state.new_folder_name.clear();
                            state.browser_folder_creator_open = false;
                            *refresh = true;
                        }
                        Err(err) => {
                            state.status = Some((format!("Cannot create folder: {err}"), false))
                        }
                    }
                }
            }
            if ui.button("Cancel").clicked() {
                state.browser_folder_creator_open = false;
                state.new_folder_name.clear();
            }
        });
    }
    ui.horizontal(|ui| {
        ui.label("Search");
        ui.add(
            egui::TextEdit::singleline(&mut state.search)
                .hint_text("Find assets across folders...")
                .desired_width(ui.available_width().min(540.0)),
        );
    });
    if !state.browser_category.is_empty() {
        let groups: std::collections::BTreeSet<String> = state
            .browser_files
            .iter()
            .filter(|asset| asset.category == state.browser_category)
            .filter_map(|asset| state.browser_record_groups.get(&asset.path))
            .filter_map(|value| value.clone())
            .collect();
        if !groups.is_empty() {
            ui.horizontal(|ui| {
                ui.label("Group");
                egui::ComboBox::from_id_salt("wk_asset_creator_group_filter")
                    .selected_text(if state.browser_group_filter.is_empty() {
                        "All assets"
                    } else {
                        &state.browser_group_filter
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(
                            &mut state.browser_group_filter,
                            String::new(),
                            "All assets",
                        );
                        for group in groups {
                            ui.selectable_value(
                                &mut state.browser_group_filter,
                                group.clone(),
                                group,
                            );
                        }
                    });
            });
        } else {
            state.browser_group_filter.clear();
        }
    }
    ui.horizontal_wrapped(|ui| {
        ui.small(
            project_path
                .file_name()
                .and_then(|s| s.to_str())
                .unwrap_or("Project"),
        );
        if !state.browser_category.is_empty() {
            if ui.small_button(" / ").clicked() {
                state.browser_folder.clear();
            }
            ui.small(short_label(&state.browser_category, 27));
            if !is_package_category(&state.browser_category) && !state.browser_folder.is_empty() {
                ui.small(" / ");
                ui.small(short_label(
                    if state.browser_folder == IMPORTED_ACTORS {
                        "Imported Actors"
                    } else {
                        &state.browser_folder
                    },
                    36,
                ));
            }
        }
        if !state.search.is_empty() {
            ui.small(" / Search results");
        }
    });
    ui.separator();
    let available = ui.available_width();
    let height = (ui.available_height() - 9.0).max(125.0);
    let nav_w = (available * 0.20).clamp(150.0, 245.0).min(available * 0.34);
    let details_w = if !state.browser_details_collapsed && available >= 840.0 {
        (available * 0.23).clamp(205.0, 285.0)
    } else {
        0.0
    };
    ui.horizontal(|ui| {
        ui.allocate_ui_with_layout(
            egui::vec2(nav_w, height),
            Layout::top_down(Align::Min),
            |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width((nav_w - 20.0).max(90.0));
                    ui.set_min_height((height - 20.0).max(100.0));
                    source_panel(ui, state);
                });
            },
        );
        ui.separator();
        let center_w = (available - nav_w - details_w - 34.0).max(160.0);
        let mut results = entries(state);
        if !state.browser_group_filter.is_empty() {
            results.retain(|entry| {
                let index = match entry {
                    Entry::ActorPackage(i) | Entry::File(i) => *i,
                    _ => return false,
                };
                state
                    .browser_record_groups
                    .get(&state.browser_files[index].path)
                    .and_then(|group| group.as_deref())
                    == Some(state.browser_group_filter.as_str())
            });
        }
        ui.allocate_ui_with_layout(
            egui::vec2(center_w, height),
            Layout::top_down(Align::Min),
            |ui| {
                asset_view(ui, state, &results, project_path, refresh, renderer);
                if details_w == 0.0 && !state.browser_details_collapsed {
                    egui::CollapsingHeader::new("Selected asset details")
                        .show(ui, |ui| details_panel(ui, state));
                }
            },
        );
        if details_w > 0.0 {
            ui.separator();
            ui.allocate_ui_with_layout(
                egui::vec2(details_w, height),
                Layout::top_down(Align::Min),
                |ui| {
                    egui::Frame::group(ui.style()).show(ui, |ui| {
                        ui.set_min_width((details_w - 20.0).max(90.0));
                        ui.set_min_height((height - 20.0).max(100.0));
                        ScrollArea::vertical()
                            .id_salt("wk_details_scroll")
                            .show(ui, |ui| details_panel(ui, state));
                    });
                },
            );
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn imported_actor_tiles_show_actual_model_names_without_pack_navigation() {
        let mut state = AssetManagerState::default();
        for (name, display) in [
            ("captain", "Captain"),
            ("sailor", "Sailor"),
            ("rowboat", "Rowboat"),
        ] {
            let path = PathBuf::from(name);
            state.browser_files.push(BrowserAsset {
                category: "Actors",
                name: name.into(),
                path: path.clone(),
                bytes: 0,
                is_directory: true,
            });
            state.actor_display_names.insert(path, display.into());
        }
        state.browser_category = "Actors".into();
        let tiles = entries(&state);
        assert_eq!(tiles.len(), 3);
        assert!(
            tiles
                .iter()
                .all(|item| matches!(item, Entry::ActorPackage(_)))
        );
        assert_eq!(entry_label(&state, &tiles[0]).0, "Captain");
        state.search = "sailor".into();
        assert_eq!(entries(&state), vec![Entry::ActorPackage(1)]);
    }
    #[test]
    fn managed_identity_only_maps_supported_project_owned_assets() {
        let project = PathBuf::from("C:/Worldkiln/MyProject");
        let audio = BrowserAsset {
            category: "Audio",
            name: "ambient.mp3".into(),
            path: project.join(".assets/audio/music/ambient.mp3"),
            bytes: 1,
            is_directory: false,
        };
        assert_eq!(
            managed_identity(&project, &audio),
            Some((
                super::super::ManagedAssetKind::Audio,
                "music/ambient.mp3".into()
            ))
        );
        let texture = BrowserAsset {
            category: "Textures",
            name: "stone.png".into(),
            path: project.join(".assets/textures/stone.png"),
            bytes: 1,
            is_directory: false,
        };
        assert_eq!(
            managed_identity(&project, &texture),
            Some((super::super::ManagedAssetKind::Texture, "stone".into()))
        );
        let other = BrowserAsset {
            category: "Video",
            ..audio.clone()
        };
        assert!(managed_identity(&project, &other).is_none());
    }

    #[test]
    fn images_right_click_create_texture_preserves_image_and_existing_texture() {
        let project =
            std::env::temp_dir().join(format!("wk-image-texture-{:032x}", rand::random::<u128>()));
        let images = project.join(".assets/media");
        std::fs::create_dir_all(&images).unwrap();
        let image_path = images.join("Grass.png");
        image::RgbaImage::from_pixel(4, 4, image::Rgba([30, 140, 50, 255]))
            .save(&image_path)
            .unwrap();
        let original = std::fs::read(&image_path).unwrap();
        let texture_root = project.join(".assets/textures");
        std::fs::create_dir_all(&texture_root).unwrap();
        std::fs::write(texture_root.join("Grass.png"), b"creator existing texture").unwrap();
        let mut state = AssetManagerState::default();
        state.browser_category = "Images".into();
        state.browser_grid = false;
        state.browser_files.push(BrowserAsset {
            category: "Images",
            name: "Grass.png".into(),
            path: image_path.clone(),
            bytes: original.len() as u64,
            is_directory: false,
        });
        let ctx = egui::Context::default();
        let mut refresh = false;
        let mut frame = |events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(800.0, 600.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let visible = entries(&state);
                        asset_view(ui, &mut state, &visible, &project, &mut refresh, None);
                    });
                },
            )
        };
        frame(vec![]);
        let output = frame(vec![]);
        let locate = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {label}"))
        };
        let tile = locate(&output, "Grass.png");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(tile),
                egui::Event::PointerButton {
                    pos: tile,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = frame(vec![]);
        let action = locate(&output, "Create Texture");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(action),
                egui::Event::PointerButton {
                    pos: action,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        drop(frame);
        assert!(refresh);
        assert_eq!(state.browser_category, "Textures");
        let copied = texture_root.join("Grass_1.png");
        assert_eq!(state.browser_selected.as_deref(), Some(copied.as_path()));
        assert_eq!(std::fs::read(&image_path).unwrap(), original);
        assert_eq!(std::fs::read(&copied).unwrap(), original);
        let registry = crate::project::asset_registry::AssetRegistry::load(&project).unwrap();
        assert!(
            registry
                .by_type(crate::project::asset_registry::AssetType::Texture)
                .any(|record| project.join(&record.path) == copied),
            "New Texture must appear in the shared registry without reopening the project"
        );
        assert_eq!(
            std::fs::read(texture_root.join("Grass.png")).unwrap(),
            b"creator existing texture"
        );
        std::fs::remove_dir_all(project).unwrap();
    }
    #[test]
    fn tiles_context_menu_create_texture_is_visible_and_executable() {
        let project = std::env::temp_dir().join(format!(
            "wk-image-tile-context-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(project.join(".assets/media")).unwrap();
        let path = project.join(".assets/media/grass.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([12, 150, 35, 255]))
            .save(&path)
            .unwrap();
        let mut state = AssetManagerState::default();
        state.browser_category = "Images".into();
        state.browser_grid = true;
        state.browser_files.push(BrowserAsset {
            category: "Images",
            name: "grass.png".into(),
            path: path.clone(),
            bytes: 202,
            is_directory: false,
        });
        let ctx = egui::Context::default();
        let mut refresh = false;
        let mut frame = |events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 750.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let visible = entries(&state);
                        asset_view(ui, &mut state, &visible, &project, &mut refresh, None);
                    });
                },
            )
        };
        frame(vec![]);
        let output = frame(vec![]);
        let find = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {label} in tile view"))
        };
        let tile = find(&output, "grass.png");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(tile),
                egui::Event::PointerButton {
                    pos: tile,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = frame(vec![]);
        let action = find(&output, "Create Texture");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(action),
                egui::Event::PointerButton {
                    pos: action,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        drop(frame);
        assert!(refresh, "Tile action must refresh the catalogue");
        assert_eq!(state.browser_category, "Textures");
        assert!(project.join(".assets/textures/grass.png").is_file());
        assert!(path.is_file(), "Original image is not moved");
        std::fs::remove_dir_all(project).unwrap();
    }
    #[test]
    fn jpeg_tile_context_menu_create_texture_converts_preserves_and_avoids_collisions() {
        let project = std::env::temp_dir().join(format!(
            "wk-jpeg-tile-context-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(project.join(".assets/media")).unwrap();
        let path = project.join(".assets/media/grass.jpeg");
        image::RgbImage::from_pixel(8, 8, image::Rgb([12, 150, 35]))
            .save(&path)
            .unwrap();
        let source_bytes = std::fs::read(&path).unwrap();
        let textures = project.join(".assets/textures");
        std::fs::create_dir_all(&textures).unwrap();
        std::fs::write(textures.join("grass.png"), b"existing user Texture").unwrap();
        let mut state = AssetManagerState::default();
        state.browser_category = "Images".into();
        state.browser_grid = true;
        state.browser_files.push(BrowserAsset {
            category: "Images",
            name: "grass.jpeg".into(),
            path: path.clone(),
            bytes: 202,
            is_directory: false,
        });
        let ctx = egui::Context::default();
        let mut refresh = false;
        let mut frame = |events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 750.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let visible = entries(&state);
                        asset_view(ui, &mut state, &visible, &project, &mut refresh, None);
                    });
                },
            )
        };
        frame(vec![]);
        let output = frame(vec![]);
        let find = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {label} in tile view"))
        };
        let tile = find(&output, "grass.jpeg");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(tile),
                egui::Event::PointerButton {
                    pos: tile,
                    button: egui::PointerButton::Secondary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = frame(vec![]);
        let action = find(&output, "Create Texture");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(action),
                egui::Event::PointerButton {
                    pos: action,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        drop(frame);
        assert!(refresh, "Tile action must refresh the catalogue");
        assert_eq!(state.browser_category, "Textures");
        let converted = project.join(".assets/textures/grass_1.png");
        assert_eq!(state.browser_selected.as_deref(), Some(converted.as_path()));
        assert_eq!(
            std::fs::read(textures.join("grass.png")).unwrap(),
            b"existing user Texture"
        );
        assert_eq!(
            std::fs::read(&path).unwrap(),
            source_bytes,
            "JPEG original must be unchanged"
        );
        let image = image::ImageReader::open(&converted)
            .unwrap()
            .with_guessed_format()
            .unwrap();
        assert_eq!(image.format(), Some(image::ImageFormat::Png));
        assert_eq!(image.decode().unwrap().width(), 8);
        assert_eq!(image::open(&converted).unwrap().height(), 8);
        std::fs::remove_dir_all(project).unwrap();
    }
    #[test]
    fn selected_image_has_visible_create_texture_action_in_tiles() {
        let project =
            std::env::temp_dir().join(format!("wk-image-toolbar-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(project.join(".assets/media")).unwrap();
        let path = project.join(".assets/media/landscape.png");
        image::RgbaImage::from_pixel(8, 8, image::Rgba([12, 150, 35, 255]))
            .save(&path)
            .unwrap();
        let mut state = AssetManagerState::default();
        state.browser_category = "Images".into();
        state.browser_grid = true;
        state.browser_files.push(BrowserAsset {
            category: "Images",
            name: "landscape.png".into(),
            path: path.clone(),
            bytes: 202,
            is_directory: false,
        });
        let ctx = egui::Context::default();
        let mut refresh = false;
        let mut frame = |events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(900.0, 750.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| {
                    egui::CentralPanel::default().show(ctx, |ui| {
                        let visible = entries(&state);
                        asset_view(ui, &mut state, &visible, &project, &mut refresh, None);
                    });
                },
            )
        };
        frame(vec![]);
        let output = frame(vec![]);
        let find = |output: &egui::FullOutput, label: &str| {
            output
                .shapes
                .iter()
                .find_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) if text.galley.job.text == label => {
                        Some(text.pos + text.galley.size() * 0.5)
                    }
                    _ => None,
                })
                .unwrap_or_else(|| panic!("Missing {label} in tile view"))
        };
        let tile = find(&output, "landscape.png");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(tile),
                egui::Event::PointerButton {
                    pos: tile,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        let output = frame(vec![]);
        let action = find(&output, "Create Texture");
        for pressed in [true, false] {
            frame(vec![
                egui::Event::PointerMoved(action),
                egui::Event::PointerButton {
                    pos: action,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                },
            ]);
        }
        drop(frame);
        assert!(refresh);
        assert_eq!(state.browser_category, "Textures");
        assert!(project.join(".assets/textures/landscape.png").is_file());
        assert!(path.is_file());
        std::fs::remove_dir_all(project).unwrap();
    }

    #[test]
    fn large_actor_inventory_is_flat_and_requires_no_folder_navigation() {
        let mut state = AssetManagerState::default();
        state.browser_category = "Actors".into();
        for i in 0..500 {
            state.browser_files.push(BrowserAsset {
                category: "Actors",
                name: format!("tree_{i:03}"),
                path: PathBuf::from(format!("tree_{i:03}")),
                bytes: 0,
                is_directory: true,
            });
        }
        state.actor_package_indices = (0..500).collect();
        state.browser_category_counts.insert("Actors", 500);
        state.catalog_loaded = true;
        assert_eq!(entries(&state).len(), 500);
        assert!(
            entries(&state)
                .iter()
                .all(|entry| matches!(entry, Entry::ActorPackage(_)))
        );
        state.browser_folder = "@imported-actors/Legacy/T".into();
        assert_eq!(
            entries(&state).len(),
            500,
            "Saved legacy folder selection must not hide registered Actors"
        );
        state.search = "tree_499".into();
        assert_eq!(entries(&state), vec![Entry::ActorPackage(499)]);
    }
    #[test]
    fn actor_search_finds_assets_without_imported_pack_folders() {
        let mut state = AssetManagerState::default();
        state.browser_category = "Actors".into();
        for name in ["boat-row", "boat-sail", "cannon", "castle-door", "6-sided"] {
            state.browser_files.push(BrowserAsset {
                category: "Actors",
                name: name.into(),
                path: PathBuf::from(name),
                bytes: 0,
                is_directory: true,
            });
        }
        state.actor_package_indices = (0..5).collect();
        state.catalog_loaded = true;
        assert_eq!(entries(&state).len(), 5);
        state.search = "boat".into();
        assert_eq!(
            entries(&state),
            vec![Entry::ActorPackage(0), Entry::ActorPackage(1)]
        );
        state.browser_folder = "some-old-pack/path".into();
        assert_eq!(entries(&state).len(), 2);
    }
    #[test]
    fn search_finds_nested_assets_without_folder_expansion() {
        let mut state = AssetManagerState::default();
        state.browser_category = "Textures".into();
        state.browser_files.push(BrowserAsset {
            category: "Textures",
            name: "packs/trees/leaf.png".into(),
            path: PathBuf::from("leaf.png"),
            bytes: 17,
            is_directory: false,
        });
        assert!(entries(&state).is_empty() == false);
        state.search = "leaf".into();
        assert_eq!(entries(&state), vec![Entry::File(0)]);
    }
    #[test]
    fn type_sort_uses_file_extensions_within_one_category() {
        let mut state = AssetManagerState::default();
        state.browser_category = "Audio".into();
        for name in ["alpha.wav", "beta.mp3"] {
            state.browser_files.push(BrowserAsset {
                category: "Audio",
                name: name.into(),
                path: PathBuf::from(name),
                bytes: 1,
                is_directory: false,
            });
        }
        assert_eq!(entries(&state), vec![Entry::File(0), Entry::File(1)]);
        state.browser_sort = BrowserSort::Category;
        assert_eq!(entries(&state), vec![Entry::File(1), Entry::File(0)]);
    }

    #[test]
    fn folder_creation_rejects_unsafe_names_and_package_directories() {
        for unsafe_name in [
            "",
            ".",
            "..",
            "../outside",
            "bad/child",
            "bad\\\\child",
            "bad:",
            "bad.",
        ] {
            assert!(!safe_folder_name(unsafe_name));
        }
        assert!(safe_folder_name("Textures Set 2"));
        let project = Path::new("project");
        assert!(folder_creation_root(project, "Actors", "").is_none());
        assert!(folder_creation_root(project, "Textures", IMPORTED_ACTORS).is_none());
        assert_eq!(
            folder_creation_root(project, "Textures", "trees"),
            Some(project.join(".assets").join("textures").join("trees"))
        );
    }

    #[test]
    fn folder_scoping_never_flattens_unopened_child_folders() {
        let mut state = AssetManagerState::default();
        state.browser_category = "Textures".into();
        for i in 0..1000 {
            state.browser_files.push(BrowserAsset {
                category: "Textures",
                name: format!("trees/item_{i:04}.png"),
                path: PathBuf::from(format!("item_{i:04}.png")),
                bytes: i + 1,
                is_directory: false,
            });
        }
        assert_eq!(entries(&state), vec![Entry::Folder("trees".into())]);
        state.browser_folder = "trees".into();
        assert_eq!(entries(&state).len(), 1000);
        state.browser_sort = BrowserSort::Size;
        assert_eq!(entries(&state)[0], Entry::File(999));
    }
}
