# Contributing

Keep changes focused and preserve the safety model. The `core` module must not
depend on launchd, systemd, Codex, OpenAI, GitHub or a UI framework.

Changes to adoption, rollback, scheduling, permissions, shell handling or
persistent schema need regression tests and should describe:

- the invariant being protected;
- the failure path and rollback behavior;
- deterministic verification evidence;
- any migration or permission impact.

Before submitting a change:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
git diff --check
```

Never add a generic root command or allow agent output to bypass policy,
approval, capability restrictions or deterministic verification.
