use crate::hints::Language;
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
        Self::repair_inner(None, source, source_offset, input, options)
    }

    #[must_use]
    pub fn repair_for(
        language: Language,
        source: SourceId,
        source_offset: usize,
        input: &str,
        options: RepairOptions,
    ) -> RepairResult {
        Self::repair_inner(Some(language), source, source_offset, input, options)
    }

    fn repair_inner(
        language: Option<Language>,
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
            repair_known_typos(
                language,
                source,
                source_offset,
                &mut text,
                &mut records,
                options,
            );
            if language == Some(Language::Python) {
                repair_python_missing_colons(
                    source,
                    source_offset,
                    &mut text,
                    &mut records,
                    options,
                );
            }
            repair_misplaced_numeric_quote(source, source_offset, &mut text, &mut records, options);
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
    language: Option<Language>,
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    const PYTHON_TYPOS: [(&str, &str); 2] = [("pritn", "print"), ("pirnt", "print")];
    const JAVASCRIPT_TYPOS: [(&str, &str); 2] = [
        ("consle.log", "console.log"),
        ("console,log", "console.log"),
    ];
    const C_TYPOS: [(&str, &str); 2] = [("pritnf", "printf"), ("printif", "printf")];
    const ALL_TYPOS: [(&str, &str); 6] = [
        ("pritn", "print"),
        ("pirnt", "print"),
        ("consle.log", "console.log"),
        ("console,log", "console.log"),
        ("pritnf", "printf"),
        ("printif", "printf"),
    ];
    let typos: &[(&str, &str)] = match language {
        Some(Language::Python) => &PYTHON_TYPOS,
        Some(Language::JavaScript) => &JAVASCRIPT_TYPOS,
        Some(Language::C) => &C_TYPOS,
        Some(_) => &[],
        None => &ALL_TYPOS,
    };
    for &(wrong, correct) in typos {
        let mut search_start = 0;
        while let Some(relative) = text[search_start..].find(wrong) {
            let index = search_start + relative;
            let end = index + wrong.len();
            let valid_prefix = text[..index]
                .chars()
                .next_back()
                .is_none_or(|character| !character.is_alphanumeric() && character != '_');
            let valid_call = text[end..].trim_start().starts_with('(');
            if !valid_prefix || !valid_call || !outside_quotes(text, index) {
                search_start = end;
                continue;
            }
            if options.strict {
                return;
            }
            let span = Span::new(source, source_offset + index, source_offset + end);
            text.replace_range(index..end, correct);
            push_record(
                records,
                span,
                wrong,
                correct,
                RepairType::Spelling,
                "recognized a common output-call spelling error",
                SemanticRisk::Medium,
            );
            search_start = index + correct.len();
        }
    }
}

fn outside_quotes(text: &str, index: usize) -> bool {
    let mut single_quote = false;
    let mut double_quote = false;
    let mut escaped = false;
    for character in text[..index].chars() {
        if escaped {
            escaped = false;
            continue;
        }
        if character == '\\' && (single_quote || double_quote) {
            escaped = true;
            continue;
        }
        match character {
            '\'' if !double_quote => single_quote = !single_quote,
            '"' if !single_quote => double_quote = !double_quote,
            _ => {}
        }
    }
    !single_quote && !double_quote
}

fn repair_python_missing_colons(
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    if options.strict {
        return;
    }
    let snapshot = text.clone();
    let mut lines = Vec::new();
    let mut offset = 0;
    for line in snapshot.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']);
        lines.push((offset, content.to_owned()));
        offset += line.len();
    }
    if offset < snapshot.len() || snapshot.is_empty() {
        lines.push((offset, snapshot[offset..].to_owned()));
    }

    let mut insertions = Vec::new();
    for (line_index, (line_start, line)) in lines.iter().enumerate() {
        let content = line.trim_end();
        let trimmed = content.trim_start();
        if trimmed.ends_with(':') || !is_python_block_header(trimmed) {
            continue;
        }
        let current_indent = content.len() - trimmed.len();
        let Some((_, next_line)) = lines[line_index + 1..]
            .iter()
            .find(|(_, candidate)| !candidate.trim().is_empty())
        else {
            continue;
        };
        let next_trimmed = next_line.trim_start();
        let next_indent = next_line.len() - next_trimmed.len();
        if next_indent <= current_indent {
            continue;
        }
        insertions.push(line_start + content.len());
    }

    for &insertion in insertions.iter().rev() {
        text.insert(insertion, ':');
    }
    for insertion in insertions {
        push_record(
            records,
            Span::empty(source, source_offset + insertion),
            "",
            ":",
            RepairType::MissingTerminator,
            "added a missing colon before an indented Python suite",
            SemanticRisk::Medium,
        );
    }
}

fn is_python_block_header(line: &str) -> bool {
    line.starts_with("if ")
        || line.starts_with("while ")
        || line.starts_with("for ")
        || line.starts_with("def ")
        || line == "else"
}

fn repair_misplaced_numeric_quote(
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    if options.strict {
        return;
    }
    let mut search_start = 0;
    while let Some(relative) = text[search_start..].find("\"\"") {
        let first_quote = search_start + relative;
        let line_start = text[..first_quote]
            .rfind('\n')
            .map_or(0, |position| position + 1);
        let prefix = text[line_start..first_quote].trim_end();
        if !["print(", "println(", "print!(", "println!("]
            .iter()
            .any(|call| prefix.ends_with(call))
        {
            search_start = first_quote + 2;
            continue;
        }
        let digits_start = first_quote + 2;
        let digits_end = text[digits_start..]
            .char_indices()
            .take_while(|(_, character)| character.is_ascii_digit())
            .last()
            .map_or(digits_start, |(offset, character)| {
                digits_start + offset + character.len_utf8()
            });
        if digits_end == digits_start
            || text[digits_end..]
                .chars()
                .find(|character| !character.is_whitespace())
                != Some(')')
        {
            search_start = first_quote + 2;
            continue;
        }
        let original = text[first_quote + 1..digits_end].to_owned();
        let repaired = format!("{}\"", &text[digits_start..digits_end]);
        text.replace_range(first_quote + 1..digits_end, &repaired);
        push_record(
            records,
            Span::new(
                source,
                source_offset + first_quote + 1,
                source_offset + digits_end,
            ),
            original,
            repaired,
            RepairType::MissingDelimiter,
            "moved a duplicated output-string quote after its numeric content",
            SemanticRisk::Medium,
        );
        search_start = first_quote + 1;
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
    fn adds_python_colon_only_for_indented_suite() {
        let result = RepairEngine::repair_for(
            Language::Python,
            SourceId(0),
            0,
            "if value > 0\n    print(value)\n",
            RepairOptions::default(),
        );
        assert_eq!(result.text, "if value > 0:\n    print(value)\n");
        assert_eq!(result.records.len(), 1);
        assert_eq!(result.records[0].repair_type, RepairType::MissingTerminator);

        let strict = RepairEngine::repair_for(
            Language::Python,
            SourceId(0),
            0,
            "if value > 0\n    print(value)\n",
            RepairOptions {
                strict: true,
                ..RepairOptions::default()
            },
        );
        assert_eq!(strict.text, "if value > 0\n    print(value)\n");
        assert!(strict.records.is_empty());
    }

    #[test]
    fn uses_language_specific_output_typos() {
        let javascript = RepairEngine::repair_for(
            Language::JavaScript,
            SourceId(0),
            0,
            "console,log(\"js\")",
            RepairOptions::default(),
        );
        assert_eq!(javascript.text, "console.log(\"js\")");

        let c = RepairEngine::repair_for(
            Language::C,
            SourceId(0),
            0,
            "pritnf(\"c\")",
            RepairOptions::default(),
        );
        assert_eq!(c.text, "printf(\"c\")");

        let python = RepairEngine::repair_for(
            Language::Python,
            SourceId(0),
            0,
            "consle.log(\"not Python\")",
            RepairOptions::default(),
        );
        assert_eq!(python.text, "consle.log(\"not Python\")");
        assert!(python.records.is_empty());

        let quoted = RepairEngine::repair_for(
            Language::JavaScript,
            SourceId(0),
            0,
            "console.log(\"consle.log(\")",
            RepairOptions::default(),
        );
        assert_eq!(quoted.text, "console.log(\"consle.log(\")");
        assert!(quoted.records.is_empty());
    }

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
    fn repairs_misplaced_numeric_output_quote() {
        let result = RepairEngine::repair(
            SourceId(0),
            0,
            "fn rust(){\n    print(\"\"666)\n}\n",
            RepairOptions::default(),
        );
        assert_eq!(result.records.len(), 1);
        assert!(result.text.contains("print(\"666\")"));
        assert_eq!(result.records[0].semantic_risk, SemanticRisk::Medium);
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
