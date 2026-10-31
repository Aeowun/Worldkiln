use std::fs::{self, File};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::AtomicBool;

use crate::engine::build::{
    BuildResult, GameBuildOptions, check_cancel, snapshot_project_for_build, unique_staging_dir,
};
use crate::engine::build_platform::BuildStage;
use crate::engine::game_package::GameExportProfile;

const PROFILE_FILE: &str = ".worldkiln-export-profile.json";
const MIN_ELF_HEADER: usize = 64;

fn verify_linux_elf(path: &Path) -> Result<(), String> {
    let mut file = File::open(path)
        .map_err(|err| format!("Cannot read Linux runtime {}: {err}", path.display()))?;
    let mut header = [0_u8; MIN_ELF_HEADER];
    file.read_exact(&mut header)
        .map_err(|err| format!("Invalid Linux runtime ELF header: {err}"))?;
    if header[..4] != [0x7f, b'E', b'L', b'F'] {
        return Err("Linux runtime must be an ELF binary, not an EXE or script".into());
    }
    if header[4] != 2 || header[5] != 1 {
        return Err("Linux runtime must be 64-bit little-endian ELF".into());
    }
    let kind = u16::from_le_bytes([header[16], header[17]]);
    if !matches!(kind, 2 | 3) {
        return Err(
            "Linux runtime ELF must be executable or position-independent executable".into(),
        );
    }
    let machine = u16::from_le_bytes([header[18], header[19]]);
    if machine != 62 {
        return Err("Linux runtime ELF must target x86_64".into());
    }
    if fs::metadata(path).map_err(|err| err.to_string())?.len() <= MIN_ELF_HEADER as u64 {
        return Err("Linux runtime is incomplete".into());
    }
    Ok(())
}

fn safe_executable_name(project: &Path, options: &GameBuildOptions) -> String {
    let requested = if options.executable_name.is_empty() {
        project
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Game")
    } else {
        &options.executable_name
    };
    let cleaned: String = requested
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if cleaned.is_empty() {
        "Game".into()
    } else {
        cleaned
    }
}

fn add_archive_files<W: Write + Seek>(
    zip: &mut zip::ZipWriter<W>,
    base: &Path,
    current: &Path,
    cancel: Option<&AtomicBool>,
) -> Result<usize, String> {
    let mut count = 0;
    let mut entries = fs::read_dir(current)
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;
    entries.sort_by_key(|e| e.file_name());
    for entry in entries {
        check_cancel(cancel)?;
        let path = entry.path();
        if entry.file_type().map_err(|e| e.to_string())?.is_dir() {
            count += add_archive_files(zip, base, &path, cancel)?;
        } else {
            let relative = path.strip_prefix(base).map_err(|e| e.to_string())?;
            let name = format!("GameData/{}", relative.to_string_lossy().replace('\\', "/"));
            zip.start_file(
                name,
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Deflated)
                    .unix_permissions(0o644),
            )
            .map_err(|e| e.to_string())?;
            let mut input = File::open(&path).map_err(|e| e.to_string())?;
            let mut buf = vec![0_u8; 64 * 1024];
            loop {
                check_cancel(cancel)?;
                let len = input.read(&mut buf).map_err(|e| e.to_string())?;
                if len == 0 {
                    break;
                }
                zip.write_all(&buf[..len]).map_err(|e| e.to_string())?;
            }
            count += 1;
        }
    }
    Ok(count)
}

fn publish_archive(stage: &Path, output: &Path) -> Result<(), String> {
    let parent = output
        .parent()
        .ok_or("Linux archive has no parent directory")?;
    let previous = unique_staging_dir(parent, "linux-previous-archive");
    if output.exists() && !output.is_file() {
        return Err(format!(
            "Linux export destination is not a file: {}",
            output.display()
        ));
    }
    let had_previous = output.is_file();
    if had_previous {
        fs::rename(output, &previous)
            .map_err(|e| format!("Cannot protect earlier Linux export: {e}"))?;
    }
    if let Err(error) = fs::rename(stage, output) {
        if had_previous {
            fs::rename(&previous, output).map_err(|rollback| format!(
                "Cannot publish Linux archive: {error}; previous archive recovery failed at {}: {rollback}",
                previous.display()
            ))?;
        }
        return Err(format!("Cannot publish Linux archive: {error}"));
    }
    if had_previous {
        let _ = fs::remove_file(previous);
    }
    Ok(())
}

pub fn build_linux_game(
    project: &Path,
    options: &GameBuildOptions,
    cancel: Option<&AtomicBool>,
    on_stage: &mut dyn FnMut(BuildStage),
) -> Result<BuildResult, String> {
    check_cancel(cancel)?;
    if !project.is_dir() {
        return Err("Game project directory is unavailable".into());
    }
    let runtime = options
        .linux_runtime_binary
        .as_deref()
        .ok_or("A native Linux ELF runtime is required")?;
    verify_linux_elf(runtime)?;
    if !project.join("world.dat").is_file() {
        return Err("Game project has no authored world.dat".into());
    }
    let exec_name = safe_executable_name(project, options);
    let project_label = project
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("Game");
    let parent = if let Some(parent) = options.output_parent.as_ref() {
        parent.clone()
    } else {
        project
            .parent()
            .unwrap_or_else(|| Path::new("."))
            .join("Builds")
            .join(project_label)
    };
    let project_root =
        fs::canonicalize(project).map_err(|e| format!("Cannot resolve project directory: {e}"))?;
    if parent.exists() {
        let output_root = fs::canonicalize(&parent)
            .map_err(|e| format!("Cannot resolve Linux output directory: {e}"))?;
        if output_root.starts_with(&project_root) {
            return Err("Linux export directory must be outside the authored project".into());
        }
    } else {
        let ancestor = parent
            .ancestors()
            .find(|p| p.exists())
            .ok_or("Cannot resolve Linux export directory")?;
        let real_ancestor = fs::canonicalize(ancestor).map_err(|e| e.to_string())?;
        let suffix = parent.strip_prefix(ancestor).map_err(|e| e.to_string())?;
        if real_ancestor.join(suffix).starts_with(&project_root) {
            return Err("Linux export directory must be outside the authored project".into());
        }
    }
    fs::create_dir_all(&parent)
        .map_err(|e| format!("Cannot create Linux output directory: {e}"))?;
    let final_archive = parent.join(format!("{exec_name}-Linux.zip"));
    let temp = unique_staging_dir(&parent, "linux");
    fs::create_dir(&temp).map_err(|e| e.to_string())?;
    let result = (|| -> Result<BuildResult, String> {
        on_stage(BuildStage::Build);
        check_cancel(cancel)?;
        let frozen_runtime = temp.join("native-game");
        let original = crate::engine::game_package::sha256_file(runtime)?;
        fs::copy(runtime, &frozen_runtime).map_err(|e| e.to_string())?;
        if crate::engine::game_package::sha256_file(&frozen_runtime)? != original
            || crate::engine::game_package::sha256_file(runtime)? != original
        {
            return Err("Linux runtime changed while creating game build".into());
        }
        on_stage(BuildStage::Cook);
        let game_data = temp.join("GameData");
        snapshot_project_for_build(project, &game_data)?;
        check_cancel(cancel)?;
        on_stage(BuildStage::Stage);
        let mut profile = GameExportProfile::current_windows(project_label);
        profile.target_platform = "linux".into();
        profile.display_title = if options.title.trim().is_empty() {
            project_label.into()
        } else {
            options.title.trim().into()
        };
        profile.game_version = options.game_version.clone();
        profile.executable_name = exec_name.clone();
        profile.window_mode = options.window_mode.clone();
        profile.resolution = Some(options.resolution);
        profile.runtime_delivery = "native_linux_elf".into();
        profile.build_configuration = options.configuration.as_str().into();
        fs::write(
            game_data.join(PROFILE_FILE),
            serde_json::to_vec_pretty(&profile).map_err(|e| e.to_string())?,
        )
        .map_err(|e| e.to_string())?;
        on_stage(BuildStage::Package);
        let staged_archive = temp.join("linux-game.zip");
        let out = File::create(&staged_archive).map_err(|e| e.to_string())?;
        let mut zip = zip::ZipWriter::new(out);
        zip.start_file(
            &exec_name,
            zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .unix_permissions(0o755),
        )
        .map_err(|e| e.to_string())?;
        let mut executable = File::open(&frozen_runtime).map_err(|e| e.to_string())?;
        let mut buf = vec![0_u8; 64 * 1024];
        loop {
            check_cancel(cancel)?;
            let len = executable.read(&mut buf).map_err(|e| e.to_string())?;
            if len == 0 {
                break;
            }
            zip.write_all(&buf[..len]).map_err(|e| e.to_string())?;
        }
        let count = add_archive_files(&mut zip, &game_data, &game_data, cancel)?;
        zip.finish().map_err(|e| e.to_string())?;
        check_cancel(cancel)?;

        let mut archive =
            zip::ZipArchive::new(File::open(&staged_archive).map_err(|e| e.to_string())?)
                .map_err(|e| e.to_string())?;
        if archive.len() != count + 1 {
            return Err("Incomplete Linux archive".into());
        }
        for index in 0..archive.len() {
            check_cancel(cancel)?;
            let mut entry = archive.by_index(index).map_err(|e| e.to_string())?;
            if entry.name() == exec_name {
                if entry.unix_mode().unwrap_or(0) & 0o111 == 0 {
                    return Err("Linux executable permissions are absent from ZIP".into());
                }
            }
            io::copy(&mut entry, &mut io::sink()).map_err(|e| e.to_string())?;
        }
        publish_archive(&staged_archive, &final_archive)?;
        Ok(BuildResult {
            output_dir: parent,
            executable: final_archive.clone(),
            output_log: format!(
                "Linux x86_64 game archive: {}\nNative runtime: {}\nProject files: {}\n",
                final_archive.display(),
                runtime.display(),
                count
            ),
        })
    })();
    let _ = fs::remove_dir_all(&temp);
    result
}
