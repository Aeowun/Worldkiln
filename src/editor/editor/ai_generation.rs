// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::ai_project::ProjectAccess;
use serde_json::Value;
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::{Duration, Instant},
};

pub(crate) fn is_generation_tool(name: &str) -> bool {
    matches!(name, "generate_image" | "generate_actor")
}
pub(super) struct GeneratedAsset {
    pub project: PathBuf,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
    pub summary: String,
}
impl GeneratedAsset {
    pub fn undo(&self, root: &Path) -> Result<String, String> {
        if self.project != root {
            return Err("Generated asset belongs to another project.".into());
        }
        let access = ProjectAccess::open(root)?;
        let relative = self
            .path
            .strip_prefix(root)
            .map_err(|e| e.to_string())?
            .to_string_lossy()
            .replace('\\', "/");
        let current = access.resolve(&relative, false)?;
        if fs::read(&current).map_err(|e| e.to_string())? != self.bytes {
            return Err("Generated file changed after creation; no undo applied.".into());
        }
        fs::remove_file(current).map_err(|e| e.to_string())?;
        Ok(format!(
            "Removed generated asset {relative}; later edits preserved."
        ))
    }
}
fn interpreter(root: &Path) -> Result<PathBuf, String> {
    let explicit = std::env::var_os("WORLDKILN_PYTHON").map(PathBuf::from);
    let candidates: Vec<_> = if let Some(path) = explicit {
        vec![path]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .filter(|path| path.is_absolute())
            .map(|path| {
                path.join(if cfg!(windows) {
                    "python.exe"
                } else {
                    "python3"
                })
            })
            .collect()
    };
    let root = root.canonicalize().map_err(|e| e.to_string())?;
    candidates.into_iter().filter(|path|path.is_absolute() && path.is_file() && !path.to_string_lossy().to_ascii_lowercase().contains("\\windowsapps\\"))
        .filter_map(|path|path.canonicalize().ok()).find(|path|!path.starts_with(&root))
        .ok_or_else(|| "Python 3 is unavailable. Install it on PATH or set WORLDKILN_PYTHON to an absolute interpreter outside the project. No files generated.".into())
}
fn python(root: &Path, name: &str, args: &Value, stop: &AtomicBool) -> Result<Vec<u8>, String> {
    let script = match name {
        "generate_image" => include_str!("generators/image_generator.py"),
        "generate_actor" => include_str!("generators/generate_actor.py"),
        _ => return Err("Unknown generation tool.".into()),
    };
    let input = serde_json::to_vec(args).map_err(|e| e.to_string())?;
    if input.len() > 1024 * 1024 {
        return Err("Generation request exceeds 1 MiB.".into());
    }
    let mut command = Command::new(interpreter(root)?);
    command
        .args(["-I", "-S", "-c", script])
        .current_dir(root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        command.creation_flags(0x08000000);
    }
    let mut child = command
        .spawn()
        .map_err(|e| format!("Could not start Python: {e}"))?;
    let mut stdin = child.stdin.take().ok_or("Python input unavailable.")?;
    let writer = std::thread::spawn(move || stdin.write_all(&input));
    let stdout = child.stdout.take().ok_or("Python output unavailable.")?;
    let stderr = child
        .stderr
        .take()
        .ok_or("Python diagnostics unavailable.")?;
    let reader = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stdout
            .take(16 * 1024 * 1024 + 1)
            .read_to_end(&mut bytes)
            .map(|_| bytes)
    });
    let errors = std::thread::spawn(move || {
        let mut bytes = Vec::new();
        stderr
            .take(4096)
            .read_to_end(&mut bytes)
            .map(|_| String::from_utf8_lossy(&bytes).into_owned())
    });
    let start = Instant::now();
    let status = loop {
        if stop.load(Ordering::Relaxed) || start.elapsed() > Duration::from_secs(120) {
            let _ = child.kill();
            let _ = child.wait();
            let _ = writer.join();
            let _ = reader.join();
            let _ = errors.join();
            return Err("Generation cancelled or exceeded 120 seconds; no asset published.".into());
        }
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(error.to_string());
            }
        }
    };
    writer
        .join()
        .map_err(|_| "Python input worker failed.")?
        .map_err(|e| e.to_string())?;
    let bytes = reader
        .join()
        .map_err(|_| "Python output worker failed.")?
        .map_err(|e| e.to_string())?;
    let error = errors
        .join()
        .map_err(|_| "Python diagnostics worker failed.")?
        .map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("Generation failed: {}", error.trim()));
    }
    if bytes.is_empty() || bytes.len() > 16 * 1024 * 1024 {
        return Err("Generated output is empty or exceeds 16 MiB.".into());
    }
    Ok(bytes)
}
pub(super) fn run(
    root: &Path,
    name: &str,
    args: &Value,
    stop: &AtomicBool,
) -> Result<GeneratedAsset, String> {
    if stop.load(Ordering::Relaxed) {
        return Err("Generation cancelled.".into());
    }
    let stem = args["name"]
        .as_str()
        .ok_or("A generated asset name is required.")?;
    if stem.is_empty()
        || stem.len() > 64
        || !stem
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"_-".contains(&b))
    {
        return Err(
            "Use 1–64 ASCII letters, digits, underscores or hyphens for the asset name.".into(),
        );
    }
    let lower = stem.to_ascii_lowercase();
    if ["con", "prn", "aux", "nul"].contains(&lower.as_str())
        || (lower.len() == 4
            && (lower.starts_with("com") || lower.starts_with("lpt"))
            && matches!(lower.as_bytes()[3], b'1'..=b'9'))
    {
        return Err("Choose a name that is not a reserved Windows device name.".into());
    }
    let access = ProjectAccess::open(root)?;
    let folder = root.join("generated");
    if !folder.exists() {
        fs::create_dir(&folder).map_err(|e| e.to_string())?;
    }
    let directory = access.resolve("generated", true)?;
    let extension = match name {
        "generate_image" => "png",
        "generate_actor" => "glb",
        _ => return Err("Unknown generator.".into()),
    };
    let filename = format!("{stem}.{extension}");
    if directory.join(&filename).exists() {
        return Err(
            "Generated asset name already exists; choose a new name. No overwrite performed."
                .into(),
        );
    }
    let bytes = python(root, name, args, stop)?;
    if stop.load(Ordering::Relaxed) {
        return Err("Generation cancelled; no asset published.".into());
    }
    let path = access.resolve("generated", true)?.join(filename);
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(&path)
        .map_err(|e| format!("Cannot create generated asset: {e}"))?;
    if let Err(error) = file.write_all(&bytes).and_then(|_| file.sync_all()) {
        drop(file);
        if fs::read(&path)
            .is_ok_and(|current| current.len() <= bytes.len() && current == bytes[..current.len()])
        {
            let _ = fs::remove_file(&path);
        }
        return Err(error.to_string());
    }
    drop(file);
    let result = if name == "generate_actor" {
        crate::project::gltf_import::inspect_glb(&path).and_then(|summary| {
            if summary.kind != crate::project::gltf_import::GlbAssetKind::StaticMesh {
                return Err("Generator must produce a supported static Actor GLB.".into());
            }
            for mesh in &summary.meshes {
                crate::project::gltf_import::load_static_glb_mesh(&path, &mesh.name)?;
            }
            Ok(format!(
                "Worldkiln-supported static GLB validated: {} mesh(es).",
                summary.meshes.len()
            ))
        })
    } else {
        image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
            .map(|im| format!("PNG decoded: {} × {}.", im.width(), im.height()))
            .map_err(|e| e.to_string())
    };
    let summary = match result {
        Ok(summary) if !stop.load(Ordering::Relaxed) => summary,
        other => {
            if fs::read(&path).is_ok_and(|current| current == bytes) {
                let _ = fs::remove_file(&path);
            }
            return Err(other
                .err()
                .unwrap_or_else(|| "Generation cancelled; output discarded.".into()));
        }
    };
    Ok(GeneratedAsset {
        project: root.to_path_buf(),
        path: root.join("generated").join(path.file_name().unwrap()),
        bytes,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn assistant_v2_python_generation_validates_glb_and_preserves_edits() {
        let root = std::env::temp_dir().join(format!("wk_generator_{}", std::process::id()));
        fs::create_dir_all(&root).unwrap();
        let result = (|| {
            let args = serde_json::json!({"name":"Triangle","vertices":[[0,0,0],[1,0,0],[0,1,0]],"triangles":[[0,1,2]]});
            let stop = AtomicBool::new(false);
            let glb = run(&root, "generate_actor", &args, &stop).unwrap();
            let summary = crate::project::gltf_import::inspect_glb(&glb.path).unwrap();
            assert_eq!(
                summary.kind,
                crate::project::gltf_import::GlbAssetKind::StaticMesh
            );
            assert!(run(&root, "generate_actor", &args, &stop).is_err());
            assert!(run(&root,"generate_actor",&serde_json::json!({"name":"Bad","vertices":[[0,0,0],[1,0,0],[0,1,0]],"triangles":[[0,1,99]]}),&stop).is_err());
            assert!(!root.join("generated/Bad.glb").exists());
            let png = run(
                &root,
                "generate_image",
                &serde_json::json!({"name":"Texture","width":32,"height":32,"pattern":"brick"}),
                &stop,
            )
            .unwrap();
            png.undo(&root).unwrap();
            assert!(!png.path.exists());
            fs::write(&glb.path, b"later creator edit").unwrap();
            assert!(glb.undo(&root).is_err());
            assert_eq!(fs::read(&glb.path).unwrap(), b"later creator edit");
        })();
        let _ = fs::remove_dir_all(&root);
        result
    }
}
