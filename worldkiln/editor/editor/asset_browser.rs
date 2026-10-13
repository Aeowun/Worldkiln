// Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:26 -05:00
//
// Content Browser: user-facing Unreal-inspired organization, NOT a new asset
// database. Package identities come from existing static/Character registries,
// and file items come from existing project-local folders. Never write files
// or deserialize imported GLBs during egui frames.
use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::{Align, Layout, Pos2, Rect, Response, Sense, Stroke, Ui};

use super::{
    Editor,
    shell::{ActorContentDragPayload, ShellState},
};
use crate::engine::audio::EngineSound;

pub const CONTENT_CATEGORIES: [&str; 8] = [
    "Actors",
    "Scripts",
    "Player Scripts",
    "Textures",
    "Media",
    "Audio",
    "UI",
    "Resources",
];

fn candidates(root: &Path, category: &str) -> Vec<PathBuf> {
    let assets = root.join(".assets");
    match category {
        "Scripts" => vec![root.join("scripts"), root.join("Scripts")],
        "Player Scripts" => vec![root.join("playerscripts"), root.join("PlayerScripts")],
        // The imported Actor identities are backed by package registries,
        // not a ".assets/Models" directory. Raw package files remain browsable
        // under Resources, without showing them as duplicate Actors.
        "Actors" | "Models" => vec![assets.join("entities"), assets.join("Models")],
        "Textures" => vec![assets.join("textures"), assets.join("Textures")],
        "Media" => vec![assets.join("media"), assets.join("Media")],
        "Audio" => vec![assets.join("audio"), assets.join("Audio")],
        "UI" => vec![assets.join("ui"), assets.join("UI")],
        "Resources" => vec![assets],
        _ => vec![root.to_owned()],
    }
}

// Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:26 -05:00
pub fn physical_category_root(root: &Path, category: &str) -> PathBuf {
    let choices = candidates(root, category);
    choices
        .iter()
        .find(|path| path.is_dir())
        .cloned()
        .unwrap_or_else(|| choices[0].clone())
}

// Import directly into the Content Browser's existing project-local asset folders.
// Keep existing assets untouched; collisions receive numbered filenames.
fn import_browser_assets(root: &Path, category: &str) -> Result<usize, String> {
    let (label, extensions): (&str, &[&str]) = match category {
        "Audio" => ("Audio", &["wav"]),
        "Textures" => ("Textures", &["png"]),
        "Media" => ("Media", &["mp4"]),
        _ => return Err("This category does not support importing.".into()),
    };
    let Some(files) = rfd::FileDialog::new()
        .add_filter(label, extensions)
        .pick_files()
    else {
        return Ok(0);
    };
    let folder = physical_category_root(root, category);
    std::fs::create_dir_all(&folder).map_err(|e| e.to_string())?;
    let mut imported = 0;
    for source in files {
        let Some(ext) = source.extension().and_then(|x| x.to_str()) else {
            continue;
        };
        if !extensions
            .iter()
            .any(|valid| ext.eq_ignore_ascii_case(valid))
        {
            continue;
        }
        let Some(name) = source.file_name() else {
            continue;
        };
        let stem = source.file_stem().unwrap_or_default().to_string_lossy();
        let mut target = folder.join(name);
        let mut suffix = 1;
        while target.exists() {
            target = folder.join(format!("{stem}_{suffix}.{ext}"));
            suffix += 1;
        }
        std::fs::copy(&source, &target)
            .map_err(|e| format!("Failed to import {}: {e}", source.display()))?;
        imported += 1;
    }
    Ok(imported)
}

// Only enumerate directories when the user switches/navigates a folder.
// Symlinks and junctions are skipped. Finite depth/count protects expensive
// project libraries; folder contents themselves remain accessible in the grid.
pub fn scan_category_folders(root: &Path) -> Vec<PathBuf> {
    fn walk(base: &Path, dir: &Path, depth: u8, out: &mut Vec<PathBuf>) {
        if depth >= 5 || out.len() >= 800 {
            return;
        }
        let Ok(read) = std::fs::read_dir(dir) else {
            return;
        };
        let mut directories = read
            .flatten()
            .filter(|entry| {
                entry
                    .file_type()
                    .is_ok_and(|kind| kind.is_dir() && !kind.is_symlink())
            })
            .map(|entry| entry.path())
            .collect::<Vec<_>>();
        directories.sort_by(|a, b| a.file_name().cmp(&b.file_name()));
        for path in directories {
            if out.len() >= 800 {
                break;
            }
            if !path.starts_with(base) {
                continue;
            }
            out.push(path.clone());
            walk(base, &path, depth + 1, out);
        }
    }
    let mut result = Vec::new();
    walk(root, root, 0, &mut result);
    result
}

fn actor_family(package: &str) -> String {
    let segment = package
        .split(|ch: char| ch == '-' || ch == '_' || ch == ' ' || ch == '.')
        .find(|segment| !segment.is_empty())
        .unwrap_or("Misc");
    let mut chars = segment.chars();
    match chars.next() {
        None => "Misc".to_owned(),
        Some(head) => head.to_uppercase().chain(chars).collect(),
    }
}
fn character_package(shell: &ShellState, package: &str) -> bool {
    shell.character_packages.iter().any(|name| name == package)
}
fn groups(shell: &ShellState, kind: &str) -> BTreeMap<String, Vec<String>> {
    let characters = kind == "Characters";
    let mut result = BTreeMap::<String, Vec<String>>::new();
    for package in &shell.actor_packages {
        if character_package(shell, package) != characters {
            continue;
        }
        result
            .entry(actor_family(package))
            .or_default()
            .push(package.clone());
    }
    result
}

#[derive(Clone, Debug)]
enum BrowseTarget {
    Root,
    Category(String),
    ActorKind(String),
    ActorFamily(String),
    Directory(PathBuf),
}
#[derive(Clone, Debug)]
enum BrowserGridItem {
    Folder {
        name: String,
        destination: BrowseTarget,
        count: Option<usize>,
    },
    Actor {
        name: String,
    },
    File {
        name: String,
        path: PathBuf,
    },
}
impl BrowserGridItem {
    fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. } | Self::Actor { name } | Self::File { name, .. } => name,
        }
    }
    fn asset_type(&self) -> &'static str {
        match self {
            Self::Folder { .. } => "Folder",
            Self::Actor { .. } => "Actor",
            Self::File { path, .. } => {
                match path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "aeo" => "AeoScript",
                    "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tga" => "Texture",
                    "wav" | "mp3" | "ogg" | "flac" => "Audio",
                    "mp4" | "webm" | "mov" => "Video",
                    "glb" | "gltf" | "fbx" | "obj" => "Model",
                    "json" => "Data",
                    _ => "File",
                }
            }
        }
    }
}

fn browser_items(shell: &ShellState) -> Vec<BrowserGridItem> {
    let query = shell.browser_search.to_ascii_lowercase();
    let mut items = Vec::new();
    match shell.asset_folder.as_str() {
        "Content" => {
            for category in CONTENT_CATEGORIES {
                items.push(BrowserGridItem::Folder {
                    name: category.to_string(),
                    destination: BrowseTarget::Category(category.to_string()),
                    count: None,
                });
            }
        }
        "Actors" => {
            if shell.actor_browser_kind.is_empty() {
                let characters = shell.character_packages.len();
                items.push(BrowserGridItem::Folder {
                    name: "Static Actors".into(),
                    destination: BrowseTarget::ActorKind("Static Actors".into()),
                    count: Some(shell.actor_packages.len().saturating_sub(characters)),
                });
                items.push(BrowserGridItem::Folder {
                    name: "Characters".into(),
                    destination: BrowseTarget::ActorKind("Characters".into()),
                    count: Some(characters),
                });
            } else {
                let map = groups(shell, &shell.actor_browser_kind);
                if shell.actor_browser_family.is_empty() {
                    // Search is intentionally across package contents in this
                    // view. Otherwise display semantic subfolders only.
                    if query.is_empty() {
                        for (family, names) in map {
                            items.push(BrowserGridItem::Folder {
                                name: family.clone(),
                                destination: BrowseTarget::ActorFamily(family),
                                count: Some(names.len()),
                            });
                        }
                    } else {
                        for names in map.values() {
                            for name in names {
                                if name.to_ascii_lowercase().contains(&query) {
                                    items.push(BrowserGridItem::Actor { name: name.clone() });
                                }
                            }
                        }
                    }
                } else if let Some(names) = map.get(&shell.actor_browser_family) {
                    for name in names {
                        items.push(BrowserGridItem::Actor { name: name.clone() });
                    }
                }
            }
        }
        _ => {
            for (path, is_dir) in &shell.content_entries {
                let Some(name) = path.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                    continue;
                };
                if *is_dir {
                    items.push(BrowserGridItem::Folder {
                        name,
                        destination: BrowseTarget::Directory(path.clone()),
                        count: None,
                    });
                } else {
                    items.push(BrowserGridItem::File {
                        name,
                        path: path.clone(),
                    });
                }
            }
        }
    }
    // When searching from Actors/Kind, above matched package identities,
    // not the virtual folder names. For other modes filter by visible name.
    let searching_actor_kind = shell.asset_folder == "Actors"
        && !shell.actor_browser_kind.is_empty()
        && shell.actor_browser_family.is_empty()
        && !query.is_empty();
    if !searching_actor_kind && !query.is_empty() {
        items.retain(|entry| entry.name().to_ascii_lowercase().contains(&query));
    }
    items
}

fn folder_key(category: &str, base: &Path, path: &Path) -> String {
    format!(
        "{category}/{}",
        path.strip_prefix(base)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    )
}
fn contains_key(shell: &ShellState, key: &str) -> bool {
    shell
        .expanded_content_folders
        .iter()
        .any(|value| value == key)
}
fn toggle_key(shell: &mut ShellState, key: &str) {
    if contains_key(shell, key) {
        shell.expanded_content_folders.retain(|value| value != key);
    } else {
        shell.expanded_content_folders.push(key.to_string());
    }
}

fn breadcrumb(shell: &ShellState, root: &Path) -> Vec<(String, BrowseTarget)> {
    let mut parts = vec![("Content".into(), BrowseTarget::Root)];
    if shell.asset_folder == "Content" {
        return parts;
    }
    parts.push((
        shell.asset_folder.clone(),
        BrowseTarget::Category(shell.asset_folder.clone()),
    ));
    if shell.asset_folder == "Actors" {
        if !shell.actor_browser_kind.is_empty() {
            parts.push((
                shell.actor_browser_kind.clone(),
                BrowseTarget::ActorKind(shell.actor_browser_kind.clone()),
            ));
        }
        if !shell.actor_browser_family.is_empty() {
            parts.push((
                shell.actor_browser_family.clone(),
                BrowseTarget::ActorFamily(shell.actor_browser_family.clone()),
            ));
        }
    } else if let Some(dir) = shell.browser_directory.as_ref() {
        let category_root = physical_category_root(root, &shell.asset_folder);
        if let Ok(relative) = dir.strip_prefix(&category_root) {
            let mut path = category_root;
            for segment in relative.components() {
                path.push(segment);
                parts.push((
                    segment.as_os_str().to_string_lossy().into_owned(),
                    BrowseTarget::Directory(path.clone()),
                ));
            }
        }
    }
    parts
}

// ThemeSettings owns color, border, typography, rounding, and UI scaling.
// All asset tile artwork is vector/egui-native, not static colored PNGs.
fn draw_tile(ui: &mut Ui, entry: &BrowserGridItem, size: egui::Vec2, selected: bool) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click_and_drag());
    let visuals = ui.visuals();
    let background = if selected {
        visuals.selection.bg_fill
    } else if response.hovered() {
        visuals.widgets.hovered.bg_fill
    } else {
        visuals.widgets.inactive.bg_fill
    };
    let stroke = if selected {
        visuals.selection.stroke
    } else if response.hovered() {
        visuals.widgets.hovered.bg_stroke
    } else {
        visuals.widgets.noninteractive.bg_stroke
    };
    let rounding = visuals.widgets.inactive.rounding;
    ui.painter().rect_filled(rect, rounding, background);
    ui.painter().rect_stroke(rect, rounding, stroke);
    let accent = visuals.selection.stroke.color;
    let foreground = visuals.text_color();
    let image_y = rect.top() + size.y * 0.34;
    let image = Pos2::new(rect.center().x, image_y);
    match entry {
        BrowserGridItem::Folder { .. } => {
            let folder_body =
                Rect::from_center_size(image + egui::vec2(0.0, 4.0), egui::vec2(40.0, 27.0));
            ui.painter().rect_filled(folder_body, 2.0, accent);
            ui.painter().rect_filled(
                Rect::from_min_size(
                    egui::pos2(folder_body.left() + 2.0, folder_body.top() - 7.0),
                    egui::vec2(19.0, 9.0),
                ),
                2.0,
                accent,
            );
        }
        BrowserGridItem::Actor { .. } => {
            let edges = [
                (-15.0, -10.0, 0.0, -18.0),
                (0.0, -18.0, 15.0, -10.0),
                (15.0, -10.0, 0.0, -2.0),
                (0.0, -2.0, -15.0, -10.0),
                (-15.0, -10.0, -15.0, 9.0),
                (15.0, -10.0, 15.0, 9.0),
                (0.0, -2.0, 0.0, 19.0),
                (-15.0, 9.0, 0.0, 19.0),
                (0.0, 19.0, 15.0, 9.0),
            ];
            for (ax, ay, bx, by) in edges {
                ui.painter().line_segment(
                    [image + egui::vec2(ax, ay), image + egui::vec2(bx, by)],
                    Stroke::new(1.7, accent),
                );
            }
        }
        BrowserGridItem::File { .. } => {
            let body = Rect::from_center_size(image, egui::vec2(31.0, 40.0));
            ui.painter()
                .rect_stroke(body, 2.0, Stroke::new(1.8, accent));
            ui.painter().line_segment(
                [
                    body.left_bottom() + egui::vec2(6.0, -12.0),
                    body.right_bottom() + egui::vec2(-6.0, -12.0),
                ],
                Stroke::new(1.5, accent),
            );
            ui.painter().line_segment(
                [
                    body.left_bottom() + egui::vec2(6.0, -18.0),
                    body.right_bottom() + egui::vec2(-6.0, -18.0),
                ],
                Stroke::new(1.5, accent),
            );
        }
    }
    let max_chars = ((size.x / 7.0).floor() as usize).clamp(11, 27);
    let mut display = entry.name().chars().take(max_chars).collect::<String>();
    if entry.name().chars().count() > max_chars {
        display.push('…');
    }
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 29.0),
        egui::Align2::CENTER_CENTER,
        display,
        egui::TextStyle::Body.resolve(ui.style()),
        foreground,
    );
    let secondary = match entry {
        BrowserGridItem::Folder { count: Some(n), .. } => format!("{n} assets"),
        _ => entry.asset_type().to_string(),
    };
    ui.painter().text(
        Pos2::new(rect.center().x, rect.bottom() - 12.0),
        egui::Align2::CENTER_CENTER,
        secondary,
        egui::TextStyle::Small.resolve(ui.style()),
        visuals.weak_text_color(),
    );
    response.on_hover_text(format!("{}\n{}", entry.name(), entry.asset_type()))
}

fn tree_row(
    ui: &mut Ui,
    label: &str,
    depth: usize,
    selected: bool,
    expanded: Option<bool>,
) -> (bool, bool) {
    let mut selected_click = false;
    let mut toggle = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        ui.add_space((depth as f32 * 13.0).clamp(0.0, 90.0));
        if let Some(expanded) = expanded {
            if ui.small_button(if expanded { "▾" } else { "▸" }).clicked() {
                toggle = true;
            }
        } else {
            ui.add_space(21.0);
        }
        if ui.selectable_label(selected, label).clicked() {
            selected_click = true;
        }
    });
    (selected_click, toggle)
}

impl Editor {
    // Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:26 -05:00
    fn browse_to(&mut self, root: &Path, target: BrowseTarget) {
        match target {
            BrowseTarget::Root => {
                self.shell.asset_folder = "Content".into();
                self.shell.actor_browser_kind.clear();
                self.shell.actor_browser_family.clear();
                self.shell.browser_directory = None;
                self.shell.refresh(Some(root));
            }
            BrowseTarget::Category(category) => {
                if !CONTENT_CATEGORIES.contains(&category.as_str()) {
                    return;
                }
                self.shell.asset_folder = category;
                self.shell.actor_browser_kind.clear();
                self.shell.actor_browser_family.clear();
                self.shell.browser_directory = None;
                self.shell.refresh(Some(root));
            }
            BrowseTarget::ActorKind(kind) => {
                if self.shell.asset_folder != "Actors"
                    || (kind != "Static Actors" && kind != "Characters")
                {
                    return;
                }
                self.shell.actor_browser_kind = kind;
                self.shell.actor_browser_family.clear();
            }
            BrowseTarget::ActorFamily(family) => {
                if self.shell.asset_folder != "Actors"
                    || self.shell.actor_browser_kind.is_empty()
                    || !groups(&self.shell, &self.shell.actor_browser_kind).contains_key(&family)
                {
                    return;
                }
                self.shell.actor_browser_family = family;
            }
            BrowseTarget::Directory(folder) => {
                let category_root = physical_category_root(root, &self.shell.asset_folder);
                // Reject traversal, paths outside the category and junctions.
                if !folder
                    .canonicalize()
                    .ok()
                    .zip(category_root.canonicalize().ok())
                    .is_some_and(|(target, base)| target.starts_with(base))
                {
                    return;
                }
                if !folder.is_dir() {
                    return;
                }
                if let Ok(relative) = folder.strip_prefix(&category_root) {
                    let mut parent = category_root.clone();
                    for component in relative.components() {
                        let key = folder_key(&self.shell.asset_folder, &category_root, &parent);
                        if !contains_key(&self.shell, &key) {
                            self.shell.expanded_content_folders.push(key);
                        }
                        parent.push(component);
                    }
                }
                self.shell.browser_directory = Some(folder);
                self.shell.refresh(Some(root));
            }
        }
        self.shell.browser_search.clear();
        self.shell.save();
    }

    fn draw_content_tree(&mut self, ui: &mut Ui, root: &Path) -> Option<BrowseTarget> {
        let mut go_to = None;
        let (clicked, _) = tree_row(ui, "Content", 0, self.shell.asset_folder == "Content", None);
        if clicked {
            go_to = Some(BrowseTarget::Root);
        }
        ui.add_space(5.0);
        for category in CONTENT_CATEGORIES {
            let active = self.shell.asset_folder == category;
            let (clicked, _) = tree_row(ui, category, 1, active, None);
            if clicked {
                go_to = Some(BrowseTarget::Category(category.into()));
            }
            if !active {
                continue;
            }
            if category == "Actors" {
                for kind in ["Static Actors", "Characters"] {
                    let active_kind = self.shell.actor_browser_kind == kind;
                    let (clicked, _) = tree_row(
                        ui,
                        kind,
                        2,
                        active_kind && self.shell.actor_browser_family.is_empty(),
                        None,
                    );
                    if clicked {
                        go_to = Some(BrowseTarget::ActorKind(kind.into()));
                    }
                    if !active_kind {
                        continue;
                    }
                    for (family, names) in groups(&self.shell, kind) {
                        let (clicked, _) = tree_row(
                            ui,
                            &format!("{family} ({})", names.len()),
                            3,
                            self.shell.actor_browser_family == family,
                            None,
                        );
                        if clicked {
                            go_to = Some(BrowseTarget::ActorFamily(family));
                        }
                    }
                }
            } else {
                let base = physical_category_root(root, category);
                // Nested physical directories read once by ShellState.refresh.
                let mut toggle = None;
                for dir in &self.shell.content_subdirs {
                    let Ok(relative) = dir.strip_prefix(&base) else {
                        continue;
                    };
                    let segments = relative.components().count();
                    if segments > 7 {
                        continue;
                    }
                    let visible = dir.parent() == Some(base.as_path())
                        || dir.parent().is_some_and(|parent| {
                            contains_key(&self.shell, &folder_key(category, &base, parent))
                        });
                    if !visible {
                        continue;
                    }
                    let key = folder_key(category, &base, dir);
                    let expanded = contains_key(&self.shell, &key);
                    let label = dir.file_name().unwrap_or_default().to_string_lossy();
                    let has_children = self
                        .shell
                        .content_subdirs
                        .iter()
                        .any(|path| path.parent() == Some(dir.as_path()));
                    let (clicked, toggled) = tree_row(
                        ui,
                        &label,
                        segments + 1,
                        self.shell.browser_directory.as_deref() == Some(dir),
                        has_children.then_some(expanded),
                    );
                    if toggled {
                        toggle = Some(key);
                    }
                    if clicked {
                        go_to = Some(BrowseTarget::Directory(dir.clone()));
                    }
                }
                if let Some(key) = toggle {
                    toggle_key(&mut self.shell, &key);
                    self.shell.save();
                }
            }
        }
        go_to
    }

    pub(crate) fn draw_unreal_asset_browser(
        &mut self,
        ui: &mut Ui,
        project_path: &Option<PathBuf>,
    ) {
        let Some(root) = project_path.as_deref() else {
            ui.label("Open a project to browse content.");
            return;
        };
        if self.shell.project_snapshot.as_deref() != Some(root) {
            self.shell.browser_directory = None;
            self.shell.refresh(Some(root));
        }

        let breadcrumbs = breadcrumb(&self.shell, root);
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, (label, target)) in breadcrumbs.iter().enumerate() {
                if i > 0 {
                    ui.label("›");
                }
                if ui.add(egui::Button::new(label).frame(false)).clicked() {
                    action = Some(target.clone());
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Asset Manager").clicked() {
                    self.asset_manager.open = true;
                }
                if ui.small_button("⟳ Refresh").clicked() {
                    self.shell.refresh(Some(root));
                }
                if matches!(
                    self.shell.asset_folder.as_str(),
                    "Audio" | "Textures" | "Media"
                ) && ui.button("Import...").clicked()
                {
                    match import_browser_assets(root, &self.shell.asset_folder) {
                        Ok(count) if count > 0 => {
                            self.shell.refresh(Some(root));
                            self.queue_engine_sound(EngineSound::Confirm);
                            if matches!(self.shell.asset_folder.as_str(), "Textures" | "Audio") {
                                crate::editor::tools::refresh_asset_catalogs(ui.ctx(), root);
                            }
                        }
                        Ok(_) => {}
                        Err(error) => {
                            eprintln!("[CONTENT BROWSER] {error}");
                            self.queue_engine_sound(EngineSound::Warning);
                        }
                    }
                }
            });
        });
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut self.shell.browser_search)
                    .hint_text("Search this folder / Actors...")
                    .desired_width((ui.available_width() * 0.55).clamp(180.0, 380.0)),
            );
            if ui.small_button("×").on_hover_text("Clear search").clicked() {
                self.shell.browser_search.clear();
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.small("Double-click folders · Drag Actors into viewport");
            });
        });
        ui.separator();

        let size = ui.available_size();
        let total_width = size.x.max(320.0);
        let tree_width = self
            .shell
            .content_tree_width
            .clamp(135.0, (total_width - 195.0).max(135.0));
        let pane_height = size.y.max(120.0);
        let mut pending_file = None;
        let mut chosen_actor = None;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(tree_width, pane_height),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.set_width(tree_width);
                    ui.small("FOLDERS");
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .id_salt("unreal_asset_content_tree")
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if let Some(target) = self.draw_content_tree(ui, root) {
                                action = Some(target);
                            }
                        });
                },
            );
            // Persist user drag-resized folder navigation width.
            let (divider, resp) =
                ui.allocate_exact_size(egui::vec2(8.0, pane_height), Sense::drag());
            ui.painter().line_segment(
                [divider.center_top(), divider.center_bottom()],
                ui.visuals().widgets.noninteractive.bg_stroke,
            );
            if resp.dragged() {
                self.shell.content_tree_width = (tree_width + ui.input(|i| i.pointer.delta().x))
                    .clamp(135.0, (total_width - 195.0).max(135.0));
            }
            if resp.drag_stopped() {
                self.shell.save();
            }
            let right_width = (total_width - tree_width - 18.0).max(180.0);
            ui.allocate_ui_with_layout(
                egui::vec2(right_width, pane_height),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.set_width(right_width);
                    let items = browser_items(&self.shell);
                    ui.horizontal(|ui| {
                        ui.strong(if self.shell.asset_folder == "Content" {
                            "All Assets"
                        } else {
                            &self.shell.asset_folder
                        });
                        ui.small(format!("{} items", items.len()));
                    });
                    ui.separator();
                    let tile_width =
                        (ui.text_style_height(&egui::TextStyle::Body) * 7.9).clamp(112.0, 162.0);
                    let tile_height =
                        (ui.text_style_height(&egui::TextStyle::Body) * 7.0).clamp(102.0, 140.0);
                    let gap = ui.spacing().item_spacing.x.max(8.0);
                    let count =
                        ((right_width - gap) / (tile_width + gap)).floor().max(1.0) as usize;
                    let rows = items.len().div_ceil(count);
                    if items.is_empty() {
                        ui.add_space(13.0);
                        ui.label("This folder is empty.");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("unreal_asset_grid")
                            .auto_shrink([false, false])
                            .show_rows(ui, tile_height, rows, |ui, range| {
                                for row in range {
                                    ui.horizontal(|ui| {
                                        ui.spacing_mut().item_spacing.x = gap;
                                        for index in
                                            row * count..((row + 1) * count).min(items.len())
                                        {
                                            let entry = &items[index];
                                            let selected = match entry {
                                                BrowserGridItem::Actor { name } => {
                                                    self.build_is_actor
                                                        && self.build_actor_package.as_deref()
                                                            == Some(name)
                                                }
                                                _ => false,
                                            };
                                            let response = draw_tile(
                                                ui,
                                                entry,
                                                egui::vec2(tile_width - 3.0, tile_height - 4.0),
                                                selected,
                                            );
                                            match entry {
                                                BrowserGridItem::Folder { destination, .. } => {
                                                    if response.clicked() {
                                                        action = Some(destination.clone());
                                                    }
                                                }
                                                BrowserGridItem::Actor { name } => {
                                                    response.dnd_set_drag_payload(
                                                        ActorContentDragPayload {
                                                            package: name.clone(),
                                                            project_root: root.to_owned(),
                                                        },
                                                    );
                                                    if response.clicked() {
                                                        chosen_actor = Some(name.clone());
                                                    }
                                                }
                                                BrowserGridItem::File { path, .. } => {
                                                    if response.clicked() {
                                                        pending_file = Some(path.clone());
                                                    }
                                                }
                                            }
                                        }
                                    });
                                }
                            });
                    }
                },
            );
        });

        // Navigate or open files AFTER callbacks, never while borrowed by the
        // inventory grid render.
        if let Some(target) = action {
            self.browse_to(root, target);
        }
        if let Some(name) = chosen_actor {
            self.build_actor_package = Some(name);
            self.build_is_actor = true;
            self.activate_editor_tool(super::EditorTool::Build);
            self.queue_engine_sound(EngineSound::Select);
        }
        if let Some(path) = pending_file {
            if path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| ext.eq_ignore_ascii_case("aeo"))
            {
                self.script_editor.open_file(path);
                self.activate_shell_workspace(super::shell::Workspace::Script);
            } else {
                self.asset_manager.search = path
                    .file_name()
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.asset_manager.open = true;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn stable_name_based_virtual_actor_folders() {
        assert_eq!(actor_family("boat-row-small"), "Boat");
        assert_eq!(actor_family("decor_tree_roots"), "Decor");
        assert_eq!(actor_family(""), "Misc");
    }
    #[test]
    fn root_categories_expose_every_asset_area() {
        let shell = ShellState::default();
        let items = browser_items(&shell);
        assert_eq!(items.len(), CONTENT_CATEGORIES.len());
        assert_eq!(items[0].name(), "Actors");
        assert!(items.iter().any(|v| v.name() == "Resources"));
        assert!(items.iter().any(|v| v.name() == "Scripts"));
    }
    #[test]
    fn registered_actor_browser_uses_kind_and_family_folders() {
        let mut shell = ShellState::default();
        shell.asset_folder = "Actors".into();
        shell.actor_packages = vec![
            "boat-row".into(),
            "boat-big".into(),
            "decor_rock".into(),
            "Rogue".into(),
        ];
        shell.character_packages = vec!["Rogue".into()];
        let root = browser_items(&shell);
        assert_eq!(root.len(), 2);
        assert_eq!(root[0].name(), "Static Actors");
        shell.actor_browser_kind = "Static Actors".into();
        let families = browser_items(&shell);
        assert!(families.iter().any(|i| i.name() == "Boat"));
        shell.actor_browser_family = "Boat".into();
        let members = browser_items(&shell);
        assert_eq!(members.len(), 2);
        assert!(
            members
                .iter()
                .all(|i| matches!(i, BrowserGridItem::Actor { .. }))
        );
        shell.actor_browser_kind = "Characters".into();
        shell.actor_browser_family.clear();
        let characters = browser_items(&shell);
        assert_eq!(characters.len(), 1);
        assert_eq!(characters[0].name(), "Rogue");
    }
    #[test]
    fn physical_paths_match_project_asset_layout() {
        let root = Path::new(r"C:\Game");
        assert_eq!(
            physical_category_root(root, "Scripts"),
            root.join("scripts")
        );
        assert_eq!(
            physical_category_root(root, "Textures"),
            root.join(".assets").join("textures")
        );
        assert_eq!(
            physical_category_root(root, "Resources"),
            root.join(".assets")
        );
    }
}
