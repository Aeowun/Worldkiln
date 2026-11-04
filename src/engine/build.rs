// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::collections::BTreeSet;
use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::build_platform::{BuildStage, GamePlatformAdapter, TargetPlatform};
use crate::engine::game_package::{
    EmbeddedGameManifest, GameExportProfile, RuntimeReleaseAsset, append_game_package,
    create_engine_assets_archive, create_project_payload_with_runtime, persist_game_id,
    planned_game_id, probe_embedded_game, verify_embedded_payload,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameBuildConfiguration {
    Development,
    Shipping,
}
impl GameBuildConfiguration {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Development => "development",
            Self::Shipping => "shipping",
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum GameBuildFormat {
    PlayableFolder,
    ZipArchive,
}
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct GameBuildOptions {
    pub target: TargetPlatform,
    pub linux_runtime_binary: Option<PathBuf>,
    pub configuration: GameBuildConfiguration,
    pub format: GameBuildFormat,
    pub output_parent: Option<PathBuf>,
    pub title: String,
    pub game_version: String,
    pub executable_name: String,
    pub window_mode: String,
    pub resolution: [u32; 2],
    pub icon: Option<PathBuf>,
}
impl Default for GameBuildOptions {
    fn default() -> Self {
        Self {
            target: TargetPlatform::Windows,
            linux_runtime_binary: None,
            configuration: GameBuildConfiguration::Shipping,
            format: GameBuildFormat::PlayableFolder,
            output_parent: None,
            title: String::new(),
            game_version: "1.0.0".into(),
            executable_name: String::new(),
            window_mode: "borderless_fullscreen".into(),
            resolution: [1920, 1080],
            icon: None,
        }
    }
}
fn validate_options(options: &GameBuildOptions) -> Result<(), String> {
    if options.target != TargetPlatform::Linux {
        options.target.ensure_native_adapter()?;
    }
    if options.target == TargetPlatform::Linux {
        if options.linux_runtime_binary.is_none() {
            return Err("Linux export requires a native x86_64 ELF runtime binary".into());
        }
        if options.format != GameBuildFormat::ZipArchive {
            return Err(
                "Linux exports from Windows require ZIP to retain executable permissions".into(),
            );
        }
        if options.configuration != GameBuildConfiguration::Shipping {
            return Err(
                "Linux Development builds require a dedicated native diagnostics runtime".into(),
            );
        }
        if options.icon.is_some() {
            return Err(
                "Linux game icon resources are not supported by the native package yet".into(),
            );
        }
    }
    if !matches!(
        options.window_mode.as_str(),
        "windowed" | "borderless_fullscreen" | "fullscreen"
    ) || options.resolution.iter().any(|v| *v < 320 || *v > 16384)
    {
        return Err("Invalid game window settings".into());
    }
    if options.title.len() > 256
        || options.game_version.trim().is_empty()
        || options.game_version.len() > 64
    {
        return Err("Invalid game title or version".into());
    }
    if !options.executable_name.is_empty()
        && (options.executable_name.len() > 100
            || !options
                .executable_name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'))
    {
        return Err(
            "Executable name may contain only letters, numbers, dash and underscore".into(),
        );
    }
    if let Some(icon) = &options.icon {
        if !icon.is_file() {
            return Err("Selected game icon is unavailable".into());
        }
        if !matches!(
            icon.extension()
                .and_then(|s| s.to_str())
                .map(|s| s.to_ascii_lowercase())
                .as_deref(),
            Some("ico" | "png")
        ) {
            return Err("Game icon must be an ICO or PNG file".into());
        }
    }
    if let Some(parent) = &options.output_parent {
        if !parent.is_dir() {
            return Err("Selected output directory does not exist".into());
        }
    }
    Ok(())
}

#[cfg(windows)]
fn embed_game_icon(executable: &Path, icon_path: &Path) -> Result<(), String> {
    use std::ffi::c_void;
    use std::os::windows::ffi::OsStrExt;
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn BeginUpdateResourceW(filename: *const u16, delete_existing: i32) -> *mut c_void;
        fn UpdateResourceW(
            update: *mut c_void,
            resource_type: *const u16,
            name: *const u16,
            language: u16,
            data: *const c_void,
            len: u32,
        ) -> i32;
        fn EndUpdateResourceW(update: *mut c_void, discard: i32) -> i32;
    }
    let bytes = fs::read(icon_path).map_err(|e| format!("Cannot read game icon: {e}"))?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err("Selected icon is too large".into());
    }
    let (icon_bytes, mut group_entry) = if icon_path
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("ico"))
    {
        if bytes.len() < 22 || bytes[0..4] != [0, 0, 1, 0] {
            return Err("Invalid ICO header".into());
        }
        let count = u16::from_le_bytes([bytes[4], bytes[5]]) as usize;
        if count == 0 || 6 + count * 16 > bytes.len() {
            return Err("Invalid ICO image directory".into());
        }
        let mut best: Option<(usize, u32)> = None;
        for i in 0..count {
            let start = 6 + i * 16;
            let entry = &bytes[start..start + 16];
            let size = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
            let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
            if size == 0 || offset.checked_add(size).is_none_or(|end| end > bytes.len()) {
                continue;
            }
            let w = if entry[0] == 0 { 256 } else { entry[0] as u32 };
            let h = if entry[1] == 0 { 256 } else { entry[1] as u32 };
            let score = w * h;
            if best.is_none_or(|(_, prior)| score > prior) {
                best = Some((i, score));
            }
        }
        let index = best.ok_or("ICO file contains no readable images")?.0;
        let entry = &bytes[6 + index * 16..6 + index * 16 + 16];
        let size = u32::from_le_bytes(entry[8..12].try_into().unwrap()) as usize;
        let offset = u32::from_le_bytes(entry[12..16].try_into().unwrap()) as usize;
        (bytes[offset..offset + size].to_vec(), entry[..12].to_vec())
    } else {
        let image = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map_err(|e| format!("Invalid PNG game icon: {e}"))?;
        let image = image
            .resize(256, 256, image::imageops::FilterType::Lanczos3)
            .to_rgba8();
        let mut canvas = image::RgbaImage::new(256, 256);
        image::imageops::overlay(
            &mut canvas,
            &image,
            ((256 - image.width()) / 2) as i64,
            ((256 - image.height()) / 2) as i64,
        );
        let mut png = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba8(canvas)
            .write_to(&mut png, image::ImageFormat::Png)
            .map_err(|e| format!("Could not encode game icon: {e}"))?;
        let payload = png.into_inner();
        let mut group = vec![0_u8, 0, 0, 0, 1, 0, 32, 0];
        group.extend_from_slice(&(payload.len() as u32).to_le_bytes());
        (payload, group)
    };
    group_entry.extend_from_slice(&1_u16.to_le_bytes());
    let mut group = vec![0_u8, 0, 1, 0, 1, 0];
    group.extend_from_slice(&group_entry);
    let filename: Vec<u16> = executable
        .as_os_str()
        .encode_wide()
        .chain(Some(0))
        .collect();
    let handle = unsafe { BeginUpdateResourceW(filename.as_ptr(), 0) };
    if handle.is_null() {
        return Err(format!(
            "Could not open executable icon resources: {}",
            std::io::Error::last_os_error()
        ));
    }
    let result = unsafe {
        let image_ok = UpdateResourceW(
            handle,
            3_usize as *const u16,
            1_usize as *const u16,
            1033,
            icon_bytes.as_ptr().cast(),
            icon_bytes.len() as u32,
        ) != 0;
        let group_ok = image_ok
            && UpdateResourceW(
                handle,
                14_usize as *const u16,
                1_usize as *const u16,
                1033,
                group.as_ptr().cast(),
                group.len() as u32,
            ) != 0;
        if !group_ok {
            let error = std::io::Error::last_os_error();
            EndUpdateResourceW(handle, 1);
            return Err(format!("Could not set game executable icon: {error}"));
        }
        EndUpdateResourceW(handle, 0) != 0
    };
    if !result {
        return Err(format!(
            "Could not commit game executable icon: {}",
            std::io::Error::last_os_error()
        ));
    }
    Ok(())
}

pub fn load_packaging_options(project: &Path) -> GameBuildOptions {
    let path = project.join(".worldkiln").join("packaging-options.json");
    fs::read(path)
        .ok()
        .and_then(|data| serde_json::from_slice(&data).ok())
        .unwrap_or_else(|| {
            let mut settings = GameBuildOptions::default();
            let project_title = project
                .file_name()
                .and_then(|name| name.to_str())
                .unwrap_or("My Game");
            settings.title = project_title.to_string();
            let executable = project_title
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_' || *c == '-')
                .collect::<String>();
            settings.executable_name = if executable.is_empty() {
                "MyGame".into()
            } else {
                executable
            };
            settings.window_mode = "windowed".into();
            settings
        })
}

pub fn save_packaging_options(project: &Path, options: &GameBuildOptions) -> Result<(), String> {
    let directory = project.join(".worldkiln");
    fs::create_dir_all(&directory).map_err(|e| format!("Cannot save packaging settings: {e}"))?;
    let bytes = serde_json::to_vec_pretty(options).map_err(|e| e.to_string())?;
    crate::project::atomic_file::write_atomically(&directory.join("packaging-options.json"), &bytes)
        .map_err(|e| e.to_string())
}

pub struct BuildResult {
    pub output_dir: PathBuf,
    pub executable: PathBuf,
    pub output_log: String,
    pub archive_path: Option<PathBuf>,
}

fn project_name(project_path: &Path) -> String {
    project_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("MyGame")
        .to_string()
}

pub(super) fn snapshot_project_for_build(source: &Path, destination: &Path) -> Result<(), String> {
    use crate::engine::game_package::{export_content_included, sha256_file};
    fn visit(
        source: &Path,
        current: &Path,
        relative: &Path,
        destination: &Path,
        verified: &mut Vec<(PathBuf, String)>,
    ) -> Result<(), String> {
        for entry in fs::read_dir(current).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = relative.join(entry.file_name());
            let logical = name.to_string_lossy().replace('\\', "/");
            if !export_content_included(&logical) {
                continue;
            }
            let metadata = fs::symlink_metadata(&path).map_err(|e| e.to_string())?;
            if metadata.file_type().is_symlink() {
                return Err(format!("Cannot build linked project content: {logical}"));
            }
            #[cfg(windows)]
            {
                use std::os::windows::fs::MetadataExt;
                if metadata.file_attributes() & 0x400 != 0 {
                    return Err(format!(
                        "Cannot build redirected project content: {logical}"
                    ));
                }
            }
            if metadata.is_dir() {
                fs::create_dir_all(destination.join(&name)).map_err(|e| e.to_string())?;
                visit(source, &path, &name, destination, verified)?;
            } else if metadata.is_file() {
                let target = destination.join(&name);
                fs::create_dir_all(target.parent().ok_or("Snapshot file has no parent")?)
                    .map_err(|e| e.to_string())?;
                fs::copy(&path, &target).map_err(|e| e.to_string())?;
                let (src_hash, src_len) = sha256_file(&path)?;
                let (dst_hash, dst_len) = sha256_file(&target)?;
                if src_hash != dst_hash || src_len != dst_len {
                    return Err(format!(
                        "Project content changed during export snapshot: {logical}"
                    ));
                }
                verified.push((name, src_hash));
            } else {
                return Err(format!("Unsupported project entry: {logical}"));
            }
        }
        let _ = source;
        Ok(())
    }
    fs::create_dir_all(destination).map_err(|e| e.to_string())?;
    let mut originals = Vec::new();
    visit(source, source, Path::new(""), destination, &mut originals)?;
    let original_paths = originals
        .iter()
        .map(|(path, _)| path.clone())
        .collect::<BTreeSet<_>>();
    for (relative, expected_hash) in &originals {
        let (hash, _) = sha256_file(&source.join(relative))?;
        if &hash != expected_hash {
            return Err(format!(
                "Project changed while Build Game was collecting its content: {}",
                relative.display()
            ));
        }
    }
    let mut current_files = Vec::new();
    fn enumerate(current: &Path, relative: &Path, files: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in fs::read_dir(current).map_err(|e| e.to_string())? {
            let entry = entry.map_err(|e| e.to_string())?;
            let path = entry.path();
            let name = relative.join(entry.file_name());
            if !crate::engine::game_package::export_content_included(&name.to_string_lossy()) {
                continue;
            }
            let ty = entry.file_type().map_err(|e| e.to_string())?;
            if ty.is_dir() {
                enumerate(&path, &name, files)?;
            } else if ty.is_file() {
                files.push(name);
            }
        }
        Ok(())
    }
    enumerate(source, Path::new(""), &mut current_files)?;
    if current_files.into_iter().collect::<BTreeSet<_>>() != original_paths {
        return Err(
            "Project asset list changed during Build Game. Retry after edits complete.".into(),
        );
    }
    Ok(())
}

pub(super) fn unique_staging_dir(parent: &Path, label: &str) -> PathBuf {
    parent.join(format!(
        ".{label}.worldkiln-build-{}-{:032x}",
        std::process::id(),
        rand::random::<u128>()
    ))
}

fn publish_build_directory(staging: &Path, destination: &Path) -> Result<Option<String>, String> {
    let parent = destination
        .parent()
        .ok_or_else(|| "Build destination has no parent directory".to_string())?;
    fs::create_dir_all(parent)
        .map_err(|error| format!("Failed to create {}: {error}", parent.display()))?;

    let backup = unique_staging_dir(parent, "backup");
    let had_previous = destination.exists();

    if had_previous {
        fs::rename(destination, &backup).map_err(|error| {
            format!(
                "Failed to protect previous successful build {}: {error}",
                destination.display()
            )
        })?;
    }

    if let Err(error) = fs::rename(staging, destination) {
        let rollback = if had_previous {
            fs::rename(&backup, destination)
        } else {
            Ok(())
        };
        return Err(format!(
            "Failed to publish new build {}: {error}; rollback: {rollback:?}",
            destination.display()
        ));
    }

    if had_previous && backup.exists() {
        if let Err(error) = fs::remove_dir_all(&backup) {
            return Ok(Some(format!(
                "New build published; obsolete backup {} remains: {error}",
                backup.display()
            )));
        }
    }

    Ok(None)
}

fn package_game_with_template(
    project_path: &Path,
    executable_template: &Path,
    runtime_asset: &RuntimeReleaseAsset,
) -> Result<BuildResult, String> {
    let mut ignore_stage = |_| {};
    package_game_with_template_inner(
        project_path,
        executable_template,
        runtime_asset,
        false,
        None,
        &GameBuildOptions::default(),
        &mut ignore_stage,
    )
}

fn package_game_with_template_inner(
    project_path: &Path,
    executable_template: &Path,
    runtime_asset: &RuntimeReleaseAsset,
    bundle_runtime: bool,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    settings: &GameBuildOptions,
    on_stage: &mut dyn FnMut(BuildStage),
) -> Result<BuildResult, String> {
    if !project_path.is_dir() {
        return Err(format!(
            "Project directory does not exist: {}",
            project_path.display()
        ));
    }
    if !executable_template.is_file() {
        return Err(format!(
            "Installed Worldkiln executable is unavailable: {}",
            executable_template.display()
        ));
    }
    if probe_embedded_game(executable_template)?.is_some() {
        return Err("Build Game must run from the installed Worldkiln editor, not from a packaged game executable".to_string());
    }

    let project_name = project_name(project_path);

    let version = env!("CARGO_PKG_VERSION");

    if !bundle_runtime && runtime_asset.release_tag != format!("win11-v{version}") {
        return Err(format!(
            "Runtime release '{}' does not match Worldkiln {}",
            runtime_asset.release_tag, version
        ));
    }

    let game_id = planned_game_id(project_path)?;

    let builds_root = project_path
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("Builds");
    let project_build_root = builds_root.join(&project_name);
    fs::create_dir_all(&project_build_root).map_err(|error| {
        format!(
            "Failed to create build root {}: {error}",
            project_build_root.display()
        )
    })?;

    let output_dir = project_build_root.join(format!("v{version}"));
    let staging = unique_staging_dir(&project_build_root, &format!("v{version}"));
    fs::create_dir(&staging)
        .map_err(|error| format!("Failed to create build staging directory: {error}"))?;

    let result = (|| {
        on_stage(BuildStage::Cook);
        let snapshot = staging.join(".authoring-snapshot");
        check_cancel(cancel)?;
        snapshot_project_for_build(project_path, &snapshot)?;
        check_cancel(cancel)?;
        let engine_archive = if bundle_runtime {
            let path = staging.join(".engine-assets.zip");
            let installed_assets = executable_template
                .parent()
                .ok_or("Engine executable has no parent directory")?
                .join(".assets");
            let assets = if installed_assets.is_dir() {
                installed_assets
            } else {
                crate::project::engine_assets_dir()
            };
            let (sha, size) = create_engine_assets_archive(&assets, &path, version)?;
            Some((path, sha, size))
        } else {
            None
        };
        let payload = staging.join(".GameData.payload.zip");
        let (payload_sha256, payload_len) = create_project_payload_with_runtime(
            &snapshot,
            &payload,
            engine_archive.as_ref().map(|(path, _, _)| path.as_path()),
        )?;
        if let Some((path, _, _)) = &engine_archive {
            fs::remove_file(path).map_err(|error| format!("Cannot clean engine runtime staging: {error}"))?;
        }
        check_cancel(cancel)?;
        fs::remove_dir_all(&snapshot)
            .map_err(|error| format!("Cannot remove temporary export snapshot: {error}"))?;

        on_stage(BuildStage::Stage);
        let executable_base = if settings.executable_name.is_empty() {
            &project_name
        } else {
            &settings.executable_name
        };
        let executable_name = if cfg!(windows) {
            format!(
                "{}.exe",
                if settings.executable_name.is_empty() {
                    &project_name
                } else {
                    &settings.executable_name
                }
            )
        } else {
            executable_base.clone()
        };
        let staged_executable = staging.join(&executable_name);

        let executable_template_sha256 = crate::engine::game_package::sha256_file(executable_template)?.0;
        let manifest = EmbeddedGameManifest {
            format_version: 1,
            game_id: game_id.clone(),
            game_name: if settings.title.trim().is_empty() {
                project_name.clone()
            } else {
                settings.title.trim().to_string()
            },
            engine_version: version.to_string(),
            payload_sha256,
            runtime_release_tag: if bundle_runtime { String::new() } else { runtime_asset.release_tag.clone() },
            runtime_asset_name: runtime_asset.asset_name.clone(),
            runtime_asset_url: if bundle_runtime { String::new() } else { runtime_asset.browser_download_url.clone() },
            runtime_asset_sha256: engine_archive.as_ref().map_or_else(|| runtime_asset.sha256.clone(), |(_, sha, _)| sha.clone()),
            runtime_asset_size: engine_archive.as_ref().map_or(runtime_asset.size, |(_, _, size)| *size),
            export_profile: Some(GameExportProfile {
                display_title: if settings.title.trim().is_empty() {
                    project_name.clone()
                } else {
                    settings.title.trim().to_string()
                },
                game_version: settings.game_version.clone(),
                executable_name: executable_base.to_string(),
                build_configuration: settings.configuration.as_str().into(),
                window_mode: settings.window_mode.clone(),
                resolution: Some(settings.resolution),
                executable_template_sha256: Some(executable_template_sha256.clone()),
                runtime_delivery: if bundle_runtime {
                    "embedded_verified_runtime".into()
                } else {
                    "bundled_verified_runtime".into()
                },
                ..GameExportProfile::current_windows(&project_name)
            }),
        };

        let icon_template = if let Some(icon) = &settings.icon {
            let template_copy = staging.join(".game-icon-template.exe");
            fs::copy(executable_template, &template_copy)
                .map_err(|e| format!("Cannot stage game icon template: {e}"))?;
            #[cfg(windows)]
            embed_game_icon(&template_copy, icon)?;
            #[cfg(not(windows))]
            return Err("Custom executable icons require Windows".into());
            Some(template_copy)
        } else {
            None
        };
        on_stage(BuildStage::Package);
        append_game_package(
            icon_template.as_deref().unwrap_or(executable_template),
            &payload,
            &manifest,
            &staged_executable,
        )?;
        if let Some(template_copy) = icon_template {
            fs::remove_file(template_copy)
                .map_err(|e| format!("Cannot clean icon template: {e}"))?;
        }
        check_cancel(cancel)?;

        let packaged = probe_embedded_game(&staged_executable)?.ok_or_else(|| {
            "Staged game executable did not contain its embedded package".to_string()
        })?;
        verify_embedded_payload(&staged_executable, &packaged)?;

        let staged_size = fs::metadata(&staged_executable)
            .map_err(|error| format!("Failed to inspect staged game executable: {error}"))?
            .len();
        let _ = staged_size;
        fs::remove_file(&payload)
            .map_err(|error| format!("Failed to remove temporary game payload: {error}"))?;

        let output_entries = fs::read_dir(&staging)
            .map_err(|error| format!("Failed to inspect staged game build: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to inspect staged game build: {error}"))?;
        let expected_files = 1;
        if output_entries.len() != expected_files || !staged_executable.is_file() {
            return Err("Game build staging contains unexpected files".to_string());
        }

        check_cancel(cancel)?;
        persist_game_id(project_path, &game_id)?;
        let cleanup_warning = publish_build_directory(&staging, &output_dir)?;

        let executable = output_dir.join(executable_name);
        let mut log = format!(
            "Self-executable package created.\nEmbedded GameData: {payload_len} bytes.\nEngine assets SHA-256: {}.\n",
            manifest.runtime_asset_sha256
        );
        if !bundle_runtime {
            log.push_str(&format!("Runtime source: {}.\n", runtime_asset.release_tag));
        }
        log.push_str(&format!(
            "Executable template SHA-256: {executable_template_sha256}.\nPackaged game SHA-256: {}.\n",
            crate::engine::game_package::sha256_file(&executable)?.0
        ));
        if bundle_runtime {
            log.push_str("Build output contains one offline executable with embedded engine runtime assets.\n");
        } else {
            log.push_str("Build output contains one executable.\n");
        }

        if let Some(warning) = cleanup_warning {
            log.push_str(&format!("WARNING: {warning}\n"));
        }
        Ok(BuildResult {
            output_dir: output_dir.clone(),
            executable,
            output_log: log,
            archive_path: None,
        })
    })();

    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }

    result
}

pub fn build_game(project_path: &Path) -> Result<BuildResult, String> {
    build_game_with_options(project_path, &GameBuildOptions::default())
}
pub(super) fn check_cancel(cancel: Option<&std::sync::atomic::AtomicBool>) -> Result<(), String> {
    if cancel.is_some_and(|flag| flag.load(std::sync::atomic::Ordering::Relaxed)) {
        return Err("Build canceled".into());
    }
    Ok(())
}
pub fn build_game_with_cancel(
    project_path: &Path,
    options: &GameBuildOptions,
    cancel: &std::sync::atomic::AtomicBool,
) -> Result<BuildResult, String> {
    let mut ignore_stage = |_| {};
    build_game_with_options_inner(project_path, options, Some(cancel), &mut ignore_stage)
}
pub fn build_game_with_options(
    project_path: &Path,
    options: &GameBuildOptions,
) -> Result<BuildResult, String> {
    let mut ignore_stage = |_| {};
    build_game_with_options_inner(project_path, options, None, &mut ignore_stage)
}
pub fn build_game_with_progress(
    project_path: &Path,
    options: &GameBuildOptions,
    cancel: &std::sync::atomic::AtomicBool,
    on_stage: &mut dyn FnMut(BuildStage),
) -> Result<BuildResult, String> {
    build_game_with_options_inner(project_path, options, Some(cancel), on_stage)
}

fn build_game_with_options_inner(
    project_path: &Path,
    options: &GameBuildOptions,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    on_stage: &mut dyn FnMut(BuildStage),
) -> Result<BuildResult, String> {
    validate_options(options)?;
    let adapter: &dyn GamePlatformAdapter = match options.target {
        TargetPlatform::Windows => &WindowsGameAdapter,
        TargetPlatform::Linux => &LinuxGameAdapter,
        _ => {
            return Err(format!(
                "{} has no native export adapter",
                options.target.capabilities().name
            ));
        }
    };
    adapter.build(project_path, options, cancel, on_stage)
}

struct LinuxGameAdapter;
impl GamePlatformAdapter for LinuxGameAdapter {
    fn target(&self) -> TargetPlatform {
        TargetPlatform::Linux
    }
    fn build(
        &self,
        project_path: &Path,
        options: &GameBuildOptions,
        cancel: Option<&std::sync::atomic::AtomicBool>,
        on_stage: &mut dyn FnMut(BuildStage),
    ) -> Result<BuildResult, String> {
        debug_assert_eq!(self.target(), options.target);
        crate::engine::linux_game_build::build_linux_game(project_path, options, cancel, on_stage)
    }
}

struct WindowsGameAdapter;

impl GamePlatformAdapter for WindowsGameAdapter {
    fn target(&self) -> TargetPlatform {
        TargetPlatform::Windows
    }

    fn build(
        &self,
        project_path: &Path,
        options: &GameBuildOptions,
        cancel: Option<&std::sync::atomic::AtomicBool>,
        on_stage: &mut dyn FnMut(BuildStage),
    ) -> Result<BuildResult, String> {
        debug_assert_eq!(self.target(), options.target);
        build_windows_game(project_path, options, cancel, on_stage)
    }
}

fn build_windows_game(
    project_path: &Path,
    options: &GameBuildOptions,
    cancel: Option<&std::sync::atomic::AtomicBool>,
    on_stage: &mut dyn FnMut(BuildStage),
) -> Result<BuildResult, String> {
    check_cancel(cancel)?;
    if !cfg!(target_os = "windows") {
        return Err("Windows game exports require a native Windows runtime and build host.".into());
    }
    on_stage(BuildStage::Build);
    let executable_template = std::env::current_exe()
        .map_err(|error| format!("Failed to locate installed Worldkiln executable: {error}"))?;

    let runtime_asset = RuntimeReleaseAsset {
        release_tag: String::new(),
        asset_name: "UserData.zip".into(),
        browser_download_url: String::new(),
        sha256: String::new(),
        size: 0,
        immutable_release: false,
    };

    let mut result = package_game_with_template_inner(
        project_path,
        &executable_template,
        &runtime_asset,
        true,
        cancel,
        options,
        on_stage,
    )?;
    check_cancel(cancel)?;
    if options.output_parent.is_some() && options.format == GameBuildFormat::PlayableFolder {
        let folder = result.output_dir.clone();
        let project_title = project_name(project_path);
        let selected_parent = options
            .output_parent
            .as_deref()
            .unwrap_or_else(|| folder.parent().unwrap_or(Path::new(".")));
        let destination = selected_parent.join(format!("{project_title}-Windows"));
        let stage = unique_staging_dir(selected_parent, "game-delivery");
        fs::create_dir(&stage).map_err(|e| e.to_string())?;
        let delivery = (|| -> Result<PathBuf, String> {
            check_cancel(cancel)?;
            for entry in fs::read_dir(&folder).map_err(|e| e.to_string())? {
                let entry = entry.map_err(|e| e.to_string())?;
                let target = stage.join(entry.file_name());
                fs::copy(entry.path(), &target).map_err(|e| e.to_string())?;
            }
            check_cancel(cancel)?;
            let _ = publish_build_directory(&stage, &destination)?;
            Ok(destination)
        })();
        if delivery.is_err() && stage.exists() {
            let _ = fs::remove_dir_all(&stage);
        }
        let destination = delivery?;
        result.executable = destination.join(
            result
                .executable
                .file_name()
                .ok_or("Missing executable name")?,
        );
        result.output_dir = destination;
    }
    if options.format == GameBuildFormat::ZipArchive {
        let archive_parent = options
            .output_parent
            .as_deref()
            .unwrap_or_else(|| result.output_dir.parent().unwrap_or(Path::new(".")));
        let zip_path = archive_parent.join(format!("{}-Windows.zip", project_name(project_path)));
        let stage = zip_path.with_extension(format!("staging-{:032x}", rand::random::<u128>()));
        let writing = (|| -> Result<(), String> {
            let file = fs::File::create(&stage).map_err(|e| e.to_string())?;
            let mut zip = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for entry in fs::read_dir(&result.output_dir).map_err(|e| e.to_string())? {
                check_cancel(cancel)?;
                let entry = entry.map_err(|e| e.to_string())?;
                if !entry.file_type().map_err(|e| e.to_string())?.is_file() {
                    return Err("Unexpected directory in staged Windows game".into());
                }
                let filename = entry.file_name().to_string_lossy().into_owned();
                zip.start_file(filename, options)
                    .map_err(|e| e.to_string())?;
                let mut input = fs::File::open(entry.path()).map_err(|e| e.to_string())?;
                let mut buffer = [0_u8; 1024 * 1024];
                loop {
                    check_cancel(cancel)?;
                    let read =
                        std::io::Read::read(&mut input, &mut buffer).map_err(|e| e.to_string())?;
                    if read == 0 {
                        break;
                    }
                    std::io::Write::write_all(&mut zip, &buffer[..read])
                        .map_err(|e| e.to_string())?;
                }
            }
            zip.finish().map_err(|e| e.to_string())?;
            Ok(())
        })();
        if writing.is_err() {
            let _ = fs::remove_file(&stage);
        }
        writing?;
        check_cancel(cancel)?;
        let backup = zip_path.with_extension("zip.previous");
        if backup.exists() {
            return Err(format!(
                "Existing ZIP backup requires manual resolution: {}",
                backup.display()
            ));
        }
        if zip_path.exists() {
            fs::rename(&zip_path, &backup).map_err(|e| e.to_string())?;
        }
        if let Err(error) = fs::rename(&stage, &zip_path) {
            if backup.exists() {
                let _ = fs::rename(&backup, &zip_path);
            }
            return Err(format!("Could not publish ZIP: {error}"));
        }
        if backup.exists() {
            let _ = fs::remove_file(backup);
        }
        result
            .output_log
            .push_str(&format!("Shareable archive: {}\n", zip_path.display()));
        result.archive_path = Some(zip_path);
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn test_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        std::env::temp_dir().join(format!(
            "Worldkiln_BuildGame_{label}_{}_{}",
            std::process::id(),
            nonce
        ))
    }

    fn runtime_asset() -> RuntimeReleaseAsset {
        RuntimeReleaseAsset {
            release_tag: format!("win11-v{}", env!("CARGO_PKG_VERSION")),
            asset_name: "UserData.zip".to_string(),
            browser_download_url: format!(
                "https://github.com/Aeowun/Worldkiln/releases/download/win11-v{}/UserData.zip",
                env!("CARGO_PKG_VERSION")
            ),
            sha256: "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef".to_string(),
            size: 123,
            immutable_release: true,
        }
    }

    #[test]
    fn export_snapshot_preserves_authored_files_and_ignores_editor_data() {
        let root = test_root("authoring_snapshot");
        let project = root.join("Project");
        fs::create_dir_all(project.join("scripts")).unwrap();
        fs::create_dir_all(project.join(".worldkiln/development/logs")).unwrap();
        fs::write(project.join("world.dat"), b"new world").unwrap();
        fs::write(project.join("scripts/code.aeo"), b"print(1)").unwrap();
        fs::write(
            project.join(".worldkiln/development/logs/run.log"),
            b"diagnostic",
        )
        .unwrap();
        let snapshot = root.join("Snapshot");
        snapshot_project_for_build(&project, &snapshot).unwrap();
        assert_eq!(fs::read(snapshot.join("world.dat")).unwrap(), b"new world");
        assert!(snapshot.join("scripts/code.aeo").exists());
        assert!(!snapshot.join(".worldkiln").exists());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn publication_replaces_existing_output_without_reporting_cleanup_failure() {
        let root = test_root("publish");
        let staging = root.join("staging");
        let dest = root.join("published");
        fs::create_dir_all(&staging).unwrap();
        fs::create_dir_all(&dest).unwrap();
        fs::write(staging.join("Game.exe"), b"new").unwrap();
        fs::write(dest.join("Game.exe"), b"old").unwrap();
        let warning = publish_build_directory(&staging, &dest).unwrap();
        assert!(warning.is_none());
        assert_eq!(fs::read(dest.join("Game.exe")).unwrap(), b"new");
        assert!(!staging.exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn self_executable_build_contains_only_the_game_executable() {
        let root = test_root("single_file");
        let project = root.join("Game");
        fs::create_dir_all(project.join("scripts")).unwrap();
        fs::write(project.join("world.dat"), b"world").unwrap();
        fs::write(project.join("scripts/main.aeo"), b"print(\"hello\")").unwrap();
        let template = root.join("Worldkiln.exe");
        fs::write(&template, b"MZ fake Worldkiln template").unwrap();

        let result = package_game_with_template(&project, &template, &runtime_asset()).unwrap();

        let entries = fs::read_dir(&result.output_dir)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].path(), result.executable);
        assert!(result.executable.is_file());

        let package = probe_embedded_game(&result.executable).unwrap().unwrap();
        verify_embedded_payload(&result.executable, &package).unwrap();

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn failed_replacement_preserves_previous_successful_build() {
        let root = test_root("preserve");
        let project = root.join("Game");
        fs::create_dir_all(&project).unwrap();
        fs::write(project.join("world.dat"), b"world").unwrap();
        let template = root.join("Worldkiln.exe");
        fs::write(&template, b"MZ fake Worldkiln template").unwrap();

        let first = package_game_with_template(&project, &template, &runtime_asset()).unwrap();
        let original = fs::read(&first.executable).unwrap();

        let bad_template = root.join("AlreadyGame.exe");
        fs::copy(&first.executable, &bad_template).unwrap();
        assert!(package_game_with_template(&project, &bad_template, &runtime_asset()).is_err());

        assert_eq!(fs::read(&first.executable).unwrap(), original);

        fs::remove_dir_all(root).ok();
    }
}
