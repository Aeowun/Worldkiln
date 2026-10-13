use crate::engine::audio::EngineSound;
use crate::world::World;

use super::{App, EditorMode, View};

// Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:32 -05:00
fn parse_camera_state(content: &str) -> Result<[f32; 6], String> {
    let values = content
        .split_whitespace()
        .map(|part| {
            part.parse::<f32>()
                .map_err(|_| format!("invalid camera number '{part}'"))
        })
        .collect::<Result<Vec<_>, _>>()?;
    if values.len() != 6 {
        return Err(format!("expected 6 camera values, found {}", values.len()));
    }
    if values.iter().any(|value| !value.is_finite()) {
        return Err("camera values must be finite".to_string());
    }
    Ok(values.try_into().expect("length checked"))
}

pub fn get_timestamp() -> String {
    chrono::Local::now().format("%H:%M:%S").to_string()
}

impl App {
    pub fn exit_to_home(&mut self) {
        self.editor.mode = EditorMode::Editor;
        self.editor.exit_ui_mode();

        // Project exit is a save boundary. Save dirty script documents before
        // destroying the project-owned editor session.
        self.editor.script_editor.save_all();

        // Failed saves remain dirty. Do not throw those buffers away.
        if self
            .editor
            .script_editor
            .open_documents
            .values()
            .any(|document| document.dirty)
        {
            self.editor
                .script_editor
                .output_log
                .push("Exit cancelled: one or more scripts could not be saved.".to_string());

            return;
        }

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:52 -05:00
        // Preserve the loaded project session if persistence is blocked or fails.
        if let Err(error) = self.save_project_checked() {
            crate::engine::diagnostics::report("ERROR", "Project", error);
            return;
        }

        self.editor.script_editor.reset_project_session();
        // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:37 -05:00
        self.editor.asset_manager = crate::editor::asset_management::AssetManagerState::default();

        self.editor.show_script_workspace = false;
        self.editor.last_show_script_workspace = false;

        self.renderer.clear_chunk_cache();
        self.renderer.set_project_asset_root(None);

        self.world = World::new();

        self.physics_world.bodies.clear();

        self.character_system.clear();
        self.actor_system.clear();

        self.project_manager.current_project = None;
        // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
        self.project_load_failed = false;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
        self.project_disk_snapshot = None;

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);

        self.editor.history.undo_stack.clear();
        self.editor.history.redo_stack.clear();
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
        self.editor.pending_inspector_history = None;
        self.editor.clear_clipboard();

        self.editor.project_hierarchy = crate::project::ProjectHierarchy::new();

        self.editor.selected_project_node_id = None;
        self.editor.selected_project_nodes.clear();
        self.editor.expanded_project_nodes.clear();

        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
        // UI document history is isolated to its owning project.
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
        self.editor.ui_document_history.clear();
        self.editor.ui_workspace = crate::engine::ui_authoring::AuthoredUiWorkspace::default();
        self.editor.ui_document = self
            .editor
            .ui_workspace
            .active_document()
            .cloned()
            .unwrap_or_default();

        self.editor.ui_selected_node = None;

        self.runtime_ui.clear();

        self.view = View::Home;
    }

    pub fn load_project(&mut self) {
        let Some(project_path) = self.project_manager.current_project.clone() else {
            return;
        };

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:03 -05:00
        // Keep every project save blocked until all persisted project state loads.
        self.project_load_failed = true;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
        self.project_disk_snapshot = None;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 13:17 -05:00
        // Clear project-owned runtime and editor state before attempting the
        // new load, so a failed project cannot leave the prior project visible.
        self.world = World::new();
        self.physics_world.bodies.clear();
        self.character_system.clear();
        self.actor_system.clear();
        self.renderer.clear_chunk_cache();
        self.editor.project_hierarchy = crate::project::ProjectHierarchy::new();
        self.editor.selected_coord = None;
        self.editor.selected_coords.clear();
        self.editor.selected_actor_id = None;
        self.editor.selected_source_id = None;
        self.editor.selected_project_node_id = None;
        self.editor.selected_project_nodes.clear();
        self.editor.expanded_project_nodes.clear();
        self.editor.selected_actor_directory_id = None;
        self.editor.hovered_cell = None;
        self.editor.hovered_actor_id = None;
        self.editor.hovered_source_id = None;
        self.editor.hovered_terrain_hit = None;
        self.editor.project_actor_sync_revision = None;
        self.editor.project_cell_sync_revision = None;
        self.editor.project_tree_cache = Default::default();
        self.editor.ui_workspace = crate::engine::ui_authoring::AuthoredUiWorkspace::default();
        self.editor.ui_document = self
            .editor
            .ui_workspace
            .active_document()
            .cloned()
            .unwrap_or_default();
        self.editor.ui_selected_node = None;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:49 -05:00
        // A project without camera.dat starts from the editor defaults, never
        // from the camera position of the previously open project.
        self.editor.camera = crate::renderer::camera::CameraController::new();
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 12:22 -05:00
        // A newly selected project cannot inherit any prior project's timeline.
        self.editor.history.undo_stack.clear();
        self.editor.history.redo_stack.clear();
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
        self.editor.pending_inspector_history = None;
        self.editor.ui_document_history.clear();
        self.editor.script_editor.reset_project_session();
        // Worldkiln 0.8.7 | WO 0.8.7-21 | 2026-10-06 10:37 -05:00
        self.editor.asset_manager = crate::editor::asset_management::AssetManagerState::default();
        self.renderer.set_project_asset_root(Some(&project_path));

        crate::character::package::reload_packages(Some(&project_path));
        crate::engine::entity_package::reload_packages(Some(&project_path));

        self.renderer.clear_chunk_cache();

        self.editor
            .script_editor
            .refresh_scripts(&Some(project_path.clone()));

        let world_path = project_path.join("world.dat");

        if let Err(error) = crate::world::persistence::load_world(&mut self.world, &world_path) {
            crate::engine::diagnostics::report(
                "ERROR",
                "Project",
                format!("Failed to load world.dat: {}", error),
            );
            return;
        }
        self.editor.editor_max_distance_chunks = self.world.render_distance.editor_chunks;
        self.editor.play_max_distance_chunks = self.world.render_distance.play_chunks;

        match crate::project::load_project_metadata(&project_path) {
            Ok(project_hierarchy) => {
                self.editor.project_hierarchy = project_hierarchy;
            }
            Err(error) => {
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Project",
                    format!("Failed to load project.json: {}", error),
                );
                return;
            }
        }

        let indexed_script_paths = self
            .editor
            .script_editor
            .project_script_index()
            .map(|index| {
                index
                    .all_modules()
                    .into_iter()
                    .map(|module| {
                        module
                            .logical_path()
                            .to_string_lossy()
                            .replace('\\', "/")
                            .to_ascii_lowercase()
                    })
                    .collect::<std::collections::HashSet<_>>()
            });

        if let Some(valid_paths) = indexed_script_paths.as_ref() {
            let stale_script_nodes = self
                .editor
                .project_hierarchy
                .nodes
                .values()
                .filter_map(|node| match &node.kind {
                    crate::project::ProjectNodeKind::Script { script_path } => {
                        let normalized = script_path.replace('\\', "/").to_ascii_lowercase();

                        if !valid_paths.contains(&normalized) {
                            Some(node.id)
                        } else {
                            None
                        }
                    }

                    _ => None,
                })
                .collect::<Vec<_>>();

            let mut removed_any = false;

            for node_id in stale_script_nodes {
                if self.editor.project_hierarchy.contains(node_id) {
                    self.editor.project_hierarchy.remove_node(node_id);
                    removed_any = true;
                }
            }

            if removed_any {
                self.editor.needs_save = true;
            }
        }

        let existing_actor_ids: std::collections::HashSet<u64> =
            self.world.actors.keys().copied().collect();

        let stale_actor_nodes: Vec<u64> = self
            .editor
            .project_hierarchy
            .nodes
            .values()
            .filter_map(|node| match node.kind {
                crate::project::ProjectNodeKind::Actor { actor_id }
                    if !existing_actor_ids.contains(&actor_id) =>
                {
                    Some(node.id)
                }

                _ => None,
            })
            .collect();

        for node_id in stale_actor_nodes {
            self.editor.project_hierarchy.remove_node(node_id);
        }

        for actor_id in existing_actor_ids {
            if self
                .editor
                .project_hierarchy
                .find_actor_node(actor_id)
                .is_none()
            {
                if let Err(error) = self
                    .editor
                    .project_hierarchy
                    .create_actor_node(None, actor_id)
                {
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Project",
                        format!(
                            "Failed to create Project Explorer Actor node {}: {}",
                            actor_id, error
                        ),
                    );
                }
            }
        }

        let actor_bindings = self.world.actor_script_bindings.clone();

        for binding in actor_bindings {
            if let Some(valid_paths) = indexed_script_paths.as_ref() {
                let normalized = binding
                    .normalized_script_path()
                    .replace('\\', "/")
                    .to_ascii_lowercase();

                if !valid_paths.contains(&normalized) {
                    continue;
                }
            }

            let matching_actor_node = self
                .editor
                .project_hierarchy
                .find_actor_node(binding.actor_id);

            let Some(actor_node_id) = matching_actor_node else {
                continue;
            };

            let already_represented =
                self.editor
                    .project_hierarchy
                    .nodes
                    .values()
                    .any(|node| match &node.kind {
                        crate::project::ProjectNodeKind::Script { script_path } => {
                            script_path == &binding.normalized_script_path()
                                && self.editor.project_hierarchy.attached_actor_id(node.id)
                                    == Some(binding.actor_id)
                        }

                        _ => false,
                    });

            if !already_represented {
                if let Err(error) = self
                    .editor
                    .project_hierarchy
                    .create_script_node(Some(actor_node_id), &binding.normalized_script_path())
                {
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Project",
                        format!(
                            "Failed to create Project Explorer Script node for Actor {}: {}",
                            binding.actor_id, error
                        ),
                    );
                }
            }
        }

        let indexed_script_absolute_paths = self.editor.script_editor.indexed_script_paths();

        for absolute_path in indexed_script_absolute_paths {
            let Ok(relative_path) = absolute_path.strip_prefix(&project_path) else {
                continue;
            };

            let logical_path = relative_path.to_string_lossy().replace('\\', "/");

            if self
                .editor
                .project_hierarchy
                .find_script_nodes(&logical_path)
                .is_empty()
            {
                if let Err(error) = self
                    .editor
                    .project_hierarchy
                    .create_script_node(None, &logical_path)
                {
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Project",
                        format!(
                            "Failed to create Project Explorer Script node '{}': {}",
                            logical_path, error
                        ),
                    );
                }
            }
        }

        // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
        // Existing Player Scripts files remain in playerscripts/; normalize
        // legacy Explorer nesting to the dedicated root without touching disk.
        let player_nodes = self
            .editor
            .project_hierarchy
            .nodes
            .values()
            .filter_map(|node| match &node.kind {
                crate::project::ProjectNodeKind::Script { script_path }
                    if script_path.starts_with("playerscripts/") && node.parent_id.is_some() =>
                {
                    Some(node.id)
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        for id in player_nodes {
            if self.editor.project_hierarchy.move_node(id, None).is_ok() {
                self.editor.needs_save = true;
            }
        }
        self.editor.project_cell_sync_revision = None;
        // Reconstruct meaningful Cell/Source references on load even if Explorer
        // has not been displayed yet. The World data remains authoritative.
        self.editor.synchronize_project_cell_nodes(&self.world);
        self.editor
            .synchronize_project_hierarchy_bindings(&mut self.world);

        match crate::project::load_editor_state(&project_path) {
            Ok(state) => {
                let active_script = state.active_script.clone();

                self.editor.apply_project_state(&state, &self.world);

                if let Some(logical_path) = active_script {
                    if let Some(path) = self.editor.script_editor.indexed_module_path(&logical_path)
                    {
                        self.editor.script_editor.open_file(path);
                    }
                }
            }

            Err(error) => {
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Project",
                    format!("Failed to load editor state: {}", error),
                );
                return;
            }
        }

        self.editor.ui_workspace =
            match crate::engine::ui_authoring::AuthoredUiWorkspace::load_from_project(&project_path)
            {
                Ok(workspace) => workspace,
                Err(error) => {
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Project",
                        format!("Failed to load project UI: {}", error),
                    );
                    return;
                }
            };
        self.editor.ui_document = self
            .editor
            .ui_workspace
            .active_document()
            .cloned()
            .unwrap_or_default();

        self.editor.ui_selected_node = None;
        self.editor.exit_ui_mode();

        // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:13 -05:00
        // Project/editor state restores selections, camera and panel sizes,
        // but it must never reopen a settings page or stale Script/Terrain
        // workspace by itself. Opening a project starts in the Home viewport.
        self.editor
            .activate_shell_workspace(crate::editor::editor::shell::Workspace::Home);

        let camera_path = project_path.join("camera.dat");

        if camera_path.exists() {
            match std::fs::read_to_string(&camera_path) {
                Ok(content) => match parse_camera_state(&content) {
                    Ok([yaw, pitch, distance, x, y, z]) => {
                        self.editor.camera.yaw = yaw;
                        self.editor.camera.pitch = pitch;
                        self.editor.camera.distance = distance;
                        self.editor.camera.target = glam::Vec3::new(x, y, z);
                    }
                    Err(error) => {
                        crate::engine::diagnostics::report(
                            "ERROR",
                            "Project",
                            format!("Invalid camera state {}: {}", camera_path.display(), error),
                        );
                        return;
                    }
                },
                Err(error) => {
                    crate::engine::diagnostics::report(
                        "ERROR",
                        "Project",
                        format!(
                            "Failed to load camera state {}: {}",
                            camera_path.display(),
                            error
                        ),
                    );
                    return;
                }
            }
        }

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:03 -05:00
        self.project_load_failed = false;
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
        match crate::project::atomic_file::snapshot_persisted_files(&project_path) {
            Ok(snapshot) => self.project_disk_snapshot = Some(snapshot),
            Err(error) => {
                self.project_load_failed = true;
                self.project_disk_snapshot = None;
                crate::engine::diagnostics::report(
                    "ERROR",
                    "Project",
                    format!("Failed to inventory persisted project files: {error}"),
                );
                return;
            }
        }
    }

    pub fn save_project(&mut self) {
        if let Err(error) = self.save_project_checked() {
            crate::engine::diagnostics::report("ERROR", "Project", error);
        }
    }

    // Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:36 -05:00
    pub fn save_project_checked(&mut self) -> Result<(), String> {
        // Worldkiln 0.8.5 | WO MAT-056 | 2026-10-10 13:51 -05:00
        self.editor.commit_pending_inspector_history();
        let Some(project_path) = self.project_manager.current_project.clone() else {
            self.autosave_last = std::time::Instant::now();
            return Ok(());
        };

        // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
        // Reject the entire save before any world, hierarchy, UI, or camera write.
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:03 -05:00
        if self.project_load_failed {
            self.autosave_last = std::time::Instant::now();
            return Err(format!(
                "Project save blocked because persisted project state failed to load: {}. Correct the project data and reload before saving.",
                project_path.display()
            ));
        }

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
        let Some(expected_snapshot) = self.project_disk_snapshot.as_ref() else {
            self.project_load_failed = true;
            self.autosave_last = std::time::Instant::now();
            return Err(format!(
                "Project save blocked because {} has no verified load snapshot. Reload the project before saving.",
                project_path.display()
            ));
        };
        let current_snapshot =
            match crate::project::atomic_file::snapshot_persisted_files(&project_path) {
                Ok(snapshot) => snapshot,
                Err(error) => {
                    self.project_load_failed = true;
                    self.autosave_last = std::time::Instant::now();
                    return Err(format!(
                        "Failed to check for external project changes: {error}"
                    ));
                }
            };
        let changed_paths: Vec<_> = expected_snapshot
            .keys()
            .chain(current_snapshot.keys())
            .collect::<std::collections::BTreeSet<_>>()
            .into_iter()
            .filter(|path| expected_snapshot.get(*path) != current_snapshot.get(*path))
            .map(|path| path.display().to_string())
            .collect();
        if !changed_paths.is_empty() {
            self.project_load_failed = true;
            self.autosave_last = std::time::Instant::now();
            return Err(format!(
                "Project save blocked because persisted files changed outside Worldkiln: {}. Reload the project before saving.",
                changed_paths.join(", ")
            ));
        }

        let mut failures = Vec::new();
        let world_path = project_path.join("world.dat");

        if let Err(error) = crate::world::persistence::save_world(&self.world, &world_path) {
            failures.push(format!("Failed to save world: {}", error));
        }

        // Worldkiln 0.8.5 | WO 0.8.7-78 | 2026-10-09 23:45 -05:00
        // Persist newly authored marker references even while Explorer is hidden.
        self.editor.synchronize_project_cell_nodes(&self.world);
        if let Err(error) =
            crate::project::save_project_metadata(&project_path, &self.editor.project_hierarchy)
        {
            failures.push(format!("Failed to save project metadata: {}", error));
        }

        self.editor.sync_active_ui_document();
        if let Err(error) = self.editor.ui_workspace.save_to_project(&project_path) {
            failures.push(format!("Failed to save project UI: {}", error));
        }

        let editor_state = self.editor.project_state(&project_path);

        if let Err(error) = crate::project::save_editor_state(&project_path, &editor_state) {
            failures.push(format!("Failed to save editor state: {}", error));
        }

        let camera_path = project_path.join("camera.dat");
        let camera = &self.editor.camera;
        let content = format!(
            "{} {} {} {} {} {}",
            camera.yaw,
            camera.pitch,
            camera.distance,
            camera.target.x,
            camera.target.y,
            camera.target.z,
        );

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:48 -05:00
        if let Err(error) =
            crate::project::atomic_file::write_atomically(&camera_path, content.as_bytes())
        {
            failures.push(format!("Failed to save camera: {}", error));
        }

        self.autosave_last = std::time::Instant::now();

        if failures.is_empty() {
            // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:50 -05:00
            match crate::project::atomic_file::snapshot_persisted_files(&project_path) {
                Ok(snapshot) => self.project_disk_snapshot = Some(snapshot),
                Err(error) => {
                    self.project_load_failed = true;
                    return Err(format!(
                        "Project saved but could not refresh file conflict baseline: {error}"
                    ));
                }
            }
            Ok(())
        } else {
            self.project_load_failed = true;
            Err(failures.join("\n"))
        }
    }

    pub fn build_game(&mut self) {
        let Some(project_path) = self.project_manager.current_project.clone() else {
            self.editor
                .append_output("\n[BUILD ERROR] No project is currently open.\n");

            self.editor.terminal_auto_scroll = true;

            self.play_engine_sound(EngineSound::BuildFailed);

            return;
        };

        // Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:37 -05:00
        if let Err(error) = self.editor.script_editor.save_all_checked() {
            self.editor.append_output(&format!(
                "\n[BUILD FAILED]\nBuild Game aborted because scripts could not be saved.\n{}\n",
                error
            ));
            self.editor.terminal_auto_scroll = true;
            self.play_engine_sound(EngineSound::BuildFailed);
            return;
        }

        if let Err(error) = self.save_project_checked() {
            self.editor.append_output(&format!(
                "\n[BUILD FAILED]\nBuild Game aborted because project data could not be saved.\n{}\n",
                error
            ));
            self.editor.terminal_auto_scroll = true;
            self.play_engine_sound(EngineSound::BuildFailed);
            return;
        }

        self.editor.append_output(
            "\n\
========================================\n\
WORLDKILN BUILD\n\
========================================\n\
BUILD STARTED\n\
",
        );

        self.editor
            .append_output(&format!("Project: {}\n", project_path.display()));

        self.editor.append_output("Configuration: Release\n");

        // Worldkiln 0.8.7 | WO 0.8.7-22 | 2026-10-06 11:24 -05:00
        self.editor
            .append_output("Target: Self-executable Worldkiln game\n");

        self.editor
            .append_output("----------------------------------------\n");

        self.play_engine_sound(EngineSound::BuildStarted);

        match crate::engine::build::build_game(&project_path) {
            Ok(result) => {
                if !result.output_log.trim().is_empty() {
                    self.editor.append_output(&result.output_log);

                    if !result.output_log.ends_with('\n') {
                        self.editor.terminal_output.push('\n');
                    }
                }

                self.editor
                    .append_output("Self-executable package complete.\n");

                self.editor.append_output(&format!(
                    "Game executable: {}\n",
                    result.executable.display()
                ));

                self.editor
                    .append_output("----------------------------------------\n");

                self.editor.append_output("BUILD COMPLETE\n");

                self.editor.append_output(&format!(
                    "Build location: {}\n",
                    result.output_dir.display()
                ));

                self.editor
                    .append_output("========================================\n");

                self.play_engine_sound(EngineSound::BuildComplete);
            }

            Err(error) => {
                self.editor.append_output(&format!(
                    "[BUILD FAILED]\n{}\n\
                         ========================================\n",
                    error
                ));

                self.play_engine_sound(EngineSound::BuildFailed);
            }
        }

        self.editor.terminal_auto_scroll = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::PathBuf;

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:32 -05:00
    #[test]
    fn camera_state_rejects_truncated_extra_and_non_finite_values() {
        assert_eq!(
            parse_camera_state("1 2 3 4 5 6").unwrap(),
            [1., 2., 3., 4., 5., 6.]
        );
        assert!(parse_camera_state("1 2 3 4 5").is_err());
        assert!(parse_camera_state("1 2 3 4 5 6 7").is_err());
        assert!(parse_camera_state("1 2 3 4 5 NaN").is_err());
        assert!(parse_camera_state("1 2 invalid 4 5 6").is_err());
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:49 -05:00
    #[test]
    fn switching_to_project_without_camera_state_resets_camera_to_defaults() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_mat002_camera_switch_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = root.join("first");
        let second = root.join("second");
        for project in [&first, &second] {
            fs::create_dir_all(project.join("scripts")).unwrap();
            crate::project::save_project_metadata(
                project,
                &crate::project::ProjectHierarchy::new(),
            )
            .unwrap();
            crate::project::save_editor_state(
                project,
                &crate::project::EditorProjectState::default(),
            )
            .unwrap();
            crate::engine::ui_authoring::AuthoredUiWorkspace::default()
                .save_to_project(project)
                .unwrap();
            crate::world::persistence::save_world(&World::new(), &project.join("world.dat"))
                .unwrap();
        }
        fs::write(first.join("camera.dat"), "1 2 3 4 5 6").unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(first);
        app.load_project();
        assert!(!app.project_load_failed);
        assert_eq!(app.editor.camera.yaw, 1.0);
        app.project_manager.current_project = Some(second);
        app.load_project();

        assert!(!app.project_load_failed);
        let defaults = crate::renderer::camera::CameraController::new();
        assert_eq!(app.editor.camera.yaw, defaults.yaw);
        assert_eq!(app.editor.camera.pitch, defaults.pitch);
        assert_eq!(app.editor.camera.distance, defaults.distance);
        assert_eq!(app.editor.camera.target, defaults.target);

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 13:14 -05:00
    #[test]
    fn switching_projects_a_to_b_to_a_reloads_each_authored_world() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_mat002_project_switch_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let first = root.join("first");
        let second = root.join("second");
        for (project, identity, position) in [
            (&first, "First", glam::Vec3::new(1.0, 2.0, 3.0)),
            (&second, "Second", glam::Vec3::new(7.0, 8.0, 9.0)),
        ] {
            fs::create_dir_all(project.join("scripts")).unwrap();
            let mut world = World::new();
            let actor_id = world.add_actor(identity, position);
            crate::world::persistence::save_world(&world, &project.join("world.dat")).unwrap();
            let mut hierarchy = crate::project::ProjectHierarchy::new();
            hierarchy.create_actor_node(None, actor_id).unwrap();
            crate::project::save_project_metadata(project, &hierarchy).unwrap();
            crate::project::save_editor_state(
                project,
                &crate::project::EditorProjectState::default(),
            )
            .unwrap();
            crate::engine::ui_authoring::AuthoredUiWorkspace::default()
                .save_to_project(project)
                .unwrap();
            fs::write(
                project.join("camera.dat"),
                format!("0 0 12 {} {} {}", position.x, position.y, position.z),
            )
            .unwrap();
        }

        let mut app = App::new_for_tests(1280.0, 720.0);
        for (project, identity, position) in [
            (&first, "First", glam::Vec3::new(1.0, 2.0, 3.0)),
            (&second, "Second", glam::Vec3::new(7.0, 8.0, 9.0)),
            (&first, "First", glam::Vec3::new(1.0, 2.0, 3.0)),
        ] {
            app.project_manager.current_project = Some(project.clone());
            app.editor.selected_coord = Some(crate::world::WorldCoord::new(99, 99, 99));
            app.editor.selected_actor_id = Some(u64::MAX);
            app.editor.selected_project_node_id = Some(u64::MAX);
            app.editor.hovered_actor_id = Some(u64::MAX);
            app.load_project();
            assert!(!app.project_load_failed);
            assert_eq!(app.editor.selected_coord, None);
            assert_eq!(app.editor.selected_actor_id, None);
            assert_eq!(app.editor.selected_project_node_id, None);
            assert_eq!(app.editor.hovered_actor_id, None);
            let actor = app.world.actors.values().next().unwrap();
            assert_eq!(actor.identity, identity);
            assert_eq!(actor.position, position);
            assert_eq!(app.editor.camera.target, position);
        }

        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 13:20 -05:00
        // A failed switch must not leave the last valid project's scene or UI visible.
        fs::write(second.join("world.dat"), b"WORLD_FORMAT 999").unwrap();
        app.project_manager.current_project = Some(second.clone());
        app.editor.selected_actor_id = Some(u64::MAX);
        app.editor.selected_coord = Some(crate::world::WorldCoord::new(99, 99, 99));
        app.load_project();
        assert!(app.project_load_failed);
        assert!(app.world.actors.is_empty());
        assert!(app.world.cells.is_empty());
        assert!(app.editor.project_hierarchy.nodes.is_empty());
        assert_eq!(app.editor.selected_actor_id, None);
        assert_eq!(app.editor.selected_coord, None);

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:52 -05:00
    #[test]
    fn external_edit_blocks_project_save_and_exit_preserves_loaded_session() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_mat002_external_edit_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        crate::project::save_project_metadata(&root, &crate::project::ProjectHierarchy::new())
            .unwrap();
        crate::project::save_editor_state(&root, &crate::project::EditorProjectState::default())
            .unwrap();
        crate::engine::ui_authoring::AuthoredUiWorkspace::default()
            .save_to_project(&root)
            .unwrap();
        crate::world::persistence::save_world(&World::new(), &root.join("world.dat")).unwrap();
        fs::write(root.join("camera.dat"), "1 2 3 4 5 6").unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project();
        assert!(!app.project_load_failed);
        app.editor.camera.yaw = 0.5;
        let external_camera = b"7 8 9 10 11 12";
        fs::write(root.join("camera.dat"), external_camera).unwrap();

        let error = app.save_project_checked().unwrap_err();
        assert!(error.contains("changed outside Worldkiln"), "{error}");
        assert!(error.contains("camera.dat"), "{error}");
        assert_eq!(fs::read(root.join("camera.dat")).unwrap(), external_camera);
        assert!(app.project_load_failed);

        app.exit_to_home();
        assert_eq!(app.project_manager.current_project.as_ref(), Some(&root));
        assert!(app.project_load_failed);

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
    fn failed_load_fixture(label: &str) -> (PathBuf, Vec<u8>) {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_wo53_{label}_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join(".assets")).unwrap();
        fs::write(root.join("scripts/preserved.aeo"), "print(\"preserved\")\n").unwrap();
        fs::write(root.join(".assets/preserved.bin"), [1, 2, 3, 4]).unwrap();
        fs::write(root.join("camera.dat"), "1 2 3 4 5 6").unwrap();
        crate::project::save_project_metadata(&root, &crate::project::ProjectHierarchy::new())
            .unwrap();
        crate::project::save_editor_state(&root, &crate::project::EditorProjectState::default())
            .unwrap();
        crate::engine::ui_authoring::AuthoredUiWorkspace::default()
            .save_to_project(&root)
            .unwrap();

        let mut world = World::new();
        world.set_cell(
            crate::world::WorldCoord::new(50, 0, 50),
            crate::world::CellType::Block,
        );
        world
            .terrain
            .generate_authored(crate::world::TerrainGenerationRequest {
                position: [0, 0, 0],
                size: [8, 16, 8],
            })
            .unwrap();
        crate::world::persistence::save_world(&world, &root.join("world.dat")).unwrap();
        let metadata = fs::read(root.join("terrain/terrain.json")).unwrap();
        fs::write(root.join("terrain/terrain.json"), "{ invalid json").unwrap();
        (root, metadata)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
    fn project_file_bytes(root: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
        let mut files = std::collections::BTreeMap::new();
        let mut directories = vec![root.to_path_buf()];
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                if entry.file_type().unwrap().is_dir() {
                    directories.push(path);
                } else {
                    files.insert(
                        path.strip_prefix(root).unwrap().to_path_buf(),
                        fs::read(path).unwrap(),
                    );
                }
            }
        }
        files
    }

    // Worldkiln 0.8.5 | WO 0.8.7-74 | 2026-10-09 22:29 -05:00
    #[test]
    fn reopening_legacy_project_settings_state_always_enters_home_viewport_without_data_changes() {
        use crate::editor::editor::feature_nav::{EditorSurface, ProjectSection};
        use crate::editor::editor::shell::Workspace;
        use crate::editor::editor::{LeftPanelTab, RightPanelTab};
        let root = std::env::temp_dir().join(format!(
            "worldkiln_wo74_start_home_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        let script = root.join("scripts/keep.aeo");
        fs::write(&script, "print(\"keep user data\")\n").unwrap();
        let mut world = World::new();
        let id = world.add_actor("Kept Actor", glam::Vec3::new(2.0, 3.0, 4.0));
        crate::world::persistence::save_world(&world, &root.join("world.dat")).unwrap();
        let mut hierarchy = crate::project::ProjectHierarchy::new();
        let node = hierarchy.create_actor_node(None, id).unwrap();
        crate::project::save_project_metadata(&root, &hierarchy).unwrap();
        let mut legacy = crate::project::EditorProjectState::default();
        legacy.left_panel_tab = "project_settings".into();
        legacy.show_script_workspace = true;
        legacy.selected_actor_id = Some(id);
        legacy.selected_project_node_id = Some(node);
        legacy.anchor = [6, 8, 10];
        crate::project::save_editor_state(&root, &legacy).unwrap();
        let state_file = root.join("editor_state.json");
        let project_data_before = fs::read(root.join("project.json")).unwrap();
        let world_data_before = fs::read(root.join("world.dat")).unwrap();
        let script_before = fs::read(&script).unwrap();
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.editor.shell.workspace = Workspace::Terrain;
        app.editor.shell.explorer_width = 295.0;
        app.editor.shell.properties_width = 360.0;
        app.editor.open_project_settings(ProjectSection::Physics);
        app.project_manager.current_project = Some(root.clone());
        app.load_project();
        assert!(!app.project_load_failed);
        assert_eq!(app.editor.navigation.surface, EditorSurface::Workspace);
        assert_eq!(app.editor.shell.workspace, Workspace::Home);
        assert_eq!(app.editor.left_panel_tab, LeftPanelTab::Project);
        assert_eq!(app.editor.right_panel_tab, RightPanelTab::Inspector);
        assert!(app.editor.shell.explorer_visible);
        assert!(app.editor.shell.properties_visible);
        assert!(!app.editor.show_script_workspace && !app.editor.ui_mode);
        assert_eq!(app.editor.selected_actor_id, Some(id));
        assert_eq!(app.editor.anchor, crate::world::WorldCoord::new(6, 8, 10));
        // The loader's authored transform contract is independently covered
        // by world persistence tests; this navigation regression verifies no
        // scene file was rewritten and the original Actor identity remains.
        assert!(app.world.actors.contains_key(&id));
        assert_eq!(app.editor.shell.explorer_width, 295.0);
        assert_eq!(app.editor.shell.properties_width, 360.0);
        assert_eq!(
            fs::read(root.join("project.json")).unwrap(),
            project_data_before
        );
        assert_eq!(fs::read(root.join("world.dat")).unwrap(), world_data_before);
        assert_eq!(fs::read(&script).unwrap(), script_before);
        // Loading the saved legacy view must not rewrite the old project
        // editor-state file just to select a different screen.
        let _ = state_file; // project authoring files remain unmodified.
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
    #[test]
    fn failed_world_load_preserves_all_files_across_save_autosave_and_exit() {
        let (root, valid_metadata) = failed_load_fixture("save_guard");
        let original_files = project_file_bytes(&root);
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project();
        assert!(app.project_load_failed);
        assert!(app.world.terrain.authored.is_none());
        assert!(app.world.cells.is_empty());

        let error = app.save_project_checked().unwrap_err();
        assert!(
            error.contains("persisted project state failed to load"),
            "{error}"
        );
        app.save_project();
        assert_eq!(project_file_bytes(&root), original_files);

        app.view = View::Editor;
        app.editor.theme_settings.autosave =
            crate::engine::preferences::AutosaveInterval::OneMinute;
        app.last_autosave_interval = crate::engine::preferences::AutosaveInterval::OneMinute;
        app.autosave_last = std::time::Instant::now() - std::time::Duration::from_secs(61);
        app.check_autosave();
        assert_eq!(project_file_bytes(&root), original_files);

        app.exit_to_home();
        assert_eq!(project_file_bytes(&root), original_files);
        // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 13:17 -05:00
        // A blocked exit retains the project session so the user can repair it.
        assert!(app.project_load_failed);
        assert_eq!(app.project_manager.current_project.as_ref(), Some(&root));

        app.project_manager.current_project = Some(root.clone());
        app.load_project();
        assert!(app.project_load_failed);
        fs::write(root.join("terrain/terrain.json"), valid_metadata).unwrap();
        app.load_project();
        assert!(!app.project_load_failed);
        assert!(app.world.terrain.authored.is_some());
        assert!(
            app.world
                .get(crate::world::WorldCoord::new(50, 0, 50))
                .is_some()
        );
        let expected_terrain = app.world.terrain.authored.clone();
        app.save_project_checked().unwrap();
        let mut loaded = World::new();
        crate::world::persistence::load_world(&mut loaded, &root.join("world.dat")).unwrap();
        assert_eq!(loaded.terrain.authored, expected_terrain);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.7 | WO 0.8.7-53 | 2026-10-09 10:13 -05:00
    #[test]
    fn failed_world_load_guard_resets_when_loading_a_new_empty_project() {
        let (root, _) = failed_load_fixture("new_project_guard");
        let original_files = project_file_bytes(&root);
        let new_project = root.join("new_project");
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project();
        assert!(app.project_load_failed);
        assert!(app.save_project_checked().is_err());
        assert_eq!(project_file_bytes(&root), original_files);

        fs::create_dir_all(new_project.join("scripts")).unwrap();
        app.project_manager.current_project = Some(new_project.clone());
        app.load_project();
        assert!(!app.project_load_failed);
        app.save_project_checked().unwrap();
        assert!(new_project.join("world.dat").is_file());
        assert_eq!(
            fs::read(root.join("world.dat")).unwrap(),
            original_files[&PathBuf::from("world.dat")]
        );
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:03 -05:00
    #[test]
    fn malformed_project_and_packages_are_visible_in_output() {
        crate::engine::diagnostics::clear();
        let root = std::env::temp_dir().join(format!(
            "worldkiln_wo04_project_diagnostics_{}",
            std::process::id()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        for directory in ["characters", "entities"] {
            let package = root.join(".assets").join(directory).join("broken");
            fs::create_dir_all(&package).unwrap();
            fs::write(package.join("package.json"), "{ invalid json").unwrap();
        }
        let corrupt_project_metadata = b"{ invalid json";
        fs::write(root.join("project.json"), corrupt_project_metadata).unwrap();
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.editor.script_editor.show_output = false;
        app.load_project();
        app.editor.collect_diagnostics();
        assert!(app.editor.script_editor.show_output);
        let output = &app.editor.terminal_output;
        assert!(
            output.contains("[ERROR] [Project] Failed to load project.json"),
            "{output}"
        );
        assert!(
            output.contains("[ERROR] [Package] Failed to load character package"),
            "{output}"
        );
        assert!(
            output.contains("[ERROR] [Package] Failed to load entity package"),
            "{output}"
        );
        assert!(app.project_load_failed);
        assert!(app.save_project_checked().is_err());
        assert_eq!(
            fs::read(root.join("project.json")).unwrap(),
            corrupt_project_metadata
        );
        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

    // Worldkiln 0.8.5 | WO MAT-002 | 2026-10-10 12:32 -05:00
    #[test]
    fn malformed_camera_state_blocks_project_save_without_rewriting_it() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_mat002_camera_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        fs::create_dir_all(root.join("playerscripts")).unwrap();
        fs::write(root.join("camera.dat"), "1 2 invalid 4 5 6").unwrap();
        crate::project::save_project_metadata(&root, &crate::project::ProjectHierarchy::new())
            .unwrap();
        crate::project::save_editor_state(&root, &crate::project::EditorProjectState::default())
            .unwrap();
        crate::engine::ui_authoring::AuthoredUiWorkspace::default()
            .save_to_project(&root)
            .unwrap();
        crate::world::persistence::save_world(&World::new(), &root.join("world.dat")).unwrap();
        let original_camera = fs::read(root.join("camera.dat")).unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project();

        assert!(app.project_load_failed);
        assert!(app.save_project_checked().is_err());
        assert_eq!(fs::read(root.join("camera.dat")).unwrap(), original_camera);

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn project_exit_clears_script_editor_document_identity() {
        let test_dir = PathBuf::from(format!("TestProject_ExitIdentity_{}", std::process::id()));
        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }
        fs::create_dir_all(test_dir.join("scripts")).unwrap();
        let path = test_dir.join("scripts/test.aeo");
        fs::write(&path, "entity Test {}").unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(test_dir.clone());
        app.load_project();
        app.editor.script_editor.open_file(path.clone());

        assert!(!app.editor.script_editor.open_documents.is_empty());
        assert!(app.editor.script_editor.active_document.is_some());

        app.exit_to_home();

        assert!(app.editor.script_editor.open_documents.is_empty());
        assert!(app.editor.script_editor.active_document.is_none());
        assert!(app.editor.script_editor.project_script_index().is_none());
        assert!(app.editor.script_editor.scripts_list.is_empty());
        assert!(app.editor.script_editor.playerscripts_list.is_empty());

        fs::remove_dir_all(&test_dir).ok();
    }

    #[test]
    fn project_b_never_retains_project_a_active_document() {
        let test_dir_a = PathBuf::from(format!("TestProject_ProjectA_{}", std::process::id()));
        let test_dir_b = PathBuf::from(format!("TestProject_ProjectB_{}", std::process::id()));

        if test_dir_a.exists() {
            fs::remove_dir_all(&test_dir_a).ok();
        }
        if test_dir_b.exists() {
            fs::remove_dir_all(&test_dir_b).ok();
        }

        fs::create_dir_all(test_dir_a.join("scripts")).unwrap();
        fs::create_dir_all(test_dir_b.join("scripts")).unwrap();

        let script_a = test_dir_a.join("scripts/project_a.aeo");
        let script_b = test_dir_b.join("scripts/project_b.aeo");

        fs::write(&script_a, "entity ProjectA {}").unwrap();
        fs::write(&script_b, "entity ProjectB {}").unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);

        app.project_manager.current_project = Some(test_dir_a.clone());
        app.load_project();
        app.editor.script_editor.open_file(script_a.clone());

        assert_eq!(
            app.editor.script_editor.active_document,
            Some(script_a.clone())
        );

        app.exit_to_home();

        assert!(app.editor.script_editor.open_documents.is_empty());
        assert!(app.editor.script_editor.active_document.is_none());

        app.project_manager.current_project = Some(test_dir_b.clone());
        app.load_project();

        assert!(
            app.editor
                .script_editor
                .open_documents
                .keys()
                .all(|path| path.starts_with(&test_dir_b))
        );
        assert_ne!(app.editor.script_editor.active_document, Some(script_a));

        fs::remove_dir_all(&test_dir_a).ok();
        fs::remove_dir_all(&test_dir_b).ok();
    }

    #[test]
    fn reload_reconciles_stale_script_nodes() {
        let test_dir = PathBuf::from(format!("TestProject_StaleReconcile_{}", std::process::id()));
        if test_dir.exists() {
            fs::remove_dir_all(&test_dir).ok();
        }
        fs::create_dir_all(test_dir.join("scripts")).unwrap();

        let real_path = test_dir.join("scripts/script_renamed.aeo");
        fs::write(&real_path, "entity Renamed {}").unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(test_dir.clone());

        // Create metadata referencing stale script.aeo instead of script_renamed.aeo
        let mut hierarchy = crate::project::ProjectHierarchy::new();
        hierarchy
            .create_script_node(None, "scripts/script.aeo")
            .unwrap();
        crate::project::save_project_metadata(&test_dir, &hierarchy).unwrap();

        app.load_project();

        assert!(
            app.editor
                .project_hierarchy
                .find_script_nodes("scripts/script.aeo")
                .is_empty()
        );
        assert_eq!(
            app.editor
                .project_hierarchy
                .find_script_nodes("scripts/script_renamed.aeo")
                .len(),
            1
        );

        fs::remove_dir_all(&test_dir).ok();
    }
}
