use super::World;
use crate::world::cell::ChunkCoord;
use crate::world::coordinate::WorldCoord;
use std::collections::{HashMap, VecDeque};

pub const RENDER_CHANGE_JOURNAL_CAPACITY: usize = 16_384;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DirtyReason {
    /// Geometry, occupancy, visibility, solidity, anchored, or cell_type changed.
    /// Invalidation neighborhood: own chunk + 6 neighboring chunks.
    Geometry,
    /// Color, texture, or visual offset changed.
    /// Invalidation neighborhood: own chunk only.
    MaterialOrOffset,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RenderChange {
    pub revision: u64,
    pub coord: WorldCoord,
    pub reason: DirtyReason,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RenderChanges {
    pub latest_revision: u64,
    pub full_resync_required: bool,
    pub changes: HashMap<WorldCoord, DirtyReason>,
}

impl World {
    pub fn render_revision(&self) -> u64 {
        self.render_revision
    }

    pub(crate) fn bump_render_revision(&mut self) {
        self.render_revision = self.render_revision.wrapping_add(1);
    }

    pub fn mark_render_dirty(&self, coord: WorldCoord, reason: DirtyReason) {
        let mut map = self.render_dirty_cells.borrow_mut();
        match map.get(&coord) {
            Some(&DirtyReason::Geometry) => {
                // Geometry is already the broader invalidation
            }
            _ => {
                map.insert(coord, reason);
            }
        }

        let mut journal = self.render_change_journal.borrow_mut();
        if journal.len() == RENDER_CHANGE_JOURNAL_CAPACITY {
            if let Some(expired) = journal.pop_front() {
                self.render_change_floor_revision.set(
                    self.render_change_floor_revision
                        .get()
                        .max(expired.revision),
                );
            }
        }
        journal.push_back(RenderChange {
            revision: self.render_revision,
            coord,
            reason,
        });
    }

    pub fn drain_render_dirty_cells(&self) -> HashMap<WorldCoord, DirtyReason> {
        std::mem::take(&mut *self.render_dirty_cells.borrow_mut())
    }

    /// Returns render changes without consuming them. Persistent renderer
    /// subsystems keep independent revision cursors, so mesh, GI, and shadow
    /// caches can observe the same edits safely.
    pub fn render_changes_since(&self, after_revision: u64) -> RenderChanges {
        let latest_revision = self.render_revision;
        if after_revision == latest_revision {
            return RenderChanges {
                latest_revision,
                full_resync_required: false,
                changes: HashMap::new(),
            };
        }

        let journal = self.render_change_journal.borrow();
        let full_resync_required = after_revision > latest_revision
            || after_revision < self.render_change_floor_revision.get()
            || (journal.is_empty() && after_revision != latest_revision);
        let mut changes = HashMap::new();
        if !full_resync_required {
            for change in journal
                .iter()
                .filter(|change| change.revision > after_revision)
            {
                match changes.get(&change.coord) {
                    Some(&DirtyReason::Geometry) => {}
                    _ => {
                        changes.insert(change.coord, change.reason);
                    }
                }
            }
        }
        RenderChanges {
            latest_revision,
            full_resync_required,
            changes,
        }
    }

    /// Marks a cell as needing physics reconciliation.
    pub fn mark_physics_dirty(&mut self, cell_id: u64) {
        self.physics_dirty_cells.insert(cell_id);
    }

    pub(crate) fn authored_chunk_coords(&self) -> std::collections::HashSet<ChunkCoord> {
        self.cells
            .keys()
            .map(|coord| ChunkCoord::from_world_coord(*coord))
            .collect()
    }

    pub fn is_authored_chunk_dirty(&self, chunk_coord: ChunkCoord) -> bool {
        self.dirty_authored_chunks.contains(&chunk_coord)
    }

    pub(crate) fn mark_authored_chunk_clean(&mut self, chunk_coord: ChunkCoord) {
        self.dirty_authored_chunks.remove(&chunk_coord);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::world::{CellType, World};

    #[test]
    fn change_journal_supports_independent_consumers() {
        let mut world = World::new();
        let start = world.render_revision();
        let coord = WorldCoord::new(2, 3, 4);
        world.set_cell(coord, CellType::Block);

        let first = world.render_changes_since(start);
        let second = world.render_changes_since(start);
        assert_eq!(first, second);
        assert_eq!(first.changes.get(&coord), Some(&DirtyReason::Geometry));
        assert!(!first.full_resync_required);
    }

    #[test]
    fn geometry_wins_when_a_coordinate_has_multiple_reasons() {
        let mut world = World::new();
        let start = world.render_revision();
        world.bump_render_revision();
        let coord = WorldCoord::new(0, 0, 0);
        world.mark_render_dirty(coord, DirtyReason::MaterialOrOffset);
        world.mark_render_dirty(coord, DirtyReason::Geometry);
        let changes = world.render_changes_since(start);
        assert_eq!(changes.changes.get(&coord), Some(&DirtyReason::Geometry));
    }

    #[test]
    fn expired_cursor_requests_full_resync() {
        let mut world = World::new();
        let start = world.render_revision();
        for x in 0..=RENDER_CHANGE_JOURNAL_CAPACITY {
            world.bump_render_revision();
            world.mark_render_dirty(
                WorldCoord::new(x as i32, 0, 0),
                DirtyReason::MaterialOrOffset,
            );
        }
        assert!(world.render_changes_since(start).full_resync_required);
    }
}
