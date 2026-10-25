# Linxts

**Linxts Adaptive Compilation System** is an experimental, AI-native adaptive multi-language compiler written in Rust. It accepts `.lx` files, detects language regions, performs traceable deterministic repairs, lowers supported syntax into a language-neutral IR, validates that IR, and executes it with a built-in VM.

> Status: **v0.1.0-alpha MVP in development.** The deterministic compiler and VM are functional for the documented subset. Native ELF generation and llama.cpp integration are not implemented in this revision. `linxts build` emits a versioned `.lxb` IR/VM artifact, not an executable.

## Why Linxts

Linxts explores a compiler model where users express intent in Python-, JavaScript-, Rust-, generic-, or natural-language fragments while the compiler owns detection, conservative repair, semantic validation, and execution. AI is a fallback, never the trust boundary: model output must become validated Linxts IR before it can run.

## Current architecture

```text
.lx source -> source manager -> hint scanner -> detector
           -> tolerant parser -> deterministic repair (if needed)
           -> Linxts IR v0.1 -> IR validator -> tree-walking VM
```

Planned AI path:

```text
unresolved -> llama.cpp 3B -> 7B -> optional larger local model
           -> explicitly authorized online provider -> IR validator
```

Normal parseable programs do not invoke a model.

## Requirements

- Rust stable 1.85 or newer
- Linux x86_64 is the first supported development target
- No GPU, model, API key, Python, Node.js, or external language runtime is required for the MVP

## Build and install

```bash
cargo build --workspace
cargo install --path crates/linxts-cli
```

Or run from the workspace:

```bash
cargo run -p linxts-cli -- run examples/hello.lx
```

Expected standard output:

```text
1
```

The CLI writes backend status to standard error so program output remains pipe-friendly.

## CLI

```text
linxts run <file.lx>       Compile and run with the Linxts VM
linxts check <file.lx>     Parse and validate without execution
linxts explain <file.lx>   Show detection, repairs, diagnostics, and IR
linxts build <file.lx>     Emit a .lxb VM artifact (not native code)
linxts models list         Show configured AI models
linxts models test         Test model availability (not enabled yet)
linxts config show         Show effective MVP settings
linxts doctor              Inspect local prerequisites
```

Compilation flags include `--aggressive` (default), `--strict`, `--offline`, `--no-ai`, `--show-ir`, `--verbose`, and `--fun-errors`.

## Examples

Generic input:

```text
PRINT 1
```

Hinted JavaScript with a repairable delimiter:

```text
^javascript
console.log("Hello"
```

Mixed input with a shared variable:

```text
^python
x = 10

^javascript
console.log(x)

^自然語言
把 x 乘以 5，然後顯示結果
```

The current VM prints `10` and `50` for the mixed example.

## Supported subset

| Capability | Status |
| --- | --- |
| Unicode source, spans, line/column diagnostics | Implemented |
| `^python`, `^py`, `^javascript`, `^js`, `^rust`, descriptive and natural-language hints | Implemented |
| Marker-free deterministic language detection | Implemented, heuristic scoring |
| Integers, strings, booleans, variables, assignment, arithmetic, comparison | Implemented |
| `print`, `Print`, `console.log`, `println!` | Implemented |
| Brace-delimited `if` and `while` | Implemented subset |
| Functions, calls, parameters, return | Implemented subset |
| Missing quote/parenthesis/brace repair and selected typo repair | Implemented |
| Chinese sum/even-range and simple variable arithmetic intents | Deterministic subset |
| Python indentation semantics | Not implemented |
| Full Python/JavaScript/Rust semantics | Not implemented |
| llama.cpp fallback router | Planned next milestone |
| Compilation cache | Planned |
| Native Linux ELF backend | Planned; `build` is not native |

Language adapters intentionally preserve distinct semantics as the project grows; syntax resemblance does not imply Python, JavaScript, and Rust are interchangeable.

## AI configuration

No model is downloaded automatically. This revision does not issue network requests or upload source code. The next milestone adds a llama.cpp HTTP provider with explicit health checks, timeout and escalation limits, versioned structured output, and mock-backed CI tests. Online providers will remain opt-in and require explicit authorization.

## Safety and privacy

- Deterministic compilation runs locally.
- AI output is untrusted and must pass schema and IR validation.
- Aggressive repair does not synthesize system commands, deletion, secret transmission, or other major side effects.
- `--offline` will prohibit online providers; `--no-ai` disables all model fallback.
- Model weights, `.env` files, tokens, and API keys are ignored by Git.

See [SECURITY.md](SECURITY.md) for reporting and threat-model notes.

## Development

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

CI performs the same checks without GPU access, model files, or credentials.

## Documentation

- [Architecture](docs/architecture.md)
- [Language hints](docs/language-hints.md)
- [IR v0.1](docs/ir-spec.md)
- [Repair engine](docs/repair-engine.md)
- [AI protocol](docs/ai-protocol.md)
- [Roadmap](docs/roadmap.md)

## Roadmap summary

1. Deterministic compiler MVP and VM
2. llama.cpp backend, structured AI candidates, and escalation router
3. Cross-language scopes and conversions
4. Cranelift or LLVM native backend producing standalone Linux ELF files
5. Editor tooling, packages, Windows, and macOS

## License

Apache License 2.0. See [LICENSE](LICENSE).
