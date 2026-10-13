//! Windows MP4 embedded audio playback for the existing Video UI.
//! WO 0.8.7-61 | 2026-10-09
//! Decode AAC through Windows Media Foundation to 16-bit PCM and feed a
//! bounded rodio sink on a background thread. No external ffmpeg dependency.
#![cfg(windows)]

use std::path::Path;
use std::sync::{Arc, atomic::Ordering};
use std::time::Duration;

use rodio::{OutputStream, Sink, buffer::SamplesBuffer};
use windows::Win32::Media::MediaFoundation::*;
use windows::Win32::System::Com::{COINIT_MULTITHREADED, CoInitializeEx, CoUninitialize};
use windows::core::PCWSTR;

use super::ui_video_windows::{VideoControl, media_foundation_file_path};

struct PcmReader {
    reader: IMFSourceReader,
    stream: u32,
    channels: u16,
    sample_rate: u32,
}

impl PcmReader {
    fn new(path: &Path) -> Result<Self, String> {
        let normalized = media_foundation_file_path(path)?;
        let wide: Vec<u16> = normalized.encode_utf16().chain(Some(0)).collect();
        let reader = unsafe { MFCreateSourceReaderFromURL(PCWSTR(wide.as_ptr()), None) }
            .map_err(|e| format!("Cannot open MP4 audio '{}': {e}", path.display()))?;
        let stream = MF_SOURCE_READER_FIRST_AUDIO_STREAM.0 as u32;
        unsafe {
            reader
                .SetStreamSelection(MF_SOURCE_READER_ALL_STREAMS.0 as u32, false)
                .map_err(|e| format!("Cannot select audio streams: {e}"))?;
            reader
                .SetStreamSelection(stream, true)
                .map_err(|e| format!("MP4 has no playable audio track: {e}"))?;
        }
        let target = unsafe { MFCreateMediaType() }
            .map_err(|e| format!("Cannot create PCM audio type: {e}"))?;
        unsafe {
            target
                .SetGUID(&MF_MT_MAJOR_TYPE, &MFMediaType_Audio)
                .map_err(|e| format!("Cannot request audio type: {e}"))?;
            target
                .SetGUID(&MF_MT_SUBTYPE, &MFAudioFormat_PCM)
                .map_err(|e| format!("Cannot request PCM audio: {e}"))?;
            reader
                .SetCurrentMediaType(stream, None, &target)
                .map_err(|e| format!("MP4 audio PCM decoder unavailable: {e}"))?;
        }
        let format = unsafe { reader.GetCurrentMediaType(stream) }
            .map_err(|e| format!("Cannot query PCM audio properties: {e}"))?;
        let channels = unsafe { format.GetUINT32(&MF_MT_AUDIO_NUM_CHANNELS) }
            .map_err(|e| format!("Cannot get audio channels: {e}"))?;
        let sample_rate = unsafe { format.GetUINT32(&MF_MT_AUDIO_SAMPLES_PER_SECOND) }
            .map_err(|e| format!("Cannot get audio sample rate: {e}"))?;
        let bits = unsafe { format.GetUINT32(&MF_MT_AUDIO_BITS_PER_SAMPLE) }
            .map_err(|e| format!("Cannot get PCM bit depth: {e}"))?;
        if !(1..=8).contains(&channels) || !(8000..=192000).contains(&sample_rate) || bits != 16 {
            return Err(format!(
                "Unsupported video audio: {channels} channels, {sample_rate} Hz, {bits}-bit PCM"
            ));
        }
        Ok(Self {
            reader,
            stream,
            channels: channels as u16,
            sample_rate,
        })
    }

    fn next_samples(&self) -> Result<Option<Vec<f32>>, String> {
        loop {
            let mut flags = 0u32;
            let mut sample = None;
            unsafe {
                self.reader
                    .ReadSample(
                        self.stream,
                        0,
                        None,
                        Some(&mut flags),
                        None,
                        Some(&mut sample),
                    )
                    .map_err(|e| format!("MP4 audio decode failed: {e}"))?;
            }
            if flags & MF_SOURCE_READERF_ENDOFSTREAM.0 as u32 != 0 {
                return Ok(None);
            }
            if flags & MF_SOURCE_READERF_ERROR.0 as u32 != 0 {
                return Err("MP4 audio decoder reported an error".into());
            }
            let Some(sample) = sample else {
                continue;
            };
            let buffer = unsafe { sample.ConvertToContiguousBuffer() }
                .map_err(|e| format!("Cannot read decoded MP4 audio: {e}"))?;
            let mut ptr = std::ptr::null_mut::<u8>();
            let mut len = 0u32;
            unsafe { buffer.Lock(&mut ptr, None, Some(&mut len)) }
                .map_err(|e| format!("Cannot lock decoded PCM audio: {e}"))?;
            let result = (|| -> Result<Vec<f32>, String> {
                if ptr.is_null() || len % 2 != 0 {
                    return Err("MP4 decoder returned invalid 16-bit PCM".into());
                }
                let data = unsafe { std::slice::from_raw_parts(ptr, len as usize) };
                let pcm = data
                    .chunks_exact(2)
                    .map(|b| i16::from_le_bytes([b[0], b[1]]) as f32 / 32768.0)
                    .collect::<Vec<_>>();
                if pcm.len() % self.channels as usize != 0 {
                    return Err("MP4 decoded audio channels were not aligned".into());
                }
                Ok(pcm)
            })();
            let _ = unsafe { buffer.Unlock() };
            let decoded = result?;
            if decoded.is_empty() {
                continue;
            }
            return Ok(Some(decoded));
        }
    }
}

/// Shared VideoControl ensures skip/visibility/playback-rate updates also
/// affect sound. Audio remains bounded (<8 PCM blocks) while video plays.
pub(super) fn start_video_audio_worker(path: std::path::PathBuf, control: Arc<VideoControl>) {
    let _ = std::thread::Builder::new()
        .name("worldkiln-ui-video-audio".into())
        .spawn(move || {
            if let Err(error) = run_audio_worker(&path, &control) {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Media",
                    format!("MP4 soundtrack '{}': {error}", path.display()),
                );
            }
        });
}

fn run_audio_worker(path: &Path, control: &VideoControl) -> Result<(), String> {
    let com = unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) };
    com.ok()
        .map_err(|e| format!("Audio COM initialization failed: {e}"))?;
    let result = (|| -> Result<(), String> {
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }
            .map_err(|e| format!("Media Foundation audio initialization failed: {e}"))?;
        let playback = (|| -> Result<(), String> {
            let (stream, handle) =
                OutputStream::try_default().map_err(|e| format!("No audio output device: {e}"))?;
            let _keep_alive = stream;
            let sink =
                Sink::try_new(&handle).map_err(|e| format!("Cannot open MP4 audio output: {e}"))?;
            while !control.stop.load(Ordering::Relaxed) {
                let pcm = PcmReader::new(path)?;
                let mut ended = false;
                while !control.stop.load(Ordering::Relaxed) {
                    let playing = control.playing.load(Ordering::Relaxed);
                    sink.set_speed(
                        control.rate_milli.load(Ordering::Relaxed).clamp(250, 4000) as f32 / 1000.0,
                    );
                    if playing {
                        sink.play();
                    } else {
                        sink.pause();
                    }
                    if !playing || sink.len() >= 8 {
                        std::thread::sleep(Duration::from_millis(15));
                        continue;
                    }
                    match pcm.next_samples()? {
                        Some(samples) => {
                            sink.append(SamplesBuffer::new(pcm.channels, pcm.sample_rate, samples))
                        }
                        None => {
                            ended = true;
                            break;
                        }
                    }
                }
                if !ended {
                    break;
                }
                // Drain queued PCM so the soundtrack is not cut short at EOF.
                // A skip/stop interrupts this wait and tears down the sink.
                while !control.stop.load(Ordering::Relaxed) && !sink.empty() {
                    let playing = control.playing.load(Ordering::Relaxed);
                    if playing {
                        sink.play();
                    } else {
                        sink.pause();
                    }
                    std::thread::sleep(Duration::from_millis(15));
                }
                if !control.looped.load(Ordering::Relaxed) {
                    break;
                }
            }
            sink.stop();
            Ok(())
        })();
        let _ = unsafe { MFShutdown() };
        playback
    })();
    unsafe { CoUninitialize() };
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    // Uses real Windows Media Foundation but no speaker/audio device.
    #[test]
    #[ignore = "Set WORLDKILN_VIDEO_SAMPLE to an MP4 with an AAC sound track"]
    fn actual_project_mp4_audio_decodes_pcm() {
        let path = std::path::PathBuf::from(
            std::env::var_os("WORLDKILN_VIDEO_SAMPLE").expect("Sample required"),
        );
        let canonical = path.canonicalize().unwrap();
        unsafe { CoInitializeEx(None, COINIT_MULTITHREADED) }
            .ok()
            .unwrap();
        unsafe { MFStartup(MF_VERSION, MFSTARTUP_FULL) }.unwrap();
        let read = (|| -> Result<(), String> {
            let reader = PcmReader::new(&canonical)?;
            let samples = reader
                .next_samples()?
                .ok_or("AAC stream returned no PCM frames")?;
            assert!(reader.channels > 0 && reader.sample_rate > 8000);
            assert!(samples.len() > 100 && samples.len() % reader.channels as usize == 0);
            assert!(samples.iter().all(|sample| sample.is_finite()));
            println!(
                "MP4_PCM_PASS channels={} sample_rate={} samples={}",
                reader.channels,
                reader.sample_rate,
                samples.len()
            );
            Ok(())
        })();
        unsafe { MFShutdown() }.unwrap();
        unsafe { CoUninitialize() };
        read.unwrap();
    }
}
