// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use super::ScriptHostBridge;

impl<'a> ScriptHostBridge<'a> {
    pub fn is_audio_playing(&self, id: u64) -> Option<bool> {
        Some(self.world.is_audio_playing(id))
    }

    pub fn is_audio_paused(&self, id: u64) -> Option<bool> {
        Some(self.world.is_audio_paused(id))
    }

    pub fn is_audio_looped(&self, id: u64) -> Option<bool> {
        Some(self.world.is_audio_looped(id))
    }

    pub fn get_audio_volume(&self, id: u64) -> Option<f32> {
        Some(self.world.get_audio_volume(id))
    }

    pub fn get_audio_min_distance(&self, id: u64) -> Option<f32> {
        Some(self.world.get_audio_min_distance(id))
    }

    pub fn get_audio_max_distance(&self, id: u64) -> Option<f32> {
        Some(self.world.get_audio_max_distance(id))
    }

    pub fn set_audio_playing(&mut self, id: u64, playing: bool) {
        self.world.set_audio_playing_runtime(id, playing);
    }

    pub fn set_audio_looped(&mut self, id: u64, looped: bool) {
        self.world.set_audio_looped_runtime(id, looped);
    }

    pub fn set_audio_volume(&mut self, id: u64, volume: f32) {
        self.world.set_audio_volume_runtime(id, volume);
    }

    pub fn set_audio_min_distance(&mut self, id: u64, distance: f32) {
        self.world.set_audio_min_distance_runtime(id, distance);
    }

    pub fn set_audio_max_distance(&mut self, id: u64, distance: f32) {
        self.world.set_audio_max_distance_runtime(id, distance);
    }

    pub fn audio_play(&mut self, id: u64) {
        self.world.audio_play_runtime(id);
    }

    pub fn audio_stop(&mut self, id: u64) {
        self.world.audio_stop_runtime(id);
    }

    pub fn audio_pause(&mut self, id: u64) {
        self.world.audio_pause_runtime(id);
    }
}
