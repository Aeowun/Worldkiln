use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScriptTreeNodeKind {
    Directory,
    Module,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptTreeNode {
    pub name: String,
    pub relative_path: PathBuf,
    pub kind: ScriptTreeNodeKind,
    pub children: Vec<ScriptTreeNode>,
}

impl ScriptTreeNode {
    fn directory(name: String, relative_path: PathBuf, children: Vec<Self>) -> Self {
        Self {
            name,
            relative_path,
            kind: ScriptTreeNodeKind::Directory,
            children,
        }
    }

    fn module(name: String, relative_path: PathBuf) -> Self {
        Self {
            name,
            relative_path,
            kind: ScriptTreeNodeKind::Module,
            children: Vec::new(),
        }
    }

    fn collect_modules(&self, output: &mut Vec<PathBuf>) {
        if self.kind == ScriptTreeNodeKind::Module {
            output.push(self.relative_path.clone());
        }

        for child in &self.children {
            child.collect_modules(output);
        }
    }

    fn find_module(&self, normalized_path: &str) -> Option<PathBuf> {
        if self.kind == ScriptTreeNodeKind::Module
            && normalize_path(&self.relative_path) == normalized_path
        {
            return Some(self.relative_path.clone());
        }

        for child in &self.children {
            if let Some(path) = child.find_module(normalized_path) {
                return Some(path);
            }
        }

        None
    }

    fn find_directory(&self, normalized_path: &str) -> Option<&Self> {
        if self.kind == ScriptTreeNodeKind::Directory
            && normalize_path(&self.relative_path) == normalized_path
        {
            return Some(self);
        }

        for child in &self.children {
            if let Some(directory) = child.find_directory(normalized_path) {
                return Some(directory);
            }
        }

        None
    }

    fn index(
        &self,
        path_index: &mut HashMap<String, PathBuf>,
        name_index: &mut HashMap<String, Vec<PathBuf>>,
        scoped_name_index: &mut HashMap<String, Vec<PathBuf>>,
    ) {
        let normalized_path = normalize_path(&self.relative_path);

        match self.kind {
            ScriptTreeNodeKind::Directory => {
                path_index.insert(normalized_path, self.relative_path.clone());

                for child in &self.children {
                    child.index(path_index, name_index, scoped_name_index);
                }
            }

            ScriptTreeNodeKind::Module => {
                path_index.insert(normalized_path, self.relative_path.clone());

                let full_name = self.name.to_lowercase();

                name_index
                    .entry(full_name)
                    .or_default()
                    .push(self.relative_path.clone());

                if let Some(stem) = self
                    .relative_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                {
                    let stem = stem.to_lowercase();

                    name_index
                        .entry(stem)
                        .or_default()
                        .push(self.relative_path.clone());
                }

                let scope = self
                    .relative_path
                    .parent()
                    .map(normalize_path)
                    .unwrap_or_default();

                let scope_key = scoped_name_key(&scope, &self.name);

                scoped_name_index
                    .entry(scope_key)
                    .or_default()
                    .push(self.relative_path.clone());

                if let Some(stem) = self
                    .relative_path
                    .file_stem()
                    .and_then(|value| value.to_str())
                {
                    let scope_key = scoped_name_key(&scope, stem);

                    scoped_name_index
                        .entry(scope_key)
                        .or_default()
                        .push(self.relative_path.clone());
                }
            }
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ScriptTree {
    pub root: PathBuf,
    pub children: Vec<ScriptTreeNode>,

    /// Normalized relative path -> node path.
    path_index: HashMap<String, PathBuf>,

    /// Lowercase module filename/stem -> matching module paths.
    name_index: HashMap<String, Vec<PathBuf>>,

    /// Normalized relative directory + module name -> matching module paths.
    scoped_name_index: HashMap<String, Vec<PathBuf>>,
}

impl ScriptTree {
    pub fn build(root: &Path) -> io::Result<Self> {
        if !root.exists() {
            return Ok(Self {
                root: root.to_path_buf(),
                children: Vec::new(),
                path_index: HashMap::new(),
                name_index: HashMap::new(),
                scoped_name_index: HashMap::new(),
            });
        }

        if !root.is_dir() {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                format!("Script tree root is not a directory: {}", root.display()),
            ));
        }

        let children = read_directory(root, PathBuf::new())?;

        let mut tree = Self {
            root: root.to_path_buf(),
            children,
            path_index: HashMap::new(),
            name_index: HashMap::new(),
            scoped_name_index: HashMap::new(),
        };

        for child in &tree.children {
            child.index(
                &mut tree.path_index,
                &mut tree.name_index,
                &mut tree.scoped_name_index,
            );
        }

        for paths in tree.name_index.values_mut() {
            paths.sort();
            paths.dedup();
        }

        for paths in tree.scoped_name_index.values_mut() {
            paths.sort();
            paths.dedup();
        }

        Ok(tree)
    }

    pub fn module_paths(&self) -> Vec<PathBuf> {
        let mut output = Vec::new();

        for child in &self.children {
            child.collect_modules(&mut output);
        }

        output.sort();
        output
    }

    pub fn find_module(&self, path: &str) -> Option<PathBuf> {
        let normalized = normalize_path_str(path.trim_matches('/'));

        if normalized.is_empty() {
            return None;
        }

        self.path_index.get(&normalized).cloned()
    }

    pub fn find_modules_named(&self, name: &str) -> Vec<PathBuf> {
        let normalized_name = normalize_module_name(name);

        if normalized_name.is_empty() {
            return Vec::new();
        }

        self.name_index
            .get(&normalized_name)
            .cloned()
            .unwrap_or_default()
    }

    pub fn find_modules_in_scope(&self, scope: &str, name: &str) -> Vec<PathBuf> {
        let normalized_scope = normalize_path_str(scope.trim_matches('/'));
        let key = scoped_name_key(&normalized_scope, name);

        self.scoped_name_index
            .get(&key)
            .cloned()
            .unwrap_or_default()
    }

    pub fn descendant_module_paths(&self, directory: &str) -> Vec<PathBuf> {
        let normalized = normalize_path_str(directory.trim_matches('/'));

        if normalized.is_empty() {
            return self.module_paths();
        }

        for child in &self.children {
            if let Some(node) = child.find_directory(&normalized) {
                let mut output = Vec::new();
                node.collect_modules(&mut output);
                output.sort();
                return output;
            }
        }

        Vec::new()
    }

    pub fn contains_path(&self, path: &str) -> bool {
        let normalized = normalize_path_str(path.trim_matches('/'));

        !normalized.is_empty() && self.path_index.contains_key(&normalized)
    }

    pub fn path_index(&self) -> &HashMap<String, PathBuf> {
        &self.path_index
    }

    pub fn name_index(&self) -> &HashMap<String, Vec<PathBuf>> {
        &self.name_index
    }

    pub fn scoped_name_index(&self) -> &HashMap<String, Vec<PathBuf>> {
        &self.scoped_name_index
    }
}

fn read_directory(root: &Path, relative_root: PathBuf) -> io::Result<Vec<ScriptTreeNode>> {
    let mut entries = fs::read_dir(root)?
        .flatten()
        .map(|entry| entry.path())
        .collect::<Vec<_>>();

    entries.sort_by(|left, right| {
        let left_name = left
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        let right_name = right
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("");

        left_name
            .cmp(right_name)
            .then_with(|| left.is_file().cmp(&right.is_file()))
    });

    let mut nodes = Vec::new();

    for path in entries {
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or_default()
            .to_string();

        let relative_path = relative_root.join(&name);

        if path.is_dir() {
            nodes.push(ScriptTreeNode::directory(
                name,
                relative_path.clone(),
                read_directory(&path, relative_path)?,
            ));

            continue;
        }

        if path.is_file() && path.extension().is_some_and(|extension| extension == "aeo") {
            nodes.push(ScriptTreeNode::module(name, relative_path));
        }
    }

    Ok(nodes)
}

fn normalize_path(path: &Path) -> String {
    normalize_path_str(&path.to_string_lossy())
}

fn normalize_path_str(path: &str) -> String {
    path.replace('\\', "/").trim_matches('/').to_lowercase()
}

fn normalize_module_name(name: &str) -> String {
    let trimmed = name.trim();

    if trimmed.is_empty() {
        return String::new();
    }

    let normalized = trimmed
        .replace('\\', "/")
        .rsplit('/')
        .next()
        .unwrap_or(trimmed)
        .to_lowercase();

    normalized
}

fn scoped_name_key(scope: &str, name: &str) -> String {
    format!(
        "{}\0{}",
        normalize_path_str(scope),
        normalize_module_name(name)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_root(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "worldkiln_script_tree_{}_{}_{}",
            name,
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn test_script_tree_discovers_nested_modules() {
        let root = test_root("nested");

        fs::create_dir_all(root.join("Dungeon/systems")).unwrap();
        fs::write(root.join("main.aeo"), "").unwrap();
        fs::write(root.join("Dungeon/dungeon_main.aeo"), "").unwrap();
        fs::write(root.join("Dungeon/systems/doors.aeo"), "").unwrap();
        fs::write(root.join("Dungeon/ignored.txt"), "").unwrap();

        let tree = ScriptTree::build(&root).unwrap();

        assert_eq!(
            tree.module_paths(),
            vec![
                PathBuf::from("Dungeon/dungeon_main.aeo"),
                PathBuf::from("Dungeon/systems/doors.aeo"),
                PathBuf::from("main.aeo"),
            ]
        );

        assert_eq!(
            tree.find_module(r"Dungeon\systems\doors.aeo"),
            Some(PathBuf::from("Dungeon/systems/doors.aeo"))
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_script_tree_builds_path_index() {
        let root = test_root("path_index");

        fs::create_dir_all(root.join("Dungeon")).unwrap();
        fs::write(root.join("Dungeon/dungeon_main.aeo"), "").unwrap();

        let tree = ScriptTree::build(&root).unwrap();

        assert!(tree.contains_path("Dungeon/dungeon_main.aeo"));
        assert!(tree.contains_path(r"Dungeon\dungeon_main.aeo"));
        assert!(!tree.contains_path("missing.aeo"));

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_script_tree_builds_name_index() {
        let root = test_root("name_index");

        fs::create_dir_all(root.join("Dungeon")).unwrap();
        fs::create_dir_all(root.join("Other")).unwrap();

        fs::write(root.join("Dungeon/doors.aeo"), "").unwrap();
        fs::write(root.join("Other/doors.aeo"), "").unwrap();

        let tree = ScriptTree::build(&root).unwrap();

        assert_eq!(
            tree.find_modules_named("doors"),
            vec![
                PathBuf::from("Dungeon/doors.aeo"),
                PathBuf::from("Other/doors.aeo"),
            ]
        );

        assert_eq!(
            tree.find_modules_named("doors.aeo"),
            vec![
                PathBuf::from("Dungeon/doors.aeo"),
                PathBuf::from("Other/doors.aeo"),
            ]
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_script_tree_scoped_name_lookup() {
        let root = test_root("scoped");

        fs::create_dir_all(root.join("Dungeon/systems")).unwrap();
        fs::write(root.join("Dungeon/doors.aeo"), "").unwrap();
        fs::write(root.join("Dungeon/systems/doors.aeo"), "").unwrap();

        let tree = ScriptTree::build(&root).unwrap();

        assert_eq!(
            tree.find_modules_in_scope("Dungeon", "doors"),
            vec![PathBuf::from("Dungeon/doors.aeo")]
        );

        assert_eq!(
            tree.find_modules_in_scope("Dungeon/systems", "doors"),
            vec![PathBuf::from("Dungeon/systems/doors.aeo")]
        );

        fs::remove_dir_all(root).ok();
    }

    #[test]
    fn test_script_tree_descendant_enumeration() {
        let root = test_root("descendants");

        fs::create_dir_all(root.join("Dungeon/systems")).unwrap();
        fs::write(root.join("Dungeon/main.aeo"), "").unwrap();
        fs::write(root.join("Dungeon/systems/doors.aeo"), "").unwrap();
        fs::write(root.join("root.aeo"), "").unwrap();

        let tree = ScriptTree::build(&root).unwrap();

        assert_eq!(
            tree.descendant_module_paths("Dungeon"),
            vec![
                PathBuf::from("Dungeon/main.aeo"),
                PathBuf::from("Dungeon/systems/doors.aeo"),
            ]
        );

        fs::remove_dir_all(root).ok();
    }
}
