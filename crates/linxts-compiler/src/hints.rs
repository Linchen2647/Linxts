use linxts_source::{SourceId, Span};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
    JavaScript,
    Rust,
    Natural,
    Generic,
}

impl std::fmt::Display for Language {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::Rust => "rust",
            Self::Natural => "natural",
            Self::Generic => "generic",
        };
        formatter.write_str(name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hint {
    pub language: Option<Language>,
    pub raw: String,
    pub span: Span,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Segment {
    pub explicit_language: Option<Language>,
    pub text: String,
    pub span: Span,
    pub hint: Option<Hint>,
}

#[derive(Debug, Default)]
pub struct HintScanner;

impl HintScanner {
    #[must_use]
    pub fn scan(source: SourceId, text: &str) -> Vec<Segment> {
        let mut segments = Vec::new();
        let mut current_language = None;
        let mut current_hint = None;
        let mut segment_start = 0;
        let mut segment_text = String::new();
        let mut offset = 0;
        let mut quote_state = QuoteState::default();

        for line_with_ending in text.split_inclusive('\n') {
            let line = line_with_ending.trim_end_matches(['\r', '\n']);
            let marker = if quote_state.is_clear() {
                parse_marker(line)
            } else {
                None
            };

            if let Some((raw, language, marker_column)) = marker {
                if !segment_text.trim().is_empty() {
                    segments.push(Segment {
                        explicit_language: current_language,
                        text: std::mem::take(&mut segment_text),
                        span: Span::new(source, segment_start, offset),
                        hint: current_hint.take(),
                    });
                } else {
                    segment_text.clear();
                }
                let marker_start = offset + marker_column;
                current_language = language;
                current_hint = Some(Hint {
                    language,
                    raw: raw.to_owned(),
                    span: Span::new(source, marker_start, offset + line.len()),
                });
                segment_start = offset + line_with_ending.len();
            } else {
                if segment_text.is_empty() {
                    segment_start = offset;
                }
                segment_text.push_str(line_with_ending);
                quote_state.consume(line_with_ending);
            }
            offset += line_with_ending.len();
        }

        if !segment_text.trim().is_empty() || segments.is_empty() {
            segments.push(Segment {
                explicit_language: current_language,
                text: segment_text,
                span: Span::new(source, segment_start, text.len()),
                hint: current_hint,
            });
        }
        segments
    }
}

fn parse_marker(line: &str) -> Option<(&str, Option<Language>, usize)> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('^') {
        return None;
    }
    let column = line.len() - trimmed.len();
    let raw = trimmed[1..].trim();
    if raw.is_empty() || raw.chars().any(char::is_whitespace) && !is_descriptive_hint(raw) {
        return None;
    }
    Some((raw, classify_alias(raw), column))
}

fn is_descriptive_hint(raw: &str) -> bool {
    let normalized = raw.to_lowercase().replace([' ', '-', '_'], "");
    ["python", "javascript", "rust", "自然語言", "自然语言"]
        .iter()
        .any(|name| normalized.contains(name))
}

#[must_use]
pub fn classify_alias(raw: &str) -> Option<Language> {
    let normalized = raw
        .trim()
        .to_lowercase()
        .replace([' ', '-', '_', '：', ':'], "");
    if ["python", "py", "這是python", "这是python", "接下來用python"]
        .iter()
        .any(|alias| normalized == *alias)
    {
        Some(Language::Python)
    } else if [
        "javascript",
        "js",
        "node",
        "nodejs",
        "這是javascript",
        "这是javascript",
        "接下來用javascript",
        "接下来用javascript",
    ]
    .iter()
    .any(|alias| normalized == *alias)
    {
        Some(Language::JavaScript)
    } else if ["rust", "rs", "這是rust", "这是rust"]
        .iter()
        .any(|alias| normalized == *alias)
    {
        Some(Language::Rust)
    } else if [
        "natural",
        "naturallanguage",
        "nl",
        "自然語言",
        "自然语言",
        "中文",
    ]
    .iter()
    .any(|alias| normalized == *alias)
    {
        Some(Language::Natural)
    } else {
        None
    }
}

#[derive(Debug, Default)]
struct QuoteState {
    single: bool,
    double: bool,
    escaped: bool,
}

impl QuoteState {
    fn is_clear(&self) -> bool {
        !self.single && !self.double
    }

    fn consume(&mut self, text: &str) {
        for character in text.chars() {
            if self.escaped {
                self.escaped = false;
                continue;
            }
            if character == '\\' && (self.single || self.double) {
                self.escaped = true;
            } else if character == '\'' && !self.double {
                self.single = !self.single;
            } else if character == '"' && !self.single {
                self.double = !self.double;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scans_descriptive_and_case_insensitive_hints() {
        let segments =
            HintScanner::scan(SourceId(0), "^這是Python\nprint(1)\n^JS\nconsole.log(2)\n");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].explicit_language, Some(Language::Python));
        assert_eq!(segments[1].explicit_language, Some(Language::JavaScript));
    }

    #[test]
    fn caret_in_string_is_not_a_hint() {
        let segments = HintScanner::scan(SourceId(0), "print(\"^javascript\")\n");
        assert_eq!(segments.len(), 1);
        assert!(segments[0].hint.is_none());
    }

    #[test]
    fn unknown_hint_is_preserved() {
        let segments = HintScanner::scan(SourceId(0), "^brainwave\nPRINT 1\n");
        assert_eq!(segments[0].hint.as_ref().expect("hint").raw, "brainwave");
        assert_eq!(segments[0].explicit_language, None);
    }
}
