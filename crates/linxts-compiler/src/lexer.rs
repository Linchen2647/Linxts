use linxts_source::{SourceId, Span};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TokenKind {
    Integer(i64),
    String(String),
    Template(String),
    Identifier(String),
    LeftParen,
    RightParen,
    LeftBrace,
    RightBrace,
    Comma,
    Dot,
    Colon,
    Semicolon,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    Bang,
    Equal,
    EqualEqual,
    BangEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    AndAnd,
    OrOr,
    Newline,
    Eof,
    Error(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Token {
    pub kind: TokenKind,
    pub span: Span,
}

pub fn lex(source: SourceId, base: usize, input: &str) -> Vec<Token> {
    let mut tokens = Vec::new();
    let mut cursor = 0;
    while cursor < input.len() {
        let character = input[cursor..].chars().next().expect("valid cursor");
        let width = character.len_utf8();
        match character {
            ' ' | '\t' | '\r' => cursor += width,
            '\n' => {
                push(
                    &mut tokens,
                    source,
                    base,
                    cursor,
                    cursor + width,
                    TokenKind::Newline,
                );
                cursor += width;
            }
            '#' => cursor = skip_to_newline(input, cursor),
            '/' if input[cursor + width..].starts_with('/') => {
                cursor = skip_to_newline(input, cursor)
            }
            '0'..='9' => {
                let start = cursor;
                cursor += width;
                while cursor < input.len()
                    && input[cursor..]
                        .chars()
                        .next()
                        .is_some_and(|next| next.is_ascii_digit())
                {
                    cursor += 1;
                }
                let value = input[start..cursor].parse().unwrap_or(i64::MAX);
                push(
                    &mut tokens,
                    source,
                    base,
                    start,
                    cursor,
                    TokenKind::Integer(value),
                );
            }
            '"' | '\'' => {
                let quote = character;
                let start = cursor;
                cursor += width;
                let mut value = String::new();
                let mut terminated = false;
                while cursor < input.len() {
                    let next = input[cursor..].chars().next().expect("valid cursor");
                    cursor += next.len_utf8();
                    if next == quote {
                        terminated = true;
                        break;
                    }
                    if next == '\\' && cursor < input.len() {
                        let escaped = input[cursor..].chars().next().expect("valid cursor");
                        cursor += escaped.len_utf8();
                        value.push(match escaped {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        value.push(next);
                    }
                }
                let kind = if terminated {
                    TokenKind::String(value)
                } else {
                    TokenKind::Error("unterminated string literal".to_owned())
                };
                push(&mut tokens, source, base, start, cursor, kind);
            }
            '`' => {
                let start = cursor;
                cursor += width;
                let mut value = String::new();
                let mut terminated = false;
                while cursor < input.len() {
                    let next = input[cursor..].chars().next().expect("valid cursor");
                    cursor += next.len_utf8();
                    if next == '`' {
                        terminated = true;
                        break;
                    }
                    if next == '\\' && cursor < input.len() {
                        let escaped = input[cursor..].chars().next().expect("valid cursor");
                        cursor += escaped.len_utf8();
                        value.push(match escaped {
                            'n' => '\n',
                            'r' => '\r',
                            't' => '\t',
                            other => other,
                        });
                    } else {
                        value.push(next);
                    }
                }
                let kind = if terminated {
                    TokenKind::Template(value)
                } else {
                    TokenKind::Error("unterminated template literal".to_owned())
                };
                push(&mut tokens, source, base, start, cursor, kind);
            }
            '(' => single(&mut tokens, source, base, &mut cursor, TokenKind::LeftParen),
            ')' => single(
                &mut tokens,
                source,
                base,
                &mut cursor,
                TokenKind::RightParen,
            ),
            '{' => single(&mut tokens, source, base, &mut cursor, TokenKind::LeftBrace),
            '}' => single(
                &mut tokens,
                source,
                base,
                &mut cursor,
                TokenKind::RightBrace,
            ),
            ',' => single(&mut tokens, source, base, &mut cursor, TokenKind::Comma),
            '.' => single(&mut tokens, source, base, &mut cursor, TokenKind::Dot),
            ':' => single(&mut tokens, source, base, &mut cursor, TokenKind::Colon),
            ';' => single(&mut tokens, source, base, &mut cursor, TokenKind::Semicolon),
            '+' => single(&mut tokens, source, base, &mut cursor, TokenKind::Plus),
            '-' => single(&mut tokens, source, base, &mut cursor, TokenKind::Minus),
            '*' => single(&mut tokens, source, base, &mut cursor, TokenKind::Star),
            '%' => single(&mut tokens, source, base, &mut cursor, TokenKind::Percent),
            '/' => single(&mut tokens, source, base, &mut cursor, TokenKind::Slash),
            '!' => paired(
                &mut tokens,
                source,
                base,
                input,
                &mut cursor,
                '=',
                TokenKind::BangEqual,
                TokenKind::Bang,
            ),
            '=' => paired(
                &mut tokens,
                source,
                base,
                input,
                &mut cursor,
                '=',
                TokenKind::EqualEqual,
                TokenKind::Equal,
            ),
            '<' => paired(
                &mut tokens,
                source,
                base,
                input,
                &mut cursor,
                '=',
                TokenKind::LessEqual,
                TokenKind::Less,
            ),
            '>' => paired(
                &mut tokens,
                source,
                base,
                input,
                &mut cursor,
                '=',
                TokenKind::GreaterEqual,
                TokenKind::Greater,
            ),
            '&' if input[cursor + width..].starts_with('&') => {
                let start = cursor;
                cursor += 2;
                push(&mut tokens, source, base, start, cursor, TokenKind::AndAnd);
            }
            '|' if input[cursor + width..].starts_with('|') => {
                let start = cursor;
                cursor += 2;
                push(&mut tokens, source, base, start, cursor, TokenKind::OrOr);
            }
            _ if is_identifier_start(character) => {
                let start = cursor;
                cursor += width;
                while cursor < input.len() {
                    let next = input[cursor..].chars().next().expect("valid cursor");
                    if !is_identifier_continue(next) {
                        break;
                    }
                    cursor += next.len_utf8();
                }
                push(
                    &mut tokens,
                    source,
                    base,
                    start,
                    cursor,
                    TokenKind::Identifier(input[start..cursor].to_owned()),
                );
            }
            _ => {
                push(
                    &mut tokens,
                    source,
                    base,
                    cursor,
                    cursor + width,
                    TokenKind::Error(format!("unexpected character `{character}`")),
                );
                cursor += width;
            }
        }
    }
    tokens.push(Token {
        kind: TokenKind::Eof,
        span: Span::empty(source, base + input.len()),
    });
    tokens
}

fn is_identifier_start(character: char) -> bool {
    character == '_' || character.is_alphabetic()
}

fn is_identifier_continue(character: char) -> bool {
    character == '_' || character.is_alphanumeric()
}

fn skip_to_newline(input: &str, start: usize) -> usize {
    input[start..]
        .find('\n')
        .map_or(input.len(), |relative| start + relative)
}

fn single(
    tokens: &mut Vec<Token>,
    source: SourceId,
    base: usize,
    cursor: &mut usize,
    kind: TokenKind,
) {
    let start = *cursor;
    *cursor += 1;
    push(tokens, source, base, start, *cursor, kind);
}

#[allow(clippy::too_many_arguments)]
fn paired(
    tokens: &mut Vec<Token>,
    source: SourceId,
    base: usize,
    input: &str,
    cursor: &mut usize,
    second: char,
    paired_kind: TokenKind,
    single_kind: TokenKind,
) {
    let start = *cursor;
    *cursor += 1;
    if input[*cursor..].starts_with(second) {
        *cursor += second.len_utf8();
        push(tokens, source, base, start, *cursor, paired_kind);
    } else {
        push(tokens, source, base, start, *cursor, single_kind);
    }
}

fn push(
    tokens: &mut Vec<Token>,
    source: SourceId,
    base: usize,
    start: usize,
    end: usize,
    kind: TokenKind,
) {
    tokens.push(Token {
        kind,
        span: Span::new(source, base + start, base + end),
    });
}
