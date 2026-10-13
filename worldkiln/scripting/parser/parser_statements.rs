use crate::scripting::ast::*;
use crate::scripting::source::SourceSpan;
use crate::scripting::token::TokenKind;

use super::{Parser, ParserError};

impl Parser {
    pub(super) fn parse_block(&mut self) -> Result<Block, ParserError> {
        self.skip_newlines();

        let left_brace = self.consume_simple(TokenKind::LeftBrace, "Expected '{'.")?;

        let start = left_brace.span.start;

        let mut statements = Vec::new();

        self.skip_newlines();

        while !self.check_simple(&TokenKind::RightBrace) && !self.is_at_end() {
            statements.push(self.parse_statement()?);

            self.skip_newlines();
        }

        let right_brace =
            self.consume_simple(TokenKind::RightBrace, "Expected '}' after block.")?;

        Ok(Block {
            span: SourceSpan::new(start, right_brace.span.end),
            statements,
        })
    }

    pub(super) fn parse_statement(&mut self) -> Result<Statement, ParserError> {
        match self.peek_kind() {
            TokenKind::Const => self.parse_variable(true),

            TokenKind::Identifier(_) => {
                if self.peek_next_simple(&TokenKind::Colon) && !self.is_method_call_peek() {
                    self.parse_variable(false)
                } else {
                    self.parse_expression_or_assignment()
                }
            }

            TokenKind::If => self.parse_if(),

            TokenKind::While => self.parse_while(),

            TokenKind::For => self.parse_for(),

            TokenKind::Return => self.parse_return(),

            _ => self.parse_expression_or_assignment(),
        }
    }

    fn parse_variable(&mut self, is_const: bool) -> Result<Statement, ParserError> {
        let start = if is_const {
            self.consume_simple(TokenKind::Const, "Expected 'const'.")?
                .span
                .start
        } else {
            self.peek().span.start
        };

        let (name, name_span) = self.consume_identifier("Expected variable name.")?;

        let type_annotation = if self.match_expression_continuation(&TokenKind::Colon) {
            self.skip_newlines();
            Some(self.parse_type()?)
        } else {
            None
        };

        let initializer = if self.match_expression_continuation(&TokenKind::Equal) {
            // A newline immediately after '=' is part of the expression.
            self.skip_newlines();

            Some(self.parse_expression()?)
        } else {
            None
        };

        if type_annotation.is_none() && initializer.is_none() {
            return Err(self.error_expected("A variable needs a type, an initializer, or both."));
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

        Ok(Statement {
            span: SourceSpan::new(start, end),
            kind: StatementKind::Variable {
                name,
                type_annotation,
                initializer,
                is_const,
            },
        })
    }

    fn parse_if(&mut self) -> Result<Statement, ParserError> {
        let if_token = self.consume_simple(TokenKind::If, "Expected 'if'.")?;

        let start = if_token.span.start;

        self.skip_newlines();

        let condition = self.parse_expression()?;
        let then_block = self.parse_block()?;

        let mut else_if = Vec::new();
        let mut else_block = None;
        let mut end = then_block.span.end;

        loop {
            self.skip_newlines();

            if !self.match_simple(&TokenKind::Else) {
                break;
            }

            self.skip_newlines();

            if self.match_simple(&TokenKind::If) {
                self.skip_newlines();

                let condition = self.parse_expression()?;

                let block = self.parse_block()?;
                end = block.span.end;

                else_if.push((condition, block));
            } else {
                let block = self.parse_block()?;
                end = block.span.end;
                else_block = Some(block);
                break;
            }
        }

        Ok(Statement {
            span: SourceSpan::new(start, end),
            kind: StatementKind::If {
                condition,
                then_block,
                else_if,
                else_block,
            },
        })
    }

    fn parse_while(&mut self) -> Result<Statement, ParserError> {
        let while_token = self.consume_simple(TokenKind::While, "Expected 'while'.")?;

        self.skip_newlines();

        let condition = self.parse_expression()?;
        let body = self.parse_block()?;

        Ok(Statement {
            span: SourceSpan::new(while_token.span.start, body.span.end),
            kind: StatementKind::While { condition, body },
        })
    }

    fn parse_for(&mut self) -> Result<Statement, ParserError> {
        let for_token = self.consume_simple(TokenKind::For, "Expected 'for'.")?;

        self.skip_newlines();
        let (name, _) = self.consume_identifier("Expected loop variable name.")?;

        self.skip_newlines();
        self.consume_simple(TokenKind::In, "Expected 'in' after loop variable.")?;

        self.skip_newlines();

        let iterable = self.parse_expression()?;
        let body = self.parse_block()?;

        Ok(Statement {
            span: SourceSpan::new(for_token.span.start, body.span.end),
            kind: StatementKind::For {
                name,
                iterable,
                body,
            },
        })
    }

    fn parse_return(&mut self) -> Result<Statement, ParserError> {
        let return_token = self.consume_simple(TokenKind::Return, "Expected 'return'.")?;

        let start = return_token.span.start;

        if self.check_simple(&TokenKind::Newline)
            || self.check_simple(&TokenKind::RightBrace)
            || self.check_simple(&TokenKind::Eof)
        {
            self.consume_statement_end();

            return Ok(Statement {
                span: return_token.span,
                kind: StatementKind::Return(None),
            });
        }

        let value = self.parse_expression()?;

        self.consume_statement_end();

        Ok(Statement {
            span: SourceSpan::new(start, value.span.end),
            kind: StatementKind::Return(Some(value)),
        })
    }

    fn parse_expression_or_assignment(&mut self) -> Result<Statement, ParserError> {
        let expression = self.parse_expression()?;

        let operator = if self.match_expression_continuation(&TokenKind::Equal) {
            Some(AssignmentOperator::Assign)
        } else if self.match_expression_continuation(&TokenKind::PlusEqual) {
            Some(AssignmentOperator::Add)
        } else if self.match_expression_continuation(&TokenKind::MinusEqual) {
            Some(AssignmentOperator::Subtract)
        } else if self.match_expression_continuation(&TokenKind::StarEqual) {
            Some(AssignmentOperator::Multiply)
        } else if self.match_expression_continuation(&TokenKind::SlashEqual) {
            Some(AssignmentOperator::Divide)
        } else {
            None
        };

        if let Some(operator) = operator {
            if !matches!(
                expression.kind,
                ExpressionKind::Identifier(_)
                    | ExpressionKind::Member { .. }
                    | ExpressionKind::Index { .. }
            ) {
                return Err(self.error_span(
                    "Invalid assignment target. Expected a variable, property, or index expression.",
                    expression.span,
                ));
            }

            // A newline after an assignment operator continues the RHS.
            self.skip_newlines();

            let value = self.parse_expression()?;

            self.consume_statement_end();

            let span = SourceSpan::new(expression.span.start, value.span.end);

            return Ok(Statement {
                span,
                kind: StatementKind::Assignment {
                    target: expression,
                    operator,
                    value,
                },
            });
        }

        self.consume_statement_end();

        let span = expression.span;

        Ok(Statement {
            span,
            kind: StatementKind::Expression(expression),
        })
    }
}
