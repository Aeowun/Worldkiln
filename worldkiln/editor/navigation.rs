use crate::navigation::{NavigationAgent, NavigationGrid, NavigationPath};
use crate::world::WorldCoord;

/// Holds the state for the editor navigation controls.
///
/// This state is used to buffer text input for coordinate fields and track
/// whether the navigation interface should be displayed.
pub struct NavigationWindow {
    pub is_open: bool,
    pub x_buf: String,
    pub y_buf: String,
    pub z_buf: String,
    pub path_start: [i32; 3],
    pub path_target: [i32; 3],
    pub agent: NavigationAgent,
    // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
    pub selected_volume_id: Option<u64>,
    pub debug_grid: Option<NavigationGrid>,
    pub debug_path: Option<NavigationPath>,
    // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:20 -05:00
    pub debug_world_revision: Option<u64>,
    pub debug_status: String,
    pub show_walkable: bool,
    pub show_blocked: bool,
    pub show_path: bool,
}

impl NavigationWindow {
    pub fn new() -> Self {
        Self {
            is_open: true,
            x_buf: "0".to_string(),
            y_buf: "0".to_string(),
            z_buf: "0".to_string(),
            path_start: [0, 1, 0],
            path_target: [4, 1, 0],
            agent: NavigationAgent::default(),
            selected_volume_id: None,
            debug_grid: None,
            debug_path: None,
            // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:44 -05:00
            debug_world_revision: None,
            debug_status: "Build a static snapshot to inspect navigation.".to_string(),
            show_walkable: true,
            show_blocked: true,
            show_path: true,
        }
    }

    /// Synchronizes the text buffers with a given world coordinate.
    pub fn sync(&mut self, coord: WorldCoord) {
        self.x_buf = coord.x.to_string();
        self.y_buf = coord.y.to_string();
        self.z_buf = coord.z.to_string();
    }

    // Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:20 -05:00
    pub fn snapshot_is_stale(&self, world_revision: u64) -> bool {
        self.debug_grid.is_some() && self.debug_world_revision != Some(world_revision)
    }

    pub fn debug_overlays_are_current(&self, world_revision: u64) -> bool {
        self.debug_grid.is_some() && !self.snapshot_is_stale(world_revision)
    }

    pub fn mark_snapshot_built(&mut self, world_revision: u64) {
        self.debug_world_revision = Some(world_revision);
    }

    pub fn invalidate_grid(&mut self, reason: impl Into<String>) {
        self.debug_grid = None;
        self.debug_path = None;
        self.debug_world_revision = None;
        self.debug_status = reason.into();
    }

    pub fn invalidate_path(&mut self, reason: impl Into<String>) {
        self.debug_path = None;
        self.debug_status = reason.into();
    }

    pub fn solve_debug_path(&mut self) {
        let Some(grid) = self.debug_grid.as_ref() else {
            self.debug_path = None;
            self.debug_status = "Build a static snapshot before finding a path.".to_string();
            return;
        };

        let [start_x, start_y, start_z] = self.path_start;
        let [target_x, target_y, target_z] = self.path_target;
        // Worldkiln 0.8.5 | WO 0.8.7-54 | 2026-10-09 12:00 -05:00
        let start = grid.project_endpoint(WorldCoord::new(start_x, start_y, start_z));
        let target = grid.project_endpoint(WorldCoord::new(target_x, target_y, target_z));
        let walkable_count = grid.walkable_positions().len();
        let blocked_count = grid.blocked_positions().len();

        match grid.find_path(
            start,
            target,
            crate::navigation::PathQueryOptions::default(),
        ) {
            Ok(path) => {
                self.debug_status = format!(
                    "Path: {} nodes, cost {}, visited {}. Snapshot: {} walkable, {} blocked.",
                    path.nodes.len(),
                    path.total_cost,
                    path.visited_nodes,
                    walkable_count,
                    blocked_count
                );
                self.debug_path = Some(path);
            }
            Err(error) => {
                self.debug_status = format!(
                    "Path failed: {error}. Snapshot: {walkable_count} walkable, {blocked_count} blocked."
                );
                self.debug_path = None;
            }
        }
    }

    pub fn clear_debug(&mut self) {
        self.debug_grid = None;
        self.debug_path = None;
        self.debug_world_revision = None;
        self.debug_status = "Build a static snapshot to inspect navigation.".to_string();
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-14 | 2026-10-05 23:20 -05:00
#[cfg(test)]
mod freshness_tests {
    use super::*;
    use crate::world::{CellType, World};

    fn test_grid() -> NavigationGrid {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        NavigationGrid::build(&world, NavigationAgent::default()).unwrap()
    }

    #[test]
    fn world_revision_mutation_marks_snapshot_stale_and_rebuild_clears_it() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);

        let mut window = NavigationWindow::new();
        window.debug_grid =
            Some(NavigationGrid::build(&world, NavigationAgent::default()).unwrap());
        window.mark_snapshot_built(world.render_revision());
        assert!(window.debug_overlays_are_current(world.render_revision()));

        world.set_cell(WorldCoord::new(1, 0, 0), CellType::Block);
        assert!(window.snapshot_is_stale(world.render_revision()));
        assert!(!window.debug_overlays_are_current(world.render_revision()));

        window.debug_grid =
            Some(NavigationGrid::build(&world, NavigationAgent::default()).unwrap());
        window.mark_snapshot_built(world.render_revision());
        assert!(!window.snapshot_is_stale(world.render_revision()));
        assert!(window.debug_overlays_are_current(world.render_revision()));
    }

    #[test]
    fn unchanged_world_revision_does_not_invalidate_snapshot() {
        let mut world = World::new();
        world.set_cell(WorldCoord::new(0, 0, 0), CellType::Block);
        let revision = world.render_revision();

        let mut window = NavigationWindow::new();
        window.debug_grid =
            Some(NavigationGrid::build(&world, NavigationAgent::default()).unwrap());
        window.mark_snapshot_built(revision);

        assert_eq!(world.render_revision(), revision);
        assert!(window.debug_overlays_are_current(world.render_revision()));
    }

    #[test]
    fn recorded_world_revision_controls_snapshot_freshness() {
        let mut window = NavigationWindow::new();
        window.debug_grid = Some(test_grid());
        window.mark_snapshot_built(11);

        assert!(!window.snapshot_is_stale(11));
        assert!(window.snapshot_is_stale(12));
    }

    #[test]
    fn grid_invalidation_clears_grid_path_and_revision() {
        let mut window = NavigationWindow::new();
        window.debug_grid = Some(test_grid());
        window.mark_snapshot_built(4);
        window.invalidate_grid("Agent settings changed; rebuild snapshot.");

        assert!(window.debug_grid.is_none());
        assert!(window.debug_path.is_none());
        assert!(window.debug_world_revision.is_none());
    }

    #[test]
    fn endpoint_invalidation_keeps_fresh_grid_revision() {
        let mut window = NavigationWindow::new();
        window.debug_grid = Some(test_grid());
        window.mark_snapshot_built(9);
        window.invalidate_path("Start/Target changed; find path again.");

        assert!(window.debug_grid.is_some());
        assert_eq!(window.debug_world_revision, Some(9));
        assert!(window.debug_path.is_none());
    }

    #[test]
    fn clear_removes_grid_path_and_freshness_state() {
        let mut window = NavigationWindow::new();
        window.debug_grid = Some(test_grid());
        window.mark_snapshot_built(3);
        window.clear_debug();

        assert!(window.debug_grid.is_none());
        assert!(window.debug_path.is_none());
        assert!(window.debug_world_revision.is_none());
    }
}
