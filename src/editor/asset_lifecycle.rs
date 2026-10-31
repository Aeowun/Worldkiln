// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::{AssetUsage, History, World};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub(super) fn validate_relative(value: &str) -> Result<(), String> {
    crate::project::asset_packs::safe_relative(value)?;
    let parts: Vec<_> = value.split('/').collect();
    let permitted = (parts.len() >= 3
        && parts[0] == ".assets"
        && [
            "textures",
            "materials",
            "effects",
            "audio",
            "media",
            "entities",
            "characters",
            "controllers",
            "cameras",
            "Models",
        ]
        .contains(&parts[1]))
        || (parts.len() >= 2 && ["scripts", "playerscripts"].contains(&parts[0]));
    if !permitted {
        return Err("Choose an asset inside a project asset category or script folder.".into());
    }
    Ok(())
}

pub(super) fn checked(project: &Path, relative: &str) -> Result<PathBuf, String> {
    validate_relative(relative)?;
    let result = project.join(relative);
    for path in result.ancestors() {
        if let Ok(meta) = fs::symlink_metadata(path) {
            if meta.file_type().is_symlink() {
                return Err("Linked asset paths cannot be modified.".into());
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if meta.file_attributes() & 0x400 != 0 {
                    return Err("Reparse asset paths cannot be modified.".into());
                }
            }
        }
    }
    Ok(result)
}

fn replacement(value: &str, old: &str, new: &str) -> Option<String> {
    if value == old {
        Some(new.into())
    } else {
        value
            .strip_prefix(old)
            .filter(|rest| rest.starts_with('/'))
            .map(|rest| format!("{new}{rest}"))
    }
}

fn rewrite_json(value: &mut serde_json::Value, old: &str, new: &str) -> usize {
    match value {
        serde_json::Value::String(text) => replacement(text, old, new)
            .map(|changed| {
                *text = changed;
                1
            })
            .unwrap_or(0),
        serde_json::Value::Array(items) => {
            items.iter_mut().map(|v| rewrite_json(v, old, new)).sum()
        }
        serde_json::Value::Object(items) => {
            items.values_mut().map(|v| rewrite_json(v, old, new)).sum()
        }
        _ => 0,
    }
}

fn metadata_files(
    project: &Path,
    directory: &Path,
    depth: usize,
    result: &mut Vec<PathBuf>,
) -> Result<(), String> {
    if depth > 32 {
        return Err("Asset folder nesting exceeds the safe operation limit.".into());
    }
    for entry in fs::read_dir(directory).map_err(|e| e.to_string())? {
        let entry = entry.map_err(|e| e.to_string())?;
        let path = entry.path();
        if path
            .file_name()
            .is_some_and(|n| n.to_string_lossy().starts_with('.') && n != ".assets")
        {
            continue;
        }
        let relative = path
            .strip_prefix(project)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let meta = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
        if meta.file_type().is_symlink() {
            continue;
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if meta.file_attributes() & 0x400 != 0 {
                continue;
            }
        }
        if meta.is_dir() {
            metadata_files(project, &path, depth + 1, result)?;
        } else if relative.ends_with(".json") && meta.len() <= 8 * 1024 * 1024 {
            result.push(path);
        }
    }
    Ok(())
}

pub(super) fn usages(
    project: &Path,
    relative: &str,
    world: &World,
) -> Result<Vec<AssetUsage>, String> {
    checked(project, relative)?;
    let mut result = Vec::new();
    let logical = relative.splitn(3, '/').nth(2).unwrap_or(relative);
    if let Some(fx_id) = relative.strip_prefix(".assets/effects/") {
        for (coord, cell) in world.iter_authored_cells() {
            if cell
                .particles
                .effect_asset
                .as_deref()
                .is_some_and(|asset| replacement(asset, fx_id, fx_id).is_some())
            {
                result.push(AssetUsage {
                    description: format!("Cell ({},{},{}) FX Type", coord.x, coord.y, coord.z),
                });
            }
        }
        for source in world.iter_sources() {
            if source
                .cell
                .particles
                .effect_asset
                .as_deref()
                .is_some_and(|asset| replacement(asset, fx_id, fx_id).is_some())
            {
                result.push(AssetUsage {
                    description: format!("Emitter #{} FX Type", source.cell.id),
                });
            }
        }
    }
    for actor in world.actors.values() {
        if actor
            .package
            .as_deref()
            .is_some_and(|p| replacement(p, logical, logical).is_some())
        {
            result.push(AssetUsage {
                description: format!("Actor #{} package", actor.id),
            });
        }
    }
    for (label, value) in [
        ("Selected Character", &world.selected_character),
        ("Selected Controller", &world.selected_controller),
        ("Selected Camera", &world.selected_camera),
    ] {
        if replacement(value, logical, logical).is_some() {
            result.push(AssetUsage {
                description: label.into(),
            });
        }
    }
    for binding in &world.script_bindings {
        if replacement(&binding.script_path, relative, relative).is_some() {
            result.push(AssetUsage {
                description: format!(
                    "Cell #{} script {}",
                    binding.target_identity, binding.script_path
                ),
            });
        }
    }
    for binding in &world.actor_script_bindings {
        if replacement(&binding.script_path, relative, relative).is_some() {
            result.push(AssetUsage {
                description: format!("Actor #{} script {}", binding.actor_id, binding.script_path),
            });
        }
    }
    let mut files = Vec::new();
    metadata_files(project, project, 0, &mut files)?;
    for path in files {
        if let Ok(bytes) = fs::read(&path) {
            if let Ok(mut value) = serde_json::from_slice(&bytes) {
                if rewrite_json(&mut value, relative, "__reference_probe__") > 0 {
                    result.push(AssetUsage {
                        description: path
                            .strip_prefix(project)
                            .unwrap_or(&path)
                            .display()
                            .to_string(),
                    });
                }
            }
        }
    }
    Ok(result)
}

pub(super) fn rename(
    project: &Path,
    old: &str,
    new: &str,
    world: &mut World,
    history: &mut History,
) -> Result<(String, usize), String> {
    let source = checked(project, old)?;
    let destination = checked(project, new)?;
    if old == new || !source.exists() {
        return Err("Choose an existing asset and a different destination.".into());
    }
    if destination.exists() || new.starts_with(&format!("{old}/")) {
        return Err("Destination already exists or is inside the source folder.".into());
    }
    let category_parts = if old.starts_with(".assets/") { 2 } else { 1 };
    if old.split('/').take(category_parts).collect::<Vec<_>>()
        != new.split('/').take(category_parts).collect::<Vec<_>>()
    {
        return Err("Move assets within their existing category.".into());
    }
    let mut files = Vec::new();
    metadata_files(project, project, 0, &mut files)?;
    let mut edits = Vec::new();
    let mut count = 0;
    for path in files {
        let before = fs::read(&path).map_err(|e| e.to_string())?;
        if let Ok(mut value) = serde_json::from_slice(&before) {
            let changed = rewrite_json(&mut value, old, new);
            if changed > 0 {
                count += changed;
                edits.push((
                    path,
                    before,
                    serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
                ));
            }
        }
    }
    fs::create_dir_all(destination.parent().ok_or("Missing destination parent")?)
        .map_err(|e| e.to_string())?;
    checked(project, new)?;
    fs::rename(&source, &destination).map_err(|e| e.to_string())?;
    let mut written: Vec<(PathBuf, Vec<u8>)> = Vec::new();
    for (path, before, after) in edits {
        let relocated = path
            .strip_prefix(&source)
            .map(|rest| destination.join(rest))
            .unwrap_or(path);
        if let Err(error) = crate::project::atomic_file::write_atomically(&relocated, &after) {
            let mut rollback_errors = Vec::new();
            for (file, bytes) in written.into_iter().rev() {
                if let Err(e) = crate::project::atomic_file::write_atomically(&file, &bytes) {
                    rollback_errors.push(e.to_string());
                }
            }
            if let Err(e) = fs::rename(&destination, &source) {
                rollback_errors.push(e.to_string());
            }
            return Err(format!(
                "Reference repair failed: {error}; rollback errors: {rollback_errors:?}"
            ));
        }
        written.push((relocated, before));
    }
    if old.starts_with(".assets/entities/") || old.starts_with(".assets/characters/") {
        let old_name = old.splitn(3, '/').nth(2).unwrap();
        let new_name = new.splitn(3, '/').nth(2).unwrap();
        for actor in world.actors.values_mut() {
            if let Some(name) = &mut actor.package {
                if let Some(value) = replacement(name, old_name, new_name) {
                    *name = value;
                    count += 1;
                }
            }
        }
        for snapshot in history
            .undo_stack
            .iter_mut()
            .chain(history.redo_stack.iter_mut())
        {
            for actor in snapshot.3.values_mut() {
                if let Some(name) = &mut actor.package {
                    if let Some(value) = replacement(name, old_name, new_name) {
                        *name = value;
                    }
                }
            }
        }
    }
    let logical_old = old.splitn(3, '/').nth(2).unwrap_or(old);
    let logical_new = new.splitn(3, '/').nth(2).unwrap_or(new);
    if old.starts_with(".assets/effects/") {
        let replace_fx = |cell: &mut crate::world::Cell| -> usize {
            if let Some(asset) = cell.particles.effect_asset.as_mut() {
                if let Some(rewritten) = replacement(asset, logical_old, logical_new) {
                    *asset = rewritten;
                    return 1;
                }
            }
            0
        };
        for cell in world.cells.values_mut() {
            count += replace_fx(cell);
        }
        for source in world.sources.values_mut() {
            count += replace_fx(&mut source.cell);
        }
        for snapshot in history
            .undo_stack
            .iter_mut()
            .chain(history.redo_stack.iter_mut())
        {
            for cell in snapshot.0.values_mut() {
                replace_fx(cell);
            }
            for source in snapshot.1.values_mut() {
                replace_fx(&mut source.cell);
            }
        }
    }
    let selected = if old.starts_with(".assets/characters/") {
        Some(&mut world.selected_character)
    } else if old.starts_with(".assets/controllers/") {
        Some(&mut world.selected_controller)
    } else if old.starts_with(".assets/cameras/") {
        Some(&mut world.selected_camera)
    } else {
        None
    };
    if let Some(value) = selected {
        if let Some(rewritten) = replacement(value, logical_old, logical_new) {
            *value = rewritten;
            count += 1;
        }
    }
    for binding in &mut world.script_bindings {
        if let Some(value) = replacement(&binding.script_path, old, new) {
            binding.script_path = value;
            count += 1;
        }
    }
    for binding in &mut world.actor_script_bindings {
        if let Some(value) = replacement(&binding.script_path, old, new) {
            binding.script_path = value;
            count += 1;
        }
    }
    for value in &mut world.disabled_scripts {
        if let Some(rewritten) = replacement(value, old, new) {
            *value = rewritten;
            count += 1;
        }
    }
    for snapshot in history
        .undo_stack
        .iter_mut()
        .chain(history.redo_stack.iter_mut())
    {
        for binding in &mut snapshot.2 {
            if let Some(value) = replacement(&binding.script_path, old, new) {
                binding.script_path = value;
            }
        }
        for binding in &mut snapshot.4 {
            if let Some(value) = replacement(&binding.script_path, old, new) {
                binding.script_path = value;
            }
        }
    }
    Ok((new.into(), count))
}

pub(super) fn duplicate(project: &Path, relative: &str) -> Result<PathBuf, String> {
    let source = checked(project, relative)?;
    if !source.exists() {
        return Err("Project asset is missing".into());
    }
    let parent = source.parent().ok_or("Missing asset parent")?;
    let stem = if source.is_dir() {
        source.file_name()
    } else {
        source.file_stem()
    }
    .and_then(|v| v.to_str())
    .ok_or("Invalid asset name")?;
    let extension = if source.is_file() {
        source.extension().and_then(|v| v.to_str()).unwrap_or("")
    } else {
        ""
    };
    let native =
        relative.starts_with(".assets/cameras/") || relative.starts_with(".assets/controllers/");
    for number in 1..=10000 {
        let copy_name = if number == 1 {
            format!("{stem}_Copy")
        } else {
            format!("{stem}_Copy_{number}")
        };
        if native && crate::project::native_logic::name_in_use(project, &copy_name) {
            continue;
        }
        let name = if extension.is_empty() {
            copy_name.clone()
        } else {
            format!("{copy_name}.{extension}")
        };
        let destination = parent.join(name);
        if destination.exists() {
            continue;
        }
        let stage = parent.join(format!(".asset-copy-{:032x}", rand::random::<u128>()));
        let result = (|| {
            fn copy(project: &Path, source: &Path, target: &Path) -> Result<(), String> {
                let relative = source
                    .strip_prefix(project)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                checked(project, &relative)?;
                if source.is_dir() {
                    fs::create_dir(target).map_err(|e| e.to_string())?;
                    for entry in fs::read_dir(source).map_err(|e| e.to_string())? {
                        let entry = entry.map_err(|e| e.to_string())?;
                        copy(project, &entry.path(), &target.join(entry.file_name()))?;
                    }
                } else {
                    let mut output = fs::OpenOptions::new()
                        .create_new(true)
                        .write(true)
                        .open(target)
                        .map_err(|e| e.to_string())?;
                    std::io::copy(
                        &mut fs::File::open(source).map_err(|e| e.to_string())?,
                        &mut output,
                    )
                    .and_then(|_| output.sync_all())
                    .map_err(|e| e.to_string())?;
                }
                Ok(())
            }
            copy(project, &source, &stage)?;
            let new_relative = destination
                .strip_prefix(project)
                .map_err(|e| e.to_string())?
                .to_string_lossy()
                .replace('\\', "/");
            let mut files = Vec::new();
            if stage.is_dir() {
                metadata_files(project, &stage, 0, &mut files)?;
            } else if extension.eq_ignore_ascii_case("json") {
                files.push(stage.clone());
            }
            let mut documents = Vec::new();
            let mut ids = std::collections::BTreeMap::new();
            for path in files {
                let bytes = fs::read(&path).map_err(|e| e.to_string())?;
                let mut value: serde_json::Value = serde_json::from_slice(&bytes)
                    .map_err(|e| format!("Cannot duplicate corrupt asset metadata: {e}"))?;
                rewrite_json(&mut value, relative, &new_relative);
                if relative.starts_with(".assets/materials/") {
                    let doc: crate::project::material::MaterialDocument =
                        serde_json::from_slice(&bytes).map_err(|e| e.to_string())?;
                    let id = format!("mat-{:032x}", rand::random::<u128>());
                    ids.insert(doc.id, id.clone());
                    value["id"] = id.into();
                }
                if path.file_name().is_some_and(|n| n == "package.json")
                    && value.get("name").is_some()
                {
                    value["name"] = copy_name.clone().into();
                }
                documents.push((path, value));
            }
            for (path, mut value) in documents {
                for (old, new) in &ids {
                    rewrite_json(&mut value, old, new);
                }
                fs::write(
                    path,
                    serde_json::to_vec_pretty(&value).map_err(|e| e.to_string())?,
                )
                .map_err(|e| e.to_string())?;
            }
            if native && stage.is_dir() {
                for entry in fs::read_dir(&stage).map_err(|e| e.to_string())? {
                    let path = entry.map_err(|e| e.to_string())?.path();
                    if path.extension().is_some_and(|ext| ext == "aeo") {
                        let source = fs::read_to_string(&path).map_err(|e| e.to_string())?;
                        let old = format!("entity {stem}");
                        if !source.contains(&old) {
                            return Err("Native package entity does not match its folder; original retained".into());
                        }
                        let rewritten = source.replacen(&old, &format!("entity {copy_name}"), 1);
                        fs::write(&path, rewritten).map_err(|e| e.to_string())?;
                    }
                }
            }
            checked(project, &new_relative)?;
            if destination.exists() {
                return Err("Duplicate destination changed concurrently; original retained".into());
            }
            fs::rename(&stage, &destination).map_err(|e| e.to_string())?;
            Ok(destination.clone())
        })();
        if result.is_err() {
            if stage.is_dir() {
                let _ = fs::remove_dir_all(&stage);
            } else {
                let _ = fs::remove_file(&stage);
            }
        }
        return result;
    }
    Err("No conflict-free asset copy name".into())
}

pub(super) fn delete(project: &Path, relative: &str) -> Result<(), String> {
    let path = checked(project, relative)?;
    let registry = crate::project::asset_registry::AssetRegistry::load(project)?;
    if let Some(record) = registry
        .assets
        .iter()
        .find(|record| record.path == relative)
    {
        let name = record.name.clone();
        if relative.starts_with(".assets/entities/") {
            return super::with_actor_groups_removed(project, &[name], || {
                crate::project::asset_registry::AssetRegistry::delete(project, &record.id)
            });
        }
        crate::project::asset_registry::AssetRegistry::delete(project, &record.id)?;
        return Ok(());
    }
    let prefix = format!("{relative}/");
    let in_folder = registry
        .assets
        .iter()
        .filter(|record| record.path.starts_with(&prefix))
        .collect::<Vec<_>>();
    if !in_folder.is_empty() {
        fn check_owned(
            project: &Path,
            dir: &Path,
            registered: &[&crate::project::asset_registry::AssetRecord],
        ) -> Result<(), String> {
            for entry in fs::read_dir(dir).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let path = entry.path();
                let relative = path
                    .strip_prefix(project)
                    .map_err(|e| e.to_string())?
                    .to_string_lossy()
                    .replace('\\', "/");
                let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
                if metadata.file_type().is_symlink() {
                    return Err(format!("Asset folder contains a linked entry: {relative}"));
                }
                if metadata.is_dir() {
                    check_owned(project, &path, registered)?;
                } else if !registered
                    .iter()
                    .any(|r| relative == r.path || relative.starts_with(&format!("{}/", r.path)))
                {
                    return Err(format!(
                        "Folder contains an unregistered creator file; preserved: {relative}"
                    ));
                }
            }
            Ok(())
        }
        if path.is_dir() {
            check_owned(project, &path, &in_folder)?;
        }
        let top_level_ids = in_folder
            .iter()
            .filter(|record| {
                !in_folder.iter().any(|parent| {
                    parent.id != record.id && record.path.starts_with(&format!("{}/", parent.path))
                })
            })
            .map(|record| record.id.clone())
            .collect::<Vec<_>>();
        let names = in_folder
            .iter()
            .filter(|record| record.kind == crate::project::asset_registry::AssetType::Actor)
            .map(|record| record.name.clone())
            .collect::<Vec<_>>();
        super::with_actor_groups_removed(project, &names, || {
            crate::project::asset_registry::AssetRegistry::delete_many(project, &top_level_ids)
        })?;
    }
    if path.is_dir() {
        let mut files = Vec::new();
        metadata_files(project, &path, 0, &mut files)?;
        fn verify(project: &Path, path: &Path) -> Result<(), String> {
            for entry in fs::read_dir(path).map_err(|e| e.to_string())? {
                let path = entry.map_err(|e| e.to_string())?.path();
                checked(
                    project,
                    &path
                        .strip_prefix(project)
                        .map_err(|e| e.to_string())?
                        .to_string_lossy()
                        .replace('\\', "/"),
                )?;
                if path.is_dir() {
                    verify(project, &path)?;
                }
            }
            Ok(())
        }
        verify(project, &path)?;
        fs::remove_dir_all(&path).map_err(|e| {
            format!(
                "Cannot delete {}: {e}. Close previews or processes using these files, then retry.",
                path.display()
            )
        })
    } else {
        fs::remove_file(&path).map_err(|e| {
            format!(
                "Cannot delete {}: {e}. Close previews or processes using this file, then retry.",
                path.display()
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            let root =
                std::env::temp_dir().join(format!("wk-lifecycle-{:032x}", rand::random::<u128>()));
            fs::create_dir_all(root.join(".assets/entities/Actor")).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }
}
