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
fn c01_read_captured_local() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 42

        const f = fn() {
            return value
        }

        r = f()
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(42.0)));
}

#[test]
fn c02_capture_after_enclosing_scope_exited() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const make = fn() {
            value: number = 42

            return fn() {
                return value
            }
        }

        const f = make()
        r = f()
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(42.0)));
}

#[test]
fn c03_capture_multiple_variables() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        a: number = 10
        b: number = 20
        c: number = 30

        const f = fn() {
            return a + b + c
        }

        r = f()
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(60.0)));
}

#[test]
fn c04_capture_variable_plus_parameter() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        base: number = 10

        const f = fn(x) {
            return base + x
        }

        r = f(5)
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(15.0)));
}
