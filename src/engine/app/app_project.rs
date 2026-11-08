// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use crate::engine::audio::EngineSound;
use crate::world::World;

use super::{App, EditorMode, View};

pub(super) fn parse_camera_state(content: &str) -> Result<[f32; 6], String> {
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
        if let Some(loader) = self.project_loader.as_mut() {
            loader.cancel();
        }
        self.project_loading_screen = None;

        self.editor.mode = EditorMode::Editor;
        self.editor.exit_ui_mode();

        self.editor.script_editor.save_all();

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

        if let Err(error) = self.save_project_checked() {
            crate::engine::diagnostics::report("ERROR", "Project", error);
            return;
        }

        self.stop_scripting();
        self.last_mode = EditorMode::Editor;

        self.editor.script_editor.reset_project_session();
        self.editor.asset_manager = crate::editor::asset_management::AssetManagerState::default();

        self.editor.show_script_workspace = false;
        self.editor.last_show_script_workspace = false;

        self.renderer.clear_chunk_cache();
        self.renderer.set_project_asset_root(None);

        self.world = World::new();
        self.editor.terrain_paint_layer_id = "surface".into();

        self.physics_world.bodies.clear();

        self.character_system.clear();
        self.actor_system.clear();

        self.project_manager.current_project = None;
        self.project_load_failed = false;
        self.project_disk_snapshot = None;
        self.live_project_sync = None;

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);

        self.editor.history.undo_stack.clear();
        self.editor.history.redo_stack.clear();
        self.editor.pending_inspector_history = None;
        self.editor.clear_clipboard();

        self.editor.project_hierarchy = crate::project::ProjectHierarchy::new();
        self.editor.project_actor_sync_revision = None;
        self.editor.project_cell_sync_revision = None;

        self.editor.selected_project_node_id = None;
        self.editor.selected_project_nodes.clear();
        self.editor.expanded_project_nodes.clear();

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

        self.loaded_project_path = None;
        self.view = View::Home;
    }

    pub fn load_project(&mut self) {
        if let Some(path) = self.project_manager.current_project.clone() {
            self.queue_project_load(path, false);
        }
    }
    pub(crate) fn request_open_project(&mut self, path: std::path::PathBuf) {
        self.queue_project_load(path, true);
    }
    fn queue_project_load(&mut self, path: std::path::PathBuf, prepare_open: bool) {
        if self.loaded_project_path.is_some()
            && (self.editor.needs_save
                || self
                    .editor
                    .script_editor
                    .open_documents
                    .values()
                    .any(|d| d.dirty))
        {
            self.project_manager.current_project = self.loaded_project_path.clone();
            crate::engine::diagnostics::report(
                "ERROR",
                "Project",
                "Project switch blocked: save current project changes first".into(),
            );
            return;
        }
        if self.loaded_project_path.is_some() {
            self.project_manager.current_project = self.loaded_project_path.clone();
        }
        let loader = self
            .project_loader
            .get_or_insert_with(super::background_project::BackgroundProjectLoader::new);
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("Project")
            .to_owned();
        loader.request(path, prepare_open);
        self.project_load_error = None;
        self.project_loading_screen = Some((std::time::Instant::now(), name));
        self.development_project_load_started = (self.development_capture.borrow().state()
            == crate::engine::development_test_run::CaptureState::Running)
            .then(std::time::Instant::now);
        self.view = View::Home;
    }
    pub(crate) fn project_loading(&self) -> bool {
        self.project_loader
            .as_ref()
            .is_some_and(|loader| loader.pending.is_some())
    }
    pub(crate) fn poll_project_load(&mut self) {
        let Some((result, diagnostics)) = self
            .project_loader
            .as_mut()
            .and_then(|loader| loader.poll())
        else {
            return;
        };
        self.project_loading_screen = None;
        if let Some(started) = self.development_project_load_started.take() {
            let operation = if result.is_ok() {
                "project_load_wall"
            } else {
                "project_load_failed_wall"
            };
            self.development_capture
                .borrow_mut()
                .record("project", operation, started.elapsed());
        }
        for message in diagnostics {
            self.editor.append_output(&format!("{message}\n"));
            self.editor.script_editor.show_output = true;
        }
        let prepared = match result {
            Ok(project) => project,
            Err(error) => {
                self.project_load_failed = self.loaded_project_path.is_none();
                let stage = self
                    .project_loader
                    .as_ref()
                    .map(|loader| loader.progress().current)
                    .unwrap_or("Preparing project");
                let message = format!("Project open failed during {stage}: {error}");
                eprintln!("[PROJECT LOAD ERROR] {message}");
                self.editor.append_output(&format!("{message}\n"));
                self.editor.script_editor.show_output = true;
                self.project_load_error = Some(message.clone());
                crate::engine::diagnostics::report("ERROR", "Project", message);
                return;
            }
        };
        let publish_started = std::time::Instant::now();
        let project_path = prepared.path.clone();
        self.stop_scripting();
        crate::engine::diagnostics::clear();
        self.editor.mode = EditorMode::Editor;
        self.last_mode = EditorMode::Editor;
        let old_world = std::mem::replace(&mut self.world, prepared.world);
        self.editor.terrain_paint_layer_id = self
            .world
            .terrain
            .config
            .terrain_material_layers()
            .first()
            .map(|l| l.id.clone())
            .unwrap_or_default();
        self.project_loader.as_ref().unwrap().retire(old_world);
        self.physics_world.bodies.clear();
        self.character_system.clear();
        self.actor_system.clear();
        self.renderer.clear_chunk_cache();
        self.editor.project_hierarchy = crate::project::ProjectHierarchy::new();
        self.editor.project_actor_sync_revision = None;
        self.editor.project_cell_sync_revision = None;
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
        self.editor.camera = crate::renderer::camera::CameraController::new();
        self.editor.history.undo_stack.clear();
        self.editor.history.redo_stack.clear();
        self.editor.pending_inspector_history = None;
        self.editor.ui_document_history.clear();

        self.editor.asset_manager = crate::editor::asset_management::AssetManagerState::default();

        self.editor.water_undo.clear();
        self.editor.water_redo.clear();
        eprintln!(
            "[PROJECT LOAD] publish reset: {:.1} ms",
            publish_started.elapsed().as_secs_f64() * 1000.0
        );
        self.renderer
            .set_project_asset_root_with_materials(Some(&project_path), prepared.materials);
        eprintln!(
            "[PROJECT LOAD] publish materials: {:.1} ms",
            publish_started.elapsed().as_secs_f64() * 1000.0
        );
        crate::character::package::publish_packages(prepared.characters);
        crate::engine::entity_package::publish_packages(prepared.entities);
        self.editor.project_actor_sync_revision = None;
        self.editor.project_cell_sync_revision = None;
        self.editor.project_hierarchy = prepared.hierarchy;
        let show_output = self.editor.script_editor.show_output;
        self.editor.script_editor = prepared.scripts.publish();
        self.editor.script_editor.show_output |= show_output;
        self.editor.editor_max_distance_chunks = self.world.render_distance.editor_chunks;
        self.editor.play_max_distance_chunks = self.world.render_distance.play_chunks;
        self.editor
            .apply_project_state(&prepared.editor_state, &self.world);
        eprintln!(
            "[PROJECT LOAD] publish editor state: {:.1} ms",
            publish_started.elapsed().as_secs_f64() * 1000.0
        );
        self.renderer.publish_project_terrain(prepared.terrain);
        eprintln!(
            "[PROJECT LOAD] publish terrain: {:.1} ms",
            publish_started.elapsed().as_secs_f64() * 1000.0
        );
        self.editor.ui_workspace = prepared.ui;
        self.editor.ui_document = self
            .editor
            .ui_workspace
            .active_document()
            .cloned()
            .unwrap_or_default();
        self.editor.exit_ui_mode();
        self.editor
            .activate_shell_workspace(crate::editor::editor::shell::Workspace::Home);
        if let Some([yaw, pitch, distance, x, y, z]) = prepared.camera {
            self.editor.camera.yaw = yaw;
            self.editor.camera.pitch = pitch;
            self.editor.camera.distance = distance;
            self.editor.camera.target = glam::Vec3::new(x, y, z);
        }
        self.editor.needs_save = prepared.needs_save;
        self.project_disk_snapshot = Some(prepared.disk_snapshot);
        self.project_load_failed = false;
        self.project_load_error = None;
        self.loaded_project_path = Some(project_path.clone());
        self.project_manager
            .publish_open_project(project_path.clone());
        self.reset_live_project_sync();
        crate::project::asset_registry::AssetRegistry::begin_background_index(project_path);
        self.project_loader.as_ref().unwrap().record_recent(
            self.project_manager.user_data_dir.clone(),
            self.project_manager.recent_projects.clone(),
        );
        self.autosave_last = std::time::Instant::now();
        self.last_frame_instant = std::time::Instant::now();
        self.view = View::Editor;
        self.play_engine_sound(EngineSound::ProjectOpened);
        eprintln!(
            "[PROJECT LOAD] publish completed: {:.1} ms",
            publish_started.elapsed().as_secs_f64() * 1000.0
        );
    }
    #[cfg(test)]
    fn load_project_and_wait_for_tests(&mut self) {
        self.load_project();
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(30);
        while self.project_loading() {
            assert!(
                std::time::Instant::now() < deadline,
                "background load timed out"
            );
            self.poll_project_load();
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }

    pub(crate) fn save_all_authored_checked(&mut self) -> Result<(), String> {
        self.editor.script_editor.save_all_checked_quiet()?;
        self.save_project_checked()?;
        self.last_autosave_failure = None;
        Ok(())
    }

    pub(crate) fn save_all_authored(&mut self) {
        if let Err(error) = self.save_all_authored_checked() {
            crate::engine::diagnostics::report("ERROR", "Project", error);
        }
    }

    pub fn save_project(&mut self) {
        let save_started = (self.development_capture.borrow().state()
            == crate::engine::development_test_run::CaptureState::Running)
            .then(std::time::Instant::now);
        let result = self.save_project_checked();
        if let Some(started) = save_started {
            self.development_capture.borrow_mut().record(
                "project",
                if result.is_ok() {
                    "project_save_cpu"
                } else {
                    "project_save_failed_cpu"
                },
                started.elapsed(),
            );
        }
        if let Err(error) = result {
            crate::engine::diagnostics::report("ERROR", "Project", error);
        }
    }

    pub fn save_project_checked(&mut self) -> Result<(), String> {
        if self.project_loading() {
            return Err("Project save blocked while background loading is pending".into());
        }
        self.editor.commit_pending_inspector_history();
        let Some(project_path) = self.project_manager.current_project.clone() else {
            self.autosave_last = std::time::Instant::now();
            return Ok(());
        };

        if self.project_load_failed {
            self.autosave_last = std::time::Instant::now();
            return Err(format!(
                "Project save blocked because persisted project state failed to load: {}. Correct the project data and reload before saving.",
                project_path.display()
            ));
        }

        let _ = self.refresh_external_scene_before_save();

        let current_snapshot = self.project_disk_snapshot.clone().unwrap_or_default();

        if let Err(error) = crate::project::atomic_file::preflight_project_save_targets(
            &project_path,
            &current_snapshot,
        ) {
            self.autosave_last = std::time::Instant::now();
            return Err(format!("Project save cannot proceed: {error}"));
        }

        let mut failures = Vec::new();
        let world_path = project_path.join("world.dat");

        if let Err(error) = crate::world::persistence::save_world(&self.world, &world_path) {
            failures.push(format!("Failed to save world: {}", error));
        }

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

        if let Err(error) =
            crate::project::atomic_file::write_atomically(&camera_path, content.as_bytes())
        {
            failures.push(format!("Failed to save camera: {}", error));
        }

        self.autosave_last = std::time::Instant::now();

        if failures.is_empty() {
            match crate::project::atomic_file::snapshot_persisted_files(&project_path) {
                Ok(snapshot) => self.project_disk_snapshot = Some(snapshot),
                Err(error) => {
                    return Err(format!(
                        "Project saved but could not refresh file conflict baseline: {error}"
                    ));
                }
            }
            self.editor.needs_save = false;
            self.reset_live_project_sync();
            Ok(())
        } else {
            Err(failures.join("\n"))
        }
    }

    pub(crate) fn can_close_editor(&mut self) -> bool {
        let scripts_dirty = self
            .editor
            .script_editor
            .open_documents
            .values()
            .any(|d| d.dirty);
        if !scripts_dirty && !self.editor.needs_save {
            return true;
        }
        if self.project_manager.current_project.is_none() || self.project_load_failed {
            if scripts_dirty {
                if let Err(error) = self.editor.script_editor.save_all_checked_quiet() {
                    crate::engine::diagnostics::report("ERROR", "Project", error);
                    return false;
                }
            }
            return true;
        }
        match self.save_all_authored_checked() {
            Ok(()) => true,
            Err(error) => {
                crate::engine::diagnostics::report("ERROR", "Project", error);
                false
            }
        }
    }

    pub(crate) fn poll_game_build(&mut self) {
        let finished = self
            .game_build_job
            .as_ref()
            .and_then(|job| match job.try_recv() {
                Ok(result) => Some(result),
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    Some(Err("Build worker ended without a result".into()))
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => None,
            });
        if let Some(result) = finished {
            self.game_build_job = None;
            let can_run = result.as_ref().ok().map(|r| r.executable.clone());
            self.packaging_status = if result.is_ok() {
                "BUILD COMPLETE"
            } else {
                "BUILD FAILED"
            }
            .into();
            self.finish_game_build(result);
            if self.packaging_run_after {
                if let Some(executable) = can_run {
                    match std::process::Command::new(&executable)
                        .current_dir(executable.parent().unwrap_or(std::path::Path::new(".")))
                        .spawn()
                    {
                        Ok(_) => self.editor.append_output("BUILD & RUN: game launched.\n"),
                        Err(error) => self
                            .editor
                            .append_output(&format!("BUILD & RUN: launch failed: {error}\n")),
                    }
                }
                self.packaging_run_after = false;
            }
        }
    }

    pub fn build_game(&mut self) {
        if self.game_build_job.is_some() {
            return;
        }
        self.last_built_zip = None;
        let Some(project_path) = self.project_manager.current_project.clone() else {
            self.editor
                .append_output("\n[BUILD ERROR] No project is currently open.\n");

            self.editor.terminal_auto_scroll = true;

            self.play_engine_sound(EngineSound::BuildFailed);

            return;
        };

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

        self.editor
            .append_output("Target: Self-executable Worldkiln game\n");

        self.editor
            .append_output("----------------------------------------\n");

        self.play_engine_sound(EngineSound::BuildStarted);

        let (sender, receiver) = std::sync::mpsc::channel();
        self.game_build_job = Some(receiver);
        self.packaging_status = "Building ? preparing game content".into();
        self.packaging_cancel
            .store(false, std::sync::atomic::Ordering::Relaxed);
        let opts = self.packaging_options.clone();
        let cancel = self.packaging_cancel.clone();
        std::thread::spawn(move || {
            let result =
                crate::engine::build::build_game_with_cancel(&project_path, &opts, &cancel);
            let _ = sender.send(result);
        });
        self.editor.terminal_auto_scroll = true;
    }

    fn finish_game_build(&mut self, result: Result<crate::engine::build::BuildResult, String>) {
        match result {
            Ok(result) => {
                self.last_built_zip = result.archive_path.clone();
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
                self.last_built_zip = None;
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

    #[test]
    fn clean_autosave_does_not_write_or_log() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_clean_autosave_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.view = View::Editor;
        app.editor.needs_save = false;
        app.editor.theme_settings.autosave =
            crate::engine::preferences::AutosaveInterval::OneMinute;
        app.last_autosave_interval = crate::engine::preferences::AutosaveInterval::OneMinute;
        app.autosave_last = std::time::Instant::now() - std::time::Duration::from_secs(61);
        let prior_messages = app.editor.script_editor.output_log.len();
        app.check_autosave();
        assert!(!root.join("world.dat").exists());
        assert_eq!(app.editor.script_editor.output_log.len(), prior_messages);
        assert!(app.last_autosave_failure.is_none());
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn dirty_autosave_persists_scene_without_success_notification() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_dirty_autosave_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(&root).unwrap();
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.view = View::Editor;
        app.editor.needs_save = true;
        app.editor.theme_settings.autosave =
            crate::engine::preferences::AutosaveInterval::OneMinute;
        app.last_autosave_interval = crate::engine::preferences::AutosaveInterval::OneMinute;
        app.autosave_last = std::time::Instant::now() - std::time::Duration::from_secs(61);
        let prior_messages = app.editor.script_editor.output_log.len();
        app.check_autosave();
        assert!(root.join("world.dat").is_file());
        assert!(!app.editor.needs_save);
        assert_eq!(app.editor.script_editor.output_log.len(), prior_messages);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn repeated_autosave_failure_reports_only_once() {
        let root = std::env::temp_dir().join(format!(
            "worldkiln_failed_autosave_{}_{}",
            std::process::id(),
            rand::random::<u64>()
        ));
        fs::create_dir_all(root.join("world.dat")).unwrap();
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.view = View::Editor;
        app.editor.needs_save = true;
        app.editor.theme_settings.autosave =
            crate::engine::preferences::AutosaveInterval::OneMinute;
        app.last_autosave_interval = crate::engine::preferences::AutosaveInterval::OneMinute;
        app.autosave_last = std::time::Instant::now() - std::time::Duration::from_secs(61);
        let before = app.editor.script_editor.output_log.len();
        app.check_autosave();
        assert!(app.last_autosave_failure.is_some());
        assert_eq!(app.editor.script_editor.output_log.len(), before + 1);
        app.autosave_last -= std::time::Duration::from_secs(61);
        app.check_autosave();
        assert_eq!(app.editor.script_editor.output_log.len(), before + 1);
        fs::remove_dir_all(root).unwrap();
    }

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
        app.load_project_and_wait_for_tests();
        assert!(!app.project_load_failed);
        assert_eq!(app.editor.camera.yaw, 1.0);
        app.save_project_checked().unwrap();
        app.project_manager.current_project = Some(second);
        app.load_project_and_wait_for_tests();

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
            if app.loaded_project_path.is_some() {
                app.save_project_checked().unwrap();
            }
            app.project_manager.current_project = Some(project.clone());
            app.editor.selected_coord = Some(crate::world::WorldCoord::new(99, 99, 99));
            app.editor.selected_actor_id = Some(u64::MAX);
            app.editor.selected_project_node_id = Some(u64::MAX);
            app.editor.hovered_actor_id = Some(u64::MAX);
            app.load_project_and_wait_for_tests();
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

        fs::write(second.join("world.dat"), b"WORLD_FORMAT 999").unwrap();
        app.project_manager.current_project = Some(second.clone());
        app.load_project_and_wait_for_tests();
        assert!(!app.project_load_failed);
        assert_eq!(app.project_manager.current_project, Some(first.clone()));
        assert_eq!(app.world.actors.values().next().unwrap().identity, "First");
        assert_eq!(
            fs::read(second.join("world.dat")).unwrap(),
            b"WORLD_FORMAT 999"
        );

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

    #[cfg(windows)]
    #[test]
    fn locked_ui_file_blocks_save_before_any_writes_and_recovers_without_reload() {
        use std::os::windows::fs::OpenOptionsExt;
        let root = std::env::temp_dir().join(format!(
            "wk_save_locked_ui_{}_{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        crate::project::save_project_metadata(&root, &crate::project::ProjectHierarchy::new())
            .unwrap();
        crate::project::save_editor_state(&root, &crate::project::EditorProjectState::default())
            .unwrap();
        crate::engine::ui_authoring::AuthoredUiWorkspace::default()
            .save_to_project(&root)
            .unwrap();
        crate::world::persistence::save_world(&World::new(), &root.join("world.dat")).unwrap();
        std::fs::write(root.join("camera.dat"), "1 2 3 4 5 6").unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project_and_wait_for_tests();
        assert!(!app.project_load_failed);
        let before = crate::project::atomic_file::snapshot_persisted_files(&root).unwrap();
        let original_camera = std::fs::read(root.join("camera.dat")).unwrap();
        app.editor.camera.yaw = 3.0;
        let lock = std::fs::OpenOptions::new()
            .read(true)
            .share_mode(0x1)
            .open(root.join("ui.json"))
            .unwrap();
        let error = app.save_project_checked().unwrap_err();
        assert!(error.contains("Cannot replace"), "{error}");
        assert!(error.contains("ui.json"), "{error}");
        assert!(!app.project_load_failed, "write lock is not a corrupt load");
        drop(lock);
        assert_eq!(
            crate::project::atomic_file::snapshot_persisted_files(&root).unwrap(),
            before,
            "save preflight must leave every persisted authored project file intact"
        );
        app.save_project_checked().unwrap();
        assert!(!app.project_load_failed);
        assert_ne!(
            std::fs::read(root.join("camera.dat")).unwrap(),
            original_camera
        );
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn external_edit_does_not_block_authoritative_project_save_or_exit() {
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
        app.load_project_and_wait_for_tests();
        assert!(!app.project_load_failed);
        app.editor.camera.yaw = 0.5;
        let external_camera = b"7 8 9 10 11 12";
        fs::write(root.join("camera.dat"), external_camera).unwrap();

        app.save_project_checked().unwrap();
        let saved = fs::read_to_string(root.join("camera.dat")).unwrap();
        assert!(
            saved.starts_with("0.5 "),
            "editor camera is authoritative: {saved}"
        );
        assert!(!app.project_load_failed);
        app.exit_to_home();
        assert!(app.project_manager.current_project.is_none());

        crate::character::package::reload_packages(None);
        crate::engine::entity_package::reload_packages(None);
        fs::remove_dir_all(root).unwrap();
    }

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
        app.load_project_and_wait_for_tests();
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
        assert!(app.world.actors.contains_key(&id));
        assert_eq!(app.editor.shell.explorer_width, 295.0);
        assert_eq!(app.editor.shell.properties_width, 360.0);
        assert_eq!(
            fs::read(root.join("project.json")).unwrap(),
            project_data_before
        );
        assert_eq!(fs::read(root.join("world.dat")).unwrap(), world_data_before);
        assert_eq!(fs::read(&script).unwrap(), script_before);
        let _ = state_file;
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn failed_world_load_preserves_all_files_across_save_autosave_and_exit() {
        let (root, valid_metadata) = failed_load_fixture("save_guard");
        let original_files = project_file_bytes(&root);
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project_and_wait_for_tests();
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
        assert!(app.project_load_failed);
        assert_eq!(app.project_manager.current_project.as_ref(), Some(&root));

        app.project_manager.current_project = Some(root.clone());
        app.load_project_and_wait_for_tests();
        assert!(app.project_load_failed);
        fs::write(root.join("terrain/terrain.json"), valid_metadata).unwrap();
        app.load_project_and_wait_for_tests();
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

    #[test]
    fn failed_world_load_guard_resets_when_loading_a_new_empty_project() {
        let (root, _) = failed_load_fixture("new_project_guard");
        let original_files = project_file_bytes(&root);
        let new_project = root.join("new_project");
        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(root.clone());
        app.load_project_and_wait_for_tests();
        assert!(app.project_load_failed);
        assert!(app.save_project_checked().is_err());
        assert_eq!(project_file_bytes(&root), original_files);

        fs::create_dir_all(new_project.join("scripts")).unwrap();
        app.project_manager.current_project = Some(new_project.clone());
        app.load_project_and_wait_for_tests();
        assert!(!app.project_load_failed);
        app.save_project_checked().unwrap();
        assert!(new_project.join("world.dat").is_file());
        assert_eq!(
            fs::read(root.join("world.dat")).unwrap(),
            original_files[&PathBuf::from("world.dat")]
        );
        fs::remove_dir_all(root).unwrap();
    }

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
        app.load_project_and_wait_for_tests();
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
        app.load_project_and_wait_for_tests();

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
        app.load_project_and_wait_for_tests();
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
        app.load_project_and_wait_for_tests();
        app.editor.script_editor.open_file(script_a.clone());

        assert_eq!(
            app.editor.script_editor.active_document,
            Some(script_a.clone())
        );

        app.exit_to_home();

        assert!(app.editor.script_editor.open_documents.is_empty());
        assert!(app.editor.script_editor.active_document.is_none());

        app.project_manager.current_project = Some(test_dir_b.clone());
        app.load_project_and_wait_for_tests();

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
    fn nested_scripts_reconcile_to_filesystem_folders() {
        let dir =
            std::env::temp_dir().join(format!("worldkiln_script_folders_{}", std::process::id()));
        if dir.exists() {
            std::fs::remove_dir_all(&dir).unwrap();
        }
        std::fs::create_dir_all(dir.join("scripts/data")).unwrap();
        std::fs::create_dir_all(dir.join("scripts/systems")).unwrap();
        std::fs::write(
            dir.join("scripts/data/registry.aeo"),
            "fn version() { return 1 }",
        )
        .unwrap();
        std::fs::write(
            dir.join("scripts/systems/dialogue.aeo"),
            "fn initialize() { return true }",
        )
        .unwrap();

        let mut existing = crate::project::ProjectHierarchy::new();
        let old_id = existing
            .create_script_node(None, "scripts/data/registry.aeo")
            .unwrap();
        crate::project::save_project_metadata(&dir, &existing).unwrap();

        let mut app = App::new_for_tests(1280.0, 720.0);
        app.project_manager.current_project = Some(dir.clone());
        app.load_project_and_wait_for_tests();
        let tree = &app.editor.project_hierarchy;
        let scripts = tree
            .root_nodes()
            .into_iter()
            .find(|&id| {
                matches!(tree.node(id).map(|n| &n.kind),
                    Some(crate::project::ProjectNodeKind::Folder { name }) if name == "Scripts"
                )
            })
            .unwrap();
        for (folder, file) in [("data", "registry.aeo"), ("systems", "dialogue.aeo")] {
            let child = tree
                .children_of(Some(scripts))
                .into_iter()
                .find(|&id| {
                    matches!(tree.node(id).map(|n| &n.kind),
                        Some(crate::project::ProjectNodeKind::Folder { name }) if name == folder
                    )
                })
                .unwrap();
            let logical = format!("scripts/{folder}/{file}");
            let ids = tree.find_script_nodes(&logical);
            assert_eq!(ids.len(), 1);
            assert_eq!(tree.parent_of(ids[0]), Some(child));
        }
        assert_eq!(
            tree.find_script_nodes("scripts/data/registry.aeo")[0],
            old_id,
            "reconciliation must preserve script node identities"
        );
        std::fs::remove_dir_all(&dir).ok();
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

        let mut hierarchy = crate::project::ProjectHierarchy::new();
        hierarchy
            .create_script_node(None, "scripts/script.aeo")
            .unwrap();
        crate::project::save_project_metadata(&test_dir, &hierarchy).unwrap();

        app.load_project_and_wait_for_tests();

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

pub(super) fn reconcile_loaded_project(
    editor: &mut crate::editor::Editor,
    world: &mut World,
    project_path: &std::path::Path,
) {
    let indexed_script_paths = editor.script_editor.project_script_index().map(|index| {
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
        let stale_script_nodes = editor
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
            if editor.project_hierarchy.contains(node_id) {
                editor.project_hierarchy.remove_node(node_id);
                removed_any = true;
            }
        }

        if removed_any {
            editor.needs_save = true;
        }
    }

    let existing_actor_ids: std::collections::HashSet<u64> = world.actors.keys().copied().collect();

    let stale_actor_nodes: Vec<u64> = editor
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
        editor.project_hierarchy.remove_node(node_id);
    }

    for actor_id in existing_actor_ids {
        if editor.project_hierarchy.find_actor_node(actor_id).is_none() {
            if let Err(error) = editor.project_hierarchy.create_actor_node(None, actor_id) {
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

    let actor_bindings = world.actor_script_bindings.clone();

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

        let matching_actor_node = editor.project_hierarchy.find_actor_node(binding.actor_id);

        let Some(actor_node_id) = matching_actor_node else {
            continue;
        };

        let already_represented =
            editor
                .project_hierarchy
                .nodes
                .values()
                .any(|node| match &node.kind {
                    crate::project::ProjectNodeKind::Script { script_path } => {
                        script_path == &binding.normalized_script_path()
                            && editor.project_hierarchy.attached_actor_id(node.id)
                                == Some(binding.actor_id)
                    }

                    _ => false,
                });

        if !already_represented {
            if let Err(error) = editor
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

    let indexed_script_absolute_paths = editor.script_editor.indexed_script_paths();

    for absolute_path in indexed_script_absolute_paths {
        let Ok(relative_path) = absolute_path.strip_prefix(&project_path) else {
            continue;
        };

        let logical_path = relative_path.to_string_lossy().replace('\\', "/");

        if editor
            .project_hierarchy
            .find_script_nodes(&logical_path)
            .is_empty()
        {
            if let Err(error) = editor
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

    editor.reconcile_filesystem_scripts(project_path);

    let player_nodes = editor
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
        if editor.project_hierarchy.move_node(id, None).is_ok() {
            editor.needs_save = true;
        }
    }
    editor.project_cell_sync_revision = None;
    editor.synchronize_project_cell_nodes(&world);
    editor.synchronize_project_hierarchy_bindings(world);
}

#[cfg(test)]
#[path = "live_project_sync_tests.rs"]
mod live_project_sync_tests;
