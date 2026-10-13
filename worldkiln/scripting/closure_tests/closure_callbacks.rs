use crate::scripting::api::HostContext;
use crate::scripting::scene::tests::{add_authored_entity, create_scene, test_host};
use crate::world::{CellType, WorldCoord};
use std::collections::HashSet;

#[test]
fn c25_on_touch_sees_captured_variable() {
    let source = r#"
const doors = find("Door")

for door in doors {
    door.on_touch = fn(c) {
        for target in doors {
            target.visible = false
            target.solid = false
        }
    }
}
"#;

    let mut th = test_host();
    let first = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Door",
    );
    let second = add_authored_entity(
        &mut th.world,
        WorldCoord::new(1, 0, 0),
        CellType::Block,
        "Door",
    );

    let first_coord = WorldCoord::new(0, 0, 0);
    let second_coord = WorldCoord::new(1, 0, 0);

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.update(0.0, &mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(first);
        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene
    };

    assert!(!th.world.is_cell_visible(first_coord));
    assert!(!th.world.is_cell_visible(second_coord));
    assert!(!th.world.is_cell_solid(first_coord));
    assert!(!th.world.is_cell_solid(second_coord));
    let _ = second;
    let _ = scene;
}

#[test]
fn c26_on_touch_sees_callback_parameter_and_capture() {
    let source = r#"
const doors = find("Door")

for door in doors {
    door.on_touch = fn(c) {
        debug.log("Parameter:", c.id)
        debug.log("Captured doors len:", doors.len())
    }
}
"#;

    let mut th = test_host();
    let cell_id = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Door",
    );

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.update(0.0, &mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);
        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene
    };

    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == format!("Parameter: {}", cell_id))
    );
    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == "Captured doors len: 1")
    );
}

#[test]
fn c27_on_touch_preserves_self() {
    let source = r#"
const doors = find("Door")
outer_secret: number = 123

for door in doors {
    door.on_touch = fn(c) {
        debug.log("Self ID:", self.id)
        debug.log("Outer secret:", outer_secret)
        debug.log("Param ID:", c.id)
    }
}
"#;

    let mut th = test_host();
    let cell_id = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Door",
    );

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.update(0.0, &mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);
        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene
    };

    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == format!("Self ID: {}", cell_id))
    );
    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == "Outer secret: 123")
    );
    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == format!("Param ID: {}", cell_id))
    );
}

#[test]
fn c28_on_overlap_preserves_captures() {
    let source = r#"
const triggers = find("Trigger")
message: string = "Overlap Active"

for t in triggers {
    t.on_overlap = fn(overlapping, c) {
        if overlapping {
            debug.log(message, c.id)
        }
    }
}
"#;

    let mut th = test_host();
    let cell_id = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Trigger",
    );

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.update(0.0, &mut host).unwrap();

        scene.on_player_overlap(cell_id, true, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene
    };

    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == format!("Overlap Active {}", cell_id))
    );
}

#[test]
fn c29_stored_callback_survives_time_before_invocation() {
    let source = r#"
fn setup() {
    const doors = find("Door")
    captured_value: number = 999
    for door in doors {
        door.on_touch = fn(c) {
            debug.log("Value:", captured_value)
        }
    }
}

setup()
"#;

    let mut th = test_host();
    let cell_id = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Door",
    );

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.update(0.0, &mut host).unwrap();

        let mut contacted = HashSet::new();
        contacted.insert(cell_id);
        scene.on_player_contact(&contacted, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();
        scene
    };

    assert!(scene.output().iter().any(|r| r.message == "Value: 999"));
}

#[test]
fn c30_multiple_objects_have_independent_callback_captures() {
    let source = r#"
const doors = find("Door")

for door in doors {
    door_id: number = door.id
    door.on_touch = fn(c) {
        debug.log("Door captured ID:", door_id)
    }
}
"#;

    let mut th = test_host();
    let door1 = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Door",
    );
    let door2 = add_authored_entity(
        &mut th.world,
        WorldCoord::new(1, 0, 0),
        CellType::Block,
        "Door",
    );

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.update(0.0, &mut host).unwrap();

        let mut contacted1 = HashSet::new();
        contacted1.insert(door1);
        scene.on_player_contact(&contacted1, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        let mut contacted2 = HashSet::new();
        contacted2.insert(door2);
        scene.on_player_contact(&contacted2, &mut host).unwrap();
        scene.update(0.0, &mut host).unwrap();

        scene
    };

    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == format!("Door captured ID: {}", door1))
    );
    assert!(
        scene
            .output()
            .iter()
            .any(|r| r.message == format!("Door captured ID: {}", door2))
    );
}
