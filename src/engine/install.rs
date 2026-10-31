// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[path = "install_backup.rs"]
mod install_backup;
#[path = "install_core.rs"]
pub(crate) mod install_core;
pub(crate) use self::install_backup::{
    InstallBackup, create_install_backup, detected_legacy_assets, finish_install_backup,
    reconcile_userdata,
};

#[path = "install_progress.rs"]
mod install_progress;
use install_progress::{EXPECTED_PACKAGE_BYTES, copy_measured};
pub(crate) use install_progress::{PackageProgress, Phase};

const AEOWUN_INSTALL_DIR_NAME: &str = "Aeowun";
const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_INSTALL_ITEMS: [&str; 3] = [".assets", "UserData", "LICENSE"];
const CORE_RESOURCE_RECEIPT: &str = ".assets/core-resources.json";
const CORE_RESOURCE_DIRECTORIES: [&str; 2] = ["engine", "UI"];
const MIN_ENGINE_SOUND_FILES: usize = 30;

const PACKAGE_DOWNLOAD_ATTEMPTS: usize = 3;
const PACKAGE_DOWNLOAD_RETRY_DELAY: Duration = Duration::from_millis(700);
pub(crate) const SUPPORT_URL: &str = "https://aeowun.com/support/";

pub(crate) const INSTALL_CHECK_COMPLETE_ARG: &str = "--worldkiln-install-check-complete";
pub(crate) const LEGACY_INSTALL_CHECK_COMPLETE_ARG: &str = "--aeo-install-check-complete";
pub(crate) const DEVELOPMENT_BUILD_ARG: &str = "--worldkiln-development-build";
pub(crate) const LEGACY_DEVELOPMENT_BUILD_ARG: &str = "--aeo-development-build";

pub(crate) fn is_install_check_complete_arg(arg: &OsStr) -> bool {
    arg == INSTALL_CHECK_COMPLETE_ARG || arg == LEGACY_INSTALL_CHECK_COMPLETE_ARG
}

pub(crate) fn is_development_build_arg(arg: &OsStr) -> bool {
    arg == DEVELOPMENT_BUILD_ARG || arg == LEGACY_DEVELOPMENT_BUILD_ARG
}

fn package_download_url() -> String {
    format!(
        "https://github.com/Aeowun/Worldkiln/releases/download/win11-v{}/UserData.zip",
        ENGINE_VERSION
    )
}

const LICENSE_CONTENT: &str = include_str!("../../LICENSE");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PackageInstallState {
    Current,
    DevelopmentBuild,
}

fn compare_versions(left: &str, right: &str) -> Option<Ordering> {
    fn numeric_core(version: &str) -> Option<Vec<u64>> {
        let version = version.trim().strip_prefix('v').unwrap_or(version.trim());
        let core = version.split(['-', '+']).next()?;
        let parts: Vec<u64> = core
            .split('.')
            .map(str::parse)
            .collect::<Result<_, _>>()
            .ok()?;

        (!parts.is_empty()).then_some(parts)
    }

    let mut left = numeric_core(left)?;
    let mut right = numeric_core(right)?;
    let width = left.len().max(right.len());
    left.resize(width, 0);
    right.resize(width, 0);
    Some(left.cmp(&right))
}

pub(crate) fn aeowun_install_dir_from(local_app_data: &Path) -> PathBuf {
    local_app_data.join(AEOWUN_INSTALL_DIR_NAME)
}

pub(crate) fn package_install_required(install_dir: &Path) -> bool {
    if !core_resources_present(install_dir) {
        return true;
    }
    let version_path = install_dir.join(".assets").join("version.txt");
    match version_path.try_exists() {
        Ok(false) => return true,
        Err(_) => return false,
        Ok(true) => {}
    }
    let Ok(version) = std::fs::read_to_string(version_path) else {
        return false;
    };

    compare_versions(version.trim(), ENGINE_VERSION) == Some(Ordering::Less)
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CoreResourceReceipt {
    schema: u32,
    engine_version: String,
    files: Vec<CoreResourceFile>,
}

#[derive(Clone, Debug, serde::Deserialize, serde::Serialize)]
#[serde(deny_unknown_fields)]
struct CoreResourceFile {
    path: String,
    bytes: u64,
    sha256: String,
}

fn core_resources_present(install_dir: &Path) -> bool {
    let Ok(bytes) = std::fs::read(install_dir.join(CORE_RESOURCE_RECEIPT)) else {
        return false;
    };
    let Ok(receipt) = serde_json::from_slice::<CoreResourceReceipt>(&bytes) else {
        return false;
    };
    if receipt.schema != 1
        || receipt.engine_version != ENGINE_VERSION
        || !core_resource_inventory_complete(&receipt.files)
    {
        return false;
    }
    receipt.files.iter().all(|file| {
        let relative = Path::new(&file.path);
        !relative.is_absolute()
            && !relative.components().any(|component| {
                matches!(
                    component,
                    std::path::Component::ParentDir
                        | std::path::Component::RootDir
                        | std::path::Component::Prefix(_)
                )
            })
            && std::fs::metadata(install_dir.join(relative))
                .is_ok_and(|metadata| metadata.is_file() && metadata.len() == file.bytes)
    })
}

fn core_resource_inventory_complete(files: &[CoreResourceFile]) -> bool {
    let paths: std::collections::BTreeSet<&str> =
        files.iter().map(|file| file.path.as_str()).collect();
    let required_static = [
        ".assets/UI/Button.json",
        ".assets/UI/Panel.json",
        ".assets/UI/Text.json",
    ];
    required_static
        .iter()
        .all(|required| paths.contains(required))
        && crate::engine::audio::EngineSound::ALL
            .iter()
            .all(|sound| paths.contains(format!(".assets/engine/{}", sound.file_name()).as_str()))
        && files
            .iter()
            .filter(|file| file.path.starts_with(".assets/engine/") && file.path.ends_with(".wav"))
            .count()
            >= MIN_ENGINE_SOUND_FILES
}

#[cfg(target_os = "windows")]
pub(crate) fn aeowun_install_dir() -> Result<PathBuf, String> {
    let local_app_data =
        std::env::var_os("LOCALAPPDATA").ok_or_else(|| "LOCALAPPDATA is not set".to_string())?;

    Ok(aeowun_install_dir_from(&PathBuf::from(local_app_data)))
}

pub(crate) fn missing_install_items(install_dir: &Path) -> Vec<&'static str> {
    REQUIRED_INSTALL_ITEMS
        .iter()
        .copied()
        .filter(|item| !install_dir.join(*item).exists())
        .collect()
}

pub(crate) const CANONICAL_EXECUTABLE_NAME: &str = "Worldkiln.exe";
pub(crate) const LEGACY_EXECUTABLE_NAME: &str = "AeoEngine.exe";
const INSTALLER_EXECUTABLE_NAME: &str = "Installer.exe";

fn paths_refer_to_same_file(left: &Path, right: &Path) -> bool {
    if left == right {
        return true;
    }

    let (Ok(left), Ok(right)) = (std::fs::canonicalize(left), std::fs::canonicalize(right)) else {
        return false;
    };

    #[cfg(target_os = "windows")]
    {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    }

    #[cfg(not(target_os = "windows"))]
    {
        left == right
    }
}

fn is_installer_executable(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        name.to_string_lossy()
            .eq_ignore_ascii_case(INSTALLER_EXECUTABLE_NAME)
    })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum ExecutableRole {
    Installer,
    InstalledEditor,
    ExternalEditor,
}

pub(crate) fn executable_role(current_exe: &Path, install_dir: &Path) -> ExecutableRole {
    if is_installer_executable(current_exe) {
        ExecutableRole::Installer
    } else if paths_refer_to_same_file(current_exe, &install_dir.join(CANONICAL_EXECUTABLE_NAME)) {
        ExecutableRole::InstalledEditor
    } else {
        ExecutableRole::ExternalEditor
    }
}

pub(crate) fn installer_bootstrap_required(
    role: ExecutableRole,
    installation_required: bool,
    _install_complete_requested: bool,
) -> bool {
    match role {
        ExecutableRole::Installer | ExecutableRole::ExternalEditor => true,
        ExecutableRole::InstalledEditor => installation_required,
    }
}

pub(crate) fn installer_required(current_exe: &Path, install_dir: &Path) -> bool {
    let canonical_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);
    let legacy_exe = install_dir.join(LEGACY_EXECUTABLE_NAME);

    !paths_refer_to_same_file(current_exe, &canonical_exe)
        || legacy_exe.exists()
        || package_install_required(install_dir)
        || !install_dir.join("LICENSE").exists()
}

pub(crate) fn cleanup_legacy_executable(install_dir: &Path) -> Result<(), String> {
    let legacy_exe = install_dir.join(LEGACY_EXECUTABLE_NAME);
    if legacy_exe.exists() {
        let _ = std::fs::remove_file(&legacy_exe);
    }
    Ok(())
}

pub(crate) fn install_engine_executable(
    source_exe: &Path,
    install_dir: &Path,
) -> Result<PathBuf, String> {
    std::fs::create_dir_all(install_dir).map_err(|err| {
        format!(
            "Failed to create Aeowun install directory {}: {err}",
            install_dir.display()
        )
    })?;

    let canonical_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);
    let legacy_exe = install_dir.join(LEGACY_EXECUTABLE_NAME);

    if !paths_refer_to_same_file(source_exe, &canonical_exe) {
        let stage = install_dir.join(format!(
            ".Worldkiln-stage-{:032x}.exe",
            rand::random::<u128>()
        ));
        let recovery = install_dir.join(format!(
            ".Worldkiln-recovery-{:032x}.exe",
            rand::random::<u128>()
        ));
        let result = (|| {
            std::fs::copy(source_exe, &stage).map_err(|e| {
                format!(
                    "Failed to stage executable {} to {}: {e}",
                    source_exe.display(),
                    stage.display()
                )
            })?;
            let source_hash =
                crate::engine::game_package::sha256_file(source_exe).map_err(|error| {
                    format!(
                        "Failed to hash installer executable {}: {error}",
                        source_exe.display()
                    )
                })?;
            if source_hash.1 == 0
                || crate::engine::game_package::sha256_file(&stage).map_err(|error| {
                    format!(
                        "Failed to verify staged executable {}: {error}",
                        stage.display()
                    )
                })? != source_hash
            {
                return Err(format!(
                    "Staged executable {} failed integrity verification against {}",
                    stage.display(),
                    source_exe.display()
                ));
            }
            std::fs::OpenOptions::new()
                .write(true)
                .open(&stage)
                .and_then(|file| file.sync_all())
                .map_err(|error| {
                    format!(
                        "Failed to sync staged executable {}: {error}",
                        stage.display()
                    )
                })?;
            let replaced = canonical_exe.exists();
            if replaced {
                std::fs::rename(&canonical_exe, &recovery).map_err(|e| {
                    format!(
                        "Cannot stage installed executable {} for rollback at {}: {e}",
                        canonical_exe.display(),
                        recovery.display()
                    )
                })?;
            }
            if let Err(error) = std::fs::rename(&stage, &canonical_exe) {
                if replaced {
                    std::fs::rename(&recovery,&canonical_exe).map_err(|rollback|format!("Failed to publish executable {} to {}: {error}; failed to restore {} to {}: {rollback}; recovery retained",stage.display(),canonical_exe.display(),recovery.display(),canonical_exe.display()))?;
                }
                return Err(format!(
                    "Failed to publish executable {} to {}: {error}",
                    stage.display(),
                    canonical_exe.display()
                ));
            }
            if crate::engine::game_package::sha256_file(&canonical_exe).map_err(|error| {
                format!(
                    "Failed to verify installed executable {}: {error}",
                    canonical_exe.display()
                )
            })? != source_hash
            {
                let _ = std::fs::remove_file(&canonical_exe);
                if replaced {
                    std::fs::rename(&recovery, &canonical_exe).map_err(|e| {
                        format!("Executable {} verification failed; failed to restore {}: {e}; recovery retained", canonical_exe.display(), recovery.display())
                    })?;
                }
                return Err(format!(
                    "Published executable {} failed integrity verification",
                    canonical_exe.display()
                ));
            }
            if replaced {
                std::fs::remove_file(&recovery).map_err(|e| {
                    format!(
                        "Executable installed but obsolete recovery cleanup failed at {}: {e}",
                        recovery.display()
                    )
                })?;
            }
            Ok(())
        })();
        let _ = std::fs::remove_file(&stage);
        result?;
    }

    if !canonical_exe.exists() {
        return Err(format!(
            "Failed to verify canonical executable at {}",
            canonical_exe.display()
        ));
    }

    if source_exe != legacy_exe {
        let _ = cleanup_legacy_executable(install_dir);
    }

    Ok(canonical_exe)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DownloadFailureKind {
    Connection,
    TemporaryServer,
    Other,
}

#[derive(Debug)]
struct DownloadFailure {
    kind: DownloadFailureKind,
    detail: String,
}

impl DownloadFailure {
    fn from_request_error(error: reqwest::Error) -> Self {
        let kind = if error.is_connect() || error.is_timeout() {
            DownloadFailureKind::Connection
        } else {
            DownloadFailureKind::Other
        };

        Self {
            kind,
            detail: error.to_string(),
        }
    }

    fn is_retryable(&self) -> bool {
        matches!(
            self.kind,
            DownloadFailureKind::Connection | DownloadFailureKind::TemporaryServer
        )
    }
}

fn classify_http_status(status: reqwest::StatusCode) -> DownloadFailureKind {
    if status.is_server_error() || status == reqwest::StatusCode::TOO_MANY_REQUESTS {
        DownloadFailureKind::TemporaryServer
    } else {
        DownloadFailureKind::Other
    }
}

fn retry_package_download(
    mut download: impl FnMut() -> Result<(), DownloadFailure>,
    mut pause: impl FnMut(Duration),
) -> Result<(), DownloadFailure> {
    for attempt in 1..=PACKAGE_DOWNLOAD_ATTEMPTS {
        match download() {
            Ok(()) => return Ok(()),
            Err(error) if error.is_retryable() && attempt < PACKAGE_DOWNLOAD_ATTEMPTS => {
                pause(PACKAGE_DOWNLOAD_RETRY_DELAY * attempt as u32);
            }
            Err(error) => return Err(error),
        }
    }

    unreachable!("the bounded package download loop always returns")
}

fn download_failure_message(error: DownloadFailure) -> String {
    let attempts = PACKAGE_DOWNLOAD_ATTEMPTS;
    match error.kind {
        DownloadFailureKind::Connection => format!(
            "Could not connect to GitHub to download Worldkiln's required files after {attempts} attempts. Check your internet connection and try again. If the problem continues, visit Aeowun Support. Details: {}",
            error.detail
        ),
        DownloadFailureKind::TemporaryServer => format!(
            "GitHub is temporarily unavailable while downloading Worldkiln's required files after {attempts} attempts. Try again later or visit Aeowun Support. Details: {}",
            error.detail
        ),
        DownloadFailureKind::Other => format!(
            "Failed to download Worldkiln's required files from GitHub. Try again or visit Aeowun Support. Details: {}",
            error.detail
        ),
    }
}

fn download_with_native_fallback(
    destination: &Path,
    rust_download: impl FnMut() -> Result<(), DownloadFailure>,
    native_download: impl FnOnce() -> Result<(), String>,
    pause: impl FnMut(Duration),
) -> Result<(), String> {
    match retry_package_download(rust_download, pause) {
        Ok(()) => {}
        Err(error) if error.kind == DownloadFailureKind::Connection => {
            let rust_error = download_failure_message(error);
            let _ = std::fs::remove_file(destination);
            if let Err(native_error) = native_download() {
                let _ = std::fs::remove_file(destination);
                return Err(format!(
                    "{rust_error} Native Windows download fallback also failed: {native_error}"
                ));
            }
        }
        Err(error) => {
            let _ = std::fs::remove_file(destination);
            return Err(download_failure_message(error));
        }
    }

    Ok(())
}

#[cfg(test)]
fn download_with_curl(url: &str, destination: &Path) -> Result<(), String> {
    download_with_curl_with_progress(url, destination, &|_| {})
}
fn download_with_curl_with_progress(
    url: &str,
    destination: &Path,
    progress: &dyn Fn(PackageProgress),
) -> Result<(), String> {
    let curl_executable = if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    };
    let mut child = Command::new(curl_executable)
        .args([
            "--fail",
            "--location",
            "--silent",
            "--show-error",
            "--retry",
            "2",
            "--retry-delay",
            "1",
            "--retry-connrefused",
            "--noproxy",
            "127.0.0.1,localhost",
            "--connect-timeout",
            "15",
            "--max-time",
            "120",
            "--output",
        ])
        .arg(destination)
        .arg(url)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::piped())
        .spawn()
        .map_err(|error| format!("Failed to start {curl_executable}: {error}"))?;
    loop {
        progress(PackageProgress {
            phase: Phase::Download,
            completed: std::fs::metadata(destination).map(|m| m.len()).unwrap_or(0),
            total: EXPECTED_PACKAGE_BYTES,
        });
        if child
            .try_wait()
            .map_err(|e| format!("Failed to inspect {curl_executable}: {e}"))?
            .is_some()
        {
            break;
        }
        std::thread::sleep(Duration::from_millis(75));
    }
    let output = child
        .wait_with_output()
        .map_err(|e| format!("Failed to collect {curl_executable}: {e}"))?;

    if output.status.success() {
        return Ok(());
    }

    let _ = std::fs::remove_file(destination);
    let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
    Err(if detail.is_empty() {
        format!("{curl_executable} exited with {}", output.status)
    } else {
        format!("{curl_executable} exited with {}: {detail}", output.status)
    })
}

#[cfg(test)]
fn verify_package_zip(zip_path: &Path) -> Result<PackageInstallState, String> {
    verify_package_zip_with_progress(zip_path, &|_| {})
}
fn verify_package_zip_with_progress(
    zip_path: &Path,
    progress: &dyn Fn(PackageProgress),
) -> Result<PackageInstallState, String> {
    progress(PackageProgress {
        phase: Phase::Verify,
        completed: 0,
        total: 1,
    });
    let zip_file = std::fs::File::open(zip_path).map_err(|error| {
        format!(
            "Failed to open downloaded UserData ZIP {}: {error}",
            zip_path.display()
        )
    })?;
    let mut archive = zip::ZipArchive::new(zip_file).map_err(|error| {
        format!(
            "Downloaded UserData file {} is not a valid ZIP: {error}",
            zip_path.display()
        )
    })?;

    let mut marker_index = None;
    let mut package_prefix = None;
    let mut package_version = None;
    let mut entry_names = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let mut entry = archive.by_index(index).map_err(|error| {
            format!(
                "Failed to inspect downloaded UserData ZIP {} entry {index}: {error}",
                zip_path.display()
            )
        })?;
        let name = entry.name().replace('\\', "/");
        entry_names.push(name.clone());

        if let Some(prefix) = name.strip_suffix(".assets/version.txt") {
            let prefix = prefix.to_string();
            let root_name = prefix.trim_end_matches('/');
            if !root_name.is_empty() && root_name.contains('/') {
                continue;
            }
            if marker_index.replace(index).is_some() {
                return Err(
                    "Downloaded UserData ZIP contains multiple package version markers".into(),
                );
            }
            let mut version = String::new();
            entry.read_to_string(&mut version).map_err(|error| {
                format!(
                    "Failed to read package version marker {name} in {}: {error}",
                    zip_path.display()
                )
            })?;
            package_prefix = Some(prefix);
            package_version = Some(version.trim().to_string());
        } else {
            std::io::copy(&mut entry, &mut std::io::sink()).map_err(|error| {
                format!(
                    "Downloaded UserData ZIP {} entry {name} failed integrity checking: {error}",
                    zip_path.display()
                )
            })?;
        }
    }

    let prefix = package_prefix.ok_or_else(|| {
        "Downloaded UserData ZIP does not contain .assets/version.txt at its root or one folder below".to_string()
    })?;
    let has_userdata = entry_names.iter().any(|name| {
        name.strip_prefix(&prefix)
            .is_some_and(|relative| relative.starts_with("UserData/"))
    });
    if !has_userdata {
        return Err("Downloaded UserData ZIP does not contain a UserData directory".into());
    }

    let version = package_version.expect("version marker and version are recorded together");
    classify_package_version(&version)
}

fn download_file(
    url: &str,
    destination: &Path,
    progress: &dyn Fn(PackageProgress),
) -> Result<(), String> {
    let client = reqwest::blocking::Client::builder()
        .build()
        .map_err(|err| format!("Failed to prepare GitHub download: {err}"))?;

    download_with_native_fallback(
        destination,
        || {
            let mut response = client
                .get(url)
                .send()
                .map_err(DownloadFailure::from_request_error)?;

            let status = response.status();
            if !status.is_success() {
                return Err(DownloadFailure {
                    kind: classify_http_status(status),
                    detail: format!("GitHub returned HTTP {status}"),
                });
            }

            let total = response
                .content_length()
                .filter(|size| *size > 0)
                .unwrap_or(EXPECTED_PACKAGE_BYTES);
            let mut output = std::fs::File::create(destination).map_err(|err| DownloadFailure {
                kind: DownloadFailureKind::Other,
                detail: format!("Failed to create {}: {err}", destination.display()),
            })?;

            copy_measured(&mut response, &mut output, |completed| {
                progress(PackageProgress {
                    phase: Phase::Download,
                    completed,
                    total,
                })
            })
            .map_err(|err| DownloadFailure {
                kind: DownloadFailureKind::Other,
                detail: format!("Failed to write {}: {err}", destination.display()),
            })?;

            Ok(())
        },
        || download_with_curl_with_progress(url, destination, progress),
        std::thread::sleep,
    )
}

fn collect_core_resource_files(
    root: &Path,
    current: &Path,
    output: &mut Vec<CoreResourceFile>,
) -> Result<(), String> {
    for entry in std::fs::read_dir(current)
        .map_err(|error| format!("Failed to inspect {}: {error}", current.display()))?
    {
        let entry = entry.map_err(|error| {
            format!(
                "Failed to inspect core resource entry in {}: {error}",
                current.display()
            )
        })?;
        let path = entry.path();
        let metadata = std::fs::symlink_metadata(&path)
            .map_err(|error| format!("Failed to inspect {}: {error}", path.display()))?;
        if metadata.file_type().is_symlink() {
            return Err(format!(
                "Linked core resource is unsupported: {}",
                path.display()
            ));
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;
            if metadata.file_attributes() & 0x400 != 0 {
                return Err(format!(
                    "Reparse core resource is unsupported: {}",
                    path.display()
                ));
            }
        }
        if metadata.is_dir() {
            collect_core_resource_files(root, &path, output)?;
            continue;
        }
        if !metadata.is_file() {
            return Err(format!("Unsupported core resource: {}", path.display()));
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|error| format!("Invalid core resource path: {error}"))?;
        let (sha256, bytes) = crate::engine::game_package::sha256_file(&path).map_err(|error| {
            format!(
                "Failed to hash packaged core resource {}: {error}",
                path.display()
            )
        })?;
        output.push(CoreResourceFile {
            path: relative.to_string_lossy().replace('\\', "/"),
            bytes,
            sha256,
        });
    }
    Ok(())
}

fn validate_core_resource_set(stage_root: &Path, files: &[CoreResourceFile]) -> Result<(), String> {
    let engine_sounds = files
        .iter()
        .filter(|file| file.path.starts_with(".assets/engine/") && file.path.ends_with(".wav"))
        .count();
    if !core_resource_inventory_complete(files) {
        return Err(format!(
            "Published core package is incomplete: required UI definitions or engine sounds are missing ({engine_sounds} engine sounds found)"
        ));
    }
    for file in files {
        let staged = stage_root.join(Path::new(&file.path));
        if crate::engine::game_package::sha256_file(&staged).map_err(|error| {
            format!(
                "Failed to verify staged core resource {}: {error}",
                staged.display()
            )
        })? != (file.sha256.clone(), file.bytes)
        {
            return Err(format!("Core resource verification failed: {}", file.path));
        }
    }
    Ok(())
}

fn preflight_core_resource_ownership(
    install_dir: &Path,
    resources: &[CoreResourceFile],
) -> Result<(), String> {
    let previous = std::fs::read(install_dir.join(CORE_RESOURCE_RECEIPT))
        .ok()
        .and_then(|bytes| serde_json::from_slice::<CoreResourceReceipt>(&bytes).ok())
        .filter(|receipt| receipt.schema == 1);
    for resource in resources {
        let destination = install_dir.join(Path::new(&resource.path));
        reject_linked_install_path(&destination)?;
        match std::fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(format!(
                    "Cannot inspect core resource destination {}: {error}",
                    destination.display()
                ));
            }
            Ok(metadata) if !metadata.is_file() => {
                return Err(format!(
                    "Refusing non-file core resource destination: {}",
                    destination.display()
                ));
            }
            Ok(_) => {}
        }
        let on_disk = crate::engine::game_package::sha256_file(&destination).map_err(|error| {
            format!(
                "Failed to verify core resource destination {}: {error}",
                destination.display()
            )
        })?;
        if on_disk == (resource.sha256.clone(), resource.bytes) {
            continue;
        }
        let previously_owned_and_unchanged = previous.as_ref().is_some_and(|receipt| {
            receipt.files.iter().any(|old| {
                old.path == resource.path
                    && (old.sha256.as_str(), old.bytes) == (on_disk.0.as_str(), on_disk.1)
            })
        });
        if !previously_owned_and_unchanged {
            return Err(format!(
                "Core resource {} differs from the package and is not an unchanged, receipt-verified engine file; existing user asset preserved",
                destination.display()
            ));
        }
    }
    Ok(())
}

fn install_core_resources_from_zip(
    zip_path: &Path,
    install_dir: &Path,
    progress: &dyn Fn(PackageProgress),
) -> Result<PackageInstallState, String> {
    let install_state = verify_package_zip_with_progress(zip_path, progress)?;
    let stage = install_dir.join(format!(".core-resources-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&stage).map_err(|error| {
        format!(
            "Failed to create core resource staging {}: {error}",
            stage.display()
        )
    })?;
    let result = (|| {
        progress(PackageProgress {
            phase: Phase::Extract,
            completed: 0,
            total: 1,
        });
        let file = std::fs::File::open(zip_path).map_err(|error| {
            format!(
                "Failed to open downloaded UserData ZIP {}: {error}",
                zip_path.display()
            )
        })?;
        let mut archive = zip::ZipArchive::new(file).map_err(|error| {
            format!(
                "Downloaded UserData file {} is not a valid ZIP: {error}",
                zip_path.display()
            )
        })?;
        archive.extract(&stage).map_err(|error| {
            format!(
                "Failed to extract core resource package {} to {}: {error}",
                zip_path.display(),
                stage.display()
            )
        })?;
        let package_root = resolve_package_root(&stage)?;
        let (package_version, _) = validate_package_version(&package_root)?;
        let staged_assets = package_root.join(".assets");
        let mut resources = Vec::new();
        for directory in CORE_RESOURCE_DIRECTORIES {
            let source = staged_assets.join(directory);
            if !source.is_dir() {
                return Err(format!(
                    "Published core package is missing .assets/{directory}"
                ));
            }
            collect_core_resource_files(&package_root, &source, &mut resources)?;
        }
        resources.sort_by(|left, right| left.path.cmp(&right.path));
        validate_core_resource_set(&package_root, &resources)?;
        preflight_core_resource_ownership(install_dir, &resources)?;

        progress(PackageProgress {
            phase: Phase::Install,
            completed: 0,
            total: 1,
        });
        for resource in &resources {
            let source = package_root.join(Path::new(&resource.path));
            let destination = install_dir.join(Path::new(&resource.path));
            preflight_core_resource_ownership(install_dir, std::slice::from_ref(resource))?;
            if destination.is_file()
                && crate::engine::game_package::sha256_file(&destination).map_err(|error| {
                    format!(
                        "Failed to verify core resource destination {}: {error}",
                        destination.display()
                    )
                })? == (resource.sha256.clone(), resource.bytes)
            {
                continue;
            }
            let bytes = std::fs::read(&source)
                .map_err(|error| format!("Failed to read {}: {error}", source.display()))?;
            crate::project::atomic_file::write_atomically(&destination, &bytes)
                .map_err(|error| format!("Failed to install {}: {error}", destination.display()))?;
            if crate::engine::game_package::sha256_file(&destination).map_err(|error| {
                format!(
                    "Failed to verify core resource destination {}: {error}",
                    destination.display()
                )
            })? != (resource.sha256.clone(), resource.bytes)
            {
                return Err(format!(
                    "Installed core resource did not verify: {}",
                    resource.path
                ));
            }
        }
        let receipt = CoreResourceReceipt {
            schema: 1,
            engine_version: ENGINE_VERSION.to_owned(),
            files: resources,
        };
        crate::project::atomic_file::write_atomically(
            &install_dir.join(".assets/version.txt"),
            package_version.as_bytes(),
        )
        .map_err(|error| {
            format!(
                "Failed to record core resource version {}: {error}",
                install_dir.join(".assets/version.txt").display()
            )
        })?;
        crate::project::atomic_file::write_atomically(
            &install_dir.join(CORE_RESOURCE_RECEIPT),
            &serde_json::to_vec_pretty(&receipt).map_err(|error| {
                format!(
                    "Failed to encode installed core resource receipt {}: {error}",
                    install_dir.join(CORE_RESOURCE_RECEIPT).display()
                )
            })?,
        )
        .map_err(|error| {
            format!(
                "Failed to record installed core resources {}: {error}",
                install_dir.join(CORE_RESOURCE_RECEIPT).display()
            )
        })?;
        if !core_resources_present(install_dir) {
            return Err("Installed core resource receipt did not verify".into());
        }
        Ok(install_state)
    })();
    let cleanup = std::fs::remove_dir_all(&stage);
    match (result, cleanup) {
        (Ok(state), Ok(())) => Ok(state),
        (Ok(_), Err(error)) => Err(format!(
            "Failed to clean core resource staging {}: {error}",
            stage.display()
        )),
        (Err(error), Ok(())) => Err(error),
        (Err(error), Err(cleanup)) => Err(format!(
            "{error}; failed to clean core resource staging {}: {cleanup}",
            stage.display()
        )),
    }
}

pub(crate) fn install_userdata_from_zip(zip_path: &Path, install_dir: &Path) -> Result<(), String> {
    let userdata_dir = install_dir.join("UserData");

    if userdata_dir.exists() {
        return Ok(());
    }

    let extraction_dir =
        install_dir.join(format!(".UserData_extract-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&extraction_dir)
        .map_err(|err| format!("Failed to create UserData extraction directory: {err}"))?;

    let zip_file = std::fs::File::open(zip_path)
        .map_err(|err| format!("Failed to open {}: {err}", zip_path.display()))?;

    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|err| format!("Invalid UserData ZIP: {err}"))?;

    archive
        .extract(&extraction_dir)
        .map_err(|err| format!("Failed to extract UserData ZIP: {err}"))?;

    let packaged_userdata = extraction_dir.join("UserData");

    if packaged_userdata.is_dir() {
        std::fs::rename(&packaged_userdata, &userdata_dir)
            .map_err(|err| format!("Failed to install UserData directory: {err}"))?;

        std::fs::remove_dir_all(&extraction_dir)
            .map_err(|err| format!("Failed to clean UserData extraction directory: {err}"))?;
    } else {
        std::fs::rename(&extraction_dir, &userdata_dir)
            .map_err(|err| format!("Failed to install UserData directory: {err}"))?;
    }

    Ok(())
}

fn reject_linked_install_path(path: &Path) -> Result<(), String> {
    for ancestor in path.ancestors() {
        match std::fs::symlink_metadata(ancestor) {
            Ok(metadata) => {
                if metadata.file_type().is_symlink() {
                    return Err(format!(
                        "Refusing linked installation path: {}",
                        path.display()
                    ));
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if metadata.file_attributes() & 0x400 != 0 {
                        return Err(format!(
                            "Refusing reparse installation path: {}",
                            path.display()
                        ));
                    }
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!("Cannot inspect {}: {error}", ancestor.display()));
            }
        }
    }
    Ok(())
}

fn copy_missing_tree(
    source: &Path,
    destination: &Path,
    skip_source: Option<&Path>,
) -> Result<(), String> {
    reject_linked_install_path(source)?;
    reject_linked_install_path(destination)?;
    std::fs::create_dir_all(destination)
        .map_err(|err| format!("Failed to create {}: {err}", destination.display()))?;

    let entries = std::fs::read_dir(source)
        .map_err(|err| format!("Failed to read {}: {err}", source.display()))?;

    for entry in entries {
        let entry = entry.map_err(|err| format!("Failed to read package entry: {err}"))?;
        let source_path = entry.path();
        if skip_source == Some(source_path.as_path()) {
            continue;
        }
        let destination_path = destination.join(entry.file_name());
        reject_linked_install_path(&source_path)?;
        reject_linked_install_path(&destination_path)?;

        if source_path.is_dir() {
            copy_missing_tree(&source_path, &destination_path, skip_source)?;
            continue;
        }
        if !source_path.is_file() {
            return Err(format!(
                "Unsupported package file: {}",
                source_path.display()
            ));
        }
        match std::fs::symlink_metadata(&destination_path) {
            Ok(metadata) if metadata.is_file() => continue,
            Ok(_) => {
                return Err(format!(
                    "Refusing non-file install destination: {}",
                    destination_path.display()
                ));
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => {
                return Err(format!(
                    "Cannot inspect {}: {error}",
                    destination_path.display()
                ));
            }
        }

        let mut from = std::fs::File::open(&source_path)
            .map_err(|error| format!("Failed to open {}: {error}", source_path.display()))?;
        let mut to = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&destination_path)
            .map_err(|error| {
                format!(
                    "Refusing to replace {}: {error}",
                    destination_path.display()
                )
            })?;
        std::io::copy(&mut from, &mut to)
            .and_then(|_| to.sync_all())
            .map_err(|error| {
                format!("Failed to install {}: {error}", destination_path.display())
            })?;
    }
    Ok(())
}

fn resolve_package_root(extraction_dir: &Path) -> Result<PathBuf, String> {
    if extraction_dir.join(".assets").join("version.txt").is_file() {
        return Ok(extraction_dir.to_path_buf());
    }

    let entries = std::fs::read_dir(extraction_dir)
        .map_err(|err| format!("Failed to inspect extracted package: {err}"))?;

    let mut package_roots = Vec::new();

    for entry in entries {
        let entry =
            entry.map_err(|err| format!("Failed to inspect extracted package entry: {err}"))?;

        let path = entry.path();

        if path.is_dir() && path.join(".assets").join("version.txt").is_file() {
            package_roots.push(path);
        }
    }

    match package_roots.len() {
        1 => Ok(package_roots.remove(0)),
        0 => Err(
            "Package does not contain .assets/version.txt at its root or inside one top-level directory"
                .to_string(),
        ),
        _ => Err("Package contains multiple possible package roots".to_string()),
    }
}

fn classify_package_version(version: &str) -> Result<PackageInstallState, String> {
    match compare_versions(version, ENGINE_VERSION) {
        Some(Ordering::Equal) => Ok(PackageInstallState::Current),
        Some(Ordering::Less) => Ok(PackageInstallState::DevelopmentBuild),
        _ => Err(format!(
            "Package version {version} does not match Worldkiln {ENGINE_VERSION}"
        )),
    }
}

fn validate_package_version(
    extraction_dir: &Path,
) -> Result<(String, PackageInstallState), String> {
    let version_path = extraction_dir.join(".assets").join("version.txt");

    let version = std::fs::read_to_string(&version_path)
        .map_err(|err| format!("Package is missing .assets/version.txt: {err}"))?;

    let version = version.trim().to_string();

    let state = classify_package_version(&version)?;
    Ok((version, state))
}

pub(crate) fn install_package_from_zip(
    zip_path: &Path,
    install_dir: &Path,
) -> Result<PackageInstallState, String> {
    install_package_from_zip_guarded(zip_path, install_dir, None)
}

fn install_package_from_zip_guarded(
    zip_path: &Path,
    install_dir: &Path,
    backup: Option<&InstallBackup>,
) -> Result<PackageInstallState, String> {
    install_package_from_zip_with_progress(zip_path, install_dir, backup, &|_| {})
}
fn install_package_from_zip_with_progress(
    zip_path: &Path,
    install_dir: &Path,
    backup: Option<&InstallBackup>,
    progress: &dyn Fn(PackageProgress),
) -> Result<PackageInstallState, String> {
    let _verified_state = verify_package_zip_with_progress(zip_path, progress)?;
    let extraction_dir =
        install_dir.join(format!(".package_extract-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&extraction_dir)
        .map_err(|err| format!("Failed to create package extraction directory: {err}"))?;

    let zip_file = std::fs::File::open(zip_path)
        .map_err(|err| format!("Failed to open {}: {err}", zip_path.display()))?;

    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|err| format!("Invalid package ZIP: {err}"))?;

    progress(PackageProgress {
        phase: Phase::Extract,
        completed: 0,
        total: 1,
    });
    archive
        .extract(&extraction_dir)
        .map_err(|err| format!("Failed to extract package ZIP: {err}"))?;

    progress(PackageProgress {
        phase: Phase::Install,
        completed: 0,
        total: 1,
    });
    let package_root = resolve_package_root(&extraction_dir)?;

    let (package_version, install_state) = validate_package_version(&package_root)?;

    let packaged_assets = package_root.join(".assets");
    let packaged_userdata = package_root.join("UserData");
    let packaged_license = package_root.join("LICENSE");

    let installed_assets = install_dir.join(".assets");
    let installed_userdata = install_dir.join("UserData");
    let installed_license = install_dir.join("LICENSE");

    let packaged_version = packaged_assets.join("version.txt");

    if packaged_assets.is_dir() {
        copy_missing_tree(
            &packaged_assets,
            &installed_assets,
            Some(packaged_version.as_path()),
        )?;
    }

    if packaged_userdata.is_dir() {
        copy_missing_tree(&packaged_userdata, &installed_userdata, None)?;
    }

    install_material_library(&package_root, install_dir)?;

    if let Some(backup) = backup {
        reconcile_userdata(backup, install_dir)?;
    }

    if !installed_license.exists() && packaged_license.is_file() {
        std::fs::copy(&packaged_license, &installed_license)
            .map_err(|err| format!("Failed to install LICENSE: {err}"))?;
    }
    ensure_license_installed(install_dir)?;

    std::fs::create_dir_all(&installed_assets)
        .map_err(|err| format!("Failed to create .assets directory: {err}"))?;

    std::fs::write(installed_assets.join("version.txt"), package_version)
        .map_err(|err| format!("Failed to update .assets/version.txt: {err}"))?;

    std::fs::remove_dir_all(&extraction_dir)
        .map_err(|err| format!("Failed to clean package extraction directory: {err}"))?;

    Ok(install_state)
}

pub(crate) fn ensure_package_installed(install_dir: &Path) -> Result<PackageInstallState, String> {
    ensure_package_installed_guarded(install_dir, None)
}

fn install_material_library(package_root: &Path, install_dir: &Path) -> Result<(), String> {
    let relative = Path::new("Worldkiln/MaterialLibrary/4K_CC0");
    let source = package_root.join(relative);
    if source.is_dir() {
        copy_missing_tree(&source, &install_dir.join(relative), None)?;
    }
    Ok(())
}

#[cfg(test)]
mod material_package_tests {
    use super::*;
    #[test]
    #[ignore = "requires explicitly selected full 4K release archive"]
    fn first_install_from_full_material_release_archive() {
        let zip = PathBuf::from(
            std::env::var_os("WK_MATERIAL_RELEASE_ZIP").expect("set WK_MATERIAL_RELEASE_ZIP"),
        );
        let unique = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("wk-full-material-install-{unique}"));
        assert!(!root.exists());
        assert_eq!(
            install_package_from_zip(&zip, &root).unwrap(),
            PackageInstallState::Current
        );
        let library = root.join("Worldkiln/MaterialLibrary/4K_CC0");
        let catalog: serde_json::Value = serde_json::from_slice(
            &std::fs::read(library.join("Worldkiln/MATERIAL_SOURCES.json")).unwrap(),
        )
        .unwrap();
        let materials = catalog["materials"].as_array().unwrap();
        assert_eq!(materials.len(), 6);
        for material in materials {
            let files = material["files"].as_array().unwrap();
            assert_eq!(files.len(), 6);
            for file in files {
                let path = library.join(file["path"].as_str().unwrap());
                assert_eq!(
                    std::fs::metadata(path).unwrap().len(),
                    file["size_bytes"].as_u64().unwrap()
                );
            }
        }
        assert!(root.join(".assets/version.txt").is_file());
        assert!(root.join("UserData").is_dir());
        assert!(!root.join(".package_extract").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn bundled_material_library_installs_outside_userdata_and_preserves_existing_files() {
        let root = std::env::temp_dir().join(format!("wk-material-package-{}", std::process::id()));
        let package = root.join("package");
        let installed = root.join("installed");
        let relative = "Worldkiln/MaterialLibrary/4K_CC0/Worldkiln";
        std::fs::create_dir_all(package.join(relative)).unwrap();
        std::fs::create_dir_all(installed.join(relative)).unwrap();
        std::fs::write(package.join(relative).join("map.png"), b"bundled").unwrap();
        std::fs::write(package.join(relative).join("existing.png"), b"replacement").unwrap();
        std::fs::write(installed.join(relative).join("existing.png"), b"owner").unwrap();
        install_material_library(&package, &installed).unwrap();
        assert_eq!(
            std::fs::read(installed.join(relative).join("map.png")).unwrap(),
            b"bundled"
        );
        assert_eq!(
            std::fs::read(installed.join(relative).join("existing.png")).unwrap(),
            b"owner"
        );
        assert!(!installed.join("UserData").exists());
        std::fs::remove_dir_all(root).unwrap();
    }
}

pub(crate) fn ensure_package_installed_guarded(
    install_dir: &Path,
    backup: Option<&InstallBackup>,
) -> Result<PackageInstallState, String> {
    ensure_package_installed_guarded_with_progress(install_dir, backup, &|_| {})
}
pub(crate) fn ensure_package_installed_guarded_with_progress(
    install_dir: &Path,
    backup: Option<&InstallBackup>,
    progress: &dyn Fn(PackageProgress),
) -> Result<PackageInstallState, String> {
    let _ = backup;
    ensure_core_scaffolding(install_dir)?;
    if core_resources_present(install_dir) {
        progress(PackageProgress {
            phase: Phase::Complete,
            completed: 1,
            total: 1,
        });
        return Ok(PackageInstallState::Current);
    }
    let download = install_dir.join(format!(
        ".core-resources-{:032x}.zip",
        rand::random::<u128>()
    ));
    let result = (|| {
        download_file(&package_download_url(), &download, progress)?;
        install_core_resources_from_zip(&download, install_dir, progress)
    })();
    let cleanup = if download.exists() {
        std::fs::remove_file(&download)
    } else {
        Ok(())
    };
    let state = match (result, cleanup) {
        (Ok(state), Ok(())) => state,
        (Ok(_), Err(error)) => {
            return Err(format!(
                "Failed to clean core resource download {}: {error}",
                download.display()
            ));
        }
        (Err(error), Ok(())) => return Err(error),
        (Err(error), Err(cleanup)) => {
            return Err(format!(
                "{error}; failed to clean core resource download {}: {cleanup}",
                download.display()
            ));
        }
    };
    progress(PackageProgress {
        phase: Phase::Complete,
        completed: 0,
        total: 0,
    });
    Ok(state)
}

pub(crate) fn ensure_core_scaffolding(install_dir: &Path) -> Result<(), String> {
    for relative in [
        "UserData",
        ".assets",
        "AssetPacks",
        "Worldkiln/MaterialLibrary",
    ] {
        let path = install_dir.join(relative);
        for ancestor in path.ancestors() {
            if let Ok(meta) = std::fs::symlink_metadata(ancestor) {
                if meta.file_type().is_symlink() {
                    return Err(format!(
                        "Linked core installation path: {}",
                        ancestor.display()
                    ));
                }
                #[cfg(windows)]
                {
                    use std::os::windows::fs::MetadataExt;
                    if meta.file_attributes() & 0x400 != 0 {
                        return Err(format!(
                            "Reparse core installation path: {}",
                            ancestor.display()
                        ));
                    }
                }
            }
        }
        std::fs::create_dir_all(&path).map_err(|error| {
            format!(
                "Failed to create required installation directory {}: {error}",
                path.display()
            )
        })?;
    }
    ensure_license_installed(install_dir)?;
    let version_path = install_dir.join(".assets/version.txt");
    let existing = std::fs::read_to_string(&version_path).ok();
    if existing.as_deref().is_none_or(|version| {
        compare_versions(version, ENGINE_VERSION)
            .is_some_and(|ordering| ordering != Ordering::Greater)
    }) {
        crate::project::atomic_file::write_atomically(&version_path, ENGINE_VERSION.as_bytes())
            .map_err(|error| {
                format!(
                    "Failed to write engine version marker {}: {error}",
                    version_path.display()
                )
            })?;
    }
    let manifest = serde_json::json!({"schema":1,"engine_version":ENGINE_VERSION,"files":[CANONICAL_EXECUTABLE_NAME,"LICENSE",".assets/version.txt",CORE_RESOURCE_RECEIPT],"embedded_resources":["editor_fonts","editor_icons","engine_shaders"],"required_resource_roots":[".assets/engine",".assets/UI"],"optional_packs":[],"directories":["UserData",".assets","AssetPacks","Worldkiln/MaterialLibrary"]});
    crate::project::atomic_file::write_atomically(
        &install_dir.join("engine-core-manifest.json"),
        &serde_json::to_vec_pretty(&manifest).map_err(|error| {
            format!(
                "Failed to encode engine core manifest {}: {error}",
                install_dir.join("engine-core-manifest.json").display()
            )
        })?,
    )
    .map_err(|error| {
        format!(
            "Failed to write engine core manifest {}: {error}",
            install_dir.join("engine-core-manifest.json").display()
        )
    })?;
    Ok(())
}

pub(crate) fn install_assets_from_zip(zip_path: &Path, install_dir: &Path) -> Result<(), String> {
    let assets_dir = install_dir.join(".assets");
    let extraction_dir =
        install_dir.join(format!(".assets_extract-{:032x}", rand::random::<u128>()));
    std::fs::create_dir(&extraction_dir)
        .map_err(|err| format!("Failed to create .assets extraction directory: {err}"))?;

    let zip_file = std::fs::File::open(zip_path)
        .map_err(|err| format!("Failed to open {}: {err}", zip_path.display()))?;

    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|err| format!("Invalid .assets ZIP: {err}"))?;

    archive
        .extract(&extraction_dir)
        .map_err(|err| format!("Failed to extract .assets ZIP: {err}"))?;

    let nested_assets = extraction_dir.join(".assets");

    let packaged_assets = if nested_assets.is_dir() {
        nested_assets
    } else {
        extraction_dir.clone()
    };

    let packaged_version = packaged_assets.join("version.txt");

    copy_missing_tree(
        &packaged_assets,
        &assets_dir,
        Some(packaged_version.as_path()),
    )?;

    std::fs::write(assets_dir.join("version.txt"), ENGINE_VERSION)
        .map_err(|err| format!("Failed to update .assets/version.txt: {err}"))?;

    if extraction_dir.exists() {
        std::fs::remove_dir_all(&extraction_dir)
            .map_err(|err| format!("Failed to clean .assets extraction directory: {err}"))?;
    }

    Ok(())
}

pub(crate) fn ensure_license_installed(install_dir: &Path) -> Result<(), String> {
    let license_path = install_dir.join("LICENSE");
    std::fs::create_dir_all(install_dir).map_err(|err| {
        format!(
            "Failed to create Aeowun install directory {}: {err}",
            install_dir.display()
        )
    })?;
    if !license_path.exists() {
        std::fs::write(&license_path, LICENSE_CONTENT).map_err(|err| {
            format!(
                "Failed to install LICENSE {}: {err}",
                license_path.display()
            )
        })?;
    }
    let text_license_path = install_dir.join("LICENSE.txt");
    if !text_license_path.exists() {
        std::fs::copy(&license_path, &text_license_path).map_err(|err| {
            format!(
                "Failed to copy LICENSE {} to {}: {err}",
                license_path.display(),
                text_license_path.display()
            )
        })?;
    }

    Ok(())
}

#[cfg(target_os = "windows")]
pub(crate) fn create_desktop_shortcut(
    installed_exe: &Path,
    development_build: bool,
) -> Result<PathBuf, String> {
    let working_dir = installed_exe
        .parent()
        .ok_or_else(|| "Installed Worldkiln executable has no parent directory".to_string())?;

    let arguments = if development_build {
        format!("{} {}", INSTALL_CHECK_COMPLETE_ARG, DEVELOPMENT_BUILD_ARG)
    } else {
        INSTALL_CHECK_COMPLETE_ARG.to_string()
    };

    let script = r#"
$desktop = [Environment]::GetFolderPath('Desktop')

if ([string]::IsNullOrWhiteSpace($desktop)) {
    throw "Windows Desktop directory could not be resolved."
}

$shortcutPath = Join-Path $desktop 'Worldkiln.lnk'

# An existing shortcut may have been customized by the owner. Never replace it.
if (Test-Path -LiteralPath $shortcutPath) {
    Write-Output $shortcutPath
    exit 0
}

$shell = New-Object -ComObject WScript.Shell
$shortcut = $shell.CreateShortcut($shortcutPath)

$shortcut.TargetPath = $env:WORLDKILN_TARGET
$shortcut.WorkingDirectory = $env:WORLDKILN_WORKDIR
$shortcut.Arguments = $env:WORLDKILN_ARGS
$shortcut.IconLocation = "$($env:WORLDKILN_TARGET),0"

$shortcut.Save()

Write-Output $shortcutPath
"#;

    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("WORLDKILN_TARGET", installed_exe)
        .env("WORLDKILN_WORKDIR", working_dir)
        .env("WORLDKILN_ARGS", arguments)
        .output()
        .map_err(|err| format!("Failed to start powershell.exe to create the Worldkiln desktop shortcut for {}: {err}", installed_exe.display()))?;

    if !output.status.success() {
        let error = String::from_utf8_lossy(&output.stderr);

        return Err(format!(
            "Failed to create Worldkiln desktop shortcut: {}",
            error.trim()
        ));
    }

    let shortcut_path = String::from_utf8_lossy(&output.stdout).trim().to_string();

    if shortcut_path.is_empty() {
        return Err("Shortcut creation returned no shortcut path".to_string());
    }

    let shortcut_path = PathBuf::from(shortcut_path);

    if !shortcut_path.is_file() {
        return Err(format!(
            "Worldkiln shortcut was not created at {}",
            shortcut_path.display()
        ));
    }

    Ok(shortcut_path)
}

pub(crate) fn installer_launch_arguments(development_build: bool) -> Vec<&'static str> {
    if development_build {
        vec![INSTALL_CHECK_COMPLETE_ARG, DEVELOPMENT_BUILD_ARG]
    } else {
        vec![INSTALL_CHECK_COMPLETE_ARG]
    }
}

pub(crate) fn is_installer_self_cleanup_eligible(current_exe: &Path, install_dir: &Path) -> bool {
    let canonical_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);

    is_installer_executable(current_exe)
        && current_exe.is_file()
        && !paths_refer_to_same_file(current_exe, &canonical_exe)
}

#[cfg(target_os = "windows")]
pub(crate) fn schedule_installer_self_cleanup(installer_exe: &Path) -> Result<(), String> {
    if !is_installer_executable(installer_exe) {
        return Err("Self-cleanup is only allowed for Installer.exe".to_string());
    }

    let pid = std::process::id();

    let script = r#"
$pidToWait = $env:WORLDKILN_INSTALLER_PID
$exeToDelete = $env:WORLDKILN_INSTALLER_EXE

if ([string]::IsNullOrWhiteSpace($pidToWait) -or [string]::IsNullOrWhiteSpace($exeToDelete)) {
    exit 1
}

if (-not [string]::Equals([IO.Path]::GetFileName($exeToDelete), 'Installer.exe', [StringComparison]::OrdinalIgnoreCase)) {
    exit 1
}

try {
    Wait-Process -Id $pidToWait -ErrorAction SilentlyContinue
} catch {
}

Start-Sleep -Milliseconds 250

if (Test-Path -LiteralPath $exeToDelete -PathType Leaf) {
    Remove-Item -LiteralPath $exeToDelete -Force -ErrorAction SilentlyContinue
}
"#;

    Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-WindowStyle",
            "Hidden",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            script,
        ])
        .env("WORLDKILN_INSTALLER_PID", pid.to_string())
        .env("WORLDKILN_INSTALLER_EXE", installer_exe)
        .spawn()
        .map_err(|err| {
            format!(
                "Failed to start powershell.exe to schedule installer self-cleanup for {}: {err}",
                installer_exe.display()
            )
        })?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn schedule_installer_self_cleanup(_installer_exe: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod version_tests {
    #[test]
    fn installer_error_identifies_missing_executable_source() {
        let root = std::env::temp_dir().join(format!(
            "wk-installer-missing-source-{:032x}",
            rand::random::<u128>()
        ));
        let source = root.join("missing/Installer.exe");
        let installed = root.join("installed");
        let error = install_engine_executable(&source, &installed).unwrap_err();
        assert!(error.contains("Failed to stage executable"), "{error}");
        assert!(error.contains(&source.display().to_string()), "{error}");
        assert!(error.contains(&installed.display().to_string()), "{error}");
        assert!(!installed.join(CANONICAL_EXECUTABLE_NAME).exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn installer_error_identifies_required_directory_collision() {
        let root = std::env::temp_dir().join(format!(
            "wk-installer-required-dir-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir(&root).unwrap();
        std::fs::write(root.join("Worldkiln"), b"owner file").unwrap();
        let error = ensure_core_scaffolding(&root).unwrap_err();
        assert!(
            error.contains("Failed to create required installation directory"),
            "{error}"
        );
        assert!(
            error.contains(&root.join("Worldkiln/MaterialLibrary").display().to_string()),
            "{error}"
        );
        assert_eq!(
            std::fs::read(root.join("Worldkiln")).unwrap(),
            b"owner file"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    use super::*;

    fn core_resource_package(root: &Path) -> PathBuf {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let path = root.join("core-resources.zip");
        let mut archive = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        let options = SimpleFileOptions::default();
        archive.start_file(".assets/version.txt", options).unwrap();
        archive.write_all(ENGINE_VERSION.as_bytes()).unwrap();
        archive.add_directory("UserData/", options).unwrap();

        let mut sounds: std::collections::BTreeSet<String> = crate::engine::audio::EngineSound::ALL
            .iter()
            .map(|sound| sound.file_name().to_owned())
            .collect();
        let mut index = 0;
        while sounds.len() < MIN_ENGINE_SOUND_FILES {
            sounds.insert(format!("fixture-{index}.wav"));
            index += 1;
        }
        for sound in sounds {
            archive
                .start_file(format!(".assets/engine/{sound}"), options)
                .unwrap();
            archive
                .write_all(format!("RIFF fixture {sound}").as_bytes())
                .unwrap();
        }

        for name in ["Button", "Panel", "Text"] {
            archive
                .start_file(format!(".assets/UI/{name}.json"), options)
                .unwrap();
            archive
                .write_all(format!(r#"{{"element_type":"{name}"}}"#).as_bytes())
                .unwrap();
        }
        archive
            .start_file(".assets/textures/optional.png", options)
            .unwrap();
        archive.write_all(b"optional").unwrap();
        archive.finish().unwrap();
        path
    }

    #[test]
    fn required_core_resources_install_and_repair_without_optional_assets() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-resource-install-{:032x}",
            rand::random::<u128>()
        ));
        let install = root.join("installed");
        std::fs::create_dir_all(install.join("UserData/Project")).unwrap();
        std::fs::write(install.join("UserData/Project/world.dat"), b"authored").unwrap();
        let package = core_resource_package(&root);

        install_core_resources_from_zip(&package, &install, &|_| {}).unwrap();
        assert!(install.join(".assets/engine/new_click.wav").is_file());
        for name in ["Button", "Panel", "Text"] {
            assert!(install.join(format!(".assets/UI/{name}.json")).is_file());
        }
        assert!(!install.join(".assets/textures/optional.png").exists());
        assert_eq!(
            std::fs::read(install.join("UserData/Project/world.dat")).unwrap(),
            b"authored"
        );
        assert!(!package_install_required(&install));

        std::fs::remove_file(install.join(".assets/UI/Panel.json")).unwrap();
        assert!(package_install_required(&install));
        install_core_resources_from_zip(&package, &install, &|_| {}).unwrap();
        assert!(install.join(".assets/UI/Panel.json").is_file());
        assert!(!package_install_required(&install));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn core_resource_upgrade_never_replaces_modified_user_asset() {
        let root = std::env::temp_dir().join(format!(
            "wk-core-owner-preserve-{:032x}",
            rand::random::<u128>()
        ));
        let install = root.join("installed");
        std::fs::create_dir_all(install.join("UserData/Project")).unwrap();
        let project = install.join("UserData/Project/world.dat");
        std::fs::write(&project, b"my saved world").unwrap();
        let package = core_resource_package(&root);

        install_core_resources_from_zip(&package, &install, &|_| {}).unwrap();
        let customized = install.join(".assets/UI/Panel.json");
        std::fs::write(&customized, b"my custom UI definition").unwrap();
        let receipt = std::fs::read(install.join(CORE_RESOURCE_RECEIPT)).unwrap();
        let version = std::fs::read(install.join(".assets/version.txt")).unwrap();
        let missing = install.join(".assets/engine/new_click.wav");
        std::fs::remove_file(&missing).unwrap();

        let error = install_core_resources_from_zip(&package, &install, &|_| {})
            .expect_err("the customized core path must stop the entire repair");
        assert!(error.contains("existing user asset preserved"), "{error}");
        assert_eq!(
            std::fs::read(&customized).unwrap(),
            b"my custom UI definition"
        );
        assert_eq!(std::fs::read(&project).unwrap(), b"my saved world");
        assert_eq!(
            std::fs::read(install.join(CORE_RESOURCE_RECEIPT)).unwrap(),
            receipt
        );
        assert_eq!(
            std::fs::read(install.join(".assets/version.txt")).unwrap(),
            version
        );
        assert!(
            !missing.exists(),
            "no partial repair before collision preflight"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn core_resource_first_install_refuses_untracked_collisions() {
        let root =
            std::env::temp_dir().join(format!("wk-core-untracked-{:032x}", rand::random::<u128>()));
        let install = root.join("installed");
        std::fs::create_dir_all(install.join(".assets/UI")).unwrap();
        std::fs::create_dir_all(install.join("UserData/Project")).unwrap();
        let customized = install.join(".assets/UI/Button.json");
        let project = install.join("UserData/Project/world.dat");
        std::fs::write(&customized, b"creator's independent button").unwrap();
        std::fs::write(&project, b"keep").unwrap();
        let package = core_resource_package(&root);

        let error = install_core_resources_from_zip(&package, &install, &|_| {})
            .expect_err("unknown file ownership must not be presumed");
        assert!(error.contains("existing user asset preserved"), "{error}");
        assert_eq!(
            std::fs::read(&customized).unwrap(),
            b"creator's independent button"
        );
        assert_eq!(std::fs::read(&project).unwrap(), b"keep");
        assert!(!install.join(CORE_RESOURCE_RECEIPT).exists());
        assert!(!install.join(".assets/version.txt").exists());
        assert!(!install.join(".assets/engine/new_click.wav").exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "requires an explicitly selected published UserData.zip"]
    fn published_userdata_installs_complete_core_resources_selectively() {
        let package = PathBuf::from(
            std::env::var_os("WK_CORE_RESOURCE_PACKAGE")
                .expect("set WK_CORE_RESOURCE_PACKAGE to the verified published UserData.zip"),
        );
        let root = std::env::temp_dir().join(format!(
            "wk-published-core-resources-{:032x}",
            rand::random::<u128>()
        ));
        std::fs::create_dir_all(root.join("UserData/Project")).unwrap();
        std::fs::write(root.join("UserData/Project/world.dat"), b"authored").unwrap();
        install_core_resources_from_zip(&package, &root, &|_| {}).unwrap();
        let receipt: CoreResourceReceipt =
            serde_json::from_slice(&std::fs::read(root.join(CORE_RESOURCE_RECEIPT)).unwrap())
                .unwrap();
        assert!(core_resource_inventory_complete(&receipt.files));
        assert_eq!(
            receipt
                .files
                .iter()
                .filter(|file| file.path.starts_with(".assets/skybox/"))
                .count(),
            0
        );
        assert!(root.join(".assets/engine/new_click.wav").is_file());
        assert!(root.join(".assets/UI/Panel.json").is_file());
        assert!(!root.join(".assets/textures").exists());
        assert_eq!(
            std::fs::read(root.join("UserData/Project/world.dat")).unwrap(),
            b"authored"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_download_retries_transient_errors_then_succeeds() {
        let mut attempts = 0;
        let mut pauses = Vec::new();

        retry_package_download(
            || {
                attempts += 1;
                if attempts < 3 {
                    Err(DownloadFailure {
                        kind: DownloadFailureKind::Connection,
                        detail: "temporary network failure".into(),
                    })
                } else {
                    Ok(())
                }
            },
            |duration| pauses.push(duration),
        )
        .unwrap();

        assert_eq!(attempts, 3);
        assert_eq!(
            pauses,
            [
                PACKAGE_DOWNLOAD_RETRY_DELAY,
                PACKAGE_DOWNLOAD_RETRY_DELAY * 2,
            ]
        );
    }

    #[test]
    fn package_download_does_not_retry_permanent_errors() {
        let mut attempts = 0;
        let mut pauses = Vec::new();

        let error = retry_package_download(
            || {
                attempts += 1;
                Err(DownloadFailure {
                    kind: DownloadFailureKind::Other,
                    detail: "HTTP 404".into(),
                })
            },
            |duration| pauses.push(duration),
        )
        .unwrap_err();

        assert_eq!(attempts, 1);
        assert!(pauses.is_empty());
        assert!(download_failure_message(error).contains("Aeowun Support"));
    }

    #[test]
    fn package_download_stops_after_three_transient_failures() {
        let mut attempts = 0;
        let mut pauses = Vec::new();

        let error = retry_package_download(
            || {
                attempts += 1;
                Err(DownloadFailure {
                    kind: DownloadFailureKind::TemporaryServer,
                    detail: "HTTP 503".into(),
                })
            },
            |duration| pauses.push(duration),
        )
        .unwrap_err();

        assert_eq!(attempts, PACKAGE_DOWNLOAD_ATTEMPTS);
        assert_eq!(pauses.len(), PACKAGE_DOWNLOAD_ATTEMPTS - 1);
        assert!(download_failure_message(error).contains("temporarily unavailable"));
    }

    #[test]
    fn package_download_retries_rate_limits_and_server_errors_only() {
        assert_eq!(
            classify_http_status(reqwest::StatusCode::TOO_MANY_REQUESTS),
            DownloadFailureKind::TemporaryServer
        );
        assert_eq!(
            classify_http_status(reqwest::StatusCode::SERVICE_UNAVAILABLE),
            DownloadFailureKind::TemporaryServer
        );
        assert_eq!(
            classify_http_status(reqwest::StatusCode::NOT_FOUND),
            DownloadFailureKind::Other
        );
    }

    #[test]
    fn package_progress_native_download_and_install_preserve_authored_files() {
        use std::io::Write;
        use zip::write::SimpleFileOptions;

        let root = bootstrap_fixture("native_curl_fallback");
        let install_dir = root.join("isolated-localappdata/Aeowun");
        let zip_path = install_dir.join("UserData.zip");
        std::fs::create_dir_all(install_dir.join(".assets")).unwrap();
        std::fs::create_dir_all(install_dir.join("UserData/ExistingProject")).unwrap();
        std::fs::write(install_dir.join(".assets/version.txt"), "0.8.4").unwrap();
        std::fs::write(
            install_dir.join("UserData/ExistingProject/world.dat"),
            b"preserve authored world",
        )
        .unwrap();

        let source_zip = root.join("package-source.zip");
        let mut zip_writer = zip::ZipWriter::new(std::fs::File::create(&source_zip).unwrap());
        zip_writer
            .start_file(".assets/version.txt", SimpleFileOptions::default())
            .unwrap();
        zip_writer.write_all(ENGINE_VERSION.as_bytes()).unwrap();
        zip_writer
            .start_file(".assets/core.txt", SimpleFileOptions::default())
            .unwrap();
        zip_writer.write_all(b"core asset").unwrap();
        zip_writer
            .start_file(
                "UserData/NewProject/project.json",
                SimpleFileOptions::default(),
            )
            .unwrap();
        zip_writer.write_all(b"new project").unwrap();
        zip_writer.finish().unwrap();

        let mut rust_attempts = 0;
        let samples = std::cell::RefCell::new(Vec::new());
        let progress = |sample| samples.borrow_mut().push(sample);
        let url = reqwest::Url::from_file_path(&source_zip)
            .expect("temp package path should form a file URL")
            .to_string();
        download_with_native_fallback(
            &zip_path,
            || {
                rust_attempts += 1;
                Err(DownloadFailure {
                    kind: DownloadFailureKind::Connection,
                    detail: "simulated Rust HTTP connection failure".into(),
                })
            },
            || download_with_curl_with_progress(&url, &zip_path, &progress),
            |_| {},
        )
        .expect("curl fallback should download the package from the local fixture server");
        assert_eq!(rust_attempts, PACKAGE_DOWNLOAD_ATTEMPTS);

        assert_eq!(
            samples.borrow().last().unwrap().completed,
            std::fs::metadata(&source_zip).unwrap().len()
        );
        install_package_from_zip_with_progress(&zip_path, &install_dir, None, &progress)
            .expect("verified package should install in the isolated profile");
        let phases: Vec<_> = samples.borrow().iter().map(|sample| sample.phase).collect();
        assert!(phases.contains(&Phase::Download));
        assert_eq!(
            &phases[phases.len() - 3..],
            &[Phase::Verify, Phase::Extract, Phase::Install]
        );
        assert!(
            samples
                .borrow()
                .iter()
                .all(|sample| sample.fraction() < 1.0)
        );
        assert_eq!(
            std::fs::read(install_dir.join("UserData/ExistingProject/world.dat")).unwrap(),
            b"preserve authored world"
        );
        assert_eq!(
            std::fs::read(install_dir.join("UserData/NewProject/project.json")).unwrap(),
            b"new project"
        );
        assert_eq!(
            std::fs::read(install_dir.join(".assets/core.txt")).unwrap(),
            b"core asset"
        );
        assert_eq!(
            std::fs::read_to_string(install_dir.join(".assets/version.txt")).unwrap(),
            ENGINE_VERSION
        );

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    #[ignore = "live GitHub download check; run explicitly before installed-profile verification"]
    fn native_curl_fallback_downloads_github_package_in_isolated_profile() {
        let root = bootstrap_fixture("live_github_curl_fallback");
        let install_dir = root.join("isolated-localappdata/Aeowun");
        let zip_path = install_dir.join("UserData.zip");
        std::fs::create_dir_all(install_dir.join(".assets")).unwrap();
        std::fs::create_dir_all(install_dir.join("UserData/ExistingProject")).unwrap();
        std::fs::write(install_dir.join(".assets/version.txt"), "0.8.4").unwrap();
        std::fs::write(
            install_dir.join("UserData/ExistingProject/world.dat"),
            b"preserve authored world",
        )
        .unwrap();

        download_with_native_fallback(
            &zip_path,
            || {
                Err(DownloadFailure {
                    kind: DownloadFailureKind::Connection,
                    detail: "simulated Rust HTTP connection failure".into(),
                })
            },
            || download_with_curl(&package_download_url(), &zip_path),
            |_| {},
        )
        .expect("curl should download and verify the published GitHub package");
        install_package_from_zip(&zip_path, &install_dir)
            .expect("published package should install in the isolated profile");

        assert_eq!(
            std::fs::read(install_dir.join("UserData/ExistingProject/world.dat")).unwrap(),
            b"preserve authored world"
        );
        assert_eq!(
            std::fs::read_to_string(install_dir.join(".assets/version.txt")).unwrap(),
            ENGINE_VERSION
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_zip_is_verified_before_extraction_or_profile_changes() {
        let root = bootstrap_fixture("invalid_package_zip");
        let install_dir = root.join("isolated-localappdata/Aeowun");
        std::fs::create_dir_all(install_dir.join("UserData")).unwrap();
        std::fs::write(install_dir.join("UserData/keep.txt"), b"keep").unwrap();
        std::fs::create_dir_all(install_dir.join(".package_extract")).unwrap();
        std::fs::write(
            install_dir.join(".package_extract/keep.txt"),
            b"extract keep",
        )
        .unwrap();
        let invalid_zip = root.join("invalid.zip");
        std::fs::write(&invalid_zip, b"not a zip archive").unwrap();

        assert!(install_package_from_zip(&invalid_zip, &install_dir).is_err());
        assert_eq!(
            std::fs::read(install_dir.join("UserData/keep.txt")).unwrap(),
            b"keep"
        );
        assert_eq!(
            std::fs::read(install_dir.join(".package_extract/keep.txt")).unwrap(),
            b"extract keep"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_update_required_for_missing_or_older_version_marker() {
        let root = bootstrap_fixture("installed_package_version_gate");
        let assets_dir = root.join(".assets");
        std::fs::create_dir_all(&assets_dir).unwrap();
        let version_path = assets_dir.join("version.txt");

        std::fs::write(&version_path, "0.0.1").unwrap();
        assert!(package_install_required(&root));

        std::fs::write(&version_path, ENGINE_VERSION).unwrap();
        assert!(package_install_required(&root));

        std::fs::write(&version_path, "999.0.0").unwrap();
        assert!(package_install_required(&root));

        std::fs::write(&version_path, "invalid").unwrap();
        assert!(package_install_required(&root));

        std::fs::remove_file(&version_path).unwrap();
        assert!(package_install_required(&root));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn version_marker_alone_never_skips_required_resource_repair() {
        let root = bootstrap_fixture("package_install_version_gate");
        std::fs::create_dir_all(root.join(".assets")).unwrap();

        for version in [ENGINE_VERSION, "999.0.0", "invalid"] {
            std::fs::write(root.join(".assets/version.txt"), version).unwrap();
            assert!(package_install_required(&root));
        }

        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn license_text_copy_preserves_existing_license_files() {
        let root = bootstrap_fixture("license");
        std::fs::write(root.join("LICENSE"), b"existing license").unwrap();
        ensure_license_installed(&root).unwrap();
        assert_eq!(
            std::fs::read(root.join("LICENSE.txt")).unwrap(),
            b"existing license"
        );
        std::fs::write(root.join("LICENSE.txt"), b"existing text license").unwrap();
        ensure_license_installed(&root).unwrap();
        assert_eq!(
            std::fs::read(root.join("LICENSE")).unwrap(),
            b"existing license"
        );
        assert_eq!(
            std::fs::read(root.join("LICENSE.txt")).unwrap(),
            b"existing text license"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    fn bootstrap_fixture(label: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "Worldkiln_Bootstrap_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&root).unwrap();
        root
    }

    #[test]
    fn external_installer_requires_bootstrap_without_appdata() {
        let root = bootstrap_fixture("first_run");
        let current_exe = root.join("Installer.exe");
        let install_dir = root.join("AppData/Local/Aeowun");
        std::fs::write(&current_exe, b"installer").unwrap();
        let role = executable_role(&current_exe, &install_dir);
        assert_eq!(role, ExecutableRole::Installer);
        assert!(installer_required(&current_exe, &install_dir));
        assert!(installer_bootstrap_required(role, true, false));
        assert!(
            !install_dir.exists(),
            "Decision must not create or modify the profile"
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_installer_cannot_be_bypassed_by_gamedata_or_shortcut_flags() {
        let root = bootstrap_fixture("bypass");
        let current_exe = root.join("iNsTaLlEr.ExE");
        let install_dir = root.join("AppData/Local/Aeowun");
        std::fs::write(&current_exe, b"installer").unwrap();
        std::fs::create_dir(root.join("GameData")).unwrap();
        let role = executable_role(&current_exe, &install_dir);
        assert!(installer_bootstrap_required(
            role,
            installer_required(&current_exe, &install_dir),
            false
        ));
        for flag in [
            INSTALL_CHECK_COMPLETE_ARG,
            LEGACY_INSTALL_CHECK_COMPLETE_ARG,
        ] {
            let requested = is_install_check_complete_arg(OsStr::new(flag));
            assert!(requested);
            assert!(installer_bootstrap_required(role, true, requested));
            assert!(
                installer_bootstrap_required(role, false, requested),
                "Even valid package state cannot change the Installer role"
            );
        }
        assert!(installer_bootstrap_required(
            ExecutableRole::ExternalEditor,
            false,
            true
        ));
        assert!(!install_dir.exists());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn canonical_editor_bootstraps_for_license_or_older_package_only() {
        let root = bootstrap_fixture("canonical");
        let install_dir = root.join("Aeowun");
        let current_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);
        let package = core_resource_package(&root);
        for missing in [None, Some(".assets"), Some("UserData"), Some("LICENSE")] {
            std::fs::create_dir_all(install_dir.join(".assets")).unwrap();
            std::fs::create_dir_all(install_dir.join("UserData")).unwrap();
            install_core_resources_from_zip(&package, &install_dir, &|_| {}).unwrap();
            std::fs::write(install_dir.join("LICENSE"), b"license").unwrap();
            std::fs::write(&current_exe, b"installed").unwrap();
            if let Some(component) = missing {
                let path = install_dir.join(component);
                if path.is_dir() {
                    std::fs::remove_dir_all(path).unwrap();
                } else {
                    std::fs::remove_file(path).unwrap();
                }
            }
            let role = executable_role(&current_exe, &install_dir);
            assert_eq!(role, ExecutableRole::InstalledEditor);
            let required = installer_required(&current_exe, &install_dir);
            let should_bootstrap = matches!(missing, Some("LICENSE" | ".assets"));
            assert_eq!(required, should_bootstrap);
            for requested in [false, true] {
                assert_eq!(
                    installer_bootstrap_required(role, required, requested),
                    should_bootstrap,
                    "missing={missing:?}, flag={requested}"
                );
            }
        }
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn github_package_url_tracks_the_engine_version_without_fallback() {
        let url = package_download_url();
        assert_eq!(
            url,
            format!(
                "https://github.com/Aeowun/Worldkiln/releases/download/win11-v{ENGINE_VERSION}/UserData.zip"
            )
        );
        assert!(!url.to_ascii_lowercase().contains("dropbox"));
    }

    #[test]
    fn numeric_versions_compare_by_component() {
        assert_eq!(compare_versions("0.8.9", "0.8.10"), Some(Ordering::Less));
        assert_eq!(compare_versions("v0.8.0", "0.8"), Some(Ordering::Equal));
        assert_eq!(compare_versions("0.9.0", "0.8.99"), Some(Ordering::Greater));
        assert_eq!(compare_versions("not-a-version", "0.8.0"), None);
    }

    #[test]
    fn older_downloaded_package_marks_a_development_build() {
        assert_eq!(
            classify_package_version("0.7.9"),
            Ok(PackageInstallState::DevelopmentBuild)
        );
    }

    #[test]
    fn current_package_is_current_and_other_mismatches_still_fail() {
        assert_eq!(
            classify_package_version(ENGINE_VERSION),
            Ok(PackageInstallState::Current)
        );
        assert!(classify_package_version("99.0.0").is_err());
        assert!(classify_package_version("old-version").is_err());
    }

    #[test]
    fn bootstrap_flags_accept_worldkiln_and_legacy_aeoengine_spellings() {
        assert!(is_install_check_complete_arg(OsStr::new(
            "--worldkiln-install-check-complete"
        )));
        assert!(is_install_check_complete_arg(OsStr::new(
            "--aeo-install-check-complete"
        )));
        assert!(is_development_build_arg(OsStr::new(
            "--worldkiln-development-build"
        )));
        assert!(is_development_build_arg(OsStr::new(
            "--aeo-development-build"
        )));
        assert!(!is_install_check_complete_arg(OsStr::new(
            "--development-build"
        )));
    }

    #[test]
    fn external_installer_path_is_eligible_for_self_cleanup() {
        let temp_dir = std::env::temp_dir().join("Worldkiln_Test_Cleanup_Eligible");
        let download_dir = temp_dir.join("Downloads");
        let install_dir = temp_dir.join("Aeowun");
        let external_installer = download_dir.join("Installer.exe");

        std::fs::create_dir_all(&download_dir).unwrap();
        std::fs::write(&external_installer, b"installer").unwrap();

        assert!(
            is_installer_self_cleanup_eligible(&external_installer, &install_dir),
            "external installer executable must be eligible for self-cleanup"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn installed_canonical_worldkiln_exe_is_not_eligible_for_self_cleanup() {
        let temp_dir = std::env::temp_dir().join("Worldkiln_Test_Cleanup_Canonical");
        let install_dir = temp_dir.join("Aeowun");
        let canonical_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);

        std::fs::create_dir_all(&install_dir).unwrap();
        std::fs::write(&canonical_exe, b"canonical").unwrap();

        assert!(
            !is_installer_self_cleanup_eligible(&canonical_exe, &install_dir),
            "installed canonical Worldkiln.exe must NEVER be eligible for self-cleanup"
        );

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn alternate_spelling_of_installed_exe_skips_copy_and_installer() {
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_Test_Canonical_Alias_{}",
            std::process::id()
        ));
        let install_dir = temp_dir.join("Aeowun");
        let canonical_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);
        let alternate_spelling = install_dir.join(".").join(CANONICAL_EXECUTABLE_NAME);

        std::fs::create_dir_all(install_dir.join("UserData")).unwrap();
        std::fs::create_dir_all(install_dir.join(".assets")).unwrap();
        let package = core_resource_package(&temp_dir);
        install_core_resources_from_zip(&package, &install_dir, &|_| {}).unwrap();
        std::fs::write(install_dir.join("LICENSE"), b"license").unwrap();
        std::fs::write(&canonical_exe, b"installed engine").unwrap();

        assert!(!installer_required(&alternate_spelling, &install_dir));
        assert!(!is_installer_self_cleanup_eligible(
            &alternate_spelling,
            &install_dir
        ));
        assert_eq!(
            install_engine_executable(&alternate_spelling, &install_dir).unwrap(),
            canonical_exe
        );
        assert_eq!(std::fs::read(&canonical_exe).unwrap(), b"installed engine");

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn external_worldkiln_exe_is_not_self_cleanup_target() {
        let temp_dir = std::env::temp_dir().join(format!(
            "Worldkiln_Test_External_Exe_{}",
            std::process::id()
        ));
        let download_dir = temp_dir.join("Downloads");
        let install_dir = temp_dir.join("Aeowun");
        let external_worldkiln = download_dir.join(CANONICAL_EXECUTABLE_NAME);

        std::fs::create_dir_all(&download_dir).unwrap();
        std::fs::write(&external_worldkiln, b"downloaded engine").unwrap();

        assert!(!is_installer_self_cleanup_eligible(
            &external_worldkiln,
            &install_dir
        ));
        #[cfg(target_os = "windows")]
        assert!(schedule_installer_self_cleanup(&external_worldkiln).is_err());

        let _ = std::fs::remove_dir_all(&temp_dir);
    }

    #[test]
    fn launch_arguments_for_development_and_release_builds() {
        let dev_args = installer_launch_arguments(true);
        assert!(dev_args.contains(&"--worldkiln-install-check-complete"));
        assert!(dev_args.contains(&"--worldkiln-development-build"));

        let release_args = installer_launch_arguments(false);
        assert!(release_args.contains(&"--worldkiln-install-check-complete"));
        assert!(!release_args.contains(&"--worldkiln-development-build"));
    }

    #[test]
    #[cfg(target_os = "windows")]
    fn desktop_shortcut_creates_valid_lnk() {
        let temp_exe = std::env::temp_dir()
            .join("Worldkiln_Test_Shortcut")
            .join("Worldkiln.exe");
        std::fs::create_dir_all(temp_exe.parent().unwrap()).unwrap();
        std::fs::write(&temp_exe, b"stub").unwrap();

        let shortcut = create_desktop_shortcut(&temp_exe, true);
        assert!(
            shortcut.is_ok(),
            "Shortcut creation should succeed on Windows: {:?}",
            shortcut
        );
        let path = shortcut.unwrap();
        assert!(
            path.is_file(),
            "Shortcut path must exist at {}",
            path.display()
        );

        let _ = std::fs::remove_dir_all(temp_exe.parent().unwrap());
    }
}
