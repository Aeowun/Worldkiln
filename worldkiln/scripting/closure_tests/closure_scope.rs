use crate::engine::entity::EntityManager;
use crate::scripting::api::HostContext;
use crate::scripting::interpreter::Interpreter;
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::value::{Scope, Value};

fn test_host() -> EntityManager {
    EntityManager::new()
}

fn create_interpreter(source: &str) -> Interpreter {
    let tokens = Lexer::new(source).tokenize().expect("tokens");
    let program = Parser::new(tokens).parse().expect("program");
    Interpreter::new(program)
}

#[test]
fn c05_closure_writes_captured_variable() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        count: number = 0

        const increment = fn() {
            count = count + 1
        }

        increment()
        increment()
        increment()

        r = count
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(3.0)));
}

#[test]
fn c06_closure_mutation_visible_to_outer_scope() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 10

        const change = fn() {
            value = 20
        }

        change()

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
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .expect("main");

    assert_eq!(instance.get_field("r"), Some(&Value::Number(20.0)));
}

#[test]
fn c07_two_closures_share_one_captured_variable() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        count: number = 0

        const increment = fn() {
            count = count + 1
        }

        const read = fn() {
            return count
        }

        increment()
        increment()

        r = read()
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(2.0)));
}

#[test]
fn c08_independent_closures_do_not_share_unrelated_environments() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0
    r4: number = 0

    fn main() {
        const make = fn(start) {
            value: number = start

            return fn() {
                value = value + 1
                return value
            }
        }

        const a = make(0)
        const b = make(100)

        r1 = a()
        r2 = a()
        r3 = b()
        r4 = b()
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
    assert_eq!(instance.get_field("r3"), Some(&Value::Number(101.0)));
    assert_eq!(instance.get_field("r4"), Some(&Value::Number(102.0)));
}

#[test]
fn c15_inner_variable_shadows_captured_variable() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        value: number = 10

        const f = fn() {
            inner_val: number = 20
            return inner_val
        }

        r1 = f()
        r2 = value
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

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(20.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(10.0)));
}

#[test]
fn c16_captured_variable_survives_local_shadowing() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        value: number = 10

        const make = fn() {
            return fn() {
                shadow_val: number = 20
                return shadow_val
            }
        }

        const f = make()

        r1 = f()
        r2 = value
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

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(20.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(10.0)));
}

#[test]
fn c17_assignment_modifies_nearest_correct_binding() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        outer: number = 10

        const f = fn() {
            outer = 20
        }

        f()

        r = outer
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(20.0)));
}

#[test]
fn c18_assignment_does_not_accidentally_create_different_binding() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 10

        const f = fn() {
            value = 20
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
    let mut instance = interpreter
        .instantiate_entity("Test", 1, &mut host)
        .expect("instance");

    interpreter
        .call(&mut instance, "main", vec![], &mut host)
        .expect("main");

    assert_eq!(instance.get_field("r"), Some(&Value::Number(20.0)));
}

#[test]
fn c45_cloned_scope_shares_variable_backing() {
    let scope1 = Scope::new();
    scope1.declare("x", Value::Number(1.0), false).unwrap();

    let scope2 = scope1.clone();
    scope2.set("x", Value::Number(2.0)).unwrap();

    assert_eq!(scope1.get("x"), Some(Value::Number(2.0)));
    assert_eq!(scope2.get("x"), Some(Value::Number(2.0)));
}

#[test]
fn c46_separate_scopes_remain_independent() {
    let scope1 = Scope::new();
    let scope2 = Scope::new();

    scope1.declare("x", Value::Number(1.0), false).unwrap();
    scope2.declare("x", Value::Number(2.0), false).unwrap();

    assert_eq!(scope1.get("x"), Some(Value::Number(1.0)));
    assert_eq!(scope2.get("x"), Some(Value::Number(2.0)));
}

#[test]
fn c47_constant_semantics_remain_intact() {
    let scope = Scope::new();
    scope.declare("C", Value::Number(100.0), true).unwrap();

    let result = scope.set("C", Value::Number(200.0));
    assert!(result.is_err());
    assert_eq!(scope.get("C"), Some(Value::Number(100.0)));
}
