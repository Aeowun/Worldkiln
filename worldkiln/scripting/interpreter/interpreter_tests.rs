#[cfg(test)]
mod tests {
    use super::super::*;

    use crate::engine::entity::{EntityManager, EntityManager as _};
    use crate::scripting::api::EngineHost;
    use crate::scripting::execution::{ScriptScheduler, ScriptTaskState, YieldReason};
    use crate::scripting::lexer::Lexer;
    use crate::scripting::parser::Parser;
    use crate::scripting::value::{HandleKind, Value};

    fn interpreter(source: &str) -> Interpreter {
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let program = Parser::new(tokens).parse().expect("parser should succeed");

        Interpreter::new(program)
    }

    fn test_host() -> EntityManager {
        EntityManager::new()
    }

    fn run(source: &str) -> (Interpreter, ScriptInstance, Value) {
        let mut interpreter = interpreter(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let result = interpreter
            .call(&mut instance, "update", vec![Value::Number(1.0)], &mut host)
            .expect("update should execute");

        (interpreter, instance, result)
    }

    fn drive_fiber_once(
        interpreter: &mut Interpreter,
        scheduler: &mut ScriptScheduler,
        task_id: crate::scripting::execution::ScriptTaskId,
        fiber: &mut ScriptFiber,
    ) -> FiberResult {
        let ready = scheduler.pop_ready().expect("task should be ready");

        assert_eq!(ready, task_id);

        scheduler.begin_running(task_id).expect("task should start");

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let result = interpreter.resume_fiber(fiber, &mut host);

        scheduler
            .apply_result(task_id, result.clone())
            .expect("scheduler should accept fiber result");

        result
    }

    #[test]
    fn executes_field_arithmetic() {
        let source = r#"
entity Test {

    value: number = 10

    fn update(dt: number) {
        value += dt
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(11.0)));
    }

    #[test]
    fn executes_local_variables() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        amount: number = 10
        amount += dt
        value = amount
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(11.0)));
    }

    #[test]
    fn executes_if_else() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        if dt > 0 {
            value = 1
        } else {
            value = 2
        }
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(1.0)));
    }

    #[test]
    fn executes_while_loop() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        counter: number = 0

        while counter < 5 {
            counter += 1
        }

        value = counter
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(5.0)));
    }

    #[test]
    fn executes_for_loop_over_array() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        for item in [1, 2, 3, 4] {
            value += item
        }
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(10.0)));
    }

    #[test]
    fn executes_return_values() {
        let source = r#"
entity Test {

    value: number = 0

    fn add(a: number, b: number): number {
        return a + b
    }

    fn update(dt: number) {
        value = add(4, 7)
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(11.0)));
    }

    #[test]
    fn debug_log_captures_output() {
        let source = r#"
entity Test {

    value: number = 42

    fn update(dt: number) {
        debug.log(value)
        debug.log("hello", value)
    }
}
"#;

        let (interpreter, _instance, _result) = run(source);

        assert_eq!(interpreter.output()[0].message, "42".to_string());

        assert_eq!(interpreter.output()[1].message, "hello 42".to_string());
    }

    #[test]
    fn nil_can_be_used_as_an_optional_condition() {
        let source = r#"
entity Test {

    value: number = 0
    light: Light?

    fn update(dt: number) {
        if light {
            value = 1
        } else {
            value = 2
        }
    }
}
"#;

        let (_interpreter, instance, _result) = run(source);

        assert_eq!(instance.get_field("value"), Some(&Value::Number(2.0)));
    }

    #[test]
    fn handles_are_truthy() {
        let value = Value::Handle {
            kind: HandleKind::Light,
            id: 25,
        };

        assert_eq!(value.is_truthy().expect("handle should be truthy"), true);
    }

    #[test]
    fn operation_budget_stops_infinite_loops() {
        let source = r#"
entity Test {

    fn update(dt: number) {
        while true {
        }
    }
}
"#;

        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let program = Parser::new(tokens).parse().expect("parser should succeed");

        let mut interpreter = Interpreter::with_limits(program, 50, 16);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let result = interpreter.call(&mut instance, "update", vec![Value::Number(1.0)], &mut host);

        assert!(result.is_err());

        assert!(
            result
                .expect_err("budget should fail")
                .contains("execution budget exhausted")
        );
    }

    #[test]
    fn call_depth_is_limited() {
        let source = r#"
entity Test {

    fn recurse(value: number): number {
        return recurse(value + 1)
    }

    fn update(dt: number) {
        recurse(0)
    }
}
"#;

        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let program = Parser::new(tokens).parse().expect("parser should succeed");

        let mut interpreter = Interpreter::with_limits(program, 100_000, 4);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let result = interpreter.call(&mut instance, "update", vec![Value::Number(1.0)], &mut host);

        assert!(result.is_err());

        assert!(
            result
                .expect_err("call depth should fail")
                .contains("call depth exceeded")
        );
    }

    #[test]
    fn fiber_resumes_after_first_wait() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        value = 1
        wait(1)
        value = 2
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should start");

        let mut scheduler = ScriptScheduler::new();

        let task_id = scheduler.spawn();

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(1.0))
        );

        assert_eq!(fiber.program_counter(), 3);

        scheduler.tick(0.99).expect("time should advance");

        assert_eq!(
            scheduler.state(task_id),
            Some(ScriptTaskState::Waiting { wake_at: 1.0 })
        );

        scheduler.tick(1.0).expect("time should advance");

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn fiber_resumes_two_waits_in_order() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        value = 1
        wait(1)
        value = 2
        wait(1)
        value = 3
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should start");

        let mut scheduler = ScriptScheduler::new();

        let task_id = scheduler.spawn();

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(1.0))
        );

        scheduler.tick(1.0).expect("time should advance");

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(2.0))
        );

        scheduler.tick(1.99).expect("time should advance");

        assert_eq!(
            scheduler.state(task_id),
            Some(ScriptTaskState::Waiting { wake_at: 2.0 })
        );

        scheduler.tick(2.0).expect("time should advance");

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(3.0))
        );
    }

    #[test]
    fn fiber_wait_inside_if_resumes_inside_if() {
        let source = r#"
entity Test {

    value: number = 0

    fn update(dt: number) {
        if true {
            value = 1
            wait(1)
            value = 2
        }
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should start");

        let mut scheduler = ScriptScheduler::new();

        let task_id = scheduler.spawn();

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(1.0))
        );

        scheduler.tick(1.0).expect("time should advance");

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn fiber_wait_inside_while_preserves_loop_position() {
        let source = r#"
entity Test {

    counter: number = 0

    fn update(dt: number) {
        while true {
            counter += 1
            wait(1)
        }
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should start");

        let mut scheduler = ScriptScheduler::new();

        let task_id = scheduler.spawn();

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("counter"),
            Some(&Value::Number(1.0))
        );

        scheduler.tick(1.0).expect("time should advance");

        let result = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("counter"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn fiber_budget_resets_between_cooperative_resumes() {
        let source = r#"
entity Test {

    counter: number = 0

    fn update(dt: number) {
        while true {
            counter += 1
            wait(1)
        }
    }
}
"#;

        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let program = Parser::new(tokens).parse().expect("parser should succeed");

        let mut interpreter = Interpreter::with_limits(program, 100, 16);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should start");

        let mut scheduler = ScriptScheduler::new();

        let task_id = scheduler.spawn();

        let first = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(first, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        scheduler.tick(1.0).expect("time should advance");

        let second = drive_fiber_once(&mut interpreter, &mut scheduler, task_id, &mut fiber);

        assert_eq!(second, FiberResult::Yield(YieldReason::WaitSeconds(1.0)));

        assert_eq!(
            fiber.instance().get_field("counter"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn fiber_rejects_negative_wait() {
        let source = r#"
entity Test {

    fn update(dt: number) {
        wait(-1)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should start");

        let result = interpreter.resume_fiber(&mut fiber, &mut host);

        assert!(matches!(
            result,
            FiberResult::Failed(message)
                if message.contains("greater than zero")
        ));

        assert!(fiber.is_finished());
    }

    #[test]
    fn fiber_rejects_non_finite_wait() {
        let source = r#"
entity Test {

    fn update(dt: number) {
        wait(dt)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let mut fiber = interpreter
            .start_fiber(instance, "update", vec![Value::Number(f64::INFINITY)])
            .expect("fiber should start");

        let result = interpreter.resume_fiber(&mut fiber, &mut host);

        assert!(matches!(
            result,
            FiberResult::Failed(message)
                if message.contains("must be finite")
        ));

        assert!(fiber.is_finished());
    }

    #[test]
    fn basket_len_property_works() {
        let source = r#"
entity Test {
    count: number = 0

    fn main() {
        b: basket = [1, 2, 3]
        count = b.len()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("count"), Some(&Value::Number(3.0)));
    }

    #[test]
    fn method_call_syntax_works() {
        let source = r#"
entity Test {
    fn main(e: Entity) {
        e:set_position(10, 20, 30)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let id = em.create_entity("Other");

        let handle = Value::Handle {
            kind: HandleKind::Entity,
            id: id.0,
        };

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        interpreter
            .call(&mut instance, "main", vec![handle], &mut host)
            .unwrap();

        assert_eq!(em.get_position(id), Some(glam::Vec3::new(10.0, 20.0, 30.0)));
    }

    #[test]
    fn get_parent_as_global_works() {
        let source = r#"
entity Test {
    parent_name: string = ""

    fn main() {
        p: Entity? = get_parent()

        if p != nil {
            parent_name = p.name
        }
    }
}
"#;

        let mut interpreter = interpreter(source);

        struct ParentHost {
            em: EntityManager,
        }

        impl EngineHost for ParentHost {
            fn entity_manager(&self) -> &EntityManager {
                &self.em
            }

            fn get_position(&self, id: u64) -> Option<glam::Vec3> {
                self.em.get_position(crate::engine::entity::EntityId(id))
            }

            fn set_position(&mut self, id: u64, pos: glam::Vec3) {
                self.em
                    .set_position(crate::engine::entity::EntityId(id), pos);
            }

            fn lookup_light(&self, _: i32, _: i32, _: i32) -> Option<u64> {
                None
            }

            fn is_light_enabled(&self, _: u64) -> Option<bool> {
                None
            }

            fn set_light_enabled(&mut self, _: u64, _: bool) {}

            fn is_collision_events_enabled(&self, _: u64) -> bool {
                true
            }

            fn get_all_cells_of_class(&self, _: &str) -> Vec<u64> {
                vec![]
            }

            fn find_objects(&self, _: &str) -> Vec<(HandleKind, u64)> {
                vec![]
            }

            fn get_children(&self, _: HandleKind, _: u64) -> Vec<(HandleKind, u64)> {
                vec![]
            }

            fn get_parent(&self, kind: HandleKind, id: u64) -> Option<(HandleKind, u64)> {
                if kind == HandleKind::Entity && id == 2 {
                    Some((HandleKind::Entity, 1))
                } else {
                    None
                }
            }

            fn get_cell_object(&self, _: u64) -> Option<(HandleKind, u64)> {
                None
            }

            fn get_property(
                &self,
                kind: HandleKind,
                id: u64,
                name: &str,
            ) -> Result<Option<Value>, String> {
                if kind == HandleKind::Entity && id == 1 && name == "name" {
                    Ok(Some(Value::String("Parent".to_string())))
                } else {
                    Ok(None)
                }
            }

            fn set_property(
                &mut self,
                _: HandleKind,
                _: u64,
                _: &str,
                _: Value,
            ) -> Result<bool, String> {
                Ok(false)
            }

            fn cell_exists(&self, _: u64) -> bool {
                false
            }

            fn entity_exists(&self, id: u64) -> bool {
                self.em.validate_handle(id)
            }

            fn get_script_property(
                &self,
                _: HandleKind,
                _: u64,
                _: &str,
            ) -> Option<crate::scripting::value::DynamicProperty> {
                None
            }

            fn set_script_property(
                &mut self,
                _: HandleKind,
                _: u64,
                _: String,
                _: Value,
                _: Option<String>,
            ) {
            }

            fn call_method(
                &mut self,
                _: HandleKind,
                _: u64,
                _: &str,
                _: &[Value],
            ) -> Result<Option<Value>, String> {
                Ok(None)
            }

            fn set_attribute(&mut self, _: u64, _: String, _: Value) -> Result<(), String> {
                Ok(())
            }

            fn remove_attribute(&mut self, _: u64, _: &str) -> Result<(), String> {
                Ok(())
            }

            fn create_runtime_cell(&mut self, _: &str) -> Result<(HandleKind, u64), String> {
                Err("Unsupported".to_string())
            }

            fn move_runtime_cell(&mut self, _: u64, _: i32, _: i32, _: i32) -> Result<(), String> {
                Err("Unsupported".to_string())
            }

            fn delete_cell(&mut self, _: u64) -> Result<(), String> {
                Ok(())
            }
        }

        let mut ph = ParentHost { em: test_host() };

        ph.em.create_entity("Parent");
        ph.em.create_entity("Test");

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut ph,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 2, &mut host)
            .unwrap();

        interpreter
            .call(&mut instance, "main", vec![], &mut host)
            .unwrap();

        assert_eq!(
            instance.get_field("parent_name"),
            Some(&Value::String("Parent".to_string()))
        );
    }

    #[test]
    fn test_string_concatenation() {
        let source = r#"
entity Test {
    r1: string = ""
    r2: string = ""
    r3: string = ""
    r4: string = ""
    r5: string = ""

    fn main() {
        r1 = "count = " + 5
        r2 = "enabled = " + true
        r3 = "value = " + nil
        r4 = 10 + " items"
        r5 = "basket = " + [1, 2]
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(
            instance.get_field("r1"),
            Some(&Value::String("count = 5".to_string()))
        );

        assert_eq!(
            instance.get_field("r2"),
            Some(&Value::String("enabled = true".to_string()))
        );

        assert_eq!(
            instance.get_field("r3"),
            Some(&Value::String("value = nil".to_string()))
        );

        assert_eq!(
            instance.get_field("r4"),
            Some(&Value::String("10 items".to_string()))
        );

        assert_eq!(
            instance.get_field("r5"),
            Some(&Value::String("basket = [1, 2]".to_string()))
        );
    }

    #[test]
    fn stdlib_math_works() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0
    r4: number = 0
    r5: number = 0

    fn main() {
        r1 = math.abs(-10.5)
        r2 = math.max(5, 10)
        r3 = math.clamp(15, 0, 10)
        r4 = math.floor(3.7)
        r5 = math.lerp(10, 20, 0.5)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r1"), Some(&Value::Number(10.5)));

        assert_eq!(instance.get_field("r2"), Some(&Value::Number(10.0)));

        assert_eq!(instance.get_field("r3"), Some(&Value::Number(10.0)));

        assert_eq!(instance.get_field("r4"), Some(&Value::Number(3.0)));

        assert_eq!(instance.get_field("r5"), Some(&Value::Number(15.0)));
    }

    #[test]
    fn stdlib_basket_works() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: string = ""
    r3: number = 0
    r4: number = 0

    fn main() {
        b: basket = [3, 1, 2]
        b.sort()
        r1 = b[0]
        r2 = b.concat("-")

        b2: basket = basket.create(3, 5)
        r3 = b2[2]

        b.insert(0, 10)
        r4 = b[0]
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(
            instance.get_field("r2"),
            Some(&Value::String("1-2-3".to_string()))
        );

        assert_eq!(instance.get_field("r3"), Some(&Value::Number(5.0)));

        assert_eq!(instance.get_field("r4"), Some(&Value::Number(10.0)));
    }

    #[test]
    fn stdlib_basket_move_works() {
        let source = r#"
entity Test {
    r1: string = ""
    r2: string = ""

    fn main() {
        b: basket = [0, 1, 2, 3]

        // Copy [1, 2] to start at 3
        basket.move(b, 1, 2, 3)

        r1 = b.concat(",")

        b2: basket = [10, 20]

        // Copy [1, 2] from b to b2 at 1
        basket.move(b, 1, 2, 1, b2)

        r2 = b2.concat(",")
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(
            instance.get_field("r1"),
            Some(&Value::String("0,1,2,1,2".to_string()))
        );

        assert_eq!(
            instance.get_field("r2"),
            Some(&Value::String("10,1,2".to_string()))
        );
    }

    #[test]
    fn stdlib_string_works() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: string = ""
    r3: string = ""
    r4: string = ""

    fn main() {
        r1 = string.len("hello")
        r2 = string.upper("world")
        r3 = string.reverse("abc")

        s: basket = string.split("a,b,c", ",")

        r4 = s.concat("|")
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r1"), Some(&Value::Number(5.0)));

        assert_eq!(
            instance.get_field("r2"),
            Some(&Value::String("WORLD".to_string()))
        );

        assert_eq!(
            instance.get_field("r3"),
            Some(&Value::String("cba".to_string()))
        );

        assert_eq!(
            instance.get_field("r4"),
            Some(&Value::String("a|b|c".to_string()))
        );
    }

    #[test]
    fn frozen_basket_mutation_fails() {
        let source = r#"
entity Test {
    fn main() {
        b: basket = [1, 2, 3]
        b.freeze()
        b[0] = 10
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let res = interpreter.call(&mut instance, "main", vec![], &mut host);

        assert!(res.is_err());

        assert!(res.unwrap_err().to_lowercase().contains("frozen"));
    }

    #[test]
    fn map_read_missing_key_returns_nil() {
        let source = r#"
entity Test {
    r1: bool = false
    r2: bool = false
    r3: bool = false
    r4: bool = false
    r5: bool = false

    fn main() {
        const values = {}

        r1 = (values["missing"] == nil)
        r2 = (values[123] == nil)
        r3 = (values["1"] == nil)

        values[1] = "numeric"

        r4 = (values["1"] == nil)
        r5 = (values[1] == "numeric")
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r1"), Some(&Value::Bool(true)));

        assert_eq!(instance.get_field("r2"), Some(&Value::Bool(true)));

        assert_eq!(instance.get_field("r3"), Some(&Value::Bool(true)));

        assert_eq!(instance.get_field("r4"), Some(&Value::Bool(true)));

        assert_eq!(instance.get_field("r5"), Some(&Value::Bool(true)));
    }

    #[test]
    fn map_deletion_semantics() {
        let source = r#"
entity Test {
    r1: bool = false

    fn main() {
        const values = {}

        values["name"] = "test"
        values["name"] = nil

        r1 = (values["name"] == nil)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r1"), Some(&Value::Bool(true)));
    }

    #[test]
    fn nested_map_read_missing_key() {
        let source = r#"
    entity Test {
        r1: bool = false

        fn main() {
            const outer = {}
            outer["inner"] = {}
            r1 = (outer["inner"]["missing"] == nil)
        }
    }
    "#;

        let mut interpreter = interpreter(source);
        let mut em = test_host();
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

        assert_eq!(instance.get_field("r1"), Some(&Value::Bool(true)));
    }

    #[test]
    fn unicode_string_literals_work() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0
    r4: string = ""
    r5: string = ""

    fn main() {
        r1 = string.len("é")
        r2 = string.len("你好")
        r3 = string.len("😀")
        r4 = string.reverse("é")

        s: basket = string.split("é")

        r5 = s[0]
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r3"), Some(&Value::Number(1.0)));

        assert_eq!(
            instance.get_field("r4"),
            Some(&Value::String("é".to_string()))
        );

        assert_eq!(
            instance.get_field("r5"),
            Some(&Value::String("é".to_string()))
        );
    }

    #[test]
    fn wait_inside_called_function_works() {
        let source = r#"
entity Test {
    value: number = 0

    fn sub() {
        wait(0.1)
        value = 1
    }

    fn main() {
        sub()
        value = 2
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let mut fiber = interpreter.start_fiber(instance, "main", vec![]).unwrap();

        let result = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(result, FiberResult::Yield(YieldReason::WaitSeconds(0.1)));

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(0.0))
        );

        let result = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(result, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn wait_preserves_locals() {
        let source = r#"
entity Test {
    value: number = 0

    fn main() {
        local_val: number = 42

        wait(0.1)

        value = local_val
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let mut fiber = interpreter.start_fiber(instance, "main", vec![]).unwrap();

        interpreter.resume_fiber(&mut fiber, &mut host);

        interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(42.0))
        );
    }

    #[test]
    fn wait_inside_loop_preserves_state() {
        let source = r#"
entity Test {
    value: number = 0

    fn main() {
        for i in [1, 2, 3] {
            value += i
            wait(0.1)
        }
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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
            fiber.instance().get_field("value"),
            Some(&Value::Number(1.0))
        );

        interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(3.0))
        );

        interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(6.0))
        );

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(res, FiberResult::Complete);
    }

    #[test]
    fn nested_calls_with_wait() {
        let source = r#"
entity Test {
    value: number = 0

    fn inner() {
        wait(0.1)
        value += 1
    }

    fn middle() {
        inner()
        wait(0.1)
        value += 10
    }

    fn main() {
        middle()
        value += 100
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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
            fiber.instance().get_field("value"),
            Some(&Value::Number(0.0))
        );

        interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(1.0))
        );

        interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(111.0))
        );
    }

    #[test]
    fn update_callback_with_wait_logic() {
        let source = r#"
entity Test {
    value: number = 0
    started: bool = false

    fn sub() {
        wait(0.1)
        value = 1
    }

    fn update() {
        if !started {
            started = true
            sub()
        }
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let mut fiber = interpreter.start_fiber(instance, "update", vec![]).unwrap();

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(res, FiberResult::Yield(YieldReason::WaitSeconds(0.1)));

        assert_eq!(
            fiber.instance().get_field("started"),
            Some(&Value::Bool(true))
        );

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(0.0))
        );

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(res, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("value"),
            Some(&Value::Number(1.0))
        );
    }

    #[test]
    fn test_top_level_execution() {
        let source = r#"
debug.log("one")
debug.log("two")
"#;

        let tokens = Lexer::new(source).tokenize().unwrap();

        let program = Parser::new(tokens).parse().unwrap();

        let mut interpreter = Interpreter::new(program.clone());

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let instance = ScriptInstance::new_empty();

        let mut fiber = interpreter
            .start_top_level_fiber(instance, &program.statements)
            .unwrap();

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(res, FiberResult::Complete);

        assert_eq!(interpreter.output()[0].message, "one");

        assert_eq!(interpreter.output()[1].message, "two");
    }

    #[test]
    fn test_top_level_wait() {
        let source = r#"
debug.log("before")
wait(0.1)
debug.log("after")
"#;

        let tokens = Lexer::new(source).tokenize().unwrap();

        let program = Parser::new(tokens).parse().unwrap();

        let mut interpreter = Interpreter::new(program.clone());

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let instance = ScriptInstance::new_empty();

        let mut fiber = interpreter
            .start_top_level_fiber(instance, &program.statements)
            .unwrap();

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(res, FiberResult::Yield(YieldReason::WaitSeconds(0.1)));

        assert_eq!(interpreter.output().len(), 1);

        assert_eq!(interpreter.output()[0].message, "before");

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(res, FiberResult::Complete);

        assert_eq!(interpreter.output().len(), 2);

        assert_eq!(interpreter.output()[1].message, "after");
    }

    #[test]
    fn test_top_level_error() {
        let source = r#"
light.set_enabled(false)
"#;

        let tokens = Lexer::new(source).tokenize().unwrap();

        let program = Parser::new(tokens).parse().unwrap();

        let mut interpreter = Interpreter::new(program.clone());

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let instance = ScriptInstance::new_empty();

        let mut fiber = interpreter
            .start_top_level_fiber(instance, &program.statements)
            .unwrap();

        let res = interpreter.resume_fiber(&mut fiber, &mut host);

        match res {
            FiberResult::Failed(msg) => {
                assert!(msg.contains("unknown variable 'light'"));
            }

            _ => panic!("Expected FiberResult::Failed"),
        }
    }

    #[test]
    fn test_functions_not_executed_automatically() {
        let source = r#"
fn helper() {
    debug.log("helper")
}

debug.log("top")
"#;

        let tokens = Lexer::new(source).tokenize().unwrap();

        let program = Parser::new(tokens).parse().unwrap();

        let mut interpreter = Interpreter::new(program.clone());

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut em,
        };

        let instance = ScriptInstance::new_empty();

        let mut fiber = interpreter
            .start_top_level_fiber(instance, &program.statements)
            .unwrap();

        interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(interpreter.output().len(), 1);

        assert_eq!(interpreter.output()[0].message, "top");
    }

    #[test]
    fn closure_wait_expression_returns_value() {
        let source = r#"
entity Test {
    result: number = 0

    fn main() {
        amount: number = 42

        const f = fn() {
            wait(0.1)
            return amount
        }

        result = f()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let mut fiber = interpreter.start_fiber(instance, "main", vec![]).unwrap();

        let first = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(first, FiberResult::Yield(YieldReason::WaitSeconds(0.1)));

        assert_eq!(
            fiber.instance().get_field("result"),
            Some(&Value::Number(0.0))
        );

        let second = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(second, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("result"),
            Some(&Value::Number(42.0))
        );
    }

    #[test]
    fn statement_call_return_value_is_not_reused_by_next_expression() {
        let source = r#"
entity Test {
    result: number = 0

    fn sub() {
        return 99
    }

    fn main() {
        sub()
        result = 7
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let mut fiber = interpreter.start_fiber(instance, "main", vec![]).unwrap();

        let result = interpreter.resume_fiber(&mut fiber, &mut host);

        assert_eq!(result, FiberResult::Complete);

        assert_eq!(
            fiber.instance().get_field("result"),
            Some(&Value::Number(7.0))
        );
    }

    #[test]
    fn closures_capture_and_mutate_lexical_scope() {
        let source = r#"
entity Test {
    value: number = 0

    fn main() {
        amount: number = 10

        const add = fn() {
            value += amount
            amount += 1
        }

        add()
        add()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        interpreter
            .call(&mut instance, "main", vec![], &mut host)
            .expect("main should execute");

        assert_eq!(instance.get_field("value"), Some(&Value::Number(21.0)));
    }

    #[test]
    fn returned_closure_retains_scope_after_creator_returns() {
        let source = r#"
entity Test {
    value: number = 0

    fn make_adder() {
        amount: number = 10

        return fn() {
            value += amount
            amount += 1
        }
    }

    fn main() {
        const add = make_adder()
        add()
        add()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        interpreter
            .call(&mut instance, "main", vec![], &mut host)
            .expect("main should execute");

        assert_eq!(instance.get_field("value"), Some(&Value::Number(21.0)));
    }

    #[test]
    fn test_basic_closure_capture_instance_field() {
        let source = r#"
entity Test {
    value: number = 10
    result: number = 0

    fn main() {
        const fn_value = fn() {
            return value
        }

        result = fn_value()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("result"), Some(&Value::Number(10.0)));
    }

    #[test]
    fn test_shared_capture_mutation_multiple_closures() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        count: number = 0

        const inc = fn() {
            count += 1
            return count
        }

        const dec = fn() {
            count -= 1
            return count
        }

        inc()
        inc()

        r1 = count

        dec()

        r2 = count
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r1"), Some(&Value::Number(2.0)));

        assert_eq!(instance.get_field("r2"), Some(&Value::Number(1.0)));
    }

    #[test]
    fn test_loop_closure_captures_per_iteration() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0

    fn main() {
        funcs: basket = []

        for value in [1, 2, 3] {
            basket.insert(
                funcs,
                fn() {
                    return value
                }
            )
        }

        const f0 = funcs[0]
        const f1 = funcs[1]
        const f2 = funcs[2]

        r1 = f0()
        r2 = f1()
        r3 = f2()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r3"), Some(&Value::Number(3.0)));
    }

    #[test]
    fn test_nested_closure_calls_preserve_environment() {
        let source = r#"
entity Test {
    result: number = 0

    fn main() {
        x: number = 5

        const outer_fn = fn(a) {
            y: number = 10

            const inner_fn = fn(b) {
                return x + y + a + b
            }

            return inner_fn
        }

        const closure = outer_fn(15)

        result = closure(20)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("result"), Some(&Value::Number(50.0)));
    }

    #[test]
    fn test_normal_named_function_non_capture() {
        let source = r#"
entity Test {
    fn helper() {
        return local_value
    }

    fn main() {
        local_value: number = 10
        helper()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let result = interpreter.call(&mut instance, "main", vec![], &mut host);

        assert!(result.is_err());

        assert!(
            result
                .unwrap_err()
                .contains("unknown variable 'local_value'")
        );
    }

    #[test]
    fn test_named_function_as_value() {
        let source = r#"
fn add(a, b) {
    return a + b
}

entity Test {
    result: number = 0

    fn main() {
        const func_val = add
        result = func_val(5, 5)
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("result"), Some(&Value::Number(10.0)));
    }

    #[test]
    fn test_multiple_independent_closure_environments() {
        let source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn make_counter(start) {
        count: number = start

        return fn() {
            count += 1
            return count
        }
    }

    fn main() {
        const c1 = make_counter(10)
        const c2 = make_counter(100)

        c1()
        r1 = c1()
        r2 = c2()
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r1"), Some(&Value::Number(12.0)));

        assert_eq!(instance.get_field("r2"), Some(&Value::Number(101.0)));
    }

    #[test]
    fn test_script_object_constructor_and_persistent_fields() {
        let source = r#"
entity Counter {
    count: number = 0

    fn constructor(start) {
        count = start
    }

    fn increment() {
        count = count + 1
        return count
    }
}

entity Test {
    r1: number = 0
    r2: number = 0
    r3: number = 0

    fn main() {
        const c = Counter(10)

        r1 = c.count

        c.increment()

        r2 = c.count

        c.increment()

        r3 = c.count
    }
}
"#;

        let mut interpreter = interpreter(source);

        let mut em = test_host();

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

        assert_eq!(instance.get_field("r2"), Some(&Value::Number(11.0)));

        assert_eq!(instance.get_field("r3"), Some(&Value::Number(12.0)));
    }

    #[test]
    fn test_generic_aeoscript_module_reference_and_calls() {
        let module_source = r#"
count: number = 0

fn increment(amount) {
    count += amount
    return count
}

fn get_count() {
    return count
}
"#;

        let caller_source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        const mod1 = scripts.someScript.aeo
        r1 = mod1.increment(5)
        r2 = mod1.increment(10)
    }
}
"#;

        let module_lexer = crate::scripting::lexer::Lexer::new(module_source);
        let module_tokens = module_lexer.tokenize().unwrap();
        let module_parser = crate::scripting::parser::Parser::new(module_tokens);
        let module_program = module_parser.parse().unwrap();

        let mut interpreter = interpreter(caller_source);
        interpreter
            .loaded_scripts
            .insert("scripts/someScript.aeo".to_string(), module_program);

        let mut em = test_host();
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

        assert_eq!(instance.get_field("r1"), Some(&Value::Number(5.0)));
        assert_eq!(instance.get_field("r2"), Some(&Value::Number(15.0)));
    }

    #[test]
    fn test_multiple_modules_isolated_state() {
        let mod_a_src = r#"
val: number = 100
fn get_val() { return val }
"#;
        let mod_b_src = r#"
val: number = 200
fn get_val() { return val }
"#;
        let caller_source = r#"
entity Test {
    r1: number = 0
    r2: number = 0

    fn main() {
        const a = scripts.modA.aeo
        const b = scripts.modB.aeo
        r1 = a.get_val()
        r2 = b.get_val()
    }
}
"#;

        let lex_a = crate::scripting::lexer::Lexer::new(mod_a_src)
            .tokenize()
            .unwrap();
        let prog_a = crate::scripting::parser::Parser::new(lex_a)
            .parse()
            .unwrap();

        let lex_b = crate::scripting::lexer::Lexer::new(mod_b_src)
            .tokenize()
            .unwrap();
        let prog_b = crate::scripting::parser::Parser::new(lex_b)
            .parse()
            .unwrap();

        let mut interpreter = interpreter(caller_source);
        interpreter
            .loaded_scripts
            .insert("scripts/modA.aeo".to_string(), prog_a);
        interpreter
            .loaded_scripts
            .insert("scripts/modB.aeo".to_string(), prog_b);

        let mut em = test_host();
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

        assert_eq!(instance.get_field("r1"), Some(&Value::Number(100.0)));
        assert_eq!(instance.get_field("r2"), Some(&Value::Number(200.0)));
    }

    #[test]
    fn test_nested_module_calls() {
        let mod_c_src = r#"
fn finish() { return 42 }
"#;
        let mod_b_src = r#"
const c = scripts.modC.aeo
fn do_work() { return c.finish() }
"#;
        let caller_source = r#"
entity Test {
    res: number = 0
    fn main() {
        const b = scripts.modB.aeo
        res = b.do_work()
    }
}
"#;

        let lex_c = crate::scripting::lexer::Lexer::new(mod_c_src)
            .tokenize()
            .unwrap();
        let prog_c = crate::scripting::parser::Parser::new(lex_c)
            .parse()
            .unwrap();

        let lex_b = crate::scripting::lexer::Lexer::new(mod_b_src)
            .tokenize()
            .unwrap();
        let prog_b = crate::scripting::parser::Parser::new(lex_b)
            .parse()
            .unwrap();

        let mut interpreter = interpreter(caller_source);
        interpreter
            .loaded_scripts
            .insert("scripts/modC.aeo".to_string(), prog_c);
        interpreter
            .loaded_scripts
            .insert("scripts/modB.aeo".to_string(), prog_b);

        let mut em = test_host();
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

        assert_eq!(instance.get_field("res"), Some(&Value::Number(42.0)));
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:21 -05:00
    #[test]
    fn debugger_instruction_points_preserve_nested_module_source_ownership() {
        let mod_c_src = r#"
fn finish() { return 42 }
"#;
        let mod_b_src = r#"
const c = scripts.modC.aeo
fn do_work() { return c.finish() }
"#;
        let caller_source = r#"
entity Test {
    res: number = 0
    fn main() {
        const b = scripts.modB.aeo
        res = b.do_work()
    }
}
"#;

        let prog_c = parse_program(mod_c_src);
        let prog_b = parse_program(mod_b_src);
        let mut interpreter = interpreter(caller_source);
        interpreter
            .loaded_scripts
            .insert("scripts/modC.aeo".to_string(), prog_c);
        interpreter
            .loaded_scripts
            .insert("scripts/modB.aeo".to_string(), prog_b);

        let mut em = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };
        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();
        instance.script_path = Some("scripts/caller.aeo".to_string());
        let mut fiber = interpreter.start_fiber(instance, "main", vec![]).unwrap();
        let mut points = Vec::new();

        let result = interpreter.resume_fiber_with_debug(&mut fiber, &mut host, |point| {
            points.push((point.script_path.clone(), point.function_name.clone()));
            false
        });

        assert_eq!(result, FiberResult::Complete);
        assert!(
            points
                .iter()
                .any(|(path, function)| path == "scripts/modB.aeo" && function == "do_work"),
            "debug points were {points:?}"
        );
        assert!(
            points
                .iter()
                .any(|(path, function)| path == "scripts/modC.aeo" && function == "finish"),
            "debug points were {points:?}"
        );
    }

    #[test]
    fn test_event_handler_local_function_module_chain() {
        let module_source = r#"
counter: number = 0
fn bump() { counter += 1 }
fn get_counter() { return counter }
"#;

        let caller_source = r#"
entity Test {
    result: number = 0

    fn local_function() {
        const m = scripts.helper.aeo
        m.bump()
    }

    fn run() {
        local_function()
        const m = scripts.helper.aeo
        result = m.get_counter()
    }
}
"#;

        let lex = crate::scripting::lexer::Lexer::new(module_source)
            .tokenize()
            .unwrap();
        let prog = crate::scripting::parser::Parser::new(lex).parse().unwrap();

        let mut interpreter = interpreter(caller_source);
        interpreter
            .loaded_scripts
            .insert("scripts/helper.aeo".to_string(), prog);

        let mut em = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        interpreter
            .call(&mut instance, "run", vec![], &mut host)
            .unwrap();

        assert_eq!(instance.get_field("result"), Some(&Value::Number(1.0)));
    }

    #[test]
    fn test_missing_module_script_returns_runtime_error() {
        let caller_source = r#"
entity Test {
    fn main() {
        const m = scripts.missingScript.aeo
    }
}
"#;

        let mut interpreter = interpreter(caller_source);
        let mut em = test_host();
        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let result = interpreter.call(&mut instance, "main", vec![], &mut host);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("Script module not found"));
    }

    fn parse_program(source: &str) -> Program {
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        Parser::new(tokens).parse().expect("parser should succeed")
    }

    #[test]
    fn attached_module_self_parent_returns_actor_handle() {
        let mut interpreter = Interpreter::new(parse_program(""));

        interpreter.loaded_scripts.insert(
            "scripts/combat.aeo".to_string(),
            parse_program(
                r#"
fn parent_ref() {
    return self.parent
}
"#,
            ),
        );

        let mut entity_manager = EntityManager::new();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entity_manager,
        };

        let module = interpreter
            .get_or_create_attached_module_instance("scripts/combat.aeo", 20000001, &mut host)
            .expect("attached Module should instantiate");

        let parent = interpreter
            .call_object_method_direct(&module, "parent_ref", vec![], &mut host)
            .expect("self.parent should resolve");

        assert_eq!(
            parent,
            Value::Handle {
                kind: HandleKind::Actor,
                id: 20000001,
            }
        );
    }

    #[test]
    fn attach_and_detach_are_first_class_global_aeoscript_builtins() {
        let mut interpreter = interpreter(
            r#"
entity Test {
    fn mount(child, parent) {
        return attach(child, "Grip", parent, "RightHand")
    }

    fn unmount(child) {
        return detach(child)
    }
}
"#,
        );
        let mut entity_manager = test_host();
        let child = entity_manager.create_entity("Sword");
        let parent = entity_manager.create_entity("Player");
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entity_manager,
        };
        let mut instance = interpreter
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let mount_error = interpreter
            .call(
                &mut instance,
                "mount",
                vec![
                    Value::Handle {
                        kind: HandleKind::Entity,
                        id: child.0,
                    },
                    Value::Handle {
                        kind: HandleKind::Entity,
                        id: parent.0,
                    },
                ],
                &mut host,
            )
            .unwrap_err();
        assert!(mount_error.contains("attach is not supported on this host"));
        assert!(!mount_error.contains("unknown variable"));

        let detach_error = interpreter
            .call(
                &mut instance,
                "unmount",
                vec![Value::Handle {
                    kind: HandleKind::Entity,
                    id: child.0,
                }],
                &mut host,
            )
            .unwrap_err();
        assert!(detach_error.contains("detach is not supported on this host"));
        assert!(!detach_error.contains("unknown variable"));
    }

    #[test]
    fn standalone_module_self_parent_is_an_error() {
        let mut interpreter = Interpreter::new(parse_program(""));

        interpreter.loaded_scripts.insert(
            "scripts/combat.aeo".to_string(),
            parse_program(
                r#"
fn parent_ref() {
    return self.parent
}
"#,
            ),
        );

        let mut entity_manager = EntityManager::new();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entity_manager,
        };

        let module = interpreter
            .get_or_create_module_instance("scripts/combat.aeo", &mut host)
            .expect("standalone Module should instantiate");

        let error = interpreter
            .call_object_method_direct(&module, "parent_ref", vec![], &mut host)
            .expect_err("standalone self.parent must fail");

        assert!(
            error.contains("self.parent is unavailable"),
            "unexpected error: {}",
            error
        );
    }

    #[test]
    fn same_module_attached_to_different_actors_has_distinct_parent_context() {
        let mut interpreter = Interpreter::new(parse_program(""));

        interpreter.loaded_scripts.insert(
            "scripts/combat.aeo".to_string(),
            parse_program(
                r#"
fn parent_ref() {
    return self.parent
}
"#,
            ),
        );

        let mut entity_manager = EntityManager::new();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entity_manager,
        };

        let first = interpreter
            .get_or_create_attached_module_instance("scripts/combat.aeo", 100, &mut host)
            .unwrap();

        let second = interpreter
            .get_or_create_attached_module_instance("scripts/combat.aeo", 200, &mut host)
            .unwrap();

        let first_parent = interpreter
            .call_object_method_direct(&first, "parent_ref", vec![], &mut host)
            .unwrap();

        let second_parent = interpreter
            .call_object_method_direct(&second, "parent_ref", vec![], &mut host)
            .unwrap();

        assert_eq!(
            first_parent,
            Value::Handle {
                kind: HandleKind::Actor,
                id: 100,
            }
        );

        assert_eq!(
            second_parent,
            Value::Handle {
                kind: HandleKind::Actor,
                id: 200,
            }
        );
    }

    #[test]
    fn module_self_destroy_invalidates_instance_and_prevents_recreation() {
        let mut interpreter = Interpreter::new(parse_program(""));

        interpreter.loaded_scripts.insert(
            "scripts/temp.aeo".to_string(),
            parse_program(
                r#"
fn destroy_self() {
    self.destroy()
}

fn ping() {
    return 7
}
"#,
            ),
        );

        let mut entity_manager = EntityManager::new();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entity_manager,
        };

        let module = interpreter
            .get_or_create_module_instance("scripts/temp.aeo", &mut host)
            .expect("Module should instantiate");

        interpreter
            .call_object_method_direct(&module, "destroy_self", vec![], &mut host)
            .expect("self.destroy() should succeed");

        let error = interpreter
            .call_object_method_direct(&module, "ping", vec![], &mut host)
            .expect_err("destroyed Module must reject old references");

        assert!(
            error.contains("has been destroyed"),
            "unexpected error: {}",
            error
        );

        let recreated = interpreter
            .get_or_create_module_instance("scripts/temp.aeo", &mut host)
            .unwrap();

        assert_eq!(recreated, Value::Nil);
    }

    #[test]
    fn destroying_one_actors_modules_does_not_destroy_another_actors_instance() {
        let mut interpreter = Interpreter::new(parse_program(""));

        interpreter.loaded_scripts.insert(
            "scripts/shared.aeo".to_string(),
            parse_program(
                r#"
fn ping() {
    return 42
}
"#,
            ),
        );

        let mut entity_manager = EntityManager::new();

        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entity_manager,
        };

        let first = interpreter
            .get_or_create_attached_module_instance("scripts/shared.aeo", 100, &mut host)
            .unwrap();

        let second = interpreter
            .get_or_create_attached_module_instance("scripts/shared.aeo", 200, &mut host)
            .unwrap();

        assert_eq!(interpreter.destroy_modules_for_actor(100), 1);

        assert!(
            interpreter
                .call_object_method_direct(&first, "ping", vec![], &mut host,)
                .is_err()
        );

        assert_eq!(
            interpreter
                .call_object_method_direct(&second, "ping", vec![], &mut host,)
                .unwrap(),
            Value::Number(42.0)
        );

        let first_again = interpreter
            .get_or_create_attached_module_instance("scripts/shared.aeo", 100, &mut host)
            .unwrap();

        assert_eq!(first_again, Value::Nil);
    }
}
