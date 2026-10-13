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
fn c19_captured_variable_survives_wait() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 42

        const f = fn() {
            wait(1)
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
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    // First step triggers wait(1)
    let res1 = interpreter.resume_fiber(&mut fiber, &mut host);
    assert!(matches!(
        res1,
        crate::scripting::execution::FiberResult::Yield(_)
    ));

    // Second step resumes after wait
    let res2 = interpreter.resume_fiber(&mut fiber, &mut host);
    assert!(matches!(
        res2,
        crate::scripting::execution::FiberResult::Complete
    ));

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(42.0)));
}

#[test]
fn c20_closure_local_variable_survives_wait() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const f = fn() {
            value: number = 42
            wait(1)
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
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    interpreter.resume_fiber(&mut fiber, &mut host);
    interpreter.resume_fiber(&mut fiber, &mut host);

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(42.0)));
}

#[test]
fn c21_captured_mutation_survives_wait() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 0

        const f = fn() {
            value = 10
            wait(1)
            value = value + 5
        }

        f()
        r = value
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = test_host();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    interpreter.resume_fiber(&mut fiber, &mut host);
    interpreter.resume_fiber(&mut fiber, &mut host);

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(15.0)));
}

#[test]
fn c21_control_no_wait() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 0

        const f = fn() {
            value = 10
            value = value + 5
        }

        f()
        r = value
    }
}
"#;

    let mut interpreter = create_interpreter(source);
    let mut em = test_host();
    let mut host = HostContext {
        delta_time: 1.0,
        engine: &mut em,
    };
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    interpreter.resume_fiber(&mut fiber, &mut host);

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(15.0)));
}

#[test]
fn c22_multiple_locals_survive_suspension() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const f = fn() {
            a: number = 10
            b: number = 20

            wait(1)

            return a + b
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
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    interpreter.resume_fiber(&mut fiber, &mut host);
    interpreter.resume_fiber(&mut fiber, &mut host);

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(30.0)));
}

#[test]
fn c23_nested_closure_survives_suspension() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const make = fn() {
            value: number = 10

            return fn() {
                wait(1)
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
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    interpreter.resume_fiber(&mut fiber, &mut host);
    interpreter.resume_fiber(&mut fiber, &mut host);

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(10.0)));
}

#[test]
fn c24_call_stack_survives_suspension_inside_closure() {
    let source = r#"
entity Test {
    r: number = 0

    fn helper() {
        wait(1)
        return 100
    }

    fn main() {
        base: number = 5

        const f = fn() {
            const h = helper()
            return base + h
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
    let instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    let mut fiber = interpreter
        .start_fiber(instance, "main", vec![])
        .expect("start fiber");

    interpreter.resume_fiber(&mut fiber, &mut host);
    interpreter.resume_fiber(&mut fiber, &mut host);

    assert_eq!(fiber.instance().get_field("r"), Some(&Value::Number(105.0)));
}
