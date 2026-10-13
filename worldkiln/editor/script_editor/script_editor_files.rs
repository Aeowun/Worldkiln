use super::{ScriptDocument, ScriptEditor, SearchResult};
use crate::project::{ProjectScriptIndex, ScriptRoot, ScriptTree};
use crate::world::World;
use std::path::{Path, PathBuf};

impl ScriptEditor {
    pub fn refresh_scripts(&mut self, project_path: &Option<PathBuf>) {
        self.scripts_list.clear();
        // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:52 -05:00
        self.project_problems.clear();
        self.playerscripts_list.clear();

        self.scripts_tree = None;
        self.playerscripts_tree = None;
        self.project_script_index = None;

        let Some(root) = project_path else {
            return;
        };

        match ProjectScriptIndex::build(root) {
            Ok(index) => {
                self.scripts_list = index
                    .tree(ScriptRoot::Scripts)
                    .module_paths()
                    .into_iter()
                    .map(|relative| root.join("scripts").join(relative))
                    .collect();

                self.playerscripts_list = index
                    .tree(ScriptRoot::PlayerScripts)
                    .module_paths()
                    .into_iter()
                    .map(|relative| root.join("playerscripts").join(relative))
                    .collect();

                self.scripts_tree = Some(index.scripts.clone());
                self.playerscripts_tree = Some(index.playerscripts.clone());

                self.scripts_list.sort();
                self.playerscripts_list.sort();

                self.project_script_index = Some(index);
            }

            Err(error) => {
                self.output_log
                    .push(format!("Failed to build project script index: {}", error));

                // Keep the editor usable even when one root is malformed.
                let scripts_dir = root.join("scripts");
                let playerscripts_dir = root.join("playerscripts");

                match ScriptTree::build(&scripts_dir) {
                    Ok(tree) => {
                        self.scripts_list = tree
                            .module_paths()
                            .into_iter()
                            .map(|relative| scripts_dir.join(relative))
                            .collect();

                        self.scripts_tree = Some(tree);
                    }

                    Err(tree_error) => {
                        self.push_output(format!(
                            "Failed to index scripts directory {:?}: {}",
                            scripts_dir, tree_error
                        ));
                    }
                }

                match ScriptTree::build(&playerscripts_dir) {
                    Ok(tree) => {
                        self.playerscripts_list = tree
                            .module_paths()
                            .into_iter()
                            .map(|relative| playerscripts_dir.join(relative))
                            .collect();

                        self.playerscripts_tree = Some(tree);
                    }

                    Err(tree_error) => {
                        self.push_output(format!(
                            "Failed to index playerscripts directory {:?}: {}",
                            playerscripts_dir, tree_error
                        ));
                    }
                }

                self.scripts_list.sort();
                self.playerscripts_list.sort();
            }
        }

        // Worldkiln 0.8.7 | WO 0.8.7-18 | 2026-10-06 02:52 -05:00
        self.refresh_project_problems(Some(root));
    }

    pub fn open_file(&mut self, path: PathBuf) {
        if self.open_documents.contains_key(&path) {
            self.active_document = Some(path);
            return;
        }

        match std::fs::read_to_string(&path) {
            Ok(source) => {
                let document = ScriptDocument::new(path.clone(), source);

                self.open_documents.insert(path.clone(), document);
                self.active_document = Some(path);
            }

            Err(error) => {
                self.output_log
                    .push(format!("Failed to open {:?}: {}", path, error));
            }
        }
    }

    pub fn close_document(&mut self, path: &PathBuf) -> bool {
        if self
            .open_documents
            .get(path)
            .is_some_and(|document| document.dirty)
        {
            return false;
        }

        self.open_documents.remove(path);

        if self.active_document.as_ref() == Some(path) {
            self.active_document = self.next_open_document();
        }

        true
    }

    pub fn save_active(&mut self) {
        let Some(path) = self.active_document.clone() else {
            return;
        };

        let Some(document) = self.open_documents.get_mut(&path) else {
            return;
        };

        if let Err(error) = document.save() {
            self.output_log
                .push(format!("Failed to save {:?}: {}", path, error));
        }
    }

    pub fn save_all(&mut self) {
        let _ = self.save_all_checked();
    }

    // Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:34 -05:00
    // Worldkiln 0.8.7 | WO 0.8.7-31 | 2026-10-07 16:49 -05:00
    pub fn save_all_checked(&mut self) -> Result<(), String> {
        let mut failures = Vec::new();

        for document in self.open_documents.values_mut() {
            if !document.dirty {
                continue;
            }

            if let Err(error) = document.save() {
                failures.push(format!("Failed to save {:?}: {}", document.path, error));
            }
        }

        // The values_mut() borrow must end before push_output() mutably borrows self.
        for message in &failures {
            self.push_output(message.clone());
        }

        if failures.is_empty() {
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    }

    pub fn create_new_script(&mut self, project_path: &PathBuf, name: &str) {
        self.create_new_script_in_root(project_path, ScriptRoot::Scripts, name);
    }

    pub fn create_new_script_in_root(
        &mut self,
        project_path: &PathBuf,
        script_root: ScriptRoot,
        name: &str,
    ) -> Option<(PathBuf, String)> {
        let relative_path = build_script_relative_path(name)?;

        let root_dir = project_path.join(script_root.directory_name());
        let parent_dir = relative_path
            .parent()
            .map(|parent| root_dir.join(parent))
            .unwrap_or_else(|| root_dir.clone());

        if let Err(error) = std::fs::create_dir_all(&parent_dir) {
            self.push_output(format!(
                "Failed to create script directory {:?}: {}",
                parent_dir, error
            ));
            return None;
        }

        let path = root_dir.join(&relative_path);

        if path.exists() {
            self.output_log
                .push(format!("File already exists: {:?}", path));
            return None;
        }

        let logical_path = format!(
            "{}/{}",
            script_root.directory_name(),
            relative_path.to_string_lossy().replace('\\', "/")
        );

        // Worldkiln 0.8.5 | WO 0.8.7-79 | 2026-10-10 00:14 -05:00
        // Manual and agent creation use the same collision-safe editor operation.
        match std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => {
                drop(file);
                self.refresh_scripts(&Some(project_path.clone()));
                self.open_file(path.clone());
                Some((path, logical_path))
            }

            Err(error) => {
                self.output_log
                    .push(format!("Failed to create script: {}", error));
                None
            }
        }
    }

    pub fn duplicate_script(
        &mut self,
        project_path: &PathBuf,
        source_path: PathBuf,
    ) -> Option<(PathBuf, String)> {
        if script_root_for_path(project_path, &source_path).is_none() {
            self.output_log
                .push("Duplicate failed: The target is not a project script file.".to_string());
            return None;
        }

        if !source_path.exists() {
            self.push_output(format!(
                "Duplicate failed: {:?} does not exist.",
                source_path
            ));
            return None;
        }

        let Some(parent) = source_path.parent() else {
            self.output_log
                .push("Duplicate failed: Script has no parent directory.".to_string());
            return None;
        };

        let Some(stem) = source_path.file_stem().and_then(|stem| stem.to_str()) else {
            self.output_log
                .push("Duplicate failed: Script filename is invalid.".to_string());
            return None;
        };

        let mut copy_number = 1usize;

        let target_path = loop {
            let filename = if copy_number == 1 {
                format!("{}_copy.aeo", stem)
            } else {
                format!("{}_copy_{}.aeo", stem, copy_number)
            };

            let candidate = parent.join(filename);

            if !candidate.exists() {
                break candidate;
            }

            let Some(next) = copy_number.checked_add(1) else {
                self.output_log
                    .push("Duplicate failed: Copy name space exhausted.".to_string());
                return None;
            };

            copy_number = next;
        };

        let Some(logical_path) = project_logical_script_path(project_path, &target_path) else {
            self.output_log
                .push("Duplicate failed: Invalid project script path.".to_string());
            return None;
        };

        // If the source is already open, duplicate what the user currently sees,
        // including unsaved edits. Otherwise read the source from disk.
        let source = if let Some(document) = self.open_documents.get(&source_path) {
            document.source.clone()
        } else {
            match std::fs::read_to_string(&source_path) {
                Ok(source) => source,

                Err(error) => {
                    self.push_output(format!("Duplicate failed: {}", error));
                    return None;
                }
            }
        };

        if let Err(error) = std::fs::write(&target_path, source) {
            self.push_output(format!("Duplicate failed: {}", error));
            return None;
        }

        self.refresh_scripts(&Some(project_path.clone()));

        self.push_output(format!(
            "Duplicated {:?} as {:?}",
            source_path.file_name().unwrap_or_default(),
            target_path.file_name().unwrap_or_default()
        ));

        Some((target_path, logical_path))
    }

    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    // Player Scripts is an actual script root, not a separate binding list.
    pub fn move_script_root(
        &mut self,
        project_path: &PathBuf,
        old_path: PathBuf,
        destination_root: ScriptRoot,
    ) -> Result<(String, String), String> {
        let old_logical = project_logical_script_path(project_path, &old_path)
            .ok_or("Script must be inside this project's script roots")?;
        let old_canonical = old_path.canonicalize().map_err(|e| e.to_string())?;
        let source_root = old_path
            .parent()
            .and_then(|_| script_root_for_path(project_path, &old_path))
            .ok_or("Unrecognized script source root")?;
        if source_root == destination_root {
            return Err("Script is already under this script root".into());
        }
        let source_dir = project_path.join(source_root.directory_name());
        let source_dir = source_dir.canonicalize().map_err(|e| e.to_string())?;
        if !old_canonical.starts_with(&source_dir) {
            return Err("Script source resolves outside its project root".into());
        }
        let file = old_path.file_name().ok_or("Script has no file name")?;
        let destination_dir = project_path.join(destination_root.directory_name());
        std::fs::create_dir_all(&destination_dir).map_err(|e| e.to_string())?;
        let destination_dir = destination_dir.canonicalize().map_err(|e| e.to_string())?;
        let project_canonical = project_path.canonicalize().map_err(|e| e.to_string())?;
        if !destination_dir.starts_with(&project_canonical) {
            return Err("Destination resolves outside the project".into());
        }
        // Keep lexical project paths in editor document keys; canonical paths
        // are only for containment checks (Windows adds a \\?\ prefix).
        let destination = project_path
            .join(destination_root.directory_name())
            .join(file);
        if destination.exists() {
            return Err(format!(
                "Destination script '{}' already exists",
                destination.display()
            ));
        }
        let new_logical = project_logical_script_path(
            project_path,
            &project_path
                .join(destination_root.directory_name())
                .join(file),
        )
        .ok_or("Invalid destination script path")?;
        std::fs::rename(&old_path, &destination).map_err(|e| e.to_string())?;
        if let Some(mut doc) = self.open_documents.remove(&old_path) {
            doc.path = destination.clone();
            self.open_documents.insert(destination.clone(), doc);
        }
        if self.active_document.as_ref() == Some(&old_path) {
            self.active_document = Some(destination.clone());
        }
        self.rename_breakpoint_path(&old_logical, &new_logical);
        self.refresh_scripts(&Some(project_path.clone()));
        Ok((old_logical, new_logical))
    }

    pub fn rename_script(
        &mut self,
        project_path: &PathBuf,
        old_path: PathBuf,
        new_name: &str,
    ) -> Option<(String, String)> {
        let new_name = new_name.trim();

        if new_name.is_empty() {
            self.output_log
                .push("Rename failed: Name cannot be empty.".to_string());
            return None;
        }

        let mut filename = new_name.to_string();

        if !filename.ends_with(".aeo") {
            filename.push_str(".aeo");
        }

        if !valid_script_filename(&filename)
            || filename == ".aeo"
            || filename.contains('/')
            || filename.contains('\\')
        {
            self.push_output(
                "Rename failed: Invalid filename. Rename only changes the file name.".to_string(),
            );
            return None;
        }

        let Some(old_logical_path) = project_logical_script_path(project_path, &old_path) else {
            self.output_log
                .push("Rename failed: The target is not a project script file.".to_string());
            return None;
        };

        let Some(parent) = old_path.parent() else {
            self.output_log
                .push("Rename failed: Script has no parent directory.".to_string());
            return None;
        };

        let new_path = parent.join(&filename);

        if new_path == old_path {
            return None;
        }

        if new_path.exists() {
            self.output_log
                .push(format!("Rename failed: {:?} already exists.", new_path));
            return None;
        }

        let Some(new_logical_path) = project_logical_script_path(project_path, &new_path) else {
            self.output_log
                .push("Rename failed: Invalid project script path.".to_string());
            return None;
        };

        match std::fs::rename(&old_path, &new_path) {
            Ok(()) => {
                if let Some(mut document) = self.open_documents.remove(&old_path) {
                    document.path = new_path.clone();
                    self.open_documents.insert(new_path.clone(), document);
                }

                if self.active_document.as_ref() == Some(&old_path) {
                    self.active_document = Some(new_path);
                }

                self.refresh_scripts(&Some(project_path.clone()));

                // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:41 -05:00
                self.rename_breakpoint_path(&old_logical_path, &new_logical_path);

                self.push_output(format!("Renamed to {:?}", filename));

                Some((old_logical_path, new_logical_path))
            }

            Err(error) => {
                self.push_output(format!("Rename failed: {}", error));
                None
            }
        }
    }

    pub fn rename_script_with_bindings(
        &mut self,
        project_path: &PathBuf,
        old_path: PathBuf,
        new_name: &str,
        world: &mut World,
    ) -> bool {
        let Some((old_path, new_path)) = self.rename_script(project_path, old_path, new_name)
        else {
            return false;
        };

        let old_normalized = old_path.replace('\\', "/").to_lowercase();

        for binding in &mut world.script_bindings {
            if binding.normalized_script_path().to_lowercase() == old_normalized {
                binding.script_path = new_path.clone();
            }
        }

        for binding in &mut world.actor_script_bindings {
            if binding.normalized_script_path().to_lowercase() == old_normalized {
                binding.script_path = new_path.clone();
            }
        }

        for disabled in &mut world.disabled_scripts {
            if disabled.replace('\\', "/").to_lowercase() == old_normalized {
                *disabled = new_path.clone();
            }
        }

        world.rebuild_actor_module_index();

        true
    }

    pub fn delete_script(&mut self, project_path: &Option<PathBuf>, path: PathBuf) -> bool {
        let Some(root) = project_path.as_ref() else {
            self.output_log
                .push("Delete failed: No project is open.".to_string());
            return false;
        };

        if script_root_for_path(root, &path).is_none() {
            self.output_log
                .push("Delete failed: The target is not a project script file.".to_string());
            return false;
        }

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:41 -05:00
        let logical_path = project_logical_script_path(root, &path);

        match std::fs::remove_file(&path) {
            Ok(()) => {
                self.open_documents.remove(&path);

                if self.active_document.as_ref() == Some(&path) {
                    self.active_document = self.next_open_document();
                }

                self.refresh_scripts(project_path);

                // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:41 -05:00
                if let Some(logical_path) = logical_path.as_deref() {
                    self.remove_breakpoints_for_path(logical_path);
                }

                self.push_output(format!(
                    "Deleted {:?}",
                    path.file_name().unwrap_or_default()
                ));

                true
            }

            Err(error) => {
                self.push_output(format!("Delete failed: {}", error));
                false
            }
        }
    }

    pub fn delete_script_if_unbound(
        &mut self,
        project_path: &Option<PathBuf>,
        path: PathBuf,
        world: &World,
    ) -> bool {
        let Some(root) = project_path.as_ref() else {
            self.output_log
                .push("Delete failed: No project is open.".to_string());
            return false;
        };

        let Some(logical_path) = project_logical_script_path(root, &path) else {
            self.output_log
                .push("Delete failed: The target is not a project script file.".to_string());
            return false;
        };

        let normalized = logical_path.replace('\\', "/").to_lowercase();

        let cell_bound = world
            .script_bindings
            .iter()
            .any(|binding| binding.normalized_script_path().to_lowercase() == normalized);

        let actor_bound = world
            .actor_script_bindings
            .iter()
            .any(|binding| binding.normalized_script_path().to_lowercase() == normalized);

        if cell_bound || actor_bound {
            self.push_output(format!(
                "Delete blocked: '{}' is still attached to an authored object. Detach the Module first.",
                logical_path
            ));
            return false;
        }

        self.delete_script(project_path, path)
    }

    pub fn apply_dirty_response(
        &mut self,
        project_path: &Option<PathBuf>,
        path: PathBuf,
        save: bool,
        cancel: bool,
        world: &World,
    ) {
        let is_deletion = self.deleting_path.as_ref() == Some(&path);

        if cancel {
            self.closing_path = None;

            if is_deletion {
                self.deleting_path = None;
            }

            return;
        }

        if save {
            let Some(document) = self.open_documents.get_mut(&path) else {
                self.closing_path = None;

                if is_deletion {
                    self.deleting_path = None;
                }

                return;
            };

            if let Err(error) = document.save() {
                self.push_output(format!(
                    "Save failed: {}. {} aborted.",
                    error,
                    if is_deletion { "Deletion" } else { "Closure" }
                ));
                return;
            }
        }

        if is_deletion {
            if self.delete_script_if_unbound(project_path, path.clone(), world) {
                self.deleting_path = None;
            } else {
                return;
            }
        } else {
            self.open_documents.remove(&path);

            if self.active_document.as_ref() == Some(&path) {
                self.active_document = self.next_open_document();
            }
        }

        self.closing_path = None;
    }

    pub fn perform_search(&mut self) {
        self.search_results.clear();

        let query = self.search_query.trim();

        if query.is_empty() {
            return;
        }

        let paths = self.indexed_script_paths();

        for path in paths {
            let source = match self.open_documents.get(&path) {
                Some(document) => document.source.clone(),
                None => match std::fs::read_to_string(&path) {
                    Ok(source) => source,
                    Err(_) => continue,
                },
            };

            for (index, line) in source.lines().enumerate() {
                if line.contains(query) {
                    self.search_results.push(SearchResult {
                        path: path.clone(),
                        line: index + 1,
                        text: line.to_string(),
                    });
                }
            }
        }
    }
}

fn build_script_relative_path(name: &str) -> Option<PathBuf> {
    let normalized = name.trim().replace('\\', "/");

    if normalized.is_empty() || normalized.starts_with('/') || normalized.ends_with('/') {
        return None;
    }

    let mut components = normalized.split('/').collect::<Vec<_>>();

    if components
        .iter()
        .any(|component| component.is_empty() || *component == "." || *component == "..")
    {
        return None;
    }

    if components
        .iter()
        .any(|component| !valid_script_filename(component))
    {
        return None;
    }

    let mut filename = components.pop()?.to_string();

    if !filename.ends_with(".aeo") {
        filename.push_str(".aeo");
    }

    if filename == ".aeo" || !valid_script_filename(&filename) {
        return None;
    }

    let mut relative = PathBuf::new();

    for component in components {
        relative.push(component);
    }

    relative.push(filename);

    Some(relative)
}

pub(crate) fn script_root_for_path(project_root: &Path, path: &Path) -> Option<ScriptRoot> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("aeo") {
        return None;
    }

    for root in [ScriptRoot::Scripts, ScriptRoot::PlayerScripts] {
        let root_dir = project_root.join(root.directory_name());

        if path.strip_prefix(&root_dir).is_ok() {
            return Some(root);
        }
    }

    None
}

pub(crate) fn project_logical_script_path(project_root: &Path, path: &Path) -> Option<String> {
    let relative = path.strip_prefix(project_root).ok()?;

    if relative
        .extension()
        .and_then(|extension| extension.to_str())
        != Some("aeo")
    {
        return None;
    }

    let root = relative.components().next()?.as_os_str().to_string_lossy();

    if root != ScriptRoot::Scripts.directory_name()
        && root != ScriptRoot::PlayerScripts.directory_name()
    {
        return None;
    }

    Some(relative.to_string_lossy().replace('\\', "/"))
}

pub(crate) fn valid_script_filename(filename: &str) -> bool {
    filename.chars().all(|character| {
        !matches!(
            character,
            '\\' | '/' | ':' | '*' | '?' | '"' | '<' | '>' | '|'
        )
    })
}
