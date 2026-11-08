// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::source::SourceSpan;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    Entity,
    Fn,
    Return,
    If,
    Else,
    While,
    For,
    In,
    Const,
    Import,

    Number(f64),
    String(String),
    Identifier(String),

    True,
    False,
    Nil,
    On,

    Plus,
    Minus,
    Star,
    Slash,
    Percent,

    Equal,
    EqualEqual,
    Bang,
    BangEqual,

    Less,
    LessEqual,
    Greater,
    GreaterEqual,

    AndAnd,
    OrOr,

    PlusEqual,
    MinusEqual,
    StarEqual,
    SlashEqual,

    LeftBrace,
    RightBrace,
    LeftParen,
    RightParen,
    LeftBracket,
    RightBracket,

    Comma,
    Dot,
    Colon,
    QuestionMark,

    Newline,

    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,

    pub span: SourceSpan,

    pub line: usize,
    pub column: usize,
}

impl Token {
    pub fn new(kind: TokenKind, span: SourceSpan, line: usize, column: usize) -> Self {
        Self {
            kind,
            span,
            line,
            column,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::source::SourceSpan;

    #[test]
    fn token_preserves_source_span() {
        let token = Token::new(
            TokenKind::Identifier("player".to_string()),
            SourceSpan::new(10, 16),
            2,
            5,
        );

        assert_eq!(token.span, SourceSpan::new(10, 16));
        assert_eq!(token.line, 2);
        assert_eq!(token.column, 5);
    }
}
