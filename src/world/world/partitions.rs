// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::World;
use crate::world::cell::{CHUNK_SIZE, ChunkCoord};
use glam::Vec3;
use std::collections::{BTreeMap, BTreeSet};

pub const PARTITION_MANIFEST_VERSION: u32 = 1;
const MAX_DEPENDENCY_CHUNKS: u64 = 128;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PartitionRecord {
    pub actors: Vec<u64>,
    pub cells: Vec<u64>,
    pub sources: Vec<u64>,
    pub actor_references: Vec<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PartitionManifest {
    pub version: u32,
    pub partitions: BTreeMap<ChunkCoord, PartitionRecord>,
    pub global_actors: Vec<u64>,
}

fn chunk_of(position: Vec3) -> Option<ChunkCoord> {
    if !position.is_finite() {
        return None;
    }
    let value = (position / CHUNK_SIZE as f32).floor();
    let valid =
        |n: f32| n.is_finite() && n > i32::MIN as f32 + 1024.0 && n < i32::MAX as f32 - 1024.0;
    if !valid(value.x) || !valid(value.y) || !valid(value.z) {
        return None;
    }
    Some(ChunkCoord::new(
        value.x as i32,
        value.y as i32,
        value.z as i32,
    ))
}

fn coverage(center: Vec3, radius: f32) -> Option<Vec<ChunkCoord>> {
    if radius < 0.0 || !radius.is_finite() {
        return None;
    }
    let low = chunk_of(center - Vec3::splat(radius))?;
    let high = chunk_of(center + Vec3::splat(radius))?;
    let dx = (high.x as i64 - low.x as i64 + 1) as u64;
    let dy = (high.y as i64 - low.y as i64 + 1) as u64;
    let dz = (high.z as i64 - low.z as i64 + 1) as u64;
    let count = dx.checked_mul(dy)?.checked_mul(dz)?;
    if count > MAX_DEPENDENCY_CHUNKS {
        return None;
    }
    let mut result = Vec::with_capacity(count as usize);
    for x in low.x..=high.x {
        for y in low.y..=high.y {
            for z in low.z..=high.z {
                result.push(ChunkCoord::new(x, y, z));
            }
        }
    }
    Some(result)
}

impl PartitionManifest {
    pub fn from_authored_world(world: &World) -> Self {
        let mut plan = Self {
            version: PARTITION_MANIFEST_VERSION,
            partitions: BTreeMap::new(),
            global_actors: Vec::new(),
        };
        for (&coord, cell) in &world.cells {
            plan.partitions
                .entry(ChunkCoord::from_world_coord(coord))
                .or_default()
                .cells
                .push(cell.id);
        }
        for source in world.sources.values() {
            if let Some(chunk) = chunk_of(source.position) {
                plan.partitions
                    .entry(chunk)
                    .or_default()
                    .sources
                    .push(source.cell.id);
            }
        }
        for actor in world.actors.values() {
            let scaled = actor.physics.collision_size.abs() * actor.scale.abs() * 0.5;
            let offset = actor.physics.collision_offset.abs() * actor.scale.abs();
            let radius = (scaled + offset).length();
            let Some(home) = chunk_of(actor.position) else {
                plan.global_actors.push(actor.id);
                continue;
            };
            let Some(chunks) = coverage(actor.position, radius) else {
                plan.global_actors.push(actor.id);
                continue;
            };
            plan.partitions
                .entry(home)
                .or_default()
                .actors
                .push(actor.id);
            for chunk in chunks {
                if chunk != home {
                    plan.partitions
                        .entry(chunk)
                        .or_default()
                        .actor_references
                        .push(actor.id);
                }
            }
        }
        for record in plan.partitions.values_mut() {
            record.actors.sort_unstable();
            record.cells.sort_unstable();
            record.sources.sort_unstable();
            record.actor_references.sort_unstable();
            record.actor_references.dedup();
        }
        plan.global_actors.sort_unstable();
        plan
    }

    pub fn validate_coverage(&self, world: &World) -> Result<(), String> {
        let mut actors = self.global_actors.clone();
        let mut cells = Vec::new();
        let mut sources = Vec::new();
        for record in self.partitions.values() {
            actors.extend_from_slice(&record.actors);
            cells.extend_from_slice(&record.cells);
            sources.extend_from_slice(&record.sources);
        }
        actors.sort_unstable();
        cells.sort_unstable();
        sources.sort_unstable();
        let sorted = |mut ids: Vec<u64>| {
            ids.sort_unstable();
            ids
        };
        if actors != sorted(world.actors.keys().copied().collect())
            || cells != sorted(world.cells.values().map(|cell| cell.id).collect())
            || sources
                != sorted(
                    world
                        .sources
                        .values()
                        .map(|source| source.cell.id)
                        .collect(),
                )
        {
            return Err("partition identity coverage incomplete; do not migrate".into());
        }
        Ok(())
    }

    pub fn resident_partitions(
        &self,
        sources: &[ResidencySource],
        always_loaded: &BTreeSet<ChunkCoord>,
    ) -> Result<BTreeSet<ChunkCoord>, String> {
        let mut selected = always_loaded.clone();
        for source in sources {
            if !source.position.is_finite() || !source.radius.is_finite() || source.radius < 0.0 {
                return Err("invalid residency source".into());
            }
            for chunk in self.partitions.keys() {
                let origin =
                    Vec3::new(chunk.x as f32, chunk.y as f32, chunk.z as f32) * CHUNK_SIZE as f32;
                let close = source
                    .position
                    .clamp(origin, origin + Vec3::splat(CHUNK_SIZE as f32));
                if close.distance_squared(source.position) <= source.radius * source.radius {
                    selected.insert(*chunk);
                }
            }
        }
        Ok(selected)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ResidencySource {
    pub position: Vec3,
    pub radius: f32,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{AuthoredSource, CellType, WorldCoord};

    #[test]
    fn advisory_partition_manifest_covers_authored_ids_without_mutation() {
        let mut world = World::new();
        let west_actor = world.add_actor("West", Vec3::new(-0.5, 1.0, -32.5));
        let east_actor = world.add_actor("East", Vec3::new(17.0, 1.0, 0.0));
        world.actors.get_mut(&east_actor).unwrap().enabled = false;
        let west_cell = world.set_cell(WorldCoord::new(-1, -1, -17), CellType::Block);
        let east_cell = world.set_cell(WorldCoord::new(33, 0, 1), CellType::Block);
        let mut source = crate::world::cell::Cell::new_light();
        source.id = 400_001;
        world.insert_source(AuthoredSource {
            cell: source,
            position: Vec3::new(-17.0, 0.0, 0.0),
        });
        let before_actors = world.actors.clone();
        let before_cells = world.cells.clone();
        let manifest = PartitionManifest::from_authored_world(&world);
        assert_eq!(manifest, PartitionManifest::from_authored_world(&world));
        assert_eq!(manifest.version, PARTITION_MANIFEST_VERSION);
        manifest.validate_coverage(&world).unwrap();
        assert_eq!(before_actors, world.actors);
        assert_eq!(before_cells, world.cells);
        let actor_ids: Vec<_> = manifest
            .partitions
            .values()
            .flat_map(|record| record.actors.iter().copied())
            .collect();
        assert!(actor_ids.contains(&west_actor) && actor_ids.contains(&east_actor));
        assert!(
            manifest
                .partitions
                .values()
                .any(|p| p.cells.contains(&west_cell))
        );
        assert!(
            manifest
                .partitions
                .values()
                .any(|p| p.cells.contains(&east_cell))
        );
        assert!(
            manifest
                .partitions
                .values()
                .any(|p| p.sources.contains(&400_001))
        );
    }

    #[test]
    fn near_border_actor_references_neighbor_and_giant_is_global() {
        let mut world = World::new();
        let id = world.add_actor("Border", Vec3::new(15.9, 0.5, 0.5));
        let manifest = PartitionManifest::from_authored_world(&world);
        manifest.validate_coverage(&world).unwrap();
        assert!(
            manifest.partitions[&ChunkCoord::new(0, 0, 0)]
                .actors
                .contains(&id)
        );
        assert!(
            manifest.partitions[&ChunkCoord::new(1, 0, 0)]
                .actor_references
                .contains(&id)
        );
        world.actors.get_mut(&id).unwrap().physics.collision_size = Vec3::splat(100_000.0);
        let giant = PartitionManifest::from_authored_world(&world);
        giant.validate_coverage(&world).unwrap();
        assert_eq!(giant.global_actors, vec![id]);
    }

    #[test]
    fn residency_source_selects_only_requested_area_and_never_unloads() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(-32, 0, 0), CellType::Block);
        world.set_cell(WorldCoord::new(100, 0, 0), CellType::Block);
        let before = world.cells.clone();
        let manifest = PartitionManifest::from_authored_world(&world);
        let forced = BTreeSet::from([ChunkCoord::new(6, 0, 0)]);
        let selected = manifest
            .resident_partitions(
                &[ResidencySource {
                    position: Vec3::new(-31.0, 1.0, 1.0),
                    radius: 6.0,
                }],
                &forced,
            )
            .unwrap();
        assert!(selected.contains(&ChunkCoord::new(-2, 0, 0)));
        assert!(selected.contains(&ChunkCoord::new(6, 0, 0)));
        assert_eq!(selected.len(), 2);
        assert_eq!(world.cells, before);
        assert!(
            manifest
                .resident_partitions(
                    &[ResidencySource {
                        position: Vec3::ZERO,
                        radius: f32::NAN
                    }],
                    &BTreeSet::new()
                )
                .is_err()
        );
    }

    #[test]
    fn uncovered_source_fails_closed_before_any_migration() {
        let mut world = World::new();
        let mut source = crate::world::cell::Cell::new_light();
        source.id = 400_002;
        world.insert_source(AuthoredSource {
            cell: source,
            position: Vec3::splat(f32::NAN),
        });
        let manifest = PartitionManifest::from_authored_world(&world);
        assert!(manifest.validate_coverage(&world).is_err());
    }
}
