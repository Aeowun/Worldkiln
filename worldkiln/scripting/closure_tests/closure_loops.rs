use crate::engine::entity::EntityManager;
use crate::scripting::api::HostContext;
use crate::scripting::interpreter::Interpreter;
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::value::Value;

fn test_host() -> EntityManager {
    EntityManager::new()
}

fn create_interpreter(source: &str) -> Interpreter {
    let tokens = Lexer::new(source).tokenize().expect("tokens");
    let program = Parser::new(tokens).parse().expect("program");
    Interpreter::new(program)
}

#[test]
fn c31_each_iteration_captures_its_own_binding() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0

    fn main() {
        callbacks: basket = []
        for item in [10, 20, 30] {
            basket.insert(callbacks, fn() {
                return item
            })
        }

        const cb1 = callbacks[0]
        const cb2 = callbacks[1]
        const cb3 = callbacks[2]

        r1 = cb1()
        r2 = cb2()
        r3 = cb3()
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = test_host();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .expect("main");

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(10.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(20.0)));
    assert_eq!(instance.get_field("r3"), Some(&Value::Number(30.0)));
}

#[test]
fn c32_loop_variable_changes_after_callback_creation() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        callbacks: basket = []
        for item in [1, 2] {
            basket.insert(callbacks, fn() {
                return item
            })
        }

        // Loop has finished here. Calling closures afterwards:
        const cb1 = callbacks[0]
        const cb2 = callbacks[1]

        r1 = cb1()
        r2 = cb2()
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = test_host();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .expect("main");

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(1.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(2.0)));
}

#[test]
fn c33_nested_loop_capture() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0
    r4: number = 0

    fn main() {
        callbacks: basket = []

        for outer in [100, 200] {
            for inner in [1, 2] {
                basket.insert(callbacks, fn() {
                    return outer + inner
                })
            }
        }

        const cb1 = callbacks[0]
        const cb2 = callbacks[1]
        const cb3 = callbacks[2]
        const cb4 = callbacks[3]

        r1 = cb1()
        r2 = cb2()
        r3 = cb3()
        r4 = cb4()
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = test_host();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .expect("main");

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(101.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(102.0)));
    assert_eq!(instance.get_field("r3"), Some(&Value::Number(201.0)));
    assert_eq!(instance.get_field("r4"), Some(&Value::Number(202.0)));
}
