// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
#[path = "asset_import.rs"]
mod asset_import;
#[path = "asset_workspace.rs"]
mod asset_workspace;

pub(crate) fn prepare_asset_import(
    project: &Path,
    sources: &[PathBuf],
) -> Result<crate::project::asset_import_plan::ImportPlan, String> {
    crate::project::asset_import_plan::plan_import(project, sources)
}
pub(crate) fn apply_asset_import(
    project: &Path,
    plan: &crate::project::asset_import_plan::ImportPlan,
    progress: Option<&std::sync::atomic::AtomicUsize>,
) -> Result<crate::project::asset_import_plan::ImportExecutionResult, String> {
    asset_import::execute_import_plan_with_progress(project, plan, progress)
}

pub(crate) fn prepare_inventory_project_import(
    project: &Path,
    collection_id: &str,
) -> Result<crate::project::asset_import_plan::ImportPlan, String> {
    let sources = crate::project::shared_inventory::sources(collection_id)?;
    let plan = prepare_asset_import(project, &sources)?;
    crate::project::shared_inventory::validate_project_import(project, &plan)?;
    Ok(plan)
}

fn draw_import_conflict_review(
    ui: &mut egui::Ui,
    plan: &mut crate::project::asset_import_plan::ImportPlan,
    rename_inputs: &mut std::collections::BTreeMap<PathBuf, String>,
    rename_error: &mut Option<String>,
) {
    use crate::project::asset_import_plan::{
        incoming_identity, refresh_incoming_conflicts, rename_incoming,
    };
    enum Action {
        Skip(PathBuf),
        Replace(PathBuf),
        Rename(PathBuf, String),
    }
    let mut groups: std::collections::BTreeMap<String, Vec<(PathBuf, String)>> =
        std::collections::BTreeMap::new();
    for asset in &plan.assets {
        let label = if asset.kind == crate::project::asset_registry::AssetType::Actor {
            asset.name.clone()
        } else {
            asset.incoming_name.clone().unwrap_or_else(|| {
                asset
                    .source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
        };
        groups
            .entry(incoming_identity(asset))
            .or_default()
            .push((asset.source.clone(), label));
    }
    let conflicting: Vec<_> = groups
        .into_iter()
        .filter(|(_, entries)| entries.len() > 1)
        .collect();
    if conflicting.is_empty() {
        *rename_error = None;
        return;
    }
    let mut action = None;
    egui::CollapsingHeader::new(format!("Resolve incoming conflicts ({})", conflicting.len()))
        .id_salt("import_source_conflicts")
        .default_open(true)
        .show(ui, |ui| {
            for (identity, entries) in conflicting {
                ui.strong(format!("{identity} — {} incoming files", entries.len()));
                for (source, default_name) in entries {
                    ui.push_id(&source, |ui| {
                        ui.small(source.display().to_string());
                        let edited = rename_inputs.entry(source.clone())
                            .or_insert_with(|| default_name.clone());
                        ui.horizontal_wrapped(|ui| {
                            if ui.button("Skip").clicked() {
                                action = Some(Action::Skip(source.clone()));
                            }
                            if ui.button("Replace").on_hover_text(
                                "Use this incoming file for this name; skip the other incoming copies"
                            ).clicked() {
                                action = Some(Action::Replace(source.clone()));
                            }
                            ui.add(egui::TextEdit::singleline(edited).desired_width(170.0));
                            if ui.button("Rename").clicked() {
                                action = Some(Action::Rename(source.clone(), edited.clone()));
                            }
                        });
                    });
                }
                ui.separator();
            }
            if let Some(error) = rename_error.as_deref() {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    match action {
        Some(Action::Skip(source)) => {
            plan.assets.retain(|a| a.source != source);
            rename_inputs.remove(&source);
            *rename_error = None;
            refresh_incoming_conflicts(plan);
        }
        Some(Action::Replace(source)) => {
            match crate::project::asset_import_plan::keep_incoming_winner(plan, &source) {
                Ok(()) => *rename_error = None,
                Err(error) => *rename_error = Some(error),
            }
        }
        Some(Action::Rename(source, new_name)) => {
            if let Some(asset) = plan.assets.iter_mut().find(|a| a.source == source) {
                match rename_incoming(asset, &new_name) {
                    Ok(()) => {
                        *rename_error = None;
                        refresh_incoming_conflicts(plan);
                    }
                    Err(error) => *rename_error = Some(error),
                }
            }
        }
        None => {}
    }
}

pub(crate) fn create_texture_asset(project: &Path, image: &Path) -> Result<PathBuf, String> {
    let asset = BrowserAsset {
        category: "Images",
        name: image
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: image.to_owned(),
        bytes: std::fs::metadata(image).map_err(|e| e.to_string())?.len(),
        is_directory: false,
    };
    asset_workspace::create_texture_from_image(project, &asset)
}
#[path = "asset_lifecycle.rs"]
mod asset_lifecycle;
#[path = "material_editor.rs"]
pub(crate) mod material_editor;
use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::editor::editor::{Editor, EditorTool, History};
use crate::renderer::Renderer;
use crate::world::{Cell, World, WorldCoord};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ManagedAssetKind {
    #[default]
    Texture,
    Audio,
    Material,
    Path,
}

impl ManagedAssetKind {
    pub const ALL: [Self; 3] = [Self::Texture, Self::Audio, Self::Material];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Texture => "Texture",
            Self::Audio => "Audio",
            Self::Material => "Material",
            Self::Path => "Asset",
        }
    }

    fn category_dir(self) -> &'static str {
        match self {
            Self::Texture => "textures",
            Self::Audio => "audio",
            Self::Material => "materials",
            Self::Path => "",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Texture => "png",
            Self::Audio => "wav",
            Self::Material => "json",
            Self::Path => "",
        }
    }

    fn accepts_extension(self, extension: &str) -> bool {
        match self {
            Self::Texture => extension.eq_ignore_ascii_case("png"),
            Self::Audio => {
                extension.eq_ignore_ascii_case("wav") || extension.eq_ignore_ascii_case("mp3")
            }
            Self::Material => extension.eq_ignore_ascii_case("json"),
            Self::Path => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ManagedAsset {
    pub kind: ManagedAssetKind,
    pub identifier: String,
    pub file_path: PathBuf,
    pub size_bytes: u64,
    pub overrides_builtin: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct AssetUsage {
    pub description: String,
}

#[derive(Clone, Debug)]
struct PendingGlbImport {
    source: PathBuf,
    summary: crate::project::GlbAssetSummary,
    group_name: String,
    collision: bool,
}

#[derive(Clone, Debug)]
struct PendingFolderImport {
    source: PathBuf,
    group_name: String,
    collision: bool,
}

#[derive(Clone, Copy)]
enum MediaCollisionAction {
    Replace,
    Rename,
    Cancel,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GlbCollisionAction {
    Replace,
    Rename,
    Cancel,
}

#[derive(Clone)]
struct BrowserPreview(egui::TextureHandle);

impl std::fmt::Debug for BrowserPreview {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("BrowserPreview")
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum BrowserSort {
    #[default]
    Name,
    Category,
    Size,
}

#[derive(Clone, Debug)]
struct BrowserAsset {
    category: &'static str,
    name: String,
    path: PathBuf,
    bytes: u64,
    is_directory: bool,
}

const BROWSER_CATEGORIES: [(&str, &str); 12] = [
    ("Effects", "effects"),
    ("Textures", "textures"),
    ("Materials", "materials"),
    ("Images", "media"),
    ("Audio", "audio"),
    ("Video", "media"),
    ("Other Media", "media"),
    ("Actors", "entities"),
    ("Characters", "characters"),
    ("Controllers", "controllers"),
    ("Cameras", "cameras"),
    ("Scripts", "scripts"),
];

fn collect_browser_files(
    root: &Path,
    dir: &Path,
    category: &'static str,
    out: &mut Vec<BrowserAsset>,
) {
    if !crate::project::fs_walk::safe_kind(dir).is_ok_and(|kind| kind == Some(true)) {
        return;
    }
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let Ok(Some(is_directory)) = crate::project::fs_walk::safe_kind(&path) else {
            continue;
        };
        if entry.file_name().to_string_lossy().starts_with('.') && is_directory {
            continue;
        }
        if is_directory {
            if let Ok(relative) = path.strip_prefix(root) {
                let categories: &[&str] = if category == "Video" {
                    &["Video", "Images", "Other Media"]
                } else {
                    std::slice::from_ref(&category)
                };
                for &folder_category in categories {
                    out.push(BrowserAsset {
                        category: folder_category,
                        name: relative.to_string_lossy().replace('\\', "/"),
                        path: path.clone(),
                        bytes: 0,
                        is_directory: true,
                    });
                }
            }
            collect_browser_files(root, &path, category, out);
        } else {
            let filename = entry.file_name().to_string_lossy().to_ascii_lowercase();
            if matches!(
                filename.as_str(),
                "package.json"
                    | "import.json"
                    | "installed-pack.json"
                    | "asset-packs.json"
                    | "readme.txt"
                    | "license.txt"
                    | "attribution.txt"
                    | "readme.md"
                    | "license.md"
            ) {
                continue;
            }
            let ext = path
                .extension()
                .and_then(|ext| ext.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            let visible = match category {
                "Controllers" | "Cameras" | "Scripts" => {
                    ext == "aeo" || (category == "Scripts" && ext == "txt")
                }
                "Materials" => ext == "json" || ext == "mat",
                "Actors" | "Characters" => ext == "glb",
                "Effects" => matches!(ext.as_str(), "efkefc" | "efkpkg" | "efkmat"),
                "Textures" => matches!(ext.as_str(), "png" | "jpg" | "jpeg"),
                "Audio" => matches!(ext.as_str(), "wav" | "mp3" | "ogg"),
                "Video" => matches!(ext.as_str(), "mp4" | "png" | "jpg" | "jpeg"),
                _ => !matches!(ext.as_str(), "json" | "txt" | "md" | "license" | "import"),
            };
            if !visible {
                continue;
            }
            if matches!(category, "Actors" | "Characters")
                && path
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case("source.glb"))
                && path
                    .parent()
                    .is_some_and(|dir| dir.join("package.json").is_file())
            {
                continue;
            }
            let Ok(relative) = path.strip_prefix(root) else {
                continue;
            };
            let category = if category == "Video" {
                match path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase()
                    .as_str()
                {
                    "mp4" => "Video",
                    "png" | "jpg" | "jpeg" => "Images",
                    _ => "Other Media",
                }
            } else {
                category
            };
            out.push(BrowserAsset {
                category,
                name: relative.to_string_lossy().replace('\\', "/"),
                bytes: entry.metadata().map(|m| m.len()).unwrap_or(0),
                is_directory: false,
                path,
            });
        }
    }
}

fn actor_import_groups_path(project: &Path) -> PathBuf {
    project.join(".assets").join("actor_import_groups.json")
}

fn read_actor_import_groups(
    project: &Path,
) -> Result<std::collections::BTreeMap<String, String>, String> {
    let path = actor_import_groups_path(project);
    if !path.exists() {
        return Ok(Default::default());
    }
    let bytes = fs::read(&path).map_err(|e| format!("Cannot read Actor groups: {e}"))?;
    serde_json::from_slice(&bytes)
        .map_err(|e| format!("Invalid Actor grouping metadata (original retained): {e}"))
}

pub(super) fn with_actor_groups_removed<T>(
    project: &Path,
    names: &[String],
    mutation: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    let mut groups = read_actor_import_groups(project)?;
    let mut changed = false;
    for name in names {
        changed |= groups.remove(name).is_some();
    }
    if !changed {
        return mutation();
    }
    let path = actor_import_groups_path(project);
    let previous = fs::read(&path).map_err(|e| e.to_string())?;
    let updated = serde_json::to_vec_pretty(&groups).map_err(|e| e.to_string())?;
    crate::project::atomic_file::write_atomically(&path, &updated)
        .map_err(|e| format!("Cannot update Actor groups before deletion: {e}"))?;
    match mutation() {
        Ok(value) => Ok(value),
        Err(error) => {
            crate::project::atomic_file::write_atomically(&path, &previous).map_err(|restore| {
                format!("Asset deletion failed: {error}; Actor group restoration failed: {restore}")
            })?;
            Err(error)
        }
    }
}

fn record_actor_import_group(
    project: &Path,
    source_name: &str,
    imported_packages: &[String],
) -> Result<(), String> {
    if imported_packages.is_empty() {
        return Ok(());
    }
    let source_path = Path::new(source_name);
    let name = source_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or(source_name);
    record_actor_import_group_named(project, name, imported_packages)
}

fn valid_actor_group_name(name: &str) -> bool {
    let name = name.trim();
    !name.is_empty()
        && name.chars().count() <= 80
        && !name
            .chars()
            .any(|c| c.is_control() || "/\\:*?\"<>|".contains(c))
}

fn record_actor_import_group_named(
    project: &Path,
    group_name: &str,
    imported_packages: &[String],
) -> Result<(), String> {
    if imported_packages.is_empty() {
        return Ok(());
    }
    if !valid_actor_group_name(group_name) {
        return Err("Package name must be 1-80 characters, without path separators or reserved filename characters.".into());
    }
    let group = if group_name.trim().eq_ignore_ascii_case("Legacy") {
        "Legacy (Imported)"
    } else {
        group_name.trim()
    };
    let mut groups = read_actor_import_groups(project)?;
    for package in imported_packages {
        groups.insert(package.clone(), group.to_owned());
    }
    let file = actor_import_groups_path(project);
    fs::create_dir_all(file.parent().ok_or("Invalid project assets root")?)
        .map_err(|e| format!("Cannot create import metadata directory: {e}"))?;
    let data = serde_json::to_vec_pretty(&groups)
        .map_err(|e| format!("Cannot serialize import groups: {e}"))?;
    crate::project::atomic_file::write_atomically(&file, &data)
        .map_err(|e| format!("Cannot save Actor import groups: {e}"))
}

fn delete_visual_import_group(
    project: &Path,
    category: &str,
    group: &str,
) -> Result<usize, String> {
    let registry = crate::project::asset_registry::AssetRegistry::load(project)?;
    let groups = read_actor_import_groups(project)?;
    let kind = match category {
        "Actors" => crate::project::asset_registry::AssetType::Actor,
        "Characters" => crate::project::asset_registry::AssetType::Character,
        _ => return Err("Only imported Actors and Characters have collection groups".into()),
    };
    let records = registry
        .assets
        .iter()
        .filter(|record| {
            record.kind == kind
                && groups
                    .get(&record.name)
                    .map(String::as_str)
                    .unwrap_or("Legacy")
                    == group
        })
        .collect::<Vec<_>>();
    if records.is_empty() {
        return Err(format!(
            "The imported group '{group}' has no registered packages"
        ));
    }
    let ids = records
        .iter()
        .map(|record| record.id.clone())
        .collect::<Vec<_>>();
    let names = records
        .iter()
        .map(|record| record.name.clone())
        .collect::<Vec<_>>();
    with_actor_groups_removed(project, &names, || {
        crate::project::asset_registry::AssetRegistry::delete_many(project, &ids)
    })
}

fn rename_visual_import_group(
    project: &Path,
    previous: &str,
    desired: &str,
    package_names: &[String],
) -> Result<(), String> {
    if !valid_actor_group_name(desired) {
        return Err("Folder name must be 1-80 characters without reserved characters".into());
    }
    let mut groups = read_actor_import_groups(project)?;
    for package in package_names {
        groups.insert(package.clone(), desired.trim().to_owned());
    }
    let file = actor_import_groups_path(project);
    fs::create_dir_all(file.parent().ok_or("Invalid metadata path")?)
        .map_err(|error| error.to_string())?;
    let data = serde_json::to_vec_pretty(&groups).map_err(|error| error.to_string())?;
    crate::project::atomic_file::write_atomically(&file, &data)
        .map_err(|error| format!("Cannot rename folder: {error}"))
}

fn scan_browser_files(project: &Path) -> Vec<BrowserAsset> {
    let Ok(registry) = crate::project::asset_registry::AssetRegistry::load(project) else {
        return Vec::new();
    };
    registry
        .assets
        .iter()
        .filter(|record| !record.is_actor_owned_texture())
        .flat_map(|record| {
            let package = project.join(&record.path);
            if matches!(
                record.kind,
                crate::project::asset_registry::AssetType::Controller
                    | crate::project::asset_registry::AssetType::Camera
            ) {
                return crate::project::asset_registry::package_aeo_files(&package)
                    .into_iter()
                    .map(|path| BrowserAsset {
                        category: record.kind.category(),
                        name: path
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                        bytes: fs::metadata(&path).map(|m| m.len()).unwrap_or(0),
                        is_directory: false,
                        path,
                    })
                    .collect::<Vec<_>>();
            }
            let is_directory = package.is_dir()
                || matches!(
                    record.kind,
                    crate::project::asset_registry::AssetType::Actor
                        | crate::project::asset_registry::AssetType::Character
                        | crate::project::asset_registry::AssetType::Animation
                );
            vec![BrowserAsset {
                category: record.kind.category(),
                name: if record.kind == crate::project::asset_registry::AssetType::Script {
                    package
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned()
                } else {
                    record.name.clone()
                },
                bytes: if package.is_file() {
                    fs::metadata(&package).map(|m| m.len()).unwrap_or(0)
                } else {
                    0
                },
                is_directory,
                path: package,
            }]
        })
        .collect()
}
fn scan_browser_files_with_pack_roots(project: &Path, pack_roots: &[PathBuf]) -> Vec<BrowserAsset> {
    let mut result = Vec::new();
    for (category, folder) in BROWSER_CATEGORIES {
        if category == "Images" || category == "Other Media" {
            continue;
        }
        let root = project.join(".assets").join(folder);
        if root.is_dir() {
            collect_browser_files(&root, &root, category, &mut result);
        }
    }
    let root = project.join("scripts");
    if root.is_dir() {
        collect_browser_files(&root, &root, "Scripts", &mut result);
    }
    for base in pack_roots {
        let Ok(bytes) = fs::read(base.join("installed-pack.json")) else {
            continue;
        };
        let Ok(pack) = serde_json::from_slice::<crate::project::asset_packs::Pack>(&bytes) else {
            continue;
        };
        for (category, folder) in BROWSER_CATEGORIES {
            if category == "Images" || category == "Other Media" {
                continue;
            }
            let category_root = base.join(".assets").join(folder);
            if !category_root.is_dir() {
                continue;
            }
            let group = pack.name.clone();
            result.push(BrowserAsset {
                category,
                name: group.clone(),
                path: category_root.clone(),
                bytes: 0,
                is_directory: true,
            });
            let mut files = Vec::new();
            collect_browser_files(&category_root, &category_root, category, &mut files);
            for mut file in files {
                file.name = format!("{group}/{}", file.name);
                result.push(file);
            }
        }
    }
    result.sort_by(|a, b| a.category.cmp(b.category).then(a.name.cmp(&b.name)));
    result
}

fn browser_visible_indices(
    assets: &[BrowserAsset],
    category: &str,
    folder: &str,
    search: &str,
) -> Vec<usize> {
    let query = search.trim().to_ascii_lowercase();
    assets
        .iter()
        .enumerate()
        .filter(|(_, asset)| {
            if asset.is_directory
                && !(matches!(
                    asset.category,
                    "Actors" | "Characters" | "Controllers" | "Cameras"
                ) && !asset.name.contains('/'))
            {
                return false;
            }
            if !category.is_empty() && asset.category != category {
                return false;
            }
            if !query.is_empty() {
                return asset.name.to_ascii_lowercase().contains(&query);
            }
            asset
                .name
                .rsplit_once('/')
                .map(|(parent, _)| parent)
                .unwrap_or("")
                == folder
        })
        .map(|(index, _)| index)
        .collect()
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MaterialSaveKind {
    Surface,
    Sky,
}
#[derive(Clone, Debug)]
struct PendingMaterialSave {
    kind: MaterialSaveKind,
    request: crate::project::material_draft::MaterialSaveRequest,
    result: std::sync::Arc<
        std::sync::Mutex<Option<Result<crate::project::material::MaterialCatalog, String>>>,
    >,
}

fn start_material_save(
    kind: MaterialSaveKind,
    request: crate::project::material_draft::MaterialSaveRequest,
) -> PendingMaterialSave {
    let result = std::sync::Arc::new(std::sync::Mutex::new(None));
    let target = std::sync::Arc::clone(&result);
    let worker_request = request.clone();
    std::thread::spawn(move || {
        let outcome =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| worker_request.run()))
                .unwrap_or_else(|_| Err("Material save worker failed".into()));
        if let Ok(mut slot) = target.lock() {
            *slot = Some(outcome);
        }
    });
    PendingMaterialSave {
        kind,
        request,
        result,
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum InventorySelection {
    Imported(String),
    Pack(crate::project::asset_packs::Reference),
}

#[derive(Clone, Debug, Default)]
pub struct AssetManagerState {
    pub open: bool,
    pub(crate) inventory_open: bool,
    pub(crate) inventory_snapshot: Option<(
        Vec<crate::project::shared_inventory::Item>,
        Vec<crate::project::shared_inventory::CachedPack>,
    )>,
    pub(crate) inventory_error: Option<String>,
    inventory_selection: Option<InventorySelection>,
    inventory_pending_pack: Option<crate::project::asset_packs::Reference>,
    inventory_pack_project: Option<PathBuf>,
    inventory_pending_collection: Option<String>,
    inventory_project_import_running: Option<String>,
    pub search: String,
    pub(crate) browser_category: String,
    pub(crate) browser_folder: String,
    browser_group_filter: String,
    browser_group_category: String,
    browser_record_groups: std::collections::HashMap<PathBuf, Option<String>>,
    browser_grid: bool,
    browser_view_initialized: bool,
    browser_details_collapsed: bool,
    browser_folder_creator_open: bool,
    browser_sort: BrowserSort,
    new_folder_name: String,
    pub kind: ManagedAssetKind,
    pub rename_asset: Option<(ManagedAssetKind, String)>,
    pub rename_target: String,
    pub delete_asset: Option<(ManagedAssetKind, String)>,
    delete_import_group: Option<(String, String)>,
    pub status: Option<(String, bool)>,
    pub(crate) sky_material_preview: Option<material_editor::SkyMaterialPreview>,
    pub(crate) sky_editor_active: bool,
    pub(crate) surface_material_editor: Option<material_editor::SurfaceMaterialEditor>,
    pub(crate) surface_editor_active: bool,
    pub(crate) character_editor: Option<crate::editor::character_editor::CharacterEditorState>,
    pub(crate) character_editor_active: bool,
    pub(crate) animation_editor: Option<crate::editor::animation_editor::AnimationEditorState>,
    pub(crate) animation_editor_active: bool,
    pub(crate) material_assignment_request: Option<(PathBuf, bool)>,
    pending_material_save: Option<PendingMaterialSave>,
    sky_material_choices: Vec<(String, String)>,
    sky_choices_project: Option<PathBuf>,
    sky_choices_catalog: Option<std::sync::Arc<crate::project::material::MaterialCatalog>>,

    catalog: Vec<ManagedAsset>,
    browser_files: Vec<BrowserAsset>,
    actor_package_indices: Vec<usize>,
    character_package_indices: Vec<usize>,
    browser_category_counts: std::collections::HashMap<&'static str, usize>,
    actor_import_groups: std::collections::BTreeMap<String, String>,
    folder_rename_source: Option<String>,
    folder_rename_draft: String,
    actor_import_groups_folded: std::collections::HashMap<String, String>,
    actor_pack_ids: std::collections::BTreeMap<String, String>,
    actor_display_names: std::collections::HashMap<PathBuf, String>,
    browser_selected: Option<PathBuf>,
    open_script_request: Option<PathBuf>,
    browser_preview: Option<(PathBuf, BrowserPreview)>,
    browser_preview_loaded: Option<PathBuf>,
    browser_thumbnails: std::collections::HashMap<PathBuf, BrowserPreview>,
    browser_thumbnail_order: std::collections::VecDeque<PathBuf>,
    browser_thumbnail_failed: std::collections::HashSet<PathBuf>,
    project_actor_packages: Vec<String>,
    project_character_packages: Vec<String>,
    catalog_project: Option<PathBuf>,
    catalog_loaded: bool,
    catalog_revision: u64,
    next_registry_poll: f64,
    pub(crate) was_open: bool,
    selected_asset: Option<(ManagedAssetKind, String)>,
    selected_usages: Vec<AssetUsage>,
    selected_usage_revision: Option<u64>,
    selected_usage_build_value: Option<String>,
    pending_glb_import: Option<PendingGlbImport>,
    pending_glb_queue: std::collections::VecDeque<PathBuf>,
    pending_import_folders: std::collections::VecDeque<PathBuf>,
    pending_folder_import: Option<PendingFolderImport>,
    pending_media_collisions: std::collections::VecDeque<PathBuf>,
    incoming_media_name: String,
    background_import: Option<
        std::sync::Arc<
            std::sync::Mutex<std::sync::mpsc::Receiver<(PathBuf, Result<PathBuf, String>)>>,
        >,
    >,
    queued_media: std::collections::VecDeque<(PathBuf, Option<String>, bool)>,
    glb_inspection: Option<
        std::sync::Arc<
            std::sync::Mutex<
                std::sync::mpsc::Receiver<(
                    PathBuf,
                    Result<crate::project::GlbAssetSummary, String>,
                )>,
            >,
        >,
    >,
    glb_job: Option<
        std::sync::Arc<
            std::sync::Mutex<
                std::sync::mpsc::Receiver<Result<crate::project::GlbFolderImportResult, String>>,
            >,
        >,
    >,
    glb_job_pending: Option<PendingGlbImport>,
    folder_job: Option<
        std::sync::Arc<
            std::sync::Mutex<
                std::sync::mpsc::Receiver<Result<crate::project::GlbFolderImportResult, String>>,
            >,
        >,
    >,
    folder_job_pending: Option<PendingFolderImport>,
    legacy_folder_job:
        Option<std::sync::Arc<std::sync::Mutex<std::sync::mpsc::Receiver<Result<String, String>>>>>,
    folder_media_job: Option<
        std::sync::Arc<
            std::sync::Mutex<std::sync::mpsc::Receiver<Result<(usize, Vec<String>), String>>>,
        >,
    >,
    import_progress_name: Option<String>,
    import_progress: f32,
    folder_progress: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    canonical_import_plan: Option<crate::project::asset_import_plan::ImportPlan>,
    canonical_import_rename: std::collections::BTreeMap<PathBuf, String>,
    canonical_import_rename_error: Option<String>,
    canonical_import_plan_project: Option<PathBuf>,
    inventory_import_sources: Option<Vec<PathBuf>>,
    canonical_import_project: Option<PathBuf>,
    canonical_import_job: Option<
        std::sync::Arc<
            std::sync::Mutex<
                std::sync::mpsc::Receiver<
                    Result<crate::project::asset_import_plan::ImportExecutionResult, String>,
                >,
            >,
        >,
    >,
    canonical_import_progress: Option<std::sync::Arc<std::sync::atomic::AtomicUsize>>,
    canonical_manage_id: Option<String>,
    canonical_manage_name: String,
    canonical_manage_group: String,
    canonical_delete_confirm: bool,
}

impl AssetManagerState {
    pub(crate) fn sky_catalog(
        &self,
        project: &Path,
    ) -> Option<std::sync::Arc<crate::project::material::MaterialCatalog>> {
        if self.sky_choices_project.as_deref() == Some(project) {
            self.sky_choices_catalog.clone()
        } else {
            None
        }
    }
    pub(crate) fn sky_materials(&mut self, project: &Path) -> Vec<(String, String)> {
        if self.sky_choices_project.as_deref() != Some(project) {
            let catalog = crate::project::material::MaterialCatalog::load(project);
            self.sky_material_choices = catalog
                .documents()
                .iter()
                .filter_map(|(name, doc)| {
                    if catalog.sky(&doc.id).is_some() {
                        Some((doc.id.clone(), name.clone()))
                    } else {
                        None
                    }
                })
                .collect();
            self.sky_choices_project = Some(project.to_owned());
            self.sky_choices_catalog = Some(std::sync::Arc::new(catalog));
        }
        self.sky_material_choices.clone()
    }
}

fn selected_material_id(project: &Path, path: &Path) -> Result<String, String> {
    let metadata = std::fs::symlink_metadata(path).map_err(|e| e.to_string())?;
    if !metadata.is_file() || metadata.file_type().is_symlink() || metadata.len() > 256 * 1024 {
        return Err("Selected Material is not a readable Material document".into());
    }
    let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
    let material: crate::project::material::MaterialDocument =
        serde_json::from_slice(&bytes).map_err(|e| format!("Invalid Material file: {e}"))?;
    crate::project::material::MaterialCatalog::load(project).resolve(&material.id)?;
    Ok(material.id)
}

fn builtin_root(kind: ManagedAssetKind) -> PathBuf {
    crate::project::engine_assets_dir().join(kind.category_dir())
}

fn project_root(project_path: &Path, kind: ManagedAssetKind) -> PathBuf {
    if kind == ManagedAssetKind::Path {
        return project_path.to_owned();
    }
    project_path.join(".assets").join(kind.category_dir())
}

fn normalize_identifier(kind: ManagedAssetKind, value: &str) -> Result<String, String> {
    if kind == ManagedAssetKind::Path {
        asset_lifecycle::validate_relative(value)?;
        return Ok(value.to_owned());
    }
    let mut normalized = value.trim().replace('\\', "/");
    while normalized.starts_with('/') {
        normalized.remove(0);
    }
    while normalized.ends_with('/') {
        normalized.pop();
    }

    if normalized.is_empty() {
        return Err("Asset identifier cannot be empty.".to_string());
    }

    let path = Path::new(&normalized);
    if path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        return Err("Asset destination must remain inside its project asset category.".to_string());
    }

    if path
        .components()
        .any(|component| matches!(component, Component::CurDir))
    {
        return Err("Asset destination cannot contain '.' path components.".to_string());
    }

    let expected_extension = kind.extension();
    match kind {
        ManagedAssetKind::Texture => {
            if normalized
                .to_ascii_lowercase()
                .ends_with(&format!(".{expected_extension}"))
            {
                let new_len = normalized.len() - expected_extension.len() - 1;
                normalized.truncate(new_len);
            }
        }
        ManagedAssetKind::Audio => {
            let ext = Path::new(&normalized)
                .extension()
                .and_then(|extension| extension.to_str());
            match ext {
                Some(extension) if kind.accepts_extension(extension) => {}
                None => {
                    normalized.push('.');
                    normalized.push_str(expected_extension);
                }
                Some(_) => return Err("Audio filename must end in .wav or .mp3.".into()),
            }
        }
        ManagedAssetKind::Material => return crate::project::material::identifier(&normalized),
        ManagedAssetKind::Path => unreachable!(),
    }

    if normalized.is_empty()
        || normalized
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err("Asset destination contains an invalid path component.".to_string());
    }

    Ok(normalized)
}

fn identifier_file_path(root: &Path, kind: ManagedAssetKind, identifier: &str) -> PathBuf {
    match kind {
        ManagedAssetKind::Texture => root.join(format!("{identifier}.png")),
        ManagedAssetKind::Audio => root.join(identifier),
        ManagedAssetKind::Material => root.join(format!("{identifier}.json")),
        ManagedAssetKind::Path => root.join(identifier),
    }
}

fn scan_category(
    current: &Path,
    root: &Path,
    kind: ManagedAssetKind,
    output: &mut Vec<ManagedAsset>,
) {
    let discovered = match crate::project::fs_walk::scan(current, 64, 250_000) {
        Ok(entries) => entries,
        Err(error) => {
            eprintln!("[ASSETS] Cannot scan {}: {error}", current.display());
            return;
        }
    };
    for (path, is_dir) in discovered {
        if is_dir
            || !path
                .extension()
                .and_then(|ext| ext.to_str())
                .is_some_and(|ext| kind.accepts_extension(ext))
        {
            continue;
        }
        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let mut identifier = relative.to_string_lossy().replace('\\', "/");
        if kind == ManagedAssetKind::Texture || kind == ManagedAssetKind::Material {
            if let Some(dot) = identifier.rfind('.') {
                identifier.truncate(dot);
            }
        }
        let size_bytes = fs::metadata(&path).map(|meta| meta.len()).unwrap_or(0);
        output.push(ManagedAsset {
            kind,
            identifier,
            file_path: path,
            size_bytes,
            overrides_builtin: false,
        });
    }
}

fn scan_project_assets_with_roots(
    project_path: &Path,
    texture_builtin: &Path,
    audio_builtin: &Path,
) -> Vec<ManagedAsset> {
    let mut assets = Vec::new();

    for (kind, builtin) in [
        (ManagedAssetKind::Texture, texture_builtin),
        (ManagedAssetKind::Audio, audio_builtin),
    ] {
        let project = project_root(project_path, kind);
        if project.is_dir() {
            scan_category(&project, &project, kind, &mut assets);
        }

        for asset in assets.iter_mut().filter(|asset| asset.kind == kind) {
            asset.overrides_builtin =
                identifier_file_path(builtin, kind, &asset.identifier).is_file();
        }
    }

    let materials = project_root(project_path, ManagedAssetKind::Material);
    scan_category(
        &materials,
        &materials,
        ManagedAssetKind::Material,
        &mut assets,
    );

    assets.sort_by(|left, right| {
        left.kind.cmp(&right.kind).then_with(|| {
            left.identifier
                .to_ascii_lowercase()
                .cmp(&right.identifier.to_ascii_lowercase())
        })
    });
    assets
}

pub(crate) fn scan_assets(project_path: &Path) -> Vec<ManagedAsset> {
    scan_project_assets_with_roots(
        project_path,
        &builtin_root(ManagedAssetKind::Texture),
        &builtin_root(ManagedAssetKind::Audio),
    )
}

fn scan_project_package_names(
    project_path: &Path,
    category: &str,
    available: &[String],
) -> Vec<String> {
    let directory = project_path.join(".assets").join(category);
    let mut packages = Vec::new();
    for path in crate::project::asset_packs::package_directories(&directory) {
        let name = path
            .strip_prefix(&directory)
            .unwrap_or(&path)
            .to_string_lossy()
            .replace('\\', "/");
        if let Some(resolved) = available
            .iter()
            .find(|candidate| candidate.eq_ignore_ascii_case(&name))
        {
            packages.push(resolved.clone());
        }
    }
    packages.sort_by_key(|name| name.to_ascii_lowercase());
    packages.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
    packages
}

pub(crate) fn model_display_name(package_dir: &Path, pack_id: Option<&str>) -> String {
    fn humanize(raw: &str) -> String {
        raw.replace(['-', '_'], " ")
            .split_whitespace()
            .map(|word| {
                if word.is_empty() {
                    return String::new();
                }
                let mut chars = word.chars();
                format!("{}{}", chars.next().unwrap().to_uppercase(), chars.as_str())
            })
            .collect::<Vec<_>>()
            .join(" ")
    }

    if let Ok(bytes) = fs::read(package_dir.join("import.json")) {
        if let Ok(source) = serde_json::from_slice::<serde_json::Value>(&bytes) {
            let file = source["source_file_name"]
                .as_str()
                .and_then(|value| Path::new(value).file_stem())
                .and_then(|value| value.to_str())
                .unwrap_or("");
            let mesh = source["source_mesh_name"].as_str().unwrap_or("");
            if !file.is_empty() {
                let base = humanize(file);
                if mesh.is_empty() || mesh.eq_ignore_ascii_case(file) {
                    return base;
                }
                return format!("{} - {}", base, humanize(mesh));
            }
        }
    }
    let raw = package_dir
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let suffix = pack_id
        .and_then(|id| raw.strip_prefix(id)?.strip_prefix('_'))
        .unwrap_or(raw);
    humanize(suffix)
}

fn managed_catalog_from_browser(
    project: &Path,
    files: &[BrowserAsset],
    texture_builtin: &Path,
    audio_builtin: &Path,
) -> Vec<ManagedAsset> {
    let mut assets = Vec::new();
    for file in files {
        if file.is_directory {
            continue;
        }
        let kind = match file.category {
            "Textures" => ManagedAssetKind::Texture,
            "Audio" => ManagedAssetKind::Audio,
            "Materials" => ManagedAssetKind::Material,
            _ => continue,
        };
        let root = project_root(project, kind);
        let Ok(relative) = file.path.strip_prefix(&root) else {
            continue;
        };
        let Some(ext) = file.path.extension().and_then(|e| e.to_str()) else {
            continue;
        };
        if !kind.accepts_extension(ext) {
            continue;
        }
        let mut identifier = relative.to_string_lossy().replace('\\', "/");
        if kind != ManagedAssetKind::Audio {
            identifier.truncate(identifier.len() - ext.len() - 1);
        }
        let overrides_builtin = match kind {
            ManagedAssetKind::Texture => {
                identifier_file_path(texture_builtin, kind, &identifier).is_file()
            }
            ManagedAssetKind::Audio => {
                identifier_file_path(audio_builtin, kind, &identifier).is_file()
            }
            _ => false,
        };
        assets.push(ManagedAsset {
            kind,
            identifier,
            file_path: file.path.clone(),
            size_bytes: file.bytes,
            overrides_builtin,
        });
    }
    assets.sort_by(|a, b| {
        a.kind.cmp(&b.kind).then_with(|| {
            a.identifier
                .to_lowercase()
                .cmp(&b.identifier.to_lowercase())
        })
    });
    assets
}

fn refresh_manager_catalog(state: &mut AssetManagerState, project_path: &Path) {
    state.sky_choices_project = None;
    if state.catalog_project.as_deref() != Some(project_path) {
        state.browser_selected = None;
        state.browser_folder.clear();
        state.canonical_import_plan = None;
        state.canonical_import_plan_project = None;
    }
    crate::project::material::MaterialCatalog::invalidate_cached(project_path);
    crate::editor::actor_model_preview::invalidate_project(project_path);
    if let Err(error) = crate::project::asset_registry::AssetRegistry::load(project_path) {
        crate::engine::diagnostics::report("ERROR", "Assets", error);
    }
    state.browser_files = scan_browser_files(project_path);
    state.browser_record_groups = crate::project::asset_registry::AssetRegistry::load(project_path)
        .map(|registry| {
            registry
                .assets
                .into_iter()
                .map(|record| (project_path.join(record.path), record.group))
                .collect()
        })
        .unwrap_or_default();
    state.actor_package_indices = state
        .browser_files
        .iter()
        .enumerate()
        .filter(|(_, asset)| asset.category == "Actors" && asset.is_directory)
        .map(|(index, _)| index)
        .collect();
    state.character_package_indices = state
        .browser_files
        .iter()
        .enumerate()
        .filter(|(_, asset)| asset.category == "Characters" && asset.is_directory)
        .map(|(index, _)| index)
        .collect();
    state.browser_category_counts = BROWSER_CATEGORIES
        .iter()
        .map(|(category, _)| {
            let count = match *category {
                "Actors" => state.actor_package_indices.len(),
                "Characters" => state.character_package_indices.len(),
                category => state
                    .browser_files
                    .iter()
                    .filter(|asset| asset.category == category && !asset.is_directory)
                    .count(),
            };
            (*category, count)
        })
        .collect();
    state.catalog = managed_catalog_from_browser(
        project_path,
        &state.browser_files,
        &builtin_root(ManagedAssetKind::Texture),
        &builtin_root(ManagedAssetKind::Audio),
    );
    state.actor_import_groups = read_actor_import_groups(project_path).unwrap_or_else(|error| {
        crate::engine::diagnostics::report("ERROR", "Assets", error);
        Default::default()
    });
    state.actor_pack_ids = Default::default();
    for asset in &state.browser_files {
        if asset.category != "Actors"
            || !asset.is_directory
            || !asset.path.join("package.json").is_file()
        {
            continue;
        }
        if let Some((group, _)) = asset.name.split_once('/') {
            if state.actor_pack_ids.contains_key(group) {
                state
                    .actor_import_groups
                    .insert(asset.name.clone(), group.to_owned());
            }
        }
    }
    state.actor_import_groups_folded = state
        .actor_import_groups
        .iter()
        .map(|(name, group)| (name.to_ascii_lowercase(), group.clone()))
        .collect();
    state.actor_display_names.clear();
    for asset in &state.browser_files {
        if asset.category != "Actors"
            || !asset.is_directory
            || !asset.path.join("package.json").is_file()
        {
            continue;
        }
        let group = state.actor_import_groups.get(&asset.name).or_else(|| {
            asset
                .name
                .split_once('/')
                .and_then(|(prefix, _)| state.actor_pack_ids.contains_key(prefix).then_some(prefix))
                .and_then(|prefix| state.actor_import_groups.get(prefix))
        });
        let pack_id = group
            .and_then(|name| state.actor_pack_ids.get(name))
            .or_else(|| {
                asset
                    .name
                    .split_once('/')
                    .and_then(|(prefix, _)| state.actor_pack_ids.get(prefix))
            });
        state
            .actor_display_names
            .insert(asset.path.clone(), asset.name.clone());
    }
    if state
        .browser_selected
        .as_ref()
        .is_some_and(|path| !state.browser_files.iter().any(|a| &a.path == path))
    {
        state.browser_selected = None;
    }
    state.browser_preview = None;
    state.browser_preview_loaded = None;
    state.browser_thumbnails.clear();
    state.browser_thumbnail_order.clear();
    state.browser_thumbnail_failed.clear();
    state.project_actor_packages = crate::project::discover_actor_packages(project_path);
    state.project_character_packages = crate::project::discover_characters(project_path);
    state.catalog_project = Some(project_path.to_path_buf());
    state.catalog_loaded = true;
    state.catalog_revision = crate::project::asset_registry::AssetRegistry::load(project_path)
        .map(|registry| registry.revision)
        .unwrap_or_default();

    if state
        .selected_asset
        .as_ref()
        .is_some_and(|(kind, identifier)| {
            !state
                .catalog
                .iter()
                .any(|asset| asset.kind == *kind && asset.identifier == *identifier)
        })
    {
        state.selected_asset = None;
    }

    state.selected_usages.clear();
    state.selected_usage_revision = None;
    state.selected_usage_build_value = None;
}

pub(crate) enum ThumbnailLoad {
    Ready(egui::TextureHandle),
    Pending,
    Failed,
}
type ThumbnailKey = (PathBuf, u64);
type DecodedThumbnail = (usize, usize, Vec<u8>);
struct ThumbnailWorker {
    queue: std::sync::mpsc::Sender<ThumbnailKey>,
    completed: std::sync::mpsc::Receiver<(ThumbnailKey, Option<DecodedThumbnail>)>,
    pending: std::collections::HashSet<ThumbnailKey>,
    cached: std::collections::HashMap<ThumbnailKey, Option<egui::TextureHandle>>,
    order: std::collections::VecDeque<ThumbnailKey>,
}
impl ThumbnailWorker {
    fn new(ctx: egui::Context) -> Self {
        let (queue, work) = std::sync::mpsc::channel::<ThumbnailKey>();
        let (done, completed) =
            std::sync::mpsc::channel::<(ThumbnailKey, Option<DecodedThumbnail>)>();
        std::thread::Builder::new()
            .name("worldkiln-asset-manager-thumbnails".into())
            .spawn(move || {
                while let Ok(key) = work.recv() {
                    let image = image::open(&key.0).ok().map(|image| {
                        let rgba = image.thumbnail(256, 256).to_rgba8();
                        (
                            rgba.width() as usize,
                            rgba.height() as usize,
                            rgba.into_raw(),
                        )
                    });
                    if done.send((key, image)).is_err() {
                        break;
                    }
                    ctx.request_repaint();
                }
            })
            .expect("asset thumbnail decoder thread");
        Self {
            queue,
            completed,
            pending: Default::default(),
            cached: Default::default(),
            order: Default::default(),
        }
    }
}
fn thumbnail_revision(path: &Path) -> Option<u64> {
    use std::time::UNIX_EPOCH;
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let stamp = meta.modified().ok()?.duration_since(UNIX_EPOCH).ok()?;
    Some((stamp.as_nanos() as u64).wrapping_add(meta.len().rotate_left(17)))
}
#[cfg(test)]
mod asynchronous_thumbnail_tests {
    use super::*;
    #[test]
    fn thumbnail_decode_is_queued_and_result_is_uploaded_on_ui_thread() {
        use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};
        let root = std::env::temp_dir().join(format!(
            "wk-thumbnail-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let image_path = root.join("sample.png");
        image::RgbaImage::from_pixel(16, 16, image::Rgba([80, 120, 160, 255]))
            .save(&image_path)
            .unwrap();
        let ctx = egui::Context::default();
        let start = Instant::now();
        assert!(matches!(
            load_browser_thumbnail(&ctx, &image_path),
            ThumbnailLoad::Pending
        ));
        assert!(
            start.elapsed() < Duration::from_millis(100),
            "UI must not decode even a small image during a repaint"
        );
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            match load_browser_thumbnail(&ctx, &image_path) {
                ThumbnailLoad::Ready(handle) => {
                    assert_eq!(handle.size(), [256, 256]);
                    break;
                }
                ThumbnailLoad::Failed => panic!("valid image failed to load"),
                ThumbnailLoad::Pending if Instant::now() < deadline => {
                    std::thread::sleep(Duration::from_millis(10));
                }
                ThumbnailLoad::Pending => panic!("thumbnail worker timed out"),
            }
        }
        std::fs::remove_dir_all(&root).unwrap();
    }
}

pub(crate) fn load_browser_thumbnail(ctx: &egui::Context, path: &Path) -> ThumbnailLoad {
    let Some(extension) = path.extension().and_then(|ext| ext.to_str()) else {
        return ThumbnailLoad::Failed;
    };
    if !matches!(
        extension.to_ascii_lowercase().as_str(),
        "png" | "jpg" | "jpeg"
    ) {
        return ThumbnailLoad::Failed;
    }
    let Some(revision) = thumbnail_revision(path) else {
        return ThumbnailLoad::Failed;
    };
    let key = (path.to_path_buf(), revision);
    static WORKER: std::sync::OnceLock<std::sync::Mutex<ThumbnailWorker>> =
        std::sync::OnceLock::new();
    let worker = WORKER.get_or_init(|| std::sync::Mutex::new(ThumbnailWorker::new(ctx.clone())));
    let mut worker = worker.lock().unwrap_or_else(|err| err.into_inner());
    for _ in 0..4 {
        let Ok((completed, decoded)) = worker.completed.try_recv() else {
            break;
        };
        worker.pending.remove(&completed);
        let preview = decoded.map(|(width, height, pixels)| {
            ctx.load_texture(
                format!(
                    "asset-manager-preview:{}:{}",
                    completed.0.display(),
                    completed.1
                ),
                egui::ColorImage::from_rgba_unmultiplied([width, height], &pixels),
                egui::TextureOptions::LINEAR,
            )
        });
        if worker.cached.len() >= 128 {
            if let Some(old) = worker.order.pop_front() {
                worker.cached.remove(&old);
            }
        }
        worker.order.push_back(completed.clone());
        worker.cached.insert(completed, preview);
    }
    if let Some(result) = worker.cached.get(&key) {
        return match result {
            Some(texture) => ThumbnailLoad::Ready(texture.clone()),
            None => ThumbnailLoad::Failed,
        };
    }
    if worker.pending.len() < 12 && worker.pending.insert(key.clone()) {
        if worker.queue.send(key.clone()).is_err() {
            worker.pending.remove(&key);
            return ThumbnailLoad::Failed;
        }
    }
    ThumbnailLoad::Pending
}

fn format_asset_size(bytes: u64) -> String {
    if bytes >= 1024 * 1024 {
        format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
    } else if bytes >= 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{bytes} B")
    }
}

fn texture_reference_matches(value: &str, identifier: &str) -> bool {
    value == identifier || value.eq_ignore_ascii_case(&format!("{identifier}.png"))
}

fn rewritten_texture_reference(value: &str, old: &str, new: &str) -> Option<String> {
    if value == old {
        Some(new.to_string())
    } else if value.eq_ignore_ascii_case(&format!("{old}.png")) {
        Some(format!("{new}.png"))
    } else {
        None
    }
}

pub(crate) fn asset_usages(
    kind: ManagedAssetKind,
    identifier: &str,
    world: &World,
    build_template: &Cell,
) -> Vec<AssetUsage> {
    let mut usages = Vec::new();

    match kind {
        ManagedAssetKind::Texture => {
            for (coord, cell) in world.iter_authored_cells() {
                if texture_reference_matches(&cell.texture, identifier) {
                    usages.push(AssetUsage {
                        description: format!(
                            "Cell ({}, {}, {}).texture",
                            coord.x, coord.y, coord.z
                        ),
                    });
                }
            }
            for source in world.iter_sources() {
                if texture_reference_matches(&source.cell.texture, identifier) {
                    usages.push(AssetUsage {
                        description: format!("Source #{}.texture", source.cell.id),
                    });
                }
            }
            for (slot, value) in [
                ("surface", &world.terrain.config.materials.surface),
                ("subsurface", &world.terrain.config.materials.subsurface),
                ("rock", &world.terrain.config.materials.rock),
                ("water", &world.terrain.config.materials.water),
            ] {
                if texture_reference_matches(value, identifier) {
                    usages.push(AssetUsage {
                        description: format!("Terrain materials.{slot}"),
                    });
                }
            }
            if texture_reference_matches(&build_template.texture, identifier) {
                usages.push(AssetUsage {
                    description: "Build template.texture".to_string(),
                });
            }
        }
        ManagedAssetKind::Audio => {
            for (coord, cell) in world.iter_authored_cells() {
                if cell.audio == identifier {
                    usages.push(AssetUsage {
                        description: format!("Cell ({}, {}, {}).audio", coord.x, coord.y, coord.z),
                    });
                }
            }
            for source in world.iter_sources() {
                if source.cell.audio == identifier {
                    usages.push(AssetUsage {
                        description: format!("Source #{}.audio", source.cell.id),
                    });
                }
            }
            if build_template.audio == identifier {
                usages.push(AssetUsage {
                    description: "Build template.audio".to_string(),
                });
            }
        }
        ManagedAssetKind::Material => {
            if world.sky.material_asset.as_deref() == Some(identifier) {
                usages.push(AssetUsage {
                    description: "World Lighting.sky".into(),
                });
            }
            if world.terrain.config.water_material_asset.as_deref() == Some(identifier) {
                usages.push(AssetUsage {
                    description: "Ocean.material".into(),
                });
            }
            for body in &world.terrain.config.water_bodies {
                if body.material_asset.as_deref() == Some(identifier) {
                    usages.push(AssetUsage {
                        description: format!("Water Body #{}.material", body.id),
                    });
                }
            }
        }
        ManagedAssetKind::Path => {}
    }

    usages.sort_by(|left, right| left.description.cmp(&right.description));
    usages
}

fn rewrite_world_references(
    kind: ManagedAssetKind,
    old: &str,
    new: &str,
    world: &mut World,
    build_template: &mut Cell,
    history: &mut History,
) -> usize {
    let mut changed = 0;

    match kind {
        ManagedAssetKind::Texture => {
            let cell_coords = world
                .iter_authored_cells()
                .filter_map(|(coord, cell)| {
                    texture_reference_matches(&cell.texture, old).then_some(coord)
                })
                .collect::<Vec<WorldCoord>>();
            for coord in cell_coords {
                let mut replaced = false;
                if let Some(cell) = world.get_mut(coord) {
                    if let Some(replacement) = rewritten_texture_reference(&cell.texture, old, new)
                    {
                        cell.texture = replacement;
                        changed += 1;
                        replaced = true;
                    }
                }
                if replaced {
                    world.touch_authored_cell(coord, crate::world::DirtyReason::MaterialOrOffset);
                }
            }

            let source_ids = world
                .iter_sources()
                .filter_map(|source| {
                    texture_reference_matches(&source.cell.texture, old).then_some(source.cell.id)
                })
                .collect::<Vec<_>>();
            for id in source_ids {
                if let Some(source) = world.source_mut(id) {
                    if let Some(replacement) =
                        rewritten_texture_reference(&source.cell.texture, old, new)
                    {
                        source.cell.texture = replacement;
                        changed += 1;
                    }
                }
                world.touch_authored_source(id);
            }

            for value in [
                &mut world.terrain.config.materials.surface,
                &mut world.terrain.config.materials.subsurface,
                &mut world.terrain.config.materials.rock,
                &mut world.terrain.config.materials.water,
            ] {
                if let Some(replacement) = rewritten_texture_reference(value, old, new) {
                    *value = replacement;
                    changed += 1;
                }
            }
            if let Some(replacement) =
                rewritten_texture_reference(&build_template.texture, old, new)
            {
                build_template.texture = replacement;
                changed += 1;
            }
            history.rewrite_texture_references(old, new);
        }
        ManagedAssetKind::Audio => {
            let cell_coords = world
                .iter_authored_cells()
                .filter_map(|(coord, cell)| (cell.audio == old).then_some(coord))
                .collect::<Vec<_>>();
            for coord in cell_coords {
                if let Some(cell) = world.get_mut(coord) {
                    cell.audio = new.to_string();
                    changed += 1;
                }
            }

            let source_ids = world
                .iter_sources()
                .filter_map(|source| (source.cell.audio == old).then_some(source.cell.id))
                .collect::<Vec<_>>();
            for id in source_ids {
                if let Some(source) = world.source_mut(id) {
                    source.cell.audio = new.to_string();
                    changed += 1;
                }
                world.touch_authored_source(id);
            }

            if build_template.audio == old {
                build_template.audio = new.to_string();
                changed += 1;
            }
            history.rewrite_audio_references(old, new);
        }
        ManagedAssetKind::Material => {}
        ManagedAssetKind::Path => {}
    }

    changed
}

pub(crate) fn rename_project_asset(
    project_path: &Path,
    kind: ManagedAssetKind,
    old_identifier: &str,
    requested_identifier: &str,
    world: &mut World,
    build_template: &mut Cell,
    history: &mut History,
) -> Result<(String, usize), String> {
    if kind == ManagedAssetKind::Path {
        let (name, mut changed) = asset_lifecycle::rename(
            project_path,
            old_identifier,
            requested_identifier,
            world,
            history,
        )?;
        if old_identifier.starts_with(".assets/effects/") {
            let fx_old = old_identifier.strip_prefix(".assets/effects/").unwrap();
            let fx_new = requested_identifier
                .strip_prefix(".assets/effects/")
                .unwrap_or("");
            if let Some(asset) = build_template.particles.effect_asset.as_mut() {
                if asset == fx_old {
                    *asset = fx_new.to_owned();
                    changed += 1;
                } else if let Some(rest) = asset.strip_prefix(fx_old).filter(|r| r.starts_with('/'))
                {
                    *asset = format!("{fx_new}{rest}");
                    changed += 1;
                }
            }
        }
        let media_kind = if old_identifier.starts_with(".assets/textures/") {
            Some(ManagedAssetKind::Texture)
        } else if old_identifier.starts_with(".assets/audio/") {
            Some(ManagedAssetKind::Audio)
        } else {
            None
        };
        if let Some(media_kind) = media_kind {
            let old = old_identifier.splitn(3, '/').nth(2).unwrap();
            let new = requested_identifier.splitn(3, '/').nth(2).unwrap();
            let field = |cell: &Cell| {
                if media_kind == ManagedAssetKind::Texture {
                    cell.texture.clone()
                } else {
                    cell.audio.clone()
                }
            };
            let mut names = std::collections::BTreeSet::new();
            names.insert(field(build_template));
            names.extend(world.iter_authored_cells().map(|(_, cell)| field(cell)));
            names.extend(world.iter_sources().map(|source| field(&source.cell)));
            for snapshot in history.undo_stack.iter().chain(history.redo_stack.iter()) {
                names.extend(snapshot.0.values().map(field));
                names.extend(snapshot.1.values().map(|source| field(&source.cell)));
            }
            for value in names {
                if let Some(rest) = value.strip_prefix(old).filter(|rest| rest.starts_with('/')) {
                    changed += rewrite_world_references(
                        media_kind,
                        &value,
                        &format!("{new}{rest}"),
                        world,
                        build_template,
                        history,
                    );
                }
            }
        }
        return Ok((name, changed));
    }
    let old_identifier = normalize_identifier(kind, old_identifier)?;
    let new_identifier = normalize_identifier(kind, requested_identifier)?;
    if old_identifier == new_identifier {
        return Err("Choose a different destination identifier.".to_string());
    }

    let root = project_root(project_path, kind);
    let source = identifier_file_path(&root, kind, &old_identifier);
    let destination = identifier_file_path(&root, kind, &new_identifier);
    for path in [&source, &destination] {
        asset_lifecycle::checked(
            project_path,
            &path
                .strip_prefix(project_path)
                .map_err(|_| "Asset outside project")?
                .to_string_lossy()
                .replace('\\', "/"),
        )?;
    }

    if !source.is_file() {
        return Err(format!(
            "Project {} '{}' does not exist.",
            kind.label().to_lowercase(),
            old_identifier
        ));
    }
    if destination.exists() {
        return Err(format!(
            "Destination '{}' already exists; replacement is not supported by WO 0.8.7-21.",
            new_identifier
        ));
    }

    let parent = destination
        .parent()
        .ok_or_else(|| "Asset destination has no parent directory.".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create '{}': {error}", parent.display()))?;

    fs::rename(&source, &destination).map_err(|error| {
        format!(
            "Failed to rename '{}' to '{}': {error}",
            source.display(),
            destination.display()
        )
    })?;
    if let Err(error) =
        crate::project::asset_registry::AssetRegistry::moved(project_path, &source, &destination)
    {
        let rollback = fs::rename(&destination, &source);
        return Err(match rollback {
            Ok(()) => format!("Asset rename was rolled back: {error}"),
            Err(restore) => format!(
                "Asset rename registry failed: {error}; filesystem rollback failed: {restore}"
            ),
        });
    }

    let changed = rewrite_world_references(
        kind,
        &old_identifier,
        &new_identifier,
        world,
        build_template,
        history,
    );

    Ok((new_identifier, changed))
}

fn project_asset_usages(
    project: &Path,
    kind: ManagedAssetKind,
    identifier: &str,
    world: &World,
    template: &Cell,
) -> Vec<AssetUsage> {
    if kind == ManagedAssetKind::Path {
        let mut result = asset_lifecycle::usages(project, identifier, world)
            .unwrap_or_else(|e| vec![AssetUsage { description: e }]);
        if let Some(fx) = identifier.strip_prefix(".assets/effects/") {
            if template
                .particles
                .effect_asset
                .as_deref()
                .is_some_and(|v| v == fx || v.starts_with(&format!("{fx}/")))
            {
                result.push(AssetUsage {
                    description: "Build Emitter FX Type".into(),
                });
            }
        }
        return result;
    }
    if kind != ManagedAssetKind::Material {
        return asset_usages(kind, identifier, world, template);
    }
    let catalog = crate::project::material::MaterialCatalog::cached(project);
    let Some(doc) = catalog.documents().get(identifier) else {
        return vec![AssetUsage {
            description: "invalid Material; repair or remove explicitly outside the manager".into(),
        }];
    };
    let mut usages = asset_usages(kind, &doc.id, world, template);
    usages.extend(
        catalog
            .dependents(&doc.id)
            .into_iter()
            .map(|name| AssetUsage {
                description: format!("Material Instance {name}.parent"),
            }),
    );
    usages
}

fn delete_project_asset_with_builtin_root(
    project_path: &Path,
    builtin_category_root: &Path,
    kind: ManagedAssetKind,
    identifier: &str,
    world: &World,
    build_template: &Cell,
) -> Result<bool, String> {
    let identifier = normalize_identifier(kind, identifier)?;
    if kind == ManagedAssetKind::Path {
        asset_lifecycle::delete(project_path, &identifier)?;
        return Ok(false);
    }
    let _ = (world, build_template);

    let root = project_root(project_path, kind);
    let file = identifier_file_path(&root, kind, &identifier);
    asset_lifecycle::checked(
        project_path,
        &file
            .strip_prefix(project_path)
            .map_err(|_| "Asset outside project")?
            .to_string_lossy()
            .replace('\\', "/"),
    )?;
    if !file.is_file() {
        return Err(format!(
            "Project {} '{}' does not exist; built-in assets are read-only.",
            kind.label().to_lowercase(),
            identifier
        ));
    }

    let builtin = identifier_file_path(builtin_category_root, kind, &identifier);
    fs::remove_file(&file)
        .map_err(|error| format!("Failed to delete '{}': {error}", file.display()))?;
    Ok(builtin.is_file())
}

pub(crate) fn delete_project_asset(
    project_path: &Path,
    kind: ManagedAssetKind,
    identifier: &str,
    world: &World,
    build_template: &Cell,
) -> Result<bool, String> {
    delete_project_asset_with_builtin_root(
        project_path,
        &builtin_root(kind),
        kind,
        identifier,
        world,
        build_template,
    )
}

fn is_project_aeo_script(project: &Path, path: &Path) -> bool {
    path.is_file()
        && path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("aeo"))
        && [
            project.join("scripts"),
            project.join("playerscripts"),
            project.join(".assets/scripts"),
            project.join(".assets/controllers"),
            project.join(".assets/cameras"),
        ]
        .iter()
        .any(|root| path.starts_with(root))
}

fn openable_aeo_asset(project: &Path, asset: &BrowserAsset) -> bool {
    matches!(asset.category, "Scripts" | "Controllers" | "Cameras")
        && !asset.is_directory
        && is_project_aeo_script(project, &asset.path)
}

impl Editor {
    pub(crate) fn invalidate_asset_pack_catalog(&mut self) {
        self.asset_manager.catalog_loaded = false;
    }
    pub(crate) fn advance_hidden_asset_imports(
        &mut self,
        ctx: &egui::Context,
        project_path: &Option<PathBuf>,
        renderer: &Renderer,
    ) {
        if self.asset_manager.open {
            return;
        }
        let Some(project) = project_path.as_deref() else {
            return;
        };
        let completed = self
            .asset_manager
            .background_import
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok());
        if let Some((source, result)) = completed {
            self.asset_manager.background_import = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok(path) => {
                    refresh_manager_catalog(&mut self.asset_manager, project);
                    crate::editor::tools::refresh_asset_catalogs(ctx, project);
                    renderer.set_project_asset_root(Some(project));
                    self.asset_manager.status =
                        Some((format!("Imported {}", path.display()), true));
                }
                Err(error) if error.starts_with("Asset already exists:") => {
                    self.asset_manager.incoming_media_name = source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.asset_manager
                        .pending_media_collisions
                        .push_back(source);
                    self.asset_manager.status=Some(("Import name collision; reopen Asset Manager to choose Replace, Rename, or Cancel.".into(),false));
                }
                Err(error) => {
                    self.script_editor
                        .push_output(format!("Import failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if let Some((source, result)) = self
            .asset_manager
            .glb_inspection
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.glb_inspection = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok(summary) => {
                    let group_name = source
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.asset_manager.pending_glb_import = Some(PendingGlbImport {
                        source,
                        summary,
                        group_name,
                        collision: false,
                    });
                    self.asset_manager.status = Some((
                        "GLB inspected; reopen Asset Manager to confirm import.".into(),
                        true,
                    ));
                }
                Err(error) => {
                    self.script_editor
                        .push_output(format!("GLB inspection failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if let Some(result) = self
            .asset_manager
            .glb_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.glb_job = None;
            self.asset_manager.import_progress_name = None;
            let pending = self.asset_manager.glb_job_pending.take();
            match result {
                Ok(imported) => {
                    let message = format!("GLB import completed: {imported:?}");
                    refresh_manager_catalog(&mut self.asset_manager, project);
                    self.asset_manager.status = Some((message, true));
                }
                Err(error) => {
                    if let Some(mut pending) = pending {
                        pending.collision = error.contains("already exists in this project");
                        self.asset_manager.pending_glb_import = Some(pending);
                    }
                    self.script_editor
                        .push_output(format!("GLB import failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if let Some(result) = self
            .asset_manager
            .legacy_folder_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.legacy_folder_job = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok(name) => {
                    refresh_manager_catalog(&mut self.asset_manager, project);
                    self.asset_manager.status =
                        Some((format!("Imported Character package '{name}'"), true));
                }
                Err(error) => {
                    self.script_editor
                        .push_output(format!("Folder import failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if let Some(result) = self
            .asset_manager
            .folder_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.folder_job = None;
            self.asset_manager.folder_progress = None;
            self.asset_manager.import_progress_name = None;
            let pending = self.asset_manager.folder_job_pending.take();
            match result {
                Ok(imported) => {
                    refresh_manager_catalog(&mut self.asset_manager, project);
                    if let Some(pending) = pending {
                        if matches!(imported, crate::project::GlbFolderImportResult::Static(_)) {
                            let (tx, rx) = std::sync::mpsc::channel();
                            self.asset_manager.folder_media_job =
                                Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                            self.asset_manager.import_progress_name = Some(
                                pending
                                    .source
                                    .file_name()
                                    .unwrap_or_default()
                                    .to_string_lossy()
                                    .into_owned(),
                            );
                            let project = project.to_path_buf();
                            std::thread::spawn(move || {
                                let _ = tx.send(asset_import::import_folder_media(
                                    &project,
                                    &pending.source,
                                ));
                            });
                        }
                    }
                    self.asset_manager.status = Some(("GLB folder imported.".into(), true));
                }
                Err(error) if error.contains("contains no .glb files") => {
                    if let Some(pending) = pending {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.asset_manager.folder_media_job =
                            Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                        self.asset_manager.import_progress_name = Some(
                            pending
                                .source
                                .file_name()
                                .unwrap_or_default()
                                .to_string_lossy()
                                .into_owned(),
                        );
                        let project = project.to_path_buf();
                        std::thread::spawn(move || {
                            let _ = tx
                                .send(asset_import::import_folder_media(&project, &pending.source));
                        });
                    }
                }
                Err(error) => {
                    if let Some(mut pending) = pending {
                        pending.collision = error.contains("already exists in this project");
                        self.asset_manager.pending_folder_import = Some(pending);
                    }
                    self.script_editor
                        .push_output(format!("Folder import failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if let Some(result) = self
            .asset_manager
            .folder_media_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.folder_media_job = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok((count, errors)) => {
                    for error in &errors {
                        self.script_editor
                            .push_output(format!("Folder import: {error}"));
                    }
                    refresh_manager_catalog(&mut self.asset_manager, project);
                    crate::editor::tools::refresh_asset_catalogs(ctx, project);
                    renderer.set_project_asset_root(Some(project));
                    self.asset_manager.status = Some((
                        format!("Imported {count} files; {} errors", errors.len()),
                        errors.is_empty(),
                    ));
                }
                Err(error) => self.asset_manager.status = Some((error, false)),
            }
        }
        if self.asset_manager.background_import.is_none()
            && self.asset_manager.import_progress_name.is_none()
        {
            if let Some((source, name, replace)) = self.asset_manager.queued_media.pop_front() {
                let project = project.to_path_buf();
                let (tx, rx) = std::sync::mpsc::channel();
                self.asset_manager.background_import =
                    Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                self.asset_manager.import_progress_name = Some(
                    source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
                std::thread::spawn(move || {
                    let result =
                        asset_import::import_media(&project, &source, name.as_deref(), true);
                    let _ = tx.send((source, result));
                });
            }
        }
    }

    pub(crate) fn draw_global_import_progress(&self, ctx: &egui::Context) {
        if let Some(name) = self.asset_manager.import_progress_name.as_ref() {
            let units = self
                .asset_manager
                .folder_progress
                .as_ref()
                .map(|counter| counter.load(std::sync::atomic::Ordering::Relaxed));
            let (ratio, detail) = if let Some(value) = units.filter(|v| *v >> 32 > 0) {
                let total = ((value >> 32) as usize).saturating_mul(2);
                let completed = (value & 0xffff_ffff) as usize;
                (
                    Some((completed as f32 / total.max(1) as f32).min(0.98)),
                    format!("Processed {}/{} tasks", completed.min(total), total),
                )
            } else {
                (None, "Preparing / processing files...".to_owned())
            };
            crate::engine::progress_widget::draw_progress(
                ctx,
                "wk_asset_import_progress",
                name,
                ratio,
                &detail,
            );
        }
    }
}

impl Editor {
    fn open_managed_aeo_script(&mut self, project_path: &Path, path: PathBuf) -> bool {
        if !is_project_aeo_script(project_path, &path) {
            return false;
        }
        self.script_editor.open_file(path);
        self.activate_shell_workspace(super::editor::shell::Workspace::Script);
        self.asset_manager.open = false;
        true
    }

    pub(crate) fn draw_asset_manager(
        &mut self,
        ctx: &egui::Context,
        world: &mut World,
        project_path: &Option<PathBuf>,
        renderer: &Renderer,
    ) {
        if !self.asset_manager.open {
            self.asset_manager.was_open = false;
            return;
        }

        let Some(project_path) = project_path.as_deref() else {
            self.asset_manager.status =
                Some(("Open a project before managing assets.".to_string(), false));
            self.asset_manager.open = false;
            self.asset_manager.was_open = false;
            return;
        };

        if self
            .asset_manager
            .sky_material_preview
            .as_ref()
            .is_some_and(|draft| draft.project.as_deref() != Some(project_path))
        {
            self.asset_manager.sky_material_preview = None;
            self.asset_manager.sky_editor_active = false;
        }
        if self
            .asset_manager
            .surface_material_editor
            .as_ref()
            .is_some_and(|edit| edit.project != project_path)
        {
            self.asset_manager.surface_material_editor = None;
            self.asset_manager.surface_editor_active = false;
        }
        if self
            .asset_manager
            .character_editor
            .as_ref()
            .is_some_and(|edit| edit.project != project_path)
        {
            self.asset_manager.character_editor = None;
            self.asset_manager.character_editor_active = false;
        }
        if self
            .asset_manager
            .animation_editor
            .as_ref()
            .is_some_and(|edit| edit.project != project_path)
        {
            self.asset_manager.animation_editor = None;
            self.asset_manager.animation_editor_active = false;
        }
        if let Some(task) = self.asset_manager.pending_material_save.as_ref() {
            let completed = task.result.try_lock().ok().and_then(|mut slot| slot.take());
            if let Some(result) = completed {
                let task = self
                    .asset_manager
                    .pending_material_save
                    .take()
                    .expect("task checked");
                if task.request.project != project_path {
                    self.asset_manager.status = Some((
                        "Material saved in its original project; current project unchanged.".into(),
                        true,
                    ));
                } else {
                    let status = match result {
                        Err(error) => Err(error),
                        Ok(catalog) => {
                            let accepted = match task.kind {
                                MaterialSaveKind::Surface => self
                                    .asset_manager
                                    .surface_material_editor
                                    .as_mut()
                                    .filter(|edit| {
                                        edit.project == project_path
                                            && edit.draft.document().id == task.request.document.id
                                    })
                                    .ok_or(
                                        "Original Surface Material editor is unavailable"
                                            .to_owned(),
                                    )
                                    .and_then(|edit| {
                                        edit.accept_background_save(&task.request, &catalog)
                                    }),
                                MaterialSaveKind::Sky => self
                                    .asset_manager
                                    .sky_material_preview
                                    .as_mut()
                                    .filter(|edit| {
                                        edit.project.as_deref() == Some(project_path)
                                            && edit.material_id()
                                                == Some(task.request.document.id.as_str())
                                    })
                                    .ok_or("Original Sky Material editor is unavailable".to_owned())
                                    .and_then(|edit| {
                                        edit.accept_background_save(&task.request, &catalog)
                                    }),
                            };
                            accepted.and_then(|_| {
                                renderer.publish_saved_material_snapshot(
                                    catalog,
                                    &task.request.document.id,
                                )
                            })
                        }
                    };
                    match status {
                        Ok(()) => {
                            self.asset_manager.status = Some(("Material saved.".into(), true));
                            if let Some(editor) =
                                self.asset_manager.surface_material_editor.as_mut()
                            {
                                if editor.draft.document().id == task.request.document.id {
                                    editor.status = Some(("Material saved.".into(), true));
                                }
                            }
                            if let Some(editor) = self.asset_manager.sky_material_preview.as_mut() {
                                if editor.material_id() == Some(task.request.document.id.as_str()) {
                                    editor.status = Some(("Material saved.".into(), true));
                                }
                            }
                            self.asset_manager.sky_choices_project = None;
                        }
                        Err(error) => {
                            self.asset_manager.status =
                                Some((format!("Save failed: {error}"), false))
                        }
                    }
                }
            } else {
                ctx.request_repaint_after(std::time::Duration::from_millis(50));
            }
        }
        if self.asset_manager.animation_editor_active {
            let root = Some(project_path.to_path_buf());
            egui::TopBottomPanel::bottom("animation_editor_content_browser")
                .resizable(true)
                .default_height(230.0)
                .min_height(120.0)
                .show(ctx, |ui| self.draw_unreal_asset_browser(ui, &root));
            let action = if let Some(editor) = self.asset_manager.animation_editor.as_mut() {
                let mut action = crate::editor::animation_editor::EditorAction::None;
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = crate::editor::animation_editor::draw(ui, editor);
                });
                action
            } else {
                crate::editor::animation_editor::EditorAction::Close
            };
            match action {
                crate::editor::animation_editor::EditorAction::None => {}
                crate::editor::animation_editor::EditorAction::Saved => {
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                    self.asset_manager.browser_category = "Animations".into();
                    self.shell.asset_folder = "Animations".into();
                    self.shell.refresh(Some(project_path));
                }
                crate::editor::animation_editor::EditorAction::Close => {
                    self.asset_manager.browser_category = "Animations".into();
                    self.shell.asset_folder = "Animations".into();
                    self.shell.refresh(Some(project_path));
                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                    self.asset_manager.animation_editor_active = false;
                    self.asset_manager.animation_editor = None;
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                }
            }
            return;
        }
        if self.asset_manager.character_editor_active {
            let root = Some(project_path.to_path_buf());
            egui::TopBottomPanel::bottom("character_editor_content_browser")
                .resizable(true)
                .default_height(265.0)
                .min_height(145.0)
                .show(ctx, |ui| self.draw_unreal_asset_browser(ui, &root));
            let previous_character_name = self
                .asset_manager
                .character_editor
                .as_ref()
                .and_then(|editor| editor.original_name.clone());
            let action = if let Some(editor) = self.asset_manager.character_editor.as_mut() {
                let mut action = crate::editor::character_editor::CharacterEditorAction::None;
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = crate::editor::character_editor::draw(ui, editor);
                });
                action
            } else {
                crate::editor::character_editor::CharacterEditorAction::Cancel
            };
            match action {
                crate::editor::character_editor::CharacterEditorAction::None => {}
                crate::editor::character_editor::CharacterEditorAction::Saved => {
                    if let Some(editor) = self.asset_manager.character_editor.as_ref() {
                        let selected = previous_character_name.as_deref()
                            == Some(world.selected_character.as_str())
                            || crate::project::asset_registry::AssetRegistry::load(project_path)
                                .ok()
                                .and_then(|registry| {
                                    registry.find(&world.selected_character).cloned()
                                })
                                .is_some_and(|entry| {
                                    entry.kind
                                        == crate::project::asset_registry::AssetType::Character
                                        && entry.path.ends_with(&format!("/{}", editor.draft.name))
                                });
                        if selected {
                            match crate::project::character_asset::assign_to_world(
                                project_path,
                                &editor.draft.name,
                                world,
                            ) {
                                Ok(()) => self.needs_save = true,
                                Err(error) => self.asset_manager.status = Some((error, false)),
                            }
                        }
                    }
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                    self.asset_manager.browser_category = "Characters".into();
                    self.shell.asset_folder = "Characters".into();
                    self.shell.refresh(Some(project_path));
                    self.asset_manager.character_editor_active = false;
                    self.asset_manager.character_editor = None;
                }
                crate::editor::character_editor::CharacterEditorAction::Cancel => {
                    self.asset_manager.character_editor_active = false;
                    self.asset_manager.character_editor = None;
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                }
            }
            return;
        }
        if self.asset_manager.surface_editor_active {
            let root = Some(project_path.to_path_buf());
            egui::TopBottomPanel::bottom("wo_asm_002_material_content_browser")
                .resizable(true)
                .default_height(265.0)
                .show(ctx, |ui| {
                    self.draw_unreal_asset_browser(ui, &root);
                });
            let mut action = material_editor::SkyEditorAction::None;
            let catalog = renderer.material_snapshot();
            let preview_sky =
                material_editor::surface_preview_sky(world.sky.material_asset.as_deref(), &catalog);
            if let Some(edit) = self.asset_manager.surface_material_editor.as_mut() {
                let material_id = format!("Material:{}", edit.draft.document().id);
                let materials = &world.terrain.config.materials;
                edit.landscape_assigned = [
                    materials.surface.as_str(),
                    materials.subsurface.as_str(),
                    materials.rock.as_str(),
                ]
                .contains(&material_id.as_str())
                    || world
                        .terrain
                        .config
                        .terrain_material_layers()
                        .iter()
                        .any(|layer| layer.material == material_id);
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = material_editor::draw_surface_material_editor(
                        ui,
                        edit,
                        &preview_sky,
                        renderer,
                    );
                });
            } else {
                action = material_editor::SkyEditorAction::Return;
            }
            if let Some(parent_id) = self
                .asset_manager
                .surface_material_editor
                .as_mut()
                .and_then(|edit| edit.graph_parent_request.take())
            {
                if self
                    .asset_manager
                    .surface_material_editor
                    .as_ref()
                    .is_some_and(|edit| edit.dirty())
                {
                    if let Some(edit) = self.asset_manager.surface_material_editor.as_mut() {
                        edit.status = Some(("Unsaved changes".into(), false));
                    }
                } else {
                    let parent_name = catalog
                        .documents()
                        .iter()
                        .find(|(name, document)| {
                            document.id == parent_id && !name.starts_with("pack/")
                        })
                        .map(|(name, _)| name.clone());
                    match parent_name {
                        Some(name) => {
                            match material_editor::SurfaceMaterialEditor::open(project_path, &name)
                            {
                                Ok(mut parent) => {
                                    parent.graph_view = true;
                                    self.asset_manager.surface_material_editor = Some(parent);
                                }
                                Err(error) => {
                                    if let Some(edit) =
                                        self.asset_manager.surface_material_editor.as_mut()
                                    {
                                        edit.status = Some((error, false));
                                    }
                                }
                            }
                        }
                        None => {
                            if let Some(edit) = self.asset_manager.surface_material_editor.as_mut()
                            {
                                edit.status =
                                    Some(("Parent Material is not project-editable".into(), false));
                            }
                        }
                    }
                }
            }
            match action {
                material_editor::SkyEditorAction::None
                | material_editor::SkyEditorAction::Apply => {}
                material_editor::SkyEditorAction::Return => {
                    self.asset_manager.surface_editor_active = false;
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                }
                material_editor::SkyEditorAction::Save => {
                    if self.asset_manager.pending_material_save.is_none() {
                        let catalog = renderer.material_snapshot();
                        let result = self
                            .asset_manager
                            .surface_material_editor
                            .as_mut()
                            .ok_or("Material editor not open".to_owned())
                            .and_then(|edit| {
                                edit.prepare_for_save(&catalog)?;
                                Ok(edit.draft.save_request())
                            });
                        match result {
                            Ok(request) => {
                                self.asset_manager.pending_material_save =
                                    Some(start_material_save(MaterialSaveKind::Surface, request))
                            }
                            Err(error) => {
                                self.asset_manager.status =
                                    Some((format!("Save failed: {error}"), false))
                            }
                        }
                    }
                }
            }
            return;
        }
        if self.asset_manager.sky_editor_active {
            let root = Some(project_path.to_path_buf());
            egui::TopBottomPanel::bottom("wo_asm_002_material_content_browser")
                .resizable(true)
                .default_height(265.0)
                .show(ctx, |ui| {
                    self.draw_unreal_asset_browser(ui, &root);
                });
            let action = if let Some(draft) = self.asset_manager.sky_material_preview.as_mut() {
                let mut action = material_editor::SkyEditorAction::None;
                egui::CentralPanel::default().show(ctx, |ui| {
                    action = material_editor::draw_sky_material_editor(ui, draft);
                });
                action
            } else {
                material_editor::SkyEditorAction::Return
            };
            match action {
                material_editor::SkyEditorAction::None => {}
                material_editor::SkyEditorAction::Return => {
                    self.asset_manager.sky_editor_active = false;
                    self.asset_manager.sky_choices_project = None;
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                }
                material_editor::SkyEditorAction::Save => {
                    if self.asset_manager.pending_material_save.is_none() {
                        let result = self
                            .asset_manager
                            .sky_material_preview
                            .as_mut()
                            .ok_or("Sky Material editor not open".to_owned())
                            .and_then(|draft| draft.prepare_save_request());
                        match result {
                            Ok(request) => {
                                self.asset_manager.pending_material_save =
                                    Some(start_material_save(MaterialSaveKind::Sky, request))
                            }
                            Err(error) => {
                                self.asset_manager.status =
                                    Some((format!("Save failed: {error}"), false))
                            }
                        }
                    }
                }
                material_editor::SkyEditorAction::Apply => {
                    if let Some(draft) = self.asset_manager.sky_material_preview.as_mut() {
                        if !draft.is_dirty() {
                            if let Some(id) = draft.material_id() {
                                if renderer.material_snapshot().resolve(id).is_err() {
                                    renderer.reload_material_assets();
                                }
                                world.sky.material_asset = Some(id.to_owned());
                                self.needs_save = true;
                                draft.status =
                                    Some(("Sky Material assigned to World.".into(), true));
                            }
                        }
                    }
                }
            }
            return;
        }

        let just_opened = !self.asset_manager.was_open;
        self.asset_manager.was_open = true;
        match crate::project::asset_registry::AssetRegistry::background_index_status(project_path) {
            Some(crate::project::asset_registry::BackgroundIndexStatus::Indexing) => {
                self.asset_manager.catalog_loaded = false;
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(egui::Spinner::new());
                        ui.label("Indexing project assets in background...");
                    });
                });
                ctx.request_repaint_after(std::time::Duration::from_millis(120));
                return;
            }
            Some(crate::project::asset_registry::BackgroundIndexStatus::Failed(error)) => {
                self.asset_manager.status =
                    Some((format!("Asset indexing failed: {error}"), false));
            }
            _ => {}
        }

        if just_opened
            || !self.asset_manager.catalog_loaded
            || self.asset_manager.catalog_project.as_deref() != Some(project_path)
        {
            refresh_manager_catalog(&mut self.asset_manager, project_path);
        }

        let now = ctx.input(|input| input.time);
        if now >= self.asset_manager.next_registry_poll {
            self.asset_manager.next_registry_poll = now + 0.75;
            if self.asset_manager.catalog_project.as_deref() == Some(project_path)
                && crate::project::asset_registry::AssetRegistry::snapshot_if_changed(
                    project_path,
                    self.asset_manager.catalog_revision,
                )
                .ok()
                .flatten()
                .is_some()
            {
                refresh_manager_catalog(&mut self.asset_manager, project_path);
                self.shell.refresh(Some(project_path));
                crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                ctx.request_repaint();
            }
        }
        if self.asset_manager.selected_asset.is_none() {
            if let Some(first) = self
                .asset_manager
                .catalog
                .iter()
                .find(|asset| asset.kind == self.asset_manager.kind)
            {
                self.asset_manager.selected_asset = Some((first.kind, first.identifier.clone()));
            }
        }

        if let Some((selected_kind, selected_identifier)) =
            self.asset_manager.selected_asset.clone()
        {
            let revision = world.render_revision();
            let build_value = match selected_kind {
                ManagedAssetKind::Texture => self.build_template.texture.clone(),
                ManagedAssetKind::Audio => self.build_template.audio.clone(),
                ManagedAssetKind::Path => String::new(),
                ManagedAssetKind::Material => format!(
                    "{:?}:{:?}",
                    world.terrain.config.water_material_asset,
                    world
                        .terrain
                        .config
                        .water_bodies
                        .iter()
                        .map(|b| (&b.id, &b.material_asset))
                        .collect::<Vec<_>>()
                ),
            };

            let usages_are_stale = self.asset_manager.selected_usage_revision != Some(revision)
                || self.asset_manager.selected_usage_build_value.as_deref()
                    != Some(build_value.as_str());

            if usages_are_stale {
                self.asset_manager.selected_usages = project_asset_usages(
                    project_path,
                    selected_kind,
                    &selected_identifier,
                    world,
                    &self.build_template,
                );
                self.asset_manager.selected_usage_revision = Some(revision);
                self.asset_manager.selected_usage_build_value = Some(build_value);
            }
        } else {
            self.asset_manager.selected_usages.clear();
            self.asset_manager.selected_usage_revision = None;
            self.asset_manager.selected_usage_build_value = None;
        }

        let texture_count = self
            .asset_manager
            .catalog
            .iter()
            .filter(|asset| asset.kind == ManagedAssetKind::Texture)
            .count();
        let material_count = self
            .asset_manager
            .catalog
            .iter()
            .filter(|asset| asset.kind == ManagedAssetKind::Material)
            .count();
        let audio_count = self
            .asset_manager
            .catalog
            .iter()
            .filter(|asset| asset.kind == ManagedAssetKind::Audio)
            .count();

        let search = self.asset_manager.search.trim().to_ascii_lowercase();
        let kind = self.asset_manager.kind;
        let visible = self
            .asset_manager
            .catalog
            .iter()
            .filter(|asset| asset.kind == kind)
            .filter(|asset| {
                search.is_empty() || asset.identifier.to_ascii_lowercase().contains(&search)
            })
            .cloned()
            .collect::<Vec<_>>();

        let selected_detail = self.asset_manager.selected_asset.as_ref().and_then(
            |(selected_kind, selected_identifier)| {
                self.asset_manager
                    .catalog
                    .iter()
                    .find(|asset| {
                        asset.kind == *selected_kind && asset.identifier == *selected_identifier
                    })
                    .cloned()
            },
        );

        let selected_usages = self.asset_manager.selected_usages.clone();
        let actor_packages = self.asset_manager.project_actor_packages.clone();
        let character_packages = self.asset_manager.project_character_packages.clone();
        let mut place_actor_package: Option<String> = None;
        let mut select_player_package: Option<String> = None;
        let mut open = self.asset_manager.open;
        let mut refresh_clicked = false;
        let mut choose_import_files = false;
        let mut choose_import_folders = false;
        let mut begin_canonical_import = false;
        let mut inventory_add_request: Option<InventorySelection> = None;
        let mut inventory_confirm_pack = false;
        let mut inventory_cancel_pack = false;
        let mut cancel_canonical_import = false;
        let mut replace_canonical_asset: Option<String> = None;
        let mut import_folder_clicked = false;
        let mut cancel_folder_clicked = false;
        let mut media_collision_action = None;
        let mut import_glb_clicked = false;
        let mut cancel_glb_clicked = false;
        let mut glb_collision_action = None;
        let mut folder_collision_action = None;

        let mut close_page = false;
        if let Some(path) = self.asset_manager.browser_selected.clone() {
            if self.asset_manager.browser_preview_loaded.as_ref() != Some(&path) {
                match load_browser_thumbnail(ctx, &path) {
                    ThumbnailLoad::Ready(texture) => {
                        self.asset_manager.browser_preview =
                            Some((path.clone(), BrowserPreview(texture)));
                        self.asset_manager.browser_preview_loaded = Some(path);
                    }
                    ThumbnailLoad::Failed => {
                        self.asset_manager.browser_preview = None;
                        self.asset_manager.browser_preview_loaded = Some(path);
                    }
                    ThumbnailLoad::Pending => {
                        ctx.request_repaint_after(std::time::Duration::from_millis(250));
                    }
                }
            }
        }
        egui::CentralPanel::default().show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading("ASSET MANAGER");
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Return to Editor").clicked() {
                            close_page = true;
                        }
                    });
                });
                ui.separator();
                ui.horizontal(|ui| {
                    ui.label(format!("{} project files indexed", self.asset_manager.browser_files.iter().filter(|a| !a.is_directory).count()));
                    if ui.button("Refresh").clicked() {
                        refresh_clicked = true;
                        self.asset_manager.inventory_snapshot = None;
                        self.asset_manager.inventory_error = None;
                    }
                    if ui.button("Asset Packs").clicked() {
                        ctx.data_mut(|data|data.insert_temp(egui::Id::new("wk_open_project_asset_packs"),true));
                    }
                    ui.menu_button("Import", |ui| {
                        if ui.button("Files...").clicked() {
                            choose_import_files = true;
                            ui.close_menu();
                        }
                        if ui.button("Folders...").clicked() {
                            choose_import_folders = true;
                            ui.close_menu();
                        }
                    });
                    if ui.selectable_label(self.asset_manager.inventory_open, "Inventory").clicked() {
                        self.asset_manager.inventory_open = !self.asset_manager.inventory_open;
                        self.asset_manager.inventory_snapshot = None;
                        self.asset_manager.inventory_error = None;
                    }
                    let unsaved_surface = self.asset_manager.surface_material_editor.as_ref()
                        .is_some_and(|editor| editor.project == project_path && editor.dirty());
                    let unsaved_sky = self.asset_manager.sky_material_preview.as_ref()
                        .is_some_and(|editor| editor.project.as_deref() == Some(project_path) && editor.is_dirty());
                    if !self.asset_manager.surface_editor_active && !self.asset_manager.sky_editor_active
                        && (unsaved_surface || unsaved_sky)
                    {
                        let button = egui::Button::new(egui::RichText::new("Open Unsaved")
                            .color(egui::Color32::BLACK).strong())
                            .fill(egui::Color32::from_rgb(255, 215, 0));
                        if ui.add(button).on_hover_text("Return to the unsaved Material editor without losing changes").clicked() {
                            let surface_selected = self.asset_manager.browser_selected.as_ref()
                                .is_some_and(|path| self.asset_manager.surface_material_editor.as_ref()
                                    .and_then(|editor| crate::project::material::file_path(project_path,&editor.name).ok())
                                    .as_ref() == Some(path));
                            self.asset_manager.surface_editor_active = unsaved_surface && (surface_selected || !unsaved_sky);
                            self.asset_manager.sky_editor_active = unsaved_sky && !self.asset_manager.surface_editor_active;
                            self.asset_manager.status = None;
                            ctx.request_repaint();
                        }
                    }
                });
                if let Some((message, success)) = &self.asset_manager.status {
                    ui.colored_label(if *success { egui::Color32::LIGHT_GREEN } else { egui::Color32::LIGHT_RED }, message);
                }
                let selected_record=self.asset_manager.browser_selected.as_ref()
                    .and_then(|path|crate::project::asset_registry::AssetRegistry::load(project_path)
                        .ok()?.assets.into_iter()
                        .find(|record|project_path.join(&record.path)==*path));
                if let Some(record)=selected_record {
                    ui.horizontal(|ui| {
                        ui.small(format!("Selected: {} ({})",record.name,record.kind.category()));
                        if ui.small_button("Manage asset").clicked(){
                            self.asset_manager.canonical_manage_id=Some(record.id.clone());
                            self.asset_manager.canonical_manage_name=record.name.clone();
                            self.asset_manager.canonical_manage_group=record.group.clone().unwrap_or_default();
                            self.asset_manager.canonical_delete_confirm=false;
                        }
                    });
                    if self.asset_manager.canonical_manage_id.as_deref()==Some(&record.id) {
                        ui.group(|ui|{
                            ui.horizontal(|ui|{
                                ui.label("Name");
                                ui.text_edit_singleline(&mut self.asset_manager.canonical_manage_name);
                                ui.label("Group");
                                ui.text_edit_singleline(&mut self.asset_manager.canonical_manage_group);
                            });
                            ui.horizontal(|ui|{
                                if ui.button("Save").clicked(){
                                    let group=(!self.asset_manager.canonical_manage_group.trim().is_empty())
                                        .then_some(self.asset_manager.canonical_manage_group.as_str());
                                    match crate::project::asset_registry::AssetRegistry::update_metadata(
                                        project_path,&record.id,&self.asset_manager.canonical_manage_name,group){
                                        Ok(_)=>{
                                            self.asset_manager.status=Some(("Asset metadata saved.".into(),true));
                                            refresh_clicked=true;
                                        }
                                        Err(error)=>self.asset_manager.status=Some((error,false)),
                                    }
                                }
                                let replaceable=!matches!(record.kind,
                                    crate::project::asset_registry::AssetType::Character
                                    |crate::project::asset_registry::AssetType::Animation
                                    |crate::project::asset_registry::AssetType::Controller
                                    |crate::project::asset_registry::AssetType::Camera);
                                if ui.add_enabled(replaceable,egui::Button::new("Replace file...")).clicked(){
                                    replace_canonical_asset=Some(record.id.clone());
                                }
                                if ui.button("Delete...").clicked(){
                                    self.asset_manager.canonical_delete_confirm=true;
                                }
                                if ui.button("Close").clicked(){
                                    self.asset_manager.canonical_manage_id=None;
                                    self.asset_manager.canonical_delete_confirm=false;
                                }
                            });
                            if self.asset_manager.canonical_delete_confirm {
                                let world_kind=match record.kind {
                                    crate::project::asset_registry::AssetType::Texture=>Some(ManagedAssetKind::Texture),
                                    crate::project::asset_registry::AssetType::Audio=>Some(ManagedAssetKind::Audio),
                                    crate::project::asset_registry::AssetType::Material=>Some(ManagedAssetKind::Material),
                                    _=>None,
                                };
                                let used_by_scene=world_kind.is_some_and(|kind|{
                                    !asset_usages(kind,&record.id,world,&self.build_template).is_empty()
                                        || !asset_usages(kind,&record.path,world,&self.build_template).is_empty()
                                });
                                let active=used_by_scene || world.actors.values().any(|actor|
                                    actor.package.as_deref()==Some(&record.id) ||
                                    actor.package.as_deref()==std::path::Path::new(&record.path)
                                        .file_name().and_then(|value|value.to_str()))
                                    || (record.kind==crate::project::asset_registry::AssetType::Character
                                        && world.selected_character==record.id);
                                let users=crate::project::asset_registry::AssetRegistry::load(project_path)
                                    .map(|registry|registry.users_of(&record.id).len()).unwrap_or(0);
                                ui.colored_label(egui::Color32::YELLOW,
                                    format!("Delete permanently? {users} registered dependents; {}.",
                                        if active {"Actor is used in the active World"}else{"Check other saved Worlds"}));
                                if ui.add_enabled(!active&&users==0,egui::Button::new("Confirm delete")).clicked(){
                                    match crate::project::asset_registry::AssetRegistry::delete_result(project_path,&record.id){
                                        Ok(_)=>{
                                            self.asset_manager.canonical_manage_id=None;
                                            self.asset_manager.canonical_delete_confirm=false;
                                            self.asset_manager.browser_selected=None;
                                            self.asset_manager.status=Some(("Asset deleted.".into(),true));
                                            refresh_clicked=true;
                                        }
                                        Err(error)=>self.asset_manager.status=Some((error,false)),
                                    }
                                }
                                if ui.small_button("Keep asset").clicked(){
                                    self.asset_manager.canonical_delete_confirm=false;
                                }
                            }
                        });
                    }
                }
                if self.asset_manager.inventory_open {
                    let back_to_assets = ui.horizontal(|ui| {
                        let back = ui.button("Back to Assets").clicked();
                        ui.heading("Inventory");
                        back
                    }).inner;
                    if back_to_assets {
                        self.asset_manager.inventory_open = false;
                    } else {
                    if self.asset_manager.inventory_snapshot.is_none()
                        && self.asset_manager.inventory_error.is_none()
                    {
                        match (
                            crate::project::shared_inventory::imported(),
                            crate::project::shared_inventory::installed_packs(),
                        ) {
                            (Ok(user), Ok(installed)) =>
                                self.asset_manager.inventory_snapshot = Some((user, installed)),
                            (Err(error), _) | (_, Err(error)) =>
                                self.asset_manager.inventory_error = Some(error),
                        }
                    }
                    if let Some(error) = self.asset_manager.inventory_error.as_ref() {
                        ui.colored_label(ui.visuals().error_fg_color, error);
                    }
                    if let Some((user, installed)) = self.asset_manager.inventory_snapshot.as_ref() {
                        let mut selection_change = None;
                        ui.strong("Imported Assets");
                        let current_collections =
                            crate::project::shared_inventory::project_collections(project_path);
                        for asset in user {
                            let key = InventorySelection::Imported(asset.id.clone());
                            ui.horizontal(|ui| {
                                if ui.selectable_label(
                                    self.asset_manager.inventory_selection.as_ref() == Some(&key),
                                    &asset.name,
                                ).clicked() {
                                    selection_change = Some(key);
                                }
                                ui.small(format!("{} files", asset.files));
                                if current_collections.as_ref().is_ok_and(|ids| ids.contains(&asset.id)) {
                                    ui.small("Installed in Project");
                                }
                            });
                        }
                        if let Err(error) = &current_collections {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        ui.separator();
                        ui.strong("Installed Packs");
                        let current_packs =
                            crate::project::asset_packs::references(project_path);
                        for pack in installed {
                            let key = InventorySelection::Pack(pack.reference.clone());
                            ui.horizontal(|ui| {
                                if ui.selectable_label(
                                    self.asset_manager.inventory_selection.as_ref() == Some(&key),
                                    &pack.name,
                                ).clicked() {
                                    selection_change = Some(key);
                                }
                                ui.small(format!("{} | {} files", pack.category, pack.files));
                                if current_packs.as_ref().is_ok_and(|refs| refs.contains(&pack.reference)) {
                                    ui.small("Installed in Project");
                                }
                            });
                        }
                        if let Err(error) = &current_packs {
                            ui.colored_label(ui.visuals().error_fg_color, error);
                        }
                        if let Some(selected) = selection_change {
                            self.asset_manager.inventory_selection = Some(selected);
                            self.asset_manager.inventory_pending_pack = None;
                            self.asset_manager.inventory_pack_project = None;
                            self.asset_manager.inventory_pending_collection = None;
                            self.asset_manager.canonical_import_plan = None;
                            self.asset_manager.canonical_import_plan_project = None;
                        }
                        if let Some(selected) = self.asset_manager.inventory_selection.as_ref() {
                            let already_installed = match selected {
                                InventorySelection::Imported(id) => current_collections.as_ref()
                                    .is_ok_and(|ids| ids.contains(id)),
                                InventorySelection::Pack(reference) => current_packs.as_ref()
                                    .is_ok_and(|refs| refs.contains(reference)),
                            };
                            if ui.add_enabled(
                                self.asset_manager.canonical_import_job.is_none() && !already_installed,
                                egui::Button::new("Add to Project"),
                            ).clicked() {
                                inventory_add_request = Some(selected.clone());
                            }
                        }
                    }
                    if let Some(plan) = self.asset_manager.canonical_import_plan.as_mut() {
                        if self.asset_manager.canonical_import_plan_project.as_deref() == Some(project_path) {
                            ui.group(|ui| {
                                ui.strong("Review import");
                                ui.small(format!("{} supported assets", plan.assets.len()));
                                ui.horizontal_wrapped(|ui| {
                                    for (kind, count) in plan.counts() {
                                        ui.label(format!("{kind}: {count}"));
                                    }
                                });
                                for item in plan.assets.iter().filter(|a| a.conflict.is_some()).take(8) {
                                    ui.colored_label(ui.visuals().error_fg_color,
                                        format!("{}: {}", item.name, item.conflict.as_deref().unwrap_or_default()));
                                }
                                draw_import_conflict_review(
                                    ui, plan,
                                    &mut self.asset_manager.canonical_import_rename,
                                    &mut self.asset_manager.canonical_import_rename_error,
                                );
                                for warning in plan.warnings.iter().take(8) {
                                    ui.colored_label(egui::Color32::YELLOW, warning);
                                }
                                ui.horizontal(|ui| {
                                    if ui.add_enabled(plan.can_import(),
                                        egui::Button::new("Import selected assets")).clicked()
                                    {
                                        begin_canonical_import = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        cancel_canonical_import = true;
                                    }
                                });
                            });
                        }
                    }
                    if let Some(reference) = self.asset_manager.inventory_pending_pack.as_ref() {
                        if self.asset_manager.inventory_pack_project.as_deref() == Some(project_path) {
                            ui.group(|ui| {
                                ui.strong("Review import");
                                let name = self.asset_manager.inventory_snapshot.as_ref()
                                    .and_then(|(_, packs)| packs.iter()
                                        .find(|pack| &pack.reference == reference)
                                        .map(|pack| pack.name.as_str()))
                                    .unwrap_or(&reference.id);
                                ui.label(name);
                                ui.horizontal(|ui| {
                                    if ui.button("Install Selected").clicked() {
                                        inventory_confirm_pack = true;
                                    }
                                    if ui.button("Cancel").clicked() {
                                        inventory_cancel_pack = true;
                                    }
                                });
                            });
                        }
                    }
                    return;
                    }
                }
                if let Some(plan)=self.asset_manager.canonical_import_plan.as_mut() {
                    ui.group(|ui| {
                        ui.strong("Review import");
                        ui.small(format!("{} supported assets",plan.assets.len()));
                        ui.horizontal_wrapped(|ui| {
                            for (kind,count) in plan.counts() {
                                ui.label(format!("{kind}: {count}"));
                            }
                        });
                        for item in plan.assets.iter().filter(|a|a.conflict.is_some()).take(8) {
                            ui.colored_label(egui::Color32::LIGHT_RED,format!(
                                "{}: {}",item.name,item.conflict.as_deref().unwrap_or_default()));
                        }
                        draw_import_conflict_review(
                            ui, plan,
                            &mut self.asset_manager.canonical_import_rename,
                            &mut self.asset_manager.canonical_import_rename_error,
                        );
                        for warning in plan.warnings.iter().take(8) {
                            ui.colored_label(egui::Color32::YELLOW,warning);
                        }
                        ui.horizontal(|ui| {
                            if ui.add_enabled(plan.can_import(),
                                egui::Button::new("Import selected assets")).clicked() {
                                begin_canonical_import=true;
                            }
                            if ui.button("Cancel").clicked() {cancel_canonical_import=true;}
                        });
                    });
                }
                if let Some(progress)=self.asset_manager.canonical_import_progress.as_ref() {
                    let percent=progress.load(std::sync::atomic::Ordering::Relaxed).min(100);
                    ui.add(egui::ProgressBar::new(percent as f32/100.0)
                        .text(format!("Importing assets: {percent}%")));
                }
                if let Some(pending) = self.asset_manager.pending_glb_import.as_mut() {
                    if !pending.collision {
                        ui.group(|ui| {
                            ui.strong("Import model package");
                            ui.label(format!("Source: {}  |  {} mesh(es), {} animation(s)",
                                pending.source.display(), pending.summary.meshes.len(),
                                pending.summary.animation_names.len()));
                            ui.horizontal(|ui| {
                                ui.label("Package name:");
                                ui.text_edit_singleline(&mut pending.group_name);
                            });
                            ui.horizontal(|ui| {
                                if ui.add_enabled(valid_actor_group_name(&pending.group_name),
                                    egui::Button::new("Import package")).clicked() { import_glb_clicked = true; }
                                if ui.button("Cancel").clicked() { cancel_glb_clicked = true; }
                            });
                        });
                    }
                }
                if let Some(pending) = self.asset_manager.pending_folder_import.as_mut() {
                    if !pending.collision {
                        ui.group(|ui| {
                            ui.strong("Import folder package");
                            ui.label(format!("Source: {}", pending.source.display()));
                            ui.horizontal(|ui| {
                                ui.label("Package name:");
                                ui.text_edit_singleline(&mut pending.group_name);
                            });
                            ui.horizontal(|ui| {
                                if ui.add_enabled(valid_actor_group_name(&pending.group_name),
                                    egui::Button::new("Import package")).clicked() { import_folder_clicked = true; }
                                if ui.button("Cancel").clicked() { cancel_folder_clicked = true; }
                            });
                        });
                    }
                }
                if let Some(source) = self.asset_manager.pending_media_collisions.front() {
                    ui.group(|ui| {
                        ui.strong("Asset name conflict");
                        ui.label(format!("{} already exists in this project.", source.file_name()
                            .unwrap_or_default().to_string_lossy()));
                        ui.horizontal(|ui| {
                            ui.label("Incoming name:");
                            ui.text_edit_singleline(&mut self.asset_manager.incoming_media_name);
                        });
                        ui.horizontal(|ui| {
                            if ui.button("Replace").clicked() { media_collision_action = Some(MediaCollisionAction::Replace); }
                            if ui.button("Rename Incoming Asset").clicked() { media_collision_action = Some(MediaCollisionAction::Rename); }
                            if ui.button("Cancel").clicked() { media_collision_action = Some(MediaCollisionAction::Cancel); }
                        });
                    });
                }
                ui.separator();
                asset_workspace::draw(ui, &mut self.asset_manager, project_path, &mut refresh_clicked, Some(renderer));
                ui.separator();

            });

        if let Some((path, as_sky)) = self.asset_manager.material_assignment_request.take() {
            let result = selected_material_id(project_path, &path).and_then(|id| {
                let catalog = crate::project::material::MaterialCatalog::load(project_path);
                if as_sky {
                    catalog
                        .sky(&id)
                        .ok_or("Material cannot be rendered as Sky".to_owned())?;
                    world.sky.material_asset = Some(id);
                    self.asset_manager.sky_choices_project = None;
                } else {
                    catalog
                        .water(&id)
                        .ok_or("Material cannot be rendered as Water".to_owned())?;
                    world.terrain.config.water_material_asset = Some(id);
                    world.terrain.mark_full_render_dirty();
                }
                self.needs_save = true;
                Ok(if as_sky {
                    "Sky Material assigned."
                } else {
                    "Ocean Water Material assigned."
                })
            });
            self.asset_manager.status = Some(match result {
                Ok(message) => (message.to_owned(), true),
                Err(error) => (format!("Material assignment failed: {error}"), false),
            });
            ctx.request_repaint();
        }

        if let Some(pending) = self
            .asset_manager
            .pending_glb_import
            .as_mut()
            .filter(|p| p.collision)
        {
            egui::Window::new("Duplicate GLB package")
                .collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.colored_label(egui::Color32::YELLOW, "Duplicate package");
                    ui.label("An Actor or Character package with this name already exists in this project.");
                    ui.small("Replace updates the existing package. Rename keeps both.");
                    ui.horizontal(|ui| {
                        ui.label("New package name:");
                        ui.text_edit_singleline(&mut pending.group_name);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Replace").clicked() { glb_collision_action = Some(GlbCollisionAction::Replace); }
                        if ui.add_enabled(valid_actor_group_name(&pending.group_name),
                            egui::Button::new("Rename")).clicked() { glb_collision_action = Some(GlbCollisionAction::Rename); }
                        if ui.button("Cancel").clicked() { glb_collision_action = Some(GlbCollisionAction::Cancel); }
                    });
                });
        } else if let Some(pending) = self
            .asset_manager
            .pending_folder_import
            .as_mut()
            .filter(|p| p.collision)
        {
            egui::Window::new("Duplicate GLB package")
                .collapsible(false).resizable(false)
                .anchor(egui::Align2::CENTER_CENTER, egui::Vec2::ZERO)
                .show(ctx, |ui| {
                    ui.colored_label(egui::Color32::YELLOW, "Duplicate package");
                    ui.label("One or more Actor packages in this folder already exist in the project.");
                    ui.small("Replace updates matching packages. Rename imports the pack under a new name.");
                    ui.horizontal(|ui| {
                        ui.label("New package name:");
                        ui.text_edit_singleline(&mut pending.group_name);
                    });
                    ui.horizontal(|ui| {
                        if ui.button("Replace").clicked() { folder_collision_action = Some(GlbCollisionAction::Replace); }
                        if ui.add_enabled(valid_actor_group_name(&pending.group_name),
                            egui::Button::new("Rename")).clicked() { folder_collision_action = Some(GlbCollisionAction::Rename); }
                        if ui.button("Cancel").clicked() { folder_collision_action = Some(GlbCollisionAction::Cancel); }
                    });
                });
        }

        if let Some(path) = self.asset_manager.open_script_request.take() {
            if self.open_managed_aeo_script(project_path, path) {
                close_page = true;
            }
        }
        if close_page {
            open = false;
        }
        self.asset_manager.open = open;
        if !open {
            self.asset_manager.was_open = false;
        }
        if let Some(package) = place_actor_package {
            self.build_actor_package = Some(package);
            self.build_is_actor = true;
            self.activate_editor_tool(EditorTool::Build);
            self.asset_manager.open = false;
            self.asset_manager.was_open = false;
            ctx.request_repaint();
        }
        if let Some(package) = select_player_package {
            match crate::project::character_asset::assign_to_world(project_path, &package, world) {
                Ok(()) => {
                    self.needs_save = true;
                }
                Err(error) => {
                    self.asset_manager.status =
                        Some((format!("Cannot assign Character: {error}"), false));
                    return;
                }
            }
            self.asset_manager.status = Some((
                format!(
                    "'{package}' selected as Player Character. Save the project to keep this choice."
                ),
                true,
            ));
            ctx.request_repaint();
        }

        let completed = self
            .asset_manager
            .background_import
            .as_ref()
            .and_then(|receiver| receiver.lock().ok()?.try_recv().ok());
        if let Some((source, result)) = completed {
            self.asset_manager.background_import = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok(path) => {
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                    renderer.set_project_asset_root(Some(project_path));
                    self.asset_manager.status =
                        Some((format!("Imported {}", path.display()), true));
                }
                Err(error) if error.starts_with("Asset already exists:") => {
                    if self.asset_manager.pending_media_collisions.is_empty() {
                        self.asset_manager.incoming_media_name = source
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned();
                    }
                    self.asset_manager
                        .pending_media_collisions
                        .push_back(source);
                }
                Err(error) => {
                    self.script_editor
                        .push_output(format!("Import failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if self.asset_manager.background_import.is_none()
            && self.asset_manager.import_progress_name.is_none()
        {
            if let Some((source, name, replace)) = self.asset_manager.queued_media.pop_front() {
                let project = project_path.to_path_buf();
                let (send, recv) = std::sync::mpsc::channel();
                self.asset_manager.background_import =
                    Some(std::sync::Arc::new(std::sync::Mutex::new(recv)));
                self.asset_manager.import_progress_name = Some(
                    source
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
                self.asset_manager.import_progress = 0.08;
                self.asset_manager.status = Some((
                    format!("Importing {} in background...", source.display()),
                    true,
                ));
                std::thread::spawn(move || {
                    let result =
                        asset_import::import_media(&project, &source, name.as_deref(), true);
                    let _ = send.send((source, result));
                });
            }
        }
        if self.asset_manager.background_import.is_some()
            || self.asset_manager.glb_inspection.is_some()
            || self.asset_manager.glb_job.is_some()
            || self.asset_manager.folder_job.is_some()
            || self.asset_manager.folder_media_job.is_some()
            || self.asset_manager.legacy_folder_job.is_some()
        {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(id) = replace_canonical_asset {
            if let Some(source) = rfd::FileDialog::new().pick_file() {
                let result = crate::project::asset_registry::AssetRegistry::load(project_path)
                    .and_then(|registry| {
                        let kind = registry.find(&id).ok_or("Unknown AssetID")?.kind.clone();
                        if kind == crate::project::asset_registry::AssetType::Actor {
                            crate::project::gltf_import::replace_registered_actor(
                                project_path,
                                &id,
                                &source,
                            )
                            .map(|_| ())
                        } else {
                            crate::project::asset_registry::AssetRegistry::replace_file(
                                project_path,
                                &id,
                                &source,
                            )
                            .map(|_| ())
                        }
                    });
                match result {
                    Ok(()) => {
                        self.asset_manager.status =
                            Some(("Asset file replaced; identity preserved.".into(), true));
                        refresh_manager_catalog(&mut self.asset_manager, project_path);
                        self.shell.refresh(Some(project_path));
                    }
                    Err(error) => self.asset_manager.status = Some((error, false)),
                }
            }
        }
        if let Some(selection) = inventory_add_request {
            self.asset_manager.inventory_pending_pack = None;
            self.asset_manager.inventory_pack_project = None;
            self.asset_manager.inventory_pending_collection = None;
            self.asset_manager.canonical_import_plan = None;
            self.asset_manager.canonical_import_rename.clear();
            self.asset_manager.canonical_import_rename_error = None;
            self.asset_manager.canonical_import_plan_project = None;
            match selection {
                InventorySelection::Imported(id) => {
                    match prepare_inventory_project_import(project_path, &id) {
                        Ok(plan) => {
                            if plan.assets.is_empty() {
                                self.asset_manager.status = Some((
                                    "No supported assets found. GLTF models must be converted to GLB before project import.".into(),
                                    false,
                                ));
                            } else {
                                self.asset_manager.inventory_pending_collection = Some(id);
                                self.asset_manager.canonical_import_plan_project =
                                    Some(project_path.to_owned());
                                self.asset_manager.canonical_import_rename.clear();
                                self.asset_manager.canonical_import_rename_error = None;
                                self.asset_manager.canonical_import_plan = Some(plan);
                                self.asset_manager.status = None;
                            }
                        }
                        Err(error) => self.asset_manager.status = Some((error, false)),
                    }
                }
                InventorySelection::Pack(reference) => {
                    let root = crate::project::asset_packs::cache_root();
                    let result = if !crate::project::asset_packs::installed(&root, &reference) {
                        Err(
                            "Selected catalogue pack is not installed or failed verification"
                                .to_owned(),
                        )
                    } else {
                        crate::project::asset_packs::references(project_path).and_then(|refs| {
                            if refs.contains(&reference) {
                                Err("This pack is already installed in this project".into())
                            } else if refs.iter().any(|r| r.id == reference.id) {
                                Err("Another version of this pack is installed in this project"
                                    .into())
                            } else {
                                Ok(())
                            }
                        })
                    };
                    match result {
                        Ok(()) => {
                            self.asset_manager.inventory_pack_project =
                                Some(project_path.to_owned());
                            self.asset_manager.inventory_pending_pack = Some(reference);
                            self.asset_manager.status = None;
                        }
                        Err(error) => self.asset_manager.status = Some((error, false)),
                    }
                }
            }
        }
        if inventory_cancel_pack {
            self.asset_manager.inventory_pending_pack = None;
            self.asset_manager.inventory_pack_project = None;
        }
        if inventory_confirm_pack {
            if self.asset_manager.inventory_pack_project.as_deref() == Some(project_path) {
                if let Some(reference) = self.asset_manager.inventory_pending_pack.take() {
                    let result = crate::project::shared_inventory::add_installed_pack_to_project(
                        project_path,
                        &reference,
                    );
                    match result {
                        Ok(()) => {
                            self.asset_manager.status =
                                Some(("Pack installed in project.".into(), true));
                            refresh_manager_catalog(&mut self.asset_manager, project_path);
                            self.shell.refresh(Some(project_path));
                        }
                        Err(error) => self.asset_manager.status = Some((error, false)),
                    }
                    self.asset_manager.inventory_pack_project = None;
                }
            } else {
                self.asset_manager.inventory_pending_pack = None;
                self.asset_manager.inventory_pack_project = None;
            }
        }
        if choose_import_files || choose_import_folders {
            if self.asset_manager.canonical_import_job.is_some() {
                self.asset_manager.status =
                    Some(("Complete the current import first.".into(), false));
            } else {
                let selected = if choose_import_files {
                    rfd::FileDialog::new().pick_files()
                } else {
                    rfd::FileDialog::new().pick_folders()
                };
                if let Some(sources) = selected {
                    if !sources.is_empty() {
                        match prepare_asset_import(project_path, &sources) {
                            Ok(plan) => {
                                self.asset_manager.status = None;
                                self.asset_manager.inventory_open = false;
                                self.asset_manager.inventory_import_sources = Some(sources);
                                self.asset_manager.canonical_import_plan_project =
                                    Some(project_path.to_owned());
                                self.asset_manager.canonical_import_rename.clear();
                                self.asset_manager.canonical_import_rename_error = None;
                                self.asset_manager.canonical_import_plan = Some(plan);
                            }
                            Err(error) => {
                                self.asset_manager.status = Some((error, false));
                            }
                        }
                    }
                }
            }
            ctx.request_repaint();
        }
        if cancel_canonical_import {
            self.asset_manager.canonical_import_plan = None;
            self.asset_manager.canonical_import_rename.clear();
            self.asset_manager.canonical_import_rename_error = None;
            self.asset_manager.canonical_import_plan_project = None;
            self.asset_manager.inventory_import_sources = None;
            self.asset_manager.inventory_pending_collection = None;
        }
        if begin_canonical_import
            && self.asset_manager.canonical_import_job.is_none()
            && self.asset_manager.canonical_import_plan_project.as_deref() == Some(project_path)
        {
            if let Some(plan) = self.asset_manager.canonical_import_plan.take() {
                self.asset_manager.canonical_import_plan_project = None;
                let project = project_path.to_owned();
                let inventory_collection = self.asset_manager.inventory_pending_collection.take();
                let external_sources = self.asset_manager.inventory_import_sources.take();
                self.asset_manager.inventory_project_import_running = inventory_collection.clone();
                if external_sources.is_some() {
                    self.asset_manager.inventory_snapshot = None;
                    self.asset_manager.inventory_error = None;
                }
                let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                let worker_progress = progress.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.asset_manager.canonical_import_progress = Some(progress);
                self.asset_manager.canonical_import_project = Some(project.clone());
                self.asset_manager.canonical_import_job =
                    Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                std::thread::spawn(move || {
                    let result = (|| -> Result<_, String> {
                        let mut selected_collection = inventory_collection;
                        let import_plan = if let Some(sources) = external_sources {
                            crate::project::shared_inventory::validate_project_import(
                                &project, &plan,
                            )?;
                            let item = crate::project::shared_inventory::store(&sources)?;
                            selected_collection = Some(item.id.clone());
                            let shared_sources =
                                crate::project::shared_inventory::sources(&item.id)?;
                            crate::project::asset_import_plan::rebase_selected_import(
                                &project,
                                &plan,
                                &sources,
                                &shared_sources,
                            )?
                        } else {
                            plan
                        };
                        crate::project::shared_inventory::validate_project_import(
                            &project,
                            &import_plan,
                        )?;
                        let summary =
                            apply_asset_import(&project, &import_plan, Some(&worker_progress))?;
                        if summary.failures.is_empty() {
                            if let Some(id) = selected_collection {
                                crate::project::shared_inventory::record_project_collection(&project, &id)
                                    .map_err(|e| format!(
                                        "Project assets imported, but the shared Inventory association could not be saved: {e}"
                                    ))?;
                            }
                        }
                        Ok(summary)
                    })();
                    let _ = tx.send(result);
                });
                self.asset_manager.status = Some(("Importing project assets...".into(), true));
            }
        }
        if let Some(result) = self
            .asset_manager
            .canonical_import_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.canonical_import_job = None;
            self.asset_manager.canonical_import_progress = None;
            let was_collection = self.asset_manager.inventory_project_import_running.take();
            let same_project = self
                .asset_manager
                .canonical_import_project
                .take()
                .as_deref()
                == Some(project_path);
            if !same_project {
                self.asset_manager.status = Some((
                    "Import finished in its original project; current project was not changed."
                        .into(),
                    true,
                ));
                return;
            }
            refresh_manager_catalog(&mut self.asset_manager, project_path);
            self.shell.refresh(Some(project_path));
            if was_collection.is_some() {
                self.asset_manager.inventory_snapshot = None;
                self.asset_manager.inventory_error = None;
                self.asset_manager.inventory_open = true;
            }
            match result {
                Ok(summary) => {
                    self.asset_manager.status = Some((
                        format!(
                            "Imported {} assets; {} failures; {} warnings (revision {}).",
                            summary.imported_ids.len(),
                            summary.failures.len(),
                            summary.warnings.len(),
                            summary.revision,
                        ),
                        summary.failures.is_empty(),
                    ));
                    for message in summary.failures.iter().chain(summary.warnings.iter()) {
                        self.script_editor
                            .push_output(format!("Asset import: {message}"));
                    }
                }
                Err(error) => {
                    self.asset_manager.status = Some((format!("Import failed: {error}"), false))
                }
            }
        }
        if self.asset_manager.canonical_import_job.is_some() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        if let Some(action) = media_collision_action {
            if let Some(source) = self.asset_manager.pending_media_collisions.pop_front() {
                match action {
                    MediaCollisionAction::Cancel => {}
                    MediaCollisionAction::Replace => self
                        .asset_manager
                        .queued_media
                        .push_back((source, None, true)),
                    MediaCollisionAction::Rename => self.asset_manager.queued_media.push_back((
                        source,
                        Some(self.asset_manager.incoming_media_name.clone()),
                        false,
                    )),
                }
                if let Some(next) = self.asset_manager.pending_media_collisions.front() {
                    self.asset_manager.incoming_media_name = next
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                }
            }
        }
        if cancel_folder_clicked || folder_collision_action == Some(GlbCollisionAction::Cancel) {
            self.asset_manager.pending_folder_import = None;
        }
        if let Some((source, result)) = self
            .asset_manager
            .glb_inspection
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.glb_inspection = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok(summary) => {
                    let group_name = source
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned();
                    self.asset_manager.pending_glb_import = Some(PendingGlbImport {
                        source,
                        summary,
                        group_name,
                        collision: false,
                    });
                }
                Err(error) => {
                    self.asset_manager.status =
                        Some((format!("GLB inspection failed: {error}"), false))
                }
            }
        }
        if self.asset_manager.pending_glb_import.is_none()
            && self.asset_manager.glb_inspection.is_none()
            && self.asset_manager.import_progress_name.is_none()
            && let Some(source) = self.asset_manager.pending_glb_queue.pop_front()
        {
            let (tx, rx) = std::sync::mpsc::channel();
            self.asset_manager.import_progress_name = Some(
                source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
            self.asset_manager.import_progress = 0.05;
            self.asset_manager.glb_inspection =
                Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
            std::thread::spawn(move || {
                let result = crate::project::inspect_glb(&source);
                let _ = tx.send((source, result));
            });
        }
        if self.asset_manager.pending_folder_import.is_none()
            && let Some(source) = self.asset_manager.pending_import_folders.pop_front()
        {
            let group_name = source
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();
            self.asset_manager.pending_folder_import = Some(PendingFolderImport {
                source,
                group_name,
                collision: false,
            });
        }

        if (import_folder_clicked
            || matches!(
                folder_collision_action,
                Some(GlbCollisionAction::Replace | GlbCollisionAction::Rename)
            ))
            && self.asset_manager.import_progress_name.is_none()
            && let Some(mut pending) = self.asset_manager.pending_folder_import.take()
        {
            let folder = pending.source.clone();
            if folder.join("package.json").is_file() {
                let project = project_path.to_path_buf();
                let source = folder.clone();
                let (tx, rx) = std::sync::mpsc::channel();
                self.asset_manager.legacy_folder_job =
                    Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                self.asset_manager.import_progress_name = Some(
                    folder
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
                self.asset_manager.import_progress = 0.1;
                std::thread::spawn(move || {
                    let result = crate::project::import_character_package(&project, &source);
                    let _ = tx.send(result);
                });
            } else {
                let project = project_path.to_path_buf();
                let folder_source = folder.clone();
                let name = pending.group_name.clone();
                let action = folder_collision_action;
                let (tx, rx) = std::sync::mpsc::channel();
                let progress = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
                self.asset_manager.folder_progress = Some(progress.clone());
                self.asset_manager.folder_job =
                    Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                self.asset_manager.folder_job_pending = Some(pending);
                self.asset_manager.import_progress_name = Some(
                    folder
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .into_owned(),
                );
                self.asset_manager.import_progress = 0.1;
                std::thread::spawn(move || {
                    let result = match action {
                        Some(GlbCollisionAction::Replace) => {
                            crate::project::replace_glb_folder(&project, &folder_source)
                        }
                        Some(GlbCollisionAction::Rename) => {
                            crate::project::import_glb_folder_with_progress(
                                &project,
                                &folder_source,
                                Some(&name),
                                Some(&progress),
                            )
                        }
                        _ => crate::project::import_glb_folder_with_progress(
                            &project,
                            &folder_source,
                            None,
                            Some(&progress),
                        ),
                    };
                    if let Ok(crate::project::GlbFolderImportResult::Static(imported)) = &result {
                        let names = imported
                            .packages
                            .iter()
                            .map(|p| p.package_name.clone())
                            .collect::<Vec<_>>();
                        if let Err(error) = record_actor_import_group_named(&project, &name, &names)
                        {
                            eprintln!(
                                "[Asset Import] Imported files, but failed to save pack identity: {error}"
                            );
                        }
                    }
                    let _ = tx.send(result);
                });
            }
        }
        if let Some(result) = self
            .asset_manager
            .legacy_folder_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.legacy_folder_job = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok(package_name) => {
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some((
                        format!("Imported Character package '{package_name}'; gameplay unchanged."),
                        true,
                    ));
                }
                Err(error) => {
                    self.script_editor
                        .push_output(format!("Import failed: {error}"));
                    self.asset_manager.status = Some((error, false));
                }
            }
        }
        if let Some(glb_result) = self
            .asset_manager
            .folder_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.folder_job = None;
            self.asset_manager.folder_progress = None;
            self.asset_manager.import_progress_name = None;
            let mut pending = self
                .asset_manager
                .folder_job_pending
                .take()
                .expect("pending folder import");
            let folder = pending.source.clone();
            match glb_result {
                Ok(crate::project::GlbFolderImportResult::Static(imported)) => {
                    let project = project_path.to_path_buf();
                    let media_folder = folder.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.asset_manager.folder_media_job =
                        Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                    self.asset_manager.import_progress_name = Some(
                        folder
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    );
                    self.asset_manager.import_progress = 0.1;
                    std::thread::spawn(move || {
                        let result = asset_import::import_folder_media(&project, &media_folder);
                        let _ = tx.send(result);
                    });
                    if let Some(first) = imported.packages.first() {
                        self.build_actor_package = Some(first.package_name.clone());
                    }
                    let names: Vec<String> = imported
                        .packages
                        .iter()
                        .map(|package| package.package_name.clone())
                        .collect();
                    let grouping =
                        record_actor_import_group_named(project_path, &pending.group_name, &names);
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some(match grouping {
                        Ok(()) => (
                            format!(
                                "Imported 3D pack '{}' with {} Actors.",
                                imported.source_file_name,
                                imported.packages.len()
                            ),
                            true,
                        ),
                        Err(error) => (
                            format!(
                                "Actors imported, but their package grouping could not be saved: {error}"
                            ),
                            false,
                        ),
                    });
                }
                Ok(crate::project::GlbFolderImportResult::Character(imported)) => {
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some((
                        format!(
                            "Imported rigged Actor folder beginning with '{}'; no Character was created.",
                            imported.package_name
                        ),
                        true,
                    ));
                }
                Err(error) if error.contains("contains no .glb files") => {
                    let project = project_path.to_path_buf();
                    let media_folder = folder.clone();
                    let (tx, rx) = std::sync::mpsc::channel();
                    self.asset_manager.folder_media_job =
                        Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
                    self.asset_manager.import_progress_name = Some(
                        folder
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .into_owned(),
                    );
                    self.asset_manager.import_progress = 0.1;
                    std::thread::spawn(move || {
                        let result = asset_import::import_folder_media(&project, &media_folder);
                        let _ = tx.send(result);
                    });
                }
                Err(error) if error.contains("already exists in this project") => {
                    pending.collision = true;
                    self.asset_manager.pending_folder_import = Some(pending);
                    self.asset_manager.status = Some((
                        format!(
                            "Duplicate GLB package: {error}. Choose Replace, Rename, or Cancel."
                        ),
                        false,
                    ));
                }
                Err(error) => {
                    self.asset_manager.status =
                        Some((format!("GLB folder import failed: {error}"), false));
                }
            }
            ctx.request_repaint();
        }

        if let Some(result) = self
            .asset_manager
            .folder_media_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.folder_media_job = None;
            self.asset_manager.import_progress_name = None;
            match result {
                Ok((count, errors)) => {
                    for error in &errors {
                        self.script_editor
                            .push_output(format!("Folder asset import failed: {error}"));
                    }
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                    renderer.set_project_asset_root(Some(project_path));
                    self.asset_manager.status = Some((
                        format!("Imported {count} media files; {} failed", errors.len()),
                        errors.is_empty(),
                    ));
                }
                Err(error) => self.asset_manager.status = Some((error, false)),
            }
        }
        if cancel_glb_clicked || glb_collision_action == Some(GlbCollisionAction::Cancel) {
            self.asset_manager.pending_glb_import = None;
            ctx.request_repaint();
        }

        if (import_glb_clicked
            || matches!(
                glb_collision_action,
                Some(GlbCollisionAction::Replace | GlbCollisionAction::Rename)
            ))
            && self.asset_manager.import_progress_name.is_none()
            && let Some(mut pending) = self.asset_manager.pending_glb_import.take()
        {
            use crate::project::{GlbAssetKind, GlbFolderImportResult};
            let project = project_path.to_path_buf();
            let source = pending.source.clone();
            let name = pending.group_name.clone();
            let kind = pending.summary.kind;
            let action = glb_collision_action;
            let (tx, rx) = std::sync::mpsc::channel();
            self.asset_manager.glb_job_pending = Some(pending);
            self.asset_manager.glb_job = Some(std::sync::Arc::new(std::sync::Mutex::new(rx)));
            self.asset_manager.import_progress_name = Some(
                source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
            );
            self.asset_manager.import_progress = 0.1;
            std::thread::spawn(move || {
                let result: Result<GlbFolderImportResult, String> = match action {
                    Some(GlbCollisionAction::Replace) => {
                        crate::project::replace_single_glb(&project, &source)
                    }
                    Some(GlbCollisionAction::Rename) => match kind {
                        GlbAssetKind::StaticMesh => {
                            crate::project::import_static_glb_named(&project, &source, &name)
                                .map(GlbFolderImportResult::Static)
                        }
                        GlbAssetKind::RiggedCharacter => {
                            crate::project::import_character_glb_named(
                                &project,
                                &source,
                                Some(&name),
                            )
                            .map(GlbFolderImportResult::Character)
                        }
                    },
                    _ => match kind {
                        GlbAssetKind::StaticMesh => {
                            crate::project::import_static_glb(&project, &source)
                                .map(GlbFolderImportResult::Static)
                        }
                        GlbAssetKind::RiggedCharacter => {
                            crate::project::import_character_glb(&project, &source)
                                .map(GlbFolderImportResult::Character)
                        }
                    },
                };
                if let Ok(GlbFolderImportResult::Static(imported)) = &result {
                    let names = imported
                        .packages
                        .iter()
                        .map(|p| p.package_name.clone())
                        .collect::<Vec<_>>();
                    if let Err(error) = record_actor_import_group_named(&project, &name, &names) {
                        eprintln!(
                            "[Asset Import] Imported files, but failed to save pack identity: {error}"
                        );
                    }
                }
                let _ = tx.send(result);
            });
        }
        if let Some(result) = self
            .asset_manager
            .glb_job
            .as_ref()
            .and_then(|rx| rx.lock().ok()?.try_recv().ok())
        {
            self.asset_manager.glb_job = None;
            self.asset_manager.import_progress_name = None;
            let mut pending = self
                .asset_manager
                .glb_job_pending
                .take()
                .expect("pending GLB import");
            use crate::project::GlbFolderImportResult;
            match result {
                Ok(imported) => {
                    let message = match imported {
                        GlbFolderImportResult::Static(imported) => {
                            let names: Vec<String> = imported
                                .packages
                                .iter()
                                .map(|package| package.package_name.clone())
                                .collect();
                            if let Err(error) = record_actor_import_group_named(
                                project_path,
                                &pending.group_name,
                                &names,
                            ) {
                                self.script_editor.push_output(format!(
                                    "Actor package grouping was not saved: {error}"
                                ));
                            }
                            if let Some(first) = imported.packages.first() {
                                self.build_actor_package = Some(first.package_name.clone());
                            }
                            format!(
                                "Imported {} Actor package(s) from {}: {}",
                                imported.packages.len(),
                                imported.source_file_name,
                                names.join(", ")
                            )
                        }
                        GlbFolderImportResult::Character(imported) => {
                            if let Err(error) = record_actor_import_group_named(
                                project_path,
                                &pending.group_name,
                                std::slice::from_ref(&imported.package_name),
                            ) {
                                self.script_editor.push_output(format!(
                                    "Actor package grouping was not saved: {error}"
                                ));
                            }
                            format!(
                                "Imported rigged Actor '{}' from {} ({} joints, {} clips); no Character was created",
                                imported.package_name,
                                imported.source_file_name,
                                imported.joint_count,
                                imported.clip_names.len()
                            )
                        }
                    };
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some((message, true));
                }
                Err(error) => {
                    if error.contains("already exists in this project") {
                        pending.collision = true;
                        self.asset_manager.status = Some((
                            format!(
                                "Duplicate GLB package: {error}. Choose Replace, Rename, or Cancel."
                            ),
                            false,
                        ));
                    } else {
                        self.asset_manager.status =
                            Some((format!("GLB import failed: {error}"), false));
                    }
                    self.asset_manager.pending_glb_import = Some(pending);
                    self.script_editor
                        .push_output(format!("GLB import: {error}"));
                }
            }
            ctx.request_repaint();
        }

        if refresh_clicked {
            renderer.reload_material_assets();
            refresh_manager_catalog(&mut self.asset_manager, project_path);
            self.shell.refresh(Some(project_path));
            crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
            renderer.set_project_asset_root(Some(project_path));
            self.asset_manager.status = Some(("Project asset list refreshed.".to_string(), true));
            ctx.request_repaint();
        }

        let mut catalog_changed = false;

        if let Some((rename_kind, identifier)) = self.asset_manager.rename_asset.clone() {
            let mut dialog_open = true;
            egui::Window::new("Rename / Move Asset")
                .open(&mut dialog_open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Current: {identifier}"));
                    ui.label("Destination identifier:");
                    ui.text_edit_singleline(&mut self.asset_manager.rename_target);
                    ui.small("Destination stays inside this project's current asset category.");

                    ui.horizontal(|ui| {
                        if ui.button("Apply").clicked() {
                            let affected=project_asset_usages(project_path,rename_kind,&identifier,world,&self.build_template);
                            if !affected.is_empty() {
                                self.script_editor.push_output(format!("WARNING: renaming/moving '{identifier}' affects: {}",affected.iter().map(|u|u.description.as_str()).collect::<Vec<_>>().join(", ")));
                            }
                            match rename_project_asset(
                                project_path,
                                rename_kind,
                                &identifier,
                                &self.asset_manager.rename_target,
                                world,
                                &mut self.build_template,
                                &mut self.history,
                            ) {
                                Ok((new_identifier, changed)) => {
                                    self.needs_save |= changed > 0;
                                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                                    renderer.set_project_asset_root(Some(project_path));
                                    self.asset_manager.selected_asset =
                                        Some((rename_kind, new_identifier.clone()));
                                    self.asset_manager.status = Some((
                                        format!(
                                            "Renamed '{}' to '{}'; updated {} authored reference(s).",
                                            identifier, new_identifier, changed
                                        ),
                                        true,
                                    ));
                                    self.asset_manager.rename_asset = None;
                                    catalog_changed = true;
                                }
                                Err(error) => {
                                    self.asset_manager.status = Some((error, false));
                                }
                            }
                        }
                        if ui.button("Cancel").clicked() {
                            self.asset_manager.rename_asset = None;
                        }
                    });
                });
            if !dialog_open {
                self.asset_manager.rename_asset = None;
            }
        }

        if let Some((delete_kind, identifier)) = self.asset_manager.delete_asset.clone() {
            let usages = project_asset_usages(
                project_path,
                delete_kind,
                &identifier,
                world,
                &self.build_template,
            );
            let mut dialog_open = true;
            egui::Window::new("Delete Project Asset")
                .open(&mut dialog_open)
                .collapsible(false)
                .resizable(false)
                .show(ctx, |ui| {
                    if usages.is_empty() {
                        ui.label(format!("Delete project asset '{identifier}'?"));
                        ui.label("This file operation is permanent.");
                    } else {
                        ui.label(
                            egui::RichText::new("Warning: deleting this asset leaves affected references missing.")
                                .color(egui::Color32::LIGHT_RED),
                        );
                        for usage in &usages {
                            ui.monospace(&usage.description);
                        }
                    }

                    ui.horizontal(|ui| {
                        if ui
                            .add(egui::Button::new("Delete"))
                            .clicked()
                        {
                            if !usages.is_empty() {
                                self.script_editor.push_output(format!("WARNING: deleting '{identifier}' affects: {}", usages.iter().map(|u| u.description.as_str()).collect::<Vec<_>>().join(", ")));
                            }
                            match delete_project_asset(
                                project_path,
                                delete_kind,
                                &identifier,
                                world,
                                &self.build_template,
                            ) {
                                Ok(revealed_builtin) => {
                                    crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                                    renderer.set_project_asset_root(Some(project_path));
                                    self.asset_manager.status = Some((
                                        if revealed_builtin {
                                            format!(
                                                "Deleted project asset '{}'; the engine default with the same identifier is active again.",
                                                identifier
                                            )
                                        } else {
                                            format!("Deleted project asset '{}'.", identifier)
                                        },
                                        true,
                                    ));
                                    self.asset_manager.delete_asset = None;
                                    self.asset_manager.selected_asset = None;
                                    catalog_changed = true;
                                }
                                Err(error) => {
                                    self.asset_manager.status = Some((error, false));
                                }
                            }
                        }

                        if ui
                            .button(if usages.is_empty() { "Cancel" } else { "Close" })
                            .clicked()
                        {
                            self.asset_manager.delete_asset = None;
                        }
                    });
                });

            if !dialog_open {
                self.asset_manager.delete_asset = None;
            }
        }

        if let Some((category, group)) = self.asset_manager.delete_import_group.clone() {
            let mut open = true;
            let mut confirm = false;
            let mut cancel = false;
            let count = self
                .asset_manager
                .browser_files
                .iter()
                .filter(|asset| {
                    asset.category == category
                        && asset.is_directory
                        && self
                            .asset_manager
                            .actor_import_groups
                            .get(&asset.name)
                            .map(String::as_str)
                            .unwrap_or("Legacy")
                            == group
                })
                .count();
            egui::Window::new("Delete Entire Imported Group")
                .open(&mut open).collapsible(false).resizable(false)
                .show(ctx, |ui| {
                    ui.label(format!("Delete '{group}' and all {count} imported {category} packages?"));
                    ui.label("This removes the entire imported collection, not just its visual folder.");
                    ui.label("If any package is used in a saved World or another asset, nothing will be deleted.");
                    ui.horizontal(|ui| {
                        if ui.button("Delete All Packages").clicked() { confirm = true; }
                        if ui.button("Cancel").clicked() { cancel = true; }
                    });
                });
            if confirm {
                self.asset_manager.browser_preview = None;
                self.asset_manager.browser_preview_loaded = None;
                self.asset_manager.browser_thumbnails.clear();
                self.asset_manager.browser_thumbnail_order.clear();
                match delete_visual_import_group(project_path, &category, &group) {
                    Ok(count) => {
                        self.asset_manager.status = Some((
                            format!("Deleted {count} packages in imported group '{group}'."),
                            true,
                        ));
                        self.asset_manager.delete_import_group = None;
                        self.asset_manager.browser_selected = None;
                        self.asset_manager.browser_folder.clear();
                        catalog_changed = true;
                        renderer.set_project_asset_root(Some(project_path));
                        crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
                    }
                    Err(error) => {
                        self.asset_manager.status = Some((error, false));
                        self.asset_manager.delete_import_group = None;
                    }
                }
            } else if cancel || !open {
                self.asset_manager.delete_import_group = None;
            }
        }

        if catalog_changed {
            refresh_manager_catalog(&mut self.asset_manager, project_path);
            self.shell.refresh(Some(project_path));
            crate::editor::tools::refresh_asset_catalogs(ctx, project_path);
            ctx.request_repaint();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn malformed_actor_groups_cannot_be_replaced_by_rename_or_delete() {
        let root = temp_root("corrupt_actor_groups");
        let path = actor_import_groups_path(&root);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        let invalid = b"{ invalid Actor grouping JSON";
        fs::write(&path, invalid).unwrap();
        assert!(rename_visual_import_group(&root, "Old", "New", &["Actor".to_string()]).is_err());
        assert!(with_actor_groups_removed(&root, &["Actor".to_string()], || Ok(())).is_err());
        assert_eq!(fs::read(&path).unwrap(), invalid);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn rigged_import_stays_actor_until_explicit_character_config_exists() {
        let root = temp_root("explicit_character_count");
        let actor = root.join(".assets/entities/RiggedHero");
        fs::create_dir_all(&actor).unwrap();
        fs::write(
            actor.join("package.json"),
            br#"{"name":"RiggedHero","version":1}"#,
        )
        .unwrap();
        fs::write(actor.join("source.glb"), b"glb").unwrap();
        fs::write(
            actor.join("character-package.json"),
            br#"{"name":"RiggedHero"}"#,
        )
        .unwrap();
        let mut state = AssetManagerState::default();
        refresh_manager_catalog(&mut state, &root);
        assert_eq!(state.actor_package_indices.len(), 1);
        assert_eq!(state.character_package_indices.len(), 0);

        let character = root.join(".assets/characters/Hero");
        fs::create_dir_all(&character).unwrap();
        fs::write(
            character.join("package.json"),
            serde_json::to_vec(&crate::project::character_asset::CharacterAsset {
                version: 1,
                name: "Hero".into(),
                source_actor: "RiggedHero".into(),
                scale: [1.0; 3],
                material: None,
                controller: None,
                camera: None,
            })
            .unwrap(),
        )
        .unwrap();
        refresh_manager_catalog(&mut state, &root);
        assert_eq!(state.actor_package_indices.len(), 1);
        assert_eq!(state.character_package_indices.len(), 1);
        fs::remove_dir_all(root).ok();
    }
    #[test]
    fn wo_asm_001_models_is_not_an_asset_type() {
        assert!(!BROWSER_CATEGORIES.iter().any(|(name, _)| *name == "Models"));
        assert!(BROWSER_CATEGORIES.iter().any(|(name, _)| *name == "Actors"));
        assert!(
            BROWSER_CATEGORIES
                .iter()
                .any(|(name, _)| *name == "Characters")
        );
        assert!(
            BROWSER_CATEGORIES
                .iter()
                .any(|(name, _)| *name == "Materials")
        );
    }

    #[test]
    fn wkasset011_click_opens_aeo_in_script_workspace_only() {
        let root = temp_root("click_aeo_in_manager");
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join(".assets/scripts")).unwrap();
        let script = root.join("scripts/start.aeo");
        let extra_script = root.join(".assets/scripts/helper.aeo");
        let other = root.join("scripts/notes.txt");
        fs::write(&script, "print('opened')").unwrap();
        fs::write(&extra_script, "print('asset opened')").unwrap();
        fs::write(&other, "keep closed").unwrap();
        let mut editor = Editor::new();
        editor.asset_manager.open = true;
        assert!(!editor.open_managed_aeo_script(&root, other.clone()));
        assert!(editor.script_editor.open_documents.is_empty());
        assert!(editor.open_managed_aeo_script(&root, script.clone()));
        assert!(editor.script_editor.open_documents.contains_key(&script));
        assert_eq!(
            editor.shell.workspace,
            crate::editor::editor::shell::Workspace::Script
        );
        assert!(!editor.asset_manager.open);
        assert!(editor.open_managed_aeo_script(&root, extra_script.clone()));
        assert!(
            editor
                .script_editor
                .open_documents
                .contains_key(&extra_script)
        );
        let other_project = root.with_extension("other_project");
        fs::create_dir_all(other_project.join("scripts")).unwrap();
        let foreign = other_project.join("scripts/outside.aeo");
        fs::write(&foreign, "print('outside')").unwrap();
        assert!(!editor.open_managed_aeo_script(&root, foreign.clone()));
        assert!(!editor.script_editor.open_documents.contains_key(&foreign));
        fs::remove_dir_all(other_project).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn wkasset011_controller_and_camera_aeo_tiles_open_in_script_editor() {
        let root = temp_root("controller_camera_open");
        let controllers = root.join(".assets/controllers");
        let cameras = root.join(".assets/cameras");
        let camera_subfolder = cameras.join("Custom");
        let controller_package = controllers.join("tps_controller");
        let scripts = root.join("scripts/systems");
        fs::create_dir_all(&controller_package).unwrap();
        fs::create_dir_all(&camera_subfolder).unwrap();
        fs::create_dir_all(&scripts).unwrap();
        let controller = controller_package.join("tps_controller.aeo");
        let camera = camera_subfolder.join("orbit_camera.aeo");
        let ignored = controllers.join("readme.txt");
        fs::write(&controller, "print('controller')").unwrap();
        fs::write(&camera, "print('camera')").unwrap();
        fs::write(&ignored, "not a script").unwrap();
        fs::write(controller_package.join("package.json"), "{}").unwrap();
        fs::write(camera_subfolder.join("package.json"), "{}").unwrap();
        let standalone = scripts.join("bootstrap.aeo");
        fs::write(&standalone, "print('bootstrap')").unwrap();
        crate::project::asset_registry::AssetRegistry::reconcile(&root).unwrap();

        let indexed = scan_browser_files(&root);
        let actual = [
            ("Controllers", controller.clone()),
            ("Cameras", camera.clone()),
            ("Scripts", standalone.clone()),
        ];
        for (kind, file) in &actual {
            let asset = indexed
                .iter()
                .find(|asset| asset.category == *kind && asset.path == *file)
                .expect("Source script must be indexed in its own category");
            assert!(openable_aeo_asset(&root, asset));
        }
        assert!(
            indexed.iter().all(|asset| asset.path != ignored),
            "Companion TXT files must not clutter Controller/Camera assets"
        );
        assert!(!openable_aeo_asset(
            &root,
            &BrowserAsset {
                category: "Images",
                name: "camera.aeo".into(),
                path: camera.clone(),
                bytes: 0,
                is_directory: false,
            }
        ));

        let mut editor = Editor::new();
        for (_, script) in &actual {
            editor.asset_manager.open = true;
            assert!(editor.open_managed_aeo_script(&root, script.clone()));
            assert_eq!(
                editor.shell.workspace,
                crate::editor::editor::shell::Workspace::Script,
            );
            assert!(editor.script_editor.open_documents.contains_key(script));
            assert!(!editor.asset_manager.open);
        }
        assert_eq!(editor.script_editor.open_documents.len(), 3);
        assert!(!editor.open_managed_aeo_script(&root, ignored));
        assert_eq!(editor.script_editor.open_documents.len(), 3);
        fs::remove_dir_all(root).unwrap();
    }

    use crate::editor::editor::History;
    use crate::world::{CellType, TerrainMaterials};

    #[test]
    fn unfinished_glb_staging_folders_are_not_legacy_actors() {
        let root = temp_root("hidden_staging");
        let _ = fs::remove_dir_all(&root);
        let entities = root.join(".assets/entities");
        fs::create_dir_all(entities.join(".Broken.worldkiln-glb-import-100-0")).unwrap();
        fs::create_dir_all(entities.join("Pirate_Crate")).unwrap();
        record_actor_import_group_named(&root, "Pirate Pack", &["Pirate_Crate".into()]).unwrap();
        let mut state = AssetManagerState::default();
        refresh_manager_catalog(&mut state, &root);
        assert_eq!(
            state
                .browser_files
                .iter()
                .filter(|entry| entry.category == "Actors"
                    && entry.is_directory
                    && !entry.name.contains('/'))
                .count(),
            1
        );
        assert_eq!(
            state
                .actor_import_groups
                .get("Pirate_Crate")
                .map(String::as_str),
            Some("Pirate Pack")
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn asset_catalog_does_not_truncate_large_folders() {
        let root = temp_root("large_folder");
        let textures = root.join(".assets/textures/nested");
        fs::create_dir_all(&textures).unwrap();
        fs::create_dir_all(textures.join("empty")).unwrap();
        for i in 0..501 {
            fs::write(textures.join(format!("tex_{i}.png")), b"png").unwrap();
        }
        let assets = scan_browser_files(&root);
        assert_eq!(assets.iter().filter(|a| !a.is_directory).count(), 501);
        assert!(
            assets
                .iter()
                .any(|a| a.is_directory && a.name == "nested/empty"),
            "indexed directories: {:?}",
            assets
                .iter()
                .filter(|a| a.is_directory)
                .map(|a| &a.name)
                .collect::<Vec<_>>()
        );
        assert_eq!(
            browser_visible_indices(&assets, "Textures", "nested", "").len(),
            501
        );
        assert!(browser_visible_indices(&assets, "Textures", "", "").is_empty());
        assert_eq!(
            browser_visible_indices(&assets, "Textures", "", "tex_500").len(),
            1
        );
    }

    #[test]
    fn imported_png_images_are_not_classified_as_textures() {
        let root = temp_root("png_image_classification");
        let incoming = root.join("incoming");
        fs::create_dir_all(&incoming).unwrap();
        for name in ["dirt.png", "grass.png", "promo.png"] {
            let source = incoming.join(name);
            image::RgbaImage::from_pixel(2, 2, image::Rgba([80, 120, 70, 255]))
                .save(&source)
                .unwrap();
            let destination =
                super::asset_import::import_media(&root, &source, None, false).unwrap();
            assert_eq!(destination, root.join(".assets/media").join(name));
        }
        let assets = scan_browser_files(&root);
        assert_eq!(
            assets
                .iter()
                .filter(|asset| asset.category == "Images" && !asset.is_directory)
                .count(),
            3
        );
        assert_eq!(
            assets
                .iter()
                .filter(|asset| asset.category == "Textures" && !asset.is_directory)
                .count(),
            0
        );
        assert_eq!(
            browser_visible_indices(&assets, "Images", "", "grass").len(),
            1
        );
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn catalog_sorts_media_by_type() {
        let root = temp_root("media_types");
        let media = root.join(".assets/media");
        fs::create_dir_all(&media).unwrap();
        fs::write(media.join("intro.mp4"), b"mp4").unwrap();
        fs::write(media.join("icon.png"), b"png").unwrap();
        let assets = scan_browser_files(&root);
        assert!(assets.iter().any(|a| a.category == "Video"));
        assert!(assets.iter().any(|a| a.category == "Images"));
        assert_eq!(browser_visible_indices(&assets, "Video", "", "").len(), 1);
        assert_eq!(
            browser_visible_indices(&assets, "Images", "", "icon").len(),
            1
        );
        assert!(browser_visible_indices(&assets, "Video", "", "icon").is_empty());
    }

    #[test]
    fn actor_import_package_identity_survives_reload_and_preserves_existing_groups() {
        let project = temp_root("actor_pack_identity");
        let _ = fs::remove_dir_all(&project);
        fs::create_dir_all(project.join(".assets")).unwrap();
        record_actor_import_group(&project, "Pirates.glb", &["boat".into(), "sail".into()])
            .unwrap();
        record_actor_import_group(&project, "Trees", &["oak".into()]).unwrap();
        let saved = read_actor_import_groups(&project).unwrap();
        assert_eq!(saved.get("boat").map(String::as_str), Some("Pirates"));
        assert_eq!(saved.get("sail").map(String::as_str), Some("Pirates"));
        assert_eq!(saved.get("oak").map(String::as_str), Some("Trees"));
        assert!(saved.get("historical").is_none());
        let _ = fs::remove_dir_all(project);
    }

    fn temp_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "WorldkilnAssetManagement_{}_{}",
            name,
            std::process::id()
        ))
    }

    fn create_roots(name: &str) -> (PathBuf, PathBuf, PathBuf) {
        let root = temp_root(name);
        let _ = fs::remove_dir_all(&root);
        let project = root.join("project");
        let builtin = root.join("builtin");
        fs::create_dir_all(project.join(".assets/textures")).unwrap();
        fs::create_dir_all(project.join(".assets/audio")).unwrap();
        fs::create_dir_all(builtin.join("textures")).unwrap();
        fs::create_dir_all(builtin.join("audio")).unwrap();
        (root, project, builtin)
    }

    #[test]
    fn imported_package_picker_indexes_project_folders_without_loading_glb_content() {
        let (root, project, _) = create_roots("imported_package_picker");
        let actor_dir = project.join(".assets/entities/NormalTree_1");
        let character_dir = project.join(".assets/characters/Rogue");
        fs::create_dir_all(&actor_dir).unwrap();
        fs::create_dir_all(&character_dir).unwrap();
        fs::write(actor_dir.join("package.json"), b"{}").unwrap();
        fs::write(character_dir.join("package.json"), b"{}").unwrap();
        fs::write(actor_dir.join("source.glb"), b"not a glb").unwrap();
        fs::write(character_dir.join("source.glb"), b"not a glb").unwrap();
        let actors = scan_project_package_names(
            &project,
            "entities",
            &["NormalTree_1".into(), "engine_cube".into()],
        );
        let characters =
            scan_project_package_names(&project, "characters", &["Rogue".into(), "Rogue".into()]);
        assert_eq!(actors, vec!["NormalTree_1"]);
        assert_eq!(characters, vec!["Rogue"]);
        assert!(scan_project_package_names(&project, "missing", &[]).is_empty());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn manager_refresh_catalog_matches_project_scan_without_double_directory_walk() {
        let (root, project, builtin) = create_roots("one_pass");
        fs::create_dir_all(project.join(".assets/textures/sub")).unwrap();
        fs::write(project.join(".assets/textures/sub/stone.png"), b"png").unwrap();
        fs::write(project.join(".assets/audio/click.wav"), b"wav").unwrap();
        fs::create_dir_all(project.join(".assets/materials")).unwrap();
        fs::write(project.join(".assets/materials/sky.json"), b"{}").unwrap();
        fs::create_dir_all(builtin.join("textures/sub")).unwrap();
        fs::write(builtin.join("textures/sub/stone.png"), b"builtin").unwrap();
        for i in 0..400 {
            fs::write(
                project.join(format!(".assets/textures/sub/batch_{i}.png")),
                b"png",
            )
            .unwrap();
            fs::write(project.join(format!(".assets/audio/batch_{i}.wav")), b"wav").unwrap();
        }
        let old_start = std::time::Instant::now();
        let prior = scan_project_assets_with_roots(
            &project,
            &builtin.join("textures"),
            &builtin.join("audio"),
        );
        let _old_browser = scan_browser_files_with_pack_roots(&project, &[]);
        let old_elapsed = old_start.elapsed();
        let new_start = std::time::Instant::now();
        let browser = scan_browser_files_with_pack_roots(&project, &[]);
        let derived = managed_catalog_from_browser(
            &project,
            &browser,
            &builtin.join("textures"),
            &builtin.join("audio"),
        );
        let new_elapsed = new_start.elapsed();
        println!(
            "WK-FS asset catalogue 803 assets: old={}ms new={}ms",
            old_elapsed.as_millis(),
            new_elapsed.as_millis()
        );
        let key = |a: &ManagedAsset| {
            (
                a.kind,
                a.identifier.clone(),
                a.size_bytes,
                a.overrides_builtin,
            )
        };
        assert_eq!(
            derived.iter().map(key).collect::<Vec<_>>(),
            prior.iter().map(key).collect::<Vec<_>>()
        );
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_asset_discovery_excludes_builtins_and_marks_project_overrides() {
        let (root, project, builtin) = create_roots("discovery");
        fs::write(builtin.join("textures/stone.png"), b"builtin").unwrap();
        fs::write(builtin.join("textures/engine_only.png"), b"builtin").unwrap();
        fs::write(project.join(".assets/textures/stone.png"), b"project").unwrap();
        fs::write(project.join(".assets/audio/click.wav"), b"wav").unwrap();

        let assets = scan_project_assets_with_roots(
            &project,
            &builtin.join("textures"),
            &builtin.join("audio"),
        );

        assert_eq!(assets.len(), 2);
        let project_stone = assets
            .iter()
            .find(|asset| asset.identifier == "stone" && asset.kind == ManagedAssetKind::Texture)
            .unwrap();
        assert!(project_stone.overrides_builtin);
        assert!(assets.iter().any(|asset| {
            asset.identifier == "click.wav" && asset.kind == ManagedAssetKind::Audio
        }));
        assert!(!assets.iter().any(|asset| asset.identifier == "engine_only"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn unreferenced_texture_and_audio_rename_move_within_category() {
        let (root, project, _) = create_roots("unreferenced_rename");
        fs::write(project.join(".assets/textures/plain.png"), b"texture").unwrap();
        fs::write(project.join(".assets/audio/plain.wav"), b"audio").unwrap();

        let mut world = World::new();
        let mut build = Cell::new_block();
        let mut history = History::new();

        let (texture_id, texture_changes) = rename_project_asset(
            &project,
            ManagedAssetKind::Texture,
            "plain",
            "environment/plain_moved",
            &mut world,
            &mut build,
            &mut history,
        )
        .unwrap();
        assert_eq!(texture_id, "environment/plain_moved");
        assert_eq!(texture_changes, 0);
        assert!(
            project
                .join(".assets/textures/environment/plain_moved.png")
                .is_file()
        );

        let (audio_id, audio_changes) = rename_project_asset(
            &project,
            ManagedAssetKind::Audio,
            "plain.wav",
            "sfx/plain_moved",
            &mut world,
            &mut build,
            &mut history,
        )
        .unwrap();
        assert_eq!(audio_id, "sfx/plain_moved.wav");
        assert_eq!(audio_changes, 0);
        assert!(project.join(".assets/audio/sfx/plain_moved.wav").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn rename_rejects_collision_and_path_traversal_without_mutation() {
        let (root, project, _) = create_roots("rename_validation");
        fs::write(project.join(".assets/textures/a.png"), b"a").unwrap();
        fs::write(project.join(".assets/textures/b.png"), b"b").unwrap();
        let mut world = World::new();
        let mut build = Cell::new_block();
        let mut history = History::new();

        assert!(
            rename_project_asset(
                &project,
                ManagedAssetKind::Texture,
                "a",
                "b",
                &mut world,
                &mut build,
                &mut history,
            )
            .is_err()
        );
        assert!(
            rename_project_asset(
                &project,
                ManagedAssetKind::Texture,
                "a",
                "../outside",
                &mut world,
                &mut build,
                &mut history,
            )
            .is_err()
        );
        assert!(project.join(".assets/textures/a.png").is_file());
        assert!(project.join(".assets/textures/b.png").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_rename_validation_leaves_file_and_authored_references_unchanged() {
        let (root, project, _) = create_roots("failed_rename_integrity");
        fs::write(project.join(".assets/audio/old.wav"), b"old").unwrap();
        fs::write(project.join(".assets/audio/taken.wav"), b"taken").unwrap();

        let mut world = World::new();
        let id = world.add_source(CellType::AudioEmitter, glam::Vec3::ZERO);
        world.source_mut(id).unwrap().cell.audio = "old.wav".to_string();
        let mut build = Cell::new_audio_emitter();
        build.audio = "old.wav".to_string();
        let mut history = History::new();

        let error = rename_project_asset(
            &project,
            ManagedAssetKind::Audio,
            "old.wav",
            "taken.wav",
            &mut world,
            &mut build,
            &mut history,
        )
        .unwrap_err();

        assert!(error.contains("already exists"));
        assert!(project.join(".assets/audio/old.wav").is_file());
        assert!(project.join(".assets/audio/taken.wav").is_file());
        assert_eq!(world.source(id).unwrap().cell.audio, "old.wav");
        assert_eq!(build.audio, "old.wav");

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn referenced_texture_rename_updates_cells_sources_terrain_build_and_history() {
        let (root, project, _) = create_roots("texture_references");
        fs::write(project.join(".assets/textures/old.png"), b"texture").unwrap();

        let mut world = World::new();
        let coord = WorldCoord::new(1, 2, 3);
        world.set_cell(coord, CellType::Block);
        world.get_mut(coord).unwrap().texture = "old".to_string();
        let source_id = world.add_source(CellType::Light, glam::Vec3::ZERO);
        world.source_mut(source_id).unwrap().cell.texture = "old.png".to_string();
        world.terrain.config.materials = TerrainMaterials {
            surface: "old".to_string(),
            subsurface: String::new(),
            rock: "old.png".to_string(),
            water: String::new(),
        };

        let mut build = Cell::new_block();
        build.texture = "old".to_string();

        let mut history = History::new();
        history.push(
            world
                .iter_authored_cells()
                .map(|(coord, cell)| (coord, cell.clone()))
                .collect(),
            world
                .iter_sources()
                .map(|source| (source.cell.id, source.clone()))
                .collect(),
            Vec::new(),
            std::collections::HashMap::new(),
            Vec::new(),
            crate::project::ProjectHierarchy::new(),
            None,
        );

        let (new_identifier, changed) = rename_project_asset(
            &project,
            ManagedAssetKind::Texture,
            "old",
            "terrain/new",
            &mut world,
            &mut build,
            &mut history,
        )
        .unwrap();

        assert_eq!(new_identifier, "terrain/new");
        assert_eq!(changed, 5);
        assert!(!project.join(".assets/textures/old.png").exists());
        assert!(project.join(".assets/textures/terrain/new.png").is_file());
        assert_eq!(world.get(coord).unwrap().texture, "terrain/new");
        assert_eq!(
            world.source(source_id).unwrap().cell.texture,
            "terrain/new.png"
        );
        assert_eq!(world.terrain.config.materials.surface, "terrain/new");
        assert_eq!(world.terrain.config.materials.rock, "terrain/new.png");
        assert_eq!(build.texture, "terrain/new");
        assert_eq!(
            history.undo_stack[0].0.get(&coord).unwrap().texture,
            "terrain/new"
        );

        let catalog = scan_project_assets_with_roots(
            &project,
            &root.join("builtin/textures"),
            &root.join("builtin/audio"),
        );
        assert!(catalog.iter().any(|asset| {
            asset.kind == ManagedAssetKind::Texture && asset.identifier == "terrain/new"
        }));
        assert!(
            !catalog.iter().any(|asset| {
                asset.kind == ManagedAssetKind::Texture && asset.identifier == "old"
            })
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn referenced_audio_rename_updates_cells_sources_build_and_history() {
        let (root, project, _) = create_roots("audio_references");
        fs::write(project.join(".assets/audio/old.wav"), b"audio").unwrap();

        let mut world = World::new();
        let coord = WorldCoord::new(4, 5, 6);
        world.set_cell(coord, CellType::AudioEmitter);
        world.get_mut(coord).unwrap().audio = "old.wav".to_string();
        let source_id = world.add_source(CellType::AudioEmitter, glam::Vec3::ZERO);
        world.source_mut(source_id).unwrap().cell.audio = "old.wav".to_string();

        let mut build = Cell::new_audio_emitter();
        build.audio = "old.wav".to_string();

        let mut history = History::new();
        history.push(
            world
                .iter_authored_cells()
                .map(|(coord, cell)| (coord, cell.clone()))
                .collect(),
            world
                .iter_sources()
                .map(|source| (source.cell.id, source.clone()))
                .collect(),
            Vec::new(),
            std::collections::HashMap::new(),
            Vec::new(),
            crate::project::ProjectHierarchy::new(),
            None,
        );

        let (_, changed) = rename_project_asset(
            &project,
            ManagedAssetKind::Audio,
            "old.wav",
            "sfx/new",
            &mut world,
            &mut build,
            &mut history,
        )
        .unwrap();

        assert_eq!(changed, 3);
        assert!(project.join(".assets/audio/sfx/new.wav").is_file());
        assert_eq!(world.get(coord).unwrap().audio, "sfx/new.wav");
        assert_eq!(world.source(source_id).unwrap().cell.audio, "sfx/new.wav");
        assert_eq!(build.audio, "sfx/new.wav");
        assert_eq!(
            history.undo_stack[0].0.get(&coord).unwrap().audio,
            "sfx/new.wav"
        );

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn delete_referenced_project_asset_preserves_authored_references_for_warning() {
        let (root, project, _) = create_roots("delete");
        fs::write(project.join(".assets/audio/used.wav"), b"used").unwrap();
        fs::write(project.join(".assets/audio/free.wav"), b"free").unwrap();

        let mut world = World::new();
        let id = world.add_source(CellType::AudioEmitter, glam::Vec3::ZERO);
        world.source_mut(id).unwrap().cell.audio = "used.wav".to_string();
        let build = Cell::new_block();

        let usage = project_asset_usages(
            &project,
            ManagedAssetKind::Audio,
            "used.wav",
            &world,
            &build,
        );
        assert!(
            usage
                .iter()
                .any(|usage| usage.description.contains("Source #"))
        );
        delete_project_asset(
            &project,
            ManagedAssetKind::Audio,
            "used.wav",
            &world,
            &build,
        )
        .unwrap();
        assert!(!project.join(".assets/audio/used.wav").exists());
        assert_eq!(world.source(id).unwrap().cell.audio, "used.wav");

        let revealed = delete_project_asset(
            &project,
            ManagedAssetKind::Audio,
            "free.wav",
            &world,
            &build,
        )
        .unwrap();
        assert!(!revealed);
        assert!(!project.join(".assets/audio/free.wav").exists());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn deleting_project_override_reports_builtin_fallback_and_catalog_drops_project_entry() {
        let (root, project, builtin) = create_roots("override_fallback");
        fs::write(builtin.join("textures/stone.png"), b"builtin").unwrap();
        fs::write(project.join(".assets/textures/stone.png"), b"project").unwrap();

        let world = World::new();
        let build = Cell::new_block();
        let revealed = delete_project_asset_with_builtin_root(
            &project,
            &builtin.join("textures"),
            ManagedAssetKind::Texture,
            "stone",
            &world,
            &build,
        )
        .unwrap();
        assert!(revealed);

        let assets = scan_project_assets_with_roots(
            &project,
            &builtin.join("textures"),
            &builtin.join("audio"),
        );
        assert!(!assets.iter().any(|asset| asset.identifier == "stone"));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn built_in_only_asset_cannot_be_renamed_or_deleted_through_project_operations() {
        let (root, project, builtin) = create_roots("builtin_read_only");
        fs::write(builtin.join("audio/system.wav"), b"builtin").unwrap();
        let mut world = World::new();
        let mut build = Cell::new_block();
        let mut history = History::new();

        assert!(
            rename_project_asset(
                &project,
                ManagedAssetKind::Audio,
                "system.wav",
                "renamed.wav",
                &mut world,
                &mut build,
                &mut history,
            )
            .is_err()
        );
        assert!(
            delete_project_asset(
                &project,
                ManagedAssetKind::Audio,
                "system.wav",
                &world,
                &build,
            )
            .is_err()
        );
        assert!(builtin.join("audio/system.wav").is_file());

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn mp3_is_managed_as_audio_and_keeps_its_extension_on_rename() {
        let root = temp_root("mp3_catalog");
        let audio = root.join(".assets/audio");
        fs::create_dir_all(&audio).unwrap();
        fs::write(audio.join("song.mp3"), b"fixture").unwrap();
        fs::write(audio.join("alert.wav"), b"fixture").unwrap();
        let assets = scan_assets(&root);
        assert!(
            assets
                .iter()
                .any(|a| a.identifier == "song.mp3" && a.kind == ManagedAssetKind::Audio)
        );
        assert!(
            assets
                .iter()
                .any(|a| a.identifier == "alert.wav" && a.kind == ManagedAssetKind::Audio)
        );
        assert_eq!(
            normalize_identifier(ManagedAssetKind::Audio, "music/new.mp3").unwrap(),
            "music/new.mp3"
        );
        assert_eq!(
            normalize_identifier(ManagedAssetKind::Audio, "music/new").unwrap(),
            "music/new.wav"
        );
        assert!(normalize_identifier(ManagedAssetKind::Audio, "music/new.exe").is_err());
        let _ = fs::remove_dir_all(root);
    }
}

#[cfg(test)]
mod water_material_management_tests {
    use super::*;
    use crate::project::material::*;
    #[test]
    fn water_material_rename_preserves_stable_body_and_instance_references() {
        let doc = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::Water(crate::world::terrain::TerrainWaterSettings::default()),
        });
        let root = std::env::temp_dir().join(&doc.id);
        fs::create_dir_all(&root).unwrap();
        save(&root, "original", &doc, None).unwrap();
        let child = MaterialDocument::new(MaterialDefinition::Instance {
            parent: doc.id.clone(),
            overrides: Default::default(),
        });
        save(&root, "child", &child, None).unwrap();
        let catalog = MaterialCatalog::load(&root);
        let mut world = World::new();
        let mut body = crate::world::terrain::WaterBody::new(
            1,
            crate::world::terrain::WaterBodyKind::Lake,
            [0.0, 0.0],
            2.0,
        );
        body.assign_material(&catalog, &doc.id).unwrap();
        world.terrain.config.water_bodies.push(body);
        let mut template = Cell::default();
        let mut history = History::new();
        let usage = project_asset_usages(
            &root,
            ManagedAssetKind::Material,
            "original",
            &world,
            &template,
        );
        assert_eq!(usage.len(), 2);
        assert!(
            !project_asset_usages(
                &root,
                ManagedAssetKind::Material,
                "original",
                &world,
                &template
            )
            .is_empty()
        );
        let (name, changed) = rename_project_asset(
            &root,
            ManagedAssetKind::Material,
            "original.json",
            "nested/renamed.json",
            &mut world,
            &mut template,
            &mut history,
        )
        .unwrap();
        assert_eq!(name, "nested/renamed");
        assert_eq!(changed, 0);
        assert!(
            !root
                .join(".assets/materials/nested/renamed.json.json")
                .exists()
        );
        assert_eq!(
            world.terrain.config.water_bodies[0]
                .material_asset
                .as_deref(),
            Some(doc.id.as_str())
        );
        let reloaded = MaterialCatalog::load(&root);
        assert!(reloaded.water(&child.id).is_some());
        assert_eq!(read(&root, "nested/renamed").unwrap().id, doc.id);
        let assets = scan_assets(&root);
        assert_eq!(
            assets
                .iter()
                .filter(|a| a.kind == ManagedAssetKind::Material)
                .count(),
            2
        );
        world.terrain.config.water_bodies.clear();
        world.terrain.config.water_material_asset = Some(doc.id.clone());
        assert!(
            !project_asset_usages(
                &root,
                ManagedAssetKind::Material,
                "nested/renamed",
                &world,
                &template
            )
            .is_empty()
        );
        world.terrain.config.water_material_asset = None;
        assert!(
            !project_asset_usages(
                &root,
                ManagedAssetKind::Material,
                "nested/renamed",
                &world,
                &template
            )
            .is_empty()
        );
        delete_project_asset(
            &root,
            ManagedAssetKind::Material,
            "child",
            &world,
            &template,
        )
        .unwrap();
        delete_project_asset(
            &root,
            ManagedAssetKind::Material,
            "nested/renamed",
            &world,
            &template,
        )
        .unwrap();
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod material_context_assignment_tests {
    use super::*;
    #[test]
    fn material_tile_assignment_reads_saved_identity_without_converting_file() {
        use crate::project::material::{MaterialDefinition, MaterialDocument, MaterialOutput};
        let root =
            std::env::temp_dir().join(format!("wkmat012-context-{:032x}", rand::random::<u128>()));
        std::fs::create_dir_all(&root).unwrap();
        let surface = MaterialDocument::new(MaterialDefinition::Asset {
            material: MaterialOutput::SurfacePbr {
                base_color: [0.22, 0.34, 0.56, 1.0],
                roughness: 0.5,
                metallic: 0.0,
            },
        });
        crate::project::material::save(&root, "source.MAT", &surface, None).unwrap();
        let path = crate::project::material::file_path(&root, "source.MAT").unwrap();
        let original = std::fs::read(&path).unwrap();
        assert_eq!(selected_material_id(&root, &path).unwrap(), surface.id);
        let catalog = crate::project::material::MaterialCatalog::load(&root);
        assert!(catalog.sky(&surface.id).is_some());
        assert!(catalog.water(&surface.id).is_some());
        assert_eq!(std::fs::read(&path).unwrap(), original);
        assert!(selected_material_id(&root, &root.join("no-such-material.json")).is_err());
        std::fs::remove_dir_all(root).unwrap();
    }
}
