pub mod interpreter_compile;
pub mod interpreter_eval;
pub mod interpreter_execution;
pub mod interpreter_tests;

use std::collections::BTreeMap;
use std::sync::Arc;

use crate::scripting::api::HostContext;
use crate::scripting::ast::*;
use crate::scripting::execution::FiberResult;
use crate::scripting::log::{LogRecord, LogSeverity};
use crate::scripting::source::SourceSpan;
use crate::scripting::value::{Scope, Value};

const DEFAULT_OPERATION_BUDGET: u64 = 100_000;
const DEFAULT_MAX_CALL_DEPTH: usize = 64;

pub(crate) const FRAME_PUSHED_SENTINEL: &str = "__AEO_FIBER_FRAME_PUSHED__";

/// A live AeoScript object instance.
///
/// Legacy entity instances and Module instances currently share this storage,
/// while their public semantics remain distinct.
#[derive(Clone, Debug)]
pub struct ScriptInstance {
    pub(super) id: u64,
    pub(super) entity_name: String,
    pub script_path: Option<String>,
    pub(super) fields: BTreeMap<String, Value>,

    /// True when this object represents a `.aeo` Module.
    pub(super) is_module: bool,

    /// Persistent authored Actor ID when this Module is attached to an Actor.
    ///
    /// Runtime Entity IDs are deliberately not stored as Module ownership.
    pub(super) parent_actor_id: Option<u64>,

    /// True after this runtime Module has been destroyed.
    pub(super) destroyed: bool,
}

impl ScriptInstance {
    pub fn id(&self) -> u64 {
        self.id
    }

    pub fn new_empty() -> Self {
        Self {
            id: 0,
            entity_name: "Global".to_string(),
            script_path: None,
            fields: BTreeMap::new(),
            is_module: false,
            parent_actor_id: None,
            destroyed: false,
        }
    }

    pub fn new_entity(entity_name: &str) -> Self {
        Self {
            id: 0,
            entity_name: entity_name.to_string(),
            script_path: None,
            fields: BTreeMap::new(),
            is_module: false,
            parent_actor_id: None,
            destroyed: false,
        }
    }

    pub fn new_module(
        module_name: impl Into<String>,
        script_path: impl Into<String>,
        parent_actor_id: Option<u64>,
    ) -> Self {
        Self {
            id: 0,
            entity_name: module_name.into(),
            script_path: Some(script_path.into()),
            fields: BTreeMap::new(),
            is_module: true,
            parent_actor_id,
            destroyed: false,
        }
    }

    pub fn with_path(mut self, path: String) -> Self {
        self.script_path = Some(path);
        self
    }

    pub fn entity_name(&self) -> &str {
        &self.entity_name
    }

    pub fn is_module(&self) -> bool {
        self.is_module
    }

    pub fn parent_actor_id(&self) -> Option<u64> {
        self.parent_actor_id
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn mark_destroyed(&mut self) {
        self.destroyed = true;
    }

    pub fn get_field(&self, name: &str) -> Option<&Value> {
        self.fields.get(name)
    }

    pub fn set_field(&mut self, name: &str, value: Value) -> Result<(), String> {
        self.fields.insert(name.to_string(), value);
        Ok(())
    }
}

#[derive(Clone, Debug)]
pub(super) enum ExecutionFlow {
    Continue,
    Return(Value),
}

/// One persistent state for a resumable AeoScript execution.
#[derive(Clone, Debug)]
pub(super) struct ForState {
    pub(super) name: String,
    pub(super) values: Vec<Value>,
    pub(super) next_index: usize,
}

#[derive(Clone, Debug)]
pub(super) struct CallFrame {
    /// Source Module that owns this function. This belongs to the frame rather
    /// than the fiber's current instance because nested Module calls swap the
    /// instance while caller frames remain suspended below them.
    pub(super) script_path: Option<String>,
    pub(super) function: Arc<CompiledFunction>,
    pub(super) pc: usize,
    pub(super) scopes: Vec<Scope>,
    pub(super) for_states: Vec<ForState>,
    pub(super) function_name: String,
    pub(super) captured_scopes: Vec<Scope>,
    pub(super) expects_return_value: bool,
    pub(super) call_site_span: Option<SourceSpan>,
    pub(super) caller_instance: Option<ScriptInstance>,
}

/// A cooperative AeoScript execution fiber.
#[derive(Debug)]
pub struct ScriptFiber {
    pub script_path: Option<String>,
    pub entity_name: String,
    pub(super) instance: ScriptInstance,
    pub(super) stack: Vec<CallFrame>,
    pub(super) finished: bool,
    pub(super) result: Option<Value>,
    /// Completed nested-call results retained while the caller re-evaluates
    /// the current expression. The stack depth distinguishes identical source
    /// spans reached by different active frames.
    pub(super) expression_returns: BTreeMap<(usize, SourceSpan), Value>,
    /// Retained when a failing instruction's frame has already been popped.
    pub(super) last_execution: Option<(Option<String>, String, Option<SourceSpan>)>,
}

impl ScriptFiber {
    pub fn last_execution_location(&self) -> Option<(Option<&str>, &str, Option<SourceSpan>)> {
        self.last_execution
            .as_ref()
            .map(|(path, function, span)| (path.as_deref(), function.as_str(), *span))
    }
    pub fn entity_name(&self) -> &str {
        &self.entity_name
    }

    pub fn function_name(&self) -> &str {
        self.stack
            .first()
            .map(|f| f.function_name.as_str())
            .unwrap_or("unknown")
    }

    pub fn program_counter(&self) -> usize {
        self.stack.last().map(|f| f.pc).unwrap_or(0)
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    pub fn instance(&self) -> &ScriptInstance {
        &self.instance
    }

    /// Returns whether this fiber is executing on behalf of the given
    /// persistent script instance, including caller instances suspended below
    /// nested Module/function call frames.
    pub fn involves_instance(&self, instance_id: u64) -> bool {
        self.instance.id() == instance_id
            || self.stack.iter().any(|frame| {
                frame
                    .caller_instance
                    .as_ref()
                    .is_some_and(|caller| caller.id() == instance_id)
            })
    }

    pub fn instance_mut(&mut self) -> &mut ScriptInstance {
        &mut self.instance
    }

    pub fn result(&self) -> Option<&Value> {
        self.result.as_ref()
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:14 -05:00
    pub fn debug_snapshot(
        &self,
        task_id: crate::scripting::execution::ScriptTaskId,
    ) -> crate::scripting::debugger::DebugSnapshot {
        use crate::scripting::debugger::{
            DebugFrameSnapshot, DebugSnapshot, DebugVariable, DebugVariableKind,
            normalize_script_path,
        };

        let variables =
            |scope: &Scope, kind: DebugVariableKind, scope_index: usize| -> Vec<DebugVariable> {
                scope
                    .values
                    .borrow()
                    .iter()
                    .map(|(name, value)| DebugVariable {
                        name: name.clone(),
                        value_type: value.type_name().to_string(),
                        display: value.display_string(),
                        kind: kind.clone(),
                        scope_index,
                    })
                    .collect()
            };

        let mut frames = Vec::new();
        let mut active_instance = self.instance.clone();

        for frame in self.stack.iter().rev() {
            let span = if frame.pc < frame.function.instructions.len() {
                frame.function.instructions[frame.pc]
                    .source_span()
                    .or(frame.call_site_span)
            } else {
                frame.call_site_span
            };
            let script_path = normalize_script_path(
                frame
                    .script_path
                    .as_deref()
                    .or(active_instance.script_path.as_deref())
                    .or(self.script_path.as_deref())
                    .unwrap_or("<unknown>"),
            );

            let locals = frame
                .scopes
                .iter()
                .enumerate()
                .flat_map(|(index, scope)| variables(scope, DebugVariableKind::Local, index))
                .collect();
            let captured = frame
                .captured_scopes
                .iter()
                .enumerate()
                .flat_map(|(index, scope)| variables(scope, DebugVariableKind::Captured, index))
                .collect();
            let instance_fields = active_instance
                .fields
                .iter()
                .map(|(name, value)| DebugVariable {
                    name: name.clone(),
                    value_type: value.type_name().to_string(),
                    display: value.display_string(),
                    kind: DebugVariableKind::InstanceField,
                    scope_index: 0,
                })
                .collect();

            frames.push(DebugFrameSnapshot {
                script_path,
                function_name: frame.function_name.clone(),
                span,
                line: None,
                column: None,
                locals,
                captured,
                instance_fields,
                entity_name: active_instance.entity_name.clone(),
                instance_id: active_instance.id,
                parent_actor_id: active_instance.parent_actor_id,
            });

            if let Some(caller) = frame.caller_instance.as_ref() {
                active_instance = caller.clone();
            }
        }

        DebugSnapshot {
            task_id: task_id.value(),
            selected_frame: 0,
            frames,
        }
    }
}

pub struct Interpreter {
    pub(super) program: Program,
    pub(super) output: Vec<LogRecord>,
    pub(super) operation_budget: u64,
    pub(super) operations_remaining: u64,
    pub(super) max_call_depth: usize,
    pub(super) call_depth: usize,
    pub(super) compiled_functions: BTreeMap<(String, String), Arc<CompiledFunction>>,

    pub(super) current_script_path: Option<String>,
    pub(super) current_entity_name: Option<String>,
    pub(super) current_entity_id: Option<u64>,
    pub(super) current_function_name: Option<String>,

    pub loaded_scripts: std::collections::HashMap<String, Program>,
    pub module_instances: std::collections::HashMap<String, Value>,
    pub(super) destroyed_module_keys: std::collections::HashSet<String>,
    pub(super) pending_module_fibers: Vec<(ScriptFiber, FiberResult)>,

    /// Runtime snapshot of the authoritative project hierarchy.
    pub(super) project_hierarchy: Option<crate::project::ProjectHierarchy>,

    /// Authored Actor ID -> authored Actor name.
    pub(super) project_actor_names: std::collections::HashMap<u64, String>,

    /// Enabled Modules belonging to the current Player Actor.
    ///
    /// Paths remain project-relative, for example:
    /// `playerscripts/Combat/combat.aeo`.
    pub(super) player_module_paths: Vec<String>,
}

impl Clone for Interpreter {
    fn clone(&self) -> Self {
        Self {
            program: self.program.clone(),
            output: self.output.clone(),
            operation_budget: self.operation_budget,
            operations_remaining: self.operations_remaining,
            max_call_depth: self.max_call_depth,
            call_depth: self.call_depth,
            compiled_functions: self.compiled_functions.clone(),
            current_script_path: self.current_script_path.clone(),
            current_entity_name: self.current_entity_name.clone(),
            current_entity_id: self.current_entity_id,
            current_function_name: self.current_function_name.clone(),
            loaded_scripts: self.loaded_scripts.clone(),
            module_instances: self.module_instances.clone(),
            destroyed_module_keys: self.destroyed_module_keys.clone(),
            pending_module_fibers: Vec::new(),
            project_hierarchy: self.project_hierarchy.clone(),
            project_actor_names: self.project_actor_names.clone(),
            player_module_paths: self.player_module_paths.clone(),
        }
    }
}

impl std::fmt::Debug for Interpreter {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Interpreter")
            .field("program", &self.program)
            .field("output", &self.output)
            .field("operation_budget", &self.operation_budget)
            .field("operations_remaining", &self.operations_remaining)
            .field("max_call_depth", &self.max_call_depth)
            .field("call_depth", &self.call_depth)
            .field(
                "compiled_functions",
                &self.compiled_functions.keys().collect::<Vec<_>>(),
            )
            .finish()
    }
}

impl Interpreter {
    pub fn new(program: Program) -> Self {
        Self {
            program,
            output: Vec::new(),
            operation_budget: DEFAULT_OPERATION_BUDGET,
            operations_remaining: DEFAULT_OPERATION_BUDGET,
            max_call_depth: DEFAULT_MAX_CALL_DEPTH,
            call_depth: 0,
            compiled_functions: BTreeMap::new(),
            current_script_path: None,
            current_entity_name: None,
            current_entity_id: None,
            current_function_name: None,
            loaded_scripts: std::collections::HashMap::new(),
            module_instances: std::collections::HashMap::new(),
            destroyed_module_keys: std::collections::HashSet::new(),
            pending_module_fibers: Vec::new(),
            project_hierarchy: None,
            project_actor_names: std::collections::HashMap::new(),
            player_module_paths: Vec::new(),
        }
    }

    pub(crate) fn take_pending_module_fibers(&mut self) -> Vec<(ScriptFiber, FiberResult)> {
        std::mem::take(&mut self.pending_module_fibers)
    }

    pub fn set_project_scope(
        &mut self,
        hierarchy: crate::project::ProjectHierarchy,
        actor_names: std::collections::HashMap<u64, String>,
    ) {
        self.project_hierarchy = Some(hierarchy);
        self.project_actor_names = actor_names;
    }

    pub fn set_player_module_paths(&mut self, mut paths: Vec<String>) {
        for path in &mut paths {
            *path = path.replace('\\', "/");
        }

        paths.retain(|path| path.starts_with("playerscripts/"));

        paths.sort();
        paths.dedup();

        self.player_module_paths = paths;
    }

    pub fn call_function_by_name(
        &mut self,
        instance: &mut ScriptInstance,
        name: &str,
        arguments: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        self.call(instance, name, arguments, host)
    }

    pub fn with_limits(program: Program, operation_budget: u64, max_call_depth: usize) -> Self {
        Self {
            program,
            output: Vec::new(),
            operation_budget,
            operations_remaining: operation_budget,
            max_call_depth,
            call_depth: 0,
            compiled_functions: BTreeMap::new(),
            current_script_path: None,
            current_entity_name: None,
            current_entity_id: None,
            current_function_name: None,
            loaded_scripts: std::collections::HashMap::new(),
            module_instances: std::collections::HashMap::new(),
            destroyed_module_keys: std::collections::HashSet::new(),
            pending_module_fibers: Vec::new(),
            project_hierarchy: None,
            project_actor_names: std::collections::HashMap::new(),
            player_module_paths: Vec::new(),
        }
    }

    pub fn program(&self) -> &Program {
        &self.program
    }

    pub fn output(&self) -> &[LogRecord] {
        &self.output
    }

    pub fn drain_output(&mut self) -> Vec<LogRecord> {
        std::mem::take(&mut self.output)
    }

    pub fn log_error(&mut self, message: String) {
        let record = LogRecord {
            severity: LogSeverity::Error,
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

    pub fn log_warning(&mut self, message: String) {
        let record = LogRecord {
            severity: LogSeverity::Warning,
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

    pub fn log_system(&mut self, message: String) {
        let record = LogRecord {
            severity: LogSeverity::System,
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

    pub fn instantiate_entity(
        &mut self,
        entity_name: &str,
        id: u64,
        host: &mut HostContext,
    ) -> Result<ScriptInstance, String> {
        self.reset_execution_budget();

        let entity = self
            .find_entity(entity_name)
            .ok_or_else(|| format!("entity '{}' does not exist", entity_name))?;

        let fields = entity
            .members
            .iter()
            .filter_map(|member| match member {
                EntityMember::Field(field) => Some(field.clone()),
                EntityMember::Function(_) => None,
            })
            .collect::<Vec<_>>();

        let mut instance = ScriptInstance {
            id,
            entity_name: entity_name.to_string(),
            script_path: None,
            fields: BTreeMap::new(),
            is_module: false,
            parent_actor_id: None,
            destroyed: false,
        };

        let mut scopes = Vec::new();

        for field in fields {
            self.tick()?;

            let value = if let Some(initializer) = &field.initializer {
                self.eval_expression(&mut instance, &mut scopes, initializer, host, &[])?
            } else {
                Value::Nil
            };

            instance.fields.insert(field.name, value);
        }

        Ok(instance)
    }

    pub fn call(
        &mut self,
        instance: &mut ScriptInstance,
        function_name: &str,
        arguments: Vec<Value>,
        host: &mut HostContext,
    ) -> Result<Value, String> {
        let function = self
            .find_function_for_instance(instance, function_name)
            .ok_or_else(|| {
                format!(
                    "function '{}' does not exist on entity '{}'",
                    function_name, instance.entity_name
                )
            })?;

        let old_budget = self.operations_remaining;
        let old_depth = self.call_depth;
        let old_path = self.current_script_path.clone();
        let old_entity = self.current_entity_name.clone();
        let old_id = self.current_entity_id;
        let old_function = self.current_function_name.clone();

        if old_function.is_none() {
            self.operations_remaining = self.operation_budget;
            self.call_depth = 0;
        }

        self.current_script_path = instance.script_path.clone();
        self.current_entity_name = Some(instance.entity_name().to_string());
        self.current_entity_id = Some(instance.id());
        self.current_function_name = Some(function_name.to_string());

        let result = self.call_user_function(instance, &function, arguments, host, &[]);

        self.operations_remaining = old_budget;
        self.call_depth = old_depth;
        self.current_script_path = old_path;
        self.current_entity_name = old_entity;
        self.current_entity_id = old_id;
        self.current_function_name = old_function;

        result
    }

    pub(super) fn call_direct(
        &mut self,
        instance: &mut ScriptInstance,
        compiled: Arc<CompiledFunction>,
        param_names: Vec<String>,
        arguments: Vec<Value>,
        self_value: Option<Value>,
        function_name: String,
        host: &mut HostContext,
        captured_scopes: Vec<Scope>,
    ) -> Result<Value, String> {
        if self.call_depth >= self.max_call_depth {
            return Err(format!(
                "AeoScript call depth exceeded the limit of {}.",
                self.max_call_depth
            ));
        }

        if param_names.len() != arguments.len() {
            return Err(format!(
                "function expected {} argument(s), got {}",
                param_names.len(),
                arguments.len()
            ));
        }

        let parameter_scope = Scope::new();

        if let Some(self_value) = self_value {
            parameter_scope.declare("self", self_value, false)?;
        }

        for (name, argument) in param_names.into_iter().zip(arguments.into_iter()) {
            parameter_scope.declare(name, argument, false)?;
        }

        let frame_script_path = instance.script_path.clone();
        let mut fiber = ScriptFiber {
            script_path: instance.script_path.clone(),
            entity_name: instance.entity_name().to_string(),
            instance: instance.clone(),
            stack: vec![CallFrame {
                script_path: frame_script_path,
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
            expression_returns: BTreeMap::new(),
            last_execution: None,
        };

        self.call_depth += 1;

        let result = self.resume_fiber(&mut fiber, host);

        self.call_depth -= 1;

        *instance = fiber.instance;

        match result {
            FiberResult::Complete => Ok(fiber.result.unwrap_or(Value::Nil)),
            FiberResult::Yield(_) => {
                Err("yielding is not supported in this synchronous call context".to_string())
            }
            FiberResult::Failed(message) => Err(message),
            _ => Ok(Value::Nil),
        }
    }

    pub(super) fn call_user_function(
        &mut self,
        instance: &mut ScriptInstance,
        function: &FunctionDecl,
        arguments: Vec<Value>,
        host: &mut HostContext,
        captured_scopes: &[Scope],
    ) -> Result<Value, String> {
        if self.call_depth >= self.max_call_depth {
            return Err(format!(
                "AeoScript call depth exceeded the limit of {}.",
                self.max_call_depth
            ));
        }

        if function.parameters.len() != arguments.len() {
            return Err(format!(
                "function '{}' expected {} argument(s), got {}",
                function.name,
                function.parameters.len(),
                arguments.len()
            ));
        }

        self.call_depth += 1;

        let result = (|| {
            let parameter_scope = Scope::new();

            for (parameter, argument) in function.parameters.iter().zip(arguments.into_iter()) {
                parameter_scope.declare(parameter.name.clone(), argument, false)?;
            }

            let mut scopes = vec![parameter_scope];

            match self.execute_block(
                instance,
                &mut scopes,
                &function.body,
                host,
                captured_scopes,
            )? {
                ExecutionFlow::Continue => Ok(Value::Nil),
                ExecutionFlow::Return(value) => Ok(value),
            }
        })();

        self.call_depth -= 1;

        result
    }

    pub(super) fn find_entity(&self, name: &str) -> Option<EntityDecl> {
        self.program
            .declarations
            .iter()
            .find_map(|declaration| match declaration {
                Declaration::Entity(entity) if entity.name == name => Some(entity.clone()),

                _ => None,
            })
    }

    pub(super) fn find_function(
        &self,
        entity_name: &str,
        function_name: &str,
    ) -> Option<FunctionDecl> {
        if let Some(entity) = self.find_entity(entity_name) {
            if let Some(function) = entity.members.into_iter().find_map(|member| match member {
                EntityMember::Function(function) if function.name == function_name => {
                    Some(function)
                }

                _ => None,
            }) {
                return Some(function);
            }
        }

        if let Some(function) =
            self.program
                .declarations
                .iter()
                .find_map(|declaration| match declaration {
                    Declaration::Function(function) if function.name == function_name => {
                        Some(function.clone())
                    }

                    _ => None,
                })
        {
            return Some(function);
        }

        for (path, program) in &self.loaded_scripts {
            let stem = std::path::Path::new(path)
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("");
            if stem.eq_ignore_ascii_case(entity_name) || path.eq_ignore_ascii_case(entity_name) {
                for decl in &program.declarations {
                    if let Declaration::Entity(entity) = decl {
                        if entity.name.eq_ignore_ascii_case(entity_name) {
                            for member in &entity.members {
                                if let EntityMember::Function(f) = member {
                                    if f.name == function_name {
                                        return Some(f.clone());
                                    }
                                }
                            }
                        }
                    }
                }

                for decl in &program.declarations {
                    if let Declaration::Function(f) = decl {
                        if f.name == function_name {
                            return Some(f.clone());
                        }
                    }
                }
            }
        }

        for program in self.loaded_scripts.values() {
            for decl in &program.declarations {
                if let Declaration::Entity(entity) = decl {
                    if entity.name.eq_ignore_ascii_case(entity_name) {
                        for member in &entity.members {
                            if let EntityMember::Function(f) = member {
                                if f.name == function_name {
                                    return Some(f.clone());
                                }
                            }
                        }
                    }
                }
            }
        }

        None
    }

    /// Resolve within the script that owns this instance before using legacy
    /// entity/global discovery. Different Modules routinely share method names;
    /// their project-relative script path is the stable code identity.
    pub(super) fn find_function_for_instance(
        &self,
        instance: &ScriptInstance,
        function_name: &str,
    ) -> Option<FunctionDecl> {
        if let Some(script_path) = instance.script_path.as_deref() {
            let normalized = script_path.replace('\\', "/");
            let alternate = if normalized.starts_with("scripts/") {
                normalized
                    .strip_prefix("scripts/")
                    .unwrap_or(&normalized)
                    .to_string()
            } else {
                format!("scripts/{normalized}")
            };

            for candidate in [&normalized, &alternate] {
                let Some(program) = self.loaded_scripts.get(candidate) else {
                    continue;
                };

                if let Some(function) =
                    program
                        .declarations
                        .iter()
                        .find_map(|declaration| match declaration {
                            Declaration::Function(function) if function.name == function_name => {
                                Some(function.clone())
                            }
                            Declaration::Entity(entity)
                                if entity.name.eq_ignore_ascii_case(instance.entity_name()) =>
                            {
                                entity.members.iter().find_map(|member| match member {
                                    EntityMember::Function(function)
                                        if function.name == function_name =>
                                    {
                                        Some(function.clone())
                                    }
                                    _ => None,
                                })
                            }
                            _ => None,
                        })
                {
                    return Some(function);
                }
            }
        }

        self.find_function(instance.entity_name(), function_name)
    }

    pub(super) fn function_cache_key(
        instance: &ScriptInstance,
        function_name: &str,
    ) -> (String, String) {
        let owner = instance
            .script_path
            .as_deref()
            .map(|path| format!("{}#{}", path.replace('\\', "/"), instance.entity_name()))
            .unwrap_or_else(|| instance.entity_name().to_string());
        (owner, function_name.to_string())
    }
}
