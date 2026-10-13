use std::collections::HashMap;

use super::api::HostContext;
use super::ast::{Declaration, EventDecl, Program, Statement};
use super::execution::{FiberResult, ScriptScheduler, ScriptTaskId};
use super::interpreter::{Interpreter, ScriptFiber, ScriptInstance};
use super::log::LogRecord;
use super::value::{Scope, Value};

/// Runtime owner for all live AeoScript execution.
///
/// The runtime connects the interpreter's persistent fibers to the cooperative
/// scheduler. It remains single-threaded and never blocks the engine thread.
///
/// IMPORTANT:
/// A top-level script gets its own persistent `Scope`.
///
/// That variable lives in the top-level scope belonging to that script.
/// Event handlers from the same script are given a clone of that Scope.
///
/// `Scope` internally uses shared backing storage, so cloning the Scope does
/// NOT copy the variables. Both sides refer to the same variable storage.
#[derive(Debug)]
pub struct ScriptRuntime {
    interpreter: Interpreter,
    scheduler: ScriptScheduler,
    fibers: HashMap<ScriptTaskId, ScriptFiber>,

    /// Every event handler is associated with the script that declared it.
    ///
    /// `None` is used by the older single-program runtime path.
    event_handlers: Vec<(Option<String>, EventDecl)>,

    /// Persistent lexical scope for each top-level script.
    ///
    /// Example:
    ///     "scripts/game_hud.aeo" -> Scope containing:
    ///         hud_width
    ///         hud_height
    ///         hud_margin
    ///         health
    ///         gold
    ///
    /// Event handlers use the same shared Scope so they can read/write the
    /// script's top-level variables.
    top_level_scopes: HashMap<String, Scope>,
    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:07 -05:00
    debugger: crate::scripting::debugger::ScriptDebugger,
}

impl ScriptRuntime {
    pub fn new(program: Program) -> Self {
        // This constructor is the older single-program runtime path.
        //
        // Since Program does not carry a script path here, event handlers are
        // registered with `None`. The binding-based scene loader uses
        // `add_event_handlers()` instead so events retain their source path.
        let event_handlers = program
            .declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Event(event) = decl {
                    Some((None, event.clone()))
                } else {
                    None
                }
            })
            .collect();

        Self {
            interpreter: Interpreter::new(program),
            scheduler: ScriptScheduler::new(),
            fibers: HashMap::new(),
            event_handlers,
            top_level_scopes: HashMap::new(),
            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:33 -05:00
            debugger: crate::scripting::debugger::ScriptDebugger::default(),
        }
    }

    /// Registers all event handlers declared by a specific script file.
    ///
    /// Keeping the script path here is critical because later, when the event
    /// fires, we use that same path to find the script's persistent top-level
    /// Scope.
    pub fn add_event_handlers(&mut self, script_path: String, program: &Program) {
        for decl in &program.declarations {
            if let Declaration::Event(event) = decl {
                self.event_handlers
                    .push((Some(script_path.clone()), event.clone()));
            }
        }
    }

    pub fn clear_event_handlers(&mut self) {
        self.event_handlers.clear();
    }

    pub fn with_interpreter(interpreter: Interpreter) -> Self {
        let event_handlers = interpreter
            .program()
            .declarations
            .iter()
            .filter_map(|decl| {
                if let Declaration::Event(event) = decl {
                    Some((None, event.clone()))
                } else {
                    None
                }
            })
            .collect();

        Self {
            interpreter,
            scheduler: ScriptScheduler::new(),
            fibers: HashMap::new(),
            event_handlers,
            top_level_scopes: HashMap::new(),
            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:33 -05:00
            debugger: crate::scripting::debugger::ScriptDebugger::default(),
        }
    }

    /// Dispatches an event to all matching handlers.
    pub fn dispatch_event(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
        _host: &mut HostContext,
        disabled_scripts: &[String],
    ) -> Result<(), String> {
        let matching: Vec<(Option<String>, EventDecl)> = self
            .event_handlers
            .iter()
            .filter(|(path, h)| {
                if let Some(p) = path {
                    if disabled_scripts.contains(p) {
                        return false;
                    }
                }

                h.name == name
            })
            .cloned()
            .collect();

        for (path, handler) in matching {
            self.start_event_fiber(path, handler, arguments.clone())?;
        }

        Ok(())
    }

    pub fn cancel_fibers_for_script(&mut self, script_path: &str) {
        let norm = script_path.replace("\\", "/");

        let to_cancel: Vec<ScriptTaskId> = self
            .fibers
            .iter()
            .filter_map(|(id, fiber)| {
                if let Some(ref path) = fiber.script_path {
                    if path.replace("\\", "/") == norm {
                        return Some(*id);
                    }
                }

                None
            })
            .collect();

        for task_id in to_cancel {
            let _ = self.scheduler.cancel(task_id);
            let _ = self.remove_fiber(task_id);
        }
    }

    /// Cancels every scheduled fiber whose persistent ScriptInstance belongs
    /// to one runtime Entity. Contact callbacks and other detached lifecycle
    /// work may outlive the Entity's tracked on_spawn/update task otherwise.
    pub fn cancel_fibers_for_instance(&mut self, instance_id: u64) -> Result<(), String> {
        if instance_id == 0 {
            return Ok(());
        }

        let task_ids = self
            .fibers
            .iter()
            .filter_map(|(task_id, fiber)| fiber.involves_instance(instance_id).then_some(*task_id))
            .collect::<Vec<_>>();

        for task_id in task_ids {
            let _ = self.cancel(task_id);
            self.remove_fiber(task_id)?;
        }

        Ok(())
    }

    /// Creates an event fiber.
    ///
    /// IMPORTANT DIAGNOSTIC POINT:
    ///
    /// The event's script path is used to locate the persistent top-level
    /// Scope. We intentionally perform that lookup BEFORE moving `script_path`
    /// into the ScriptInstance.
    ///
    /// We also check whether the Scope already contains `health`.
    ///
    /// Checks captured_scope and health variables in the top-level scope.
    fn start_event_fiber(
        &mut self,
        script_path: Option<String>,
        handler: EventDecl,
        arguments: Vec<Value>,
    ) -> Result<ScriptTaskId, String> {
        // -------------------------------------------------------------
        // STEP 1:
        // Find the persistent top-level Scope belonging to this script.
        // -------------------------------------------------------------
        let captured_scope = script_path
            .as_ref()
            .and_then(|path| self.top_level_scopes.get(path));

        // Clone the Scope handle.
        //
        // This does NOT copy the actual variable storage. Scope uses shared
        // Arc<RefCell<...>> backing, so the event handler and top-level script
        // share the same variables.
        let captured_scopes: Vec<Scope> = captured_scope.cloned().into_iter().collect();

        // -------------------------------------------------------------
        // STEP 2:
        // Create the event's ScriptInstance.
        //
        // Do this AFTER looking up the Scope because assigning
        // `script_path` into the instance moves the Option<String>.
        // -------------------------------------------------------------
        let mut instance = ScriptInstance::new_empty();

        instance.script_path = script_path.clone();

        // -------------------------------------------------------------
        // STEP 3:
        // Start the actual interpreter fiber.
        //
        // The captured_scopes argument is what allows the event handler to
        // resolve top-level variables such as `health`.
        // -------------------------------------------------------------
        let fiber = self.interpreter.start_event_fiber(
            instance,
            handler.clone(),
            arguments,
            captured_scopes,
        )?;

        // -------------------------------------------------------------
        // STEP 4:
        // Register the fiber with the scheduler.
        // -------------------------------------------------------------
        let task_id = self.scheduler.spawn();

        self.fibers.insert(task_id, fiber);

        Ok(task_id)
    }

    /// Spawns the older single-program top-level fiber.
    ///
    /// NOTE:
    /// `spawn_top_level_custom()` is the binding-aware path used by the
    /// current ScriptScene loader. That path creates and stores the
    /// persistent top-level Scope needed by events.
    pub fn spawn_top_level(&mut self, script_path: String) -> Result<Option<ScriptTaskId>, String> {
        let statements = self.interpreter.program().statements.clone();

        if statements.is_empty() {
            return Ok(None);
        }

        let mut instance = ScriptInstance::new_empty();
        instance.script_path = Some(script_path);

        let fiber = self
            .interpreter
            .start_top_level_fiber(instance, &statements)?;

        let task_id = self.scheduler.spawn();

        self.fibers.insert(task_id, fiber);

        Ok(Some(task_id))
    }

    /// Spawns a top-level fiber for one specific script file.
    ///
    /// This is the important path for the current multi-script scene system.
    ///
    /// Each script gets its own Scope stored in `top_level_scopes` and a top-level fiber using that exact Scope.
    ///
    /// Later, an event from the same script can retrieve the Scope by path.
    pub fn spawn_top_level_custom(
        &mut self,
        script_path: String,
        statements: &[Statement],
    ) -> Result<Option<ScriptTaskId>, String> {
        if statements.is_empty() {
            return Ok(None);
        }

        // Create the persistent scope that will hold the script's globals.
        let top_level_scope = Scope::new();

        // Store the Scope BEFORE creating the fiber so the runtime can retrieve
        // this exact shared environment later when an event fires.
        self.top_level_scopes
            .insert(script_path.clone(), top_level_scope.clone());

        // Create the ScriptInstance for the top-level script.
        let mut instance = ScriptInstance::new_empty();
        instance.script_path = Some(script_path.clone());

        // Start the top-level fiber using the persistent Scope.
        let fiber = self.interpreter.start_top_level_fiber_with_scope(
            instance,
            statements,
            top_level_scope,
        )?;

        // Schedule the top-level fiber.
        let task_id = self.scheduler.spawn();

        self.fibers.insert(task_id, fiber);

        Ok(Some(task_id))
    }

    pub fn interpreter(&self) -> &Interpreter {
        &self.interpreter
    }

    pub fn interpreter_mut(&mut self) -> &mut Interpreter {
        &mut self.interpreter
    }

    pub fn scheduler(&self) -> &ScriptScheduler {
        &self.scheduler
    }

    pub fn scheduler_mut(&mut self) -> &mut ScriptScheduler {
        &mut self.scheduler
    }

    pub fn task_count(&self) -> usize {
        self.fibers.len()
    }

    pub fn fiber_count(&self) -> usize {
        self.fibers.len()
    }

    pub fn fiber(&self, id: ScriptTaskId) -> Option<&ScriptFiber> {
        self.fibers.get(&id)
    }

    pub fn fibers(&self) -> &HashMap<ScriptTaskId, ScriptFiber> {
        &self.fibers
    }

    pub fn fiber_mut(&mut self, id: ScriptTaskId) -> Option<&mut ScriptFiber> {
        self.fibers.get_mut(&id)
    }

    pub fn fibers_mut(&mut self) -> &mut HashMap<ScriptTaskId, ScriptFiber> {
        &mut self.fibers
    }

    pub fn top_level_scope(&self) -> Scope {
        self.top_level_scopes
            .values()
            .next()
            .cloned()
            .unwrap_or_default()
    }

    /// Starts a new persistent script fiber and schedules it immediately.
    pub fn spawn(
        &mut self,
        instance: ScriptInstance,
        function_name: &str,
        arguments: Vec<Value>,
    ) -> Result<ScriptTaskId, String> {
        let fiber = self
            .interpreter
            .start_fiber(instance, function_name, arguments)?;

        let task_id = self.scheduler.spawn();

        self.fibers.insert(task_id, fiber);

        Ok(task_id)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-15 | 2026-10-06 00:18 -05:00
    pub fn spawn_named_script_handler(
        &mut self,
        script_path: &str,
        function: &super::ast::FunctionDecl,
    ) -> Result<ScriptTaskId, String> {
        let normalized = script_path.replace('\\', "/");
        let captured_scope = self
            .top_level_scopes
            .get(&normalized)
            .cloned()
            .unwrap_or_default();

        let fiber = self.interpreter.start_named_script_function_fiber(
            normalized,
            function,
            captured_scope,
        )?;
        let task_id = self.scheduler.spawn();
        self.fibers.insert(task_id, fiber);
        Ok(task_id)
    }

    /// Spawns a callable closure/function value.
    ///
    /// The captured scopes are supplied directly by the caller.
    pub fn spawn_callable(
        &mut self,
        compiled: std::sync::Arc<super::ast::CompiledFunction>,
        param_names: Vec<String>,
        arguments: Vec<Value>,
        function_name: String,
        captured_scopes: Vec<super::value::Scope>,
    ) -> Result<ScriptTaskId, String> {
        let instance = ScriptInstance::new_empty();

        let fiber = self.interpreter.start_direct_fiber(
            instance,
            compiled,
            param_names,
            arguments,
            None,
            function_name.clone(),
            captured_scopes,
        )?;

        let task_id = self.scheduler.spawn();

        self.fibers.insert(task_id, fiber);

        Ok(task_id)
    }

    pub fn adopt_pending_module_fibers(&mut self) -> Result<Vec<ScriptTaskId>, String> {
        let pending = self.interpreter.take_pending_module_fibers();
        let mut task_ids = Vec::with_capacity(pending.len());

        for (fiber, result) in pending {
            let task_id = match result {
                FiberResult::Continue => self.scheduler.spawn(),

                FiberResult::Yield(reason) => self.scheduler.spawn_waiting(reason)?,

                // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:33 -05:00
                FiberResult::Paused | FiberResult::Complete | FiberResult::Failed(_) => {
                    return Err(
                        "AeoScript attempted to schedule a finished Module fiber.".to_string()
                    );
                }
            };

            self.fibers.insert(task_id, fiber);
            task_ids.push(task_id);
        }

        Ok(task_ids)
    }

    /// Advances runtime time and executes the fibers that were ready for this
    /// tick.
    ///
    /// Each task receives at most one execution slice. This prevents a task
    /// returning `Continue` from consuming the whole tick by immediately
    /// re-entering itself.
    pub fn tick(
        &mut self,
        new_time: f64,
        host: &mut HostContext,
    ) -> Result<Vec<(ScriptTaskId, FiberResult)>, String> {
        self.scheduler.tick(new_time)?;

        self.adopt_pending_module_fibers()?;

        let ready_count = self.scheduler.ready_count();

        let mut results = Vec::with_capacity(ready_count);

        for _ in 0..ready_count {
            let task_id = match self.scheduler.pop_ready() {
                Some(task_id) => task_id,
                None => break,
            };

            self.scheduler.begin_running(task_id)?;

            // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:07 -05:00
            let mut paused_point = None;
            let result = {
                let fiber = self.fibers.get_mut(&task_id).ok_or_else(|| {
                    format!(
                        "AeoScript runtime has no fiber for task {}.",
                        task_id.value()
                    )
                })?;
                let debugger = &mut self.debugger;
                self.interpreter
                    .resume_fiber_with_debug(fiber, host, |point| {
                        let pause = debugger.should_pause(task_id, point);
                        if pause {
                            paused_point = Some(point.clone());
                        }
                        pause
                    })
            };

            self.scheduler.apply_result(task_id, result.clone())?;
            if result == FiberResult::Paused {
                if let Some(point) = paused_point {
                    self.debugger.record_pause(task_id, point);
                }
            }

            self.adopt_pending_module_fibers()?;

            results.push((task_id, result));
        }

        Ok(results)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:07 -05:00
    pub fn set_debug_breakpoints(
        &mut self,
        breakpoints: Vec<crate::scripting::debugger::DebugBreakpoint>,
    ) {
        self.debugger.set_breakpoints(breakpoints);
    }

    pub fn debugger(&self) -> &crate::scripting::debugger::ScriptDebugger {
        &self.debugger
    }

    // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:21 -05:00
    pub fn debug_snapshot(&self) -> Option<crate::scripting::debugger::DebugSnapshot> {
        let task_id = self.debugger.paused_task()?;
        self.fibers
            .get(&task_id)
            .map(|fiber| fiber.debug_snapshot(task_id))
    }

    pub fn debug_command(
        &mut self,
        command: crate::scripting::debugger::DebugCommand,
    ) -> Result<(), String> {
        let task_id = self.debugger.prepare_command(command)?;
        self.scheduler.resume_paused(task_id)
    }

    pub fn clear_debugger_runtime_state(&mut self) {
        self.debugger.clear_runtime_state();
    }

    pub fn cancel(&mut self, task_id: ScriptTaskId) -> Result<(), String> {
        self.debugger.clear_task(task_id);
        self.scheduler.cancel(task_id)
    }

    /// Removes a completed or failed fiber from runtime storage and purges its scheduler task.
    pub fn remove_fiber(&mut self, task_id: ScriptTaskId) -> Result<Option<ScriptFiber>, String> {
        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:33 -05:00
        self.debugger.clear_task(task_id);
        let fiber = self.fibers.remove(&task_id);
        if fiber.is_some() {
            self.scheduler.remove(task_id)?;
        }
        Ok(fiber)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::engine::entity::EntityManager;
    use crate::scripting::execution::{ScriptTaskState, YieldReason};
    use crate::scripting::lexer::Lexer;
    use crate::scripting::parser::Parser;
    use crate::scripting::value::{HandleKind, Value};

    fn runtime(source: &str) -> ScriptRuntime {
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let program = Parser::new(tokens).parse().expect("parser should succeed");

        ScriptRuntime::new(program)
    }

    fn test_host() -> EntityManager {
        EntityManager::new()
    }

    #[test]
    fn runtime_spawns_ready_fiber() {
        let source = r#"
entity Test {
    value: number = 0

    fn update(dt: number) {
        value = 1
    }
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let task_id = runtime
            .spawn(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should spawn");

        assert_eq!(runtime.task_count(), 1);

        assert_eq!(
            runtime.scheduler().state(task_id),
            Some(ScriptTaskState::Ready)
        );
    }

    #[test]
    fn runtime_connects_wait_to_scheduler() {
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

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let task_id = runtime
            .spawn(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should spawn");

        let results = runtime
            .tick(0.0, &mut host)
            .expect("first tick should succeed");

        assert_eq!(
            results,
            vec![(task_id, FiberResult::Yield(YieldReason::WaitSeconds(1.0)))]
        );

        assert_eq!(
            runtime.scheduler().state(task_id),
            Some(ScriptTaskState::Waiting { wake_at: 1.0 })
        );

        assert_eq!(
            runtime
                .fiber(task_id)
                .expect("fiber should exist")
                .instance()
                .get_field("value"),
            Some(&Value::Number(1.0))
        );

        let results = runtime
            .tick(0.5, &mut host)
            .expect("second tick should succeed");

        assert!(results.is_empty());

        assert_eq!(
            runtime
                .fiber(task_id)
                .expect("fiber should exist")
                .instance()
                .get_field("value"),
            Some(&Value::Number(1.0))
        );

        let results = runtime
            .tick(1.0, &mut host)
            .expect("wake tick should succeed");

        assert_eq!(results, vec![(task_id, FiberResult::Complete)]);

        assert_eq!(
            runtime.scheduler().state(task_id),
            Some(ScriptTaskState::Completed)
        );

        assert_eq!(
            runtime
                .fiber(task_id)
                .expect("fiber should exist")
                .instance()
                .get_field("value"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn runtime_does_not_immediately_rerun_continuing_task() {
        let source = r#"
entity Test {
    value: number = 0

    fn update(dt: number) {
        value += 1
    }
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let task_id = runtime
            .spawn(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should spawn");

        let results = runtime.tick(0.0, &mut host).expect("tick should succeed");

        assert_eq!(results, vec![(task_id, FiberResult::Complete)]);
    }

    #[test]
    fn runtime_supports_multiple_waiting_scripts() {
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

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let first_instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("first entity should instantiate");

        let second_instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("second entity should instantiate");

        let first = runtime
            .spawn(first_instance, "update", vec![Value::Number(1.0)])
            .expect("first fiber should spawn");

        let second = runtime
            .spawn(second_instance, "update", vec![Value::Number(1.0)])
            .expect("second fiber should spawn");

        let results = runtime.tick(0.0, &mut host).expect("tick should succeed");

        assert_eq!(results.len(), 2);

        assert_eq!(
            runtime.scheduler().state(first),
            Some(ScriptTaskState::Waiting { wake_at: 1.0 })
        );

        assert_eq!(
            runtime.scheduler().state(second),
            Some(ScriptTaskState::Waiting { wake_at: 1.0 })
        );

        let results = runtime
            .tick(1.0, &mut host)
            .expect("wake tick should succeed");

        assert_eq!(results.len(), 2);

        assert!(
            results
                .iter()
                .all(|(_, result)| *result == FiberResult::Complete)
        );
    }

    #[test]
    fn runtime_rejects_zero_wait() {
        let source = r#"
entity Test {
    fn update(dt: number) {
        wait(0)
    }
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let task_id = runtime
            .spawn(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should spawn");

        let results = runtime
            .tick(0.0, &mut host)
            .expect("runtime tick should itself succeed");

        assert_eq!(results.len(), 1);

        assert!(matches!(
            results[0],
            (
                id,
                FiberResult::Failed(ref message)
            ) if id == task_id
                && message.contains("greater than zero")
        ));

        assert_eq!(
            runtime.scheduler().state(task_id),
            Some(ScriptTaskState::Failed)
        );
    }

    #[test]
    fn runtime_accepts_small_positive_wait() {
        let source = r#"
entity Test {
    value: number = 0

    fn update(dt: number) {
        value = 1
        wait(0.0001)
        value = 2
    }
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .expect("entity should instantiate");

        let task_id = runtime
            .spawn(instance, "update", vec![Value::Number(1.0)])
            .expect("fiber should spawn");

        let results = runtime
            .tick(0.0, &mut host)
            .expect("runtime tick should succeed");

        assert_eq!(
            results,
            vec![(
                task_id,
                FiberResult::Yield(YieldReason::WaitSeconds(0.0001))
            )]
        );

        assert_eq!(
            runtime.scheduler().state(task_id),
            Some(ScriptTaskState::Waiting { wake_at: 0.0001 })
        );

        let results = runtime
            .tick(0.0001, &mut host)
            .expect("wake tick should succeed");

        assert_eq!(results, vec![(task_id, FiberResult::Complete)]);

        assert_eq!(
            runtime
                .fiber(task_id)
                .expect("fiber should exist")
                .instance()
                .get_field("value"),
            Some(&Value::Number(2.0))
        );
    }

    #[test]
    fn runtime_execution_smoke_test() {
        let source = r#"
entity Test {
    value: number = 0

    fn main() {
        value = 100
    }
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let task_id = runtime.spawn(instance, "main", vec![]).unwrap();

        let results = runtime.tick(0.0, &mut host).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].0, task_id);
        assert_eq!(results[0].1, FiberResult::Complete);

        assert_eq!(
            runtime
                .fiber(task_id)
                .unwrap()
                .instance()
                .get_field("value"),
            Some(&Value::Number(100.0))
        );
    }

    #[test]
    fn runtime_failure_is_observable() {
        let source = r#"
entity Test {
    fn fail() {
        const x = 1 / 0
    }
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, &mut host)
            .unwrap();

        let task_id = runtime.spawn(instance, "fail", vec![]).unwrap();

        let results = runtime.tick(0.0, &mut host).unwrap();

        assert_eq!(results.len(), 1);

        match &results[0].1 {
            FiberResult::Failed(msg) => {
                assert!(msg.contains("division by zero"));
            }
            _ => panic!("Expected FiberResult::Failed"),
        }

        assert_eq!(
            runtime.scheduler().state(task_id),
            Some(ScriptTaskState::Failed)
        );
    }

    #[test]
    fn runtime_dispatches_event() {
        let source = r#"
on PlayerSpawned(player) {
    debug.log("Spawned:", player.name)
}
"#;

        let mut runtime = runtime(source);
        let mut em = test_host();

        em.create_entity("Player");

        let mut host = HostContext {
            delta_time: 1.0,
            engine: &mut em,
        };

        let args = vec![Value::Handle {
            kind: HandleKind::Entity,
            id: 1,
        }];

        runtime
            .dispatch_event("PlayerSpawned", args, &mut host, &[])
            .unwrap();

        assert_eq!(runtime.task_count(), 1);

        runtime.tick(0.0, &mut host).unwrap();

        assert!(
            runtime
                .interpreter()
                .output()
                .iter()
                .any(|r| r.message.contains("Spawned: Player"))
        );
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:26 -05:00
#[cfg(test)]
mod debugger_runtime_tests {
    use super::*;
    use crate::engine::entity::EntityManager;
    use crate::scripting::api::HostContext;
    use crate::scripting::debugger::{DebugCommand, resolve_breakpoint};
    use crate::scripting::execution::{FiberResult, ScriptTaskState};
    use crate::scripting::lexer::Lexer;
    use crate::scripting::parser::Parser;
    use crate::scripting::value::Value;

    fn runtime(source: &str) -> ScriptRuntime {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        ScriptRuntime::new(program)
    }

    // Worldkiln 0.8.7 | WO 0.8.7-23 | 2026-10-06 17:25 -05:00
    fn line(source: &str, text: &str) -> usize {
        // "helper()" also appears in "fn helper()"; these fixtures intend the
        // executable call in main(), which is the last matching line.
        source
            .lines()
            .enumerate()
            .filter(|(_, candidate)| candidate.contains(text))
            .map(|(index, _)| index)
            .last()
            .unwrap()
            + 1
    }

    fn spawn_main(
        runtime: &mut ScriptRuntime,
        source: &str,
        path: &str,
        host: &mut HostContext,
        breakpoint_text: &str,
    ) -> ScriptTaskId {
        let mut instance = runtime
            .interpreter_mut()
            .instantiate_entity("Test", 1, host)
            .unwrap();
        instance.script_path = Some(path.to_string());
        runtime.set_debug_breakpoints(vec![
            resolve_breakpoint(path, source, line(source, breakpoint_text)).unwrap(),
        ]);
        runtime.spawn(instance, "main", vec![]).unwrap()
    }

    #[test]
    fn breakpoint_pauses_before_statement_mutates_instance() {
        let source = r#"
entity Test {
    value: number = 0
    fn main() {
        value = 5
    }
}
"#;
        let path = "scripts/test.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let task = spawn_main(&mut runtime, source, path, &mut host, "value = 5");

        let results = runtime.tick(0.0, &mut host).unwrap();
        assert_eq!(results, vec![(task, FiberResult::Paused)]);
        assert_eq!(
            runtime.scheduler().state(task),
            Some(ScriptTaskState::Paused)
        );
        assert_eq!(
            runtime.fiber(task).unwrap().instance().get_field("value"),
            Some(&Value::Number(0.0))
        );

        runtime.debug_command(DebugCommand::Continue).unwrap();
        assert_eq!(
            runtime.tick(0.0, &mut host).unwrap(),
            vec![(task, FiberResult::Complete)]
        );
        assert_eq!(
            runtime.fiber(task).unwrap().instance().get_field("value"),
            Some(&Value::Number(5.0))
        );
    }

    #[test]
    fn step_into_enters_nested_script_function() {
        let source = r#"
entity Test {
    fn helper() {
        debug.log("helper")
    }
    fn main() {
        helper()
        debug.log("after")
    }
}
"#;
        let path = "scripts/steps.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let task = spawn_main(&mut runtime, source, path, &mut host, "helper()");
        assert_eq!(
            runtime.tick(0.0, &mut host).unwrap(),
            vec![(task, FiberResult::Paused)]
        );

        runtime.debug_command(DebugCommand::StepInto).unwrap();
        assert_eq!(
            runtime.tick(0.0, &mut host).unwrap(),
            vec![(task, FiberResult::Paused)]
        );

        let snapshot = runtime.debug_snapshot().unwrap();
        assert_eq!(snapshot.frames[0].function_name, "helper");
        assert!(snapshot.frames.len() >= 2);
    }

    #[test]
    fn step_over_skips_nested_script_function_body() {
        let source = r#"
entity Test {
    fn helper() {
        debug.log("helper")
    }
    fn main() {
        helper()
        debug.log("after")
    }
}
"#;
        let path = "scripts/steps.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let task = spawn_main(&mut runtime, source, path, &mut host, "helper()");
        runtime.tick(0.0, &mut host).unwrap();

        runtime.debug_command(DebugCommand::StepOver).unwrap();
        assert_eq!(
            runtime.tick(0.0, &mut host).unwrap(),
            vec![(task, FiberResult::Paused)]
        );
        assert_eq!(
            runtime.debug_snapshot().unwrap().frames[0].function_name,
            "main"
        );
    }

    #[test]
    fn step_out_returns_to_caller() {
        let source = r#"
entity Test {
    fn helper() {
        debug.log("helper")
    }
    fn main() {
        helper()
        debug.log("after")
    }
}
"#;
        let path = "scripts/steps.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let task = spawn_main(&mut runtime, source, path, &mut host, "helper()");
        runtime.tick(0.0, &mut host).unwrap();

        runtime.debug_command(DebugCommand::StepInto).unwrap();
        runtime.tick(0.0, &mut host).unwrap();
        runtime.debug_command(DebugCommand::StepOut).unwrap();
        assert_eq!(
            runtime.tick(0.0, &mut host).unwrap(),
            vec![(task, FiberResult::Paused)]
        );
        assert_eq!(
            runtime.debug_snapshot().unwrap().frames[0].function_name,
            "main"
        );
    }

    #[test]
    fn step_intent_survives_wait_without_busy_looping() {
        let source = r#"
entity Test {
    value: number = 0
    fn main() {
        value = 1
        wait(1)
        value = 2
    }
}
"#;
        let path = "scripts/wait_step.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let task = spawn_main(&mut runtime, source, path, &mut host, "wait(1)");

        assert_eq!(
            runtime.tick(0.0, &mut host).unwrap(),
            vec![(task, FiberResult::Paused)]
        );
        runtime.debug_command(DebugCommand::StepInto).unwrap();

        let results = runtime.tick(0.0, &mut host).unwrap();
        assert!(matches!(results.as_slice(), [(id, FiberResult::Yield(_))] if *id == task));
        assert!(runtime.tick(0.5, &mut host).unwrap().is_empty());

        assert_eq!(
            runtime.tick(1.0, &mut host).unwrap(),
            vec![(task, FiberResult::Paused)]
        );
        assert_eq!(
            runtime.fiber(task).unwrap().instance().get_field("value"),
            Some(&Value::Number(1.0))
        );
    }

    #[test]
    fn canceling_paused_task_clears_debugger_focus() {
        let source = r#"
entity Test {
    fn main() {
        debug.log("pause")
    }
}
"#;
        let path = "scripts/cancel.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let task = spawn_main(&mut runtime, source, path, &mut host, "debug.log");
        runtime.tick(0.0, &mut host).unwrap();
        assert!(runtime.debug_snapshot().is_some());

        runtime.cancel(task).unwrap();
        assert!(runtime.debug_snapshot().is_none());
    }

    #[test]
    fn paused_snapshot_exposes_locals_and_instance_fields_read_only() {
        let source = r#"
entity Test {
    health: number = 10
    fn main() {
        const local_value = 7
        debug.log(local_value)
    }
}
"#;
        let path = "scripts/inspect.aeo";
        let mut runtime = runtime(source);
        let mut entities = EntityManager::new();
        let mut host = HostContext {
            delta_time: 0.0,
            engine: &mut entities,
        };
        let _task = spawn_main(
            &mut runtime,
            source,
            path,
            &mut host,
            "debug.log(local_value)",
        );
        runtime.tick(0.0, &mut host).unwrap();

        let snapshot = runtime.debug_snapshot().unwrap();
        let frame = &snapshot.frames[0];
        assert!(
            frame
                .locals
                .iter()
                .any(|value| value.name == "local_value" && value.display == "7")
        );
        assert!(
            frame
                .instance_fields
                .iter()
                .any(|value| value.name == "health" && value.display == "10")
        );
    }
}
