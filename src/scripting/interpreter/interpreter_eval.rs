// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::sync::Arc;

use crate::scripting::api::{
    HostContext, call_host_function, call_host_member, resolve_host_member_property,
    resolve_host_property, set_host_member_property,
};
use crate::scripting::ast::*;
use crate::scripting::log::{LogRecord, LogSeverity};
use crate::scripting::source::SourceSpan;
use crate::scripting::value::{HandleKind, MapKey, Scope, Value};

use super::interpreter_compile::{FunctionCompiler, compile_function};
use super::{CallFrame, FRAME_PUSHED_SENTINEL, Interpreter, ScriptFiber, ScriptInstance};

impl Interpreter {
    fn traceback_value(&self, fiber: Option<&ScriptFiber>) -> Value {
        let mut frames = vec![format!(
            "{} ({})",
            self.current_function_name.as_deref().unwrap_or("top-level"),
            self.current_script_path.as_deref().unwrap_or("<runtime>")
        )];
        if let Some(fiber) = fiber {
            for caller in fiber.stack.iter().rev() {
                frames.push(format!(
                    "{} ({})",
                    caller.function_name,
                    caller
                        .script_path
                        .as_deref()
                        .or(fiber.script_path.as_deref())
                        .unwrap_or("<runtime>")
                ));
            }
        }
        Value::String(frames.join("\n"))
    }

    fn resolve_member_value(
        &mut self,
        object_value: Value,
        name: &String,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        if let Value::Namespace(ref namespace) = object_value {
            if let Some(value) = resolve_host_property(host, namespace, name)? {
                return Ok(value);
            }

            if let Some(value) =
                crate::scripting::stdlib::resolve_stdlib_property(namespace, name, host)?
            {
                return Ok(value);
            }

            if let Some(parent_id) = Self::project_scope_parent(namespace) {
                if name == "find" {
                    return Ok(Value::Namespace(Self::project_find_namespace(parent_id)));
                }

                return self.resolve_project_scope_member(parent_id, name, host);
            }

            if namespace == "scripts"
                || namespace.starts_with("scripts/")
                || namespace.starts_with("scripts.")
            {
                if name == "aeo" {
                    let script_path = if namespace.ends_with(".aeo") {
                        namespace.to_string()
                    } else {
                        format!("{}.aeo", namespace)
                    };
                    return self.get_or_create_module_instance(&script_path, host);
                }

                let new_path = if namespace == "scripts" {
                    format!("scripts/{}", name)
                } else {
                    format!("{}/{}", namespace, name)
                };

                if new_path.ends_with(".aeo") {
                    return self.get_or_create_module_instance(&new_path, host);
                }

                return Ok(Value::Namespace(new_path));
            }
        }

        match object_value {
            Value::Map(map) => {
                let borrowed = map.borrow();

                Ok(borrowed
                    .get(&MapKey::String(name.clone()))
                    .cloned()
                    .unwrap_or(Value::Nil))
            }

            Value::Handle { kind, id } => {
                if let Some(value) = resolve_host_member_property(host, kind, id, name)? {
                    return Ok(value);
                }

                if kind == HandleKind::Actor && host.engine.get_player_actor_id() == Some(id) {
                    if name == "find" {
                        return Ok(Value::Namespace(format!("player#{}/find", id)));
                    }

                    if let Some(module) = self.resolve_player_module_member(id, name, host)? {
                        return Ok(module);
                    }
                }

                if kind == HandleKind::Actor {
                    if let Some(script_path) = host.engine.get_actor_module_path(id, name) {
                        return self.get_or_create_attached_module_instance(&script_path, id, host);
                    }
                }

                Err(format!(
                    "engine member '{}' is not available on handle kind {} yet",
                    name,
                    kind.name()
                ))
            }

            Value::Object(ref instance_arc) => {
                let borrowed = instance_arc.borrow();

                if borrowed.is_module() && borrowed.is_destroyed() {
                    return Err(format!(
                        "Module '{}' has been destroyed",
                        borrowed.entity_name()
                    ));
                }

                if borrowed.is_module() && name == "parent" {
                    if let Some(actor_id) = borrowed.parent_actor_id() {
                        return Ok(Value::Handle {
                            kind: HandleKind::Actor,
                            id: actor_id,
                        });
                    }

                    return Err(format!(
                        "Module '{}' is not attached to an Actor; self.parent is unavailable",
                        borrowed.entity_name()
                    ));
                }

                if let Some(val) = borrowed.get_field(name) {
                    Ok(val.clone())
                } else if let Some(function) = self.find_function_for_instance(&borrowed, name) {
                    let cache_key = Self::function_cache_key(&borrowed, name);

                    let compiled = if let Some(compiled) = self.compiled_functions.get(&cache_key) {
                        Arc::clone(compiled)
                    } else {
                        Arc::new(compile_function(&function))
                    };

                    let param_names = function.parameters.iter().map(|p| p.name.clone()).collect();

                    Ok(Value::Function(compiled, param_names, vec![]))
                } else {
                    Ok(Value::Nil)
                }
            }

            other => Err(format!(
                "cannot access member '{}' on {}",
                name,
                other.type_name()
            )),
        }
    }

    pub(super) fn assign_target(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        target: &Expression,
        operator: AssignmentOperator,
        right: Value,
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<(), String> {
        match &target.kind {
            ExpressionKind::Identifier(name) => {
                let value = if operator == AssignmentOperator::Assign {
                    right
                } else {
                    let left = self.resolve_identifier(instance, scopes, name, captured_scopes)?;

                    self.apply_assignment(operator, left, right)?
                };

                for scope in scopes.iter_mut().rev() {
                    if scope.contains(name) {
                        return scope.set(name, value);
                    }
                }

                for scope in captured_scopes.iter().rev() {
                    if scope.contains(name) {
                        return scope.set(name, value);
                    }
                }

                if instance.get_field(name).is_some() {
                    return instance.set_field(name, value);
                }

                Err(format!(
                    "Cannot assign to undeclared variable '{name}'. Declare it first with `name: Type = ...` or `const name = ...`."
                ))
            }

            ExpressionKind::Member { object, name } => {
                let object_value =
                    self.eval_expression(instance, scopes, object, host, captured_scopes)?;

                let value = if operator == AssignmentOperator::Assign {
                    right
                } else {
                    let left = match &object_value {
                        Value::Namespace(namespace) => {
                            crate::scripting::stdlib::resolve_stdlib_property(
                                namespace, name, host,
                            )?
                            .ok_or_else(|| format!("Cannot read {name} on {namespace}"))?
                        }
                        Value::Map(map) => map
                            .borrow()
                            .get(&MapKey::String(name.clone()))
                            .cloned()
                            .unwrap_or(Value::Nil),

                        Value::Handle { kind, id } => {
                            if let Some(v) = resolve_host_member_property(host, *kind, *id, name)? {
                                v
                            } else {
                                return Err(format!(
                                    "engine member '{}' is not available on handle kind {} yet",
                                    name,
                                    kind.name()
                                ));
                            }
                        }

                        Value::Object(instance_arc) => instance_arc
                            .borrow()
                            .get_field(name)
                            .cloned()
                            .unwrap_or(Value::Nil),

                        _ => {
                            return Err(format!(
                                "cannot access member on type {}",
                                object_value.type_name()
                            ));
                        }
                    };

                    self.apply_assignment(operator, left, right)?
                };

                match object_value {
                    Value::Namespace(namespace) => {
                        if name == "alpha" {
                            if let Some((material, layer)) =
                                crate::scripting::stdlib::material_layer_namespace(&namespace)
                            {
                                return host.engine.material_layer_set_alpha(
                                    material,
                                    layer,
                                    value.as_number()?,
                                );
                            }
                        }
                        Err(format!(
                            "Cannot assign {name} on engine namespace {namespace}"
                        ))
                    }
                    Value::Map(map) => {
                        if matches!(value, Value::Nil) {
                            map.borrow_mut().remove(&MapKey::String(name.clone()));
                        } else {
                            map.borrow_mut().insert(MapKey::String(name.clone()), value);
                        }

                        Ok(())
                    }

                    Value::Handle { kind, id } => set_host_member_property(
                        host,
                        kind,
                        id,
                        name,
                        value,
                        instance.script_path.clone(),
                    ),

                    Value::Object(instance_arc) => {
                        if matches!(value, Value::Nil) {
                            instance_arc.borrow_mut().set_field(name, Value::Nil)?;
                        } else {
                            instance_arc.borrow_mut().set_field(name, value)?;
                        }

                        Ok(())
                    }

                    _ => Err(format!(
                        "cannot assign to member of type {}",
                        object_value.type_name()
                    )),
                }
            }

            ExpressionKind::Index {
                object: target_object,
                index,
            } => {
                if let ExpressionKind::Member {
                    object: handle_expr,
                    name,
                } = &target_object.kind
                {
                    if name == "attributes" {
                        let handle_value = self.eval_expression(
                            instance,
                            scopes,
                            handle_expr,
                            host,
                            captured_scopes,
                        )?;

                        if let Value::Handle { kind, id } = handle_value {
                            if matches!(
                                kind,
                                HandleKind::Cell
                                    | HandleKind::Light
                                    | HandleKind::Actor
                                    | HandleKind::Entity
                            ) {
                                let key_value = self.eval_expression(
                                    instance,
                                    scopes,
                                    index,
                                    host,
                                    captured_scopes,
                                )?;

                                let key = key_value.as_string()?;

                                let value_to_assign = if operator == AssignmentOperator::Assign {
                                    right
                                } else {
                                    let left = host
                                        .engine
                                        .get_handle_attribute(kind, id, key)?
                                        .unwrap_or(Value::Nil);
                                    self.apply_assignment(operator, left, right)?
                                };

                                if matches!(value_to_assign, Value::Nil) {
                                    host.engine.remove_handle_attribute(kind, id, key)?;
                                } else {
                                    host.engine.set_handle_attribute(
                                        kind,
                                        id,
                                        key.to_string(),
                                        value_to_assign,
                                    )?;
                                }

                                return Ok(());
                            }
                        }
                    }
                }

                let object_value =
                    self.eval_expression(instance, scopes, target_object, host, captured_scopes)?;

                let index_value =
                    self.eval_expression(instance, scopes, index, host, captured_scopes)?;

                let value = if operator == AssignmentOperator::Assign {
                    right
                } else {
                    let left = match &object_value {
                        Value::Array(array) => {
                            let idx = index_value.as_index()?;

                            array
                                .borrow()
                                .elements
                                .get(idx)
                                .cloned()
                                .ok_or_else(|| format!("index {} out of bounds", idx))?
                        }

                        Value::Map(map) => {
                            let key = index_value.as_map_key()?;

                            map.borrow().get(&key).cloned().unwrap_or(Value::Nil)
                        }

                        _ => {
                            return Err(format!("cannot index type {}", object_value.type_name()));
                        }
                    };

                    self.apply_assignment(operator, left, right)?
                };

                match object_value {
                    Value::Array(array) => {
                        let idx = index_value.as_index()?;

                        let mut borrowed = array.borrow_mut();

                        if borrowed.frozen {
                            return Err("cannot mutate frozen basket".to_string());
                        }

                        if idx >= borrowed.elements.len() {
                            return Err(format!("index {} out of bounds", idx));
                        }

                        borrowed.elements[idx] = value;

                        Ok(())
                    }

                    Value::Map(map) => {
                        let key = index_value.as_map_key()?;

                        if matches!(value, Value::Nil) {
                            map.borrow_mut().remove(&key);
                        } else {
                            map.borrow_mut().insert(key, value);
                        }

                        Ok(())
                    }

                    _ => Err(format!(
                        "cannot assign to index of type {}",
                        object_value.type_name()
                    )),
                }
            }

            _ => Err(format!(
                "cannot assign to expression of type {:?}",
                target.kind
            )),
        }
    }

    pub(super) fn apply_assignment(
        &self,
        operator: AssignmentOperator,
        left: Value,
        right: Value,
    ) -> Result<Value, String> {
        match operator {
            AssignmentOperator::Assign => Ok(right),

            AssignmentOperator::Add => self.apply_binary(BinaryOperator::Add, left, right),

            AssignmentOperator::Subtract => {
                self.apply_binary(BinaryOperator::Subtract, left, right)
            }

            AssignmentOperator::Multiply => {
                self.apply_binary(BinaryOperator::Multiply, left, right)
            }

            AssignmentOperator::Divide => self.apply_binary(BinaryOperator::Divide, left, right),
        }
    }

    pub(super) fn invoke_callable_opt_fiber(
        &mut self,
        instance: &mut ScriptInstance,
        compiled: Arc<CompiledFunction>,
        params: Vec<String>,
        arguments: Vec<Value>,
        self_value: Option<Value>,
        function_name: String,
        host: &mut HostContext,
        captures: Vec<Scope>,
        call_site_span: Option<SourceSpan>,
        opt_fiber: Option<&mut ScriptFiber>,
    ) -> Result<Value, String> {
        if instance.is_module() {
            if let Some(script_path) = instance.script_path.as_deref() {
                if !host.engine.is_script_enabled(script_path) {
                    return Err(format!("Script Module '{}' is disabled", script_path));
                }
            }
        }

        if let Some(fiber) = opt_fiber {
            if fiber.stack.len() >= self.max_call_depth {
                return Err(format!(
                    "AeoScript call depth exceeded the limit of {}.",
                    self.max_call_depth
                ));
            }

            if params.len() != arguments.len() {
                return Err(format!(
                    "function expected {} argument(s), got {}",
                    params.len(),
                    arguments.len()
                ));
            }

            let caller_instance = match self_value.as_ref() {
                Some(Value::Object(obj_arc)) => {
                    let caller = instance.clone();
                    fiber.instance = obj_arc.borrow().clone();
                    Some(caller)
                }
                _ => {
                    fiber.instance = instance.clone();
                    None
                }
            };
            let callee_script_path = fiber.instance.script_path.clone();

            let parameter_scope = Scope::new();

            if let Some(sv) = self_value {
                parameter_scope.declare("self", sv, false)?;
            }

            for (name, argument) in params.into_iter().zip(arguments.into_iter()) {
                parameter_scope.declare(name, argument, false)?;
            }

            fiber.stack.push(CallFrame {
                script_path: callee_script_path,
                function: compiled,
                pc: 0,
                scopes: vec![parameter_scope],
                for_states: Vec::new(),
                function_name,
                captured_scopes: captures,
                expects_return_value: true,
                call_site_span,
                caller_instance,
            });

            Err(FRAME_PUSHED_SENTINEL.to_string())
        } else {
            self.call_direct(
                instance,
                compiled,
                params,
                arguments,
                self_value,
                function_name,
                host,
                captures,
            )
        }
    }

    pub(super) fn eval_expression_on_fiber(
        &mut self,
        fiber: &mut ScriptFiber,
        scopes: &mut Vec<Scope>,
        expression: &Expression,
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<Value, String> {
        let mut instance = std::mem::replace(&mut fiber.instance, ScriptInstance::new_empty());

        let result = self.eval_expression_opt_fiber(
            &mut instance,
            scopes,
            expression,
            host,
            captured_scopes,
            Some(fiber),
        );

        if let Err(error) = &result {
            if error == FRAME_PUSHED_SENTINEL {
                return result;
            }
        }

        fiber.instance = instance;

        result
    }

    pub(super) fn eval_expression(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        expression: &Expression,
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<Value, String> {
        self.eval_expression_opt_fiber(instance, scopes, expression, host, captured_scopes, None)
    }

    pub(super) fn eval_expression_opt_fiber(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        expression: &Expression,
        host: &mut HostContext,
        captured_scopes: &[Scope],
        mut opt_fiber: Option<&mut ScriptFiber>,
    ) -> Result<Value, String> {
        self.tick()?;

        if matches!(
            expression.kind,
            ExpressionKind::Call { .. } | ExpressionKind::MethodCall { .. }
        ) {
            if let Some(fiber) = opt_fiber.as_deref_mut() {
                let frame_depth = fiber.stack.len() + 1;
                if let Some(value) = fiber
                    .expression_returns
                    .get(&(frame_depth, expression.span))
                {
                    return Ok(value.clone());
                }
            }
        }

        match &expression.kind {
            ExpressionKind::Literal(val) => Ok(val.clone()),

            ExpressionKind::Number(value) => Ok(Value::Number(*value)),

            ExpressionKind::String(value) => Ok(Value::String(value.clone())),

            ExpressionKind::Bool(value) => Ok(Value::Bool(*value)),

            ExpressionKind::Nil => Ok(Value::Nil),

            ExpressionKind::Identifier(name) => {
                let value = self.resolve_identifier(instance, scopes, name, captured_scopes)?;

                if matches!(
                    &value,
                    Value::Namespace(namespace)
                        if namespace == "player"
                ) {
                    if let Some(actor_id) = host.engine.get_player_actor_id() {
                        return Ok(Value::Handle {
                            kind: HandleKind::Actor,
                            id: actor_id,
                        });
                    }

                    return Ok(Value::Nil);
                }

                Ok(value)
            }

            ExpressionKind::Unary {
                operator,
                expression,
            } => {
                let value = self.eval_expression_opt_fiber(
                    instance,
                    scopes,
                    expression,
                    host,
                    captured_scopes,
                    opt_fiber,
                )?;

                match operator {
                    UnaryOperator::Negate => Ok(Value::Number(-value.as_number()?)),

                    UnaryOperator::Not => Ok(Value::Bool(!value.is_truthy()?)),
                }
            }

            ExpressionKind::Binary {
                left,
                operator,
                right,
            } => {
                if *operator == BinaryOperator::And {
                    let left_value = self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        left,
                        host,
                        captured_scopes,
                        opt_fiber.as_deref_mut(),
                    )?;

                    if !left_value.is_truthy()? {
                        return Ok(Value::Bool(false));
                    }

                    let right_value = self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        right,
                        host,
                        captured_scopes,
                        opt_fiber,
                    )?;

                    return Ok(Value::Bool(right_value.is_truthy()?));
                }

                if *operator == BinaryOperator::Or {
                    let left_value = self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        left,
                        host,
                        captured_scopes,
                        opt_fiber.as_deref_mut(),
                    )?;

                    if left_value.is_truthy()? {
                        return Ok(Value::Bool(true));
                    }

                    let right_value = self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        right,
                        host,
                        captured_scopes,
                        opt_fiber,
                    )?;

                    return Ok(Value::Bool(right_value.is_truthy()?));
                }

                let left_value = self.eval_expression_opt_fiber(
                    instance,
                    scopes,
                    left,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                let right_value = self.eval_expression_opt_fiber(
                    instance,
                    scopes,
                    right,
                    host,
                    captured_scopes,
                    opt_fiber,
                )?;

                self.apply_binary(*operator, left_value, right_value).map_err(|error| {
                    let _location = crate::engine::development_capture::operation("script", "binary_failure_location", false)
                        .map(|span| span.diagnostic(serde_json::json!({
                            "script": instance.script_path, "caller_script": self.current_script_path,
                            "function": self.current_function_name,
                            "expression_bytes": [expression.span.start, expression.span.end],
                            "left_bytes": [left.span.start, left.span.end],
                            "right_bytes": [right.span.start, right.span.end],
                            "error": error,
                        })));
                    error
                })
            }

            ExpressionKind::Call { callee, arguments } => self.eval_call_opt_fiber(
                instance,
                scopes,
                expression.span,
                callee,
                arguments,
                host,
                captured_scopes,
                opt_fiber,
            ),

            ExpressionKind::MethodCall {
                object,
                method,
                arguments,
            } => {
                let object_value = self.eval_expression_opt_fiber(
                    instance,
                    scopes,
                    object,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                if let Value::Namespace(ref name) = object_value {
                    let arg_values = self.eval_arguments_opt_fiber(
                        instance,
                        scopes,
                        arguments,
                        host,
                        captured_scopes,
                        opt_fiber.as_deref_mut(),
                    )?;

                    if name == "debug" && method == "traceback" {
                        if !arg_values.is_empty() {
                            return Err("debug.traceback expects no arguments".into());
                        }
                        return Ok(self.traceback_value(opt_fiber.as_deref()));
                    }

                    if let Some(res) = crate::scripting::stdlib::call_stdlib_function(
                        self,
                        instance,
                        scopes,
                        name,
                        method,
                        &arg_values,
                        host,
                    )? {
                        return Ok(res);
                    }
                }

                if let Value::Namespace(ref name) = object_value {
                    if name == "debug" && method == "log" {
                        let arg_values = self.eval_arguments_opt_fiber(
                            instance,
                            scopes,
                            arguments,
                            host,
                            captured_scopes,
                            opt_fiber,
                        )?;

                        self.log_values(&arg_values);

                        return Ok(Value::Nil);
                    }
                }

                let arg_values = self.eval_arguments_opt_fiber(
                    instance,
                    scopes,
                    arguments,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                match object_value {
                    Value::Handle { kind, id } => {
                        if let Some(result) =
                            self.resolve_attachment_method(kind, id, method, &arg_values, host)?
                        {
                            return Ok(result);
                        }
                        if let Some(result) = call_host_member(host, kind, id, method, &arg_values)?
                        {
                            Ok(result)
                        } else if let Some(crate::scripting::value::DynamicProperty {
                            value: Value::Function(compiled, params, caps),
                            ..
                        }) = host.engine.get_script_property(kind, id, method)
                        {
                            self.invoke_callable_opt_fiber(
                                instance,
                                compiled,
                                params,
                                arg_values,
                                Some(object_value),
                                method.clone(),
                                host,
                                caps,
                                Some(expression.span),
                                opt_fiber,
                            )
                        } else {
                            Err(format!(
                                "engine method '{}' is not available on handle kind {}",
                                method,
                                kind.name()
                            ))
                        }
                    }

                    _ => Err(format!(
                        "cannot call method '{}' on {}",
                        method,
                        object_value.type_name()
                    )),
                }
            }

            ExpressionKind::Member { object, name } => {
                let owner = self.eval_expression_opt_fiber(
                    instance,
                    scopes,
                    object,
                    host,
                    captured_scopes,
                    opt_fiber,
                )?;
                self.resolve_member_value(owner, name, host)
            }

            ExpressionKind::Index { object, index } => {
                let object = if let ExpressionKind::Member {
                    object: owner,
                    name,
                } = &object.kind
                {
                    if name == "attributes" {
                        let subject = self.eval_expression_opt_fiber(
                            instance,
                            scopes,
                            owner,
                            host,
                            captured_scopes,
                            opt_fiber.as_deref_mut(),
                        )?;
                        match subject {
                            Value::Handle { kind, id }
                                if matches!(
                                    kind,
                                    HandleKind::Actor
                                        | HandleKind::Entity
                                        | HandleKind::Cell
                                        | HandleKind::Light
                                ) =>
                            {
                                let key_value = self.eval_expression_opt_fiber(
                                    instance,
                                    scopes,
                                    index,
                                    host,
                                    captured_scopes,
                                    opt_fiber,
                                )?;
                                return match &key_value {
                                    Value::String(key) => Ok(host
                                        .engine
                                        .get_handle_attribute(kind, id, key)?
                                        .unwrap_or(Value::Nil)),
                                    _ => {
                                        key_value.as_map_key()?;
                                        Ok(Value::Nil)
                                    }
                                };
                            }
                            other => self.resolve_member_value(other, name, host)?,
                        }
                    } else {
                        self.eval_expression_opt_fiber(
                            instance,
                            scopes,
                            object,
                            host,
                            captured_scopes,
                            opt_fiber.as_deref_mut(),
                        )?
                    }
                } else {
                    self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        object,
                        host,
                        captured_scopes,
                        opt_fiber.as_deref_mut(),
                    )?
                };

                let index = self.eval_expression_opt_fiber(
                    instance,
                    scopes,
                    index,
                    host,
                    captured_scopes,
                    opt_fiber,
                )?;

                self.eval_index(object, index)
            }

            ExpressionKind::Array(expressions) => {
                let mut values = Vec::with_capacity(expressions.len());

                for expression in expressions {
                    values.push(self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        expression,
                        host,
                        captured_scopes,
                        opt_fiber.as_deref_mut(),
                    )?);
                }

                Ok(Value::array(values))
            }

            ExpressionKind::Map(entries) => {
                let mut values = BTreeMap::new();

                for (key_expr, expression) in entries {
                    let key_value = self.eval_expression_opt_fiber(
                        instance,
                        scopes,
                        key_expr,
                        host,
                        captured_scopes,
                        opt_fiber.as_deref_mut(),
                    )?;

                    let key = key_value.as_map_key()?;

                    values.insert(
                        key,
                        self.eval_expression_opt_fiber(
                            instance,
                            scopes,
                            expression,
                            host,
                            captured_scopes,
                            opt_fiber.as_deref_mut(),
                        )?,
                    );
                }

                Ok(Value::Map(Arc::new(RefCell::new(values))))
            }

            ExpressionKind::AnonymousFunction {
                parameters,
                return_type: _,
                body,
            } => {
                let mut compiler = FunctionCompiler {
                    instructions: Vec::new(),
                };

                compiler.compile_block(body);

                let compiled = Arc::new(CompiledFunction {
                    instructions: compiler.instructions,
                });

                let param_names = parameters
                    .iter()
                    .map(|parameter| parameter.name.clone())
                    .collect();

                let mut captured_env = Vec::with_capacity(captured_scopes.len() + scopes.len());

                for scope in captured_scopes.iter().chain(scopes.iter()) {
                    if !captured_env
                        .iter()
                        .any(|existing: &Scope| existing == scope)
                    {
                        captured_env.push(scope.clone());
                    }
                }

                Ok(Value::Function(compiled, param_names, captured_env))
            }
        }
    }

    pub(super) fn eval_call(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        call_span: SourceSpan,
        callee: &Expression,
        arguments: &[Expression],
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<Value, String> {
        self.eval_call_opt_fiber(
            instance,
            scopes,
            call_span,
            callee,
            arguments,
            host,
            captured_scopes,
            None,
        )
    }

    pub fn call_object_method_direct(
        &mut self,
        object: &Value,
        method_name: &str,
        args: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let Value::Object(instance_arc) = object else {
            return Err(format!(
                "expected Value::Object, got {}",
                object.type_name()
            ));
        };

        let (entity_name, is_module, is_destroyed) = {
            let borrowed = instance_arc.borrow();

            (
                borrowed.entity_name().to_string(),
                borrowed.is_module(),
                borrowed.is_destroyed(),
            )
        };

        if is_module && method_name == "destroy" {
            if !args.is_empty() {
                return Err("Module.destroy() expects 0 arguments".to_string());
            }

            return Ok(Value::Bool(self.destroy_module_instance(object)));
        }

        if is_module && is_destroyed {
            return Err(format!("Module '{}' has been destroyed", entity_name));
        }

        let Some(function) = self.find_function_for_instance(&instance_arc.borrow(), method_name)
        else {
            return Err(format!(
                "cannot access method '{}' on {}",
                method_name, entity_name
            ));
        };

        let cache_key = Self::function_cache_key(&instance_arc.borrow(), method_name);

        let compiled = if let Some(compiled) = self.compiled_functions.get(&cache_key) {
            Arc::clone(compiled)
        } else {
            let compiled = Arc::new(compile_function(&function));

            self.compiled_functions
                .insert(cache_key.clone(), Arc::clone(&compiled));

            compiled
        };

        let param_names = function.parameters.iter().map(|p| p.name.clone()).collect();

        let mut target_instance = instance_arc.borrow().clone();
        let module_scopes = if target_instance.is_module() {
            let key = match target_instance.parent_actor_id() {
                Some(actor_id) => format!(
                    "actor:{}:{}",
                    actor_id,
                    target_instance.script_path.as_deref().unwrap_or_default()
                ),
                None => format!(
                    "standalone:{}",
                    target_instance.script_path.as_deref().unwrap_or_default()
                ),
            };
            self.module_scopes.get(&key).cloned().into_iter().collect()
        } else {
            Vec::new()
        };

        let result = self.invoke_callable_opt_fiber(
            &mut target_instance,
            compiled,
            param_names,
            args,
            Some(Value::Object(Arc::clone(instance_arc))),
            method_name.to_string(),
            host,
            module_scopes,
            None,
            None,
        )?;

        if target_instance.is_module() {
            if let Some(script_path) = target_instance.script_path.as_deref() {
                let module_key = match target_instance.parent_actor_id() {
                    Some(actor_id) => {
                        format!("actor:{}:{}", actor_id, script_path)
                    }

                    None => {
                        format!("standalone:{}", script_path)
                    }
                };

                if self.destroyed_module_keys.contains(&module_key) {
                    target_instance.mark_destroyed();
                }
            }
        }

        if !instance_arc.borrow().is_module() && !instance_arc.borrow().is_destroyed() {
            *instance_arc.borrow_mut() = target_instance;
        }

        Ok(result)
    }

    pub(crate) fn eval_call_opt_fiber(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        call_span: SourceSpan,
        callee: &Expression,
        arguments: &[Expression],
        host: &mut HostContext,
        captured_scopes: &[Scope],
        mut opt_fiber: Option<&mut ScriptFiber>,
    ) -> Result<Value, String> {
        if let ExpressionKind::Member { object, name } = &callee.kind {
            let object_value = self.eval_expression_opt_fiber(
                instance,
                scopes,
                object,
                host,
                captured_scopes,
                opt_fiber.as_deref_mut(),
            )?;

            if let Value::Namespace(ref namespace) = object_value {
                let arg_values = self.eval_arguments_opt_fiber(
                    instance,
                    scopes,
                    arguments,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                if let Some(parent_id) = Self::project_scope_parent(namespace) {
                    if name == "find" {
                        if arg_values.len() != 1 {
                            return Err("find expects exactly 1 argument (name)".to_string());
                        }

                        let query = arg_values[0].as_string()?;

                        return self.resolve_project_actor_find(parent_id, query, false, host);
                    }
                }

                if let Some(parent_id) = Self::project_find_scope(namespace) {
                    if name == "all" {
                        if arg_values.len() != 1 {
                            return Err("find.all expects exactly 1 argument (name)".to_string());
                        }

                        let query = arg_values[0].as_string()?;

                        return self.resolve_project_actor_find(parent_id, query, true, host);
                    }
                }

                if let Some(player_actor_id) = Self::player_find_actor_id(namespace) {
                    if name == "all" {
                        if arg_values.len() != 1 {
                            return Err(
                                "player.find.all expects exactly 1 argument (name)".to_string()
                            );
                        }

                        let query = arg_values[0].as_string()?;

                        return self.resolve_player_module_find(player_actor_id, query, true, host);
                    }
                }

                if namespace == "debug" && name == "traceback" {
                    if !arg_values.is_empty() {
                        return Err("debug.traceback expects no arguments".into());
                    }
                    return Ok(self.traceback_value(opt_fiber.as_deref()));
                }
                if namespace == "debug" && name == "log" {
                    self.log_values(&arg_values);
                    return Ok(Value::Nil);
                }

                if let Some(res) = crate::scripting::stdlib::call_stdlib_function(
                    self,
                    instance,
                    scopes,
                    namespace,
                    name,
                    &arg_values,
                    host,
                )? {
                    return Ok(res);
                }

                if let Some(res) = call_host_function(host, name, &arg_values)? {
                    return Ok(res);
                }

                return Err(format!("unknown function '{}.{}'", namespace, name));
            }

            let arg_values = self.eval_arguments_opt_fiber(
                instance,
                scopes,
                arguments,
                host,
                captured_scopes,
                opt_fiber.as_deref_mut(),
            )?;

            match &object_value {
                Value::Handle { kind, id } => {
                    if let Some(result) =
                        self.resolve_attachment_method(*kind, *id, name, &arg_values, host)?
                    {
                        return Ok(result);
                    }
                    if *kind == HandleKind::Actor
                        && host.engine.get_player_actor_id() == Some(*id)
                        && matches!(
                            name.as_str(),
                            "is_valid"
                                | "set_velocity"
                                | "get_velocity"
                                | "set_gravity_scale"
                                | "get_gravity_scale"
                                | "set_controller_enabled"
                                | "is_controller_enabled"
                                | "set_horizontal_velocity"
                                | "set_facing_direction"
                                | "select_animation"
                                | "is_grounded"
                                | "apply_vertical_impulse"
                        )
                    {
                        if let Some(result) = crate::scripting::stdlib::call_stdlib_function(
                            self,
                            instance,
                            scopes,
                            "player",
                            name,
                            &arg_values,
                            host,
                        )? {
                            return Ok(result);
                        }
                    }
                    if *kind == HandleKind::Actor
                        && host.engine.get_player_actor_id() == Some(*id)
                        && name == "is_swimming"
                    {
                        if !arg_values.is_empty() {
                            return Err("player.is_swimming() expects 0 arguments".to_string());
                        }
                        return Ok(Value::Bool(host.engine.is_player_swimming()));
                    }
                    if *kind == HandleKind::Actor
                        && host.engine.get_player_actor_id() == Some(*id)
                        && name == "find"
                    {
                        if arg_values.len() != 1 {
                            return Err("player.find expects exactly 1 argument (name)".to_string());
                        }

                        let query = arg_values[0].as_string()?;

                        return self.resolve_player_module_find(*id, query, false, host);
                    }

                    let destroyed_actor_id = if name == "destroy" {
                        match kind {
                            HandleKind::Actor => Some(*id),
                            HandleKind::Entity => host.engine.get_script_actor_id_for_entity(*id),
                            _ => None,
                        }
                    } else {
                        None
                    };

                    if let Some(result) = call_host_member(host, *kind, *id, name, &arg_values)? {
                        if let Some(actor_id) = destroyed_actor_id {
                            self.destroy_modules_for_actor(actor_id);
                        }

                        return Ok(result);
                    }

                    if let Some(crate::scripting::value::DynamicProperty {
                        value: Value::Function(compiled, params, caps),
                        ..
                    }) = host.engine.get_script_property(*kind, *id, name)
                    {
                        return self.invoke_callable_opt_fiber(
                            instance,
                            compiled,
                            params,
                            arg_values,
                            Some(object_value),
                            name.clone(),
                            host,
                            caps,
                            Some(call_span),
                            opt_fiber,
                        );
                    }

                    return Err(format!(
                        "engine member '{}' is not available on handle kind {}",
                        name,
                        kind.name()
                    ));
                }

                Value::Array(_) | Value::Map(_) | Value::String(_) => {
                    let type_name = object_value.type_name();

                    let stdlib_ns = match type_name {
                        "basket" => "basket",
                        "map" => "map",
                        "string" => "string",
                        _ => type_name,
                    };

                    let mut full_args = Vec::with_capacity(arg_values.len() + 1);

                    full_args.push(object_value.clone());
                    full_args.extend(arg_values.clone());

                    if let Some(res) = crate::scripting::stdlib::call_stdlib_function(
                        self, instance, scopes, stdlib_ns, name, &full_args, host,
                    )? {
                        return Ok(res);
                    }

                    if let Value::Map(map) = &object_value {
                        if let Some(Value::Function(compiled, params, caps)) =
                            map.borrow().get(&MapKey::String(name.clone())).cloned()
                        {
                            return self.invoke_callable_opt_fiber(
                                instance,
                                compiled,
                                params,
                                arg_values,
                                None,
                                name.clone(),
                                host,
                                caps,
                                Some(call_span),
                                opt_fiber,
                            );
                        }
                    }

                    return Err(format!("cannot access member '{}' on {}", name, type_name));
                }

                Value::Object(instance_arc) => {
                    let (entity_name, is_module, is_destroyed) = {
                        let borrowed = instance_arc.borrow();

                        (
                            borrowed.entity_name().to_string(),
                            borrowed.is_module(),
                            borrowed.is_destroyed(),
                        )
                    };

                    if is_module && name == "destroy" {
                        if !arg_values.is_empty() {
                            return Err("Module.destroy() expects 0 arguments".to_string());
                        }

                        return Ok(Value::Bool(self.destroy_module_instance(&object_value)));
                    }

                    if is_module && is_destroyed {
                        return Err(format!("Module '{}' has been destroyed", entity_name));
                    }

                    let resolved_function = {
                        let borrowed = instance_arc.borrow();
                        self.find_function_for_instance(&borrowed, name)
                            .map(|function| {
                                let cache_key = Self::function_cache_key(&borrowed, name);
                                (function, cache_key)
                            })
                    };

                    if let Some((function, cache_key)) = resolved_function {
                        let compiled =
                            if let Some(compiled) = self.compiled_functions.get(&cache_key) {
                                Arc::clone(compiled)
                            } else {
                                let compiled = Arc::new(compile_function(&function));

                                self.compiled_functions
                                    .insert(cache_key.clone(), Arc::clone(&compiled));

                                compiled
                            };

                        let param_names =
                            function.parameters.iter().map(|p| p.name.clone()).collect();

                        let mut target_instance = instance_arc.borrow().clone();
                        let execution_instance = if opt_fiber.is_some() {
                            &mut *instance
                        } else {
                            &mut target_instance
                        };

                        let module_scopes = if is_module {
                            let borrowed = instance_arc.borrow();
                            let key = match borrowed.parent_actor_id() {
                                Some(actor_id) => format!(
                                    "actor:{}:{}",
                                    actor_id,
                                    borrowed.script_path.as_deref().unwrap_or_default()
                                ),
                                None => format!(
                                    "standalone:{}",
                                    borrowed.script_path.as_deref().unwrap_or_default()
                                ),
                            };
                            self.module_scopes.get(&key).cloned().into_iter().collect()
                        } else {
                            Vec::new()
                        };

                        let result = self.invoke_callable_opt_fiber(
                            execution_instance,
                            compiled,
                            param_names,
                            arg_values,
                            Some(Value::Object(Arc::clone(instance_arc))),
                            name.clone(),
                            host,
                            module_scopes,
                            Some(call_span),
                            opt_fiber,
                        )?;

                        if target_instance.is_module() {
                            if let Some(script_path) = target_instance.script_path.as_deref() {
                                let module_key = match target_instance.parent_actor_id() {
                                    Some(actor_id) => {
                                        format!("actor:{}:{}", actor_id, script_path)
                                    }

                                    None => {
                                        format!("standalone:{}", script_path)
                                    }
                                };

                                if self.destroyed_module_keys.contains(&module_key) {
                                    target_instance.mark_destroyed();
                                }
                            }
                        }

                        if !instance_arc.borrow().is_module()
                            && !instance_arc.borrow().is_destroyed()
                        {
                            *instance_arc.borrow_mut() = target_instance;
                        }

                        return Ok(result);
                    } else if let Some(val) = {
                        let borrowed = instance_arc.borrow();
                        borrowed.get_field(name).cloned()
                    } {
                        if let Value::Function(compiled, params, caps) = val {
                            let mut target_instance = instance_arc.borrow().clone();
                            let module_scopes = if target_instance.is_module() {
                                let key = match target_instance.parent_actor_id() {
                                    Some(actor_id) => format!(
                                        "actor:{}:{}",
                                        actor_id,
                                        target_instance.script_path.as_deref().unwrap_or_default()
                                    ),
                                    None => format!(
                                        "standalone:{}",
                                        target_instance.script_path.as_deref().unwrap_or_default()
                                    ),
                                };
                                self.module_scopes.get(&key).cloned().into_iter().collect()
                            } else {
                                Vec::new()
                            };
                            let execution_instance = if opt_fiber.is_some() {
                                &mut *instance
                            } else {
                                &mut target_instance
                            };

                            let result = self.invoke_callable_opt_fiber(
                                execution_instance,
                                compiled,
                                params,
                                arg_values,
                                Some(Value::Object(Arc::clone(instance_arc))),
                                name.clone(),
                                host,
                                if module_scopes.is_empty() {
                                    caps
                                } else {
                                    module_scopes
                                },
                                Some(call_span),
                                opt_fiber,
                            )?;

                            if target_instance.is_module() {
                                if let Some(script_path) = target_instance.script_path.as_deref() {
                                    let module_key = match target_instance.parent_actor_id() {
                                        Some(actor_id) => {
                                            format!("actor:{}:{}", actor_id, script_path)
                                        }

                                        None => {
                                            format!("standalone:{}", script_path)
                                        }
                                    };

                                    if self.destroyed_module_keys.contains(&module_key) {
                                        target_instance.mark_destroyed();
                                    }
                                }
                            }

                            if !instance_arc.borrow().is_module()
                                && !instance_arc.borrow().is_destroyed()
                            {
                                *instance_arc.borrow_mut() = target_instance;
                            }

                            return Ok(result);
                        }
                    }

                    return Err(format!(
                        "cannot access method '{}' on {}",
                        name, entity_name
                    ));
                }

                _ => {}
            }
        }

        if let ExpressionKind::Identifier(ref type_name) = callee.kind {
            if let Some(entity) = self.find_entity(type_name) {
                let arg_values = self.eval_arguments_opt_fiber(
                    instance,
                    scopes,
                    arguments,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                let mut obj_instance = ScriptInstance::new_entity(type_name);

                for member in &entity.members {
                    if let EntityMember::Field(field) = member {
                        let init_val = if let Some(ref init_expr) = field.initializer {
                            self.eval_expression_opt_fiber(
                                &mut obj_instance,
                                scopes,
                                init_expr,
                                host,
                                captured_scopes,
                                opt_fiber.as_deref_mut(),
                            )?
                        } else {
                            Value::Nil
                        };

                        obj_instance.set_field(&field.name, init_val)?;
                    }
                }

                let obj_arc = Arc::new(RefCell::new(obj_instance));

                if let Some(constructor) = entity.members.iter().find_map(|m| match m {
                    EntityMember::Function(f) if f.name == "constructor" => Some(f.clone()),

                    _ => None,
                }) {
                    let cache_key = (type_name.clone(), "constructor".to_string());

                    let compiled = if let Some(compiled) = self.compiled_functions.get(&cache_key) {
                        Arc::clone(compiled)
                    } else {
                        let compiled = Arc::new(compile_function(&constructor));

                        self.compiled_functions
                            .insert(cache_key, Arc::clone(&compiled));

                        compiled
                    };

                    let param_names = constructor
                        .parameters
                        .iter()
                        .map(|p| p.name.clone())
                        .collect();

                    let mut target_instance = obj_arc.borrow().clone();
                    let execution_instance = if opt_fiber.is_some() {
                        &mut *instance
                    } else {
                        &mut target_instance
                    };

                    let _ = self.invoke_callable_opt_fiber(
                        execution_instance,
                        compiled,
                        param_names,
                        arg_values,
                        Some(Value::Object(Arc::clone(&obj_arc))),
                        "constructor".to_string(),
                        host,
                        vec![],
                        Some(call_span),
                        opt_fiber,
                    )?;

                    *obj_arc.borrow_mut() = target_instance;
                }

                return Ok(Value::Object(obj_arc));
            }
        }

        let resolved = match self.eval_expression_opt_fiber(
            instance,
            scopes,
            callee,
            host,
            captured_scopes,
            opt_fiber.as_deref_mut(),
        ) {
            Ok(value) => Some(value),

            Err(e) if e == FRAME_PUSHED_SENTINEL => {
                return Err(e);
            }

            Err(_) if matches!(callee.kind, ExpressionKind::Identifier(_)) => None,

            Err(error) => return Err(error),
        };

        match resolved {
            Some(Value::Function(compiled, params, captures)) => {
                let values = self.eval_arguments_opt_fiber(
                    instance,
                    scopes,
                    arguments,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                self.invoke_callable_opt_fiber(
                    instance,
                    compiled,
                    params,
                    values,
                    None,
                    "anonymous".to_string(),
                    host,
                    captures,
                    Some(call_span),
                    opt_fiber,
                )
            }

            Some(Value::Namespace(ref name)) => {
                let values = self.eval_arguments_opt_fiber(
                    instance,
                    scopes,
                    arguments,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                if let Some(result) = call_host_function(host, name, &values)? {
                    return Ok(result);
                }

                match name.as_str() {
                    "print" => {
                        self.log_values(&values);
                        Ok(Value::Nil)
                    }

                    "wait" => Err(
                        "wait() is a yielding operation and must be used as a standalone statement on a resumable AeoScript fiber."
                            .to_string(),
                    ),

                    _ => Err(format!("cannot call namespace '{}'", name)),
                }
            }

            Some(value) => Err(format!("cannot call a {} value", value.type_name())),

            None => {
                let ExpressionKind::Identifier(name) = &callee.kind else {
                    return Err("that expression cannot be called yet.".to_string());
                };

                let values = self.eval_arguments_opt_fiber(
                    instance,
                    scopes,
                    arguments,
                    host,
                    captured_scopes,
                    opt_fiber.as_deref_mut(),
                )?;

                if let Some(result) = call_host_function(host, name, &values)? {
                    return Ok(result);
                }

                match name.as_str() {
                    "print" => {
                        self.log_values(&values);
                        Ok(Value::Nil)
                    }

                    "wait" => Err(
                        "wait() is a yielding operation and must be used as a standalone statement on a resumable AeoScript fiber."
                            .to_string(),
                    ),

                    "get_parent" => {
                        if !values.is_empty() {
                            return Err(
                                "get_parent() expects no arguments"
                                    .to_string(),
                            );
                        }

                        if instance.id() == 0 {
                            return Err(
                                "get_parent() called from an unattached script"
                                    .to_string(),
                            );
                        }

                        if let Some((kind, id)) =
                            host.engine.get_parent(
                                HandleKind::Entity,
                                instance.id(),
                            )
                        {
                            Ok(Value::Handle { kind, id })
                        } else {
                            Ok(Value::Nil)
                        }
                    }

                    _ => {
                        let function = self
                            .find_function_for_instance(instance, name)
                            .ok_or_else(|| {
                                format!(
                                    "function '{}' does not exist",
                                    name
                                )
                            })?;

                        let cache_key = Self::function_cache_key(instance, name);

                        let compiled =
                            if let Some(compiled) =
                                self.compiled_functions
                                    .get(&cache_key)
                            {
                                Arc::clone(compiled)
                            } else {
                                let compiled =
                                    Arc::new(
                                        compile_function(
                                            &function,
                                        ),
                                    );

                                self.compiled_functions
                                    .insert(
                                        cache_key,
                                        Arc::clone(&compiled),
                                    );

                                compiled
                            };

                        let param_names = function
                            .parameters
                            .iter()
                            .map(|p| p.name.clone())
                            .collect();

                        self.invoke_callable_opt_fiber(
                            instance,
                            compiled,
                            param_names,
                            values,
                            None,
                            name.clone(),
                            host,
                            vec![],
                            Some(call_span),
                            opt_fiber,
                        )
                    }
                }
            }
        }
    }

    pub(super) fn eval_arguments(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        arguments: &[Expression],
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<Vec<Value>, String> {
        self.eval_arguments_opt_fiber(instance, scopes, arguments, host, captured_scopes, None)
    }

    pub(super) fn eval_arguments_opt_fiber(
        &mut self,
        instance: &mut ScriptInstance,
        scopes: &mut Vec<Scope>,
        arguments: &[Expression],
        host: &mut HostContext,
        captured_scopes: &[Scope],
        mut opt_fiber: Option<&mut ScriptFiber>,
    ) -> Result<Vec<Value>, String> {
        let mut values = Vec::with_capacity(arguments.len());

        for argument in arguments {
            values.push(self.eval_expression_opt_fiber(
                instance,
                scopes,
                argument,
                host,
                captured_scopes,
                opt_fiber.as_deref_mut(),
            )?);
        }

        Ok(values)
    }

    fn resolve_project_actor_find(
        &mut self,
        parent_id: Option<crate::project::ProjectNodeId>,
        query: &str,
        all: bool,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let matches = self
            .project_hierarchy
            .as_ref()
            .map(|hierarchy| {
                hierarchy.find_actor_ids_named_recursive(
                    parent_id,
                    &self.project_actor_names,
                    query,
                )
            })
            .unwrap_or_default()
            .into_iter()
            .filter(|actor_id| host.engine.actor_exists(*actor_id))
            .map(|actor_id| Value::Handle {
                kind: HandleKind::Actor,
                id: actor_id,
            })
            .collect::<Vec<_>>();

        if all {
            return Ok(Value::array(matches));
        }

        match matches.len() {
            0 => {
                self.log_warning(format!(
                    "find(\"{}\"): expected 1 match, found 0; returning nil",
                    query
                ));

                Ok(Value::Nil)
            }

            1 => Ok(matches[0].clone()),

            count => {
                self.log_warning(format!(
                    "find(\"{}\"): expected 1 match, found {}; using [0]",
                    query, count
                ));

                Ok(matches[0].clone())
            }
        }
    }

    fn resolve_attachment_method(
        &self,
        kind: HandleKind,
        id: u64,
        method: &str,
        arguments: &[Value],
        host: &mut HostContext,
    ) -> Result<Option<Value>, String> {
        let Some(hierarchy) = self.project_hierarchy.as_ref() else {
            return Ok(None);
        };
        let cell_handle = |cell_id: u64, host: &HostContext| Value::Handle {
            kind: if host.engine.is_light_enabled(cell_id).is_some() {
                HandleKind::Light
            } else {
                HandleKind::Cell
            },
            id: cell_id,
        };
        match (kind, method) {
            (HandleKind::Actor, "get_attachments") => {
                if !arguments.is_empty() {
                    return Err("Actor.get_attachments() expects 0 arguments".into());
                }
                let mut values: Vec<Value> = hierarchy
                    .attachment_cell_nodes(id)
                    .into_iter()
                    .filter_map(|node_id| match &hierarchy.node(node_id)?.kind {
                        crate::project::ProjectNodeKind::Cell { cell_id, .. } => {
                            Some(cell_handle(*cell_id, host))
                        }
                        _ => None,
                    })
                    .collect();
                values.extend(
                    host.engine
                        .runtime_attachment_children(id)
                        .into_iter()
                        .map(|child| cell_handle(child, host)),
                );
                Ok(Some(Value::array(values)))
            }
            (HandleKind::Actor, "find_attachment_by_name") => {
                if arguments.len() != 1 {
                    return Err("Actor.find_attachment_by_name(name) expects 1 string".into());
                }
                let name = arguments[0].as_string()?;
                for child in host.engine.runtime_attachment_children(id) {
                    if host.engine.runtime_attachment_name(child).as_deref() == Some(name) {
                        return Ok(Some(cell_handle(child, host)));
                    }
                }
                for node_id in hierarchy.attachment_cell_nodes(id) {
                    if let Some(crate::project::ProjectNodeKind::Cell { cell_id, label, .. }) =
                        hierarchy.node(node_id).map(|node| &node.kind)
                    {
                        if label.as_deref() == Some(name) {
                            return Ok(Some(cell_handle(*cell_id, host)));
                        }
                    }
                }
                Ok(Some(Value::Nil))
            }
            (HandleKind::Cell | HandleKind::Light | HandleKind::Sound, "get_parent_actor") => {
                if !arguments.is_empty() {
                    return Err(format!(
                        "{}.get_parent_actor() expects 0 arguments",
                        kind.name()
                    ));
                }
                let parent = hierarchy
                    .find_cell_node(id)
                    .and_then(|node_id| hierarchy.cell_attachment(node_id))
                    .map(|binding| binding.parent_actor_id)
                    .or_else(|| host.engine.runtime_attachment_parent(id))
                    .map(|actor_id| Value::Handle {
                        kind: HandleKind::Actor,
                        id: actor_id,
                    })
                    .unwrap_or(Value::Nil);
                Ok(Some(parent))
            }
            _ => Ok(None),
        }
    }

    fn project_scope_parent(namespace: &str) -> Option<Option<crate::project::ProjectNodeId>> {
        if namespace == "proj" {
            return Some(None);
        }

        let node_id = namespace
            .strip_prefix("proj#")?
            .parse::<crate::project::ProjectNodeId>()
            .ok()?;

        Some(Some(node_id))
    }

    fn project_find_scope(namespace: &str) -> Option<Option<crate::project::ProjectNodeId>> {
        if namespace == "proj/find" {
            return Some(None);
        }

        let encoded = namespace.strip_prefix("proj#")?.strip_suffix("/find")?;

        let node_id = encoded.parse::<crate::project::ProjectNodeId>().ok()?;

        Some(Some(node_id))
    }

    fn project_find_namespace(parent_id: Option<crate::project::ProjectNodeId>) -> String {
        match parent_id {
            Some(node_id) => format!("proj#{}/find", node_id),
            None => "proj/find".to_string(),
        }
    }

    fn resolve_project_scope_member(
        &mut self,
        parent_id: Option<crate::project::ProjectNodeId>,
        name: &str,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let Some(hierarchy) = self.project_hierarchy.as_ref() else {
            return Ok(Value::Nil);
        };

        let matches =
            hierarchy.find_direct_named_children(parent_id, &self.project_actor_names, name);

        let mut values = Vec::new();

        for node_id in matches {
            let Some(node) = hierarchy.node(node_id) else {
                continue;
            };

            match &node.kind {
                crate::project::ProjectNodeKind::Folder { .. } => {
                    values.push(Value::Namespace(format!("proj#{}", node_id)));
                }

                crate::project::ProjectNodeKind::Actor { actor_id } => {
                    if host.engine.actor_exists(*actor_id) {
                        values.push(Value::Handle {
                            kind: HandleKind::Actor,
                            id: *actor_id,
                        });
                    }
                }

                crate::project::ProjectNodeKind::Script { .. }
                | crate::project::ProjectNodeKind::Cell { .. } => {}
            }
        }

        match values.len() {
            0 => Ok(Value::Nil),

            1 => Ok(values.remove(0)),

            count => {
                self.log_warning(format!(
                    "project path member '{}': expected 1 match, found {}; using [0]",
                    name, count
                ));

                Ok(values.remove(0))
            }
        }
    }

    fn player_find_actor_id(namespace: &str) -> Option<u64> {
        namespace
            .strip_prefix("player#")?
            .strip_suffix("/find")?
            .parse::<u64>()
            .ok()
    }

    fn player_module_paths_named(&self, name: &str, host: &HostContext) -> Vec<String> {
        let normalized_name = name
            .trim()
            .strip_suffix(".aeo")
            .unwrap_or(name.trim())
            .to_lowercase();

        self.player_module_paths
            .iter()
            .filter(|path| {
                host.engine.is_script_enabled(path)
                    && std::path::Path::new(path)
                        .file_stem()
                        .and_then(|stem| stem.to_str())
                        .is_some_and(|stem| stem.to_lowercase() == normalized_name)
            })
            .cloned()
            .collect()
    }

    fn resolve_player_module_member(
        &mut self,
        player_actor_id: u64,
        name: &str,
        host: &mut HostContext,
    ) -> Result<Option<Value>, String> {
        let paths = self.player_module_paths_named(name, host);

        if paths.is_empty() {
            return Ok(None);
        }

        if paths.len() > 1 {
            self.log_warning(format!(
                "player.{}: expected 1 Module, found {}; using [0]",
                name,
                paths.len()
            ));
        }

        let value =
            self.get_or_create_attached_module_instance(&paths[0], player_actor_id, host)?;

        Ok(Some(value))
    }

    fn resolve_player_module_find(
        &mut self,
        player_actor_id: u64,
        query: &str,
        all: bool,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let paths = self.player_module_paths_named(query, host);

        let mut modules = Vec::new();

        for path in paths {
            let module =
                self.get_or_create_attached_module_instance(&path, player_actor_id, host)?;

            if !matches!(module, Value::Nil) {
                modules.push(module);
            }
        }

        if all {
            return Ok(Value::array(modules));
        }

        match modules.len() {
            0 => {
                self.log_warning(format!(
                    "player.find(\"{}\"): expected 1 match, found 0; returning nil",
                    query
                ));

                Ok(Value::Nil)
            }

            1 => Ok(modules.remove(0)),

            count => {
                self.log_warning(format!(
                    "player.find(\"{}\"): expected 1 match, found {}; using [0]",
                    query, count
                ));

                Ok(modules.remove(0))
            }
        }
    }

    pub(super) fn log_values(&mut self, values: &[Value]) {
        let message = values
            .iter()
            .map(Value::display_string)
            .collect::<Vec<_>>()
            .join(" ");

        let record = LogRecord {
            severity: LogSeverity::Script,
            script_path: self.current_script_path.clone(),
            entity_name: self.current_entity_name.clone(),
            entity_id: self.current_entity_id,
            context_name: self.current_function_name.clone(),
            message,
            line: None,
            column: None,
        };

        self.output.push(record);
    }

    pub(super) fn eval_index(&self, object: Value, index: Value) -> Result<Value, String> {
        match object {
            Value::Array(values) => {
                let index = index.as_index()?;

                let borrowed = values.borrow();

                borrowed
                    .elements
                    .get(index)
                    .cloned()
                    .ok_or_else(|| format!("array index {} is out of bounds", index))
            }

            Value::Map(map) => {
                let key = index.as_map_key()?;

                let borrowed = map.borrow();

                Ok(borrowed.get(&key).cloned().unwrap_or(Value::Nil))
            }

            other => Err(format!("cannot index {}", other.type_name())),
        }
    }

    pub(super) fn apply_binary(
        &self,
        operator: BinaryOperator,
        left: Value,
        right: Value,
    ) -> Result<Value, String> {
        if matches!(
            operator,
            BinaryOperator::Subtract
                | BinaryOperator::Multiply
                | BinaryOperator::Divide
                | BinaryOperator::Modulo
                | BinaryOperator::Less
                | BinaryOperator::LessEqual
                | BinaryOperator::Greater
                | BinaryOperator::GreaterEqual
        ) && (!matches!(&left, Value::Number(_)) || !matches!(&right, Value::Number(_)))
        {
            let _failure = crate::engine::development_capture::operation(
                "script",
                "numeric_operand_failure",
                false,
            )
            .map(|span| {
                span.diagnostic(serde_json::json!({
                    "operator": format!("{operator:?}"), "left_type": left.type_name(),
                    "right_type": right.type_name(), "script": self.current_script_path,
                    "function": self.current_function_name,
                    "left_number": if let Value::Number(n) = &left { Some(*n) } else { None },
                    "right_number": if let Value::Number(n) = &right { Some(*n) } else { None },
                }))
            });
        }
        match operator {
            BinaryOperator::Add => match (left, right) {
                (Value::Number(a), Value::Number(b)) => Ok(Value::Number(a + b)),

                (Value::String(a), Value::String(b)) => Ok(Value::String(format!("{}{}", a, b))),

                (Value::String(a), other) => {
                    Ok(Value::String(format!("{}{}", a, other.display_string())))
                }

                (other, Value::String(b)) => {
                    Ok(Value::String(format!("{}{}", other.display_string(), b)))
                }

                (left, right) => Err(format!(
                    "cannot add {} and {}",
                    left.type_name(),
                    right.type_name()
                )),
            },

            BinaryOperator::Subtract => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                Ok(Value::Number(left - right))
            }

            BinaryOperator::Multiply => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                Ok(Value::Number(left * right))
            }

            BinaryOperator::Divide => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                if right == 0.0 {
                    return Err("division by zero.".to_string());
                }

                Ok(Value::Number(left / right))
            }

            BinaryOperator::Modulo => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                if right == 0.0 {
                    return Err("modulo by zero.".to_string());
                }

                Ok(Value::Number(left % right))
            }

            BinaryOperator::Equal => Ok(Value::Bool(left == right)),

            BinaryOperator::NotEqual => Ok(Value::Bool(left != right)),

            BinaryOperator::Less => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                Ok(Value::Bool(left < right))
            }

            BinaryOperator::LessEqual => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                Ok(Value::Bool(left <= right))
            }

            BinaryOperator::Greater => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                Ok(Value::Bool(left > right))
            }

            BinaryOperator::GreaterEqual => {
                let left = left.as_number()?;
                let right = right.as_number()?;

                Ok(Value::Bool(left >= right))
            }

            BinaryOperator::And | BinaryOperator::Or => {
                unreachable!("logical operators are handled with short-circuit evaluation")
            }
        }
    }

    pub(super) fn resolve_identifier(
        &self,
        instance: &ScriptInstance,
        scopes: &[Scope],
        name: &str,
        captured_scopes: &[Scope],
    ) -> Result<Value, String> {
        for scope in scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value);
            }
        }

        for scope in captured_scopes.iter().rev() {
            if let Some(value) = scope.get(name) {
                return Ok(value);
            }
        }

        if let Some(value) = instance.get_field(name) {
            return Ok(value.clone());
        }

        for scope in scopes.iter().rev() {
            if let Some(Value::Object(instance_arc)) = scope.get("self") {
                if let Some(value) = instance_arc.borrow().get_field(name) {
                    return Ok(value.clone());
                }
            }
        }

        if name == "math"
            || name == "engine"
            || name == "basket"
            || name == "string"
            || name == "cell"
            || name == "debug"
            || name == "entity"
            || name == "script"
            || name == "event"
            || name == "test"
            || name == "time"
            || name == "task"
            || name == "ui"
            || name == "input"
            || name == "camera"
            || name == "physics"
            || name == "particles"
            || name == "terrain"
            || name == "postprocess"
            || name == "tags"
            || name == "observe"
            || name == "player"
            || name == "save"
            || name == "game"
            || name == "get"
            || name == "scripts"
            || name == "proj"
        {
            return Ok(Value::Namespace(name.to_string()));
        }

        if let Some(function) = self.find_function_for_instance(instance, name) {
            let cache_key = Self::function_cache_key(instance, name);

            let compiled = if let Some(compiled) = self.compiled_functions.get(&cache_key) {
                Arc::clone(compiled)
            } else {
                Arc::new(compile_function(&function))
            };

            let param_names = function.parameters.iter().map(|p| p.name.clone()).collect();

            let captured_env = if instance.entity_name() == "Global" || instance.is_module() {
                if self.current_function_name.as_deref() == Some("top-level") {
                    scopes.to_vec()
                } else {
                    captured_scopes.to_vec()
                }
            } else {
                Vec::new()
            };

            return Ok(Value::Function(compiled, param_names, captured_env));
        }

        let is_global_host_fn = matches!(
            name,
            "find"
                | "get_entity"
                | "spawn"
                | "attach"
                | "detach"
                | "get_light"
                | "getAllCellsOfClass"
                | "enable_script"
                | "disable_script"
                | "is_script_enabled"
                | "fire_event"
                | "complete_test"
                | "is_test_completed"
                | "test_passed"
                | "test_summary"
        );

        if !is_global_host_fn {
            eprintln!(
                "[INTERPRETER][ERROR] unknown variable='{}' script={:?} entity={:?} function={:?} local_scopes={} captured_scopes={}",
                name,
                instance
                    .script_path
                    .as_deref()
                    .or(self.current_script_path.as_deref()),
                self.current_entity_name,
                self.current_function_name,
                scopes.len(),
                captured_scopes.len()
            );
        }

        Err(format!("unknown variable '{}'", name))
    }

    pub fn get_or_create_module_instance(
        &mut self,
        script_path: &str,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        self.get_or_create_module_instance_with_parent(script_path, None, host)
    }

    pub fn get_or_create_attached_module_instance(
        &mut self,
        script_path: &str,
        actor_id: u64,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        self.get_or_create_module_instance_with_parent(script_path, Some(actor_id), host)
    }

    fn get_or_create_module_instance_with_parent(
        &mut self,
        script_path: &str,
        parent_actor_id: Option<u64>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let norm_path = script_path.replace('\\', "/");

        let candidates = [
            norm_path.clone(),
            if norm_path.starts_with("scripts/") {
                norm_path.strip_prefix("scripts/").unwrap().to_string()
            } else {
                format!("scripts/{}", norm_path)
            },
        ];

        let mut found_program: Option<(String, Program)> = None;

        for candidate in &candidates {
            if let Some(program) = self.loaded_scripts.get(candidate) {
                found_program = Some((candidate.clone(), program.clone()));
                break;
            }
        }

        let (canonical_path, program) =
            found_program.ok_or_else(|| format!("Script module not found: '{}'", script_path))?;

        if !host.engine.is_script_enabled(&canonical_path) {
            return Err(format!("Script Module '{}' is disabled", canonical_path));
        }

        let cache_key = match parent_actor_id {
            Some(actor_id) => {
                format!("actor:{}:{}", actor_id, canonical_path)
            }

            None => {
                format!("standalone:{}", canonical_path)
            }
        };

        if self.destroyed_module_keys.contains(&cache_key) {
            return Ok(Value::Nil);
        }

        if let Some(instance) = self.module_instances.get(&cache_key) {
            return Ok(instance.clone());
        }

        let module_name = std::path::Path::new(&canonical_path)
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or("module")
            .to_string();

        let module_instance = ScriptInstance::new_module(
            module_name.clone(),
            canonical_path.clone(),
            parent_actor_id,
        );

        let instance_arc = Arc::new(RefCell::new(module_instance.clone()));
        let module_value = Value::Object(Arc::clone(&instance_arc));
        let module_scope = Scope::new();
        module_scope.declare("self", module_value.clone(), false)?;
        self.module_scopes
            .insert(cache_key.clone(), module_scope.clone());

        if !program.statements.is_empty() {
            let compiled = Arc::new(
                crate::scripting::interpreter::interpreter_compile::compile_top_level(
                    &program.statements,
                ),
            );

            let mut fiber = ScriptFiber {
                script_path: Some(canonical_path.clone()),
                entity_name: module_name,
                instance: module_instance,
                stack: vec![CallFrame {
                    script_path: Some(canonical_path.clone()),
                    function: compiled,
                    pc: 0,
                    scopes: vec![module_scope],
                    for_states: Vec::new(),
                    function_name: "top-level".to_string(),
                    captured_scopes: Vec::new(),
                    expects_return_value: false,
                    call_site_span: None,
                    caller_instance: None,
                }],
                finished: false,
                result: None,
                expression_returns: std::collections::BTreeMap::new(),
                last_execution: None,
            };

            let result = self.resume_fiber(&mut fiber, host);

            if !fiber.instance.is_module()
                && fiber.instance.script_path.as_deref() == Some(canonical_path.as_str())
            {
                *instance_arc.borrow_mut() = fiber.instance.clone();
            }

            match result {
                crate::scripting::execution::FiberResult::Failed(error) => {
                    return Err(error);
                }

                crate::scripting::execution::FiberResult::Complete => {}

                crate::scripting::execution::FiberResult::Paused => {
                    return Err(
                        "debugger pause is unavailable during synchronous Module initialization"
                            .to_string(),
                    );
                }

                result @ (crate::scripting::execution::FiberResult::Continue
                | crate::scripting::execution::FiberResult::Yield(_)) => {
                    self.pending_module_fibers.push((fiber, result));
                }
            }
        }

        if instance_arc.borrow().is_destroyed() {
            self.module_scopes.remove(&cache_key);
            self.destroyed_module_keys.insert(cache_key);
            return Ok(Value::Nil);
        }

        self.module_instances
            .insert(cache_key, module_value.clone());

        Ok(module_value)
    }

    pub fn evict_module_instances_for_script(&mut self, script_path: &str) {
        let normalized = script_path.replace('\\', "/");
        let suffix = format!(":{}", normalized);
        let keys: Vec<String> = self
            .module_instances
            .keys()
            .filter(|key| key.ends_with(&suffix))
            .cloned()
            .collect();
        for key in keys {
            if let Some(Value::Object(obj)) = self.module_instances.remove(&key) {
                obj.borrow_mut().mark_destroyed();
            }
            self.module_scopes.remove(&key);
        }
        self.pending_module_fibers
            .retain(|(fiber, _)| !fiber.involves_script_path(&normalized));
    }

    pub fn is_destroyed_module_fiber(&self, fiber: &ScriptFiber) -> bool {
        self.destroyed_module_keys.iter().any(|key| {
            if let Some(path) = key.strip_prefix("standalone:") {
                return fiber.involves_script_path(path);
            }
            if let Some(rest) = key.strip_prefix("actor:") {
                if let Some((actor, path)) = rest.split_once(':') {
                    if let Ok(actor_id) = actor.parse::<u64>() {
                        return fiber.involves_script_path(path)
                            && (fiber.instance.parent_actor_id() == Some(actor_id)
                                || fiber.stack.iter().any(|frame| {
                                    frame.caller_instance.as_ref().is_some_and(|instance| {
                                        instance.parent_actor_id() == Some(actor_id)
                                    })
                                }));
                    }
                }
            }
            false
        })
    }

    pub fn destroy_module_instance(&mut self, object: &Value) -> bool {
        let Value::Object(target_arc) = object else {
            return false;
        };

        {
            let mut target = target_arc.borrow_mut();

            if !target.is_module() || target.is_destroyed() {
                return false;
            }

            target.mark_destroyed();
        }

        let keys = self
            .module_instances
            .iter()
            .filter_map(|(key, value)| {
                let Value::Object(instance_arc) = value else {
                    return None;
                };

                Arc::ptr_eq(instance_arc, target_arc).then(|| key.clone())
            })
            .collect::<Vec<_>>();

        for key in keys {
            self.module_instances.remove(&key);
            self.module_scopes.remove(&key);
            self.destroyed_module_keys.insert(key);
        }

        true
    }

    pub fn destroy_modules_for_actor(&mut self, actor_id: u64) -> usize {
        let prefix = format!("actor:{}:", actor_id);

        let keys = self
            .module_instances
            .keys()
            .filter(|key| key.starts_with(&prefix))
            .cloned()
            .collect::<Vec<_>>();

        let destroyed = keys.len();

        for key in keys {
            if let Some(Value::Object(instance_arc)) = self.module_instances.remove(&key) {
                instance_arc.borrow_mut().mark_destroyed();
            }

            self.module_scopes.remove(&key);
            self.destroyed_module_keys.insert(key);
        }

        destroyed
    }
}
