//! Linxts Unified IR v0.1 and its trust-boundary validator.

use linxts_source::Span;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

pub const IR_SCHEMA_VERSION: &str = "0.1";
pub const MAX_IR_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Module {
    pub schema_version: String,
    pub source_hash: String,
    pub body: Block,
    pub functions: Vec<Function>,
}

impl Module {
    #[must_use]
    pub fn new(source_hash: impl Into<String>) -> Self {
        Self {
            schema_version: IR_SCHEMA_VERSION.to_owned(),
            source_hash: source_hash.into(),
            body: Block::default(),
            functions: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Block {
    pub statements: Vec<Statement>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Function {
    pub name: String,
    pub parameters: Vec<Parameter>,
    pub body: Block,
    pub return_type: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Parameter {
    pub name: String,
    pub ty: Type,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Statement {
    pub kind: StatementKind,
    pub span: Span,
    pub effect: Effect,
}

impl Statement {
    #[must_use]
    pub fn new(kind: StatementKind, span: Span) -> Self {
        let effect = match &kind {
            StatementKind::Print(_) => Effect::Output,
            StatementKind::Expression(expression) => expression.effect,
            _ => Effect::Pure,
        };
        Self { kind, span, effect }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StatementKind {
    Variable {
        name: String,
        value: Expression,
        mutable: bool,
    },
    Assignment {
        name: String,
        value: Expression,
    },
    Print(Expression),
    Expression(Expression),
    Conditional {
        condition: Expression,
        then_block: Block,
        else_block: Option<Block>,
    },
    Loop {
        condition: Expression,
        body: Block,
    },
    Return(Option<Expression>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Expression {
    pub kind: ExpressionKind,
    pub span: Span,
    pub ty: Type,
    pub effect: Effect,
}

impl Expression {
    #[must_use]
    pub fn new(kind: ExpressionKind, span: Span) -> Self {
        Self {
            kind,
            span,
            ty: Type::Unknown,
            effect: Effect::Pure,
        }
    }

    #[must_use]
    pub fn integer(value: i64, span: Span) -> Self {
        Self {
            kind: ExpressionKind::Literal(Literal::Integer(value)),
            span,
            ty: Type::Integer,
            effect: Effect::Pure,
        }
    }

    #[must_use]
    pub fn string(value: impl Into<String>, span: Span) -> Self {
        Self {
            kind: ExpressionKind::Literal(Literal::String(value.into())),
            span,
            ty: Type::String,
            effect: Effect::Pure,
        }
    }

    #[must_use]
    pub fn boolean(value: bool, span: Span) -> Self {
        Self {
            kind: ExpressionKind::Literal(Literal::Boolean(value)),
            span,
            ty: Type::Boolean,
            effect: Effect::Pure,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ExpressionKind {
    Literal(Literal),
    Variable {
        name: String,
    },
    Binary {
        operator: BinaryOperator,
        left: Box<Expression>,
        right: Box<Expression>,
    },
    Unary {
        operator: UnaryOperator,
        operand: Box<Expression>,
    },
    Call {
        callee: String,
        arguments: Vec<Expression>,
    },
    Error {
        message: String,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum Literal {
    Integer(i64),
    String(String),
    Boolean(bool),
    Unit,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BinaryOperator {
    Add,
    Subtract,
    Multiply,
    Divide,
    Remainder,
    Equal,
    NotEqual,
    Less,
    LessEqual,
    Greater,
    GreaterEqual,
    And,
    Or,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum UnaryOperator {
    Negate,
    Not,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Type {
    Integer,
    String,
    Boolean,
    Unit,
    Unknown,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Effect {
    Pure,
    Output,
    External,
    Unknown,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ValidationError {
    pub code: &'static str,
    pub message: String,
    pub span: Option<Span>,
}

#[derive(Debug, Default)]
pub struct Validator {
    errors: Vec<ValidationError>,
    functions: HashMap<String, usize>,
}

impl Validator {
    #[must_use]
    pub fn validate(module: &Module) -> Vec<ValidationError> {
        let mut validator = Self::default();
        validator.validate_module(module);
        validator.errors
    }

    fn validate_module(&mut self, module: &Module) {
        if module.schema_version != IR_SCHEMA_VERSION {
            self.error(
                "LXT-IR-001",
                format!(
                    "unsupported IR schema {}; expected {IR_SCHEMA_VERSION}",
                    module.schema_version
                ),
                None,
            );
        }
        for function in &module.functions {
            if self
                .functions
                .insert(function.name.clone(), function.parameters.len())
                .is_some()
            {
                self.error(
                    "LXT-IR-002",
                    format!("duplicate function `{}`", function.name),
                    Some(function.span),
                );
            }
        }
        self.validate_block(&module.body, &mut Vec::new(), 0, false);
        for function in &module.functions {
            let mut scopes = vec![HashSet::new()];
            for parameter in &function.parameters {
                if !scopes[0].insert(parameter.name.clone()) {
                    self.error(
                        "LXT-IR-003",
                        format!("duplicate parameter `{}`", parameter.name),
                        Some(parameter.span),
                    );
                }
            }
            self.validate_block(&function.body, &mut scopes, 0, true);
        }
    }

    fn validate_block(
        &mut self,
        block: &Block,
        scopes: &mut Vec<HashSet<String>>,
        depth: usize,
        in_function: bool,
    ) {
        if depth > MAX_IR_DEPTH {
            self.error(
                "LXT-IR-004",
                format!("IR nesting exceeds limit of {MAX_IR_DEPTH}"),
                None,
            );
            return;
        }
        scopes.push(HashSet::new());
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Variable { name, value, .. } => {
                    self.validate_expression(value, scopes, depth + 1);
                    if !scopes
                        .last_mut()
                        .expect("scope exists")
                        .insert(name.clone())
                    {
                        self.error(
                            "LXT-IR-005",
                            format!("variable `{name}` is already declared in this scope"),
                            Some(statement.span),
                        );
                    }
                }
                StatementKind::Assignment { name, value } => {
                    if !is_declared(scopes, name) {
                        self.error(
                            "LXT-IR-006",
                            format!("assignment to undeclared variable `{name}`"),
                            Some(statement.span),
                        );
                    }
                    self.validate_expression(value, scopes, depth + 1);
                }
                StatementKind::Print(expression) | StatementKind::Expression(expression) => {
                    self.validate_expression(expression, scopes, depth + 1);
                }
                StatementKind::Conditional {
                    condition,
                    then_block,
                    else_block,
                } => {
                    self.validate_expression(condition, scopes, depth + 1);
                    self.validate_block(then_block, scopes, depth + 1, in_function);
                    if let Some(block) = else_block {
                        self.validate_block(block, scopes, depth + 1, in_function);
                    }
                }
                StatementKind::Loop { condition, body } => {
                    self.validate_expression(condition, scopes, depth + 1);
                    self.validate_block(body, scopes, depth + 1, in_function);
                }
                StatementKind::Return(expression) => {
                    if !in_function {
                        self.error(
                            "LXT-IR-007",
                            "return outside a function".to_owned(),
                            Some(statement.span),
                        );
                    }
                    if let Some(expression) = expression {
                        self.validate_expression(expression, scopes, depth + 1);
                    }
                }
            }
        }
        scopes.pop();
    }

    fn validate_expression(
        &mut self,
        expression: &Expression,
        scopes: &[HashSet<String>],
        depth: usize,
    ) {
        if depth > MAX_IR_DEPTH {
            self.error(
                "LXT-IR-004",
                format!("IR nesting exceeds limit of {MAX_IR_DEPTH}"),
                Some(expression.span),
            );
            return;
        }
        match &expression.kind {
            ExpressionKind::Literal(_) => {}
            ExpressionKind::Variable { name } => {
                if !is_declared(scopes, name) {
                    self.error(
                        "LXT-IR-008",
                        format!("use of undeclared variable `{name}`"),
                        Some(expression.span),
                    );
                }
            }
            ExpressionKind::Binary { left, right, .. } => {
                self.validate_expression(left, scopes, depth + 1);
                self.validate_expression(right, scopes, depth + 1);
            }
            ExpressionKind::Unary { operand, .. } => {
                self.validate_expression(operand, scopes, depth + 1);
            }
            ExpressionKind::Call { callee, arguments } => {
                let Some(expected) = self.functions.get(callee).copied() else {
                    self.error(
                        "LXT-IR-009",
                        format!("call to unknown function `{callee}`"),
                        Some(expression.span),
                    );
                    return;
                };
                if expected != arguments.len() {
                    self.error(
                        "LXT-IR-010",
                        format!(
                            "function `{callee}` expects {expected} arguments, got {}",
                            arguments.len()
                        ),
                        Some(expression.span),
                    );
                }
                for argument in arguments {
                    self.validate_expression(argument, scopes, depth + 1);
                }
            }
            ExpressionKind::Error { message } => self.error(
                "LXT-IR-011",
                format!("unresolved parser error node: {message}"),
                Some(expression.span),
            ),
        }
    }

    fn error(&mut self, code: &'static str, message: String, span: Option<Span>) {
        self.errors.push(ValidationError {
            code,
            message,
            span,
        });
    }
}

fn is_declared(scopes: &[HashSet<String>], name: &str) -> bool {
    scopes.iter().rev().any(|scope| scope.contains(name))
}

#[cfg(test)]
mod tests {
    use super::*;
    use linxts_source::SourceId;

    #[test]
    fn rejects_undeclared_variables() {
        let span = Span::empty(SourceId(0), 0);
        let mut module = Module::new("hash");
        module.body.statements.push(Statement::new(
            StatementKind::Print(Expression::new(
                ExpressionKind::Variable {
                    name: "missing".to_owned(),
                },
                span,
            )),
            span,
        ));
        let errors = Validator::validate(&module);
        assert_eq!(errors[0].code, "LXT-IR-008");
    }

    #[test]
    fn accepts_declared_variables() {
        let span = Span::empty(SourceId(0), 0);
        let mut module = Module::new("hash");
        module.body.statements.push(Statement::new(
            StatementKind::Variable {
                name: "x".to_owned(),
                value: Expression::integer(1, span),
                mutable: true,
            },
            span,
        ));
        module.body.statements.push(Statement::new(
            StatementKind::Print(Expression::new(
                ExpressionKind::Variable {
                    name: "x".to_owned(),
                },
                span,
            )),
            span,
        ));
        assert!(Validator::validate(&module).is_empty());
    }
}
