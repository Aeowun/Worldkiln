// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use rodio::{Decoder, OutputStream, OutputStreamHandle, Sink, Source};
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::path::PathBuf;
use std::time::{Duration, Instant};

use crate::world::{CellType, World};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EngineSound {
    Click,
    Select,
    Toggle,
    Confirm,

    ProjectOpened,
    ProjectCreated,

    PlayStarted,
    PlayStopped,

    BuildStarted,
    BuildComplete,
    BuildFailed,

    Rejected,
    Warning,
    CriticalAlert,
}

impl EngineSound {
    pub const ALL: &'static [Self] = &[
        Self::Click,
        Self::Select,
        Self::Toggle,
        Self::Confirm,
        Self::ProjectOpened,
        Self::ProjectCreated,
        Self::PlayStarted,
        Self::PlayStopped,
        Self::BuildStarted,
        Self::BuildComplete,
        Self::BuildFailed,
        Self::Rejected,
        Self::Warning,
        Self::CriticalAlert,
    ];

    pub const INTERFACE: &'static [Self] =
        &[Self::Click, Self::Select, Self::Toggle, Self::Confirm];

    pub const PROJECT: &'static [Self] = &[Self::ProjectOpened, Self::ProjectCreated];

    pub const PLAY_MODE: &'static [Self] = &[Self::PlayStarted, Self::PlayStopped];

    pub const BUILD: &'static [Self] =
        &[Self::BuildStarted, Self::BuildComplete, Self::BuildFailed];

    pub const NOTIFICATIONS: &'static [Self] =
        &[Self::Rejected, Self::Warning, Self::CriticalAlert];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Click => "UI Click",
            Self::Select => "Selection",
            Self::Toggle => "Toggle",
            Self::Confirm => "Confirmation",

            Self::ProjectOpened => "Project Opened",
            Self::ProjectCreated => "Project Created",

            Self::PlayStarted => "Play Started",
            Self::PlayStopped => "Play Stopped",

            Self::BuildStarted => "Build Started",
            Self::BuildComplete => "Build Complete",
            Self::BuildFailed => "Build Failed",

            Self::Rejected => "Rejected Action",
            Self::Warning => "Warning",
            Self::CriticalAlert => "Critical Alert",
        }
    }

    pub const fn file_name(self) -> &'static str {
        match self {
            Self::Click => "new_click.wav",
            Self::Select => "new_click.wav",
            Self::Toggle => "done.wav",
            Self::Confirm => "pop.wav",

            Self::ProjectOpened => "open.wav",
            Self::ProjectCreated => "open.wav",

            Self::PlayStarted => "click1.wav",
            Self::PlayStopped => "click1.wav",

            Self::BuildStarted => "notification.wav",
            Self::BuildComplete => "build_complete.wav",
            Self::BuildFailed => "notification.wav",

            Self::Rejected => "notification.wav",
            Self::Warning => "negative_sound2.wav",
            Self::CriticalAlert => "notification.wav",
        }
    }

    pub const fn is_ui_sound(self) -> bool {
        matches!(
            self,
            Self::Click | Self::Select | Self::Toggle | Self::Confirm
        )
    }
}

#[derive(Clone, Copy, Debug)]
pub struct EngineAudioSettings {
    pub engine_sounds_enabled: bool,
    pub ui_sounds_enabled: bool,
    pub engine_volume: f32,
    pub ui_volume: f32,
}

impl Default for EngineAudioSettings {
    fn default() -> Self {
        Self {
            engine_sounds_enabled: true,
            ui_sounds_enabled: true,
            engine_volume: 1.0,
            ui_volume: 1.0,
        }
    }
}

pub struct ActiveEmitterState {
    pub position: glam::Vec3,
    pub sound_path: String,
    pub is_playing: bool,
    pub is_paused: bool,
    pub is_looped: bool,
    pub loop_interval: f32,
    pub restart_after: Option<Instant>,
    pub volume: f32,
    pub min_distance: f32,
    pub max_distance: f32,
    pub pitch: f32,
    pub play_serial: u64,
    pub sink: Option<Sink>,
}

pub struct AudioSystem {
    _stream: Option<OutputStream>,
    stream_handle: Option<OutputStreamHandle>,
    emitters: HashMap<u64, ActiveEmitterState>,
    erosion_tone: Option<Sink>,
    last_erosion_tone: Option<Instant>,
}

fn loop_uses_native_repeat(looped: bool, loop_interval: f32) -> bool {
    looped && loop_interval <= 0.0
}

fn emitter_position(world: &World, id: u64) -> glam::Vec3 {
    world
        .source_position(id)
        .or_else(|| world.runtime_audio_position(id))
        .or_else(|| {
            world.resolve_cell_id(id).map(|coord| {
                glam::Vec3::new(
                    coord.x as f32 + 0.5,
                    coord.y as f32 + 0.5,
                    coord.z as f32 + 0.5,
                )
            })
        })
        .unwrap_or(glam::Vec3::ZERO)
}

fn distance_attenuation(distance: f32, min_distance: f32, max_distance: f32) -> f32 {
    let min_distance = min_distance.max(0.0);
    let max_distance = max_distance.max(min_distance);
    if distance <= min_distance {
        1.0
    } else if distance >= max_distance || max_distance <= min_distance {
        0.0
    } else {
        1.0 - (distance - min_distance) / (max_distance - min_distance)
    }
}

impl AudioSystem {
    #[cfg(test)]
    pub(crate) fn new_for_tests() -> Self {
        Self {
            _stream: None,
            stream_handle: None,
            emitters: HashMap::new(),
            erosion_tone: None,
            last_erosion_tone: None,
        }
    }

    pub fn new() -> Self {
        let (stream, handle) = match OutputStream::try_default() {
            Ok((s, h)) => (Some(s), Some(h)),
            Err(e) => {
                crate::engine::diagnostics::report(
                    "WARNING",
                    "Audio",
                    format!(
                        "No audio output device available ({e}); running in silent mode. Check the system output device."
                    ),
                );
                (None, None)
            }
        };

        Self {
            _stream: stream,
            stream_handle: handle,
            emitters: HashMap::new(),
            erosion_tone: None,
            last_erosion_tone: None,
        }
    }

    pub fn update(
        &mut self,
        world: &World,
        project_path: &Option<PathBuf>,
        listener_position: glam::Vec3,
    ) {
        let active_cell_ids: HashSet<u64> = world.iter_audio_emitter_ids().collect();

        for id in active_cell_ids.iter().copied() {
            if let Some(cell) = world.get_effective_cell_by_id(id) {
                if cell.cell_type == CellType::AudioEmitter {
                    let position = emitter_position(world, id);
                    let playing = world.is_audio_playing(id);
                    let paused = world.is_audio_paused(id);
                    let looped = world.is_audio_looped(id);
                    let loop_interval = world.get_audio_loop_interval(id);
                    let volume = world.get_audio_volume(id);
                    let pitch = world.get_audio_pitch(id);
                    let play_serial = world.get_audio_play_serial(id);
                    let min_distance = world.get_audio_min_distance(id);
                    let max_distance = world.get_audio_max_distance(id);
                    let audible_volume = volume
                        * distance_attenuation(
                            position.distance(listener_position),
                            min_distance,
                            max_distance,
                        );
                    let audio_rel_path = world.get_audio_path(id);

                    if playing && !paused {
                        let need_start = match self.emitters.get(&id) {
                            Some(state) => {
                                !state.is_playing
                                    || state.play_serial != play_serial
                                    || state.sound_path != audio_rel_path
                                    || state.is_looped != looped
                                    || (state.loop_interval - loop_interval).abs() > 1e-4
                            }
                            None => true,
                        };

                        if need_start {
                            if let Some(old_state) = self.emitters.remove(&id) {
                                if let Some(sink) = old_state.sink {
                                    sink.stop();
                                }
                            }

                            let sink_opt = match self.start_emitter(
                                &audio_rel_path,
                                project_path,
                                audible_volume,
                                pitch,
                                loop_uses_native_repeat(looped, loop_interval),
                            ) {
                                Ok(sink) => Some(sink),
                                Err(error) => {
                                    crate::engine::diagnostics::report(
                                        "ERROR",
                                        "Audio",
                                        format!(
                                            "AudioEmitter {id}, property Audio Path '{audio_rel_path}': {error}"
                                        ),
                                    );
                                    None
                                }
                            };

                            self.emitters.insert(
                                id,
                                ActiveEmitterState {
                                    position,
                                    sound_path: audio_rel_path.clone(),
                                    is_playing: true,
                                    is_paused: false,
                                    is_looped: looped,
                                    loop_interval,
                                    restart_after: None,
                                    volume,
                                    min_distance,
                                    max_distance,
                                    pitch,
                                    play_serial,
                                    sink: sink_opt,
                                },
                            );
                        } else if let Some(state) = self.emitters.get_mut(&id) {
                            state.position = position;
                            state.volume = volume;
                            if (state.pitch - pitch).abs() > 1e-4 {
                                if let Some(ref sink) = state.sink {
                                    sink.set_speed(pitch);
                                }
                                state.pitch = pitch;
                            }
                            state.min_distance = min_distance;
                            state.max_distance = max_distance;
                            if let Some(ref sink) = state.sink {
                                sink.set_volume(audible_volume);
                            }
                            if state.is_paused {
                                state.is_paused = false;
                                if let Some(ref sink) = state.sink {
                                    sink.play();
                                }
                            }

                            if state.is_looped && state.loop_interval > 0.0 {
                                let finished = state
                                    .sink
                                    .as_ref()
                                    .map(|sink| sink.empty())
                                    .unwrap_or(false);

                                if finished {
                                    state.restart_after.get_or_insert_with(|| {
                                        Instant::now()
                                            + Duration::from_secs_f32(state.loop_interval)
                                    });
                                } else {
                                    state.restart_after = None;
                                }
                            }
                        }
                    } else if paused && playing {
                        if let Some(state) = self.emitters.get_mut(&id) {
                            if !state.is_paused {
                                state.is_paused = true;
                                if let Some(ref sink) = state.sink {
                                    sink.pause();
                                }
                            }
                        }
                    } else {
                        if let Some(state) = self.emitters.remove(&id) {
                            if let Some(sink) = state.sink {
                                sink.stop();
                            }
                        }
                    }

                    let restart_due = self.emitters.get(&id).is_some_and(|state| {
                        state.is_playing
                            && !state.is_paused
                            && state.is_looped
                            && state.loop_interval > 0.0
                            && state
                                .restart_after
                                .is_some_and(|deadline| Instant::now() >= deadline)
                    });

                    if restart_due {
                        if let Some(state) = self.emitters.remove(&id) {
                            if let Some(sink) = state.sink {
                                sink.stop();
                            }
                        }

                        let sink_opt = match self.start_emitter(
                            &audio_rel_path,
                            project_path,
                            audible_volume,
                            pitch,
                            false,
                        ) {
                            Ok(sink) => Some(sink),
                            Err(error) => {
                                crate::engine::diagnostics::report(
                                    "ERROR",
                                    "Audio",
                                    format!(
                                        "AudioEmitter {id}, property Audio Path '{audio_rel_path}': {error}"
                                    ),
                                );
                                None
                            }
                        };

                        self.emitters.insert(
                            id,
                            ActiveEmitterState {
                                position,
                                sound_path: audio_rel_path,
                                is_playing: true,
                                is_paused: false,
                                is_looped: looped,
                                loop_interval,
                                restart_after: None,
                                volume,
                                min_distance,
                                max_distance,
                                pitch,
                                play_serial,
                                sink: sink_opt,
                            },
                        );
                    }
                }
            }
        }

        let to_remove: Vec<u64> = self
            .emitters
            .keys()
            .cloned()
            .filter(|id| !active_cell_ids.contains(id))
            .collect();

        for id in to_remove {
            if let Some(state) = self.emitters.remove(&id) {
                if let Some(sink) = state.sink {
                    sink.stop();
                }
            }
        }
    }

    fn start_emitter(
        &self,
        rel_path: &str,
        project_path: &Option<PathBuf>,
        volume: f32,
        pitch: f32,
        looped: bool,
    ) -> Result<Sink, String> {
        let path = resolve_audio_file_path(rel_path, project_path).ok_or_else(||
            "audio file not found; choose an existing file under the project or built-in .assets/audio folder".to_string())?;
        let file = File::open(&path).map_err(|e| format!("cannot open {}: {e}", path.display()))?;
        let source = Decoder::new(BufReader::new(file))
            .map_err(|e| format!("cannot decode {}: {e}", path.display()))?;
        let handle = self.stream_handle.as_ref().ok_or_else(|| {
            "audio output unavailable; check the system output device".to_string()
        })?;
        let sink = Sink::try_new(handle).map_err(|e| format!("cannot start playback: {e}"))?;
        sink.set_volume(volume);
        sink.set_speed(pitch);
        if looped {
            sink.append(source.repeat_infinite());
        } else {
            sink.append(source);
        }
        sink.play();
        Ok(sink)
    }

    pub fn stop_all(&mut self) {
        for (_, state) in self.emitters.drain() {
            if let Some(sink) = state.sink {
                sink.stop();
            }
        }
    }

    pub(crate) fn play_erosion_edit(&mut self, magnitude: f32, settings: &EngineAudioSettings) {
        if !settings.engine_sounds_enabled || !magnitude.is_finite() || magnitude <= 0.0 {
            return;
        }
        let volume = settings.engine_volume.clamp(0.0, 1.0) * settings.ui_volume.clamp(0.0, 1.0);
        if volume <= 0.0 {
            return;
        }
        let now = Instant::now();
        if self
            .last_erosion_tone
            .is_some_and(|last| now.duration_since(last) < Duration::from_millis(125))
            || self.erosion_tone.as_ref().is_some_and(|sink| !sink.empty())
        {
            return;
        }
        let Some(handle) = self.stream_handle.as_ref() else {
            return;
        };
        let Ok(sink) = Sink::try_new(handle) else {
            return;
        };
        let frequency = 220.0 + 660.0 * (magnitude.ln_1p() / 5.0).clamp(0.0, 1.0);
        const SAMPLE_RATE: u32 = 22_050;
        const SAMPLES: usize = 882;
        let samples: Vec<f32> = (0..SAMPLES)
            .map(|i| {
                let t = i as f32 / SAMPLE_RATE as f32;
                let envelope = (std::f32::consts::PI * i as f32 / SAMPLES as f32).sin();
                (std::f32::consts::TAU * frequency * t).sin() * envelope * 0.08
            })
            .collect();
        sink.set_volume(volume);
        sink.append(rodio::buffer::SamplesBuffer::new(1, SAMPLE_RATE, samples));
        sink.play();
        self.erosion_tone = Some(sink);
        self.last_erosion_tone = Some(now);
    }
    pub fn play_engine_sound(&self, sound: EngineSound, settings: &EngineAudioSettings) {
        if !settings.engine_sounds_enabled {
            return;
        }

        if sound.is_ui_sound() && !settings.ui_sounds_enabled {
            return;
        }

        let Some(handle) = self.stream_handle.as_ref() else {
            return;
        };

        let path = crate::project::engine_assets_dir()
            .join("engine")
            .join(sound.file_name());

        let Ok(file) = File::open(&path) else {
            crate::engine::diagnostics::report(
                "WARNING",
                "Audio",
                format!("[AUDIO] Engine sound not found: {}", path.display()),
            );
            return;
        };

        let Ok(source) = Decoder::new(BufReader::new(file)) else {
            crate::engine::diagnostics::report(
                "WARNING",
                "Audio",
                format!("[AUDIO] Failed to decode engine sound: {}", path.display()),
            );
            return;
        };

        let sink = match Sink::try_new(handle) {
            Ok(sink) => sink,
            Err(error) => {
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Audio",
                    format!("Cannot play engine sound '{}': {error}", path.display()),
                );
                return;
            }
        };

        let volume = if sound.is_ui_sound() {
            settings.engine_volume.clamp(0.0, 1.0) * settings.ui_volume.clamp(0.0, 1.0)
        } else {
            settings.engine_volume.clamp(0.0, 1.0)
        };

        if volume <= 0.0 {
            return;
        }

        sink.set_volume(volume);
        sink.append(source);
        sink.play();

        sink.detach();
    }
}

fn safe_audio_asset_path(path: &str) -> Option<&std::path::Path> {
    let candidate = std::path::Path::new(path);
    if path.trim().is_empty()
        || !candidate
            .components()
            .all(|part| matches!(part, std::path::Component::Normal(_)))
        || path.contains('\\')
    {
        return None;
    }
    Some(candidate)
}

pub fn resolve_audio_file_path(rel_path: &str, project_path: &Option<PathBuf>) -> Option<PathBuf> {
    let rel_path = safe_audio_asset_path(rel_path)?;

    if let Some(proj) = project_path {
        let proj_file = proj.join(".assets").join("audio").join(rel_path);
        if proj_file.exists() {
            return Some(proj_file);
        }
    }

    let builtin_file = crate::project::engine_assets_dir()
        .join("audio")
        .join(rel_path);
    if builtin_file.exists() {
        return Some(builtin_file);
    }

    None
}

#[cfg(all(test, feature = "editor"))]
mod tests {
    use super::*;

    #[test]
    fn audio_resolver_rejects_absolute_and_traversal_paths_without_touching_other_assets() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_mat025_audio_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let audio_dir = root.join(".assets").join("audio");
        std::fs::create_dir_all(audio_dir.join("nested")).unwrap();
        std::fs::write(audio_dir.join("nested").join("safe.wav"), b"RIFF").unwrap();
        std::fs::write(root.join("secret.wav"), b"secret").unwrap();
        let project = Some(root.clone());
        assert_eq!(
            resolve_audio_file_path("nested/safe.wav", &project),
            Some(audio_dir.join("nested").join("safe.wav"))
        );
        for forbidden in [
            "../secret.wav",
            "nested/../../secret.wav",
            "/secret.wav",
            "C:\\Windows\\system.wav",
            "..\\secret.wav",
            "./nested/safe.wav",
            "",
        ] {
            assert_eq!(
                resolve_audio_file_path(forbidden, &project),
                None,
                "must refuse Audio Path {forbidden:?}"
            );
        }
        assert_eq!(std::fs::read(root.join("secret.wav")).unwrap(), b"secret");
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn loop_interval_selects_native_repeat_only_for_zero_delay() {
        assert!(!loop_uses_native_repeat(false, 0.0));
        assert!(loop_uses_native_repeat(true, 0.0));
        assert!(!loop_uses_native_repeat(true, 0.25));
    }

    #[test]
    fn emitter_distance_attenuation_is_full_near_and_silent_past_max() {
        assert_eq!(distance_attenuation(5.0, 10.0, 100.0), 1.0);
        assert!((distance_attenuation(55.0, 10.0, 100.0) - 0.5).abs() < 1e-6);
        assert_eq!(distance_attenuation(100.0, 10.0, 100.0), 0.0);
        assert_eq!(distance_attenuation(1000.0, 10.0, 100.0), 0.0);
    }

    #[test]
    fn audio_emitter_uses_exact_freeform_source_position() {
        let mut world = World::new();
        let position = glam::Vec3::new(2.5, 1.0, 55.0);
        let id = world.add_source(CellType::AudioEmitter, position);
        assert_eq!(emitter_position(&world, id), position);
    }

    #[test]
    fn sound_pitch_validates_range_and_each_play_request_retriggers() {
        let mut world = World::new();
        let id = world.create_runtime_cell(CellType::AudioEmitter);
        assert_eq!(world.get_audio_pitch(id), 1.0);
        assert_eq!(world.get_audio_play_serial(id), 0);

        world.set_audio_pitch_runtime(id, 2.0).unwrap();
        assert_eq!(world.get_audio_pitch(id), 2.0);
        assert!(world.set_audio_pitch_runtime(id, 0.0).is_err());
        assert!(world.set_audio_pitch_runtime(id, f32::NAN).is_err());
        assert_eq!(world.get_audio_pitch(id), 2.0);

        world.audio_play_runtime(id);
        world.audio_stop_runtime(id);
        world.audio_play_runtime(id);
        assert!(world.is_audio_playing(id));
        assert_eq!(world.get_audio_play_serial(id), 2);

        let mut audio = AudioSystem::new_for_tests();
        audio.update(&world, &None, glam::Vec3::ZERO);
        assert_eq!(audio.emitters.get(&id).unwrap().play_serial, 2);
        assert_eq!(audio.emitters.get(&id).unwrap().pitch, 2.0);

        world.audio_play_runtime(id);
        audio.update(&world, &None, glam::Vec3::ZERO);
        assert_eq!(audio.emitters.get(&id).unwrap().play_serial, 3);

        world.set_audio_pitch_runtime(id, 1.5).unwrap();
        audio.update(&world, &None, glam::Vec3::ZERO);
        assert_eq!(audio.emitters.get(&id).unwrap().pitch, 1.5);
        assert_eq!(audio.emitters.get(&id).unwrap().play_serial, 3);

        world.audio_pause_runtime(id);
        audio.update(&world, &None, glam::Vec3::ZERO);
        assert!(audio.emitters.get(&id).unwrap().is_paused);
        world.audio_play_runtime(id);
        assert_eq!(
            world.get_audio_play_serial(id),
            3,
            "resume must not retrigger"
        );
        audio.update(&world, &None, glam::Vec3::ZERO);
        assert!(!audio.emitters.get(&id).unwrap().is_paused);
        assert_eq!(audio.emitters.get(&id).unwrap().play_serial, 3);
    }

    #[test]
    fn missing_and_corrupt_audio_produce_one_actionable_diagnostic_per_failure() {
        crate::engine::diagnostics::clear();
        let root = std::env::temp_dir().join(format!(
            "worldkiln_audio_diagnostics_{}",
            std::process::id()
        ));
        std::fs::create_dir_all(root.join(".assets/audio")).unwrap();
        std::fs::write(root.join(".assets/audio/broken.wav"), b"not audio").unwrap();
        let mut system = AudioSystem {
            _stream: None,
            stream_handle: None,
            emitters: HashMap::new(),
            erosion_tone: None,
            last_erosion_tone: None,
        };
        let mut world = World::new();
        let coord = crate::world::WorldCoord::new(0, 0, 0);
        let id = world.set_cell(coord, CellType::AudioEmitter);
        let cell = world.get_mut(coord).unwrap();
        cell.audio = "missing.wav".into();
        cell.playing = true;
        let mut editor = crate::editor::Editor::new();
        editor.script_editor.show_output = false;
        for _ in 0..60 {
            system.update(&world, &Some(root.clone()), glam::Vec3::ZERO);
        }
        editor.collect_diagnostics();
        assert!(editor.script_editor.show_output);
        assert_eq!(
            editor
                .terminal_output
                .matches("audio file not found")
                .count(),
            1
        );
        assert!(editor.terminal_output.contains(&format!(
            "[ERROR] [Audio] AudioEmitter {id}, property Audio Path 'missing.wav'"
        )));
        world.get_mut(coord).unwrap().audio = "broken.wav".into();
        system.update(&world, &Some(root.clone()), glam::Vec3::ZERO);
        editor.collect_diagnostics();
        assert!(editor.terminal_output.contains("cannot decode"));
        assert!(editor.terminal_output.contains("broken.wav"));
        system.stop_all();
        assert!(editor.terminal_output.contains("audio file not found"));
        std::fs::remove_dir_all(root).unwrap();
    }
}
