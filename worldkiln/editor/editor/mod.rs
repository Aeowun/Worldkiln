// Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:50 -05:00
pub mod ai_chat;
// Worldkiln 0.8.5 | WO 0.8.7-61 | 2026-10-09 15:00 -05:00
pub mod ai_provider;
// Worldkiln 0.8.5 | WO 0.8.7-63 | 2026-10-09
pub mod ai_agent;
pub mod ai_project;
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
mod ai_editor;
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:20 -05:00
mod ai_stream;
// Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:17 -05:00
#[cfg(test)]
mod ai_agent_tests;
// Worldkiln 0.8.7 | WO 0.8.7-66 | 2026-10-09: searchable source-linked docs corpus.
pub mod ai_knowledge;
// Worldkiln 0.8.5 | WO 0.8.7-62 | 2026-10-09
pub mod ai_script;
// Worldkiln 0.8.5 | WO 0.8.7-65 | 2026-10-09
pub mod ai_secure;
// Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
pub mod inspector;
pub mod panel_cache;
pub mod panels;
pub mod project_explorer;
// Worldkiln 0.8.5 | WO 0.8.7-70 | 2026-10-09 19:25 -05:00
pub mod asset_browser;
// Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:38 -05:00
pub mod feature_nav;
pub mod shell;
// Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
pub mod transform_gizmo;
pub mod ui;
// Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:52 -05:00
mod terrain_layers_ui;
// Worldkiln 0.8.5 | WO 0.8.7-58 | 2026-10-09
mod save_data_ui;
// Worldkiln 0.8.5 | WO 0.8.7-59 | 2026-10-09
mod save_data_discovery;

pub use inspector::{AttributeType, PropertyChange};

use super::navigation::NavigationWindow;
use crate::engine::EditorMode;
use crate::project::{EditorProjectState, ProjectHierarchy, ProjectNodeId};
use crate::renderer::camera::CameraController;
use crate::scripting::binding::ScriptBinding;
use crate::world::{AttributeValue, Cell, WorldCoord};
use std::collections::{HashMap, HashSet};
use std::path::Path;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GridPlane {
    Xz,
    Yz,
    Xy,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum UiPreviewResolution {
    Res1280x720,
    Res1600x900,
    Res1920x1080,
    Custom(f32, f32),
}

impl UiPreviewResolution {
    pub fn size(&self) -> [f32; 2] {
        match self {
            Self::Res1280x720 => [1280.0, 720.0],
            Self::Res1600x900 => [1600.0, 900.0],
            Self::Res1920x1080 => [1920.0, 1080.0],
            Self::Custom(w, h) => [*w, *h],
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum EditorTool {
    Navigate,
    Build,
    Erase,
    Select,
}

// Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
// Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 22:02 -05:00
// The left panel has exactly two authoring destinations. Legacy
// Lighting/Physics/ProjectSettings tabs migrate to central Project Settings.
pub enum LeftPanelTab {
    #[default]
    Project,
    Terrain,
}

// Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 13:06 -05:00
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum RightPanelTab {
    #[default]
    Inspector,
    Navigation,
    Landscaping,
    Ai,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
// Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
pub enum SettingsPage {
    #[default]
    Lighting,
    Editor,
    Theme,
    Audio,
    Input,
    Ai,
}

impl SettingsPage {
    pub const ALL: [Self; 6] = [
        Self::Lighting,
        Self::Editor,
        Self::Theme,
        Self::Audio,
        Self::Input,
        Self::Ai,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Lighting => "LIGHTING",
            Self::Editor => "EDITOR",
            Self::Theme => "THEME",
            Self::Audio => "AUDIO",
            Self::Input => "INPUT",
            Self::Ai => "AI",
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ColorTarget {
    Build,
    PropertyColor(WorldCoord),
    PropertyLight(WorldCoord),
}

#[derive(Clone, Debug)]
pub struct ClipboardCell {
    pub offset: WorldCoord,
    pub cell: Cell,
    pub script_binding: Option<ScriptBinding>,
}

// Worldkiln 0.8.7 | WO 0.8.7-13 | 2026-10-05 23:08 -05:00
#[derive(Clone, Debug)]
pub enum EditorClipboard {
    Cells {
        pivot_coord: WorldCoord,
        cells: Vec<ClipboardCell>,
    },
    Actor {
        actor: crate::world::Actor,
        actor_bindings: Vec<crate::scripting::binding::ActorScriptBinding>,
    },
    Source {
        source: crate::world::AuthoredSource,
        script_binding: Option<ScriptBinding>,
    },
}

#[derive(Clone, Debug)]
pub struct GrabState {
    pub source_coords: Vec<WorldCoord>,
    pub anchor_coord: WorldCoord,
    pub current_target: WorldCoord,
    pub delta: WorldCoord,
    pub valid: bool,
}

#[derive(Clone, Debug)]
pub struct ActorGrabState {
    pub actor_id: u64,
    pub start_position: glam::Vec3,
    pub current_position: glam::Vec3,
}

#[derive(Clone, Debug)]
pub struct SourceGrabState {
    pub source_id: u64,
    pub start_position: glam::Vec3,
    pub current_position: glam::Vec3,
}

#[derive(Clone, Debug)]
pub struct UiDragState {
    pub node_id: u32,
    pub start_pointer: egui::Pos2,
    pub start_position: [f32; 2],
}

// Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:17 -05:00
// Ordinary edits retain their full Terrain snapshots. Brush strokes store
// only before-values of touched voxel columns; undo/redo capture the inverse
// columns and restore locally without cloning a multi-million-voxel Terrain.
#[derive(Clone, Debug)]
pub(crate) enum TerrainHistorySnapshot {
    Full(Option<crate::world::TerrainResult>),
    // Worldkiln 0.8.5 | WO MAT-001 | 2026-10-10 12:10 -05:00
    // Ordinary Cell/Actor/Source edits do not snapshot or restore Terrain.
    Unchanged,
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    // Layer stroke history uses O(1) saved count + only the inverse tail,
    // never clones the entire brush journal on each new stroke.
    LayerEditCount {
        layer_id: u64,
        count: usize,
    },
    LayerEditAppend {
        layer_id: u64,
        edits: Vec<crate::world::terrain::LayerBrushEdit>,
    },
    Columns {
        origin: [i32; 3],
        size: [u32; 3],
        spacing: f32,
        values: HashMap<(u32, u32), Vec<crate::world::terrain::SmoothVoxel>>,
        materials: Vec<String>,
    },
}
impl TerrainHistorySnapshot {
    pub(crate) fn counterpart(&self, terrain: &crate::world::terrain::TerrainState) -> Self {
        if matches!(self, Self::Unchanged) {
            return Self::Unchanged;
        }
        // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
        if let Self::LayerEditCount { layer_id, count } = self {
            let edits = terrain
                .config
                .generation_layers
                .iter()
                .find(|l| l.id == *layer_id)
                .and_then(|l| l.brush_edits.get((*count).min(l.brush_edits.len())..))
                .map(ToOwned::to_owned)
                .unwrap_or_default();
            return Self::LayerEditAppend {
                layer_id: *layer_id,
                edits,
            };
        }
        if let Self::LayerEditAppend { layer_id, .. } = self {
            return Self::LayerEditCount {
                layer_id: *layer_id,
                count: terrain
                    .config
                    .generation_layers
                    .iter()
                    .find(|l| l.id == *layer_id)
                    .map(|l| l.brush_edits.len())
                    .unwrap_or(0),
            };
        }
        let Self::Columns {
            origin,
            size,
            spacing,
            values,
            ..
        } = self
        else {
            return Self::Full(terrain.authored.clone());
        };
        let Some(crate::world::TerrainResult {
            data: crate::world::terrain::TerrainData::Voxel(grid),
            ..
        }) = terrain.authored.as_ref()
        else {
            return Self::Full(terrain.authored.clone());
        };
        if grid.origin != *origin || grid.size != *size || grid.spacing_studs != *spacing {
            return Self::Full(terrain.authored.clone());
        }
        let mut copied = HashMap::with_capacity(values.len());
        for &(x, z) in values.keys() {
            copied.insert(
                (x, z),
                (0..grid.size[1])
                    .map(|y| grid.voxel(x, y, z).unwrap())
                    .collect(),
            );
        }
        Self::Columns {
            origin: *origin,
            size: *size,
            spacing: *spacing,
            values: copied,
            materials: grid.materials.clone(),
        }
    }

    pub(crate) fn apply(self, terrain: &mut crate::world::terrain::TerrainState) {
        match self {
            Self::Unchanged => {}
            Self::Full(snapshot) => terrain.replace_authored(snapshot),
            // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
            Self::LayerEditCount { layer_id, count } => {
                if let Some(layer) = terrain
                    .config
                    .generation_layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                {
                    layer.brush_edits.truncate(count);
                    if let Err(error) = terrain.rebuild_generation_layers() {
                        eprintln!("Layer brush history could not be recomposed: {error}");
                    }
                }
            }
            Self::LayerEditAppend { layer_id, edits } => {
                if let Some(layer) = terrain
                    .config
                    .generation_layers
                    .iter_mut()
                    .find(|l| l.id == layer_id)
                {
                    layer.brush_edits.extend(edits);
                    if let Err(error) = terrain.rebuild_generation_layers() {
                        eprintln!("Layer brush history could not be recomposed: {error}");
                    }
                }
            }
            Self::Columns {
                origin,
                size,
                spacing,
                values,
                materials,
            } => {
                let Some(crate::world::TerrainResult {
                    data: crate::world::terrain::TerrainData::Voxel(grid),
                    ..
                }) = terrain.authored.as_mut()
                else {
                    return;
                };
                if grid.origin != origin || grid.size != size || grid.spacing_studs != spacing {
                    return;
                }
                grid.materials = materials;
                let mut min_x = u32::MAX;
                let mut min_z = u32::MAX;
                let mut max_x = 0;
                let mut max_z = 0;
                for ((x, z), column) in values {
                    if x >= size[0] || z >= size[2] || column.len() != size[1] as usize {
                        continue;
                    }
                    for (y, voxel) in column.into_iter().enumerate() {
                        grid.set_voxel(x, y as u32, z, voxel);
                    }
                    min_x = min_x.min(x);
                    max_x = max_x.max(x);
                    min_z = min_z.min(z);
                    max_z = max_z.max(z);
                }
                if min_x != u32::MAX {
                    let min_wx = origin[0] as f32 + spacing * min_x as f32;
                    let max_wx = origin[0] as f32 + spacing * (max_x + 1) as f32;
                    let min_wz = origin[2] as f32 + spacing * min_z as f32;
                    let max_wz = origin[2] as f32 + spacing * (max_z + 1) as f32;
                    let center = [
                        (min_wx + max_wx) * 0.5,
                        origin[1] as f32,
                        (min_wz + max_wz) * 0.5,
                    ];
                    let radius = ((max_wx - min_wx).hypot(max_wz - min_wz) * 0.5) + spacing * 2.0;
                    terrain.mark_authored_brush_dirty(center, radius);
                }
            }
        }
    }
}

// Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
type HistorySnapshot = (
    HashMap<WorldCoord, Cell>,
    HashMap<u64, crate::world::AuthoredSource>,
    Vec<ScriptBinding>,
    HashMap<u64, crate::world::Actor>,
    Vec<crate::scripting::binding::ActorScriptBinding>,
    ProjectHierarchy,
    TerrainHistorySnapshot,
);

pub struct History {
    pub undo_stack: Vec<HistorySnapshot>,
    pub redo_stack: Vec<HistorySnapshot>,
    pub limit: usize,
}

// Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
/// Independent undo state for one authored UI document.
#[derive(Default)]
pub(crate) struct UiDocumentHistory {
    pub undo: Vec<crate::engine::ui_authoring::AuthoredUiDocument>,
    pub redo: Vec<crate::engine::ui_authoring::AuthoredUiDocument>,
    pub edit_start: Option<crate::engine::ui_authoring::AuthoredUiDocument>,
    pub last_change_at: f64,
}

impl History {
    pub fn new() -> Self {
        Self::new_with_limit(100)
    }

    pub fn new_with_limit(limit: usize) -> Self {
        Self {
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            limit,
        }
    }

    pub fn set_limit(&mut self, limit: usize) {
        self.limit = limit.max(1);

        if self.undo_stack.len() > self.limit {
            let excess = self.undo_stack.len() - self.limit;
            self.undo_stack.drain(0..excess);
        }

        if self.redo_stack.len() > self.limit {
            let excess = self.redo_stack.len() - self.limit;
            self.redo_stack.drain(0..excess);
        }
    }

    pub fn push_undo_entry(&mut self, entry: HistorySnapshot) {
        self.undo_stack.push(entry);
        if self.undo_stack.len() > self.limit {
            let excess = self.undo_stack.len() - self.limit;
            self.undo_stack.drain(0..excess);
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 09:55 -05:00
    pub(crate) fn rewrite_texture_references(&mut self, old: &str, new: &str) {
        for stack in [&mut self.undo_stack, &mut self.redo_stack] {
            for snapshot in stack {
                let (cells, sources, _, _, _, _, _) = snapshot;
                for cell in cells.values_mut() {
                    if cell.texture == old {
                        cell.texture = new.to_string();
                    } else if cell.texture.eq_ignore_ascii_case(&format!("{old}.png")) {
                        cell.texture = format!("{new}.png");
                    }
                }
                for source in sources.values_mut() {
                    if source.cell.texture == old {
                        source.cell.texture = new.to_string();
                    } else if source
                        .cell
                        .texture
                        .eq_ignore_ascii_case(&format!("{old}.png"))
                    {
                        source.cell.texture = format!("{new}.png");
                    }
                }
            }
        }
    }

    pub(crate) fn rewrite_audio_references(&mut self, old: &str, new: &str) {
        for stack in [&mut self.undo_stack, &mut self.redo_stack] {
            for snapshot in stack {
                let (cells, sources, _, _, _, _, _) = snapshot;
                for cell in cells.values_mut() {
                    if cell.audio == old {
                        cell.audio = new.to_string();
                    }
                }
                for source in sources.values_mut() {
                    if source.cell.audio == old {
                        source.cell.audio = new.to_string();
                    }
                }
            }
        }
    }

    pub fn push(
        &mut self,
        cells: HashMap<WorldCoord, Cell>,
        sources: HashMap<u64, crate::world::AuthoredSource>,
        bindings: Vec<ScriptBinding>,
        actors: HashMap<u64, crate::world::Actor>,
        actor_bindings: Vec<crate::scripting::binding::ActorScriptBinding>,
        project_hierarchy: ProjectHierarchy,
        terrain: Option<crate::world::TerrainResult>,
    ) {
        self.push_undo_entry((
            cells,
            sources,
            bindings,
            actors,
            actor_bindings,
            project_hierarchy,
            TerrainHistorySnapshot::Full(terrain),
        ));
        self.redo_stack.clear();
    }
}

pub struct Editor {
    pub mode: EditorMode,
    pub ui_mode: bool,

    pub camera: CameraController,
    pub anchor: WorldCoord,

    pub hovered_cell: Option<WorldCoord>,
    pub hovered_actor_id: Option<u64>,
    pub hovered_source_id: Option<u64>,
    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    pub hovered_terrain_hit: Option<crate::editor::grid::picking::SurfaceHit>,

    pub selected_coord: Option<WorldCoord>,
    pub selected_coords: Vec<WorldCoord>,
    pub selected_actor_id: Option<u64>,
    pub selected_source_id: Option<u64>,
    pub selected_project_node_id: Option<ProjectNodeId>,
    pub selected_project_nodes: Vec<ProjectNodeId>,
    pub expanded_project_nodes: HashSet<ProjectNodeId>,
    pub project_hierarchy: ProjectHierarchy,
    pub(crate) project_actor_sync_revision: Option<u64>,
    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    pub(crate) project_cell_sync_revision: Option<u64>,
    pub(crate) project_tree_cache: panel_cache::ProjectTreeCache,

    pub project_explorer_search: String,
    pub show_project_explorer_folder_dialog: bool,
    pub project_explorer_folder_name: String,
    pub project_explorer_folder_parent: Option<ProjectNodeId>,
    pub project_explorer_folder_error: Option<String>,
    pub project_explorer_focused: bool,
    pub show_project_explorer_rename_dialog: bool,
    pub project_explorer_rename_node_id: Option<ProjectNodeId>,
    pub project_explorer_rename_name: String,
    pub project_explorer_rename_error: Option<String>,
    pub show_project_explorer_delete_dialog: bool,
    pub project_explorer_delete_node_id: Option<ProjectNodeId>,
    pub project_explorer_clipboard_node_id: Option<ProjectNodeId>,
    pub project_explorer_status: Option<(String, f64)>,
    pub project_explorer_drag_hover: Option<(ProjectNodeId, f64)>,

    pub selected_actor_directory_id: Option<u64>,
    pub actor_directory_name: String,
    pub build_is_actor: bool,
    pub build_actor_package: Option<String>,
    // Worldkiln 0.8.7 | WO 0.8.7-33 | 2026-10-08 15:59 -05:00
    // Large imported packs use a dedicated searchable hierarchy instead of
    // the toolbar's former unbounded flat package menu.
    pub show_actor_asset_browser: bool,
    pub actor_asset_search: String,
    // Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 22:55 -05:00
    pub character_import_status: Option<(String, bool)>,

    pub current_tool: EditorTool,
    // Worldkiln 0.8.5 | WO 0.8.7-69 | 2026-10-09
    // Transform is a Select submode: preserves old Build/Erase/Navigate input.
    pub transform_mode: transform_gizmo::TransformMode,
    pub actor_transform_preview: Option<transform_gizmo::ActorTransformPreview>,

    pub editor_max_distance_chunks: i32,
    pub play_max_distance_chunks: i32,

    pub navigation_window: NavigationWindow,
    pub show_properties_window: bool,
    pub show_world_window: bool,
    // Worldkiln 0.8.5 | WO 0.8.7-58 | 2026-10-09
    // Runtime save edits live outside authored World/Undo history.
    pub(crate) save_data_ui: save_data_ui::SaveDataEditor,
    pub left_panel_tab: LeftPanelTab,
    pub plane_picking: bool,

    pub right_panel_split: f32,
    pub right_panel_tab: RightPanelTab,
    // Worldkiln 0.8.5 | WO 0.8.7-60 | 2026-10-09 14:50 -05:00
    pub ai_chat: ai_chat::AiChat,
    // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
    agent_undo: Vec<ai_editor::AgentCheckpoint>,
    agent_redo: Vec<ai_editor::AgentCheckpoint>,
    agent_file_changes: Vec<ai_editor::AgentFileChange>,
    agent_change_id: u64,
    // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
    pub shell: shell::ShellState,
    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 13:06 -05:00
    pub show_landscaping_window: bool,

    // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:38 -05:00
    pub navigation: feature_nav::NavigationState,
    pub settings_page: SettingsPage,
    // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:20 -05:00
    pub asset_manager: crate::editor::asset_management::AssetManagerState,

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    pub terrain_generation_position: [i32; 3],
    pub terrain_generation_size: [i32; 3],
    pub terrain_mode_selection: crate::world::terrain::TerrainMode,
    pub terrain_rebuild_required: bool,
    pub terrain_generation_error: Option<String>,
    // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:51 -05:00
    pub terrain_selected_generation_layer: Option<u64>,
    pub terrain_pending_delete_layer: Option<u64>,
    pub terrain_confirm_base_replace: bool,

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    pub sculpt_tool: crate::world::terrain::SculptTool,
    pub sculpt_brush_radius: f32,
    pub sculpt_strength: f32,
    pub sculpt_flatten_target: Option<f32>,
    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    pub terrain_paint_material_slot: crate::world::terrain::TerrainPaintMaterialSlot,
    pub sculpt_stroke_active: bool,
    pub sculpt_stroke_mutated: bool,
    pub sculpt_stroke_snapshot: Option<HistorySnapshot>,
    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    pub actor_brush_enabled: bool,
    pub actor_brush_package: Option<String>,
    pub actor_brush_density: f32,
    pub actor_brush_scale_min: f32,
    pub actor_brush_scale_max: f32,
    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
    pub actor_brush_tilt_min_degrees: f32,
    pub actor_brush_tilt_max_degrees: f32,
    pub actor_brush_random_y_rotation: bool,
    pub actor_brush_y_offset: f32,
    pub actor_brush_stroke_active: bool,
    pub actor_brush_stroke_mutated: bool,
    pub actor_brush_stroke_snapshot: Option<HistorySnapshot>,
    pub actor_brush_last_center: Option<glam::Vec3>,
    pub actor_brush_stroke_positions: Vec<glam::Vec3>,
    pub actor_brush_seed: u64,

    pub show_script_workspace: bool,
    pub last_show_script_workspace: bool,
    pub script_editor: super::script_editor::ScriptEditor,

    pub show_clear_confirmation: bool,
    pub needs_clear_world: bool,
    pub needs_save: bool,
    pub needs_exit: bool,
    pub needs_build: bool,

    pub build_template: crate::world::Cell,

    pub active_color_target: Option<ColorTarget>,

    pub viewport_rect: egui::Rect,
    pub viewport_ppp: f32,

    pub history: History,
    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
    // Inspector widget edits remain one pending World transaction while the
    // text field or pointer gesture owns focus; Undo commits it before replay.
    pub(crate) pending_inspector_history: Option<HistorySnapshot>,

    pub clipboard: Option<EditorClipboard>,
    pub grab_state: Option<GrabState>,
    pub actor_grab_state: Option<ActorGrabState>,
    pub source_grab_state: Option<SourceGrabState>,

    pub terminal_output: String,
    pub terminal_auto_scroll: bool,
    terminal_recent_diagnostics: std::collections::VecDeque<String>,
    pub(crate) terminal_output_revision: u64,
    pub(crate) output_panel_cache: panel_cache::OutputPanelCache,
    pub output_panel_height: f32,
    pub script_output_panel_height: f32,

    pub attribute_add_name: String,
    pub attribute_add_type: AttributeType,
    pub attribute_add_value: AttributeValue,
    pub attribute_add_error: Option<String>,

    pub last_selected_coord: Option<WorldCoord>,

    // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
    // World-panel derived data is revision-cached and rebuilt only after World mutations.
    pub(crate) world_navigator_cache: panel_cache::WorldNavigatorCache,

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
    // UI documents own their history and never share the World stack.
    pub ui_workspace: crate::engine::ui_authoring::AuthoredUiWorkspace,
    pub ui_document: crate::engine::ui_authoring::AuthoredUiDocument,
    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
    // UI document edits retain their own mode-scoped undo state.
    pub(crate) ui_document_history: std::collections::HashMap<u32, UiDocumentHistory>,
    pub ui_selected_node: Option<u32>,
    pub ui_drag_state: Option<UiDragState>,
    pub ui_drag_asset: Option<String>,
    pub ui_preview_resolution: UiPreviewResolution,
    pub ui_show_safe_area: bool,
    pub ui_hierarchy_drag_node: Option<u32>,
    pub ui_stretch_edges_linked: bool,
    pub show_ui_document_dialog: bool,
    pub ui_document_dialog_rename_id: Option<u32>,
    pub ui_document_dialog_name: String,
    pub ui_document_dialog_error: Option<String>,

    pub engine_audio_settings: crate::engine::audio::EngineAudioSettings,
    pub pending_engine_sounds: Vec<crate::engine::audio::EngineSound>,

    pub theme_settings: crate::engine::preferences::ThemeSettings,
}

impl Editor {
    pub(crate) fn append_output(&mut self, message: &str) {
        if let Some(start) = message
            .find("[ERROR]")
            .or_else(|| message.find("[WARNING]"))
        {
            // Ignore timestamp changes when suppressing repeated callback errors.
            let key: String = message[start..].chars().take(8192).collect();
            if self.terminal_recent_diagnostics.contains(&key) {
                return;
            }
            if self.terminal_recent_diagnostics.len() == 256 {
                self.terminal_recent_diagnostics.pop_front();
            }
            self.terminal_recent_diagnostics.push_back(key);
            self.script_editor.show_output = true;
        }
        self.terminal_output.push_str(message);
        crate::engine::diagnostics::retain_output(&mut self.terminal_output);
        self.terminal_output_revision = self.terminal_output_revision.wrapping_add(1);
    }

    pub(crate) fn clear_output(&mut self) {
        crate::engine::diagnostics::clear();
        self.terminal_recent_diagnostics.clear();
        self.terminal_output.clear();
        self.terminal_output_revision = self.terminal_output_revision.wrapping_add(1);
        self.script_editor.clear_output_log();
    }

    pub(crate) fn collect_diagnostics(&mut self) {
        for message in crate::engine::diagnostics::drain() {
            self.append_output(&message);
        }
        crate::engine::diagnostics::retain_output(&mut self.terminal_output);
        self.script_editor.trim_output_log(256);
    }

    pub fn new() -> Self {
        let theme_settings = crate::engine::preferences::load_editor_preferences();
        let history = History::new_with_limit(theme_settings.undo_history_limit);

        Self {
            mode: EditorMode::default(),
            ui_mode: false,

            camera: CameraController::new(),
            anchor: WorldCoord::new(0, 0, 0),

            hovered_cell: None,
            hovered_actor_id: None,
            hovered_source_id: None,
            hovered_terrain_hit: None,

            selected_coord: None,
            selected_coords: Vec::new(),
            selected_actor_id: None,
            selected_source_id: None,
            selected_project_node_id: None,
            selected_project_nodes: Vec::new(),
            expanded_project_nodes: HashSet::new(),
            project_hierarchy: ProjectHierarchy::new(),
            project_actor_sync_revision: None,
            project_cell_sync_revision: None,
            project_tree_cache: panel_cache::ProjectTreeCache {
                dirty: true,
                ..Default::default()
            },

            project_explorer_search: String::new(),
            show_project_explorer_folder_dialog: false,
            project_explorer_folder_name: String::new(),
            project_explorer_folder_parent: None,
            project_explorer_folder_error: None,
            project_explorer_focused: false,
            show_project_explorer_rename_dialog: false,
            project_explorer_rename_node_id: None,
            project_explorer_rename_name: String::new(),
            project_explorer_rename_error: None,
            show_project_explorer_delete_dialog: false,
            project_explorer_delete_node_id: None,
            project_explorer_clipboard_node_id: None,
            project_explorer_status: None,
            project_explorer_drag_hover: None,

            selected_actor_directory_id: None,
            actor_directory_name: String::new(),
            build_is_actor: false,
            build_actor_package: None,
            show_actor_asset_browser: false,
            actor_asset_search: String::new(),
            // Worldkiln 0.8.7 | WO 0.8.7-12 | 2026-10-05 23:31 -05:00
            character_import_status: None,

            current_tool: EditorTool::Select,
            transform_mode: transform_gizmo::TransformMode::Select,
            actor_transform_preview: None,

            editor_max_distance_chunks: 0,
            play_max_distance_chunks: 0,

            navigation_window: NavigationWindow::new(),
            show_properties_window: true,
            show_world_window: true,
            save_data_ui: save_data_ui::SaveDataEditor::default(),
            left_panel_tab: LeftPanelTab::Project,
            plane_picking: false,

            right_panel_split: 0.5,
            right_panel_tab: RightPanelTab::Inspector,
            ai_chat: ai_chat::AiChat::new(),
            // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:00 -05:00
            agent_undo: Vec::new(),
            agent_redo: Vec::new(),
            agent_file_changes: Vec::new(),
            agent_change_id: 0,
            shell: shell::ShellState::load(),
            show_landscaping_window: true,

            navigation: feature_nav::NavigationState::default(),
            settings_page: SettingsPage::default(),
            // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:20 -05:00
            asset_manager: crate::editor::asset_management::AssetManagerState::default(),
            terrain_generation_position: [0, 0, 0],
            terrain_generation_size: [64, 32, 64],
            terrain_mode_selection: crate::world::terrain::TerrainMode::Smooth,
            terrain_rebuild_required: false,
            terrain_generation_error: None,
            // Worldkiln 0.8.5 | WO 0.9.3-23 | 2026-10-09 11:51 -05:00
            terrain_selected_generation_layer: None,
            terrain_pending_delete_layer: None,
            terrain_confirm_base_replace: false,

            sculpt_tool: crate::world::terrain::SculptTool::None,
            sculpt_brush_radius: 4.0,
            sculpt_strength: 0.25,
            sculpt_flatten_target: None,
            // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
            terrain_paint_material_slot: crate::world::terrain::TerrainPaintMaterialSlot::Surface,
            sculpt_stroke_active: false,
            sculpt_stroke_mutated: false,
            sculpt_stroke_snapshot: None,
            actor_brush_enabled: false,
            actor_brush_package: None,
            actor_brush_density: 1.0,
            actor_brush_scale_min: 0.85,
            actor_brush_scale_max: 1.15,
            actor_brush_tilt_min_degrees: 0.0,
            actor_brush_tilt_max_degrees: 8.0,
            actor_brush_random_y_rotation: true,
            actor_brush_y_offset: 0.0,
            actor_brush_stroke_active: false,
            actor_brush_stroke_mutated: false,
            actor_brush_stroke_snapshot: None,
            actor_brush_last_center: None,
            actor_brush_stroke_positions: Vec::new(),
            actor_brush_seed: 0,

            show_script_workspace: false,
            last_show_script_workspace: false,
            script_editor: super::script_editor::ScriptEditor::new(),

            show_clear_confirmation: false,
            needs_clear_world: false,
            needs_save: false,
            needs_exit: false,
            needs_build: false,

            build_template: crate::world::Cell::new_block(),

            active_color_target: None,

            viewport_rect: egui::Rect::NOTHING,
            viewport_ppp: 1.0,

            history,
            pending_inspector_history: None,

            clipboard: None,
            grab_state: None,
            actor_grab_state: None,
            source_grab_state: None,

            terminal_output: String::new(),
            terminal_auto_scroll: true,
            terminal_recent_diagnostics: std::collections::VecDeque::new(),
            terminal_output_revision: 0,
            output_panel_cache: panel_cache::OutputPanelCache::default(),
            output_panel_height: 190.0,
            script_output_panel_height: 118.0,

            attribute_add_name: String::new(),
            attribute_add_type: AttributeType::String,
            attribute_add_value: AttributeValue::String(String::new()),
            attribute_add_error: None,

            last_selected_coord: None,

            // Worldkiln 0.8.7 | WO 0.8.7-30 | 2026-10-06 20:13 -05:00
            world_navigator_cache: panel_cache::WorldNavigatorCache::default(),

            ui_workspace: crate::engine::ui_authoring::AuthoredUiWorkspace::default(),
            ui_document: crate::engine::ui_authoring::AuthoredUiDocument::default(),
            ui_document_history: std::collections::HashMap::new(),
            ui_selected_node: None,
            ui_drag_state: None,
            ui_drag_asset: None,
            ui_preview_resolution: UiPreviewResolution::Res1920x1080,
            ui_show_safe_area: false,
            ui_hierarchy_drag_node: None,
            ui_stretch_edges_linked: false,
            show_ui_document_dialog: false,
            ui_document_dialog_rename_id: None,
            ui_document_dialog_name: String::new(),
            ui_document_dialog_error: None,

            engine_audio_settings: crate::engine::audio::EngineAudioSettings::default(),
            pending_engine_sounds: Vec::new(),

            theme_settings,
        }
    }

    pub(crate) fn queue_engine_sound(&mut self, sound: crate::engine::audio::EngineSound) {
        self.pending_engine_sounds.push(sound);
    }

    pub fn camera_speed(&self) -> f32 {
        self.theme_settings.camera_speed()
    }

    pub fn camera_sensitivity(&self) -> f32 {
        self.theme_settings.camera_sensitivity()
    }

    pub fn expand_all_project_nodes(&mut self) {
        for &id in self.project_hierarchy.nodes.keys() {
            self.expanded_project_nodes.insert(id);
        }
    }

    pub fn collapse_all_project_nodes(&mut self) {
        self.expanded_project_nodes.clear();
    }

    pub fn clear_clipboard(&mut self) {
        self.clipboard = None;
    }

    // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:11 -05:00
    /// Selection in the viewport/Explorer must always show the matching
    /// Inspector. Do not silently keep the right dock on Landscaping/AI.
    pub(crate) fn reveal_selected_properties(&mut self) {
        let layout_changed = !self.show_properties_window
            || !self.shell.properties_visible
            || self.right_panel_tab != RightPanelTab::Inspector;
        self.show_properties_window = true;
        self.shell.properties_visible = true;
        self.right_panel_tab = RightPanelTab::Inspector;
        if layout_changed {
            self.shell.save();
        }
    }

    // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:11 -05:00
    /// A click in the Build texture browser sets both the future Build
    /// template and, when applicable, selected authored blocks. Non-block
    /// cells/Actors and unselected cells must remain unmodified. Material
    /// cache invalidation uses the original authored-cell mutation contract.
    pub(crate) fn apply_build_texture_selection(
        &mut self,
        world: &mut crate::world::World,
        texture: String,
    ) -> usize {
        self.build_template.texture = texture.clone();
        let coords = self
            .selected_coords
            .iter()
            .copied()
            .chain(self.selected_coord)
            .collect::<std::collections::HashSet<_>>();
        let targets = coords
            .into_iter()
            .filter(|coord| {
                world.get(*coord).is_some_and(|cell| {
                    matches!(
                        cell.cell_type,
                        crate::world::CellType::Block | crate::world::CellType::FxBlock
                    ) && cell.texture != texture
                })
            })
            .collect::<Vec<_>>();
        if targets.is_empty() {
            return 0;
        }
        let previous = self.create_snapshot(world);
        self.history.push_undo_entry(previous);
        self.history.redo_stack.clear();
        for coord in &targets {
            if let Some(cell) = world.get_mut(*coord) {
                cell.texture = texture.clone();
            }
            world.touch_authored_cell(*coord, crate::world::DirtyReason::MaterialOrOffset);
        }
        self.needs_save = true;
        self.reveal_selected_properties();
        targets.len()
    }

    /// Selects an authored Actor.
    ///
    /// Actor and Cell selection are deliberately mutually exclusive.
    pub fn select_actor(&mut self, actor_id: u64) {
        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
        self.ensure_scene_home_workspace();
        self.selected_actor_id = Some(actor_id);
        self.selected_source_id = None;

        self.selected_coord = None;
        self.selected_coords.clear();

        // Viewport Actor selection also keeps the Project Explorer on the same
        // authored object when that Actor already has a hierarchy node.
        if let Some(node_id) = self.project_hierarchy.find_actor_node(actor_id) {
            self.selected_project_node_id = Some(node_id);
            self.selected_project_nodes = vec![node_id];
        }

        self.reveal_selected_properties();
    }

    /// Selects an authored Cell.
    ///
    /// Actor and Cell selection are deliberately mutually exclusive.
    pub fn select_cell(&mut self, coord: WorldCoord) {
        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
        self.ensure_scene_home_workspace();
        self.selected_actor_id = None;
        self.selected_source_id = None;

        self.selected_coord = Some(coord);
        self.selected_coords = vec![coord];

        self.reveal_selected_properties();
    }

    // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:15 -05:00
    /// Select an authored UI element from the UI hierarchy or canvas and
    /// reveal the existing editable UI Properties inspector.
    pub(crate) fn select_ui_node(&mut self, node_id: u32) {
        self.ui_selected_node = Some(node_id);
        self.show_properties_window = true;
        let changed =
            !self.shell.properties_visible || self.right_panel_tab != RightPanelTab::Inspector;
        self.shell.properties_visible = true;
        self.right_panel_tab = RightPanelTab::Inspector;
        if changed {
            self.shell.save();
        }
    }

    pub fn select_source(&mut self, source_id: u64) {
        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
        self.ensure_scene_home_workspace();
        self.selected_actor_id = None;
        self.selected_source_id = Some(source_id);
        self.selected_coord = None;
        self.selected_coords.clear();
        self.reveal_selected_properties();
    }

    /// Clears the active object selection without changing the current tool.
    pub fn clear_selection(&mut self) {
        self.selected_actor_id = None;
        self.selected_source_id = None;
        self.selected_coord = None;
        self.selected_coords.clear();
    }

    pub fn project_state(&self, project_path: &Path) -> EditorProjectState {
        let active_script = self
            .script_editor
            .active_document
            .as_ref()
            .and_then(|path| path.strip_prefix(project_path).ok())
            .map(|path| path.to_string_lossy().replace('\\', "/"));

        EditorProjectState {
            anchor: [self.anchor.x, self.anchor.y, self.anchor.z],
            selected_actor_id: self.selected_actor_id,
            selected_actor_directory_id: self.selected_actor_directory_id,
            selected_project_node_id: self.selected_project_node_id,
            expanded_project_nodes: {
                let mut nodes: Vec<_> = self.expanded_project_nodes.iter().copied().collect();

                nodes.sort_unstable();
                nodes
            },
            show_properties_window: self.show_properties_window,
            show_world_window: self.show_world_window,
            // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
            left_panel_tab: match self.left_panel_tab {
                LeftPanelTab::Project => "project".to_string(),
                LeftPanelTab::Terrain => "terrain".to_string(),
            },
            navigation_open: self.navigation_window.is_open,
            right_panel_split: self.right_panel_split,
            show_script_workspace: self.show_script_workspace,
            active_script,
            ..Default::default()
        }
    }

    pub fn apply_project_state(&mut self, state: &EditorProjectState, world: &crate::world::World) {
        self.anchor = WorldCoord::new(state.anchor[0], state.anchor[1], state.anchor[2]);

        self.camera.target = glam::Vec3::new(
            state.anchor[0] as f32,
            state.anchor[1] as f32,
            state.anchor[2] as f32,
        );

        self.navigation_window.sync(self.anchor);

        self.selected_actor_id = state.selected_actor_id;
        self.selected_actor_directory_id = state.selected_actor_directory_id;

        self.selected_project_node_id = state.selected_project_node_id;

        self.selected_project_nodes.clear();

        self.expanded_project_nodes = state.expanded_project_nodes.iter().copied().collect();

        if let Some(node_id) = self.selected_project_node_id {
            if !self.project_hierarchy.contains(node_id) {
                self.selected_project_node_id = None;
            }
        }

        self.selected_coord = None;
        self.selected_coords.clear();

        if self.selected_actor_id.is_some() {
            self.selected_actor_directory_id = None;
        }

        self.show_properties_window = state.show_properties_window;

        // Worldkiln 0.9.3 | WO 0.9.3-21 | 2026-10-07 15:37 -05:00
        // show_world_window is retained only for backward-compatible editor state loading.
        self.show_world_window = false;
        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:13 -05:00
        // Historical left-panel names are presentation metadata, not a
        // command to replace the 3D viewport on project open. Legacy
        // Lighting/Physics/Project Settings entries resolve to the normal
        // Explorer instead of triggering a modal settings surface.
        self.left_panel_tab = match state.left_panel_tab.to_ascii_lowercase().as_str() {
            "terrain" => LeftPanelTab::Terrain,
            _ => LeftPanelTab::Project,
        };

        self.navigation_window.is_open = state.navigation_open;

        self.right_panel_split = state.right_panel_split.clamp(0.1, 0.9);

        self.show_script_workspace = state.show_script_workspace;

        self.last_show_script_workspace = false;

        self.sanitize_actor_hierarchy_selection(world);
        // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:21 -05:00
        // Legacy editor states can persist a highlighted Actor node without
        // storing selected_actor_id, leaving the Inspector deceptively blank.
        // Resolve it ONCE during project load; not on every render/clear.
        if self.selected_actor_id.is_none() {
            if let Some(node_id) = self.selected_project_node_id {
                if let Some(crate::project::ProjectNodeKind::Actor { actor_id }) =
                    self.project_hierarchy.node(node_id).map(|node| &node.kind)
                {
                    if world.actors.contains_key(actor_id) {
                        self.selected_actor_id = Some(*actor_id);
                    }
                }
            }
        }
        if self.selected_actor_id.is_some() {
            self.reveal_selected_properties();
        }
    }

    /// Clears transient hover state.
    pub fn clear_hover(&mut self) {
        self.hovered_actor_id = None;
        self.hovered_source_id = None;
        self.hovered_cell = None;
        self.hovered_terrain_hit = None;
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:10 -05:00
    /// Clears ordinary scene-object hover without discarding an independently
    /// resolved Terrain surface hit used by Landscaping sculpt tools.
    pub fn clear_scene_object_hover(&mut self) {
        self.hovered_actor_id = None;
        self.hovered_source_id = None;
        self.hovered_cell = None;
    }

    /// Activates a standard editor tool and disables Landscaping sculpting so
    /// two authoring tools cannot own the viewport at the same time.
    pub fn activate_editor_tool(&mut self, tool: EditorTool) {
        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
        self.finish_sculpt_stroke();
        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:20 -05:00
        // Ordinary Select/Build/Erase are scene-editing tools. The moment
        // one is chosen from a terrain/navigation/script context, restore
        // the Home Explorer + Properties layout. Sculpting's Navigate tool
        // and independent UI authoring are deliberately excluded.
        if matches!(
            tool,
            EditorTool::Select | EditorTool::Build | EditorTool::Erase
        ) {
            self.ensure_scene_home_workspace();
        }
        self.current_tool = tool;
        self.transform_mode = transform_gizmo::TransformMode::Select;
        self.actor_transform_preview = None;
        self.sculpt_tool = crate::world::terrain::SculptTool::None;
        self.finish_actor_brush_stroke();
        self.actor_brush_enabled = false;
        self.hovered_terrain_hit = None;
    }

    /// Activates a Landscaping sculpt tool. A non-None sculpt tool parks the
    /// ordinary editor tool on Navigate internally while leaving sculpt as the
    /// only visible/authoring-active tool.
    pub fn activate_sculpt_tool(&mut self, tool: crate::world::terrain::SculptTool) {
        // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
        if tool != self.sculpt_tool {
            self.finish_sculpt_stroke();
        }
        self.sculpt_tool = tool;
        if tool != crate::world::terrain::SculptTool::None {
            self.finish_actor_brush_stroke();
            self.actor_brush_enabled = false;
            self.current_tool = EditorTool::Navigate;
            self.transform_mode = transform_gizmo::TransformMode::Select;
            self.actor_transform_preview = None;
            self.hovered_actor_id = None;
            self.hovered_source_id = None;
            self.hovered_cell = None;
        } else {
            self.hovered_terrain_hit = None;
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    pub fn set_actor_brush_enabled(&mut self, enabled: bool) {
        if enabled == self.actor_brush_enabled {
            return;
        }
        self.finish_sculpt_stroke();
        self.finish_actor_brush_stroke();
        self.actor_brush_enabled = enabled;
        if enabled {
            self.sculpt_tool = crate::world::terrain::SculptTool::None;
            self.current_tool = EditorTool::Navigate;
            self.clear_scene_object_hover();
        } else {
            self.hovered_terrain_hit = None;
        }
    }

    /// Returns whether Landscaping currently owns pointer-driven Terrain
    /// authoring. The App still separately requires the Editor view.
    pub fn sculpt_viewport_input_allowed(
        &self,
        in_viewport: bool,
        egui_wants_pointer: bool,
    ) -> bool {
        self.sculpt_tool != crate::world::terrain::SculptTool::None
            && self.mode == EditorMode::Editor
            && !self.ui_mode
            && !self.show_script_workspace
            && in_viewport
            && !egui_wants_pointer
    }

    pub fn actor_brush_viewport_input_allowed(
        &self,
        in_viewport: bool,
        egui_wants_pointer: bool,
    ) -> bool {
        self.actor_brush_enabled
            && self.actor_brush_package.is_some()
            && self.mode == EditorMode::Editor
            && !self.ui_mode
            && !self.show_script_workspace
            && in_viewport
            && !egui_wants_pointer
    }

    /// Finalizes one continuous sculpt stroke into the normal World history.
    /// Calling this repeatedly is intentionally idempotent.
    pub fn finish_sculpt_stroke(&mut self) {
        if !self.sculpt_stroke_active {
            return;
        }

        self.sculpt_stroke_active = false;
        if self.sculpt_stroke_mutated {
            if let Some(snapshot) = self.sculpt_stroke_snapshot.take() {
                self.history.push_undo_entry(snapshot);
            }
            self.history.redo_stack.clear();
            self.needs_save = true;
        } else {
            self.sculpt_stroke_snapshot = None;
        }

        self.sculpt_stroke_mutated = false;
        self.sculpt_flatten_target = None;
    }

    /// Commits every Actor scattered during one pointer drag as one history item.
    pub fn finish_actor_brush_stroke(&mut self) {
        if !self.actor_brush_stroke_active {
            return;
        }
        self.actor_brush_stroke_active = false;
        if self.actor_brush_stroke_mutated {
            if let Some(snapshot) = self.actor_brush_stroke_snapshot.take() {
                self.history.push_undo_entry(snapshot);
            }
            self.history.redo_stack.clear();
            self.needs_save = true;
        } else {
            self.actor_brush_stroke_snapshot = None;
        }
        self.actor_brush_stroke_mutated = false;
        self.actor_brush_last_center = None;
        self.actor_brush_stroke_positions.clear();
    }

    pub fn editor_tool_is_active(&self, tool: EditorTool) -> bool {
        self.sculpt_tool == crate::world::terrain::SculptTool::None
            && !self.actor_brush_enabled
            && self.current_tool == tool
    }

    pub fn set_anchor(&mut self, coord: WorldCoord) {
        self.anchor = coord;

        self.camera.target = glam::Vec3::new(coord.x as f32, coord.y as f32, coord.z as f32);

        self.navigation_window.sync(coord);
    }

    // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
    pub fn enter_ui_mode(&mut self) {
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
        self.commit_pending_inspector_history();
        // Worldkiln 0.8.5 | WO 0.8.7-71 | 2026-10-09 19:50 -05:00
        // Direct entry through Project Manager/UI document picker must restore
        // the existing UI Nodes and UI Properties, just like the UI tab.
        self.shell.explorer_visible = true;
        self.shell.properties_visible = true;
        self.show_properties_window = true;
        self.right_panel_tab = RightPanelTab::Inspector;
        self.ui_mode = true;
        self.mode = EditorMode::Editor;
        self.show_script_workspace = false;
        self.ui_drag_state = None;
        self.ui_drag_asset = None;
        // Workspace switching must not discard scene / UI selection.
        self.clear_hover();
    }

    pub fn sync_active_ui_document(&mut self) {
        if let Some(entry) = self
            .ui_workspace
            .document_mut(self.ui_workspace.active_document_id)
        {
            entry.document = self.ui_document.clone();
        }
    }

    pub fn open_ui_document(&mut self, document_id: u32) -> Result<(), String> {
        self.sync_active_ui_document();
        let document = self
            .ui_workspace
            .document(document_id)
            .ok_or_else(|| format!("unknown UI document {}", document_id))?
            .document
            .clone();
        self.ui_workspace.set_active_document(document_id)?;
        self.ui_document = document;
        self.ui_selected_node = None; // A new document has its own selection identity.
        self.ui_document_history.entry(document_id).or_default();
        self.enter_ui_mode();
        Ok(())
    }

    // Worldkiln 0.8.5 | WO 0.8.7-68 | 2026-10-09 16:54 -05:00
    pub fn exit_ui_mode(&mut self) {
        self.ui_mode = false;
        self.ui_drag_state = None;
        self.ui_drag_asset = None;
        self.clear_hover();
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:00 -05:00
    pub fn create_snapshot(&self, world: &crate::world::World) -> HistorySnapshot {
        (
            world.cells.clone(),
            world.sources.clone(),
            world.script_bindings.clone(),
            world.actors.clone(),
            world.actor_script_bindings.clone(),
            self.project_hierarchy.clone(),
            TerrainHistorySnapshot::Full(world.terrain.authored.clone()),
        )
    }

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
    pub(crate) fn commit_pending_inspector_history(&mut self) {
        if let Some(snapshot) = self.pending_inspector_history.take() {
            self.history.push_undo_entry(snapshot);
            self.history.redo_stack.clear();
        }
    }

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
    pub(crate) fn finish_inspector_history_if_idle(&mut self, ctx: &egui::Context) {
        let interaction_active =
            ctx.wants_keyboard_input() || ctx.input(|input| input.pointer.any_down());
        if !interaction_active {
            self.commit_pending_inspector_history();
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:17 -05:00
    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    pub(crate) fn create_layer_brush_snapshot(
        &self,
        world: &crate::world::World,
        id: u64,
    ) -> HistorySnapshot {
        let mut snapshot = self.create_snapshot_without_terrain(world);
        snapshot.6 = TerrainHistorySnapshot::LayerEditCount {
            layer_id: id,
            count: world
                .terrain
                .config
                .generation_layers
                .iter()
                .find(|l| l.id == id)
                .map(|l| l.brush_edits.len())
                .unwrap_or(0),
        };
        snapshot
    }

    pub(crate) fn create_brush_snapshot(&self, world: &crate::world::World) -> HistorySnapshot {
        let mut snapshot = self.create_snapshot_without_terrain(world);
        if let Some(crate::world::TerrainResult {
            data: crate::world::terrain::TerrainData::Voxel(grid),
            ..
        }) = world.terrain.authored.as_ref()
        {
            snapshot.6 = TerrainHistorySnapshot::Columns {
                origin: grid.origin,
                size: grid.size,
                spacing: grid.spacing_studs,
                values: HashMap::new(),
                materials: grid.materials.clone(),
            };
        } else {
            snapshot.6 = TerrainHistorySnapshot::Full(world.terrain.authored.clone());
        }
        snapshot
    }

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
    // Non-Terrain edits avoid copying potentially millions of authored voxels.
    pub(crate) fn create_snapshot_without_terrain(
        &self,
        world: &crate::world::World,
    ) -> HistorySnapshot {
        (
            world.cells.clone(),
            world.sources.clone(),
            world.script_bindings.clone(),
            world.actors.clone(),
            world.actor_script_bindings.clone(),
            self.project_hierarchy.clone(),
            TerrainHistorySnapshot::Unchanged,
        )
    }

    pub(crate) fn capture_brush_columns(
        &mut self,
        world: &crate::world::World,
        center: [f32; 3],
        radius: f32,
    ) {
        let Some(snapshot) = self.sculpt_stroke_snapshot.as_mut() else {
            return;
        };
        let TerrainHistorySnapshot::Columns {
            origin,
            size,
            spacing,
            values,
            ..
        } = &mut snapshot.6
        else {
            return;
        };
        let Some(crate::world::TerrainResult {
            data: crate::world::terrain::TerrainData::Voxel(grid),
            ..
        }) = world.terrain.authored.as_ref()
        else {
            return;
        };
        if grid.origin != *origin
            || grid.size != *size
            || grid.spacing_studs != *spacing
            || !radius.is_finite()
            || radius <= 0.0
        {
            return;
        }
        let cx = (center[0] - origin[0] as f32) / *spacing - 0.5;
        let cz = (center[2] - origin[2] as f32) / *spacing - 0.5;
        // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:33 -05:00
        // Local slope/material refresh affects a two-column halo around the
        // sculpt footprint; Undo must capture those adjacent columns too.
        let r = radius / *spacing + 2.0;
        // Worldkiln 0.8.5 | WO MAT-053 | 2026-10-10 17:21 -05:00
        // A side-face Paint brush intersects voxel AABBs even when their
        // centers lie outside its radius. Undo must capture that same
        // conservative XZ footprint, not the old center-distance circle.
        // Including a one-sample halo also covers sculpt edits at seams.
        // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:36 -05:00
        // Procedural slope recomputation includes *every* column in its
        // bounding square, including corners outside the radial sculpt area.
        // Match the same rectangle here so Undo preserves full material
        // recovery, but keep the snapshot scoped to those local columns.
        let min_x = ((cx - r).floor() as i32).clamp(0, size[0] as i32 - 1) as u32;
        let max_x = ((cx + r).ceil() as i32).clamp(0, size[0] as i32 - 1) as u32;
        let min_z = ((cz - r).floor() as i32).clamp(0, size[2] as i32 - 1) as u32;
        let max_z = ((cz + r).ceil() as i32).clamp(0, size[2] as i32 - 1) as u32;
        for z in min_z..=max_z {
            for x in min_x..=max_x {
                values.entry((x, z)).or_insert_with(|| {
                    (0..size[1]).map(|y| grid.voxel(x, y, z).unwrap()).collect()
                });
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 15:10 -05:00
    #[test]
    fn scene_object_hover_clear_preserves_terrain_hit() {
        let mut editor = Editor::new();
        editor.hovered_cell = Some(WorldCoord::new(1, 2, 3));
        editor.hovered_actor_id = Some(42);
        editor.hovered_source_id = Some(84);
        editor.hovered_terrain_hit = Some(crate::editor::grid::picking::SurfaceHit {
            coord: WorldCoord::new(4, 5, 6),
            point: glam::Vec3::new(4.5, 5.0, 6.5),
            normal: glam::Vec3::Y,
            distance: 10.0,
        });

        editor.clear_scene_object_hover();

        assert!(editor.hovered_cell.is_none());
        assert!(editor.hovered_actor_id.is_none());
        assert!(editor.hovered_source_id.is_none());
        assert!(editor.hovered_terrain_hit.is_some());
    }

    // Worldkiln 0.9.3 | WO 0.9.3-11 | 2026-10-07 01:37 -05:00
    #[test]
    fn paint_tool_defaults_to_surface_and_uses_sculpt_authoring_gate() {
        let mut editor = Editor::new();
        assert_eq!(
            editor.terrain_paint_material_slot,
            crate::world::terrain::TerrainPaintMaterialSlot::Surface
        );

        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Paint);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::Paint);
        assert_eq!(editor.current_tool, EditorTool::Navigate);
        assert!(editor.sculpt_viewport_input_allowed(true, false));

        editor.activate_editor_tool(EditorTool::Build);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::None);
        assert!(editor.editor_tool_is_active(EditorTool::Build));
    }

    // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:18 -05:00
    // Worldkiln 0.8.5 | WO 0.8.7-72 | 2026-10-09 20:23 -05:00
    // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:25 -05:00
    #[test]
    fn scene_selection_and_build_tools_switch_from_other_workspaces_to_home() {
        use crate::world::WorldCoord;
        use shell::Workspace;
        let mut editor = Editor::new();
        for (workspace, kind) in [
            (Workspace::Terrain, "Cell"),
            (Workspace::Navigation, "Source"),
            (Workspace::Script, "Actor"),
        ] {
            editor.activate_shell_workspace(workspace);
            editor.shell.explorer_visible = false;
            editor.shell.properties_visible = false;
            editor.right_panel_tab = RightPanelTab::Landscaping;
            match kind {
                "Cell" => editor.select_cell(WorldCoord::new(4, 2, 9)),
                "Source" => editor.select_source(41),
                "Actor" => editor.select_actor(42),
                _ => unreachable!(),
            }
            assert_eq!(editor.shell.workspace, Workspace::Home, "{kind}");
            assert_eq!(editor.left_panel_tab, LeftPanelTab::Project, "{kind}");
            assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector, "{kind}");
            assert!(
                editor.shell.explorer_visible && editor.shell.properties_visible,
                "{kind}"
            );
            assert!(!editor.ui_mode && !editor.show_script_workspace, "{kind}");
            match kind {
                "Cell" => assert_eq!(editor.selected_coord, Some(WorldCoord::new(4, 2, 9))),
                "Source" => assert_eq!(editor.selected_source_id, Some(41)),
                "Actor" => assert_eq!(editor.selected_actor_id, Some(42)),
                _ => unreachable!(),
            }
        }
        for tool in [EditorTool::Select, EditorTool::Build, EditorTool::Erase] {
            editor.activate_shell_workspace(Workspace::Terrain);
            editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Paint);
            editor.activate_editor_tool(tool);
            assert_eq!(editor.shell.workspace, Workspace::Home);
            assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
            assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
            assert!(editor.shell.explorer_visible && editor.shell.properties_visible);
            assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::None);
        }
    }
    #[test]
    fn ui_element_selection_stays_in_ui_and_landscape_sculpt_stays_in_terrain() {
        let mut editor = Editor::new();
        editor.activate_shell_workspace(shell::Workspace::Ui);
        editor.select_ui_node(77);
        assert_eq!(editor.shell.workspace, shell::Workspace::Ui);
        assert!(editor.ui_mode);
        assert_eq!(editor.ui_selected_node, Some(77));
        editor.activate_shell_workspace(shell::Workspace::Terrain);
        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Paint);
        assert_eq!(editor.shell.workspace, shell::Workspace::Terrain);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::Paint);
    }
    #[test]
    fn legacy_project_sidebar_value_cannot_open_project_settings_on_restore() {
        let mut editor = Editor::new();
        let world = crate::world::World::new();
        for old in ["project_settings", "lighting", "physics"] {
            editor.activate_shell_workspace(shell::Workspace::Terrain);
            let mut state = crate::project::EditorProjectState::default();
            state.left_panel_tab = old.into();
            editor.apply_project_state(&state, &world);
            assert_eq!(
                editor.navigation.surface,
                feature_nav::EditorSurface::Workspace,
                "restoring {old} must not open a page"
            );
            assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
        }
    }

    #[test]
    fn restored_highlighted_actor_node_reconstitutes_inspector_selection_once() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let id = world.add_actor("Restored Actor", glam::Vec3::new(2.0, 3.0, 4.0));
        let node = editor
            .project_hierarchy
            .create_actor_node(None, id)
            .unwrap();
        let mut saved = crate::project::EditorProjectState::default();
        saved.selected_project_node_id = Some(node);
        saved.selected_actor_id = None; // Legacy state with highlighted row only.
        saved.show_properties_window = false;
        editor.shell.properties_visible = false;
        editor.right_panel_tab = RightPanelTab::Landscaping;
        editor.apply_project_state(&saved, &world);
        assert_eq!(editor.selected_actor_id, Some(id));
        assert_eq!(editor.selected_project_node_id, Some(node));
        assert!(editor.shell.properties_visible);
        assert!(editor.show_properties_window);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
        editor.clear_selection();
        // Explicit clearing remains meaningful: no render-time auto reselect.
        assert_eq!(editor.selected_actor_id, None);
    }

    #[test]
    fn selecting_actor_cell_or_source_switches_open_properties_inspector() {
        use crate::world::WorldCoord;
        let mut editor = Editor::new();
        for (kind, index) in [("Actor", 11), ("Cell", 12), ("Source", 13)] {
            // Reproduce the user's hidden Inspector while Landscaping/AI is open.
            editor.show_properties_window = false;
            editor.shell.properties_visible = false;
            editor.right_panel_tab = RightPanelTab::Landscaping;
            match kind {
                "Actor" => editor.select_actor(index),
                "Cell" => editor.select_cell(WorldCoord::new(index as i32, 1, 2)),
                "Source" => editor.select_source(index),
                _ => unreachable!(),
            }
            assert!(
                editor.show_properties_window,
                "{kind} should show Properties"
            );
            assert!(
                editor.shell.properties_visible,
                "{kind} should unhide right dock"
            );
            assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
            match kind {
                "Actor" => assert_eq!(editor.selected_actor_id, Some(index)),
                "Cell" => assert_eq!(
                    editor.selected_coord,
                    Some(WorldCoord::new(index as i32, 1, 2))
                ),
                "Source" => assert_eq!(editor.selected_source_id, Some(index)),
                _ => unreachable!(),
            }
        }
    }
    #[test]
    fn selecting_ui_node_reopens_existing_ui_properties_without_mutating_scene() {
        let mut editor = Editor::new();
        editor.enter_ui_mode();
        editor.shell.properties_visible = false;
        editor.show_properties_window = false;
        editor.right_panel_tab = RightPanelTab::Ai;
        editor.selected_actor_id = Some(28);
        editor.select_ui_node(37);
        assert_eq!(editor.ui_selected_node, Some(37));
        assert_eq!(editor.selected_actor_id, Some(28));
        assert!(editor.show_properties_window);
        assert!(editor.shell.properties_visible);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
    }
    #[test]
    fn clicking_texture_updates_build_template_selected_blocks_and_undo() {
        use crate::world::{CellType, World, WorldCoord};
        let mut editor = Editor::new();
        let mut world = World::new();
        let a = WorldCoord::new(1, 1, 1);
        let b = WorldCoord::new(2, 1, 1);
        let untouched = WorldCoord::new(3, 1, 1);
        world.set_cell(a, CellType::Block);
        world.set_cell(b, CellType::FxBlock);
        world.set_cell(untouched, CellType::Block);
        let original = world.get(untouched).unwrap().texture.clone();
        editor.selected_coords = vec![a, b];
        editor.selected_coord = Some(b);
        editor.shell.properties_visible = false;
        editor.right_panel_tab = RightPanelTab::Navigation;
        let previous_history_len = editor.history.undo_stack.len();
        let changed = editor.apply_build_texture_selection(&mut world, "wood_1".into());
        assert_eq!(changed, 2);
        assert_eq!(editor.build_template.texture, "wood_1");
        assert_eq!(world.get(a).unwrap().texture, "wood_1");
        assert_eq!(world.get(b).unwrap().texture, "wood_1");
        assert_eq!(world.get(untouched).unwrap().texture, original);
        assert_eq!(editor.history.undo_stack.len(), previous_history_len + 1);
        assert!(editor.needs_save);
        assert!(editor.shell.properties_visible);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
        // Second click on identical texture must not grow the undo stack.
        assert_eq!(
            editor.apply_build_texture_selection(&mut world, "wood_1".into()),
            0
        );
        assert_eq!(editor.history.undo_stack.len(), previous_history_len + 1);
    }
    #[test]
    fn texture_picked_without_selected_block_sets_future_build_only() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        assert_eq!(
            editor.apply_build_texture_selection(&mut world, "tile_24".into()),
            0
        );
        assert_eq!(editor.build_template.texture, "tile_24");
        assert!(editor.history.undo_stack.is_empty());
        assert!(!editor.needs_save);
    }

    #[test]
    fn sculpt_and_standard_authoring_tools_are_mutually_exclusive() {
        let mut editor = Editor::new();
        editor.activate_editor_tool(EditorTool::Build);
        assert!(editor.editor_tool_is_active(EditorTool::Build));

        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Raise);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::Raise);
        assert_eq!(editor.current_tool, EditorTool::Navigate);
        assert!(!editor.editor_tool_is_active(EditorTool::Navigate));
        assert!(!editor.editor_tool_is_active(EditorTool::Build));

        editor.activate_editor_tool(EditorTool::Erase);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::None);
        assert!(editor.editor_tool_is_active(EditorTool::Erase));
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:02 -05:00
    #[test]
    fn sculpt_input_gate_rejects_play_ui_script_and_egui_ownership() {
        let mut editor = Editor::new();
        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Raise);

        assert!(editor.sculpt_viewport_input_allowed(true, false));
        assert!(!editor.sculpt_viewport_input_allowed(false, false));
        assert!(!editor.sculpt_viewport_input_allowed(true, true));

        editor.show_script_workspace = true;
        assert!(!editor.sculpt_viewport_input_allowed(true, false));
        editor.show_script_workspace = false;

        editor.ui_mode = true;
        assert!(!editor.sculpt_viewport_input_allowed(true, false));
        editor.ui_mode = false;

        editor.mode = EditorMode::Play;
        assert!(!editor.sculpt_viewport_input_allowed(true, false));
        editor.mode = EditorMode::Editor;

        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::None);
        assert!(!editor.sculpt_viewport_input_allowed(true, false));
    }

    // Worldkiln 0.8.5 | WO 0.8.7-56 | 2026-10-09 13:00 -05:00
    #[test]
    fn layer_stroke_history_undo_redo_uses_compact_journal_tail() {
        let mut world = crate::world::World::new();
        let request = crate::world::terrain::TerrainGenerationRequest {
            position: [0, 0, 0],
            size: [24, 32, 24],
        };
        let id = world.terrain.add_generation_layer(request).unwrap();
        world.terrain.generate_selected_layer(id).unwrap();
        let base = world.terrain.authored.clone();
        let editor = Editor::new();
        let before = editor.create_layer_brush_snapshot(&world, id);
        let brush = crate::world::terrain::LayerBrushEdit::Paint {
            center: [12.5, 18.0, 12.5],
            radius: 3.0,
            material: "history_paint_test".into(),
        };
        assert!(
            world
                .terrain
                .apply_generation_layer_brush(id, brush.clone())
                .unwrap()
        );
        assert_eq!(
            world.terrain.config.generation_layers[0].brush_edits.len(),
            1
        );
        let redo = before.6.counterpart(&world.terrain);
        match &redo {
            TerrainHistorySnapshot::LayerEditAppend { edits, .. } => {
                assert_eq!(edits, &vec![brush])
            }
            _ => panic!("expected compact layer edit inverse"),
        }
        before.6.apply(&mut world.terrain);
        assert!(
            world.terrain.config.generation_layers[0]
                .brush_edits
                .is_empty()
        );
        let undo = redo.counterpart(&world.terrain);
        redo.apply(&mut world.terrain);
        assert_eq!(
            world.terrain.config.generation_layers[0].brush_edits.len(),
            1
        );
        undo.apply(&mut world.terrain);
        assert!(
            world.terrain.config.generation_layers[0]
                .brush_edits
                .is_empty()
        );
        assert_eq!(world.terrain.authored, base);
    }

    #[test]
    fn finishing_changed_sculpt_stroke_commits_exactly_one_history_entry() {
        let mut editor = Editor::new();
        let world = crate::world::World::new();
        let snapshot = editor.create_snapshot(&world);

        editor.sculpt_stroke_active = true;
        editor.sculpt_stroke_mutated = true;
        editor.sculpt_stroke_snapshot = Some(snapshot.clone());
        editor.sculpt_flatten_target = Some(3.0);
        editor.history.redo_stack.push(snapshot);

        editor.finish_sculpt_stroke();

        assert_eq!(editor.history.undo_stack.len(), 1);
        assert!(editor.history.redo_stack.is_empty());
        assert!(editor.needs_save);
        assert!(!editor.sculpt_stroke_active);
        assert!(!editor.sculpt_stroke_mutated);
        assert!(editor.sculpt_stroke_snapshot.is_none());
        assert!(editor.sculpt_flatten_target.is_none());

        editor.finish_sculpt_stroke();
        assert_eq!(editor.history.undo_stack.len(), 1);
    }

    #[test]
    fn finishing_unchanged_sculpt_stroke_does_not_dirty_or_create_history() {
        let mut editor = Editor::new();
        let world = crate::world::World::new();

        editor.sculpt_stroke_active = true;
        editor.sculpt_stroke_snapshot = Some(editor.create_snapshot(&world));
        editor.finish_sculpt_stroke();

        assert!(editor.history.undo_stack.is_empty());
        assert!(!editor.needs_save);
        assert!(!editor.sculpt_stroke_active);
        assert!(editor.sculpt_stroke_snapshot.is_none());
    }

    // Worldkiln 0.9.1 | WO 0.9.1-01 | 2026-10-05 12:21 -05:00
    #[test]
    fn terrain_settings_generate_authored_data_and_mark_project_dirty() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        editor.terrain_generation_position = [-4, 0, 7];
        editor.terrain_generation_size = [8, 16, 9];
        editor.terrain_mode_selection = crate::world::terrain::TerrainMode::Block;

        assert!(editor.generate_authored_terrain(&mut world));
        assert!(world.terrain.authored.is_some());
        assert_eq!(
            world.terrain.config.mode,
            crate::world::terrain::TerrainMode::Block
        );
        assert!(editor.needs_save);
        assert!(editor.terrain_generation_error.is_none());
    }

    #[test]
    fn terrain_settings_surface_actionable_generation_errors() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        editor.terrain_generation_size = [0, 16, 16];

        assert!(!editor.generate_authored_terrain(&mut world));
        assert!(world.terrain.authored.is_none());
        assert!(
            editor
                .terrain_generation_error
                .as_deref()
                .is_some_and(|error| error.contains("x") && error.contains("greater than zero"))
        );
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    #[test]
    fn failed_generation_preserves_existing_terrain_and_committed_mode() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        world.terrain.config.mode = crate::world::terrain::TerrainMode::Smooth;
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 16, 8],
            })
            .unwrap();
        let authored_before = world.terrain.authored.clone();

        editor.terrain_mode_selection = crate::world::terrain::TerrainMode::Block;
        editor.terrain_generation_size = [0, 16, 8];

        assert!(!editor.generate_authored_terrain(&mut world));
        assert_eq!(world.terrain.authored, authored_before);
        assert_eq!(
            world.terrain.config.mode,
            crate::world::terrain::TerrainMode::Smooth
        );
        assert!(!editor.needs_save);
    }

    #[test]
    fn test_actor_hierarchy_selection_starts_empty() {
        let editor = Editor::new();

        assert_eq!(editor.selected_actor_id, None);
        assert_eq!(editor.selected_actor_directory_id, None);
        assert!(editor.actor_directory_name.is_empty());
    }

    #[test]
    fn test_actor_and_directory_selection_are_independent() {
        let mut editor = Editor::new();

        editor.selected_actor_directory_id = Some(42);
        editor.selected_actor_id = None;

        assert_eq!(editor.selected_actor_directory_id, Some(42));
        assert_eq!(editor.selected_actor_id, None);

        editor.selected_actor_id = Some(99);
        editor.selected_actor_directory_id = None;

        assert_eq!(editor.selected_actor_id, Some(99));
        assert_eq!(editor.selected_actor_directory_id, None);
    }

    #[test]
    fn test_missing_actor_selection_is_cleared_by_hierarchy_safety() {
        let mut editor = Editor::new();
        let world = crate::world::World::new();

        editor.selected_actor_id = Some(123456);

        editor.sanitize_actor_hierarchy_selection(&world);

        assert_eq!(editor.selected_actor_id, None);
    }

    #[test]
    fn test_missing_directory_selection_is_cleared_by_hierarchy_safety() {
        let mut editor = Editor::new();
        let world = crate::world::World::new();

        editor.selected_actor_directory_id = Some(654321);

        editor.sanitize_actor_hierarchy_selection(&world);

        assert_eq!(editor.selected_actor_directory_id, None);
    }

    #[test]
    fn test_editor_script_workspace_toggle() {
        let mut editor = Editor::new();

        assert!(!editor.show_script_workspace);

        editor.show_script_workspace = true;

        assert!(editor.show_script_workspace);

        editor.show_script_workspace = false;

        assert!(!editor.show_script_workspace);
    }

    #[test]
    fn output_log_is_rendered_in_script_editor_workspace() {
        let mut editor = Editor::new();
        editor.show_script_workspace = true;
        editor.terminal_output = "[SCRIPT] play-mode-output-sentinel\n".to_string();

        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let world = &mut crate::world::World::new();
        let renderer = crate::renderer::Renderer::new_for_tests(1280.0, 800.0);

        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(screen_rect),
                ..Default::default()
            },
            |ctx| editor.show_ui(ctx, world, &None, &renderer),
        );

        let log_is_visible = output.shapes.iter().any(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return false;
            };

            text.galley.job.text.contains("play-mode-output-sentinel")
                && text.visual_bounding_rect().intersects(screen_rect)
        });
        let output_header_y = output.shapes.iter().find_map(|clipped| {
            let egui::Shape::Text(text) = &clipped.shape else {
                return None;
            };

            text.galley
                .job
                .text
                .eq_ignore_ascii_case("Output")
                .then_some(text.pos.y)
        });

        assert!(
            editor.script_editor.show_output,
            "Output should stay enabled while entering Script Editor mode"
        );
        assert!(
            log_is_visible,
            "the runtime log should be painted while Script Editor is open"
        );
        assert!(
            output_header_y.is_some_and(|y| y > screen_rect.bottom() - 160.0),
            "the Script Editor Output dock should use its compact bottom layout"
        );
    }

    #[test]
    fn output_controls_are_visible_and_clear_both_logs_at_constrained_sizes() {
        for (width, height, scale) in [
            (1280.0, 720.0, 1.0),
            (1600.0, 900.0, 1.25),
            (1920.0, 1080.0, 1.5),
            (800.0, 450.0, 2.0),
        ] {
            for script_mode in [false, true] {
                let mut editor = Editor::new();
                editor.show_script_workspace = script_mode;
                editor.output_panel_height = 110.0;
                editor.script_output_panel_height = 86.0;
                editor.terminal_output = "long output line\n".repeat(100);
                editor.script_editor.push_output("editor log".into());
                let ctx = egui::Context::default();
                let screen = egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(width / scale, height / scale),
                );
                let render = |editor: &mut Editor, events| {
                    ctx.run(
                        egui::RawInput {
                            screen_rect: Some(screen),
                            events,
                            ..Default::default()
                        },
                        |ctx| editor.draw_output_panel(ctx),
                    )
                };
                let output = render(&mut editor, Vec::new());
                let mut clear_pos = None;
                for label in ["Clear", "Auto-scroll", "×"] {
                    let clipped = output.shapes.iter().find(|clipped| {
                        matches!(&clipped.shape, egui::Shape::Text(text) if text.galley.job.text == label)
                    }).unwrap_or_else(|| panic!("missing {label}"));
                    let egui::Shape::Text(text) = &clipped.shape else {
                        unreachable!()
                    };
                    let rect = text.visual_bounding_rect();
                    assert!(
                        screen.contains_rect(rect) && clipped.clip_rect.contains_rect(rect),
                        "{label} clipped at {width}x{height}, scale {scale}, script mode {script_mode}: {rect:?}"
                    );
                    if label == "Clear" {
                        clear_pos = Some(rect.center());
                    }
                }
                let pos = clear_pos.unwrap();
                let _ = render(
                    &mut editor,
                    vec![
                        egui::Event::PointerMoved(pos),
                        egui::Event::PointerButton {
                            pos,
                            button: egui::PointerButton::Primary,
                            pressed: true,
                            modifiers: egui::Modifiers::default(),
                        },
                    ],
                );
                let _ = render(
                    &mut editor,
                    vec![egui::Event::PointerButton {
                        pos,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: egui::Modifiers::default(),
                    }],
                );
                assert!(editor.terminal_output.is_empty());
                assert!(editor.script_editor.output_log.is_empty());
            }
        }
    }

    #[test]
    fn output_panel_keeps_user_height_across_frames_and_visibility_toggle() {
        let mut editor = Editor::new();
        // WO68: Home starts collapsed. The user explicitly opens Output
        // before dragging its splitter; resize and visibility still persist.
        editor.shell.dock = super::shell::DockTab::Output;
        editor.shell.dock_expanded = true;
        editor.script_editor.show_output = true;
        let ctx = egui::Context::default();
        let screen_rect = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0));
        let world = &mut crate::world::World::new();
        let renderer = crate::renderer::Renderer::new_for_tests(1280.0, 800.0);

        let render = |ctx: &egui::Context,
                      editor: &mut Editor,
                      world: &mut crate::world::World,
                      events: Vec<egui::Event>| {
            ctx.run(
                egui::RawInput {
                    screen_rect: Some(screen_rect),
                    events,
                    ..Default::default()
                },
                |ctx| editor.show_ui(ctx, world, &None, &renderer),
            )
        };

        let header_y = |output: &egui::FullOutput| {
            output.shapes.iter().find_map(|clipped| {
                let egui::Shape::Text(text) = &clipped.shape else {
                    return None;
                };
                text.galley
                    .job
                    .text
                    .eq_ignore_ascii_case("Output")
                    .then_some(text.pos.y)
            })
        };

        let first = render(&ctx, &mut editor, world, Vec::new());
        let splitter = egui::pos2(640.0, header_y(&first).unwrap() - 10.0);
        let modifiers = egui::Modifiers::default();
        let _ = render(
            &ctx,
            &mut editor,
            world,
            vec![
                egui::Event::PointerMoved(splitter),
                egui::Event::PointerButton {
                    pos: splitter,
                    button: egui::PointerButton::Primary,
                    pressed: true,
                    modifiers,
                },
            ],
        );
        let expanded_pointer = splitter - egui::vec2(0.0, 220.0);
        let _ = render(
            &ctx,
            &mut editor,
            world,
            vec![egui::Event::PointerMoved(expanded_pointer)],
        );
        let expanded_height = editor.output_panel_height;
        let _ = render(
            &ctx,
            &mut editor,
            world,
            vec![egui::Event::PointerButton {
                pos: expanded_pointer,
                button: egui::PointerButton::Primary,
                pressed: false,
                modifiers,
            }],
        );

        editor.script_editor.show_output = false;
        let _ = render(&ctx, &mut editor, world, Vec::new());
        editor.script_editor.show_output = true;
        let second = render(&ctx, &mut editor, world, Vec::new());

        assert!(
            expanded_height > 350.0,
            "drag should enlarge the Output dock"
        );
        assert_eq!(editor.output_panel_height, expanded_height);
        assert!(header_y(&second).is_some_and(|y| y < 500.0));
    }

    #[test]
    fn test_ui_mode_toggle_state() {
        let mut editor = Editor::new();

        assert!(!editor.ui_mode);

        editor.enter_ui_mode();

        assert!(editor.ui_mode);
        assert_eq!(editor.mode, EditorMode::Editor);

        editor.exit_ui_mode();

        assert!(!editor.ui_mode);
    }

    #[test]
    fn test_switching_ui_documents_preserves_each_canvas() {
        let mut editor = Editor::new();
        editor
            .ui_document
            .nodes
            .push(crate::engine::ui_authoring::AuthoredUiNode {
                id: 1,
                text: "MAIN".to_string(),
                ..Default::default()
            });
        editor.ui_document.next_id = 2;

        editor.sync_active_ui_document();
        let hud = editor.ui_workspace.create_document("Game HUD").unwrap();
        editor.open_ui_document(hud).unwrap();
        editor
            .ui_document
            .nodes
            .push(crate::engine::ui_authoring::AuthoredUiNode {
                id: 1,
                text: "HUD".to_string(),
                ..Default::default()
            });

        editor.open_ui_document(1).unwrap();
        assert_eq!(editor.ui_document.nodes[0].text, "MAIN");
        editor.open_ui_document(hud).unwrap();
        assert_eq!(editor.ui_document.nodes[0].text, "HUD");
    }

    // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
    #[test]
    fn ui_document_undo_redo_timelines_survive_switches_and_stay_isolated() {
        let mut editor = Editor::new();
        editor
            .ui_document
            .nodes
            .push(crate::engine::ui_authoring::AuthoredUiNode {
                id: 1,
                text: "MAIN before".to_string(),
                ..Default::default()
            });
        editor.ui_document.next_id = 2;
        editor.sync_active_ui_document();

        let hud = editor.ui_workspace.create_document("Game HUD").unwrap();
        editor.open_ui_document(hud).unwrap();
        editor
            .ui_document
            .nodes
            .push(crate::engine::ui_authoring::AuthoredUiNode {
                id: 1,
                text: "HUD before".to_string(),
                ..Default::default()
            });
        let hud_before = editor.ui_document.clone();
        editor.ui_document.nodes[0].text = "HUD after".to_string();
        editor
            .ui_document_history
            .entry(hud)
            .or_default()
            .undo
            .push(hud_before);
        editor.sync_active_ui_document();

        editor.open_ui_document(1).unwrap();
        editor.ui_document.nodes[0].text = "MAIN after".to_string();
        let main_id = editor.ui_workspace.active_document_id;
        let main_before = crate::engine::ui_authoring::AuthoredUiDocument {
            nodes: vec![crate::engine::ui_authoring::AuthoredUiNode {
                id: 1,
                text: "MAIN before".to_string(),
                ..Default::default()
            }],
            next_id: 2,
            ..Default::default()
        };
        editor
            .ui_document_history
            .entry(main_id)
            .or_default()
            .undo
            .push(main_before);
        editor.undo_ui_document();
        assert_eq!(editor.ui_document.nodes[0].text, "MAIN before");
        editor.redo_ui_document();
        assert_eq!(editor.ui_document.nodes[0].text, "MAIN after");

        editor.open_ui_document(hud).unwrap();
        assert_eq!(editor.ui_document.nodes[0].text, "HUD after");
        editor.undo_ui_document();
        assert_eq!(editor.ui_document.nodes[0].text, "HUD before");
    }

    #[test]
    fn test_actor_selection_clears_cell_selection() {
        let mut editor = Editor::new();

        let cell = WorldCoord::new(5, 2, 8);

        editor.select_cell(cell);

        assert_eq!(editor.selected_coord, Some(cell));
        assert!(editor.selected_actor_id.is_none());

        editor.select_actor(42);

        assert_eq!(editor.selected_actor_id, Some(42));
        assert!(editor.selected_coord.is_none());
        assert!(editor.selected_coords.is_empty());
    }

    #[test]
    fn test_cell_selection_clears_actor_selection() {
        let mut editor = Editor::new();

        editor.select_actor(42);

        assert_eq!(editor.selected_actor_id, Some(42));

        let cell = WorldCoord::new(-2, 4, 7);

        editor.select_cell(cell);

        assert!(editor.selected_actor_id.is_none());
        assert_eq!(editor.selected_coord, Some(cell));
        assert_eq!(editor.selected_coords, vec![cell]);
    }

    #[test]
    fn test_clear_selection_clears_actor_and_cells() {
        let mut editor = Editor::new();

        editor.select_actor(42);
        editor.clear_selection();

        assert!(editor.selected_actor_id.is_none());
        assert!(editor.selected_coord.is_none());
        assert!(editor.selected_coords.is_empty());
    }

    #[test]
    fn test_editor_script_refresh_on_transition() {
        let mut editor = Editor::new();

        let test_dir = std::env::temp_dir().join("TestProject_RefreshTransition");

        let scripts_dir = test_dir.join("scripts");

        if test_dir.exists() {
            let _ = std::fs::remove_dir_all(&test_dir);
        }

        std::fs::create_dir_all(&scripts_dir).unwrap();

        std::fs::write(scripts_dir.join("test.aeo"), "").unwrap();

        let ctx = egui::Context::default();
        let mut world = crate::world::World::new();
        let renderer = crate::renderer::Renderer::new_for_tests(1280.0, 800.0);

        assert!(!editor.show_script_workspace);
        assert!(editor.script_editor.scripts_list.is_empty());

        editor.show_script_workspace = true;

        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            editor.show_ui(ctx, &mut world, &Some(test_dir.clone()), &renderer);
        });

        assert_eq!(editor.script_editor.scripts_list.len(), 1);

        let _ = std::fs::remove_dir_all(&test_dir);
    }

    #[test]
    fn test_editor_state_restores_actor_selection_only_when_valid() {
        let mut editor = Editor::new();
        let world = crate::world::World::new();

        let state = crate::project::EditorProjectState {
            selected_actor_id: Some(99999),
            selected_actor_directory_id: Some(88888),
            anchor: [4, 5, 6],
            ..Default::default()
        };

        editor.apply_project_state(&state, &world);

        assert_eq!(editor.anchor, WorldCoord::new(4, 5, 6));

        assert!(editor.selected_actor_id.is_none());
        assert!(editor.selected_actor_directory_id.is_none());
    }

    #[test]
    fn test_project_explorer_state_starts_empty() {
        let editor = Editor::new();

        assert_eq!(editor.selected_project_node_id, None);

        assert!(editor.selected_project_nodes.is_empty());

        assert!(editor.expanded_project_nodes.is_empty());

        assert!(editor.project_hierarchy.nodes.is_empty());
    }

    #[test]
    fn test_project_explorer_selection_can_reference_hierarchy_node() {
        let mut editor = Editor::new();

        let folder = editor
            .project_hierarchy
            .create_folder(None, "Dungeon")
            .unwrap();

        editor.selected_project_node_id = Some(folder);
        editor.selected_project_nodes = vec![folder];
        editor.expanded_project_nodes.insert(folder);

        assert_eq!(editor.selected_project_node_id, Some(folder));

        assert_eq!(editor.selected_project_nodes, vec![folder]);

        assert!(editor.expanded_project_nodes.contains(&folder));
    }

    #[test]
    fn test_history_default_limit() {
        let history = History::new();
        assert_eq!(history.limit, 100);
    }

    #[test]
    fn test_history_respects_configured_limit() {
        let mut history = History::new_with_limit(3);
        assert_eq!(history.limit, 3);

        for _ in 0..5 {
            history.push(
                HashMap::new(),
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
                Vec::new(),
                ProjectHierarchy::new(),
                None,
            );
        }

        assert_eq!(history.undo_stack.len(), 3);
    }

    #[test]
    fn test_history_lowering_limit_preserves_newest_entries() {
        let mut history = History::new_with_limit(10);

        for _ in 0..5 {
            history.push(
                HashMap::new(),
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
                Vec::new(),
                ProjectHierarchy::new(),
                None,
            );
        }
        assert_eq!(history.undo_stack.len(), 5);

        history.set_limit(2);
        assert_eq!(history.limit, 2);
        assert_eq!(history.undo_stack.len(), 2);
    }

    #[test]
    fn test_history_increasing_limit_permits_future_growth() {
        let mut history = History::new_with_limit(2);

        for _ in 0..5 {
            history.push(
                HashMap::new(),
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
                Vec::new(),
                ProjectHierarchy::new(),
                None,
            );
        }
        assert_eq!(history.undo_stack.len(), 2);

        history.set_limit(5);
        assert_eq!(history.limit, 5);
        assert_eq!(history.undo_stack.len(), 2);

        for _ in 0..3 {
            history.push(
                HashMap::new(),
                HashMap::new(),
                Vec::new(),
                HashMap::new(),
                Vec::new(),
                ProjectHierarchy::new(),
                None,
            );
        }
        assert_eq!(history.undo_stack.len(), 5);
    }

    #[test]
    fn test_history_push_undo_entry_does_not_clear_redo() {
        let mut history = History::new_with_limit(5);
        let snapshot = (
            HashMap::new(),
            HashMap::new(),
            Vec::new(),
            HashMap::new(),
            Vec::new(),
            ProjectHierarchy::new(),
            TerrainHistorySnapshot::Full(None),
        );

        history.redo_stack.push(snapshot.clone());
        assert_eq!(history.redo_stack.len(), 1);

        history.push_undo_entry(snapshot.clone());

        assert_eq!(history.undo_stack.len(), 1);
        assert_eq!(history.redo_stack.len(), 1);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 21:37 -05:00
    #[test]
    fn actor_brush_drag_commits_one_grouped_history_entry() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        editor.actor_brush_stroke_active = true;
        editor.actor_brush_stroke_mutated = true;
        editor.actor_brush_stroke_snapshot = Some(editor.create_snapshot(&world));
        world.add_actor("tree", glam::Vec3::new(1.0, 2.0, 3.0));
        world.add_actor("rock", glam::Vec3::new(4.0, 5.0, 6.0));

        editor.finish_actor_brush_stroke();

        assert_eq!(editor.history.undo_stack.len(), 1);
        assert!(editor.history.redo_stack.is_empty());
        assert!(editor.needs_save);
        assert!(!editor.actor_brush_stroke_active);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-44 | 2026-10-08 22:13 -05:00
    #[test]
    fn selecting_actor_brush_and_sculpt_tools_keeps_one_landscaping_tool_active() {
        let mut editor = Editor::new();
        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Paint);
        editor.set_actor_brush_enabled(true);
        assert!(editor.actor_brush_enabled);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::None);

        editor.activate_sculpt_tool(crate::world::terrain::SculptTool::Raise);
        assert!(!editor.actor_brush_enabled);
        assert_eq!(editor.sculpt_tool, crate::world::terrain::SculptTool::Raise);
    }

    // Worldkiln 0.9.2 | WO 0.9.2-02 | 2026-10-05 23:18 -05:00
    #[test]
    fn sculpt_stroke_undo_and_redo_restore_exact_authored_terrain() {
        let editor = Editor::new();
        let mut history = History::new();
        let mut world = crate::world::World::new();
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [16, 32, 16],
            })
            .unwrap();

        let initial_snapshot = editor.create_snapshot(&world);
        let initial_terrain = world.terrain.authored.clone();
        assert!(
            world.sculpt_authored_terrain(&crate::world::terrain::TerrainSculptRequest {
                tool: crate::world::terrain::SculptTool::Raise,
                center: [8.5, 0.0, 8.5],
                radius: 4.0,
                strength: 3.0,
                flatten_target: None,
            })
        );
        let sculpted_snapshot = editor.create_snapshot(&world);
        let sculpted_terrain = world.terrain.authored.clone();
        assert_ne!(sculpted_terrain, initial_terrain);

        history.push_undo_entry(initial_snapshot);
        let undo = history.undo_stack.pop().unwrap();
        history.redo_stack.push(sculpted_snapshot.clone());
        undo.6.apply(&mut world.terrain);
        assert_eq!(world.terrain.authored, initial_terrain);

        let redo = history.redo_stack.pop().unwrap();
        history.push_undo_entry(editor.create_snapshot(&world));
        redo.6.apply(&mut world.terrain);
        assert_eq!(world.terrain.authored, sculpted_terrain);
        assert_eq!(history.undo_stack.len(), 1);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:21 -05:00
    #[test]
    fn wo37_brush_undo_records_small_columns_and_round_trips_without_full_terrain() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [32, 32, 32],
            })
            .unwrap();
        let original = world.terrain.authored.clone();
        editor.sculpt_stroke_snapshot = Some(editor.create_brush_snapshot(&world));
        editor.sculpt_stroke_active = true;
        let request = crate::world::terrain::TerrainSculptRequest {
            tool: crate::world::terrain::SculptTool::Raise,
            center: [16.0, 22.0, 16.0],
            radius: 4.0,
            strength: 3.0,
            flatten_target: None,
        };
        editor.capture_brush_columns(&world, request.center, request.radius);
        assert!(world.sculpt_authored_terrain(&request));
        let after_first = world.terrain.authored.clone();
        // Drag over the same position again: original column values must remain.
        editor.capture_brush_columns(&world, request.center, request.radius);
        assert!(world.sculpt_authored_terrain(&request));
        let after = world.terrain.authored.clone();
        assert_ne!(original, after);
        assert_ne!(after_first, after);
        let TerrainHistorySnapshot::Columns { values, .. } =
            &editor.sculpt_stroke_snapshot.as_ref().unwrap().6
        else {
            panic!("Sculpt brush should use local history");
        };
        // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:35 -05:00
        // Recomputed slope bands touch a two-column halo; record those
        // original voxels for complete Undo without copying all 256 columns.
        assert!(
            values.len() > 0 && values.len() < 128,
            "unexpected columns: {}",
            values.len()
        );
        editor.sculpt_stroke_mutated = true;
        editor.finish_sculpt_stroke();
        assert_eq!(editor.history.undo_stack.len(), 1);
        let undo = editor.history.undo_stack.pop().unwrap();
        let redo = undo.6.counterpart(&world.terrain);
        undo.6.apply(&mut world.terrain);
        assert_eq!(world.terrain.authored, original);
        redo.apply(&mut world.terrain);
        assert_eq!(world.terrain.authored, after);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:21 -05:00
    #[test]
    fn wo37_marooned_sized_grid_sculpt_localizes_history_and_preserves_distant_voxels() {
        use crate::world::terrain::{
            SmoothVoxel, SmoothVoxelGrid, TerrainData, TerrainGeneratorType, TerrainResult,
        };
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let mut grid = SmoothVoxelGrid::empty_with_spacing(
            [-24, 0, -30],
            [266, 48, 155],
            2.0,
            vec!["grass_1".into()],
        )
        .unwrap();
        let plane = (grid.size[0] * grid.size[2]) as usize;
        grid.voxels[0..plane * 12].fill(SmoothVoxel::from_unit_occupancy(0, 1.0));
        // Distant noncolumnar geometry must not block an unrelated brush.
        assert!(grid.set_voxel(0, 30, 0, SmoothVoxel::from_unit_occupancy(0, 1.0)));
        assert!(!grid.is_columnar_surface());
        world.terrain.replace_authored(Some(TerrainResult {
            request: crate::world::TerrainGenerationRequest {
                position: [-24, 0, -30],
                size: [532, 96, 310],
            },
            generator: TerrainGeneratorType::Programmatic,
            generator_implementation_version: 1,
            data: TerrainData::Voxel(grid),
        }));
        let before = world.terrain.authored.as_ref().unwrap().clone();
        editor.sculpt_stroke_snapshot = Some(editor.create_brush_snapshot(&world));
        editor.sculpt_stroke_active = true;
        let request = crate::world::terrain::TerrainSculptRequest {
            tool: crate::world::terrain::SculptTool::Raise,
            center: [120.0, 24.0, 100.0],
            radius: 4.0,
            strength: 2.0,
            flatten_target: None,
        };
        editor.capture_brush_columns(&world, request.center, request.radius);
        let start = std::time::Instant::now();
        assert!(world.sculpt_authored_terrain(&request));
        let elapsed = start.elapsed();
        eprintln!("WO37 large grid (1,979,040 samples) single local brush: {elapsed:?}");
        // A large terrain edit should touch only nearby columns, not all 41k.
        let TerrainHistorySnapshot::Columns { values, .. } =
            &editor.sculpt_stroke_snapshot.as_ref().unwrap().6
        else {
            panic!("Expected local patch for large brush");
        };
        // Worldkiln 0.8.5 | WO MAT-054 | 2026-10-10 17:37 -05:00
        // The local slope-material halo adds adjacent columns, still far
        // below 128 of the world's 41,230 columns (not a full snapshot).
        assert!(!values.is_empty() && values.len() < 128);
        if let (TerrainData::Voxel(current), TerrainData::Voxel(original)) =
            (&world.terrain.authored.as_ref().unwrap().data, &before.data)
        {
            assert_eq!(current.voxel(0, 4, 0), original.voxel(0, 4, 0));
        } else {
            panic!("Expected Voxel Terrain");
        }
        editor.sculpt_stroke_mutated = true;
        editor.finish_sculpt_stroke();
        let undo = editor.history.undo_stack.pop().unwrap();
        let redo = undo.6.counterpart(&world.terrain);
        undo.6.apply(&mut world.terrain);
        assert_eq!(world.terrain.authored, Some(before));
        redo.apply(&mut world.terrain);
    }

    // Worldkiln 0.8.7 | WO 0.8.7-37 | 2026-10-08 19:30 -05:00
    #[test]
    fn wo37_paint_stroke_undo_restores_materials_palette_and_voxel_values() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        world
            .generate_authored_terrain(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [32, 32, 32],
            })
            .unwrap();
        let original = world.terrain.authored.clone();
        let paint = crate::world::terrain::TerrainPaintRequest {
            center: [16.0, 21.0, 16.0],
            radius: 5.0,
            material: "wo37_test_paint_palette".to_string(),
        };
        editor.sculpt_stroke_snapshot = Some(editor.create_brush_snapshot(&world));
        editor.sculpt_stroke_active = true;
        editor.capture_brush_columns(&world, paint.center, paint.radius);
        assert!(world.paint_authored_terrain(&paint));
        let painted = world.terrain.authored.clone();
        assert_ne!(original, painted);
        editor.sculpt_stroke_mutated = true;
        editor.finish_sculpt_stroke();
        let undo = editor.history.undo_stack.pop().unwrap();
        let redo = undo.6.counterpart(&world.terrain);
        undo.6.apply(&mut world.terrain);
        assert_eq!(
            world.terrain.authored, original,
            "Undo must restore entire palette"
        );
        redo.apply(&mut world.terrain);
        assert_eq!(
            world.terrain.authored, painted,
            "Redo must restore paint palette"
        );
    }
}
