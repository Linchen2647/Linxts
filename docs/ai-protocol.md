# AI candidate protocol

Status: implemented as schema version `1` in `linxts-ai`. The llama.cpp backend supplies a recursive JSON Schema and then independently decodes and validates the typed IR.

AI backends return a versioned JSON envelope. Raw source code is not an executable result.

```json
{
  "schema_version": "1",
  "status": "candidate",
  "language": "natural",
  "interpretation": "Print integers from 1 through 10",
  "ir": {
    "schema_version": "0.2",
    "source_hash": "...",
    "body": { "statements": [] },
    "functions": []
  },
  "assumptions": [],
  "warnings": [],
  "provider": {
    "kind": "llama_cpp",
    "model_id": "local-model-id"
  }
}
```

The candidate schema recursively describes format segments as either literal text or nested value expressions. Format values are subject to the same scope, effect, depth, and source-binding checks as all other expressions.

The `provider` field is optional in model output and is overwritten by the trusted backend with the actual provider/model identity.

## Required checks

1. Enforce response size, timeout, and token budgets before decoding.
2. Decode strict JSON and reject trailing non-whitespace data.
3. Require known envelope and IR schema versions.
4. Require `status == "candidate"` and a supported language label.
5. Ensure the source hash and compilation context match the active request.
6. Deserialize only into typed IR structures.
7. Run IR validation and semantic analysis.
8. Enforce side-effect policy.
9. Cache only the validated candidate and include provider/model identity in the key.

Model self-reported confidence is advisory and never bypasses these checks. Invalid JSON, invalid IR, timeout, cancellation, and provider failure are distinct escalation outcomes. Successful fallback emits one `LXT-AI-211` informational diagnostic per route attempt so real-model acceptance can distinguish a completed 3B inference failure from a health-check failure and a validated 7B candidate; these diagnostics contain provider identities and outcomes, not raw source.

Online providers require explicit authorization before any source is transmitted. Logs must redact credentials and avoid storing raw prompts by default.
