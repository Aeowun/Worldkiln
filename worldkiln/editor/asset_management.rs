use std::fs;
use std::path::{Component, Path, PathBuf};

use crate::editor::editor::{Editor, EditorTool, History};
use crate::renderer::Renderer;
use crate::world::{Cell, World, WorldCoord};

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 13:34 -05:00
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum ManagedAssetKind {
    #[default]
    Texture,
    Audio,
}

impl ManagedAssetKind {
    pub const ALL: [Self; 2] = [Self::Texture, Self::Audio];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Texture => "Texture",
            Self::Audio => "Audio",
        }
    }

    fn category_dir(self) -> &'static str {
        match self {
            Self::Texture => "textures",
            Self::Audio => "audio",
        }
    }

    fn extension(self) -> &'static str {
        match self {
            Self::Texture => "png",
            Self::Audio => "wav",
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
}

#[derive(Clone, Debug, Default)]
pub struct AssetManagerState {
    pub open: bool,
    pub search: String,
    pub kind: ManagedAssetKind,
    pub rename_asset: Option<(ManagedAssetKind, String)>,
    pub rename_target: String,
    pub delete_asset: Option<(ManagedAssetKind, String)>,
    pub status: Option<(String, bool)>,

    // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 13:31 -05:00
    // Project Assets is a management surface, not an engine-asset browser. Cache the
    // project-only catalog across frames so opening the window never recursively walks
    // engine assets or re-reads filesystem metadata every rendered frame.
    catalog: Vec<ManagedAsset>,
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:14 -05:00
    // Scan project package names only on catalog refresh, never while painting UI.
    project_actor_packages: Vec<String>,
    project_character_packages: Vec<String>,
    catalog_project: Option<PathBuf>,
    catalog_loaded: bool,
    was_open: bool,
    selected_asset: Option<(ManagedAssetKind, String)>,
    selected_usages: Vec<AssetUsage>,
    selected_usage_revision: Option<u64>,
    selected_usage_build_value: Option<String>,
    pending_glb_import: Option<PendingGlbImport>,
}

fn builtin_root(kind: ManagedAssetKind) -> PathBuf {
    crate::project::engine_assets_dir().join(kind.category_dir())
}

fn project_root(project_path: &Path, kind: ManagedAssetKind) -> PathBuf {
    project_path.join(".assets").join(kind.category_dir())
}

fn normalize_identifier(kind: ManagedAssetKind, value: &str) -> Result<String, String> {
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
            if !normalized
                .to_ascii_lowercase()
                .ends_with(&format!(".{expected_extension}"))
            {
                normalized.push('.');
                normalized.push_str(expected_extension);
            }
        }
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
    }
}

fn scan_category(
    current: &Path,
    root: &Path,
    kind: ManagedAssetKind,
    output: &mut Vec<ManagedAsset>,
) {
    let Ok(entries) = fs::read_dir(current) else {
        return;
    };

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            scan_category(&path, root, kind, output);
            continue;
        }
        if !path.is_file()
            || !path
                .extension()
                .and_then(|extension| extension.to_str())
                .is_some_and(|extension| extension.eq_ignore_ascii_case(kind.extension()))
        {
            continue;
        }

        let Ok(relative) = path.strip_prefix(root) else {
            continue;
        };
        let mut identifier = relative.to_string_lossy().replace('\\', "/");
        if kind == ManagedAssetKind::Texture {
            if let Some(dot) = identifier.rfind('.') {
                identifier.truncate(dot);
            }
        }

        let size_bytes = entry.metadata().map(|metadata| metadata.len()).unwrap_or(0);
        output.push(ManagedAsset {
            kind,
            identifier,
            file_path: path,
            size_bytes,
            overrides_builtin: false,
        });
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 13:31 -05:00
// The manager intentionally enumerates only project-owned assets. Built-in engine assets
// remain available in property selectors/runtime resolution, but they are not manageable
// project content and are therefore never listed here.
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

// Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:14 -05:00
// Package browsers must not deserialize GLBs or traverse package directories
// during a rendered frame. The import and project-open paths already refresh
// these registries; here we index only project-owned package folder names.
fn scan_project_package_names(
    project_path: &Path,
    category: &str,
    available: &[String],
) -> Vec<String> {
    let directory = project_path.join(".assets").join(category);
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut packages = Vec::new();
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|kind| kind.is_dir()) {
            continue;
        }
        let name = entry.file_name();
        let name = name.to_string_lossy();
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

fn refresh_manager_catalog(state: &mut AssetManagerState, project_path: &Path) {
    state.catalog = scan_assets(project_path);
    state.project_actor_packages = scan_project_package_names(
        project_path,
        "entities",
        &crate::engine::entity_package::available_package_names(),
    );
    state.project_character_packages = scan_project_package_names(
        project_path,
        "characters",
        &crate::character::package::available_package_names(),
    );
    state.catalog_project = Some(project_path.to_path_buf());
    state.catalog_loaded = true;

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

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:51 -05:00
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
    }

    usages.sort_by(|left, right| left.description.cmp(&right.description));
    usages
}

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:51 -05:00
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
                    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
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
    }

    changed
}

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:51 -05:00
pub(crate) fn rename_project_asset(
    project_path: &Path,
    kind: ManagedAssetKind,
    old_identifier: &str,
    requested_identifier: &str,
    world: &mut World,
    build_template: &mut Cell,
    history: &mut History,
) -> Result<(String, usize), String> {
    let old_identifier = normalize_identifier(kind, old_identifier)?;
    let new_identifier = normalize_identifier(kind, requested_identifier)?;
    if old_identifier == new_identifier {
        return Err("Choose a different destination identifier.".to_string());
    }

    let root = project_root(project_path, kind);
    let source = identifier_file_path(&root, kind, &old_identifier);
    let destination = identifier_file_path(&root, kind, &new_identifier);

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

fn delete_project_asset_with_builtin_root(
    project_path: &Path,
    builtin_category_root: &Path,
    kind: ManagedAssetKind,
    identifier: &str,
    world: &World,
    build_template: &Cell,
) -> Result<bool, String> {
    let identifier = normalize_identifier(kind, identifier)?;
    let usages = asset_usages(kind, &identifier, world, build_template);
    if !usages.is_empty() {
        let details = usages
            .iter()
            .map(|usage| usage.description.as_str())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!(
            "Delete blocked: '{}' is used by {}.",
            identifier, details
        ));
    }

    let root = project_root(project_path, kind);
    let file = identifier_file_path(&root, kind, &identifier);
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

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:31 -05:00
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

impl Editor {
    // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 13:31 -05:00
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
            return;
        };

        let just_opened = !self.asset_manager.was_open;
        self.asset_manager.was_open = true;

        if just_opened
            || !self.asset_manager.catalog_loaded
            || self.asset_manager.catalog_project.as_deref() != Some(project_path)
        {
            refresh_manager_catalog(&mut self.asset_manager, project_path);
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
            };

            let usages_are_stale = self.asset_manager.selected_usage_revision != Some(revision)
                || self.asset_manager.selected_usage_build_value.as_deref()
                    != Some(build_value.as_str());

            if usages_are_stale {
                self.asset_manager.selected_usages = asset_usages(
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
        let pending_glb_import = self.asset_manager.pending_glb_import.clone();
        let actor_packages = self.asset_manager.project_actor_packages.clone();
        let character_packages = self.asset_manager.project_character_packages.clone();
        let mut place_actor_package: Option<String> = None;
        let mut select_player_package: Option<String> = None;
        let mut open = self.asset_manager.open;
        let mut refresh_clicked = false;
        let mut choose_glb_clicked = false;
        let mut choose_glb_folder_clicked = false;
        let mut import_glb_clicked = false;
        let mut cancel_glb_clicked = false;

        egui::Window::new("Project Assets")
            .open(&mut open)
            .default_size(egui::vec2(900.0, 600.0))
            .min_size(egui::vec2(680.0, 460.0))
            .show(ctx, |ui| {
                ui.label(
                    egui::RichText::new("PROJECT ASSETS")
                        .strong()
                        .size(16.0),
                );
                ui.small(
                    "Manage files owned by this project. Engine-provided assets are intentionally hidden.",
                );
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    if ui.button("Import 3D Asset / GLB...").clicked() {
                        choose_glb_clicked = true;
                    }
                    if ui.button("Import 3D Asset Folder...").clicked() {
                        choose_glb_folder_clicked = true;
                    }
                });

                if let Some(pending) = pending_glb_import.as_ref() {
                    ui.add_space(6.0);
                    ui.group(|ui| {
                        let kind = match pending.summary.kind {
                            crate::project::GlbAssetKind::StaticMesh => "Static Actor meshes",
                            crate::project::GlbAssetKind::RiggedCharacter => "Rigged Character",
                        };
                        ui.strong("GLB import summary");
                        ui.label(format!("Source: {}", pending.source.display()));
                        ui.label(format!("Classification: {kind}"));
                        ui.label(format!(
                            "Meshes: {}  Materials: {}  Skins: {}  Animations: {}",
                            pending.summary.meshes.len(),
                            pending.summary.materials.len(),
                            pending.summary.skin_joint_counts.len(),
                            pending.summary.animation_names.len(),
                        ));
                        let mesh_names = pending
                            .summary
                            .meshes
                            .iter()
                            .map(|mesh| mesh.name.as_str())
                            .collect::<Vec<_>>()
                            .join(", ");
                        ui.small(format!("Named meshes: {mesh_names}"));

                        ui.horizontal(|ui| {
                            let import_label = match pending.summary.kind {
                                crate::project::GlbAssetKind::StaticMesh => "Import Actor packages",
                                crate::project::GlbAssetKind::RiggedCharacter => {
                                    "Import Character package"
                                }
                            };
                            let import = ui.button(import_label);
                            if import.clicked() {
                                import_glb_clicked = true;
                            }
                            if ui.button("Cancel").clicked() {
                                cancel_glb_clicked = true;
                            }
                        });
                    });
                }
                ui.add_space(6.0);

                // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:14 -05:00
                // Import selection is a project-owned package reference, not
                // another load/parse operation. Placement reuses Build: Actor.
                egui::CollapsingHeader::new(format!(
                    "Imported 3D packages ({} Actors, {} Characters)",
                    actor_packages.len(), character_packages.len()
                ))
                .default_open(true)
                .show(ui, |ui| {
                    ui.strong("Actors");
                    if actor_packages.is_empty() {
                        ui.small("No project Actor packages. Import a static GLB to add one.");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("imported_actor_package_list")
                            .max_height(112.0)
                            .show(ui, |ui| {
                                for package in &actor_packages {
                                    ui.push_id(("imported_actor", package.as_str()), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.monospace(package);
                                            if ui.button("Place in World").clicked() {
                                                place_actor_package = Some(package.clone());
                                            }
                                        });
                                    });
                                }
                            });
                    }
                    ui.add_space(5.0);
                    ui.strong("Characters");
                    if character_packages.is_empty() {
                        ui.small("No project Character packages. Import an animated GLB to add one.");
                    } else {
                        egui::ScrollArea::vertical()
                            .id_salt("imported_character_package_list")
                            .max_height(96.0)
                            .show(ui, |ui| {
                                for package in &character_packages {
                                    ui.push_id(("imported_character", package.as_str()), |ui| {
                                        ui.horizontal(|ui| {
                                            ui.monospace(package);
                                            if world.selected_character == *package {
                                                ui.small("Current Player");
                                            } else if ui.button("Use as Player").clicked() {
                                                select_player_package = Some(package.clone());
                                            }
                                        });
                                    });
                                }
                            });
                    }
                });
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    for candidate in ManagedAssetKind::ALL {
                        let count = match candidate {
                            ManagedAssetKind::Texture => texture_count,
                            ManagedAssetKind::Audio => audio_count,
                        };
                        let selected = self.asset_manager.kind == candidate;
                        if ui
                            .selectable_label(
                                selected,
                                format!("{} ({count})", candidate.label()),
                            )
                            .clicked()
                        {
                            self.asset_manager.kind = candidate;
                            if self
                                .asset_manager
                                .selected_asset
                                .as_ref()
                                .is_some_and(|(selected_kind, _)| *selected_kind != candidate)
                            {
                                self.asset_manager.selected_asset = self
                                    .asset_manager
                                    .catalog
                                    .iter()
                                    .find(|asset| asset.kind == candidate)
                                    .map(|asset| (asset.kind, asset.identifier.clone()));
                                self.asset_manager.selected_usage_revision = None;
                                self.asset_manager.selected_usage_build_value = None;
                            }
                            ctx.request_repaint();
                        }
                    }

                    ui.separator();
                    ui.add(
                        egui::TextEdit::singleline(&mut self.asset_manager.search)
                            .desired_width(260.0)
                            .hint_text("Search project assets"),
                    );

                    if ui.button("Refresh").clicked() {
                        refresh_clicked = true;
                    }
                });

                if let Some((message, success)) = &self.asset_manager.status {
                    ui.add_space(4.0);
                    ui.label(
                        egui::RichText::new(message).color(if *success {
                            egui::Color32::LIGHT_GREEN
                        } else {
                            egui::Color32::LIGHT_RED
                        }),
                    );
                }

                ui.separator();

                let list_width = 340.0_f32.min((ui.available_width() * 0.42).max(250.0));
                let content_height = ui.available_height();

                ui.horizontal(|ui| {
                    ui.allocate_ui_with_layout(
                        egui::vec2(list_width, content_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            ui.horizontal(|ui| {
                                ui.strong(match kind {
                                    ManagedAssetKind::Texture => "Project textures",
                                    ManagedAssetKind::Audio => "Project audio",
                                });
                                ui.label(format!("{} shown", visible.len()));
                            });
                            ui.add_space(4.0);

                            if visible.is_empty() {
                                ui.add_space(18.0);
                                if search.is_empty() {
                                    ui.label(match kind {
                                        ManagedAssetKind::Texture => {
                                            "No project textures. Import textures through the existing Texture Browser."
                                        }
                                        ManagedAssetKind::Audio => {
                                            "No project audio. Import audio through the existing Audio Browser."
                                        }
                                    });
                                } else {
                                    ui.label("No project assets match this search.");
                                }
                            } else {
                                egui::ScrollArea::vertical()
                                    .id_salt("project_asset_manager_list")
                                    .auto_shrink([false, false])
                                    .show(ui, |ui| {
                                        for asset in &visible {
                                            ui.push_id(
                                                (asset.kind, asset.identifier.as_str()),
                                                |ui| {
                                                    let selected = self
                                                        .asset_manager
                                                        .selected_asset
                                                        .as_ref()
                                                        .is_some_and(|(selected_kind, identifier)| {
                                                            *selected_kind == asset.kind
                                                                && identifier == &asset.identifier
                                                        });

                                                    let response = ui.selectable_label(
                                                        selected,
                                                        egui::RichText::new(&asset.identifier)
                                                            .monospace(),
                                                    );
                                                    if response.clicked() {
                                                        self.asset_manager.selected_asset = Some((
                                                            asset.kind,
                                                            asset.identifier.clone(),
                                                        ));
                                                        self.asset_manager
                                                            .selected_usage_revision = None;
                                                        self.asset_manager
                                                            .selected_usage_build_value = None;
                                                        ctx.request_repaint();
                                                    }

                                                    ui.horizontal(|ui| {
                                                        ui.small(format_asset_size(asset.size_bytes));
                                                        if asset.overrides_builtin {
                                                            ui.small("Overrides engine default");
                                                        }
                                                    });
                                                    ui.separator();
                                                },
                                            );
                                        }
                                    });
                            }
                        },
                    );

                    ui.separator();

                    ui.allocate_ui_with_layout(
                        egui::vec2(ui.available_width(), content_height),
                        egui::Layout::top_down(egui::Align::Min),
                        |ui| {
                            if let Some(asset) = selected_detail.as_ref() {
                                ui.label(
                                    egui::RichText::new(&asset.identifier)
                                        .monospace()
                                        .strong()
                                        .size(15.0),
                                );
                                ui.add_space(4.0);

                                egui::Grid::new("project_asset_details_grid")
                                    .num_columns(2)
                                    .spacing(egui::vec2(14.0, 6.0))
                                    .show(ui, |ui| {
                                        ui.label("Type");
                                        ui.label(asset.kind.label());
                                        ui.end_row();

                                        ui.label("Size");
                                        ui.label(format_asset_size(asset.size_bytes));
                                        ui.end_row();

                                        ui.label("Project path");
                                        let display_path = asset
                                            .file_path
                                            .strip_prefix(project_path)
                                            .unwrap_or(&asset.file_path);
                                        ui.monospace(display_path.display().to_string());
                                        ui.end_row();

                                        if asset.overrides_builtin {
                                            ui.label("Resolution");
                                            ui.label("Project file overrides the engine default");
                                            ui.end_row();
                                        }
                                    });

                                ui.add_space(10.0);
                                ui.horizontal(|ui| {
                                    if ui.button("Rename / Move").clicked() {
                                        self.asset_manager.rename_asset =
                                            Some((asset.kind, asset.identifier.clone()));
                                        self.asset_manager.rename_target =
                                            asset.identifier.clone();
                                        self.asset_manager.status = None;
                                    }

                                    if ui.button("Delete").clicked() {
                                        self.asset_manager.delete_asset =
                                            Some((asset.kind, asset.identifier.clone()));
                                        self.asset_manager.status = None;
                                    }
                                });

                                ui.separator();
                                ui.strong(format!("Used by ({})", selected_usages.len()));

                                if selected_usages.is_empty() {
                                    ui.small("No authored references found.");
                                } else {
                                    egui::ScrollArea::vertical()
                                        .id_salt("project_asset_usage_list")
                                        .max_height(220.0)
                                        .show(ui, |ui| {
                                            for usage in &selected_usages {
                                                ui.monospace(&usage.description);
                                            }
                                        });
                                }
                            } else {
                                ui.add_space(24.0);
                                ui.label("Select a project asset to inspect or manage it.");
                            }
                        },
                    );
                });
            });

        self.asset_manager.open = open;
        if !open {
            self.asset_manager.was_open = false;
        }
        // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:14 -05:00
        if let Some(package) = place_actor_package {
            self.build_actor_package = Some(package);
            self.build_is_actor = true;
            self.activate_editor_tool(EditorTool::Build);
            self.asset_manager.open = false;
            self.asset_manager.was_open = false;
            ctx.request_repaint();
        }
        if let Some(package) = select_player_package {
            world.selected_character = package.clone();
            // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:57 -05:00
            // Existing imported Characters may predate automatic controller
            // selection. Pair them with their separately owned controller when
            // the package is selected, without touching built-in controllers.
            let imported_controller = format!("{package}_Controller");
            if crate::project::discover_controllers(project_path)
                .iter()
                .any(|controller| controller.eq_ignore_ascii_case(&imported_controller))
            {
                world.selected_controller = imported_controller;
            }
            self.needs_save = true;
            self.asset_manager.status = Some((
                format!(
                    "'{package}' selected as Player Character. Save the project to keep this choice."
                ),
                true,
            ));
            ctx.request_repaint();
        }

        if choose_glb_clicked
            && let Some(source) = rfd::FileDialog::new()
                .add_filter("Binary glTF", &["glb"])
                .pick_file()
        {
            match crate::project::inspect_glb(&source) {
                Ok(summary) => {
                    self.asset_manager.pending_glb_import =
                        Some(PendingGlbImport { source, summary });
                    self.asset_manager.status = Some((
                        "GLB inspected successfully. Review the import summary.".to_string(),
                        true,
                    ));
                }
                Err(error) => {
                    self.asset_manager.pending_glb_import = None;
                    self.asset_manager.status =
                        Some((format!("GLB inspection failed: {error}"), false));
                }
            }
            ctx.request_repaint();
        }

        if choose_glb_folder_clicked && let Some(folder) = rfd::FileDialog::new().pick_folder() {
            // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
            // Static packs may share relative images across many GLBs, while
            // a Character download commonly wraps one rigged model.glb.
            match crate::project::import_glb_folder(project_path, &folder) {
                Ok(crate::project::GlbFolderImportResult::Static(imported)) => {
                    if let Some(first) = imported.packages.first() {
                        self.build_actor_package = Some(first.package_name.clone());
                    }
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some((
                        format!(
                            "Imported 3D asset folder '{}' as {} Actor package(s).",
                            imported.source_file_name,
                            imported.packages.len()
                        ),
                        true,
                    ));
                }
                Ok(crate::project::GlbFolderImportResult::Character(imported)) => {
                    world.selected_character = imported.package_name.clone();
                    world.selected_controller = imported.controller_package_name.clone();
                    self.needs_save = true;
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some((
                        format!(
                            "Imported Character folder as '{}' and selected controller '{}'.",
                            imported.package_name, imported.controller_package_name
                        ),
                        true,
                    ));
                }
                Err(error) => {
                    self.asset_manager.status =
                        Some((format!("GLB folder import failed: {error}"), false));
                }
            }
            ctx.request_repaint();
        }

        if cancel_glb_clicked {
            self.asset_manager.pending_glb_import = None;
            ctx.request_repaint();
        }

        if import_glb_clicked && let Some(pending) = self.asset_manager.pending_glb_import.take() {
            let result = match pending.summary.kind {
                crate::project::GlbAssetKind::StaticMesh => {
                    crate::project::import_static_glb(project_path, &pending.source).map(
                        |imported| {
                            let package_names = imported
                                .packages
                                .iter()
                                .map(|package| package.package_name.as_str())
                                .collect::<Vec<_>>()
                                .join(", ");
                            if let Some(first) = imported.packages.first() {
                                self.build_actor_package = Some(first.package_name.clone());
                            }
                            format!(
                                "Imported {} Actor package(s) from {}: {}",
                                imported.packages.len(),
                                imported.source_file_name,
                                package_names
                            )
                        },
                    )
                }
                crate::project::GlbAssetKind::RiggedCharacter => {
                    crate::project::import_character_glb(project_path, &pending.source).map(
                        |imported| {
                            // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 09:52 -05:00
                            // Imported Characters own a project controller;
                            // never mutate or silently reuse the built-in Hero controller.
                            world.selected_character = imported.package_name.clone();
                            world.selected_controller = imported.controller_package_name.clone();
                            self.needs_save = true;
                            let combat = imported
                                .global_script_path
                                .as_deref()
                                .map(|path| format!("; installed global combat Module {path}"))
                                .unwrap_or_default();
                            format!(
                                "Imported Character package '{}' from {} ({} joints, {} clips); selected controller '{}'{}",
                                imported.package_name,
                                imported.source_file_name,
                                imported.joint_count,
                                imported.clip_names.len(),
                                imported.controller_package_name,
                                combat
                            )
                        },
                    )
                }
            };
            match result {
                Ok(message) => {
                    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:14 -05:00
                    // Import refreshed the registries. Refresh this UI catalog
                    // once so the new packages appear immediately.
                    refresh_manager_catalog(&mut self.asset_manager, project_path);
                    self.asset_manager.status = Some((message, true));
                }
                Err(error) => {
                    self.asset_manager.pending_glb_import = Some(pending);
                    self.asset_manager.status =
                        Some((format!("GLB import failed: {error}"), false));
                }
            }
            ctx.request_repaint();
        }

        if refresh_clicked {
            refresh_manager_catalog(&mut self.asset_manager, project_path);
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
            let usages = asset_usages(delete_kind, &identifier, world, &self.build_template);
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
                            egui::RichText::new("Deletion blocked: this asset is referenced.")
                                .color(egui::Color32::LIGHT_RED),
                        );
                        for usage in &usages {
                            ui.monospace(&usage.description);
                        }
                    }

                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(usages.is_empty(), egui::Button::new("Delete"))
                            .clicked()
                        {
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

        if catalog_changed {
            refresh_manager_catalog(&mut self.asset_manager, project_path);
            ctx.request_repaint();
        }
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:51 -05:00
#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::editor::History;
    use crate::world::{CellType, TerrainMaterials};

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

    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-07 22:14 -05:00
    #[test]
    fn imported_package_picker_indexes_project_folders_without_loading_glb_content() {
        let (root, project, _) = create_roots("imported_package_picker");
        let actor_dir = project.join(".assets/entities/NormalTree_1");
        let character_dir = project.join(".assets/characters/Rogue");
        fs::create_dir_all(&actor_dir).unwrap();
        fs::create_dir_all(&character_dir).unwrap();
        // Deliberately invalid source contents: this picker must not parse GLBs.
        fs::write(actor_dir.join("source.glb"), b"not a glb").unwrap();
        fs::write(character_dir.join("source.glb"), b"not a glb").unwrap();
        let actors = scan_project_package_names(
            &project,
            "entities",
            &["NormalTree_1".into(), "engine_cube".into()],
        );
        let characters = scan_project_package_names(
            &project,
            "characters",
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
            &["Rogue".into(), "Rogue".into()],
        );
        assert_eq!(actors, vec!["NormalTree_1"]);
        assert_eq!(characters, vec!["Rogue"]);
        assert!(scan_project_package_names(&project, "missing", &[]).is_empty());
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
    fn delete_blocks_referenced_project_asset_and_allows_unreferenced_asset() {
        let (root, project, _) = create_roots("delete");
        fs::write(project.join(".assets/audio/used.wav"), b"used").unwrap();
        fs::write(project.join(".assets/audio/free.wav"), b"free").unwrap();

        let mut world = World::new();
        let id = world.add_source(CellType::AudioEmitter, glam::Vec3::ZERO);
        world.source_mut(id).unwrap().cell.audio = "used.wav".to_string();
        let build = Cell::new_block();

        let error = delete_project_asset(
            &project,
            ManagedAssetKind::Audio,
            "used.wav",
            &world,
            &build,
        )
        .unwrap_err();
        assert!(error.contains("Source #"));
        assert!(project.join(".assets/audio/used.wav").is_file());
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
}
