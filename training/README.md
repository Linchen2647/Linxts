# Linxts-Mini training workspace

This workspace builds auditable training data and model comparisons for the first AI fallback tier. The repository currently uses Linxts IR schema `0.2`; generated labels always use the schema constant exported by `linxts-ir` rather than a duplicated example schema.

## Dataset pipeline

Generate a deterministic seed corpus into an ignored directory:

```bash
cargo run -p linxts-training --offline -- generate \
  --repository-root . \
  --output training/data/fallback-v2 \
  --seed 20261026

cargo run -p linxts-training --offline -- validate \
  --dataset training/data/fallback-v2
```

The generator derives cases from checked-in examples, compiler/parser/repair tests, the AI candidate protocol, and fixed holdouts. Every target is produced from source accepted by the deterministic compiler, normalized to the active input snapshot, passed through `CandidateEnvelope::validate` and the IR validator, executed in the VM, and compared with explicit expected stdout. Unsupported natural-language inputs use a checked-in deterministic code oracle; unverified AI output is never used as a label.

The generator provides three practical scales within the same deterministic pipeline: the original 21-case conceptual seed, the 1,121-sample pilot corpus, and the current 1,941-sample fallback-focused corpus. The expanded corpus contains 1,699 training, 221 validation, and 21 fixed test examples. It adds compiler-verified factorial, square accumulation, countdown, repeated-output, maximum, composed arithmetic, function-call, and divisibility intents that the deterministic natural-language subset does not handle. Four paraphrases of each semantic program share one `group_id`, so they cannot cross split boundaries. The 21-case test split contains 16 disjoint fallback-only holdouts plus the five original cross-task holdouts. This remains a bounded training corpus, not a claim of general compiler coverage.

## Record format

Each JSONL record contains:

- `schema_version`: dataset schema identity;
- `id` and `group_id`: stable identity and leakage boundary;
- `split`: `train`, `validation`, or fixed `test`;
- `task`: one of the six Linxts-Mini tasks;
- `input.source`, compiler diagnostics, and optional deliberately invalid IR;
- `target`: the exact strict AI `CandidateEnvelope`, containing current Linxts IR;
- `verification`: source hash, compiler/validator/execution checks, and expected stdout;
- `provenance`: repository source path, reference, and generation method.

The validator rejects unknown fields, duplicate content, cross-split `group_id` leakage, source-hash mismatch, invalid candidate envelopes, invalid IR, changed compiler labels, execution mismatches, invalid correction fixtures that accidentally pass, and split file fingerprint changes.

## Benchmark

The no-AI deterministic baseline runs directly from the frozen **test split** by default:

```bash
cargo run -p linxts-training --offline -- benchmark \
  --dataset training/data/fallback-v2
```

Use `--split all` only for development diagnostics; model selection must use the untouched test split.

After approval has placed a base model locally, generate prediction JSONL without allowing downloads:

```bash
python3 -I training/scripts/evaluate_model.py \
  --model training/models/<local-base-model> \
  --dataset training/data/fallback-v2 \
  --split test \
  --output training/runs/<run>/predictions.jsonl
```

Add `--adapter training/runs/<run>/adapter` to evaluate a LoRA checkpoint. Model systems produce JSONL prediction records:

```json
{"sample_id":"...","candidate":{},"latency_ms":12.5,"prompt_tokens":100,"completion_tokens":200}
```

Evaluate a base model, Linxts-Mini, 3B, or 7B prediction file independently:

```bash
cargo run -p linxts-training --offline -- benchmark \
  --dataset training/data/fallback-v2 \
  --predictions training/runs/<run>/predictions.jsonl \
  --system linxts-mini
```

Reports include language detection accuracy, repair parse success, strict candidate schema validity, IR validator pass rate, execution correctness, latency, token counts, and escalation-required rate. Training loss is not an acceptance metric.

## Environment and training entrypoint

The environment probe is dependency-free and read-only:

```bash
python3 -I training/scripts/check_environment.py
```

Prompt formatting can be tested without PyTorch or a model:

```bash
python3 -I training/scripts/train_lora.py \
  --config training/configs/qwen25-coder-05b-fallback-v2.toml \
  --format-smoke
```

After explicit approval has supplied an isolated, pinned training environment and a local base-model directory, run the mandatory two-step optimizer smoke test:

```bash
python3 -I training/scripts/train_lora.py \
  --config training/configs/qwen25-coder-05b-fallback-v2.toml \
  --smoke-test
```

The script uses local-only model loading, refuses missing models and existing output directories, fixes Python/PyTorch/data seeds, records dependency and dataset manifests, supports gradient checkpointing, and resumes from the latest recorded checkpoint only when `--resume` is given. It also rejects any example that exceeds the configured sequence length instead of silently truncating a trusted target; the expanded corpus uses length 1,536 after tokenizer analysis found a 1,032-token maximum. The ordinary LoRA configuration is the default. The 4-bit configuration is experimental and must not be used until its AMD-specific smoke test passes.

The approved isolated ROCm environment and pinned Qwen2.5-Coder-0.5B model completed a real two-step LoRA optimizer smoke on the RX 6800 XT. The exact package lock and experiment record are checked in. This proves bounded pipeline compatibility only: the smoke adapter scored 0% strict schema validity on the fixed test split and is not eligible for router use or release.

A subsequent authorized 100-step pilot trained on 995 generated examples after correcting the prompt to include the trusted source hash. It reached 40% strict schema validity, 40% IR-validator pass rate, and 20% execution correctness on the five fixed holdouts. The only deterministic-fallback holdout remained unsolved, so the pilot checkpoint was not eligible for router use or GGUF release.

The first expanded run was invalidated after a semantic audit found factorial 6 in both generated training data and a fixed holdout under different group IDs. The corrected fallback-v2 corpus excludes that semantic holdout and adds a regression assertion. Its leak-free run trained for 300 optimizer steps on the 1,699/221 train/validation split. On 21 fixed holdouts, all predictions passed strict schema and IR validation, 18 executed correctly (85.71%), and all 17 cases that actually require AI fallback—including the excluded factorial-6 test—executed correctly. Three deterministic-supported cross-task cases remained semantically wrong. The adapter is therefore recorded as a candidate for GGUF/llama.cpp acceptance, not as a deployed or production-accepted Mini tier.

## Artifact policy

Source, configurations, scripts, tests, experiment manifests, and small text reports belong in Git. Generated datasets, checkpoints, merged models, caches, and GGUF files are ignored. Record model IDs, licenses, immutable revisions, hashes, dependency versions, random seeds, commands, and metrics in `training/experiments/`; do not commit weights or credentials.
