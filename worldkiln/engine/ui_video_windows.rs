// Worldkiln 0.8.7 | WO 0.8.7-52 | 2026-10-09 09:34 -05:00
//! Windows-native MP4/H.264 video decoding, used by the in-house UI player.
//! COM and Media Foundation are initialized exclusively on this worker thread.
//! Frames are RGB pixels sent on a bounded queue; the render thread never decodes video.
#![cfg(windows)]
use std::ffi::OsStr;
use std::path::Path;
use std::sync::mpsc::SyncSender;
use std::sync::{
    Arc,
    atomic::{AtomicBool, AtomicU32, Ordering},
};
use std::time::{Duration, Instant};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::PCWSTR;

pub struct DecodedVideoFrame {
    pub width: usize,
    pub height: usize,
    pub timestamp: Duration,
    // WO 0.8.7-62: already an egui pixel vector, so the main UI thread
    // can hand it directly to the GPU texture without allocating/copying
    // another complete image each frame.
    pub image: egui::ColorImage,
}
// Worldkiln 0.8.5 | WO 0.8.7-62 | 2026-10-09
// Avoid 8.3 MiB texture reuploads and a second full-size conversion every
// frame in the egui renderer. Render 720p for 1080p cinematic sources.
// Keep the original MP4 unchanged on disk and preserve its aspect ratio.
const MAX_VIDEO_UPLOAD_WIDTH: usize = 1280;
const MAX_VIDEO_UPLOAD_HEIGHT: usize = 720;
const LATE_FRAME_TOLERANCE: Duration = Duration::from_millis(75);

fn video_upload_dimensions(width: usize, height: usize) -> (usize, usize) {
    if width <= MAX_VIDEO_UPLOAD_WIDTH && height <= MAX_VIDEO_UPLOAD_HEIGHT {
        return (width, height);
    }
    let scale = (MAX_VIDEO_UPLOAD_WIDTH as f64 / width as f64)
        .min(MAX_VIDEO_UPLOAD_HEIGHT as f64 / height as f64);
    (
        (width as f64 * scale).round().max(1.0) as usize,
        (height as f64 * scale).round().max(1.0) as usize,
    )
}

/// Media Foundation RGB32 is BGRX. Downsample+convert in one pass, on the
/// decoder thread. A frame dropped for lateness never reaches this function.
fn convert_bgrx_frame(raw: &[u8], width: usize, height: usize) -> egui::ColorImage {
    let (out_width, out_height) = video_upload_dimensions(width, height);
    let mut colors = vec![egui::Color32::BLACK; out_width * out_height];
    for y in 0..out_height {
        let source_row = (y * height / out_height) * width * 4;
        let dest_row = y * out_width;
        for x in 0..out_width {
            let index = source_row + (x * width / out_width) * 4;
            colors[dest_row + x] =
                egui::Color32::from_rgb(raw[index + 2], raw[index + 1], raw[index]);
        }
    }
    egui::ColorImage {
        size: [out_width, out_height],
        pixels: colors,
    }
}

fn is_late_frame(relative: Duration, elapsed: Duration, is_first: bool) -> bool {
    !is_first && elapsed.saturating_sub(relative) > LATE_FRAME_TOLERANCE
}

pub type DecodeEvent = Result<DecodedVideoFrame, String>;
pub struct VideoControl {
    pub playing: AtomicBool,
    pub looped: AtomicBool,
    pub stop: AtomicBool,
    pub rate_milli: AtomicU32,
}
impl VideoControl {
    pub fn new() -> Self {
        Self {
            playing: AtomicBool::new(false),
            looped: AtomicBool::new(false),
            stop: AtomicBool::new(false),
            rate_milli: AtomicU32::new(1000),
        }
    }
}
impl Default for VideoControl {
    fn default() -> Self {
        Self::new()
    }
}

pub fn start_video_worker(
    path: std::path::PathBuf,
    tx: SyncSender<DecodeEvent>,
    control: Arc<VideoControl>,
) {
    let _ = std::thread::Builder::new()
        .name("worldkiln-ui-video".to_string())
        .spawn(move || {
            let result = decode_video(&path, &tx, &control);
            if let Err(message) = result {
                let _ = tx.try_send(Err(message));
            }
        });
}

// Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:38 -05:00
// std::fs::canonicalize on Windows normally yields \\?\C:\...; this
// extended-length filesystem prefix is NOT a valid file URL for the Media
// Foundation source reader and results in 0x80070035 (network path missing).
// Keep canonicalization/project-asset containment checks in ui_media.rs;
// normalize only the *decoder input*, and reject unsupported device paths.
pub(super) fn media_foundation_file_path(path: &Path) -> Result<String, String> {
    let name = path
        .to_str()
        .ok_or("MP4 path contains unsupported Unicode")?;
    if let Some(rest) = name.strip_prefix(r"\\?\") {
        if let Some(unc) = rest.strip_prefix(r"UNC\") {
            if unc.is_empty() || !unc.contains('\\') {
                return Err("Invalid extended UNC media path".into());
            }
            return Ok(format!(r"\\{unc}"));
        }
        let bytes = rest.as_bytes();
        if bytes.len() >= 3
            && bytes[0].is_ascii_alphabetic()
            && bytes[1] == b':'
            && bytes[2] == b'\\'
        {
            return Ok(rest.to_owned());
        }
        return Err("Unsupported extended device media path".into());
    }
    if name.starts_with(r"\\.\") {
        return Err("Unsupported device media path".into());
    }
    Ok(name.to_owned())
}

fn decode_video(
    path: &Path,
    tx: &SyncSender<DecodeEvent>,
    control: &VideoControl,
) -> Result<(), String> {
    // Media Foundation is available on supported desktop Windows. It's not
    // guaranteed on Windows N editions without the Media Feature Pack.
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    com.ok()
        .map_err(|e| format!("Video COM initialization failed: {e}"))?;
    let result = (|| -> Result<(), String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|e| format!("Media Foundation initialization failed: {e}"))?;
        let running = (|| -> Result<(), String> {
            // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:38 -05:00
            let media_url = media_foundation_file_path(path)?;
            let wide: Vec<u16> = OsStr::new(&media_url)
                .encode_wide()
                .chain(Some(0))
                .collect();
            let source = PCWSTR(wide.as_ptr());
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            while !control.stop.load(Ordering::Relaxed) {
                // Source Reader uses the video processor to convert decoded
                // NV12/YUV frames to RGB32, rather than requesting RGB32
                // directly from an H.264 decoder.
                let mut attributes = None;
                unsafe { MFCreateAttributes(&mut attributes, 3) }
                    .map_err(|e| format!("Cannot create decoder attributes: {e}"))?;
                let attributes = attributes.ok_or("Media Foundation returned no attributes")?;
                unsafe {
                    attributes
                        .SetUINT32(&MF_SOURCE_READER_ENABLE_VIDEO_PROCESSING, 1)
                        .map_err(|e| format!("Cannot enable RGB color conversion: {e}"))?;
                    attributes
                        .SetUINT32(&MF_READWRITE_ENABLE_HARDWARE_TRANSFORMS, 1)
                        .map_err(|e| format!("Cannot enable video transforms: {e}"))?;
                }
                let reader = unsafe { MFCreateSourceReaderFromURL(source, &attributes) }
                    .map_err(|e| format!("Cannot open MP4 '{}': {e}", path.display()))?;
                unsafe {
                    reader
                        .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                        .map_err(|e| format!("Cannot select video stream: {e}"))?;
                    reader
                        .SetStreamSelection(stream, true)
                        .map_err(|e| format!("Video stream missing: {e}"))?;
                }
                let media = unsafe { MFCreateMediaType() }
                    .map_err(|e| format!("Cannot create RGB video media type: {e}"))?;
                unsafe {
                    media
                        .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Video)
                        .map_err(|e| format!("Video type rejected: {e}"))?;
                    media
                        .SetGUID(&MF_MT_SUBTYPE, &MFVideoFormat_RGB32)
                        .map_err(|e| format!("RGB video output rejected: {e}"))?;
                    reader
                        .SetCurrentMediaType(stream, None, &media)
                        .map_err(|e| format!("H.264 RGB32 converter unavailable: {e}"))?;
                }
                let current = unsafe { reader.GetCurrentMediaType(stream) }
                    .map_err(|e| format!("Cannot query video frame dimensions: {e}"))?;
                let dimensions = unsafe { current.GetUINT64(&MF_MT_FRAME_SIZE) }
                    .map_err(|e| format!("Cannot query frame size: {e}"))?;
                let width = (dimensions >> 32) as usize;
                let height = (dimensions & 0xffffffff) as usize;
                if width < 2 || height < 2 || width > 4096 || height > 2304 {
                    return Err(format!("Unsupported video frame size: {width}x{height}"));
                }
                let pixels = width
                    .checked_mul(height)
                    .and_then(|p| p.checked_mul(4))
                    .ok_or_else(|| "Invalid video buffer dimensions".to_string())?;
                let mut first_timestamp: Option<i64> = None;
                let mut started_at: Option<Instant> = None;
                let mut first_frame = true;
                let mut paused_at: Option<Instant> = None;
                loop {
                    if control.stop.load(Ordering::Relaxed) {
                        return Ok(());
                    }
                    if !first_frame && !control.playing.load(Ordering::Relaxed) {
                        paused_at.get_or_insert_with(Instant::now);
                        std::thread::sleep(Duration::from_millis(20));
                        continue;
                    }
                    if let Some(pause) = paused_at.take() {
                        if let Some(origin) = started_at.as_mut() {
                            *origin = *origin + pause.elapsed();
                        }
                    }
                    let mut flags = 0u32;
                    let mut sample: Option<IMFSample> = None;
                    let mut time = 0i64;
                    unsafe {
                        reader
                            .ReadSample(
                                stream,
                                0,
                                None,
                                Some(&mut flags),
                                Some(&mut time),
                                Some(&mut sample),
                            )
                            .map_err(|e| format!("H.264 decode failed: {e}"))?;
                    }
                    if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                        break;
                    }
                    if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                        return Err("Video decoder reported an error".into());
                    }
                    let Some(sample) = sample else {
                        continue;
                    };
                    // Worldkiln 0.8.5 | WO 0.8.7-62 | 2026-10-09
                    // Decide whether a video frame is already too late BEFORE
                    // mapping/copying its full RGB buffer. Decode next frame
                    // instead of slowing both video and the game's editor loop.
                    let base = *first_timestamp.get_or_insert(time);
                    let timestamp = Duration::from_nanos((time - base).max(0) as u64 * 100);
                    let now = Instant::now();
                    let started = *started_at.get_or_insert(now);
                    let speed =
                        (control.rate_milli.load(Ordering::Relaxed).max(250) as f64) / 1000.0;
                    let relative = timestamp.div_f64(speed);
                    if is_late_frame(relative, started.elapsed(), first_frame) {
                        continue;
                    }
                    let buffer = unsafe { sample.ConvertToContiguousBuffer() }
                        .map_err(|e| format!("Cannot retrieve decoded frame: {e}"))?;
                    let mut ptr = std::ptr::null_mut::<u8>();
                    let mut len = 0u32;
                    unsafe { buffer.Lock(&mut ptr, None, Some(&mut len)) }
                        .map_err(|e| format!("Cannot lock decoded frame: {e}"))?;
                    let result = (|| -> Result<egui::ColorImage, String> {
                        if ptr.is_null() || (len as usize) < pixels {
                            return Err(format!("Decoded frame shorter than {pixels} bytes"));
                        }
                        let raw = unsafe { std::slice::from_raw_parts(ptr, pixels) };
                        Ok(convert_bgrx_frame(raw, width, height))
                    })();
                    let _ = unsafe { buffer.Unlock() };
                    let image = result?;
                    // Delay only until this sample's presentation deadline.
                    // A late frame is dropped before allocating its texture.
                    if relative > started.elapsed() {
                        std::thread::sleep(
                            (relative - started.elapsed()).min(Duration::from_millis(35)),
                        );
                    }
                    let frame = DecodedVideoFrame {
                        width: image.size[0],
                        height: image.size[1],
                        timestamp,
                        image,
                    };
                    // Never block render or decoder on a slow UI frame.
                    let _ = tx.try_send(Ok(frame));
                    first_frame = false;
                }
                if !control.looped.load(Ordering::Relaxed) {
                    break;
                }
            }
            Ok(())
        })();
        let _ = unsafe { MFShutdown() };
        running
    })();
    unsafe { CoUninitialize() };
    result
}

use std::os::windows::ffi::OsStrExt;

#[cfg(test)]
mod tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-62 | 2026-10-09
    #[test]
    fn cinematic_upload_reduces_1080p_bandwidth_and_preserves_small_video() {
        assert_eq!(video_upload_dimensions(1920, 1080), (1280, 720));
        assert_eq!(video_upload_dimensions(3840, 2160), (1280, 720));
        assert_eq!(video_upload_dimensions(720, 480), (720, 480));
        let raw = vec![32, 64, 192, 0, 8, 16, 24, 0, 1, 2, 3, 0, 4, 5, 6, 0];
        let image = convert_bgrx_frame(&raw, 2, 2);
        assert_eq!(image.size, [2, 2]);
        assert_eq!(image.pixels[0], egui::Color32::from_rgb(192, 64, 32));
        assert_eq!(image.pixels[1], egui::Color32::from_rgb(24, 16, 8));
        assert_eq!(1280 * 720 * 4, 3_686_400);
    }

    #[test]
    fn late_frame_is_dropped_without_suppressing_initial_frame() {
        assert!(!is_late_frame(
            Duration::ZERO,
            Duration::from_secs(10),
            true
        ));
        assert!(is_late_frame(
            Duration::from_millis(100),
            Duration::from_millis(250),
            false
        ));
        assert!(!is_late_frame(
            Duration::from_millis(100),
            Duration::from_millis(120),
            false
        ));
    }

    #[test]
    #[ignore = "Requires WORLDKILN_VIDEO_SAMPLE pointing at an MP4"]
    fn media_foundation_advances_multiple_frames() {
        let file = std::path::PathBuf::from(
            std::env::var_os("WORLDKILN_VIDEO_SAMPLE").expect("Set WORLDKILN_VIDEO_SAMPLE"),
        );
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let control = Arc::new(VideoControl::new());
        control.playing.store(true, Ordering::Relaxed);
        start_video_worker(file, tx, control.clone());
        let mut first = None;
        let mut last = Duration::ZERO;
        for _ in 0..12 {
            let frame = rx
                .recv_timeout(Duration::from_secs(8))
                .expect("Timed out awaiting decoded MP4 frames")
                .expect("Frame decoder error");
            first.get_or_insert(frame.timestamp);
            assert!(frame.timestamp >= last, "Video timestamps regressed");
            last = frame.timestamp;
        }
        assert!(last > first.unwrap(), "No advancement beyond first frame");
        control.stop.store(true, Ordering::Relaxed);
        println!("VIDEO_MULTI_FRAME_PASS last={last:?}");
    }

    // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:38 -05:00
    #[test]
    fn media_foundation_normalizes_canonical_windows_paths() {
        assert_eq!(
            media_foundation_file_path(Path::new(r"\\?\C:\Users\fixit\Video.mp4")).unwrap(),
            r"C:\Users\fixit\Video.mp4"
        );
        assert_eq!(
            media_foundation_file_path(Path::new(r"\\?\UNC\server\share\Video.mp4")).unwrap(),
            r"\\server\share\Video.mp4"
        );
        assert_eq!(
            media_foundation_file_path(Path::new(r"C:\Users\fixit\Video.mp4")).unwrap(),
            r"C:\Users\fixit\Video.mp4"
        );
        assert!(media_foundation_file_path(Path::new(r"\\?\Volume{123}\abc.mp4")).is_err());
        assert!(media_foundation_file_path(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }

    #[test]
    #[ignore = "Requires WORLDKILN_VIDEO_SAMPLE pointing at an MP4"]
    fn media_foundation_decodes_actual_mp4_first_frame() {
        let file = std::path::PathBuf::from(
            std::env::var_os("WORLDKILN_VIDEO_SAMPLE").expect("Set WORLDKILN_VIDEO_SAMPLE"),
        );
        assert!(file.is_file(), "MP4 must exist");
        // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:38 -05:00
        // Reproduce the production route: ui_media::resolve_ui_media_path
        // returns a *canonical* Windows \\?\ path, not a raw input string.
        let file = file.canonicalize().expect("Canonical video source path");
        assert!(file.to_string_lossy().starts_with(r"\\?\"));
        let (tx, rx) = std::sync::mpsc::sync_channel(2);
        let control = Arc::new(VideoControl::new());
        start_video_worker(file, tx, control.clone());
        let frame = rx
            .recv_timeout(Duration::from_secs(25))
            .expect("No decoded MP4 frame in 25 seconds")
            .expect("Decoder failed on sample");
        assert!(frame.width > 80 && frame.height > 80);
        assert_eq!(frame.image.pixels.len(), frame.width * frame.height);
        assert!(frame.image.pixels.iter().all(|pixel| pixel.a() == 255));
        println!(
            "VIDEO_DECODE_PASS {}x{} pixels={} timestamp={:?}",
            frame.width,
            frame.height,
            frame.image.pixels.len(),
            frame.timestamp
        );
        control.stop.store(true, Ordering::Relaxed);
    }
}
