use crate::scripting::ast::{
    Block, Declaration, EntityMember, EventDecl, Expression, ExpressionKind, FunctionDecl,
    Parameter, Program, Statement, StatementKind, Type, TypeKind,
};
use crate::scripting::diagnostic::DiagnosticSeverity;
use crate::scripting::source::SourceSpan;
use std::collections::{BTreeSet, HashSet};

// Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:18 -05:00
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SemanticDiagnostic {
    pub severity: DiagnosticSeverity,
    pub span: SourceSpan,
    pub message: String,
}

fn error(span: SourceSpan, message: impl Into<String>) -> SemanticDiagnostic {
    SemanticDiagnostic {
        severity: DiagnosticSeverity::Error,
        span,
        message: message.into(),
    }
}

fn warning(span: SourceSpan, message: impl Into<String>) -> SemanticDiagnostic {
    SemanticDiagnostic {
        severity: DiagnosticSeverity::Warning,
        span,
        message: message.into(),
    }
}

fn named_type_name(ty: &Type) -> Option<(&str, bool)> {
    match &ty.kind {
        TypeKind::Named(name) => Some((name.as_str(), false)),
        TypeKind::Optional(inner) => named_type_name(inner).map(|(name, _)| (name, true)),
    }
}

fn statically_known_expression_type(expression: &Expression) -> Option<&'static str> {
    match &expression.kind {
        ExpressionKind::Number(_) => Some("number"),
        ExpressionKind::String(_) => Some("string"),
        ExpressionKind::Bool(_) => Some("bool"),
        ExpressionKind::Nil => Some("nil"),
        ExpressionKind::Array(_) => Some("basket"),
        ExpressionKind::Map(_) => Some("map"),
        ExpressionKind::AnonymousFunction { .. } => Some("function"),
        ExpressionKind::Literal(value) => Some(value.type_name()),
        ExpressionKind::Identifier(_)
        | ExpressionKind::Unary { .. }
        | ExpressionKind::Binary { .. }
        | ExpressionKind::Call { .. }
        | ExpressionKind::MethodCall { .. }
        | ExpressionKind::Member { .. }
        | ExpressionKind::Index { .. } => None,
    }
}

fn check_known_type_mismatch(
    ty: &Type,
    expression: &Expression,
    context: &str,
    output: &mut Vec<SemanticDiagnostic>,
) {
    let Some((expected, optional)) = named_type_name(ty) else {
        return;
    };
    let Some(actual) = statically_known_expression_type(expression) else {
        return;
    };

    if actual == "nil" && optional {
        return;
    }
    if expected.eq_ignore_ascii_case(actual) {
        return;
    }

    output.push(error(
        expression.span,
        format!(
            "{context} is annotated as '{expected}' but the initializer is statically '{actual}'"
        ),
    ));
}

fn check_parameters(owner: &str, parameters: &[Parameter], output: &mut Vec<SemanticDiagnostic>) {
    let mut names = HashSet::new();
    for parameter in parameters {
        if !names.insert(parameter.name.as_str()) {
            output.push(error(
                parameter.span,
                format!(
                    "{owner} declares duplicate parameter '{}'; parameter names share one lexical scope",
                    parameter.name
                ),
            ));
        }
    }
}

// Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:33 -05:00
fn check_expression(expression: &Expression, output: &mut Vec<SemanticDiagnostic>) {
    match &expression.kind {
        ExpressionKind::Unary { expression, .. } => check_expression(expression, output),
        ExpressionKind::Binary { left, right, .. } => {
            check_expression(left, output);
            check_expression(right, output);
        }
        ExpressionKind::Call { callee, arguments } => {
            check_expression(callee, output);
            for argument in arguments {
                check_expression(argument, output);
            }
        }
        ExpressionKind::MethodCall {
            object, arguments, ..
        } => {
            check_expression(object, output);
            for argument in arguments {
                check_expression(argument, output);
            }
        }
        ExpressionKind::Member { object, .. } => check_expression(object, output),
        ExpressionKind::Index { object, index } => {
            check_expression(object, output);
            check_expression(index, output);
        }
        ExpressionKind::Array(values) => {
            for value in values {
                check_expression(value, output);
            }
        }
        ExpressionKind::Map(entries) => {
            for (key, value) in entries {
                check_expression(key, output);
                check_expression(value, output);
            }
        }
        ExpressionKind::AnonymousFunction {
            parameters,
            return_type,
            body,
        } => {
            check_parameters("anonymous function", parameters, output);
            check_block(body, output);
            check_returns(body, return_type.as_ref(), "anonymous function", output);
        }
        ExpressionKind::Number(_)
        | ExpressionKind::String(_)
        | ExpressionKind::Bool(_)
        | ExpressionKind::Nil
        | ExpressionKind::Literal(_)
        | ExpressionKind::Identifier(_) => {}
    }
}

fn check_block(block: &Block, output: &mut Vec<SemanticDiagnostic>) {
    let mut declared = HashSet::new();

    for statement in &block.statements {
        match &statement.kind {
            StatementKind::Variable {
                name,
                type_annotation,
                initializer,
                ..
            } => {
                if !declared.insert(name.as_str()) {
                    output.push(error(
                        statement.span,
                        format!(
                            "variable '{}' is already declared in this lexical scope",
                            name
                        ),
                    ));
                }
                if let Some(initializer) = initializer {
                    check_expression(initializer, output);
                    if let Some(ty) = type_annotation {
                        check_known_type_mismatch(
                            ty,
                            initializer,
                            &format!("variable '{name}'"),
                            output,
                        );
                    }
                }
            }
            StatementKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            } => {
                check_expression(condition, output);
                check_block(then_block, output);
                for (condition, block) in else_if {
                    check_expression(condition, output);
                    check_block(block, output);
                }
                if let Some(block) = else_block {
                    check_block(block, output);
                }
            }
            StatementKind::While { condition, body } => {
                check_expression(condition, output);
                check_block(body, output);
            }
            StatementKind::For { iterable, body, .. } => {
                check_expression(iterable, output);
                check_block(body, output);
            }
            StatementKind::Assignment { target, value, .. } => {
                check_expression(target, output);
                check_expression(value, output);
            }
            StatementKind::Return(Some(expression)) | StatementKind::Expression(expression) => {
                check_expression(expression, output);
            }
            StatementKind::Return(None) => {}
        }
    }
}

fn check_returns(
    block: &Block,
    return_type: Option<&Type>,
    owner: &str,
    output: &mut Vec<SemanticDiagnostic>,
) {
    for statement in &block.statements {
        match &statement.kind {
            StatementKind::Return(Some(expression)) => {
                if let Some(return_type) = return_type {
                    check_known_type_mismatch(
                        return_type,
                        expression,
                        &format!("return value of {owner}"),
                        output,
                    );
                }
            }
            StatementKind::Return(None) => {
                if let Some(return_type) = return_type {
                    let optional = named_type_name(return_type)
                        .map(|(_, optional)| optional)
                        .unwrap_or(false);
                    if !optional {
                        output.push(error(
                            statement.span,
                            format!(
                                "{owner} declares a non-optional return type but returns without a value"
                            ),
                        ));
                    }
                }
            }
            StatementKind::If {
                then_block,
                else_if,
                else_block,
                ..
            } => {
                check_returns(then_block, return_type, owner, output);
                for (_, block) in else_if {
                    check_returns(block, return_type, owner, output);
                }
                if let Some(block) = else_block {
                    check_returns(block, return_type, owner, output);
                }
            }
            StatementKind::While { body, .. } | StatementKind::For { body, .. } => {
                check_returns(body, return_type, owner, output)
            }
            StatementKind::Variable { .. }
            | StatementKind::Assignment { .. }
            | StatementKind::Expression(_) => {}
        }
    }
}

fn check_function(function: &FunctionDecl, owner: &str, output: &mut Vec<SemanticDiagnostic>) {
    check_parameters(owner, &function.parameters, output);
    check_block(&function.body, output);
    check_returns(&function.body, function.return_type.as_ref(), owner, output);
}

fn check_event(event: &EventDecl, output: &mut Vec<SemanticDiagnostic>) {
    let owner = format!("event '{}'", event.name);
    check_parameters(&owner, &event.parameters, output);
    check_block(&event.body, output);
}

fn check_entity_lifecycle(function: &FunctionDecl, output: &mut Vec<SemanticDiagnostic>) {
    let expected = match function.name.as_str() {
        "on_spawn" | "on_ready" | "on_destroy" => Some(0),
        "update" => Some(1),
        _ => None,
    };
    let Some(expected) = expected else {
        return;
    };
    if function.parameters.len() != expected {
        output.push(error(
            function.span,
            format!(
                "Entity lifecycle '{}' requires {} parameter(s), found {}",
                function.name,
                expected,
                function.parameters.len()
            ),
        ));
    }
}

fn explicit_import_logical_path(path: &[String]) -> Option<String> {
    if path.len() < 3 {
        return None;
    }
    let root = path.first()?.as_str();
    if root != "scripts" && root != "playerscripts" {
        return None;
    }
    if path.last()?.as_str() != "aeo" {
        return None;
    }
    let module = path[1..path.len() - 1].join("/");
    if module.is_empty() {
        return None;
    }
    Some(format!("{root}/{module}.aeo"))
}

// Worldkiln 0.8.7 | WO 0.8.7-19 | 2026-10-06 03:18 -05:00
pub fn analyze_program(
    program: &Program,
    available_modules: &BTreeSet<String>,
) -> Vec<SemanticDiagnostic> {
    let mut output = Vec::new();

    check_block(
        &Block {
            span: program.span,
            statements: program.statements.clone(),
        },
        &mut output,
    );

    for declaration in &program.declarations {
        match declaration {
            Declaration::Function(function) => {
                check_function(
                    function,
                    &format!("function '{}'", function.name),
                    &mut output,
                );
            }
            Declaration::Event(event) => check_event(event, &mut output),
            Declaration::Entity(entity) => {
                let mut member_names = HashSet::new();
                for member in &entity.members {
                    match member {
                        EntityMember::Field(field) => {
                            if !member_names.insert(field.name.as_str()) {
                                output.push(warning(
                                    field.span,
                                    format!(
                                        "Entity '{}' repeats member '{}'; runtime field/member lookup may shadow an earlier declaration",
                                        entity.name, field.name
                                    ),
                                ));
                            }
                            if let (Some(ty), Some(initializer)) =
                                (&field.type_annotation, &field.initializer)
                            {
                                check_known_type_mismatch(
                                    ty,
                                    initializer,
                                    &format!("field '{}.{}'", entity.name, field.name),
                                    &mut output,
                                );
                            }
                        }
                        EntityMember::Function(function) => {
                            if !member_names.insert(function.name.as_str()) {
                                output.push(warning(
                                    function.span,
                                    format!(
                                        "Entity '{}' repeats member '{}'; runtime member lookup may shadow an earlier declaration",
                                        entity.name, function.name
                                    ),
                                ));
                            }
                            check_entity_lifecycle(function, &mut output);
                            check_function(
                                function,
                                &format!("function '{}.{}'", entity.name, function.name),
                                &mut output,
                            );
                        }
                    }
                }
            }
            Declaration::Import(import) => {
                if let Some(logical) = explicit_import_logical_path(&import.path) {
                    let normalized = logical.to_ascii_lowercase();
                    if !available_modules
                        .iter()
                        .any(|candidate| candidate.to_ascii_lowercase() == normalized)
                    {
                        output.push(error(
                            import.span,
                            format!(
                                "import '{}' does not resolve to an indexed project Module",
                                import.path.join(".")
                            ),
                        ));
                    }
                }
            }
        }
    }

    output.sort_by_key(|diagnostic| {
        (
            diagnostic.span.start,
            diagnostic.span.end,
            diagnostic.message.clone(),
        )
    });
    output.dedup();
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::lexer::Lexer;
    use crate::scripting::parser::Parser;

    fn analyze(source: &str, modules: &[&str]) -> Vec<SemanticDiagnostic> {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        analyze_program(
            &program,
            &modules.iter().map(|path| path.to_string()).collect(),
        )
    }

    // Worldkiln 0.8.7 | WO 0.8.7-26 | 2026-10-06 15:03 -05:00
    #[test]
    fn duplicate_same_scope_variable_is_error_but_nested_shadowing_is_valid() {
        // AeoScript declarations are explicit: `const name = ...` or
        // `name: Type = ...`. Bare `name = ...` syntax is reassignment and
        // must never be reinterpreted as a declaration by semantic analysis.
        let duplicate = analyze(
            "fn test() {\n value: number = 1\n value: number = 2\n}\n",
            &[],
        );
        assert!(
            duplicate
                .iter()
                .any(|diagnostic| diagnostic.message.contains("already declared"))
        );

        let shadow = analyze(
            "fn test() {\n value: number = 1\n if true {\n value: number = 2\n }\n}\n",
            &[],
        );
        assert!(
            !shadow
                .iter()
                .any(|diagnostic| diagnostic.message.contains("already declared"))
        );
    }

    #[test]
    fn reassignment_is_not_reported_as_duplicate_declaration() {
        let diagnostics = analyze(
            "fn test() {\n value: number = 1\n value = 2\n value += 3\n}\n",
            &[],
        );

        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("already declared"))
        );
    }

    #[test]
    fn duplicate_anonymous_function_parameters_are_errors() {
        let diagnostics = analyze(
            "fn test() {\n const callback = fn(value, value) { return value }\n}\n",
            &[],
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("duplicate parameter"))
        );
    }

    #[test]
    fn duplicate_parameters_are_errors() {
        let diagnostics = analyze("fn test(value, value) {}\n", &[]);
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("duplicate parameter"))
        );
    }

    #[test]
    fn explicit_rooted_missing_import_is_error_and_existing_module_is_valid() {
        let missing = analyze("import scripts.missing.aeo\n", &[]);
        assert!(
            missing
                .iter()
                .any(|diagnostic| diagnostic.message.contains("does not resolve"))
        );

        let present = analyze(
            "import scripts.systems.health.aeo\n",
            &["scripts/systems/health.aeo"],
        );
        assert!(present.is_empty());
    }

    #[test]
    fn lifecycle_arity_is_checked_without_rejecting_custom_members() {
        let diagnostics = analyze(
            "entity Test {\n fn on_spawn(value) {}\n fn update() {}\n fn custom(a, b) {}\n}\n",
            &[],
        );
        assert_eq!(
            diagnostics
                .iter()
                .filter(|diagnostic| diagnostic.message.contains("lifecycle"))
                .count(),
            2
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("custom"))
        );
    }

    #[test]
    fn only_statically_known_type_mismatches_are_errors() {
        let diagnostics = analyze(
            "fn test(dynamic_value) {\n a: number = \"text\"\n b: number = dynamic_value\n c: string? = nil\n}\n",
            &[],
        );
        assert!(
            diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("variable 'a'"))
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("variable 'b'"))
        );
        assert!(
            !diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("variable 'c'"))
        );
    }

    #[test]
    fn heterogeneous_dynamic_collections_are_not_rejected() {
        let diagnostics = analyze(
            "fn test() {\n const values = [1, \"two\", true, nil]\n const data = {\"a\": 1, \"b\": \"two\"}\n}\n",
            &[],
        );
        assert!(diagnostics.is_empty());
    }

    #[test]
    fn repeated_entity_members_are_warnings_not_errors() {
        let diagnostics = analyze("entity Test {\n value = 1\n value = 2\n}\n", &[]);
        assert!(diagnostics.iter().any(|diagnostic| {
            diagnostic.severity == DiagnosticSeverity::Warning
                && diagnostic.message.contains("repeats member")
        }));
    }
}
