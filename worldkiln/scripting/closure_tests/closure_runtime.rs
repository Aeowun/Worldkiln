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
fn c43_closure_mutates_runtime_state_only() {
    let source = r#"
const doors = find("Door")

for door in doors {
    door.on_touch = fn(c) {
        c.visible = false
    }
}
"#;

    let mut th = test_host();
    let coord = WorldCoord::new(0, 0, 0);
    let cell_id = add_authored_entity(&mut th.world, coord, CellType::Block, "Door");

    assert!(th.world.is_cell_visible(coord));

    {
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

        assert!(
            !host
                .engine
                .get_property(
                    crate::scripting::value::HandleKind::Cell,
                    cell_id,
                    "visible"
                )
                .unwrap()
                .unwrap()
                .as_bool()
                .unwrap()
        );

        scene.stop(&mut host);
    }

    th.world.clear_runtime_state();
    assert!(th.world.is_cell_visible(coord));
}

#[test]
fn c44_closure_plus_wait_does_not_persist_runtime_mutation() {
    let source = r#"
const doors = find("Door")

for door in doors {
    door.on_touch = fn(c) {
        c.visible = false
        wait(1)
        c.visible = true
    }
}
"#;

    let mut th = test_host();
    let coord = WorldCoord::new(0, 0, 0);
    let cell_id = add_authored_entity(&mut th.world, coord, CellType::Block, "Door");

    {
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

        scene.stop(&mut host);
    }

    th.world.clear_runtime_state();
    assert!(th.world.is_cell_visible(coord));
}

#[test]
fn c56_invoke_same_closure_repeatedly() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        count: number = 0
        const inc = fn() {
            count = count + 1
            return count
        }

        for i in [1, 2, 3, 4, 5, 6, 7, 8, 9, 10] {
            inc()
        }

        r = count
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(10.0)));
}

#[test]
fn c57_create_many_independent_closures() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn make(x) {
        return fn() { return x }
    }

    fn main() {
        closures: basket = []
        for i in [1, 2, 3, 4, 5] {
            basket.insert(closures, make(i * 10))
        }

        const c1 = closures[0]
        const c5 = closures[4]

        r1 = c1()
        r2 = c5()
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

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(10.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(50.0)));
}

#[test]
fn c58_repeated_callback_registration() {
    let source = r#"
fn register1() {
    const doors = find("Door")
    for door in doors {
        door.on_touch = fn(c) {
            debug.log("Version 1")
        }
    }
}

fn register2() {
    const doors = find("Door")
    for door in doors {
        door.on_touch = fn(c) {
            debug.log("Version 2")
        }
    }
}

register1()
register2()
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

    assert!(scene.output().iter().any(|r| r.message == "Version 2"));
}

#[test]
fn c59_metamorphic_direct_value_vs_captured_value() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        val: number = 123
        const closure = fn() { return val }

        r1 = val
        r2 = closure()
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

    assert_eq!(instance.get_field("r1"), instance.get_field("r2"));
}

#[test]
fn c60_metamorphic_closure_copied_vs_original() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        count: number = 0
        const f = fn() {
            count = count + 1
            return count
        }
        const g = f

        r1 = f()
        r2 = g()
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

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(1.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(2.0)));
}

#[test]
fn c61_closure_before_after_unrelated_execution() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn unrelated() {
        dummy: number = 999
        return dummy * 2
    }

    fn main() {
        val: number = 50
        const closure = fn() { return val }

        r1 = closure()
        unrelated()
        r2 = closure()
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
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(50.0)));
}
