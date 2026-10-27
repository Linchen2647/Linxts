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
- Local llama.cpp HTTP client with health checks and configurable Linxts-Mini → 3B → 7B → larger escalation.
- Compiler-validated Linxts-Mini dataset generation, leakage-safe splits, independent benchmark, local-only LoRA runner, and experiment records.
- A pinned Qwen2.5-Coder-0.5B two-step LoRA smoke on ROCm/RX 6800 XT, recorded as pipeline compatibility rather than model-quality acceptance.
- A compiler-validated 1,121-sample pilot corpus and authorized 100-step LoRA run, improving held-out schema/validator validity to 40% and execution correctness to 20% without promoting the checkpoint.
- A 1,941-sample fallback-focused corpus with paraphrase-group split isolation, 16 new AI-required holdouts, strict sequence-length checks that reject label truncation, semantic holdout-leak regression coverage, and a leak-free 300-step LoRA run reaching 100% schema/validator validity and 85.71% execution correctness over 21 fixed tests.
- Inference timeout, retry, cancellation, size/context/token, and authorization limits.
- Per-attempt AI route diagnostics plus a hash-pinned, repeatable real 3B → 7B acceptance runner kept separate from mock CI coverage.
- Versioned, atomic, size-bounded compilation cache with IR revalidation.
- System-C bootstrap backend producing standalone Linux executables.
- Reproducible local AI environment assessment covering llama.cpp availability and Vulkan/ROCm readiness.
- CI configuration, documentation, examples, and tests.

### Not yet available

- online AI providers.
- direct native ELF/EXE/macOS code generation (Linux executables currently use the system-C bootstrap backend).
- complete Python, JavaScript, Rust, or C compatibility.
