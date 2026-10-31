// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::asset_registry::AssetType;
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    path::{Path, PathBuf},
};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PlannedAsset {
    pub source: PathBuf,
    pub name: String,
    pub kind: AssetType,
    pub conflict: Option<String>,
    #[serde(default)]
    pub incoming_name: Option<String>,
    #[serde(default)]
    pub import_subdir: Option<PathBuf>,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImportExecutionResult {
    pub imported_ids: Vec<String>,
    pub failures: Vec<String>,
    pub warnings: Vec<String>,
    pub revision: u64,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct ImportPlan {
    pub assets: Vec<PlannedAsset>,
    pub warnings: Vec<String>,
}
impl ImportPlan {
    pub fn count(&self, kind: AssetType) -> usize {
        self.assets
            .iter()
            .filter(|asset| asset.kind == kind)
            .count()
    }
    pub fn counts(&self) -> BTreeMap<String, usize> {
        let mut result = BTreeMap::new();
        for item in &self.assets {
            *result.entry(item.kind.category().to_owned()).or_insert(0) += 1;
        }
        result
    }
    pub fn can_import(&self) -> bool {
        !self.assets.is_empty() && self.assets.iter().all(|a| a.conflict.is_none())
    }
}
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IncomingCollision {
    pub identity: String,
    pub files: Vec<PathBuf>,
    pub identical_contents: bool,
}

fn same_file_bytes(first: &Path, second: &Path) -> Result<bool, String> {
    use std::io::Read;
    for source in [first, second] {
        let meta = fs::symlink_metadata(source)
            .map_err(|e| format!("Cannot inspect collision source {}: {e}", source.display()))?;
        if !meta.file_type().is_file() {
            return Err(format!(
                "Not a regular collision source: {}",
                source.display()
            ));
        }
    }
    let first_size = fs::metadata(first).map_err(|e| e.to_string())?.len();
    let second_size = fs::metadata(second).map_err(|e| e.to_string())?.len();
    if first_size != second_size {
        return Ok(false);
    }
    let mut first_reader =
        std::io::BufReader::new(fs::File::open(first).map_err(|e| e.to_string())?);
    let mut second_reader =
        std::io::BufReader::new(fs::File::open(second).map_err(|e| e.to_string())?);
    let mut left = [0u8; 65536];
    let mut right = [0u8; 65536];
    loop {
        let count = first_reader.read(&mut left).map_err(|e| e.to_string())?;
        if count == 0 {
            return Ok(true);
        }
        second_reader
            .read_exact(&mut right[..count])
            .map_err(|e| e.to_string())?;
        if left[..count] != right[..count] {
            return Ok(false);
        }
    }
}

pub fn incoming_collisions(plan: &ImportPlan) -> Result<Vec<IncomingCollision>, String> {
    let mut identities: BTreeMap<String, Vec<PathBuf>> = BTreeMap::new();
    for asset in &plan.assets {
        let key = incoming_identity(asset);
        identities
            .entry(key)
            .or_default()
            .push(asset.source.clone());
    }
    let mut conflicts = Vec::new();
    for (identity, mut files) in identities {
        if files.len() < 2 {
            continue;
        }
        files.sort();
        let first = &files[0];
        let mut identical_contents = true;
        for candidate in &files[1..] {
            if !same_file_bytes(first, candidate)? {
                identical_contents = false;
                break;
            }
        }
        conflicts.push(IncomingCollision {
            identity,
            files,
            identical_contents,
        });
    }
    Ok(conflicts)
}

pub fn incoming_identity(asset: &PlannedAsset) -> String {
    let name = if asset.kind == AssetType::Actor {
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
    let relative = if asset.kind == AssetType::Actor {
        PathBuf::from(name)
    } else if let Some(folder) = &asset.import_subdir {
        folder.join(name)
    } else {
        PathBuf::from(name)
    };
    format!(
        "{}:{}",
        asset.kind.category(),
        relative
            .to_string_lossy()
            .replace('\\', "/")
            .to_ascii_lowercase()
    )
}

pub fn refresh_incoming_conflicts(plan: &mut ImportPlan) {
    let mut seen = BTreeSet::new();
    for asset in &mut plan.assets {
        if asset.conflict.as_deref()
            == Some("Another incoming file would create the same asset identity")
        {
            asset.conflict = None;
        }
        let identity = incoming_identity(asset);
        if !seen.insert(identity) {
            asset.conflict =
                Some("Another incoming file would create the same asset identity".into());
        }
    }
}

pub fn keep_incoming_winner(plan: &mut ImportPlan, source: &Path) -> Result<(), String> {
    let identity = plan
        .assets
        .iter()
        .find(|asset| asset.source == source)
        .map(incoming_identity)
        .ok_or("Incoming file is no longer in this review")?;
    plan.assets
        .retain(|asset| asset.source == source || incoming_identity(asset) != identity);
    refresh_incoming_conflicts(plan);
    Ok(())
}

pub fn rename_incoming(asset: &mut PlannedAsset, proposed: &str) -> Result<(), String> {
    let name = proposed.trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
    {
        return Err("Enter a valid asset name".into());
    }
    if asset.kind == AssetType::Actor {
        if !name
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err("Actor names use letters, numbers, underscores or dashes".into());
        }
        asset.name = name.to_owned();
    } else {
        let extension = asset
            .source
            .extension()
            .and_then(|s| s.to_str())
            .ok_or("Missing original file extension")?;
        if !Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .is_some_and(|ext| ext.eq_ignore_ascii_case(extension))
        {
            return Err("Keep the original file extension when renaming".into());
        }
        let stem = Path::new(name)
            .file_stem()
            .and_then(|s| s.to_str())
            .ok_or("Invalid asset filename")?;
        if stem.is_empty() || matches!(stem, "." | "..") {
            return Err("Enter a valid asset filename".into());
        }
        asset.name = stem
            .chars()
            .map(|c| {
                if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        asset.incoming_name = Some(name.to_owned());
    }
    Ok(())
}

pub fn rebase_selected_import(
    project: &Path,
    chosen: &ImportPlan,
    original_roots: &[PathBuf],
    snapshot_roots: &[PathBuf],
) -> Result<ImportPlan, String> {
    if original_roots.len() != snapshot_roots.len() {
        return Err("Inventory source roots changed during import".into());
    }
    let mut snapshot = plan_import(project, snapshot_roots)?;
    let mut selected = BTreeMap::new();
    for asset in &chosen.assets {
        let mut mapped = None;
        for (source, stored) in original_roots.iter().zip(snapshot_roots) {
            if asset.source == *source {
                mapped = Some(stored.clone());
                break;
            }
            if source.is_dir() {
                if let Ok(relative) = asset.source.strip_prefix(source) {
                    mapped = Some(stored.join(relative));
                    break;
                }
            }
        }
        let mapped = mapped.ok_or("Reviewed source no longer belongs to the selected import")?;
        let mut entry = asset.clone();
        entry.source = mapped.clone();
        if selected.insert(mapped, entry).is_some() {
            return Err("A reviewed source occurs more than once".into());
        }
    }
    let mut accepted = Vec::new();
    for scanned in snapshot.assets.drain(..) {
        if let Some(mut entry) = selected.remove(&scanned.source) {
            if scanned.kind != entry.kind {
                return Err("An incoming asset type changed during import".into());
            }
            entry.source = scanned.source;
            accepted.push(entry);
        }
    }
    if !selected.is_empty() {
        return Err("A selected source was missing from the Inventory snapshot".into());
    }
    snapshot.assets = accepted;
    refresh_incoming_conflicts(&mut snapshot);
    Ok(snapshot)
}

fn inspect(
    project: &Path,
    path: &Path,
    subdir: Option<&Path>,
    plan: &mut ImportPlan,
    seen: &mut BTreeSet<String>,
) {
    let Some(ext) = path.extension().and_then(|x| x.to_str()) else {
        return;
    };
    let kind = match ext.to_ascii_lowercase().as_str() {
        "glb" | "gltf" => AssetType::Actor,
        "png" | "jpg" | "jpeg" => AssetType::Image,
        "wav" | "mp3" => AssetType::Audio,
        "mp4" => AssetType::Video,
        "efkefc" | "efkpkg" => AssetType::Effect,
        _ => return,
    };
    if kind == AssetType::Actor && ext.eq_ignore_ascii_case("gltf") {
        plan.warnings.push(format!(
            "{}: GLTF requires binary GLB import",
            path.display()
        ));
        return;
    }
    let name = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let safe = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
                c
            } else {
                '_'
            }
        })
        .collect::<String>();
    let folder = match kind {
        AssetType::Actor => "entities",
        AssetType::Image | AssetType::Video => "media",
        AssetType::Audio => "audio",
        AssetType::Effect => "effects",
        AssetType::Script => "scripts",
        _ => "other",
    };
    let import_subdir = if matches!(kind, AssetType::Image | AssetType::Video | AssetType::Audio) {
        subdir.map(Path::to_path_buf)
    } else {
        None
    };
    let target = if kind == AssetType::Actor {
        project.join(".assets").join(folder).join(&safe)
    } else if kind == AssetType::Script {
        project
            .join(folder)
            .join(path.file_name().unwrap_or_default())
    } else {
        project
            .join(".assets")
            .join(folder)
            .join(import_subdir.as_deref().unwrap_or(Path::new("")))
            .join(path.file_name().unwrap_or_default())
    };
    let identity = if kind == AssetType::Actor {
        format!("{}:{}", kind.category(), safe.to_ascii_lowercase())
    } else {
        let relative = import_subdir
            .as_deref()
            .unwrap_or(Path::new(""))
            .join(path.file_name().unwrap_or_default());
        format!(
            "{}:{}",
            kind.category(),
            relative
                .to_string_lossy()
                .replace('\\', "/")
                .to_ascii_lowercase()
        )
    };
    let conflict = if !seen.insert(identity) {
        Some("Another incoming file would create the same asset identity".into())
    } else {
        None
    };
    if target.exists() {
        plan.warnings.push(format!(
            "{}: existing asset will be replaced on import",
            target.display()
        ));
    }
    plan.assets.push(PlannedAsset {
        source: path.to_owned(),
        name: safe,
        kind,
        conflict,
        incoming_name: None,
        import_subdir,
    });
}
pub fn plan_import(project: &Path, sources: &[PathBuf]) -> Result<ImportPlan, String> {
    let mut result = ImportPlan::default();
    let mut seen = BTreeSet::new();
    let mut pending: Vec<(PathBuf, usize, Option<PathBuf>)> = sources
        .iter()
        .map(|path| {
            let prefix = path
                .is_dir()
                .then(|| path.file_name().map(PathBuf::from))
                .flatten();
            (path.clone(), 0, prefix)
        })
        .collect();
    let mut inspected = 0usize;
    while let Some((path, depth, subdir)) = pending.pop() {
        if depth > 24 || inspected > 10000 {
            return Err("Import folder exceeds safe scan limits".into());
        }
        inspected += 1;
        let meta = fs::symlink_metadata(&path)
            .map_err(|e| format!("Cannot inspect '{}': {e}", path.display()))?;
        if meta.file_type().is_symlink() {
            result
                .warnings
                .push(format!("Skipped linked source: {}", path.display()));
        } else if meta.is_dir() {
            for entry in fs::read_dir(&path).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let child = entry.path();
                let child_subdir = if child.is_dir() {
                    subdir.as_ref().map(|part| part.join(entry.file_name()))
                } else {
                    subdir.clone()
                };
                pending.push((child, depth + 1, child_subdir));
            }
        } else if meta.is_file() {
            inspect(project, &path, subdir.as_deref(), &mut result, &mut seen);
        }
    }
    result
        .assets
        .sort_by(|a, b| a.kind.cmp(&b.kind).then_with(|| a.name.cmp(&b.name)));
    Ok(result)
}

#[cfg(test)]
mod collision_tests {
    use super::*;

    #[test]
    fn identical_duplicate_filenames_are_identified_without_changing_sources() {
        let root =
            std::env::temp_dir().join(format!("wk-collision-{:032x}", rand::random::<u128>()));
        let project = root.join("world");
        let left = root.join("pack/buildings/blue");
        let right = root.join("pack/buildings/red");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        fs::write(left.join("hexagons_medieval.png"), b"same contents").unwrap();
        fs::write(right.join("hexagons_medieval.png"), b"same contents").unwrap();
        let plan = plan_import(&project, &[root.join("pack")]).unwrap();
        assert_eq!(plan.assets.len(), 2);
        assert!(plan.can_import());
        assert!(incoming_collisions(&plan).unwrap().is_empty());
        assert_ne!(plan.assets[0].import_subdir, plan.assets[1].import_subdir);
        assert!(!project.join(".assets").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn incoming_choices_survive_source_snapshot_without_changing_originals() {
        let root =
            std::env::temp_dir().join(format!("wk-incoming-{:032x}", rand::random::<u128>()));
        let project = root.join("project");
        let source = root.join("download");
        let snapshot = root.join("inventory_copy");
        fs::create_dir_all(&project).unwrap();
        for folder in ["OBJ", "GLB", "FBX"] {
            let dir = source.join(folder).join("Textures");
            fs::create_dir_all(&dir).unwrap();
            fs::write(dir.join("colormap.png"), folder.as_bytes()).unwrap();
            let copied = snapshot.join(folder).join("Textures");
            fs::create_dir_all(&copied).unwrap();
            fs::write(copied.join("colormap.png"), folder.as_bytes()).unwrap();
        }
        let mut chosen = plan_import(&project, std::slice::from_ref(&source)).unwrap();
        assert_eq!(chosen.assets.len(), 3);
        assert!(chosen.can_import());
        let glb = source.join("GLB/Textures/colormap.png");
        rename_incoming(
            chosen.assets.iter_mut().find(|a| a.source == glb).unwrap(),
            "colormap_glb.png",
        )
        .unwrap();
        let obj = source.join("OBJ/Textures/colormap.png");
        chosen.assets.retain(|a| a.source != obj);
        refresh_incoming_conflicts(&mut chosen);
        assert!(chosen.can_import());
        let rebased =
            rebase_selected_import(&project, &chosen, &[source.clone()], &[snapshot]).unwrap();
        assert_eq!(rebased.assets.len(), 2);
        assert!(rebased.can_import());
        assert!(
            rebased
                .assets
                .iter()
                .any(|a| a.incoming_name.as_deref() == Some("colormap_glb.png"))
        );
        assert!(
            rebased
                .assets
                .iter()
                .any(|a| a.incoming_name.is_none() && a.source.to_string_lossy().contains("FBX"))
        );
        assert_eq!(
            fs::read(source.join("GLB/Textures/colormap.png")).unwrap(),
            b"GLB"
        );
        assert!(!project.join(".assets").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn replace_selects_an_incoming_winner_without_touching_existing_assets() {
        let root = std::env::temp_dir().join(format!(
            "wk-incoming-winner-{:032x}",
            rand::random::<u128>()
        ));
        let project = root.join("project");
        let a = root.join("set_a");
        let b = root.join("set_b");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&a).unwrap();
        fs::create_dir_all(&b).unwrap();
        fs::write(a.join("color.png"), b"first").unwrap();
        fs::write(b.join("color.png"), b"second").unwrap();
        let mut planned =
            plan_import(&project, &[a.join("color.png"), b.join("color.png")]).unwrap();
        assert!(!planned.can_import());
        keep_incoming_winner(&mut planned, &b.join("color.png")).unwrap();
        assert_eq!(planned.assets.len(), 1);
        assert!(planned.can_import());
        assert_eq!(planned.assets[0].source, b.join("color.png"));
        assert_eq!(fs::read(a.join("color.png")).unwrap(), b"first");
        assert_eq!(fs::read(b.join("color.png")).unwrap(), b"second");
        assert!(!project.join(".assets").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn renamed_file_keeps_original_extension_and_source() {
        let source = PathBuf::from("models/colormap.png");
        let mut asset = PlannedAsset {
            source: source.clone(),
            name: "colormap".into(),
            kind: AssetType::Image,
            conflict: None,
            incoming_name: None,
            import_subdir: None,
        };
        assert!(rename_incoming(&mut asset, "colormap.jpg").is_err());
        assert!(rename_incoming(&mut asset, "../colormap.png").is_err());
        rename_incoming(&mut asset, "colormap_glb.png").unwrap();
        assert_eq!(asset.incoming_name.as_deref(), Some("colormap_glb.png"));
        assert_eq!(asset.source, source);
    }

    #[test]
    fn separate_folders_allow_same_filename_with_different_contents() {
        let root =
            std::env::temp_dir().join(format!("wk-collision-{:032x}", rand::random::<u128>()));
        let project = root.join("world");
        let left = root.join("pack/buildings/blue");
        let right = root.join("pack/buildings/red");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&left).unwrap();
        fs::create_dir_all(&right).unwrap();
        fs::write(left.join("texture.png"), b"blue color").unwrap();
        fs::write(right.join("texture.png"), b"red color").unwrap();
        let plan = plan_import(&project, &[root.join("pack")]).unwrap();
        assert_eq!(plan.assets.len(), 2);
        let groups = incoming_collisions(&plan).unwrap();
        assert!(groups.is_empty());
        assert!(plan.can_import());
        assert_ne!(plan.assets[0].import_subdir, plan.assets[1].import_subdir);
        assert_eq!(fs::read(left.join("texture.png")).unwrap(), b"blue color");
        assert_eq!(fs::read(right.join("texture.png")).unwrap(), b"red color");
        fs::remove_dir_all(root).unwrap();
    }
}
