// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN. All rights reserved.
// Licensed under the AeoEngine Source-Available License.
// See LICENSE in the repository root for the complete terms.
use crate::scripting::ast::{
    Block, Declaration, EntityMember, EventDecl, Expression, ExpressionKind, FunctionDecl,
    Parameter, Program, Statement, StatementKind, Type, TypeKind,
};
use crate::scripting::diagnostic::DiagnosticSeverity;
use crate::scripting::source::SourceSpan;
use std::collections::{BTreeSet, HashSet};

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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum KnownApiType {
    Unknown,
    Number,
    Bool,
    String,
    Basket(Option<usize>),
    Actor,
    Module,
    Math,
    Debug,
    Time,
    Player,
    Save,
    Cell,
    Entity,
}
struct ApiChecker {
    scopes: Vec<std::collections::HashMap<String, (KnownApiType, bool)>>,
    attached: bool,
    errors: Vec<SemanticDiagnostic>,
}
impl ApiChecker {
    fn new(attached: bool) -> Self {
        Self {
            scopes: vec![Default::default()],
            attached,
            errors: vec![],
        }
    }
    fn lookup(&self, name: &str) -> KnownApiType {
        for scope in self.scopes.iter().rev() {
            if let Some((kind, _)) = scope.get(name) {
                return *kind;
            }
        }
        match name {
            "self" if self.attached => KnownApiType::Module,
            "math" => KnownApiType::Math,
            "debug" => KnownApiType::Debug,
            "time" => KnownApiType::Time,
            "player" => KnownApiType::Player,
            "save" => KnownApiType::Save,
            "cell" => KnownApiType::Cell,
            "entity" => KnownApiType::Entity,
            _ => KnownApiType::Unknown,
        }
    }
    fn type_of(&self, expr: &Expression) -> KnownApiType {
        use KnownApiType as K;
        match &expr.kind {
            ExpressionKind::Number(_) => K::Number,
            ExpressionKind::String(_) => K::String,
            ExpressionKind::Bool(_) => K::Bool,
            ExpressionKind::Array(items) => K::Basket(Some(items.len())),
            ExpressionKind::Identifier(name) => self.lookup(name),
            ExpressionKind::Member { object, name } => {
                match (self.type_of(object), name.as_str()) {
                    (K::Module, "parent") => K::Actor,
                    (K::Actor, "position" | "rotation" | "scale") | (K::Player, "position") => {
                        K::Basket(Some(3))
                    }
                    (
                        K::Actor,
                        "visible" | "enabled" | "can_collide" | "solid" | "collision_enabled"
                        | "anchored",
                    ) => K::Bool,
                    (K::Basket(_), _) => K::Unknown,
                    _ => K::Unknown,
                }
            }
            ExpressionKind::MethodCall { object, method, .. } => {
                match (self.type_of(object), method.as_str()) {
                    (K::Basket(_), "len") | (K::Math, _) => K::Number,
                    (K::Actor, "is_navigating" | "navigate_to" | "cancel_navigation")
                    | (K::Player, "is_grounded" | "is_swimming" | "is_valid")
                    | (K::Save, "has" | "remove")
                    | (K::Cell, "exists")
                    | (K::Entity, "exists") => K::Bool,
                    (K::Actor, "find_path") => K::Unknown,
                    (K::Time, "elapsed") => K::Number,
                    _ => K::Unknown,
                }
            }
            ExpressionKind::Binary { left, right, .. }
                if self.type_of(left) == K::Number && self.type_of(right) == K::Number =>
            {
                K::Number
            }
            ExpressionKind::Unary { expression, .. } => self.type_of(expression),
            _ => K::Unknown,
        }
    }
    fn fail(&mut self, span: SourceSpan, message: impl Into<String>) {
        self.errors.push(error(span, message));
    }
    fn caution(&mut self, span: SourceSpan, message: impl Into<String>) {
        self.errors.push(warning(span, message));
    }
    fn arity(
        &mut self,
        span: SourceSpan,
        owner: &str,
        method: &str,
        count: usize,
        min: usize,
        max: usize,
    ) {
        if count < min || count > max {
            self.fail(
                span,
                format!(
                    "{owner}.{method}() expects {} argument(s), received {count}",
                    if min == max {
                        format!("{min}")
                    } else {
                        format!("{min}..={max}")
                    }
                ),
            );
        }
    }
    fn check_builtin_argument(
        &mut self,
        expr: &Expression,
        owner: &str,
        method: &str,
        expected: &str,
    ) {
        use KnownApiType as K;
        let actual = self.type_of(expr);
        let invalid = match expected {
            "number" => !matches!(actual, K::Number | K::Unknown),
            "string" => !matches!(actual, K::String | K::Unknown),
            "handle" => matches!(actual, K::Number | K::Bool | K::String | K::Basket(_)),
            _ => false,
        };
        if invalid {
            self.fail(
                expr.span,
                format!("{owner}.{method} expects a {expected} argument"),
            );
        }
    }
    fn check_value(&mut self, expr: &Expression) {
        use KnownApiType as K;
        match &expr.kind {
            ExpressionKind::Member { object, name } => {
                self.check_value(object);
                match (self.type_of(object), name.as_str()) {
                    (K::Module, "position" | "rotation" | "scale") =>
                        self.fail(expr.span, format!(
                            "self.{name} is not an Actor property in an attached Module. Use self.parent.{name}"
                        )),
                    (K::Basket(_), "length") =>
                        self.fail(expr.span, "AeoScript baskets do not expose .length. Call .len()"),
                    (K::Number | K::Bool, _) =>
                        self.fail(expr.span, "Numbers and booleans do not have object properties"),
                    (K::String, "length") =>
                        self.fail(expr.span, "AeoScript strings use len(), not .length"),
                    _ => {}
                }
            }
            ExpressionKind::MethodCall {
                object,
                method,
                arguments,
            } => {
                self.check_value(object);
                for arg in arguments {
                    self.check_value(arg);
                }
                match self.type_of(object) {
                    K::Module
                        if matches!(
                            method.as_str(),
                            "set_position"
                                | "navigate_to"
                                | "cancel_navigation"
                                | "is_navigating"
                                | "translate"
                        ) =>
                    {
                        self.fail(expr.span, format!("self.{method}() is not a Module API. Use self.parent.{method}() on an attached Actor"));
                    }
                    K::Actor => match method.as_str() {
                        "get_attachments" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 0, 0);
                        }
                        "find_attachment_by_name" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 1, 1);
                            if let Some(name) = arguments.first() {
                                self.check_builtin_argument(name, "Actor", method, "string");
                            }
                        }
                        "find_path" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 1, 1);
                            if let Some(target) = arguments.first() {
                                if !matches!(self.type_of(target), K::Basket(_) | K::Unknown) {
                                    self.fail(target.span, "Actor.find_path target must be an [x, y, z] basket");
                                }
                                if let K::Basket(Some(n)) = self.type_of(target) {
                                    if n != 3 {
                                        self.fail(target.span, format!("Actor.find_path requires 3 coordinates, received {n}"));
                                    }
                                }
                            }
                        }
                        "navigate_to" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 1, 3);
                            if let Some(target) = arguments.first() {
                                if !matches!(self.type_of(target), K::Basket(_) | K::Unknown) {
                                    self.fail(
                                        target.span,
                                        "Actor.navigate_to target must be an [x, y, z] basket",
                                    );
                                }
                                if let K::Basket(Some(n)) = self.type_of(target) {
                                    if n != 3 {
                                        self.fail(target.span, format!("Actor.navigate_to target must contain 3 coordinates, received {n}"));
                                    }
                                }
                                if let ExpressionKind::Array(items) = &target.kind {
                                    for component in items {
                                        if matches!(
                                            self.type_of(component),
                                            K::String | K::Bool | K::Basket(_)
                                        ) {
                                            self.fail(
                                                component.span,
                                                "Actor.navigate_to coordinates must be numeric",
                                            );
                                        }
                                    }
                                }
                            }
                            for arg in arguments.iter().skip(1) {
                                if matches!(self.type_of(arg), K::String | K::Bool | K::Basket(_)) {
                                    self.fail(
                                        arg.span,
                                        "Actor.navigate_to speed and stop distance must be numbers",
                                    );
                                }
                            }
                        }
                        "cancel_navigation" | "is_navigating" | "is_valid"
                        | "is_grounded" | "name" | "destroy" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 0, 0)
                        }
                        "set_position" | "translate" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 3, 3)
                        }
                        "set_horizontal_velocity" | "set_facing_direction" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 2, 2)
                        }
                        "select_animation" | "apply_vertical_impulse" => {
                            self.arity(expr.span, "Actor", method, arguments.len(), 1, 1)
                        }
                        _ => self.caution(
                            expr.span,
                            format!("Actor method '{method}' is not a built-in; verify a script-defined handler exists"),
                        ),
                    },
                    K::Basket(_) => match method.as_str() {
                        "length" => self.fail(
                            expr.span,
                            "AeoScript baskets do not have length(). Use len()",
                        ),
                        "len" | "clear" | "sort" | "freeze" => {
                            self.arity(expr.span, "basket", method, arguments.len(), 0, 0)
                        }
                        "insert" => self.arity(expr.span, "basket", method, arguments.len(), 1, 2),
                        "remove" => self.arity(expr.span, "basket", method, arguments.len(), 0, 1),
                        "find" => self.arity(expr.span, "basket", method, arguments.len(), 1, 2),
                        "clone" => self.arity(expr.span, "basket", method, arguments.len(), 0, 0),
                        "concat" => self.arity(expr.span, "basket", method, arguments.len(), 0, 3),
                        "move" => self.arity(expr.span, "basket", method, arguments.len(), 3, 4),
                        _ => self.fail(expr.span, format!(
                            "Unknown basket method '{method}'. Use the documented basket API (e.g. len(), insert(), remove())"
                        )),
                    },
                    K::Math => {
                        match method.as_str() {
                            "random" => {
                                if arguments.len() != 0 && arguments.len() != 2 {
                                    self.fail(expr.span, "math.random() expects either no arguments or two integer bounds (min, max)");
                                }
                                if arguments.len() == 2 {
                                    for arg in arguments {
                                        if let ExpressionKind::Number(n) = arg.kind {
                                            if !n.is_finite() || n.fract() != 0.0 {
                                                self.fail(
                                                    arg.span,
                                                    "math.random bounds must be finite integers",
                                                );
                                            }
                                        }
                                    }
                                    if let (ExpressionKind::Number(a), ExpressionKind::Number(b)) =
                                        (&arguments[0].kind, &arguments[1].kind)
                                    {
                                        if a > b {
                                            self.fail(
                                                expr.span,
                                                "math.random lower bound cannot exceed upper bound",
                                            );
                                        }
                                    }
                                }
                            }
                            "abs" | "floor" | "ceil" | "round" | "sqrt" | "sin" | "cos" | "tan" => {
                                self.arity(expr.span, "math", method, arguments.len(), 1, 1)
                            }
                            "min" | "max" | "pow" => {
                                self.arity(expr.span, "math", method, arguments.len(), 2, 2)
                            }
                            "clamp" | "lerp" => {
                                self.arity(expr.span, "math", method, arguments.len(), 3, 3)
                            }
                            "deg_to_rad" | "rad_to_deg" => {
                                self.arity(expr.span, "math", method, arguments.len(), 1, 1)
                            }
                            _ => self.fail(expr.span, format!(
                                "Unknown math function '{method}'. Check the supported AeoScript math API"
                            )),
                        }
                        for arg in arguments {
                            if matches!(self.type_of(arg), K::String | K::Bool | K::Basket(_)) {
                                self.fail(
                                    arg.span,
                                    format!("math.{method} expects numeric arguments"),
                                );
                            }
                        }
                    }
                    K::Player => match method.as_str() {
                        "set_horizontal_velocity" | "set_facing_direction" =>
                            self.arity(expr.span, "player", method, arguments.len(), 2, 2),
                        "set_velocity" => self.arity(expr.span, "player", method, arguments.len(), 3, 3),
                        "set_gravity_scale" | "set_controller_enabled" =>
                            self.arity(expr.span, "player", method, arguments.len(), 1, 1),
                        "get_velocity" | "get_gravity_scale" | "is_controller_enabled" =>
                            self.arity(expr.span, "player", method, arguments.len(), 0, 0),
                        "select_animation" | "apply_vertical_impulse" =>
                            self.arity(expr.span, "player", method, arguments.len(), 1, 1),
                        "is_grounded" | "is_swimming" | "is_valid" =>
                            self.arity(expr.span, "player", method, arguments.len(), 0, 0),
                        _ => self.fail(expr.span, format!("Unknown player API '{method}'")),
                    },
                    K::Save => {
                        match method.as_str() {
                            "get" | "get_default" | "has" | "remove" =>
                                self.arity(expr.span, "save", method, arguments.len(), 1, 1),
                            "set" =>
                                self.arity(expr.span, "save", method, arguments.len(), 2, 3),
                            "commit" =>
                                self.arity(expr.span, "save", method, arguments.len(), 0, 0),
                            "restart" | "reset" =>
                                self.arity(expr.span, "save", method, arguments.len(), 0, 1),
                            _ => self.fail(expr.span, format!("Unknown save API '{method}'")),
                        }
                        if matches!(method.as_str(), "get" | "get_default" | "has" | "remove" | "set" | "reset") {
                            if let Some(key) = arguments.first() {
                                if matches!(self.type_of(key), K::Number | K::Bool | K::Basket(_)) {
                                    self.fail(key.span, "Save key must be a string");
                                }
                            }
                        }
                        if method == "set" {
                            for value in arguments.iter().skip(1) {
                                if matches!(self.type_of(value), K::Basket(_)) {
                                    self.fail(value.span, "save.set values/defaults support number, bool or string; not baskets");
                                }
                            }
                        }
                    }
                    K::Cell => match method.as_str() {
                        "new" | "delete" | "get" | "exists" =>
                            self.arity(expr.span, "cell", method, arguments.len(), 1, 1),
                        _ => self.fail(expr.span, format!("Unknown cell API '{method}'")),
                    },
                    K::Entity => match method.as_str() {
                        "get" | "exists" =>
                            self.arity(expr.span, "entity", method, arguments.len(), 1, 1),
                        _ => self.fail(expr.span, format!("Unknown entity API '{method}'")),
                    },
                    K::Time if method == "elapsed" =>
                        self.arity(expr.span, "time", method, arguments.len(), 0, 0),
                    K::Time => self.fail(expr.span, format!("Unknown time API '{method}'")),
                    K::Debug if method == "log" => {}
                    K::Debug if method == "traceback" =>
                        self.arity(expr.span, "debug", method, arguments.len(), 0, 0),
                    K::Debug => self.fail(expr.span, format!("Unknown debug API '{method}'")),
                    K::Number | K::Bool => self.fail(
                        expr.span, "Cannot call methods on a number or boolean"
                    ),
                    K::String => match method.as_str() {
                        "len" | "lower" | "upper" | "reverse" =>
                            self.arity(expr.span, "string", method, arguments.len(), 0, 0),
                        "split" => self.arity(expr.span, "string", method, arguments.len(), 0, 1),
                        _ => self.fail(expr.span, format!("Unknown string method '{method}'")),
                    },
                    _ => {}
                }
                let receiver = self.type_of(object);
                for (position, argument) in arguments.iter().enumerate() {
                    let expected = match (receiver, method.as_str(), position) {
                        (
                            K::Actor,
                            "set_position"
                            | "translate"
                            | "set_horizontal_velocity"
                            | "set_facing_direction",
                            _,
                        ) => Some("number"),
                        (K::Actor, "apply_vertical_impulse", 0) => Some("number"),
                        (K::Actor, "select_animation", 0) => Some("string"),
                        (K::Player, "set_horizontal_velocity" | "set_facing_direction", _) => {
                            Some("number")
                        }
                        (K::Player, "apply_vertical_impulse" | "set_gravity_scale", 0) => {
                            Some("number")
                        }
                        (K::Player, "set_velocity", _) => Some("number"),
                        (K::Player, "set_controller_enabled", 0) => Some("bool"),
                        (K::Player, "select_animation", 0) => Some("string"),
                        (K::Cell, "new", 0) => Some("string"),
                        (K::Cell, "get" | "exists", 0) | (K::Entity, "get" | "exists", 0) => {
                            Some("number")
                        }
                        (K::Cell, "delete", 0) => Some("handle"),
                        (K::Basket(_), "insert", 0) if arguments.len() == 2 => Some("number"),
                        _ => None,
                    };
                    if let Some(expected) = expected {
                        let owner = match receiver {
                            K::Actor => "Actor",
                            K::Player => "player",
                            K::Cell => "cell",
                            K::Entity => "entity",
                            K::Basket(_) => "basket",
                            _ => "API",
                        };
                        self.check_builtin_argument(argument, owner, method, expected);
                    }
                }
                if matches!(
                    method.as_str(),
                    "insert" | "remove" | "clear" | "move" | "concat"
                ) && matches!(self.type_of(object), K::Basket(_))
                {
                    if let ExpressionKind::Identifier(name) = &object.kind {
                        if let Some(scope) = self
                            .scopes
                            .iter_mut()
                            .rev()
                            .find(|scope| scope.contains_key(name))
                        {
                            if let Some((kind, _)) = scope.get_mut(name) {
                                *kind = K::Basket(None);
                            }
                        }
                    }
                }
            }
            ExpressionKind::Call { callee, arguments } => {
                if let ExpressionKind::Member { object, name } = &callee.kind {
                    self.check_value(&Expression {
                        span: expr.span,
                        kind: ExpressionKind::MethodCall {
                            object: object.clone(),
                            method: name.clone(),
                            arguments: arguments.clone(),
                        },
                    });
                } else {
                    self.check_value(callee);
                    for arg in arguments {
                        self.check_value(arg);
                    }
                    if matches!(
                        self.type_of(callee),
                        K::Number | K::Bool | K::String | K::Basket(_)
                    ) {
                        self.fail(expr.span, "Cannot call a non-function value");
                    }
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.check_value(left);
                self.check_value(right);
            }
            ExpressionKind::Unary { expression, .. } => self.check_value(expression),
            ExpressionKind::Index { object, index } => {
                self.check_value(object);
                self.check_value(index);
                if let (K::Basket(Some(length)), ExpressionKind::Number(value)) =
                    (self.type_of(object), &index.kind)
                {
                    if !value.is_finite()
                        || value.fract() != 0.0
                        || *value < 0.0
                        || *value >= length as f64
                    {
                        self.caution(
                            index.span,
                            format!(
                                "Basket index {value} may be outside the original 0..{length} range"
                            ),
                        );
                    }
                }
            }
            ExpressionKind::Array(items) => {
                for item in items {
                    self.check_value(item)
                }
            }
            ExpressionKind::Map(entries) => {
                for (key, val) in entries {
                    self.check_value(key);
                    self.check_value(val);
                }
            }
            ExpressionKind::AnonymousFunction {
                parameters, body, ..
            } => {
                let baseline = self.scopes.clone();
                self.scopes.push(Default::default());
                for p in parameters {
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(p.name.clone(), (K::Unknown, false));
                }
                self.check_block(body, false);
                self.scopes.pop();
                let possible = self.scopes.clone();
                self.scopes = baseline.clone();
                self.weaken_changed_outer_types(&baseline, &possible);
            }
            _ => {}
        }
    }
    fn weaken_changed_outer_types(
        &mut self,
        baseline: &[std::collections::HashMap<String, (KnownApiType, bool)>],
        observed: &[std::collections::HashMap<String, (KnownApiType, bool)>],
    ) {
        for (scope, (before, after)) in self
            .scopes
            .iter_mut()
            .zip(baseline.iter().zip(observed.iter()))
        {
            for (name, (kind, is_const)) in scope.iter_mut() {
                if !*is_const
                    && before.get(name).map(|(kind, _)| kind)
                        != after.get(name).map(|(kind, _)| kind)
                {
                    *kind = KnownApiType::Unknown;
                }
            }
        }
    }

    fn check_block(&mut self, block: &Block, own_scope: bool) {
        if own_scope {
            self.scopes.push(Default::default());
        }
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Variable {
                    name,
                    initializer,
                    is_const,
                    ..
                } => {
                    let kind = initializer
                        .as_ref()
                        .map(|val| {
                            self.check_value(val);
                            self.type_of(val)
                        })
                        .unwrap_or(KnownApiType::Unknown);
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(name.clone(), (kind, *is_const));
                }
                StatementKind::Assignment {
                    target,
                    value,
                    operator,
                } => {
                    self.check_value(target);
                    self.check_value(value);
                    if let ExpressionKind::Identifier(name) = &target.kind {
                        let inferred = match operator {
                            crate::scripting::ast::AssignmentOperator::Assign => {
                                self.type_of(value)
                            }
                            crate::scripting::ast::AssignmentOperator::Add
                                if matches!(self.lookup(name), KnownApiType::String)
                                    || matches!(self.type_of(value), KnownApiType::String) =>
                            {
                                KnownApiType::String
                            }
                            _ => KnownApiType::Unknown,
                        };
                        let previous = self
                            .scopes
                            .iter()
                            .rev()
                            .find_map(|scope| scope.get(name).copied());
                        if previous.is_some_and(|(_, is_const)| is_const) {
                            self.fail(
                                target.span,
                                format!("Cannot reassign const variable '{name}'"),
                            );
                        } else if let Some(scope) = self
                            .scopes
                            .iter_mut()
                            .rev()
                            .find(|scope| scope.contains_key(name))
                        {
                            scope.insert(name.clone(), (inferred, false));
                        } else {
                            self.fail(
                                target.span,
                                format!(
                                    "Cannot assign to undeclared variable '{name}'. Declare it with `name: Type = ...` or `const name = ...` first"
                                ),
                            );
                        }
                    }
                }
                StatementKind::Expression(expr) | StatementKind::Return(Some(expr)) => {
                    self.check_value(expr)
                }
                StatementKind::If {
                    condition,
                    then_block,
                    else_if,
                    else_block,
                } => {
                    let baseline = self.scopes.clone();
                    self.check_value(condition);
                    self.check_block(then_block, true);
                    let mut possibilities = vec![self.scopes.clone()];
                    for (condition, block) in else_if {
                        self.scopes = baseline.clone();
                        self.check_value(condition);
                        self.check_block(block, true);
                        possibilities.push(self.scopes.clone());
                    }
                    if let Some(block) = else_block {
                        self.scopes = baseline.clone();
                        self.check_block(block, true);
                        possibilities.push(self.scopes.clone());
                    }
                    self.scopes = baseline.clone();
                    for possible in &possibilities {
                        self.weaken_changed_outer_types(&baseline, possible);
                    }
                }
                StatementKind::While { condition, body } => {
                    let baseline = self.scopes.clone();
                    self.check_value(condition);
                    self.check_block(body, true);
                    let possibly_executed = self.scopes.clone();
                    self.scopes = baseline.clone();
                    self.weaken_changed_outer_types(&baseline, &possibly_executed);
                }
                StatementKind::For {
                    name,
                    iterable,
                    body,
                } => {
                    let baseline = self.scopes.clone();
                    self.check_value(iterable);
                    self.scopes.push(Default::default());
                    self.scopes
                        .last_mut()
                        .unwrap()
                        .insert(name.clone(), (KnownApiType::Unknown, false));
                    self.check_block(body, false);
                    self.scopes.pop();
                    let possibly_executed = self.scopes.clone();
                    self.scopes = baseline.clone();
                    self.weaken_changed_outer_types(&baseline, &possibly_executed);
                }
                StatementKind::Return(None) => {}
            }
        }
        if own_scope {
            self.scopes.pop();
        }
    }
}
pub fn strict_builtin_api_diagnostics(
    program: &Program,
    attached_module: bool,
) -> Vec<SemanticDiagnostic> {
    let mut checker = ApiChecker::new(attached_module);
    checker.check_block(
        &Block {
            span: program.span,
            statements: program.statements.clone(),
        },
        false,
    );
    for declaration in &program.declarations {
        match declaration {
            Declaration::Function(function) => {
                let baseline = checker.scopes.clone();
                checker.scopes.push(Default::default());
                for p in &function.parameters {
                    checker
                        .scopes
                        .last_mut()
                        .unwrap()
                        .insert(p.name.clone(), (KnownApiType::Unknown, false));
                }
                checker.check_block(&function.body, false);
                checker.scopes.pop();
                let possible = checker.scopes.clone();
                checker.scopes = baseline.clone();
                checker.weaken_changed_outer_types(&baseline, &possible);
            }
            Declaration::Event(event) => {
                let baseline = checker.scopes.clone();
                checker.scopes.push(Default::default());
                for p in &event.parameters {
                    checker
                        .scopes
                        .last_mut()
                        .unwrap()
                        .insert(p.name.clone(), (KnownApiType::Unknown, false));
                }
                checker.check_block(&event.body, false);
                checker.scopes.pop();
                let possible = checker.scopes.clone();
                checker.scopes = baseline.clone();
                checker.weaken_changed_outer_types(&baseline, &possible);
            }
            Declaration::Entity(entity) => {
                checker.scopes.push(Default::default());
                for member in &entity.members {
                    if let EntityMember::Field(field) = member {
                        if let Some(initializer) = &field.initializer {
                            checker.check_value(initializer);
                        }
                        let kind = field
                            .initializer
                            .as_ref()
                            .map(|value| checker.type_of(value))
                            .unwrap_or(KnownApiType::Unknown);
                        checker
                            .scopes
                            .last_mut()
                            .unwrap()
                            .insert(field.name.clone(), (kind, false));
                    }
                }
                for member in &entity.members {
                    if let EntityMember::Function(function) = member {
                        let baseline = checker.scopes.clone();
                        checker.scopes.push(Default::default());
                        for parameter in &function.parameters {
                            checker
                                .scopes
                                .last_mut()
                                .unwrap()
                                .insert(parameter.name.clone(), (KnownApiType::Unknown, false));
                        }
                        checker.check_block(&function.body, false);
                        checker.scopes.pop();
                        let possible = checker.scopes.clone();
                        checker.scopes = baseline.clone();
                        checker.weaken_changed_outer_types(&baseline, &possible);
                    }
                }
                checker.scopes.pop();
            }
            _ => {}
        }
    }
    checker.errors
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::lexer::Lexer;
    use crate::scripting::parser::Parser;

    #[test]
    fn bare_assignments_require_declarations_in_every_scope() {
        for source in [
            "PLAYER_FOLLOW_DISTANCE = 5.0\n",
            "fn move() {\n target = 5\n}\n",
            "count: number = 0\nfn update() {\n coutn = 1\n}\n",
            "fn update() {\n if true {\n hidden = 2\n }\n}\n",
            "entity Guard {\n health: number = 3\n fn update() {\n helth = 4\n }\n}\n",
            "fn update() {\n const callback = fn() {\n extra = 1\n }\n}\n",
        ] {
            let program = Parser::new(Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap();
            let diagnostics = strict_builtin_api_diagnostics(&program, false);
            assert!(
                diagnostics.iter().any(|diagnostic| {
                    diagnostic.severity == DiagnosticSeverity::Error
                        && diagnostic.message.contains("undeclared variable")
                }),
                "Expected undeclared assignment error: {source:?}, got {diagnostics:?}"
            );
        }
        for source in [
            "speed: number = 5\nspeed = 6\n",
            "speed: number = 0\nfn update() {\n speed = 5\n}\n",
            "fn update(speed: number) {\n speed = 10\n}\n",
            "entity Guard {\n health: number = 3\n fn update() {\n health = 4\n }\n}\n",
            "fn update() {\n current: number = 0\n if true {\n current = 2\n }\n}\n",
        ] {
            let program = Parser::new(Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap();
            let diagnostics = strict_builtin_api_diagnostics(&program, false);
            assert!(
                !diagnostics
                    .iter()
                    .any(|diagnostic| diagnostic.severity == DiagnosticSeverity::Error),
                "Valid reassignment rejected: {source:?}, got {diagnostics:?}"
            );
        }
    }
    #[test]
    fn strict_builtin_api_errors_are_source_backed_and_contextual() {
        let cases = [
            ("const items = [1, 2]\nitems.length\n", false, ".length"),
            (
                "const items = [1, 2]\nitems.insert()\n",
                false,
                "basket.insert",
            ),
            ("const n = math.random(10)\n", false, "math.random"),
            ("const n = math.random(5, 1)\n", false, "lower bound"),
            ("const n = math.sqrt(\"bad\")\n", false, "numeric"),
            ("const n = math.random(1.5, 3)\n", false, "integers"),
            ("const x = 3\nx = 4\n", false, "const variable"),
            (
                "const guard = self.parent\nself.set_position([0, 0, 0])\n",
                true,
                "not a Module API",
            ),
            (
                "const guard = self.parent\nconst p = self.position\n",
                true,
                "not an Actor property",
            ),
            (
                "const guard = self.parent\nguard.navigate_to([1, 2])\n",
                true,
                "3 coordinates",
            ),
            (
                "const guard = self.parent\nguard.navigate_to(\"bad\")\n",
                true,
                "[x, y, z]",
            ),
            (
                "const guard = self.parent\nguard.is_navigating(1)\n",
                true,
                "expects 0",
            ),
            ("save.commit(123)\n", false, "save.commit"),
            ("save.get(25)\n", false, "Save key"),
            ("save.set(\"points\", [1, 2])\n", false, "not baskets"),
            (
                "player.set_horizontal_velocity(2)\n",
                false,
                "player.set_horizontal_velocity",
            ),
            ("player.made_up_action()\n", false, "Unknown player API"),
            ("cell.new()\n", false, "cell.new"),
            ("const text = \"Hello\"\ntext.len(2)\n", false, "string.len"),
            ("time.elapsed(1)\n", false, "time.elapsed"),
            ("time.does_not_exist()\n", false, "Unknown time API"),
            ("debug.made_up()\n", false, "Unknown debug API"),
            ("cell.does_not_exist()\n", false, "Unknown cell API"),
            ("entity.does_not_exist()\n", false, "Unknown entity API"),
            ("math.pow(2)\n", false, "math.pow"),
            ("cell.new(42)\n", false, "cell.new expects a string"),
            ("cell.get(\"bad\")\n", false, "cell.get expects a number"),
            (
                "entity.get(\"bad\")\n",
                false,
                "entity.get expects a number",
            ),
            (
                "player.select_animation(12)\n",
                false,
                "player.select_animation expects a string",
            ),
            (
                "player.set_horizontal_velocity(\"fast\", 0)\n",
                false,
                "expects a number",
            ),
            (
                "const b = [1, 2]\nb.insert(\"index\", 3)\n",
                false,
                "basket.insert expects a number",
            ),
            (
                "const guard = self.parent\nguard.set_position(\"x\", 1, 2)\n",
                true,
                "Actor.set_position expects a number",
            ),
            (
                "const guard = self.parent\nguard.select_animation()\n",
                true,
                "Actor.select_animation",
            ),
            (
                "const guard = self.parent\nguard.set_horizontal_velocity(1)\n",
                true,
                "Actor.set_horizontal_velocity",
            ),
            (
                "const guard = self.parent\nguard.set_position([0, 0, 0])\n",
                true,
                "Actor.set_position",
            ),
            ("const n = 42\nn.move()\n", false, "Cannot call methods"),
            (
                "const n = 42\nn.position\n",
                false,
                "do not have object properties",
            ),
            ("const n = 42\nn()\n", false, "Cannot call a non-function"),
            (
                "const flag = true\nflag.toggle()\n",
                false,
                "Cannot call methods",
            ),
        ];
        for (source, attached, expected) in cases {
            let tokens = Lexer::new(source).tokenize().unwrap();
            let program = Parser::new(tokens).parse().unwrap();
            let errors = strict_builtin_api_diagnostics(&program, attached);
            assert!(
                errors.iter().any(|e| e.message.contains(expected)),
                "expected {expected} in source {source:?}; got {errors:?}"
            );
        }
        for (source, attached) in [
            ("const items = [1, 2]\nitems.len()\n", false),
            ("const n = math.random()\n", false),
            ("const n = math.random(1, 3)\n", false),
            ("debug.log()\n", false),
            ("debug.log(\"Actor\", 12, true)\n", false),
            ("time.elapsed()\n", false),
            ("cell.exists(1)\n", false),
            ("entity.get(1)\n", false),
            ("save.commit()\n", false),
            ("save.set(\"stage\", true)\n", false),
            ("player.is_grounded()\n", false),
            ("const here = player.position\n", false),
            ("const text = \"Hello\"\ntext.split(\",\")\n", false),
            ("const items = [1, 2]\nitems.insert(3)\nitems[2]\n", false),
            (
                "custom_player: Actor = get_custom_player()\ncustom_player.my_custom_action()\n",
                false,
            ),
            (
                "const guard = self.parent\nguard.navigate_to([0, 1, 2], 2.0, 0.35)\n",
                true,
            ),
            (
                "const guard = self.parent\nguard.set_position(1, 2, 3)\nguard.translate(1, 0, 0)\nguard.set_horizontal_velocity(2, 0)\nguard.set_facing_direction(0, 1)\nguard.select_animation(\"Walk\")\nguard.apply_vertical_impulse(4)\nguard.is_grounded()\nguard.name()\n",
                true,
            ),
            (
                "const custom = engine.custom_object\ncustom.special_method()\n",
                false,
            ),
        ] {
            let tokens = Lexer::new(source).tokenize().unwrap();
            let program = Parser::new(tokens).parse().unwrap();
            let errors = strict_builtin_api_diagnostics(&program, attached);
            assert!(
                errors.is_empty(),
                "valid/dynamic source {source:?} rejected: {errors:?}"
            );
        }
        for source in [
            "const items = [1, 2]\nitems.insert(3)\nitems[2]\n",
            "const items = [1, 2]\nconst alias = items\nalias.insert(3)\nitems[2]\n",
            "const items = [1, 2]\nfn extend(b) { b.insert(3) }\nextend(items)\nitems[2]\n",
            "const items = [1, 2]\nitems.concat()\nitems.concat(\",\", 1, 2)\n",
        ] {
            let program = Parser::new(Lexer::new(source).tokenize().unwrap())
                .parse()
                .unwrap();
            let diagnostics = strict_builtin_api_diagnostics(&program, false);
            assert!(
                !diagnostics
                    .iter()
                    .any(|d| d.severity == DiagnosticSeverity::Error),
                "dynamic basket operation rejected: {source:?} -> {diagnostics:?}"
            );
        }
        let custom = "const guard = self.parent\nguard.custom_action()\n";
        let program = Parser::new(Lexer::new(custom).tokenize().unwrap())
            .parse()
            .unwrap();
        let diagnostics = strict_builtin_api_diagnostics(&program, true);
        assert!(diagnostics.iter().any(|d| {
            d.severity == DiagnosticSeverity::Warning
                && d.message.contains("script-defined handler")
        }));
        assert!(
            !diagnostics
                .iter()
                .any(|d| d.severity == DiagnosticSeverity::Error)
        );
    }
    fn analyze(source: &str, modules: &[&str]) -> Vec<SemanticDiagnostic> {
        let tokens = Lexer::new(source).tokenize().unwrap();
        let program = Parser::new(tokens).parse().unwrap();
        analyze_program(
            &program,
            &modules.iter().map(|path| path.to_string()).collect(),
        )
    }

    #[test]
    fn duplicate_same_scope_variable_is_error_but_nested_shadowing_is_valid() {
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
