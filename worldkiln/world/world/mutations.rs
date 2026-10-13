//! World mutation and cell-index maintenance.
//!
//! Handles authored cell creation, replacement, deletion, movement, pasting,
//! ID remapping, and maintenance of the specialized Light, AudioEmitter,
//! and editor-marker lookup indices.
// Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:33 -05:00
use crate::scripting::binding::ScriptBinding;
use crate::world::cell::{Cell, CellType, ChunkCoord};
use crate::world::coordinate::WorldCoord;
use crate::world::source::AuthoredSource;

use super::World;
use super::dirty::DirtyReason;

impl World {
    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    fn editor_navigator_tracks(cell_type: CellType) -> bool {
        // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
        // Track just editor-relevant markers; ordinary Blocks stay unindexed.
        matches!(
            cell_type,
            CellType::SpawnPoint
                | CellType::FxBlock
                | CellType::Player
                | CellType::Light
                | CellType::AudioEmitter
                | CellType::ParticleEmitter
        )
    }

    fn index_authored_cell_summary(&mut self, coord: WorldCoord, cell_type: CellType) {
        *self.authored_cell_type_counts.entry(cell_type).or_insert(0) += 1;
        if Self::editor_navigator_tracks(cell_type) {
            self.editor_navigator_coords
                .entry(cell_type)
                .or_default()
                .insert(coord);
        }
    }

    fn unindex_authored_cell_summary(&mut self, coord: WorldCoord, cell_type: CellType) {
        if let Some(count) = self.authored_cell_type_counts.get_mut(&cell_type) {
            *count = count.saturating_sub(1);
            if *count == 0 {
                self.authored_cell_type_counts.remove(&cell_type);
            }
        }

        if Self::editor_navigator_tracks(cell_type) {
            if let Some(coords) = self.editor_navigator_coords.get_mut(&cell_type) {
                coords.remove(&coord);
                if coords.is_empty() {
                    self.editor_navigator_coords.remove(&cell_type);
                }
            }
        }
    }

    fn rebuild_authored_cell_summary(&mut self) {
        self.authored_cell_type_counts.clear();
        self.editor_navigator_coords.clear();

        let entries = self
            .cells
            .iter()
            .map(|(coord, cell)| (*coord, cell.cell_type))
            .collect::<Vec<_>>();

        for (coord, cell_type) in entries {
            self.index_authored_cell_summary(coord, cell_type);
        }
    }

    fn index_special_cell(&mut self, id: u64, cell_type: CellType) {
        match cell_type {
            CellType::Light => {
                self.light_ids.insert(id);
                self.editor_marker_ids.insert(id);
            }

            CellType::AudioEmitter => {
                self.audio_emitter_ids.insert(id);
                self.editor_marker_ids.insert(id);
            }

            CellType::ParticleEmitter => {
                self.particle_emitter_ids.insert(id);
                self.editor_marker_ids.insert(id);
            }
            _ => {}
        }
    }

    fn unindex_special_cell(&mut self, id: u64, cell_type: CellType) {
        match cell_type {
            CellType::Light => {
                self.light_ids.remove(&id);
                self.editor_marker_ids.remove(&id);
            }

            CellType::AudioEmitter => {
                self.audio_emitter_ids.remove(&id);
                self.editor_marker_ids.remove(&id);
            }

            CellType::ParticleEmitter => {
                self.particle_emitter_ids.remove(&id);
                self.editor_marker_ids.remove(&id);
            }
            _ => {}
        }
    }

    /// Mutable authored Cell lookup with no implicit invalidation.
    ///
    /// Callers that actually change render-relevant state must explicitly call
    /// `touch_authored_cell` with the narrowest applicable reason.
    pub fn get_mut(&mut self, coord: WorldCoord) -> Option<&mut Cell> {
        self.cells.get_mut(&coord)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    pub fn touch_authored_cell(&mut self, coord: WorldCoord, reason: DirtyReason) {
        let Some(id) = self.cells.get(&coord).map(|cell| cell.id) else {
            return;
        };

        self.bump_render_revision();
        self.mark_render_dirty(coord, reason);
        self.dirty_authored_chunks
            .insert(ChunkCoord::from_world_coord(coord));

        if reason == DirtyReason::Geometry {
            self.mark_physics_dirty(id);
            self.update_spatial_index_at(coord);
        }
    }

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
    pub fn set_cell(&mut self, coord: WorldCoord, cell_type: CellType) -> u64 {
        // Erasing an already-empty coordinate is a true no-op: do not publish
        // a render revision or dirty chunk when there is no authored content.
        if cell_type == CellType::Empty && !self.cells.contains_key(&coord) {
            return 0;
        }

        self.bump_render_revision();
        self.mark_render_dirty(coord, DirtyReason::Geometry);
        self.dirty_authored_chunks
            .insert(ChunkCoord::from_world_coord(coord));

        // Remove whatever currently occupies this coordinate first.
        let old_cell = self.cells.remove(&coord);

        if let Some(old_cell) = old_cell {
            self.unindex_authored_cell_summary(coord, old_cell.cell_type);
            self.id_to_coord.remove(&old_cell.id);
            self.runtime_state.remove(&old_cell.id);
            self.unindex_special_cell(old_cell.id, old_cell.cell_type);
            self.mark_physics_dirty(old_cell.id);

            // Empty means delete the cell entirely.
            if cell_type == CellType::Empty {
                self.update_spatial_index_at(coord);
                return old_cell.id;
            }
        } else if cell_type == CellType::Empty {
            self.update_spatial_index_at(coord);
            return 0;
        }

        let mut cell = match cell_type {
            CellType::Block => Cell::new_block(),
            CellType::Light => Cell::new_light(),
            CellType::SpawnPoint => Cell::new_spawn_point(),
            CellType::AudioEmitter => Cell::new_audio_emitter(),
            CellType::ParticleEmitter => {
                Cell::new_particle_emitter(crate::engine::particles::EmitterPreset::Fire)
            }
            _ => {
                let mut cell = Cell::default();
                cell.cell_type = cell_type;
                cell
            }
        };

        cell.id = self.generate_unique_id(coord, cell_type);

        let id = cell.id;

        self.id_to_coord.insert(id, coord);
        self.mark_physics_dirty(id);
        self.index_special_cell(id, cell_type);

        self.cells.insert(coord, cell);
        self.index_authored_cell_summary(coord, cell_type);

        self.update_spatial_index_at(coord);

        id
    }

    pub fn add_source(&mut self, source_type: CellType, position: glam::Vec3) -> u64 {
        assert!(AuthoredSource::supports(source_type));
        assert!(position.is_finite());
        self.bump_render_revision();
        let anchor = WorldCoord::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        let id = self.generate_unique_id(anchor, source_type);
        self.sources
            .insert(id, AuthoredSource::new(id, source_type, position));
        self.index_special_cell(id, source_type);
        id
    }

    // Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:08 -05:00
    pub fn paste_source(
        &mut self,
        source: &AuthoredSource,
        source_binding: Option<&ScriptBinding>,
        position: glam::Vec3,
    ) -> u64 {
        assert!(position.is_finite());
        let source_type = source.source_type();
        let anchor = WorldCoord::new(
            position.x.floor() as i32,
            position.y.floor() as i32,
            position.z.floor() as i32,
        );
        let id = self.generate_unique_id(anchor, source_type);

        let mut pasted = source.clone();
        pasted.position = position;
        pasted.cell.id = id;
        self.insert_source(pasted);

        if let Some(binding) = source_binding {
            let mut binding = binding.clone();
            binding.target_identity = id;
            self.script_bindings.push(binding);
        }

        id
    }

    pub fn insert_source(&mut self, source: AuthoredSource) {
        self.observe_authored_id(source.cell.id);
        self.index_special_cell(source.cell.id, source.cell.cell_type);
        self.sources.insert(source.cell.id, source);
        self.bump_render_revision();
    }

    pub fn remove_source(&mut self, id: u64) -> Option<AuthoredSource> {
        let source = self.sources.remove(&id)?;
        self.unindex_special_cell(id, source.cell.cell_type);
        self.runtime_state.remove(&id);
        self.script_bindings
            .retain(|binding| binding.target_identity != id);
        self.bump_render_revision();
        Some(source)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:08 -05:00
    pub fn touch_authored_source(&mut self, id: u64) {
        if self.sources.contains_key(&id) {
            self.bump_render_revision();
        }
    }

    pub fn set_source_position(&mut self, id: u64, position: glam::Vec3) -> Result<(), String> {
        if !position.is_finite() {
            return Err("Source position must contain finite values".into());
        }
        let source = self
            .sources
            .get_mut(&id)
            .ok_or_else(|| format!("Unknown authored source {id}"))?;
        source.position = position;
        self.bump_render_revision();
        Ok(())
    }

    /// Internal helper to update the ID index when an ID is changed manually
    /// (for example while loading persisted cells).
    pub(crate) fn update_id_mapping(&mut self, old_id: u64, new_id: u64, coord: WorldCoord) {
        if let Some(mut source) = self.sources.remove(&old_id) {
            self.unindex_special_cell(old_id, source.cell.cell_type);
            source.cell.id = new_id;
            self.index_special_cell(new_id, source.cell.cell_type);
            self.sources.insert(new_id, source);
            self.observe_authored_id(new_id);
            return;
        }

        let cell_type = self.cells.get(&coord).map(|cell| cell.cell_type);

        if let Some(cell_type) = cell_type {
            self.unindex_special_cell(old_id, cell_type);
            self.index_special_cell(new_id, cell_type);
        }

        self.id_to_coord.remove(&old_id);
        self.id_to_coord.insert(new_id, coord);
        self.observe_authored_id(new_id);
    }

    /// Rebuilds the ID and specialized lookup indices.
    ///
    /// Call this if the authored cell map is replaced directly,
    /// such as during undo/redo or world loading.
    pub fn rebuild_id_mapping(&mut self) {
        self.bump_render_revision();

        let coords: Vec<WorldCoord> = self.cells.keys().copied().collect();

        for coord in coords {
            self.mark_render_dirty(coord, DirtyReason::Geometry);
        }

        self.id_to_coord.clear();

        for (coord, cell) in &self.cells {
            self.id_to_coord.insert(cell.id, *coord);
        }

        self.rebuild_authored_cell_summary();
        self.rebuild_spatial_index();
        self.rebuild_light_and_marker_indices();
        self.rebuild_actor_module_index();

        self.dirty_authored_chunks = self
            .cells
            .keys()
            .map(|coord| ChunkCoord::from_world_coord(*coord))
            .collect();
    }

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:28 -05:00
    // Undo/Redo swaps Cell maps but only refreshes indices and render chunks
    // whose authored Cell value actually changed.
    pub(crate) fn swap_authored_cells_for_history(
        &mut self,
        replacement: std::collections::HashMap<WorldCoord, Cell>,
    ) -> std::collections::HashMap<WorldCoord, Cell> {
        let current = std::mem::replace(&mut self.cells, replacement);
        let mut changed = current
            .iter()
            .filter_map(|(coord, old)| (self.cells.get(coord) != Some(old)).then_some(*coord))
            .collect::<Vec<_>>();
        changed.extend(
            self.cells
                .keys()
                .filter(|coord| !current.contains_key(coord))
                .copied(),
        );
        let has_cell_changes = !changed.is_empty();

        for coord in &changed {
            if let Some(old) = current.get(coord) {
                self.unindex_authored_cell_summary(*coord, old.cell_type);
                self.id_to_coord.remove(&old.id);
                self.unindex_special_cell(old.id, old.cell_type);
                self.mark_physics_dirty(old.id);
                self.runtime_state.remove(&old.id);
            }
        }
        for coord in changed {
            // Copy the compact index keys before mutating World-owned indices.
            if let Some((id, cell_type)) =
                self.cells.get(&coord).map(|cell| (cell.id, cell.cell_type))
            {
                self.index_authored_cell_summary(coord, cell_type);
                self.id_to_coord.insert(id, coord);
                self.index_special_cell(id, cell_type);
                self.mark_physics_dirty(id);
            }
            self.bump_render_revision();
            self.mark_render_dirty(coord, DirtyReason::Geometry);
            self.dirty_authored_chunks
                .insert(ChunkCoord::from_world_coord(coord));
            self.update_spatial_index_at(coord);
        }
        if !has_cell_changes {
            // Actor/source/project history can change without changing Cell chunks.
            self.bump_render_revision();
        }
        current
    }

    pub(crate) fn rebuild_light_and_marker_indices(&mut self) {
        self.light_ids.clear();
        self.editor_marker_ids.clear();
        self.audio_emitter_ids.clear();
        self.particle_emitter_ids.clear();

        let entries: Vec<(u64, CellType)> = self
            .cells
            .values()
            .filter(|cell| {
                !self
                    .runtime_state
                    .get(&cell.id)
                    .map_or(false, |rs| rs.is_deleted)
            })
            .chain(self.runtime_cells.values())
            .map(|cell| (cell.id, cell.cell_type))
            .collect();

        let source_entries: Vec<(u64, CellType)> = self
            .sources
            .values()
            .map(|source| (source.cell.id, source.cell.cell_type))
            .collect();

        for (id, cell_type) in entries.into_iter().chain(source_entries) {
            self.index_special_cell(id, cell_type);
        }
    }

    pub(crate) fn rebuild_audio_emitter_index(&mut self) {
        self.rebuild_light_and_marker_indices();
    }

    pub fn add_actor(&mut self, identity: impl Into<String>, position: glam::Vec3) -> u64 {
        self.bump_render_revision();

        let id = self.generate_unique_actor_id();

        let raw_identity: String = identity.into();
        let trimmed = raw_identity.trim();

        let final_identity = if trimmed.is_empty() {
            format!("Actor_{}", id)
        } else {
            trimmed.to_string()
        };

        let actor = crate::world::Actor::new(id, final_identity, position);

        self.actors.insert(id, actor);

        id
    }

    // Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:08 -05:00
    pub fn paste_actor(
        &mut self,
        source: &crate::world::Actor,
        source_bindings: &[crate::scripting::binding::ActorScriptBinding],
        position: glam::Vec3,
    ) -> u64 {
        assert!(position.is_finite());
        self.bump_render_revision();

        let id = self.generate_unique_actor_id();
        let mut actor = source.clone();
        actor.id = id;
        actor.position = position;
        self.actors.insert(id, actor);

        for binding in source_bindings {
            let mut binding = binding.clone();
            binding.actor_id = id;
            self.actor_script_bindings.push(binding);
        }
        self.rebuild_actor_module_index();

        id
    }

    pub fn remove_actor(&mut self, actor_id: u64) -> Option<crate::world::Actor> {
        self.bump_render_revision();

        self.actor_script_bindings
            .retain(|binding| binding.actor_id != actor_id);

        self.actor_hierarchy.remove_actor(actor_id);

        let removed = self.actors.remove(&actor_id);

        self.rebuild_actor_module_index();

        removed
    }

    pub fn generate_unique_actor_id(&mut self) -> u64 {
        let id = self.next_actor_id;

        self.next_actor_id = self
            .next_actor_id
            .checked_add(1)
            .expect("Authored Actor ID space exhausted");

        id
    }

    pub fn observe_authored_actor_id(&mut self, id: u64) {
        if id >= self.next_actor_id {
            self.next_actor_id = id
                .checked_add(1)
                .expect("Authored Actor ID space exhausted");
        }
    }

    pub(crate) fn generate_unique_id(&mut self, _coord: WorldCoord, _cell_type: CellType) -> u64 {
        let id = self.next_cell_id;

        self.next_cell_id = self
            .next_cell_id
            .checked_add(1)
            .expect("Authored cell ID space exhausted");

        id
    }

    /// Advances the allocator past an existing authored ID.
    ///
    /// Used when loading IDs that were already assigned on disk.
    pub(crate) fn observe_authored_id(&mut self, id: u64) {
        if id >= self.next_cell_id {
            self.next_cell_id = id.checked_add(1).expect("Authored cell ID space exhausted");
        }
    }

    /// Moves a collection of authored cells by a given delta offset,
    /// preserving their cell IDs.
    ///
    /// Returns Err if the movement destination collides with unrelated cells.
    pub fn move_cells(
        &mut self,
        source_coords: &[WorldCoord],
        delta: WorldCoord,
    ) -> Result<(), String> {
        if delta == WorldCoord::new(0, 0, 0) || source_coords.is_empty() {
            return Ok(());
        }

        let source_set: std::collections::HashSet<WorldCoord> =
            source_coords.iter().copied().collect();

        // Validate that no destination coordinate collides with an unrelated cell.
        for &src in source_coords {
            let dest = WorldCoord::new(src.x + delta.x, src.y + delta.y, src.z + delta.z);

            if self.cells.contains_key(&dest) && !source_set.contains(&dest) {
                return Err("Destination is occupied by an unrelated cell".to_string());
            }
        }

        self.bump_render_revision();

        // Remove all source cells first to avoid self-overwrite when
        // moving onto positions belonging to the same selection.
        let mut moved = Vec::new();

        for &src in source_coords {
            if let Some(cell) = self.cells.remove(&src) {
                self.unindex_authored_cell_summary(src, cell.cell_type);
                self.id_to_coord.remove(&cell.id);
                self.mark_physics_dirty(cell.id);
                self.mark_render_dirty(src, DirtyReason::Geometry);

                self.dirty_authored_chunks
                    .insert(ChunkCoord::from_world_coord(src));

                self.update_spatial_index_at(src);

                moved.push((src, cell));
            }
        }

        // Reinsert cells at their destination coordinates.
        for (old_coord, cell) in moved {
            let dest = WorldCoord::new(
                old_coord.x + delta.x,
                old_coord.y + delta.y,
                old_coord.z + delta.z,
            );

            let id = cell.id;

            let cell_type = cell.cell_type;
            self.cells.insert(dest, cell);
            self.index_authored_cell_summary(dest, cell_type);
            self.id_to_coord.insert(id, dest);
            self.mark_physics_dirty(id);
            self.mark_render_dirty(dest, DirtyReason::Geometry);

            self.dirty_authored_chunks
                .insert(ChunkCoord::from_world_coord(dest));

            self.update_spatial_index_at(dest);
        }

        Ok(())
    }

    /// Pastes a group of copied cells from the clipboard at the given target pivot.
    ///
    /// Generates new unique IDs for every pasted cell and remaps script bindings.
    #[cfg(feature = "editor")]
    pub fn paste_cells(
        &mut self,
        clipboard_cells: &[crate::editor::ClipboardCell],
        target_pivot: WorldCoord,
    ) -> Result<Vec<WorldCoord>, String> {
        if clipboard_cells.is_empty() {
            return Ok(Vec::new());
        }

        // Validate that no destination coordinate is occupied.
        for item in clipboard_cells {
            let dest = WorldCoord::new(
                target_pivot.x + item.offset.x,
                target_pivot.y + item.offset.y,
                target_pivot.z + item.offset.z,
            );

            if self.cells.contains_key(&dest) {
                return Err("Destination is occupied".to_string());
            }
        }

        self.bump_render_revision();

        let mut pasted_coords = Vec::with_capacity(clipboard_cells.len());

        for item in clipboard_cells {
            let dest = WorldCoord::new(
                target_pivot.x + item.offset.x,
                target_pivot.y + item.offset.y,
                target_pivot.z + item.offset.z,
            );

            let mut cell = item.cell.clone();
            let cell_type = cell.cell_type;

            let new_id = self.generate_unique_id(dest, cell_type);

            cell.id = new_id;

            self.cells.insert(dest, cell);
            self.index_authored_cell_summary(dest, cell_type);
            self.id_to_coord.insert(new_id, dest);
            self.mark_physics_dirty(new_id);
            self.mark_render_dirty(dest, DirtyReason::Geometry);

            self.index_special_cell(new_id, cell_type);

            self.dirty_authored_chunks
                .insert(ChunkCoord::from_world_coord(dest));

            if let Some(binding) = &item.script_binding {
                let mut new_binding = binding.clone();
                new_binding.target_identity = new_id;
                self.script_bindings.push(new_binding);
            }

            self.update_spatial_index_at(dest);

            pasted_coords.push(dest);
        }

        Ok(pasted_coords)
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
#[cfg(test)]
mod authored_cell_performance_index_tests {
    use super::*;

    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 10:46 -05:00
    #[test]
    fn erasing_missing_cell_is_a_render_noop_but_real_erase_invalidates() {
        let mut world = World::new();
        let missing = WorldCoord::new(-4, 2, 9);
        let revision = world.render_revision();

        assert_eq!(world.set_cell(missing, CellType::Empty), 0);
        assert_eq!(world.render_revision(), revision);
        assert!(world.drain_render_dirty_cells().is_empty());

        let id = world.set_cell(missing, CellType::Block);
        let before_erase = world.render_revision();
        assert_eq!(world.set_cell(missing, CellType::Empty), id);
        assert!(world.render_revision() > before_erase);
        assert!(world.get(missing).is_none());
    }

    #[test]
    fn mutable_lookup_does_not_invalidate_render_state_until_touched() {
        let mut world = World::new();
        let coord = WorldCoord::new(1, 2, 3);
        world.set_cell(coord, CellType::Block);
        let revision = world.render_revision();
        let _ = world.drain_render_dirty_cells();

        assert!(world.get_mut(coord).is_some());
        assert_eq!(world.render_revision(), revision);
        assert!(world.drain_render_dirty_cells().is_empty());

        world.touch_authored_cell(coord, DirtyReason::MaterialOrOffset);
        assert!(world.render_revision() > revision);
        assert_eq!(
            world.drain_render_dirty_cells().get(&coord),
            Some(&DirtyReason::MaterialOrOffset)
        );
    }

    #[test]
    fn block_count_is_indexed_without_duplicate_block_navigation_storage() {
        let mut world = World::new();
        for x in 0..10_000 {
            world.set_cell(WorldCoord::new(x, 0, 0), CellType::Block);
        }
        let spawn = WorldCoord::new(0, 1, 0);
        world.set_cell(spawn, CellType::SpawnPoint);

        assert_eq!(world.authored_cell_count(CellType::Block), 10_000);
        assert_eq!(world.authored_cell_count(CellType::SpawnPoint), 1);
        assert_eq!(
            world.iter_editor_navigator_coords(CellType::Block).count(),
            0
        );
        assert_eq!(
            world
                .iter_editor_navigator_coords(CellType::SpawnPoint)
                .collect::<Vec<_>>(),
            vec![spawn]
        );
    }
}

#[cfg(test)]
mod actor_hierarchy_compatibility_tests {
    use super::World;
    use glam::Vec3;

    #[test]
    fn actor_creation_starts_at_project_root() {
        let mut world = World::new();

        let actor_id = world.add_actor("Coin", Vec3::ZERO);

        assert!(world.actors.contains_key(&actor_id));

        assert_eq!(world.actor_hierarchy.directory_for_actor(actor_id), None);
    }

    #[test]
    fn removing_actor_removes_hierarchy_membership() {
        let mut world = World::new();

        let actor_id = world.add_actor("Coin", Vec3::ZERO);

        let directory_id = world
            .actor_hierarchy
            .create_directory(None, "Collectibles")
            .unwrap();

        world
            .actor_hierarchy
            .set_actor_directory(actor_id, Some(directory_id))
            .unwrap();

        assert_eq!(
            world.actor_hierarchy.directory_for_actor(actor_id),
            Some(directory_id)
        );

        assert!(world.remove_actor(actor_id).is_some());

        assert!(!world.actors.contains_key(&actor_id));

        assert_eq!(world.actor_hierarchy.directory_for_actor(actor_id), None);
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:08 -05:00
#[cfg(test)]
mod viewport_clipboard_paste_tests {
    use super::*;
    use crate::scripting::binding::{ActorScriptBinding, ScriptBinding};
    use crate::world::CellType;
    use glam::Vec3;

    #[test]
    fn actor_paste_allocates_fresh_id_and_preserves_authored_state_and_modules() {
        let mut world = World::new();
        let original_id = world.add_actor("PatrolGuard", Vec3::new(1.25, 2.5, 3.75));
        {
            let actor = world.actors.get_mut(&original_id).unwrap();
            actor.rotation = Vec3::new(0.1, 0.2, 0.3);
            actor.scale = Vec3::new(1.2, 0.8, 1.4);
            actor.visible = false;
            actor.enabled = false;
            actor.package = Some("character_guard".to_string());
            actor.physics.mass = 3.5;
            actor.physics.collision_size = Vec3::new(0.8, 1.9, 0.7);
            actor
                .attributes
                .insert("hp".to_string(), crate::world::AttributeValue::Number(75.0));
        }
        world.actor_script_bindings.push(ActorScriptBinding::new(
            original_id,
            "scripts/guard_brain.aeo",
        ));

        let source = world.actors.get(&original_id).unwrap().clone();
        let bindings: Vec<_> = world
            .actor_script_bindings
            .iter()
            .filter(|binding| binding.actor_id == original_id)
            .cloned()
            .collect();
        let target = Vec3::new(9.125, 4.5, -2.875);

        let pasted_id = world.paste_actor(&source, &bindings, target);
        let pasted = world.actors.get(&pasted_id).unwrap();

        assert_ne!(pasted_id, original_id);
        assert_eq!(pasted.identity, source.identity);
        assert_eq!(pasted.position, target);
        assert_eq!(pasted.rotation, source.rotation);
        assert_eq!(pasted.scale, source.scale);
        assert_eq!(pasted.visible, source.visible);
        assert_eq!(pasted.enabled, source.enabled);
        assert_eq!(pasted.package, source.package);
        assert_eq!(pasted.physics, source.physics);
        assert_eq!(pasted.attributes, source.attributes);
        assert!(
            world
                .actor_script_bindings
                .iter()
                .any(|binding| binding.actor_id == pasted_id
                    && binding.script_path == "scripts/guard_brain.aeo")
        );
    }

    #[test]
    fn source_paste_allocates_fresh_id_preserves_precision_settings_and_binding() {
        let mut world = World::new();
        let original_id = world.add_source(CellType::AudioEmitter, Vec3::new(1.125, 2.25, 3.375));
        {
            let source = world.source_mut(original_id).unwrap();
            source.cell.audio = "audio/ambience/wind.wav".to_string();
            source.cell.looped = true;
            source.cell.loop_interval = 1.75;
            source.cell.volume = 0.42;
            source.cell.visible = true;
        }
        world
            .script_bindings
            .push(ScriptBinding::new(original_id, "scripts/source.aeo"));

        let source = world.source(original_id).unwrap().clone();
        let binding = world
            .script_bindings
            .iter()
            .find(|binding| binding.target_identity == original_id)
            .cloned();
        let target = Vec3::new(7.125, 8.25, -9.375);

        let pasted_id = world.paste_source(&source, binding.as_ref(), target);
        let pasted = world.source(pasted_id).unwrap();

        assert_ne!(pasted_id, original_id);
        assert_eq!(pasted.position, target);
        assert_eq!(pasted.cell.cell_type, CellType::AudioEmitter);
        assert_eq!(pasted.cell.audio, source.cell.audio);
        assert_eq!(pasted.cell.looped, source.cell.looped);
        assert_eq!(pasted.cell.loop_interval, source.cell.loop_interval);
        assert_eq!(pasted.cell.volume, source.cell.volume);
        assert_eq!(pasted.cell.visible, source.cell.visible);
        assert!(
            world
                .script_bindings
                .iter()
                .any(|binding| binding.target_identity == pasted_id
                    && binding.script_path == "scripts/source.aeo")
        );
    }
}
