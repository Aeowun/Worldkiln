use super::source::SourceSpan;
use super::token::{Token, TokenKind};

#[derive(Clone, Debug, PartialEq)]
pub struct LexerError {
    pub message: String,
    pub line: usize,
    pub column: usize,
}

impl LexerError {
    fn new(message: impl Into<String>, line: usize, column: usize) -> Self {
        Self {
            message: message.into(),
            line,
            column,
        }
    }
}

pub struct Lexer<'a> {
    source: &'a [u8],
    position: usize,
    line: usize,
    column: usize,
}

impl<'a> Lexer<'a> {
    pub fn new(source: &'a str) -> Self {
        Self {
            source: source.as_bytes(),
            position: 0,
            line: 1,
            column: 1,
        }
    }

    pub fn tokenize(mut self) -> Result<Vec<Token>, LexerError> {
        let mut tokens = Vec::new();

        loop {
            let token = self.next_token()?;
            let is_eof = matches!(token.kind, TokenKind::Eof);

            tokens.push(token);

            if is_eof {
                break;
            }
        }

        Ok(tokens)
    }

    fn next_token(&mut self) -> Result<Token, LexerError> {
        self.skip_spaces_and_comments();

        let line = self.line;
        let column = self.column;
        let start = self.position;

        let byte = match self.peek() {
            Some(byte) => byte,
            None => {
                return Ok(Token::new(
                    TokenKind::Eof,
                    SourceSpan::single(self.position as u32),
                    line,
                    column,
                ));
            }
        };

        match byte {
            b'\n' => {
                self.advance();

                Ok(self.token(TokenKind::Newline, start, line, column))
            }

            b'{' => {
                self.advance();

                Ok(self.token(TokenKind::LeftBrace, start, line, column))
            }

            b'}' => {
                self.advance();

                Ok(self.token(TokenKind::RightBrace, start, line, column))
            }

            b'(' => {
                self.advance();

                Ok(self.token(TokenKind::LeftParen, start, line, column))
            }

            b')' => {
                self.advance();

                Ok(self.token(TokenKind::RightParen, start, line, column))
            }

            b'[' => {
                self.advance();

                Ok(self.token(TokenKind::LeftBracket, start, line, column))
            }

            b']' => {
                self.advance();

                Ok(self.token(TokenKind::RightBracket, start, line, column))
            }

            b',' => {
                self.advance();

                Ok(self.token(TokenKind::Comma, start, line, column))
            }

            b'.' => {
                self.advance();

                Ok(self.token(TokenKind::Dot, start, line, column))
            }

            b':' => {
                self.advance();

                Ok(self.token(TokenKind::Colon, start, line, column))
            }

            b'?' => {
                self.advance();

                Ok(self.token(TokenKind::QuestionMark, start, line, column))
            }

            b'+' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::PlusEqual
                } else {
                    TokenKind::Plus
                };

                Ok(self.token(kind, start, line, column))
            }

            b'-' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::MinusEqual
                } else {
                    TokenKind::Minus
                };

                Ok(self.token(kind, start, line, column))
            }

            b'*' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::StarEqual
                } else {
                    TokenKind::Star
                };

                Ok(self.token(kind, start, line, column))
            }

            b'/' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::SlashEqual
                } else {
                    TokenKind::Slash
                };

                Ok(self.token(kind, start, line, column))
            }

            b'%' => {
                self.advance();

                Ok(self.token(TokenKind::Percent, start, line, column))
            }

            b'=' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::EqualEqual
                } else {
                    TokenKind::Equal
                };

                Ok(self.token(kind, start, line, column))
            }

            b'!' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::BangEqual
                } else {
                    TokenKind::Bang
                };

                Ok(self.token(kind, start, line, column))
            }

            b'<' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::LessEqual
                } else {
                    TokenKind::Less
                };

                Ok(self.token(kind, start, line, column))
            }

            b'>' => {
                self.advance();

                let kind = if self.match_byte(b'=') {
                    TokenKind::GreaterEqual
                } else {
                    TokenKind::Greater
                };

                Ok(self.token(kind, start, line, column))
            }

            b'&' => {
                self.advance();

                if self.match_byte(b'&') {
                    Ok(self.token(TokenKind::AndAnd, start, line, column))
                } else {
                    Err(LexerError::new(
                        "Unexpected '&'. Use '&&' for logical AND.",
                        line,
                        column,
                    ))
                }
            }

            b'|' => {
                self.advance();

                if self.match_byte(b'|') {
                    Ok(self.token(TokenKind::OrOr, start, line, column))
                } else {
                    Err(LexerError::new(
                        "Unexpected '|'. Use '||' for logical OR.",
                        line,
                        column,
                    ))
                }
            }

            b'"' => self.lex_string(),

            b'0'..=b'9' => self.lex_number(),

            b'a'..=b'z' | b'A'..=b'Z' | b'_' => self.lex_identifier(),

            _ => {
                let character = byte as char;
                self.advance();

                Err(LexerError::new(
                    format!("Unexpected character '{}'.", character),
                    line,
                    column,
                ))
            }
        }
    }

    fn lex_identifier(&mut self) -> Result<Token, LexerError> {
        let line = self.line;
        let column = self.column;
        let start = self.position;

        while let Some(byte) = self.peek() {
            if is_identifier_continue(byte) {
                self.advance();
            } else {
                break;
            }
        }

        let text = std::str::from_utf8(&self.source[start..self.position])
            .map_err(|_| LexerError::new("Invalid UTF-8 in identifier.", line, column))?;

        let kind = match text {
            "entity" => TokenKind::Entity,
            "fn" => TokenKind::Fn,
            "return" => TokenKind::Return,
            "if" => TokenKind::If,
            "else" => TokenKind::Else,
            "while" => TokenKind::While,
            "for" => TokenKind::For,
            "in" => TokenKind::In,
            "const" => TokenKind::Const,
            "import" => TokenKind::Import,
            "true" => TokenKind::True,
            "false" => TokenKind::False,
            "nil" => TokenKind::Nil,
            "on" => TokenKind::On,
            _ => TokenKind::Identifier(text.to_string()),
        };

        Ok(self.token(kind, start, line, column))
    }

    fn lex_number(&mut self) -> Result<Token, LexerError> {
        let line = self.line;
        let column = self.column;
        let start = self.position;

        while matches!(self.peek(), Some(b'0'..=b'9')) {
            self.advance();
        }

        if self.peek() == Some(b'.') {
            if matches!(self.peek_next(), Some(b'0'..=b'9')) {
                self.advance();

                while matches!(self.peek(), Some(b'0'..=b'9')) {
                    self.advance();
                }
            }
        }

        let text = std::str::from_utf8(&self.source[start..self.position])
            .map_err(|_| LexerError::new("Invalid number literal.", line, column))?;

        let value = text
            .parse::<f64>()
            .map_err(|_| LexerError::new("Invalid number literal.", line, column))?;

        Ok(self.token(TokenKind::Number(value), start, line, column))
    }

    fn lex_string(&mut self) -> Result<Token, LexerError> {
        let line = self.line;
        let column = self.column;
        let start = self.position;

        self.advance();

        let mut value = String::new();

        loop {
            let byte = match self.peek() {
                Some(byte) => byte,
                None => {
                    return Err(LexerError::new(
                        "Unterminated string literal.",
                        line,
                        column,
                    ));
                }
            };

            match byte {
                b'"' => {
                    self.advance();
                    break;
                }

                b'\\' => {
                    self.advance();

                    let escaped = match self.peek() {
                        Some(byte) => byte,
                        None => {
                            return Err(LexerError::new(
                                "Unterminated escape sequence.",
                                self.line,
                                self.column,
                            ));
                        }
                    };

                    self.advance();

                    match escaped {
                        b'n' => value.push('\n'),
                        b'r' => value.push('\r'),
                        b't' => value.push('\t'),
                        b'\\' => value.push('\\'),
                        b'"' => value.push('"'),
                        _ => {
                            return Err(LexerError::new(
                                format!("Unknown escape sequence '\\{}'.", escaped as char),
                                self.line,
                                self.column.saturating_sub(1),
                            ));
                        }
                    }
                }

                b'\n' => {
                    return Err(LexerError::new(
                        "String literals cannot contain an unescaped newline.",
                        self.line,
                        self.column,
                    ));
                }

                _ => {
                    let chunk_start = self.position;
                    while let Some(b) = self.peek() {
                        if b == b'"' || b == b'\\' || b == b'\n' {
                            break;
                        }
                        self.advance();
                    }
                    let chunk = std::str::from_utf8(&self.source[chunk_start..self.position])
                        .map_err(|_| {
                            LexerError::new(
                                "Invalid UTF-8 in string literal.",
                                self.line,
                                self.column,
                            )
                        })?;
                    value.push_str(chunk);
                }
            }
        }

        Ok(self.token(TokenKind::String(value), start, line, column))
    }

    fn skip_spaces_and_comments(&mut self) {
        loop {
            let mut consumed = false;

            while matches!(self.peek(), Some(b' ' | b'\t' | b'\r')) {
                self.advance();
                consumed = true;
            }

            if self.peek() == Some(b'/') && self.peek_next() == Some(b'/') {
                consumed = true;

                self.advance();
                self.advance();

                while let Some(byte) = self.peek() {
                    if byte == b'\n' {
                        break;
                    }

                    self.advance();
                }
            }

            if !consumed {
                break;
            }
        }
    }

    fn token(&self, kind: TokenKind, start: usize, line: usize, column: usize) -> Token {
        Token::new(
            kind,
            SourceSpan::new(start as u32, self.position as u32),
            line,
            column,
        )
    }

    fn peek(&self) -> Option<u8> {
        self.source.get(self.position).copied()
    }

    fn peek_next(&self) -> Option<u8> {
        self.source.get(self.position + 1).copied()
    }

    fn advance(&mut self) -> Option<u8> {
        let byte = self.peek()?;

        self.position += 1;

        if byte == b'\n' {
            self.line += 1;
            self.column = 1;
        } else {
            self.column += 1;
        }

        Some(byte)
    }

    fn match_byte(&mut self, expected: u8) -> bool {
        if self.peek() == Some(expected) {
            self.advance();
            true
        } else {
            false
        }
    }
}

fn is_identifier_continue(byte: u8) -> bool {
    matches!(
        byte,
        b'a'..=b'z'
            | b'A'..=b'Z'
            | b'0'..=b'9'
            | b'_'
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scripting::token::TokenKind;

    #[test]
    fn tokenizes_basic_entity() {
        let source = r#"
entity Door {
    open: bool = false

    fn update(dt: number) {
        open = true
    }
}
"#;

        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        assert!(tokens.iter().any(|t| t.kind == TokenKind::Entity));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::Fn));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::False));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::True));
    }

    #[test]
    fn tokenizes_operators() {
        let source = r#"
a += 1
b -= 2
c *= 3
d /= 4
if a == b && c != d {
}
"#;

        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        assert!(tokens.iter().any(|t| t.kind == TokenKind::PlusEqual));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::MinusEqual));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::StarEqual));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::SlashEqual));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::EqualEqual));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::AndAnd));
        assert!(tokens.iter().any(|t| t.kind == TokenKind::BangEqual));
    }

    #[test]
    fn tokenizes_strings() {
        let source = r#"
message = "hello\nworld"
"#;

        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        assert!(
            tokens
                .iter()
                .any(|t| { t.kind == TokenKind::String("hello\nworld".to_string()) })
        );
    }

    #[test]
    fn tokens_carry_byte_spans() {
        let source = "entity Test";
        let tokens = Lexer::new(source).tokenize().expect("lexer should succeed");

        let entity = tokens
            .iter()
            .find(|token| token.kind == TokenKind::Entity)
            .expect("entity token should exist");

        assert_eq!(entity.span, SourceSpan::new(0, 6));

        let identifier = tokens
            .iter()
            .find(|token| matches!(token.kind, TokenKind::Identifier(_)))
            .expect("identifier token should exist");

        assert_eq!(identifier.span, SourceSpan::new(7, 11));
    }
}
