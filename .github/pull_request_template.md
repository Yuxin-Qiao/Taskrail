## Summary

<!-- What changed and why? Keep the scope narrow. -->

## Validation

- [ ] `cargo fmt --all -- --check`
- [ ] `cargo clippy --locked --workspace --all-targets --all-features -- -D warnings`
- [ ] `cargo test --locked --workspace --all-features`
- [ ] `cargo test --locked --workspace --doc`
- [ ] `cargo package --locked --package taskrail`
- [ ] `cargo audit -D warnings`
- [ ] `cargo deny check advisories bans licenses sources`
- [ ] Relevant macOS/Linux or integration checks

## Safety

- [ ] No credentials, private paths, runtime state, or generated artifacts are included.
- [ ] Native scheduler definitions and observed jobs remain unchanged unless explicitly intended.
