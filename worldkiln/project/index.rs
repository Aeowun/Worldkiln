use super::script_tree::{ScriptTree, ScriptTreeNode};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ScriptRoot {
    Scripts,
    PlayerScripts,
}

impl ScriptRoot {
    pub fn directory_name(self) -> &'static str {
        match self {
            Self::Scripts => "scripts",
            Self::PlayerScripts => "playerscripts",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedModule {
    pub root: ScriptRoot,
    pub relative_path: PathBuf,
}

impl IndexedModule {
    pub fn logical_path(&self) -> PathBuf {
        Path::new(self.root.directory_name()).join(&self.relative_path)
    }
}

#[derive(Clone, Debug)]
pub struct ProjectScriptIndex {
    pub project_root: PathBuf,
    pub scripts: ScriptTree,
    pub playerscripts: ScriptTree,
    path_index: HashMap<String, IndexedModule>,
    name_index: HashMap<String, Vec<IndexedModule>>,
}

impl ProjectScriptIndex {
    pub fn build(project_root: &Path) -> io::Result<Self> {
        let scripts = ScriptTree::build(&project_root.join("scripts"))?;
        let playerscripts = ScriptTree::build(&project_root.join("playerscripts"))?;

        let mut index = Self {
            project_root: project_root.to_path_buf(),
            scripts,
            playerscripts,
            path_index: HashMap::new(),
            name_index: HashMap::new(),
        };

        index.index_tree(ScriptRoot::Scripts);
        index.index_tree(ScriptRoot::PlayerScripts);

        for modules in index.name_index.values_mut() {
            modules.sort_by(|a, b| {
                normalize_path(&a.logical_path()).cmp(&normalize_path(&b.logical_path()))
            });

            modules.dedup();
        }

        Ok(index)
    }

    fn index_tree(&mut self, root: ScriptRoot) {
        let relative_paths = match root {
            ScriptRoot::Scripts => self.scripts.module_paths(),
            ScriptRoot::PlayerScripts => self.playerscripts.module_paths(),
        };

        for relative_path in relative_paths {
            let module = IndexedModule {
                root,
                relative_path,
            };

            let logical_path = normalize_path(&module.logical_path());

            self.path_index.insert(logical_path, module.clone());

            let name = module
                .relative_path
                .file_stem()
                .and_then(|value| value.to_str())
                .unwrap_or_default()
                .to_lowercase();

            if !name.is_empty() {
                self.name_index.entry(name).or_default().push(module);
            }
        }
    }

    pub fn find_module(&self, path: &str) -> Option<&IndexedModule> {
        let normalized = normalize_path_str(path);

        if normalized.is_empty() {
            return None;
        }

        self.path_index.get(&normalized)
    }

    pub fn find_modules_named(&self, name: &str) -> Vec<&IndexedModule> {
        let normalized = normalize_name(name);

        if normalized.is_empty() {
            return Vec::new();
        }

        self.name_index
            .get(&normalized)
            .map(|modules| modules.iter().collect())
            .unwrap_or_default()
    }

    pub fn find_modules_named_in_scope(
        &self,
        root: ScriptRoot,
        scope: &str,
        name: &str,
    ) -> Vec<&IndexedModule> {
        let normalized_scope = normalize_path_str(scope);
        let normalized_name = normalize_name(name);

        if normalized_name.is_empty() {
            return Vec::new();
        }

        let tree = match root {
            ScriptRoot::Scripts => &self.scripts,
            ScriptRoot::PlayerScripts => &self.playerscripts,
        };

        tree.find_modules_in_scope(&normalized_scope, &normalized_name)
            .into_iter()
            .filter_map(|relative_path| {
                let logical_path = Path::new(root.directory_name()).join(relative_path);

                self.path_index.get(&normalize_path(&logical_path))
            })
            .collect()
    }

    pub fn contains_path(&self, path: &str) -> bool {
        self.find_module(path).is_some()
    }

    pub fn all_modules(&self) -> Vec<IndexedModule> {
        let mut modules = self.path_index.values().cloned().collect::<Vec<_>>();

        modules.sort_by(|a, b| {
            normalize_path(&a.logical_path()).cmp(&normalize_path(&b.logical_path()))
        });

        modules
    }

    pub fn absolute_path(&self, module: &IndexedModule) -> PathBuf {
        self.project_root.join(module.logical_path())
    }

    pub fn tree(&self, root: ScriptRoot) -> &ScriptTree {
        match root {
            ScriptRoot::Scripts => &self.scripts,
            ScriptRoot::PlayerScripts => &self.playerscripts,
        }
    }

    pub fn root_children(&self, root: ScriptRoot) -> &[ScriptTreeNode] {
        &self.tree(root).children
    }
}

fn normalize_path(path: &Path) -> String {
    normalize_path_str(&path.to_string_lossy())
}

fn normalize_path_str(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_lowercase()
}

fn normalize_name(name: &str) -> String {
    let cleaned = name.trim().replace('\\', "/");

    let filename = cleaned.rsplit('/').next().unwrap_or_default();

    filename
        .strip_suffix(".aeo")
        .unwrap_or(filename)
        .to_lowercase()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_project_script_index_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_project_index_combines_script_roots() {
        let root = test_root("combined");

        fs::create_dir_all(root.join("scripts/Dungeon")).unwrap();
        fs::create_dir_all(root.join("playerscripts/Controller")).unwrap();

        fs::write(root.join("scripts/Dungeon/main.aeo"), "").unwrap();
        fs::write(root.join("playerscripts/Controller/TopDown.aeo"), "").unwrap();

        let index = ProjectScriptIndex::build(&root).unwrap();

        assert_eq!(index.all_modules().len(), 2);
        assert!(index.contains_path("scripts/Dungeon/main.aeo"));
        assert!(index.contains_path("playerscripts/Controller/TopDown.aeo"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_project_index_paths_are_root_aware() {
        let root = test_root("roots");

        fs::create_dir_all(root.join("scripts/Dungeon")).unwrap();
        fs::create_dir_all(root.join("playerscripts/Dungeon")).unwrap();

        fs::write(root.join("scripts/Dungeon/TopDown.aeo"), "").unwrap();
        fs::write(root.join("playerscripts/Dungeon/TopDown.aeo"), "").unwrap();

        let index = ProjectScriptIndex::build(&root).unwrap();

        assert_ne!(
            index
                .find_module("scripts/Dungeon/TopDown.aeo")
                .unwrap()
                .root,
            index
                .find_module("playerscripts/Dungeon/TopDown.aeo")
                .unwrap()
                .root
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_project_index_name_lookup() {
        let root = test_root("names");

        fs::create_dir_all(root.join("scripts/A")).unwrap();
        fs::create_dir_all(root.join("scripts/B")).unwrap();

        fs::write(root.join("scripts/A/doors.aeo"), "").unwrap();
        fs::write(root.join("scripts/B/doors.aeo"), "").unwrap();

        let index = ProjectScriptIndex::build(&root).unwrap();

        let matches = index.find_modules_named("doors.aeo");

        assert_eq!(matches.len(), 2);

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_project_index_scoped_lookup() {
        let root = test_root("scope");

        fs::create_dir_all(root.join("scripts/Dungeon/systems")).unwrap();

        fs::write(root.join("scripts/Dungeon/main.aeo"), "").unwrap();
        fs::write(root.join("scripts/Dungeon/systems/doors.aeo"), "").unwrap();

        let index = ProjectScriptIndex::build(&root).unwrap();

        let matches =
            index.find_modules_named_in_scope(ScriptRoot::Scripts, "Dungeon/systems", "doors");

        assert_eq!(matches.len(), 1);
        assert_eq!(
            matches[0].relative_path,
            PathBuf::from("Dungeon/systems/doors.aeo")
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_project_index_absolute_path() {
        let root = test_root("absolute");

        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::write(root.join("scripts/main.aeo"), "").unwrap();

        let index = ProjectScriptIndex::build(&root).unwrap();
        let module = index.find_module("scripts/main.aeo").unwrap();

        assert_eq!(index.absolute_path(module), root.join("scripts/main.aeo"));

        fs::remove_dir_all(root).ok();
    }
}
