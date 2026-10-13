use crate::project::ProjectHierarchy;
use crate::world::ActorHierarchy;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

pub const PROJECT_METADATA_FILE: &str = "project.json";
pub const PROJECT_METADATA_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ProjectMetadata {
    #[serde(default = "default_version")]
    pub version: u32,

    #[serde(default)]
    pub project_hierarchy: ProjectHierarchy,

    /// Legacy v1 hierarchy. Deserialized only so existing projects can migrate.
    #[serde(default)]
    pub actor_hierarchy: Option<ActorHierarchy>,
}

fn default_version() -> u32 {
    PROJECT_METADATA_VERSION
}

impl Default for ProjectMetadata {
    fn default() -> Self {
        Self {
            version: PROJECT_METADATA_VERSION,
            project_hierarchy: ProjectHierarchy::new(),
            actor_hierarchy: None,
        }
    }
}

pub fn metadata_path(project_path: &Path) -> PathBuf {
    project_path.join(PROJECT_METADATA_FILE)
}

pub fn load_project_metadata(project_path: &Path) -> io::Result<ProjectHierarchy> {
    let path = metadata_path(project_path);

    if !path.exists() {
        return Ok(ProjectHierarchy::new());
    }

    let content = fs::read_to_string(&path)?;

    let metadata = serde_json::from_str::<ProjectMetadata>(&content).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to parse {}: {}", path.display(), error),
        )
    })?;

    if metadata.version > PROJECT_METADATA_VERSION {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            format!(
                "Project metadata version {} is newer than supported version {}",
                metadata.version, PROJECT_METADATA_VERSION
            ),
        ));
    }

    let mut hierarchy = if metadata.version < 2 {
        if let Some(actor_hierarchy) = metadata.actor_hierarchy {
            migrate_actor_hierarchy(&actor_hierarchy)?
        } else {
            metadata.project_hierarchy
        }
    } else {
        metadata.project_hierarchy
    };

    hierarchy.rebuild_indexes();

    hierarchy.validate().map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Invalid Project hierarchy: {}", error),
        )
    })?;

    Ok(hierarchy)
}

fn migrate_actor_hierarchy(legacy: &ActorHierarchy) -> io::Result<ProjectHierarchy> {
    use std::collections::BTreeMap;

    let mut hierarchy = ProjectHierarchy::new();
    let mut directory_map = BTreeMap::new();

    fn create_directory_recursive(
        legacy: &ActorHierarchy,
        hierarchy: &mut ProjectHierarchy,
        directory_map: &mut BTreeMap<u64, u64>,
        directory_id: u64,
    ) -> Result<u64, String> {
        if let Some(existing) = directory_map.get(&directory_id) {
            return Ok(*existing);
        }

        let directory = legacy
            .directories
            .get(&directory_id)
            .ok_or_else(|| format!("Missing legacy Actor directory {}", directory_id))?;

        let parent = if let Some(parent_id) = directory.parent_id {
            Some(create_directory_recursive(
                legacy,
                hierarchy,
                directory_map,
                parent_id,
            )?)
        } else {
            None
        };

        let new_id = hierarchy.create_folder(parent, &directory.name)?;

        directory_map.insert(directory_id, new_id);

        Ok(new_id)
    }

    for directory_id in legacy.directories.keys().copied() {
        create_directory_recursive(legacy, &mut hierarchy, &mut directory_map, directory_id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }

    for (actor_id, directory_id) in &legacy.actor_directories {
        let parent_id = directory_map.get(directory_id).copied().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "Legacy Actor {} references unmigrated directory {}",
                    actor_id, directory_id
                ),
            )
        })?;

        hierarchy
            .create_actor_node(Some(parent_id), *actor_id)
            .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;
    }

    hierarchy
        .validate()
        .map_err(|error| io::Error::new(io::ErrorKind::InvalidData, error))?;

    Ok(hierarchy)
}

pub fn save_project_metadata(
    project_path: &Path,
    project_hierarchy: &ProjectHierarchy,
) -> io::Result<()> {
    fs::create_dir_all(project_path)?;

    let metadata = ProjectMetadata {
        version: PROJECT_METADATA_VERSION,
        project_hierarchy: project_hierarchy.clone(),
        actor_hierarchy: None,
    };

    let content = serde_json::to_string_pretty(&metadata).map_err(|error| {
        io::Error::new(
            io::ErrorKind::InvalidData,
            format!("Failed to serialize project metadata: {}", error),
        )
    })?;

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
    crate::project::atomic_file::write_atomically(
        &metadata_path(project_path),
        format!("{}\n", content).as_bytes(),
    )
}

pub fn ensure_project_metadata(project_path: &Path) -> io::Result<()> {
    let path = metadata_path(project_path);

    if path.exists() {
        return Ok(());
    }

    save_project_metadata(project_path, &ProjectHierarchy::new())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_project_path(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_project_metadata_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_metadata_round_trip() {
        let project_path = test_project_path("round_trip");

        fs::create_dir_all(&project_path).unwrap();

        let mut hierarchy = ProjectHierarchy::new();

        let dungeon = hierarchy.create_folder(None, "Dungeon").unwrap();

        let enemies = hierarchy.create_folder(Some(dungeon), "Enemies").unwrap();

        let actor = hierarchy.create_actor_node(Some(enemies), 42).unwrap();

        hierarchy
            .create_script_node(Some(actor), "scripts/goblin.aeo")
            .unwrap();

        save_project_metadata(&project_path, &hierarchy).unwrap();

        let loaded = load_project_metadata(&project_path).unwrap();

        assert_eq!(loaded, hierarchy);
        assert!(loaded.validate().is_ok());

        fs::remove_dir_all(project_path).ok();
    }

    #[test]
    fn test_v1_actor_hierarchy_migrates_to_project_hierarchy() {
        let project_path = test_project_path("v1_migration");

        fs::create_dir_all(&project_path).unwrap();

        fs::write(
            metadata_path(&project_path),
            r#"{
                "version": 1,
                "actor_hierarchy": {
                    "next_directory_id": 3,
                    "directories": {
                        "1": {
                            "id": 1,
                            "name": "Dungeon",
                            "parent_id": null
                        },
                        "2": {
                            "id": 2,
                            "name": "Enemies",
                            "parent_id": 1
                        }
                    },
                    "actor_directories": {
                        "42": 2
                    }
                }
            }"#,
        )
        .unwrap();

        let hierarchy = load_project_metadata(&project_path).unwrap();

        assert_eq!(hierarchy.find_folder_nodes("Dungeon").len(), 1);

        assert_eq!(hierarchy.find_folder_nodes("Enemies").len(), 1);

        assert!(hierarchy.find_actor_node(42).is_some());

        assert!(hierarchy.validate().is_ok());

        fs::remove_dir_all(project_path).ok();
    }

    #[test]
    fn test_missing_metadata_returns_empty_hierarchy() {
        let project_path = test_project_path("missing");

        fs::create_dir_all(&project_path).unwrap();

        let loaded = load_project_metadata(&project_path).unwrap();

        assert!(loaded.nodes.is_empty());

        fs::remove_dir_all(project_path).ok();
    }

    #[test]
    fn test_metadata_is_created_when_missing() {
        let project_path = test_project_path("create");

        fs::create_dir_all(&project_path).unwrap();

        ensure_project_metadata(&project_path).unwrap();

        assert!(metadata_path(&project_path).exists());

        let loaded = load_project_metadata(&project_path).unwrap();

        assert!(loaded.nodes.is_empty());

        fs::remove_dir_all(project_path).ok();
    }

    #[test]
    fn test_invalid_metadata_is_rejected() {
        let project_path = test_project_path("invalid");

        fs::create_dir_all(&project_path).unwrap();

        fs::write(
            metadata_path(&project_path),
            r#"{
                "version": 2,
                "project_hierarchy": {
                    "next_node_id": 2,
                    "nodes": {
                        "1": {
                            "id": 1,
                            "parent_id": 999,
                            "kind": {
                                "Folder": {
                                    "name": "Dungeon"
                                }
                            }
                        }
                    }
                }
            }"#,
        )
        .unwrap();

        let result = load_project_metadata(&project_path);

        assert!(result.is_err());

        fs::remove_dir_all(project_path).ok();
    }
}
