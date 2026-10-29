use crate::hints::{Language, Segment};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum Detection {
    Detected {
        language: Language,
        confidence: u8,
        evidence: Vec<String>,
    },
    Unknown {
        evidence: Vec<String>,
    },
    Ambiguous {
        candidates: Vec<(Language, u8)>,
        evidence: Vec<String>,
    },
    Mixed {
        languages: Vec<Language>,
    },
}

impl Detection {
    #[must_use]
    pub fn primary_language(&self) -> Option<Language> {
        match self {
            Self::Detected { language, .. } => Some(*language),
            Self::Ambiguous { candidates, .. } => candidates.first().map(|item| item.0),
            Self::Unknown { .. } | Self::Mixed { .. } => None,
        }
    }
}

#[derive(Debug, Default)]
pub struct LanguageDetector;

impl LanguageDetector {
    #[must_use]
    pub fn detect_segment(segment: &Segment) -> Detection {
        if let Some(language) = segment.explicit_language {
            return Detection::Detected {
                language,
                confidence: 100,
                evidence: vec!["explicit language hint".to_owned()],
            };
        }

        let text = segment.text.trim();
        if text.is_empty() {
            return Detection::Unknown {
                evidence: vec!["empty segment".to_owned()],
            };
        }

        let mut scores = HashMap::from([
            (Language::Python, 0_i32),
            (Language::JavaScript, 0),
            (Language::Rust, 0),
            (Language::C, 0),
            (Language::Natural, 0),
            (Language::Generic, 1),
        ]);
        let mut evidence = Vec::new();
        let lower = text.to_lowercase();

        score(
            &mut scores,
            Language::JavaScript,
            lower.matches("console.log").count() as i32 * 8,
        );
        score(
            &mut scores,
            Language::JavaScript,
            lower.matches("const ").count() as i32 * 3 + lower.matches("let ").count() as i32 * 2,
        );
        score(
            &mut scores,
            Language::Rust,
            lower.matches("println!").count() as i32 * 8,
        );
        score(
            &mut scores,
            Language::Rust,
            lower.matches("fn ").count() as i32 * 5 + lower.matches("let mut ").count() as i32 * 4,
        );
        score(
            &mut scores,
            Language::C,
            lower.matches("printf(").count() as i32 * 8
                + lower.matches("int main").count() as i32 * 7
                + lower.matches("#include").count() as i32 * 6,
        );
        score(
            &mut scores,
            Language::Python,
            lower.matches("def ").count() as i32 * 5 + lower.matches("print(").count() as i32 * 3,
        );
        score(
            &mut scores,
            Language::Python,
            text.lines()
                .filter(|line| line.trim_end().ends_with(':'))
                .count() as i32
                * 2,
        );

        let cjk_count = text
            .chars()
            .filter(|character| ('\u{3400}'..='\u{9fff}').contains(character))
            .count();
        if cjk_count >= 3 {
            score(
                &mut scores,
                Language::Natural,
                i32::try_from(cjk_count.min(10)).unwrap_or(10),
            );
            evidence.push("contains a natural-language CJK phrase".to_owned());
        }
        if lower.contains("console.log") {
            evidence.push("JavaScript console call".to_owned());
        }
        if lower.contains("println!") || lower.contains("fn main") {
            evidence.push("Rust macro or function syntax".to_owned());
        }
        if lower.contains("printf(") || lower.contains("int main") {
            evidence.push("C function or stdio syntax".to_owned());
        }
        if lower.contains("print(") && !lower.contains("println!") && !lower.contains("printf(") {
            evidence.push("Python-style print call".to_owned());
        }
        if matches!(lower.split_whitespace().next(), Some("print")) {
            score(&mut scores, Language::Generic, 6);
            evidence.push("generic PRINT statement".to_owned());
        }

        let mut ranked: Vec<_> = scores.into_iter().collect();
        ranked.sort_by_key(|item| std::cmp::Reverse(item.1));
        let (best_language, best_score) = ranked[0];
        let second_score = ranked[1].1;
        if best_score <= 1 {
            return Detection::Unknown { evidence };
        }
        let confidence = ((best_score * 10 + 40).clamp(0, 99)) as u8;
        if best_score - second_score <= 1 && best_score < 8 {
            Detection::Ambiguous {
                candidates: ranked
                    .into_iter()
                    .take(2)
                    .map(|(language, value)| (language, ((value * 10 + 40).clamp(0, 99)) as u8))
                    .collect(),
                evidence,
            }
        } else {
            Detection::Detected {
                language: best_language,
                confidence,
                evidence,
            }
        }
    }

    #[must_use]
    pub fn detect_file(segments: &[Segment]) -> Detection {
        let detections: Vec<_> = segments.iter().map(Self::detect_segment).collect();
        let mut languages = Vec::new();
        for detection in &detections {
            if let Some(language) = detection.primary_language()
                && !languages.contains(&language)
            {
                languages.push(language);
            }
        }
        if languages.len() > 1 {
            Detection::Mixed { languages }
        } else {
            detections.into_iter().next().unwrap_or(Detection::Unknown {
                evidence: vec!["no segments".to_owned()],
            })
        }
    }
}

fn score(scores: &mut HashMap<Language, i32>, language: Language, value: i32) {
    *scores.entry(language).or_default() += value;
}

#[cfg(test)]
mod tests {
    use super::*;
    use linxts_source::{SourceId, Span};

    fn segment(text: &str) -> Segment {
        Segment {
            explicit_language: None,
            text: text.to_owned(),
            span: Span::new(SourceId(0), 0, text.len()),
            hint: None,
        }
    }

    #[test]
    fn uses_multiple_javascript_features() {
        let result = LanguageDetector::detect_segment(&segment("const x = 1; console.log(x);"));
        assert_eq!(result.primary_language(), Some(Language::JavaScript));
    }

    #[test]
    fn recognizes_c_syntax() {
        let result = LanguageDetector::detect_segment(&segment(
            "int main() { printf(\"Hello\"); return 0; }",
        ));
        assert_eq!(result.primary_language(), Some(Language::C));
    }

    #[test]
    fn recognizes_natural_language() {
        let result = LanguageDetector::detect_segment(&segment("幫我算出一到一百的總和"));
        assert_eq!(result.primary_language(), Some(Language::Natural));
    }
}
