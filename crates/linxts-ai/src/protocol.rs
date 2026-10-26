use linxts_ir::{
    Block, Effect, Expression, ExpressionKind, Module, StatementKind, ValidationError, Validator,
};
use serde::{Deserialize, Serialize};
use thiserror::Error;

pub const AI_SCHEMA_VERSION: &str = "1";
pub const MAX_ASSUMPTIONS: usize = 32;
pub const MAX_WARNINGS: usize = 32;
pub const MAX_INTERPRETATION_BYTES: usize = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateStatus {
    Candidate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CandidateLanguage {
    Python,
    Javascript,
    Rust,
    C,
    Natural,
    Generic,
    Mixed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProviderMetadata {
    pub kind: String,
    pub model_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CandidateEnvelope {
    pub schema_version: String,
    pub status: CandidateStatus,
    pub language: CandidateLanguage,
    pub interpretation: String,
    pub ir: Module,
    #[serde(default)]
    pub assumptions: Vec<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
    #[serde(default)]
    pub provider: Option<ProviderMetadata>,
}

impl CandidateEnvelope {
    pub fn validate(&self, expected_source_hash: &str) -> Result<(), CandidateValidationError> {
        if self.schema_version != AI_SCHEMA_VERSION {
            return Err(CandidateValidationError::SchemaVersion {
                actual: self.schema_version.clone(),
            });
        }
        if self.interpretation.trim().is_empty()
            || self.interpretation.len() > MAX_INTERPRETATION_BYTES
        {
            return Err(CandidateValidationError::InterpretationLength);
        }
        if self.assumptions.len() > MAX_ASSUMPTIONS {
            return Err(CandidateValidationError::TooManyAssumptions);
        }
        if self.warnings.len() > MAX_WARNINGS {
            return Err(CandidateValidationError::TooManyWarnings);
        }
        if self.ir.source_hash != expected_source_hash {
            return Err(CandidateValidationError::SourceHashMismatch);
        }
        ensure_safe_effects(&self.ir.body)?;
        for function in &self.ir.functions {
            ensure_safe_effects(&function.body)?;
        }
        let errors = Validator::validate(&self.ir);
        if !errors.is_empty() {
            return Err(CandidateValidationError::InvalidIr(errors));
        }
        Ok(())
    }

    pub fn set_provider(&mut self, kind: impl Into<String>, model_id: impl Into<String>) {
        self.provider = Some(ProviderMetadata {
            kind: kind.into(),
            model_id: model_id.into(),
        });
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum CandidateValidationError {
    #[error("unsupported AI schema version `{actual}`")]
    SchemaVersion { actual: String },
    #[error("interpretation must contain 1..={MAX_INTERPRETATION_BYTES} bytes")]
    InterpretationLength,
    #[error("candidate contains more than {MAX_ASSUMPTIONS} assumptions")]
    TooManyAssumptions,
    #[error("candidate contains more than {MAX_WARNINGS} warnings")]
    TooManyWarnings,
    #[error("candidate source hash does not match the active source snapshot")]
    SourceHashMismatch,
    #[error("candidate contains an external or unknown side effect")]
    UnsafeEffect,
    #[error("candidate IR failed validation")]
    InvalidIr(Vec<ValidationError>),
}

fn ensure_safe_effects(block: &Block) -> Result<(), CandidateValidationError> {
    for statement in &block.statements {
        safe_effect(statement.effect)?;
        match &statement.kind {
            StatementKind::Variable { value, .. }
            | StatementKind::Assignment { value, .. }
            | StatementKind::Print(value)
            | StatementKind::Expression(value) => ensure_expression_effects(value)?,
            StatementKind::Conditional {
                condition,
                then_block,
                else_block,
            } => {
                ensure_expression_effects(condition)?;
                ensure_safe_effects(then_block)?;
                if let Some(else_block) = else_block {
                    ensure_safe_effects(else_block)?;
                }
            }
            StatementKind::Loop { condition, body } => {
                ensure_expression_effects(condition)?;
                ensure_safe_effects(body)?;
            }
            StatementKind::Return(value) => {
                if let Some(value) = value {
                    ensure_expression_effects(value)?;
                }
            }
        }
    }
    Ok(())
}

fn ensure_expression_effects(expression: &Expression) -> Result<(), CandidateValidationError> {
    safe_effect(expression.effect)?;
    match &expression.kind {
        ExpressionKind::Binary { left, right, .. } => {
            ensure_expression_effects(left)?;
            ensure_expression_effects(right)?;
        }
        ExpressionKind::Unary { operand, .. } => ensure_expression_effects(operand)?,
        ExpressionKind::Call { arguments, .. } => {
            for argument in arguments {
                ensure_expression_effects(argument)?;
            }
        }
        ExpressionKind::Literal(_)
        | ExpressionKind::Variable { .. }
        | ExpressionKind::Error { .. } => {}
    }
    Ok(())
}

fn safe_effect(effect: Effect) -> Result<(), CandidateValidationError> {
    match effect {
        Effect::Pure | Effect::Output => Ok(()),
        Effect::External | Effect::Unknown => Err(CandidateValidationError::UnsafeEffect),
    }
}

#[must_use]
pub fn candidate_json_schema() -> serde_json::Value {
    serde_json::json!({
        "$schema": "https://json-schema.org/draft/2020-12/schema",
        "type": "object",
        "additionalProperties": false,
        "required": [
            "schema_version", "status", "language", "interpretation", "ir",
            "assumptions", "warnings"
        ],
        "properties": {
            "schema_version": {"const": AI_SCHEMA_VERSION},
            "status": {"const": "candidate"},
            "language": {
                "enum": ["python", "javascript", "rust", "c", "natural", "generic", "mixed"]
            },
            "interpretation": {"type": "string", "minLength": 1, "maxLength": 4096},
            "ir": {"$ref": "#/$defs/module"},
            "assumptions": {
                "type": "array", "maxItems": MAX_ASSUMPTIONS,
                "items": {"type": "string", "maxLength": 4096}
            },
            "warnings": {
                "type": "array", "maxItems": MAX_WARNINGS,
                "items": {"type": "string", "maxLength": 4096}
            },
            "provider": {
                "oneOf": [
                    {"type": "null"},
                    {
                        "type": "object",
                        "properties": {
                            "kind": {"type": "string", "maxLength": 128},
                            "model_id": {"type": "string", "maxLength": 512}
                        },
                        "required": ["kind", "model_id"],
                        "additionalProperties": false
                    }
                ]
            }
        },
        "$defs": {
            "span": {
                "type": "object",
                "additionalProperties": false,
                "required": ["source", "start", "end"],
                "properties": {
                    "source": {"type": "integer", "minimum": 0},
                    "start": {"type": "integer", "minimum": 0},
                    "end": {"type": "integer", "minimum": 0}
                }
            },
            "type": {
                "enum": ["integer", "string", "boolean", "unit", "unknown"]
            },
            "effect": {"enum": ["pure", "output"]},
            "literal": {
                "oneOf": [
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "value"],
                        "properties": {
                            "kind": {"const": "integer"},
                            "value": {"type": "integer"}
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "value"],
                        "properties": {
                            "kind": {"const": "string"},
                            "value": {"type": "string"}
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "value"],
                        "properties": {
                            "kind": {"const": "boolean"},
                            "value": {"type": "boolean"}
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind"],
                        "properties": {"kind": {"const": "unit"}}
                    }
                ]
            },
            "expression": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "span", "ty", "effect"],
                "properties": {
                    "kind": {"$ref": "#/$defs/expressionKind"},
                    "span": {"$ref": "#/$defs/span"},
                    "ty": {"$ref": "#/$defs/type"},
                    "effect": {"$ref": "#/$defs/effect"}
                }
            },
            "expressionKind": {
                "oneOf": [
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "literal"},
                            "data": {"$ref": "#/$defs/literal"}
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "variable"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["name"],
                                "properties": {"name": {"type": "string", "minLength": 1}}
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "binary"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["operator", "left", "right"],
                                "properties": {
                                    "operator": {"enum": [
                                        "add", "subtract", "multiply", "divide", "remainder",
                                        "equal", "not_equal", "less", "less_equal", "greater",
                                        "greater_equal", "and", "or"
                                    ]},
                                    "left": {"$ref": "#/$defs/expression"},
                                    "right": {"$ref": "#/$defs/expression"}
                                }
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "unary"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["operator", "operand"],
                                "properties": {
                                    "operator": {"enum": ["negate", "not"]},
                                    "operand": {"$ref": "#/$defs/expression"}
                                }
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "call"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["callee", "arguments"],
                                "properties": {
                                    "callee": {"type": "string", "minLength": 1},
                                    "arguments": {
                                        "type": "array", "maxItems": 256,
                                        "items": {"$ref": "#/$defs/expression"}
                                    }
                                }
                            }
                        }
                    }
                ]
            },
            "statement": {
                "type": "object",
                "additionalProperties": false,
                "required": ["kind", "span", "effect"],
                "properties": {
                    "kind": {"$ref": "#/$defs/statementKind"},
                    "span": {"$ref": "#/$defs/span"},
                    "effect": {"$ref": "#/$defs/effect"}
                }
            },
            "block": {
                "type": "object", "additionalProperties": false,
                "required": ["statements"],
                "properties": {
                    "statements": {
                        "type": "array", "maxItems": 100000,
                        "items": {"$ref": "#/$defs/statement"}
                    }
                }
            },
            "statementKind": {
                "oneOf": [
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "variable"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["name", "value", "mutable"],
                                "properties": {
                                    "name": {"type": "string", "minLength": 1},
                                    "value": {"$ref": "#/$defs/expression"},
                                    "mutable": {"type": "boolean"}
                                }
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "assignment"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["name", "value"],
                                "properties": {
                                    "name": {"type": "string", "minLength": 1},
                                    "value": {"$ref": "#/$defs/expression"}
                                }
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "print"},
                            "data": {"$ref": "#/$defs/expression"}
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "expression"},
                            "data": {"$ref": "#/$defs/expression"}
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "conditional"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["condition", "then_block", "else_block"],
                                "properties": {
                                    "condition": {"$ref": "#/$defs/expression"},
                                    "then_block": {"$ref": "#/$defs/block"},
                                    "else_block": {
                                        "oneOf": [
                                            {"type": "null"},
                                            {"$ref": "#/$defs/block"}
                                        ]
                                    }
                                }
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "loop"},
                            "data": {
                                "type": "object", "additionalProperties": false,
                                "required": ["condition", "body"],
                                "properties": {
                                    "condition": {"$ref": "#/$defs/expression"},
                                    "body": {"$ref": "#/$defs/block"}
                                }
                            }
                        }
                    },
                    {
                        "type": "object", "additionalProperties": false,
                        "required": ["kind", "data"],
                        "properties": {
                            "kind": {"const": "return"},
                            "data": {
                                "oneOf": [
                                    {"type": "null"},
                                    {"$ref": "#/$defs/expression"}
                                ]
                            }
                        }
                    }
                ]
            },
            "parameter": {
                "type": "object", "additionalProperties": false,
                "required": ["name", "ty", "span"],
                "properties": {
                    "name": {"type": "string", "minLength": 1},
                    "ty": {"$ref": "#/$defs/type"},
                    "span": {"$ref": "#/$defs/span"}
                }
            },
            "function": {
                "type": "object", "additionalProperties": false,
                "required": ["name", "parameters", "body", "return_type", "span"],
                "properties": {
                    "name": {"type": "string", "minLength": 1},
                    "parameters": {
                        "type": "array", "maxItems": 256,
                        "items": {"$ref": "#/$defs/parameter"}
                    },
                    "body": {"$ref": "#/$defs/block"},
                    "return_type": {"$ref": "#/$defs/type"},
                    "span": {"$ref": "#/$defs/span"}
                }
            },
            "module": {
                "type": "object", "additionalProperties": false,
                "required": ["schema_version", "source_hash", "body", "functions"],
                "properties": {
                    "schema_version": {"const": "0.1"},
                    "source_hash": {"type": "string", "minLength": 1, "maxLength": 128},
                    "body": {"$ref": "#/$defs/block"},
                    "functions": {
                        "type": "array", "maxItems": 10000,
                        "items": {"$ref": "#/$defs/function"}
                    }
                }
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use linxts_ir::{Expression, Statement};

    fn candidate() -> CandidateEnvelope {
        let span = Default::default();
        let mut module = Module::new("source-hash");
        module.body.statements.push(Statement::new(
            StatementKind::Print(Expression::integer(1, span)),
            span,
        ));
        CandidateEnvelope {
            schema_version: AI_SCHEMA_VERSION.to_owned(),
            status: CandidateStatus::Candidate,
            language: CandidateLanguage::Natural,
            interpretation: "Print one".to_owned(),
            ir: module,
            assumptions: Vec::new(),
            warnings: Vec::new(),
            provider: None,
        }
    }

    #[test]
    fn validates_safe_candidate() {
        assert!(candidate().validate("source-hash").is_ok());
    }

    #[test]
    fn rejects_source_hash_mismatch() {
        assert_eq!(
            candidate().validate("other"),
            Err(CandidateValidationError::SourceHashMismatch)
        );
    }

    #[test]
    fn schema_includes_recursive_ir_definitions() {
        let schema = candidate_json_schema();
        assert!(schema.pointer("/$defs/module").is_some());
        assert!(schema.pointer("/$defs/statementKind").is_some());
        assert!(schema.pointer("/$defs/expressionKind").is_some());
    }

    #[test]
    fn rejects_unknown_fields() {
        let mut json = serde_json::to_value(candidate()).expect("serialize");
        json.as_object_mut()
            .expect("object")
            .insert("surprise".to_owned(), serde_json::json!(true));
        assert!(serde_json::from_value::<CandidateEnvelope>(json).is_err());
    }
}
