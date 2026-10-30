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
            match language {
                Some(Language::Python) => {
                    repair_python_missing_colons(
                        source,
                        source_offset,
                        &mut text,
                        &mut records,
                        options,
                    );
                    repair_python_range_typos(
                        source,
                        source_offset,
                        &mut text,
                        &mut records,
                        options,
                    );
                    repair_bracketed_output_call(
                        Language::Python,
                        source,
                        source_offset,
                        &mut text,
                        &mut records,
                        options,
                    );
                }
                Some(Language::JavaScript) => repair_bracketed_output_call(
                    Language::JavaScript,
                    source,
                    source_offset,
                    &mut text,
                    &mut records,
                    options,
                ),
                Some(Language::C) => {
                    repair_c_printf_missing_comma(
                        source,
                        source_offset,
                        &mut text,
                        &mut records,
                        options,
                    );
                    repair_c_missing_semicolons(source, source_offset, &mut text, &mut records);
                }
                _ => {}
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

fn repair_python_range_typos(
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    if options.strict {
        return;
    }
    for wrong in ["ragne", "rnage"] {
        let mut search_start = 0;
        while let Some(relative) = text[search_start..].find(wrong) {
            let index = search_start + relative;
            let end = index + wrong.len();
            let line_start = text[..index].rfind('\n').map_or(0, |position| position + 1);
            let prefix = text[line_start..index].trim();
            if !prefix.starts_with("for ")
                || !prefix.ends_with(" in")
                || !text[end..].trim_start().starts_with('(')
                || !outside_quotes(text, index)
            {
                search_start = end;
                continue;
            }
            text.replace_range(index..end, "range");
            push_record(
                records,
                Span::new(source, source_offset + index, source_offset + end),
                wrong,
                "range",
                RepairType::Spelling,
                "corrected a common Python range-call spelling error",
                SemanticRisk::Medium,
            );
            search_start = index + "range".len();
        }
    }
}

fn repair_bracketed_output_call(
    language: Language,
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
    options: RepairOptions,
) {
    if options.strict {
        return;
    }
    let call = match language {
        Language::Python => "print",
        Language::JavaScript => "console.log",
        _ => return,
    };
    let snapshot = text.clone();
    let mut replacements = Vec::new();
    let mut offset = 0;
    for line in snapshot.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']).trim_end();
        let trimmed = content.trim_start();
        let suffix = trimmed.strip_prefix(call);
        let Some(arguments) = suffix.and_then(|suffix| suffix.strip_prefix('[')) else {
            offset += line.len();
            continue;
        };
        let arguments = arguments.strip_suffix(';').unwrap_or(arguments);
        let Some(body) = arguments.strip_suffix(']') else {
            offset += line.len();
            continue;
        };
        if body.contains(['[', ']']) {
            offset += line.len();
            continue;
        }
        let indentation = content.len() - trimmed.len();
        let opening = offset + indentation + call.len();
        let closing = opening + 1 + body.len();
        replacements.push((opening, closing));
        offset += line.len();
    }
    for &(opening, closing) in replacements.iter().rev() {
        text.replace_range(closing..closing + 1, ")");
        text.replace_range(opening..opening + 1, "(");
    }
    for (opening, closing) in replacements {
        push_record(
            records,
            Span::new(source, source_offset + opening, source_offset + closing + 1),
            "[]",
            "()",
            RepairType::MissingDelimiter,
            format!("replaced bracket delimiters on a {language} output call"),
            SemanticRisk::Medium,
        );
    }
}

fn repair_c_printf_missing_comma(
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
    while let Some(relative) = text[search_start..].find("printf(") {
        let call_start = search_start + relative;
        if !outside_quotes(text, call_start) {
            search_start = call_start + "printf(".len();
            continue;
        }
        let string_start = call_start + "printf(".len();
        let arguments = &text[string_start..];
        let Some(quote_offset) = arguments.find('"') else {
            break;
        };
        if arguments
            .find([')', ';', '\n', '\r'])
            .is_some_and(|boundary| boundary < quote_offset)
        {
            search_start = string_start;
            continue;
        }
        let opening_quote = string_start + quote_offset;
        let mut escaped = false;
        let mut closing_quote = None;
        for (offset, character) in text[opening_quote + 1..].char_indices() {
            if escaped {
                escaped = false;
            } else if character == '\\' {
                escaped = true;
            } else if character == '"' {
                closing_quote = Some(opening_quote + 1 + offset);
                break;
            } else if character == '\n' {
                break;
            }
        }
        let Some(closing_quote) = closing_quote else {
            search_start = opening_quote + 1;
            continue;
        };
        let after_quote = closing_quote + 1;
        let rest = &text[after_quote..];
        let whitespace = rest.len() - rest.trim_start().len();
        let Some(next) = rest[whitespace..].chars().next() else {
            break;
        };
        if matches!(next, ',' | ')' | ';' | '\n' | '\r') {
            search_start = after_quote;
            continue;
        }
        text.insert(after_quote, ',');
        push_record(
            records,
            Span::empty(source, source_offset + after_quote),
            "",
            ",",
            RepairType::MissingDelimiter,
            "inserted a missing comma after a C printf format string",
            SemanticRisk::Medium,
        );
        search_start = after_quote + 1;
    }
}

fn repair_c_missing_semicolons(
    source: SourceId,
    source_offset: usize,
    text: &mut String,
    records: &mut Vec<RepairRecord>,
) {
    let snapshot = text.clone();
    let mut insertions = Vec::new();
    let mut offset = 0;
    for line in snapshot.split_inclusive('\n') {
        let content = line.trim_end_matches(['\r', '\n']).trim_end();
        let trimmed = content.trim_start();
        let needs_semicolon = (trimmed.starts_with("return ") || trimmed.starts_with("printf("))
            && !trimmed.ends_with([';', '{', '}'])
            && !trimmed.contains("//");
        if needs_semicolon {
            insertions.push(offset + content.len());
        }
        offset += line.len();
    }
    for &insertion in insertions.iter().rev() {
        text.insert(insertion, ';');
    }
    for insertion in insertions {
        push_record(
            records,
            Span::empty(source, source_offset + insertion),
            "",
            ";",
            RepairType::MissingTerminator,
            "added a missing semicolon to a C statement",
            SemanticRisk::Low,
        );
    }
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
    fn repairs_python_range_and_output_brackets() {
        let result = RepairEngine::repair_for(
            Language::Python,
            SourceId(0),
            0,
            "for i in ragne(3):\n    print[i]\n",
            RepairOptions::default(),
        );
        assert_eq!(result.text, "for i in range(3):\n    print(i)\n");
        assert_eq!(result.records.len(), 2);
    }

    #[test]
    fn repairs_javascript_output_brackets() {
        let result = RepairEngine::repair_for(
            Language::JavaScript,
            SourceId(0),
            0,
            "console.log[\"javascript\"];\n",
            RepairOptions::default(),
        );
        assert_eq!(result.text, "console.log(\"javascript\");\n");
        assert_eq!(result.records.len(), 1);
    }

    #[test]
    fn repairs_c_printf_comma_and_statement_terminators() {
        let result = RepairEngine::repair_for(
            Language::C,
            SourceId(0),
            0,
            "printf(\"value=%d\" value)\nreturn 0\n",
            RepairOptions::default(),
        );
        assert_eq!(result.text, "printf(\"value=%d\", value);\nreturn 0;\n");
        assert_eq!(result.records.len(), 3);
    }

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
        assert_eq!(c.text, "printf(\"c\");");

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
