use std::collections::BTreeSet;

use crate::scripting::ast::{Block, Declaration, EntityMember, Program, Statement, StatementKind};
use crate::scripting::lexer::Lexer;
use crate::scripting::parser::Parser;
use crate::scripting::source::SourceSpan;

// Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:46 -05:00
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub struct DebugBreakpoint {
    pub script_path: String,
    pub line: usize,
    pub span_start: u32,
}

// Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 00:54 -05:00
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugInstructionPoint {
    pub script_path: String,
    pub function_name: String,
    pub span: SourceSpan,
    pub stack_depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugLocation {
    pub script_path: String,
    pub function_name: String,
    pub span: SourceSpan,
    pub line: usize,
    pub column: usize,
    pub stack_depth: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugVariableKind {
    Local,
    Captured,
    InstanceField,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugVariable {
    pub name: String,
    pub value_type: String,
    pub display: String,
    pub kind: DebugVariableKind,
    pub scope_index: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugFrameSnapshot {
    pub script_path: String,
    pub function_name: String,
    pub span: Option<SourceSpan>,
    pub line: Option<usize>,
    pub column: Option<usize>,
    pub locals: Vec<DebugVariable>,
    pub captured: Vec<DebugVariable>,
    pub instance_fields: Vec<DebugVariable>,
    pub entity_name: String,
    pub instance_id: u64,
    pub parent_actor_id: Option<u64>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DebugSnapshot {
    pub task_id: u64,
    pub selected_frame: usize,
    /// Frames are ordered current/callee first, then callers.
    pub frames: Vec<DebugFrameSnapshot>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DebugStepMode {
    Into,
    Over { origin_depth: usize },
    Out { origin_depth: usize },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugCommand {
    Continue,
    StepInto,
    StepOver,
    StepOut,
}

// Worldkiln 0.8.7 | WO 0.8.7-25 | 2026-10-06 15:03 -05:00
pub fn normalize_script_path(path: &str) -> String {
    path.replace('\\', "/")
        .split('/')
        .filter(|component| !component.is_empty() && *component != ".")
        .collect::<Vec<_>>()
        .join("/")
}

pub fn source_position(source: &str, span: SourceSpan) -> (usize, usize) {
    let offset = (span.start as usize).min(source.len());
    let prefix = &source[..offset];
    let line = prefix.bytes().filter(|byte| *byte == b'\n').count() + 1;
    let column = prefix
        .rsplit_once('\n')
        .map(|(_, tail)| tail.chars().count() + 1)
        .unwrap_or_else(|| prefix.chars().count() + 1);
    (line, column)
}

// Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:18 -05:00
fn collect_statement_spans(statement: &Statement, output: &mut Vec<SourceSpan>) {
    match &statement.kind {
        StatementKind::Variable { initializer, .. } => {
            if let Some(expression) = initializer {
                output.push(expression.span);
            }
        }
        StatementKind::Assignment { value, .. } => output.push(value.span),
        StatementKind::Expression(expression) => output.push(expression.span),
        StatementKind::Return(expression) => {
            if let Some(expression) = expression {
                output.push(expression.span);
            }
        }
        StatementKind::If {
            condition,
            then_block,
            else_if,
            else_block,
        } => {
            output.push(condition.span);
            collect_block_spans(then_block, output);
            for (condition, block) in else_if {
                output.push(condition.span);
                collect_block_spans(block, output);
            }
            if let Some(block) = else_block {
                collect_block_spans(block, output);
            }
        }
        StatementKind::While { condition, body } => {
            output.push(condition.span);
            collect_block_spans(body, output);
        }
        StatementKind::For { iterable, body, .. } => {
            output.push(iterable.span);
            collect_block_spans(body, output);
        }
    }
}

fn collect_block_spans(block: &Block, output: &mut Vec<SourceSpan>) {
    for statement in &block.statements {
        collect_statement_spans(statement, output);
    }
}

pub fn executable_statement_spans(program: &Program) -> Vec<SourceSpan> {
    let mut spans = Vec::new();

    for statement in &program.statements {
        collect_statement_spans(statement, &mut spans);
    }

    for declaration in &program.declarations {
        match declaration {
            Declaration::Function(function) => collect_block_spans(&function.body, &mut spans),
            Declaration::Event(event) => collect_block_spans(&event.body, &mut spans),
            Declaration::Entity(entity) => {
                for member in &entity.members {
                    if let EntityMember::Function(function) = member {
                        collect_block_spans(&function.body, &mut spans);
                    }
                }
            }
            Declaration::Import(_) => {}
        }
    }

    spans.sort_by_key(|span| (span.start, span.end));
    spans.dedup();
    spans
}

pub fn resolve_breakpoint(
    script_path: &str,
    source: &str,
    requested_line: usize,
) -> Result<DebugBreakpoint, String> {
    if requested_line == 0 {
        return Err("breakpoint lines are one-based".to_string());
    }

    let tokens = Lexer::new(source)
        .tokenize()
        .map_err(|error| format!("cannot resolve breakpoint: {}", error.message))?;
    let program = Parser::new(tokens)
        .parse()
        .map_err(|error| format!("cannot resolve breakpoint: {}", error.message))?;

    let candidates = executable_statement_spans(&program)
        .into_iter()
        .map(|span| {
            let (line, _) = source_position(source, span);
            (line, span)
        })
        .filter(|(line, _)| *line >= requested_line)
        .collect::<Vec<_>>();

    let Some((line, span)) = candidates
        .into_iter()
        .min_by_key(|(line, span)| (*line, span.start))
    else {
        return Err(format!(
            "line {} has no executable statement at or after it",
            requested_line
        ));
    };

    Ok(DebugBreakpoint {
        script_path: normalize_script_path(script_path),
        line,
        span_start: span.start,
    })
}

// Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 01:02 -05:00
#[derive(Clone, Debug, Default)]
pub struct ScriptDebugger {
    breakpoints: BTreeSet<(String, u32)>,
    paused_task: Option<crate::scripting::execution::ScriptTaskId>,
    paused_point: Option<DebugInstructionPoint>,
    skip_once: Option<(
        crate::scripting::execution::ScriptTaskId,
        String,
        u32,
        usize,
    )>,
    step: Option<(
        crate::scripting::execution::ScriptTaskId,
        DebugStepMode,
        String,
        u32,
        usize,
    )>,
}

impl ScriptDebugger {
    pub fn set_breakpoints<I>(&mut self, breakpoints: I)
    where
        I: IntoIterator<Item = DebugBreakpoint>,
    {
        self.breakpoints = breakpoint_set(breakpoints);
    }

    pub fn breakpoints(&self) -> &BTreeSet<(String, u32)> {
        &self.breakpoints
    }

    pub fn paused_task(&self) -> Option<crate::scripting::execution::ScriptTaskId> {
        self.paused_task
    }

    pub fn paused_point(&self) -> Option<&DebugInstructionPoint> {
        self.paused_point.as_ref()
    }

    pub fn should_pause(
        &mut self,
        task_id: crate::scripting::execution::ScriptTaskId,
        point: &DebugInstructionPoint,
    ) -> bool {
        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:08 -05:00
        // The debugger has one active focus. Other fibers keep running rather
        // than accumulating untracked paused tasks behind the selected stop.
        if self
            .paused_task
            .is_some_and(|paused_task| paused_task != task_id)
        {
            return false;
        }
        if self
            .step
            .as_ref()
            .is_some_and(|(step_task, _, _, _, _)| *step_task != task_id)
        {
            return false;
        }

        if let Some((skip_task, skip_path, skip_span, skip_depth)) = &self.skip_once {
            if *skip_task == task_id
                && skip_path == &point.script_path
                && *skip_span == point.span.start
                && *skip_depth == point.stack_depth
            {
                self.skip_once = None;
                return false;
            }
        }

        if let Some((step_task, mode, origin_path, origin_span, origin_depth)) = &self.step {
            if *step_task == task_id {
                let moved = origin_path != &point.script_path
                    || *origin_span != point.span.start
                    || *origin_depth != point.stack_depth;
                let stop = match *mode {
                    DebugStepMode::Into => moved,
                    DebugStepMode::Over { origin_depth } => {
                        moved && point.stack_depth <= origin_depth
                    }
                    DebugStepMode::Out { origin_depth } => point.stack_depth < origin_depth,
                };
                if stop {
                    self.step = None;
                    return true;
                }
            }
        }

        self.breakpoints
            .contains(&(point.script_path.clone(), point.span.start))
    }

    pub fn record_pause(
        &mut self,
        task_id: crate::scripting::execution::ScriptTaskId,
        point: DebugInstructionPoint,
    ) {
        self.paused_task = Some(task_id);
        self.paused_point = Some(point);
    }

    pub fn prepare_command(
        &mut self,
        command: DebugCommand,
    ) -> Result<crate::scripting::execution::ScriptTaskId, String> {
        let task_id = self
            .paused_task
            .ok_or_else(|| "AeoScript debugger is not paused.".to_string())?;
        let point = self
            .paused_point
            .clone()
            .ok_or_else(|| "AeoScript debugger has no paused source location.".to_string())?;

        // Worldkiln 0.8.7 | WO 0.8.7-17 | 2026-10-06 02:12 -05:00
        if command == DebugCommand::StepOut && point.stack_depth <= 1 {
            return Err("Step Out is unavailable at the root frame.".to_string());
        }

        self.skip_once = Some((
            task_id,
            point.script_path.clone(),
            point.span.start,
            point.stack_depth,
        ));
        self.step = match command {
            DebugCommand::Continue => None,
            DebugCommand::StepInto => Some((
                task_id,
                DebugStepMode::Into,
                point.script_path.clone(),
                point.span.start,
                point.stack_depth,
            )),
            DebugCommand::StepOver => Some((
                task_id,
                DebugStepMode::Over {
                    origin_depth: point.stack_depth,
                },
                point.script_path.clone(),
                point.span.start,
                point.stack_depth,
            )),
            DebugCommand::StepOut => Some((
                task_id,
                DebugStepMode::Out {
                    origin_depth: point.stack_depth,
                },
                point.script_path.clone(),
                point.span.start,
                point.stack_depth,
            )),
        };

        self.paused_task = None;
        self.paused_point = None;
        Ok(task_id)
    }

    pub fn clear_task(&mut self, task_id: crate::scripting::execution::ScriptTaskId) {
        if self.paused_task == Some(task_id) {
            self.paused_task = None;
            self.paused_point = None;
        }
        if self
            .skip_once
            .as_ref()
            .is_some_and(|(id, _, _, _)| *id == task_id)
        {
            self.skip_once = None;
        }
        if self
            .step
            .as_ref()
            .is_some_and(|(id, _, _, _, _)| *id == task_id)
        {
            self.step = None;
        }
    }

    pub fn clear_runtime_state(&mut self) {
        self.paused_task = None;
        self.paused_point = None;
        self.skip_once = None;
        self.step = None;
    }
}

pub fn breakpoint_set<I>(breakpoints: I) -> BTreeSet<(String, u32)>
where
    I: IntoIterator<Item = DebugBreakpoint>,
{
    breakpoints
        .into_iter()
        .map(|breakpoint| {
            (
                normalize_script_path(&breakpoint.script_path),
                breakpoint.span_start,
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn breakpoint_normalizes_project_relative_paths_and_resolves_next_statement() {
        let source = "fn test() {\n\n    const value = 1\n    value = 2\n}\n";
        let breakpoint = resolve_breakpoint(r".\\scripts\\systems\\test.aeo", source, 2).unwrap();

        assert_eq!(breakpoint.script_path, "scripts/systems/test.aeo");
        assert_eq!(breakpoint.line, 3);
        assert!(breakpoint.span_start > 0);
    }

    #[test]
    fn breakpoint_rejects_lines_after_last_executable_statement() {
        let source = "fn test() {\n    const value = 1\n}\n\n";
        let error = resolve_breakpoint("scripts/test.aeo", source, 4).unwrap_err();
        assert!(error.contains("no executable statement"));
    }

    #[test]
    fn active_debug_focus_does_not_pause_a_second_fiber() {
        let first = crate::scripting::execution::ScriptTaskId::new(1);
        let second = crate::scripting::execution::ScriptTaskId::new(2);
        let point = DebugInstructionPoint {
            script_path: "scripts/test.aeo".to_string(),
            function_name: "main".to_string(),
            span: SourceSpan::new(10, 11),
            stack_depth: 1,
        };
        let mut debugger = ScriptDebugger::default();
        debugger.set_breakpoints(vec![DebugBreakpoint {
            script_path: point.script_path.clone(),
            line: 1,
            span_start: point.span.start,
        }]);
        debugger.record_pause(first, point.clone());

        assert!(!debugger.should_pause(second, &point));
    }

    #[test]
    fn rejected_root_step_out_keeps_pause_state_intact() {
        let task = crate::scripting::execution::ScriptTaskId::new(1);
        let point = DebugInstructionPoint {
            script_path: "scripts/test.aeo".to_string(),
            function_name: "main".to_string(),
            span: SourceSpan::new(10, 11),
            stack_depth: 1,
        };
        let mut debugger = ScriptDebugger::default();
        debugger.record_pause(task, point);

        assert!(debugger.prepare_command(DebugCommand::StepOut).is_err());
        assert_eq!(debugger.paused_task(), Some(task));
        assert!(debugger.paused_point().is_some());
    }

    #[test]
    fn source_positions_are_one_based() {
        let source = "one\ntwo\nthree";
        assert_eq!(source_position(source, SourceSpan::new(4, 7)), (2, 1));
    }
}
