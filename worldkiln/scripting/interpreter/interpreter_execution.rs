use std::sync::Arc;

use crate::scripting::api::HostContext;
use crate::scripting::ast::*;
use crate::scripting::execution::{FiberResult, YieldReason};
use crate::scripting::value::{Scope, Value};

use super::interpreter_compile::{compile_event, compile_function, compile_top_level};
use super::{
    CallFrame, ExecutionFlow, FRAME_PUSHED_SENTINEL, ForState, Interpreter, ScriptFiber,
    ScriptInstance,
};

impl ScriptFiber {
    pub(super) fn fail(&mut self, message: String) -> FiberResult {
        self.finished = true;
        FiberResult::Failed(message)
    }

    pub(super) fn complete(&mut self, value: Value) -> FiberResult {
        self.finished = true;
        self.result = Some(value);
        FiberResult::Complete
    }
}

impl Interpreter {
    /// Creates a persistent fiber for a function.
    ///
    /// The function body is compiled once into a compact execution plan.
    /// Subsequent resumes only advance the existing program counter and runtime
    /// state.
    pub fn start_fiber(
        &mut self,
        instance: ScriptInstance,
        function_name: &str,
        arguments: Vec<Value>,
    ) -> Result<ScriptFiber, String> {
        let entity_name = instance.entity_name.clone();

        let function = self
            .find_function_for_instance(&instance, function_name)
            .ok_or_else(|| {
                format!(
                    "function '{}' does not exist on entity '{}'",
                    function_name, entity_name
                )
            })?;

        if function.parameters.len() != arguments.len() {
            return Err(format!(
                "function '{}' expected {} argument(s), got {}",
                function.name,
                function.parameters.len(),
                arguments.len()
            ));
        }

        let cache_key = Self::function_cache_key(&instance, function_name);

        let compiled = if let Some(compiled) = self.compiled_functions.get(&cache_key) {
            Arc::clone(compiled)
        } else {
            let compiled = Arc::new(compile_function(&function));

            self.compiled_functions
                .insert(cache_key, Arc::clone(&compiled));

            compiled
        };

        let parameter_scope = Scope::new();

        for (parameter, argument) in function.parameters.iter().zip(arguments.into_iter()) {
            parameter_scope
                .declare(parameter.name.clone(), argument, false)
                .map_err(|error| {
                    format!("failed to bind parameter '{}': {}", parameter.name, error)
                })?;
        }

        let script_path = instance.script_path.clone();
        Ok(ScriptFiber {
            script_path: script_path.clone(),
            entity_name,
            instance,
            stack: vec![CallFrame {
                script_path,
                function: compiled,
                pc: 0,
                scopes: vec![parameter_scope],
                for_states: Vec::new(),
                function_name: function_name.to_string(),
                captured_scopes: Vec::new(),
                expects_return_value: false,
                call_site_span: None,
                caller_instance: None,
            }],
            finished: false,
            result: None,
            expression_returns: std::collections::BTreeMap::new(),
            last_execution: None,
        })
    }

    // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:16 -05:00
    pub fn start_named_script_function_fiber(
        &mut self,
        script_path: String,
        function: &FunctionDecl,
        captured_scope: Scope,
    ) -> Result<ScriptFiber, String> {
        if !function.parameters.is_empty() {
            return Err(format!(
                "authored UI handler '{}' must accept 0 arguments, found {}",
                function.name,
                function.parameters.len()
            ));
        }

        let mut instance = ScriptInstance::new_empty();
        instance.script_path = Some(script_path.clone());

        let cache_key = (format!("ui:{}", script_path), function.name.clone());
        let compiled = if let Some(compiled) = self.compiled_functions.get(&cache_key) {
            Arc::clone(compiled)
        } else {
            let compiled = Arc::new(compile_function(function));
            self.compiled_functions
                .insert(cache_key, Arc::clone(&compiled));
            compiled
        };

        self.start_direct_fiber(
            instance,
            compiled,
            Vec::new(),
            Vec::new(),
            None,
            function.name.clone(),
            vec![captured_scope],
        )
    }

    pub fn start_direct_fiber(
        &mut self,
        instance: ScriptInstance,
        compiled: Arc<CompiledFunction>,
        param_names: Vec<String>,
        arguments: Vec<Value>,
        self_value: Option<Value>,
        function_name: String,
        captured_scopes: Vec<Scope>,
    ) -> Result<ScriptFiber, String> {
        if param_names.len() != arguments.len() {
            return Err(format!(
                "anonymous function expected {} argument(s), got {}",
                param_names.len(),
                arguments.len()
            ));
        }

        let parameter_scope = Scope::new();

        if let Some(sv) = self_value {
            parameter_scope.declare("self", sv, false)?;
        }

        for (name, argument) in param_names.into_iter().zip(arguments.into_iter()) {
            parameter_scope
                .declare(name.clone(), argument, false)
                .map_err(|error| {
                    format!("failed to bind anonymous parameter '{}': {}", name, error)
                })?;
        }

        let script_path = instance.script_path.clone();
        Ok(ScriptFiber {
            script_path: script_path.clone(),
            entity_name: instance.entity_name().to_string(),
            instance,
            stack: vec![CallFrame {
                script_path,
                function: compiled,
                pc: 0,
                scopes: vec![parameter_scope],
                for_states: Vec::new(),
                function_name,
                captured_scopes,
                expects_return_value: false,
                call_site_span: None,
                caller_instance: None,
            }],
            finished: false,
            result: None,
            expression_returns: std::collections::BTreeMap::new(),
            last_execution: None,
        })
    }

    pub fn start_event_fiber(
        &mut self,
        instance: ScriptInstance,
        event: EventDecl,
        arguments: Vec<Value>,
        captured_scopes: Vec<Scope>,
    ) -> Result<ScriptFiber, String> {
        if event.parameters.len() != arguments.len() {
            return Err(format!(
                "event '{}' expected {} argument(s), got {}",
                event.name,
                event.parameters.len(),
                arguments.len()
            ));
        }

        let compiled = Arc::new(compile_event(&event));

        let parameter_scope = Scope::new();

        for (parameter, argument) in event.parameters.iter().zip(arguments.into_iter()) {
            parameter_scope
                .declare(parameter.name.clone(), argument, false)
                .map_err(|error| {
                    format!("failed to bind parameter '{}': {}", parameter.name, error)
                })?;
        }

        let script_path = instance.script_path.clone();
        Ok(ScriptFiber {
            script_path: script_path.clone(),
            entity_name: instance.entity_name().to_string(),
            instance,
            stack: vec![CallFrame {
                script_path,
                function: compiled,
                pc: 0,
                scopes: vec![parameter_scope],
                for_states: Vec::new(),
                function_name: event.name.clone(),
                captured_scopes,
                expects_return_value: false,
                call_site_span: None,
                caller_instance: None,
            }],
            finished: false,
            result: None,
            expression_returns: std::collections::BTreeMap::new(),
            last_execution: None,
        })
    }

    pub fn start_top_level_fiber(
        &mut self,
        instance: ScriptInstance,
        statements: &[Statement],
    ) -> Result<ScriptFiber, String> {
        self.start_top_level_fiber_with_scope(instance, statements, Scope::new())
    }

    pub fn start_top_level_fiber_with_scope(
        &mut self,
        instance: ScriptInstance,
        statements: &[Statement],
        top_level_scope: Scope,
    ) -> Result<ScriptFiber, String> {
        let compiled = Arc::new(compile_top_level(statements));

        let script_path = instance.script_path.clone();
        Ok(ScriptFiber {
            script_path: script_path.clone(),
            entity_name: instance.entity_name().to_string(),
            instance,
            stack: vec![CallFrame {
                script_path,
                function: compiled,
                pc: 0,
                scopes: vec![top_level_scope],
                expects_return_value: false,
                for_states: Vec::new(),
                function_name: "top-level".to_string(),
                captured_scopes: Vec::new(),
                call_site_span: None,
                caller_instance: None,
            }],
            finished: false,
            result: None,
            expression_returns: std::collections::BTreeMap::new(),
            last_execution: None,
        })
    }

    pub fn resume_fiber(&mut self, fiber: &mut ScriptFiber, host: &mut HostContext) -> FiberResult {
        self.resume_fiber_with_debug(fiber, host, |_| false)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:57 -05:00
    pub fn resume_fiber_with_debug<F>(
        &mut self,
        fiber: &mut ScriptFiber,
        host: &mut HostContext,
        mut should_pause: F,
    ) -> FiberResult
    where
        F: FnMut(&crate::scripting::debugger::DebugInstructionPoint) -> bool,
    {
        if fiber.finished {
            return FiberResult::Failed("cannot resume a completed AeoScript fiber.".to_string());
        }

        let old_budget = self.operations_remaining;
        let old_depth = self.call_depth;
        let old_path = self.current_script_path.clone();
        let old_entity = self.current_entity_name.clone();
        let old_id = self.current_entity_id;
        let old_function = self.current_function_name.clone();

        let nested_execution = old_function.is_some();

        if !nested_execution {
            self.operations_remaining = self.operation_budget;
        }

        self.current_script_path = fiber.script_path.clone();
        self.current_entity_name = Some(fiber.entity_name.clone());
        self.current_entity_id = Some(fiber.instance().id());

        let result = loop {
            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:57 -05:00
            // Debugging stops before a debuggable instruction mutates runtime state.
            if let Some(frame) = fiber.stack.last() {
                if frame.pc < frame.function.instructions.len() {
                    if let Some(span) = frame.function.instructions[frame.pc].source_span() {
                        let path = frame
                            .script_path
                            .as_deref()
                            .or(fiber.script_path.as_deref())
                            .unwrap_or("<unknown>");
                        let point = crate::scripting::debugger::DebugInstructionPoint {
                            script_path: crate::scripting::debugger::normalize_script_path(path),
                            function_name: frame.function_name.clone(),
                            span,
                            stack_depth: fiber.stack.len(),
                        };
                        if should_pause(&point) {
                            break FiberResult::Paused;
                        }
                    }
                }
            }

            let stack_depth = fiber.stack.len() + 1;

            let Some(mut frame) = fiber.stack.pop() else {
                break fiber.complete(Value::Nil);
            };

            self.current_function_name = Some(frame.function_name.clone());

            if frame.pc >= frame.function.instructions.len() {
                if let Some(first_scope) = frame.scopes.first() {
                    if let Some(Value::Object(obj_arc)) = first_scope.get("self") {
                        *obj_arc.borrow_mut() = fiber.instance.clone();
                    }
                }

                if let Some(caller) = frame.caller_instance {
                    fiber.instance = caller;
                }

                if fiber.stack.is_empty() {
                    break fiber.complete(Value::Nil);
                }

                if frame.expects_return_value {
                    if let Some(call_site) = frame.call_site_span {
                        fiber
                            .expression_returns
                            .insert((fiber.stack.len(), call_site), Value::Nil);
                    }
                }
                continue;
            }

            if let Err(error) = self.tick() {
                self.log_error(error.clone());
                break fiber.fail(error);
            }

            let instruction = frame.function.instructions[frame.pc].clone();
            fiber.last_execution = Some((
                frame
                    .script_path
                    .clone()
                    .or_else(|| fiber.script_path.clone()),
                frame.function_name.clone(),
                instruction.source_span().or(frame.call_site_span),
            ));

            match instruction {
                Instruction::EnterScope => {
                    frame.scopes.push(Scope::new());
                    frame.pc += 1;
                }

                Instruction::ExitScope => {
                    if frame.scopes.len() <= 1 {
                        let error = "AeoScript runtime scope underflow.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    }

                    frame.scopes.pop();
                    frame.pc += 1;
                }

                Instruction::Variable {
                    name,
                    initializer,
                    is_const,
                } => {
                    let captured_scopes = frame.captured_scopes.clone();

                    let value = match initializer {
                        Some(initializer) => match self.eval_expression_on_fiber(
                            fiber,
                            &mut frame.scopes,
                            &initializer,
                            host,
                            &captured_scopes,
                        ) {
                            Ok(value) => value,

                            Err(error) if error == FRAME_PUSHED_SENTINEL => {
                                let insert_at = fiber.stack.len().saturating_sub(1);
                                fiber.stack.insert(insert_at, frame);
                                continue;
                            }

                            Err(error) => {
                                self.log_error(error.clone());
                                break fiber.fail(error);
                            }
                        },

                        None => Value::Nil,
                    };

                    let Some(scope) = frame.scopes.last() else {
                        let error = "AeoScript runtime has no active scope.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    };

                    if let Err(error) = scope.declare(name.clone(), value.clone(), is_const) {
                        self.log_error(error.clone());
                        break fiber.fail(error);
                    }

                    if frame.function_name == "top-level" {
                        let _ = fiber.instance.set_field(&name, value.clone());
                    }

                    frame.pc += 1;
                }

                Instruction::Assignment {
                    target,
                    operator,
                    value,
                } => {
                    let captured_scopes = frame.captured_scopes.clone();

                    let right = match self.eval_expression_on_fiber(
                        fiber,
                        &mut frame.scopes,
                        &value,
                        host,
                        &captured_scopes,
                    ) {
                        Ok(value) => value,

                        Err(error) if error == FRAME_PUSHED_SENTINEL => {
                            let insert_at = fiber.stack.len().saturating_sub(1);
                            fiber.stack.insert(insert_at, frame);
                            continue;
                        }

                        Err(error) => {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }
                    };

                    if let Err(error) = self.assign_target(
                        &mut fiber.instance,
                        &mut frame.scopes,
                        &target,
                        operator,
                        right,
                        host,
                        &captured_scopes,
                    ) {
                        self.log_error(error.clone());
                        break fiber.fail(error);
                    }

                    frame.pc += 1;
                }

                Instruction::Evaluate(expression) => {
                    let captured_scopes = frame.captured_scopes.clone();

                    match self.eval_expression_on_fiber(
                        fiber,
                        &mut frame.scopes,
                        &expression,
                        host,
                        &captured_scopes,
                    ) {
                        Ok(_) => {}

                        Err(error) if error == FRAME_PUSHED_SENTINEL => {
                            let insert_at = fiber.stack.len().saturating_sub(1);
                            fiber.stack.insert(insert_at, frame);
                            continue;
                        }

                        Err(error) => {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }
                    }

                    frame.pc += 1;
                }

                Instruction::Jump { target } => {
                    frame.pc = target;
                }

                Instruction::JumpIfFalse { condition, target } => {
                    let captured_scopes = frame.captured_scopes.clone();

                    let value = match self.eval_expression_on_fiber(
                        fiber,
                        &mut frame.scopes,
                        &condition,
                        host,
                        &captured_scopes,
                    ) {
                        Ok(value) => value,

                        Err(error) if error == FRAME_PUSHED_SENTINEL => {
                            let insert_at = fiber.stack.len().saturating_sub(1);
                            fiber.stack.insert(insert_at, frame);
                            continue;
                        }

                        Err(error) => {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }
                    };

                    match value.is_truthy() {
                        Ok(true) => frame.pc += 1,

                        Ok(false) => frame.pc = target,

                        Err(error) => {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }
                    }
                }

                Instruction::ForInit {
                    name,
                    iterable,
                    end,
                } => {
                    let captured_scopes = frame.captured_scopes.clone();

                    let iterable_value = match self.eval_expression(
                        &mut fiber.instance,
                        &mut frame.scopes,
                        &iterable,
                        host,
                        &captured_scopes,
                    ) {
                        Ok(value) => value,

                        Err(error) => {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }
                    };

                    let values = match iterable_value {
                        Value::Array(values) => values.borrow().elements.clone(),

                        other => {
                            let error =
                                format!("cannot iterate over {} in for loop", other.type_name());

                            self.log_error(error.clone());

                            break fiber.fail(error);
                        }
                    };

                    if values.is_empty() {
                        frame.pc = end;
                        fiber.stack.push(frame);
                        continue;
                    }

                    let first_value = values[0].clone();

                    frame.for_states.push(ForState {
                        name: name.clone(),
                        values,
                        next_index: 1,
                    });

                    let Some(scope) = frame.scopes.last() else {
                        let error = "AeoScript runtime has no active scope.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    };

                    if let Err(error) = scope.declare(name, first_value, false) {
                        self.log_error(error.clone());
                        break fiber.fail(error);
                    }

                    frame.pc += 1;
                }

                Instruction::ForNext { body_start, end } => {
                    let Some(loop_state) = frame.for_states.last_mut() else {
                        let error = "AeoScript runtime for-loop state underflow.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    };

                    if loop_state.next_index < loop_state.values.len() {
                        let value = loop_state.values[loop_state.next_index].clone();

                        loop_state.next_index += 1;

                        let name = loop_state.name.clone();

                        let Some(scope) = frame.scopes.last_mut() else {
                            let error = "AeoScript runtime has no active scope.".to_string();

                            self.log_error(error.clone());

                            break fiber.fail(error);
                        };

                        *scope = Scope::new();

                        if let Err(error) = scope.declare(name, value, false) {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }

                        frame.pc = body_start;
                    } else {
                        frame.for_states.pop();
                        frame.pc = end;
                    }
                }

                Instruction::CallUserFunction {
                    name, arguments, ..
                } => {
                    let resolved_callable = self.resolve_identifier(
                        &fiber.instance,
                        &frame.scopes,
                        &name,
                        &frame.captured_scopes,
                    );

                    // Worldkiln 0.8.7 | WO 0.8.7-23 | 2026-10-06 17:25 -05:00
                    // A failing argument owns the diagnostic span; the parent call
                    // span remains the debugger's step location.
                    let mut values = Vec::with_capacity(arguments.len());
                    let mut argument_error = None;
                    for argument in &arguments {
                        match self.eval_expression(
                            &mut fiber.instance,
                            &mut frame.scopes,
                            argument,
                            host,
                            &frame.captured_scopes,
                        ) {
                            Ok(value) => values.push(value),
                            Err(error) => {
                                argument_error = Some((error, argument.span));
                                break;
                            }
                        }
                    }

                    if let Some((error, span)) = argument_error {
                        fiber.last_execution = Some((
                            frame
                                .script_path
                                .clone()
                                .or_else(|| fiber.script_path.clone()),
                            frame.function_name.clone(),
                            Some(span),
                        ));
                        self.log_error(error.clone());
                        break fiber.fail(error);
                    }

                    match resolved_callable {
                        Ok(Value::Function(compiled, params, captures)) => {
                            if params.len() != values.len() {
                                let error = format!(
                                    "function '{}' expected {} argument(s), got {}",
                                    name,
                                    params.len(),
                                    values.len()
                                );

                                self.log_error(error.clone());

                                break fiber.fail(error);
                            }

                            if stack_depth >= self.max_call_depth {
                                let error = format!(
                                    "AeoScript call depth exceeded the limit of {}.",
                                    self.max_call_depth
                                );

                                self.log_error(error.clone());

                                break fiber.fail(error);
                            }

                            let parameter_scope = Scope::new();

                            let bind_result = (|| -> Result<(), String> {
                                for (param_name, argument) in
                                    params.into_iter().zip(values.into_iter())
                                {
                                    parameter_scope.declare(param_name, argument, false)?;
                                }

                                Ok(())
                            })();

                            if let Err(error) = bind_result {
                                self.log_error(error.clone());
                                break fiber.fail(error);
                            }

                            frame.pc += 1;

                            fiber.stack.push(CallFrame {
                                script_path: frame.script_path.clone(),
                                function: compiled,
                                pc: 0,
                                scopes: vec![parameter_scope],
                                for_states: Vec::new(),
                                function_name: name,
                                captured_scopes: captures,
                                expects_return_value: false,
                                call_site_span: None,
                                caller_instance: None,
                            });

                            let insert_at = fiber.stack.len().saturating_sub(1);

                            fiber.stack.insert(insert_at, frame);

                            continue;
                        }

                        Ok(value) => {
                            let error = format!(
                                "cannot call '{}' because it is a {}",
                                name,
                                value.type_name()
                            );

                            self.log_error(error.clone());

                            break fiber.fail(error);
                        }

                        Err(_) => {
                            match crate::scripting::api::call_host_function(host, &name, &values) {
                                Ok(Some(_)) => {
                                    frame.pc += 1;
                                    fiber.stack.push(frame);
                                    continue;
                                }

                                Ok(None) => {}

                                Err(error) => {
                                    self.log_error(error.clone());
                                    break fiber.fail(error);
                                }
                            }

                            match name.as_str() {
                                "print" => {
                                    self.log_values(&values);
                                    frame.pc += 1;
                                    fiber.stack.push(frame);
                                    continue;
                                }

                                "get_parent" => {
                                    frame.pc += 1;
                                    fiber.stack.push(frame);
                                    continue;
                                }

                                "wait" => {
                                    if values.len() != 1 {
                                        let error =
                                            "wait() expects exactly one argument.".to_string();

                                        self.log_error(error.clone());

                                        break fiber.fail(error);
                                    }

                                    let seconds = match values[0].as_number() {
                                        Ok(seconds) => seconds,

                                        Err(error) => {
                                            self.log_error(error.clone());
                                            break fiber.fail(error);
                                        }
                                    };

                                    if !seconds.is_finite() || seconds <= 0.0 {
                                        let error =
                                            "AeoScript wait duration must be finite and greater than zero."
                                                .to_string();

                                        self.log_error(error.clone());

                                        break fiber.fail(error);
                                    }

                                    frame.pc += 1;
                                    fiber.stack.push(frame);

                                    break FiberResult::Yield(YieldReason::WaitSeconds(seconds));
                                }

                                _ => {}
                            }

                            let function =
                                match self.find_function_for_instance(&fiber.instance, &name) {
                                    Some(function) => function,

                                    None => {
                                        let error = format!("function '{}' does not exist", name);

                                        self.log_error(error.clone());

                                        break fiber.fail(error);
                                    }
                                };

                            if function.parameters.len() != values.len() {
                                let error = format!(
                                    "function '{}' expected {} argument(s), got {}",
                                    function.name,
                                    function.parameters.len(),
                                    values.len()
                                );

                                self.log_error(error.clone());

                                break fiber.fail(error);
                            }

                            if stack_depth >= self.max_call_depth {
                                let error = format!(
                                    "AeoScript call depth exceeded the limit of {}.",
                                    self.max_call_depth
                                );

                                self.log_error(error.clone());

                                break fiber.fail(error);
                            }

                            let cache_key = Self::function_cache_key(&fiber.instance, &name);

                            let compiled =
                                if let Some(compiled) = self.compiled_functions.get(&cache_key) {
                                    Arc::clone(compiled)
                                } else {
                                    let compiled = Arc::new(compile_function(&function));

                                    self.compiled_functions
                                        .insert(cache_key, Arc::clone(&compiled));

                                    compiled
                                };

                            let parameter_scope = Scope::new();

                            let bind_result = (|| -> Result<(), String> {
                                for (parameter, argument) in
                                    function.parameters.iter().zip(values.into_iter())
                                {
                                    parameter_scope.declare(
                                        parameter.name.clone(),
                                        argument,
                                        false,
                                    )?;
                                }

                                Ok(())
                            })();

                            if let Err(error) = bind_result {
                                self.log_error(error.clone());
                                break fiber.fail(error);
                            }

                            frame.pc += 1;

                            fiber.stack.push(CallFrame {
                                script_path: frame.script_path.clone(),
                                function: compiled,
                                pc: 0,
                                scopes: vec![parameter_scope],
                                for_states: Vec::new(),
                                function_name: name,
                                captured_scopes: Vec::new(),
                                expects_return_value: false,
                                call_site_span: None,
                                caller_instance: None,
                            });

                            let insert_at = fiber.stack.len().saturating_sub(1);

                            fiber.stack.insert(insert_at, frame);

                            continue;
                        }
                    }
                }

                Instruction::Wait { arguments, .. } => {
                    if arguments.len() != 1 {
                        let error = "wait() expects exactly one argument.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    }

                    let captured_scopes = frame.captured_scopes.clone();

                    let seconds = match self.eval_expression(
                        &mut fiber.instance,
                        &mut frame.scopes,
                        &arguments[0],
                        host,
                        &captured_scopes,
                    ) {
                        Ok(value) => match value.as_number() {
                            Ok(value) => value,

                            Err(error) => {
                                self.log_error(error.clone());
                                break fiber.fail(error);
                            }
                        },

                        Err(error) => {
                            self.log_error(error.clone());
                            break fiber.fail(error);
                        }
                    };

                    if !seconds.is_finite() {
                        let error = "AeoScript wait duration must be finite.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    }

                    if seconds <= 0.0 {
                        let error =
                            "AeoScript wait duration must be greater than zero.".to_string();

                        self.log_error(error.clone());

                        break fiber.fail(error);
                    }

                    frame.pc += 1;
                    fiber.stack.push(frame);

                    break FiberResult::Yield(YieldReason::WaitSeconds(seconds));
                }

                Instruction::Return(expression) => {
                    let captured_scopes = frame.captured_scopes.clone();

                    let value = match expression {
                        Some(expression) => match self.eval_expression_on_fiber(
                            fiber,
                            &mut frame.scopes,
                            &expression,
                            host,
                            &captured_scopes,
                        ) {
                            Ok(value) => value,

                            Err(error) if error == FRAME_PUSHED_SENTINEL => {
                                let insert_at = fiber.stack.len().saturating_sub(1);

                                fiber.stack.insert(insert_at, frame);

                                continue;
                            }

                            Err(error) => {
                                self.log_error(error.clone());
                                break fiber.fail(error);
                            }
                        },

                        None => Value::Nil,
                    };

                    if let Some(first_scope) = frame.scopes.first() {
                        if let Some(Value::Object(obj_arc)) = first_scope.get("self") {
                            *obj_arc.borrow_mut() = fiber.instance.clone();
                        }
                    }

                    if let Some(caller) = frame.caller_instance {
                        fiber.instance = caller;
                    }

                    if fiber.stack.is_empty() {
                        break fiber.complete(value);
                    }

                    if frame.expects_return_value {
                        if let Some(call_site) = frame.call_site_span {
                            fiber
                                .expression_returns
                                .insert((fiber.stack.len(), call_site), value);
                        }
                    }

                    continue;
                }
            }

            let completed_frame_depth = fiber.stack.len() + 1;
            fiber
                .expression_returns
                .retain(|(depth, _), _| *depth != completed_frame_depth);
            fiber.stack.push(frame);
        };

        self.operations_remaining = old_budget;
        self.call_depth = old_depth;
        self.current_script_path = old_path;
        self.current_entity_name = old_entity;
        self.current_entity_id = old_id;
        self.current_function_name = old_function;

        result
    }

    pub(super) fn reset_execution_budget(&mut self) {
        self.operations_remaining = self.operation_budget;
        self.call_depth = 0;
    }

    pub(super) fn tick(&mut self) -> Result<(), String> {
        if self.operations_remaining == 0 {
            return Err("AeoScript execution budget exhausted.".to_string());
        }

        self.operations_remaining -= 1;

        Ok(())
    }

    pub(super) fn execute_block(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        block: &Block,
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<ExecutionFlow, String> {
        scopes.push(Scope::new());

        let result = (|| {
            for statement in &block.statements {
                match self.execute_statement(instance, scopes, statement, host, captured_scopes)? {
                    ExecutionFlow::Continue => {}

                    ExecutionFlow::Return(value) => {
                        return Ok(ExecutionFlow::Return(value));
                    }
                }
            }

            Ok(ExecutionFlow::Continue)
        })();

        scopes.pop();

        result
    }

    fn execute_statement(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        statement: &Statement,
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<ExecutionFlow, String> {
        self.tick()?;

        match &statement.kind {
            StatementKind::Variable {
                name,
                initializer,
                is_const,
                ..
            } => {
                let value = if let Some(initializer) = initializer {
                    self.eval_expression(instance, scopes, initializer, host, captured_scopes)?
                } else {
                    Value::Nil
                };

                scopes
                    .last_mut()
                    .expect("function execution always has a scope")
                    .declare(name.clone(), value, *is_const)?;

                Ok(ExecutionFlow::Continue)
            }

            StatementKind::Assignment {
                target,
                operator,
                value,
            } => {
                let right = self.eval_expression(instance, scopes, value, host, captured_scopes)?;

                self.assign_target(
                    instance,
                    scopes,
                    target,
                    *operator,
                    right,
                    host,
                    captured_scopes,
                )?;

                Ok(ExecutionFlow::Continue)
            }

            StatementKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                let condition_value =
                    self.eval_expression(instance, scopes, condition, host, captured_scopes)?;

                if condition_value.is_truthy()? {
                    return self.execute_block(instance, scopes, then_block, host, captured_scopes);
                }

                for (else_if_condition, block) in else_if {
                    let value = self.eval_expression(
                        instance,
                        scopes,
                        else_if_condition,
                        host,
                        captured_scopes,
                    )?;

                    if value.is_truthy()? {
                        return self.execute_block(instance, scopes, block, host, captured_scopes);
                    }
                }

                if let Some(block) = else_block {
                    return self.execute_block(instance, scopes, block, host, captured_scopes);
                }

                Ok(ExecutionFlow::Continue)
            }

            StatementKind::While { condition, body } => {
                loop {
                    self.tick()?;

                    let value =
                        self.eval_expression(instance, scopes, condition, host, captured_scopes)?;

                    if !value.is_truthy()? {
                        break;
                    }

                    match self.execute_block(instance, scopes, body, host, captured_scopes)? {
                        ExecutionFlow::Continue => {}

                        ExecutionFlow::Return(value) => {
                            return Ok(ExecutionFlow::Return(value));
                        }
                    }
                }

                Ok(ExecutionFlow::Continue)
            }

            StatementKind::For {
                name,
                iterable,
                body,
            } => {
                let iterable_value =
                    self.eval_expression(instance, scopes, iterable, host, captured_scopes)?;

                let values = match iterable_value {
                    Value::Array(values) => values.borrow().elements.clone(),

                    other => {
                        return Err(format!(
                            "cannot iterate over {} in for loop",
                            other.type_name()
                        ));
                    }
                };

                let result = (|| {
                    for value in values {
                        self.tick()?;

                        scopes.push(Scope::new());

                        scopes.last().expect("iteration scope exists").declare(
                            name.clone(),
                            value,
                            false,
                        )?;

                        let iteration_result =
                            self.execute_block(instance, scopes, body, host, captured_scopes);

                        scopes.pop();

                        let iteration_result = iteration_result?;

                        match iteration_result {
                            ExecutionFlow::Continue => {}

                            ExecutionFlow::Return(value) => {
                                return Ok(ExecutionFlow::Return(value));
                            }
                        }
                    }

                    Ok(ExecutionFlow::Continue)
                })();

                result
            }

            StatementKind::Return(value) => {
                let result = if let Some(expression) = value {
                    self.eval_expression(instance, scopes, expression, host, captured_scopes)?
                } else {
                    Value::Nil
                };

                Ok(ExecutionFlow::Return(result))
            }

            StatementKind::Expression(expression) => {
                self.eval_expression(instance, scopes, expression, host, captured_scopes)?;

                Ok(ExecutionFlow::Continue)
            }
        }
    }
}
