use crate::scripting::ast::*;
use crate::scripting::source::SourceSpan;
use crate::scripting::token::TokenKind;

use super::{Parser, ParserError};

impl Parser {
    pub(super) fn parse_declaration(&mut self) -> Result<Declaration, ParserError> {
        match self.peek_kind() {
            TokenKind::Entity => self.parse_entity().map(Declaration::Entity),

            TokenKind::Fn => self.parse_function().map(Declaration::Function),

            TokenKind::Import => self.parse_import().map(Declaration::Import),

            TokenKind::On => self.parse_event().map(Declaration::Event),

            _ => Err(self.error_expected("Expected a declaration.")),
        }
    }

    pub(super) fn parse_event(&mut self) -> Result<EventDecl, ParserError> {
        let on_token = self.consume_simple(TokenKind::On, "Expected 'on'.")?;
        let start = on_token.span.start;

        self.skip_newlines();

        let mut name = if self.match_simple(&TokenKind::Dot) {
            let (segment, _) = self.consume_identifier("Expected event name after 'on.'.")?;
            segment
        } else {
            let (segment, _) = self.consume_identifier("Expected event name.")?;
            segment
        };

        loop {
            if !self.match_expression_continuation(&TokenKind::Dot) {
                break;
            }

            let (segment, _) = self.consume_identifier("Expected event name segment after '.'.")?;
            name.push('.');
            name.push_str(&segment);
        }

        self.consume_simple(TokenKind::LeftParen, "Expected '(' after event name.")?;

        let parameters = self.parse_parameter_list()?;

        self.skip_newlines();

        let body = self.parse_block()?;

        Ok(EventDecl {
            span: SourceSpan::new(start, body.span.end),
            name,
            parameters,
            body,
        })
    }

    pub(super) fn parse_entity(&mut self) -> Result<EntityDecl, ParserError> {
        let entity_token = self.consume_simple(TokenKind::Entity, "Expected 'entity'.")?;

        let start = entity_token.span.start;

        let (name, _) = self.consume_identifier("Expected entity name.")?;

        self.skip_newlines();

        self.consume_simple(TokenKind::LeftBrace, "Expected '{' after entity name.")?;

        let mut members = Vec::new();

        self.skip_newlines();

        while !self.check_simple(&TokenKind::RightBrace) && !self.is_at_end() {
            if self.check_simple(&TokenKind::Fn) {
                members.push(EntityMember::Function(self.parse_function()?));
            } else {
                members.push(EntityMember::Field(self.parse_field()?));
            }

            self.skip_newlines();
        }

        let right_brace =
            self.consume_simple(TokenKind::RightBrace, "Expected '}' after entity body.")?;

        Ok(EntityDecl {
            span: SourceSpan::new(start, right_brace.span.end),
            name,
            members,
        })
    }

    pub(super) fn parse_field(&mut self) -> Result<FieldDecl, ParserError> {
        let (name, name_span) = self.consume_identifier("Expected field name.")?;

        let type_annotation = if self.match_expression_continuation(&TokenKind::Colon) {
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };

        let initializer = if self.match_expression_continuation(&TokenKind::Equal) {
            // A newline after '=' continues the initializer.
            self.skip_newlines();

            Some(self.parse_expression()?)
        } else {
            None
        };

        if type_annotation.is_none() && initializer.is_none() {
            return Err(self.error_expected("A field needs a type, an initializer, or both."));
        }

        self.consume_statement_end();

        let end = initializer
            .as_ref()
            .map(|expression| expression.span.end)
            .or_else(|| {
                type_annotation
                    .as_ref()
                    .map(|type_annotation| type_annotation.span.end)
            })
            .unwrap_or(name_span.end);

        Ok(FieldDecl {
            span: SourceSpan::new(name_span.start, end),
            name,
            type_annotation,
            initializer,
        })
    }

    pub(super) fn parse_parameter_list(&mut self) -> Result<Vec<Parameter>, ParserError> {
        let mut parameters = Vec::new();

        self.skip_newlines();

        if !self.check_simple(&TokenKind::RightParen) {
            loop {
                let (parameter_name, parameter_name_span) =
                    self.consume_identifier("Expected parameter name.")?;

                let type_annotation = if self.match_expression_continuation(&TokenKind::Colon) {
                    self.skip_newlines();
                    Some(self.parse_type()?)
                } else {
                    None
                };

                let parameter_end = type_annotation
                    .as_ref()
                    .map(|type_annotation| type_annotation.span.end)
                    .unwrap_or(parameter_name_span.end);

                parameters.push(Parameter {
                    span: SourceSpan::new(parameter_name_span.start, parameter_end),
                    name: parameter_name,
                    type_annotation,
                });

                self.skip_newlines();

                if !self.match_simple(&TokenKind::Comma) {
                    break;
                }

                self.skip_newlines();

                if self.check_simple(&TokenKind::RightParen) {
                    break;
                }
            }
        }

        self.consume_simple(TokenKind::RightParen, "Expected ')' after parameters.")?;

        Ok(parameters)
    }

    pub(super) fn parse_function(&mut self) -> Result<FunctionDecl, ParserError> {
        let fn_token = self.consume_simple(TokenKind::Fn, "Expected 'fn'.")?;

        let start = fn_token.span.start;

        self.skip_newlines();
        let (name, _) = self.consume_identifier("Expected function name.")?;

        self.consume_simple(TokenKind::LeftParen, "Expected '(' after function name.")?;

        let parameters = self.parse_parameter_list()?;

        self.skip_newlines();

        let return_type = if self.match_expression_continuation(&TokenKind::Colon) {
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };

        self.skip_newlines();

        let body = self.parse_block()?;

        Ok(FunctionDecl {
            span: SourceSpan::new(start, body.span.end),
            name,
            parameters,
            return_type,
            body,
        })
    }

    pub(super) fn parse_import(&mut self) -> Result<ImportDecl, ParserError> {
        let import_token = self.consume_simple(TokenKind::Import, "Expected 'import'.")?;

        let start = import_token.span.start;

        let (first, first_span) = self.consume_identifier("Expected import name.")?;

        let mut path = vec![first];
        let mut end = first_span.end;

        while self.match_simple(&TokenKind::Dot) {
            let (part, part_span) = self.consume_identifier("Expected identifier after '.'.")?;

            end = part_span.end;
            path.push(part);
        }

        self.consume_statement_end();

        Ok(ImportDecl {
            span: SourceSpan::new(start, end),
            path,
        })
    }

    pub(super) fn parse_type(&mut self) -> Result<Type, ParserError> {
        self.skip_newlines();

        let (name, name_span) = self.consume_identifier("Expected type name.")?;
        let mut name = name;
        let mut end = name_span.end;

        loop {
            if !self.match_expression_continuation(&TokenKind::Dot) {
                break;
            }

            let (segment, segment_span) =
                self.consume_identifier("Expected type name segment after '.'.")?;
            name.push('.');
            name.push_str(&segment);
            end = segment_span.end;
        }

        let base = Type {
            span: SourceSpan::new(name_span.start, end),
            kind: TypeKind::Named(name),
        };

        if let Some(question) = self.match_simple_return(&TokenKind::QuestionMark) {
            return Ok(Type {
                span: SourceSpan::new(name_span.start, question.span.end),
                kind: TypeKind::Optional(Box::new(base)),
            });
        }

        Ok(base)
    }
}
