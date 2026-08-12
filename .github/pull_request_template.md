## Summary

<!-- What changed and why? Keep the scope narrow. -->

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --workspace --all-features`
- [ ] Relevant macOS/Linux or integration checks

## Safety

- [ ] No credentials, private paths, runtime state, or generated artifacts are included.
- [ ] Native scheduler definitions and observed jobs remain unchanged unless explicitly intended.
