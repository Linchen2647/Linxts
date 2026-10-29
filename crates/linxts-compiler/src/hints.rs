use linxts_source::{SourceId, Span};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Language {
    Python,
    JavaScript,
    Rust,
    C,
    Natural,
    Generic,
}

impl std::fmt::Display for Language {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::Python => "python",
            Self::JavaScript => "javascript",
            Self::Rust => "rust",
            Self::C => "c",
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

            if let Some(marker) = marker {
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
                let marker_start = offset + marker.column;
                current_language = marker.language;
                current_hint = Some(Hint {
                    language: marker.language,
                    raw: marker.raw.to_owned(),
                    span: Span::new(source, marker_start, offset + marker.marker_end),
                });
                if let Some(inline) = marker.inline {
                    segment_start = offset + marker.inline_column;
                    segment_text.push_str(inline);
                    let line_ending = &line_with_ending[line.len()..];
                    segment_text.push_str(line_ending);
                    quote_state.consume(inline);
                    quote_state.consume(line_ending);
                } else {
                    segment_start = offset + line_with_ending.len();
                }
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

struct ParsedMarker<'a> {
    raw: &'a str,
    language: Option<Language>,
    column: usize,
    marker_end: usize,
    inline: Option<&'a str>,
    inline_column: usize,
}

fn parse_marker(line: &str) -> Option<ParsedMarker<'_>> {
    let trimmed = line.trim_start();
    if !trimmed.starts_with('^') {
        return None;
    }
    let column = line.len() - trimmed.len();
    let after_caret = &trimmed[1..];
    let leading = after_caret.len() - after_caret.trim_start().len();
    let raw = after_caret.trim();
    if raw.is_empty() {
        return None;
    }

    if let Some(language) = classify_alias(raw) {
        return Some(ParsedMarker {
            raw,
            language: Some(language),
            column,
            marker_end: line.len(),
            inline: None,
            inline_column: line.len(),
        });
    }

    if let Some(first_whitespace) = raw.find(char::is_whitespace) {
        let alias = &raw[..first_whitespace];
        let language = classify_alias(alias)?;
        let rest = &raw[first_whitespace..];
        let inline = rest.trim_start();
        if inline.is_empty() {
            return None;
        }
        let inline_column = column + 1 + leading + first_whitespace + rest.len() - inline.len();
        return Some(ParsedMarker {
            raw: alias,
            language: Some(language),
            column,
            marker_end: column + 1 + leading + alias.len(),
            inline: Some(inline),
            inline_column,
        });
    }

    Some(ParsedMarker {
        raw,
        language: None,
        column,
        marker_end: line.len(),
        inline: None,
        inline_column: line.len(),
    })
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
    } else if ["c", "clang", "c語言", "c语言", "這是c", "这是c"]
        .iter()
        .any(|alias| normalized == *alias)
    {
        Some(Language::C)
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
    fn supports_c_and_inline_known_hints() {
        let segments = HintScanner::scan(SourceId(0), "^C語言\nprintf(\"C\")\n^python print(1)\n");
        assert_eq!(segments.len(), 2);
        assert_eq!(segments[0].explicit_language, Some(Language::C));
        assert_eq!(segments[1].explicit_language, Some(Language::Python));
        assert_eq!(segments[1].text.trim(), "print(1)");
        assert_eq!(segments[1].hint.as_ref().expect("hint").raw, "python");
    }

    #[test]
    fn unknown_hint_is_preserved() {
        let segments = HintScanner::scan(SourceId(0), "^brainwave\nPRINT 1\n");
        assert_eq!(segments[0].hint.as_ref().expect("hint").raw, "brainwave");
        assert_eq!(segments[0].explicit_language, None);
    }
}
