// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
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
pub mod partitions;
pub mod queries;
pub mod runtime;
pub mod settings;
pub mod spatial_index;
mod tags;

pub use dirty::{DirtyReason, RenderChange, RenderChanges};
pub use settings::{LightingSettings, RenderDistanceSettings, SkySettings};
pub use spatial_index::{ChunkCellIndex, WorldSpatialIndex};

#[derive(Clone)]
pub struct World {
    pub(crate) navigation_collision_revision: std::cell::Cell<u64>,
    pub(crate) navigation_collision_changes:
        std::cell::RefCell<VecDeque<(u64, Option<WorldCoord>)>>,
    pub(crate) navigation_world_id: u64,
    pub(crate) navigation_runtime_geometry: crate::navigation::RuntimeCollisionSnapshot,
    pub terrain: TerrainState,
    pub navmesh_volumes: Vec<crate::navigation::NavMeshBoundsVolume>,
    pub navmesh_settings: crate::navigation::NavMeshGenerationSettings,

    pub(crate) cells: HashMap<WorldCoord, Cell>,

    pub(crate) sources: HashMap<u64, AuthoredSource>,

    pub(crate) navigation_bounds_cache:
        std::cell::RefCell<Option<(u64, Option<(WorldCoord, WorldCoord)>)>>,

    pub(crate) runtime_state: HashMap<u64, RuntimeCellState>,
    pub(crate) runtime_entity_audio: HashMap<u64, u64>,

    pub(crate) physics_dirty_cells: HashSet<u64>,

    pub(crate) id_to_coord: HashMap<u64, WorldCoord>,

    pub(crate) next_cell_id: u64,

    pub(crate) runtime_cells: HashMap<u64, Cell>,

    pub(crate) runtime_id_to_coord: HashMap<u64, WorldCoord>,

    pub(crate) coord_to_runtime_id: HashMap<WorldCoord, u64>,

    pub(crate) audio_emitter_ids: HashSet<u64>,

    pub(crate) light_ids: HashSet<u64>,

    pub(crate) editor_marker_ids: HashSet<u64>,

    pub gravity: Vec3,
    pub max_walkable_slope_degrees: f32,
    pub character_walk_speed: f32,
    pub animation_budget_enabled: bool,
    pub animation_pose_budget_ms: f32,

    pub particles: crate::engine::particles::ParticleSystem,
    pub(crate) particle_emitter_ids: HashSet<u64>,

    pub lighting: LightingSettings,

    pub render_distance: RenderDistanceSettings,

    pub sky: SkySettings,
    pub(crate) runtime_material_catalog: Option<crate::project::material::MaterialCatalog>,
    pub(crate) runtime_material_tweens: crate::scripting::tween::TweenScheduler<(String, String)>,
    pub(crate) runtime_property_tweens:
        crate::scripting::tween::TweenScheduler<(crate::scripting::value::HandleKind, u64)>,
    pub(crate) runtime_material_changes: std::collections::BTreeMap<(String, String), f32>,
    pub(crate) runtime_post_process_authored:
        Option<crate::project::post_process_pipeline::PostProcessPipeline>,
    pub(crate) runtime_post_process_changes:
        Vec<crate::project::post_process_pipeline::PostProcessCommand>,

    pub script_bindings: Vec<ScriptBinding>,

    pub actors: HashMap<u64, Actor>,
    pub(crate) runtime_actor_tags: HashMap<u64, std::collections::BTreeSet<String>>,
    pub(crate) actor_tag_index: std::cell::RefCell<Option<tags::ActorTagIndex>>,

    pub actor_script_bindings: Vec<ActorScriptBinding>,

    pub(crate) actor_module_index: crate::world::ActorModuleIndex,

    pub actor_hierarchy: ActorHierarchy,

    pub next_actor_id: u64,

    pub disabled_scripts: Vec<String>,

    pub selected_character: String,

    pub player_character_scale: Vec3,

    pub selected_controller: String,

    pub selected_camera: String,

    pub runtime_camera_script: Option<String>,
    pub runtime_camera_applied: Option<String>,

    pub cursor_visible: bool,

    pub screen_locked: bool,

    pub(crate) render_revision: u64,

    pub(crate) authored_cell_type_counts: HashMap<CellType, usize>,

    pub(crate) editor_navigator_coords: HashMap<CellType, HashSet<WorldCoord>>,

    pub(crate) render_dirty_cells: std::cell::RefCell<HashMap<WorldCoord, DirtyReason>>,

    pub(crate) render_change_journal: std::cell::RefCell<VecDeque<RenderChange>>,
    pub(crate) render_change_floor_revision: std::cell::Cell<u64>,

    pub spatial_index: WorldSpatialIndex,

    pub(crate) dirty_authored_chunks: HashSet<ChunkCoord>,
}

impl World {
    pub fn active_camera_script(&self) -> &str {
        self.runtime_camera_script
            .as_deref()
            .unwrap_or(&self.selected_camera)
    }

    pub fn set_runtime_camera_script(&mut self, name: &str) -> Result<(), String> {
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

    pub fn navigation_revision(&self) -> u64 {
        self.render_revision
            ^ self.terrain.render_revision().rotate_left(31)
            ^ (self.terrain.config.plains.water_level.to_bits() as u64).rotate_left(13)
    }

    pub fn new() -> Self {
        Self {
            navigation_runtime_geometry: Default::default(),
            navigation_collision_revision: std::cell::Cell::new(0),
            navigation_collision_changes: std::cell::RefCell::new(VecDeque::new()),
            navigation_world_id: {
                static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            },
            terrain: TerrainState::default(),
            navmesh_volumes: Vec::new(),
            navmesh_settings: crate::navigation::NavMeshGenerationSettings::default(),
            max_walkable_slope_degrees: 45.0,
            character_walk_speed: 4.0,
            animation_budget_enabled: false,
            animation_pose_budget_ms: 2.0,
            cells: HashMap::new(),
            sources: HashMap::new(),
            navigation_bounds_cache: std::cell::RefCell::new(None),
            runtime_state: HashMap::new(),
            runtime_entity_audio: HashMap::new(),
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
            runtime_material_catalog: None,
            runtime_material_tweens: Default::default(),
            runtime_property_tweens: Default::default(),
            runtime_material_changes: Default::default(),
            runtime_post_process_authored: None,
            runtime_post_process_changes: Vec::new(),
            script_bindings: Vec::new(),
            actors: HashMap::new(),
            runtime_actor_tags: HashMap::new(),
            actor_tag_index: std::cell::RefCell::new(None),
            actor_script_bindings: Vec::new(),
            actor_module_index: crate::world::ActorModuleIndex::default(),
            actor_hierarchy: ActorHierarchy::new(),
            next_actor_id: 20_000_000,
            disabled_scripts: Vec::new(),
            selected_character: String::new(),
            player_character_scale: Vec3::ONE,
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

impl crate::scripting::tween::TweenBinding for World {
    type Target = (String, String);
    fn read(
        &self,
        target: &Self::Target,
        property: &str,
    ) -> Result<crate::scripting::tween::TweenValue, String> {
        use crate::scripting::tween::TweenValue;
        if property != "alpha" {
            return Err(format!(
                "Unsupported Material layer tween property: {property}"
            ));
        }
        let catalog = self
            .runtime_material_catalog
            .as_ref()
            .ok_or("Material catalog is not initialized for this Play session")?;
        let alpha = catalog
            .layers(&target.0)?
            .into_iter()
            .find(|layer| layer.name == target.1)
            .ok_or_else(|| format!("Material layer not found: {}", target.1))?
            .opacity;
        Ok(TweenValue::Scalar(alpha as f64))
    }
    fn validate(
        &self,
        target: &Self::Target,
        property: &str,
        value: crate::scripting::tween::TweenValue,
    ) -> Result<(), String> {
        use crate::scripting::tween::TweenValue;
        self.read(target, property)?.interpolate(value, 0.0)?;
        if !matches!(value,TweenValue::Scalar(alpha) if alpha.is_finite() && (0.0..=1.0).contains(&alpha))
        {
            return Err("Material layer alpha must be a number between 0 and 1".into());
        }
        Ok(())
    }
    fn write(
        &mut self,
        target: &Self::Target,
        property: &str,
        value: crate::scripting::tween::TweenValue,
    ) -> Result<(), String> {
        use crate::scripting::tween::TweenValue;
        if property != "alpha" {
            return Err(format!(
                "Unsupported Material layer tween property: {property}"
            ));
        }
        let TweenValue::Scalar(alpha) = value else {
            return Err("Material layer alpha requires a number".into());
        };
        if !alpha.is_finite() || !(0.0..=1.0).contains(&alpha) {
            return Err("Material layer alpha must be between 0 and 1".into());
        }
        let catalog = self
            .runtime_material_catalog
            .as_mut()
            .ok_or("Material catalog is unavailable")?;
        catalog.set_runtime_layer_alpha(&target.0, &target.1, alpha as f32)?;
        self.runtime_material_changes
            .insert(target.clone(), alpha as f32);
        Ok(())
    }
}
