# Changelog

All notable changes to Linxts will be documented here. The project follows Semantic Versioning after the alpha version suffix.

## [Unreleased]

### Added

- Initial Rust workspace and Git-managed compiler MVP.
- UTF-8 source manager with snapshots and Unicode source positions.
- Language hints, deterministic detection, tolerant parser, and bounded repair engine.
- Basic C hint/detection/parser adapter with `int main`, declarations, `printf`, and return.
- Inline known language hints without silently discarding trailing source.
- Unterminated-string repair reuses existing structural closers to avoid cascading over-repair.
- Linxts Unified IR v0.1, validator, tree-walking VM, and CLI.
- Mixed-language top-level variables and a deterministic Chinese intent subset.
- Versioned AI candidate protocol, recursive IR JSON Schema, and safety validation.
- Local llama.cpp HTTP client with health checks and 3B → 7B → larger escalation.
- Inference timeout, retry, cancellation, size/context/token, and authorization limits.
- Versioned, atomic, size-bounded compilation cache with IR revalidation.
- System-C bootstrap backend producing standalone Linux executables.
- CI configuration, documentation, examples, and tests.

### Not yet available

- online AI providers and validated AI cache.
- native ELF/EXE/macOS binary generation.
- complete Python, JavaScript, or Rust compatibility.
