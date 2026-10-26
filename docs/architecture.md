# Architecture

Linxts is split into a small number of working crates rather than one crate per future subsystem.

- `linxts-source`: bounded UTF-8 loading, snapshots, source IDs, spans, and Unicode positions.
- `linxts-ir`: Unified IR v0.2 plus structural, scope, call, format-expression, and error-node validation.
- `linxts-compiler`: hints, staged detection, lexer, tolerant parser, deterministic repair, natural-language deterministic lowering, pipeline diagnostics, and validated compilation cache.
- `linxts-runtime`: local tree-walking VM with loop and call-depth limits.
- `linxts-backend`: native system-C bootstrap code generator and toolchain driver.
- `linxts-ai`: versioned candidate protocol, safety validation, native llama.cpp HTTP client, and bounded model escalation.
- `linxts-cli`: user-facing commands, optional AI orchestration, and stable exit-code boundaries.

## Current pipeline

1. The source manager reads bounded UTF-8 and computes a SHA-256 snapshot.
2. The hint scanner recognizes line-level `^` markers while tracking quoted strings.
3. The detector scores syntax and token features. Explicit known hints take precedence.
4. The parser produces IR and diagnostics without panicking. Failed parsing retains partial error nodes.
5. In aggressive mode, deterministic repair runs for at most four passes and parsing is retried.
6. Natural-language phrases covered by deterministic rules lower directly to IR.
7. The deterministic IR validator rejects schema mismatches, unresolved error nodes, scope errors, bad calls, and excessive nesting.
8. If only parser/validation errors remain and AI is enabled, configured local llama.cpp tiers are tried in 3B → 7B → larger order. The router bounds health checks, retries, time, prompt/response size, context, output, cancellation, and escalation.
9. Candidate JSON is decoded with unknown fields denied, bound to the active source hash, restricted to pure/output effects, and passed through the IR validator again.
10. Successful compilations are atomically cached under a versioned content key; every hit is size/version/source checked and its IR is revalidated.
11. `run` executes valid IR in the VM. Default `build` lowers IR to generated C and invokes the system C11 compiler; `build --vm` serializes `.lxb`.

## Trust boundaries

Source text, cache entries, model responses, and serialized IR are untrusted. Native or VM execution is allowed only after validation. AI providers must produce the versioned candidate envelope in `ai-protocol.md`; raw generated code is never directly executed. The CLI accepts local llama.cpp endpoints only on loopback addresses. The reserved online tier cannot run unless both offline mode is disabled and explicit authorization is supplied; the current CLI supplies no such authorization and implements no online provider.

## Backend status

The VM is an explicitly labeled development backend. `.lxb` is JSON-encoded IR for inspectability during alpha development and is not a native binary. The system-C bootstrap backend produces a genuine platform executable linked to the platform C runtime and requiring no Linxts or AI at execution time. It is an interim backend: direct Cranelift/LLVM lowering and a stable Linxts runtime ABI remain planned.
