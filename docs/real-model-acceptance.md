# Real-model 3B → 7B acceptance

This acceptance is deliberately separate from the mock-provider CI suite. Passing the normal workspace tests proves routing, protocol validation, and failure handling with controlled fixtures; it does **not** prove that a real 3B or 7B model can produce valid Linxts IR.

No model is downloaded by Linxts or by the acceptance runner. Obtain user approval before downloading a GGUF file, installing llama.cpp, installing build tools, or changing GPU drivers.

## What the acceptance proves

A passing run proves, for the exact pinned artifacts recorded in its manifest, that:

1. both loopback llama.cpp servers pass `/health`;
2. the 3B server completes a real inference attempt but does not produce an acceptable candidate;
3. the router records that failure and advances to the 7B tier;
4. the 7B server returns a candidate that passes strict JSON decoding, source-hash binding, candidate policy, and IR validation;
5. the accepted IR executes in the VM and prints the checked-in expected output;
6. the same route and output pass at least three consecutive repetitions with temperature `0.0`, retries disabled, and cache disabled.

A 3B health failure (`attempt #0`) is not accepted as real-model escalation evidence. A run where the 3B model produces a structurally valid candidate also does not prove escalation, even if its semantics are wrong; the runner fails rather than relabeling that result. This exposes an important current limit: the compiler can validate admissibility, but it cannot infer an oracle for arbitrary user intent.

## Pinned inputs

The runner records:

- Git commit and dirty/clean state;
- Rust and Cargo versions;
- kernel and architecture;
- caller-supplied llama.cpp version and backend identity;
- exact 3B and 7B model IDs;
- SHA-256 hashes computed directly from both GGUF files;
- fixture SHA-256, timeout, sampler temperature, retry count, and repetition count;
- health, explanation, route diagnostics, runtime stdout, and runtime stderr for review.

The initial checked-in fixture asks for `6!` and expects `720`. It is outside the deterministic natural-language subset but representable with existing validated integer/loop IR. Model pairs for which the 3B tier already succeeds do not demonstrate escalation with this fixture; add a reviewed fixture with an explicit expected output rather than weakening the acceptance criteria.

## Prerequisites

- Two already-approved, locally stored GGUF files whose stated parameter classes are 3B and 7B.
- Two real `llama-server` processes bound only to loopback, normally ports `8080` and `8081`.
- The same documented context size and compatible server options for both tiers.
- Vulkan startup logs showing the selected backend and layer offload when claiming GPU acceleration.
- `cargo`, `rustc`, `git`, and `sha256sum` in `PATH`.

Record the complete server launch commands separately with the evidence. CLI flags vary between llama.cpp versions, so use the help output from the pinned server binary rather than copying unverified flags. Do not expose the servers on a LAN address.

## Running

Set metadata and paths for artifacts that already exist locally, then invoke the runner:

```bash
export LINXTS_LLAMA_3B_URL=http://127.0.0.1:8080
export LINXTS_LLAMA_3B_MODEL='<exact 3B model ID and quantization>'
export LINXTS_LLAMA_3B_GGUF='/absolute/path/to/approved-3b.gguf'
export LINXTS_LLAMA_7B_URL=http://127.0.0.1:8081
export LINXTS_LLAMA_7B_MODEL='<exact 7B model ID and quantization>'
export LINXTS_LLAMA_7B_GGUF='/absolute/path/to/approved-7b.gguf'
export LINXTS_LLAMA_SERVER_VERSION='<exact version or commit>'
export LINXTS_LLAMA_BACKEND='Vulkan; include relevant build/offload flags'

scripts/accept-real-model-escalation.sh
```

The runner uses `cargo build --locked --offline`, clears the optional larger-model tier, forces `--offline --no-cache`, sets retries to zero, and requires at least three repetitions. Evidence is written to a new timestamped directory under `target/real-model-acceptance/`; an existing directory is never overwritten. `LINXTS_ACCEPTANCE_OUT` can select a new explicit directory, `LINXTS_AI_TIMEOUT_SECS` can increase the local timeout, and `LINXTS_ACCEPTANCE_REPEATS` can increase—but not reduce below three—the repetition count.

## Interpreting results

Only `result=PASS` in `manifest.txt`, together with all retained logs, is a successful real-model acceptance. A failure is still useful evidence and must be reported as such. In particular:

- 3B accepted: no 3B→7B escalation was demonstrated;
- 3B attempt absent: only health/transport fallback may have occurred;
- 7B invalid: the selected 7B artifact did not satisfy the protocol for this fixture;
- output mismatch: validated structure did not establish semantic correctness;
- inconsistent repetitions: the model/server configuration is not reproducible enough for acceptance.

Never describe `cli_reports_mock_escalation_attempts` or the router's mock tests as this real-model acceptance. They remain deterministic CI coverage and require no model, GPU, network service, or credentials.
