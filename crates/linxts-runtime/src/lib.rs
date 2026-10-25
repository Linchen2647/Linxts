//! Tree-walking VM backend for Linxts Unified IR v0.1.

use linxts_ir::{
    BinaryOperator, Block, Expression, ExpressionKind, Function, Literal, Module, StatementKind,
    UnaryOperator,
};
use linxts_source::Span;
use std::collections::HashMap;
use std::io::Write;
use thiserror::Error;

const MAX_LOOP_ITERATIONS: usize = 1_000_000;
const MAX_CALL_DEPTH: usize = 128;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Value {
    Integer(i64),
    String(String),
    Boolean(bool),
    Unit,
}

impl std::fmt::Display for Value {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Integer(value) => write!(formatter, "{value}"),
            Self::String(value) => formatter.write_str(value),
            Self::Boolean(value) => write!(formatter, "{value}"),
            Self::Unit => Ok(()),
        }
    }
}

#[derive(Debug, Error)]
pub enum RuntimeError {
    #[error("LXT-VM-001: variable `{name}` is not defined")]
    UndefinedVariable { name: String, span: Span },
    #[error("LXT-VM-002: cannot apply {operator} to {left} and {right}")]
    InvalidBinaryOperation {
        operator: &'static str,
        left: &'static str,
        right: &'static str,
        span: Span,
    },
    #[error("LXT-VM-003: division by zero")]
    DivisionByZero { span: Span },
    #[error("LXT-VM-004: expected a boolean condition, got {actual}")]
    InvalidCondition { actual: &'static str, span: Span },
    #[error("LXT-VM-005: function `{name}` is not defined")]
    UndefinedFunction { name: String, span: Span },
    #[error("LXT-VM-006: function `{name}` expects {expected} arguments, got {actual}")]
    WrongArgumentCount {
        name: String,
        expected: usize,
        actual: usize,
        span: Span,
    },
    #[error("LXT-VM-007: loop iteration limit ({MAX_LOOP_ITERATIONS}) exceeded")]
    LoopLimit { span: Span },
    #[error("LXT-VM-008: function call depth limit ({MAX_CALL_DEPTH}) exceeded")]
    CallDepth { span: Span },
    #[error("LXT-VM-009: unresolved error node reached execution: {message}")]
    ErrorNode { message: String, span: Span },
    #[error("LXT-VM-010: output failed: {0}")]
    Output(#[from] std::io::Error),
}

pub struct Vm<'a, W: Write> {
    output: &'a mut W,
    scopes: Vec<HashMap<String, Value>>,
    functions: HashMap<String, Function>,
    call_depth: usize,
}

impl<'a, W: Write> Vm<'a, W> {
    #[must_use]
    pub fn new(output: &'a mut W) -> Self {
        Self {
            output,
            scopes: vec![HashMap::new()],
            functions: HashMap::new(),
            call_depth: 0,
        }
    }

    pub fn execute(&mut self, module: &Module) -> Result<Value, RuntimeError> {
        self.functions = module
            .functions
            .iter()
            .map(|function| (function.name.clone(), function.clone()))
            .collect();
        if let Some(value) = self.execute_block(&module.body)? {
            return Ok(value);
        }
        if self.functions.contains_key("main") {
            self.call_function("main", Vec::new(), module.functions[0].span)
        } else {
            Ok(Value::Unit)
        }
    }

    fn execute_block(&mut self, block: &Block) -> Result<Option<Value>, RuntimeError> {
        for statement in &block.statements {
            match &statement.kind {
                StatementKind::Variable { name, value, .. } => {
                    let value = self.evaluate(value)?;
                    self.scopes
                        .last_mut()
                        .expect("VM always has a scope")
                        .insert(name.clone(), value);
                }
                StatementKind::Assignment { name, value } => {
                    let value = self.evaluate(value)?;
                    let Some(scope) = self
                        .scopes
                        .iter_mut()
                        .rev()
                        .find(|scope| scope.contains_key(name))
                    else {
                        return Err(RuntimeError::UndefinedVariable {
                            name: name.clone(),
                            span: statement.span,
                        });
                    };
                    scope.insert(name.clone(), value);
                }
                StatementKind::Print(expression) => {
                    let value = self.evaluate(expression)?;
                    writeln!(self.output, "{value}")?;
                }
                StatementKind::Expression(expression) => {
                    self.evaluate(expression)?;
                }
                StatementKind::Conditional {
                    condition,
                    then_block,
                    else_block,
                } => {
                    let condition = self.evaluate(condition)?;
                    let selected = match condition {
                        Value::Boolean(true) => Some(then_block),
                        Value::Boolean(false) => else_block.as_ref(),
                        other => {
                            return Err(RuntimeError::InvalidCondition {
                                actual: type_name(&other),
                                span: statement.span,
                            });
                        }
                    };
                    if let Some(selected) = selected {
                        self.scopes.push(HashMap::new());
                        let result = self.execute_block(selected);
                        self.scopes.pop();
                        if let Some(value) = result? {
                            return Ok(Some(value));
                        }
                    }
                }
                StatementKind::Loop { condition, body } => {
                    for iteration in 0..=MAX_LOOP_ITERATIONS {
                        let value = self.evaluate(condition)?;
                        match value {
                            Value::Boolean(false) => break,
                            Value::Boolean(true) => {}
                            other => {
                                return Err(RuntimeError::InvalidCondition {
                                    actual: type_name(&other),
                                    span: condition.span,
                                });
                            }
                        }
                        if iteration == MAX_LOOP_ITERATIONS {
                            return Err(RuntimeError::LoopLimit {
                                span: statement.span,
                            });
                        }
                        self.scopes.push(HashMap::new());
                        let result = self.execute_block(body);
                        self.scopes.pop();
                        if let Some(value) = result? {
                            return Ok(Some(value));
                        }
                    }
                }
                StatementKind::Return(value) => {
                    return value.as_ref().map_or(Ok(Some(Value::Unit)), |value| {
                        self.evaluate(value).map(Some)
                    });
                }
            }
        }
        Ok(None)
    }

    fn evaluate(&mut self, expression: &Expression) -> Result<Value, RuntimeError> {
        match &expression.kind {
            ExpressionKind::Literal(literal) => Ok(match literal {
                Literal::Integer(value) => Value::Integer(*value),
                Literal::String(value) => Value::String(value.clone()),
                Literal::Boolean(value) => Value::Boolean(*value),
                Literal::Unit => Value::Unit,
            }),
            ExpressionKind::Variable { name } => self
                .scopes
                .iter()
                .rev()
                .find_map(|scope| scope.get(name).cloned())
                .ok_or_else(|| RuntimeError::UndefinedVariable {
                    name: name.clone(),
                    span: expression.span,
                }),
            ExpressionKind::Binary {
                operator,
                left,
                right,
            } => {
                let left = self.evaluate(left)?;
                if *operator == BinaryOperator::And && left == Value::Boolean(false) {
                    return Ok(Value::Boolean(false));
                }
                if *operator == BinaryOperator::Or && left == Value::Boolean(true) {
                    return Ok(Value::Boolean(true));
                }
                let right = self.evaluate(right)?;
                apply_binary(*operator, left, right, expression.span)
            }
            ExpressionKind::Unary { operator, operand } => {
                let value = self.evaluate(operand)?;
                match (operator, value) {
                    (UnaryOperator::Negate, Value::Integer(value)) => {
                        Ok(Value::Integer(value.saturating_neg()))
                    }
                    (UnaryOperator::Not, Value::Boolean(value)) => Ok(Value::Boolean(!value)),
                    (operator, value) => Err(RuntimeError::InvalidBinaryOperation {
                        operator: match operator {
                            UnaryOperator::Negate => "unary -",
                            UnaryOperator::Not => "not",
                        },
                        left: type_name(&value),
                        right: "unit",
                        span: expression.span,
                    }),
                }
            }
            ExpressionKind::Call { callee, arguments } => {
                let arguments = arguments
                    .iter()
                    .map(|argument| self.evaluate(argument))
                    .collect::<Result<Vec<_>, _>>()?;
                self.call_function(callee, arguments, expression.span)
            }
            ExpressionKind::Error { message } => Err(RuntimeError::ErrorNode {
                message: message.clone(),
                span: expression.span,
            }),
        }
    }

    fn call_function(
        &mut self,
        name: &str,
        arguments: Vec<Value>,
        span: Span,
    ) -> Result<Value, RuntimeError> {
        if self.call_depth >= MAX_CALL_DEPTH {
            return Err(RuntimeError::CallDepth { span });
        }
        let function =
            self.functions
                .get(name)
                .cloned()
                .ok_or_else(|| RuntimeError::UndefinedFunction {
                    name: name.to_owned(),
                    span,
                })?;
        if function.parameters.len() != arguments.len() {
            return Err(RuntimeError::WrongArgumentCount {
                name: name.to_owned(),
                expected: function.parameters.len(),
                actual: arguments.len(),
                span,
            });
        }
        let scope = function
            .parameters
            .iter()
            .zip(arguments)
            .map(|(parameter, value)| (parameter.name.clone(), value))
            .collect();
        self.scopes.push(scope);
        self.call_depth += 1;
        let result = self.execute_block(&function.body);
        self.call_depth -= 1;
        self.scopes.pop();
        Ok(result?.unwrap_or(Value::Unit))
    }
}

fn apply_binary(
    operator: BinaryOperator,
    left: Value,
    right: Value,
    span: Span,
) -> Result<Value, RuntimeError> {
    let left_type = type_name(&left);
    let right_type = type_name(&right);
    let result = match (operator, left, right) {
        (BinaryOperator::Add, Value::Integer(left), Value::Integer(right)) => {
            Value::Integer(left.saturating_add(right))
        }
        (BinaryOperator::Add, Value::String(left), Value::String(right)) => {
            Value::String(left + &right)
        }
        (BinaryOperator::Subtract, Value::Integer(left), Value::Integer(right)) => {
            Value::Integer(left.saturating_sub(right))
        }
        (BinaryOperator::Multiply, Value::Integer(left), Value::Integer(right)) => {
            Value::Integer(left.saturating_mul(right))
        }
        (BinaryOperator::Divide, Value::Integer(_), Value::Integer(0))
        | (BinaryOperator::Remainder, Value::Integer(_), Value::Integer(0)) => {
            return Err(RuntimeError::DivisionByZero { span });
        }
        (BinaryOperator::Divide, Value::Integer(left), Value::Integer(right)) => {
            Value::Integer(left / right)
        }
        (BinaryOperator::Remainder, Value::Integer(left), Value::Integer(right)) => {
            Value::Integer(left % right)
        }
        (BinaryOperator::Equal, left, right) => Value::Boolean(left == right),
        (BinaryOperator::NotEqual, left, right) => Value::Boolean(left != right),
        (BinaryOperator::Less, Value::Integer(left), Value::Integer(right)) => {
            Value::Boolean(left < right)
        }
        (BinaryOperator::LessEqual, Value::Integer(left), Value::Integer(right)) => {
            Value::Boolean(left <= right)
        }
        (BinaryOperator::Greater, Value::Integer(left), Value::Integer(right)) => {
            Value::Boolean(left > right)
        }
        (BinaryOperator::GreaterEqual, Value::Integer(left), Value::Integer(right)) => {
            Value::Boolean(left >= right)
        }
        (BinaryOperator::And, Value::Boolean(left), Value::Boolean(right)) => {
            Value::Boolean(left && right)
        }
        (BinaryOperator::Or, Value::Boolean(left), Value::Boolean(right)) => {
            Value::Boolean(left || right)
        }
        (operator, _, _) => {
            return Err(RuntimeError::InvalidBinaryOperation {
                operator: operator_name(operator),
                left: left_type,
                right: right_type,
                span,
            });
        }
    };
    Ok(result)
}

fn operator_name(operator: BinaryOperator) -> &'static str {
    match operator {
        BinaryOperator::Add => "+",
        BinaryOperator::Subtract => "-",
        BinaryOperator::Multiply => "*",
        BinaryOperator::Divide => "/",
        BinaryOperator::Remainder => "%",
        BinaryOperator::Equal => "==",
        BinaryOperator::NotEqual => "!=",
        BinaryOperator::Less => "<",
        BinaryOperator::LessEqual => "<=",
        BinaryOperator::Greater => ">",
        BinaryOperator::GreaterEqual => ">=",
        BinaryOperator::And => "and",
        BinaryOperator::Or => "or",
    }
}

fn type_name(value: &Value) -> &'static str {
    match value {
        Value::Integer(_) => "integer",
        Value::String(_) => "string",
        Value::Boolean(_) => "boolean",
        Value::Unit => "unit",
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use linxts_ir::{Expression, Module, Statement};
    use linxts_source::{SourceId, Span};

    #[test]
    fn executes_print() {
        let span = Span::empty(SourceId(0), 0);
        let mut module = Module::new("hash");
        module.body.statements.push(Statement::new(
            StatementKind::Print(Expression::integer(1, span)),
            span,
        ));
        let mut output = Vec::new();
        Vm::new(&mut output).execute(&module).expect("execute");
        assert_eq!(String::from_utf8(output).expect("UTF-8"), "1\n");
    }
}
