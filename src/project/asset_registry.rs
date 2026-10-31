// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

static WRITE_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

pub const REGISTRY_VERSION: u32 = 1;
const REGISTRY_FILE: &str = "asset_registry.json";

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssetType {
    Actor,
    Character,
    Animation,
    Controller,
    Camera,
    Texture,
    Material,
    Image,
    Video,
    Audio,
    Effect,
    Script,
}

impl AssetType {
    pub fn category(&self) -> &'static str {
        match self {
            Self::Actor => "Actors",
            Self::Character => "Characters",
            Self::Animation => "Animations",
            Self::Controller => "Controllers",
            Self::Camera => "Cameras",
            Self::Texture => "Textures",
            Self::Material => "Materials",
            Self::Image => "Images",
            Self::Video => "Video",
            Self::Audio => "Audio",
            Self::Effect => "Effects",
            Self::Script => "Scripts",
        }
    }
    fn folder(&self) -> &'static str {
        match self {
            Self::Actor => "entities",
            Self::Character => "characters",
            Self::Animation => "animations",
            Self::Controller => "controllers",
            Self::Camera => "cameras",
            Self::Texture => "textures",
            Self::Material => "materials",
            Self::Image | Self::Video => "media",
            Self::Audio => "audio",
            Self::Effect => "effects",
            Self::Script => "scripts",
        }
    }
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetSelection {
    pub asset_id: String,
    pub kind: AssetType,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetOperationResult {
    pub asset_id: String,
    pub revision: u64,
    pub removed: bool,
    pub affected_ids: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct AssetRecord {
    pub id: String,
    pub name: String,
    pub kind: AssetType,
    pub path: String,
    #[serde(default)]
    pub dependencies: Vec<String>,
    #[serde(default)]
    pub missing_dependencies: Vec<String>,
    #[serde(default)]
    pub group: Option<String>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct AssetInspection {
    pub materials: Vec<String>,
    pub meshes: Vec<String>,
    pub animations: Vec<String>,
    pub skeleton_joint_count: usize,
    pub source_actor: Option<String>,
    pub controller: Option<String>,
    pub camera: Option<String>,
    pub collision_enabled: Option<bool>,
    pub collision_shape_count: usize,
    pub dependencies: Vec<String>,
    pub missing_dependencies: Vec<String>,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct AssetRegistry {
    pub version: u32,
    #[serde(default)]
    pub revision: u64,
    pub assets: Vec<AssetRecord>,
}
impl Default for AssetRegistry {
    fn default() -> Self {
        Self {
            version: REGISTRY_VERSION,
            revision: 0,
            assets: Vec::new(),
        }
    }
}

pub(crate) fn package_aeo_files(package: &Path) -> Vec<PathBuf> {
    let Ok(files) = fs::read_dir(package) else {
        return Vec::new();
    };
    let mut scripts = files
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            !path.is_symlink()
                && path.is_file()
                && path
                    .extension()
                    .and_then(|ext| ext.to_str())
                    .is_some_and(|ext| ext.eq_ignore_ascii_case("aeo"))
        })
        .collect::<Vec<_>>();
    scripts.sort();
    scripts
}

fn registry_path(project: &Path) -> PathBuf {
    project.join(".assets").join(REGISTRY_FILE)
}
fn key(path: &Path, project: &Path) -> Result<String, String> {
    let relative = path
        .strip_prefix(project)
        .map_err(|_| "Asset escapes project".to_string())?;
    if relative
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("Asset path contains noncanonical components".into());
    }
    Ok(relative.to_string_lossy().replace('\\', "/"))
}
fn add_file(
    entries: &mut BTreeMap<String, (AssetType, String)>,
    project: &Path,
    path: PathBuf,
    kind: AssetType,
) {
    if !path.is_file() || path.is_symlink() {
        return;
    }
    if let Ok(id) = key(&path, project) {
        let name = path
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        entries.insert(id, (kind, name));
    }
}
fn collect_files(
    project: &Path,
    dir: &Path,
    kind: AssetType,
    extensions: &[&str],
    entries: &mut BTreeMap<String, (AssetType, String)>,
    depth: usize,
) {
    if depth > 8 || dir.is_symlink() {
        return;
    }
    let Ok(read) = fs::read_dir(dir) else { return };
    for entry in read.flatten() {
        let path = entry.path();
        if path.is_symlink() {
            continue;
        }
        if path.is_dir() {
            collect_files(project, &path, kind.clone(), extensions, entries, depth + 1);
        } else if path
            .extension()
            .and_then(|v| v.to_str())
            .is_some_and(|v| extensions.iter().any(|x| v.eq_ignore_ascii_case(x)))
        {
            add_file(entries, project, path, kind.clone());
        }
    }
}
fn native_asset_id(project: &Path, relative: &str, kind: &AssetType) -> Option<String> {
    let path = project.join(relative);
    match kind {
        AssetType::Animation => fs::read(path.join(crate::animation::asset::ANIMATION_FILE))
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|document| document.get("id")?.as_str().map(str::to_owned))
            .filter(|id| !id.trim().is_empty()),
        AssetType::Material => fs::read(path)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
            .and_then(|document| document.get("id")?.as_str().map(str::to_owned))
            .filter(|id| !id.trim().is_empty()),
        AssetType::Effect => crate::project::fx_assets::id_from_path(project, &path),
        _ => None,
    }
}
fn discover(project: &Path) -> BTreeMap<String, (AssetType, String)> {
    let mut found = BTreeMap::new();
    let assets = project.join(".assets");
    for kind in [
        AssetType::Actor,
        AssetType::Character,
        AssetType::Animation,
        AssetType::Controller,
        AssetType::Camera,
    ] {
        let root = assets.join(kind.folder());
        let Ok(read) = fs::read_dir(&root) else {
            continue;
        };
        for entry in read.flatten() {
            let path = entry.path();
            if !path.is_dir() || path.is_symlink() {
                continue;
            }
            if entry.file_name().to_string_lossy().starts_with('.') {
                continue;
            }
            let marker = if kind == AssetType::Animation {
                crate::animation::asset::ANIMATION_FILE
            } else {
                "package.json"
            };
            let scripted_native = matches!(kind, AssetType::Controller | AssetType::Camera)
                && fs::read_dir(&path).ok().is_some_and(|entries| {
                    entries
                        .flatten()
                        .any(|entry| entry.path().extension().is_some_and(|ext| ext == "aeo"))
                });
            if !path.join(marker).is_file() && !scripted_native {
                continue;
            }
            if kind == AssetType::Character
                && !crate::project::character_asset::is_explicit_character_dir(&path)
            {
                continue;
            }
            if let Ok(relative) = key(&path, project) {
                let name = if kind == AssetType::Animation {
                    fs::read(path.join(crate::animation::asset::ANIMATION_FILE))
                        .ok()
                        .and_then(|bytes| serde_json::from_slice::<serde_json::Value>(&bytes).ok())
                        .and_then(|document| document.get("name")?.as_str().map(str::to_owned))
                        .filter(|name| !name.trim().is_empty())
                        .unwrap_or_else(|| entry.file_name().to_string_lossy().to_string())
                } else {
                    entry.file_name().to_string_lossy().to_string()
                };
                found.insert(relative, (kind.clone(), name));
            }
        }
    }
    for (kind, exts) in [
        (AssetType::Texture, &["png", "jpg", "jpeg"][..]),
        (AssetType::Material, &["json", "mat"][..]),
        (AssetType::Audio, &["wav", "mp3", "ogg"][..]),
        (AssetType::Effect, &["efkefc", "efkpkg"][..]),
    ] {
        collect_files(
            project,
            &assets.join(kind.folder()),
            kind,
            exts,
            &mut found,
            0,
        );
    }
    let actor_root = assets.join("entities");
    if let Ok(packages) = fs::read_dir(&actor_root) {
        for package in packages.flatten() {
            let path = package.path();
            if !path.is_dir() || path.is_symlink() || !path.join("package.json").is_file() {
                continue;
            }
            collect_files(
                project,
                &path.join("textures"),
                AssetType::Texture,
                &["png", "jpg", "jpeg"],
                &mut found,
                0,
            );
        }
    }
    let media = assets.join("media");
    collect_files(
        project,
        &media,
        AssetType::Image,
        &["png", "jpg", "jpeg"],
        &mut found,
        0,
    );
    collect_files(
        project,
        &media,
        AssetType::Video,
        &["mp4", "webm", "mov"],
        &mut found,
        0,
    );
    collect_files(
        project,
        &project.join("scripts"),
        AssetType::Script,
        &["aeo"],
        &mut found,
        0,
    );
    found
}
impl AssetRegistry {
    pub fn load(project: &Path) -> Result<Self, String> {
        let bytes = fs::read(registry_path(project))
            .map_err(|e| format!("Project asset registry unavailable: {e}"))?;
        let data: Self = serde_json::from_slice(&bytes)
            .map_err(|e| format!("Invalid project asset registry: {e}"))?;
        if data.version != REGISTRY_VERSION {
            return Err("Unsupported project asset registry version".into());
        }
        let mut ids = BTreeSet::new();
        let mut paths = BTreeSet::new();
        for record in &data.assets {
            if !ids.insert(&record.id) || !paths.insert(&record.path) {
                return Err("Duplicate identity in project asset registry".into());
            }
            let relative = Path::new(&record.path);
            if relative
                .components()
                .any(|part| !matches!(part, std::path::Component::Normal(_)))
            {
                return Err("Unsafe path in project asset registry".into());
            }
        }
        Ok(data)
    }
    pub fn save(&self, project: &Path) -> Result<(), String> {
        let file = registry_path(project);
        fs::create_dir_all(file.parent().ok_or("Invalid registry root")?)
            .map_err(|e| e.to_string())?;
        let mut next = self.clone();
        let previous = fs::read(&file)
            .ok()
            .and_then(|bytes| serde_json::from_slice::<AssetRegistry>(&bytes).ok());
        if let Some(ref old) = previous {
            if old.assets == self.assets && old.version == self.version {
                return Ok(());
            }
        }
        next.revision = previous.map_or(1, |old| old.revision.saturating_add(1));
        let bytes = serde_json::to_vec_pretty(&next).map_err(|e| e.to_string())?;
        for attempt in 0..6 {
            match crate::project::atomic_file::write_atomically(&file, &bytes) {
                Ok(()) => return Ok(()),
                Err(error)
                    if attempt < 5
                        && (error.kind() == std::io::ErrorKind::PermissionDenied
                            || error.raw_os_error() == Some(32)) =>
                {
                    std::thread::sleep(std::time::Duration::from_millis(35 * (attempt + 1)));
                }
                Err(error) => {
                    return Err(format!(
                        "Asset registry publication failed at {}: {error}",
                        file.display()
                    ));
                }
            }
        }
        unreachable!("The final registry publication attempt always returns")
    }
    pub fn by_type(&self, kind: AssetType) -> impl Iterator<Item = &AssetRecord> {
        self.assets.iter().filter(move |entry| entry.kind == kind)
    }
    pub fn notify_content_changed(project: &Path) -> Result<u64, String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        Self::notify_content_changed_locked(project)
    }
    fn notify_content_changed_locked(project: &Path) -> Result<u64, String> {
        let mut registry = Self::load(project)?;
        registry.revision = registry.revision.saturating_add(1);
        let bytes = serde_json::to_vec_pretty(&registry).map_err(|e| e.to_string())?;
        crate::project::atomic_file::write_atomically(&registry_path(project), &bytes)
            .map_err(|e| e.to_string())?;
        Ok(registry.revision)
    }
    pub fn snapshot_if_changed(project: &Path, last_revision: u64) -> Result<Option<Self>, String> {
        let current = Self::load(project)?;
        Ok((current.revision != last_revision).then_some(current))
    }
    pub fn selection(&self, id: &str) -> Option<AssetSelection> {
        self.find(id).map(|asset| AssetSelection {
            asset_id: asset.id.clone(),
            kind: asset.kind.clone(),
        })
    }
    pub fn resolve_selection(&self, selection: &AssetSelection) -> Result<&AssetRecord, String> {
        let asset = self
            .find(&selection.asset_id)
            .ok_or("Asset selection is no longer registered")?;
        if asset.kind != selection.kind {
            return Err("Asset selection type mismatch".into());
        }
        Ok(asset)
    }
    pub fn inspect(&self, project: &Path, id: &str) -> Result<AssetInspection, String> {
        let record = self.find(id).ok_or("Unknown AssetID")?;
        let mut detail = AssetInspection {
            dependencies: record.dependencies.clone(),
            missing_dependencies: record.missing_dependencies.clone(),
            ..AssetInspection::default()
        };
        if !project.join(&record.path).exists() {
            detail
                .missing_dependencies
                .push(format!("Missing registered resource: {}", record.path));
            return Ok(detail);
        }
        match record.kind {
            AssetType::Actor => {
                let package = project.join(&record.path);
                if let Ok(bytes) = fs::read(package.join("package.json")) {
                    if let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        if let Some(materials) = json
                            .pointer("/appearance/materials")
                            .and_then(|v| v.as_object())
                        {
                            detail.materials = materials.keys().cloned().collect();
                        }
                        if let Some(geometry) = json
                            .pointer("/appearance/geometry")
                            .and_then(|v| v.as_array())
                        {
                            detail.meshes = geometry
                                .iter()
                                .enumerate()
                                .map(|(i, part)| {
                                    part.get("node")
                                        .and_then(|v| v.as_str())
                                        .map(str::to_owned)
                                        .unwrap_or_else(|| format!("Part {i}"))
                                })
                                .collect();
                        }
                        detail.collision_enabled = json
                            .pointer("/physics/collision_enabled")
                            .and_then(|v| v.as_bool());
                        detail.collision_shape_count = json
                            .pointer("/physics/collision_shapes")
                            .and_then(|v| v.as_array())
                            .map_or(0, |shapes| shapes.len());
                    }
                }
                let source = package.join("source.glb");
                if source.is_file() {
                    match crate::project::inspect_glb(&source) {
                        Ok(summary) => {
                            detail.meshes =
                                summary.meshes.into_iter().map(|mesh| mesh.name).collect();
                            detail.materials =
                                summary.materials.into_iter().map(|mat| mat.name).collect();
                            detail.animations = summary.animation_names;
                            detail.skeleton_joint_count = summary.skin_joint_counts.iter().sum();
                        }
                        Err(error) => detail
                            .missing_dependencies
                            .push(format!("GLB inspection failed: {error}")),
                    }
                }
            }
            AssetType::Character => {
                let bytes = fs::read(project.join(&record.path).join("package.json"))
                    .map_err(|e| format!("Cannot inspect Character: {e}"))?;
                let asset: crate::project::character_asset::CharacterAsset =
                    serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                detail.source_actor = Some(asset.source_actor);
                detail.controller = asset.controller;
                detail.camera = asset.camera;
                if let Some(actor) = record.dependencies.iter().find(|dependency| {
                    self.find(dependency)
                        .is_some_and(|candidate| candidate.kind == AssetType::Actor)
                }) {
                    if let Ok(source) = self.inspect(project, actor) {
                        detail.meshes = source.meshes;
                        detail.materials = source.materials;
                        detail.animations = source.animations;
                        detail.skeleton_joint_count = source.skeleton_joint_count;
                        detail.collision_enabled = source.collision_enabled;
                        detail.collision_shape_count = source.collision_shape_count;
                    }
                }
            }
            AssetType::Material => {
                detail.materials.push(record.name.clone());
            }
            _ => {}
        }
        Ok(detail)
    }
    pub fn item(&self, id: &str) -> Option<&AssetRecord> {
        self.find(id)
    }
    pub fn category(&self, kind: AssetType) -> Vec<&AssetRecord> {
        self.by_type(kind).collect()
    }
    pub fn users_of(&self, id: &str) -> Vec<&AssetRecord> {
        self.assets
            .iter()
            .filter(|record| record.dependencies.iter().any(|dep| dep == id))
            .collect()
    }
    pub fn unresolved(&self) -> Vec<&AssetRecord> {
        self.assets
            .iter()
            .filter(|entry| !entry.missing_dependencies.is_empty())
            .collect()
    }
    pub fn preview_path(&self, project: &Path, id: &str) -> Option<PathBuf> {
        let entry = self.find(id)?;
        let path = project.join(&entry.path);
        match entry.kind {
            AssetType::Actor => Some(path.join("source.glb")),
            AssetType::Character => {
                let actor = self.assets.iter().find(|actor| {
                    actor.kind == AssetType::Actor && entry.dependencies.contains(&actor.id)
                })?;
                Some(project.join(&actor.path).join("source.glb"))
            }
            _ => Some(path),
        }
        .filter(|path| path.is_file())
    }
    fn edit_entry(
        project: &Path,
        id: &str,
        mutate: impl FnOnce(&mut AssetRecord) -> Result<(), String>,
    ) -> Result<AssetRecord, String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let mut registry = Self::load(project)?;
        let item = registry
            .assets
            .iter_mut()
            .find(|asset| asset.id == id)
            .ok_or_else(|| format!("Unknown AssetID: {id}"))?;
        mutate(item)?;
        let result = item.clone();
        registry.save(project)?;
        Ok(result)
    }
    pub fn moved(project: &Path, old_path: &Path, new_path: &Path) -> Result<(), String> {
        if !registry_path(project).is_file() {
            return Ok(());
        }
        let old = key(old_path, project)?;
        let new = key(new_path, project)?;
        if old == new {
            return Ok(());
        }
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let mut registry = Self::load(project)?;
        if registry.assets.iter().any(|asset| asset.path == new) {
            return Err("Moved asset destination conflicts with existing AssetID".into());
        }
        if let Some(record) = registry.assets.iter_mut().find(|asset| asset.path == old) {
            record.path = new;
            let old_basename = old_path.file_name().unwrap_or_default().to_string_lossy();
            if record.name == old_basename {
                record.name = new_path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();
            }
            registry.save(project)?;
        }
        Ok(())
    }
    pub fn rename(project: &Path, id: &str, name: &str) -> Result<AssetRecord, String> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err("Invalid asset display name".into());
        }
        Self::edit_entry(project, id, |item| {
            item.name = name.to_owned();
            Ok(())
        })
    }
    pub fn update_metadata(
        project: &Path,
        id: &str,
        name: &str,
        group: Option<&str>,
    ) -> Result<AssetOperationResult, String> {
        let name = name.trim();
        if name.is_empty() || name.len() > 128 || name.chars().any(char::is_control) {
            return Err("Asset display name must contain 1–128 printable characters".into());
        }
        let group = group.map(str::trim).filter(|v| !v.is_empty());
        if group.is_some_and(|v| {
            v.len() > 128 || v.contains(['/', '\\']) || v.chars().any(char::is_control)
        }) {
            return Err("Invalid creator-defined group".into());
        }
        Self::edit_entry(project, id, |item| {
            item.name = name.to_owned();
            item.group = group.map(str::to_owned);
            Ok(())
        })?;
        let revision = Self::load(project)?.revision;
        Ok(AssetOperationResult {
            asset_id: id.to_owned(),
            revision,
            removed: false,
            affected_ids: vec![id.to_owned()],
        })
    }
    pub fn rename_result(
        project: &Path,
        id: &str,
        name: &str,
    ) -> Result<AssetOperationResult, String> {
        Self::rename(project, id, name)?;
        let registry = Self::load(project)?;
        Ok(AssetOperationResult {
            asset_id: id.to_owned(),
            revision: registry.revision,
            removed: false,
            affected_ids: vec![id.to_owned()],
        })
    }
    pub fn group_result(
        project: &Path,
        id: &str,
        group: Option<&str>,
    ) -> Result<AssetOperationResult, String> {
        Self::assign_group(project, id, group)?;
        let registry = Self::load(project)?;
        Ok(AssetOperationResult {
            asset_id: id.to_owned(),
            revision: registry.revision,
            removed: false,
            affected_ids: vec![id.to_owned()],
        })
    }
    pub fn assign_group(
        project: &Path,
        id: &str,
        group: Option<&str>,
    ) -> Result<AssetRecord, String> {
        let name = group.map(str::trim).filter(|s| !s.is_empty());
        if name.is_some_and(|s| {
            s.len() > 128 || s.contains(['/', '\\']) || s.chars().any(char::is_control)
        }) {
            return Err("Invalid creator-defined group".into());
        }
        Self::edit_entry(project, id, |item| {
            item.group = name.map(str::to_owned);
            Ok(())
        })
    }
    pub fn rollback_new_imports(
        project: &Path,
        baseline: &Self,
        created_paths: &[String],
    ) -> Result<(), String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let mut current = Self::load(project)?;
        let old_ids = baseline
            .assets
            .iter()
            .map(|a| a.id.as_str())
            .collect::<BTreeSet<_>>();
        let additions = current
            .assets
            .iter()
            .filter(|a| !old_ids.contains(a.id.as_str()))
            .filter(|a| {
                created_paths
                    .iter()
                    .any(|path| a.path == *path || a.path.starts_with(&format!("{path}/")))
            })
            .cloned()
            .collect::<Vec<_>>();
        let added_ids = additions
            .iter()
            .map(|a| a.id.as_str())
            .collect::<BTreeSet<_>>();
        for addition in &additions {
            if current
                .users_of(&addition.id)
                .iter()
                .any(|user| !added_ids.contains(user.id.as_str()))
            {
                return Err(format!(
                    "Cannot safely roll back referenced asset '{}'",
                    addition.name
                ));
            }
        }
        let mut roots = additions.iter().map(|a| a.path.clone()).collect::<Vec<_>>();
        roots.sort_by_key(|path| path.len());
        let mut selected = Vec::new();
        for relative in roots {
            if selected
                .iter()
                .any(|parent: &String| relative.starts_with(&format!("{parent}/")))
            {
                continue;
            }
            let path = project.join(&relative);
            if path.is_symlink()
                || !(relative.starts_with(".assets/") || relative.starts_with("scripts/"))
            {
                return Err(format!("Unsafe import rollback location: {relative}"));
            }
            if !path
                .parent()
                .ok_or("Invalid rollback location")?
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(project.canonicalize().map_err(|e| e.to_string())?)
            {
                return Err(format!("Rollback escaped project: {relative}"));
            }
            selected.push(relative);
        }
        let pending = project.join(".assets").join(".pending-import-rollbacks");
        fs::create_dir_all(&pending).map_err(|e| e.to_string())?;
        let mut staged: Vec<(PathBuf, PathBuf)> = Vec::new();
        for relative in selected {
            let source = project.join(&relative);
            if !source.exists() {
                continue;
            }
            let moved = pending.join(format!("rollback-{:032x}", rand::random::<u128>()));
            if let Err(error) = fs::rename(&source, &moved) {
                for (original, staged_path) in staged.iter().rev() {
                    let _ = fs::rename(staged_path, original);
                }
                return Err(format!(
                    "Import rollback staging failed; originals retained: {error}"
                ));
            }
            staged.push((source, moved));
        }
        current
            .assets
            .retain(|a| !added_ids.contains(a.id.as_str()));
        if let Err(error) = current.save(project) {
            for (original, moved) in staged.iter().rev() {
                let _ = fs::rename(moved, original);
            }
            return Err(format!(
                "Import rollback registry publication failed; files restored: {error}"
            ));
        }
        for (_, moved) in staged {
            let result = if moved.is_dir() {
                fs::remove_dir_all(&moved)
            } else {
                fs::remove_file(&moved)
            };
            if let Err(error) = result {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Assets",
                    format!(
                        "Deferred import rollback cleanup at {}: {error}",
                        moved.display()
                    ),
                );
            }
        }
        Ok(())
    }
    pub fn delete_result(project: &Path, id: &str) -> Result<AssetOperationResult, String> {
        let previous = Self::load(project)?;
        let root = previous.find(id).ok_or("Unknown AssetID")?;
        let prefix = format!("{}/", root.path);
        let mut affected = vec![id.to_owned()];
        affected.extend(
            previous
                .assets
                .iter()
                .filter(|entry| entry.path.starts_with(&prefix))
                .map(|entry| entry.id.clone()),
        );
        Self::delete(project, id)?;
        let registry = Self::load(project)?;
        Ok(AssetOperationResult {
            asset_id: id.to_owned(),
            revision: registry.revision,
            removed: true,
            affected_ids: affected,
        })
    }
    fn saved_world_uses(
        project: &Path,
        id: &str,
        physical_name: &str,
    ) -> Result<Option<PathBuf>, String> {
        let mut pending = vec![(project.to_path_buf(), 0usize)];
        let mut inspected = 0usize;
        while let Some((folder, depth)) = pending.pop() {
            if depth > 8 {
                return Err("Cannot verify deeply nested saved Worlds".into());
            }
            for item in fs::read_dir(&folder).map_err(|e| e.to_string())? {
                let path = item.map_err(|e| e.to_string())?.path();
                if path.is_symlink() {
                    continue;
                }
                if path.is_dir() {
                    let name = path
                        .file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or_default();
                    if matches!(
                        name,
                        ".assets" | ".worldkiln" | "target" | ".git" | "node_modules"
                    ) {
                        continue;
                    }
                    pending.push((path, depth + 1));
                } else if path
                    .file_name()
                    .and_then(|n| n.to_str())
                    .is_some_and(|n| n.eq_ignore_ascii_case("world.dat") || n.ends_with(".world"))
                {
                    inspected += 1;
                    if inspected > 1000 {
                        return Err("Too many saved Worlds to verify safely".into());
                    }
                    let bytes = fs::read(&path).map_err(|e| e.to_string())?;
                    if bytes.len() > 128 * 1024 * 1024 {
                        return Err(format!(
                            "World is too large for deletion validation: {}",
                            path.display()
                        ));
                    }
                    let world = String::from_utf8_lossy(&bytes);
                    for line in world.lines() {
                        let fields = line.split_whitespace().collect::<Vec<_>>();
                        if fields.is_empty() {
                            continue;
                        }
                        let referenced = match fields[0] {
                            "ACTOR" => fields.get(14).copied(),
                            "SELECTED_CHARACTER" | "SELECTED_CONTROLLER" | "SELECTED_CAMERA" => {
                                fields.get(1).copied()
                            }
                            _ => None,
                        };
                        if referenced
                            .is_some_and(|reference| reference == id || reference == physical_name)
                            || (line.contains(id) && id.len() > 8)
                        {
                            return Ok(Some(path));
                        }
                    }
                }
            }
        }
        Ok(None)
    }
    pub fn delete(project: &Path, id: &str) -> Result<(), String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let mut registry = Self::load(project)?;
        let users = registry.users_of(id);
        if !users.is_empty() {
            return Err(format!(
                "Asset is referenced by: {}",
                users
                    .iter()
                    .map(|asset| asset.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ));
        }
        let entry = registry.find(id).ok_or("Asset was not found")?.clone();
        let name = Path::new(&entry.path)
            .file_name()
            .and_then(|n| n.to_str())
            .ok_or("Invalid asset package name")?;
        if let Some(world) = Self::saved_world_uses(project, id, name)? {
            return Err(format!(
                "Cannot delete asset referenced by saved World: {}",
                world.display()
            ));
        }
        let nested_prefix = format!("{}/", entry.path);
        let nested_ids = registry
            .assets
            .iter()
            .filter(|asset| asset.path.starts_with(&nested_prefix))
            .map(|asset| asset.id.clone())
            .collect::<BTreeSet<_>>();
        for child_id in &nested_ids {
            let external = registry
                .users_of(child_id)
                .into_iter()
                .filter(|user| user.id != id && !nested_ids.contains(&user.id))
                .map(|user| user.name.clone())
                .collect::<Vec<_>>();
            if !external.is_empty() {
                return Err(format!(
                    "Cannot remove package: nested resource is used by {}",
                    external.join(", ")
                ));
            }
        }
        let path = project.join(&entry.path);
        if !(entry.path.starts_with(".assets/") || entry.path.starts_with("scripts/"))
            || path == project
            || path.is_symlink()
        {
            return Err("Cannot delete an asset outside canonical project storage".into());
        }
        let root = project.canonicalize().map_err(|e| e.to_string())?;
        if !path
            .parent()
            .ok_or("Invalid asset parent")?
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(root)
        {
            return Err("Asset is outside the active project".into());
        }
        let pending_root = project.join(".assets").join(".pending-deletes");
        fs::create_dir_all(&pending_root).map_err(|e| e.to_string())?;
        let pending = pending_root.join(format!("asset-{:032x}", rand::random::<u128>()));
        if path.exists() {
            fs::rename(&path, &pending).map_err(|e| format!(
                "Cannot remove '{}' because Windows still holds the asset: {e}. Close any model previews or external programs using it and retry.",
                path.display()
            ))?;
        }
        registry
            .assets
            .retain(|asset| asset.id != id && !nested_ids.contains(&asset.id));
        if let Err(error) = registry.save(project) {
            if pending.exists() {
                let _ = fs::rename(&pending, &path);
            }
            return Err(format!(
                "Asset registry update failed; restored original asset: {error}"
            ));
        }
        if pending.exists() {
            let result = if pending.is_dir() {
                fs::remove_dir_all(&pending)
            } else {
                fs::remove_file(&pending)
            };
            if let Err(error) = result {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Assets",
                    format!(
                        "Asset removed from project, but Windows deferred physical cleanup of {}: {error}",
                        pending.display()
                    ),
                );
            }
        }
        Ok(())
    }
    pub fn delete_many(project: &Path, ids: &[String]) -> Result<usize, String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let mut registry = Self::load(project)?;
        let chosen = ids.iter().cloned().collect::<BTreeSet<_>>();
        if chosen.is_empty() {
            return Ok(0);
        }
        let mut roots = Vec::<(PathBuf, String)>::new();
        let mut removed = chosen.clone();
        for id in &chosen {
            let record = registry
                .find(id)
                .ok_or_else(|| format!("Unknown asset ID: {id}"))?;
            let path = project.join(&record.path);
            if !record.path.starts_with(".assets/") || path.is_symlink() {
                return Err(format!("Unsafe asset location: {}", record.path));
            }
            let canonical_root = project.canonicalize().map_err(|e| e.to_string())?;
            if !path
                .parent()
                .ok_or("Missing asset parent")?
                .canonicalize()
                .map_err(|e| e.to_string())?
                .starts_with(&canonical_root)
            {
                return Err(format!("Asset escapes project: {}", record.path));
            }
            let physical_name = path
                .file_name()
                .and_then(|x| x.to_str())
                .ok_or("Invalid imported asset name")?;
            if let Some(saved) = Self::saved_world_uses(project, id, physical_name)? {
                return Err(format!(
                    "Cannot delete '{}': used by saved World {}. Remove the scene Actor first.",
                    record.name,
                    saved.display()
                ));
            }
            let prefix = format!("{}/", record.path);
            removed.extend(
                registry
                    .assets
                    .iter()
                    .filter(|r| r.path.starts_with(&prefix))
                    .map(|r| r.id.clone()),
            );
            roots.push((path, record.name.clone()));
        }
        for id in &removed {
            let dependent_names = registry
                .users_of(id)
                .into_iter()
                .filter(|r| !removed.contains(&r.id))
                .map(|r| r.name.clone())
                .collect::<Vec<_>>();
            if !dependent_names.is_empty() {
                return Err(format!(
                    "Cannot delete group: '{}' is still used by {}",
                    registry.find(id).map(|r| r.name.as_str()).unwrap_or(id),
                    dependent_names.join(", ")
                ));
            }
        }
        let pending_dir = project.join(".assets").join(".pending-deletes");
        fs::create_dir_all(&pending_dir).map_err(|e| e.to_string())?;
        let mut staged = Vec::<(PathBuf, PathBuf)>::new();
        for (source, name) in &roots {
            if !source.exists() {
                continue;
            }
            let dest = pending_dir.join(format!("group-{:032x}", rand::random::<u128>()));
            if let Err(error) = fs::rename(source, &dest) {
                for (original, moved) in staged.iter().rev() {
                    let _ = fs::rename(moved, original);
                }
                return Err(format!(
                    "Cannot delete imported group. Windows could not release '{}': {error}. No packages were removed.",
                    name
                ));
            }
            staged.push((source.clone(), dest));
        }
        registry.assets.retain(|r| !removed.contains(&r.id));
        if let Err(error) = registry.save(project) {
            for (original, moved) in staged.iter().rev() {
                let _ = fs::rename(moved, original);
            }
            return Err(format!(
                "Import group registry save failed, files restored: {error}"
            ));
        }
        for (_, file) in &staged {
            let result = if file.is_dir() {
                fs::remove_dir_all(file)
            } else {
                fs::remove_file(file)
            };
            if let Err(error) = result {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Assets",
                    format!(
                        "Deleted from registry; pending Windows file cleanup: {}: {error}",
                        file.display()
                    ),
                );
            }
        }
        Ok(roots.len())
    }

    pub fn find(&self, id: &str) -> Option<&AssetRecord> {
        self.assets.iter().find(|entry| entry.id == id)
    }
    pub fn resolve<'a>(&'a self, project: &Path, id: &str) -> Option<PathBuf> {
        self.find(id).map(|entry| project.join(&entry.path))
    }
    pub fn native_logic_reference(project: &Path, kind: AssetType, reference: &str) -> String {
        if let Ok(index) = Self::load(project) {
            if let Some(entry) = index.find(reference) {
                if entry.kind == kind {
                    return Path::new(&entry.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_string();
                }
            }
        }
        reference.to_owned()
    }
    pub fn native_logic_asset_id(project: &Path, kind: AssetType, reference: &str) -> String {
        if let Ok(index) = Self::load(project) {
            if let Some(entry) = index.find(reference) {
                return if entry.kind == kind {
                    entry.id.clone()
                } else {
                    reference.to_owned()
                };
            }
            if let Some(entry) = index.by_type(kind).find(|entry| {
                Path::new(&entry.path)
                    .file_name()
                    .is_some_and(|name| name.to_string_lossy().eq_ignore_ascii_case(reference))
            }) {
                return entry.id.clone();
            }
        }
        reference.to_owned()
    }
    pub fn actor_directory(project: &Path, reference: &str) -> Result<PathBuf, String> {
        if let Ok(registry) = Self::load(project) {
            if let Some(asset) = registry.find(reference) {
                if asset.kind != AssetType::Actor {
                    return Err("Source is not an Actor".into());
                }
                return Ok(project.join(&asset.path));
            }
        }
        let name = Path::new(reference);
        if name.components().count() != 1
            || !matches!(
                name.components().next(),
                Some(std::path::Component::Normal(_))
            )
        {
            return Err("Invalid Actor reference".into());
        }
        Ok(project.join(".assets/entities").join(name))
    }
    pub fn runtime_actor_name(&self, id: &str) -> Result<&str, String> {
        let item = self.find(id).ok_or("Unknown Actor AssetID")?;
        if item.kind != AssetType::Actor {
            return Err("Not an Actor".into());
        }
        Path::new(&item.path)
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| "Missing Actor package name".to_string())
    }
    pub fn resolve_actor(
        &self,
        project: &Path,
        id: &str,
    ) -> Result<std::sync::Arc<crate::engine::entity_package::EntityPackage>, String> {
        let item = self.find(id).ok_or("Unknown Actor AssetID")?;
        if item.kind != AssetType::Actor {
            return Err("AssetID is not an Actor".into());
        }
        crate::engine::entity_package::load_package(&project.join(&item.path))
            .map(std::sync::Arc::new)
    }
    pub fn resolve_character(
        &self,
        project: &Path,
        id: &str,
    ) -> Result<crate::project::character_asset::CharacterAsset, String> {
        let item = self.find(id).ok_or("Unknown Character AssetID")?;
        if item.kind != AssetType::Character {
            return Err("AssetID is not a Character".into());
        }
        let package_name = Path::new(&item.path)
            .file_name()
            .and_then(|s| s.to_str())
            .ok_or("Invalid Character package identity")?;
        crate::project::character_asset::load(project, package_name)
    }
    pub fn replace_file(project: &Path, id: &str, source: &Path) -> Result<AssetRecord, String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let registry = Self::load(project)?;
        let entry = registry.find(id).ok_or("Unknown AssetID")?.clone();
        if matches!(
            entry.kind,
            AssetType::Actor | AssetType::Character | AssetType::Controller | AssetType::Camera
        ) {
            return Err("Package replacement requires its native importer".into());
        }
        let target = project.join(&entry.path);
        if !target.is_file() || target.is_symlink() || !source.is_file() || source.is_symlink() {
            return Err("Asset replacement requires two regular files".into());
        }
        if source.canonicalize().ok() == target.canonicalize().ok() {
            return Err("Cannot replace an asset with itself".into());
        }
        if !source
            .extension()
            .and_then(|x| x.to_str())
            .zip(target.extension().and_then(|x| x.to_str()))
            .is_some_and(|(a, b)| a.eq_ignore_ascii_case(b))
        {
            return Err("Replacement must retain its asset format".into());
        }
        let parent = target.parent().ok_or("Missing asset parent")?;
        if !parent
            .canonicalize()
            .map_err(|e| e.to_string())?
            .starts_with(project.canonicalize().map_err(|e| e.to_string())?)
        {
            return Err("Replacement target escapes project".into());
        }
        let staged = parent.join(format!(
            ".asset-replace-{:032x}.tmp",
            rand::random::<u128>()
        ));
        let old = parent.join(format!(".asset-prior-{:032x}.tmp", rand::random::<u128>()));
        if entry.kind == AssetType::Material {
            let contents = fs::read(source).map_err(|e| e.to_string())?;
            let mut document: crate::project::material::MaterialDocument =
                serde_json::from_slice(&contents).map_err(|e| e.to_string())?;
            document.id = entry.id.clone();
            fs::write(
                &staged,
                serde_json::to_vec_pretty(&document).map_err(|e| e.to_string())?,
            )
            .map_err(|e| e.to_string())?;
        } else {
            fs::copy(source, &staged).map_err(|e| e.to_string())?;
        }
        if let Err(error) = fs::rename(&target, &old) {
            let _ = fs::remove_file(&staged);
            return Err(format!("Cannot stage previous asset: {error}"));
        }
        if let Err(error) = fs::rename(&staged, &target) {
            let _ = fs::rename(&old, &target);
            let _ = fs::remove_file(&staged);
            return Err(format!("Cannot publish replacement: {error}"));
        }
        drop(_guard);
        let publication = Self::reconcile(project)
            .and_then(|_| Self::notify_content_changed(project).map(|_| ()));
        if let Err(error) = publication {
            let _ = fs::remove_file(&target);
            fs::rename(&old, &target).map_err(|restore| {
                format!("Registry update failed: {error}; original asset restore failed: {restore}")
            })?;
            let _ = Self::reconcile(project);
            return Err(format!(
                "Replacement rolled back after registry failure: {error}"
            ));
        }
        let _ = fs::remove_file(old);
        Ok(Self::load(project)?.find(id).cloned().unwrap_or(entry))
    }
    pub fn reconcile(project: &Path) -> Result<Self, String> {
        let _guard = WRITE_LOCK
            .lock()
            .map_err(|_| "Asset registry writer unavailable")?;
        let previous = match Self::load(project) {
            Ok(index) => index,
            Err(error) if error.contains("unavailable") => Self::default(),
            Err(error) => return Err(error),
        };
        let mut existing: BTreeMap<String, AssetRecord> = previous
            .assets
            .into_iter()
            .map(|entry| (entry.path.clone(), entry))
            .collect();
        let mut assets = Vec::new();
        for (path, (kind, name)) in discover(project) {
            let runtime_id = native_asset_id(project, &path, &kind);
            let mut record = existing.remove(&path).unwrap_or_else(|| AssetRecord {
                id: runtime_id
                    .clone()
                    .unwrap_or_else(|| format!("{:032x}", rand::random::<u128>())),
                name: name.clone(),
                kind: kind.clone(),
                path: path.clone(),
                dependencies: Vec::new(),
                missing_dependencies: Vec::new(),
                group: None,
            });
            if let Some(native) = runtime_id {
                record.id = native;
            }
            record.kind = kind;
            record.path = path;
            assets.push(record);
        }
        assets.extend(existing.into_values());
        let mut unique_ids = BTreeSet::new();
        for record in &assets {
            if !unique_ids.insert(record.id.as_str()) {
                return Err(format!(
                    "Duplicate canonical AssetID detected: {}",
                    record.id
                ));
            }
        }
        assets.sort_by(|a, b| {
            a.kind
                .cmp(&b.kind)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        let mut result = Self {
            version: REGISTRY_VERSION,
            revision: 0,
            assets,
        };
        result.resolve_dependencies(project);
        result.save(project)?;
        Self::load(project)
    }
    fn resolve_dependencies(&mut self, project: &Path) {
        let asset_ids: BTreeSet<String> =
            self.assets.iter().map(|asset| asset.id.clone()).collect();
        let actor_ids: BTreeSet<String> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Actor)
            .map(|a| a.id.clone())
            .collect();
        let actors: BTreeMap<String, String> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Actor)
            .map(|a| {
                (
                    Path::new(&a.path)
                        .file_name()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_lowercase(),
                    a.id.clone(),
                )
            })
            .collect();
        let native_ids: BTreeMap<String, AssetType> = self
            .assets
            .iter()
            .filter(|a| matches!(a.kind, AssetType::Controller | AssetType::Camera))
            .map(|a| (a.id.clone(), a.kind.clone()))
            .collect();
        let material_ids: BTreeSet<String> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Material)
            .map(|a| a.id.clone())
            .collect();
        let materials: BTreeMap<String, String> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Material)
            .map(|a| (a.name.to_ascii_lowercase(), a.id.clone()))
            .collect();
        let native: BTreeMap<(AssetType, String), String> = self
            .assets
            .iter()
            .filter(|a| matches!(a.kind, AssetType::Controller | AssetType::Camera))
            .map(|a| {
                (
                    (
                        a.kind.clone(),
                        Path::new(&a.path)
                            .file_name()
                            .unwrap_or_default()
                            .to_string_lossy()
                            .to_ascii_lowercase(),
                    ),
                    a.id.clone(),
                )
            })
            .collect();
        let local_textures: BTreeMap<String, Vec<String>> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Texture && a.path.starts_with(".assets/entities/"))
            .filter_map(|a| {
                a.path
                    .strip_prefix(".assets/entities/")
                    .and_then(|p| p.split_once('/'))
                    .map(|(actor, _)| (actor.to_ascii_lowercase(), a.id.clone()))
            })
            .fold(BTreeMap::new(), |mut map, (actor, id)| {
                map.entry(actor).or_default().push(id);
                map
            });
        let missing_paths: BTreeMap<String, String> = self
            .assets
            .iter()
            .filter(|a| !project.join(&a.path).exists())
            .map(|a| (a.id.clone(), a.path.clone()))
            .collect();
        let textures: BTreeMap<String, Vec<String>> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Texture)
            .fold(BTreeMap::new(), |mut all, asset| {
                let stem = Path::new(&asset.path)
                    .file_stem()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                all.entry(stem).or_default().push(asset.id.clone());
                all
            });
        let texture_paths: BTreeMap<String, String> = self
            .assets
            .iter()
            .filter(|a| a.kind == AssetType::Texture)
            .map(|a| (a.path.to_ascii_lowercase(), a.id.clone()))
            .collect();
        for asset in &mut self.assets {
            if asset.kind == AssetType::Actor {
                let physical_name = Path::new(&asset.path)
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_ascii_lowercase();
                asset.dependencies = local_textures
                    .get(&physical_name)
                    .cloned()
                    .unwrap_or_default();
                asset.missing_dependencies = asset
                    .dependencies
                    .iter()
                    .filter_map(|id| missing_paths.get(id).cloned())
                    .collect();
                let package = project.join(&asset.path);
                if package.join("import.json").is_file() && !package.join("source.glb").is_file() {
                    asset.missing_dependencies.push("source.glb".into());
                }
                continue;
            }
            if asset.kind == AssetType::Character {
                let Ok(bytes) = fs::read(project.join(&asset.path).join("package.json")) else {
                    continue;
                };
                let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                    continue;
                };
                let mut dependencies = Vec::new();
                let mut missing = Vec::new();
                if let Some(actor) = json.get("source_actor").and_then(|x| x.as_str()) {
                    if actor_ids.contains(actor) {
                        dependencies.push(actor.to_owned());
                    } else if let Some(id) = actors.get(&actor.to_ascii_lowercase()) {
                        dependencies.push(id.clone());
                    } else {
                        missing.push(format!("Actor: {actor}"));
                    }
                }
                for field in ["controller", "camera"] {
                    if let Some(name) = json.get(field).and_then(|x| x.as_str()) {
                        let kind = if field == "camera" {
                            AssetType::Camera
                        } else {
                            AssetType::Controller
                        };
                        if native_ids.get(name) == Some(&kind) {
                            dependencies.push(name.to_owned());
                        } else if let Some(id) = native.get(&(kind, name.to_ascii_lowercase())) {
                            dependencies.push(id.clone());
                        } else {
                            missing.push(format!("{field}: {name}"));
                        }
                    }
                }
                if let Some(material) = json.get("material").and_then(|v| v.as_str()) {
                    if material_ids.contains(material) {
                        dependencies.push(material.to_owned());
                    } else if let Some(id) = materials.get(&material.to_ascii_lowercase()) {
                        dependencies.push(id.clone());
                    } else {
                        missing.push(format!("Material: {material}"));
                    }
                }
                asset.dependencies = dependencies;
                asset.missing_dependencies = missing;
                continue;
            }
            if asset.kind == AssetType::Animation {
                let path = project
                    .join(&asset.path)
                    .join(crate::animation::asset::ANIMATION_FILE);
                let Ok(bytes) = fs::read(path) else { continue };
                let Ok(document) =
                    serde_json::from_slice::<crate::animation::AnimationAsset>(&bytes)
                else {
                    continue;
                };
                if asset_ids.contains(&document.source.rig_asset_id) {
                    asset.dependencies = vec![document.source.rig_asset_id];
                    asset.missing_dependencies.clear();
                } else {
                    asset.dependencies.clear();
                    asset.missing_dependencies =
                        vec![format!("Rig: {}", document.source.rig_asset_id)];
                }
                continue;
            }
            if asset.kind != AssetType::Material {
                continue;
            }
            let path = project.join(&asset.path);
            let Ok(bytes) = fs::read(path) else { continue };
            let Ok(json) = serde_json::from_slice::<serde_json::Value>(&bytes) else {
                continue;
            };
            let mut dependencies = BTreeSet::new();
            let mut missing = BTreeSet::new();
            if let Some(values) = json.get("textures").and_then(|v| v.as_object()) {
                for value in values.values().filter_map(|v| v.as_str()) {
                    let stem = Path::new(value)
                        .file_stem()
                        .unwrap_or_default()
                        .to_string_lossy()
                        .to_ascii_lowercase();
                    if let Some(id) = texture_paths.get(&value.to_ascii_lowercase()) {
                        dependencies.insert(id.clone());
                    } else if let Some(ids) = textures.get(&stem) {
                        if ids.len() == 1 {
                            dependencies.insert(ids[0].clone());
                        } else {
                            missing.insert(format!("Ambiguous texture: {value}"));
                        }
                    } else {
                        missing.insert(value.to_owned());
                    }
                }
            }
            asset.dependencies = dependencies.into_iter().collect();
            asset.missing_dependencies = missing.into_iter().collect();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BackgroundIndexStatus {
    Indexing,
    Ready,
    Failed(String),
}

fn background_index_jobs()
-> &'static std::sync::Mutex<std::collections::HashMap<PathBuf, BackgroundIndexStatus>> {
    static JOBS: std::sync::OnceLock<
        std::sync::Mutex<std::collections::HashMap<PathBuf, BackgroundIndexStatus>>,
    > = std::sync::OnceLock::new();
    JOBS.get_or_init(|| std::sync::Mutex::new(std::collections::HashMap::new()))
}

impl AssetRecord {
    pub fn is_actor_owned_texture(&self) -> bool {
        self.kind == AssetType::Texture
            && self
                .path
                .replace('\\', "/")
                .starts_with(".assets/entities/")
    }
}

impl AssetRegistry {
    pub fn actor_owned_textures<'a>(&'a self, record: &AssetRecord) -> Vec<&'a AssetRecord> {
        let actor = match record.kind {
            AssetType::Actor => Some(record),
            AssetType::Character => record
                .dependencies
                .iter()
                .filter_map(|id| self.find(id))
                .find(|asset| asset.kind == AssetType::Actor),
            _ => None,
        };
        let Some(actor) = actor else {
            return Vec::new();
        };
        let prefix = format!("{}/textures/", actor.path.trim_end_matches('/'));
        self.assets
            .iter()
            .filter(|asset| asset.is_actor_owned_texture() && asset.path.starts_with(&prefix))
            .collect()
    }

    pub fn begin_background_index(project: PathBuf) {
        let jobs = background_index_jobs();
        {
            let mut jobs = {
                let _wait =
                    crate::engine::development_capture::operation("wait", "asset_index_lock", true);
                jobs.lock().unwrap_or_else(|poison| poison.into_inner())
            };
            if matches!(jobs.get(&project), Some(BackgroundIndexStatus::Indexing)) {
                return;
            }
            jobs.insert(project.clone(), BackgroundIndexStatus::Indexing);
        }
        let worker_path = project.clone();
        let started = std::thread::Builder::new()
            .name("Worldkiln asset inventory".to_owned())
            .spawn(move || {
                let _job = crate::engine::development_capture::operation(
                    "asset",
                    "background_index",
                    false,
                );
                let result = std::panic::catch_unwind(|| Self::reconcile(&worker_path))
                    .unwrap_or_else(|_| Err("Asset inventory worker panicked".to_owned()));
                let status = match result {
                    Ok(registry) => {
                        eprintln!(
                            "[ASSETS] indexed {} assets in background",
                            registry.assets.len()
                        );
                        BackgroundIndexStatus::Ready
                    }
                    Err(error) => {
                        eprintln!("[ASSETS] background indexing failed: {error}");
                        BackgroundIndexStatus::Failed(error)
                    }
                };
                background_index_jobs()
                    .lock()
                    .unwrap_or_else(|poison| poison.into_inner())
                    .insert(worker_path, status);
            });
        if let Err(error) = started {
            jobs.lock()
                .unwrap_or_else(|poison| poison.into_inner())
                .insert(project, BackgroundIndexStatus::Failed(error.to_string()));
        }
    }

    pub fn background_index_status(project: &Path) -> Option<BackgroundIndexStatus> {
        background_index_jobs()
            .lock()
            .unwrap_or_else(|poison| poison.into_inner())
            .get(project)
            .cloned()
    }
}
#[cfg(test)]
mod deletion_consistency_regressions {
    use super::*;

    fn sample(name: &str) -> AssetRecord {
        AssetRecord {
            id: format!("id-{name}"),
            name: name.to_owned(),
            kind: AssetType::Actor,
            path: format!(".assets/entities/{name}"),
            dependencies: vec![],
            missing_dependencies: vec![],
            group: Some("TestGroup".into()),
        }
    }

    #[test]
    fn group_delete_removes_every_package_and_registry_id() {
        let root =
            std::env::temp_dir().join(format!("wk-group-delete-{:032x}", rand::random::<u128>()));
        for name in ["actor_a", "actor_b", "keep_me"] {
            let dir = root.join(".assets/entities").join(name);
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("package.json"), b"{}").unwrap();
        }
        let mut registry = AssetRegistry::default();
        registry.assets = ["actor_a", "actor_b", "keep_me"]
            .into_iter()
            .map(sample)
            .collect();
        registry.save(&root).unwrap();
        let count =
            AssetRegistry::delete_many(&root, &["id-actor_a".into(), "id-actor_b".into()]).unwrap();
        assert_eq!(count, 2);
        let remaining = AssetRegistry::load(&root).unwrap();
        assert!(remaining.find("id-actor_a").is_none());
        assert!(remaining.find("id-actor_b").is_none());
        assert!(remaining.find("id-keep_me").is_some());
        assert!(!root.join(".assets/entities/actor_a").exists());
        assert!(!root.join(".assets/entities/actor_b").exists());
        assert!(root.join(".assets/entities/keep_me").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn historical_development_backup_never_blocks_deletion() {
        let root = std::env::temp_dir().join(format!(
            "wk-backup-deletion-{:032x}",
            rand::random::<u128>()
        ));
        let dir = root.join(".assets/entities/unused_actor");
        fs::create_dir_all(&dir).unwrap();
        fs::write(dir.join("package.json"), b"{}").unwrap();
        let mut registry = AssetRegistry::default();
        registry.assets.push(sample("unused_actor"));
        registry.save(&root).unwrap();
        let backup = root.join(".worldkiln/backups/archive");
        fs::create_dir_all(&backup).unwrap();
        fs::write(
            backup.join("world.dat"),
            b"ACTOR 100 unused_actor 0 0 0 0 0 0 1 1 1 true true unused_actor",
        )
        .unwrap();
        AssetRegistry::delete(&root, "id-unused_actor").unwrap();
        assert!(!dir.exists());
        assert!(AssetRegistry::load(&root).unwrap().assets.is_empty());
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(all(test, target_os = "windows"))]
mod indexing_sharing_regression {
    use super::*;
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    #[test]
    fn asset_index_waits_for_temporary_windows_file_lock() {
        let root = std::env::temp_dir().join(format!(
            "wk_asset_index_lock_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        fs::create_dir_all(root.join(".assets")).unwrap();
        let target = registry_path(&root);
        fs::write(&target, b"pending first index").unwrap();
        let lock = OpenOptions::new()
            .read(true)
            .share_mode(0)
            .open(&target)
            .unwrap();
        let release = std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(130));
            drop(lock);
        });
        let registry = AssetRegistry::default();
        let result = registry.save(&root);
        release.join().unwrap();
        assert!(
            result.is_ok(),
            "Transient registry sharing lock: {result:?}"
        );
        assert_eq!(
            AssetRegistry::load(&root).unwrap().version,
            REGISTRY_VERSION
        );
        fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod animation_asset_registry_tests {
    use super::*;

    #[test]
    fn reconcile_registers_animation_by_its_native_id_and_rig_dependency() {
        let root = std::env::temp_dir().join(format!(
            "wk_animation_registry_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        let asset = crate::animation::AnimationAsset::new("Wave", "missing-rig-id");
        asset.save_authoritative(&root).unwrap();

        let registry = AssetRegistry::reconcile(&root).unwrap();
        let record = registry
            .find(&asset.id)
            .expect("animation uses its document ID");
        assert_eq!(record.kind, AssetType::Animation);
        assert_eq!(record.name, "Wave");
        assert_eq!(record.missing_dependencies, vec!["Rig: missing-rig-id"]);

        fs::remove_dir_all(root).unwrap();
    }
}
