# Architecture

Linxts is split into a small number of working crates rather than one crate per future subsystem.

- `linxts-source`: bounded UTF-8 loading, snapshots, source IDs, spans, and Unicode positions.
- `linxts-ir`: Unified IR v0.1 plus structural, scope, call, and error-node validation.
- `linxts-compiler`: hints, staged detection, lexer, tolerant parser, deterministic repair, natural-language deterministic lowering, and pipeline diagnostics.
- `linxts-runtime`: local tree-walking VM with loop and call-depth limits.
- `linxts-cli`: user-facing commands and stable exit-code boundaries.

## Current pipeline

1. The source manager reads bounded UTF-8 and computes a SHA-256 snapshot.
2. The hint scanner recognizes line-level `^` markers while tracking quoted strings.
3. The detector scores syntax and token features. Explicit known hints take precedence.
4. The parser produces IR and diagnostics without panicking. Failed parsing retains partial error nodes.
5. In aggressive mode, deterministic repair runs for at most four passes and parsing is retried.
6. Natural-language phrases covered by deterministic rules lower directly to IR.
7. The IR validator rejects schema mismatches, unresolved error nodes, scope errors, bad calls, and excessive nesting.
8. `run` executes valid IR in the VM. `build` serializes valid IR as a `.lxb` artifact.

## Trust boundaries

Source text, cache entries, model responses, and serialized IR are untrusted. Native or VM execution is allowed only after validation. Future AI providers must produce the versioned candidate envelope in `ai-protocol.md`; raw generated code is not directly executed.

## Backend status

The VM is an explicitly labeled development backend. `.lxb` is JSON-encoded IR for inspectability during alpha development and is not a native binary. Native code generation will use a separately reviewed backend and runtime ABI.
