// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::character::system::CharacterSystem;
use glam::{Quat, Vec2, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const CHARACTER_BATCH_LIMIT: usize = 8;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterState {
    pub id: u64,
    pub owner: u64,
    pub package: String,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    pub clip: String,
    pub clip_time: f32,
    pub clip_playing: bool,
    pub clip_rate: f32,
}

impl CharacterState {
    pub fn valid(&self) -> bool {
        self.id != 0
            && !self.package.is_empty()
            && self.package.len() <= 200
            && !self.clip.is_empty()
            && self.clip.len() <= 160
            && self.clip_time.is_finite()
            && self.clip_time >= 0.0
            && self.clip_rate.is_finite()
            && (0.0..=8.0).contains(&self.clip_rate)
            && self
                .position
                .iter()
                .chain(&self.velocity)
                .all(|value| value.is_finite())
            && self.rotation.iter().all(|value| value.is_finite())
            && (0.98..=1.02).contains(&self.rotation.iter().map(|v| v * v).sum::<f32>())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CharacterBatch {
    pub epoch: u64,
    pub offset: u32,
    pub total: u32,
    pub characters: Vec<CharacterState>,
}

impl CharacterBatch {
    pub fn valid(&self) -> bool {
        self.epoch > 0
            && self.characters.len() <= CHARACTER_BATCH_LIMIT
            && self.offset as u64 + self.characters.len() as u64 <= self.total as u64
            && (self.total == 0 || !self.characters.is_empty())
            && self.characters.iter().all(CharacterState::valid)
            && self.characters.windows(2).all(|p| p[0].id < p[1].id)
    }
}

pub const NPC_NAMESPACE: u64 = 1u64 << 63;

#[derive(Default)]
pub struct CharacterReplicator {
    epoch: u64,
    offset: usize,
    pending_epoch: u64,
    pending_offset: usize,
    completed_epoch: u64,
    pending_ids: BTreeSet<u64>,
    known_ids: BTreeSet<u64>,
    replicas: HashMap<u64, u64>,
    created_ids: BTreeSet<u64>,
    original_collision: HashMap<u64, (bool, bool)>,
}

impl CharacterReplicator {
    pub fn snapshot(&mut self, system: &CharacterSystem, owners: &[(u64, u64)]) -> CharacterBatch {
        let mut entries = BTreeMap::new();
        for character in system.get_active_characters() {
            let owner = owners
                .iter()
                .find(|(_, id)| *id == character.id)
                .map(|(p, _)| *p);
            let id = owner.unwrap_or(NPC_NAMESPACE | character.id);
            entries.insert(
                id,
                CharacterState {
                    id,
                    owner: owner.unwrap_or(0),
                    package: character.package_name.clone(),
                    position: character.transform.position.to_array(),
                    rotation: character.transform.rotation.to_array(),
                    velocity: character.movement.velocity.to_array(),
                    clip: character
                        .animation_controller
                        .current_clip_name()
                        .to_owned(),
                    clip_time: character.animation_controller.playback_time(),
                    clip_playing: character.animation_controller.is_playing(),
                    clip_rate: character.animation_controller.playback_rate(),
                },
            );
        }
        let states: Vec<_> = entries.into_values().collect();
        let total = states.len();
        if self.offset >= total {
            self.offset = 0;
        }
        let from = self.offset;
        let to = (from + CHARACTER_BATCH_LIMIT).min(total);
        let batch = CharacterBatch {
            epoch: self.epoch + 1,
            offset: from as u32,
            total: total as u32,
            characters: states[from..to].to_vec(),
        };
        if to == total {
            self.offset = 0;
            self.epoch += 1;
        } else {
            self.offset = to;
        }
        batch
    }
    pub fn apply(
        &mut self,
        system: &mut CharacterSystem,
        local_player: u64,
        batch: CharacterBatch,
    ) -> bool {
        if !batch.valid() || batch.epoch < self.pending_epoch || batch.epoch <= self.completed_epoch
        {
            return false;
        }
        if batch.epoch != self.pending_epoch {
            if batch.offset != 0 {
                return false;
            }
            self.pending_epoch = batch.epoch;
            self.pending_offset = 0;
            self.pending_ids.clear();
        }
        if batch.offset as usize != self.pending_offset {
            return false;
        }
        for state in &batch.characters {
            self.pending_ids.insert(state.id);
            if state.id == local_player {
                continue;
            }
            let local = if let Some(local) = self.replicas.get(&state.id).copied() {
                local
            } else {
                let native = if state.id & NPC_NAMESPACE != 0 {
                    let native_id = state.id & !NPC_NAMESPACE;
                    system
                        .get_active_characters()
                        .find(|c| c.id == native_id)
                        .map(|c| c.id)
                } else {
                    None
                };
                let local = native.unwrap_or_else(|| {
                    let character = system
                        .spawn_character(Vec3::from_array(state.position), Some(&state.package));
                    self.created_ids.insert(state.id);
                    character
                });
                self.replicas.insert(state.id, local);
                local
            };
            if let Some(remote) = system.get_active_characters_mut().find(|c| c.id == local) {
                if !self.created_ids.contains(&state.id) {
                    self.original_collision
                        .entry(local)
                        .or_insert((remote.collision_enabled, remote.collision_solid));
                }
                let target = Vec3::from_array(state.position);

                remote.transform.position = target;
                remote.transform.rotation = Quat::from_array(state.rotation);
                remote.movement.velocity = Vec3::from_array(state.velocity);
                remote.facing = {
                    let forward = remote.transform.rotation * Vec3::Z;
                    Vec2::new(forward.x, forward.z).normalize_or_zero()
                };
                remote.animation_controller.sync_network_playback(
                    &state.clip,
                    state.clip_time,
                    state.clip_playing,
                    state.clip_rate,
                );
                remote.animation_override = Some(
                    crate::character::animation::TargetAnimation::Named(state.clip.clone()),
                );
                remote.collision_enabled = false;
                remote.collision_solid = false;
            }
        }
        self.pending_offset += batch.characters.len();
        if self.pending_offset == batch.total as usize {
            let absent: Vec<_> = self
                .known_ids
                .difference(&self.pending_ids)
                .copied()
                .collect();
            for id in absent {
                if let Some(local) = self.replicas.remove(&id) {
                    if self.created_ids.remove(&id) {
                        system.despawn_network_character(local);
                    } else if let Some(native) =
                        system.get_active_characters_mut().find(|c| c.id == local)
                    {
                        native.collision_solid = false;
                        native.collision_enabled = false;
                    }
                }
            }
            self.known_ids = std::mem::take(&mut self.pending_ids);
            self.completed_epoch = batch.epoch;
        }
        true
    }
    pub fn release_guest_replicas(&mut self, system: &mut CharacterSystem) {
        for (id, local) in self.replicas.drain() {
            if self.created_ids.contains(&id) {
                system.despawn_network_character(local);
            }
        }
        for (local, (enabled, solid)) in self.original_collision.drain() {
            if let Some(character) = system.get_active_characters_mut().find(|c| c.id == local) {
                character.collision_enabled = enabled;
                character.collision_solid = solid;
                character.animation_override = None;
            }
        }
        self.created_ids.clear();
        self.known_ids.clear();
        self.pending_ids.clear();
    }

    pub fn known_count(&self) -> usize {
        self.known_ids.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn state(id: u64) -> CharacterState {
        CharacterState {
            id,
            owner: 1,
            package: "KilnCharacter".into(),
            position: [0.0, 1.0, 0.0],
            rotation: [0.0, 0.0, 0.0, 1.0],
            velocity: [0.0; 3],
            clip: "Idle".into(),
            clip_time: 0.0,
            clip_playing: true,
            clip_rate: 1.0,
        }
    }
    #[test]
    fn validates_identity_order_and_finite_character_transforms() {
        let batch = CharacterBatch {
            epoch: 1,
            offset: 0,
            total: 2,
            characters: vec![state(1), state(2)],
        };
        assert!(batch.valid());
        let mut duplicate = batch.clone();
        duplicate.characters[1].id = 1;
        assert!(!duplicate.valid());
        let mut invalid = batch.clone();
        invalid.characters[0].position[0] = f32::NAN;
        assert!(!invalid.valid());
        let mut stale = batch;
        stale.offset = 1;
        assert!(!stale.valid());
    }
}
