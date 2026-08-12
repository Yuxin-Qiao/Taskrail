# Contributing

Keep changes focused on the local automation manager. The `core` module must
not depend on launchd, systemd, Codex, OpenAI, GitHub, or a UI framework.

The main user journey is:

```text
add/register → list → daemon → run → history/logs → tui
```

Changes to discovery, adoption, rollback, scheduling, command execution, or the
persistent Registry need regression tests and should describe the invariant and
the failure path being protected.

Before submitting a change:

```bash
cargo fmt --all -- --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo test --locked --workspace --all-features
cargo test --locked --workspace --doc
cargo audit -D warnings
cargo deny check advisories bans licenses sources
git diff --check
```

The repository's default Rust toolchain is pinned in `rust-toolchain.toml`.
CI also tests the declared minimum supported Rust version and the current
stable toolchain. Dependency changes must keep `Cargo.lock`, `deny.toml`, and
the security workflows passing.

Keep optional integrations at the edge. Do not add arbitrary shell execution,
remote write operations, or a second source of truth for runs and logs.
