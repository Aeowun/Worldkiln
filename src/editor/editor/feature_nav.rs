// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::shell::{DockTab, Workspace};
use super::transform_gizmo::TransformMode;
use super::{Editor, EditorTool, LeftPanelTab, RightPanelTab, SettingsPage};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum EditorSurface {
    #[default]
    Workspace,
    AppSettings,
    ProjectSettings,
}
impl EditorSurface {
    pub const fn is_settings(self) -> bool {
        !matches!(self, Self::Workspace)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ProjectSection {
    #[default]
    SaveData,
    Streaming,
    Mouse,
    Character,
    Controller,
    Camera,
    Lighting,
    Physics,
    PostProcessing,
}
impl ProjectSection {
    pub const ALL: [Self; 8] = [
        Self::SaveData,
        Self::Streaming,
        Self::Mouse,
        Self::Character,
        Self::Controller,
        Self::Camera,
        Self::Lighting,
        Self::Physics,
    ];
    pub const fn label(self) -> &'static str {
        match self {
            Self::SaveData => "SAVE DATA",
            Self::Streaming => "STREAMING",
            Self::Mouse => "MOUSE",
            Self::Character => "CHARACTER",
            Self::Controller => "CONTROLLER",
            Self::Camera => "CAMERA",
            Self::Lighting => "WORLD LIGHTING",
            Self::Physics => "WORLD PHYSICS",
            Self::PostProcessing => "POST PROCESSING",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct NavigationSnapshot {
    surface: EditorSurface,
    workspace: Workspace,
    settings_page: SettingsPage,
    project_section: ProjectSection,
    asset_manager: bool,
    character_editor: bool,
    material_editor: bool,
    sky_editor: bool,
    material_category: String,
    material_folder: String,
    explorer: bool,
    properties: bool,
    dock_expanded: bool,
    dock: DockTab,
    content_category: String,
    content_kind: String,
    content_family: String,
    content_directory: Option<std::path::PathBuf>,
}
#[derive(Debug, Default)]
pub struct NavigationState {
    pub surface: EditorSurface,
    pub project_section: ProjectSection,
    pub feature_search: String,
    pub pending_build_texture_picker: bool,
    pub(crate) previous: Vec<NavigationSnapshot>,
    pub(crate) next: Vec<NavigationSnapshot>,
    pub(crate) last_seen: Option<NavigationSnapshot>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Feature {
    Home,
    Terrain,
    Navigation,
    UiAuthoring,
    ScriptEditor,
    ScriptSearch,
    ScriptDebugger,
    ProjectSettings,
    SaveData,
    Streaming,
    Mouse,
    Character,
    Controller,
    Camera,
    WorldLighting,
    WorldPhysics,
    ContentBrowser,
    Actors,
    Scripts,
    PlayerScripts,
    Textures,
    Media,
    AudioAssets,
    UiAssets,
    Resources,
    AssetManager,
    Explorer,
    Properties,
    Landscaping,
    Output,
    Problems,
    AiAssistant,
    EditorPreferences,
    Theme,
    RenderLighting,
    AudioPreferences,
    InputPreferences,
    AiPreferences,
    Select,
    Move,
    Rotate,
    Scale,
    Build,
    Erase,
    BuildTextures,
    NewScript,
    NewUiDocument,
    Save,
    BuildGame,
    Play,
    ExitToHome,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FeatureOwner {
    Workspace,
    ProjectSettings,
    AppSettings,
    Content,
    Inspector,
    Dock,
    AssetManager,
    Tool,
    Action,
}
#[derive(Debug, Clone, Copy)]
pub struct FeatureDescriptor {
    pub id: Feature,
    pub title: &'static str,
    pub group: &'static str,
    pub owner: FeatureOwner,
}
macro_rules! item {
    ($id:ident,$name:expr,$group:expr,$owner:ident) => {
        FeatureDescriptor {
            id: Feature::$id,
            title: $name,
            group: $group,
            owner: FeatureOwner::$owner,
        }
    };
}
pub const DIRECTORY: &[FeatureDescriptor] = &[
    item!(Home, "Home / Scene Editor", "WORKSPACES", Workspace),
    item!(Terrain, "Terrain / Landscaping", "WORKSPACES", Workspace),
    item!(Navigation, "Navigation Editor", "WORKSPACES", Workspace),
    item!(UiAuthoring, "UI Authoring", "WORKSPACES", Workspace),
    item!(ScriptEditor, "AeoScript Editor", "WORKSPACES", Workspace),
    item!(
        ScriptSearch,
        "Project Script Search",
        "AEOSCRIPT",
        Workspace
    ),
    item!(ScriptDebugger, "AeoScript Debugger", "AEOSCRIPT", Workspace),
    item!(
        ProjectSettings,
        "Project Settings",
        "PROJECT",
        ProjectSettings
    ),
    item!(SaveData, "Save Data", "PROJECT", ProjectSettings),
    item!(Streaming, "World Streaming", "PROJECT", ProjectSettings),
    item!(Mouse, "Cursor & Mouse", "PROJECT", ProjectSettings),
    item!(Character, "Player Character", "PROJECT", ProjectSettings),
    item!(Controller, "Player Controller", "PROJECT", ProjectSettings),
    item!(Camera, "Project Camera", "PROJECT", ProjectSettings),
    item!(
        WorldLighting,
        "World Lighting & Sky",
        "PROJECT",
        ProjectSettings
    ),
    item!(
        WorldPhysics,
        "World Physics / Gravity",
        "PROJECT",
        ProjectSettings
    ),
    item!(ContentBrowser, "Content Browser", "CONTENT", Content),
    item!(Actors, "Actor Assets", "CONTENT", Content),
    item!(Scripts, "Script Assets", "CONTENT", Content),
    item!(PlayerScripts, "Player Scripts", "CONTENT", Content),
    item!(Textures, "Texture Assets", "CONTENT", Content),
    item!(Media, "Media Assets", "CONTENT", Content),
    item!(AudioAssets, "Audio Assets", "CONTENT", Content),
    item!(UiAssets, "UI Assets", "CONTENT", Content),
    item!(Resources, "Other Resources", "CONTENT", Content),
    item!(
        AssetManager,
        "Asset Manager / Import",
        "CONTENT",
        AssetManager
    ),
    item!(Explorer, "Project Explorer", "PANELS", Inspector),
    item!(Properties, "Object Properties", "PANELS", Inspector),
    item!(Landscaping, "Landscaping Tools", "PANELS", Inspector),
    item!(Output, "Output Log", "PANELS", Dock),
    item!(Problems, "Problems & Diagnostics", "PANELS", Dock),
    item!(AiAssistant, "AI Assistant", "PANELS", Inspector),
    item!(
        EditorPreferences,
        "Editor Preferences",
        "ENGINE SETTINGS",
        AppSettings
    ),
    item!(Theme, "Theme & Appearance", "ENGINE SETTINGS", AppSettings),
    item!(
        RenderLighting,
        "Renderer / Lighting",
        "ENGINE SETTINGS",
        AppSettings
    ),
    item!(
        AudioPreferences,
        "Audio Preferences",
        "ENGINE SETTINGS",
        AppSettings
    ),
    item!(
        InputPreferences,
        "Input & Shortcuts",
        "ENGINE SETTINGS",
        AppSettings
    ),
    item!(
        AiPreferences,
        "AI Providers & Models",
        "ENGINE SETTINGS",
        AppSettings
    ),
    item!(Select, "Select Tool", "BUILD TOOLS", Tool),
    item!(Move, "Move Tool", "BUILD TOOLS", Tool),
    item!(Rotate, "Rotate Tool", "BUILD TOOLS", Tool),
    item!(Scale, "Scale Tool", "BUILD TOOLS", Tool),
    item!(Build, "Build Tool", "BUILD TOOLS", Tool),
    item!(Erase, "Erase Tool", "BUILD TOOLS", Tool),
    item!(BuildTextures, "Build Texture Picker", "BUILD TOOLS", Tool),
    item!(NewScript, "New AeoScript", "CREATE", Action),
    item!(NewUiDocument, "New UI Document", "CREATE", Action),
    item!(Save, "Save Project", "FILE", Action),
    item!(BuildGame, "Build Game", "FILE", Action),
    item!(Play, "Play Game", "FILE", Action),
    item!(ExitToHome, "Exit to Home", "FILE", Action),
];
impl Editor {
    fn navigation_snapshot(&self) -> NavigationSnapshot {
        NavigationSnapshot {
            surface: self.navigation.surface,
            workspace: self.shell.workspace,
            settings_page: self.settings_page,
            project_section: self.navigation.project_section,
            asset_manager: self.asset_manager.open,
            character_editor: self.asset_manager.character_editor_active,
            material_editor: self.asset_manager.surface_editor_active,
            sky_editor: self.asset_manager.sky_editor_active,
            material_category: self.asset_manager.browser_category.clone(),
            material_folder: self.asset_manager.browser_folder.clone(),
            explorer: self.shell.explorer_visible,
            properties: self.shell.properties_visible,
            dock_expanded: self.shell.dock_expanded,
            dock: self.shell.dock,
            content_category: self.shell.asset_folder.clone(),
            content_kind: self.shell.actor_browser_kind.clone(),
            content_family: self.shell.actor_browser_family.clone(),
            content_directory: self.shell.browser_directory.clone(),
        }
    }

    pub(crate) fn observe_navigation_history(&mut self) {
        let state = self.navigation_snapshot();
        if let Some(previous) = self.navigation.last_seen.clone() {
            if previous != state {
                if self.navigation.previous.last() != Some(&previous) {
                    if self.navigation.previous.len() >= 48 {
                        self.navigation.previous.remove(0);
                    }
                    self.navigation.previous.push(previous);
                }
                self.navigation.next.clear();
            }
        }
        self.navigation.last_seen = Some(state);
    }

    fn restore_navigation_snapshot(&mut self, state: NavigationSnapshot) {
        if self.shell.workspace != state.workspace {
            self.activate_shell_workspace(state.workspace);
        }
        self.navigation.surface = state.surface;
        self.settings_page = state.settings_page;
        self.navigation.project_section = state.project_section;
        self.asset_manager.open = state.asset_manager;
        self.asset_manager.was_open = false;
        self.asset_manager.character_editor_active = state.character_editor;
        if !state.character_editor {
            self.asset_manager.character_editor = None;
        }
        self.shell.explorer_visible = state.explorer;
        self.shell.properties_visible = state.properties;
        self.shell.dock_expanded = state.dock_expanded;
        self.shell.dock = state.dock;
        self.asset_manager.surface_editor_active = state.material_editor;
        self.asset_manager.sky_editor_active = state.sky_editor;
        self.asset_manager.browser_category = state.material_category;
        self.asset_manager.browser_folder = state.material_folder;
        self.shell.asset_folder = state.content_category;
        self.shell.actor_browser_kind = state.content_kind;
        self.shell.actor_browser_family = state.content_family;
        self.shell.browser_directory = state.content_directory;
        let root = self.shell.project_snapshot.clone();
        self.shell.refresh(root.as_deref());
        self.navigation.last_seen = Some(self.navigation_snapshot());
    }

    pub(crate) fn navigate_history_back(&mut self) -> bool {
        self.observe_navigation_history();
        let Some(state) = self.navigation.previous.pop() else {
            return false;
        };
        let here = self.navigation_snapshot();
        if self.navigation.next.len() >= 48 {
            self.navigation.next.remove(0);
        }
        self.navigation.next.push(here);
        self.restore_navigation_snapshot(state);
        true
    }

    pub(crate) fn navigate_history_forward(&mut self) -> bool {
        self.observe_navigation_history();
        let Some(state) = self.navigation.next.pop() else {
            return false;
        };
        let here = self.navigation_snapshot();
        if self.navigation.previous.len() >= 48 {
            self.navigation.previous.remove(0);
        }
        self.navigation.previous.push(here);
        self.restore_navigation_snapshot(state);
        true
    }

    pub(crate) fn open_engine_settings(&mut self, page: SettingsPage) {
        self.asset_manager.open = false;
        self.asset_manager.was_open = false;
        self.navigation.surface = EditorSurface::AppSettings;
        self.settings_page = page;
    }
    pub(crate) fn open_project_settings(&mut self, section: ProjectSection) {
        self.asset_manager.open = false;
        self.asset_manager.was_open = false;
        self.navigation.surface = EditorSurface::ProjectSettings;
        self.navigation.project_section = section;
    }
    pub(crate) fn close_settings(&mut self) {
        self.navigation.surface = EditorSurface::Workspace;
    }
    pub(crate) fn navigate_to(&mut self, feature: Feature) {
        use Feature::*;
        match feature {
            Home => self.activate_shell_workspace(Workspace::Home),
            Terrain => self.activate_shell_workspace(Workspace::Terrain),
            Navigation => self.activate_shell_workspace(Workspace::Navigation),
            UiAuthoring => self.activate_shell_workspace(Workspace::Ui),
            ScriptEditor => self.activate_shell_workspace(Workspace::Script),
            ScriptSearch => {
                self.activate_shell_workspace(Workspace::Script);
                self.script_editor.show_search = true;
            }
            ScriptDebugger => {
                self.activate_shell_workspace(Workspace::Script);
            }

            ProjectSettings => self.open_project_settings(self.navigation.project_section),
            SaveData => self.open_project_settings(ProjectSection::SaveData),
            Streaming => self.open_project_settings(ProjectSection::Streaming),
            Mouse => self.open_project_settings(ProjectSection::Mouse),
            Character => self.open_project_settings(ProjectSection::Character),
            Controller => self.open_project_settings(ProjectSection::Controller),
            Camera => self.open_project_settings(ProjectSection::Camera),
            WorldLighting => self.open_project_settings(ProjectSection::Lighting),
            WorldPhysics => self.open_project_settings(ProjectSection::Physics),

            ContentBrowser => self.open_content_category("Content"),
            Actors => self.open_content_category("Actors"),
            Scripts => self.open_content_category("Scripts"),
            PlayerScripts => self.open_content_category("Player Scripts"),
            Textures => self.open_content_category("Textures"),
            Media => self.open_content_category("Media"),
            AudioAssets => self.open_content_category("Audio"),
            UiAssets => self.open_content_category("UI"),
            Resources => self.open_content_category("Resources"),
            AssetManager => {
                self.close_settings();
                self.asset_manager.open = true;
            }
            Explorer => {
                self.close_settings();
                self.shell.explorer_visible = true;
                self.left_panel_tab = LeftPanelTab::Project;
                self.shell.save();
            }
            Properties => {
                self.close_settings();
                self.show_properties_window = true;
                self.shell.properties_visible = true;
                self.right_panel_tab = RightPanelTab::Inspector;
                self.shell.save();
            }
            Landscaping => self.activate_shell_workspace(Workspace::Terrain),
            Output => {
                self.close_settings();
                self.shell.dock = DockTab::Output;
                self.shell.dock_expanded = true;
                self.shell.save();
            }
            Problems => {
                self.close_settings();
                self.shell.dock = DockTab::Problems;
                self.shell.dock_expanded = true;
                self.script_editor.show_problems = true;
                self.shell.save();
            }
            AiAssistant => {
                self.close_settings();
                self.ai_chat.open = true;
                self.shell.properties_visible = true;
                self.right_panel_tab = RightPanelTab::Ai;
                self.shell.save();
            }

            EditorPreferences => self.open_engine_settings(SettingsPage::Editor),
            Theme => self.open_engine_settings(SettingsPage::Theme),
            RenderLighting => self.open_engine_settings(SettingsPage::Lighting),
            AudioPreferences => self.open_engine_settings(SettingsPage::Audio),
            InputPreferences => self.open_engine_settings(SettingsPage::Input),
            AiPreferences => self.open_engine_settings(SettingsPage::Ai),

            Select | Move | Rotate | Scale | Build | Erase | BuildTextures => {
                self.close_settings();
                if self.ui_mode || self.show_script_workspace {
                    self.activate_shell_workspace(Workspace::Home);
                }
                match feature {
                    Select => self.activate_transform_mode(TransformMode::Select),
                    Move => self.activate_transform_mode(TransformMode::Move),
                    Rotate => self.activate_transform_mode(TransformMode::Rotate),
                    Scale => self.activate_transform_mode(TransformMode::Scale),
                    Build | BuildTextures => self.activate_editor_tool(EditorTool::Build),
                    Erase => self.activate_editor_tool(EditorTool::Erase),
                    _ => unreachable!(),
                }
                if feature == BuildTextures {
                    self.build_is_actor = false;
                    self.navigation.pending_build_texture_picker = true;
                }
            }

            NewScript => {
                self.activate_shell_workspace(Workspace::Script);
                self.script_editor.show_new_script_dialog = true;
            }
            NewUiDocument => {
                self.activate_shell_workspace(Workspace::Ui);
                self.ui_document_dialog_rename_id = None;
                self.ui_document_dialog_name.clear();
                self.ui_document_dialog_error = None;
                self.show_ui_document_dialog = true;
            }
            Save => {
                self.save_requested = true;
            }
            BuildGame => {
                self.needs_build = true;
            }
            Play => {
                self.close_settings();
                self.mode = crate::engine::EditorMode::Play;
            }
            ExitToHome => {
                self.needs_exit = true;
            }
        }
    }
    fn open_content_category(&mut self, category: &str) {
        self.close_settings();
        self.shell.asset_folder = category.to_owned();
        self.shell.browser_directory = None;
        self.shell.actor_browser_kind.clear();
        self.shell.actor_browser_family.clear();
        self.shell.dock = DockTab::Content;
        self.shell.dock_expanded = true;
        let root = self.shell.project_snapshot.clone();
        self.shell.refresh(root.as_deref());
        self.shell.save();
    }
    pub(crate) fn draw_feature_directory(&mut self, ui: &mut egui::Ui) {
        ui.menu_button("Features", |ui| {
            ui.set_min_width(345.0);
            ui.add(
                egui::TextEdit::singleline(&mut self.navigation.feature_search)
                    .hint_text("Find any editor feature...")
                    .desired_width(325.0),
            );
            ui.separator();
            let query = self.navigation.feature_search.trim().to_ascii_lowercase();
            let mut chosen = None;
            egui::ScrollArea::vertical()
                .max_height(510.0)
                .show(ui, |ui| {
                    let mut last_group = "";
                    for info in DIRECTORY {
                        if !query.is_empty()
                            && !format!("{} {}", info.title, info.group)
                                .to_ascii_lowercase()
                                .contains(&query)
                        {
                            continue;
                        }
                        if last_group != info.group {
                            if !last_group.is_empty() {
                                ui.separator();
                            }
                            ui.small(egui::RichText::new(info.group).strong());
                            last_group = info.group;
                        }
                        if ui.add(egui::Button::new(info.title).frame(false)).clicked() {
                            chosen = Some(info.id);
                            ui.close_menu();
                        }
                    }
                    if last_group.is_empty() {
                        ui.label("No matching features.");
                    }
                });
            if let Some(feature) = chosen {
                self.navigate_to(feature);
                self.navigation.feature_search.clear();
            }
        })
        .response
        .on_hover_text("All editor features in two clicks");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn wo_asm_001_browser_history_returns_through_pages_and_panels() {
        let mut editor = Editor::new();
        editor.observe_navigation_history();
        let initial = editor.navigation_snapshot();
        editor.open_project_settings(ProjectSection::Character);
        editor.observe_navigation_history();
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        editor.open_engine_settings(SettingsPage::Theme);
        editor.observe_navigation_history();
        assert_eq!(editor.navigation.surface, EditorSurface::AppSettings);

        assert!(editor.navigate_history_back());
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert_eq!(editor.navigation.project_section, ProjectSection::Character);
        assert!(editor.navigate_history_back());
        assert_eq!(editor.navigation_snapshot(), initial);
        assert!(editor.navigate_history_forward());
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);

        editor.asset_manager.open = true;
        editor.close_settings();
        editor.observe_navigation_history();
        assert!(editor.navigation.next.is_empty());
        assert!(editor.navigate_history_back());
        assert!(!editor.asset_manager.open);
    }

    #[test]
    fn wo_asm_001_history_tracks_panel_visibility_without_scene_edits() {
        let mut editor = Editor::new();
        editor.observe_navigation_history();
        let original = editor.shell.explorer_visible;
        editor.shell.explorer_visible = !original;
        editor.observe_navigation_history();
        assert!(editor.navigate_history_back());
        assert_eq!(editor.shell.explorer_visible, original);
        assert!(editor.navigate_history_forward());
        assert_eq!(editor.shell.explorer_visible, !original);
    }

    #[test]
    fn browser_back_exits_character_editor_without_leaving_a_stale_draft() {
        let mut editor = Editor::new();
        editor.open_project_settings(ProjectSection::Character);
        editor.observe_navigation_history();
        editor.asset_manager.open = true;
        editor.asset_manager.character_editor_active = true;
        editor.observe_navigation_history();

        assert!(editor.navigate_history_back());
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert!(!editor.asset_manager.open);
        assert!(!editor.asset_manager.character_editor_active);
        assert!(editor.asset_manager.character_editor.is_none());
    }

    fn rendered_labels(editor: &mut Editor) -> Vec<String> {
        let ctx = egui::Context::default();
        let renderer = crate::renderer::Renderer::new_for_tests(1440.0, 900.0);
        let mut world = crate::world::World::new();
        let output = ctx.run(
            egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    egui::Pos2::ZERO,
                    egui::vec2(1440.0, 900.0),
                )),
                ..Default::default()
            },
            |ctx| editor.show_ui(ctx, &mut world, &None, &renderer),
        );
        output
            .shapes
            .into_iter()
            .filter_map(|shape| {
                if let egui::Shape::Text(text) = shape.shape {
                    Some(text.galley.job.text.clone())
                } else {
                    None
                }
            })
            .collect()
    }
    #[test]
    fn project_settings_has_only_real_sections_and_never_opens_overview() {
        assert_eq!(ProjectSection::default(), ProjectSection::SaveData);
        assert_eq!(ProjectSection::ALL.len(), 8);
        assert_eq!(ProjectSection::ALL[0], ProjectSection::SaveData);
        assert!(
            !ProjectSection::ALL
                .iter()
                .any(|page| page.label() == "OVERVIEW")
        );
        let mut editor = Editor::new();
        editor.shell.initialized = true;
        assert_eq!(editor.navigation.surface, EditorSurface::Workspace);
        assert_eq!(editor.shell.workspace, Workspace::Home);
        editor.navigate_to(Feature::ProjectSettings);
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert_eq!(editor.navigation.project_section, ProjectSection::SaveData);
        let labels = rendered_labels(&mut editor);
        assert!(
            labels.iter().any(|text| text == "SAVE DATA"),
            "No real initial settings: {labels:?}"
        );
        assert!(
            labels.iter().any(|text| text == "Project Settings"),
            "Missing the direct Project Settings workspace tab: {labels:?}"
        );
        assert!(!labels.iter().any(|text| text == "OVERVIEW"));
        editor.navigate_to(Feature::Camera);
        editor.navigate_to(Feature::Home);
        editor.navigate_to(Feature::ProjectSettings);
        assert_eq!(
            editor.navigation.project_section,
            ProjectSection::Camera,
            "Reopening Project Settings should resume the last real category"
        );
    }
    #[test]
    fn switching_to_project_settings_from_ui_does_not_evict_active_workspace() {
        let mut editor = Editor::new();
        editor.enter_ui_mode();
        editor.shell.initialized = true;
        editor.navigate_to(Feature::ProjectSettings);
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        let labels = rendered_labels(&mut editor);
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert!(labels.iter().any(|s| s == "PROJECT SETTINGS"));
        assert!(editor.ui_mode);
        editor.close_settings();
        assert!(editor.ui_mode);
    }
    #[test]
    fn every_catalog_destination_has_a_real_canonical_surface() {
        for entry in DIRECTORY {
            let mut editor = Editor::new();
            editor.shell.initialized = true;
            editor.navigate_to(entry.id);
            match entry.owner {
                FeatureOwner::ProjectSettings => assert_eq!(
                    editor.navigation.surface,
                    EditorSurface::ProjectSettings,
                    "Project feature {:?} failed to open",
                    entry.id
                ),
                FeatureOwner::AppSettings => assert_eq!(
                    editor.navigation.surface,
                    EditorSurface::AppSettings,
                    "Engine Settings feature {:?} failed to open",
                    entry.id
                ),
                FeatureOwner::Content => {
                    assert_eq!(editor.shell.dock, DockTab::Content);
                    assert!(editor.shell.dock_expanded);
                }
                FeatureOwner::AssetManager => assert!(editor.asset_manager.open),
                FeatureOwner::Dock => assert!(editor.shell.dock_expanded),
                FeatureOwner::Tool => {
                    assert_eq!(editor.navigation.surface, EditorSurface::Workspace)
                }
                FeatureOwner::Inspector => {
                    assert!(editor.shell.explorer_visible || editor.shell.properties_visible)
                }
                FeatureOwner::Workspace => {
                    assert_eq!(editor.navigation.surface, EditorSurface::Workspace)
                }
                FeatureOwner::Action => {}
            }
        }
    }

    #[test]
    fn canonical_feature_catalog_has_unique_ids_names_and_one_owner() {
        let mut ids = std::collections::HashSet::new();
        let mut names = std::collections::HashSet::new();
        assert!(DIRECTORY.len() > 40);
        for item in DIRECTORY {
            assert!(ids.insert(item.id), "duplicate destination: {:?}", item.id);
            assert!(names.insert(item.title), "duplicate name: {}", item.title);
            assert!(!item.group.is_empty());
        }
    }
    #[test]
    fn settings_take_over_from_asset_manager_and_ai_stays_in_manager() {
        let mut editor = Editor::new();
        editor.navigate_to(Feature::AssetManager);
        assert!(editor.asset_manager.open);
        editor.navigate_to(Feature::AiAssistant);
        assert!(editor.asset_manager.open);
        assert!(editor.ai_chat.open);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Ai);
        editor.navigate_to(Feature::Theme);
        assert_eq!(editor.navigation.surface, EditorSurface::AppSettings);
        assert!(!editor.asset_manager.open);
        editor.navigate_to(Feature::AssetManager);
        assert_eq!(editor.navigation.surface, EditorSurface::Workspace);
        assert!(editor.asset_manager.open);
        editor.navigate_to(Feature::ProjectSettings);
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert!(!editor.asset_manager.open);
        editor.navigate_to(Feature::AssetManager);
        assert!(editor.asset_manager.open);
    }

    #[test]
    fn engine_vs_project_settings_are_distinct_exclusive_surfaces() {
        let mut editor = Editor::new();
        editor.navigate_to(Feature::SaveData);
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert_eq!(editor.navigation.project_section, ProjectSection::SaveData);
        editor.navigate_to(Feature::Theme);
        assert_eq!(editor.navigation.surface, EditorSurface::AppSettings);
        assert_eq!(editor.settings_page, SettingsPage::Theme);
        editor.navigate_to(Feature::ProjectSettings);
        assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        assert_eq!(editor.navigation.project_section, ProjectSection::SaveData);
        editor.navigate_to(Feature::Home);
        assert_eq!(editor.navigation.surface, EditorSurface::Workspace);
    }
    #[test]
    fn tools_panels_and_content_features_route_into_existing_owners() {
        let mut editor = Editor::new();
        editor.navigate_to(Feature::BuildTextures);
        assert_eq!(editor.current_tool, EditorTool::Build);
        assert!(editor.navigation.pending_build_texture_picker);
        editor.navigate_to(Feature::AiAssistant);
        assert!(editor.ai_chat.open && editor.shell.properties_visible);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Ai);
        editor.navigate_to(Feature::Properties);
        assert_eq!(editor.right_panel_tab, RightPanelTab::Inspector);
        editor.navigate_to(Feature::Textures);
        assert_eq!(editor.shell.asset_folder, "Textures");
        assert_eq!(editor.shell.dock, DockTab::Content);
        editor.navigate_to(Feature::ScriptEditor);
        assert!(editor.show_script_workspace);
    }
}
