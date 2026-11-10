// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use std::{
    fs,
    io::{BufReader, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
};

fn validate_mp4(source: &Path, file_size: u64) -> Result<(), String> {
    let mut f =
        BufReader::new(fs::File::open(source).map_err(|e| format!("Cannot read video: {e}"))?);
    let mut offset = 0_u64;
    let mut first = true;
    let mut has_movie = false;
    let mut has_data = false;
    while offset < file_size {
        if file_size - offset < 8 {
            return Err("MP4 contains an incomplete atom header".into());
        }
        let mut header = [0_u8; 8];
        f.read_exact(&mut header)
            .map_err(|e| format!("Cannot read MP4 atom: {e}"))?;
        let basic_size = u32::from_be_bytes(header[..4].try_into().unwrap());
        let kind = &header[4..8];
        if first && kind != b"ftyp" {
            return Err("MP4 is missing its container header".into());
        }
        first = false;
        let (size, header_size) = if basic_size == 1 {
            let mut extended = [0_u8; 8];
            f.read_exact(&mut extended)
                .map_err(|e| format!("Truncated MP4 extended size: {e}"))?;
            (u64::from_be_bytes(extended), 16_u64)
        } else if basic_size == 0 {
            (file_size - offset, 8)
        } else {
            (u64::from(basic_size), 8)
        };
        if size < header_size || size > file_size - offset {
            return Err("MP4 contains an invalid or truncated atom".into());
        }
        if kind == b"moov" {
            has_movie = true;
        }
        if kind == b"mdat" && size > header_size {
            has_data = true;
        }
        offset += size;
        f.seek(SeekFrom::Start(offset))
            .map_err(|e| format!("Cannot seek within MP4: {e}"))?;
    }
    if !has_movie || !has_data {
        return Err("MP4 is missing movie metadata or video data".into());
    }
    Ok(())
}

fn import_effect(
    project: &Path,
    source: &Path,
    incoming_name: Option<&str>,
    replace: bool,
) -> Result<PathBuf, String> {
    use std::time::{SystemTime, UNIX_EPOCH};
    if !source.is_file() || source.is_symlink() {
        return Err("FX import source must be a regular file".into());
    }
    let meta = source.metadata().map_err(|e| e.to_string())?;
    if meta.len() == 0 || meta.len() > 512 * 1024 * 1024 {
        return Err("FX import size must be 1 byte to 512 MiB".into());
    }
    let source_name = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or("Invalid FX filename")?;
    let name = incoming_name.unwrap_or(source_name).trim();
    let ext = source.extension().and_then(|e| e.to_str()).unwrap_or("");
    if name.is_empty()
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        || !Path::new(name)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case(ext))
    {
        return Err("FX filename must be safe and retain its file extension".into());
    }
    let package_name = Path::new(name)
        .file_stem()
        .and_then(|n| n.to_str())
        .ok_or("Invalid FX package name")?;
    if package_name.is_empty() || matches!(package_name, "." | "..") {
        return Err("Invalid FX package name".into());
    }
    let category_root = project.join(".assets").join("effects");
    let directory = category_root.join(package_name);
    super::asset_lifecycle::checked(project, &format!(".assets/effects/{package_name}"))?;
    fs::create_dir_all(&category_root).map_err(|e| e.to_string())?;
    if !category_root
        .canonicalize()
        .map_err(|e| e.to_string())?
        .starts_with(project.canonicalize().map_err(|e| e.to_string())?)
    {
        return Err("Effects root escapes the project".into());
    }
    if directory.exists() && !replace {
        return Err(format!("Asset already exists: {}", directory.display()));
    }
    if directory.is_symlink() {
        return Err("Cannot replace a symlinked FX package".into());
    }
    let stamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let stage = category_root.join(format!(".fx-import-{}-{stamp}", std::process::id()));
    fs::create_dir(&stage).map_err(|e| e.to_string())?;
    let created = (|| -> Result<PathBuf, String> {
        let mut total_bytes = 0u64;
        let mut effects = 0usize;
        let mut count = 0usize;
        if ext.eq_ignore_ascii_case("efkpkg") {
            let file = fs::File::open(source).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipArchive::new(file)
                .map_err(|e| format!("Invalid Effekseer FX package: {e}"))?;
            if zip.len() > 2048 {
                return Err("FX archive contains too many files".into());
            }
            for i in 0..zip.len() {
                let mut entry = zip.by_index(i).map_err(|e| e.to_string())?;
                if entry.is_dir() {
                    continue;
                }
                if entry
                    .unix_mode()
                    .is_some_and(|m| (m & 0o170000) == 0o120000)
                {
                    return Err("FX archive must not contain symlinks".into());
                }
                let rel = entry
                    .enclosed_name()
                    .ok_or("Unsafe archive path")?
                    .to_path_buf();
                if rel.components().count() > 12 {
                    return Err("FX archive nesting exceeds limit".into());
                }
                let suffix = rel
                    .extension()
                    .and_then(|e| e.to_str())
                    .unwrap_or("")
                    .to_ascii_lowercase();
                if !matches!(
                    suffix.as_str(),
                    "efkefc"
                        | "efkmat"
                        | "efkmodel"
                        | "efkcurve"
                        | "png"
                        | "jpg"
                        | "jpeg"
                        | "tga"
                        | "bmp"
                        | "wav"
                        | "ogg"
                        | "flac"
                ) {
                    continue;
                }
                total_bytes = total_bytes.saturating_add(entry.size());
                if total_bytes > 512 * 1024 * 1024 {
                    return Err("FX archive exceeds 512 MiB extracted".into());
                }
                let dest = stage.join(&rel);
                if let Some(parent) = dest.parent() {
                    fs::create_dir_all(parent).map_err(|e| e.to_string())?;
                }
                let mut out = fs::OpenOptions::new()
                    .write(true)
                    .create_new(true)
                    .open(&dest)
                    .map_err(|e| e.to_string())?;
                std::io::copy(&mut entry, &mut out).map_err(|e| e.to_string())?;
                if suffix == "efkefc" {
                    effects += 1;
                }
                count += 1;
            }
            if effects == 0 {
                return Err("FX package contains no .efkefc effects".into());
            }
        } else {
            let destination = stage.join(name);
            fs::copy(source, &destination).map_err(|e| e.to_string())?;
            total_bytes += meta.len();
            count += 1;
            let parent = source.parent().ok_or("Missing FX source folder")?;
            fn copy_resources(
                from: &Path,
                to: &Path,
                bytes: &mut u64,
                count: &mut usize,
                depth: usize,
            ) -> Result<(), String> {
                if depth > 8 {
                    return Err("FX resource directory too deep".into());
                }
                for entry in fs::read_dir(from).map_err(|e| e.to_string())? {
                    let entry = entry.map_err(|e| e.to_string())?;
                    let kind = entry.file_type().map_err(|e| e.to_string())?;
                    if kind.is_symlink() {
                        return Err("FX resources contain symlinks".into());
                    }
                    let dest = to.join(entry.file_name());
                    if kind.is_dir() {
                        fs::create_dir(&dest).map_err(|e| e.to_string())?;
                        copy_resources(&entry.path(), &dest, bytes, count, depth + 1)?;
                    } else if kind.is_file() {
                        let size = entry.metadata().map_err(|e| e.to_string())?.len();
                        *bytes = bytes.saturating_add(size);
                        *count += 1;
                        if *bytes > 512 * 1024 * 1024 || *count > 2048 {
                            return Err("FX resources exceed import limits".into());
                        }
                        fs::copy(entry.path(), &dest).map_err(|e| e.to_string())?;
                    }
                }
                Ok(())
            }
            for folder in ["Texture", "Model", "Sound", "Material", "Curve"] {
                let path = parent.join(folder);
                if path.is_dir() {
                    if path.is_symlink() {
                        return Err("FX resource directory is a symlink".into());
                    }
                    let dest = stage.join(folder);
                    fs::create_dir(&dest).map_err(|e| e.to_string())?;
                    copy_resources(&path, &dest, &mut total_bytes, &mut count, 0)?;
                }
            }
        }
        Ok(stage.join(name))
    })();
    let staged_effect = match created {
        Ok(effect) => effect,
        Err(error) => {
            let _ = fs::remove_dir_all(&stage);
            return Err(error);
        }
    };
    let old = category_root.join(format!(".fx-backup-{}-{stamp}", std::process::id()));
    let had_old = directory.exists();
    if had_old {
        fs::rename(&directory, &old).map_err(|e| {
            let _ = fs::remove_dir_all(&stage);
            e.to_string()
        })?;
    }
    if let Err(error) = fs::rename(&stage, &directory) {
        let _ = fs::remove_dir_all(&stage);
        if had_old {
            let _ = fs::rename(&old, &directory);
        }
        return Err(format!("FX import commit failed: {error}"));
    }
    if had_old {
        let _ = fs::remove_dir_all(&old);
    }
    if ext.eq_ignore_ascii_case("efkpkg") {
        Ok(directory)
    } else {
        Ok(directory.join(staged_effect.file_name().unwrap()))
    }
}

pub(super) fn import_media(
    project: &Path,
    source: &Path,
    incoming_name: Option<&str>,
    replace: bool,
) -> Result<PathBuf, String> {
    import_media_scoped(project, source, incoming_name, replace, None)
}

pub(super) fn import_media_scoped(
    project: &Path,
    source: &Path,
    incoming_name: Option<&str>,
    replace: bool,
    subdir: Option<&Path>,
) -> Result<PathBuf, String> {
    let extension = source
        .extension()
        .and_then(|s| s.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "efkefc" | "efkpkg") {
        return import_effect(project, source, incoming_name, replace);
    }
    let category = match extension.as_str() {
        "png" | "jpg" | "jpeg" | "mp4" => "media",
        "wav" | "mp3" => "audio",
        _ => return Err(format!("Unsupported external asset: {}", source.display())),
    };
    if !source.is_file() || source.is_symlink() {
        return Err(format!(
            "Import source must be a regular file: {}",
            source.display()
        ));
    }
    let original_name = source
        .file_name()
        .and_then(|s| s.to_str())
        .ok_or("Import source has no readable filename")?;
    let name = incoming_name.unwrap_or(original_name).trim();
    if name.is_empty()
        || matches!(name, "." | "..")
        || name
            .chars()
            .any(|c| c.is_control() || "\\/:*?\"<>|".contains(c))
        || Path::new(name)
            .extension()
            .and_then(|s| s.to_str())
            .is_none_or(|ext| !ext.eq_ignore_ascii_case(&extension))
    {
        return Err("Incoming filename must be a safe name with the original extension".into());
    }

    let metadata = fs::metadata(source).map_err(|e| format!("Cannot inspect import: {e}"))?;
    if metadata.len() == 0 || metadata.len() > 512 * 1024 * 1024 {
        return Err("Import file is empty or larger than 512 MiB".into());
    }
    match extension.as_str() {
        "png" | "jpg" | "jpeg" => {
            image::ImageReader::open(source)
                .map_err(|e| format!("Cannot read image: {e}"))?
                .decode()
                .map_err(|e| format!("Image did not decode: {e}"))?;
        }
        "wav" | "mp3" => {
            let file = fs::File::open(source).map_err(|e| format!("Cannot read audio: {e}"))?;
            rodio::Decoder::new(BufReader::new(file))
                .map_err(|e| format!("Audio format did not decode: {e}"))?;
        }
        "mp4" => validate_mp4(source, metadata.len())?,
        _ => unreachable!(),
    }
    let scoped = subdir.unwrap_or(Path::new(""));
    if scoped
        .components()
        .any(|part| !matches!(part, std::path::Component::Normal(_)))
    {
        return Err("Unsafe incoming media folder".into());
    }
    let relative = Path::new(".assets").join(category).join(scoped).join(name);
    super::asset_lifecycle::checked(project, &relative.to_string_lossy().replace('\\', "/"))?;
    let dir = project.join(".assets").join(category).join(scoped);
    fs::create_dir_all(&dir).map_err(|e| format!("Cannot create project asset directory: {e}"))?;
    let actual_root =
        fs::canonicalize(project).map_err(|e| format!("Invalid project root: {e}"))?;
    let actual_dir =
        fs::canonicalize(&dir).map_err(|e| format!("Invalid project asset folder: {e}"))?;
    if !actual_dir.starts_with(&actual_root) {
        return Err("Project asset directory points outside the active project".into());
    }
    let destination = dir.join(name);
    if destination.is_file() && fs::canonicalize(source).ok() == fs::canonicalize(&destination).ok()
    {
        return Ok(destination);
    }
    if destination.exists() && !replace {
        return Err(format!("Asset already exists: {}", destination.display()));
    }
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_err(|e| e.to_string())?
        .as_nanos();
    let staged = dir.join(format!(".wk-import-{}-{tick}.tmp", std::process::id()));
    fs::copy(source, &staged).map_err(|e| format!("Cannot stage import: {e}"))?;
    let backup = dir.join(format!(
        ".wk-import-backup-{}-{tick}.tmp",
        std::process::id()
    ));
    let had_old = destination.exists();
    if had_old && replace {
        if let Err(error) = fs::rename(&destination, &backup) {
            let _ = fs::remove_file(&staged);
            return Err(format!(
                "Cannot stage existing asset for replacement: {error}"
            ));
        }
    }
    if let Err(error) = fs::rename(&staged, &destination) {
        let _ = fs::remove_file(&staged);
        if had_old && replace {
            let _ = fs::rename(&backup, &destination);
        }
        return Err(format!("Cannot publish imported asset: {error}"));
    }
    let publication =
        crate::project::asset_registry::AssetRegistry::reconcile(project).and_then(|_| {
            if had_old && replace {
                crate::project::asset_registry::AssetRegistry::notify_content_changed(project)
                    .map(|_| ())
            } else {
                Ok(())
            }
        });
    if let Err(error) = publication {
        let _ = fs::remove_file(&destination);
        if had_old && replace {
            fs::rename(&backup, &destination).map_err(|restore| {
                format!("Registry update failed: {error}; original file restore failed: {restore}")
            })?;
        }
        let _ = crate::project::asset_registry::AssetRegistry::reconcile(project);
        return Err(format!(
            "Import rolled back after registry failure: {error}"
        ));
    }
    if had_old && replace {
        let _ = fs::remove_file(backup);
    }
    Ok(destination)
}

#[cfg(test)]
mod scoped_media_import_regressions {
    use super::*;

    #[test]
    fn nested_same_named_textures_do_not_replace_legacy_flat_asset() {
        let root =
            std::env::temp_dir().join(format!("wk-import-media-{:032x}", rand::random::<u128>()));
        let project = root.join("project");
        let pack = root.join("pack");
        fs::create_dir_all(project.join(".assets/media")).unwrap();
        fs::write(
            project.join(".assets/media/colormap.png"),
            b"legacy contents",
        )
        .unwrap();
        for folder in ["Models/GLB Format/Textures", "Models/FBX Format/Textures"] {
            let dir = pack.join(folder);
            fs::create_dir_all(&dir).unwrap();
            let mut image = image::RgbImage::new(1, 1);
            image.put_pixel(
                0,
                0,
                image::Rgb(if folder.contains("GLB") {
                    [255, 0, 0]
                } else {
                    [0, 255, 0]
                }),
            );
            image.save(dir.join("colormap.png")).unwrap();
        }
        crate::project::asset_registry::AssetRegistry::reconcile(&project).unwrap();
        let plan = crate::project::asset_import_plan::plan_import(&project, &[pack]).unwrap();
        assert!(plan.can_import());
        assert_eq!(plan.assets.len(), 2);
        crate::project::shared_inventory::validate_project_import(&project, &plan).unwrap();
        let result = execute_import_plan(&project, &plan).unwrap();
        assert!(result.failures.is_empty());
        assert_eq!(result.imported_ids.len(), 2);
        let a = project.join(".assets/media/pack/Models/GLB Format/Textures/colormap.png");
        let b = project.join(".assets/media/pack/Models/FBX Format/Textures/colormap.png");
        assert!(a.is_file() && b.is_file());
        assert_ne!(fs::read(a).unwrap(), fs::read(b).unwrap());
        assert_eq!(
            fs::read(project.join(".assets/media/colormap.png")).unwrap(),
            b"legacy contents"
        );
        fs::remove_dir_all(root).unwrap();
    }
}

pub(super) fn execute_import_plan(
    project: &Path,
    plan: &crate::project::asset_import_plan::ImportPlan,
) -> Result<crate::project::asset_import_plan::ImportExecutionResult, String> {
    execute_import_plan_with_progress(project, plan, None)
}

pub(super) fn execute_import_plan_with_progress(
    project: &Path,
    plan: &crate::project::asset_import_plan::ImportPlan,
    progress: Option<&std::sync::atomic::AtomicUsize>,
) -> Result<crate::project::asset_import_plan::ImportExecutionResult, String> {
    use crate::project::asset_import_plan::ImportExecutionResult;
    use crate::project::asset_registry::{AssetRegistry, AssetType};
    if !plan.can_import() {
        return Err("Resolve import conflicts before starting".into());
    }
    let before = AssetRegistry::load(project)?;
    let before_ids = before
        .assets
        .iter()
        .map(|asset| asset.id.clone())
        .collect::<std::collections::HashSet<_>>();
    let mut imported = Vec::new();
    let mut failures = Vec::new();
    if let Some(progress) = progress {
        progress.store(0, std::sync::atomic::Ordering::Relaxed);
    }
    for (index, entry) in plan.assets.iter().enumerate() {
        match entry.kind {
            AssetType::Actor => {
                let result = crate::project::inspect_glb(&entry.source).and_then(|summary| {
                    match summary.kind {
                        crate::project::GlbAssetKind::StaticMesh => {
                            crate::project::import_static_glb_named(
                                project,
                                &entry.source,
                                &entry.name,
                            )
                            .map(|value| {
                                value
                                    .packages
                                    .into_iter()
                                    .map(|p| p.package_name)
                                    .collect::<Vec<_>>()
                            })
                        }
                        crate::project::GlbAssetKind::RiggedCharacter => {
                            crate::project::import_character_glb_named(
                                project,
                                &entry.source,
                                Some(&entry.name),
                            )
                            .map(|value| vec![value.package_name])
                        }
                    }
                });
                match result {
                    Ok(names) => imported.extend(names),
                    Err(error) => failures.push(format!("{}: {error}", entry.source.display())),
                }
            }
            AssetType::Image | AssetType::Video | AssetType::Audio | AssetType::Effect => {
                match import_media_scoped(
                    project,
                    &entry.source,
                    entry.incoming_name.as_deref(),
                    true,
                    entry.import_subdir.as_deref(),
                ) {
                    Ok(path) => imported.push(path.display().to_string()),
                    Err(error) => failures.push(format!("{}: {error}", entry.source.display())),
                }
            }
            _ => failures.push(format!(
                "{}: unsupported V1 source type",
                entry.source.display()
            )),
        }
        if let Some(progress) = progress {
            progress.store(
                (index + 1) * 100 / plan.assets.len(),
                std::sync::atomic::Ordering::Relaxed,
            );
        }
    }
    if !failures.is_empty() {
        let created = imported
            .iter()
            .map(|name| {
                let path = std::path::Path::new(name);
                if path.is_absolute() {
                    path.strip_prefix(project)
                        .map(|relative| relative.to_string_lossy().replace('\\', "/"))
                        .unwrap_or_default()
                } else {
                    format!(".assets/entities/{name}")
                }
            })
            .filter(|path| !path.is_empty())
            .collect::<Vec<_>>();
        AssetRegistry::rollback_new_imports(project, &before, &created).map_err(|rollback| {
            format!(
                "Import failed: {}; rollback incomplete: {rollback}",
                failures.join("; ")
            )
        })?;
        return Err(format!(
            "Import failed; all newly imported assets rolled back: {}",
            failures.join("; ")
        ));
    }
    let after = AssetRegistry::reconcile(project)?;
    let created = after
        .assets
        .iter()
        .filter(|asset| !before_ids.contains(&asset.id))
        .collect::<Vec<_>>();
    let imported_ids = created
        .iter()
        .map(|asset| asset.id.clone())
        .chain(
            before
                .assets
                .iter()
                .filter(|asset| {
                    imported.iter().any(|name| {
                        name == &asset.path
                            || name.ends_with(&asset.name)
                            || name.ends_with(&format!("/{}", asset.name))
                            || name == &project.join(&asset.path).display().to_string()
                    })
                })
                .map(|asset| asset.id.clone()),
        )
        .collect::<std::collections::BTreeSet<_>>()
        .into_iter()
        .collect();
    let mut warnings = plan.warnings.clone();
    for asset in &created {
        for missing in &asset.missing_dependencies {
            warnings.push(format!("{}: unresolved dependency: {missing}", asset.name));
        }
    }
    Ok(ImportExecutionResult {
        imported_ids,
        failures,
        warnings,
        revision: after.revision,
    })
}

pub(super) fn import_folder_media(
    project: &Path,
    source_root: &Path,
) -> Result<(usize, Vec<String>), String> {
    if !source_root.is_dir() || source_root.is_symlink() {
        return Err(format!(
            "Not a regular import folder: {}",
            source_root.display()
        ));
    }
    fn collect(folder: &Path, paths: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(folder).map_err(|e| format!("Cannot read import folder: {e}"))? {
            let entry = entry.map_err(|e| format!("Cannot read folder entry: {e}"))?;
            let path = entry.path();
            let ty = entry
                .file_type()
                .map_err(|e| format!("Cannot inspect entry: {e}"))?;
            if ty.is_symlink() {
                continue;
            }
            if ty.is_dir() {
                collect(&path, paths)?;
            } else if ty.is_file()
                && path
                    .extension()
                    .and_then(|s| s.to_str())
                    .is_some_and(|ext| {
                        matches!(
                            ext.to_ascii_lowercase().as_str(),
                            "png" | "jpg" | "jpeg" | "mp4" | "wav" | "mp3" | "efkefc" | "efkpkg"
                        )
                    })
            {
                paths.push(path);
                if paths.len() > 2000 {
                    return Err(
                        "A folder import cannot contain more than 2000 media files at once".into(),
                    );
                }
            }
        }
        Ok(())
    }
    let mut sources = Vec::new();
    collect(source_root, &mut sources)?;
    sources.sort();
    let mut imported = 0;
    let mut errors = Vec::new();
    for source in sources {
        let relative = source
            .parent()
            .and_then(|parent| parent.strip_prefix(source_root).ok());
        match import_media_scoped(project, &source, None, false, relative) {
            Ok(_) => imported += 1,
            Err(error) => errors.push(format!("{}: {error}", source.display())),
        }
    }
    Ok((imported, errors))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unsafe_windows_asset_names_never_create_project_directories() {
        let root =
            std::env::temp_dir().join(format!("wk_import_names_{:032x}", rand::random::<u128>()));
        let project = root.join("project");
        let external = root.join("external");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&external).unwrap();
        let source = external.join("valid.png");
        image::RgbImage::new(2, 2).save(&source).unwrap();
        for name in ["CON.png", "LPT1.png", "NUL.png"] {
            assert!(import_media(&project, &source, Some(name), false).is_err());
            assert!(!project.join(".assets").exists());
        }
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn linked_asset_directory_is_rejected_before_creating_external_folders() {
        let root =
            std::env::temp_dir().join(format!("wk_import_link_{:032x}", rand::random::<u128>()));
        let project = root.join("project");
        let outside = root.join("outside");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(&outside).unwrap();
        let source = outside.join("valid.png");
        image::RgbImage::new(2, 2).save(&source).unwrap();
        let linked = project.join(".assets");
        #[cfg(windows)]
        {
            let link = std::process::Command::new("cmd")
                .args(["/C", "mklink", "/J"])
                .arg(&linked)
                .arg(&outside)
                .output()
                .unwrap();
            assert!(
                link.status.success(),
                "Cannot create isolated test junction: {}",
                String::from_utf8_lossy(&link.stderr)
            );
        }
        #[cfg(unix)]
        std::os::unix::fs::symlink(&outside, &linked).unwrap();
        assert!(!outside.join("textures").exists());
        assert!(import_media(&project, &source, None, false).is_err());
        assert!(
            !outside.join("textures").exists(),
            "Import must not create files through a linked directory"
        );
        #[cfg(windows)]
        fs::remove_dir(&linked).unwrap();
        #[cfg(unix)]
        fs::remove_file(&linked).unwrap();
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn all_imported_image_formats_remain_images_until_explicit_texture_creation() {
        let root = std::env::temp_dir().join(format!(
            "wk_images_only_import_{:032x}",
            rand::random::<u128>()
        ));
        let incoming = root.join("incoming");
        std::fs::create_dir_all(&incoming).unwrap();
        for name in ["grass.png", "water.jpg", "poster.jpeg"] {
            let source = incoming.join(name);
            image::RgbImage::from_pixel(4, 4, image::Rgb([18, 63, 112]))
                .save(&source)
                .unwrap();
            let original = std::fs::read(&source).unwrap();
            let destination = import_media(&root, &source, None, false).unwrap();
            assert_eq!(destination, root.join(".assets/media").join(name));
            assert_eq!(std::fs::read(destination).unwrap(), original);
            assert!(!root.join(".assets/textures").join(name).exists());
        }
        assert!(
            !root.join(".assets/textures").exists(),
            "Ordinary image imports must never create Textures"
        );
        let assets = super::super::scan_browser_files(&root);
        for name in ["grass.png", "water.jpg", "poster.jpeg"] {
            assert!(
                assets
                    .iter()
                    .any(|a| a.category == "Images" && a.name == name)
            );
        }
        assert!(
            !assets
                .iter()
                .any(|a| a.category == "Textures" && !a.is_directory)
        );
        std::fs::remove_dir_all(root).unwrap();
    }
}

#[cfg(test)]
mod fx_import_tests {
    use super::*;

    #[test]
    fn project_fx_import_preserves_relative_resources() {
        let base =
            std::env::temp_dir().join(format!("wk-fx003-import-{:016x}", rand::random::<u64>()));
        let project = base.join("Project");
        let origin = base.join("Source");
        fs::create_dir_all(&project).unwrap();
        fs::create_dir_all(origin.join("Texture")).unwrap();
        fs::write(origin.join("Campfire.efkefc"), b"FX test payload").unwrap();
        fs::write(origin.join("Texture").join("fire.png"), b"test bytes").unwrap();
        let effect = import_effect(&project, &origin.join("Campfire.efkefc"), None, false).unwrap();
        assert_eq!(
            effect,
            project.join(".assets/effects/Campfire/Campfire.efkefc")
        );
        assert!(effect.is_file());
        assert!(effect.parent().unwrap().join("Texture/fire.png").is_file());
        let id = crate::project::fx_assets::id_from_path(&project, &effect).unwrap();
        assert_eq!(id, "Campfire/Campfire.efkefc");
        assert_eq!(
            crate::project::fx_assets::resolve(&project, &id),
            Some(effect.canonicalize().unwrap())
        );
        assert!(
            import_effect(&project, &origin.join("Campfire.efkefc"), None, false)
                .unwrap_err()
                .starts_with("Asset already exists:")
        );
        fs::remove_dir_all(base).unwrap();
    }

    #[test]
    fn effect_package_import_extracts_multiple_effects_and_rejects_unsafe_archive() {
        use std::io::Write;
        let base =
            std::env::temp_dir().join(format!("wk-fx003-pack-{:016x}", rand::random::<u64>()));
        fs::create_dir_all(base.join("Project")).unwrap();
        let source = base.join("Effects.efkpkg");
        let zip_file = fs::File::create(&source).unwrap();
        let mut writer = zip::ZipWriter::new(zip_file);
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("Fire.efkefc", options).unwrap();
        writer.write_all(b"FX sample").unwrap();
        writer.start_file("Texture/Fire.png", options).unwrap();
        writer.write_all(b"texture").unwrap();
        writer.finish().unwrap();
        let dest = import_effect(&base.join("Project"), &source, None, false).unwrap();
        assert!(dest.join("Fire.efkefc").is_file());
        assert!(dest.join("Texture/Fire.png").is_file());

        let hostile = base.join("Hostile.efkpkg");
        let zip_file = fs::File::create(&hostile).unwrap();
        let mut writer = zip::ZipWriter::new(zip_file);
        writer.start_file("../escaped.efkefc", options).unwrap();
        writer.write_all(b"malicious").unwrap();
        writer.finish().unwrap();
        let failure = import_effect(&base.join("Project"), &hostile, None, false).unwrap_err();
        assert!(
            failure.contains("Unsafe archive"),
            "unexpected rejection: {failure}"
        );
        assert!(!base.join("Project/.assets/effects/escaped.efkefc").exists());
        assert!(!base.join("Project/.assets/effects/Hostile").exists());
        fs::remove_dir_all(base).unwrap();
    }
}
