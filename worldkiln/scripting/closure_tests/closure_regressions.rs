use crate::engine::entity::EntityManager;
use crate::scripting::api::HostContext;
use crate::scripting::interpreter::Interpreter;
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::scene::tests::{add_authored_entity, create_scene, test_host};
use crate::scripting::value::Value;
use crate::world::{CellType, WorldCoord};
use std::collections::HashSet;

fn create_interpreter(source: &str) -> Interpreter {
    let tokens = Lexer::new(source).tokenize().unwrap();
    let program = Parser::new(tokens).parse().unwrap();
    Interpreter::new(program)
}

#[test]
fn c49_ordinary_top_level_script_execution() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        x: number = 10
        y: number = 20
        r = x + y
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = EntityManager::new();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .unwrap();

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .unwrap();

    assert_eq!(instance.get_field("r"), Some(&Value::Number(30.0)));
}

#[test]
fn c50_ordinary_named_function_invocation() {
    let source = r#"
entity Test {
    r: number = 0

    fn add(a, b) {
        return a + b
    }

    fn main() {
        r = add(15, 25)
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = EntityManager::new();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .unwrap();

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .unwrap();

    assert_eq!(instance.get_field("r"), Some(&Value::Number(40.0)));
}

#[test]
fn c51_builtin_stdlib_invocation() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: string = ""

    fn main() {
        r1 = math.abs(-50)
        r2 = string.upper("aeoscript")
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = EntityManager::new();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .unwrap();

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .unwrap();

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(50.0)));
    assert_eq!(
        instance.get_field("r2"),
        Some(&Value::String("AEOSCRIPT".to_string()))
    );
}

#[test]
fn c52_entity_bound_script_execution() {
    let source = r#"
entity Guard {
    hp: number = 100

    fn update(dt) {
        hp = hp - 10
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = EntityManager::new();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Guard", 1, &mut host)
        .unwrap();

    interpreter
        .call(&mut instance, "update", vec![Value::Number(0.1)], &mut host)
        .unwrap();

    assert_eq!(instance.get_field("hp"), Some(&Value::Number(90.0)));
}

#[test]
fn c53_existing_event_callback_behavior() {
    let source = r#"
entity Test {
    fn on_touch(c) {
        debug.log("Event touch fired:", c.id)
    }
}
"#;

    let mut th = test_host();
    let _ent_id = th.entity_manager.create_entity("Test");
    let cell_id = add_authored_entity(
        &mut th.world,
        WorldCoord::new(0, 0, 0),
        CellType::Block,
        "Test",
    );

    let mut scene = {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        let mut scene = create_scene(source, &mut host);
        scene.entities[0].associated_cell_id = Some(cell_id);
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
            .any(|r| r.message == format!("Event touch fired: {}", cell_id))
    );
}

#[test]
fn c54_existing_wait_behavior() {
    let source = r#"
entity Test {
    status: string = "start"

    fn main() {
        status = "waiting"
        wait(1)
        status = "done"
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = EntityManager::new();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .unwrap();

    let mut fiber = interpreter.start_fiber(instance, "main", vec![]).unwrap();

    interpreter.resume_fiber(&mut fiber, &mut host);
    assert_eq!(
        fiber.instance().get_field("status"),
        Some(&Value::String("waiting".to_string()))
    );

    interpreter.resume_fiber(&mut fiber, &mut host);
    assert_eq!(
        fiber.instance().get_field("status"),
        Some(&Value::String("done".to_string()))
    );
}

#[test]
fn c55_existing_property_get_set_behavior() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        m: map = { hp: 100 }
        m["hp"] = 150
        r = m["hp"]
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = EntityManager::new();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .unwrap();

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .unwrap();

    assert_eq!(instance.get_field("r"), Some(&Value::Number(150.0)));
}

#[test]
fn c56_the_exact_motivating_doors_regression() {
    let source = r#"
const doors = find("Door")

for door in doors {
    door.on_touch = fn(c) {
        debug.log("Door touched:", c.id)

        for target in doors {
            target.visible = false
            target.solid = false
        }

        wait(1)

        for target in doors {
            target.visible = true
            target.solid = true
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

        // 1. closure is created, 2. doors is captured, 3. callback is stored
        scene.update(0.0, &mut host).unwrap();

        // 4. callback is invoked later, 5. c is bound, 6. doors is still accessible
        let mut contacted = HashSet::new();
        contacted.insert(first);
        scene.on_player_contact(&contacted, &mut host).unwrap();

        // 7. first loop executes, 8. runtime mutations occur, 9. wait() yields
        scene.update(0.0, &mut host).unwrap();
        scene
    };

    assert!(!th.world.is_cell_visible(first_coord));
    assert!(!th.world.is_cell_visible(second_coord));
    assert!(!th.world.is_cell_solid(first_coord));
    assert!(!th.world.is_cell_solid(second_coord));
    assert!(
        scene
            .output()
            .iter()
            .any(|record| record.message == format!("Door touched: {}", first))
    );

    // 10. fiber resumes, 11. doors is still accessible, 12. second loop executes, 13. runtime mutations are restored
    {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.update(1.0, &mut host).unwrap();
    }

    assert!(th.world.is_cell_visible(first_coord));
    assert!(th.world.is_cell_visible(second_coord));
    assert!(th.world.is_cell_solid(first_coord));
    assert!(th.world.is_cell_solid(second_coord));

    // 14. no authored-state mutation occurs
    {
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut th,
        };
        scene.stop(&mut host);
    }
    th.world.clear_runtime_state();
    assert!(th.world.is_cell_visible(first_coord));
    assert!(th.world.is_cell_visible(second_coord));
    let _ = second;
}
