# Linxts-Mini training workspace

This workspace builds auditable training data and model comparisons for the first AI fallback tier. The repository currently uses Linxts IR schema `0.2`; generated labels always use the schema constant exported by `linxts-ir` rather than a duplicated example schema.

## Dataset pipeline

Generate a deterministic seed corpus into an ignored directory:

```bash
cargo run -p linxts-training --offline -- generate \
  --repository-root . \
  --output training/data/seed-v1 \
  --seed 20261026

cargo run -p linxts-training --offline -- validate \
  --dataset training/data/seed-v1
```

The generator derives cases from checked-in examples, compiler/parser/repair tests, the AI candidate protocol, and fixed holdouts. Every target is produced from source accepted by the deterministic compiler, normalized to the active input snapshot, passed through `CandidateEnvelope::validate` and the IR validator, executed in the VM, and compared with explicit expected stdout. Unsupported natural-language inputs use a checked-in deterministic code oracle; unverified AI output is never used as a label.

The initial corpus is intentionally small and serves pipeline development and smoke testing. It is not large enough to claim useful fine-tuning.

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

The no-AI deterministic baseline runs directly from the same frozen corpus:

```bash
cargo run -p linxts-training --offline -- benchmark \
  --dataset training/data/seed-v1
```

Model systems produce JSONL prediction records:

```json
{"sample_id":"...","candidate":{},"latency_ms":12.5,"prompt_tokens":100,"completion_tokens":200}
```

Evaluate a base model, Linxts-Mini, 3B, or 7B prediction file independently:

```bash
cargo run -p linxts-training --offline -- benchmark \
  --dataset training/data/seed-v1 \
  --predictions training/runs/<run>/predictions.jsonl \
  --system linxts-mini
```

Reports include language detection accuracy, repair parse success, strict candidate schema validity, IR validator pass rate, execution correctness, latency, token counts, and escalation-required rate. Training loss is not an acceptance metric.

## Artifact policy

Source, configurations, scripts, tests, experiment manifests, and small text reports belong in Git. Generated datasets, checkpoints, merged models, caches, and GGUF files are ignored. Record model IDs, licenses, immutable revisions, hashes, dependency versions, random seeds, commands, and metrics in `training/experiments/`; do not commit weights or credentials.
