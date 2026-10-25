# AI candidate protocol

Status: design frozen for the next implementation milestone; no model response is consumed by the current MVP.

AI backends return a versioned JSON envelope. Raw source code is not an executable result.

```json
{
  "schema_version": "1",
  "status": "candidate",
  "language": "natural",
  "interpretation": "Print integers from 1 through 10",
  "ir": {
    "schema_version": "0.1",
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

Model self-reported confidence is advisory and never bypasses these checks. Invalid JSON, invalid IR, timeout, cancellation, and provider failure are distinct escalation outcomes.

Online providers require explicit authorization before any source is transmitted. Logs must redact credentials and avoid storing raw prompts by default.
