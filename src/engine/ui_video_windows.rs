// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
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
    pub image: egui::ColorImage,
}
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
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    com.ok()
        .map_err(|e| format!("Video COM initialization failed: {e}"))?;
    let result = (|| -> Result<(), String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|e| format!("Media Foundation initialization failed: {e}"))?;
        let running = (|| -> Result<(), String> {
            let media_url = media_foundation_file_path(path)?;
            let wide: Vec<u16> = OsStr::new(&media_url)
                .encode_wide()
                .chain(Some(0))
                .collect();
            let source = PCWSTR(wide.as_ptr());
            let stream = MF_SOURCE_READER_FIRST_VIDEO_STREAM.0 as u32;
            while !control.stop.load(Ordering::Relaxed) {
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
    fn media_foundation_normalizes_canonical_windows_paths() {
        assert_eq!(
            media_foundation_file_path(Path::new(r"\\?\C:\Media\Video.mp4")).unwrap(),
            r"C:\Media\Video.mp4"
        );
        assert_eq!(
            media_foundation_file_path(Path::new(r"\\?\UNC\server\share\Video.mp4")).unwrap(),
            r"\\server\share\Video.mp4"
        );
        assert_eq!(
            media_foundation_file_path(Path::new(r"C:\Media\Video.mp4")).unwrap(),
            r"C:\Media\Video.mp4"
        );
        assert!(media_foundation_file_path(Path::new(r"\\?\Volume{123}\abc.mp4")).is_err());
        assert!(media_foundation_file_path(Path::new(r"\\.\PhysicalDrive0")).is_err());
    }

}
