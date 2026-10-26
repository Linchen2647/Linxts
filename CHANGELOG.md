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
- Linxts Unified IR v0.2 with validated cross-language format expressions, tree-walking VM support, and native backend lowering.
- Python indentation suites and one-, two-, or three-argument `for ... in range(...)` loops with positive or negative literal steps.
- Mixed-language top-level variables and a deterministic Chinese intent subset.
- VM and native integration coverage combining shared variables, cross-language function calls, loops, and formatting.
- Versioned AI candidate protocol, recursive IR JSON Schema, and safety validation.
- Local llama.cpp HTTP client with health checks and 3B → 7B → larger escalation.
- Inference timeout, retry, cancellation, size/context/token, and authorization limits.
- Versioned, atomic, size-bounded compilation cache with IR revalidation.
- System-C bootstrap backend producing standalone Linux executables.
- Reproducible local AI environment assessment covering llama.cpp availability and Vulkan/ROCm readiness.
- CI configuration, documentation, examples, and tests.

### Not yet available

- online AI providers.
- direct native ELF/EXE/macOS code generation (Linux executables currently use the system-C bootstrap backend).
- complete Python, JavaScript, Rust, or C compatibility.
