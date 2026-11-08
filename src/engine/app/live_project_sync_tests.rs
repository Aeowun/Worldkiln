// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::editor::script_editor::ScriptDocument;
use std::fs;
use std::path::PathBuf;

static NEXT_FIXTURE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "wk_external_sync_{}_{}_{}",
            std::process::id(),
            NEXT_FIXTURE.fetch_add(1, std::sync::atomic::Ordering::Relaxed),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        fs::create_dir_all(root.join("scripts")).unwrap();
        Self(root)
    }
    fn app(&self) -> App {
        let mut world = World::new();
        world.add_actor("Original", glam::Vec3::ZERO);
        crate::world::persistence::save_world(&world, &self.0.join("world.dat")).unwrap();
        crate::project::save_project_metadata(&self.0, &crate::project::ProjectHierarchy::new())
            .unwrap();
        crate::project::save_editor_state(&self.0, &crate::project::EditorProjectState::default())
            .unwrap();
        crate::engine::ui_authoring::AuthoredUiWorkspace::default()
            .save_to_project(&self.0)
            .unwrap();
        let mut app = App::new_for_tests(800.0, 600.0);
        app.project_manager.current_project = Some(self.0.clone());
        app.load_project_and_wait_for_tests();
        app
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn external_sync_dirty_script_save_is_authoritative() {
    let f = Fixture::new();
    let path = f.0.join("scripts/main.aeo");
    fs::write(&path, "const value = 1\n").unwrap();
    let mut doc = ScriptDocument::new(path.clone(), fs::read_to_string(&path).unwrap());
    doc.source = "const value = 2\n".into();
    doc.dirty = true;
    fs::write(&path, "const value = 3\n").unwrap();
    doc.save().expect("outside edits must never block Save");
    assert_eq!(fs::read_to_string(&path).unwrap(), "const value = 2\n");
    assert!(!doc.dirty);
}

#[test]
fn external_sync_scene_addition_is_accepted_without_closing_or_reloading_editor() {
    let f = Fixture::new();
    let mut app = f.app();
    let selected = *app.world.actors.keys().next().unwrap();
    app.editor.select_actor(selected);
    app.editor.camera.yaw = 0.876;
    let mut remote = World::new();
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    let added = remote.add_actor("Outside", glam::Vec3::new(3.0, 2.0, 1.0));
    crate::world::persistence::save_world(&remote, &f.0.join("world.dat")).unwrap();
    assert!(
        app.save_project_checked().is_ok(),
        "valid outside scene changes should refresh before Save"
    );
    assert!(app.world.actors.contains_key(&added));
    assert_eq!(app.editor.selected_actor_id, Some(selected));
    assert_eq!(app.editor.camera.yaw, 0.876);
    assert_eq!(app.view, View::Editor);
}

fn poll_until(app: &mut App, accepted: impl Fn(&App) -> bool) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(6);
    while !accepted(app) && std::time::Instant::now() < deadline {
        app.poll_live_project_sync();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(
        accepted(app),
        "refresh did not arrive; output: {:?}",
        app.editor.script_editor.output_log
    );
}

#[test]
fn external_sync_background_refreshes_scripts_and_scene_without_reopening() {
    let f = Fixture::new();
    let mut app = f.app();
    let path = f.0.join("scripts/main.aeo");
    fs::write(&path, "const value = 1\n").unwrap();
    app.editor.script_editor.open_documents.insert(
        path.clone(),
        ScriptDocument::new(path.clone(), "const value = 1\n".into()),
    );
    app.editor.camera.yaw = 0.876;
    let original = *app.world.actors.keys().next().unwrap();
    app.editor.select_actor(original);
    app.world.actors.get_mut(&original).unwrap().position.x = 42.0;
    let mut remote = World::new();
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    let added = remote.add_actor("Outside", glam::Vec3::ONE);
    crate::world::persistence::save_world(&remote, &f.0.join("world.dat")).unwrap();
    fs::write(&path, "const value = 2\n").unwrap();
    poll_until(&mut app, |a| {
        a.world.actors.contains_key(&added)
            && a.editor.script_editor.open_documents[&path].source == "const value = 2\n"
    });
    assert_eq!(app.world.actors[&original].position.x, 42.0);
    assert_eq!(app.editor.selected_actor_id, Some(original));
    assert_eq!(app.editor.camera.yaw, 0.876);
    assert_eq!(app.view, View::Editor);
}

#[test]
fn external_sync_disjoint_script_edits_merge_and_conflicts_keep_editor_text() {
    let f = Fixture::new();
    let path = f.0.join("scripts/main.aeo");
    let base = "const first = 1\nconst second = 2\n";
    fs::write(&path, base).unwrap();
    let mut doc = ScriptDocument::new(path.clone(), base.into());
    doc.source = "const first = 3\nconst second = 2\n".into();
    doc.dirty = true;
    assert!(
        doc.accept_external_source("const first = 1\nconst second = 4\n")
            .unwrap()
    );
    assert_eq!(doc.source, "const first = 3\nconst second = 4\n");
    assert!(doc.dirty);
    assert!(
        doc.accept_external_source("const first = 5\nconst second = 4\n")
            .is_err()
    );
    assert_eq!(doc.source, "const first = 3\nconst second = 4\n");
    fs::write(&path, "const first = 5\nconst second = 4\n").unwrap();
    doc.save().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), doc.source);
}

#[test]
fn external_sync_invalid_script_preserves_buffer_but_cannot_block_save() {
    let f = Fixture::new();
    let path = f.0.join("scripts/main.aeo");
    fs::write(&path, "const value = 1\n").unwrap();
    let mut doc = ScriptDocument::new(path.clone(), "const value = 1\n".into());
    fs::write(&path, "const = \n").unwrap();
    assert!(doc.accept_external_source("const = \n").is_err());
    assert_eq!(doc.source, "const value = 1\n");
    doc.save().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "const value = 1\n");
}

#[test]
fn external_sync_deleted_script_can_be_saved_by_authoritative_editor() {
    let f = Fixture::new();
    let path = f.0.join("scripts/main.aeo");
    fs::write(&path, "const value = 1\n").unwrap();
    let mut doc = ScriptDocument::new(path.clone(), "const value = 1\n".into());
    fs::remove_file(&path).unwrap();
    doc.save().unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), doc.source);
}

#[test]
fn external_sync_scene_conflict_never_blocks_authoritative_save() {
    let f = Fixture::new();
    let mut app = f.app();
    let id = *app.world.actors.keys().next().unwrap();
    let mut remote = World::new();
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    remote.actors.get_mut(&id).unwrap().position.x = 9.0;
    crate::world::persistence::save_world(&remote, &f.0.join("world.dat")).unwrap();
    app.world.actors.get_mut(&id).unwrap().position.x = 7.0;
    app.save_project_checked()
        .expect("outside conflict must never veto Save");
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    assert_eq!(app.world.actors[&id].position.x, 7.0);
    assert_eq!(remote.actors[&id].position.x, 7.0);
}

#[test]
fn external_sync_incomplete_scene_never_blocks_authoritative_save() {
    let f = Fixture::new();
    let mut app = f.app();
    let id = *app.world.actors.keys().next().unwrap();
    fs::write(f.0.join("world.dat"), "WORLD_FORMAT 2\n").unwrap();
    app.save_project_checked().unwrap();
    let mut saved = World::new();
    crate::world::persistence::load_world(&mut saved, &f.0.join("world.dat")).unwrap();
    assert!(saved.actors.contains_key(&id));
}

#[test]
fn external_sync_scene_refresh_is_editor_only() {
    let f = Fixture::new();
    let mut app = f.app();
    let mut remote = World::new();
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    let added = remote.add_actor("Outside", glam::Vec3::ONE);
    crate::world::persistence::save_world(&remote, &f.0.join("world.dat")).unwrap();
    app.editor.mode = EditorMode::Play;
    app.poll_live_project_sync();
    assert!(!app.world.actors.contains_key(&added));
    app.editor.mode = EditorMode::Editor;
    poll_until(&mut app, |a| a.world.actors.contains_key(&added));
}

#[test]
fn external_sync_cells_and_sources_refresh_without_terrain_reload() {
    let f = Fixture::new();
    let mut app = f.app();
    let mut remote = World::new();
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    let coord = crate::world::WorldCoord::new(3, 4, 5);
    remote.set_cell(coord, crate::world::CellType::Block);
    let cell_id = remote.get(coord).unwrap().id;
    let source_id = remote.add_source(crate::world::CellType::Light, glam::Vec3::ONE);
    crate::world::persistence::save_world(&remote, &f.0.join("world.dat")).unwrap();
    let manifest = fs::read_to_string(f.0.join("world.dat")).unwrap();
        let generation = manifest
            .lines()
            .find_map(|line| line.strip_prefix("STORAGE_GENERATION "))
            .unwrap();
        let terrain = f.0.join(".world_generations").join(generation).join("terrain");
        fs::create_dir_all(&terrain).unwrap();
    fs::write(terrain.join("terrain.json"), "not terrain json").unwrap();
    let mut probe = World::new();
    assert!(crate::world::persistence::load_world(&mut probe, &f.0.join("world.dat")).is_err());
    poll_until(&mut app, |a| {
        a.world.get(coord).is_some() && a.world.source(source_id).is_some()
    });
    assert_eq!(app.world.get(coord).unwrap().id, cell_id);
    assert!(app.world.terrain.authored.is_none());
}

#[test]
fn external_sync_conflicting_actor_does_not_prevent_unrelated_addition() {
    let f = Fixture::new();
    let mut app = f.app();
    let id = *app.world.actors.keys().next().unwrap();
    let mut remote = World::new();
    crate::world::persistence::load_world(&mut remote, &f.0.join("world.dat")).unwrap();
    remote.actors.get_mut(&id).unwrap().position.x = 9.0;
    let added = remote.add_actor("Outside", glam::Vec3::ONE);
    crate::world::persistence::save_world(&remote, &f.0.join("world.dat")).unwrap();
    app.world.actors.get_mut(&id).unwrap().position.x = 7.0;
    poll_until(&mut app, |a| a.world.actors.contains_key(&added));
    assert_eq!(app.world.actors[&id].position.x, 7.0);
    app.save_project_checked().unwrap();
}

#[test]
fn external_sync_scripts_refresh_even_when_scene_write_is_invalid() {
    let f = Fixture::new();
    let mut app = f.app();
    let path = f.0.join("scripts/main.aeo");
    fs::write(&path, "const value = 1\n").unwrap();
    app.editor.script_editor.open_documents.insert(
        path.clone(),
        ScriptDocument::new(path.clone(), "const value = 1\n".into()),
    );
    fs::write(f.0.join("world.dat"), "WORLD_FORMAT 2\n").unwrap();
    fs::write(&path, "const value = 2\n").unwrap();
    poll_until(&mut app, |a| {
        a.editor.script_editor.open_documents[&path].source == "const value = 2\n"
    });
    assert_eq!(app.world.actors.len(), 1);
}
