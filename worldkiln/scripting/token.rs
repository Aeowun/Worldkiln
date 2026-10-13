use super::source::SourceSpan;

#[derive(Clone, Debug, PartialEq)]
pub enum TokenKind {
    // Keywords
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

    // Literals
    Number(f64),
    String(String),
    Identifier(String),

    True,
    False,
    Nil,
    On,

    // Operators
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

    // Punctuation
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

    // Line termination
    Newline,

    Eof,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Token {
    pub kind: TokenKind,

    /// Compact byte-offset span in the original source.
    pub span: SourceSpan,

    /// Kept directly on tokens so parser errors can report immediately
    /// without maintaining a separate line map during parsing.
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
