use std::collections::HashMap;
use std::sync::Arc;

use crate::project::{ProjectNodeId, ProjectNodeKind};
use crate::world::{CellType, World, WorldCoord};

use super::Editor;

#[derive(Default)]
pub(crate) struct WorldNavigatorCache {
    pub revision: Option<u64>,
    pub block_count: usize,
    pub spawn_points: Vec<WorldCoord>,
    pub fx_blocks: Vec<WorldCoord>,
    pub players: Vec<WorldCoord>,
    pub lights: Vec<u64>,
    pub audio_emitters: Vec<u64>,
    pub particle_emitters: Vec<u64>,
    pub actors: Vec<u64>,
}

#[derive(Default)]
pub(crate) struct ProjectTreeCache {
    pub dirty: bool,
    pub world_revision: Option<u64>,
    pub children: HashMap<Option<ProjectNodeId>, Vec<ProjectNodeId>>,
    pub display_names: HashMap<ProjectNodeId, String>,
    pub lowercase_names: HashMap<ProjectNodeId, String>,
}

#[derive(Default)]
pub(crate) struct OutputPanelCache {
    pub terminal_revision: u64,
    pub script_revision: u64,
    pub width_bits: u32,
    pub galley: Option<Arc<egui::Galley>>,
}

impl Editor {
    pub(crate) fn invalidate_project_tree_cache(&mut self) {
        self.project_tree_cache.dirty = true;
    }

    pub(crate) fn ensure_project_tree_cache(&mut self, world: &World) {
        let world_revision = world.render_revision();
        if !self.project_tree_cache.dirty
            && self.project_tree_cache.world_revision == Some(world_revision)
        {
            return;
        }

        let mut display_names = HashMap::with_capacity(self.project_hierarchy.nodes.len());
        let mut lowercase_names = HashMap::with_capacity(self.project_hierarchy.nodes.len());
        let mut children: HashMap<Option<ProjectNodeId>, Vec<ProjectNodeId>> = HashMap::new();

        for node in self.project_hierarchy.nodes.values() {
            let name = match &node.kind {
                ProjectNodeKind::Folder { name } => name.clone(),
                ProjectNodeKind::Actor { actor_id } => world
                    .actors
                    .get(actor_id)
                    .map(|actor| {
                        if actor.identity.trim().is_empty() {
                            format!("Actor_{}", actor.id)
                        } else {
                            actor.identity.clone()
                        }
                    })
                    .unwrap_or_else(|| format!("Missing Actor {}", actor_id)),
                // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
                ProjectNodeKind::Cell { cell_id, label } => {
                    Self::cell_node_title(*cell_id, label, world)
                }
                ProjectNodeKind::Script { script_path } => std::path::Path::new(script_path)
                    .file_name()
                    .and_then(|name| name.to_str())
                    .unwrap_or(script_path)
                    .to_string(),
            };
            lowercase_names.insert(node.id, name.to_ascii_lowercase());
            display_names.insert(node.id, name);
            children.entry(node.parent_id).or_default().push(node.id);
        }

        for ids in children.values_mut() {
            ids.sort_by(|left, right| {
                let left_node = self.project_hierarchy.node(*left);
                let right_node = self.project_hierarchy.node(*right);
                let rank = |node: Option<&crate::project::ProjectNode>| {
                    node.map_or(99, |node| match node.kind {
                        ProjectNodeKind::Folder { .. } => 0,
                        ProjectNodeKind::Actor { .. } | ProjectNodeKind::Cell { .. } => 1,
                        ProjectNodeKind::Script { .. } => 2,
                    })
                };
                rank(left_node)
                    .cmp(&rank(right_node))
                    .then_with(|| {
                        lowercase_names
                            .get(left)
                            .map(String::as_str)
                            .unwrap_or("")
                            .cmp(lowercase_names.get(right).map(String::as_str).unwrap_or(""))
                    })
                    .then_with(|| left.cmp(right))
            });
        }

        self.project_tree_cache.children = children;
        self.project_tree_cache.display_names = display_names;
        self.project_tree_cache.lowercase_names = lowercase_names;
        self.project_tree_cache.world_revision = Some(world_revision);
        self.project_tree_cache.dirty = false;
    }

    pub(crate) fn cached_project_children(
        &self,
        parent: Option<ProjectNodeId>,
    ) -> Vec<ProjectNodeId> {
        self.project_tree_cache
            .children
            .get(&parent)
            .cloned()
            .unwrap_or_default()
    }

    pub(crate) fn cached_project_display_name(&self, id: ProjectNodeId) -> Option<&str> {
        self.project_tree_cache
            .display_names
            .get(&id)
            .map(String::as_str)
    }

    pub(crate) fn cached_project_node_matches_filter(
        &self,
        node_id: ProjectNodeId,
        query: &str,
    ) -> bool {
        let query = query.trim().to_ascii_lowercase();
        if query.is_empty() {
            return true;
        }

        if self
            .project_tree_cache
            .lowercase_names
            .get(&node_id)
            .is_some_and(|name| name.contains(&query))
        {
            return true;
        }

        if let Some(node) = self.project_hierarchy.node(node_id) {
            if let ProjectNodeKind::Script { script_path } = &node.kind {
                if script_path.to_ascii_lowercase().contains(&query) {
                    return true;
                }
            }
        }

        self.project_tree_cache
            .children
            .get(&Some(node_id))
            .is_some_and(|children| {
                children
                    .iter()
                    .copied()
                    .any(|child| self.cached_project_node_matches_filter(child, &query))
            })
    }

    pub(crate) fn ensure_world_navigator_cache(&mut self, world: &World) {
        let revision = world.render_revision();
        if self.world_navigator_cache.revision == Some(revision) {
            return;
        }

        let mut spawn_points = world
            .iter_editor_navigator_coords(CellType::SpawnPoint)
            .collect::<Vec<_>>();
        let mut fx_blocks = world
            .iter_editor_navigator_coords(CellType::FxBlock)
            .collect::<Vec<_>>();
        let mut players = world
            .iter_editor_navigator_coords(CellType::Player)
            .collect::<Vec<_>>();
        let sort_coord =
            |a: &WorldCoord, b: &WorldCoord| a.x.cmp(&b.x).then(a.y.cmp(&b.y)).then(a.z.cmp(&b.z));
        spawn_points.sort_by(sort_coord);
        fx_blocks.sort_by(sort_coord);
        players.sort_by(sort_coord);

        let mut lights = Vec::new();
        let mut audio_emitters = Vec::new();
        let mut particle_emitters = Vec::new();
        for source in world.iter_sources() {
            match source.cell.cell_type {
                CellType::Light => lights.push(source.cell.id),
                CellType::AudioEmitter => audio_emitters.push(source.cell.id),
                CellType::ParticleEmitter => particle_emitters.push(source.cell.id),
                _ => {}
            }
        }
        lights.sort_unstable();
        audio_emitters.sort_unstable();
        particle_emitters.sort_unstable();

        let mut actors = world.actors.keys().copied().collect::<Vec<_>>();
        actors.sort_unstable();

        self.world_navigator_cache = WorldNavigatorCache {
            revision: Some(revision),
            block_count: world.authored_cell_count(CellType::Block),
            spawn_points,
            fx_blocks,
            players,
            lights,
            audio_emitters,
            particle_emitters,
            actors,
        };
    }
}
