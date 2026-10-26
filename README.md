# Linxts

**Linxts Adaptive Compilation System** is an experimental, AI-native adaptive multi-language compiler written in Rust. It accepts `.lx` files, detects language regions, performs traceable deterministic repairs, lowers supported syntax into a language-neutral IR, validates that IR, and executes it with a built-in VM.

> Status: **v0.1.0-alpha MVP in development.** The deterministic compiler, VM, structured AI protocol, local llama.cpp fallback router, validated cache, and a Linux native bootstrap backend are functional for the documented subset. `linxts build` produces a real executable through the system C compiler; direct Cranelift/LLVM code generation is not implemented. Use `--vm` to emit a versioned `.lxb` IR/VM artifact instead.

## Why Linxts

Linxts explores a compiler model where users express intent in Python-, JavaScript-, Rust-, C-, generic-, or natural-language fragments while the compiler owns detection, conservative repair, semantic validation, and execution. AI is a fallback, never the trust boundary: model output must become validated Linxts IR before it can run.

## Current architecture

```text
.lx source -> source manager -> hint scanner -> detector
           -> tolerant parser -> deterministic repair (if needed)
           -> Linxts IR v0.2 -> IR validator -> tree-walking VM
```

AI fallback path:

```text
unresolved -> llama.cpp 3B -> 7B -> optional larger local model
           -> strict candidate JSON -> IR validator -> VM
```

An online-provider tier is reserved by the router but is skipped without explicit authorization; no online provider is implemented yet. Normal parseable programs do not invoke a model.

## Requirements

- Rust stable 1.85 or newer
- Linux x86_64 is the first supported development target
- No GPU, model, API key, Python, Node.js, or external language runtime is required for the MVP
- A C11 compiler (`cc`) is required only for `linxts build`; the resulting executable does not need Linxts or AI at runtime

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
linxts build <file.lx>     Produce a native executable via system C (default)
linxts build <file.lx> --vm  Emit a .lxb VM artifact instead
linxts models list         Show configured AI models
linxts models test         Health-check configured llama.cpp servers
linxts config show         Show effective MVP settings
linxts doctor              Inspect local prerequisites
```

Compilation flags include `--aggressive` (default), `--strict`, `--offline`, `--no-ai`, `--no-cache`, `--show-ir`, `--verbose`, and `--fun-errors`.

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

C subset with a descriptive hint:

```text
^C語言
int main() {
    printf("Hello from C");
    return 0;
}
```

Known short hints may also share a line with their first statement, for example `^python print("Hello")`.

Mixed input with a shared variable:

```text
^python
x = 10

^javascript
console.log(x)

^自然語言
把 x 乘以 5，然後顯示結果
```

The current VM prints `10` and `50` for the mixed example. Integration coverage also exercises a Python-defined function called from JavaScript, a Rust-style loop over shared values, and formatted JavaScript/Rust/C output through both the VM and native bootstrap backend.

## Supported subset

| Capability | Status |
| --- | --- |
| Unicode source, spans, line/column diagnostics | Implemented |
| `^python`, `^py`, `^javascript`, `^js`, `^rust`, `^c`, `^C語言`, descriptive and natural-language hints | Implemented |
| Marker-free deterministic language detection | Implemented, heuristic scoring |
| Integers, strings, booleans, variables, assignment, arithmetic, comparison | Implemented |
| `print`, `Print`, `console.log`, `println!`, and `printf` | Implemented subset |
| Python f-strings, JavaScript templates, Rust `{}`/captured placeholders, and C format placeholders | Implemented basic interpolation subset |
| C `int`/`void` functions, integer declarations, `printf`, and return | Implemented subset |
| Brace-delimited `if` and `while` | Implemented subset |
| Functions, calls, parameters, return | Implemented subset |
| Missing quote/parenthesis/brace repair and selected typo repair | Implemented |
| Chinese sum/even-range and simple variable arithmetic intents | Deterministic subset |
| Python indentation and `for ... in range(...)` | Implemented subset; ranges lower to validated loop IR |
| Full Python/JavaScript/Rust/C semantics | Not implemented |
| llama.cpp structured fallback and 3B → 7B → larger-local escalation | Implemented, opt-in configuration |
| AI timeout, retry, cancellation token, token/context budget, health check | Implemented |
| Online model providers | Not implemented; router authorization gate exists |
| Compilation cache | Implemented for successful validated compilations; native artifacts not cached yet |
| Linux x86_64 native executable | Implemented through an explicit system-C bootstrap backend |
| Direct Cranelift/LLVM backend | Planned |

The Python adapter recognizes space- or tab-indented suites after `:` and one-, two-, or three-argument `range` loops. The range step must be a nonzero integer literal; both ascending and descending ranges are supported. This remains a deliberate subset of Python rather than a Python runtime.

Language adapters intentionally preserve distinct semantics as the project grows; syntax resemblance does not imply Python, JavaScript, Rust, and C are interchangeable.

## AI configuration

No model is downloaded automatically. Start compatible llama.cpp HTTP servers yourself, then configure one or more loopback endpoints:

```bash
export LINXTS_LLAMA_3B_URL=http://127.0.0.1:8080
export LINXTS_LLAMA_3B_MODEL=my-3b-model
export LINXTS_LLAMA_7B_URL=http://127.0.0.1:8081
export LINXTS_LLAMA_7B_MODEL=my-7b-model
# Optional: LINXTS_LLAMA_LARGE_URL and LINXTS_LLAMA_LARGE_MODEL

linxts models list
linxts models test
```

The client uses llama.cpp's native `/health` and `/completion` endpoints. CLI configuration rejects non-loopback URLs to avoid accidentally uploading source to an untrusted host. `LINXTS_AI_TIMEOUT_SECS` (1–600) and `LINXTS_AI_RETRIES` (0–5) control bounded inference. `--no-ai` prevents all model calls; `--offline` permits local endpoints but reserves the online tier as disabled.

Every completion must contain a strict versioned candidate envelope. Source hash, schema version, side effects, scopes, calls, error nodes, and the entire IR are validated before the compiler replaces deterministic parse errors. Invalid JSON or IR escalates to the next configured local tier. No online provider is implemented or contacted.

## Native compilation

To produce and run a real Linux executable:

```bash
linxts build examples/hello.lx --output hello
./hello
```

To emit the inspectable VM artifact instead:

```bash
linxts build examples/hello.lx --vm   # examples/hello.lxb
```

The native path lowers validated Linxts IR to generated C and invokes the configured C11 compiler (`$CC`, otherwise `cc`). The temporary C file is removed after compilation. This is deliberately reported as the **system C bootstrap backend**, not as Cranelift, LLVM, or direct ELF code generation. The resulting executable needs the platform C runtime, but not Linxts, llama.cpp, a model, Python, or Node.js.

## Compilation cache

Successful validated compilations—including deterministic detection/repair data or an accepted AI candidate—are cached under `$LINXTS_CACHE_DIR`, `$XDG_CACHE_HOME/linxts`, or `~/.cache/linxts`. Keys include source/context/dependency hashes, compiler and IR schema versions, model identities, repair mode, target, and compile options. Cache entries are size-bounded, written atomically, version-checked, source-bound, and revalidated before use. Use `--no-cache` or `LINXTS_DISABLE_CACHE=1` to bypass it. Native artifacts are not cached yet.

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
- [IR v0.2](docs/ir-spec.md)
- [Repair engine](docs/repair-engine.md)
- [AI protocol](docs/ai-protocol.md)
- [Local AI environment](docs/local-ai-environment.md)
- [Roadmap](docs/roadmap.md)

## Roadmap summary

1. Deterministic compiler MVP and VM
2. llama.cpp backend, structured AI candidates, and escalation router
3. Cross-language scopes and conversions
4. Cranelift or LLVM native backend producing standalone Linux ELF files
5. Editor tooling, packages, Windows, and macOS

## License

Apache License 2.0. See [LICENSE](LICENSE).
