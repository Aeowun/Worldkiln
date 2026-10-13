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
fn c12_calling_closure_from_closure() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        value: number = 10

        const first = fn() {
            return value
        }

        const second = fn() {
            return first()
        }

        r = second()
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
fn c13_named_function_does_not_dynamically_capture_caller_locals() {
    let source = r#"
entity Test {
    fn read_value() {
        return value
    }

    fn main() {
        value: number = 99
        return read_value()
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

    let result = interpreter.call(&mut instance, "main", vec![], &mut host);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown variable 'value'"));
}

#[test]
fn c14_closure_explicitly_captures_caller_environment() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const caller = fn() {
            value: number = 99

            return fn() {
                return value
            }
        }

        const f = caller()

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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(99.0)));
}

#[test]
fn c34_closures_intentionally_share_mutable_state() {
    let source = r#"
entity Test {
    r: number = 0

    fn make_pair() {
        value: number = 0

        const increment = fn() {
            value = value + 1
        }

        const read = fn() {
            return value
        }

        return [increment, read]
    }

    fn main() {
        const pair = make_pair()
        const inc = pair[0]
        const rd = pair[1]

        inc()
        inc()

        r = rd()
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
fn c35_separate_factories_produce_separate_environments() {
    let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn make_counter() {
        count: number = 0
        return fn() {
            count = count + 1
            return count
        }
    }

    fn main() {
        const c1 = make_counter()
        const c2 = make_counter()

        c1()
        c1()
        c2()

        r1 = c1()
        r2 = c2()
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

    assert_eq!(instance.get_field("r1"), Some(&Value::Number(3.0)));
    assert_eq!(instance.get_field("r2"), Some(&Value::Number(2.0)));
}

#[test]
fn c36_closure_copied_as_value_preserves_environment() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        count: number = 0

        const f = fn() {
            count = count + 1
            return count
        }

        const g = f

        f()
        r = g()
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
fn c37_function_stored_in_variable_remains_callable() {
    let source = r#"
entity Test {
    r: number = 0

    fn main() {
        const f = fn() {
            return 42
        }

        const g = f

        r = g()
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
fn c38_function_passed_as_argument_retains_capture() {
    let source = r#"
entity Test {
    r: number = 0

    fn apply(callback) {
        return callback(10)
    }

    fn main() {
        base: number = 5

        const closure = fn(x) {
            return base + x
        }

        r = apply(closure)
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

#[test]
fn c39_missing_capture_remains_an_error() {
    let source = r#"
entity Test {
    fn main() {
        const f = fn() {
            return nonexistent_variable
        }

        f()
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

    let result = interpreter.call(&mut instance, "main", vec![], &mut host);
    assert!(result.is_err());
    assert!(
        result
            .unwrap_err()
            .contains("unknown variable 'nonexistent_variable'")
    );
}

#[test]
fn c40_capture_does_not_expose_unrelated_scopes() {
    let source = r#"
entity Test {
    fn main() {
        const make = fn() {
            secret: number = 123
            return fn() {
                return 1
            }
        }

        const f = make()
        return secret
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

    let result = interpreter.call(&mut instance, "main", vec![], &mut host);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown variable 'secret'"));
}

#[test]
fn c41_one_callback_cannot_see_another_callbacks_locals() {
    let source = r#"
entity Test {
    fn main() {
        const cb1 = fn() {
            local1: number = 10
            return local1
        }

        const cb2 = fn() {
            return local1
        }

        cb1()
        cb2()
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

    let result = interpreter.call(&mut instance, "main", vec![], &mut host);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("unknown variable 'local1'"));
}

#[test]
fn c42_recursive_function_behavior_remains_unchanged() {
    let source = r#"
entity Test {
    r: number = 0

    fn factorial(n) {
        if n <= 1 {
            return 1
        }
        return n * factorial(n - 1)
    }

    fn main() {
        r = factorial(5)
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

    assert_eq!(instance.get_field("r"), Some(&Value::Number(120.0)));
}

#[test]
fn c48_function_value_identity_safety() {
    let source = r#"
entity Test {
    r: bool = false

    fn main() {
        const f1 = fn() { return 1 }
        const f2 = f1

        r = (f1 == f2)
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

    assert_eq!(instance.get_field("r"), Some(&Value::Bool(true)));
}
