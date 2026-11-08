// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::editor::script_editor::{ScriptDocument, ScriptEditor};

fn fixture(tag: &str) -> PathBuf {
    let tick = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root =
        std::env::temp_dir().join(format!("wk_asset003_{tag}_{}_{}", std::process::id(), tick));
    std::fs::create_dir_all(root.join("scripts")).unwrap();
    std::fs::create_dir_all(root.join("playerscripts")).unwrap();
    root
}

fn draft(editor: &mut ScriptEditor, root: &Path, category: &str, name: &str) -> PathBuf {
    let path = script_path(root, category, name);
    let mut doc = ScriptDocument::new(path.clone(), starter_script(category, name).unwrap());
    doc.dirty = true;
    doc.native_draft = true;
    editor.open_documents.insert(path.clone(), doc);
    editor.active_document = Some(path.clone());
    path
}

#[test]
fn native_save_all_reopen_explicit_assignment_and_world_persistence() {
    let root = fixture("save");
    let mut editor = ScriptEditor::new();
    let mut world = crate::world::World::new();
    let before = (
        world.selected_camera.clone(),
        world.selected_controller.clone(),
    );
    let camera = draft(&mut editor, &root, "cameras", "AuthorCamera");
    let controller = draft(&mut editor, &root, "controllers", "AuthorController");
    editor.save_all_checked().unwrap();
    assert_eq!(
        (
            world.selected_camera.clone(),
            world.selected_controller.clone()
        ),
        before
    );
    assert!(crate::project::discover_cameras(&root).contains(&"AuthorCamera".into()));
    assert!(crate::project::discover_controllers(&root).contains(&"AuthorController".into()));
    for path in [&camera, &controller] {
        assert!(!editor.open_documents[path].dirty);
        assert!(editor.close_document(path));
        editor.open_file(path.clone());
        assert!(editor.open_documents[path].program.is_some());
    }
    editor.open_documents.get_mut(&camera).unwrap().source =
        starter_script("cameras", "AuthorCamera")
            .unwrap()
            .replace("distance = 6.0", "distance = 11.0");
    editor.active_document = Some(camera.clone());
    editor.save_active();
    assert!(
        std::fs::read_to_string(&camera)
            .unwrap()
            .contains("distance = 11.0")
    );
    world.selected_camera = "AuthorCamera".into();
    world.selected_controller = "AuthorController".into();
    let path = root.join("world.dat");
    crate::world::persistence::save_world(&world, &path).unwrap();
    let mut loaded = crate::world::World::new();
    crate::world::persistence::load_world(&mut loaded, &path).unwrap();
    assert_eq!(loaded.selected_camera, "AuthorCamera");
    assert_eq!(loaded.selected_controller, "AuthorController");
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_cancel_discard_and_save_errors_preserve_drafts_and_existing_data() {
    let root = fixture("cancel");
    let mut editor = ScriptEditor::new();
    let world = crate::world::World::new();
    let path = draft(&mut editor, &root, "cameras", "CancelCamera");
    editor.closing_path = Some(path.clone());
    editor.apply_dirty_response(&Some(root.clone()), path.clone(), false, true, &world);
    assert!(editor.open_documents[&path].dirty);
    assert!(!path.parent().unwrap().exists());
    editor.apply_dirty_response(&Some(root.clone()), path.clone(), false, false, &world);
    assert!(!editor.open_documents.contains_key(&path));
    assert!(!path.parent().unwrap().exists());
    draft(&mut editor, &root, "cameras", "CancelCamera");
    editor.open_documents.get_mut(&path).unwrap().source = "entity CancelCamera {".into();
    assert!(editor.save_all_checked().is_err());
    assert!(editor.open_documents[&path].dirty);
    assert!(!path.exists());
    assert!(
        editor
            .output_log
            .iter()
            .any(|line| line.contains("Native asset syntax"))
    );
    editor.open_documents.get_mut(&path).unwrap().source =
        starter_script("cameras", "CancelCamera").unwrap();
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    std::fs::write(&path, "concurrently authored source").unwrap();
    assert!(editor.save_all_checked().is_err());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        "concurrently authored source"
    );
    assert!(editor.open_documents[&path].dirty);
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_template_packages_instantiate_and_execute_in_script_scene() {
    use crate::scripting::{api::HostContext, scene::ScriptScene, value::Value};
    let root = fixture("runtime");
    for (category, name) in [
        ("cameras", "RuntimeCamera"),
        ("controllers", "RuntimeController"),
    ] {
        save_native_script(
            &script_path(&root, category, name),
            &starter_script(category, name).unwrap(),
            true,
        )
        .unwrap();
    }
    let mut host = crate::scripting::scene::tests::test_host();
    let world = host.world.clone();
    let mut scene = ScriptScene::load_from_bindings(
        &root,
        &world,
        &[],
        &mut host.entity_manager,
        &std::collections::HashMap::new(),
        0.0,
    )
    .unwrap();
    let mut context = HostContext {
        delta_time: 0.0,
        engine: &mut host,
    };
    scene.start(&mut context).unwrap();
    for (category, name, args) in [
        ("cameras", "RuntimeCamera", vec![Value::Nil]),
        (
            "controllers",
            "RuntimeController",
            vec![Value::Nil, Value::Nil, Value::Nil],
        ),
    ] {
        let object = scene
            .instantiate_package_object(category, name, args, &mut context)
            .unwrap();
        scene
            .call_object_method(&object, "update", vec![Value::Number(0.016)], &mut context)
            .unwrap();
    }
    assert!(
        scene
            .instantiate_package_object("cameras", "MissingCamera", vec![Value::Nil], &mut context)
            .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}

#[test]
fn native_edit_resolves_builtin_filename_and_prefers_project_package() {
    let root = fixture("edit");
    let builtin = editable_script(&root, "cameras", "thirdPerson").unwrap();
    assert_eq!(builtin.file_name().unwrap(), "thirdPersonCamera.aeo");
    let local = root.join(".assets/cameras/thirdPerson/custom_entry.aeo");
    std::fs::create_dir_all(local.parent().unwrap()).unwrap();
    std::fs::write(&local, "entity LocalCamera {}").unwrap();
    assert_eq!(
        editable_script(&root, "cameras", "thirdPerson").unwrap(),
        local
    );
    assert!(validate_source("cameras", "Broken", "entity Broken {}").is_err());
    assert!(
        validate_source(
            "controllers",
            "Broken",
            "entity Other { fn constructor(a,b,c) {} }"
        )
        .is_err()
    );
    std::fs::remove_dir_all(root).unwrap();
}
