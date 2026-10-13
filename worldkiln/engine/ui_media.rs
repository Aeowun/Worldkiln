// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:17 -05:00
//! Shared, project-local media validation used by UI Editor and AeoScript.
//! The same contract is used by the image and video player backends.
use std::path::{Component, Path, PathBuf};

/// Sources are project-local: Media/intro.mp4, Media/art.png,
/// or a pre-imported texture, e.g. textures/wood.png (project .assets).
pub fn validate_ui_media_source(source: &str) -> Result<(), String> {
    if source.is_empty() {
        return Ok(());
    }
    if source.trim() != source
        || source.contains('\\')
        || source.contains(':')
        || source.starts_with('/')
    {
        return Err("source must be a relative Media/ path using forward slashes".into());
    }
    // Path::components() normalizes "." on Windows, so reject suspicious
    // raw segments BEFORE normalization and canonical filesystem checks.
    if source
        .split('/')
        .any(|segment| segment.is_empty() || segment == "." || segment == "..")
    {
        return Err("source contains an empty, dot or traversal component".into());
    }
    let path = Path::new(source);
    let mut components = path.components();
    let first = components.next();
    if !matches!(first, Some(Component::Normal(label))
        if label == "Media" || label == "textures")
    {
        return Err("source must start with Media/ or textures/".into());
    }
    let mut count = 0;
    for component in components {
        match component {
            Component::Normal(segment) => {
                if segment.to_string_lossy().starts_with('.') {
                    return Err("source contains a hidden or unsafe path component".into());
                }
                count += 1;
            }
            _ => return Err("source contains unsafe path traversal".into()),
        }
    }
    if count == 0 {
        return Err("source must name a media file inside Media/".into());
    }
    let is_texture = source.starts_with("textures/");
    match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("png" | "jpg" | "jpeg") => Ok(()),
        Some("mp4") if !is_texture => Ok(()),
        _ => Err(
            "image supports PNG/JPEG in textures/ or Media/; video supports MP4 in Media/".into(),
        ),
    }
}

/// Converts an asset path to project/.assets/... without re-importing pictures.
/// Symlinks may not lead outside the project: existing files must canonicalize inside it.
pub fn resolve_ui_media_path(project: &Path, source: &str) -> Result<PathBuf, String> {
    validate_ui_media_source(source)?;
    if source.is_empty() {
        return Err("source is empty".into());
    }
    let raw = project.join(".assets").join(source);
    if !raw.is_file() {
        return Err(format!(
            "media file '{}' does not exist in project assets",
            source
        ));
    }
    let canonical_root = project
        .join(".assets")
        .canonicalize()
        .map_err(|e| format!("Cannot inspect project assets: {e}"))?;
    let file = raw
        .canonicalize()
        .map_err(|e| format!("Cannot inspect media file: {e}"))?;
    if !file.starts_with(&canonical_root) {
        return Err("source escapes project assets".into());
    }
    Ok(file)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ui_media_source_rejects_traversal_urls_and_unsupported_files() {
        for source in [
            "C:/temp/out.mp4",
            "../escape.mp4",
            "Media/../escape.mp4",
            "Media\\escape.mp4",
            "https://example.com/x.mp4",
            "/Media/a.mp4",
            "Media/.secret.mp4",
            "Media/intro.avi",
            "Media/",
            "Media/a.mp4 ",
            "notmedia/a.mp4",
            "Media/./clip.mp4",
            "textures/a.mp4",
        ] {
            assert!(
                validate_ui_media_source(source).is_err(),
                "Unexpectedly allowed {source:?}"
            );
        }
    }

    #[test]
    fn ui_media_source_accepts_local_image_and_video_assets() {
        for source in [
            "",
            "Media/intro.mp4",
            "Media/credits.PNG",
            "Media/art/portrait.jpeg",
            "Media/art/credits.jpg",
            "textures/grass.png",
            "textures/custom/stone.PNG",
        ] {
            assert!(validate_ui_media_source(source).is_ok(), "{source:?}");
        }
    }

    #[test]
    fn ui_media_resolves_preimported_texture_without_copy() {
        let root = std::env::temp_dir().join(format!("wo52_texture_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".assets/textures")).unwrap();
        std::fs::write(root.join(".assets/textures/wood.png"), b"png_fixture").unwrap();
        let actual = resolve_ui_media_path(&root, "textures/wood.png").unwrap();
        assert_eq!(
            actual,
            root.join(".assets/textures/wood.png")
                .canonicalize()
                .unwrap()
        );
        std::fs::remove_dir_all(&root).unwrap();
    }
    #[test]
    fn ui_media_resolve_requires_existing_project_file() {
        let root = std::env::temp_dir().join(format!("wo52_media_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join(".assets/Media")).unwrap();
        std::fs::write(root.join(".assets/Media/intro.mp4"), b"mock").unwrap();
        assert!(
            resolve_ui_media_path(&root, "Media/intro.mp4")
                .unwrap()
                .is_file()
        );
        assert!(resolve_ui_media_path(&root, "Media/missing.mp4").is_err());
        std::fs::remove_dir_all(&root).unwrap();
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:46 -05:00
// A resolved project-media path is immutable until the project/UI session is
// recreated. Do not re-canonicalize the same path every rendered frame.
thread_local! {
    static UI_RESOLVED_PATH_CACHE: std::cell::RefCell<std::collections::HashMap<
        (PathBuf,String),PathBuf>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
fn cached_media_path(project: &Path, source: &str) -> Result<PathBuf, String> {
    validate_ui_media_source(source)?;
    UI_RESOLVED_PATH_CACHE.with(|cache| {
        let key = (project.to_path_buf(), source.to_string());
        if let Some(path) = cache.borrow().get(&key) {
            return Ok(path.clone());
        }
        let path = resolve_ui_media_path(project, source)?;
        cache.borrow_mut().insert(key, path.clone());
        Ok(path)
    })
}

// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:26 -05:00
// Image decoding is cached per file and egui context on the UI thread;
// frames are reused, never read or decoded every render frame.
thread_local! {
    static UI_IMAGE_CACHE: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf,
        egui::TextureHandle>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}

pub fn paint_media_frame(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: egui::Rect,
    source: &str,
    fit: crate::engine::ui::UiMediaFit,
    tint: [f32; 4],
    project_path: Option<&Path>,
) {
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let Some(project_path) = project_path else {
        paint_missing_media(painter, rect, "No active project");
        return;
    };
    let path = match cached_media_path(project_path, source) {
        Ok(path) => path,
        Err(error) => {
            paint_missing_media(painter, rect, &error);
            return;
        }
    };
    UI_IMAGE_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        let needs_load = !cache.contains_key(&path);
        if needs_load {
            let decoded = (|| -> Result<egui::ColorImage, String> {
                let data = std::fs::read(&path).map_err(|e| e.to_string())?;
                if data.len() > 64 * 1024 * 1024 {
                    return Err("Image exceeds 64 MiB file limit".into());
                }
                let img = image::load_from_memory(&data).map_err(|e| e.to_string())?;
                if img.width() > 8192 || img.height() > 8192 {
                    return Err("Image exceeds 8192-pixel dimension limit".into());
                }
                let rgba = img.to_rgba8();
                Ok(egui::ColorImage::from_rgba_unmultiplied(
                    [rgba.width() as usize, rgba.height() as usize],
                    rgba.as_raw(),
                ))
            })();
            match decoded {
                Ok(decoded) => {
                    let texture = ctx.load_texture(
                        format!("ui_media_{}", path.to_string_lossy()),
                        decoded,
                        egui::TextureOptions::LINEAR,
                    );
                    cache.insert(path.clone(), texture);
                }
                Err(error) => {
                    cache.remove(&path);
                    paint_missing_media(painter, rect, &format!("Media decode: {error}"));
                    return;
                }
            }
        }
        if let Some(texture) = cache.get(&path) {
            let native_size = texture.size_vec2();
            let color = egui::Color32::from_rgba_unmultiplied(
                (tint[0] * 255.0).clamp(0.0, 255.0) as u8,
                (tint[1] * 255.0).clamp(0.0, 255.0) as u8,
                (tint[2] * 255.0).clamp(0.0, 255.0) as u8,
                (tint[3] * 255.0).clamp(0.0, 255.0) as u8,
            );
            paint_texture_fit(painter, texture.id(), rect, native_size, fit, color);
        }
    });
}

pub fn paint_texture_fit(
    painter: &egui::Painter,
    texture: egui::TextureId,
    rect: egui::Rect,
    native: egui::Vec2,
    fit: crate::engine::ui::UiMediaFit,
    tint: egui::Color32,
) {
    if native.x <= 0.0 || native.y <= 0.0 || rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let (mut target, mut uv) = (
        rect,
        egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
    );
    let frame_aspect = rect.width() / rect.height();
    let image_aspect = native.x / native.y;
    match fit {
        crate::engine::ui::UiMediaFit::Stretch => {}
        crate::engine::ui::UiMediaFit::Contain => {
            let dimensions = if image_aspect > frame_aspect {
                egui::vec2(rect.width(), rect.width() / image_aspect)
            } else {
                egui::vec2(rect.height() * image_aspect, rect.height())
            };
            target = egui::Rect::from_center_size(rect.center(), dimensions);
        }
        crate::engine::ui::UiMediaFit::Cover => {
            if image_aspect > frame_aspect {
                let width = frame_aspect / image_aspect;
                uv.min.x = (1.0 - width) * 0.5;
                uv.max.x = (1.0 + width) * 0.5;
            } else {
                let height = image_aspect / frame_aspect;
                uv.min.y = (1.0 - height) * 0.5;
                uv.max.y = (1.0 + height) * 0.5;
            }
        }
    }
    painter.image(texture, target, uv, tint);
}
// Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:30 -05:00
// Do not replace long actionable video error messages with "Media unavailable".
// Keep the on-screen text bounded and publish full details to the deduplicated
// diagnostics channel, so the editor Output identifies the missing source.
fn media_error_label(message: &str) -> &str {
    if message.contains("does not exist in project assets") {
        return "Video missing: import MP4 into project .assets/Media";
    }
    if message.contains("source must be a relative")
        || message.contains("source escapes project")
        || message.contains("source must start with")
    {
        return "Video source must use a project Media/ path";
    }
    if message.contains("Cannot open MP4") {
        return "Cannot open MP4 (see Output for details)";
    }
    if message.contains("H.264")
        || message.contains("decoder")
        || message.contains("Media Foundation")
    {
        return "Video decoding failed (see Output for details)";
    }
    if message.len() > 80 {
        return "Media load failed (see Output for details)";
    }
    message
}
fn paint_missing_media(painter: &egui::Painter, rect: egui::Rect, message: &str) {
    painter.rect_filled(rect, 2.0, egui::Color32::from_rgb(28, 29, 37));
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        media_error_label(message),
        egui::FontId::monospace(12.0),
        egui::Color32::LIGHT_GRAY,
    );
}

// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:40 -05:00
// Actual MP4 frame playback: bounded async Windows Media Foundation decode,
// egui texture reused and updated only when a NEW video frame arrives.
#[cfg(windows)]
struct UiVideoEntry {
    control: std::sync::Arc<crate::engine::ui_video_windows::VideoControl>,
    frames: std::sync::mpsc::Receiver<crate::engine::ui_video_windows::DecodeEvent>,
    texture: Option<egui::TextureHandle>,
    error: Option<String>,
    // Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09
    last_painted: std::time::Instant,
}
#[cfg(windows)]
thread_local! {
    static UI_VIDEO_CACHE: std::cell::RefCell<std::collections::HashMap<std::path::PathBuf,UiVideoEntry>> =
        std::cell::RefCell::new(std::collections::HashMap::new());
}
#[cfg(windows)]
pub fn paint_video_frame(
    painter: &egui::Painter,
    ctx: &egui::Context,
    rect: egui::Rect,
    source: &str,
    fit: crate::engine::ui::UiMediaFit,
    tint: [f32; 4],
    playing: bool,
    looped: bool,
    playback_rate: f32,
    project_path: Option<&Path>,
) {
    use std::sync::atomic::Ordering;
    if rect.width() <= 0.0 || rect.height() <= 0.0 {
        return;
    }
    let Some(project_path) = project_path else {
        paint_missing_media(painter, rect, "Video requires an active project");
        return;
    };
    if !source.to_ascii_lowercase().ends_with(".mp4") {
        paint_missing_media(painter, rect, "Video source must be .mp4");
        return;
    }
    let path = match resolve_ui_media_path(project_path, source) {
        Ok(path) => path,
        Err(error) => {
            // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:30 -05:00
            crate::engine::diagnostics::report(
                "ERROR",
                "Media",
                format!("Video source '{source}': {error}"),
            );
            paint_missing_media(painter, rect, &error);
            return;
        }
    };
    if let Ok(len) = path.metadata().map(|m| m.len()) {
        if len > 1024 * 1024 * 1024 {
            paint_missing_media(painter, rect, "Video exceeds 1 GiB file limit");
            return;
        }
    }
    UI_VIDEO_CACHE.with(|cache| {
        let mut cache = cache.borrow_mut();
        if !cache.contains_key(&path) {
            // Bound session cache. Stopping previous background workers also
            // releases decoder resources when users switch media assets.
            if cache.len() >= 3 {
                for (_, entry) in cache.drain() {
                    entry.control.stop.store(true, Ordering::Relaxed);
                }
            }
            let control = std::sync::Arc::new(crate::engine::ui_video_windows::VideoControl::new());
            control.playing.store(playing, Ordering::Relaxed);
            control.looped.store(looped, Ordering::Relaxed);
            let (tx, rx) = std::sync::mpsc::sync_channel(2);
            crate::engine::ui_video_windows::start_video_worker(path.clone(), tx, control.clone());
            // Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09
            // Decode/play the soundtrack on a separate Media Foundation worker.
            // The same control drives pause, speed, looping and skip/shutdown.
            crate::engine::ui_video_audio_windows::start_video_audio_worker(
                path.clone(),
                control.clone(),
            );
            cache.insert(
                path.clone(),
                UiVideoEntry {
                    control,
                    frames: rx,
                    texture: None,
                    error: None,
                    last_painted: std::time::Instant::now(),
                },
            );
        }
        let Some(entry) = cache.get_mut(&path) else {
            return;
        };
        entry.last_painted = std::time::Instant::now();
        entry.control.playing.store(playing, Ordering::Relaxed);
        entry.control.looped.store(looped, Ordering::Relaxed);
        entry.control.rate_milli.store(
            (playback_rate.clamp(0.25, 4.0) * 1000.0) as u32,
            Ordering::Relaxed,
        );
        let mut newest = None;
        while let Ok(next) = entry.frames.try_recv() {
            match next {
                Ok(frame) => newest = Some(frame),
                Err(error) => {
                    // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:30 -05:00
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Media",
                        format!("Video decoder for '{}': {error}", path.display()),
                    );
                    entry.error = Some(error);
                }
            }
        }
        if let Some(frame) = newest {
            // Worldkiln 0.8.5 | WO 0.8.7-62 | 2026-10-09
            // Pixel conversion and allocation were already done in the
            // decoder worker. Transfer the ready ColorImage by move; avoid
            // another full-frame conversion on the 3D/UI render thread.
            let decoded = frame.image;
            if let Some(texture) = &mut entry.texture {
                texture.set(decoded, egui::TextureOptions::LINEAR);
            } else {
                entry.texture = Some(ctx.load_texture(
                    format!("worldkiln_video_{}", path.to_string_lossy()),
                    decoded,
                    egui::TextureOptions::LINEAR,
                ));
            }
        }
        if let Some(texture) = &entry.texture {
            let color = egui::Color32::from_rgba_unmultiplied(
                (tint[0] * 255.0).clamp(0.0, 255.0) as u8,
                (tint[1] * 255.0).clamp(0.0, 255.0) as u8,
                (tint[2] * 255.0).clamp(0.0, 255.0) as u8,
                (tint[3] * 255.0).clamp(0.0, 255.0) as u8,
            );
            paint_texture_fit(painter, texture.id(), rect, texture.size_vec2(), fit, color);
        } else {
            paint_missing_media(
                painter,
                rect,
                entry.error.as_deref().unwrap_or("Loading video..."),
            );
        }
        // Video decoding never drives terrain/editor work itself; repaint only
        // while animated and the UI frame is being displayed.
        if playing || entry.texture.is_none() {
            ctx.request_repaint_after(std::time::Duration::from_millis(33));
        }
    });
}

/// End-of-frame cleanup: invisible Videos stop BOTH the audio and video workers.
/// Without this, hiding a cinematic via AeoScript would leave audio running.
/// It also allows a later replay to start from the beginning instead of
/// reusing a finished media decoder entry.
/// Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09
#[cfg(windows)]
pub fn finish_video_ui_frame() {
    use std::sync::atomic::Ordering;
    UI_VIDEO_CACHE.with(|cache| {
        cache.borrow_mut().retain(|_, entry| {
            let keep = entry.last_painted.elapsed() < std::time::Duration::from_millis(200);
            if !keep {
                entry.control.stop.store(true, Ordering::Relaxed);
            }
            keep
        });
    });
}

/// Imports an MP4 into the *current project*, leaving the selected original
/// unchanged. Collision-safe names preserve all earlier footage.
pub fn import_mp4(project: &Path, source: &Path) -> Result<String, String> {
    if !project.is_dir() {
        return Err("Open a project before importing MP4".into());
    }
    if !source.is_file()
        || !source
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("mp4"))
    {
        return Err("Select a valid local .mp4 file".into());
    }
    if source.metadata().map_err(|e| e.to_string())?.len() > 1024 * 1024 * 1024 {
        return Err("Videos larger than 1 GiB are not supported".into());
    }
    let assets = project.join(".assets");
    std::fs::create_dir_all(assets.join("Media")).map_err(|e| e.to_string())?;
    let source_name = source
        .file_stem()
        .and_then(|stem| stem.to_str())
        .ok_or_else(|| "Invalid MP4 filename".to_string())?;
    let filename = source
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| "Invalid MP4 filename".to_string())?;
    let mut dest = assets.join("Media").join(filename);
    let mut suffix = 1;
    while dest.exists() {
        dest = assets
            .join("Media")
            .join(format!("{source_name}_{suffix}.mp4"));
        suffix += 1;
    }
    std::fs::copy(source, &dest).map_err(|e| format!("MP4 import failed: {e}"))?;
    let rel = format!("Media/{}", dest.file_name().unwrap().to_string_lossy());
    validate_ui_media_source(&rel)?;
    Ok(rel)
}

#[cfg(test)]
mod media_import_tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:30 -05:00
    #[test]
    fn video_errors_remain_actionable_instead_of_generic_media_unavailable() {
        let missing = "media file 'Media/MAROONED_The_Lost_Crew_Atmospheric_Cinematic_v2.mp4' does not exist in project assets";
        assert_eq!(
            media_error_label(missing),
            "Video missing: import MP4 into project .assets/Media"
        );
        assert_eq!(
            media_error_label("source must be a relative Media/ path using forward slashes"),
            "Video source must use a project Media/ path"
        );
        assert_eq!(
            media_error_label("Cannot open MP4 'C:/Some/long/example.mp4': 0xdeadbeef"),
            "Cannot open MP4 (see Output for details)"
        );
        assert!(
            media_error_label("H.264 RGB32 converter unavailable: some error").contains("decoding")
        );
    }

    #[test]
    fn imports_two_mp4s_with_safe_unique_names() {
        let dir = std::env::temp_dir().join(format!("wo52_import_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("Project")).unwrap();
        let original = dir.join("demo.mp4");
        std::fs::write(&original, b"clip").unwrap();
        assert_eq!(
            import_mp4(&dir.join("Project"), &original).unwrap(),
            "Media/demo.mp4"
        );
        assert_eq!(
            import_mp4(&dir.join("Project"), &original).unwrap(),
            "Media/demo_1.mp4"
        );
        assert_eq!(std::fs::read(&original).unwrap(), b"clip");
        assert!(resolve_ui_media_path(&dir.join("Project"), "Media/demo_1.mp4").is_ok());
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
