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
fn c09_closure_captures_closure_local_variable() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const make = fn() {
            value: number = 10

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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(10.0)));
}

#[test]
fn c10_nested_closure_mutation() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        const make = fn() {
            value: number = 0

            const increment = fn() {
                value = value + 1
                return value
            }

            return increment
        }

        const f = make()

        r1 = f()
        r2 = f()
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
fn c11_two_level_closure_chain() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const outer = fn() {
            a: number = 10

            return fn() {
                b: number = 20

                return fn() {
                    return a + b
                }
            }
        }

        const middle = outer()
        const inner = middle()

        r = inner()
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(30.0)));
}
