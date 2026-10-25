## Summary

<!-- What behavior changes and why? -->

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets -- -D warnings`
- [ ] `cargo test --workspace`
- [ ] `cargo build --workspace`

## Compiler guarantees

- [ ] Source spans and repair records remain traceable.
- [ ] New untrusted input is validated before execution.
- [ ] Tests assert outputs, diagnostics, and/or exit status rather than only absence of panic.
- [ ] Documentation accurately marks unsupported behavior.
- [ ] No secrets, model weights, generated artifacts, or user data are included.

## Security and compatibility

<!-- Describe side effects, provider/network behavior, IR/schema changes, and migration impact. -->
