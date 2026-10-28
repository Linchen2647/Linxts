//! Linxts deterministic compiler pipeline.

mod cache;
mod detector;
mod hints;
mod lexer;
mod parser;
mod repair;

pub use cache::{CacheError, CacheKeyInput, CompilationCache};
pub use detector::{Detection, LanguageDetector};
pub use hints::{Hint, HintScanner, Language, Segment, classify_alias};
pub use parser::ParseDiagnostic;
pub use repair::{
    MAX_REPAIR_PASSES, RepairEngine, RepairOptions, RepairRecord, RepairResult, RepairType,
    SemanticRisk,
};

use linxts_ir::{
    BinaryOperator, Block, Expression, ExpressionKind, Module, Statement, StatementKind, Validator,
};
use linxts_source::{SourceFile, Span};
use parser::{ParsedUnit, Parser};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompileOptions {
    pub aggressive: bool,
    pub strict: bool,
    pub no_ai: bool,
    pub offline: bool,
}

impl Default for CompileOptions {
    fn default() -> Self {
        Self {
            aggressive: true,
            strict: false,
            no_ai: false,
            offline: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Stage {
    Source,
    Detection,
    Parser,
    Repair,
    Ai,
    Validation,
    Backend,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Diagnostic {
    pub code: String,
    pub severity: Severity,
    pub stage: Stage,
    pub message: String,
    pub message_zh_tw: Option<String>,
    pub span: Option<Span>,
    pub suggestion: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compilation {
    pub module: Module,
    pub detection: Detection,
    pub segment_detections: Vec<Detection>,
    pub repairs: Vec<RepairRecord>,
    pub diagnostics: Vec<Diagnostic>,
    pub used_ai: bool,
}

impl Compilation {
    #[must_use]
    pub fn has_errors(&self) -> bool {
        self.diagnostics
            .iter()
            .any(|diagnostic| diagnostic.severity == Severity::Error)
    }
}

#[derive(Debug, Default)]
pub struct Compiler;

impl Compiler {
    #[must_use]
    pub fn compile(source: &SourceFile, options: CompileOptions) -> Compilation {
        let segments = HintScanner::scan(source.id(), source.text());
        let detection = LanguageDetector::detect_file(&segments);
        let segment_detections: Vec<_> = segments
            .iter()
            .map(LanguageDetector::detect_segment)
            .collect();
        let mut module = Module::new(source.snapshot_hash());
        let mut repairs = Vec::new();
        let mut diagnostics = Vec::new();
        let mut declared = HashSet::new();

        if source.text().trim().is_empty() {
            diagnostics.push(Diagnostic {
                code: "LXT-100".to_owned(),
                severity: Severity::Error,
                stage: Stage::Source,
                message: "source file is empty".to_owned(),
                message_zh_tw: Some("原始碼檔案是空的".to_owned()),
                span: Some(Span::empty(source.id(), 0)),
                suggestion: Some("Add at least one statement, for example `PRINT 1`.".to_owned()),
            });
        }

        for (segment, segment_detection) in segments.iter().zip(&segment_detections) {
            let language = segment
                .explicit_language
                .or_else(|| segment_detection.primary_language())
                .unwrap_or(Language::Generic);

            if language == Language::Natural {
                match parse_natural(segment, &mut declared) {
                    Some(block) => module.body.statements.extend(block.statements),
                    None => diagnostics.push(Diagnostic {
                        code: "LXT-120".to_owned(),
                        severity: Severity::Error,
                        stage: Stage::Parser,
                        message:
                            "deterministic natural-language parser could not resolve this intent"
                                .to_owned(),
                        message_zh_tw: Some("確定性自然語言解析器無法理解此意圖".to_owned()),
                        span: Some(segment.span),
                        suggestion: Some(if options.no_ai {
                            "Rewrite the intent as code; AI fallback is disabled.".to_owned()
                        } else {
                            "Configure an AI backend or rewrite the intent more explicitly."
                                .to_owned()
                        }),
                    }),
                }
                continue;
            }

            let mut trial_declared = declared.clone();
            let original = Parser::parse(
                source.id(),
                segment.span.start,
                &segment.text,
                &mut trial_declared,
                language == Language::Python,
            );
            if original.is_complete() {
                declared = trial_declared;
                merge_unit(&mut module, original);
                continue;
            }

            if !options.aggressive {
                append_parse_diagnostics(&mut diagnostics, original.diagnostics);
                continue;
            }

            let repaired = RepairEngine::repair(
                source.id(),
                segment.span.start,
                &segment.text,
                RepairOptions {
                    strict: options.strict,
                    ..RepairOptions::default()
                },
            );
            repairs.extend(repaired.records.iter().cloned());
            let mut repaired_declared = declared.clone();
            let reparsed = Parser::parse(
                source.id(),
                segment.span.start,
                &repaired.text,
                &mut repaired_declared,
                language == Language::Python,
            );
            if reparsed.is_complete() {
                declared = repaired_declared;
                merge_unit(&mut module, reparsed);
                for record in repaired.records {
                    diagnostics.push(Diagnostic {
                        code: "LXT-R01".to_owned(),
                        severity: Severity::Info,
                        stage: Stage::Repair,
                        message: format!("{}: {}", record.id, record.reason),
                        message_zh_tw: Some(format!("{}：{}", record.id, record.reason)),
                        span: Some(record.span),
                        suggestion: None,
                    });
                }
            } else {
                merge_unit_without_diagnostics(&mut module, &reparsed);
                append_parse_diagnostics(&mut diagnostics, reparsed.diagnostics);
            }
        }

        for error in Validator::validate(&module) {
            diagnostics.push(Diagnostic {
                code: error.code.to_owned(),
                severity: Severity::Error,
                stage: Stage::Validation,
                message: error.message,
                message_zh_tw: Some("統一中介表示驗證失敗".to_owned()),
                span: error.span,
                suggestion: Some("Clarify the expression or declaration.".to_owned()),
            });
        }

        Compilation {
            module,
            detection,
            segment_detections,
            repairs,
            diagnostics,
            used_ai: false,
        }
    }
}

fn merge_unit(module: &mut Module, unit: ParsedUnit) {
    module.body.statements.extend(unit.block.statements);
    module.functions.extend(unit.functions);
}

fn merge_unit_without_diagnostics(module: &mut Module, unit: &ParsedUnit) {
    module
        .body
        .statements
        .extend(unit.block.statements.iter().cloned());
    module.functions.extend(unit.functions.iter().cloned());
}

fn append_parse_diagnostics(target: &mut Vec<Diagnostic>, diagnostics: Vec<ParseDiagnostic>) {
    target.extend(diagnostics.into_iter().map(|diagnostic| Diagnostic {
        code: diagnostic.code.to_owned(),
        severity: Severity::Error,
        stage: Stage::Parser,
        message: diagnostic.message,
        message_zh_tw: Some("無法解析此處的運算式或語句".to_owned()),
        span: Some(diagnostic.span),
        suggestion: Some(
            "Check delimiters or make the intended operation more explicit.".to_owned(),
        ),
    }));
}

fn parse_natural(segment: &Segment, declared: &mut HashSet<String>) -> Option<Block> {
    let text = segment.text.trim();
    let numbers = extract_numbers(text);
    let span = segment.span;

    if (text.contains("總和") || text.contains("总和")) && numbers.len() >= 2 {
        let start = numbers[0];
        let end = numbers[1];
        let sum_name = unique_internal_name(declared, "__linxts_sum");
        let index_name = unique_internal_name(declared, "__linxts_i");
        declared.insert(sum_name.clone());
        declared.insert(index_name.clone());
        return Some(sum_loop(span, start, end, sum_name, index_name));
    }

    if (text.contains("偶數") || text.contains("偶数")) && numbers.len() >= 2 {
        let index_name = unique_internal_name(declared, "__linxts_i");
        declared.insert(index_name.clone());
        return Some(even_loop(span, numbers[0], numbers[1], index_name));
    }

    if text.contains("連乘")
        || text.contains("连乘")
        || text.contains("階乘")
        || text.contains("阶乘")
    {
        let (start, end) = match numbers.as_slice() {
            [end] => (1, *end),
            [start, end, ..] => (*start, *end),
            [] => return None,
        };
        let product_name = unique_internal_name(declared, "__linxts_product");
        let index_name = unique_internal_name(declared, "__linxts_i");
        declared.insert(product_name.clone());
        declared.insert(index_name.clone());
        return Some(product_loop(span, start, end, product_name, index_name));
    }

    if let Some(operation) = natural_variable_operation(text, span, declared) {
        return Some(operation);
    }

    None
}

fn sum_loop(span: Span, start: i64, end: i64, sum: String, index: String) -> Block {
    Block {
        statements: vec![
            variable(&sum, 0, span),
            variable(&index, start, span),
            Statement::new(
                StatementKind::Loop {
                    condition: binary(
                        variable_expression(&index, span),
                        BinaryOperator::LessEqual,
                        Expression::integer(end, span),
                        span,
                    ),
                    body: Block {
                        statements: vec![
                            Statement::new(
                                StatementKind::Assignment {
                                    name: sum.clone(),
                                    value: binary(
                                        variable_expression(&sum, span),
                                        BinaryOperator::Add,
                                        variable_expression(&index, span),
                                        span,
                                    ),
                                },
                                span,
                            ),
                            increment(&index, span),
                        ],
                    },
                },
                span,
            ),
            Statement::new(StatementKind::Print(variable_expression(&sum, span)), span),
        ],
    }
}

fn product_loop(span: Span, start: i64, end: i64, product: String, index: String) -> Block {
    Block {
        statements: vec![
            variable(&product, 1, span),
            variable(&index, start, span),
            Statement::new(
                StatementKind::Loop {
                    condition: binary(
                        variable_expression(&index, span),
                        BinaryOperator::LessEqual,
                        Expression::integer(end, span),
                        span,
                    ),
                    body: Block {
                        statements: vec![
                            Statement::new(
                                StatementKind::Assignment {
                                    name: product.clone(),
                                    value: binary(
                                        variable_expression(&product, span),
                                        BinaryOperator::Multiply,
                                        variable_expression(&index, span),
                                        span,
                                    ),
                                },
                                span,
                            ),
                            increment(&index, span),
                        ],
                    },
                },
                span,
            ),
            Statement::new(
                StatementKind::Print(variable_expression(&product, span)),
                span,
            ),
        ],
    }
}

fn even_loop(span: Span, start: i64, end: i64, index: String) -> Block {
    Block {
        statements: vec![
            variable(&index, start, span),
            Statement::new(
                StatementKind::Loop {
                    condition: binary(
                        variable_expression(&index, span),
                        BinaryOperator::LessEqual,
                        Expression::integer(end, span),
                        span,
                    ),
                    body: Block {
                        statements: vec![
                            Statement::new(
                                StatementKind::Conditional {
                                    condition: binary(
                                        binary(
                                            variable_expression(&index, span),
                                            BinaryOperator::Remainder,
                                            Expression::integer(2, span),
                                            span,
                                        ),
                                        BinaryOperator::Equal,
                                        Expression::integer(0, span),
                                        span,
                                    ),
                                    then_block: Block {
                                        statements: vec![Statement::new(
                                            StatementKind::Print(variable_expression(&index, span)),
                                            span,
                                        )],
                                    },
                                    else_block: None,
                                },
                                span,
                            ),
                            increment(&index, span),
                        ],
                    },
                },
                span,
            ),
        ],
    }
}

fn natural_variable_operation(text: &str, span: Span, declared: &HashSet<String>) -> Option<Block> {
    let number = extract_numbers(text).into_iter().next()?;
    let variable_name = declared
        .iter()
        .filter(|name| !name.starts_with("__linxts"))
        .find(|name| text.contains(name.as_str()))?
        .clone();
    let operator = if text.contains("乘") {
        BinaryOperator::Multiply
    } else if text.contains("加") {
        BinaryOperator::Add
    } else if text.contains("減") || text.contains("减") {
        BinaryOperator::Subtract
    } else {
        return None;
    };
    let value = binary(
        variable_expression(&variable_name, span),
        operator,
        Expression::integer(number, span),
        span,
    );
    Some(Block {
        statements: vec![
            Statement::new(
                StatementKind::Assignment {
                    name: variable_name.clone(),
                    value,
                },
                span,
            ),
            Statement::new(
                StatementKind::Print(variable_expression(&variable_name, span)),
                span,
            ),
        ],
    })
}

fn extract_numbers(text: &str) -> Vec<i64> {
    let mut values = Vec::new();
    let mut current = String::new();
    for character in text.chars() {
        if character.is_ascii_digit() {
            current.push(character);
        } else if !current.is_empty() {
            if let Ok(value) = current.parse() {
                values.push(value);
            }
            current.clear();
        }
    }
    if !current.is_empty()
        && let Ok(value) = current.parse()
    {
        values.push(value);
    }
    values
}

fn unique_internal_name(declared: &HashSet<String>, base: &str) -> String {
    let mut candidate = base.to_owned();
    let mut suffix = 0;
    while declared.contains(&candidate) {
        suffix += 1;
        candidate = format!("{base}_{suffix}");
    }
    candidate
}

fn variable(name: &str, value: i64, span: Span) -> Statement {
    Statement::new(
        StatementKind::Variable {
            name: name.to_owned(),
            value: Expression::integer(value, span),
            mutable: true,
        },
        span,
    )
}

fn increment(name: &str, span: Span) -> Statement {
    Statement::new(
        StatementKind::Assignment {
            name: name.to_owned(),
            value: binary(
                variable_expression(name, span),
                BinaryOperator::Add,
                Expression::integer(1, span),
                span,
            ),
        },
        span,
    )
}

fn variable_expression(name: &str, span: Span) -> Expression {
    Expression::new(
        ExpressionKind::Variable {
            name: name.to_owned(),
        },
        span,
    )
}

fn binary(left: Expression, operator: BinaryOperator, right: Expression, span: Span) -> Expression {
    Expression::new(
        ExpressionKind::Binary {
            operator,
            left: Box::new(left),
            right: Box::new(right),
        },
        span,
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use linxts_ir::StatementKind;
    use linxts_source::SourceId;

    fn compile(text: &str) -> Compilation {
        let source = SourceFile::from_text(SourceId(0), "test.lx", text);
        Compiler::compile(&source, CompileOptions::default())
    }

    #[test]
    fn compiles_bare_print_without_ai() {
        let result = compile("PRINT 1\n");
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert!(!result.used_ai);
        assert!(matches!(
            result.module.body.statements[0].kind,
            StatementKind::Print(_)
        ));
    }

    #[test]
    fn repairs_missing_javascript_parenthesis() {
        let result = compile("^javascript\nconsole.log(\"Hello\"\n");
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.repairs.len(), 1);
    }

    #[test]
    fn compiles_mixed_language_shared_variable() {
        let result = compile(
            "^python\nx = 10\n^javascript\nconsole.log(x)\n^自然語言\n把 x 乘以 5，然後顯示結果\n",
        );
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.module.body.statements.len(), 4);
    }

    #[test]
    fn compiles_simplified_chinese_natural_product() {
        let result = compile("^自然语言\n计算1~6的连乘，并输出\n");
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.module.body.statements.len(), 4);
    }

    #[test]
    fn compiles_natural_language_sum() {
        let result = compile("幫我算出1到100的總和\n");
        assert!(!result.has_errors(), "{:?}", result.diagnostics);
        assert_eq!(result.module.body.statements.len(), 4);
    }
}
