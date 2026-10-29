use crate::{Compilation, CompileOptions};
use linxts_ir::{IR_SCHEMA_VERSION, Validator};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};
use thiserror::Error;

const CACHE_FORMAT_VERSION: u32 = 1;
const MAX_CACHE_ENTRY_BYTES: usize = 16 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CacheKeyInput {
    pub source_hash: String,
    pub context_hash: String,
    pub dependency_hashes: Vec<String>,
    pub compiler_version: String,
    pub ir_schema_version: String,
    pub model_identity: Option<String>,
    pub repair_mode: String,
    pub target: String,
    pub options: BTreeMap<String, String>,
}

impl CacheKeyInput {
    #[must_use]
    pub fn new(source_hash: impl Into<String>, compile_options: CompileOptions) -> Self {
        Self {
            source_hash: source_hash.into(),
            context_hash: String::new(),
            dependency_hashes: Vec::new(),
            compiler_version: env!("CARGO_PKG_VERSION").to_owned(),
            ir_schema_version: IR_SCHEMA_VERSION.to_owned(),
            model_identity: None,
            repair_mode: if compile_options.strict {
                "strict".to_owned()
            } else if compile_options.aggressive {
                "aggressive".to_owned()
            } else {
                "disabled".to_owned()
            },
            target: format!("{}-{}", std::env::consts::ARCH, std::env::consts::OS),
            options: BTreeMap::from([
                ("no_ai".to_owned(), compile_options.no_ai.to_string()),
                ("offline".to_owned(), compile_options.offline.to_string()),
            ]),
        }
    }

    #[must_use]
    pub fn digest(&self) -> String {
        let mut canonical = self.clone();
        canonical.dependency_hashes.sort();
        let encoded = serde_json::to_vec(&canonical).expect("cache key fields are serializable");
        format!("{:x}", Sha256::digest(encoded))
    }
}

#[derive(Debug, Clone)]
pub struct CompilationCache {
    root: PathBuf,
}

impl CompilationCache {
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    #[must_use]
    pub fn default_for_user() -> Self {
        if let Some(path) = std::env::var_os("LINXTS_CACHE_DIR") {
            return Self::new(path);
        }
        if let Some(path) = std::env::var_os("XDG_CACHE_HOME") {
            return Self::new(PathBuf::from(path).join("linxts"));
        }
        if let Some(home) = std::env::var_os("HOME") {
            return Self::new(PathBuf::from(home).join(".cache/linxts"));
        }
        Self::new(".cache/linxts")
    }

    pub fn load(&self, key_input: &CacheKeyInput) -> Result<Option<Compilation>, CacheError> {
        let key = key_input.digest();
        let path = self.entry_path(&key);
        let metadata = match std::fs::metadata(&path) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(error) => {
                return Err(CacheError::Io {
                    path,
                    source: error,
                });
            }
        };
        if metadata.len() > MAX_CACHE_ENTRY_BYTES as u64 {
            return Err(CacheError::TooLarge {
                actual: metadata.len() as usize,
                limit: MAX_CACHE_ENTRY_BYTES,
            });
        }
        let bytes = std::fs::read(&path).map_err(|source| CacheError::Io {
            path: path.clone(),
            source,
        })?;
        let entry: CacheEntry = serde_json::from_slice(&bytes)
            .map_err(|error| CacheError::Corrupt(error.to_string()))?;
        if entry.format_version != CACHE_FORMAT_VERSION {
            return Err(CacheError::VersionConflict {
                actual: entry.format_version,
                expected: CACHE_FORMAT_VERSION,
            });
        }
        if entry.key != key
            || entry.source_hash != key_input.source_hash
            || entry.compilation.module.source_hash != key_input.source_hash
        {
            return Err(CacheError::Corrupt(
                "cache key or source hash mismatch".to_owned(),
            ));
        }
        validate_compilation(&entry.compilation)?;
        Ok(Some(entry.compilation))
    }

    pub fn store(
        &self,
        key_input: &CacheKeyInput,
        compilation: &Compilation,
    ) -> Result<(), CacheError> {
        validate_compilation(compilation)?;
        if compilation.module.source_hash != key_input.source_hash {
            return Err(CacheError::Corrupt(
                "compilation source hash does not match cache key".to_owned(),
            ));
        }
        let key = key_input.digest();
        let entry = CacheEntry {
            format_version: CACHE_FORMAT_VERSION,
            key: key.clone(),
            source_hash: key_input.source_hash.clone(),
            compilation: compilation.clone(),
        };
        let bytes =
            serde_json::to_vec(&entry).map_err(|error| CacheError::Corrupt(error.to_string()))?;
        if bytes.len() > MAX_CACHE_ENTRY_BYTES {
            return Err(CacheError::TooLarge {
                actual: bytes.len(),
                limit: MAX_CACHE_ENTRY_BYTES,
            });
        }

        let directory = self.version_directory();
        std::fs::create_dir_all(&directory).map_err(|source| CacheError::Io {
            path: directory.clone(),
            source,
        })?;
        let path = self.entry_path(&key);
        let temporary = temporary_path(&directory, &key);
        let write_result = (|| {
            let mut file = std::fs::OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&temporary)
                .map_err(|source| CacheError::Io {
                    path: temporary.clone(),
                    source,
                })?;
            file.write_all(&bytes).map_err(|source| CacheError::Io {
                path: temporary.clone(),
                source,
            })?;
            file.sync_all().map_err(|source| CacheError::Io {
                path: temporary.clone(),
                source,
            })?;
            std::fs::rename(&temporary, &path).map_err(|source| CacheError::Io {
                path: path.clone(),
                source,
            })?;
            Ok(())
        })();
        if write_result.is_err() {
            let _ = std::fs::remove_file(temporary);
        }
        write_result
    }

    fn version_directory(&self) -> PathBuf {
        self.root.join(format!("v{CACHE_FORMAT_VERSION}"))
    }

    fn entry_path(&self, key: &str) -> PathBuf {
        self.version_directory().join(format!("{key}.json"))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct CacheEntry {
    format_version: u32,
    key: String,
    source_hash: String,
    compilation: Compilation,
}

#[derive(Debug, Error)]
pub enum CacheError {
    #[error("cache I/O failed for {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("cache entry is corrupt: {0}")]
    Corrupt(String),
    #[error("cache format version {actual} conflicts with expected version {expected}")]
    VersionConflict { actual: u32, expected: u32 },
    #[error("cache entry is {actual} bytes; limit is {limit}")]
    TooLarge { actual: usize, limit: usize },
    #[error("refusing to cache or load an invalid compilation: {0}")]
    InvalidCompilation(String),
}

fn validate_compilation(compilation: &Compilation) -> Result<(), CacheError> {
    if compilation.has_errors() {
        return Err(CacheError::InvalidCompilation(
            "compilation contains error diagnostics".to_owned(),
        ));
    }
    let validation = Validator::validate(&compilation.module);
    if !validation.is_empty() {
        return Err(CacheError::InvalidCompilation(
            validation
                .into_iter()
                .map(|error| format!("{}: {}", error.code, error.message))
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    Ok(())
}

fn temporary_path(directory: &Path, key: &str) -> PathBuf {
    let nonce = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    directory.join(format!(".{key}.{}.{nonce}.tmp", std::process::id()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Compiler;
    use linxts_source::{SourceFile, SourceId};

    #[test]
    fn validated_compilation_round_trips() {
        let root = test_root("round-trip");
        let source = SourceFile::from_text(SourceId(0), "cache.lx", "PRINT 1\n");
        let options = CompileOptions::default();
        let compilation = Compiler::compile(&source, options);
        let key = CacheKeyInput::new(source.snapshot_hash(), options);
        let cache = CompilationCache::new(&root);
        cache.store(&key, &compilation).expect("store cache");
        let loaded = cache.load(&key).expect("load cache").expect("cache hit");
        assert_eq!(loaded, compilation);
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn rejects_cache_version_conflict() {
        let root = test_root("version-conflict");
        let source = SourceFile::from_text(SourceId(0), "cache.lx", "PRINT 1\n");
        let options = CompileOptions::default();
        let compilation = Compiler::compile(&source, options);
        let key_input = CacheKeyInput::new(source.snapshot_hash(), options);
        let cache = CompilationCache::new(&root);
        cache.store(&key_input, &compilation).expect("store cache");
        let path = cache.entry_path(&key_input.digest());
        let mut json: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&path).expect("read entry")).expect("JSON");
        json["format_version"] = serde_json::json!(999);
        std::fs::write(&path, serde_json::to_vec(&json).expect("encode")).expect("write entry");
        assert!(matches!(
            cache.load(&key_input),
            Err(CacheError::VersionConflict { actual: 999, .. })
        ));
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn key_changes_with_model_and_mode() {
        let mut local = CacheKeyInput::new("hash", CompileOptions::default());
        let mut strict = CacheKeyInput::new(
            "hash",
            CompileOptions {
                strict: true,
                ..CompileOptions::default()
            },
        );
        assert_ne!(local.digest(), strict.digest());
        strict.model_identity = Some("model-a".to_owned());
        local.model_identity = Some("model-b".to_owned());
        assert_ne!(local.digest(), strict.digest());
    }

    fn test_root(label: &str) -> PathBuf {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("clock")
            .as_nanos();
        std::env::temp_dir().join(format!(
            "linxts-cache-{label}-{}-{nonce}",
            std::process::id()
        ))
    }
}
