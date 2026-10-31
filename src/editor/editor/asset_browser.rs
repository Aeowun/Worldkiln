// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::project::asset_registry::{AssetInspection, AssetRecord, AssetRegistry};
use egui::{Color32, RichText};
use std::collections::BTreeMap;
use std::collections::VecDeque;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

struct CachedProjectRegistry {
    root: PathBuf,
    stamp: Option<SystemTime>,
    records: Arc<AssetRegistry>,
}

pub(crate) fn project_asset_snapshot(root: &Path) -> Arc<AssetRegistry> {
    static CACHE: OnceLock<Mutex<Option<CachedProjectRegistry>>> = OnceLock::new();
    let stamp = std::fs::metadata(root.join(".assets/asset_registry.json"))
        .and_then(|metadata| metadata.modified())
        .ok();
    let cache = CACHE.get_or_init(|| Mutex::new(None));
    let mut saved = cache.lock().unwrap_or_else(|error| error.into_inner());
    if let Some(current) = saved.as_ref() {
        if current.root == root && current.stamp == stamp {
            return Arc::clone(&current.records);
        }
    }
    let records = Arc::new(match AssetRegistry::load(root) {
        Ok(registry) => registry,
        Err(error) => {
            crate::engine::diagnostics::report(
                "ERROR",
                "Assets",
                format!("Project asset registry unavailable: {error}"),
            );
            if let Some(previous) = saved.as_ref().filter(|previous| previous.root == root) {
                previous.records.as_ref().clone()
            } else {
                AssetRegistry::default()
            }
        }
    });
    *saved = Some(CachedProjectRegistry {
        root: root.to_path_buf(),
        stamp,
        records: Arc::clone(&records),
    });
    records
}

use std::borrow::Cow;
use std::path::{Path, PathBuf};

use egui::{Align, Layout, Pos2, Rect, Response, Sense, Stroke, Ui};

use super::{
    Editor,
    shell::{ActorContentDragPayload, ShellState},
};
use crate::engine::audio::EngineSound;

pub const CONTENT_CATEGORIES: [&str; 11] = [
    "Actors",
    "Characters",
    "Controllers",
    "Cameras",
    "Textures",
    "Materials",
    "Images",
    "Video",
    "Audio",
    "Effects",
    "Scripts",
];

#[derive(Clone, Debug)]
pub(crate) struct ProjectAssetDragPayload {
    pub project_root: PathBuf,
    pub path: PathBuf,
    pub category: String,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum AssetDropKind {
    SurfaceMaterial,
    WaterMaterial,
    SkyMaterial,
    Script,
    Audio,
    Texture,
    Image,
    Video,
}
impl AssetDropKind {
    fn label(self) -> &'static str {
        match self {
            Self::SurfaceMaterial => "MATERIAL",
            Self::WaterMaterial => "WATER MATERIAL",
            Self::SkyMaterial => "SKY MATERIAL",
            Self::Script => "AEOSCRIPT",
            Self::Audio => "AUDIO",
            Self::Texture => "TEXTURE",
            Self::Image => "IMAGE",
            Self::Video => "VIDEO",
        }
    }
}
fn payload_is_candidate(
    root: &Path,
    payload: &ProjectAssetDragPayload,
    kind: AssetDropKind,
) -> bool {
    if root != payload.project_root || !payload.path.is_file() {
        return false;
    }
    let (category, folder, formats): (&str, &str, &[&str]) = match kind {
        AssetDropKind::SurfaceMaterial
        | AssetDropKind::WaterMaterial
        | AssetDropKind::SkyMaterial => ("Materials", "materials", &["json"]),
        AssetDropKind::Script if payload.category == "Player Scripts" => {
            ("Player Scripts", "playerscripts", &["aeo"])
        }
        AssetDropKind::Script => ("Scripts", "scripts", &["aeo"]),
        AssetDropKind::Audio => ("Audio", "audio", &["wav", "mp3"]),
        AssetDropKind::Texture => ("Textures", "textures", &["png"]),
        AssetDropKind::Image if payload.category == "Textures" => {
            ("Textures", "textures", &["png", "jpg", "jpeg"])
        }
        AssetDropKind::Image => ("Images", "media", &["png", "jpg", "jpeg"]),
        AssetDropKind::Video => ("Video", "media", &["mp4"]),
    };
    let category_root = if kind == AssetDropKind::Script {
        physical_category_root(root, category)
    } else {
        root.join(".assets").join(folder)
    };
    let category_matches = payload.category == category
        || (kind == AssetDropKind::Image && payload.category == "Media")
        || (kind == AssetDropKind::Video && payload.category == "Media");
    if !category_matches
        || !payload.path.starts_with(&category_root)
        || !payload
            .path
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| formats.iter().any(|f| e.eq_ignore_ascii_case(f)))
    {
        return false;
    }
    let Ok(canonical_project) = std::fs::canonicalize(root) else {
        return false;
    };
    let Ok(canonical_path) = std::fs::canonicalize(&payload.path) else {
        return false;
    };
    let owned_root = if kind == AssetDropKind::Script {
        std::fs::canonicalize(&category_root).ok()
    } else {
        Some(canonical_project.join(".assets").join(folder))
    };
    let Some(owned_root) = owned_root else {
        return false;
    };
    canonical_path.starts_with(owned_root)
}

pub(crate) fn validated_asset_assignment(
    project: &Path,
    payload: &ProjectAssetDragPayload,
    kind: AssetDropKind,
) -> Option<String> {
    if !payload_is_candidate(project, payload, kind) {
        return None;
    }
    match kind {
        AssetDropKind::SurfaceMaterial
        | AssetDropKind::WaterMaterial
        | AssetDropKind::SkyMaterial => {
            let relative = payload
                .path
                .strip_prefix(project.join(".assets/materials"))
                .ok()?;
            let name = relative
                .with_extension("")
                .to_string_lossy()
                .replace(char::from(92), "/");
            let doc = crate::project::material::read(project, &name).ok()?;
            let catalog = crate::project::material::MaterialCatalog::cached(project);
            let output = catalog.resolve(&doc.id).ok()?;
            let valid = match kind {
                AssetDropKind::SurfaceMaterial => matches!(
                    output,
                    crate::project::material::MaterialOutput::SurfacePbr { .. }
                ),
                AssetDropKind::WaterMaterial => {
                    matches!(output, crate::project::material::MaterialOutput::Water(_))
                }
                AssetDropKind::SkyMaterial => {
                    matches!(output, crate::project::material::MaterialOutput::Sky(_))
                }
                _ => false,
            };
            valid.then(|| {
                if kind == AssetDropKind::SurfaceMaterial {
                    format!("Material:{}", doc.id)
                } else {
                    doc.id
                }
            })
        }
        AssetDropKind::Script => payload
            .path
            .strip_prefix(project)
            .ok()
            .map(|path| path.to_string_lossy().replace(char::from(92), "/")),
        AssetDropKind::Audio => payload
            .path
            .strip_prefix(project.join(".assets/audio"))
            .ok()
            .map(|path| path.to_string_lossy().replace(char::from(92), "/")),
        AssetDropKind::Texture => payload
            .path
            .strip_prefix(project.join(".assets/textures"))
            .ok()
            .map(|path| path.to_string_lossy().replace(char::from(92), "/")),
        AssetDropKind::Image | AssetDropKind::Video => {
            let base = if payload.category == "Textures" {
                project.join(".assets/textures")
            } else {
                project.join(".assets/media")
            };
            let relative = payload.path.strip_prefix(base).ok()?;
            let prefix = if payload.category == "Textures" {
                "textures"
            } else {
                "Media"
            };
            let source = format!(
                "{prefix}/{}",
                relative.to_string_lossy().replace(char::from(92), "/")
            );
            crate::engine::ui_media::resolve_ui_media_path(project, &source)
                .ok()
                .map(|_| source)
        }
    }
}

#[derive(Clone)]
struct MaterialDropLabels(Arc<BTreeMap<String, String>>);
fn material_drop_label(
    ctx: &egui::Context,
    project: Option<&Path>,
    value: &str,
    kind: AssetDropKind,
) -> Option<String> {
    if !matches!(
        kind,
        AssetDropKind::SurfaceMaterial | AssetDropKind::SkyMaterial | AssetDropKind::WaterMaterial
    ) {
        return None;
    }
    let project = project?;
    let id = value.strip_prefix("Material:").unwrap_or(value);
    let key = egui::Id::new(("material_drop_labels", project));
    let found = ctx.data(|data| data.get_temp::<MaterialDropLabels>(key));
    let labels = if let Some(found) = found {
        found
    } else {
        let documents = crate::project::material::MaterialCatalog::cached(project);
        let mut labels = BTreeMap::new();
        for (name, doc) in documents.documents() {
            let display = name.rsplit('/').next().unwrap_or(name).to_owned();
            labels.insert(doc.id.clone(), display);
        }
        let stored = MaterialDropLabels(Arc::new(labels));
        ctx.data_mut(|data| data.insert_temp(key, stored.clone()));
        stored
    };
    labels.0.get(id).cloned()
}

pub(crate) fn compact_asset_drop_size(available_width: f32) -> egui::Vec2 {
    egui::vec2(available_width.max(72.0).min(300.0), 44.0)
}

pub(crate) fn draw_project_asset_drop(
    ui: &mut Ui,
    project: Option<&Path>,
    value: &mut String,
    kind: AssetDropKind,
) -> bool {
    ui.label(egui::RichText::new(format!("{}:", kind.label())).strong());
    let (rect, response) = ui.allocate_exact_size(
        compact_asset_drop_size(ui.available_width()),
        Sense::hover(),
    );
    let hover = response.dnd_hover_payload::<ProjectAssetDragPayload>();
    let valid = hover.as_ref().is_some_and(|payload| {
        project.is_some_and(|root| payload_is_candidate(root, payload, kind))
    });
    let visuals = ui.visuals();
    let selected = !value.is_empty();
    let border = if hover.is_some() && !valid {
        visuals.error_fg_color
    } else if valid || selected {
        visuals.selection.stroke.color
    } else {
        visuals.widgets.inactive.fg_stroke.color
    };
    let background = if selected {
        visuals.selection.bg_fill.gamma_multiply(0.24)
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    let rect = rect.shrink(1.0);
    ui.painter().rect_filled(rect, 5.0, background);
    let width = if valid {
        2.5
    } else if selected {
        1.8
    } else {
        1.0
    };
    for (from, to) in [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ] {
        let delta = to - from;
        let distance = delta.length();
        if distance <= 0.0 {
            continue;
        }
        let direction = delta / distance;
        let mut offset = 0.0;
        while offset < distance {
            ui.painter().line_segment(
                [
                    from + direction * offset,
                    from + direction * (offset + 7.0).min(distance),
                ],
                Stroke::new(width, border),
            );
            offset += 12.0;
        }
    }
    let primary = if hover.is_some() {
        if valid {
            "RELEASE TO ASSIGN"
        } else {
            "INVALID ASSET TYPE"
        }
    } else if selected {
        value.strip_prefix("Material:").unwrap_or(value)
    } else {
        "DROP ASSET FROM CONTENT BROWSER"
    };
    let display_name = if selected && hover.is_none() {
        material_drop_label(ui.ctx(), project, value, kind).unwrap_or_else(|| primary.to_owned())
    } else {
        primary.to_owned()
    };
    let letters = ((rect.width() - 18.0) / 7.0).max(8.0) as usize;
    let shorten = |text: &str| -> String {
        if text.chars().count() <= letters {
            text.to_owned()
        } else {
            format!(
                "{}...",
                text.chars()
                    .take(letters.saturating_sub(3))
                    .collect::<String>()
            )
        }
    };
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 9.0),
        egui::Align2::CENTER_CENTER,
        shorten(&display_name),
        egui::TextStyle::Body.resolve(ui.style()),
        visuals.text_color(),
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        if selected {
            "DROP ANOTHER ASSET TO REPLACE"
        } else {
            "CONTENT BROWSER"
        },
        egui::TextStyle::Small.resolve(ui.style()),
        visuals.weak_text_color(),
    );
    if let Some(payload) = response.dnd_release_payload::<ProjectAssetDragPayload>() {
        if let Some(new_value) =
            project.and_then(|root| validated_asset_assignment(root, &payload, kind))
        {
            if *value != new_value {
                *value = new_value;
                return true;
            }
        }
    }
    false
}

pub(crate) fn draw_actor_package_drop(
    ui: &mut Ui,
    project: Option<&Path>,
    selected: &mut Option<String>,
    available: &[String],
) -> bool {
    ui.label(egui::RichText::new("ACTOR:").strong());
    let (rect, response) = ui.allocate_exact_size(
        compact_asset_drop_size(ui.available_width()),
        Sense::hover(),
    );
    let hovering = response.dnd_hover_payload::<ActorContentDragPayload>();
    let valid = hovering.as_ref().is_some_and(|asset| {
        super::shell::resolve_actor_content_drag(asset, project, available).is_some()
    });
    let visuals = ui.visuals();
    let assigned = selected.is_some();
    let border = if hovering.is_some() && !valid {
        visuals.error_fg_color
    } else if valid || assigned {
        visuals.selection.stroke.color
    } else {
        visuals.widgets.inactive.fg_stroke.color
    };
    let rect = rect.shrink(1.0);
    let background = if assigned {
        visuals.selection.bg_fill.gamma_multiply(0.24)
    } else {
        visuals.widgets.inactive.weak_bg_fill
    };
    ui.painter().rect_filled(rect, 5.0, background);
    let width = if valid {
        2.5
    } else if assigned {
        1.8
    } else {
        1.0
    };
    for (from, to) in [
        (rect.left_top(), rect.right_top()),
        (rect.right_top(), rect.right_bottom()),
        (rect.right_bottom(), rect.left_bottom()),
        (rect.left_bottom(), rect.left_top()),
    ] {
        let delta = to - from;
        let distance = delta.length();
        if distance <= 0.0 {
            continue;
        }
        let direction = delta / distance;
        let mut offset = 0.0;
        while offset < distance {
            ui.painter().line_segment(
                [
                    from + direction * offset,
                    from + direction * (offset + 7.0).min(distance),
                ],
                Stroke::new(width, border),
            );
            offset += 12.0;
        }
    }
    let title = if hovering.is_some() {
        if valid {
            "RELEASE TO SELECT"
        } else {
            "INVALID ACTOR"
        }
    } else {
        selected
            .as_deref()
            .unwrap_or("DROP ACTOR FROM CONTENT BROWSER")
    };
    let limit = ((rect.width() - 16.0) / 7.0).max(9.0) as usize;
    let display = if title.chars().count() > limit {
        format!(
            "{}...",
            title
                .chars()
                .take(limit.saturating_sub(3))
                .collect::<String>()
        )
    } else {
        title.to_owned()
    };
    ui.painter().text(
        rect.center() - egui::vec2(0.0, 9.0),
        egui::Align2::CENTER_CENTER,
        display,
        egui::TextStyle::Body.resolve(ui.style()),
        visuals.text_color(),
    );
    ui.painter().text(
        rect.center() + egui::vec2(0.0, 12.0),
        egui::Align2::CENTER_CENTER,
        if assigned {
            "DROP ANOTHER ACTOR TO REPLACE"
        } else {
            "CONTENT BROWSER"
        },
        egui::TextStyle::Small.resolve(ui.style()),
        visuals.weak_text_color(),
    );
    if let Some(dropped) = response.dnd_release_payload::<ActorContentDragPayload>() {
        if let Some(package) =
            super::shell::resolve_actor_content_drag(&dropped, project, available)
        {
            if selected.as_deref() != Some(package.as_str()) {
                *selected = Some(package);
                return true;
            }
        }
    }
    false
}

fn candidates(root: &Path, category: &str) -> Vec<PathBuf> {
    let assets = root.join(".assets");
    match category {
        "Scripts" => vec![root.join("scripts"), root.join("Scripts")],
        "Player Scripts" => vec![root.join("playerscripts"), root.join("PlayerScripts")],
        "Actors" | "Models" => vec![assets.join("entities"), assets.join("Models")],
        "Characters" => vec![assets.join("characters")],
        "Textures" => vec![assets.join("textures"), assets.join("Textures")],
        "Materials" => vec![assets.join("materials")],
        "Effects" => vec![assets.join("effects")],
        "Media" => vec![assets.join("media"), assets.join("Media")],
        "Audio" => vec![assets.join("audio"), assets.join("Audio")],
        "UI" => vec![assets.join("ui"), assets.join("UI")],
        "Resources" => vec![assets],
        _ => vec![root.to_owned()],
    }
}

pub fn physical_category_root(root: &Path, category: &str) -> PathBuf {
    let choices = candidates(root, category);
    choices
        .iter()
        .find(|path| path.is_dir())
        .cloned()
        .unwrap_or_else(|| choices[0].clone())
}

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
                crate::project::fs_walk::safe_kind(&entry.path())
                    .is_ok_and(|kind| kind == Some(true))
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

fn actor_family(shell: &ShellState, package: &str) -> String {
    let display = shell
        .actor_display_names
        .get(package)
        .map(String::as_str)
        .unwrap_or(package);
    let segment = display
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
fn groups_uncached(shell: &ShellState, kind: &str) -> BTreeMap<String, Vec<String>> {
    let characters = kind == "Characters";
    let mut result = BTreeMap::<String, Vec<String>>::new();
    for package in &shell.actor_packages {
        if character_package(shell, package) != characters {
            continue;
        }
        result
            .entry(actor_family(shell, package))
            .or_default()
            .push(package.clone());
    }
    for names in result.values_mut() {
        names.sort_by_key(|name| {
            shell
                .actor_display_names
                .get(name)
                .map(String::as_str)
                .unwrap_or(name.as_str())
                .to_ascii_lowercase()
        });
    }
    result
}

pub(super) fn refresh_actor_group_cache(shell: &mut ShellState) {
    shell.actor_groups_static = groups_uncached(shell, "Static Actors");
    shell.actor_groups_characters = groups_uncached(shell, "Characters");
    shell.actor_groups_valid = true;
    shell.browser_items_cache = None;
}

fn groups<'a>(shell: &'a ShellState, kind: &str) -> Cow<'a, BTreeMap<String, Vec<String>>> {
    if shell.actor_groups_valid {
        if kind == "Characters" {
            return Cow::Borrowed(&shell.actor_groups_characters);
        }
        return Cow::Borrowed(&shell.actor_groups_static);
    }
    Cow::Owned(groups_uncached(shell, kind))
}

#[derive(Debug)]
pub(crate) struct BrowserItemCache {
    key: BrowserItemKey,
    items: Arc<[BrowserGridItem]>,
}
#[derive(Debug, PartialEq, Eq)]
struct BrowserItemKey {
    folder: String,
    kind: String,
    family: String,
    search: String,
    directory: Option<PathBuf>,
    registry_project: Option<PathBuf>,
    registry_stamp: Option<SystemTime>,
}
impl BrowserItemKey {
    fn current(shell: &ShellState) -> Self {
        Self {
            folder: shell.asset_folder.clone(),
            kind: shell.actor_browser_kind.clone(),
            family: shell.actor_browser_family.clone(),
            search: shell.browser_search.clone(),
            directory: shell.browser_directory.clone(),
            registry_project: None,
            registry_stamp: None,
        }
    }
    fn for_registry(shell: &ShellState, root: &Path) -> Self {
        let mut key = Self::current(shell);
        key.registry_project = Some(root.to_path_buf());
        key.registry_stamp = std::fs::metadata(root.join(".assets/asset_registry.json"))
            .and_then(|metadata| metadata.modified())
            .ok();
        key
    }
}
fn browser_items_cached(shell: &mut ShellState) -> Arc<[BrowserGridItem]> {
    let key = BrowserItemKey::current(shell);
    if let Some(saved) = &shell.browser_items_cache {
        if saved.key == key {
            return saved.items.clone();
        }
    }
    let items: Arc<[BrowserGridItem]> = browser_items(shell).into();
    shell.browser_items_cache = Some(BrowserItemCache {
        key,
        items: items.clone(),
    });
    items
}

fn visual_group_metadata_checked(root: &Path) -> Result<BTreeMap<String, String>, String> {
    let file = root.join(".assets/actor_import_groups.json");
    if !file.exists() {
        return Ok(BTreeMap::new());
    }
    let bytes = std::fs::read(file).map_err(|e| e.to_string())?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid Actor groups; original retained: {e}"))
}
fn visual_group_metadata(root: &Path) -> BTreeMap<String, String> {
    visual_group_metadata_checked(root).unwrap_or_default()
}
fn imported_visual_groups(
    registry: &AssetRegistry,
    category: &str,
    root: Option<&Path>,
) -> BTreeMap<String, usize> {
    let names = root.map(visual_group_metadata).unwrap_or_default();
    let mut result = BTreeMap::new();
    for record in registry
        .assets
        .iter()
        .filter(|r| !r.is_actor_owned_texture() && r.kind.category() == category)
    {
        let group = names
            .get(&record.name)
            .or_else(|| names.get(&record.id))
            .map(String::as_str)
            .unwrap_or("Legacy");
        *result.entry(group.to_owned()).or_insert(0) += 1;
    }
    result
}
fn rename_import_display_group(
    root: &Path,
    registry: &AssetRegistry,
    category: &str,
    from: &str,
    to: &str,
) -> Result<(), String> {
    let to = to.trim();
    if to.is_empty()
        || to.chars().count() > 80
        || to
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
    {
        return Err("Invalid visual folder name".into());
    }
    let mut metadata = visual_group_metadata_checked(root)?;
    let originals = metadata.clone();
    for record in registry
        .assets
        .iter()
        .filter(|r| !r.is_actor_owned_texture() && r.kind.category() == category)
    {
        let group = originals
            .get(&record.name)
            .or_else(|| originals.get(&record.id))
            .map(String::as_str)
            .unwrap_or("Legacy");
        if group == from {
            metadata.insert(record.name.clone(), to.into());
        }
    }
    let bytes = serde_json::to_vec_pretty(&metadata).map_err(|e| e.to_string())?;
    crate::project::atomic_file::write_atomically(
        &root.join(".assets/actor_import_groups.json"),
        &bytes,
    )
    .map_err(|e| e.to_string())
}

fn registered_browser_items(shell: &ShellState, registry: &AssetRegistry) -> Vec<BrowserGridItem> {
    if shell.asset_folder == "Content" {
        return CONTENT_CATEGORIES
            .iter()
            .map(|&category| BrowserGridItem::Folder {
                name: category.to_owned(),
                destination: BrowseTarget::Category(category.to_owned()),
                count: Some(
                    registry
                        .assets
                        .iter()
                        .filter(|r| !r.is_actor_owned_texture() && r.kind.category() == category)
                        .count(),
                ),
            })
            .collect();
    }
    let query = shell.browser_search.trim().to_ascii_lowercase();
    if matches!(shell.asset_folder.as_str(), "Actors" | "Characters")
        && query.is_empty()
        && shell.actor_browser_family.is_empty()
    {
        let groups = imported_visual_groups(
            registry,
            &shell.asset_folder,
            shell.project_snapshot.as_deref(),
        );
        return groups
            .into_iter()
            .map(|(name, count)| BrowserGridItem::Folder {
                destination: BrowseTarget::ActorFamily(name.clone()),
                name,
                count: Some(count),
            })
            .collect();
    }
    let mut assets: Vec<_> = registry
        .assets
        .iter()
        .filter(|record| !record.is_actor_owned_texture())
        .filter(|record| record.kind.category() == shell.asset_folder)
        .filter(|record| {
            if matches!(shell.asset_folder.as_str(), "Actors" | "Characters")
                && query.is_empty()
                && !shell.actor_browser_family.is_empty()
            {
                let root = shell.project_snapshot.as_deref();
                let groups = root.map(visual_group_metadata).unwrap_or_default();
                groups
                    .get(&record.name)
                    .or_else(|| groups.get(&record.id))
                    .map(String::as_str)
                    .unwrap_or("Legacy")
                    == shell.actor_browser_family
            } else {
                true
            }
        })
        .filter(|record| {
            query.is_empty()
                || record.name.to_ascii_lowercase().contains(&query)
                || record.id.to_ascii_lowercase().contains(&query)
        })
        .map(|record| BrowserGridItem::Registered {
            id: record.id.clone(),
            name: record.name.clone(),
            category: record.kind.category(),
            path: PathBuf::from(&record.path),
        })
        .collect();
    assets.sort_by(|a, b| {
        a.name()
            .to_ascii_lowercase()
            .cmp(&b.name().to_ascii_lowercase())
    });
    assets
}
fn registered_browser_items_cached(shell: &mut ShellState, root: &Path) -> Arc<[BrowserGridItem]> {
    let key = BrowserItemKey::for_registry(shell, root);
    if let Some(cached) = &shell.browser_items_cache {
        if cached.key == key {
            return Arc::clone(&cached.items);
        }
    }
    let registry = project_asset_snapshot(root);
    let items: Arc<[BrowserGridItem]> = registered_browser_items(shell, &registry).into();
    shell.browser_items_cache = Some(BrowserItemCache {
        key,
        items: Arc::clone(&items),
    });
    items
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
    Character {
        name: String,
    },
    File {
        name: String,
        path: PathBuf,
    },
    Registered {
        id: String,
        name: String,
        category: &'static str,
        path: PathBuf,
    },
}
impl BrowserGridItem {
    fn name(&self) -> &str {
        match self {
            Self::Folder { name, .. }
            | Self::Actor { name }
            | Self::Character { name }
            | Self::File { name, .. }
            | Self::Registered { name, .. } => name,
        }
    }
    fn asset_type(&self) -> &'static str {
        match self {
            Self::Folder { .. } => "Folder",
            Self::Registered { category, .. } => match *category {
                "Actors" => "Actor",
                "Characters" => "Character",
                "Controllers" => "Controller",
                "Cameras" => "Camera",
                "Textures" => "Texture",
                "Materials" => "Material",
                "Images" => "Image",
                "Video" => "Video",
                "Audio" => "Audio",
                "Effects" => "Effect",
                "Scripts" => "Script",
                _ => "Asset",
            },
            Self::Actor { .. } => "Actor",
            Self::Character { .. } => "Character",
            Self::File { path, .. } => {
                match path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "aeo" => "AeoScript",
                    "efkefc" | "efkpkg" => "Effect",
                    "png" | "jpg" | "jpeg" | "webp" | "bmp" | "tga" => "Texture",
                    "wav" | "mp3" | "ogg" | "flac" => "Audio",
                    "mp4" | "webm" | "mov" => "Video",
                    "glb" | "gltf" | "fbx" | "obj" => "Model",
                    "json"
                        if path.components().any(|part| {
                            part.as_os_str()
                                .to_string_lossy()
                                .eq_ignore_ascii_case("materials")
                        }) =>
                    {
                        "Material"
                    }
                    "json"
                        if path.file_name().and_then(|name| name.to_str()).is_some_and(
                            |name| name.to_ascii_lowercase().ends_with(".mat.json"),
                        ) =>
                    {
                        "Material"
                    }
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
        "Characters" => {
            for name in &shell.character_packages {
                items.push(BrowserGridItem::Character { name: name.clone() });
            }
        }
        "Actors" => {
            if true {
                for name in &shell.actor_packages {
                    items.push(BrowserGridItem::Actor { name: name.clone() });
                }
            } else if shell.actor_browser_kind.is_empty() {
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
                    if query.is_empty() {
                        for (family, names) in map.iter() {
                            items.push(BrowserGridItem::Folder {
                                name: family.clone(),
                                destination: BrowseTarget::ActorFamily(family.clone()),
                                count: Some(names.len()),
                            });
                        }
                    } else {
                        for names in map.values() {
                            for name in names {
                                if name.to_ascii_lowercase().contains(&query)
                                    || shell.actor_display_names.get(name).is_some_and(|label| {
                                        label.to_ascii_lowercase().contains(&query)
                                    })
                                {
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
    let searching_actor_kind = shell.asset_folder == "Actors"
        && !shell.actor_browser_kind.is_empty()
        && shell.actor_browser_family.is_empty()
        && !query.is_empty();
    if !searching_actor_kind && !query.is_empty() {
        items.retain(|entry| {
            entry.name().to_ascii_lowercase().contains(&query)
                || match entry {
                    BrowserGridItem::Actor { name } => shell
                        .actor_display_names
                        .get(name)
                        .is_some_and(|label| label.to_ascii_lowercase().contains(&query)),
                    _ => false,
                }
        });
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
    if matches!(shell.asset_folder.as_str(), "Actors" | "Characters") {
        if shell.actor_browser_kind.is_empty() && !shell.actor_browser_family.is_empty() {
            parts.push((
                shell.actor_browser_family.clone(),
                BrowseTarget::ActorFamily(shell.actor_browser_family.clone()),
            ));
        }
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

fn draw_tile(
    ui: &mut Ui,
    entry: &BrowserGridItem,
    size: egui::Vec2,
    selected: bool,
    display_name: Option<&str>,
) -> Response {
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
        BrowserGridItem::Actor { .. } | BrowserGridItem::Character { .. } => {
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
        BrowserGridItem::Registered {
            category: "Actors" | "Characters",
            ..
        } => {
            let points = [
                egui::vec2(-17.0, -8.0),
                egui::vec2(0.0, -18.0),
                egui::vec2(17.0, -8.0),
                egui::vec2(17.0, 11.0),
                egui::vec2(0.0, 20.0),
                egui::vec2(-17.0, 11.0),
            ];
            for i in 0..points.len() {
                ui.painter().line_segment(
                    [image + points[i], image + points[(i + 1) % points.len()]],
                    Stroke::new(1.7, accent),
                );
            }
        }
        BrowserGridItem::File { .. } | BrowserGridItem::Registered { .. } => {
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
    let title = display_name.unwrap_or_else(|| entry.name());
    let max_chars = ((size.x / 7.0).floor() as usize).clamp(11, 27);
    let mut display = title.chars().take(max_chars).collect::<String>();
    if title.chars().count() > max_chars {
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
    response.on_hover_text(format!(
        "{}\n{}\nPackage: {}",
        title,
        entry.asset_type(),
        entry.name()
    ))
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
            if ui
                .small_button(if expanded { "-" } else { "+" })
                .on_hover_text(if expanded {
                    "Collapse folder"
                } else {
                    "Expand folder"
                })
                .clicked()
            {
                toggle = true;
            }
        } else {
            ui.add_space(21.0);
        }
        if ui.selectable_label(selected, label).clicked() {
            selected_click = true;
            if expanded.is_some() {
                toggle = true;
            }
        }
    });
    (selected_click, toggle)
}

type PreviewJob = (PathBuf, u64);
type PreviewResult = (PathBuf, u64, Option<(usize, usize, Vec<u8>)>);
struct ImagePreviewCache {
    entries: VecDeque<(PathBuf, u64, Option<egui::TextureHandle>)>,
    pending: std::collections::HashSet<PreviewJob>,
    requests: std::sync::mpsc::Sender<PreviewJob>,
    results: std::sync::mpsc::Receiver<PreviewResult>,
}
impl ImagePreviewCache {
    fn new(context: egui::Context) -> Self {
        let (requests, jobs) = std::sync::mpsc::channel::<PreviewJob>();
        let (completed, results) = std::sync::mpsc::channel::<PreviewResult>();
        std::thread::Builder::new()
            .name("worldkiln-thumbnail-decode".into())
            .spawn(move || {
                while let Ok((path, revision)) = jobs.recv() {
                    let result = image::open(&path).ok().map(|image| {
                        let rgba = image.thumbnail(240, 240).into_rgba8();
                        (
                            rgba.width() as usize,
                            rgba.height() as usize,
                            rgba.into_raw(),
                        )
                    });
                    if completed.send((path, revision, result)).is_err() {
                        break;
                    }
                    context.request_repaint();
                }
            })
            .expect("thumbnail decode worker");
        Self {
            entries: VecDeque::new(),
            pending: Default::default(),
            requests,
            results,
        }
    }
}
fn image_preview(ui: &Ui, path: &Path, revision: u64) -> Option<egui::TextureHandle> {
    static CACHE: OnceLock<Mutex<ImagePreviewCache>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(ImagePreviewCache::new(ui.ctx().clone())));
    let mut cache = cache.lock().unwrap_or_else(|error| error.into_inner());
    for _ in 0..8 {
        let Ok((path, revision, result)) = cache.results.try_recv() else {
            break;
        };
        cache.pending.remove(&(path.clone(), revision));
        let preview = result.map(|(width, height, rgba)| {
            ui.ctx().load_texture(
                format!("worldkiln_asset_thumb:{}:{revision}", path.display()),
                egui::ColorImage::from_rgba_unmultiplied([width, height], &rgba),
                egui::TextureOptions::LINEAR,
            )
        });
        cache.entries.retain(|(p, _, _)| p != &path);
        if cache.entries.len() >= 128 {
            cache.entries.pop_front();
        }
        cache.entries.push_back((path, revision, preview));
    }
    if let Some((_, _, preview)) = cache
        .entries
        .iter()
        .find(|(p, r, _)| p == path && *r == revision)
    {
        return preview.clone();
    }
    let job = (path.to_path_buf(), revision);
    if cache.pending.len() < 32 && cache.pending.insert(job.clone()) {
        if cache.requests.send(job.clone()).is_err() {
            cache.pending.remove(&job);
        }
    }
    None
}
fn paint_image_preview(ui: &Ui, bounds: Rect, path: &Path, revision: u64) -> bool {
    let Some(handle) = image_preview(ui, path, revision) else {
        return false;
    };
    let native = handle.size_vec2();
    let scale = (bounds.width() / native.x.max(1.0)).min(bounds.height() / native.y.max(1.0));
    let size = native * scale;
    let destination = Rect::from_center_size(bounds.center(), size);
    ui.painter().image(
        handle.id(),
        destination,
        Rect::from_min_max(Pos2::ZERO, egui::pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    true
}

pub(crate) fn draw_registered_asset_inspector(
    ui: &mut Ui,
    record: &AssetRecord,
    registry: &AssetRegistry,
    root: &Path,
) {
    ui.label(RichText::new(format!("{} · {}", record.kind.category(), record.name)).strong());
    ui.horizontal(|ui| {
        ui.small("Asset ID");
        ui.monospace(&record.id);
        if ui.small_button("Copy").clicked() {
            ui.ctx().copy_text(record.id.clone());
        }
    });
    ui.small(format!("Project resource: {}", record.path));
    if let Some(group) = &record.group {
        ui.small(format!("Group: {group}"));
    }
    let resource = root.join(&record.path);
    if !resource.exists() {
        ui.colored_label(
            Color32::from_rgb(230, 145, 80),
            "Source resource unavailable",
        );
    }
    if matches!(
        record.kind,
        crate::project::asset_registry::AssetType::Actor
            | crate::project::asset_registry::AssetType::Character
    ) {
        let (area, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(225.0), 135.0),
            Sense::hover(),
        );
        if !crate::editor::actor_model_preview::paint_model(ui, area, &resource) {
            ui.painter().text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "3D preview unavailable",
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
    }
    ui.separator();
    if matches!(
        record.kind,
        crate::project::asset_registry::AssetType::Texture
            | crate::project::asset_registry::AssetType::Image
    ) {
        let (area, _) = ui.allocate_exact_size(
            egui::vec2(ui.available_width().min(225.0), 145.0),
            Sense::hover(),
        );
        if !paint_image_preview(ui, area, &resource, registry.revision) {
            ui.painter().text(
                area.center(),
                egui::Align2::CENTER_CENTER,
                "Image preview unavailable",
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
    }
    ui.separator();
    static DETAILS: OnceLock<
        Mutex<VecDeque<(PathBuf, String, u64, Result<AssetInspection, String>)>>,
    > = OnceLock::new();
    let entries = DETAILS.get_or_init(|| Mutex::new(VecDeque::new()));
    let mut cache = entries.lock().unwrap_or_else(|error| error.into_inner());
    let detail = if let Some((_, _, _, saved)) = cache.iter().find(|(project, id, revision, _)| {
        project == root && id == &record.id && *revision == registry.revision
    }) {
        saved.clone()
    } else {
        let result = registry.inspect(root, &record.id);
        if cache.len() >= 32 {
            cache.pop_front();
        }
        cache.push_back((
            root.to_path_buf(),
            record.id.clone(),
            registry.revision,
            result.clone(),
        ));
        result
    };
    drop(cache);
    match detail {
        Ok(detail) => {
            let section = |ui: &mut Ui, title: &str, values: &[String]| {
                egui::CollapsingHeader::new(format!("{title} ({})", values.len()))
                    .default_open(!values.is_empty())
                    .show(ui, |ui| {
                        if values.is_empty() {
                            ui.small("None");
                        }
                        for value in values {
                            ui.label(value);
                        }
                    });
            };
            if matches!(
                record.kind,
                crate::project::asset_registry::AssetType::Actor
            ) {
                section(ui, "Geometry", &detail.meshes);
                section(ui, "Materials", &detail.materials);
                section(ui, "Animations", &detail.animations);
                ui.label(format!("Skeleton joints: {}", detail.skeleton_joint_count));
                ui.label(format!(
                    "Collision: {}",
                    match detail.collision_enabled {
                        Some(true) => "Enabled",
                        Some(false) => "Disabled",
                        None => "Not specified",
                    }
                ));
                ui.label(format!(
                    "Collision shapes: {}",
                    detail.collision_shape_count
                ));
            }
            if matches!(
                record.kind,
                crate::project::asset_registry::AssetType::Character
            ) {
                for (label, value) in [
                    ("Source Actor", detail.source_actor.as_deref()),
                    ("Controller", detail.controller.as_deref()),
                    ("Camera", detail.camera.as_deref()),
                ] {
                    ui.label(format!("{label}: {}", value.unwrap_or("Not assigned")));
                }
            }
            let actor_textures = registry.actor_owned_textures(record);
            if !actor_textures.is_empty() {
                egui::CollapsingHeader::new(format!("Textures ({})", actor_textures.len())).show(
                    ui,
                    |ui| {
                        for texture in actor_textures {
                            ui.push_id(&texture.id, |ui| {
                                egui::CollapsingHeader::new(&texture.name).show(ui, |ui| {
                                    ui.small(&texture.path);
                                    let (area, _) = ui.allocate_exact_size(
                                        egui::vec2(ui.available_width().min(225.0), 140.0),
                                        Sense::hover(),
                                    );
                                    if !paint_image_preview(
                                        ui,
                                        area,
                                        &root.join(&texture.path),
                                        registry.revision,
                                    ) {
                                        ui.small("Texture preview unavailable");
                                    }
                                });
                            });
                        }
                    },
                );
            }
            ui.separator();
            ui.strong(format!("Dependencies ({})", detail.dependencies.len()));
            for id in &detail.dependencies {
                if let Some(dependency) = registry.find(id) {
                    ui.label(format!(
                        "{}: {}",
                        dependency.kind.category(),
                        dependency.name
                    ))
                    .on_hover_text(id);
                } else {
                    ui.colored_label(
                        Color32::from_rgb(230, 145, 80),
                        format!("Unresolved AssetID: {id}"),
                    );
                }
            }
            if detail.dependencies.is_empty() {
                ui.small("No registered dependencies");
            }
            for missing in &detail.missing_dependencies {
                ui.colored_label(
                    Color32::from_rgb(230, 145, 80),
                    format!("Missing: {missing}"),
                );
            }
        }
        Err(error) => {
            ui.colored_label(
                Color32::from_rgb(230, 145, 80),
                format!("Inspection unavailable: {error}"),
            );
        }
    }
}
impl Editor {
    fn browse_to(&mut self, root: &Path, target: BrowseTarget) {
        match target {
            BrowseTarget::Root => {
                self.shell.asset_folder = "Content".into();
                self.shell.actor_browser_kind.clear();
                self.shell.actor_browser_family.clear();
                self.shell.browser_directory = None;
                self.shell.browser_items_cache = None;
            }
            BrowseTarget::Category(category) => {
                if !CONTENT_CATEGORIES.contains(&category.as_str()) {
                    return;
                }
                self.shell.asset_folder = category;
                self.shell.actor_browser_kind.clear();
                self.shell.actor_browser_family.clear();
                self.shell.browser_directory = None;
                self.shell.browser_items_cache = None;
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
                if !matches!(self.shell.asset_folder.as_str(), "Actors" | "Characters") {
                    return;
                }
                self.shell.actor_browser_family = family;
                self.shell.browser_items_cache = None;
            }
            BrowseTarget::Directory(folder) => {
                let category_root = physical_category_root(root, &self.shell.asset_folder);
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

    fn draw_content_tree(&mut self, ui: &mut Ui, _root: &Path) -> Option<BrowseTarget> {
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
        let asset_revision = project_asset_snapshot(root).revision;
        if self.shell.project_snapshot.as_deref() != Some(root) {
            self.shell.browser_directory = None;
            self.shell.refresh(Some(root));
        }

        let breadcrumbs = breadcrumb(&self.shell, root);
        let mut create_effect = false;
        let mut action = None;
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 6.0;
            for (i, (label, target)) in breadcrumbs.iter().enumerate() {
                if i > 0 {
                    ui.label("/");
                }
                if ui.add(egui::Button::new(label).frame(false)).clicked() {
                    action = Some(target.clone());
                }
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Asset Manager").clicked() {
                    self.asset_manager.open = true;
                }
                if self.shell.asset_folder == "Effects"
                    && ui.small_button("Create Effect").clicked()
                {
                    create_effect = true;
                }
                if ui.small_button("Refresh").clicked() {
                    self.shell.refresh(Some(root));
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
                ui.small("Select assets or drag them into supported targets");
            });
        });
        ui.separator();
        match AssetRegistry::background_index_status(root) {
            Some(crate::project::asset_registry::BackgroundIndexStatus::Indexing) => {
                ui.horizontal(|ui| {
                    ui.add(egui::Spinner::new().size(14.0));
                    ui.small("Indexing project assets in background...");
                });
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(120));
            }
            Some(crate::project::asset_registry::BackgroundIndexStatus::Failed(error)) => {
                ui.colored_label(
                    Color32::from_rgb(230, 145, 80),
                    format!("Asset indexing failed: {error}"),
                );
            }
            _ => {}
        }

        let size = ui.available_size();
        let total_width = size.x.max(320.0);
        let tree_width = self
            .shell
            .content_tree_width
            .clamp(135.0, (total_width - 195.0).max(135.0));
        let pane_height = size.y.max(120.0);
        let mut pending_file = None;
        let mut pending_effect = None;
        let mut chosen_actor = None;
        let mut chosen_character = None;
        let selection_key = egui::Id::new(("worldkiln_asset_selection", root));
        let mut selected_id: Option<String> = ui.ctx().data(|data| data.get_temp(selection_key));
        let mut inspect_id: Option<String> = None;
        ui.horizontal(|ui| {
            ui.allocate_ui_with_layout(
                egui::vec2(tree_width, pane_height),
                Layout::top_down(Align::LEFT),
                |ui| {
                    ui.set_width(tree_width);
                    ui.small("ASSET TYPES");
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
                    let items = registered_browser_items_cached(&mut self.shell, root);
                    let builtins_count = if self.shell.asset_folder == "Effects" {
                        let query = self.shell.browser_search.trim().to_ascii_lowercase();
                        [
                            crate::engine::particles::EmitterPreset::Fire,
                            crate::engine::particles::EmitterPreset::Smoke,
                            crate::engine::particles::EmitterPreset::Magic,
                        ]
                        .iter()
                        .filter(|preset| query.is_empty()
                            || preset.display_name().to_ascii_lowercase().contains(&query))
                        .count()
                    } else {
                        0
                    };
                    ui.horizontal(|ui| {
                        ui.strong(if self.shell.asset_folder == "Content" {
                            "All Assets"
                        } else {
                            &self.shell.asset_folder
                        });
                        ui.small(format!("{} items", items.len() + builtins_count));
                    });
                    ui.separator();
                    let tile_width =
                        (ui.text_style_height(&egui::TextStyle::Body) * 7.9).clamp(112.0, 162.0);
                    let tile_height =
                        (ui.text_style_height(&egui::TextStyle::Body) * 7.0).clamp(102.0, 140.0);
                    let gap = ui.spacing().item_spacing.x.max(8.0);
                    if self.shell.asset_folder == "Effects" {
                        let presets = [
                            crate::engine::particles::EmitterPreset::Fire,
                            crate::engine::particles::EmitterPreset::Smoke,
                            crate::engine::particles::EmitterPreset::Magic,
                        ];
                        let filter = self.shell.browser_search.trim().to_ascii_lowercase();
                        let visible = presets.into_iter()
                            .filter(|preset| filter.is_empty()
                                || preset.display_name().to_ascii_lowercase().contains(&filter))
                            .collect::<Vec<_>>();
                        if !visible.is_empty() {
                            ui.strong("Built-in");
                            ui.horizontal_wrapped(|ui| {
                                ui.spacing_mut().item_spacing.x = gap;
                                for preset in visible {
                                    let tile = BrowserGridItem::Registered {
                                        id: format!("builtin:{}", preset.name()),
                                        name: preset.display_name().to_owned(),
                                        category: "Effects",
                                        path: PathBuf::new(),
                                    };
                                    let response = draw_tile(
                                        ui,
                                        &tile,
                                        egui::vec2(tile_width - 3.0, tile_height - 4.0),
                                        false,
                                        None,
                                    );
                                    response.dnd_set_drag_payload(
                                        super::shell::BuiltinFxContentDragPayload {
                                            preset,
                                            project_root: root.to_owned(),
                                        },
                                    );
                                }
                            });
                            ui.separator();
                        }
                    }
                    let count =
                        ((right_width - gap) / (tile_width + gap)).floor().max(1.0) as usize;
                    let rows = items.len().div_ceil(count);
                    if items.is_empty() {
                        if self.shell.asset_folder != "Effects" {
                            ui.add_space(13.0);
                            ui.label("No assets registered in this category.");
                        }
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
                                                BrowserGridItem::Registered { id, .. } => {
                                                    selected_id.as_deref() == Some(id.as_str())
                                                }
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
                                                match entry {
                                                    BrowserGridItem::Actor { name } => self.shell.actor_display_names
                                                        .get(name).map(String::as_str),
                                                    _ => None,
                                                },
                                            );
                                            if let BrowserGridItem::Registered { category, path, .. } = entry {
                                                if matches!(*category, "Actors" | "Characters") {
                                                    let preview = Rect::from_min_max(
                                                        response.rect.min + egui::vec2(9.0, 7.0),
                                                        egui::pos2(response.rect.right() - 9.0,
                                                            response.rect.bottom() - 48.0),
                                                    );
                                                    if !crate::editor::actor_model_preview::paint_model(
                                                        ui, preview, &root.join(path),
                                                    ) {
                                                        ui.painter().text(preview.center(), egui::Align2::CENTER_CENTER,
                                                            "Preview unavailable", egui::TextStyle::Small.resolve(ui.style()),
                                                            ui.visuals().weak_text_color());
                                                    }
                                                }
                                            }
                                            if let BrowserGridItem::Registered { category: "Images" | "Textures", path, .. } = entry {
                                                let preview = Rect::from_min_max(
                                                    response.rect.min + egui::vec2(9.0, 7.0),
                                                    egui::pos2(response.rect.right() - 9.0, response.rect.bottom() - 48.0),
                                                );
                                                if !paint_image_preview(ui, preview, &root.join(path),asset_revision) {
                                                    ui.painter().text(preview.center(), egui::Align2::CENTER_CENTER,
                                                        "Preview unavailable", egui::TextStyle::Small.resolve(ui.style()),
                                                        ui.visuals().weak_text_color());
                                                }
                                            }
                                            if let BrowserGridItem::Actor { name } = entry {
                                                let folder = if character_package(&self.shell, name) {
                                                    "characters"
                                                } else {
                                                    "entities"
                                                };
                                                let model_path = root.join(".assets").join(folder).join(name);
                                                let preview = Rect::from_min_max(
                                                    response.rect.min + egui::vec2(9.0, 7.0),
                                                    egui::pos2(
                                                        response.rect.right() - 9.0,
                                                        response.rect.bottom() - 48.0,
                                                    ),
                                                );
                                                crate::editor::actor_model_preview::paint_model(
                                                    ui, preview, &model_path,
                                                );
                                            }
                                            match entry {
                                                BrowserGridItem::Folder { destination, name, .. } => {
                                                    if let BrowseTarget::ActorFamily(group)=destination {
                                                        let id=ui.id().with(("visual_import_rename", &self.shell.asset_folder, group));
                                                        response.context_menu(|ui| {
                                                            if ui.button("Rename Folder").clicked() {
                                                                ui.ctx().data_mut(|d|d.insert_temp(id,Some(group.clone())));
                                                                ui.close_menu();
                                                            }
                                                        });
                                                        let pending:Option<String>=ui.ctx().data(|d|d.get_temp(id)).flatten();
                                                        if let Some(mut draft)=pending {
                                                            let mut save=false;
                                                            let mut cancel=false;
                                                            ui.ctx().data_mut(|d|d.remove::<Option<String>>(id));
                                                            egui::Window::new(format!("Rename {}", name))
                                                                .id(id.with("dialog"))
                                                                .collapsible(false).resizable(false)
                                                                .show(ui.ctx(),|ui| {
                                                                    ui.text_edit_singleline(&mut draft);
                                                                    ui.horizontal(|ui| {
                                                                        if ui.button("Save").clicked(){save=true;}
                                                                        if ui.button("Cancel").clicked(){cancel=true;}
                                                                    });
                                                                });
                                                            if save {
                                                                let registry=project_asset_snapshot(root);
                                                                match rename_import_display_group(root,&registry,&self.shell.asset_folder,group,&draft){
                                                                    Ok(())=>{
                                                                        self.shell.browser_items_cache=None;
                                                                        self.shell.actor_browser_family.clear();
                                                                        self.shell.refresh(Some(root));
                                                                    }
                                                                    Err(e)=>crate::engine::diagnostics::report("ERROR","Content Browser",e),
                                                                }
                                                            } else if !cancel {
                                                                ui.ctx().data_mut(|d|d.insert_temp(id,Some(draft)));
                                                            }
                                                        }
                                                    }
                                                    if response.double_clicked() {
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
                                                BrowserGridItem::Character { name } => {
                                                    response.dnd_set_drag_payload(
                                                        ActorContentDragPayload {
                                                            package: name.clone(),
                                                            project_root: root.to_owned(),
                                                        },
                                                    );
                                                    if response.double_clicked() {
                                                        chosen_character = Some(name.clone());
                                                    }
                                                }
                                                BrowserGridItem::Registered { id, name, category, path } => {
                                                    let package = path.file_name().and_then(|n| n.to_str())
                                                        .unwrap_or(name).to_owned();
                                                    if *category == "Actors" {
                                                        response.dnd_set_drag_payload(ActorContentDragPayload {
                                                            package: id.clone(), project_root: root.to_owned(),
                                                        });
                                                    } else if *category == "Characters" {
                                                        response.dnd_set_drag_payload(ActorContentDragPayload {
                                                            package: id.clone(), project_root: root.to_owned(),
                                                        });
                                                        if response.double_clicked() { chosen_character = Some(package); }
                                                    } else {
                                                        let package = root.join(path);
                                                        let resolved = if matches!(*category, "Controllers" | "Cameras") {
                                                            crate::project::asset_registry::package_aeo_files(&package)
                                                                .into_iter().next().unwrap_or(package)
                                                        } else { package };
                                                        if *category == "Effects" {
                                                            if let Some(effect_id) = crate::project::fx_assets::id_from_path(root, &resolved) {
                                                                response.dnd_set_drag_payload(super::shell::FxContentDragPayload {
                                                                    asset_id: effect_id, project_root: root.to_path_buf(),
                                                                });
                                                            }
                                                            if response.double_clicked() { pending_effect = Some(resolved); }
                                                        } else {
                                                            response.dnd_set_drag_payload(ProjectAssetDragPayload {
                                                                project_root: root.to_owned(), path: resolved.clone(),
                                                                category: category.to_string(),
                                                            });
                                                            if response.clicked() { pending_file = Some(resolved); }
                                                        }
                                                    }
                                                    if response.clicked() {
                                                        selected_id = Some(id.clone());
                                                    }
                                                    if response.double_clicked() {
                                                        inspect_id = Some(id.clone());
                                                    }
                                                }
                                                BrowserGridItem::File { path, .. } => {
                                                    if self.shell.asset_folder != "Effects" {
                                                        response.dnd_set_drag_payload(ProjectAssetDragPayload {
                                                            project_root: root.to_owned(),
                                                            path: path.clone(),
                                                            category: self.shell.asset_folder.clone(),
                                                        });
                                                    }
                                                    if self.shell.asset_folder == "Effects" {
                                                        if let Some(asset_id) = crate::project::fx_assets::id_from_path(root, path) {
                                                            response.dnd_set_drag_payload(super::shell::FxContentDragPayload {
                                                                asset_id,
                                                                project_root: root.to_path_buf(),
                                                            });
                                                        }
                                                    }
                                                    if self.shell.asset_folder == "Effects"
                                                        && path.extension().and_then(|s|s.to_str())
                                                            .is_some_and(|s|s.eq_ignore_ascii_case("efkefc"))
                                                        && response.double_clicked()
                                                    {
                                                        pending_effect = Some(path.clone());
                                                    } else if response.clicked()
                                                        && self.shell.asset_folder != "Effects"
                                                    {
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

        ui.ctx()
            .data_mut(|data| data.insert_temp(selection_key, selected_id.clone()));
        if let Some(id) = inspect_id.or(selected_id) {
            let registry = project_asset_snapshot(root);
            if let Some(record) = registry.find(&id) {
                egui::CollapsingHeader::new(format!("Selected: {}", record.name))
                    .id_salt(("asset_detail", &record.id))
                    .default_open(true)
                    .show(ui, |ui| {
                        draw_registered_asset_inspector(ui, record, &registry, root)
                    });
            }
        }

        if let Some(target) = action {
            self.browse_to(root, target);
        }
        if let Some(name) = chosen_actor {
            self.build_actor_package = Some(name);
            self.build_is_actor = true;
            self.queue_engine_sound(EngineSound::Select);
        }
        if let Some(name) = chosen_character {
            match crate::editor::character_editor::CharacterEditorState::open(root, &name) {
                Ok(editor) => {
                    self.asset_manager.character_editor = Some(editor);
                    self.asset_manager.character_editor_active = true;
                    self.asset_manager.open = true;
                }
                Err(error) => crate::engine::diagnostics::report("ERROR", "Characters", error),
            }
        }
        if create_effect {
            if self
                .effect_workspace
                .as_ref()
                .is_some_and(|existing| existing.dirty)
            {
                self.effect_issue = Some("Save the current Effect before creating another".into());
                self.effect_visible = true;
            } else {
                let mut created = None;
                for index in 0..1_000 {
                    let name = if index == 0 {
                        "Effect".to_owned()
                    } else {
                        format!("Effect {}", index + 1)
                    };
                    if !root
                        .join(".assets")
                        .join("effects")
                        .join(name.replace(' ', "_"))
                        .exists()
                    {
                        created = Some(crate::editor::effect_editor::EffectWorkspace::create(
                            root, &name,
                        ));
                        break;
                    }
                }
                match created {
                    Some(Ok(workspace)) => {
                        self.effect_workspace = Some(workspace);
                        self.effect_visible = true;
                        self.effect_issue = None;
                        self.shell.refresh(Some(root));
                    }
                    Some(Err(error)) => {
                        crate::engine::diagnostics::report("ERROR", "AeoFX", error.clone());
                        self.effect_issue = Some(error);
                    }
                    None => self.effect_issue = Some("Effect name limit reached".into()),
                }
            }
        }
        if let Some(path) = pending_effect {
            if self.effect_workspace.as_ref().is_some_and(|existing| {
                existing.dirty && existing.path.canonicalize().ok() != path.canonicalize().ok()
            }) {
                self.effect_issue = Some("Save the current Effect before opening another".into());
                self.effect_visible = true;
            } else if self.effect_workspace.as_ref().is_some_and(|existing| {
                existing.path.canonicalize().ok() == path.canonicalize().ok()
            }) {
                self.effect_visible = true;
                self.effect_issue = None;
            } else {
                match crate::editor::effect_editor::EffectWorkspace::open(root, &path) {
                    Ok(workspace) => {
                        self.effect_workspace = Some(workspace);
                        self.effect_visible = true;
                        self.effect_issue = None;
                    }
                    Err(error) => {
                        crate::engine::diagnostics::report("ERROR", "AeoFX", error.clone());
                        self.effect_issue = Some(error);
                    }
                }
            }
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
    fn compact_asset_drop_targets_stay_bounded_in_wide_and_narrow_panels() {
        for (available, expected_width) in [
            (72.0, 72.0),
            (180.0, 180.0),
            (300.0, 300.0),
            (640.0, 300.0),
            (1_920.0, 300.0),
        ] {
            let size = compact_asset_drop_size(available);
            assert_eq!(size.x, expected_width);
            assert_eq!(size.y, 44.0);
        }
    }

    #[test]
    fn wo_asm_002_audio_image_video_script_texture_source_types() {
        let root = std::env::temp_dir().join(format!(
            "woasm002-filetypes-{:032x}",
            rand::random::<u128>()
        ));
        for folder in [
            ".assets/audio",
            ".assets/media",
            ".assets/textures",
            "scripts",
            "playerscripts",
        ] {
            std::fs::create_dir_all(root.join(folder)).unwrap();
        }
        let candidates = [
            (
                ".assets/audio/music/theme.wav",
                "Audio",
                AssetDropKind::Audio,
                "music/theme.wav",
            ),
            (
                ".assets/media/images/poster.jpeg",
                "Media",
                AssetDropKind::Image,
                "Media/images/poster.jpeg",
            ),
            (
                ".assets/media/images/icon.png",
                "Images",
                AssetDropKind::Image,
                "Media/images/icon.png",
            ),
            (
                ".assets/media/intro.mp4",
                "Media",
                AssetDropKind::Video,
                "Media/intro.mp4",
            ),
            (
                ".assets/textures/wood.png",
                "Textures",
                AssetDropKind::Texture,
                "wood.png",
            ),
            (
                ".assets/textures/wood.png",
                "Textures",
                AssetDropKind::Image,
                "textures/wood.png",
            ),
            (
                "scripts/boot.aeo",
                "Scripts",
                AssetDropKind::Script,
                "scripts/boot.aeo",
            ),
            (
                "playerscripts/welcome.aeo",
                "Player Scripts",
                AssetDropKind::Script,
                "playerscripts/welcome.aeo",
            ),
        ];
        for (relative, category, kind, expected) in candidates {
            let file = root.join(relative);
            std::fs::create_dir_all(file.parent().unwrap()).unwrap();
            std::fs::write(&file, b"test fixture").unwrap();
            let payload = ProjectAssetDragPayload {
                project_root: root.clone(),
                path: file,
                category: category.into(),
            };
            assert_eq!(
                validated_asset_assignment(&root, &payload, kind).as_deref(),
                Some(expected),
                "{relative} expected {expected}"
            );
            for wrong_kind in [
                AssetDropKind::SurfaceMaterial,
                AssetDropKind::Audio,
                AssetDropKind::Texture,
                AssetDropKind::Video,
                AssetDropKind::Script,
            ] {
                let compatible_texture_image = category == "Textures"
                    && matches!(kind, AssetDropKind::Image | AssetDropKind::Texture)
                    && matches!(wrong_kind, AssetDropKind::Image | AssetDropKind::Texture);
                if wrong_kind != kind && !compatible_texture_image {
                    assert!(
                        validated_asset_assignment(&root, &payload, wrong_kind).is_none(),
                        "{relative}: wrong asset kind {wrong_kind:?} accepted"
                    );
                }
            }
            let mut forged = payload.clone();
            forged.project_root = root.join("not-this-project");
            assert!(validated_asset_assignment(&root, &forged, kind).is_none());
        }
        let mut wrong_extension = ProjectAssetDragPayload {
            project_root: root.clone(),
            path: root.join(".assets/audio/phony.ogg"),
            category: "Audio".into(),
        };
        std::fs::write(&wrong_extension.path, b"fixture").unwrap();
        assert_eq!(
            validated_asset_assignment(&root, &wrong_extension, AssetDropKind::Audio),
            None
        );
        wrong_extension.path = root.join(".assets/media/intro.mp4");
        wrong_extension.category = "Media".into();
        assert!(
            validated_asset_assignment(&root, &wrong_extension, AssetDropKind::Image).is_none()
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wo_asm_002_water_and_sky_materials_cannot_fill_surface_slots() {
        use crate::project::material::{MaterialDefinition, MaterialDocument, MaterialOutput};
        let root =
            std::env::temp_dir().join(format!("woasm002-mattypes-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let materials = [
            (
                "surface",
                MaterialOutput::SurfacePbr {
                    base_color: [0.3, 0.4, 0.5, 1.0],
                    roughness: 0.6,
                    metallic: 0.1,
                },
                AssetDropKind::SurfaceMaterial,
            ),
            (
                "water",
                MaterialOutput::Water(Default::default()),
                AssetDropKind::WaterMaterial,
            ),
            (
                "sky",
                MaterialOutput::Sky(crate::project::sky_material::default_sky().clone()),
                AssetDropKind::SkyMaterial,
            ),
        ];
        for (name, output, correct_kind) in materials {
            let document = MaterialDocument::new(MaterialDefinition::Asset { material: output });
            crate::project::material::save(&root, name, &document, None).unwrap();
            let payload = ProjectAssetDragPayload {
                project_root: root.clone(),
                path: crate::project::material::file_path(&root, name).unwrap(),
                category: "Materials".into(),
            };
            assert_eq!(
                validated_asset_assignment(&root, &payload, correct_kind),
                Some(if correct_kind == AssetDropKind::SurfaceMaterial {
                    format!("Material:{}", document.id)
                } else {
                    document.id.clone()
                })
            );
            for kind in [
                AssetDropKind::SurfaceMaterial,
                AssetDropKind::SkyMaterial,
                AssetDropKind::WaterMaterial,
            ] {
                if kind != correct_kind {
                    assert_eq!(validated_asset_assignment(&root, &payload, kind), None);
                }
            }
        }
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn wo_asm_002_surface_drop_validates_kind_and_project_and_persists_cell() {
        use crate::project::material::{MaterialDefinition, MaterialDocument, MaterialOutput};
        let root =
            std::env::temp_dir().join(format!("wo_asm_002_drag_{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(root.join(".assets/materials")).unwrap();
        let surface = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.4, 0.5, 0.6, 1.0],
                roughness: 0.8,
                metallic: 0.0,
            },
        });
        crate::project::material::save(&root, "sample_surface", &surface, None).unwrap();
        let payload = ProjectAssetDragPayload {
            project_root: root.clone(),
            path: crate::project::material::file_path(&root, "sample_surface").unwrap(),
            category: "Materials".into(),
        };
        let id = validated_asset_assignment(&root, &payload, AssetDropKind::SurfaceMaterial);
        assert_eq!(id, Some(format!("Material:{}", surface.id)));
        assert_eq!(
            validated_asset_assignment(&root, &payload, AssetDropKind::Image),
            None
        );
        assert_eq!(
            validated_asset_assignment(
                &root.join("other_project"),
                &payload,
                AssetDropKind::SurfaceMaterial
            ),
            None
        );
        let mut bad_type = payload.clone();
        bad_type.category = "Audio".into();
        assert_eq!(
            validated_asset_assignment(&root, &bad_type, AssetDropKind::SurfaceMaterial),
            None
        );
        let mut blocked = payload.clone();
        blocked.path = root.join(".assets/materials/nonexistent.json");
        assert_eq!(
            validated_asset_assignment(&root, &blocked, AssetDropKind::SurfaceMaterial),
            None
        );
        let non_surface = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(crate::world::terrain::TerrainWaterSettings::default()),
        });
        crate::project::material::save(&root, "water_material", &non_surface, None).unwrap();
        blocked.path = crate::project::material::file_path(&root, "water_material").unwrap();
        assert_eq!(
            validated_asset_assignment(&root, &blocked, AssetDropKind::SurfaceMaterial),
            None
        );

        let mut cell = crate::world::Cell::new_block();
        cell.texture = id.unwrap();
        let stored = crate::world::storage::StoredCell::from_cell(
            crate::world::WorldCoord::new(2, 3, 4),
            &cell,
        );
        assert_eq!(stored.texture, cell.texture);
        let (_, reopened) = stored.into_cell(crate::world::ChunkCoord::new(0, 0, 0));
        assert_eq!(reopened.texture, cell.texture);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn stable_name_based_virtual_actor_folders() {
        let mut shell = ShellState::default();
        assert_eq!(actor_family(&shell, "boat-row-small"), "Boat");
        assert_eq!(actor_family(&shell, "decor_tree_roots"), "Decor");
        assert_eq!(actor_family(&shell, ""), "Misc");
        shell
            .actor_display_names
            .insert("kenney-pirate-kit_boat".into(), "Boat Row Small".into());
        assert_eq!(actor_family(&shell, "kenney-pirate-kit_boat"), "Boat");
    }
    #[test]
    fn fixed_categories_are_visible_and_counted_from_registry() {
        assert_eq!(
            CONTENT_CATEGORIES,
            [
                "Actors",
                "Characters",
                "Controllers",
                "Cameras",
                "Textures",
                "Materials",
                "Images",
                "Video",
                "Audio",
                "Effects",
                "Scripts",
            ]
        );
        let mut registry = AssetRegistry::default();
        for (id, kind) in [
            ("captain", crate::project::asset_registry::AssetType::Actor),
            ("sailor", crate::project::asset_registry::AssetType::Actor),
            ("rowboat", crate::project::asset_registry::AssetType::Actor),
            ("hero", crate::project::asset_registry::AssetType::Character),
        ] {
            registry.assets.push(AssetRecord {
                id: id.into(),
                name: id.into(),
                kind,
                path: format!(".assets/entities/{id}"),
                dependencies: vec![],
                missing_dependencies: vec![],
                group: None,
            });
        }
        let mut shell = ShellState::default();
        let home = registered_browser_items(&shell, &registry);
        assert_eq!(home.len(), 11, "Empty canonical categories remain visible");
        let actors = home.iter().find(|v| v.name() == "Actors").unwrap();
        assert!(matches!(
            actors,
            BrowserGridItem::Folder { count: Some(3), .. }
        ));
        assert!(home.iter().any(|v| matches!(v, BrowserGridItem::Folder {
            name, count:Some(0),..} if name=="Cameras")));
        shell.asset_folder = "Actors".into();
        let actors = registered_browser_items(&shell, &registry);
        assert_eq!(actors.len(), 3);
        assert!(actors.iter().all(|asset| matches!(
            asset,
            BrowserGridItem::Registered {
                category: "Actors",
                ..
            }
        )));
        shell.asset_folder = "Characters".into();
        let characters = registered_browser_items(&shell, &registry);
        assert_eq!(characters.len(), 1);
        assert_eq!(characters[0].name(), "hero");
        shell.asset_folder = "Actors".into();
        shell.browser_search = "sail".into();
        assert_eq!(registered_browser_items(&shell, &registry).len(), 1);
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

#[cfg(test)]
mod water_material_content_tests {
    use super::*;
    #[test]
    fn water_material_content_categories_expose_real_water_choices_and_project_materials() {
        assert!(CONTENT_CATEGORIES.contains(&"Materials"));
        assert_eq!(
            physical_category_root(Path::new("project"), "Materials"),
            Path::new("project/.assets/materials")
        );
    }
}
