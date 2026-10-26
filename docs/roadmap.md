# Roadmap

## Phase 1 — Compiler MVP

- [x] Git repository and Rust workspace
- [x] UTF-8 source manager, spans, snapshots, and bounded input
- [x] language hint scanner and aliases
- [x] deterministic staged language detection
- [x] tolerant lexer/parser with partial error nodes
- [x] bounded deterministic repair and audit records
- [x] Unified IR v0.2 with cross-language format expressions and validator
- [x] tree-walking VM backend
- [x] CLI commands and diagnostics
- [x] representative unit and integration tests
- [x] GitHub Actions configuration
- [x] basic C adapter (`int main`, integer declarations, `printf`, return)
- [x] Python indentation suites and `for ... in range(...)` lowering
- [ ] broader Python and JavaScript/Rust/C subset coverage
- [x] compilation cache for successful validated compilations

## Phase 2 — AI compilation

- [x] typed AI candidate protocol implementation
- [x] llama.cpp HTTP health and completion client
- [x] local 3B → 7B → optional larger model escalation
- [x] configurable Linxts-Mini first-tier endpoint and status checks
- [x] compiler-validated Linxts-Mini seed dataset, split validator, and independent benchmark
- [x] approved two-step 0.5B LoRA compatibility smoke and base/smoke-adapter benchmark
- [ ] expand the reviewed corpus and run a formally approved Mini/3B/7B comparison
- [x] timeout, retry, cancellation, and context/token budgets
- [x] mock provider and offline CI tests
- [x] reproducible real-model 3B → 7B acceptance runner and evidence specification
- [ ] execute real-model acceptance with approved, hash-pinned GGUF artifacts
- [x] validated AI candidate stored in the compilation cache
- [ ] explicitly authorized, extensible online provider interface

## Phase 3 — Mixed language

- [x] simple top-level variables shared across deterministic segments
- [x] simple natural-language arithmetic over a shared variable
- [x] VM and native integration coverage for shared variables, functions, loops, and formatting
- [ ] explicit cross-language conversion matrix
- [ ] richer lexical scopes and function boundaries
- [ ] module/import model

## Phase 4 — Native compilation

- [x] system-C bootstrap backend producing a standalone Linux executable
- [x] native executable integration test
- [ ] evaluate Cranelift and LLVM against the IR and runtime requirements
- [ ] define stable runtime ABI
- [ ] direct Linux x86_64 object/link pipeline
- [ ] optimizations and release artifacts

## Phase 5 — Ecosystem

- [ ] language server and VS Code extension
- [ ] formatter
- [ ] package manager
- [ ] Windows and macOS backends
- [ ] additional source-language adapters

Version targets are `v0.1.0-alpha`, `v0.2.0-alpha`, `v0.3.0-beta`, then `v1.0.0` after native compilation and compatibility commitments are met.
