// Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:33 -05:00
//! Canonical editor navigation. Every feature owns exactly one destination;
//! menu, workspace, toolbar and feature-directory shortcuts all dispatch here.
//! The active settings surface is a typed enum, never a collection of booleans.
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

// Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:13 -05:00
/// Every Project Settings destination is a real editor. There is no
/// text-only Overview page in the category rail or route.
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
        }
    }
}

#[derive(Debug, Default)]
pub struct NavigationState {
    pub surface: EditorSurface,
    pub project_section: ProjectSection,
    pub feature_search: String,
    // A one-shot UI request, not a second texture picker or build template.
    pub pending_build_texture_picker: bool,
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
    // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 22:02 -05:00
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
    pub(crate) fn open_engine_settings(&mut self, page: SettingsPage) {
        // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:33 -05:00
        self.navigation.surface = EditorSurface::AppSettings;
        self.settings_page = page;
    }
    pub(crate) fn open_project_settings(&mut self, section: ProjectSection) {
        // Project-owned settings never mutate the engine-wide preferences page.
        self.navigation.surface = EditorSurface::ProjectSettings;
        self.navigation.project_section = section;
    }
    pub(crate) fn close_settings(&mut self) {
        self.navigation.surface = EditorSurface::Workspace;
    }
    pub(crate) fn navigate_to(&mut self, feature: Feature) {
        // Canonical routing only. No copied views, hidden fake windows or
        // direct project-data mutation except explicit File actions.
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
                // Debugger bar + inspector are part of Script Editor, not
                // separate floating panels or duplicated debugger logic.
            }

            // WO74: the top-level Project Settings action resumes a real
            // category; the first entry starts at Save Data, never Overview.
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
                self.needs_save = true;
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
    /// One visible Features menu => one destination click. No nested menus.
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
    // Worldkiln 0.8.5 | WO 0.8.7-73 | 2026-10-09 21:57 -05:00
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
    // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:25 -05:00
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
    fn every_project_configuration_section_actually_renders_with_two_click_route() {
        use Feature::*;
        let pages = [
            (ProjectSettings, "PROJECT SETTINGS"),
            (SaveData, "SAVE DATA"),
            (Streaming, "STREAMING"),
            (Mouse, "MOUSE"),
            (Character, "CHARACTER"),
            (Controller, "CONTROLLER"),
            (Camera, "CAMERA"),
            (WorldLighting, "LIGHTING"),
            (WorldPhysics, "PHYSICS"),
        ];
        for (feature, expected) in pages {
            let mut editor = Editor::new();
            editor.shell.initialized = true;
            editor.navigate_to(feature);
            assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
            let labels = rendered_labels(&mut editor);
            assert!(
                labels.iter().any(|s| s == expected),
                "Project route {feature:?} has no visible {expected} section; labels: {labels:?}"
            );
            assert_eq!(editor.navigation.surface, EditorSurface::ProjectSettings);
        }
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
