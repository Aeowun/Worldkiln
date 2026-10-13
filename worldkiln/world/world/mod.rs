//! Core World state and world-level data structures.
//!
//! Owns authored and runtime cell storage, ID and spatial indices, render
//! invalidation state, scene settings, and the specialized lookup indices
//! used by rendering and editor systems.
use crate::scripting::binding::{ActorScriptBinding, ScriptBinding};
use crate::world::actor::Actor;
use crate::world::actor_hierarchy::ActorHierarchy;
use crate::world::cell::{Cell, CellType, ChunkCoord, RuntimeCellState};
use crate::world::coordinate::WorldCoord;
use crate::world::source::AuthoredSource;
use crate::world::terrain::TerrainState;
use glam::Vec3;
use std::collections::VecDeque;
use std::collections::{HashMap, HashSet};

pub mod dirty;
pub mod mutations;
pub mod particles;
pub mod queries;
pub mod runtime;
pub mod settings;
pub mod spatial_index;

pub use dirty::{DirtyReason, RenderChange, RenderChanges};
pub use settings::{LightingSettings, RenderDistanceSettings, SkySettings};
pub use spatial_index::{ChunkCellIndex, WorldSpatialIndex};

#[derive(Clone)]
pub struct World {
    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:00 -05:00
    /// Authored Terrain plus a session-only runtime overlay.
    pub terrain: TerrainState,
    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
    /// Navigation bounds are independent of Terrain generation layers.
    pub navmesh_volumes: Vec<crate::navigation::NavMeshBoundsVolume>,
    pub navmesh_settings: crate::navigation::NavMeshGenerationSettings,

    // Authored grid data. We use a HashMap because the world is unbounded
    // and most coordinates are empty.
    pub(crate) cells: HashMap<WorldCoord, Cell>,

    /// Authored non-voxel scene sources keyed by their persistent Cell handle.
    pub(crate) sources: HashMap<u64, AuthoredSource>,

    /// Cached authored-world bounds used by runtime navigation.
    ///
    /// The cache is keyed by render_revision so world mutations invalidate it
    /// automatically without requiring every mutation path to touch it.
    pub(crate) navigation_bounds_cache:
        std::cell::RefCell<Option<(u64, Option<(WorldCoord, WorldCoord)>)>>,

    // Temporary runtime-only overrides for cell state, keyed by Cell ID.
    pub(crate) runtime_state: HashMap<u64, RuntimeCellState>,

    // Tracks which cells have had physics-relevant properties changed at runtime.
    pub(crate) physics_dirty_cells: HashSet<u64>,

    // Optimized lookup for cell coordinates by ID.
    pub(crate) id_to_coord: HashMap<u64, WorldCoord>,

    /// Next globally unique authored cell ID.
    ///
    /// This is persistent world state. It must not depend on which chunks
    /// happen to be resident in memory.
    pub(crate) next_cell_id: u64,

    // Storage for cells created at runtime via cell.new()
    pub(crate) runtime_cells: HashMap<u64, Cell>,

    // ID -> Coord index for runtime cells
    pub(crate) runtime_id_to_coord: HashMap<u64, WorldCoord>,

    // Coord -> ID index for runtime cells (spatial index)
    pub(crate) coord_to_runtime_id: HashMap<WorldCoord, u64>,

    /// Resident authored/runtime audio emitters.
    /// Audio uses this index rather than scanning every resident cell each frame.
    pub(crate) audio_emitter_ids: HashSet<u64>,

    /// Dedicated index for point lights (`CellType::Light`).
    pub(crate) light_ids: HashSet<u64>,

    /// Dedicated index for editor markers
    /// (`CellType::Light` and `CellType::AudioEmitter`).
    pub(crate) editor_marker_ids: HashSet<u64>,

    // The world wide gravity vector used by the physics simulation.
    pub gravity: Vec3,

    /// Transient simulation and a resident emitter index; never persisted.
    pub particles: crate::engine::particles::ParticleSystem,
    pub(crate) particle_emitter_ids: HashSet<u64>,

    // Authoritative scene lighting settings.
    pub lighting: LightingSettings,

    pub render_distance: RenderDistanceSettings,

    // Authoritative scene sky settings.
    pub sky: SkySettings,

    /// Authored script bindings for entities in this world.
    pub script_bindings: Vec<ScriptBinding>,

    /// Authored Actors in the world.
    pub actors: HashMap<u64, Actor>,

    /// Authored script bindings for Actors in this world.
    pub actor_script_bindings: Vec<ActorScriptBinding>,

    /// Runtime/editor lookup index for Actor-attached Modules.
    ///
    /// This is derived from `actor_script_bindings` and is not persisted.
    pub(crate) actor_module_index: crate::world::ActorModuleIndex,

    /// Project-level hierarchy metadata organizing authored Actors.
    pub actor_hierarchy: ActorHierarchy,

    /// Next globally unique authored Actor ID.
    pub next_actor_id: u64,

    /// Paths to script files that are globally disabled.
    pub disabled_scripts: Vec<String>,

    /// Selected character package name for spawning.
    pub selected_character: String,

    /// Selected controller profile name.
    pub selected_controller: String,

    /// Selected camera profile name.
    pub selected_camera: String,

    // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
    /// Play-only camera override; Project Settings remains the authored default.
    pub runtime_camera_script: Option<String>,
    /// Which package was actually instantiated by the current Play scene.
    pub runtime_camera_applied: Option<String>,

    /// Default Play-mode mouse cursor visibility setting.
    pub cursor_visible: bool,

    /// Default Play-mode mouse screen/cursor locking setting.
    pub screen_locked: bool,

    /// Incremented whenever World geometry or render-relevant properties change.
    pub(crate) render_revision: u64,

    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    /// O(1) authored Cell counts by type. Editor panels must not duplicate or
    /// sort the complete authored Block population.
    pub(crate) authored_cell_type_counts: HashMap<CellType, usize>,

    /// Coordinate indices only for small non-Block Cell categories that need
    /// individual World-panel navigation.
    pub(crate) editor_navigator_coords: HashMap<CellType, HashSet<WorldCoord>>,

    /// Lightweight runtime-only record of coordinates that need render invalidation.
    pub(crate) render_dirty_cells: std::cell::RefCell<HashMap<WorldCoord, DirtyReason>>,

    /// Non-destructive bounded stream for persistent renderer caches.
    pub(crate) render_change_journal: std::cell::RefCell<VecDeque<RenderChange>>,
    pub(crate) render_change_floor_revision: std::cell::Cell<u64>,

    /// Persistent spatial index partitioning cells into 16x16x16 chunks.
    pub spatial_index: WorldSpatialIndex,

    /// Authored chunks changed while resident.
    pub(crate) dirty_authored_chunks: HashSet<ChunkCoord>,
}

impl World {
    // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
    /// Get the currently requested Play camera, falling back to Project Settings.
    pub fn active_camera_script(&self) -> &str {
        self.runtime_camera_script
            .as_deref()
            .unwrap_or(&self.selected_camera)
    }

    pub fn set_runtime_camera_script(&mut self, name: &str) -> Result<(), String> {
        // Names correspond to camera package directories, not arbitrary paths.
        if name.is_empty()
            || name.len() > 128
            || !name
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-')
        {
            return Err(
                "camera.set_script expects a camera package name using letters, numbers, _ or -"
                    .into(),
            );
        }
        self.runtime_camera_script = if name == self.selected_camera {
            None
        } else {
            Some(name.to_string())
        };
        Ok(())
    }

    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
    /// The World and Terrain have separate revision clocks.
    pub fn navigation_revision(&self) -> u64 {
        // Water level can be edited without regenerating Terrain. Include its
        // visual waterline in navigation freshness even when topology is unchanged.
        self.render_revision
            ^ self.terrain.render_revision().rotate_left(31)
            ^ (self.terrain.config.plains.water_level.to_bits() as u64).rotate_left(13)
    }

    pub fn new() -> Self {
        Self {
            terrain: TerrainState::default(),
            navmesh_volumes: Vec::new(),
            navmesh_settings: crate::navigation::NavMeshGenerationSettings::default(),
            cells: HashMap::new(),
            sources: HashMap::new(),
            navigation_bounds_cache: std::cell::RefCell::new(None),
            runtime_state: HashMap::new(),
            physics_dirty_cells: HashSet::new(),
            id_to_coord: HashMap::new(),
            next_cell_id: 10_000_000,
            runtime_cells: HashMap::new(),
            runtime_id_to_coord: HashMap::new(),
            coord_to_runtime_id: HashMap::new(),
            audio_emitter_ids: HashSet::new(),
            light_ids: HashSet::new(),
            editor_marker_ids: HashSet::new(),
            gravity: Vec3::new(0.0, -9.81, 0.0),
            particles: Default::default(),
            particle_emitter_ids: HashSet::new(),
            lighting: LightingSettings::default(),
            render_distance: RenderDistanceSettings::default(),
            sky: SkySettings::default(),
            script_bindings: Vec::new(),
            actors: HashMap::new(),
            actor_script_bindings: Vec::new(),
            actor_module_index: crate::world::ActorModuleIndex::default(),
            actor_hierarchy: ActorHierarchy::new(),
            next_actor_id: 20_000_000,
            disabled_scripts: Vec::new(),
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 15:32 -05:00
            // Worldkiln 0.8.5 | WO MAT-010 | 2026-10-10 18:11 -05:00
            // Empty package catalogs have no Player selection. Never persist a
            // character name for a package that is absent from the registry.
            selected_character: crate::character::package::available_package_names()
                .into_iter()
                .next()
                .unwrap_or_default(),
            selected_controller: "thirdPerson_Controller".to_string(),
            selected_camera: "thirdPerson".to_string(),
            runtime_camera_script: None,
            runtime_camera_applied: None,
            cursor_visible: false,
            screen_locked: true,
            render_revision: 1,
            authored_cell_type_counts: HashMap::new(),
            editor_navigator_coords: HashMap::new(),
            render_dirty_cells: std::cell::RefCell::new(HashMap::new()),
            render_change_journal: std::cell::RefCell::new(VecDeque::new()),
            render_change_floor_revision: std::cell::Cell::new(0),
            spatial_index: WorldSpatialIndex::default(),
            dirty_authored_chunks: HashSet::new(),
        }
    }

    /// Returns the bounds of authored world cells.
    ///
    /// The result is cached until render_revision changes.
    pub fn navigation_bounds(&self) -> Option<(WorldCoord, WorldCoord)> {
        let revision = self.render_revision;
        let cached = *self.navigation_bounds_cache.borrow();

        if let Some((cached_revision, bounds)) = cached {
            if cached_revision == revision {
                return bounds;
            }
        }

        let mut coords = self.cells.keys().copied();

        let Some(first) = coords.next() else {
            *self.navigation_bounds_cache.borrow_mut() = Some((revision, None));
            return None;
        };

        let mut min = first;
        let mut max = first;

        for coord in coords {
            min.x = min.x.min(coord.x);
            min.y = min.y.min(coord.y);
            min.z = min.z.min(coord.z);

            max.x = max.x.max(coord.x);
            max.y = max.y.max(coord.y);
            max.z = max.z.max(coord.z);
        }

        let bounds = Some((min, max));

        *self.navigation_bounds_cache.borrow_mut() = Some((revision, bounds));

        bounds
    }
}

impl Default for World {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::editor::ClipboardCell;
    use crate::editor::editor::History;
    use crate::project::ProjectHierarchy;
    use crate::scripting::binding::ScriptBinding;
    use crate::world::cell::CellType;
    use glam::Vec3;

    #[test]
    fn test_copy_preserves_full_cell_data_and_relative_offsets() {
        let mut world = World::new();
        let c1 = WorldCoord::new(10, 2, 10);
        let c2 = WorldCoord::new(11, 2, 10);

        let id1 = world.set_cell(c1, CellType::Block);

        if let Some(cell) = world.get_mut(c1) {
            cell.color_rgb = Vec3::new(1.0, 0.0, 0.0);
            cell.texture = "CustomTexture".to_string();
            cell.entity_identity = Some("MyEntity".to_string());
            cell.attributes.insert(
                "hp".to_string(),
                crate::world::cell::AttributeValue::Number(100.0),
            );
        }

        let id2 = world.set_cell(c2, CellType::Light);

        world
            .script_bindings
            .push(ScriptBinding::new(id1, "scripts/player.aeo"));

        let pivot = c1;
        let cell1 = world.get(c1).unwrap().clone();
        let cell2 = world.get(c2).unwrap().clone();

        let clip_cell1 = ClipboardCell {
            offset: WorldCoord::new(c1.x - pivot.x, c1.y - pivot.y, c1.z - pivot.z),
            cell: cell1,
            script_binding: world
                .script_bindings
                .iter()
                .find(|b| b.target_identity == id1)
                .cloned(),
        };

        let clip_cell2 = ClipboardCell {
            offset: WorldCoord::new(c2.x - pivot.x, c2.y - pivot.y, c2.z - pivot.z),
            cell: cell2,
            script_binding: world
                .script_bindings
                .iter()
                .find(|b| b.target_identity == id2)
                .cloned(),
        };

        assert_eq!(clip_cell1.cell.color_rgb, Vec3::new(1.0, 0.0, 0.0));
        assert_eq!(clip_cell1.cell.texture, "CustomTexture");
        assert_eq!(
            clip_cell1.cell.entity_identity,
            Some("MyEntity".to_string())
        );
        assert_eq!(
            clip_cell1.script_binding.unwrap().script_path,
            "scripts/player.aeo"
        );

        assert_eq!(clip_cell1.offset, WorldCoord::new(0, 0, 0));
        assert_eq!(clip_cell2.offset, WorldCoord::new(1, 0, 0));
    }

    #[test]
    fn test_authored_ids_are_monotonic() {
        let mut world = World::new();

        let id1 = world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        let id2 = world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);
        let id3 = world.set_cell(WorldCoord::new(2, 0, 0), CellType::Block);

        assert_eq!(id1, 10_000_000);
        assert_eq!(id2, 10_000_001);
        assert_eq!(id3, 10_000_002);
        assert_eq!(world.next_cell_id, 10_000_003);
    }

    #[test]
    fn test_deleted_authored_id_is_not_reused() {
        let mut world = World::new();

        let first = WorldCoord::new(0, 0, 0);
        let second = WorldCoord::new(1, 0, 0);

        let id1 = world.set_cell(first, CellType::Block);

        world.set_cell(first, CellType::Empty);

        let id2 = world.set_cell(second, CellType::Block);

        assert_eq!(id2, id1 + 1);
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_observing_existing_id_advances_allocator() {
        let mut world = World::new();

        world.observe_authored_id(50_000_000);

        let id = world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        assert_eq!(id, 50_000_001);
        assert_eq!(world.next_cell_id, 50_000_002);
    }

    #[test]
    fn test_paste_creates_new_ids_and_preserves_properties_bindings_and_layout() {
        let mut world = World::new();
        let c1 = WorldCoord::new(10, 2, 10);
        let c2 = WorldCoord::new(11, 2, 10);

        let id1 = world.set_cell(c1, CellType::Block);

        if let Some(cell) = world.get_mut(c1) {
            cell.color_rgb = Vec3::new(0.8, 0.1, 0.2);
            cell.entity_identity = Some("Hero".to_string());
        }

        let id2 = world.set_cell(c2, CellType::Block);

        world
            .script_bindings
            .push(ScriptBinding::new(id1, "scripts/hero.aeo"));

        let clipboard = vec![
            ClipboardCell {
                offset: WorldCoord::new(0, 0, 0),
                cell: world.get(c1).unwrap().clone(),
                script_binding: world
                    .script_bindings
                    .iter()
                    .find(|b| b.target_identity == id1)
                    .cloned(),
            },
            ClipboardCell {
                offset: WorldCoord::new(1, 0, 0),
                cell: world.get(c2).unwrap().clone(),
                script_binding: None,
            },
        ];

        let target_pivot = WorldCoord::new(20, 5, 20);

        let pasted_coords = world
            .paste_cells(&clipboard, target_pivot)
            .expect("Paste should succeed");

        let pasted_cell1 = world.get(WorldCoord::new(20, 5, 20)).unwrap();
        let pasted_cell2 = world.get(WorldCoord::new(21, 5, 20)).unwrap();

        assert_ne!(pasted_cell1.id, id1);
        assert_ne!(pasted_cell2.id, id2);

        assert_eq!(pasted_cell1.color_rgb, Vec3::new(0.8, 0.1, 0.2));
        assert_eq!(pasted_cell1.entity_identity, Some("Hero".to_string()));

        let new_binding = world
            .script_bindings
            .iter()
            .find(|b| b.target_identity == pasted_cell1.id);

        assert!(new_binding.is_some());
        assert_eq!(new_binding.unwrap().script_path, "scripts/hero.aeo");

        assert_eq!(
            pasted_coords,
            vec![WorldCoord::new(20, 5, 20), WorldCoord::new(21, 5, 20)]
        );
    }

    #[test]
    fn test_move_preserves_ids_bindings_layout_and_allows_self_overlap() {
        let mut world = World::new();
        let c1 = WorldCoord::new(10, 2, 10);
        let c2 = WorldCoord::new(11, 2, 10);

        let id1 = world.set_cell(c1, CellType::Block);
        let id2 = world.set_cell(c2, CellType::Block);

        world
            .script_bindings
            .push(ScriptBinding::new(id1, "scripts/block1.aeo"));

        let source_coords = vec![c1, c2];
        let delta = WorldCoord::new(1, 0, 0);

        world
            .move_cells(&source_coords, delta)
            .expect("Move onto own positions should succeed");

        let moved1 = world.get(WorldCoord::new(11, 2, 10)).unwrap();
        let moved2 = world.get(WorldCoord::new(12, 2, 10)).unwrap();

        assert_eq!(moved1.id, id1);
        assert_eq!(moved2.id, id2);

        assert_eq!(world.resolve_cell_id(id1), Some(WorldCoord::new(11, 2, 10)));

        assert!(world.get(c1).is_none());
    }

    #[test]
    fn test_destination_collision_rejects_atomically() {
        let mut world = World::new();
        let c1 = WorldCoord::new(10, 2, 10);
        let c_obstacle = WorldCoord::new(15, 2, 10);

        let id1 = world.set_cell(c1, CellType::Block);
        let _id_obs = world.set_cell(c_obstacle, CellType::Block);

        let res = world.move_cells(&[c1], WorldCoord::new(5, 0, 0));

        assert!(res.is_err());
        assert_eq!(world.get(c1).unwrap().id, id1);
    }

    #[test]
    fn test_noop_move_does_nothing() {
        let mut world = World::new();
        let c1 = WorldCoord::new(10, 2, 10);

        let id1 = world.set_cell(c1, CellType::Block);

        assert!(world.move_cells(&[c1], WorldCoord::new(0, 0, 0)).is_ok());

        assert_eq!(world.get(c1).unwrap().id, id1);
    }

    #[test]
    fn test_undo_and_redo_restore_exact_previous_world() {
        let mut world = World::new();
        let c1 = WorldCoord::new(10, 2, 10);

        let id1 = world.set_cell(c1, CellType::Block);
        let source_start = Vec3::new(1.25, 2.5, 3.75);
        let source_end = Vec3::new(9.5, 8.25, 7.125);
        let source_id = world.add_source(CellType::Light, source_start);

        world
            .script_bindings
            .push(ScriptBinding::new(id1, "scripts/test.aeo"));

        let mut history = History::new();

        history.push(
            world.cells.clone(),
            world.sources.clone(),
            world.script_bindings.clone(),
            world.actors.clone(),
            world.actor_script_bindings.clone(),
            ProjectHierarchy::new(),
            world.terrain.authored.clone(),
        );

        world.move_cells(&[c1], WorldCoord::new(5, 5, 5)).unwrap();
        world.set_source_position(source_id, source_end).unwrap();

        assert!(world.get(c1).is_none());
        assert!(world.get(WorldCoord::new(15, 7, 15)).is_some());

        let (
            prev_cells,
            prev_sources,
            prev_bindings,
            prev_actors,
            prev_actor_bindings,
            _prev_hierarchy,
            _prev_terrain,
        ) = history.undo_stack.pop().unwrap();

        history.redo_stack.push((
            world.cells.clone(),
            world.sources.clone(),
            world.script_bindings.clone(),
            world.actors.clone(),
            world.actor_script_bindings.clone(),
            ProjectHierarchy::new(),
            // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:22 -05:00
            crate::editor::editor::TerrainHistorySnapshot::Full(world.terrain.authored.clone()),
        ));

        world.cells = prev_cells;
        world.sources = prev_sources;
        world.script_bindings = prev_bindings;
        world.actors = prev_actors;
        world.actor_script_bindings = prev_actor_bindings;
        world.rebuild_id_mapping();

        assert_eq!(world.get(c1).unwrap().id, id1);
        assert!(world.get(WorldCoord::new(15, 7, 15)).is_none());
        assert_eq!(world.script_bindings.len(), 1);
        assert_eq!(world.source_position(source_id), Some(source_start));

        let (
            next_cells,
            next_sources,
            next_bindings,
            next_actors,
            next_actor_bindings,
            _next_hierarchy,
            _next_terrain,
        ) = history.redo_stack.pop().unwrap();

        world.cells = next_cells;
        world.sources = next_sources;
        world.script_bindings = next_bindings;
        world.actors = next_actors;
        world.actor_script_bindings = next_actor_bindings;
        world.rebuild_id_mapping();

        assert!(world.get(c1).is_none());
        assert_eq!(world.get(WorldCoord::new(15, 7, 15)).unwrap().id, id1);
        assert_eq!(world.source_position(source_id), Some(source_end));
    }

    #[test]
    fn test_paste_and_move_update_spatial_index_and_dirty_chunks() {
        let mut world = World::new();

        let c1 = WorldCoord::new(0, 0, 0);
        let _id1 = world.set_cell(c1, CellType::Block);

        let _ = world.drain_render_dirty_cells();

        let clipboard = vec![ClipboardCell {
            offset: WorldCoord::new(0, 0, 0),
            cell: world.get(c1).unwrap().clone(),
            script_binding: None,
        }];

        let target = WorldCoord::new(20, 20, 20);

        world.paste_cells(&clipboard, target).unwrap();

        let target_chunk = crate::world::cell::ChunkCoord::from_world_coord(target);

        let active = world
            .get_active_coords_in_chunk(target_chunk)
            .expect("Chunk in spatial index");

        assert!(active.contains(&target));
        assert!(world.is_authored_chunk_dirty(target_chunk));
        assert!(world.render_dirty_cells.borrow().contains_key(&target));

        let delta = WorldCoord::new(5, 5, 5);

        let move_dest = WorldCoord::new(target.x + delta.x, target.y + delta.y, target.z + delta.z);

        world.move_cells(&[target], delta).unwrap();

        if let Some(active_old) = world.get_active_coords_in_chunk(target_chunk) {
            assert!(!active_old.contains(&target));
        }

        let dest_chunk = crate::world::cell::ChunkCoord::from_world_coord(move_dest);

        let active_new = world
            .get_active_coords_in_chunk(dest_chunk)
            .expect("Dest chunk in spatial index");

        assert!(active_new.contains(&move_dest));
        assert!(world.is_authored_chunk_dirty(dest_chunk));
    }

    #[test]
    fn test_light_and_marker_indices_maintenance() {
        let mut world = World::new();

        let c1 = WorldCoord::new(1, 1, 1);
        let c2 = WorldCoord::new(2, 2, 2);

        let id_light = world.set_cell(c1, CellType::Light);
        let id_audio = world.set_cell(c2, CellType::AudioEmitter);

        let lights: Vec<u64> = world.iter_light_ids().collect();
        let markers: Vec<u64> = world.iter_editor_marker_ids().collect();

        assert_eq!(lights, vec![id_light]);
        assert!(markers.contains(&id_light));
        assert!(markers.contains(&id_audio));

        world.set_cell(c1, CellType::Empty);

        let lights_after: Vec<u64> = world.iter_light_ids().collect();

        let markers_after: Vec<u64> = world.iter_editor_marker_ids().collect();

        assert!(lights_after.is_empty());
        assert_eq!(markers_after, vec![id_audio]);
    }

    #[test]
    fn test_special_indices_update_when_cell_type_is_replaced() {
        let mut world = World::new();
        let coord = WorldCoord::new(0, 0, 0);

        let light_id = world.set_cell(coord, CellType::Light);

        assert!(world.light_ids.contains(&light_id));
        assert!(world.editor_marker_ids.contains(&light_id));
        assert!(!world.audio_emitter_ids.contains(&light_id));

        let block_id = world.set_cell(coord, CellType::Block);

        assert!(!world.light_ids.contains(&light_id));
        assert!(!world.editor_marker_ids.contains(&light_id));
        assert!(!world.audio_emitter_ids.contains(&light_id));
        assert!(world.get(coord).is_some());
        assert_eq!(world.get(coord).unwrap().id, block_id);

        let audio_id = world.set_cell(coord, CellType::AudioEmitter);

        assert!(!world.light_ids.contains(&block_id));
        assert!(!world.editor_marker_ids.contains(&block_id));

        assert!(world.audio_emitter_ids.contains(&audio_id));
        assert!(world.editor_marker_ids.contains(&audio_id));
    }

    #[test]
    fn test_paste_updates_special_indices() {
        let mut world = World::new();

        let light_coord = WorldCoord::new(0, 0, 0);
        let audio_coord = WorldCoord::new(1, 0, 0);

        let light_id = world.set_cell(light_coord, CellType::Light);

        let audio_id = world.set_cell(audio_coord, CellType::AudioEmitter);

        let clipboard = vec![
            ClipboardCell {
                offset: WorldCoord::new(0, 0, 0),
                cell: world.get(light_coord).unwrap().clone(),
                script_binding: None,
            },
            ClipboardCell {
                offset: WorldCoord::new(1, 0, 0),
                cell: world.get(audio_coord).unwrap().clone(),
                script_binding: None,
            },
        ];

        let target = WorldCoord::new(10, 0, 0);

        world.paste_cells(&clipboard, target).unwrap();

        let pasted_light = world.get(target).unwrap();
        let pasted_audio = world.get(WorldCoord::new(11, 0, 0)).unwrap();

        assert_ne!(pasted_light.id, light_id);
        assert_ne!(pasted_audio.id, audio_id);

        assert!(world.light_ids.contains(&pasted_light.id));
        assert!(world.editor_marker_ids.contains(&pasted_light.id));

        assert!(world.audio_emitter_ids.contains(&pasted_audio.id));

        assert!(world.editor_marker_ids.contains(&pasted_audio.id));
    }

    // Worldkiln 0.8.7 | WO 0.8.7-41 | 2026-10-08 20:46 -05:00
    #[test]
    fn runtime_camera_selection_preserves_authored_default_and_restores_original() {
        let mut world = World::new();
        assert_eq!(world.active_camera_script(), "thirdPerson");
        let original = world.active_camera_script().to_owned();
        world.set_runtime_camera_script("firstPerson").unwrap();
        assert_eq!(world.active_camera_script(), "firstPerson");
        assert_eq!(world.selected_camera, "thirdPerson");
        world.set_runtime_camera_script(&original).unwrap();
        assert_eq!(world.active_camera_script(), "thirdPerson");
        assert!(world.runtime_camera_script.is_none());
    }

    #[test]
    fn runtime_camera_selection_rejects_paths_and_empty_names() {
        let mut world = World::new();
        for invalid in ["", "../firstPerson", "first/person", "first person"] {
            assert!(world.set_runtime_camera_script(invalid).is_err());
        }
        assert_eq!(world.active_camera_script(), "thirdPerson");
    }

    #[test]
    fn runtime_camera_selection_tracks_consecutive_requests() {
        let mut world = World::new();
        world.set_runtime_camera_script("firstPerson").unwrap();
        let before = world.active_camera_script().to_owned();
        world.set_runtime_camera_script("topDown").unwrap();
        assert_eq!(world.active_camera_script(), "topDown");
        world.set_runtime_camera_script(&before).unwrap();
        assert_eq!(world.active_camera_script(), "firstPerson");
    }

    #[test]
    fn test_rebuild_restores_special_indices() {
        let mut world = World::new();

        let light_coord = WorldCoord::new(1, 1, 1);
        let audio_coord = WorldCoord::new(2, 2, 2);

        let light_id = world.set_cell(light_coord, CellType::Light);

        let audio_id = world.set_cell(audio_coord, CellType::AudioEmitter);

        world.light_ids.clear();
        world.audio_emitter_ids.clear();
        world.editor_marker_ids.clear();

        assert!(world.light_ids.is_empty());
        assert!(world.audio_emitter_ids.is_empty());
        assert!(world.editor_marker_ids.is_empty());

        world.rebuild_id_mapping();

        assert!(world.light_ids.contains(&light_id));
        assert!(world.audio_emitter_ids.contains(&audio_id));
        assert!(world.editor_marker_ids.contains(&light_id));
        assert!(world.editor_marker_ids.contains(&audio_id));
    }
}
