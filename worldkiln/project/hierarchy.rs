use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap, HashSet};

pub type ProjectNodeId = u64;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ProjectNodeKind {
    Folder {
        name: String,
    },
    Actor {
        actor_id: u64,
    },
    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    Cell {
        cell_id: u64,
        #[serde(default)]
        label: Option<String>,
    },
    Script {
        script_path: String,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectNode {
    pub id: ProjectNodeId,
    pub parent_id: Option<ProjectNodeId>,
    pub kind: ProjectNodeKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProjectHierarchy {
    pub next_node_id: ProjectNodeId,
    pub nodes: BTreeMap<ProjectNodeId, ProjectNode>,
}

impl Default for ProjectHierarchy {
    fn default() -> Self {
        Self::new()
    }
}

impl ProjectHierarchy {
    pub fn new() -> Self {
        Self {
            next_node_id: 1,
            nodes: BTreeMap::new(),
        }
    }

    pub fn node(&self, id: ProjectNodeId) -> Option<&ProjectNode> {
        self.nodes.get(&id)
    }

    pub fn node_mut(&mut self, id: ProjectNodeId) -> Option<&mut ProjectNode> {
        self.nodes.get_mut(&id)
    }

    pub fn contains(&self, id: ProjectNodeId) -> bool {
        self.nodes.contains_key(&id)
    }

    pub fn parent_of(&self, id: ProjectNodeId) -> Option<ProjectNodeId> {
        self.nodes.get(&id).and_then(|node| node.parent_id)
    }

    pub fn children_of(&self, parent_id: Option<ProjectNodeId>) -> Vec<ProjectNodeId> {
        let mut children: Vec<ProjectNodeId> = self
            .nodes
            .values()
            .filter(|node| node.parent_id == parent_id)
            .map(|node| node.id)
            .collect();

        children.sort_unstable();
        children
    }

    pub fn root_nodes(&self) -> Vec<ProjectNodeId> {
        self.children_of(None)
    }

    /// Returns every descendant below `parent_id` in deterministic hierarchy order.
    ///
    /// `None` represents the project root.
    pub fn descendants_of(&self, parent_id: Option<ProjectNodeId>) -> Vec<ProjectNodeId> {
        fn collect(
            hierarchy: &ProjectHierarchy,
            parent_id: Option<ProjectNodeId>,
            visited: &mut HashSet<ProjectNodeId>,
            output: &mut Vec<ProjectNodeId>,
        ) {
            for child_id in hierarchy.children_of(parent_id) {
                if !visited.insert(child_id) {
                    continue;
                }

                output.push(child_id);

                collect(hierarchy, Some(child_id), visited, output);
            }
        }

        let mut visited = HashSet::new();
        let mut output = Vec::new();

        collect(self, parent_id, &mut visited, &mut output);

        output
    }

    /// Recursively finds authored Actors by name below a project scope.
    ///
    /// Actor names are supplied separately because ProjectHierarchy stores
    /// persistent Actor IDs rather than duplicating authored Actor identity.
    pub fn find_actor_ids_named_recursive(
        &self,
        parent_id: Option<ProjectNodeId>,
        actor_names: &HashMap<u64, String>,
        name: &str,
    ) -> Vec<u64> {
        self.descendants_of(parent_id)
            .into_iter()
            .filter_map(|node_id| {
                let node = self.node(node_id)?;

                let ProjectNodeKind::Actor { actor_id } = node.kind else {
                    return None;
                };

                let actor_name = actor_names.get(&actor_id)?;

                (actor_name == name).then_some(actor_id)
            })
            .collect()
    }

    /// Finds direct project children whose public project name matches `name`.
    ///
    /// Only Folders and Actors participate in `proj` hierarchy paths.
    /// Script files remain under the `scripts` / `playerscripts` roots.
    pub fn find_direct_named_children(
        &self,
        parent_id: Option<ProjectNodeId>,
        actor_names: &HashMap<u64, String>,
        name: &str,
    ) -> Vec<ProjectNodeId> {
        self.children_of(parent_id)
            .into_iter()
            .filter(|node_id| {
                let Some(node) = self.node(*node_id) else {
                    return false;
                };

                match &node.kind {
                    ProjectNodeKind::Folder { name: folder_name } => folder_name == name,

                    ProjectNodeKind::Actor { actor_id } => actor_names
                        .get(actor_id)
                        .is_some_and(|actor_name| actor_name == name),

                    ProjectNodeKind::Script { .. } | ProjectNodeKind::Cell { .. } => false,
                }
            })
            .collect()
    }

    pub fn is_descendant(&self, ancestor_id: ProjectNodeId, target_id: ProjectNodeId) -> bool {
        let mut current = self.parent_of(target_id);
        let mut visited = HashSet::new();

        while let Some(parent_id) = current {
            if !visited.insert(parent_id) {
                return false;
            }

            if parent_id == ancestor_id {
                return true;
            }

            current = self.parent_of(parent_id);
        }

        false
    }

    fn direct_parent_accepts(parent_kind: &ProjectNodeKind, child_kind: &ProjectNodeKind) -> bool {
        match (parent_kind, child_kind) {
            (ProjectNodeKind::Folder { .. }, _) => true,

            // Actors may organize other Actors, Folders, and Scripts.
            (ProjectNodeKind::Actor { .. }, _) => true,

            // Scripts may contain folders and other scripts, but never actors.
            (
                ProjectNodeKind::Script { .. },
                ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. },
            ) => false,
            (ProjectNodeKind::Script { .. }, _) => true,
            (ProjectNodeKind::Cell { .. }, _) => false,
        }
    }

    /// Returns whether `parent_id` may contain a node of `child_kind`.
    ///
    /// In addition to the direct parent rule, this enforces the semantic
    /// hierarchy rule that an Actor may not exist anywhere beneath a Script.
    pub fn can_parent(
        &self,
        parent_id: Option<ProjectNodeId>,
        child_kind: &ProjectNodeKind,
    ) -> bool {
        let Some(parent_id) = parent_id else {
            return true;
        };

        let Some(parent) = self.node(parent_id) else {
            return false;
        };

        if !Self::direct_parent_accepts(&parent.kind, child_kind) {
            return false;
        }

        if matches!(
            child_kind,
            ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. }
        ) {
            let mut current = Some(parent_id);
            let mut visited = HashSet::new();

            while let Some(id) = current {
                if !visited.insert(id) {
                    return false;
                }

                let Some(node) = self.node(id) else {
                    return false;
                };

                if matches!(node.kind, ProjectNodeKind::Script { .. }) {
                    return false;
                }

                current = node.parent_id;
            }
        }

        true
    }

    /// Finds the nearest ancestor that is not a Folder.
    pub fn nearest_non_folder_ancestor(&self, id: ProjectNodeId) -> Option<&ProjectNode> {
        let mut current = self.parent_of(id);
        let mut visited = HashSet::new();

        while let Some(parent_id) = current {
            if !visited.insert(parent_id) {
                return None;
            }

            let node = self.node(parent_id)?;

            if !matches!(node.kind, ProjectNodeKind::Folder { .. }) {
                return Some(node);
            }

            current = node.parent_id;
        }

        None
    }

    /// Determines Actor attachment for an existing Script node.
    ///
    /// A Script attaches to the nearest non-Folder ancestor when that ancestor
    /// is an Actor. A Script beneath another Script therefore does not attach
    /// separately to the Actor.
    pub fn attached_actor_id(&self, script_node_id: ProjectNodeId) -> Option<u64> {
        let node = self.node(script_node_id)?;

        if !matches!(node.kind, ProjectNodeKind::Script { .. }) {
            return None;
        }

        match self.nearest_non_folder_ancestor(script_node_id)?.kind {
            ProjectNodeKind::Actor { actor_id } => Some(actor_id),
            _ => None,
        }
    }

    /// Determines Actor attachment if a Script were placed under `parent_id`.
    fn attached_actor_id_for_parent(&self, parent_id: Option<ProjectNodeId>) -> Option<u64> {
        let mut current = parent_id;
        let mut visited = HashSet::new();

        while let Some(id) = current {
            if !visited.insert(id) {
                return None;
            }

            let node = self.node(id)?;

            match &node.kind {
                ProjectNodeKind::Folder { .. } => {
                    current = node.parent_id;
                }

                ProjectNodeKind::Actor { actor_id } => {
                    return Some(*actor_id);
                }

                ProjectNodeKind::Script { .. } | ProjectNodeKind::Cell { .. } => {
                    return None;
                }
            }
        }

        None
    }

    fn validate_folder_name(name: &str) -> Result<String, String> {
        let name = name.trim();

        if name.is_empty() {
            return Err("Folder name cannot be empty".to_string());
        }

        if name.contains('/') || name.contains('\\') {
            return Err("Folder name cannot contain '/' or '\\'".to_string());
        }

        Ok(name.to_string())
    }

    fn normalize_script_path(path: &str) -> Result<String, String> {
        let path = path.trim().replace('\\', "/");

        if path.is_empty() {
            return Err("Script path cannot be empty".to_string());
        }

        if path.starts_with('/') || path.contains(':') {
            return Err("Script path must be project-relative".to_string());
        }

        let parts: Vec<&str> = path.split('/').collect();

        if parts.len() < 2 {
            return Err("Script path must begin with 'scripts/' or 'playerscripts/'".to_string());
        }

        if parts[0] != "scripts" && parts[0] != "playerscripts" {
            return Err("Script path must begin with 'scripts/' or 'playerscripts/'".to_string());
        }

        if parts
            .iter()
            .any(|part| part.is_empty() || *part == "." || *part == "..")
        {
            return Err("Script path contains an invalid path component".to_string());
        }

        if !path.to_ascii_lowercase().ends_with(".aeo") {
            return Err("Script path must reference an .aeo file".to_string());
        }

        Ok(path)
    }

    fn allocate_node_id(&mut self) -> Result<ProjectNodeId, String> {
        let id = self.next_node_id;

        if id == 0 {
            return Err("Project node ID allocation reached zero".to_string());
        }

        self.next_node_id = id
            .checked_add(1)
            .ok_or_else(|| "Project node ID space exhausted".to_string())?;

        Ok(id)
    }

    pub fn create_folder(
        &mut self,
        parent_id: Option<ProjectNodeId>,
        name: &str,
    ) -> Result<ProjectNodeId, String> {
        let name = Self::validate_folder_name(name)?;

        let kind = ProjectNodeKind::Folder { name };

        if !self.can_parent(parent_id, &kind) {
            return Err("Invalid parent for Folder".to_string());
        }

        let id = self.allocate_node_id()?;

        self.nodes.insert(
            id,
            ProjectNode {
                id,
                parent_id,
                kind,
            },
        );

        Ok(id)
    }

    pub fn create_actor_node(
        &mut self,
        parent_id: Option<ProjectNodeId>,
        actor_id: u64,
    ) -> Result<ProjectNodeId, String> {
        if self.find_actor_node(actor_id).is_some() {
            return Err(format!(
                "Actor {} already has a Project Explorer node",
                actor_id
            ));
        }

        let kind = ProjectNodeKind::Actor { actor_id };

        if !self.can_parent(parent_id, &kind) {
            return Err("Invalid parent for Actor".to_string());
        }

        let id = self.allocate_node_id()?;

        self.nodes.insert(
            id,
            ProjectNode {
                id,
                parent_id,
                kind,
            },
        );

        Ok(id)
    }

    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    pub fn find_cell_node(&self, cell_id: u64) -> Option<ProjectNodeId> {
        self.nodes.values().find_map(|node| match node.kind {
            ProjectNodeKind::Cell { cell_id: id, .. } if id == cell_id => Some(node.id),
            _ => None,
        })
    }
    pub fn create_cell_node(
        &mut self,
        parent_id: Option<ProjectNodeId>,
        cell_id: u64,
    ) -> Result<ProjectNodeId, String> {
        if cell_id == 0 || self.find_cell_node(cell_id).is_some() {
            return Err(format!(
                "Invalid or duplicate Cell Explorer reference {cell_id}"
            ));
        }
        let kind = ProjectNodeKind::Cell {
            cell_id,
            label: None,
        };
        if !self.can_parent(parent_id, &kind) {
            return Err("Invalid parent for Cell".into());
        }
        let id = self.allocate_node_id()?;
        self.nodes.insert(
            id,
            ProjectNode {
                id,
                parent_id,
                kind,
            },
        );
        Ok(id)
    }
    pub fn rename_cell_node(&mut self, id: ProjectNodeId, name: &str) -> Result<(), String> {
        let name = name.trim();
        if name.is_empty() || name.contains('/') || name.contains('\\') {
            return Err("Invalid Cell display name".into());
        }
        match self.node_mut(id).map(|node| &mut node.kind) {
            Some(ProjectNodeKind::Cell { label, .. }) => {
                *label = Some(name.into());
                Ok(())
            }
            _ => Err("Target is not a Cell".into()),
        }
    }

    pub fn create_script_node(
        &mut self,
        parent_id: Option<ProjectNodeId>,
        script_path: &str,
    ) -> Result<ProjectNodeId, String> {
        let script_path = Self::normalize_script_path(script_path)?;

        let kind = ProjectNodeKind::Script {
            script_path: script_path.clone(),
        };

        if !self.can_parent(parent_id, &kind) {
            return Err("Invalid parent for Script".to_string());
        }

        if let Some(actor_id) = self.attached_actor_id_for_parent(parent_id) {
            let duplicate = self.nodes.values().any(|node| {
                let same_script = matches!(
                    &node.kind,
                    ProjectNodeKind::Script { script_path: existing_path }
                        if existing_path == &script_path
                );

                same_script && self.attached_actor_id(node.id) == Some(actor_id)
            });

            if duplicate {
                return Err(format!(
                    "Script '{}' is already attached to Actor {}",
                    script_path, actor_id
                ));
            }
        }

        let id = self.allocate_node_id()?;

        self.nodes.insert(
            id,
            ProjectNode {
                id,
                parent_id,
                kind,
            },
        );

        Ok(id)
    }

    pub fn move_node(
        &mut self,
        id: ProjectNodeId,
        new_parent_id: Option<ProjectNodeId>,
    ) -> Result<(), String> {
        let node = self
            .node(id)
            .cloned()
            .ok_or_else(|| "Node not found".to_string())?;

        if let Some(parent_id) = new_parent_id {
            if parent_id == id {
                return Err("Cannot parent node to itself".to_string());
            }

            if !self.contains(parent_id) {
                return Err("Target parent node not found".to_string());
            }

            if self.is_descendant(id, parent_id) {
                return Err("Cannot move node into its own descendant".to_string());
            }
        }

        if !self.can_parent(new_parent_id, &node.kind) {
            return Err("Target parent cannot accept this node type".to_string());
        }

        if let ProjectNodeKind::Script { script_path } = &node.kind {
            if let Some(actor_id) = self.attached_actor_id_for_parent(new_parent_id) {
                let duplicate = self.nodes.values().any(|other| {
                    if other.id == id {
                        return false;
                    }

                    let same_script = matches!(
                        &other.kind,
                        ProjectNodeKind::Script {
                            script_path: existing_path
                        } if existing_path == script_path
                    );

                    same_script && self.attached_actor_id(other.id) == Some(actor_id)
                });

                if duplicate {
                    return Err(format!(
                        "Script '{}' is already attached to Actor {}",
                        script_path, actor_id
                    ));
                }
            }
        }

        let old_parent_id = node.parent_id;
        self.node_mut(id)
            .expect("node existence checked above")
            .parent_id = new_parent_id;

        if let Err(error) = self.validate() {
            self.node_mut(id)
                .expect("node existence checked above")
                .parent_id = old_parent_id;
            return Err(error);
        }

        Ok(())
    }

    /// Moves a set of nodes as one transaction.
    ///
    /// Validation is performed against a cloned hierarchy and the original is
    /// only replaced after every move and the resulting hierarchy validate.
    pub fn move_nodes_atomic(
        &mut self,
        ids: &[ProjectNodeId],
        new_parent_id: Option<ProjectNodeId>,
    ) -> Result<(), String> {
        let mut unique = Vec::new();
        let mut seen = HashSet::new();

        for &id in ids {
            if seen.insert(id) {
                unique.push(id);
            }
        }

        if unique.is_empty() {
            return Err("No project nodes selected".to_string());
        }

        if let Some(parent_id) = new_parent_id {
            if unique.contains(&parent_id) {
                return Err("Cannot move a selection into itself".to_string());
            }
        }

        for (index, &left) in unique.iter().enumerate() {
            for &right in &unique[index + 1..] {
                if self.is_descendant(left, right) || self.is_descendant(right, left) {
                    return Err(
                        "Cannot move an ancestor and its descendant in one selection".to_string(),
                    );
                }
            }
        }

        let mut candidate = self.clone();
        for &id in &unique {
            candidate.move_node(id, new_parent_id)?;
        }
        candidate.validate()?;

        *self = candidate;
        Ok(())
    }

    /// Duplicates a Script reference or a Folder subtree.
    ///
    /// Actor nodes remain unique authored references, so attempting to
    /// duplicate one (including inside a Folder subtree) is rejected.
    pub fn duplicate_node(
        &mut self,
        id: ProjectNodeId,
        new_parent_id: Option<ProjectNodeId>,
    ) -> Result<ProjectNodeId, String> {
        let source = self
            .node(id)
            .cloned()
            .ok_or_else(|| "Node not found".to_string())?;

        if self.subtree_contains_script(id) {
            return Err(
                "Script duplication requires creating a new script source file.".to_string(),
            );
        }

        if matches!(source.kind, ProjectNodeKind::Folder { .. }) {
            if let Some(parent_id) = new_parent_id {
                if parent_id == id || self.is_descendant(id, parent_id) {
                    return Err("Cannot duplicate a Folder into its own subtree".to_string());
                }
            }
        }

        if self.subtree_contains_actor(id) {
            return Err("Actor Explorer references cannot be duplicated".to_string());
        }

        let mut candidate = self.clone();
        let duplicate_id = candidate.duplicate_subtree_inner(&source, new_parent_id, true)?;
        candidate.validate()?;

        *self = candidate;
        Ok(duplicate_id)
    }

    fn subtree_contains_script(&self, id: ProjectNodeId) -> bool {
        let Some(node) = self.node(id) else {
            return false;
        };

        matches!(node.kind, ProjectNodeKind::Script { .. })
            || self
                .children_of(Some(id))
                .into_iter()
                .any(|child_id| self.subtree_contains_script(child_id))
    }

    fn subtree_contains_actor(&self, id: ProjectNodeId) -> bool {
        let Some(node) = self.node(id) else {
            return false;
        };

        matches!(
            node.kind,
            ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. }
        ) || self
            .children_of(Some(id))
            .into_iter()
            .any(|child_id| self.subtree_contains_actor(child_id))
    }

    fn duplicate_subtree_inner(
        &mut self,
        source: &ProjectNode,
        parent_id: Option<ProjectNodeId>,
        rename_root_folder: bool,
    ) -> Result<ProjectNodeId, String> {
        let new_id = match &source.kind {
            ProjectNodeKind::Folder { name } => {
                let name = if rename_root_folder {
                    format!("{} Copy", name)
                } else {
                    name.clone()
                };
                self.create_folder(parent_id, &name)?
            }
            ProjectNodeKind::Script { .. } => {
                return Err(
                    "Script references cannot be duplicated without duplicating their source file."
                        .to_string(),
                );
            }
            ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. } => {
                return Err("Scene Explorer references cannot be duplicated".to_string());
            }
        };

        let children = self.children_of(Some(source.id));
        for child_id in children {
            let child = self
                .node(child_id)
                .cloned()
                .ok_or_else(|| "Folder subtree changed while duplicating".to_string())?;
            self.duplicate_subtree_inner(&child, Some(new_id), false)?;
        }

        Ok(new_id)
    }

    pub fn rename_folder(&mut self, id: ProjectNodeId, new_name: &str) -> Result<(), String> {
        let new_name = Self::validate_folder_name(new_name)?;

        let Some(node) = self.node_mut(id) else {
            return Err("Node not found".to_string());
        };

        match &mut node.kind {
            ProjectNodeKind::Folder { name } => {
                *name = new_name;
                Ok(())
            }

            _ => Err("Target node is not a Folder".to_string()),
        }
    }

    /// Removes a node and all descendants.
    ///
    /// Project hierarchy deletion is deliberately subtree-based so deleting a
    /// Script cannot silently convert its children into Actor modules.
    pub fn remove_node(&mut self, id: ProjectNodeId) -> Option<ProjectNode> {
        let root = self.node(id)?.clone();
        self.remove_subtree(id);
        Some(root)
    }

    pub fn remove_subtree(&mut self, id: ProjectNodeId) -> Vec<ProjectNodeId> {
        if !self.contains(id) {
            return Vec::new();
        }

        let mut removed = Vec::new();

        fn collect_ids(
            hierarchy: &ProjectHierarchy,
            current_id: ProjectNodeId,
            out: &mut Vec<ProjectNodeId>,
        ) {
            out.push(current_id);

            for child_id in hierarchy.children_of(Some(current_id)) {
                collect_ids(hierarchy, child_id, out);
            }
        }

        collect_ids(self, id, &mut removed);

        for node_id in &removed {
            self.nodes.remove(node_id);
        }

        removed
    }

    pub fn find_actor_node(&self, actor_id: u64) -> Option<ProjectNodeId> {
        self.nodes.values().find_map(|node| {
            if let ProjectNodeKind::Actor {
                actor_id: node_actor_id,
            } = node.kind
            {
                if node_actor_id == actor_id {
                    return Some(node.id);
                }
            }

            None
        })
    }

    pub fn find_script_nodes(&self, script_path: &str) -> Vec<ProjectNodeId> {
        let Ok(script_path) = Self::normalize_script_path(script_path) else {
            return Vec::new();
        };

        self.nodes
            .values()
            .filter_map(|node| {
                if let ProjectNodeKind::Script {
                    script_path: existing_path,
                } = &node.kind
                {
                    if existing_path == &script_path {
                        return Some(node.id);
                    }
                }

                None
            })
            .collect()
    }

    pub fn replace_script_path(&mut self, old_path: &str, new_path: &str) -> Result<usize, String> {
        let old_path = Self::normalize_script_path(old_path)?;
        let new_path = Self::normalize_script_path(new_path)?;

        if old_path == new_path {
            return Ok(0);
        }

        let mut candidate = self.clone();
        let mut replaced = 0;

        for node in candidate.nodes.values_mut() {
            let ProjectNodeKind::Script { script_path } = &mut node.kind else {
                continue;
            };

            if *script_path == old_path {
                *script_path = new_path.clone();
                replaced += 1;
            }
        }

        candidate.validate()?;

        *self = candidate;

        Ok(replaced)
    }

    pub fn find_folder_nodes(&self, name: &str) -> Vec<ProjectNodeId> {
        let name = name.trim();

        self.nodes
            .values()
            .filter_map(|node| {
                if let ProjectNodeKind::Folder { name: folder_name } = &node.kind {
                    if folder_name == name {
                        return Some(node.id);
                    }
                }

                None
            })
            .collect()
    }

    pub fn rebuild_indexes(&mut self) {
        let max_id = self.nodes.keys().copied().max().unwrap_or(0);

        self.next_node_id = max_id.checked_add(1).unwrap_or(u64::MAX);
    }

    pub fn validate(&self) -> Result<(), String> {
        let mut actor_ids = HashSet::new();
        let mut cell_ids = HashSet::new();

        for (&id, node) in &self.nodes {
            if id == 0 {
                return Err("Project node ID 0 is invalid".to_string());
            }

            if node.id != id {
                return Err(format!(
                    "Node key {} does not match node.id {}",
                    id, node.id
                ));
            }

            match &node.kind {
                ProjectNodeKind::Folder { name } => {
                    Self::validate_folder_name(name)?;
                }

                ProjectNodeKind::Actor { actor_id } => {
                    if *actor_id == 0 {
                        return Err(format!("Actor node {} references invalid Actor ID 0", id));
                    }

                    if !actor_ids.insert(*actor_id) {
                        return Err(format!(
                            "Actor {} has multiple Project Explorer nodes",
                            actor_id
                        ));
                    }
                }

                ProjectNodeKind::Cell { cell_id, label } => {
                    if *cell_id == 0 || !cell_ids.insert(*cell_id) {
                        return Err(format!(
                            "Invalid or duplicate Cell Explorer reference {cell_id}"
                        ));
                    }
                    if label.as_ref().is_some_and(|s| s.trim().is_empty()) {
                        return Err("Cell Explorer name cannot be blank".into());
                    }
                }
                ProjectNodeKind::Script { script_path } => {
                    Self::normalize_script_path(script_path)?;
                }
            }

            if let Some(parent_id) = node.parent_id {
                if parent_id == id {
                    return Err(format!("Node {} is parented to itself", id));
                }

                if !self.contains(parent_id) {
                    return Err(format!(
                        "Node {} references missing parent {}",
                        id, parent_id
                    ));
                }

                if self.is_descendant(id, parent_id) {
                    return Err(format!(
                        "Node {} contains parent cycle through {}",
                        id, parent_id
                    ));
                }

                if !self.can_parent(Some(parent_id), &node.kind) {
                    return Err(format!("Parent {} cannot hold child {}", parent_id, id));
                }
            }
        }

        // A script may only be attached once to a given Actor.
        let mut actor_script_pairs: HashSet<(u64, String)> = HashSet::new();

        for node in self.nodes.values() {
            let ProjectNodeKind::Script { script_path } = &node.kind else {
                continue;
            };

            if let Some(actor_id) = self.attached_actor_id(node.id) {
                let key = (actor_id, script_path.clone());

                if !actor_script_pairs.insert(key) {
                    return Err(format!(
                        "Script '{}' is attached more than once to Actor {}",
                        script_path, actor_id
                    ));
                }
            }
        }

        if self.next_node_id == 0 {
            return Err("next_node_id cannot be zero".to_string());
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
    #[test]
    fn meaningful_cell_nodes_are_unique_persistent_and_not_implicit_categories() {
        let mut h = ProjectHierarchy::new();
        let parent = h.create_folder(None, "GameWorld").unwrap();
        let lights = h.create_folder(Some(parent), "Lights").unwrap();
        let id = h.create_cell_node(None, 10_000_777).unwrap();
        assert!(h.create_cell_node(None, 10_000_777).is_err());
        assert!(!h.can_parent(Some(id), &ProjectNodeKind::Actor { actor_id: 55 }));
        h.move_node(id, Some(lights)).unwrap();
        h.rename_cell_node(id, "North Beacon").unwrap();
        let raw = serde_json::to_string(&h).unwrap();
        let restored: ProjectHierarchy = serde_json::from_str(&raw).unwrap();
        assert!(restored.validate().is_ok());
        assert_eq!(restored.parent_of(id), Some(lights));
        assert!(matches!(&restored.node(id).unwrap().kind,
            ProjectNodeKind::Cell { cell_id: 10_000_777, label: Some(name) } if name == "North Beacon"));
        assert!(h.duplicate_node(id, None).is_err());
    }

    #[test]
    fn test_create_nodes_and_hierarchy() {
        let mut hierarchy = ProjectHierarchy::new();

        let folder = hierarchy.create_folder(None, "Dungeon").unwrap();

        let enemies = hierarchy.create_folder(Some(folder), "Enemies").unwrap();

        let actor = hierarchy.create_actor_node(Some(enemies), 101).unwrap();

        let script = hierarchy
            .create_script_node(Some(actor), "scripts/goblin.aeo")
            .unwrap();

        assert_eq!(hierarchy.root_nodes(), vec![folder]);
        assert_eq!(hierarchy.children_of(Some(folder)), vec![enemies]);
        assert_eq!(hierarchy.children_of(Some(enemies)), vec![actor]);
        assert_eq!(hierarchy.children_of(Some(actor)), vec![script]);

        assert_eq!(hierarchy.attached_actor_id(script), Some(101));

        assert!(hierarchy.validate().is_ok());
    }

    #[test]
    fn test_rename_and_move_folder() {
        let mut hierarchy = ProjectHierarchy::new();

        let folder_a = hierarchy.create_folder(None, "RootFolder").unwrap();

        let folder_b = hierarchy.create_folder(None, "TargetFolder").unwrap();

        hierarchy.rename_folder(folder_a, "RenamedFolder").unwrap();

        match &hierarchy.node(folder_a).unwrap().kind {
            ProjectNodeKind::Folder { name } => {
                assert_eq!(name, "RenamedFolder");
            }

            _ => panic!("Expected folder"),
        }

        hierarchy.move_node(folder_a, Some(folder_b)).unwrap();

        assert_eq!(hierarchy.parent_of(folder_a), Some(folder_b));

        assert!(hierarchy.validate().is_ok());
    }

    #[test]
    fn test_reject_cycles() {
        let mut hierarchy = ProjectHierarchy::new();

        let a = hierarchy.create_folder(None, "A").unwrap();
        let b = hierarchy.create_folder(Some(a), "B").unwrap();
        let c = hierarchy.create_folder(Some(b), "C").unwrap();

        assert!(hierarchy.move_node(a, Some(c)).is_err());
        assert!(hierarchy.move_node(a, Some(a)).is_err());
    }

    #[test]
    fn test_script_cannot_contain_actor_even_through_folder() {
        let mut hierarchy = ProjectHierarchy::new();

        let script = hierarchy
            .create_script_node(None, "scripts/controller.aeo")
            .unwrap();

        let folder = hierarchy.create_folder(Some(script), "Helpers").unwrap();

        assert!(hierarchy.create_actor_node(Some(folder), 42).is_err());

        assert!(hierarchy.validate().is_ok());
    }

    #[test]
    fn test_actor_nodes_are_unique() {
        let mut hierarchy = ProjectHierarchy::new();

        let folder_a = hierarchy.create_folder(None, "A").unwrap();

        let folder_b = hierarchy.create_folder(None, "B").unwrap();

        hierarchy.create_actor_node(Some(folder_a), 42).unwrap();

        assert!(hierarchy.create_actor_node(Some(folder_b), 42).is_err());
    }

    #[test]
    fn test_remove_node_removes_subtree() {
        let mut hierarchy = ProjectHierarchy::new();

        let actor = hierarchy.create_actor_node(None, 10).unwrap();

        let controller = hierarchy
            .create_script_node(Some(actor), "scripts/controller.aeo")
            .unwrap();

        let child = hierarchy
            .create_script_node(Some(controller), "scripts/damage.aeo")
            .unwrap();

        let root_folder = hierarchy.create_folder(None, "Other").unwrap();

        let _ = root_folder;

        let removed = hierarchy.remove_node(controller);

        assert!(removed.is_some());
        assert!(!hierarchy.contains(controller));
        assert!(!hierarchy.contains(child));
        assert!(hierarchy.validate().is_ok());
    }

    #[test]
    fn test_script_attachment_semantics() {
        let mut hierarchy = ProjectHierarchy::new();

        let actor = hierarchy.create_actor_node(None, 1).unwrap();

        let folder = hierarchy.create_folder(Some(actor), "Logic").unwrap();

        let script = hierarchy
            .create_script_node(Some(folder), "scripts/combat.aeo")
            .unwrap();

        assert_eq!(hierarchy.attached_actor_id(script), Some(1));

        let child = hierarchy
            .create_script_node(Some(script), "scripts/damage.aeo")
            .unwrap();

        assert_eq!(hierarchy.attached_actor_id(child), None);

        let root = hierarchy.create_folder(None, "Systems").unwrap();

        hierarchy.move_node(script, Some(root)).unwrap();

        assert_eq!(hierarchy.attached_actor_id(script), None);

        let actor_b = hierarchy.create_actor_node(None, 2).unwrap();

        hierarchy.move_node(script, Some(actor_b)).unwrap();

        assert_eq!(hierarchy.attached_actor_id(script), Some(2));
    }

    #[test]
    fn test_duplicate_script_attachment_rejected() {
        let mut hierarchy = ProjectHierarchy::new();

        let actor_a = hierarchy.create_actor_node(None, 1).unwrap();

        hierarchy
            .create_script_node(Some(actor_a), "scripts/combat.aeo")
            .unwrap();

        assert!(
            hierarchy
                .create_script_node(Some(actor_a), "scripts/combat.aeo",)
                .is_err()
        );

        let actor_b = hierarchy.create_actor_node(None, 2).unwrap();

        assert!(
            hierarchy
                .create_script_node(Some(actor_b), "scripts/combat.aeo",)
                .is_ok()
        );
    }

    #[test]
    fn test_script_path_validation() {
        let mut hierarchy = ProjectHierarchy::new();

        assert!(
            hierarchy
                .create_script_node(None, "../escape.aeo",)
                .is_err()
        );

        assert!(
            hierarchy
                .create_script_node(None, "other/foo.aeo",)
                .is_err()
        );

        assert!(
            hierarchy
                .create_script_node(None, "scripts/foo.txt",)
                .is_err()
        );

        assert!(
            hierarchy
                .create_script_node(None, "scripts/foo.aeo",)
                .is_ok()
        );

        assert!(
            hierarchy
                .create_script_node(None, "playerscripts/controller.aeo",)
                .is_ok()
        );
    }

    #[test]
    fn test_rebuild_indexes_repairs_next_id() {
        let mut hierarchy = ProjectHierarchy::new();

        hierarchy.nodes.insert(
            42,
            ProjectNode {
                id: 42,
                parent_id: None,
                kind: ProjectNodeKind::Folder {
                    name: "Existing".to_string(),
                },
            },
        );

        hierarchy.next_node_id = 1;
        hierarchy.rebuild_indexes();

        assert_eq!(hierarchy.next_node_id, 43);

        let created = hierarchy.create_folder(None, "New").unwrap();

        assert_eq!(created, 43);
    }

    #[test]
    fn test_round_trip_serialization() {
        let mut hierarchy = ProjectHierarchy::new();

        let folder = hierarchy.create_folder(None, "Dungeon").unwrap();

        let actor = hierarchy.create_actor_node(Some(folder), 100).unwrap();

        hierarchy
            .create_script_node(Some(actor), "scripts/dungeon.aeo")
            .unwrap();

        let encoded = serde_json::to_string(&hierarchy).unwrap();

        let decoded: ProjectHierarchy = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, hierarchy);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn test_atomic_multi_move_commits_all_nodes() {
        let mut hierarchy = ProjectHierarchy::new();
        let source = hierarchy.create_folder(None, "Source").unwrap();
        let target = hierarchy.create_folder(None, "Target").unwrap();
        let a = hierarchy.create_folder(Some(source), "A").unwrap();
        let b = hierarchy.create_folder(Some(source), "B").unwrap();

        hierarchy.move_nodes_atomic(&[a, b], Some(target)).unwrap();

        assert_eq!(hierarchy.parent_of(a), Some(target));
        assert_eq!(hierarchy.parent_of(b), Some(target));
    }

    #[test]
    fn test_atomic_multi_move_rolls_back_when_one_move_is_invalid() {
        let mut hierarchy = ProjectHierarchy::new();
        let actor = hierarchy.create_actor_node(None, 1).unwrap();
        let folder = hierarchy.create_folder(None, "Folder").unwrap();
        let script = hierarchy
            .create_script_node(None, "scripts/controller.aeo")
            .unwrap();
        let before = hierarchy.clone();

        assert!(
            hierarchy
                .move_nodes_atomic(&[folder, actor], Some(script))
                .is_err()
        );
        assert_eq!(hierarchy, before);
    }

    #[test]
    fn test_atomic_multi_move_rejects_overlapping_selection() {
        let mut hierarchy = ProjectHierarchy::new();
        let a = hierarchy.create_folder(None, "A").unwrap();
        let b = hierarchy.create_folder(Some(a), "B").unwrap();
        let before = hierarchy.clone();

        assert!(hierarchy.move_nodes_atomic(&[a, b], None).is_err());
        assert_eq!(hierarchy, before);
    }

    #[test]
    fn test_duplicate_node_rejects_script_sources_and_script_subtrees() {
        let mut hierarchy = ProjectHierarchy::new();

        let folder = hierarchy.create_folder(None, "Systems").unwrap();

        let script = hierarchy
            .create_script_node(Some(folder), "scripts/combat.aeo")
            .unwrap();

        let before = hierarchy.clone();

        assert!(hierarchy.duplicate_node(script, None).is_err());
        assert!(hierarchy.duplicate_node(folder, None).is_err());

        assert_eq!(hierarchy, before);
    }

    #[test]
    fn test_duplicate_plain_folder_subtree() {
        let mut hierarchy = ProjectHierarchy::new();

        let folder = hierarchy.create_folder(None, "Systems").unwrap();

        hierarchy.create_folder(Some(folder), "Utilities").unwrap();

        let folder_copy = hierarchy.duplicate_node(folder, None).unwrap();

        assert_eq!(hierarchy.children_of(Some(folder_copy)).len(), 1);

        assert!(matches!(
            hierarchy.node(folder_copy).unwrap().kind,
            ProjectNodeKind::Folder { ref name }
                if name == "Systems Copy"
        ));
    }

    #[test]
    fn test_duplicate_actor_and_actor_subtree_are_rejected() {
        let mut hierarchy = ProjectHierarchy::new();
        let folder = hierarchy.create_folder(None, "Actors").unwrap();
        let actor = hierarchy.create_actor_node(Some(folder), 1).unwrap();
        let before = hierarchy.clone();

        assert!(hierarchy.duplicate_node(actor, None).is_err());
        assert!(hierarchy.duplicate_node(folder, None).is_err());
        assert_eq!(hierarchy, before);
    }

    #[test]
    fn test_folder_with_actor_cannot_move_below_script() {
        let mut hierarchy = ProjectHierarchy::new();
        let folder = hierarchy.create_folder(None, "Actors").unwrap();
        hierarchy.create_actor_node(Some(folder), 1).unwrap();
        let script = hierarchy
            .create_script_node(None, "scripts/controller.aeo")
            .unwrap();
        let before = hierarchy.clone();

        assert!(hierarchy.move_node(folder, Some(script)).is_err());
        assert_eq!(hierarchy, before);
    }

    #[test]
    fn test_chk05_acceptance_sequence_survives_round_trip() {
        let mut hierarchy = ProjectHierarchy::new();
        let dungeon = hierarchy.create_folder(None, "Dungeon").unwrap();
        let enemies = hierarchy.create_folder(Some(dungeon), "Enemies").unwrap();
        let goblin = hierarchy.create_actor_node(Some(enemies), 1).unwrap();
        let orc = hierarchy.create_actor_node(Some(enemies), 2).unwrap();
        let systems = hierarchy.create_folder(None, "Systems").unwrap();
        let combat = hierarchy
            .create_script_node(Some(systems), "scripts/combat.aeo")
            .unwrap();

        hierarchy.move_node(combat, Some(goblin)).unwrap();
        assert_eq!(hierarchy.attached_actor_id(combat), Some(1));

        hierarchy.move_node(combat, Some(orc)).unwrap();
        assert_eq!(hierarchy.attached_actor_id(combat), Some(2));

        let goblin_combat = hierarchy
            .create_script_node(Some(goblin), "scripts/combat_copy.aeo")
            .unwrap();
        assert_eq!(hierarchy.attached_actor_id(goblin_combat), Some(1));

        hierarchy.move_node(goblin_combat, Some(combat)).unwrap();
        assert_eq!(hierarchy.attached_actor_id(combat), Some(2));
        assert_eq!(hierarchy.attached_actor_id(goblin_combat), None);

        hierarchy.move_node(goblin_combat, None).unwrap();
        assert_eq!(hierarchy.attached_actor_id(goblin_combat), None);

        let encoded = serde_json::to_string(&hierarchy).unwrap();
        let decoded: ProjectHierarchy = serde_json::from_str(&encoded).unwrap();
        assert_eq!(decoded, hierarchy);
        assert!(decoded.validate().is_ok());
    }

    #[test]
    fn test_replace_script_path_updates_every_reference() {
        let mut hierarchy = ProjectHierarchy::new();

        let actor_a = hierarchy.create_actor_node(None, 1).unwrap();
        let actor_b = hierarchy.create_actor_node(None, 2).unwrap();

        let root_script = hierarchy
            .create_script_node(None, "scripts/old.aeo")
            .unwrap();

        let actor_a_script = hierarchy
            .create_script_node(Some(actor_a), "scripts/old.aeo")
            .unwrap();

        let actor_b_script = hierarchy
            .create_script_node(Some(actor_b), "scripts/old.aeo")
            .unwrap();

        let replaced = hierarchy
            .replace_script_path("scripts/old.aeo", "scripts/new.aeo")
            .unwrap();

        assert_eq!(replaced, 3);

        for id in [root_script, actor_a_script, actor_b_script] {
            assert_eq!(
                hierarchy.node(id).unwrap().kind,
                ProjectNodeKind::Script {
                    script_path: "scripts/new.aeo".to_string()
                }
            );
        }

        assert!(hierarchy.find_script_nodes("scripts/old.aeo").is_empty());
        assert_eq!(hierarchy.find_script_nodes("scripts/new.aeo").len(), 3);
    }
}
