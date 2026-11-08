// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{Editor, EditorTool, LeftPanelTab, RightPanelTab};
use crate::engine::audio::EngineSound;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Workspace {
    #[default]
    #[serde(alias = "Model")]
    Home,
    Terrain,
    Navigation,
    Ui,
    Script,
}
impl Workspace {
    pub const ALL: [Self; 5] = [
        Self::Home,
        Self::Terrain,
        Self::Navigation,
        Self::Ui,
        Self::Script,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Home => "Home",
            Self::Terrain => "Terrain",
            Self::Navigation => "Navigation",
            Self::Ui => "UI",
            Self::Script => "Script",
        }
    }
}

#[derive(Clone, Copy, Default, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum DockTab {
    #[default]
    Content,
    Output,
    Problems,
    Diagnostics,
}
impl DockTab {
    pub const ALL: [Self; 4] = [
        Self::Content,
        Self::Output,
        Self::Problems,
        Self::Diagnostics,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::Content => "Content Browser",
            Self::Output => "Output",
            Self::Problems => "Problems",
            Self::Diagnostics => "Diagnostics",
        }
    }
}
fn shell_path() -> PathBuf {
    crate::project::project::default_user_data_dir().join("editor_shell_layout.json")
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct ShellState {
    pub workspace: Workspace,
    pub dock: DockTab,
    pub explorer_visible: bool,
    pub properties_visible: bool,
    pub dock_expanded: bool,
    #[serde(skip)]
    pub play_tray_hidden: bool,
    #[serde(default)]
    pub layout_revision: u8,
    pub explorer_width: f32,
    pub properties_width: f32,
    pub dock_height: f32,
    pub diagnostics: super::diagnostics_workspace::Layout,
    #[serde(default)]
    pub content_browser_height_initialized: bool,
    #[serde(skip)]
    pub auto_size_explorer: bool,
    #[serde(skip)]
    pub auto_size_properties: bool,
    #[serde(skip)]
    pub initialized: bool,
    #[serde(skip)]
    pub project_snapshot: Option<PathBuf>,
    #[serde(skip)]
    pub content_entries: Vec<(PathBuf, bool)>,
    #[serde(skip)]
    pub actor_packages: Vec<String>,
    #[serde(skip)]
    pub actor_display_names: std::collections::HashMap<String, String>,
    #[serde(skip)]
    pub(crate) actor_groups_static: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(skip)]
    pub(crate) actor_groups_characters: std::collections::BTreeMap<String, Vec<String>>,
    #[serde(skip)]
    pub(crate) actor_groups_valid: bool,
    #[serde(skip)]
    pub(crate) browser_items_cache: Option<super::asset_browser::BrowserItemCache>,
    #[serde(skip)]
    pub browser_directory: Option<PathBuf>,
    #[serde(skip)]
    pub browser_search: String,
    #[serde(skip)]
    pub dock_resize_dirty: bool,
    #[serde(skip)]
    pub explorer_search_visible: bool,
    #[serde(skip)]
    pub dock_initialized: bool,
    #[serde(skip)]
    pub dock_requested_last: Option<DockTab>,
    #[serde(default = "default_asset_folder")]
    pub asset_folder: String,
    #[serde(default)]
    pub actor_browser_kind: String,
    #[serde(default)]
    pub actor_browser_family: String,
    #[serde(default = "default_content_tree_width")]
    pub content_tree_width: f32,
    #[serde(default)]
    pub expanded_content_folders: Vec<String>,
    #[serde(skip)]
    pub content_subdirs: Vec<PathBuf>,
    #[serde(skip)]
    pub(crate) content_child_dirs: std::collections::HashSet<PathBuf>,
    #[serde(skip)]
    pub character_packages: Vec<String>,
}
fn default_asset_folder() -> String {
    "Content".into()
}
fn default_content_tree_width() -> f32 {
    220.0
}
impl Default for ShellState {
    fn default() -> Self {
        Self {
            workspace: Workspace::Home,
            dock: DockTab::Content,
            explorer_visible: true,
            properties_visible: true,
            dock_expanded: false,
            play_tray_hidden: false,
            layout_revision: 3,
            explorer_width: 300.0,
            properties_width: 325.0,
            dock_height: 176.0,
            diagnostics: Default::default(),
            content_browser_height_initialized: false,
            auto_size_explorer: true,
            auto_size_properties: true,
            initialized: false,
            project_snapshot: None,
            content_entries: Vec::new(),
            actor_packages: Vec::new(),
            actor_display_names: Default::default(),
            actor_groups_static: Default::default(),
            actor_groups_characters: Default::default(),
            actor_groups_valid: false,
            browser_items_cache: None,
            browser_directory: None,
            browser_search: String::new(),
            dock_resize_dirty: false,
            explorer_search_visible: false,
            dock_initialized: false,
            dock_requested_last: None,
            asset_folder: "Content".into(),
            actor_browser_kind: String::new(),
            actor_browser_family: String::new(),
            content_tree_width: default_content_tree_width(),
            expanded_content_folders: Vec::new(),
            content_subdirs: Vec::new(),
            content_child_dirs: Default::default(),
            character_packages: Vec::new(),
        }
    }
}
impl ShellState {
    fn restored_asset_folder(value: &str) -> String {
        match value {
            "Models" | "Actors" => "Actors".to_string(),
            "Content" | "Characters" | "Scripts" | "Player Scripts" | "Textures" | "Media"
            | "Audio" | "UI" | "Resources" => value.to_string(),
            _ => default_asset_folder(),
        }
    }
    pub fn load() -> Self {
        if cfg!(test) {
            return Self::default();
        }
        let mut result = std::fs::read(shell_path())
            .ok()
            .and_then(|bytes| serde_json::from_slice::<Self>(&bytes).ok())
            .unwrap_or_default();
        result.diagnostics.normalize();
        if result.layout_revision < 2 {
            result.dock = DockTab::Content;
            result.dock_expanded = false;
        }
        if result.layout_revision < 3 {
            result.auto_size_explorer = (result.explorer_width - 220.0).abs() < 4.0;
            result.auto_size_properties = (result.properties_width - 280.0).abs() < 4.0;
            result.layout_revision = 3;
        }
        Self {
            workspace: Workspace::Home,
            explorer_width: result.explorer_width.clamp(220.0, 500.0),
            properties_width: result.properties_width.clamp(240.0, 520.0),
            dock_height: result.dock_height.clamp(32.0, 550.0),
            content_tree_width: result.content_tree_width.clamp(140.0, 460.0),
            asset_folder: Self::restored_asset_folder(&result.asset_folder),
            initialized: false,
            ..result
        }
    }
    pub fn recommended_panel_widths(screen_width: f32) -> (f32, f32) {
        let usable = screen_width.max(850.0);
        (
            (usable * 0.226).clamp(240.0, 400.0),
            (usable * 0.226).clamp(260.0, 410.0),
        )
    }
    pub fn apply_responsive_panel_sizes(&mut self, screen_width: f32) {
        if !self.auto_size_explorer && !self.auto_size_properties {
            return;
        }
        let (left, right) = Self::recommended_panel_widths(screen_width);
        if self.auto_size_explorer {
            self.explorer_width = left;
            self.auto_size_explorer = false;
        }
        if self.auto_size_properties {
            self.properties_width = right;
            self.auto_size_properties = false;
        }
        self.save();
    }
    pub fn save(&self) {
        if cfg!(test) {
            return;
        }
        let path = shell_path();
        if let Some(parent) = path.parent() {
            if std::fs::create_dir_all(parent).is_err() {
                return;
            }
        }
        let Ok(bytes) = serde_json::to_vec_pretty(self) else {
            return;
        };
        let temp = path.with_extension("json.tmp");
        if std::fs::write(&temp, &bytes).is_ok() {
            if std::fs::rename(&temp, &path).is_err() {
                let _ = std::fs::remove_file(&path);
                let _ = std::fs::rename(&temp, &path);
            }
        }
    }
    pub fn folder_path(&self, root: &Path) -> PathBuf {
        super::asset_browser::physical_category_root(root, &self.asset_folder)
    }
    pub fn refresh(&mut self, root: Option<&Path>) {
        if self.project_snapshot.as_deref() != root {
            self.browser_directory = None;
        }
        self.project_snapshot = root.map(Path::to_path_buf);
        self.content_entries.clear();
        self.content_subdirs.clear();
        self.content_child_dirs.clear();
        self.actor_packages.clear();
        self.actor_display_names.clear();
        self.character_packages.clear();
        self.actor_groups_static.clear();
        self.actor_groups_characters.clear();
        self.actor_groups_valid = false;
        self.browser_items_cache = None;
        let Some(root) = root else {
            return;
        };
        if self.asset_folder == "Content" {
            return;
        }
        if self.asset_folder == "Actors" {
            self.browser_directory = None;
            self.actor_packages = crate::project::asset_registry::AssetRegistry::load(root)
                .map(|registry| {
                    registry
                        .by_type(crate::project::asset_registry::AssetType::Actor)
                        .filter(|entry| root.join(&entry.path).is_dir())
                        .map(|entry| entry.name.clone())
                        .collect()
                })
                .unwrap_or_default();
            for package in &self.actor_packages {
                let dir = root.join(".assets/entities").join(package);
                if dir.join("import.json").is_file() {
                    self.actor_display_names.insert(
                        package.clone(),
                        crate::editor::asset_management::model_display_name(&dir, None),
                    );
                }
            }
            self.character_packages = crate::project::discover_characters(root);
            super::asset_browser::refresh_actor_group_cache(self);
            return;
        }
        if self.asset_folder == "Characters" {
            self.browser_directory = None;
            self.character_packages = crate::project::discover_characters(root);
            return;
        }
        let category = self.folder_path(root);
        self.content_subdirs = super::asset_browser::scan_category_folders(&category);
        self.content_child_dirs = self
            .content_subdirs
            .iter()
            .filter_map(|path| path.parent().map(Path::to_path_buf))
            .collect();
        let base = category.canonicalize().ok();
        let folder = self
            .browser_directory
            .as_ref()
            .filter(|candidate| {
                let Some(base) = base.as_ref() else {
                    return false;
                };
                candidate
                    .canonicalize()
                    .ok()
                    .is_some_and(|real| real.starts_with(base))
            })
            .cloned()
            .unwrap_or(category);
        let Ok(read) = std::fs::read_dir(&folder) else {
            return;
        };
        let mut entries = read
            .flatten()
            .filter_map(|entry| {
                let ty = entry.file_type().ok()?;
                if ty.is_symlink() {
                    return None;
                }
                Some((entry.path(), ty.is_dir()))
            })
            .collect::<Vec<_>>();
        entries.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.file_name().cmp(&b.0.file_name()))
        });
        self.content_entries = entries;
    }
}

#[derive(Clone, Debug)]
pub(crate) struct ActorContentDragPayload {
    pub package: String,
    pub project_root: PathBuf,
}

pub(crate) fn resolve_actor_content_drag(
    payload: &ActorContentDragPayload,
    project_path: Option<&Path>,
    available: &[String],
) -> Option<String> {
    let project = project_path?;
    if payload.project_root.as_path() != project {
        return None;
    }
    let registry = super::asset_browser::project_asset_snapshot(project);
    let package = if let Some(asset) = registry.find(&payload.package) {
        if !matches!(
            asset.kind,
            crate::project::asset_registry::AssetType::Actor
                | crate::project::asset_registry::AssetType::Character
        ) {
            return None;
        }
        Path::new(&asset.path).file_name()?.to_str()?.to_owned()
    } else {
        payload.package.clone()
    };
    available
        .iter()
        .any(|name| name == &package)
        .then_some(package)
}

#[derive(Clone, Debug)]
pub(crate) struct FxContentDragPayload {
    pub asset_id: String,
    pub project_root: PathBuf,
}

#[derive(Clone, Debug)]
pub(crate) struct BuiltinFxContentDragPayload {
    pub preset: crate::engine::particles::EmitterPreset,
    pub project_root: PathBuf,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum WaterPlacementKind {
    Ocean,
    Lake,
    River,
}
impl WaterPlacementKind {
    fn body(self) -> Option<crate::world::terrain::WaterBodyKind> {
        match self {
            Self::Ocean => None,
            Self::Lake => Some(crate::world::terrain::WaterBodyKind::Lake),
            Self::River => Some(crate::world::terrain::WaterBodyKind::River),
        }
    }
}

#[derive(Clone, Debug)]
pub(crate) struct WaterContentDragPayload {
    pub kind: WaterPlacementKind,
    pub project_root: PathBuf,
}

pub(crate) fn actor_content_drop_position(
    editor: &Editor,
    world: &crate::world::World,
    pointer: egui::Pos2,
) -> Option<glam::Vec3> {
    let viewport = editor.viewport_rect;
    if !viewport.is_positive() || !viewport.contains(pointer) {
        return None;
    }
    let x = pointer.x - viewport.min.x;
    let y = pointer.y - viewport.min.y;
    let w = viewport.width();
    let h = viewport.height();
    let terrain =
        crate::editor::grid::picking::raycast_terrain_surface(x, y, w, h, &editor.camera, world);
    let cell = crate::editor::grid::picking::raycast_world_surface(
        x,
        y,
        w,
        h,
        &editor.camera,
        world,
        false,
    );
    let hit = match (terrain, cell) {
        (Some(terrain), Some(cell)) => Some(if terrain.distance <= cell.distance {
            terrain.point
        } else {
            cell.point
        }),
        (Some(terrain), None) => Some(terrain.point),
        (None, Some(cell)) => Some(cell.point),
        (None, None) => None,
    };
    if let Some(position) = hit {
        return position.is_finite().then_some(position);
    }
    let (origin, direction) = crate::editor::grid::picking::screen_ray(x, y, w, h, &editor.camera)?;
    let plane_height = editor.anchor.y as f32;
    let t = if direction.y.abs() > 0.001 {
        (plane_height - origin.y) / direction.y
    } else {
        -1.0
    };
    let distance = if t >= 0.0 && t < 200.0 { t } else { 12.0 };
    let position = origin + direction * distance;
    position.is_finite().then_some(position)
}

#[derive(Clone, Copy)]
enum ToolIcon {
    Select,
    Move,
    Rotate,
    Scale,
    Build,
    Erase,
}

fn theme_color_luminance(color: egui::Color32) -> f32 {
    let channel = |value: u8| {
        let value = f32::from(value) / 255.0;
        if value <= 0.04045 {
            value / 12.92
        } else {
            ((value + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * channel(color.r()) + 0.7152 * channel(color.g()) + 0.0722 * channel(color.b())
}

fn theme_color_contrast(left: egui::Color32, right: egui::Color32) -> f32 {
    let left = theme_color_luminance(left);
    let right = theme_color_luminance(right);
    (left.max(right) + 0.05) / (left.min(right) + 0.05)
}

pub(crate) fn readable_theme_color(
    color: egui::Color32,
    background: egui::Color32,
) -> egui::Color32 {
    if theme_color_contrast(color, background) >= 4.5 {
        return color;
    }
    let target = if theme_color_luminance(background) > 0.35 {
        egui::Color32::BLACK
    } else {
        egui::Color32::WHITE
    };
    for step in 1..=10 {
        let amount = step as f32 / 10.0;
        let blend = |value: u8, target: u8| {
            (f32::from(value) + (f32::from(target) - f32::from(value)) * amount).round() as u8
        };
        let candidate = egui::Color32::from_rgb(
            blend(color.r(), target.r()),
            blend(color.g(), target.g()),
            blend(color.b(), target.b()),
        );
        if theme_color_contrast(candidate, background) >= 4.5 {
            return candidate;
        }
    }
    target
}

fn active_shell_color(ui: &egui::Ui) -> egui::Color32 {
    readable_theme_color(ui.visuals().selection.bg_fill, ui.visuals().panel_fill)
}

fn shell_icon_tool(
    ui: &mut egui::Ui,
    icon: ToolIcon,
    label: &str,
    selected: bool,
) -> egui::Response {
    let body_height = ui.text_style_height(&egui::TextStyle::Body);
    let size = egui::vec2(
        (body_height * 3.65).clamp(51.0, 72.0),
        (body_height * 4.05).clamp(57.0, 79.0),
    );
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::click());
    let visual = ui.style().interact_selectable(&response, selected);
    if response.hovered() {
        ui.painter().rect_filled(
            rect.shrink(1.0),
            ui.visuals().widgets.hovered.rounding,
            ui.visuals().widgets.hovered.bg_fill,
        );
    }
    if selected {
        let y = rect.bottom() - 2.0;
        ui.painter().line_segment(
            [
                egui::pos2(rect.left() + 7.0, y),
                egui::pos2(rect.right() - 7.0, y),
            ],
            egui::Stroke::new(1.5, active_shell_color(ui)),
        );
    }
    let stroke = egui::Stroke::new(
        1.45,
        if !ui.is_enabled() {
            ui.visuals().weak_text_color()
        } else if selected {
            active_shell_color(ui)
        } else if response.hovered() {
            visual.fg_stroke.color
        } else {
            ui.visuals().text_color()
        },
    );
    let painter = ui.painter();
    let c = egui::pos2(rect.center().x, rect.top() + size.y * 0.35);
    let icon_scale = (size.y / 54.0).clamp(1.0, 1.55);
    let at = |x: f32, y: f32| egui::pos2(c.x + x * icon_scale, c.y + y * icon_scale);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    match icon {
        ToolIcon::Select => {
            for (a, b) in [
                ((-7.0, -9.0), (-4.0, 8.0)),
                ((-4.0, 8.0), (0.0, 2.0)),
                ((0.0, 2.0), (7.0, 4.0)),
                ((7.0, 4.0), (-7.0, -9.0)),
            ] {
                line(a, b);
            }
        }
        ToolIcon::Move => {
            line((0.0, 8.0), (0.0, -9.0));
            line((0.0, -9.0), (-3.0, -5.0));
            line((0.0, -9.0), (3.0, -5.0));
            line((0.0, 1.0), (-8.0, 7.0));
            line((-8.0, 7.0), (-6.0, 3.0));
            line((0.0, 1.0), (8.0, 7.0));
            line((8.0, 7.0), (4.0, 7.0));
        }
        ToolIcon::Rotate => {
            painter.circle_stroke(c, 7.0, stroke);
            line((5.0, -8.0), (8.0, -7.0));
            line((8.0, -7.0), (8.0, -3.0));
        }
        ToolIcon::Scale => {
            for (a, b) in [
                ((-7.0, -2.0), (-7.0, -8.0)),
                ((-7.0, -8.0), (-1.0, -8.0)),
                ((2.0, 8.0), (8.0, 8.0)),
                ((8.0, 8.0), (8.0, 2.0)),
                ((-2.0, 3.0), (7.0, -6.0)),
                ((7.0, -6.0), (2.0, -6.0)),
                ((7.0, -6.0), (7.0, -1.0)),
            ] {
                line(a, b);
            }
        }
        ToolIcon::Build => {
            for (a, b) in [
                ((0.0, -8.0), (8.0, -4.0)),
                ((8.0, -4.0), (0.0, 0.0)),
                ((0.0, 0.0), (-8.0, -4.0)),
                ((-8.0, -4.0), (0.0, -8.0)),
                ((-8.0, -4.0), (-8.0, 6.0)),
                ((-8.0, 6.0), (0.0, 10.0)),
                ((0.0, 10.0), (8.0, 6.0)),
                ((8.0, 6.0), (8.0, -4.0)),
                ((0.0, 0.0), (0.0, 10.0)),
            ] {
                line(a, b);
            }
        }
        ToolIcon::Erase => {
            for (a, b) in [
                ((-8.0, 3.0), (0.0, -8.0)),
                ((0.0, -8.0), (9.0, -1.0)),
                ((9.0, -1.0), (1.0, 9.0)),
                ((1.0, 9.0), (-8.0, 3.0)),
                ((-3.0, -4.0), (6.0, 4.0)),
            ] {
                line(a, b);
            }
        }
    }
    painter.text(
        egui::pos2(rect.center().x, rect.bottom() - body_height * 0.67),
        egui::Align2::CENTER_CENTER,
        label,
        egui::TextStyle::Body.resolve(ui.style()),
        if !ui.is_enabled() {
            ui.visuals().weak_text_color()
        } else if selected {
            active_shell_color(ui)
        } else {
            ui.visuals().text_color()
        },
    );
    response
}

impl Editor {
    pub(crate) fn activate_shell_workspace(&mut self, workspace: Workspace) {
        self.effect_visible = false;
        if self.shell.workspace != workspace {
            self.commit_pending_inspector_history();
        }
        self.shell.workspace = workspace;
        self.close_settings();
        match workspace {
            Workspace::Ui => {
                self.show_script_workspace = false;
                if !self.ui_mode {
                    self.enter_ui_mode();
                }
                self.shell.explorer_visible = true;
                self.shell.properties_visible = true;
                self.show_properties_window = true;
                self.right_panel_tab = RightPanelTab::Inspector;
            }
            Workspace::Script => {
                if self.ui_mode {
                    self.exit_ui_mode();
                }
                self.show_script_workspace = true;
                self.shell.explorer_visible = true;
                self.left_panel_tab = LeftPanelTab::Project;
                self.show_world_window = false;
            }
            Workspace::Navigation => {
                if self.ui_mode {
                    self.exit_ui_mode();
                }
                self.show_script_workspace = false;
                self.navigation_window.is_open = true;
                self.right_panel_tab = RightPanelTab::Navigation;
            }
            Workspace::Terrain => {
                if self.ui_mode {
                    self.exit_ui_mode();
                }
                self.show_script_workspace = false;
                self.show_landscaping_window = true;
                self.right_panel_tab = RightPanelTab::Landscaping;
                self.shell.properties_visible = true;
                self.shell.explorer_visible = true;
                self.left_panel_tab = LeftPanelTab::Terrain;
                self.show_world_window = false;
            }
            Workspace::Home => {
                if self.ui_mode {
                    self.exit_ui_mode();
                }
                self.show_script_workspace = false;
                self.shell.explorer_visible = true;
                self.left_panel_tab = LeftPanelTab::Project;
                self.show_world_window = false;
                self.show_properties_window = true;
                self.right_panel_tab = RightPanelTab::Inspector;
                self.shell.properties_visible = true;
            }
        }
        self.shell.save();
    }
    pub(crate) fn ensure_scene_home_workspace(&mut self) {
        if self.navigation.surface != super::feature_nav::EditorSurface::Workspace
            || self.shell.workspace != Workspace::Home
            || self.ui_mode
            || self.show_script_workspace
            || self.left_panel_tab != LeftPanelTab::Project
            || !self.shell.explorer_visible
            || !self.shell.properties_visible
        {
            self.activate_shell_workspace(Workspace::Home);
        }
    }

    pub fn activate_ribbon_shortcut(&mut self, digit: u8) -> bool {
        use super::transform_gizmo::TransformMode;
        match digit {
            1 => self.activate_transform_mode(TransformMode::Select),
            2 => self.activate_transform_mode(TransformMode::Move),
            3 => self.activate_transform_mode(TransformMode::Rotate),
            4 => self.activate_transform_mode(TransformMode::Scale),
            5 => self.activate_editor_tool(EditorTool::Build),
            6 => self.activate_editor_tool(EditorTool::Erase),
            _ => return false,
        }
        true
    }
    pub(crate) fn draw_workspace_tabs(&mut self, ctx: &egui::Context) {
        self.shell
            .apply_responsive_panel_sizes(ctx.screen_rect().width());
        if !self.shell.initialized {
            self.shell.initialized = true;
            if self.navigation.surface.is_settings() {
            } else if self.ui_mode {
                self.activate_shell_workspace(Workspace::Ui);
            } else if self.show_script_workspace {
                self.activate_shell_workspace(Workspace::Script);
            } else if !self.navigation.surface.is_settings() {
                let workspace = self.shell.workspace;
                self.activate_shell_workspace(workspace);
            }
        }
        egui::TopBottomPanel::top("worldkiln_workspace_tabs").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(8.0);
                ui.spacing_mut().item_spacing.x = 15.0;
                let tab_height = ui.text_style_height(&egui::TextStyle::Body) + 12.0;
                for page in Workspace::ALL {
                    let selected = self.navigation.surface
                        == super::feature_nav::EditorSurface::Workspace
                        && self.shell.workspace == page;
                    let color = if selected {
                        active_shell_color(ui)
                    } else {
                        ui.visuals().text_color()
                    };
                    let response = ui.add(
                        egui::Button::new(egui::RichText::new(page.label()).color(color))
                            .frame(false)
                            .min_size(egui::vec2(0.0, tab_height)),
                    );
                    if selected {
                        let rect = response.rect;
                        ui.painter().line_segment(
                            [
                                egui::pos2(rect.left(), rect.bottom() - 1.0),
                                egui::pos2(rect.right(), rect.bottom() - 1.0),
                            ],
                            egui::Stroke::new(1.3, active_shell_color(ui)),
                        );
                    }
                    if response.clicked() {
                        self.activate_shell_workspace(page);
                        self.queue_engine_sound(EngineSound::Select);
                    }
                }
                let selected =
                    self.navigation.surface == super::feature_nav::EditorSurface::ProjectSettings;
                let color = if selected {
                    active_shell_color(ui)
                } else {
                    ui.visuals().text_color()
                };
                let response = ui.add(
                    egui::Button::new(egui::RichText::new("Project Settings").color(color))
                        .frame(false)
                        .min_size(egui::vec2(0.0, tab_height)),
                );
                if selected {
                    let rect = response.rect;
                    ui.painter().line_segment(
                        [
                            egui::pos2(rect.left(), rect.bottom() - 1.0),
                            egui::pos2(rect.right(), rect.bottom() - 1.0),
                        ],
                        egui::Stroke::new(1.3, active_shell_color(ui)),
                    );
                }
                if response.clicked() {
                    self.navigate_to(super::feature_nav::Feature::ProjectSettings);
                    self.queue_engine_sound(EngineSound::Select);
                }
            });
            ui.add_space(4.0);
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            let r = ui.max_rect();
            ui.painter().line_segment(
                [
                    egui::pos2(r.left() + 20.0, r.bottom() - 1.0),
                    egui::pos2(r.right() - 20.0, r.bottom() - 1.0),
                ],
                stroke,
            );
        });
    }
    pub(crate) fn draw_shell_ribbon(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<PathBuf>,
    ) {
        egui::TopBottomPanel::top("worldkiln_contextual_ribbon").show(ctx, |ui| {
            ui.add_space(4.0);
            ui.horizontal(|ui| {
                ui.add_space(10.0);
                if self.ui_mode {
                    ui.label("UI");
                    if ui.small_button("Properties").clicked() {
                        self.shell.properties_visible = true;
                    }
                    if ui.small_button("Content").clicked() {
                        self.shell.dock = DockTab::Content;
                        self.shell.dock_expanded = true;
                    }
                } else if self.show_script_workspace {
                    ui.label("Script");
                    if ui.small_button("+ Script").clicked() {
                        self.script_editor.show_new_script_dialog = true;
                    }
                    if ui.small_button("Explorer").clicked() {
                        self.shell.explorer_visible = true;
                        self.left_panel_tab = LeftPanelTab::Project;
                    }
                } else {
                    use super::transform_gizmo::TransformMode;
                    let select = shell_icon_tool(
                        ui,
                        ToolIcon::Select,
                        "Select",
                        self.editor_tool_is_active(EditorTool::Select)
                            && self.transform_mode == TransformMode::Select,
                    );
                    if select.on_hover_text("1 · Select").clicked() {
                        self.activate_ribbon_shortcut(1);
                        self.queue_engine_sound(EngineSound::Select);
                    }
                    self.enforce_cell_transform_mode();
                    for (icon, label, mode, digit) in [
                        (ToolIcon::Move, "Move", TransformMode::Move, 2),
                        (ToolIcon::Rotate, "Rotate", TransformMode::Rotate, 3),
                        (ToolIcon::Scale, "Scale", TransformMode::Scale, 4),
                    ] {
                        let enabled = self.transform_mode_enabled(mode);
                        let response = ui
                            .add_enabled_ui(enabled, |ui| {
                                shell_icon_tool(
                                    ui,
                                    icon,
                                    label,
                                    self.editor_tool_is_active(EditorTool::Select)
                                        && self.transform_mode == mode,
                                )
                            })
                            .inner;
                        if response
                            .on_hover_text(format!(
                                "{digit} · {label} selected Actor in viewport. Drag X/Y/Z handles."
                            ))
                            .clicked()
                        {
                            self.activate_ribbon_shortcut(digit);
                            self.show_properties_window = true;
                            self.right_panel_tab = RightPanelTab::Inspector;
                            self.shell.properties_visible = true;
                            self.shell.save();
                            self.queue_engine_sound(EngineSound::Select);
                        }
                    }
                    for (tool, icon, label, digit) in [
                        (EditorTool::Build, ToolIcon::Build, "Build", 5),
                        (EditorTool::Erase, ToolIcon::Erase, "Erase", 6),
                    ] {
                        if shell_icon_tool(ui, icon, label, self.editor_tool_is_active(tool))
                            .on_hover_text(format!("{digit} · {label}"))
                            .clicked()
                        {
                            self.activate_ribbon_shortcut(digit);
                            self.queue_engine_sound(EngineSound::Select);
                        }
                    }
                }
                ui.add_space(8.0);
                ui.menu_button("...", |ui| {
                    ui.label("Additional tools");
                    ui.separator();
                    egui::ScrollArea::vertical()
                        .max_height(340.0)
                        .show(ui, |ui| {
                            ui.set_min_width(555.0);
                            crate::editor::tools::draw_tool_bar(ui, self, project_path);
                        });
                })
                .response
                .on_hover_text("More tools and editing options");
            });
            if !self.ui_mode
                && !self.show_script_workspace
                && self.current_tool == EditorTool::Build
            {
                ui.add_space(3.0);
                crate::editor::tools::draw_build_context_controls(ui, self, project_path);
                ui.add_space(3.0);
            }
            ui.add_space(4.0);
            let stroke = ui.visuals().widgets.noninteractive.bg_stroke;
            let rect = ui.max_rect();
            ui.painter().line_segment(
                [
                    egui::pos2(rect.left() + 20.0, rect.bottom() - 1.0),
                    egui::pos2(rect.right() - 20.0, rect.bottom() - 1.0),
                ],
                stroke,
            );
        });
        if std::mem::take(&mut self.navigation.pending_build_texture_picker) {
            self.shell.dock = DockTab::Content;
            self.shell.dock_expanded = true;
            self.shell.asset_folder = "Materials".to_string();
            self.shell.browser_directory = project_path
                .as_deref()
                .map(|root| super::asset_browser::physical_category_root(root, "Materials"));
            self.shell.browser_items_cache = None;
        }
        if let Some(texture) =
            crate::editor::tools::draw_build_asset_browsers(ctx, self, project_path)
        {
            self.apply_build_texture_selection(world, texture);
            ctx.request_repaint();
        }
    }
    pub(crate) fn draw_shell_dock(&mut self, ctx: &egui::Context, project_path: &Option<PathBuf>) {
        let playing = self.mode == crate::engine::EditorMode::Play;
        let requested = if self.script_editor.show_problems {
            Some(DockTab::Problems)
        } else if self.script_editor.show_output {
            Some(DockTab::Output)
        } else {
            None
        };
        if playing && self.shell.play_tray_hidden {
            self.shell.dock_initialized = true;
            self.shell.dock_requested_last = requested;
            return;
        }
        if !self.shell.dock_initialized {
            self.shell.dock_initialized = true;
            self.shell.dock_requested_last = requested;
            if (self.show_script_workspace || (playing && self.terminal_output.contains("[ERROR]")))
                && requested == Some(DockTab::Output)
            {
                self.shell.dock = DockTab::Output;
                self.shell.dock_expanded = true;
                self.shell.dock_height = self.script_output_panel_height;
            }
        } else if requested != self.shell.dock_requested_last {
            self.shell.dock_requested_last = requested;
            if let Some(requested) = requested {
                self.shell.dock = requested;
                self.shell.dock_expanded = true;
                self.shell.dock_height = if self.show_script_workspace {
                    self.script_output_panel_height
                } else {
                    self.output_panel_height
                };
            }
        }
        if self.shell.dock == DockTab::Content
            && self.shell.dock_expanded
            && !self.shell.content_browser_height_initialized
        {
            self.shell.content_browser_height_initialized = true;
            self.shell.dock_height = self.shell.dock_height.max(310.0);
            self.shell.save();
        }
        let expanded = self.shell.dock_expanded;
        let minimum = if self.show_script_workspace {
            86.0
        } else {
            110.0
        };
        let height = if expanded {
            self.shell.dock_height.max(minimum)
        } else {
            39.0
        };
        egui::TopBottomPanel::bottom("worldkiln_content_dock")
            .resizable(false)
            .exact_height(height)
            .show(ctx, |ui| {
                if expanded {
                    let (rect, response) = ui.allocate_exact_size(
                        egui::vec2(ui.available_width(), 7.0),
                        egui::Sense::drag(),
                    );
                    let response = response.on_hover_cursor(egui::CursorIcon::ResizeVertical);
                    ui.painter().line_segment(
                        [rect.left_center(), rect.right_center()],
                        ui.visuals().widgets.inactive.bg_stroke,
                    );
                    if response.dragged() {
                        let delta = ui.ctx().input(|i| i.pointer.delta().y);
                        self.shell.dock_height =
                            (self.shell.dock_height - delta).clamp(minimum, 550.0);
                        self.shell.dock_resize_dirty = true;
                        ui.ctx().request_repaint();
                    }
                }
                if !expanded {
                    ui.add_space(6.0);
                }
                ui.horizontal(|ui| {
                    let dock_tab_height = ui.text_style_height(&egui::TextStyle::Body) + 10.0;
                    for page in DockTab::ALL {
                        let label = match page {
                            DockTab::Content => "Content Browser".into(),
                            DockTab::Output => "Output".into(),
                            DockTab::Problems => {
                                let n = self.script_editor.active_problem_count();
                                if n > 0 {
                                    format!("Problems ({n})")
                                } else {
                                    "Problems".into()
                                }
                            }
                            DockTab::Diagnostics => "Diagnostics".into(),
                        };
                        if ui
                            .add(
                                egui::Button::new(label)
                                    .selected(self.shell.dock == page)
                                    .frame(false)
                                    .min_size(egui::vec2(0.0, dock_tab_height)),
                            )
                            .clicked()
                        {
                            self.shell.dock = page;
                            self.shell.dock_expanded = true;
                            self.script_editor.show_problems = page == DockTab::Problems;
                            self.script_editor.show_output = page == DockTab::Output;
                            self.shell.dock_requested_last = if page == DockTab::Content {
                                None
                            } else {
                                Some(page)
                            };
                            if page == DockTab::Output
                                || page == DockTab::Problems
                                || page == DockTab::Diagnostics
                            {
                                self.shell.dock_height = if self.show_script_workspace {
                                    self.script_output_panel_height
                                } else {
                                    self.output_panel_height
                                };
                            }
                            self.shell.save();
                        }
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui
                            .small_button(if self.shell.dock_expanded {
                                "Hide"
                            } else {
                                "Show"
                            })
                            .on_hover_text(if self.shell.dock_expanded {
                                "Collapse bottom dock"
                            } else {
                                "Expand bottom dock"
                            })
                            .clicked()
                        {
                            self.shell.dock_expanded = !self.shell.dock_expanded;
                            self.shell.save();
                        }
                    });
                });
                if !expanded {
                    return;
                }
                ui.separator();
                match self.shell.dock {
                    DockTab::Output => self.render_terminal_content(ui),
                    DockTab::Problems => self.script_editor.draw_problems(ui),
                    DockTab::Diagnostics => self.draw_shell_diagnostics(ui),
                    DockTab::Content => self.draw_shell_content(ui, project_path),
                }
            });
        if expanded {
            if self.show_script_workspace {
                self.script_output_panel_height = self.shell.dock_height;
            } else {
                self.output_panel_height = self.shell.dock_height;
            }
            if ctx.input(|i| i.pointer.any_released()) && self.shell.dock_resize_dirty {
                self.shell.dock_resize_dirty = false;
                self.shell.save();
            }
        }
    }
    fn draw_diagnostic_nodes(
        ui: &mut egui::Ui,
        grid_id: &'static str,
        heading: &'static str,
        nodes: &[crate::engine::development_test_run::DiagnosticNode],
    ) {
        ui.label(egui::RichText::new(heading).strong());
        if nodes.is_empty() {
            ui.weak("No recorded activity yet");
            return;
        }
        let largest = nodes
            .iter()
            .map(|n| n.total.as_secs_f64())
            .fold(0.000001_f64, f64::max);
        egui::Grid::new(grid_id)
            .striped(true)
            .num_columns(4)
            .spacing([12.0, 3.0])
            .show(ui, |ui| {
                ui.strong("Source / operation");
                ui.strong("Calls");
                ui.strong("Peak");
                ui.strong("Total CPU");
                ui.end_row();
                for node in nodes {
                    ui.label(&node.label).on_hover_text(&node.label);
                    ui.monospace(node.calls.to_string());
                    ui.monospace(format!("{:.2} ms", node.max.as_secs_f64() * 1000.0));
                    ui.add(
                        egui::ProgressBar::new(
                            (node.total.as_secs_f64() / largest).clamp(0.0, 1.0) as f32,
                        )
                        .desired_width(125.0)
                        .text(format!("{:.1} ms", node.total.as_secs_f64() * 1000.0)),
                    );
                    ui.end_row();
                }
            });
    }

    fn draw_shell_diagnostics(&mut self, ui: &mut egui::Ui) {
        self.draw_diagnostics_workspace(ui);
    }

    fn draw_shell_content(&mut self, ui: &mut egui::Ui, project_path: &Option<PathBuf>) {
        self.draw_unreal_asset_browser(ui, project_path);
    }

    fn place_validated_actor_package(
        &mut self,
        world: &mut crate::world::World,
        package: &str,
        position: glam::Vec3,
    ) -> Result<u64, String> {
        if package.trim().is_empty() || !position.is_finite() {
            return Err("Invalid Actor package or placement location".to_string());
        }
        let asset = crate::engine::entity_package::get_package(package);
        let mut position = position;
        position.y = crate::editor::actor_placement::surface_placement_y(
            position.y,
            asset
                .as_ref()
                .and_then(|asset| asset.triangle_mesh_bounds()),
            1.0,
            glam::Vec3::ZERO,
            0.0,
        );
        let previous = self.create_snapshot(world);
        let display_name = asset
            .as_ref()
            .map(|asset| asset.metadata.name.clone())
            .unwrap_or_else(|| package.to_owned());
        let actor_id = world.add_actor(display_name, position);
        if let Some(actor) = world.actors.get_mut(&actor_id) {
            actor.package = Some(package.to_owned());
            if let Some(pack) = asset.as_ref() {
                if pack.triangle_mesh_bounds().is_some() {
                    crate::engine::collision_geometry::set_actor_package_collision(actor, pack);
                }
            }
        }
        let node_id = match self.project_hierarchy.create_actor_node(None, actor_id) {
            Ok(id) => id,
            Err(error) => {
                world.remove_actor(actor_id);
                return Err(format!("Could not register dropped Actor: {error}"));
            }
        };
        self.history.push_undo_entry(previous);
        self.history.redo_stack.clear();
        self.project_tree_cache.dirty = true;
        self.project_actor_sync_revision = None;
        self.activate_transform_mode(super::transform_gizmo::TransformMode::Select);
        self.select_actor(actor_id);
        self.selected_project_node_id = Some(node_id);
        self.selected_project_nodes = vec![node_id];
        self.show_properties_window = true;
        self.shell.properties_visible = true;
        self.right_panel_tab = RightPanelTab::Inspector;
        self.needs_save = true;
        Ok(actor_id)
    }

    pub(crate) fn place_water_content(
        &mut self,
        world: &mut crate::world::World,
        kind: WaterPlacementKind,
        position: glam::Vec3,
    ) -> Result<(), String> {
        if !position.is_finite() {
            return Err("Water placement requires a finite position".into());
        }
        if let Some(kind) = kind.body() {
            let id = world
                .terrain
                .config
                .water_bodies
                .iter()
                .map(|b| b.id)
                .max()
                .unwrap_or(0)
                .checked_add(1)
                .ok_or("Water body IDs exhausted")?;
            let mut body = crate::world::terrain::WaterBody::new(
                id,
                kind,
                [position.x, position.z],
                position.y,
            );
            body.shore_material_layer = world
                .terrain
                .config
                .resolved_layer_id("subsurface")
                .to_owned();
            body.validate().map_err(str::to_owned)?;
            self.push_water_undo(super::water_spline::WaterEditSnapshot::capture(
                &world.terrain.config,
                self.water_selected_body,
            ));
            world.terrain.config.water_bodies.push(body);
            self.water_select(id);
        } else {
            if world.terrain.config.ocean_enabled {
                return Ok(());
            }
            self.push_water_undo(super::water_spline::WaterEditSnapshot::capture(
                &world.terrain.config,
                self.water_selected_body,
            ));
            world.terrain.config.ocean_enabled = true;
            self.water_selected_body = None;
        }
        world.terrain.mark_full_render_dirty();
        self.needs_save = true;
        Ok(())
    }

    pub(crate) fn handle_water_content_drop(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<PathBuf>,
    ) {
        if self.mode != crate::engine::EditorMode::Editor
            || self.show_script_workspace
            || self.ui_mode
            || self.navigation.surface.is_settings()
            || self.asset_manager.open
        {
            return;
        }
        let Some(payload) = egui::DragAndDrop::payload::<WaterContentDragPayload>(ctx) else {
            return;
        };
        if project_path.as_deref() != Some(payload.project_root.as_path()) {
            return;
        }
        let Some(pointer) = ctx.input(|i| i.pointer.latest_pos()) else {
            return;
        };
        let Some(position) = actor_content_drop_position(self, world, pointer) else {
            return;
        };
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let Some(payload) = egui::DragAndDrop::take_payload::<WaterContentDragPayload>(ctx) else {
            return;
        };
        if let Err(error) = self.place_water_content(world, payload.kind, position) {
            self.project_explorer_status = Some((error, ctx.input(|i| i.time) + 6.0));
        }
    }

    pub(crate) fn handle_fx_content_drop(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<PathBuf>,
    ) {
        if self.mode != crate::engine::EditorMode::Editor
            || self.show_script_workspace
            || self.ui_mode
            || self.navigation.surface.is_settings()
        {
            return;
        }
        let builtin = egui::DragAndDrop::payload::<BuiltinFxContentDragPayload>(ctx);
        let project_fx = egui::DragAndDrop::payload::<FxContentDragPayload>(ctx);
        let project_root = if let Some(payload) = &builtin {
            payload.project_root.as_path()
        } else if let Some(payload) = &project_fx {
            payload.project_root.as_path()
        } else {
            return;
        };
        if project_path.as_deref() != Some(project_root) {
            return;
        }
        let Some(pointer) = ctx
            .input(|i| i.pointer.latest_pos())
            .or_else(|| ctx.pointer_interact_pos())
        else {
            return;
        };
        let Some(position) = actor_content_drop_position(self, world, pointer) else {
            return;
        };
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let (preset, effect_asset) = if builtin.is_some() {
            let Some(payload) = egui::DragAndDrop::take_payload::<BuiltinFxContentDragPayload>(ctx)
            else {
                return;
            };
            (payload.preset, None)
        } else {
            let Some(payload) = egui::DragAndDrop::take_payload::<FxContentDragPayload>(ctx) else {
                return;
            };
            if crate::project::fx_assets::resolve(payload.project_root.as_path(), &payload.asset_id)
                .is_none()
            {
                self.project_explorer_status = Some((
                    "FX asset unavailable; refresh Effects".into(),
                    ctx.input(|i| i.time) + 6.0,
                ));
                return;
            }
            (
                crate::engine::particles::EmitterPreset::Fire,
                Some(payload.asset_id.clone()),
            )
        };
        let previous = self.create_snapshot(world);
        let id = world.add_source(crate::world::CellType::ParticleEmitter, position);
        if let Some(source) = world.source_mut(id) {
            source.cell.particles.preset = preset;
            source.cell.particles.effect_asset = effect_asset;
        }
        self.history.push_undo_entry(previous);
        self.needs_save = true;
        self.select_source(id);
        self.queue_engine_sound(EngineSound::Select);
    }

    pub(crate) fn handle_actor_content_drop(
        &mut self,
        ctx: &egui::Context,
        world: &mut crate::world::World,
        project_path: &Option<PathBuf>,
    ) {
        if !egui::DragAndDrop::has_payload_of_type::<ActorContentDragPayload>(ctx)
            || self.mode != crate::engine::EditorMode::Editor
            || self.show_script_workspace
            || self.ui_mode
            || self.navigation.surface.is_settings()
        {
            return;
        }
        let Some(pointer) = ctx
            .input(|i| i.pointer.latest_pos())
            .or_else(|| ctx.pointer_interact_pos())
        else {
            return;
        };
        let Some(position) = actor_content_drop_position(self, world, pointer) else {
            return;
        };
        let Some(payload) = egui::DragAndDrop::payload::<ActorContentDragPayload>(ctx) else {
            return;
        };
        if project_path.as_deref() != Some(payload.project_root.as_path()) {
            return;
        }
        let asset = crate::engine::entity_package::get_package(&payload.package);
        let mut display_position = position;
        display_position.y = crate::editor::actor_placement::surface_placement_y(
            position.y,
            asset
                .as_ref()
                .and_then(|asset| asset.triangle_mesh_bounds()),
            1.0,
            glam::Vec3::ZERO,
            0.0,
        );
        let painter = ctx
            .layer_painter(egui::LayerId::new(
                egui::Order::Foreground,
                egui::Id::new("worldkiln_actor_content_ghost"),
            ))
            .with_clip_rect(self.viewport_rect);
        let color = ctx.style().visuals.selection.stroke.color;
        let stroke = egui::Stroke::new(1.6, color);
        painter.circle_stroke(pointer, 10.0, stroke);
        painter.line_segment(
            [
                pointer + egui::vec2(-16.0, 0.0),
                pointer + egui::vec2(16.0, 0.0),
            ],
            stroke,
        );
        painter.line_segment(
            [
                pointer + egui::vec2(0.0, -16.0),
                pointer + egui::vec2(0.0, 16.0),
            ],
            stroke,
        );
        painter.text(
            pointer + egui::vec2(15.0, 15.0),
            egui::Align2::LEFT_TOP,
            format!(
                "Drop {} at {:.1}, {:.1}, {:.1}",
                payload.package, display_position.x, display_position.y, display_position.z
            ),
            egui::TextStyle::Body.resolve(&ctx.style()),
            color,
        );
        if !ctx.input(|i| i.pointer.any_released()) {
            return;
        }
        let Some(payload) = egui::DragAndDrop::take_payload::<ActorContentDragPayload>(ctx) else {
            return;
        };
        let available = crate::project::available_actor_package_names();
        let Some(package) =
            resolve_actor_content_drag(&payload, project_path.as_deref(), &available)
        else {
            self.project_explorer_status = Some((
                "Actor package was unloaded; refresh the Actors browser".to_string(),
                ctx.input(|i| i.time) + 6.0,
            ));
            return;
        };
        if let Err(error) = self.place_validated_actor_package(world, &package, position) {
            self.project_explorer_status = Some((error, ctx.input(|i| i.time) + 6.0));
        } else {
            self.queue_engine_sound(EngineSound::Select);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspector_brush_and_viewport_share_registered_actor_drag_resolution() {
        use crate::project::asset_registry::{AssetRecord, AssetRegistry, AssetType};
        let root = std::env::temp_dir().join(format!(
            "wk_actor_drop_identity_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos(),
        ));
        std::fs::create_dir_all(root.join(".assets")).unwrap();
        let registry = AssetRegistry {
            version: 1,
            revision: 1,
            assets: vec![
                AssetRecord {
                    id: "actor-asset-id".into(),
                    name: "Tree".into(),
                    kind: AssetType::Actor,
                    path: ".assets/entities/tree".into(),
                    dependencies: vec![],
                    missing_dependencies: vec![],
                    group: None,
                },
                AssetRecord {
                    id: "character-asset-id".into(),
                    name: "Player".into(),
                    kind: AssetType::Character,
                    path: ".assets/characters/player".into(),
                    dependencies: vec![],
                    missing_dependencies: vec![],
                    group: None,
                },
            ],
        };
        std::fs::write(
            root.join(".assets/asset_registry.json"),
            serde_json::to_vec(&registry).unwrap(),
        )
        .unwrap();
        let available = vec!["tree".into(), "player".into()];
        let drag = ActorContentDragPayload {
            project_root: root.clone(),
            package: "actor-asset-id".into(),
        };
        assert_eq!(
            resolve_actor_content_drag(&drag, Some(&root), &available),
            Some("tree".into()),
            "Canonical ActorID must resolve to the native Actor package",
        );
        let legacy = ActorContentDragPayload {
            package: "tree".into(),
            ..drag.clone()
        };
        assert_eq!(
            resolve_actor_content_drag(&legacy, Some(&root), &available),
            Some("tree".into()),
            "Existing package-name drags must continue to work",
        );
        let invalid_type = ActorContentDragPayload {
            package: "character-asset-id".into(),
            ..drag.clone()
        };
        assert_eq!(
            resolve_actor_content_drag(&invalid_type, Some(&root), &available),
            Some("player".into()),
            "A Character AssetID should resolve through the Character-backed Actor placement path",
        );
        let foreign = ActorContentDragPayload {
            project_root: root.join("another-project"),
            ..drag.clone()
        };
        assert!(resolve_actor_content_drag(&foreign, Some(&root), &available).is_none());
        assert!(resolve_actor_content_drag(&drag, None, &available).is_none());
        assert!(resolve_actor_content_drag(&drag, Some(&root), &[]).is_none());
        let missing = ActorContentDragPayload {
            package: "missing-id".into(),
            ..drag
        };
        assert!(resolve_actor_content_drag(&missing, Some(&root), &available).is_none());
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn models_category_migrates_to_registered_actor_browser() {
        assert_eq!(ShellState::restored_asset_folder("Models"), "Actors");
        assert_eq!(ShellState::restored_asset_folder("Actors"), "Actors");
        assert_eq!(ShellState::restored_asset_folder("Textures"), "Textures");
        assert_eq!(ShellState::restored_asset_folder("unknown"), "Content");
        let root =
            std::env::temp_dir().join(format!("worldkiln_actor_scoping_{}", std::process::id()));
        let first = root.join("first");
        let second = root.join("second");
        let actor = first.join(".assets/entities/LocalBoat");
        std::fs::create_dir_all(&actor).unwrap();
        std::fs::create_dir_all(&second).unwrap();
        std::fs::write(actor.join("package.json"), "{}").unwrap();
        let mut shell = ShellState::default();
        shell.asset_folder = "Actors".into();
        shell.refresh(Some(&first));
        assert_eq!(shell.actor_packages, vec!["LocalBoat"]);
        shell.refresh(Some(&second));
        assert!(
            shell.actor_packages.is_empty(),
            "Old project Actors must not leak into new project"
        );
        assert!(shell.content_entries.is_empty());
        assert!(shell.browser_directory.is_none());
        let json = serde_json::to_string(&shell).unwrap();
        assert!(
            !json.contains("actor_packages"),
            "cached registry must not persist"
        );
    }
    #[test]
    fn actor_drop_resolves_real_voxel_surface_and_rejects_non_viewport() {
        use crate::world::{CellType, WorldCoord};
        let mut editor = Editor::new();
        editor.viewport_rect =
            egui::Rect::from_min_size(egui::pos2(140.0, 65.0), egui::vec2(800.0, 600.0));
        editor.camera.target = glam::Vec3::splat(0.5);
        let mut world = crate::world::World::new();
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        let p = editor.viewport_rect.center();
        let expected = crate::editor::grid::picking::raycast_world_surface(
            400.0,
            300.0,
            800.0,
            600.0,
            &editor.camera,
            &world,
            false,
        )
        .expect("center ray must hit voxel")
        .point;
        let position = actor_content_drop_position(&editor, &world, p).unwrap();
        assert!(position.distance(expected) < 0.001);
        assert!(actor_content_drop_position(&editor, &world, egui::pos2(20.0, 20.0)).is_none());
        let empty = crate::world::World::new();
        let fallback = actor_content_drop_position(&editor, &empty, p).unwrap();
        assert!(fallback.is_finite());
    }
    #[test]
    fn browser_actor_creation_registers_project_node_undo_and_selection() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let position = glam::Vec3::new(1.25, 2.0, -5.5);
        let actor_id = editor
            .place_validated_actor_package(&mut world, "TestPackage", position)
            .unwrap();
        let actor = &world.actors[&actor_id];
        assert_eq!(actor.position, position);
        assert_eq!(actor.package.as_deref(), Some("TestPackage"));
        assert_eq!(editor.selected_actor_id, Some(actor_id));
        assert!(editor.project_hierarchy.find_actor_node(actor_id).is_some());
        assert_eq!(editor.selected_project_nodes.len(), 1);
        assert!(editor.project_tree_cache.dirty);
        assert!(editor.needs_save);
        assert_eq!(editor.history.undo_stack.len(), 1);
        assert!(!editor.history.undo_stack[0].3.contains_key(&actor_id));
        assert!(
            !editor
                .place_validated_actor_package(&mut world, "", position)
                .is_ok()
        );
        assert!(
            !editor
                .place_validated_actor_package(&mut world, "TestPackage", glam::Vec3::NAN)
                .is_ok()
        );
        assert_eq!(world.actors.len(), 1);
        assert_eq!(editor.history.undo_stack.len(), 1);
    }
    #[test]
    fn browser_drop_outside_viewport_does_not_edit_world() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let root = PathBuf::from("some_project");
        editor.viewport_rect =
            egui::Rect::from_min_size(egui::pos2(200.0, 100.0), egui::vec2(900.0, 600.0));
        let outside = egui::pos2(10.0, 800.0);
        ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1300.0, 900.0),
                )),
                events: vec![
                    egui::Event::PointerMoved(outside),
                    egui::Event::PointerButton {
                        pos: outside,
                        button: egui::PointerButton::Primary,
                        pressed: false,
                        modifiers: Default::default(),
                    },
                ],
                ..Default::default()
            },
            |ctx| {
                egui::DragAndDrop::set_payload(
                    ctx,
                    ActorContentDragPayload {
                        package: "FakePackage".into(),
                        project_root: root.clone(),
                    },
                );
                editor.handle_actor_content_drop(ctx, &mut world, &Some(root.clone()));
            },
        );
        assert!(world.actors.is_empty());
        assert!(!editor.needs_save);
        assert!(editor.history.undo_stack.is_empty());
    }
    #[test]
    fn all_six_ribbon_keyboard_actions_are_functional() {
        use super::super::transform_gizmo::TransformMode;
        let mut editor = Editor::new();
        for (digit, mode) in [
            (1, TransformMode::Select),
            (2, TransformMode::Move),
            (3, TransformMode::Rotate),
            (4, TransformMode::Scale),
        ] {
            assert!(editor.activate_ribbon_shortcut(digit));
            assert_eq!(editor.current_tool, EditorTool::Select);
            assert_eq!(editor.transform_mode, mode);
        }
        assert!(editor.activate_ribbon_shortcut(5));
        assert_eq!(editor.current_tool, EditorTool::Build);
        assert!(editor.activate_ribbon_shortcut(6));
        assert_eq!(editor.current_tool, EditorTool::Erase);
        assert!(!editor.activate_ribbon_shortcut(0));
        assert!(!editor.activate_ribbon_shortcut(7));
        assert_eq!(editor.current_tool, EditorTool::Erase);
    }
    #[test]
    fn shell_defaults_are_theme_independent_and_restorable() {
        let state = ShellState::default();
        let json = serde_json::to_string(&state).unwrap();
        let restored: ShellState = serde_json::from_str(&json).unwrap();
        assert_eq!(restored.workspace, Workspace::Home);
        assert_eq!(restored.dock, DockTab::Content);
        assert_eq!(restored.explorer_width, 300.0);
        assert!(
            !restored.dock_expanded,
            "the approved Home shell starts with the dock collapsed"
        );
        assert_eq!(restored.layout_revision, 3);
        assert!(!json.contains("accent") && !json.contains("theme"));
    }
    #[test]
    fn approved_home_shell_renders_single_world_tree_and_compact_dock() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let renderer = crate::renderer::Renderer::new_for_tests(1440.0, 900.0);
        let ctx = egui::Context::default();
        let out = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| editor.show_ui(ctx, &mut world, &None, &renderer),
        );
        let labels = out
            .shapes
            .iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = &shape.shape {
                    Some(text.galley.job.text.clone())
                } else {
                    None
                }
            })
            .collect::<Vec<_>>();
        let has = |label: &str| labels.iter().any(|value| value == label);
        assert!(has("Explorer"), "missing Explorer panel: {labels:?}");
        assert!(has("Project"), "missing unified Project root: {labels:?}");
        assert!(
            has("Player Scripts"),
            "missing Player Scripts root: {labels:?}"
        );
        for obsolete in [
            "World",
            "Project Assets",
            "Actors",
            "Cells",
            "Lighting",
            "Other Assets",
        ] {
            assert!(
                !has(obsolete),
                "obsolete Explorer category {obsolete} still displayed: {labels:?}"
            );
        }
        assert!(has("Content Browser"), "missing bottom dock: {labels:?}");
        assert!(
            has("File") && has("Edit") && has("View"),
            "missing approved menu"
        );
        for navigation_label in ["Back", "Forward"] {
            assert_eq!(
                labels
                    .iter()
                    .filter(|label| label.as_str() == navigation_label)
                    .count(),
                1,
                "navigation button {navigation_label} must render exactly once: {labels:?}"
            );
        }
        assert!(!has("PROJECT EXPLORER"), "duplicate project tree header");
        assert!(!has("+ Folder"), "creation actions should be contextual");
        assert!(
            !editor.shell.dock_expanded,
            "Home must not begin with a large Output dock"
        );
    }
    #[test]
    fn responsive_sizing_matches_desktop_proportions_and_keeps_theme_external() {
        let (left, right) = ShellState::recommended_panel_widths(1660.0);
        assert!((left / 1660.0 - 0.226).abs() < 0.01);
        assert!((right / 1660.0 - 0.226).abs() < 0.01);
        assert!(1660.0 - left - right > 750.0, "viewport must remain usable");
        let mut shell = ShellState::default();
        shell.apply_responsive_panel_sizes(1660.0);
        assert_eq!(shell.explorer_width, left);
        assert_eq!(shell.properties_width, right);
        shell.explorer_width = 257.0;
        shell.apply_responsive_panel_sizes(1920.0);
        assert_eq!(shell.explorer_width, 257.0);
        let serialized = serde_json::to_string(&shell).unwrap();
        assert!(!serialized.contains("auto_size") && !serialized.contains("theme"));
    }
    #[test]
    fn narrow_widths_keep_panels_and_center_usable() {
        let (left, right) = ShellState::recommended_panel_widths(1000.0);
        assert!(left >= 240.0 && right >= 260.0);
        assert!(1000.0 - left - right >= 490.0);
    }
    #[test]
    fn pointer_clicks_open_project_settings_tab_then_home_viewport() {
        fn render(
            ctx: &egui::Context,
            editor: &mut Editor,
            world: &mut crate::world::World,
            renderer: &crate::renderer::Renderer,
            events: Vec<egui::Event>,
        ) -> Vec<(String, egui::Rect)> {
            let out = ctx.run(
                egui::RawInput {
                    screen_rect: Some(egui::Rect::from_min_size(
                        egui::Pos2::ZERO,
                        egui::vec2(1440.0, 900.0),
                    )),
                    events,
                    ..Default::default()
                },
                |ctx| editor.show_ui(ctx, world, &None, renderer),
            );
            out.shapes
                .into_iter()
                .filter_map(|shape| {
                    if let egui::Shape::Text(text) = shape.shape {
                        Some((text.galley.job.text.clone(), text.visual_bounding_rect()))
                    } else {
                        None
                    }
                })
                .collect()
        }
        let ctx = egui::Context::default();
        let renderer = crate::renderer::Renderer::new_for_tests(1440.0, 900.0);
        let mut world = crate::world::World::new();
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        let labels = render(&ctx, &mut editor, &mut world, &renderer, vec![]);
        let project_tab = labels
            .iter()
            .find(|(label, rect)| label == "Project Settings" && rect.top() < 125.0)
            .unwrap_or_else(|| panic!("Project Settings tab missing: {labels:?}"))
            .1
            .center();
        let click = |pos, pressed| egui::Event::PointerButton {
            pos,
            button: egui::PointerButton::Primary,
            pressed,
            modifiers: egui::Modifiers::default(),
        };
        render(
            &ctx,
            &mut editor,
            &mut world,
            &renderer,
            vec![
                egui::Event::PointerMoved(project_tab),
                click(project_tab, true),
            ],
        );
        render(
            &ctx,
            &mut editor,
            &mut world,
            &renderer,
            vec![click(project_tab, false)],
        );
        assert_eq!(
            editor.navigation.surface,
            super::super::feature_nav::EditorSurface::ProjectSettings
        );
        assert_eq!(
            editor.navigation.project_section,
            super::super::feature_nav::ProjectSection::SaveData
        );
        let labels = render(&ctx, &mut editor, &mut world, &renderer, vec![]);
        let home_tab = labels
            .iter()
            .find(|(label, rect)| label == "Home" && rect.top() < 125.0)
            .unwrap_or_else(|| panic!("Home tab missing on Project Settings: {labels:?}"))
            .1
            .center();
        render(
            &ctx,
            &mut editor,
            &mut world,
            &renderer,
            vec![egui::Event::PointerMoved(home_tab), click(home_tab, true)],
        );
        render(
            &ctx,
            &mut editor,
            &mut world,
            &renderer,
            vec![click(home_tab, false)],
        );
        assert_eq!(
            editor.navigation.surface,
            super::super::feature_nav::EditorSurface::Workspace
        );
        assert_eq!(editor.shell.workspace, Workspace::Home);
        assert!(editor.shell.explorer_visible && editor.shell.properties_visible);
        assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
    }

    #[test]
    fn project_settings_workspace_tab_is_after_script_and_switches_back_to_home() {
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        let labels = rendered_workspace_labels(&mut editor);
        let positions = |name: &str| {
            labels
                .iter()
                .find(|(label, rect, _)| label == name && rect.top() < 125.0)
                .map(|(_, rect, _)| rect.left())
                .unwrap_or_else(|| panic!("Missing top tab {name}: {labels:?}"))
        };
        assert!(
            positions("Project Settings") > positions("Script"),
            "Project Settings must appear immediately after Script"
        );
        assert!(positions("Script") > positions("UI"));
        editor.navigate_to(super::super::feature_nav::Feature::ProjectSettings);
        assert_eq!(
            editor.navigation.surface,
            super::super::feature_nav::EditorSurface::ProjectSettings
        );
        let open_labels = rendered_workspace_labels(&mut editor);
        assert!(open_labels.iter().any(|(label, _, _)| label == "SAVE DATA"));
        assert!(
            open_labels
                .iter()
                .any(|(label, _, _)| label == "Project Settings")
        );
        assert!(!open_labels.iter().any(|(label, _, _)| label == "OVERVIEW"));
        editor.activate_shell_workspace(Workspace::Home);
        assert_eq!(editor.shell.workspace, Workspace::Home);
        assert_eq!(
            editor.navigation.surface,
            super::super::feature_nav::EditorSurface::Workspace
        );
        assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
        assert!(editor.shell.explorer_visible && editor.shell.properties_visible);
    }

    #[test]
    fn terrain_workspace_opens_actual_landscaping_panel() {
        let mut editor = Editor::new();
        editor.shell.properties_visible = false;
        editor.left_panel_tab = LeftPanelTab::Project;
        editor.right_panel_tab = RightPanelTab::Inspector;
        editor.show_landscaping_window = false;
        editor.activate_shell_workspace(Workspace::Terrain);
        assert!(editor.show_landscaping_window);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Landscaping);
        assert_eq!(editor.left_panel_tab, LeftPanelTab::Terrain);
        assert!(editor.shell.explorer_visible);
        assert!(editor.shell.properties_visible);
        editor.activate_shell_workspace(Workspace::Home);
        assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
    }
    #[test]
    fn ui_workspace_restores_both_real_authoring_panels_and_retains_selection() {
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        editor.shell.explorer_visible = false;
        editor.shell.properties_visible = false;
        editor.left_panel_tab = LeftPanelTab::Terrain;
        editor.right_panel_tab = RightPanelTab::Ai;
        editor.ui_selected_node = Some(51);
        editor.activate_shell_workspace(Workspace::Ui);
        assert!(editor.ui_mode);
        assert!(editor.shell.explorer_visible);
        assert!(editor.shell.properties_visible);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
        assert!(editor.show_properties_window);
        assert_eq!(editor.ui_selected_node, Some(51));
        editor.shell.explorer_visible = false;
        assert!(!editor.shell.explorer_visible);
        editor.activate_shell_workspace(Workspace::Ui);
        assert!(editor.shell.explorer_visible);
    }
    #[test]
    fn script_workspace_restores_left_project_explorer_without_changing_selection() {
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        editor.shell.explorer_visible = false;
        editor.left_panel_tab = LeftPanelTab::Terrain;
        editor.show_world_window = true;
        editor.selected_project_node_id = Some(45);
        editor.activate_shell_workspace(Workspace::Script);
        assert!(editor.show_script_workspace);
        assert!(!editor.ui_mode);
        assert!(editor.shell.explorer_visible);
        assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
        assert!(!editor.show_world_window);
        assert_eq!(editor.selected_project_node_id, Some(45));
    }
    #[test]
    fn direct_ui_document_mode_restores_panels_without_clearing_document_selection() {
        let mut editor = Editor::new();
        editor.shell.explorer_visible = false;
        editor.shell.properties_visible = false;
        editor.ui_selected_node = Some(29);
        editor.enter_ui_mode();
        assert!(editor.shell.explorer_visible);
        assert!(editor.shell.properties_visible);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
        assert_eq!(editor.ui_selected_node, Some(29));
    }
    #[test]
    fn diagnostics_dock_renders_live_script_nodes_and_shared_recorder_controls() {
        use crate::engine::development_test_run::TestRunRecorder;
        let mut recorder = TestRunRecorder::default();
        assert!(recorder.start(false));
        recorder.record_script(
            "scripts/gameplay/enemies/ai_controller.aeo",
            std::time::Duration::from_millis(1050),
        );
        recorder.record("render", "frame", std::time::Duration::from_millis(1060));
        let mut editor = Editor::new();
        editor.development_test_run_ui.summary = Some(recorder.summary());
        editor.development_test_run_ui.has_project = true;
        editor.development_test_run_ui.can_export = true;
        let ctx = egui::Context::default();
        let result = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| {
                egui::CentralPanel::default().show(ctx, |ui| {
                    editor.draw_shell_diagnostics(ui);
                });
            },
        );
        let labels: Vec<String> = result
            .shapes
            .into_iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = shape.shape {
                    Some(text.galley.job.text.clone())
                } else {
                    None
                }
            })
            .collect();
        for expected in [
            "Overview",
            "Frame time / FPS",
            "CPU",
            "GPU",
            "Scripts",
            "Start",
            "Pause",
            "Stop",
            "Save",
            "Export",
            "Recent Events",
        ] {
            assert!(
                labels.iter().any(|s| s.contains(expected)),
                "{expected} missing in Diagnostics dock: {labels:?}"
            );
        }
        assert!(DockTab::ALL.contains(&DockTab::Diagnostics));
        let encoded = serde_json::to_string(&DockTab::Diagnostics).unwrap();
        assert_eq!(
            serde_json::from_str::<DockTab>(&encoded).unwrap(),
            DockTab::Diagnostics
        );
    }

    fn rendered_workspace_labels(editor: &mut Editor) -> Vec<(String, egui::Rect, egui::Rect)> {
        let mut world = crate::world::World::new();
        let renderer = crate::renderer::Renderer::new_for_tests(1440.0, 900.0);
        let ctx = egui::Context::default();
        let result = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| editor.show_ui(ctx, &mut world, &None, &renderer),
        );
        result
            .shapes
            .into_iter()
            .filter_map(|painted| {
                let egui::Shape::Text(text) = painted.shape else {
                    return None;
                };
                Some((
                    text.galley.job.text.clone(),
                    text.visual_bounding_rect(),
                    painted.clip_rect,
                ))
            })
            .collect()
    }
    #[test]
    fn ui_workspace_actually_renders_ui_nodes_and_properties_on_both_sides() {
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        editor.shell.explorer_visible = false;
        editor.shell.properties_visible = false;
        editor.activate_shell_workspace(Workspace::Ui);
        let labels = rendered_workspace_labels(&mut editor);
        let nodes = labels
            .iter()
            .find(|(s, _, _)| s == "UI NODES")
            .unwrap_or_else(|| panic!("UI authoring missing left hierarchy: {labels:?}"));
        let properties = labels
            .iter()
            .find(|(s, _, _)| s == "UI PROPERTIES")
            .unwrap_or_else(|| panic!("UI authoring missing right Inspector: {labels:?}"));
        assert!(nodes.1.left() < 500.0);
        assert!(
            nodes.2.right() < 750.0,
            "left hierarchy overlaps authoring canvas"
        );
        assert!(properties.1.left() > 875.0);
        assert!(
            properties.2.left() > 750.0,
            "right Inspector overlaps canvas"
        );
    }
    #[test]
    fn script_file_workspace_renders_left_project_explorer_with_actual_editor() {
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        editor.shell.explorer_visible = false;
        editor.show_script_workspace = true;
        editor.shell.workspace = Workspace::Home;
        let labels = rendered_workspace_labels(&mut editor);
        assert_eq!(editor.shell.workspace, Workspace::Script);
        assert!(editor.shell.explorer_visible);
        assert_eq!(editor.left_panel_tab, LeftPanelTab::Project);
        assert!(labels.iter().any(|(s, _, _)| s == "Explorer"));
        assert!(labels.iter().any(|(s, _, _)| s == "Project"));
        assert!(labels.iter().any(|(s, _, _)| s == "Player Scripts"));
        assert!(!labels.iter().any(|(s, _, _)| s == "Project Assets"));
    }
    #[test]
    fn terrain_workspace_renders_real_layers_on_left_and_landscaping_on_right() {
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        editor.shell.explorer_visible = false;
        editor.shell.properties_visible = false;
        editor.left_panel_tab = LeftPanelTab::Project;
        editor.activate_shell_workspace(Workspace::Terrain);
        let labels = rendered_workspace_labels(&mut editor);
        let layers = labels
            .iter()
            .find(|(s, _, _)| s == "Terrain Layers")
            .unwrap_or_else(|| panic!("left terrain layers missing: {labels:?}"));
        assert!(layers.1.left() < 500.0);
        assert!(layers.2.right() < 750.0);
        let settings = labels
            .iter()
            .find(|(s, _, _)| s == "Generate")
            .unwrap_or_else(|| panic!("left Generate controls missing: {labels:?}"));
        assert!(settings.2.right() < 750.0);
        let landscaping = labels
            .iter()
            .find(|(s, _, _)| s == "Landscaping")
            .expect("right Landscaping panel missing");
        assert!(landscaping.1.left() > 875.0);
        assert!(editor.shell.explorer_visible && editor.shell.properties_visible);
    }

    #[test]
    fn model_tab_removed_but_old_layout_settings_migrate_to_home() {
        assert_eq!(Workspace::ALL.len(), 5);
        assert!(
            !Workspace::ALL
                .iter()
                .any(|workspace| workspace.label() == "Model")
        );
        let old = serde_json::from_str::<Workspace>(r#""Model""#).unwrap();
        assert_eq!(old, Workspace::Home);
    }
    #[test]
    fn move_rotate_and_scale_activate_dedicated_viewport_modes() {
        use super::super::transform_gizmo::TransformMode;
        let mut editor = Editor::new();
        for mode in [
            TransformMode::Move,
            TransformMode::Rotate,
            TransformMode::Scale,
        ] {
            editor.activate_transform_mode(mode);
            assert_eq!(editor.current_tool, EditorTool::Select);
            assert_eq!(editor.transform_mode, mode);
        }
        editor.activate_editor_tool(EditorTool::Build);
        assert_eq!(editor.transform_mode, TransformMode::Select);
    }

    #[test]
    fn all_workspaces_and_docks_round_trip() {
        for page in Workspace::ALL {
            assert_eq!(
                serde_json::from_str::<Workspace>(&serde_json::to_string(&page).unwrap()).unwrap(),
                page
            );
        }
        for tab in DockTab::ALL {
            assert_eq!(
                serde_json::from_str::<DockTab>(&serde_json::to_string(&tab).unwrap()).unwrap(),
                tab
            );
        }
    }
}

#[cfg(test)]
mod water_material_placement_tests {
    use super::*;
    #[test]
    fn water_material_browser_placement_selects_and_supports_undo_redo() {
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let point = glam::Vec3::new(15.0, 4.0, 22.0);
        editor
            .place_water_content(&mut world, WaterPlacementKind::Lake, point)
            .unwrap();
        assert_eq!(world.terrain.config.water_bodies.len(), 1);
        assert_eq!(editor.water_selected_body, Some(1));
        assert_eq!(world.terrain.config.water_bodies[0].elevation, 4.0);
        assert!(editor.needs_save);
        assert!(editor.water_undo_redo(&mut world, false));
        assert!(world.terrain.config.water_bodies.is_empty());
        editor.water_selected_body = None;
        assert!(editor.water_undo_redo(&mut world, true));
        assert_eq!(world.terrain.config.water_bodies.len(), 1);
        assert!(
            editor
                .place_water_content(&mut world, WaterPlacementKind::River, glam::Vec3::NAN)
                .is_err()
        );
        editor
            .place_water_content(&mut world, WaterPlacementKind::River, point)
            .unwrap();
        assert_eq!(world.terrain.config.water_bodies[1].id, 2);
        let old_look = world.terrain.config.water;
        world.terrain.config.ocean_enabled = false;
        editor
            .place_water_content(&mut world, WaterPlacementKind::Ocean, point)
            .unwrap();
        assert!(world.terrain.config.ocean_enabled);
        assert_eq!(world.terrain.config.water, old_look);
        assert_eq!(world.terrain.config.water_bodies.len(), 2);
        assert!(editor.water_undo_redo(&mut world, false));
        assert!(!world.terrain.config.ocean_enabled);
        assert!(editor.water_undo_redo(&mut world, true));
        assert!(world.terrain.config.ocean_enabled);
        let commands = editor.water_undo.len();
        editor
            .place_water_content(&mut world, WaterPlacementKind::Ocean, point)
            .unwrap();
        assert_eq!(editor.water_undo.len(), commands);
    }
    #[test]
    fn water_material_content_drop_rejects_cross_project_payloads() {
        let ctx = egui::Context::default();
        let mut editor = Editor::new();
        let mut world = crate::world::World::new();
        let _ = ctx.run(egui::RawInput::default(), |ctx| {
            egui::DragAndDrop::set_payload(
                ctx,
                WaterContentDragPayload {
                    kind: WaterPlacementKind::Lake,
                    project_root: PathBuf::from("other"),
                },
            );
            editor.handle_water_content_drop(ctx, &mut world, &Some(PathBuf::from("current")));
        });
        assert!(world.terrain.config.water_bodies.is_empty());
        assert!(editor.water_undo.is_empty());
    }
}
