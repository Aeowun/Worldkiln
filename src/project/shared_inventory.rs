// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::{Component, Path, PathBuf},
};

const MANIFEST: &str = "inventory.json";
const MAX_FILES: usize = 20000;
const MAX_DEPTH: usize = 24;
const MAX_BYTES: u64 = 16 * 1024 * 1024 * 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Item {
    pub schema: u32,
    pub id: String,
    pub name: String,
    pub roots: Vec<String>,
    pub files: usize,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct CachedPack {
    pub reference: super::asset_packs::Reference,
    pub name: String,
    pub category: String,
    pub files: usize,
}

pub fn root() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("Aeowun/AssetInventory")
}

fn checked(path: &Path) -> Result<fs::Metadata, String> {
    let m = fs::symlink_metadata(path).map_err(|e| format!("{}: {e}", path.display()))?;
    if m.file_type().is_symlink() {
        return Err("Linked inventory source".into());
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if m.file_attributes() & 0x400 != 0 {
            return Err("Reparse inventory source".into());
        }
    }
    Ok(m)
}
fn safe_relative(value: &str) -> Result<PathBuf, String> {
    let p = Path::new(value);
    if !p
        .components()
        .all(|part| matches!(part, Component::Normal(_)))
    {
        return Err("Invalid inventory relative path".into());
    }
    Ok(p.to_path_buf())
}
fn enumerate(
    path: &Path,
    relative: &Path,
    depth: usize,
    files: &mut Vec<(PathBuf, PathBuf, u64)>,
    bytes: &mut u64,
) -> Result<(), String> {
    if depth > MAX_DEPTH || files.len() >= MAX_FILES {
        return Err("Inventory file/depth limit reached".into());
    }
    let m = checked(path)?;
    if m.is_dir() {
        let mut children = fs::read_dir(path)
            .map_err(|e| e.to_string())?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|e| e.to_string())?;
        children.sort_by_key(|e| e.file_name());
        for child in children {
            let part = child.file_name();
            enumerate(&child.path(), &relative.join(part), depth + 1, files, bytes)?;
        }
    } else if m.is_file() {
        *bytes = bytes
            .checked_add(m.len())
            .filter(|v| *v <= MAX_BYTES)
            .ok_or("Inventory exceeds 16 GiB")?;
        files.push((path.to_owned(), relative.to_owned(), m.len()));
    } else {
        return Err("Unsupported inventory file".into());
    }
    Ok(())
}

pub fn store_at(base: &Path, sources: &[PathBuf]) -> Result<Item, String> {
    if sources.is_empty() || sources.len() > 1024 {
        return Err("No valid inventory sources".into());
    }
    if base.exists() {
        checked(base)?;
    }
    let mut files = Vec::new();
    let mut bytes = 0u64;
    let mut roots = Vec::new();
    for (index, source) in sources.iter().enumerate() {
        let name = source.file_name().ok_or("Missing inventory source name")?;
        let relative = PathBuf::from("files").join(index.to_string()).join(name);
        enumerate(source, &relative, 0, &mut files, &mut bytes)?;
        roots.push(relative.to_string_lossy().replace('\\', "/"));
    }
    if files.is_empty() {
        return Err("Empty inventory collection".into());
    }
    let id = format!("user-{:032x}", rand::random::<u128>());
    let item = Item {
        schema: 1,
        id: id.clone(),
        name: if sources.len() == 1 {
            sources[0]
                .file_name()
                .unwrap()
                .to_string_lossy()
                .into_owned()
        } else {
            format!("{} selected sources", sources.len())
        },
        roots,
        files: files.len(),
        bytes,
    };
    fs::create_dir_all(base).map_err(|e| e.to_string())?;
    let staging = base.join(format!(".staging-{id}"));
    fs::create_dir(&staging).map_err(|e| e.to_string())?;
    let result = (|| -> Result<(), String> {
        for (src, relative, len) in &files {
            let target = staging.join(relative);
            fs::create_dir_all(target.parent().ok_or("Invalid inventory destination")?)
                .map_err(|e| e.to_string())?;
            let copied = fs::copy(src, &target).map_err(|e| e.to_string())?;
            if copied != *len {
                return Err("Source changed during import".into());
            }
        }
        super::atomic_file::write_atomically(
            &staging.join(MANIFEST),
            &serde_json::to_vec_pretty(&item).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        let final_path = base.join(&id);
        if final_path.exists() {
            return Err("Inventory ID collision".into());
        }
        fs::rename(&staging, &final_path).map_err(|e| e.to_string())
    })();
    if result.is_err() {
        let _ = fs::remove_dir_all(&staging);
    }
    result?;
    Ok(item)
}
pub fn store(sources: &[PathBuf]) -> Result<Item, String> {
    store_at(&root(), sources)
}

pub fn sources_at(base: &Path, id: &str) -> Result<Vec<PathBuf>, String> {
    if !id.starts_with("user-") || id.len() != 37 || !id[5..].bytes().all(|c| c.is_ascii_hexdigit())
    {
        return Err("Invalid inventory ID".into());
    }
    let folder = base.join(id);
    checked(&folder)?;
    let item: Item =
        serde_json::from_slice(&fs::read(folder.join(MANIFEST)).map_err(|e| e.to_string())?)
            .map_err(|e| e.to_string())?;
    if item.schema != 1 || item.id != id {
        return Err("Inventory metadata mismatch".into());
    }
    item.roots
        .iter()
        .map(|r| {
            let p = folder.join(safe_relative(r)?);
            checked(&p)?;
            Ok(p)
        })
        .collect()
}
pub fn sources(id: &str) -> Result<Vec<PathBuf>, String> {
    sources_at(&root(), id)
}
pub fn imported_at(base: &Path) -> Result<Vec<Item>, String> {
    if !base.exists() {
        return Ok(Vec::new());
    }
    let mut results = Vec::new();
    for d in fs::read_dir(base).map_err(|e| e.to_string())? {
        let d = d.map_err(|e| e.to_string())?;
        if !checked(&d.path())?.is_dir() {
            continue;
        }
        let name = d.file_name().to_string_lossy().into_owned();
        if !name.starts_with("user-") {
            continue;
        }
        let item: Item =
            serde_json::from_slice(&fs::read(d.path().join(MANIFEST)).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if item.id != name || item.schema != 1 {
            return Err("Invalid inventory entry".into());
        }
        sources_at(base, &name)?;
        results.push(item);
    }
    results.sort_by(|a, b| a.name.cmp(&b.name).then(a.id.cmp(&b.id)));
    Ok(results)
}
pub fn imported() -> Result<Vec<Item>, String> {
    imported_at(&root())
}

pub fn installed_packs_at(cache: &Path) -> Result<Vec<CachedPack>, String> {
    if !cache.exists() {
        return Ok(Vec::new());
    }
    let mut packs = Vec::new();
    for dir in fs::read_dir(cache).map_err(|e| e.to_string())? {
        let dir = dir.map_err(|e| e.to_string())?;
        if !checked(&dir.path())?.is_dir() {
            continue;
        }
        for ver in fs::read_dir(dir.path()).map_err(|e| e.to_string())? {
            let ver = ver.map_err(|e| e.to_string())?;
            if !checked(&ver.path())?.is_dir() {
                continue;
            }
            let path = ver.path().join("installed-pack.json");
            if !path.is_file() {
                continue;
            }
            let pack: super::asset_packs::Pack =
                serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
                    .map_err(|e| e.to_string())?;
            let reference = pack.reference();
            if super::asset_packs::installed(cache, &reference) {
                packs.push(CachedPack {
                    name: pack.name,
                    category: pack.category,
                    files: pack.entries.len(),
                    reference,
                });
            }
        }
    }
    packs.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(packs)
}
pub fn installed_packs() -> Result<Vec<CachedPack>, String> {
    installed_packs_at(&super::asset_packs::cache_root())
}

pub fn add_installed_pack_to_project(
    project: &Path,
    chosen: &super::asset_packs::Reference,
) -> Result<(), String> {
    let cache = super::asset_packs::cache_root();
    if !super::asset_packs::installed(&cache, chosen) {
        return Err("Selected catalogue pack is not installed".into());
    }
    let mut bound = super::asset_packs::references(project)?;
    if let Some(existing) = bound.iter().find(|r| r.id == chosen.id) {
        if existing == chosen {
            return Ok(());
        }
        return Err("Another version of this pack is already bound to the project".into());
    }
    bound.push(chosen.clone());
    super::asset_packs::bind(&cache, project, bound)
}

#[derive(Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
struct ProjectInventory {
    schema: u32,
    collections: Vec<String>,
}

fn project_inventory_path(project: &Path) -> PathBuf {
    project.join(".assets").join("inventory-collections.json")
}

pub fn project_collections(project: &Path) -> Result<Vec<String>, String> {
    let path = project_inventory_path(project);
    if !path.exists() {
        return Ok(Vec::new());
    }
    let index: ProjectInventory =
        serde_json::from_slice(&fs::read(&path).map_err(|e| e.to_string())?)
            .map_err(|e| format!("Project inventory index invalid: {e}"))?;
    if index.schema != 1 {
        return Err("Unsupported project inventory index".into());
    }
    Ok(index.collections)
}

pub fn record_project_collection(project: &Path, id: &str) -> Result<(), String> {
    sources(id)?;
    let mut collections = project_collections(project)?;
    if collections.iter().any(|value| value == id) {
        return Ok(());
    }
    collections.push(id.to_owned());
    let path = project_inventory_path(project);
    fs::create_dir_all(path.parent().ok_or("Invalid inventory index path")?)
        .map_err(|e| e.to_string())?;
    super::atomic_file::write_atomically(
        &path,
        &serde_json::to_vec_pretty(&ProjectInventory {
            schema: 1,
            collections,
        })
        .map_err(|e| e.to_string())?,
    )
    .map_err(|e| e.to_string())
}

pub fn validate_project_import(
    project: &Path,
    plan: &super::asset_import_plan::ImportPlan,
) -> Result<(), String> {
    use super::asset_registry::{AssetRegistry, AssetType};
    if !plan.can_import() {
        let reason = plan
            .assets
            .iter()
            .find_map(|a| a.conflict.as_deref())
            .unwrap_or("No supported assets were selected");
        return Err(format!("Cannot add collection to this project: {reason}"));
    }
    let registry = AssetRegistry::load(project)?;
    for entry in &plan.assets {
        let folder = match entry.kind {
            AssetType::Actor => "entities",
            AssetType::Image | AssetType::Video => "media",
            AssetType::Audio => "audio",
            AssetType::Effect => "effects",
            _ => return Err(format!("Unsupported asset type: {}", entry.name)),
        };
        let basename = if entry.kind == AssetType::Actor {
            entry.name.clone()
        } else {
            entry.incoming_name.clone().unwrap_or_else(|| {
                entry
                    .source
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned()
            })
        };
        let dest = project
            .join(".assets")
            .join(folder)
            .join(entry.import_subdir.as_deref().unwrap_or(Path::new("")))
            .join(&basename);
        if dest.exists() {
            return Err(format!(
                "{} already exists in this project; existing asset preserved",
                basename
            ));
        }
        let relative = dest
            .strip_prefix(project)
            .map_err(|_| "Incoming asset escapes project".to_owned())?
            .to_string_lossy()
            .replace('\\', "/");
        if registry
            .assets
            .iter()
            .any(|record| record.kind == entry.kind && record.path.eq_ignore_ascii_case(&relative))
        {
            return Err(format!(
                "{} is already registered in this project; existing asset preserved",
                basename
            ));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn import_preserves_source_tree_and_does_not_touch_projects() {
        let base = std::env::temp_dir().join(format!("inventory-{:032x}", rand::random::<u128>()));
        let sources = base.join("download");
        let project = base.join("world");
        fs::create_dir_all(sources.join("a")).unwrap();
        fs::create_dir_all(sources.join("b")).unwrap();
        fs::create_dir_all(&project).unwrap();
        fs::write(sources.join("a/texture.png"), b"abc").unwrap();
        fs::write(sources.join("b/texture.png"), b"def").unwrap();
        let inventory = base.join("inventory");
        let item = store_at(&inventory, &[sources.clone()]).unwrap();
        assert_eq!(item.files, 2);
        let mounted = sources_at(&inventory, &item.id).unwrap();
        assert_eq!(fs::read(mounted[0].join("a/texture.png")).unwrap(), b"abc");
        assert_eq!(fs::read(mounted[0].join("b/texture.png")).unwrap(), b"def");
        assert!(project.join(".assets").exists() == false);
        assert_eq!(imported_at(&inventory).unwrap().len(), 1);
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn project_import_refuses_existing_asset_and_keeps_both_projects_separate() {
        let base =
            std::env::temp_dir().join(format!("inventory-project-{:032x}", rand::random::<u128>()));
        let incoming = base.join("incoming");
        let project_a = base.join("world_a");
        let project_b = base.join("world_b");
        fs::create_dir_all(&incoming).unwrap();
        fs::create_dir_all(&project_a).unwrap();
        fs::create_dir_all(&project_b).unwrap();
        super::super::asset_registry::AssetRegistry::default()
            .save(&project_a)
            .unwrap();
        fs::write(incoming.join("pic.png"), b"source").unwrap();
        let imported = store_at(&base.join("inventory"), &[incoming.clone()]).unwrap();
        let sources = sources_at(&base.join("inventory"), &imported.id).unwrap();
        let plan = super::super::asset_import_plan::plan_import(&project_a, &sources).unwrap();
        assert_eq!(plan.assets.len(), 1);
        assert!(validate_project_import(&project_a, &plan).is_ok());
        let imported_file = project_a
            .join(".assets/media")
            .join(plan.assets[0].import_subdir.as_deref().unwrap_or(Path::new("")))
            .join(plan.assets[0].incoming_name.as_deref().unwrap_or("pic.png"));
        fs::create_dir_all(imported_file.parent().unwrap()).unwrap();
        fs::write(&imported_file, b"owned").unwrap();
        let error = validate_project_import(&project_a, &plan).unwrap_err();
        assert!(error.contains("already exists"));
        assert_eq!(
            fs::read(&imported_file).unwrap(),
            b"owned"
        );
        assert!(!project_b.join(".assets").exists());
        fs::remove_dir_all(base).unwrap();
    }
    #[test]
    fn project_inventory_index_reads_without_creating_files() {
        let project =
            std::env::temp_dir().join(format!("inventory-lookup-{:032x}", rand::random::<u128>()));
        fs::create_dir_all(&project).unwrap();
        assert!(project_collections(&project).unwrap().is_empty());
        assert!(!project.join(".assets").exists());
        fs::remove_dir_all(project).unwrap();
    }
    #[test]
    fn empty_inventory_and_cache_are_valid() {
        let base =
            std::env::temp_dir().join(format!("inventory-empty-{:032x}", rand::random::<u128>()));
        assert!(imported_at(&base).unwrap().is_empty());
        assert!(installed_packs_at(&base).unwrap().is_empty());
    }
}
