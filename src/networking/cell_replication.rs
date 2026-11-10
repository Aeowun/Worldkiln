// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::engine::physics::physics_world::PhysicsWorld;
use crate::world::cell::{Cell, RuntimeCellState};
use crate::world::world::DirtyReason;
use crate::world::{World, WorldCoord};
use glam::{Quat, Vec3};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet, HashMap};

const CELL_BATCH_LIMIT: usize = 6;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetworkBody {
    pub position: [f32; 3],
    pub rotation: [f32; 4],
    pub velocity: [f32; 3],
    pub angular_velocity: [f32; 3],
    pub color: [f32; 3],
    pub texture: String,
    pub visible: bool,
    pub sleeping: bool,
}

impl NetworkBody {
    pub fn valid(&self) -> bool {
        self.position
            .iter()
            .chain(&self.velocity)
            .chain(&self.angular_velocity)
            .chain(&self.color)
            .all(|v| v.is_finite())
            && self.rotation.iter().all(|v| v.is_finite())
            && (0.98..=1.02).contains(&self.rotation.iter().map(|v| v * v).sum::<f32>())
            && self.texture.len() <= 512
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NetworkCell {
    pub id: u64,
    pub cell: Option<Cell>,
    pub coord: Option<[i32; 3]>,
    pub state: Option<RuntimeCellState>,
    pub body: Option<NetworkBody>,
}

impl NetworkCell {
    pub fn valid(&self) -> bool {
        if self.id == 0
            || self.cell.as_ref().is_some_and(|c| c.id != self.id)
            || self
                .coord
                .is_some_and(|p| p.iter().any(|v| v.unsigned_abs() > 1_000_000))
        {
            return false;
        }
        self.cell.as_ref().is_none_or(|c| {
            c.texture.len() <= 512
                && c.color_rgb.is_finite()
                && c.light_color.is_finite()
                && c.attributes.len() <= 128
                && c.particles.validate().is_ok()
        }) && self.body.as_ref().is_none_or(NetworkBody::valid)
            && self.state.as_ref().is_none_or(|s| {
                s.source_position.is_none_or(Vec3::is_finite)
                    && s.visual_offset.is_none_or(Vec3::is_finite)
                    && s.color_rgb.is_none_or(Vec3::is_finite)
                    && s.light_color.is_none_or(Vec3::is_finite)
                    && s.attribute_overrides.len() <= 128
            })
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct CellBatch {
    pub epoch: u64,
    pub offset: u32,
    pub total: u32,
    pub cells: Vec<NetworkCell>,
}

impl CellBatch {
    pub fn valid(&self) -> bool {
        self.epoch > 0
            && self.cells.len() <= CELL_BATCH_LIMIT
            && self.offset as u64 + self.cells.len() as u64 <= self.total as u64
            && (self.total == 0 || !self.cells.is_empty())
            && self.cells.iter().all(NetworkCell::valid)
            && self.cells.windows(2).all(|pair| pair[0].id < pair[1].id)
    }
}

#[derive(Default)]
pub struct CellReplicator {
    epoch: u64,
    offset: usize,
    pending_epoch: u64,
    pending_offset: usize,
    last_complete: u64,
    pending_ids: BTreeSet<u64>,
    known_ids: BTreeSet<u64>,
    local_ids: HashMap<u64, u64>,
    original_body_flags: HashMap<u64, (bool, bool)>,
}

impl CellReplicator {
    pub fn snapshot(&mut self, world: &World) -> CellBatch {
        self.snapshot_internal(world, None)
    }

    pub fn snapshot_with_physics(&mut self, world: &World, physics: &PhysicsWorld) -> CellBatch {
        self.snapshot_internal(world, Some(physics))
    }

    fn snapshot_internal(&mut self, world: &World, physics: Option<&PhysicsWorld>) -> CellBatch {
        let mut entries = BTreeMap::new();
        for (&id, cell) in &world.runtime_cells {
            let coord = world.runtime_id_to_coord.get(&id).map(|p| [p.x, p.y, p.z]);
            entries.insert(
                id,
                NetworkCell {
                    id,
                    cell: Some(cell.clone()),
                    coord,
                    state: world.runtime_state.get(&id).cloned(),
                    body: None,
                },
            );
        }
        for (&id, state) in &world.runtime_state {
            if !entries.contains_key(&id)
                && (world.id_to_coord.contains_key(&id) || world.sources.contains_key(&id))
            {
                entries.insert(
                    id,
                    NetworkCell {
                        id,
                        cell: None,
                        coord: None,
                        state: Some(state.clone()),
                        body: None,
                    },
                );
            }
        }
        if let Some(physics) = physics {
            for body in &physics.bodies {
                let id = body.cell_id;
                if id == 0 || world.get_effective_cell_by_id(id).is_none() {
                    continue;
                }
                let entry = entries.entry(id).or_insert_with(|| NetworkCell {
                    id,
                    cell: None,
                    coord: None,
                    state: world.runtime_state.get(&id).cloned(),
                    body: None,
                });
                entry.body = Some(NetworkBody {
                    position: body.position.to_array(),
                    rotation: body.rotation.to_array(),
                    velocity: body.velocity.to_array(),
                    angular_velocity: body.angular_velocity.to_array(),
                    color: body.color_rgb.to_array(),
                    texture: body.texture.clone(),
                    visible: body.visible,
                    sleeping: body.is_sleeping,
                });
            }
        }
        let all: Vec<_> = entries.into_values().collect();
        let total = all.len();
        if self.offset >= total {
            self.offset = 0;
        }
        let from = self.offset;
        let to = (from + CELL_BATCH_LIMIT).min(total);
        let batch = CellBatch {
            epoch: self.epoch + 1,
            offset: from as u32,
            total: total as u32,
            cells: all[from..to].to_vec(),
        };
        if to == total {
            self.offset = 0;
            self.epoch += 1;
        } else {
            self.offset = to;
        }
        batch
    }

    pub fn apply(&mut self, world: &mut World, batch: CellBatch) -> bool {
        self.apply_internal(world, None, batch)
    }

    pub fn apply_with_physics(
        &mut self,
        world: &mut World,
        physics: &mut PhysicsWorld,
        batch: CellBatch,
    ) -> bool {
        self.apply_internal(world, Some(physics), batch)
    }

    fn apply_internal(
        &mut self,
        world: &mut World,
        mut physics: Option<&mut PhysicsWorld>,
        batch: CellBatch,
    ) -> bool {
        if !batch.valid() || batch.epoch <= self.last_complete || batch.epoch < self.pending_epoch {
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
        for state in &batch.cells {
            if let Some(cell) = &state.cell {
                self.apply_runtime(world, state, cell);
            } else if world.id_to_coord.contains_key(&state.id)
                || world.sources.contains_key(&state.id)
            {
                self.apply_override(world, state.id, state.state.clone());
            }
            self.pending_ids.insert(state.id);
        }
        self.pending_offset += batch.cells.len();
        if self.pending_offset == batch.total as usize {
            self.reconcile(world);
            self.known_ids = std::mem::take(&mut self.pending_ids);
            self.last_complete = batch.epoch;
        }
        if let Some(physics) = physics.as_deref_mut() {
            physics.sync_with_world(world);
            for state in &batch.cells {
                let Some(source) = &state.body else {
                    continue;
                };
                let local_id = self.local_ids.get(&state.id).copied().unwrap_or(state.id);
                if let Some(body) = physics.bodies.iter_mut().find(|b| b.cell_id == local_id) {
                    self.original_body_flags
                        .entry(local_id)
                        .or_insert((body.anchored, body.gravity_participation));
                    body.position = Vec3::from_array(source.position);
                    body.rotation = Quat::from_array(source.rotation);
                    body.velocity = Vec3::from_array(source.velocity);
                    body.angular_velocity = Vec3::from_array(source.angular_velocity);
                    body.color_rgb = Vec3::from_array(source.color);
                    body.texture = source.texture.clone();
                    body.visible = source.visible;
                    body.is_sleeping = source.sleeping;
                    body.anchored = true;
                    body.gravity_participation = false;
                }
            }
        }
        true
    }

    pub fn restore_guest_simulation(&mut self, physics: &mut PhysicsWorld) {
        for (cell_id, (anchored, gravity)) in self.original_body_flags.drain() {
            if let Some(body) = physics
                .bodies
                .iter_mut()
                .find(|body| body.cell_id == cell_id)
            {
                body.anchored = anchored;
                body.gravity_participation = gravity;
                body.is_sleeping = false;
            }
        }
    }

    fn apply_runtime(&mut self, world: &mut World, state: &NetworkCell, source: &Cell) {
        let id = self
            .local_ids
            .get(&state.id)
            .copied()
            .filter(|&id| {
                world
                    .runtime_cells
                    .get(&id)
                    .is_some_and(|c| c.cell_type == source.cell_type)
            })
            .unwrap_or_else(|| {
                let id = world.create_runtime_cell(source.cell_type);
                self.local_ids.insert(state.id, id);
                id
            });
        let mut cell = source.clone();
        cell.id = id;
        let changed = world.runtime_cells.get(&id) != Some(&cell);
        if changed {
            world.runtime_cells.insert(id, cell);
        }
        let current = world.runtime_id_to_coord.get(&id).copied();
        let target = state.coord.map(|p| WorldCoord::new(p[0], p[1], p[2]));
        if let Some(coord) = target {
            if current != Some(coord) {
                let _ = world.move_runtime_cell(id, coord);
            }
        }
        let previous = world.runtime_state.get(&id);
        let state_changed = previous != state.state.as_ref();
        if state_changed {
            match &state.state {
                Some(runtime) => {
                    world.runtime_state.insert(id, runtime.clone());
                }
                None => {
                    world.runtime_state.remove(&id);
                }
            }
        }
        if changed || state_changed {
            world.bump_render_revision();
            world.mark_physics_dirty(id);
            if let Some(coord) = target {
                world.mark_render_dirty(coord, DirtyReason::Geometry);
                world.update_spatial_index_at(coord);
            }
        }
    }

    fn apply_override(&self, world: &mut World, id: u64, state: Option<RuntimeCellState>) {
        if world.runtime_state.get(&id) == state.as_ref() {
            return;
        }
        match state {
            Some(state) => {
                world.runtime_state.insert(id, state);
            }
            None => {
                world.runtime_state.remove(&id);
            }
        }
        world.bump_render_revision();
        world.mark_physics_dirty(id);
        if let Some(coord) = world.resolve_cell_id(id) {
            world.mark_render_dirty(coord, DirtyReason::Geometry);
            world.update_spatial_index_at(coord);
        }
    }

    fn reconcile(&mut self, world: &mut World) {
        let missing: Vec<u64> = self
            .known_ids
            .difference(&self.pending_ids)
            .copied()
            .collect();
        for remote_id in missing {
            if let Some(local_id) = self.local_ids.remove(&remote_id) {
                world.delete_cell_runtime(local_id);
            } else {
                self.apply_override(world, remote_id, None);
            }
        }
        let authoritative: BTreeSet<u64> = self.local_ids.values().copied().collect();
        let local_only: Vec<u64> = world
            .runtime_id_to_coord
            .keys()
            .filter(|id| !authoritative.contains(id))
            .copied()
            .collect();
        for id in local_only {
            world.delete_cell_runtime(id);
        }
        let unauthorized_overrides: Vec<u64> = world
            .runtime_state
            .keys()
            .filter(|id| {
                !self.pending_ids.contains(id)
                    && !world.runtime_cells.contains_key(id)
                    && (world.id_to_coord.contains_key(id) || world.sources.contains_key(id))
            })
            .copied()
            .collect();
        for id in unauthorized_overrides {
            self.apply_override(world, id, None);
        }
    }
    pub fn replicated_count(&self) -> usize {
        self.known_ids.len()
    }
}

#[cfg(test)]
#[path = "cell_replication_tests.rs"]
mod tests;
