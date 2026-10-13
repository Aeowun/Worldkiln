use std::fs;
use std::path::{Path, PathBuf};

use crate::engine::game_package::{
    EmbeddedGameManifest, RuntimeReleaseAsset, append_game_package, create_project_payload,
    ensure_game_id, probe_embedded_game, resolve_runtime_release_asset, verify_embedded_payload,
};

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:42 -05:00
// Mirrors the existing aeowun.com game-submission ceiling so Build Game
// never emits a single-EXE artifact that the Games page cannot accept.
const AEOWUN_GAMES_MAX_UPLOAD_BYTES: u64 = 500 * 1024 * 1024;

fn validate_games_page_size(bytes: u64) -> Result<(), String> {
    if bytes > AEOWUN_GAMES_MAX_UPLOAD_BYTES {
        return Err(format!(
            "Built game is {:.1} MiB, above the aeowun.com Games limit of 500 MiB.",
            bytes as f64 / (1024.0 * 1024.0)
        ));
    }
    Ok(())
}

pub struct BuildResult {
    pub output_dir: PathBuf,
    pub executable: PathBuf,
    pub output_log: String,
}

fn project_name(project_path: &Path) -> String {
    project_path
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.trim().is_empty())
        .unwrap_or("MyGame")
        .to_string()
}

fn unique_staging_dir(parent: &Path, label: &str) -> PathBuf {
    parent.join(format!(
        ".{label}.worldkiln-build-{}-{:032x}",
        std::process::id(),
        rand::random::<u128>()
    ))
}

fn publish_build_directory(staging: &Path, destination: &Path) -> Result<(), String> {
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
        fs::remove_dir_all(&backup).map_err(|error| {
            format!(
                "New build was published, but old build backup {} could not be removed: {error}",
                backup.display()
            )
        })?;
    }

    Ok(())
}

fn package_game_with_template(
    project_path: &Path,
    executable_template: &Path,
    runtime_asset: &RuntimeReleaseAsset,
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
    let game_id = ensure_game_id(project_path)?;
    let version = env!("CARGO_PKG_VERSION");

    if runtime_asset.release_tag != format!("win11-v{version}") {
        return Err(format!(
            "Runtime release '{}' does not match Worldkiln {}",
            runtime_asset.release_tag, version
        ));
    }

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
        let payload = staging.join(".GameData.payload.zip");
        let (payload_sha256, payload_len) = create_project_payload(project_path, &payload)?;

        let executable_name = if cfg!(windows) {
            format!("{project_name}.exe")
        } else {
            project_name.clone()
        };
        let staged_executable = staging.join(&executable_name);

        let manifest = EmbeddedGameManifest {
            format_version: 1,
            game_id,
            game_name: project_name.clone(),
            engine_version: version.to_string(),
            payload_sha256,
            runtime_release_tag: runtime_asset.release_tag.clone(),
            runtime_asset_name: runtime_asset.asset_name.clone(),
            runtime_asset_url: runtime_asset.browser_download_url.clone(),
            runtime_asset_sha256: runtime_asset.sha256.clone(),
            runtime_asset_size: runtime_asset.size,
        };

        append_game_package(executable_template, &payload, &manifest, &staged_executable)?;

        let packaged = probe_embedded_game(&staged_executable)?.ok_or_else(|| {
            "Staged game executable did not contain its embedded package".to_string()
        })?;
        verify_embedded_payload(&staged_executable, &packaged)?;

        let staged_size = fs::metadata(&staged_executable)
            .map_err(|error| format!("Failed to inspect staged game executable: {error}"))?
            .len();
        validate_games_page_size(staged_size)?;

        fs::remove_file(&payload)
            .map_err(|error| format!("Failed to remove temporary game payload: {error}"))?;

        let output_entries = fs::read_dir(&staging)
            .map_err(|error| format!("Failed to inspect staged game build: {error}"))?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|error| format!("Failed to inspect staged game build: {error}"))?;
        if output_entries.len() != 1
            || output_entries[0].path() != staged_executable
            || !staged_executable.is_file()
        {
            return Err(
                "Self-executable build staging contains unexpected companion files".to_string(),
            );
        }

        publish_build_directory(&staging, &output_dir)?;

        let executable = output_dir.join(executable_name);
        let mut log = format!(
            "Self-executable package created.\nEmbedded GameData: {} bytes.\nRuntime package: {} / {}.\nRuntime SHA-256: {}.\n",
            payload_len, runtime_asset.release_tag, runtime_asset.asset_name, runtime_asset.sha256
        );
        if runtime_asset.immutable_release {
            log.push_str("GitHub release is immutable.\n");
        } else {
            log.push_str(
                "WARNING: GitHub release is not marked immutable; the exact runtime SHA-256 is still pinned inside this game executable.\n",
            );
        }
        log.push_str("Build output contains exactly one file: the game executable.\n");

        Ok(BuildResult {
            output_dir: output_dir.clone(),
            executable,
            output_log: log,
        })
    })();

    if result.is_err() && staging.exists() {
        let _ = fs::remove_dir_all(&staging);
    }

    result
}

// Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:12 -05:00
pub fn build_game(project_path: &Path) -> Result<BuildResult, String> {
    let executable_template = std::env::current_exe()
        .map_err(|error| format!("Failed to locate installed Worldkiln executable: {error}"))?;

    let runtime_asset = resolve_runtime_release_asset(env!("CARGO_PKG_VERSION"))?;

    package_game_with_template(project_path, &executable_template, &runtime_asset)
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
    fn games_page_size_limit_matches_existing_submission_contract() {
        assert!(validate_games_page_size(AEOWUN_GAMES_MAX_UPLOAD_BYTES).is_ok());
        assert!(validate_games_page_size(AEOWUN_GAMES_MAX_UPLOAD_BYTES + 1).is_err());
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
