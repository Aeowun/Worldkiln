// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::actor::{ActorSystem, RuntimeActor};
use crate::engine::entity::{EntityId, EntityManager};
use crate::game::GameRuntime;
use glam::{EulerRot, Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

pub const BATCH_LIMIT: usize = 24;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActorState {
    pub id: u64,
    pub name: String,
    pub package: Option<String>,
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub scale: [f32; 3],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub dynamic: bool,
    pub enabled: bool,
    pub visible: bool,
    pub animation_clip: Option<String>,
    pub animation_time: f32,
    pub animation_playing: bool,
    pub animation_rate: f32,
}

impl ActorState {
    pub fn valid(&self) -> bool {
        self.id != 0
            && !self.name.is_empty()
            && self.name.len() <= 160
            && self.package.as_ref().is_none_or(|x| x.len() <= 160)
            && self
                .position
                .iter()
                .chain(&self.velocity)
                .chain(&self.angular_velocity)
                .chain(&self.scale)
                .all(|x| x.is_finite())
            && self.scale.iter().all(|x| *x > 0.0)
            && self
                .animation_clip
                .as_ref()
                .is_none_or(|x| !x.is_empty() && x.len() <= 160)
            && self.animation_time.is_finite()
            && self.animation_time >= 0.0
            && self.animation_rate.is_finite()
            && (0.0..=8.0).contains(&self.animation_rate)
            && self.rotation.iter().all(|x| x.is_finite())
            && (0.98..=1.02).contains(&self.rotation.iter().map(|x| x * x).sum::<f32>())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ActorBatch {
    pub epoch: u64,
    pub offset: u32,
    pub total: u32,
    pub actors: Vec<ActorState>,
}

impl ActorBatch {
    pub fn valid(&self) -> bool {
        self.epoch > 0
            && self.actors.len() <= BATCH_LIMIT
            && self.offset as u64 + self.actors.len() as u64 <= self.total as u64
            && (self.total == 0 || !self.actors.is_empty())
            && self.actors.iter().all(ActorState::valid)
            && self.actors.windows(2).all(|pair| pair[0].id < pair[1].id)
    }
}

#[derive(Default)]
pub struct ActorReplicator {
    host_epoch: u64,
    host_offset: usize,
    last_complete_epoch: u64,
    pending_epoch: u64,
    pending_offset: usize,
    pending_ids: BTreeSet<u64>,
    known_ids: BTreeSet<u64>,
    client_entities: HashMap<u64, EntityId>,
    network_created: BTreeSet<u64>,
    original_physics: HashMap<EntityId, (bool, bool)>,
    replicated_dynamic: HashMap<u64, bool>,
}

impl ActorReplicator {
    pub fn snapshot(&mut self, game: &GameRuntime) -> ActorBatch {
        self.snapshot_entities(&game.actor_system, &game.entity_manager)
    }

    pub fn snapshot_entities(
        &mut self,
        actors: &ActorSystem,
        entities: &EntityManager,
    ) -> ActorBatch {
        let mut ordered = BTreeMap::new();
        for (entity_id, actor) in actors.actors() {
            let Some(id) = actors.script_actor_id_for_entity(*entity_id) else {
                continue;
            };
            let Some(entity) = entities.get_entity(*entity_id) else {
                continue;
            };
            let rotation = Quat::from_euler(
                EulerRot::XYZ,
                entity.rotation.x,
                entity.rotation.y,
                entity.rotation.z,
            );
            let state = ActorState {
                id,
                name: entity.name.clone(),
                package: actor
                    .package_name
                    .clone()
                    .or_else(|| entity.package_name.clone()),
                position: entity.position.to_array(),
                rotation: rotation.to_array(),
                scale: entity.scale.to_array(),
                velocity: entity.physics.velocity.to_array(),
                angular_velocity: entity.physics.angular_velocity.to_array(),
                dynamic: entity.physics.enabled && !entity.physics.anchored,
                enabled: entity.enabled,
                visible: entity.visible,
                animation_clip: entity.animation.current_clip.clone(),
                animation_time: entity.animation.time,
                animation_playing: entity.animation.playing,
                animation_rate: entity.animation.playback_rate,
            };
            if state.valid() {
                ordered.insert(id, state);
            }
        }
        let all: Vec<_> = ordered.into_values().collect();
        let total = all.len();
        if self.host_offset >= total {
            self.host_offset = 0;
        }
        let offset = self.host_offset;
        let end = (offset + BATCH_LIMIT).min(total);
        let batch = ActorBatch {
            epoch: self.host_epoch + 1,
            offset: offset as u32,
            total: total as u32,
            actors: all[offset..end].to_vec(),
        };
        if end == total {
            self.host_offset = 0;
            self.host_epoch += 1;
        } else {
            self.host_offset = end;
        }
        batch
    }

    pub fn disable_guest_simulation(&mut self, game: &mut GameRuntime) {
        self.disable_guest_entities(&game.actor_system, &mut game.entity_manager);
    }

    pub fn disable_guest_entities(&mut self, actors: &ActorSystem, entities: &mut EntityManager) {
        let ids: Vec<_> = actors.actors().keys().copied().collect();
        for id in ids {
            if let Some(entity) = entities.get_entity_mut(id) {
                self.original_physics
                    .entry(id)
                    .or_insert((entity.physics.enabled, entity.physics.anchored));
                if entity.physics.enabled && !entity.physics.anchored {
                    entity.physics.enabled = false;
                    entity.physics.anchored = true;
                    entity.physics.velocity = Vec3::ZERO;
                    entity.physics.angular_velocity = Vec3::ZERO;
                    entity.physics.accumulated_force = Vec3::ZERO;
                    entity.physics.accumulated_torque = Vec3::ZERO;
                }
            }
        }
    }
    fn resolve(
        &mut self,
        actors: &mut ActorSystem,
        entities: &mut EntityManager,
        state: &ActorState,
    ) -> EntityId {
        if let Some(id) = self.client_entities.get(&state.id).copied() {
            if entities.get_entity(id).is_some() {
                return id;
            }
        }
        if let Some(id) = actors.get_entity_for_actor(state.id) {
            self.client_entities.insert(state.id, id);
            return id;
        }
        let id = entities.create_entity(&state.name);
        if let Some(entity) = entities.get_entity_mut(id) {
            entity.package_name = state.package.clone();
        }
        actors.register_actor(RuntimeActor {
            entity_id: id,
            authored_actor_id: Some(state.id),
            package_name: state.package.clone(),
            initial_position: Vec3::from_array(state.position),
        });
        self.client_entities.insert(state.id, id);
        self.network_created.insert(state.id);
        id
    }

    pub fn apply(&mut self, game: &mut GameRuntime, batch: ActorBatch) -> bool {
        self.apply_entities(&mut game.actor_system, &mut game.entity_manager, batch)
    }

    pub fn apply_entities(
        &mut self,
        actors: &mut ActorSystem,
        entities: &mut EntityManager,
        batch: ActorBatch,
    ) -> bool {
        if !batch.valid()
            || batch.epoch <= self.last_complete_epoch
            || batch.epoch < self.pending_epoch
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
        if self.pending_offset != batch.offset as usize {
            return false;
        }
        for state in &batch.actors {
            self.replicated_dynamic.insert(state.id, state.dynamic);
            let id = self.resolve(actors, entities, state);
            if let Some(entity) = entities.get_entity_mut(id) {
                entity.position = Vec3::from_array(state.position);
                let (x, y, z) = Quat::from_array(state.rotation).to_euler(EulerRot::XYZ);
                entity.rotation = Vec3::new(x, y, z);
                entity.scale = Vec3::from_array(state.scale);
                entity.physics.velocity = Vec3::from_array(state.velocity);
                entity.physics.angular_velocity = Vec3::from_array(state.angular_velocity);
                entity.enabled = state.enabled;
                entity.visible = state.visible;
                if entity.animation.current_clip != state.animation_clip
                    || (entity.animation.time - state.animation_time).abs() > 0.15
                {
                    entity.animation.current_clip = state.animation_clip.clone();
                    entity.animation.seek(state.animation_time);
                }
                entity.animation.playing = state.animation_playing;
                entity.animation.playback_rate = state.animation_rate;
                if state.dynamic {
                    entity.physics.enabled = false;
                    entity.physics.anchored = true;
                }
                entity.physics.accumulated_force = Vec3::ZERO;
                entity.physics.accumulated_torque = Vec3::ZERO;
            }
            self.pending_ids.insert(state.id);
        }
        self.pending_offset += batch.actors.len();
        if self.pending_offset == batch.total as usize {
            let missing: Vec<_> = self
                .known_ids
                .difference(&self.pending_ids)
                .copied()
                .collect();
            for id in missing {
                if let Some(entity_id) = self.client_entities.remove(&id) {
                    if self.network_created.remove(&id) {
                        actors.remove_runtime_actor(entity_id);
                        entities.remove_entity(entity_id);
                    } else if let Some(entity) = entities.get_entity_mut(entity_id) {
                        entity.enabled = false;
                        entity.visible = false;
                        entity.physics.enabled = false;
                    }
                }
            }
            self.known_ids = std::mem::take(&mut self.pending_ids);
            self.last_complete_epoch = batch.epoch;
        }
        true
    }

    pub fn restore_guest_simulation(&mut self, game: &mut GameRuntime) {
        for (id, (enabled, anchored)) in self.original_physics.drain() {
            if let Some(entity) = game.entity_manager.get_entity_mut(id) {
                entity.physics.enabled = enabled;
                entity.physics.anchored = anchored;
            }
        }
        for (&id, &dynamic) in &self.replicated_dynamic {
            if !self.network_created.contains(&id) {
                continue;
            }
            if let Some(entity_id) = self.client_entities.get(&id) {
                if let Some(entity) = game.entity_manager.get_entity_mut(*entity_id) {
                    entity.physics.enabled = dynamic;
                    entity.physics.anchored = !dynamic;
                }
            }
        }
    }

    pub fn replicated_count(&self) -> usize {
        self.known_ids.len()
    }

    pub fn push_nearest(&self, game: &mut GameRuntime, player: u64, origin: Vec3) -> Option<u64> {
        self.push_nearest_entity(&game.actor_system, &mut game.entity_manager, player, origin)
    }

    pub fn push_nearest_entity(
        &self,
        actors: &ActorSystem,
        entities: &mut EntityManager,
        player: u64,
        origin: Vec3,
    ) -> Option<u64> {
        if player == 0 || !origin.is_finite() {
            return None;
        }
        let nearest = actors
            .actors()
            .keys()
            .filter_map(|id| {
                let entity = entities.get_entity(*id)?;
                if !entity.enabled || !entity.physics.enabled || entity.physics.anchored {
                    return None;
                }
                let delta = entity.position - origin;
                let horizontal = Vec3::new(delta.x, 0.0, delta.z);
                let distance = horizontal.length();
                (distance > 0.001 && distance <= 3.0).then_some((
                    *id,
                    distance,
                    horizontal.normalize(),
                ))
            })
            .min_by(|a, b| a.1.total_cmp(&b.1))?;
        let actor_id = actors.script_actor_id_for_entity(nearest.0)?;
        let entity = entities.get_entity_mut(nearest.0)?;
        let mass = entity.physics.mass.max(0.01);
        entity
            .physics
            .apply_impulse((nearest.2 * 5.0 + Vec3::Y * 0.25) * mass);
        entity.physics.accumulated_torque += Vec3::Y.cross(nearest.2) * mass;
        eprintln!("MP-03 CONTACT owner={player} actor={actor_id}");
        Some(actor_id)
    }
}

#[cfg(test)]
#[path = "replication_tests.rs"]
mod tests;
