#[cfg(test)]
mod tests {
    use super::super::*;
    use crate::scripting::ast::{
        BinaryOperator, Declaration, EntityMember, ExpressionKind, StatementKind, TypeKind,
    };
    use crate::scripting::lexer::Lexer;

    fn parse(source: &str) -> Program {
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        Parser::new(tokens).parse().expect("parser should succeed")
    }

    #[test]
    fn parses_basic_entity() {
        let program = parse(
            r#"
entity Door {

    open: bool = false

    fn update(dt: number) {

        if open {
            print("open")
        }
    }
}
"#,
        );

        assert_eq!(program.declarations.len(), 1);

        let Declaration::Entity(entity) = &program.declarations[0] else {
            panic!("expected entity");
        };

        assert_eq!(entity.name, "Door");
        assert_eq!(entity.members.len(), 2);
        assert!(!entity.span.is_empty());
        assert!(!program.span.is_empty());
    }

    #[test]
    fn parses_function_parameters_and_return_type() {
        let program = parse(
            r#"
fn add(a: number, b: number): number {
    return a + b
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        assert_eq!(function.name, "add");
        assert_eq!(function.parameters.len(), 2);

        assert_eq!(
            function
                .return_type
                .as_ref()
                .map(|return_type| &return_type.kind),
            Some(&TypeKind::Named("number".to_string()))
        );

        assert!(
            function
                .parameters
                .iter()
                .all(|parameter| !parameter.span.is_empty())
        );
    }

    #[test]
    fn parses_expression_precedence() {
        let program = parse(
            r#"
entity Test {

    value: number = 1 + 2 * 3
}
"#,
        );

        let Declaration::Entity(entity) = &program.declarations[0] else {
            panic!("expected entity");
        };

        let EntityMember::Field(field) = &entity.members[0] else {
            panic!("expected field");
        };

        let Some(expression) = &field.initializer else {
            panic!("expected initializer");
        };

        assert!(matches!(
            expression.kind,
            ExpressionKind::Binary {
                operator: BinaryOperator::Add,
                ..
            }
        ));

        assert!(field.span.start <= expression.span.start);

        assert!(expression.span.start < expression.span.end);

        assert_eq!(field.span.end, expression.span.end);
    }

    #[test]
    fn parses_calls_and_member_access() {
        let program = parse(
            r#"
entity Test {

    fn update(dt: number) {
        character.jump()
        transform.move_forward(dt)
    }
}

"#,
        );

        let Declaration::Entity(entity) = &program.declarations[0] else {
            panic!("expected entity");
        };

        let EntityMember::Function(function) = &entity.members[0] else {
            panic!("expected function");
        };

        assert_eq!(function.body.statements.len(), 2);

        assert!(
            function
                .body
                .statements
                .iter()
                .all(|statement| { !statement.span.is_empty() })
        );
    }

    #[test]
    fn parses_multiline_initializer_after_equal() {
        let program = parse(
            r#"
fn test() {
    const value =
        10
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        assert_eq!(function.body.statements.len(), 1);

        let StatementKind::Variable { initializer, .. } = &function.body.statements[0].kind else {
            panic!("expected variable declaration");
        };

        assert!(initializer.is_some());
    }

    #[test]
    fn parses_multiline_binary_expression() {
        let program = parse(
            r#"
fn test() {
    const value =
        10 +
        20 *
        2
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let StatementKind::Variable { initializer, .. } = &function.body.statements[0].kind else {
            panic!("expected variable declaration");
        };

        let Some(expression) = initializer else {
            panic!("expected initializer");
        };

        assert!(matches!(
            expression.kind,
            ExpressionKind::Binary {
                operator: BinaryOperator::Add,
                ..
            }
        ));
    }

    #[test]
    fn parses_multiline_member_expression() {
        let program = parse(
            r#"
fn test() {
    const value =
        player
            .position
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let StatementKind::Variable { initializer, .. } = &function.body.statements[0].kind else {
            panic!("expected variable declaration");
        };

        let Some(expression) = initializer else {
            panic!("expected initializer");
        };

        assert!(matches!(expression.kind, ExpressionKind::Member { .. }));
    }

    #[test]
    fn parses_multiline_assignment() {
        let program = parse(
            r#"
fn test() {
    value: number = 10

    value =
        20 +
        5
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        assert_eq!(function.body.statements.len(), 2);

        assert!(matches!(
            function.body.statements[1].kind,
            StatementKind::Assignment { .. }
        ));
    }

    #[test]
    fn preserves_statement_boundaries() {
        let program = parse(
            r#"
const first = 10
const second = 20

debug.log(first)
debug.log(second)
"#,
        );

        assert_eq!(program.statements.len(), 4);
    }

    #[test]
    fn parses_multiline_function_call() {
        let program = parse(
            r#"
fn test() {
    const value =
        math.sqrt(
            16
        )
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let StatementKind::Variable { initializer, .. } = &function.body.statements[0].kind else {
            panic!("expected variable declaration");
        };

        let Some(expression) = initializer else {
            panic!("expected initializer");
        };

        assert!(matches!(expression.kind, ExpressionKind::Call { .. }));
    }

    #[test]
    fn parses_entity_namespace_calls() {
        let program = parse(
            r#"
    fn test() {
        const exists =
            entity.exists(10)

        const player =
            entity.get(10)
    }
    "#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        assert_eq!(function.body.statements.len(), 2);

        let StatementKind::Variable { initializer, .. } = &function.body.statements[0].kind else {
            panic!("expected variable declaration");
        };

        let Some(expression) = initializer else {
            panic!("expected initializer");
        };

        assert!(matches!(expression.kind, ExpressionKind::Call { .. }));
    }

    #[test]
    fn parses_for_loop() {
        let program = parse(
            r#"
entity Test {

    fn update(dt: number) {

        for enemy in enemies {
            enemy.highlight()
        }
    }
}
"#,
        );

        let Declaration::Entity(entity) = &program.declarations[0] else {
            panic!("expected entity");
        };

        let EntityMember::Function(function) = &entity.members[0] else {
            panic!("expected function");
        };

        assert!(matches!(
            function.body.statements[0].kind,
            StatementKind::For { .. }
        ));

        assert!(!function.body.statements[0].span.is_empty());
    }

    #[test]
    fn parses_import() {
        let program = parse(
            r#"
import Shared.Math
"#,
        );

        let Declaration::Import(import) = &program.declarations[0] else {
            panic!("expected import");
        };

        assert_eq!(import.path, vec!["Shared".to_string(), "Math".to_string()]);

        assert!(!import.span.is_empty());
    }

    #[test]
    fn parser_errors_keep_source_spans() {
        let source = "entity Test {";
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let error = Parser::new(tokens).parse().expect_err("parser should fail");

        assert_eq!(error.line, 1);
        assert!(error.column > 0);
        assert!(error.span.start <= error.span.end);
    }

    #[test]
    fn optional_types_keep_their_full_span() {
        let source = r#"
fn find_player(): Entity? {
    return nil
}
"#;

        let program = parse(source);

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let return_type = function.return_type.as_ref().expect("return type");

        assert!(matches!(return_type.kind, TypeKind::Optional(_)));

        assert_eq!(return_type.span.len(), "Entity?".len() as u32);
    }

    #[test]
    fn parses_mutable_local_variable() {
        let program = parse(
            r#"
fn test() {
    value: number = 10
    value += 5
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let StatementKind::Variable {
            name,
            type_annotation,
            initializer,
            is_const,
        } = &function.body.statements[0].kind
        else {
            panic!("expected variable declaration");
        };

        assert_eq!(name, "value");
        assert!(type_annotation.is_some());
        assert!(initializer.is_some());
        assert!(!is_const);
    }

    #[test]
    fn parses_const_local_variable() {
        let program = parse(
            r#"
fn test() {
    const limit: number = 100
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let StatementKind::Variable { name, is_const, .. } = &function.body.statements[0].kind
        else {
            panic!("expected variable declaration");
        };

        assert_eq!(name, "limit");
        assert!(is_const);
    }

    #[test]
    fn parses_light_style_script() {
        let program = parse(
            r#"
entity LightBlinker {

    light: Light?

    fn update(dt: number) {
        light = get_light(10, 5, 3)

        if light {
            light:set_enabled(true)
        }
    }
}
"#,
        );

        let Declaration::Entity(entity) = &program.declarations[0] else {
            panic!("expected entity");
        };

        assert_eq!(entity.members.len(), 2);

        let EntityMember::Field(field) = &entity.members[0] else {
            panic!("expected field");
        };

        assert!(matches!(
            field
                .type_annotation
                .as_ref()
                .map(|type_annotation| &type_annotation.kind),
            Some(TypeKind::Optional(_))
        ));

        let EntityMember::Function(function) = &entity.members[1] else {
            panic!("expected function");
        };

        assert_eq!(function.body.statements.len(), 2);
    }

    #[test]
    fn test_top_level_statements() {
        let source = r#"
debug.log("hello")
const x = 10
if x > 0 {
    debug.log("positive")
}
"#;
        let program = parse(source);
        assert_eq!(program.statements.len(), 3);
        assert_eq!(program.declarations.len(), 0);

        match &program.statements[0].kind {
            StatementKind::Expression(expr) => {
                assert!(matches!(expr.kind, ExpressionKind::Call { .. }));
            }
            _ => panic!("expected expression statement"),
        }
    }

    #[test]
    fn test_mixed_top_level() {
        let source = r#"
debug.log("start")
fn helper() {}
debug.log("end")
entity Test {}
"#;
        let program = parse(source);
        assert_eq!(program.statements.len(), 2);
        assert_eq!(program.declarations.len(), 2);
    }

    #[test]
    fn parses_namespaced_event() {
        let program = parse(
            r#"
on.keypress(key: string, down: bool) {
    debug.log(key)
}
"#,
        );

        let Declaration::Event(event) = &program.declarations[0] else {
            panic!("expected event");
        };

        assert_eq!(event.name, "keypress");
        assert_eq!(event.parameters.len(), 2);
    }

    #[test]
    fn parses_multi_segment_namespaced_event() {
        let program = parse(
            r#"
on.input.keypress(key: string, down: bool) {
    debug.log(key)
}
"#,
        );

        let Declaration::Event(event) = &program.declarations[0] else {
            panic!("expected event");
        };

        assert_eq!(event.name, "input.keypress");
    }

    #[test]
    fn parses_trailing_commas_everywhere() {
        let program = parse(
            r#"
fn test(
    first: number,
    second: number,
) {
    const values = [
        1,
        2,
        3,
    ]

    const map = {
        one: 1,
        two: 2,
    }

    const callback = fn(
        value: number,
    ) {
        return value
    }

    debug.log(
        values[0],
        map["one"],
    )
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        assert_eq!(function.parameters.len(), 2);
        assert_eq!(function.body.statements.len(), 4);
    }

    #[test]
    fn parses_newlines_inside_indexes_and_types() {
        let program = parse(
            r#"
fn test(
    value:
        number,
) {
    const list = [
        1,
        2,
    ]

    const selected =
        list[
            0
        ]
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        assert_eq!(function.parameters.len(), 1);
        assert_eq!(function.body.statements.len(), 2);
    }

    #[test]
    fn parses_namespaced_types() {
        let program = parse(
            r#"
fn find(): ui.Panel? {
    return nil
}
"#,
        );

        let Declaration::Function(function) = &program.declarations[0] else {
            panic!("expected function");
        };

        let return_type = function.return_type.as_ref().expect("return type");
        let TypeKind::Optional(inner) = &return_type.kind else {
            panic!("expected optional return type");
        };

        assert!(matches!(
            &inner.kind,
            TypeKind::Named(name) if name == "ui.Panel"
        ));
    }

    #[test]
    fn rejects_invalid_assignment_target() {
        let source = "fn test() {\n    10 = 20\n}";
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let error = Parser::new(tokens)
            .parse()
            .expect_err("parser should reject literal assignment");

        assert!(error.message.contains("Invalid assignment target"));
        assert_eq!(error.line, 2);
    }

    #[test]
    fn parser_errors_report_found_token() {
        let source = "fn test( {\n}";
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let error = Parser::new(tokens).parse().expect_err("parser should fail");

        assert!(error.message.contains("Expected parameter name"));
        assert!(error.message.contains("Found '{'"));
        assert_eq!(error.line, 1);
    }

    #[test]
    fn test_parse_checked_in_hearthvale_scripts() {
        // Parser fixtures belong under testdata; release staging is never test input.
        let scripts_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("src/scripting/testdata/hearthvale/scripts/systems");

        for entry in std::fs::read_dir(scripts_dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_file() && path.extension().map_or(false, |e| e == "aeo") {
                let source = std::fs::read_to_string(&path).unwrap();
                let lexer = Lexer::new(&source);
                let tokens = lexer
                    .tokenize()
                    .expect(&format!("Lexer error in {:?}", path));
                let parser = Parser::new(tokens);
                let _program = parser
                    .parse()
                    .expect(&format!("Parser error in {:?}", path));
            }
        }
    }
}
