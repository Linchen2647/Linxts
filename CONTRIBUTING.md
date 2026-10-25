# Contributing to Linxts

Linxts is in alpha development. Discuss large language, IR, security, or backend changes in an issue before implementation.

## Local checks

Run all checks before submitting a change:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
cargo build --workspace
```

Tests must not require a GPU, local model, API key, network access, or interactive input. AI behavior must be covered by a mock provider.

## Change expectations

- Keep source spans and repair provenance intact through lowering.
- Never execute raw model output.
- Add output, diagnostic-code, and exit-status assertions for compiler behavior.
- Avoid adding an empty crate for a future feature.
- Update README status tables and the roadmap truthfully.
- Do not commit `.env` files, credentials, model weights, or user source files.

Use Conventional Commits such as `feat(parser): ...`, `fix(repair): ...`, and `test(ir): ...`.

## Pull requests

Describe the behavior change, validation performed, security implications, and unsupported cases. Keep `main` buildable; feature work should normally branch from `dev` once that integration branch exists remotely.
