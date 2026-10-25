//! Source loading, snapshots, and Unicode-aware source locations.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use thiserror::Error;

pub const DEFAULT_MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct SourceId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct Span {
    pub source: SourceId,
    pub start: usize,
    pub end: usize,
}

impl Span {
    #[must_use]
    pub fn new(source: SourceId, start: usize, end: usize) -> Self {
        Self { source, start, end }
    }

    #[must_use]
    pub fn empty(source: SourceId, offset: usize) -> Self {
        Self::new(source, offset, offset)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Position {
    pub line: usize,
    pub column: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SourceFile {
    id: SourceId,
    name: String,
    path: Option<PathBuf>,
    text: String,
    line_starts: Vec<usize>,
    snapshot_hash: String,
}

impl SourceFile {
    #[must_use]
    pub fn from_text(id: SourceId, name: impl Into<String>, text: impl Into<String>) -> Self {
        let text = text.into();
        let line_starts = line_starts(&text);
        let snapshot_hash = format!("{:x}", Sha256::digest(text.as_bytes()));
        Self {
            id,
            name: name.into(),
            path: None,
            text,
            line_starts,
            snapshot_hash,
        }
    }

    pub fn from_path(id: SourceId, path: impl AsRef<Path>) -> Result<Self, SourceError> {
        Self::from_path_with_limit(id, path, DEFAULT_MAX_SOURCE_BYTES)
    }

    pub fn from_path_with_limit(
        id: SourceId,
        path: impl AsRef<Path>,
        max_bytes: usize,
    ) -> Result<Self, SourceError> {
        let path = path.as_ref();
        let bytes = std::fs::read(path).map_err(|source| SourceError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        if bytes.len() > max_bytes {
            return Err(SourceError::TooLarge {
                path: path.to_path_buf(),
                actual: bytes.len(),
                limit: max_bytes,
            });
        }
        let text = String::from_utf8(bytes).map_err(|error| SourceError::InvalidUtf8 {
            path: path.to_path_buf(),
            valid_up_to: error.utf8_error().valid_up_to(),
        })?;
        let name = path
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or("<source>")
            .to_owned();
        let mut file = Self::from_text(id, name, text);
        file.path = Some(path.to_path_buf());
        Ok(file)
    }

    #[must_use]
    pub fn id(&self) -> SourceId {
        self.id
    }

    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    #[must_use]
    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    #[must_use]
    pub fn text(&self) -> &str {
        &self.text
    }

    #[must_use]
    pub fn snapshot_hash(&self) -> &str {
        &self.snapshot_hash
    }

    #[must_use]
    pub fn position(&self, offset: usize) -> Position {
        let bounded = offset.min(self.text.len());
        let line_index = match self.line_starts.binary_search(&bounded) {
            Ok(index) => index,
            Err(index) => index.saturating_sub(1),
        };
        let line_start = self.line_starts[line_index];
        let column = self.text[line_start..bounded].chars().count() + 1;
        Position {
            line: line_index + 1,
            column,
        }
    }

    #[must_use]
    pub fn line_text(&self, line: usize) -> Option<&str> {
        let start = *self.line_starts.get(line.checked_sub(1)?)?;
        let end = self
            .line_starts
            .get(line)
            .copied()
            .unwrap_or(self.text.len());
        Some(self.text[start..end].trim_end_matches(['\r', '\n']))
    }
}

#[derive(Debug, Default)]
pub struct SourceManager {
    files: Vec<SourceFile>,
}

impl SourceManager {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn add_text(&mut self, name: impl Into<String>, text: impl Into<String>) -> SourceId {
        let id = SourceId(self.files.len() as u32);
        self.files.push(SourceFile::from_text(id, name, text));
        id
    }

    pub fn load(&mut self, path: impl AsRef<Path>) -> Result<SourceId, SourceError> {
        let id = SourceId(self.files.len() as u32);
        self.files.push(SourceFile::from_path(id, path)?);
        Ok(id)
    }

    #[must_use]
    pub fn get(&self, id: SourceId) -> Option<&SourceFile> {
        self.files.get(id.0 as usize)
    }

    #[must_use]
    pub fn files(&self) -> &[SourceFile] {
        &self.files
    }
}

#[derive(Debug, Error)]
pub enum SourceError {
    #[error("cannot read source file {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("source file {path} is not valid UTF-8 (first invalid byte at {valid_up_to})")]
    InvalidUtf8 { path: PathBuf, valid_up_to: usize },
    #[error("source file {path} is {actual} bytes; limit is {limit} bytes")]
    TooLarge {
        path: PathBuf,
        actual: usize,
        limit: usize,
    },
}

fn line_starts(text: &str) -> Vec<usize> {
    let mut starts = vec![0];
    for (index, byte) in text.bytes().enumerate() {
        if byte == b'\n' && index + 1 < text.len() {
            starts.push(index + 1);
        }
    }
    starts
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_unicode_columns() {
        let source = SourceFile::from_text(SourceId(0), "unicode.lx", "甲乙\nprint");
        assert_eq!(source.position("甲".len()), Position { line: 1, column: 2 });
        assert_eq!(
            source.position("甲乙\n".len()),
            Position { line: 2, column: 1 }
        );
    }

    #[test]
    fn detects_invalid_utf8() {
        let dir = std::env::temp_dir();
        let path = dir.join(format!("linxts-invalid-{}.lx", std::process::id()));
        std::fs::write(&path, [0xff, 0xfe]).expect("write fixture");
        let result = SourceFile::from_path(SourceId(0), &path);
        let _ = std::fs::remove_file(path);
        assert!(matches!(result, Err(SourceError::InvalidUtf8 { .. })));
    }
}
