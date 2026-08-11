# Taskrail documentation

Taskrail is a local automation manager for developers. The product center is
small:

```text
add/register → schedule → run → history/logs → tui
```

## Current product

- The Rust crate and CLI are named `taskrail`.
- The local Registry stores automations, runs, logs, events, and metrics.
- The daemon evaluates interval and cron triggers.
- The TUI is the primary visual view.
- Native scheduler discovery and optional Codex/GitHub integrations stay at the
  edge of the core manager.
- Commands are direct argv and shell strings are rejected.

## Documents

- [Security](../SECURITY.md) — authoritative security boundary.
- [Contributing](../CONTRIBUTING.md) — development workflow.
- [Architecture decisions](adr/) — focused decisions that still describe the
  current implementation.
- [Research report](../deep-research-report.md) — historical product and
  architecture research; it contains proposals that were intentionally removed
  from the current MVP.

The removed MCP, Codex App Server, approval, privileged-helper, and policy-engine
documents are no longer part of the current product contract.
