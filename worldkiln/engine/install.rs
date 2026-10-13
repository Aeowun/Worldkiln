use std::cmp::Ordering;
use std::ffi::OsStr;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

const AEOWUN_INSTALL_DIR_NAME: &str = "Aeowun";
const ENGINE_VERSION: &str = env!("CARGO_PKG_VERSION");
const REQUIRED_INSTALL_ITEMS: [&str; 3] = [".assets", "UserData", "LICENSE"];

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:18 -05:00
const PACKAGE_DOWNLOAD_ATTEMPTS: usize = 3;
const PACKAGE_DOWNLOAD_RETRY_DELAY: Duration = Duration::from_millis(700);
pub(crate) const SUPPORT_URL: &str = "https://aeowun.com/support/";

// Bootstrap flags are a phased-rename compatibility contract. New Worldkiln
// launchers emit the canonical spellings, while already-distributed AeoEngine
// launchers and local tooling may continue sending the legacy spellings.
// Do not remove the legacy aliases until a dedicated compatibility work order
// explicitly ends support for those launchers.
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

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
pub(crate) fn package_install_required(install_dir: &Path) -> bool {
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

    // Windows file paths are case-insensitive. Canonicalization also removes
    // alternate spellings such as `Aeowun\\.\\Worldkiln.exe` before comparison.
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
    // Installer identity is authoritative even when GameData or shortcut flags coexist.
    if is_installer_executable(current_exe) {
        ExecutableRole::Installer
    } else if paths_refer_to_same_file(current_exe, &install_dir.join(CANONICAL_EXECUTABLE_NAME)) {
        ExecutableRole::InstalledEditor
    } else {
        ExecutableRole::ExternalEditor
    }
}

/// Pure bootstrap policy. A flag is never proof of executable identity or package validity.
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

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
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
    std::fs::create_dir_all(install_dir)
        .map_err(|err| format!("Failed to create Aeowun install directory: {err}"))?;

    let canonical_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);
    let legacy_exe = install_dir.join(LEGACY_EXECUTABLE_NAME);

    // A running installed exe may have a different textual path (for example,
    // a Windows extended path). Never attempt to copy a file onto itself.
    if !paths_refer_to_same_file(source_exe, &canonical_exe) {
        std::fs::copy(source_exe, &canonical_exe).map_err(|err| {
            format!("Failed to copy {CANONICAL_EXECUTABLE_NAME} into Aeowun: {err}")
        })?;
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

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
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

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
fn download_with_curl(url: &str, destination: &Path) -> Result<(), String> {
    let curl_executable = if cfg!(target_os = "windows") {
        "curl.exe"
    } else {
        "curl"
    };
    let output = Command::new(curl_executable)
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
        .output()
        .map_err(|error| format!("Failed to start {curl_executable}: {error}"))?;

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

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
fn verify_package_zip(zip_path: &Path) -> Result<PackageInstallState, String> {
    let zip_file = std::fs::File::open(zip_path)
        .map_err(|error| format!("Failed to open downloaded UserData ZIP: {error}"))?;
    let mut archive = zip::ZipArchive::new(zip_file)
        .map_err(|error| format!("Downloaded UserData file is not a valid ZIP: {error}"))?;

    let mut marker_index = None;
    let mut package_prefix = None;
    let mut package_version = None;
    let mut entry_names = Vec::with_capacity(archive.len());

    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|error| format!("Failed to inspect downloaded UserData ZIP: {error}"))?;
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
            entry
                .read_to_string(&mut version)
                .map_err(|error| format!("Failed to read package version marker: {error}"))?;
            package_prefix = Some(prefix);
            package_version = Some(version.trim().to_string());
        } else {
            std::io::copy(&mut entry, &mut std::io::sink()).map_err(|error| {
                format!("Downloaded UserData ZIP failed integrity checking: {error}")
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

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
fn download_file(url: &str, destination: &Path) -> Result<(), String> {
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

            let mut output = std::fs::File::create(destination).map_err(|err| DownloadFailure {
                kind: DownloadFailureKind::Other,
                detail: format!("Failed to create {}: {err}", destination.display()),
            })?;

            std::io::copy(&mut response, &mut output).map_err(|err| DownloadFailure {
                kind: DownloadFailureKind::Other,
                detail: format!("Failed to write {}: {err}", destination.display()),
            })?;

            Ok(())
        },
        || download_with_curl(url, destination),
        std::thread::sleep,
    )
}

pub(crate) fn install_userdata_from_zip(zip_path: &Path, install_dir: &Path) -> Result<(), String> {
    let userdata_dir = install_dir.join("UserData");

    if userdata_dir.exists() {
        return Ok(());
    }

    let extraction_dir = install_dir.join(".UserData_extract");

    if extraction_dir.exists() {
        std::fs::remove_dir_all(&extraction_dir).map_err(|err| {
            format!("Failed to clear temporary UserData extraction directory: {err}")
        })?;
    }

    std::fs::create_dir_all(&extraction_dir)
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

fn copy_missing_tree(
    source: &Path,
    destination: &Path,
    skip_source: Option<&Path>,
) -> Result<(), String> {
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

        if source_path.is_dir() {
            copy_missing_tree(&source_path, &destination_path, skip_source)?;
            continue;
        }

        if destination_path.exists() {
            continue;
        }

        std::fs::copy(&source_path, &destination_path)
            .map_err(|err| format!("Failed to install {}: {err}", destination_path.display()))?;
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
    // Package/executable version contract:
    // - package == executable: normal current installation
    // - package < executable: executable is ahead of the published package,
    //   so install it as a Development Build instead of rejecting it
    // - package > executable or unparsable: preserve the normal mismatch error
    // This comparison is about the downloaded package, not the already-installed
    // .assets marker on disk. Do not invert the operands.
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
    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
    let _verified_state = verify_package_zip(zip_path)?;
    let extraction_dir = install_dir.join(".package_extract");

    if extraction_dir.exists() {
        std::fs::remove_dir_all(&extraction_dir)
            .map_err(|err| format!("Failed to clear package extraction directory: {err}"))?;
    }

    std::fs::create_dir_all(&extraction_dir)
        .map_err(|err| format!("Failed to create package extraction directory: {err}"))?;

    let zip_file = std::fs::File::open(zip_path)
        .map_err(|err| format!("Failed to open {}: {err}", zip_path.display()))?;

    let mut archive =
        zip::ZipArchive::new(zip_file).map_err(|err| format!("Invalid package ZIP: {err}"))?;

    archive
        .extract(&extraction_dir)
        .map_err(|err| format!("Failed to extract package ZIP: {err}"))?;

    let package_root = resolve_package_root(&extraction_dir)?;

    let (package_version, install_state) = validate_package_version(&package_root)?;

    let packaged_assets = package_root.join(".assets");
    let packaged_userdata = package_root.join("UserData");
    let packaged_license = package_root.join("LICENSE");

    let installed_assets = install_dir.join(".assets");
    let installed_userdata = install_dir.join("UserData");
    let installed_license = install_dir.join("LICENSE");

    let packaged_version = packaged_assets.join("version.txt");

    //
    // .assets
    //
    // Install missing files only.
    // version.txt is skipped and written LAST.
    //
    if packaged_assets.is_dir() {
        copy_missing_tree(
            &packaged_assets,
            &installed_assets,
            Some(packaged_version.as_path()),
        )?;
    }

    //
    // UserData
    //
    // Install missing files only.
    // Never overwrite existing projects/settings/builds/etc.
    //
    if packaged_userdata.is_dir() {
        copy_missing_tree(&packaged_userdata, &installed_userdata, None)?;
    }

    //
    // LICENSE
    //
    // Only install it when missing.
    //
    if !installed_license.exists() && packaged_license.is_file() {
        std::fs::copy(&packaged_license, &installed_license)
            .map_err(|err| format!("Failed to install LICENSE: {err}"))?;
    }
    ensure_license_installed(install_dir)?;

    //
    // Mark package current ONLY after every install operation succeeded.
    //
    std::fs::create_dir_all(&installed_assets)
        .map_err(|err| format!("Failed to create .assets directory: {err}"))?;

    std::fs::write(installed_assets.join("version.txt"), package_version)
        .map_err(|err| format!("Failed to update .assets/version.txt: {err}"))?;

    std::fs::remove_dir_all(&extraction_dir)
        .map_err(|err| format!("Failed to clean package extraction directory: {err}"))?;

    Ok(install_state)
}

// Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
pub(crate) fn ensure_package_installed(install_dir: &Path) -> Result<PackageInstallState, String> {
    if !package_install_required(install_dir) {
        return Ok(PackageInstallState::Current);
    }

    std::fs::create_dir_all(install_dir)
        .map_err(|err| format!("Failed to create Aeowun install directory: {err}"))?;

    let zip_path = install_dir.join("UserData.zip");

    download_file(&package_download_url(), &zip_path)?;

    let install_result = install_package_from_zip(&zip_path, install_dir);

    if zip_path.exists() {
        std::fs::remove_file(&zip_path)
            .map_err(|err| format!("Failed to remove UserData.zip: {err}"))?;
    }

    install_result
}

pub(crate) fn install_assets_from_zip(zip_path: &Path, install_dir: &Path) -> Result<(), String> {
    let assets_dir = install_dir.join(".assets");
    let extraction_dir = install_dir.join(".assets_extract");

    if extraction_dir.exists() {
        std::fs::remove_dir_all(&extraction_dir).map_err(|err| {
            format!("Failed to clear temporary .assets extraction directory: {err}")
        })?;
    }

    std::fs::create_dir_all(&extraction_dir)
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
    std::fs::create_dir_all(install_dir)
        .map_err(|err| format!("Failed to create Aeowun install directory: {err}"))?;
    if !license_path.exists() {
        std::fs::write(&license_path, LICENSE_CONTENT)
            .map_err(|err| format!("Failed to install LICENSE: {err}"))?;
    }
    // Keep the legacy license contract while exposing the Windows-friendly filename.
    let text_license_path = install_dir.join("LICENSE.txt");
    if !text_license_path.exists() {
        std::fs::copy(&license_path, &text_license_path)
            .map_err(|err| format!("Failed to install LICENSE.txt: {err}"))?;
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
        .map_err(|err| format!("Failed to create Worldkiln desktop shortcut: {err}"))?;

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

    // Cleanup is exclusively for a downloaded Installer.exe. In particular,
    // Worldkiln.exe must remain safe even if its path differs textually from
    // the canonical install path or installation has failed.
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
        .map_err(|err| format!("Failed to schedule installer self-cleanup: {err}"))?;

    Ok(())
}

#[cfg(not(target_os = "windows"))]
pub(crate) fn schedule_installer_self_cleanup(_installer_exe: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(test)]
mod version_tests {
    use super::*;

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

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
    #[test]
    fn native_curl_fallback_downloads_and_installs_in_isolated_profile() {
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
            || download_with_curl(&url, &zip_path),
            |_| {},
        )
        .expect("curl fallback should download the package from the local fixture server");
        assert_eq!(rust_attempts, PACKAGE_DOWNLOAD_ATTEMPTS);

        install_package_from_zip(&zip_path, &install_dir)
            .expect("verified package should install in the isolated profile");
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

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
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

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:32 -05:00
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
        assert!(!package_install_required(&root));

        std::fs::write(&version_path, "999.0.0").unwrap();
        assert!(!package_install_required(&root));

        std::fs::write(&version_path, "invalid").unwrap();
        assert!(!package_install_required(&root));

        std::fs::remove_file(&version_path).unwrap();
        assert!(package_install_required(&root));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn package_install_skips_download_for_equal_newer_or_invalid_versions() {
        let root = bootstrap_fixture("package_install_version_gate");
        std::fs::create_dir_all(root.join(".assets")).unwrap();

        for version in [ENGINE_VERSION, "999.0.0", "invalid"] {
            std::fs::write(root.join(".assets/version.txt"), version).unwrap();
            assert_eq!(
                ensure_package_installed(&root).unwrap(),
                PackageInstallState::Current
            );
            assert!(!root.join("UserData.zip").exists());
            assert!(!root.join("UserData").exists());
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

    // Worldkiln 0.8.5 | WO MAT-036 | 2026-10-10 11:28 -05:00
    #[test]
    fn canonical_editor_bootstraps_for_license_or_older_package_only() {
        let root = bootstrap_fixture("canonical");
        let install_dir = root.join("Aeowun");
        let current_exe = install_dir.join(CANONICAL_EXECUTABLE_NAME);
        for missing in [None, Some(".assets"), Some("UserData"), Some("LICENSE")] {
            std::fs::create_dir_all(install_dir.join(".assets")).unwrap();
            std::fs::create_dir_all(install_dir.join("UserData")).unwrap();
            std::fs::write(install_dir.join(".assets/version.txt"), ENGINE_VERSION).unwrap();
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
        std::fs::write(install_dir.join(".assets/version.txt"), ENGINE_VERSION).unwrap();
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
