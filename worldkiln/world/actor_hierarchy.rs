use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

use crate::world::Actor;

/// Persistent project/world organization for authored Actors.
///
/// The project root is implicit. Actor directories are metadata only and are
/// not filesystem directories.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorDirectory {
    pub id: u64,
    pub name: String,
    pub parent_id: Option<u64>,
}

/// Persistent Actor hierarchy.
///
/// Only directory records and Actor memberships are serialized. Lookup
/// indexes are rebuilt after loading.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ActorHierarchy {
    pub next_directory_id: u64,
    pub directories: BTreeMap<u64, ActorDirectory>,
    pub actor_directories: BTreeMap<u64, u64>,

    #[serde(skip)]
    #[serde(default)]
    directory_children: HashMap<Option<u64>, Vec<u64>>,

    #[serde(skip)]
    #[serde(default)]
    directory_names: HashMap<(Option<u64>, String), Vec<u64>>,
}

impl ActorHierarchy {
    pub fn new() -> Self {
        let mut hierarchy = Self {
            next_directory_id: 1,
            directories: BTreeMap::new(),
            actor_directories: BTreeMap::new(),
            directory_children: HashMap::new(),
            directory_names: HashMap::new(),
        };

        hierarchy.rebuild_indexes();
        hierarchy
    }

    pub fn rebuild_indexes(&mut self) {
        self.directory_children.clear();
        self.directory_names.clear();

        for directory in self.directories.values() {
            self.directory_children
                .entry(directory.parent_id)
                .or_default()
                .push(directory.id);

            self.directory_names
                .entry((directory.parent_id, directory.name.clone()))
                .or_default()
                .push(directory.id);
        }

        for children in self.directory_children.values_mut() {
            children.sort_unstable();
        }

        for matches in self.directory_names.values_mut() {
            matches.sort_unstable();
        }
    }

    pub fn validate(&self) -> Result<(), String> {
        for directory in self.directories.values() {
            let name = directory.name.trim();

            if name.is_empty() {
                return Err(format!(
                    "Actor directory {} has an empty name",
                    directory.id
                ));
            }

            if name.contains('/') || name.contains('\\') {
                return Err(format!(
                    "Actor directory '{}' contains a path separator",
                    directory.name
                ));
            }

            if directory.parent_id == Some(directory.id) {
                return Err(format!(
                    "Actor directory {} cannot be its own parent",
                    directory.id
                ));
            }

            if let Some(parent_id) = directory.parent_id {
                if !self.directories.contains_key(&parent_id) {
                    return Err(format!(
                        "Actor directory {} references missing parent {}",
                        directory.id, parent_id
                    ));
                }
            }
        }

        for directory_id in self.directories.keys().copied() {
            let mut current = Some(directory_id);
            let mut visited = HashSet::new();

            while let Some(id) = current {
                if !visited.insert(id) {
                    return Err(format!(
                        "Actor directory hierarchy contains a cycle at {}",
                        id
                    ));
                }

                current = self
                    .directories
                    .get(&id)
                    .and_then(|directory| directory.parent_id);
            }
        }

        for (actor_id, directory_id) in &self.actor_directories {
            if !self.directories.contains_key(directory_id) {
                return Err(format!(
                    "Actor {} references missing directory {}",
                    actor_id, directory_id
                ));
            }
        }

        Ok(())
    }

    pub fn create_directory(
        &mut self,
        parent_id: Option<u64>,
        name: impl Into<String>,
    ) -> Result<u64, String> {
        let name = name.into().trim().to_string();

        if name.is_empty() {
            return Err("Actor directory name cannot be empty".to_string());
        }

        if name.contains('/') || name.contains('\\') {
            return Err("Actor directory name cannot contain '/' or '\\'".to_string());
        }

        if let Some(parent_id) = parent_id {
            if !self.directories.contains_key(&parent_id) {
                return Err(format!(
                    "Actor directory parent {} does not exist",
                    parent_id
                ));
            }
        }

        let id = self.next_directory_id;

        self.next_directory_id = self
            .next_directory_id
            .checked_add(1)
            .ok_or_else(|| "Actor directory ID space exhausted".to_string())?;

        self.directories.insert(
            id,
            ActorDirectory {
                id,
                name,
                parent_id,
            },
        );

        self.rebuild_indexes();

        Ok(id)
    }

    pub fn rename_directory(
        &mut self,
        directory_id: u64,
        name: impl Into<String>,
    ) -> Result<(), String> {
        let name = name.into().trim().to_string();

        if name.is_empty() {
            return Err("Actor directory name cannot be empty".to_string());
        }

        if name.contains('/') || name.contains('\\') {
            return Err("Actor directory name cannot contain '/' or '\\'".to_string());
        }

        let directory = self
            .directories
            .get_mut(&directory_id)
            .ok_or_else(|| format!("Actor directory {} does not exist", directory_id))?;

        directory.name = name;
        self.rebuild_indexes();

        Ok(())
    }

    pub fn move_directory(
        &mut self,
        directory_id: u64,
        new_parent_id: Option<u64>,
    ) -> Result<(), String> {
        if !self.directories.contains_key(&directory_id) {
            return Err(format!("Actor directory {} does not exist", directory_id));
        }

        if let Some(parent_id) = new_parent_id {
            if !self.directories.contains_key(&parent_id) {
                return Err(format!(
                    "Actor directory parent {} does not exist",
                    parent_id
                ));
            }

            let mut current = Some(parent_id);

            while let Some(id) = current {
                if id == directory_id {
                    return Err(
                        "Actor directory cannot be moved inside itself or one of its descendants"
                            .to_string(),
                    );
                }

                current = self
                    .directories
                    .get(&id)
                    .and_then(|directory| directory.parent_id);
            }
        }

        self.directories
            .get_mut(&directory_id)
            .expect("directory existence checked above")
            .parent_id = new_parent_id;

        self.rebuild_indexes();

        Ok(())
    }

    pub fn remove_directory(&mut self, directory_id: u64) -> Result<ActorDirectory, String> {
        if !self.directories.contains_key(&directory_id) {
            return Err(format!("Actor directory {} does not exist", directory_id));
        }

        if self
            .directory_children
            .get(&Some(directory_id))
            .is_some_and(|children| !children.is_empty())
        {
            return Err("Actor directory is not empty".to_string());
        }

        if self
            .actor_directories
            .values()
            .any(|member_directory_id| *member_directory_id == directory_id)
        {
            return Err("Actor directory still contains Actors".to_string());
        }

        let directory = self
            .directories
            .remove(&directory_id)
            .expect("directory existence checked above");

        self.rebuild_indexes();

        Ok(directory)
    }

    pub fn contains_directory(&self, directory_id: u64) -> bool {
        self.directories.contains_key(&directory_id)
    }

    pub fn directory_for_actor(&self, actor_id: u64) -> Option<u64> {
        self.actor_directories.get(&actor_id).copied()
    }

    pub fn set_actor_directory(
        &mut self,
        actor_id: u64,
        directory_id: Option<u64>,
    ) -> Result<(), String> {
        if let Some(directory_id) = directory_id {
            if !self.directories.contains_key(&directory_id) {
                return Err(format!("Actor directory {} does not exist", directory_id));
            }

            self.actor_directories.insert(actor_id, directory_id);
        } else {
            self.actor_directories.remove(&actor_id);
        }

        Ok(())
    }

    pub fn remove_actor(&mut self, actor_id: u64) {
        self.actor_directories.remove(&actor_id);
    }

    pub fn directory_children(&self, parent_id: Option<u64>) -> Vec<u64> {
        self.directory_children
            .get(&parent_id)
            .cloned()
            .unwrap_or_default()
    }

    pub fn actor_ids_in_directory(
        &self,
        directory_id: Option<u64>,
        actors: &HashMap<u64, Actor>,
    ) -> Vec<u64> {
        let mut ids = if let Some(directory_id) = directory_id {
            self.actor_directories
                .iter()
                .filter_map(|(actor_id, member_directory_id)| {
                    (*member_directory_id == directory_id).then_some(*actor_id)
                })
                .collect::<Vec<_>>()
        } else {
            actors
                .keys()
                .filter(|actor_id| !self.actor_directories.contains_key(actor_id))
                .copied()
                .collect::<Vec<_>>()
        };

        ids.sort_unstable();
        ids
    }

    pub fn resolve_directory_path(&self, path: &str) -> Vec<u64> {
        let normalized_path = path.replace('\\', "/");

        let components = normalized_path
            .split('/')
            .map(str::trim)
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();

        if components.is_empty() {
            return self.directory_children(None);
        }

        let mut parents = vec![None];

        for component in components {
            let mut next = Vec::new();

            for parent_id in &parents {
                if let Some(matches) = self
                    .directory_names
                    .get(&(*parent_id, component.to_string()))
                {
                    next.extend(matches.iter().copied());
                }
            }

            next.sort_unstable();
            next.dedup();

            if next.is_empty() {
                return Vec::new();
            }

            parents = next.into_iter().map(Some).collect();
        }

        parents.into_iter().flatten().collect()
    }

    pub fn resolve_actor_path(&self, actors: &HashMap<u64, Actor>, path: &str) -> Vec<u64> {
        let normalized_path = path.replace('\\', "/");

        let components = normalized_path
            .split('/')
            .map(str::trim)
            .filter(|component| !component.is_empty())
            .collect::<Vec<_>>();

        let Some(actor_name) = components.last() else {
            return Vec::new();
        };

        let directory_path = &components[..components.len() - 1];

        let parent_directories = if directory_path.is_empty() {
            vec![None]
        } else {
            self.resolve_directory_path(&directory_path.join("/"))
                .into_iter()
                .map(Some)
                .collect()
        };

        let mut matches = Vec::new();

        for (actor_id, actor) in actors {
            let actor_directory = self.actor_directories.get(actor_id).copied();

            if !parent_directories.contains(&actor_directory) {
                continue;
            }

            if actor.identity == *actor_name {
                matches.push(*actor_id);
            }
        }

        matches.sort_unstable();
        matches
    }

    pub fn directory_path(&self, directory_id: u64) -> Option<Vec<String>> {
        let mut current = Some(directory_id);
        let mut path = Vec::new();
        let mut visited = HashSet::new();

        while let Some(id) = current {
            if !visited.insert(id) {
                return None;
            }

            let directory = self.directories.get(&id)?;
            path.push(directory.name.clone());
            current = directory.parent_id;
        }

        path.reverse();
        Some(path)
    }
}

impl Default for ActorHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nested_directories_and_paths() {
        let mut hierarchy = ActorHierarchy::new();

        let dungeon = hierarchy.create_directory(None, "Dungeon").unwrap();

        let collectibles = hierarchy
            .create_directory(Some(dungeon), "Collectibles")
            .unwrap();

        assert_eq!(hierarchy.resolve_directory_path("Dungeon"), vec![dungeon]);

        assert_eq!(
            hierarchy.resolve_directory_path("Dungeon/Collectibles"),
            vec![collectibles]
        );

        assert_eq!(
            hierarchy.directory_path(collectibles),
            Some(vec!["Dungeon".to_string(), "Collectibles".to_string()])
        );
    }

    #[test]
    fn test_actor_membership_and_resolution() {
        let mut hierarchy = ActorHierarchy::new();

        let dungeon = hierarchy.create_directory(None, "Dungeon").unwrap();

        let collectibles = hierarchy
            .create_directory(Some(dungeon), "Collectibles")
            .unwrap();

        let mut actors = HashMap::new();

        actors.insert(1, Actor::new(1, "Coin", glam::Vec3::ZERO));
        actors.insert(2, Actor::new(2, "Coin", glam::Vec3::ONE));

        hierarchy
            .set_actor_directory(1, Some(collectibles))
            .unwrap();

        hierarchy
            .set_actor_directory(2, Some(collectibles))
            .unwrap();

        assert_eq!(
            hierarchy.actor_ids_in_directory(Some(collectibles), &actors),
            vec![1, 2]
        );

        assert_eq!(
            hierarchy.resolve_actor_path(&actors, "Dungeon/Collectibles/Coin"),
            vec![1, 2]
        );
    }

    #[test]
    fn test_duplicate_names_do_not_destroy_identity() {
        let mut hierarchy = ActorHierarchy::new();

        let first = hierarchy.create_directory(None, "Dungeon").unwrap();
        let second = hierarchy.create_directory(None, "Dungeon").unwrap();

        assert_ne!(first, second);

        let matches = hierarchy.resolve_directory_path("Dungeon");

        assert_eq!(matches.len(), 2);
        assert!(matches.contains(&first));
        assert!(matches.contains(&second));
    }

    #[test]
    fn test_move_directory_rejects_descendant_cycle() {
        let mut hierarchy = ActorHierarchy::new();

        let root = hierarchy.create_directory(None, "Root").unwrap();
        let child = hierarchy.create_directory(Some(root), "Child").unwrap();

        assert!(hierarchy.move_directory(root, Some(child)).is_err());
    }

    #[test]
    fn test_remove_directory_requires_empty_directory() {
        let mut hierarchy = ActorHierarchy::new();

        let root = hierarchy.create_directory(None, "Root").unwrap();
        let child = hierarchy.create_directory(Some(root), "Child").unwrap();

        assert!(hierarchy.remove_directory(root).is_err());

        hierarchy.remove_directory(child).unwrap();
        hierarchy.remove_directory(root).unwrap();

        assert!(!hierarchy.contains_directory(root));
    }
}
