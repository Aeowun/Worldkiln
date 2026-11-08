// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{AnimationAsset, AnimationEventOccurrence, events_crossed};

#[derive(Clone, Debug)]
pub struct AnimationPlayer {
    absolute_time: f64,
    playing: bool,
    playback_rate: f32,
}

#[derive(Clone, Debug)]
pub struct AnimationPlaybackUpdate<'a> {
    pub previous_time: f64,
    pub current_time: f64,
    pub sample_time: f32,
    pub events: Vec<AnimationEventOccurrence<'a>>,
}

impl Default for AnimationPlayer {
    fn default() -> Self {
        Self {
            absolute_time: 0.0,
            playing: false,
            playback_rate: 1.0,
        }
    }
}

impl AnimationPlayer {
    pub fn play(&mut self) {
        self.playing = true;
    }
    pub fn pause(&mut self) {
        self.playing = false;
    }
    pub fn is_playing(&self) -> bool {
        self.playing
    }
    pub fn playback_rate(&self) -> f32 {
        self.playback_rate
    }
    pub fn set_playback_rate(&mut self, rate: f32) {
        self.playback_rate = rate.max(0.0);
    }
    pub fn absolute_time(&self) -> f64 {
        self.absolute_time
    }

    pub fn seek(&mut self, time: f64) {
        self.absolute_time = time.max(0.0);
    }

    pub fn update<'a>(
        &mut self,
        asset: &'a AnimationAsset,
        delta_seconds: f32,
    ) -> AnimationPlaybackUpdate<'a> {
        let previous_time = self.absolute_time;
        if self.playing && delta_seconds.is_finite() && delta_seconds > 0.0 {
            self.absolute_time += f64::from(delta_seconds * self.playback_rate);
            if !asset.looping && self.absolute_time >= f64::from(asset.duration) {
                self.absolute_time = f64::from(asset.duration);
                self.playing = false;
            }
        }
        let current_time = self.absolute_time;
        let sample_time = if asset.looping && asset.duration > 0.0 {
            current_time.rem_euclid(f64::from(asset.duration)) as f32
        } else {
            current_time.clamp(0.0, f64::from(asset.duration)) as f32
        };
        let events = if self.playing || current_time > previous_time {
            events_crossed(asset, previous_time, current_time)
        } else {
            Vec::new()
        };
        AnimationPlaybackUpdate {
            previous_time,
            current_time,
            sample_time,
            events,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::animation::{AnimationEvent, AnimationSource};
    use std::collections::BTreeMap;

    #[test]
    fn player_dispatches_events_across_a_loop_and_stops_non_looping_clips() {
        let mut asset = AnimationAsset {
            version: 1,
            id: "clip".into(),
            name: "Clip".into(),
            source: AnimationSource {
                rig_asset_id: "rig".into(),
                imported_clip: None,
            },
            duration: 1.0,
            frames_per_second: 30.0,
            looping: true,
            root_motion: Default::default(),
            tracks: Vec::new(),
            events: vec![AnimationEvent {
                id: "event".into(),
                time: 0.2,
                name: "step".into(),
                payload: BTreeMap::new(),
            }],
        };
        let mut player = AnimationPlayer::default();
        player.seek(0.9);
        player.play();
        let update = player.update(&asset, 0.4);
        assert_eq!(update.events.len(), 1);
        assert!((update.sample_time - 0.3).abs() < 0.0001);

        asset.looping = false;
        player.seek(0.9);
        player.play();
        let update = player.update(&asset, 0.4);
        assert_eq!(update.current_time, 1.0);
        assert!(!player.is_playing());
    }
}
