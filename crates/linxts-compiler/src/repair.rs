use linxts_source::{SourceId, Span};
use serde::{Deserialize, Serialize};

pub const MAX_REPAIR_PASSES: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SemanticRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepairType {
    MissingDelimiter,
    Spelling,
    CaseNormalization,
    MissingTerminator,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RepairRecord {
    pub id: String,
    pub span: Span,
    pub original_text: String,
    pub repaired_text: String,
    pub repair_type: RepairType,
    pub reason: String,
    pub semantic_risk: SemanticRisk,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RepairResult {
    pub text: String,
    pub records: Vec<RepairRecord>,
    pub passes: usize,
}

#[derive(Debug, Clone, Copy)]
pub struct RepairOptions {
    pub strict: bool,
    pub max_passes: usize,
}

impl Default for RepairOptions {
    fn default() -> Self {
        Self {
            strict: false,
            max_passes: MAX_REPAIR_PASSES,
        }
    }
}

#[derive(Debug, Default)]
pub struct RepairEngine;

impl RepairEngine {
    #[must_use]
    pub fn repair(
        source: SourceId,
        source_offset: usize,
        input: &str,
        options: RepairOptions,
    ) -> RepairResult {
        let mut text = input.to_owned();
        let mut records = Vec::new();
        let pass_limit = options.max_passes.min(MAX_REPAIR_PASSES);
        let mut passes = 0;

        while passes < pass_limit {
            passes += 1;
            let before = text.clone();
            repair_known_typos(source, source_offset, &mut text, &mut records, options);
            repair_delimiters(source, source_offset, &mut text, &mut records, options);
            if text == before {
                break;
            }
        }

        RepairResult {
            text,
            records,
            passes,
        }
    }
}

fn repair_known_typos(
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    const TYPOS: [(&str, &str); 3] = [
        ("consle.log", "console.log"),
        ("pritn", "print"),
        ("pirnt", "print"),
    ];
    for (wrong, correct) in TYPOS {
        while let Some(index) = text.find(wrong) {
            if options.strict {
                return;
            }
            let span = Span::new(
                source,
                source_offset + index,
                source_offset + index + wrong.len(),
            );
            text.replace_range(index..index + wrong.len(), correct);
            push_record(
                records,
                span,
                wrong,
                correct,
                RepairType::Spelling,
                "recognized a common output-call spelling error",
                SemanticRisk::Medium,
            );
        }
    }
}

fn repair_delimiters(
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    let mut stack = Vec::new();
    let mut single_quote = false;
    let mut double_quote = false;
    let mut single_quote_start = None;
    let mut double_quote_start = None;
    let mut escaped = false;

    for (index, character) in text.char_indices() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && (single_quote || double_quote) {
            escaped = true;
            continue;
        }
        match character {
            '\'' if !double_quote => {
                single_quote = !single_quote;
                single_quote_start = single_quote.then_some(index);
            }
            '"' if !single_quote => {
                double_quote = !double_quote;
                double_quote_start = double_quote.then_some(index);
            }
            '(' | '[' | '{' if !single_quote && !double_quote => stack.push(character),
            ')' | ']' | '}'
                if !single_quote
                    && !double_quote
                    && stack
                        .last()
                        .is_some_and(|opening| matching(*opening) == character) =>
            {
                stack.pop();
            }
            _ => {}
        }
    }

    if single_quote || double_quote {
        if options.strict {
            return;
        }
        let (quote, quote_start) = if single_quote {
            ("'", single_quote_start.expect("open quote has a start"))
        } else {
            ("\"", double_quote_start.expect("open quote has a start"))
        };
        let insertion = stack.last().map_or_else(
            || trailing_content_end(text),
            |opening| {
                let expected = matching(*opening);
                let search_start = quote_start + quote.len();
                text[search_start..]
                    .char_indices()
                    .find_map(|(relative, character)| {
                        (character == expected).then_some(search_start + relative)
                    })
                    .unwrap_or_else(|| trailing_content_end(text))
            },
        );
        text.insert_str(insertion, quote);
        push_record(
            records,
            Span::empty(source, source_offset + insertion),
            "",
            quote,
            RepairType::MissingDelimiter,
            "closed an unterminated string literal",
            SemanticRisk::Medium,
        );
        return;
    }

    if stack.is_empty() {
        return;
    }
    for opening in stack.into_iter().rev() {
        let closing = matching(opening).to_string();
        let risk = if opening == '{' {
            SemanticRisk::Medium
        } else {
            SemanticRisk::Low
        };
        if options.strict && risk > SemanticRisk::Low {
            continue;
        }
        let insertion = trailing_content_end(text);
        let repaired = if opening == '{' {
            format!("\n{closing}")
        } else {
            closing.clone()
        };
        text.insert_str(insertion, &repaired);
        push_record(
            records,
            Span::empty(source, source_offset + insertion),
            "",
            &repaired,
            RepairType::MissingDelimiter,
            format!("balanced an unmatched `{opening}`"),
            risk,
        );
    }
}

fn trailing_content_end(text: &str) -> usize {
    text.trim_end_matches(char::is_whitespace).len()
}

fn matching(opening: char) -> char {
    match opening {
        '(' => ')',
        '[' => ']',
        '{' => '}',
        _ => opening,
    }
}

fn push_record(
    records: &mut Vec<RepairRecord>,
    span: Span,
    original: impl Into<String>,
    repaired: impl Into<String>,
    repair_type: RepairType,
    reason: impl Into<String>,
    semantic_risk: SemanticRisk,
) {
    let sequence = records.len() + 1;
    records.push(RepairRecord {
        id: format!("RPR-{sequence:04}"),
        span,
        original_text: original.into(),
        repaired_text: repaired.into(),
        repair_type,
        reason: reason.into(),
        semantic_risk,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_javascript_parenthesis() {
        let result = RepairEngine::repair(
            SourceId(0),
            0,
            "console.log(\"Hello\"",
            RepairOptions::default(),
        );
        assert_eq!(result.text, "console.log(\"Hello\")");
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].semantic_risk, SemanticRisk::Low);
    }

    #[test]
    fn quote_repair_reuses_existing_structural_closers() {
        let result = RepairEngine::repair(
            SourceId(0),
            0,
            "fn main() {\nprint!(1\",{})\n}\n",
            RepairOptions::default(),
        );
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].repaired_text, "\"");
        assert!(result.text.contains("1\",{}\")"));
    }

    #[test]
    fn strict_mode_rejects_medium_risk_brace_repair() {
        let result = RepairEngine::repair(
            SourceId(0),
            0,
            "fn main() {\nprint(1)",
            RepairOptions {
                strict: true,
                ..RepairOptions::default()
            },
        );
        assert!(!result.text.ends_with('}'));
    }
}
