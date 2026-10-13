use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const EDITOR_STATE_FILE: &str = "editor.json";
pub const EDITOR_STATE_VERSION: u32 = 3;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct EditorProjectState {
    #[serde(default = "default_version")]
    pub version: u32,

    #[serde(default)]
    pub anchor: [i32; 3],

    #[serde(default)]
    pub selected_actor_id: Option<u64>,

    // Legacy v1 field retained for editor.json migration.
    #[serde(default)]
    pub selected_actor_directory_id: Option<u64>,

    #[serde(default)]
    pub selected_project_node_id: Option<u64>,

    #[serde(default)]
    pub expanded_project_nodes: Vec<u64>,

    #[serde(default = "default_true")]
    pub show_properties_window: bool,

    #[serde(default = "default_true")]
    pub show_world_window: bool,

    #[serde(default = "default_left_panel_tab")]
    pub left_panel_tab: String,

    #[serde(default = "default_true")]
    pub navigation_open: bool,

    #[serde(default = "default_split")]
    pub right_panel_split: f32,

    #[serde(default)]
    pub show_script_workspace: bool,

    #[serde(default)]
    pub active_script: Option<String>,
}

fn default_version() -> u32 {
    EDITOR_STATE_VERSION
}

fn default_true() -> bool {
    true
}

fn default_split() -> f32 {
    0.5
}

fn default_left_panel_tab() -> String {
    "project".to_string()
}

impl Default for EditorProjectState {
    fn default() -> Self {
        Self {
            version: EDITOR_STATE_VERSION,
            anchor: [0, 0, 0],
            selected_actor_id: None,
            selected_actor_directory_id: None,
            selected_project_node_id: None,
            expanded_project_nodes: Vec::new(),
            show_properties_window: true,
            show_world_window: true,
            left_panel_tab: default_left_panel_tab(),
            navigation_open: true,
            right_panel_split: 0.5,
            show_script_workspace: false,
            active_script: None,
        }
    }
}

pub fn editor_state_path(project_path: &Path) -> PathBuf {
    project_path.join(EDITOR_STATE_FILE)
}

pub fn load_editor_state(project_path: &Path) -> io::Result<EditorProjectState> {
    let path = editor_state_path(project_path);

    if !path.exists() {
        return Ok(EditorProjectState::default());
    }

    let content = fs::read_to_string(&path)?;

    let state = serde_json::from_str::<EditorProjectState>(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to parse {}: {}", path.display(), error),
        )
    })?;

    if state.version > EDITOR_STATE_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Editor state version {} is newer than supported version {}",
                state.version, EDITOR_STATE_VERSION
            ),
        ));
    }

    Ok(state)
}

pub fn save_editor_state(project_path: &Path, state: &EditorProjectState) -> io::Result<()> {
    fs::create_dir_all(project_path)?;

    let content = serde_json::to_string_pretty(state).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to serialize editor state: {}", error),
        )
    })?;

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
    crate::project::atomic_file::write_atomically(
        &editor_state_path(project_path),
        format!("{}\n", content).as_bytes(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_editor_state_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_editor_state_round_trip() {
        let root = test_root("roundtrip");

        fs::create_dir_all(&root).unwrap();

        let state = EditorProjectState {
            anchor: [12, -4, 8],
            selected_actor_id: Some(42),
            selected_actor_directory_id: Some(7),
            selected_project_node_id: Some(42),
            expanded_project_nodes: vec![1, 2, 42],
            show_properties_window: false,
            show_world_window: true,
            navigation_open: false,
            right_panel_split: 0.72,
            show_script_workspace: true,
            active_script: Some("scripts/Dungeon/main.aeo".to_string()),
            ..Default::default()
        };

        save_editor_state(&root, &state).unwrap();

        let loaded = load_editor_state(&root).unwrap();

        assert_eq!(loaded, state);

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_missing_editor_state_returns_default() {
        let root = test_root("missing");

        fs::create_dir_all(&root).unwrap();

        let state = load_editor_state(&root).unwrap();

        assert_eq!(state, EditorProjectState::default());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_future_editor_state_version_is_rejected() {
        let root = test_root("future");

        fs::create_dir_all(&root).unwrap();

        fs::write(
            editor_state_path(&root),
            r#"{
                "version": 999,
                "anchor": [0, 0, 0],
                "selected_actor_id": null,
                "selected_actor_directory_id": null,
                "show_properties_window": true,
                "show_world_window": true,
                "navigation_open": true,
                "right_panel_split": 0.5,
                "show_script_workspace": false,
                "active_script": null
            }"#,
        )
        .unwrap();

        assert!(load_editor_state(&root).is_err());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_legacy_editor_state_without_project_explorer_fields_loads() {
        let root = test_root("legacy_project_explorer");

        fs::create_dir_all(&root).unwrap();

        fs::write(
            editor_state_path(&root),
            r#"{
                "version": 1,
                "anchor": [0, 0, 0],
                "selected_actor_id": 42,
                "selected_actor_directory_id": 7,
                "show_properties_window": true,
                "show_world_window": true,
                "navigation_open": true,
                "right_panel_split": 0.5,
                "show_script_workspace": false,
                "active_script": null
            }"#,
        )
        .unwrap();

        let state = load_editor_state(&root).unwrap();

        assert_eq!(state.selected_actor_id, Some(42));
        assert_eq!(state.selected_actor_directory_id, Some(7));
        assert_eq!(state.selected_project_node_id, None);
        assert!(state.expanded_project_nodes.is_empty());

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_editor_state_active_script_is_project_relative() {
        let root = PathBuf::from("Project");

        let state = EditorProjectState {
            active_script: Some("playerscripts/Controller/player.aeo".to_string()),
            ..Default::default()
        };

        let encoded = serde_json::to_string(&state).unwrap();
        let decoded: EditorProjectState = serde_json::from_str(&encoded).unwrap();

        assert_eq!(
            decoded.active_script,
            Some("playerscripts/Controller/player.aeo".to_string())
        );

        assert_eq!(
            root.join(decoded.active_script.unwrap()),
            PathBuf::from("Project/playerscripts/Controller/player.aeo")
        );
    }
}
