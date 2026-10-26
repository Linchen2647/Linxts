use crate::lexer::{Token, TokenKind, lex};
use linxts_ir::{
    BinaryOperator, Block, Expression, ExpressionKind, FormatSegment, Function, Parameter,
    Statement, StatementKind, Type, UnaryOperator,
};
use linxts_source::{SourceId, Span};
use std::collections::HashSet;

const MAX_PARSE_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseDiagnostic {
    pub code: &'static str,
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Default)]
pub struct ParsedUnit {
    pub block: Block,
    pub functions: Vec<Function>,
    pub diagnostics: Vec<ParseDiagnostic>,
}

impl ParsedUnit {
    #[must_use]
    pub fn is_complete(&self) -> bool {
        self.diagnostics.is_empty()
    }
}

enum BlockEnd {
    Eof,
    Brace,
    Dedent,
}

pub struct Parser<'a> {
    tokens: Vec<Token>,
    cursor: usize,
    diagnostics: Vec<ParseDiagnostic>,
    functions: Vec<Function>,
    declared: &'a mut HashSet<String>,
    indentation_sensitive: bool,
    depth: usize,
}

impl<'a> Parser<'a> {
    #[must_use]
    pub fn parse(
        source: SourceId,
        base: usize,
        input: &str,
        declared: &'a mut HashSet<String>,
        indentation_sensitive: bool,
    ) -> ParsedUnit {
        let tokens = lex(source, base, input, indentation_sensitive);
        let mut parser = Self {
            tokens,
            cursor: 0,
            diagnostics: Vec::new(),
            functions: Vec::new(),
            declared,
            indentation_sensitive,
            depth: 0,
        };
        let block = parser.parse_statements(BlockEnd::Eof);
        ParsedUnit {
            block,
            functions: parser.functions,
            diagnostics: parser.diagnostics,
        }
    }

    fn parse_statements(&mut self, end: BlockEnd) -> Block {
        let mut statements = Vec::new();
        let mut closed = matches!(end, BlockEnd::Eof);
        self.skip_separators();
        while !self.at(&TokenKind::Eof) {
            let reached_end = match end {
                BlockEnd::Eof => false,
                BlockEnd::Brace => self.at(&TokenKind::RightBrace),
                BlockEnd::Dedent => self.at(&TokenKind::Dedent),
            };
            if reached_end {
                self.advance();
                closed = true;
                break;
            }
            if self.at(&TokenKind::RightBrace) {
                self.error_here("LXT-101", "unmatched closing brace");
                self.advance();
                self.skip_separators();
                continue;
            }
            if self.at(&TokenKind::Indent) || self.at(&TokenKind::Dedent) {
                self.error_here("LXT-122", "unexpected indentation boundary");
                self.advance();
                self.skip_separators();
                continue;
            }
            let before = self.cursor;
            statements.extend(self.parse_next_statements());
            if self.cursor == before {
                self.error_here("LXT-102", "parser made no progress");
                self.advance();
            }
            self.skip_separators();
        }
        if !closed {
            let (code, message) = match end {
                BlockEnd::Brace => ("LXT-103", "missing closing brace"),
                BlockEnd::Dedent => ("LXT-123", "missing dedent at end of block"),
                BlockEnd::Eof => unreachable!("top-level block is always closed"),
            };
            self.error_here(code, message);
        }
        Block { statements }
    }

    fn parse_next_statements(&mut self) -> Vec<Statement> {
        if self.check_identifier("for") {
            self.parse_for_range()
        } else {
            self.parse_statement().into_iter().collect()
        }
    }

    fn parse_statement(&mut self) -> Option<Statement> {
        if self.depth > MAX_PARSE_DEPTH {
            self.error_here("LXT-105", "parser nesting limit exceeded");
            self.synchronize();
            return None;
        }
        if self.is_c_function_start() {
            self.parse_c_function();
            return None;
        }
        if self.is_c_declaration_start() {
            return Some(self.parse_c_declaration());
        }
        if self.check_identifier("fn")
            || self.check_identifier("def")
            || self.check_identifier("function")
        {
            self.parse_function();
            return None;
        }
        if self.check_identifier("if") {
            return Some(self.parse_conditional());
        }
        if self.check_identifier("while") {
            return Some(self.parse_loop());
        }
        if self.check_identifier("return") {
            return Some(self.parse_return());
        }
        if self.check_identifier("let")
            || self.check_identifier("const")
            || self.check_identifier("var")
        {
            return Some(self.parse_declaration());
        }
        if self.is_print_start() {
            return Some(self.parse_print());
        }
        if let TokenKind::Error(message) = self.current().kind.clone() {
            let span = self.current().span;
            self.diagnostics.push(ParseDiagnostic {
                code: "LXT-106",
                message: message.clone(),
                span,
            });
            self.advance();
            return Some(Statement::new(
                StatementKind::Expression(Expression::new(ExpressionKind::Error { message }, span)),
                span,
            ));
        }
        if let Some(name) = self.assignment_name() {
            return Some(self.parse_assignment(name));
        }

        let start = self.current().span;
        let expression = self.parse_expression(0);
        let span = merge(start, expression.span);
        Some(Statement::new(StatementKind::Expression(expression), span))
    }

    fn parse_function(&mut self) {
        let start = self.advance().span;
        let Some((name, name_span)) = self.take_identifier() else {
            self.error_here("LXT-107", "expected function name");
            self.synchronize();
            return;
        };
        if !self.consume(&TokenKind::LeftParen) {
            self.error_here("LXT-108", "expected `(` after function name");
        }
        let mut parameters = Vec::new();
        while !self.at(&TokenKind::RightParen) && !self.at(&TokenKind::Eof) {
            let Some((parameter, span)) = self.take_identifier() else {
                self.error_here("LXT-109", "expected parameter name");
                self.advance();
                continue;
            };
            parameters.push(Parameter {
                name: parameter,
                ty: Type::Unknown,
                span,
            });
            if self.consume(&TokenKind::Colon) {
                while !self.at(&TokenKind::Comma)
                    && !self.at(&TokenKind::RightParen)
                    && !self.at(&TokenKind::Eof)
                {
                    self.advance();
                }
            }
            if !self.consume(&TokenKind::Comma) {
                break;
            }
        }
        if !self.consume(&TokenKind::RightParen) {
            self.error_here("LXT-110", "expected `)` after parameters");
        }

        let globals = self.declared.clone();
        for parameter in &parameters {
            self.declared.insert(parameter.name.clone());
        }
        let body = self.parse_required_block();
        *self.declared = globals;
        let end = body
            .statements
            .last()
            .map_or(name_span, |statement| statement.span);
        self.functions.push(Function {
            name,
            parameters,
            body,
            return_type: Type::Unknown,
            span: merge(start, end),
        });
    }

    fn parse_c_function(&mut self) {
        let return_token = self.advance().clone();
        let return_type = match &return_token.kind {
            TokenKind::Identifier(name) if name.eq_ignore_ascii_case("void") => Type::Unit,
            _ => Type::Integer,
        };
        let Some((name, name_span)) = self.take_identifier() else {
            self.error_here("LXT-107", "expected C function name");
            self.synchronize();
            return;
        };
        if !self.consume(&TokenKind::LeftParen) {
            self.error_here("LXT-108", "expected `(` after C function name");
        }
        let mut parameters = Vec::new();
        if self.check_identifier("void") {
            self.advance();
        } else {
            while !self.at(&TokenKind::RightParen) && !self.at(&TokenKind::Eof) {
                if self.check_identifier("int") || self.check_identifier("char") {
                    self.advance();
                    self.consume(&TokenKind::Star);
                }
                let Some((parameter, span)) = self.take_identifier() else {
                    self.error_here("LXT-109", "expected C parameter name");
                    self.advance();
                    continue;
                };
                parameters.push(Parameter {
                    name: parameter,
                    ty: Type::Unknown,
                    span,
                });
                if !self.consume(&TokenKind::Comma) {
                    break;
                }
            }
        }
        if !self.consume(&TokenKind::RightParen) {
            self.error_here("LXT-110", "expected `)` after C parameters");
        }

        let globals = self.declared.clone();
        for parameter in &parameters {
            self.declared.insert(parameter.name.clone());
        }
        let body = self.parse_required_block();
        *self.declared = globals;
        let end = body
            .statements
            .last()
            .map_or(name_span, |statement| statement.span);
        self.functions.push(Function {
            name,
            parameters,
            body,
            return_type,
            span: merge(return_token.span, end),
        });
    }

    fn parse_c_declaration(&mut self) -> Statement {
        let start = self.advance().span;
        self.consume(&TokenKind::Star);
        let Some((name, name_span)) = self.take_identifier() else {
            self.error_here("LXT-111", "expected C variable name");
            return Statement::new(
                StatementKind::Expression(Expression::new(
                    ExpressionKind::Error {
                        message: "missing C variable name".to_owned(),
                    },
                    start,
                )),
                start,
            );
        };
        let value = if self.consume(&TokenKind::Equal) {
            self.parse_expression(0)
        } else {
            Expression::integer(0, name_span)
        };
        self.declared.insert(name.clone());
        let span = merge(start, value.span);
        Statement::new(
            StatementKind::Variable {
                name,
                value,
                mutable: true,
            },
            span,
        )
    }

    fn parse_conditional(&mut self) -> Statement {
        let start = self.advance().span;
        let condition = self.parse_expression(0);
        let then_block = self.parse_required_block();
        self.skip_separators();
        let else_block = if self.check_identifier("else") {
            self.advance();
            Some(self.parse_required_block())
        } else {
            None
        };
        let end = else_block
            .as_ref()
            .and_then(|block| block.statements.last())
            .or_else(|| then_block.statements.last())
            .map_or(condition.span, |statement| statement.span);
        Statement::new(
            StatementKind::Conditional {
                condition,
                then_block,
                else_block,
            },
            merge(start, end),
        )
    }

    fn parse_loop(&mut self) -> Statement {
        let start = self.advance().span;
        let condition = self.parse_expression(0);
        let body = self.parse_required_block();
        let end = body
            .statements
            .last()
            .map_or(condition.span, |statement| statement.span);
        Statement::new(StatementKind::Loop { condition, body }, merge(start, end))
    }

    fn parse_for_range(&mut self) -> Vec<Statement> {
        let start = self.advance().span;
        let Some((name, name_span)) = self.take_identifier() else {
            self.error_here("LXT-124", "expected loop variable after `for`");
            self.synchronize();
            return Vec::new();
        };
        if !self.consume_identifier("in") {
            self.error_here("LXT-125", "expected `in` after loop variable");
        }
        if !self.consume_identifier("range") {
            self.error_here("LXT-126", "only `range(...)` for-loops are supported");
        }
        if !self.consume(&TokenKind::LeftParen) {
            self.error_here("LXT-127", "expected `(` after `range`");
        }

        let mut arguments = Vec::new();
        while !self.at(&TokenKind::RightParen) && !self.at_separator() && !self.at(&TokenKind::Eof)
        {
            arguments.push(self.parse_expression(0));
            if !self.consume(&TokenKind::Comma) {
                break;
            }
        }
        if !self.consume(&TokenKind::RightParen) {
            self.error_here("LXT-128", "expected `)` after range arguments");
        }

        let arguments_valid = (1..=3).contains(&arguments.len());
        if !arguments_valid {
            self.error_here("LXT-129", "range expects one, two, or three arguments");
        }
        let fallback = Expression::integer(0, name_span);
        let (initial, limit, step) = match arguments.as_slice() {
            [limit] => (
                Expression::integer(0, name_span),
                limit.clone(),
                Expression::integer(1, name_span),
            ),
            [initial, limit] => (
                initial.clone(),
                limit.clone(),
                Expression::integer(1, name_span),
            ),
            [initial, limit, step] => (initial.clone(), limit.clone(), step.clone()),
            _ => (
                fallback.clone(),
                fallback.clone(),
                Expression::integer(1, name_span),
            ),
        };
        let step_value = match &step.kind {
            ExpressionKind::Literal(linxts_ir::Literal::Integer(value)) if *value != 0 => *value,
            ExpressionKind::Unary {
                operator: UnaryOperator::Negate,
                operand,
            } => match &operand.kind {
                ExpressionKind::Literal(linxts_ir::Literal::Integer(0)) => {
                    self.error_here("LXT-130", "range step cannot be zero");
                    1
                }
                ExpressionKind::Literal(linxts_ir::Literal::Integer(value)) => {
                    value.saturating_neg()
                }
                _ => {
                    self.error_here("LXT-131", "range step must be an integer literal");
                    1
                }
            },
            ExpressionKind::Literal(linxts_ir::Literal::Integer(_)) => {
                self.error_here("LXT-130", "range step cannot be zero");
                1
            }
            _ => {
                self.error_here("LXT-131", "range step must be an integer literal");
                1
            }
        };

        let newly_declared = self.declared.insert(name.clone());
        let mut body = self.parse_required_block();
        let increment_value = Expression::new(
            ExpressionKind::Binary {
                operator: BinaryOperator::Add,
                left: Box::new(Expression::new(
                    ExpressionKind::Variable { name: name.clone() },
                    name_span,
                )),
                right: Box::new(step),
            },
            name_span,
        );
        body.statements.push(Statement::new(
            StatementKind::Assignment {
                name: name.clone(),
                value: increment_value,
            },
            name_span,
        ));

        let initializer = Statement::new(
            if newly_declared {
                StatementKind::Variable {
                    name: name.clone(),
                    value: initial,
                    mutable: true,
                }
            } else {
                StatementKind::Assignment {
                    name: name.clone(),
                    value: initial,
                }
            },
            merge(start, name_span),
        );
        let condition = Expression::new(
            ExpressionKind::Binary {
                operator: if step_value > 0 {
                    BinaryOperator::Less
                } else {
                    BinaryOperator::Greater
                },
                left: Box::new(Expression::new(
                    ExpressionKind::Variable { name },
                    name_span,
                )),
                right: Box::new(limit),
            },
            name_span,
        );
        let end = body
            .statements
            .last()
            .map_or(condition.span, |statement| statement.span);
        let loop_statement =
            Statement::new(StatementKind::Loop { condition, body }, merge(start, end));

        if arguments_valid {
            vec![initializer, loop_statement]
        } else {
            Vec::new()
        }
    }

    fn parse_return(&mut self) -> Statement {
        let start = self.advance().span;
        let value = if self.at_separator() || self.at(&TokenKind::RightBrace) {
            None
        } else {
            Some(self.parse_expression(0))
        };
        let end = value.as_ref().map_or(start, |expression| expression.span);
        Statement::new(StatementKind::Return(value), merge(start, end))
    }

    fn parse_declaration(&mut self) -> Statement {
        let start = self.advance().span;
        let mut mutable = !self.previous_identifier_is("const");
        if self.check_identifier("mut") {
            self.advance();
            mutable = true;
        }
        let Some((name, _name_span)) = self.take_identifier() else {
            self.error_here("LXT-111", "expected variable name");
            let error = Expression::new(
                ExpressionKind::Error {
                    message: "missing variable name".to_owned(),
                },
                start,
            );
            return Statement::new(StatementKind::Expression(error), start);
        };
        if self.consume(&TokenKind::Colon) {
            while !self.at(&TokenKind::Equal) && !self.at_separator() && !self.at(&TokenKind::Eof) {
                self.advance();
            }
        }
        if !self.consume(&TokenKind::Equal) {
            self.error_here("LXT-112", "expected `=` in variable declaration");
        }
        let value = self.parse_expression(0);
        self.declared.insert(name.clone());
        let span = merge(start, value.span);
        Statement::new(
            StatementKind::Variable {
                name,
                value,
                mutable,
            },
            span,
        )
    }

    fn parse_assignment(&mut self, name: String) -> Statement {
        let start = self.advance().span;
        self.advance();
        let value = self.parse_expression(0);
        let span = merge(start, value.span);
        if self.declared.insert(name.clone()) {
            Statement::new(
                StatementKind::Variable {
                    name,
                    value,
                    mutable: true,
                },
                span,
            )
        } else {
            Statement::new(StatementKind::Assignment { name, value }, span)
        }
    }

    fn parse_print(&mut self) -> Statement {
        let start = self.current().span;
        let c_style = self.check_identifier("printf");
        let mut macro_style = false;
        if self.check_identifier("console") {
            self.advance();
            self.consume(&TokenKind::Dot);
            if !self.check_identifier("log") {
                self.error_here("LXT-113", "expected `log` after `console.`");
            } else {
                self.advance();
            }
        } else {
            self.advance();
            macro_style = self.consume(&TokenKind::Bang);
        }
        let parenthesized = self.consume(&TokenKind::LeftParen);
        let first = self.parse_expression(0);
        let mut arguments = Vec::new();
        while self.consume(&TokenKind::Comma) {
            arguments.push(self.parse_expression(0));
        }
        if parenthesized && !self.consume(&TokenKind::RightParen) {
            self.error_here("LXT-114", "expected `)` after print argument");
        }
        let end = self.previous().span;
        let span = merge(start, end);
        let expression = if arguments.is_empty() && macro_style {
            if let ExpressionKind::Literal(linxts_ir::Literal::String(template)) = &first.kind {
                named_format(template, false, span)
            } else {
                first
            }
        } else if arguments.is_empty() {
            first
        } else if let ExpressionKind::Literal(linxts_ir::Literal::String(template)) = &first.kind {
            match positional_format(template, arguments, c_style, span) {
                Ok(expression) => expression,
                Err(message) => {
                    self.diagnostics.push(ParseDiagnostic {
                        code: "LXT-119",
                        message: message.clone(),
                        span,
                    });
                    Expression::new(ExpressionKind::Error { message }, span)
                }
            }
        } else {
            let message = "formatted print requires a string template as its first argument";
            self.diagnostics.push(ParseDiagnostic {
                code: "LXT-119",
                message: message.to_owned(),
                span,
            });
            Expression::new(
                ExpressionKind::Error {
                    message: message.to_owned(),
                },
                span,
            )
        };
        Statement::new(StatementKind::Print(expression), span)
    }

    fn parse_required_block(&mut self) -> Block {
        self.depth += 1;
        let block = if self.consume(&TokenKind::LeftBrace) {
            self.parse_statements(BlockEnd::Brace)
        } else {
            self.consume(&TokenKind::Colon);
            if self.consume(&TokenKind::Newline) {
                while self.consume(&TokenKind::Newline) {}
                if self.consume(&TokenKind::Indent) {
                    self.parse_statements(BlockEnd::Dedent)
                } else if self.indentation_sensitive {
                    self.error_here("LXT-132", "expected an indented block");
                    Block::default()
                } else if self.at(&TokenKind::Eof) {
                    self.error_here("LXT-115", "expected block body");
                    Block::default()
                } else {
                    Block {
                        statements: self.parse_next_statements(),
                    }
                }
            } else if self.at(&TokenKind::Eof) {
                self.error_here("LXT-115", "expected block body");
                Block::default()
            } else {
                Block {
                    statements: self.parse_next_statements(),
                }
            }
        };
        self.depth = self.depth.saturating_sub(1);
        block
    }

    fn parse_expression(&mut self, minimum_precedence: u8) -> Expression {
        let mut left = self.parse_unary();
        while let Some((operator, precedence)) = self.current_binary_operator() {
            if precedence < minimum_precedence {
                break;
            }
            self.advance();
            let right = self.parse_expression(precedence + 1);
            let span = merge(left.span, right.span);
            left = Expression::new(
                ExpressionKind::Binary {
                    operator,
                    left: Box::new(left),
                    right: Box::new(right),
                },
                span,
            );
        }
        left
    }

    fn parse_unary(&mut self) -> Expression {
        let operator = if self.consume(&TokenKind::Minus) {
            Some(UnaryOperator::Negate)
        } else if self.consume(&TokenKind::Bang) || self.consume_identifier("not") {
            Some(UnaryOperator::Not)
        } else {
            None
        };
        if let Some(operator) = operator {
            let start = self.previous().span;
            let operand = self.parse_unary();
            let span = merge(start, operand.span);
            Expression::new(
                ExpressionKind::Unary {
                    operator,
                    operand: Box::new(operand),
                },
                span,
            )
        } else {
            self.parse_primary()
        }
    }

    fn parse_primary(&mut self) -> Expression {
        let token = self.advance().clone();
        match token.kind {
            TokenKind::Integer(value) => Expression::integer(value, token.span),
            TokenKind::String(value) => Expression::string(value, token.span),
            TokenKind::Template(value) => named_format(&value, true, token.span),
            TokenKind::Identifier(name)
                if name.eq_ignore_ascii_case("f")
                    && matches!(&self.current().kind, TokenKind::String(_)) =>
            {
                let string_token = self.advance().clone();
                let TokenKind::String(value) = string_token.kind else {
                    unreachable!("guard requires string token");
                };
                named_format(&value, false, merge(token.span, string_token.span))
            }
            TokenKind::Identifier(name) if name.eq_ignore_ascii_case("true") => {
                Expression::boolean(true, token.span)
            }
            TokenKind::Identifier(name) if name.eq_ignore_ascii_case("false") => {
                Expression::boolean(false, token.span)
            }
            TokenKind::Identifier(mut name) => {
                while self.consume(&TokenKind::Dot) {
                    let Some((part, _)) = self.take_identifier() else {
                        self.error_here("LXT-116", "expected identifier after `.`");
                        break;
                    };
                    name.push('.');
                    name.push_str(&part);
                }
                if self.consume(&TokenKind::LeftParen) {
                    let mut arguments = Vec::new();
                    while !self.at(&TokenKind::RightParen) && !self.at(&TokenKind::Eof) {
                        arguments.push(self.parse_expression(0));
                        if !self.consume(&TokenKind::Comma) {
                            break;
                        }
                    }
                    if !self.consume(&TokenKind::RightParen) {
                        self.error_here("LXT-117", "expected `)` after function arguments");
                    }
                    let end = self.previous().span;
                    Expression::new(
                        ExpressionKind::Call {
                            callee: name,
                            arguments,
                        },
                        merge(token.span, end),
                    )
                } else {
                    Expression::new(ExpressionKind::Variable { name }, token.span)
                }
            }
            TokenKind::LeftParen => {
                let expression = self.parse_expression(0);
                if !self.consume(&TokenKind::RightParen) {
                    self.error_here("LXT-118", "expected `)` after expression");
                }
                expression
            }
            TokenKind::Error(message) => {
                self.diagnostics.push(ParseDiagnostic {
                    code: "LXT-106",
                    message: message.clone(),
                    span: token.span,
                });
                Expression::new(ExpressionKind::Error { message }, token.span)
            }
            _ => {
                self.diagnostics.push(ParseDiagnostic {
                    code: "LXT-104",
                    message: "unable to resolve expression".to_owned(),
                    span: token.span,
                });
                Expression::new(
                    ExpressionKind::Error {
                        message: "unable to resolve expression".to_owned(),
                    },
                    token.span,
                )
            }
        }
    }

    fn current_binary_operator(&self) -> Option<(BinaryOperator, u8)> {
        match &self.current().kind {
            TokenKind::OrOr => Some((BinaryOperator::Or, 1)),
            TokenKind::AndAnd => Some((BinaryOperator::And, 2)),
            TokenKind::Identifier(name) if name.eq_ignore_ascii_case("or") => {
                Some((BinaryOperator::Or, 1))
            }
            TokenKind::Identifier(name) if name.eq_ignore_ascii_case("and") => {
                Some((BinaryOperator::And, 2))
            }
            TokenKind::EqualEqual => Some((BinaryOperator::Equal, 3)),
            TokenKind::BangEqual => Some((BinaryOperator::NotEqual, 3)),
            TokenKind::Less => Some((BinaryOperator::Less, 4)),
            TokenKind::LessEqual => Some((BinaryOperator::LessEqual, 4)),
            TokenKind::Greater => Some((BinaryOperator::Greater, 4)),
            TokenKind::GreaterEqual => Some((BinaryOperator::GreaterEqual, 4)),
            TokenKind::Plus => Some((BinaryOperator::Add, 5)),
            TokenKind::Minus => Some((BinaryOperator::Subtract, 5)),
            TokenKind::Star => Some((BinaryOperator::Multiply, 6)),
            TokenKind::Slash => Some((BinaryOperator::Divide, 6)),
            TokenKind::Percent => Some((BinaryOperator::Remainder, 6)),
            _ => None,
        }
    }

    fn assignment_name(&self) -> Option<String> {
        match (
            &self.current().kind,
            self.tokens.get(self.cursor + 1).map(|t| &t.kind),
        ) {
            (TokenKind::Identifier(name), Some(TokenKind::Equal)) => Some(name.clone()),
            _ => None,
        }
    }

    fn is_c_function_start(&self) -> bool {
        (self.check_identifier("int") || self.check_identifier("void"))
            && matches!(
                self.tokens.get(self.cursor + 1).map(|token| &token.kind),
                Some(TokenKind::Identifier(_))
            )
            && matches!(
                self.tokens.get(self.cursor + 2).map(|token| &token.kind),
                Some(TokenKind::LeftParen)
            )
    }

    fn is_c_declaration_start(&self) -> bool {
        if !(self.check_identifier("int") || self.check_identifier("char")) {
            return false;
        }
        matches!(
            self.tokens.get(self.cursor + 1).map(|token| &token.kind),
            Some(TokenKind::Identifier(_)) | Some(TokenKind::Star)
        )
    }

    fn is_print_start(&self) -> bool {
        match &self.current().kind {
            TokenKind::Identifier(name) => {
                name.eq_ignore_ascii_case("print")
                    || name.eq_ignore_ascii_case("println")
                    || name.eq_ignore_ascii_case("printf")
                    || (name.eq_ignore_ascii_case("console")
                        && matches!(
                            self.tokens.get(self.cursor + 1).map(|token| &token.kind),
                            Some(TokenKind::Dot)
                        ))
            }
            _ => false,
        }
    }

    fn synchronize(&mut self) {
        while !self.at(&TokenKind::Eof) && !self.at_separator() {
            self.advance();
        }
    }

    fn skip_separators(&mut self) {
        while self.at_separator() {
            self.advance();
        }
    }

    fn at_separator(&self) -> bool {
        self.at(&TokenKind::Newline) || self.at(&TokenKind::Semicolon)
    }

    fn check_identifier(&self, expected: &str) -> bool {
        matches!(&self.current().kind, TokenKind::Identifier(name) if name.eq_ignore_ascii_case(expected))
    }

    fn consume_identifier(&mut self, expected: &str) -> bool {
        if self.check_identifier(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn previous_identifier_is(&self, expected: &str) -> bool {
        matches!(&self.previous().kind, TokenKind::Identifier(name) if name.eq_ignore_ascii_case(expected))
    }

    fn take_identifier(&mut self) -> Option<(String, Span)> {
        let TokenKind::Identifier(name) = self.current().kind.clone() else {
            return None;
        };
        let span = self.advance().span;
        Some((name, span))
    }

    fn consume(&mut self, expected: &TokenKind) -> bool {
        if self.at(expected) {
            self.advance();
            true
        } else {
            false
        }
    }

    fn at(&self, expected: &TokenKind) -> bool {
        std::mem::discriminant(&self.current().kind) == std::mem::discriminant(expected)
    }

    fn current(&self) -> &Token {
        &self.tokens[self.cursor.min(self.tokens.len() - 1)]
    }

    fn previous(&self) -> &Token {
        &self.tokens[self.cursor.saturating_sub(1)]
    }

    fn advance(&mut self) -> &Token {
        let index = self.cursor;
        if !self.at(&TokenKind::Eof) {
            self.cursor += 1;
        }
        &self.tokens[index]
    }

    fn error_here(&mut self, code: &'static str, message: impl Into<String>) {
        self.diagnostics.push(ParseDiagnostic {
            code,
            message: message.into(),
            span: self.current().span,
        });
    }
}

fn positional_format(
    template: &str,
    arguments: Vec<Expression>,
    c_style: bool,
    span: Span,
) -> Result<Expression, String> {
    let mut arguments = arguments.into_iter();
    let mut segments = Vec::new();
    let mut text = String::new();
    let characters: Vec<char> = template.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        if c_style && characters[index] == '%' {
            if characters.get(index + 1) == Some(&'%') {
                text.push('%');
                index += 2;
                continue;
            }
            if matches!(characters.get(index + 1), Some('d' | 'i' | 's' | 'f')) {
                flush_format_text(&mut segments, &mut text);
                let value = arguments
                    .next()
                    .ok_or_else(|| "not enough values for printf placeholders".to_owned())?;
                segments.push(FormatSegment::Value(value));
                index += 2;
                continue;
            }
            return Err("unsupported or incomplete printf placeholder".to_owned());
        }
        if !c_style && characters[index] == '{' {
            if characters.get(index + 1) == Some(&'{') {
                text.push('{');
                index += 2;
                continue;
            }
            if characters.get(index + 1) == Some(&'}') {
                flush_format_text(&mut segments, &mut text);
                let value = arguments
                    .next()
                    .ok_or_else(|| "not enough values for `{}` placeholders".to_owned())?;
                segments.push(FormatSegment::Value(value));
                index += 2;
                continue;
            }
            return Err("only `{}` positional placeholders are supported".to_owned());
        }
        if !c_style && characters[index] == '}' && characters.get(index + 1) == Some(&'}') {
            text.push('}');
            index += 2;
            continue;
        }
        text.push(characters[index]);
        index += 1;
    }
    flush_format_text(&mut segments, &mut text);
    if arguments.next().is_some() {
        return Err("too many values for format placeholders".to_owned());
    }
    Ok(Expression::format(segments, span))
}

fn named_format(template: &str, javascript: bool, span: Span) -> Expression {
    let mut segments = Vec::new();
    let mut text = String::new();
    let characters: Vec<char> = template.chars().collect();
    let mut index = 0;
    while index < characters.len() {
        let opening = if javascript {
            characters[index] == '$' && characters.get(index + 1) == Some(&'{')
        } else {
            characters[index] == '{' && characters.get(index + 1) != Some(&'{')
        };
        if !opening {
            if !javascript && characters[index] == '{' && characters.get(index + 1) == Some(&'{') {
                text.push('{');
                index += 2;
            } else if !javascript
                && characters[index] == '}'
                && characters.get(index + 1) == Some(&'}')
            {
                text.push('}');
                index += 2;
            } else {
                text.push(characters[index]);
                index += 1;
            }
            continue;
        }

        flush_format_text(&mut segments, &mut text);
        index += if javascript { 2 } else { 1 };
        let name_start = index;
        while index < characters.len() && characters[index] != '}' {
            index += 1;
        }
        if index == characters.len() {
            return Expression::new(
                ExpressionKind::Error {
                    message: "unterminated interpolation placeholder".to_owned(),
                },
                span,
            );
        }
        let name: String = characters[name_start..index].iter().collect();
        if !is_simple_identifier(&name) {
            return Expression::new(
                ExpressionKind::Error {
                    message: "interpolation supports simple variable names only".to_owned(),
                },
                span,
            );
        }
        segments.push(FormatSegment::Value(Expression::new(
            ExpressionKind::Variable { name },
            span,
        )));
        index += 1;
    }
    flush_format_text(&mut segments, &mut text);
    Expression::format(segments, span)
}

fn flush_format_text(segments: &mut Vec<FormatSegment>, text: &mut String) {
    if !text.is_empty() {
        segments.push(FormatSegment::Text(std::mem::take(text)));
    }
}

fn is_simple_identifier(value: &str) -> bool {
    let mut characters = value.chars();
    characters
        .next()
        .is_some_and(|character| character == '_' || character.is_alphabetic())
        && characters.all(|character| character == '_' || character.is_alphanumeric())
}

fn merge(start: Span, end: Span) -> Span {
    Span::new(
        start.source,
        start.start.min(end.start),
        start.end.max(end.end),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(text: &str) -> ParsedUnit {
        Parser::parse(SourceId(0), 0, text, &mut HashSet::new(), false)
    }

    fn parse_python(text: &str) -> ParsedUnit {
        Parser::parse(SourceId(0), 0, text, &mut HashSet::new(), true)
    }

    #[test]
    fn parses_case_insensitive_bare_print() {
        let unit = parse("PRINT 1\n");
        assert!(unit.is_complete(), "{:?}", unit.diagnostics);
        assert!(matches!(
            unit.block.statements[0].kind,
            StatementKind::Print(_)
        ));
    }

    #[test]
    fn parses_javascript_console_log() {
        let unit = parse("console.log(\"Hello\");");
        assert!(unit.is_complete(), "{:?}", unit.diagnostics);
        assert!(matches!(
            unit.block.statements[0].kind,
            StatementKind::Print(_)
        ));
    }

    #[test]
    fn parses_c_main_and_printf() {
        let unit = parse("int main() { printf(\"Hello\"); return 0; }");
        assert!(unit.is_complete(), "{:?}", unit.diagnostics);
        assert_eq!(unit.functions.len(), 1);
        assert_eq!(unit.functions[0].name, "main");
        assert!(matches!(
            unit.functions[0].body.statements[0].kind,
            StatementKind::Print(_)
        ));
    }

    #[test]
    fn parses_cross_language_format_forms() {
        for source in [
            "x = 7\nprint(f\"value={x}\")\n",
            "let x = 7; console.log(`value=${x}`);",
            "let x = 7; println!(\"value={}\", x);",
            "int x = 7; printf(\"value=%d\", x);",
        ] {
            let unit = parse(source);
            assert!(unit.is_complete(), "{source}: {:?}", unit.diagnostics);
            assert!(matches!(
                unit.block.statements[1].kind,
                StatementKind::Print(Expression {
                    kind: ExpressionKind::Format { .. },
                    ..
                })
            ));
        }
    }

    #[test]
    fn parses_python_indentation_and_range_loop() {
        let unit = parse_python(
            "total = 0\nfor i in range(1, 4):\n    total = total + i\n    print(i)\nprint(total)\n",
        );
        assert!(unit.is_complete(), "{:?}", unit.diagnostics);
        assert_eq!(unit.block.statements.len(), 4);
        let StatementKind::Loop { body, .. } = &unit.block.statements[2].kind else {
            panic!("range should lower to a condition-controlled loop");
        };
        assert_eq!(body.statements.len(), 3);
    }

    #[test]
    fn rejects_unindented_python_block() {
        let unit = parse_python("if true:\nprint(1)\n");
        assert_eq!(unit.diagnostics[0].code, "LXT-132");
    }

    #[test]
    fn keeps_partial_ast_on_error() {
        let unit = parse("print(\"Hello\"");
        assert!(!unit.is_complete());
        assert_eq!(unit.block.statements.len(), 1);
    }

    #[test]
    fn parses_while_block() {
        let unit = parse("let x = 0\nwhile x < 2 { print(x); x = x + 1; }");
        assert!(unit.is_complete(), "{:?}", unit.diagnostics);
        assert_eq!(unit.block.statements.len(), 2);
    }
}
