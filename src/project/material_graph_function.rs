// Worldkiln
// Copyright (c) 2026 Zachary Joubert / AEOWUN
// SPDX-License-Identifier: Apache-2.0
use super::{CustomPin, PinType};
use std::collections::BTreeMap;

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Ident(String),
    Number(String),
    Symbol(char),
}
#[derive(Clone)]
struct Expr {
    kind: PinType,
    source: String,
}
pub struct CompiledExpression {
    pub glsl: String,
    pub value_type: PinType,
}

fn lex(source: &str) -> Result<Vec<Token>, String> {
    if source.len() > 8192 || source.is_empty() {
        return Err("Function source must contain 1-8192 bytes".into());
    }
    let bytes = source.as_bytes();
    let mut i = 0;
    let mut tokens = Vec::new();
    while i < bytes.len() {
        if bytes[i].is_ascii_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        let token = if bytes[i].is_ascii_alphabetic() || bytes[i] == b'_' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            Token::Ident(source[start..i].to_owned())
        } else if bytes[i].is_ascii_digit() || bytes[i] == b'.' {
            i += 1;
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            let piece = &source[start..i];
            let value = piece
                .parse::<f32>()
                .map_err(|_| format!("Invalid numeric literal {piece}"))?;
            if !value.is_finite() {
                return Err("Material numeric literal must be finite".into());
            }
            Token::Number(format!("{value:.8}"))
        } else if b"+-*/(),=;".contains(&bytes[i]) {
            i += 1;
            Token::Symbol(bytes[start] as char)
        } else {
            return Err(format!(
                "Unsupported character at byte {i}; only safe expressions are allowed"
            ));
        };
        tokens.push(token);
        if tokens.len() > 256 {
            return Err("Material expression exceeds 256 tokens".into());
        }
    }
    Ok(tokens)
}
struct Parser<'a> {
    tokens: Vec<Token>,
    cursor: usize,
    depth: usize,
    inputs: &'a BTreeMap<String, (PinType, String)>,
}
impl Parser<'_> {
    fn next(&mut self) -> Option<Token> {
        let t = self.tokens.get(self.cursor).cloned();
        if t.is_some() {
            self.cursor += 1;
        }
        t
    }
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.cursor)
    }
    fn expect(&mut self, character: char) -> Result<(), String> {
        match self.next() {
            Some(Token::Symbol(c)) if c == character => Ok(()),
            _ => Err(format!("Expected '{character}' in Material expression")),
        }
    }
    fn parse(&mut self, min_precedence: u8) -> Result<Expr, String> {
        self.depth += 1;
        if self.depth > 24 {
            return Err("Material expression nesting exceeds 24".into());
        }
        let mut left = match self.next() {
            Some(Token::Number(v)) => Expr {
                kind: PinType::Scalar,
                source: v,
            },
            Some(Token::Symbol('-')) => {
                let inner = self.parse(3)?;
                Expr {
                    kind: inner.kind,
                    source: format!("(-{})", inner.source),
                }
            }
            Some(Token::Symbol('(')) => {
                let expression = self.parse(0)?;
                self.expect(')')?;
                expression
            }
            Some(Token::Ident(name)) => {
                if matches!(self.peek(), Some(Token::Symbol('('))) {
                    self.next();
                    let mut args = Vec::new();
                    if !matches!(self.peek(), Some(Token::Symbol(')'))) {
                        loop {
                            args.push(self.parse(0)?);
                            if matches!(self.peek(), Some(Token::Symbol(','))) {
                                self.next();
                                continue;
                            }
                            break;
                        }
                    }
                    self.expect(')')?;
                    Self::call(&name, args)?
                } else {
                    let (kind, uniform) = self
                        .inputs
                        .get(&name)
                        .ok_or_else(|| format!("Unknown input '{name}' in Material expression"))?;
                    Expr {
                        kind: *kind,
                        source: uniform.clone(),
                    }
                }
            }
            _ => return Err("Expected numeric constant, input, or math function".into()),
        };
        loop {
            let (precedence, op) = match self.peek() {
                Some(Token::Symbol('+')) => (1, '+'),
                Some(Token::Symbol('-')) => (1, '-'),
                Some(Token::Symbol('*')) => (2, '*'),
                Some(Token::Symbol('/')) => (2, '/'),
                _ => break,
            };
            if precedence < min_precedence {
                break;
            }
            self.next();
            let right = self.parse(precedence + 1)?;
            let kind = if left.kind == right.kind {
                left.kind
            } else if (op == '*' || op == '/') && right.kind == PinType::Scalar {
                left.kind
            } else if op == '*' && left.kind == PinType::Scalar {
                right.kind
            } else {
                return Err("Material arithmetic inputs must have matching types".into());
            };
            if op == '/' && right.source == "0.00000000" {
                return Err("Division by zero in Material expression".into());
            }
            left = Expr {
                kind,
                source: format!("({} {op} {})", left.source, right.source),
            };
        }
        self.depth -= 1;
        Ok(left)
    }
    fn call(name: &str, args: Vec<Expr>) -> Result<Expr, String> {
        let kinds: Vec<_> = args.iter().map(|a| a.kind).collect();
        let result = match (name, args.len()) {
            ("sin" | "cos" | "abs" | "fract", 1) => kinds[0],
            ("sqrt", 1) if kinds[0] == PinType::Scalar => PinType::Scalar,
            ("min" | "max", 2) if kinds[0] == kinds[1] => kinds[0],
            ("clamp", 3) if kinds[1] == kinds[2] && kinds[0] == kinds[1] => kinds[0],
            ("mix", 3) if kinds[0] == kinds[1] && kinds[2] == PinType::Scalar => kinds[0],
            ("vec2", 2) if kinds.iter().all(|&kind| kind == PinType::Scalar) => PinType::Vec2,
            ("vec3", 3) if kinds.iter().all(|&kind| kind == PinType::Scalar) => PinType::Vec3,
            ("color", 4) if kinds.iter().all(|&kind| kind == PinType::Scalar) => PinType::Color,
            _ => {
                return Err(format!(
                    "Unsupported function '{name}' or incorrect argument types"
                ));
            }
        };
        let glsl_name = if name == "color" { "vec4" } else { name };
        Ok(Expr {
            kind: result,
            source: format!(
                "{glsl_name}({})",
                args.iter()
                    .map(|a| a.source.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        })
    }
}
pub fn compile(
    source: &str,
    inputs: &[CustomPin],
    outputs: &[CustomPin],
    node: u32,
) -> Result<CompiledExpression, String> {
    if outputs.len() != 1 {
        return Err("Custom Function currently supports exactly one output".into());
    }
    let mut names = BTreeMap::new();
    for pin in inputs {
        if !pin
            .name
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'_')
        {
            return Err("Invalid Custom Function input".into());
        }
        names.insert(
            pin.name.clone(),
            (pin.value_type, format!("wk_f{node}_{}", pin.name)),
        );
    }
    let tokens = lex(source)?;
    let mut p = Parser {
        tokens,
        cursor: 0,
        depth: 0,
        inputs: &names,
    };
    match p.next() {
        Some(Token::Ident(target)) if target == outputs[0].name => {}
        _ => {
            return Err(format!(
                "Custom Function must assign '{} = expression;'",
                outputs[0].name
            ));
        }
    }
    p.expect('=')?;
    let compiled = p.parse(0)?;
    p.expect(';')?;
    if p.peek().is_some() {
        return Err("Only one expression assignment is allowed".into());
    }
    if compiled.kind != outputs[0].value_type {
        return Err(format!(
            "Custom Function output expects {:?}, expression returns {:?}",
            outputs[0].value_type, compiled.kind
        ));
    }
    Ok(CompiledExpression {
        glsl: compiled.source,
        value_type: compiled.kind,
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    fn pin(name: &str, kind: PinType) -> CustomPin {
        CustomPin {
            name: name.into(),
            value_type: kind,
        }
    }
    #[test]
    fn compiles_typed_math_with_safe_identifiers() {
        let v = compile(
            "result = clamp(value * 0.5 + 0.1, 0.0, 1.0);",
            &[pin("value", PinType::Scalar)],
            &[pin("result", PinType::Scalar)],
            7,
        )
        .unwrap();
        assert!(v.glsl.contains("wk_f7_value"));
        assert!(v.glsl.contains("clamp("));
    }
    #[test]
    fn rejects_injection_unknown_symbols_and_unbounded_math() {
        let ins = [pin("value", PinType::Scalar)];
        let outs = [pin("result", PinType::Scalar)];
        for code in [
            "result = value; discard;",
            "result = texture(data, uv);",
            "result = value.some;",
            "result = value[0];",
            "result = #define BAD;",
            "result = custom(value);",
            "result = value / 0.0;",
            "result = vec2(1.0,2.0);",
            "fn hack() { return value; }",
        ] {
            assert!(compile(code, &ins, &outs, 4).is_err(), "{code}");
        }
    }
    #[test]
    fn vector_construction_and_math_are_typed() {
        let v = compile(
            "out = mix(vec3(1,0,0), vec3(0,1,0), t);",
            &[pin("t", PinType::Scalar)],
            &[pin("out", PinType::Vec3)],
            8,
        )
        .unwrap();
        assert_eq!(v.value_type, PinType::Vec3);
        assert!(v.glsl.contains("vec3("));
    }
}
