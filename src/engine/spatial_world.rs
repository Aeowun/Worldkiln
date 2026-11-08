// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::engine::entity::EntityId;
use glam::Vec3;
use std::collections::{BTreeSet, HashMap};

const CELL_SIZE: f32 = 16.0;
const MAX_BUCKETS_PER_ENTRY: u64 = 512;

type Bucket = (i32, i32, i32);

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Bounds3 {
    pub min: Vec3,
    pub max: Vec3,
}

impl Bounds3 {
    pub fn new(min: Vec3, max: Vec3) -> Result<Self, String> {
        if !min.is_finite()
            || !max.is_finite()
            || min.x >= max.x
            || min.y >= max.y
            || min.z >= max.z
        {
            return Err("spatial bounds require finite, positive 3D extents".into());
        }
        Ok(Self { min, max })
    }

    #[inline]
    pub fn overlaps(self, rhs: Self) -> bool {
        self.min.x < rhs.max.x
            && self.max.x > rhs.min.x
            && self.min.y < rhs.max.y
            && self.max.y > rhs.min.y
            && self.min.z < rhs.max.z
            && self.max.z > rhs.min.z
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpatialMotion {
    Static,
    Dynamic,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpatialMember {
    pub bounds: Bounds3,
    pub motion: SpatialMotion,
    pub enabled: bool,
    pub has_interaction: bool,
    pub layer: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SpatialFilter {
    pub include_static: bool,
    pub include_dynamic: bool,
    pub require_interaction: bool,
    pub layer_mask: u32,
}

impl Default for SpatialFilter {
    fn default() -> Self {
        Self {
            include_static: true,
            include_dynamic: true,
            require_interaction: false,
            layer_mask: u32::MAX,
        }
    }
}

impl SpatialFilter {
    fn matches(self, entry: SpatialMember) -> bool {
        entry.enabled
            && (entry.layer & self.layer_mask != 0)
            && (!self.require_interaction || entry.has_interaction)
            && match entry.motion {
                SpatialMotion::Static => self.include_static,
                SpatialMotion::Dynamic => self.include_dynamic,
            }
    }
}

fn cells(bounds: Bounds3) -> Option<Vec<Bucket>> {
    let floor = |n: f32| (n / CELL_SIZE).floor();
    let low = bounds.min.map(floor);
    let high = bounds.max.map(floor);
    if !low.is_finite()
        || !high.is_finite()
        || [low.x, low.y, low.z, high.x, high.y, high.z]
            .iter()
            .any(|v| *v <= i32::MIN as f32 + 1024.0 || *v >= i32::MAX as f32 - 1024.0)
    {
        return None;
    }
    let (x0, y0, z0) = (low.x as i32, low.y as i32, low.z as i32);
    let (x1, y1, z1) = (high.x as i32, high.y as i32, high.z as i32);
    let count = ((x1 as i64 - x0 as i64 + 1) as u64)
        .checked_mul((y1 as i64 - y0 as i64 + 1) as u64)?
        .checked_mul((z1 as i64 - z0 as i64 + 1) as u64)?;
    if count > MAX_BUCKETS_PER_ENTRY {
        return None;
    }
    let mut keys = Vec::with_capacity(count as usize);
    for x in x0..=x1 {
        for y in y0..=y1 {
            for z in z0..=z1 {
                keys.push((x, y, z));
            }
        }
    }
    Some(keys)
}

#[derive(Clone, Debug)]
struct StoredMember {
    value: SpatialMember,
    cells: Option<Vec<Bucket>>,
}

#[derive(Default)]
pub struct SpatialWorld {
    entries: HashMap<EntityId, StoredMember>,
    buckets: HashMap<Bucket, BTreeSet<EntityId>>,
    oversized: BTreeSet<EntityId>,
    revision: u64,
}

impl SpatialWorld {
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn len(&self) -> usize {
        self.entries.len()
    }
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn upsert(&mut self, id: EntityId, member: SpatialMember) -> bool {
        if self.entries.get(&id).is_some_and(|e| e.value == member) {
            return false;
        }
        self.remove_internal(id);
        let keys = if member.enabled {
            cells(member.bounds)
        } else {
            Some(Vec::new())
        };
        if member.enabled {
            if let Some(keys) = &keys {
                for &key in keys {
                    self.buckets.entry(key).or_default().insert(id);
                }
            } else {
                self.oversized.insert(id);
            }
        }
        self.entries.insert(
            id,
            StoredMember {
                value: member,
                cells: keys,
            },
        );
        self.revision = self.revision.wrapping_add(1);
        true
    }

    fn remove_internal(&mut self, id: EntityId) -> bool {
        let Some(previous) = self.entries.remove(&id) else {
            return false;
        };
        if let Some(keys) = previous.cells {
            for key in keys {
                if let std::collections::hash_map::Entry::Occupied(mut bucket) =
                    self.buckets.entry(key)
                {
                    bucket.get_mut().remove(&id);
                    if bucket.get().is_empty() {
                        bucket.remove();
                    }
                }
            }
        } else {
            self.oversized.remove(&id);
        }
        true
    }

    pub fn remove(&mut self, id: EntityId) -> bool {
        if !self.remove_internal(id) {
            return false;
        }
        self.revision = self.revision.wrapping_add(1);
        true
    }

    pub fn clear(&mut self) {
        if self.entries.is_empty() {
            return;
        }
        self.entries.clear();
        self.buckets.clear();
        self.oversized.clear();
        self.revision = self.revision.wrapping_add(1);
    }

    pub fn overlap_aabb_into(
        &self,
        bounds: Bounds3,
        filter: SpatialFilter,
        output: &mut Vec<EntityId>,
        scratch: &mut BTreeSet<EntityId>,
    ) {
        output.clear();
        scratch.clear();
        match cells(bounds) {
            Some(keys) => {
                for key in keys {
                    if let Some(bucket) = self.buckets.get(&key) {
                        scratch.extend(bucket.iter().copied());
                    }
                }
                scratch.extend(self.oversized.iter().copied());
            }
            None => scratch.extend(self.entries.keys().copied()),
        }
        for id in scratch.iter().copied() {
            if let Some(entry) = self.entries.get(&id) {
                if filter.matches(entry.value) && bounds.overlaps(entry.value.bounds) {
                    output.push(id);
                }
            }
        }
    }
}

#[derive(Default)]
pub struct ProximityObserver {
    active: BTreeSet<EntityId>,
    previous_bounds: Option<Bounds3>,
    previous_filter: Option<SpatialFilter>,
    previous_revision: Option<u64>,
    results: Vec<EntityId>,
    scratch: BTreeSet<EntityId>,
    pub actual_queries: u64,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct OverlapTransitions {
    pub entered: Vec<EntityId>,
    pub exited: Vec<EntityId>,
}

impl ProximityObserver {
    pub fn update(
        &mut self,
        world: &SpatialWorld,
        bounds: Bounds3,
        filter: SpatialFilter,
    ) -> OverlapTransitions {
        if self.previous_bounds == Some(bounds)
            && self.previous_filter == Some(filter)
            && self.previous_revision == Some(world.revision())
        {
            return OverlapTransitions::default();
        }
        self.actual_queries += 1;
        world.overlap_aabb_into(bounds, filter, &mut self.results, &mut self.scratch);
        let next: BTreeSet<EntityId> = self.results.iter().copied().collect();
        let transitions = OverlapTransitions {
            entered: next.difference(&self.active).copied().collect(),
            exited: self.active.difference(&next).copied().collect(),
        };
        self.active = next;
        self.previous_bounds = Some(bounds);
        self.previous_filter = Some(filter);
        self.previous_revision = Some(world.revision());
        transitions
    }

    pub fn overlaps(&self) -> &BTreeSet<EntityId> {
        &self.active
    }

    pub fn reset(&mut self) {
        self.active.clear();
        self.previous_bounds = None;
        self.previous_filter = None;
        self.previous_revision = None;
        self.results.clear();
        self.scratch.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn b(min: [f32; 3], max: [f32; 3]) -> Bounds3 {
        Bounds3::new(min.into(), max.into()).unwrap()
    }

    fn member(bounds: Bounds3, motion: SpatialMotion) -> SpatialMember {
        SpatialMember {
            bounds,
            motion,
            enabled: true,
            has_interaction: true,
            layer: 1,
        }
    }

    #[test]
    fn spatial_exact_results_match_exhaustive_for_negative_and_oversized_bounds() {
        let mut index = SpatialWorld::default();
        let entries = [
            (
                1,
                b([-22.0, -3.0, -22.0], [-19.0, 2.0, -18.0]),
                SpatialMotion::Static,
            ),
            (
                2,
                b([-21.0, -2.0, -20.0], [-17.0, 3.0, -17.0]),
                SpatialMotion::Dynamic,
            ),
            (
                3,
                b([100.0, 0.0, 100.0], [110.0, 4.0, 111.0]),
                SpatialMotion::Static,
            ),
            (
                4,
                b([-2000.0, -2.0, -2000.0], [2000.0, 10.0, 2000.0]),
                SpatialMotion::Static,
            ),
        ];
        for &(id, bounds, motion) in &entries {
            assert!(index.upsert(EntityId(id), member(bounds, motion)));
        }
        let mut out = Vec::new();
        let mut scratch = BTreeSet::new();
        for region in [
            b([-22.0, -3.0, -22.0], [-18.0, 2.0, -18.0]),
            b([98.0, -1.0, 98.0], [112.0, 6.0, 113.0]),
            b([-5000.0, -9.0, -5000.0], [5000.0, 11.0, 5000.0]),
            b([5000.0, 0.0, 5000.0], [5010.0, 10.0, 5010.0]),
        ] {
            index.overlap_aabb_into(region, SpatialFilter::default(), &mut out, &mut scratch);
            let expected: Vec<_> = entries
                .iter()
                .filter(|(_, bounds, _)| region.overlaps(*bounds))
                .map(|(id, _, _)| EntityId(*id))
                .collect();
            assert_eq!(out, expected);
        }
        assert!(Bounds3::new(Vec3::ZERO, Vec3::ZERO).is_err());
        assert!(Bounds3::new(Vec3::splat(f32::NAN), Vec3::ONE).is_err());
    }

    #[test]
    fn observer_emits_only_real_enter_exit_on_mutations() {
        let mut index = SpatialWorld::default();
        let region = b([-2.0, -2.0, -2.0], [2.0, 2.0, 2.0]);
        let close = member(
            b([-1.0, -1.0, -1.0], [1.0, 1.0, 1.0]),
            SpatialMotion::Dynamic,
        );
        let distant = member(
            b([50.0, 1.0, 50.0], [52.0, 2.0, 52.0]),
            SpatialMotion::Static,
        );
        let mut observer = ProximityObserver::default();
        let filter = SpatialFilter {
            require_interaction: true,
            ..Default::default()
        };
        assert_eq!(
            observer.update(&index, region, filter),
            OverlapTransitions::default()
        );
        assert_eq!(observer.actual_queries, 1);
        assert_eq!(
            observer.update(&index, region, filter),
            OverlapTransitions::default()
        );
        assert_eq!(observer.actual_queries, 1, "settled scene cannot rescan");
        index.upsert(EntityId(1), close);
        assert_eq!(
            observer.update(&index, region, filter).entered,
            vec![EntityId(1)]
        );
        assert!(!index.upsert(EntityId(1), close));
        assert_eq!(
            observer.update(&index, region, filter),
            OverlapTransitions::default()
        );
        index.upsert(EntityId(2), distant);
        assert!(observer.update(&index, region, filter).entered.is_empty());
        index.upsert(
            EntityId(1),
            SpatialMember {
                enabled: false,
                ..close
            },
        );
        assert_eq!(
            observer.update(&index, region, filter).exited,
            vec![EntityId(1)]
        );
        index.upsert(EntityId(1), close);
        assert_eq!(
            observer.update(&index, region, filter).entered,
            vec![EntityId(1)]
        );
        index.remove(EntityId(1));
        assert_eq!(
            observer.update(&index, region, filter).exited,
            vec![EntityId(1)]
        );
        index.clear();
        assert_eq!(
            observer.update(&index, region, filter),
            OverlapTransitions::default()
        );
    }

    #[test]
    fn spatial_filters_keep_classification_separate_from_query_layers() {
        let mut world = SpatialWorld::default();
        let bounds = b([0.0, 0.0, 0.0], [2.0, 2.0, 2.0]);
        world.upsert(EntityId(1), member(bounds, SpatialMotion::Static));
        world.upsert(
            EntityId(2),
            SpatialMember {
                has_interaction: false,
                layer: 2,
                motion: SpatialMotion::Dynamic,
                ..member(bounds, SpatialMotion::Dynamic)
            },
        );
        let mut result = Vec::new();
        let mut scratch = BTreeSet::new();
        world.overlap_aabb_into(
            bounds,
            SpatialFilter {
                include_static: false,
                include_dynamic: true,
                require_interaction: false,
                layer_mask: 2,
            },
            &mut result,
            &mut scratch,
        );
        assert_eq!(result, vec![EntityId(2)]);
        world.overlap_aabb_into(
            bounds,
            SpatialFilter {
                require_interaction: true,
                ..Default::default()
            },
            &mut result,
            &mut scratch,
        );
        assert_eq!(result, vec![EntityId(1)]);
    }
}
