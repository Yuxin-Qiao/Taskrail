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
- ChatGPT Scheduled tasks can call the local MCP adapter through a private
  OpenAI Secure MCP Tunnel.
- Native scheduler discovery and typed semantic integrations stay at the edge of
  the core manager.
- Commands are direct argv and shell strings are rejected.
- The semantic integration layer covers Mole, restic, rclone, GitHub, Homebrew,
  mas, OSV-Scanner, Gitleaks, Trivy, and Topgrade; write-capable actions are
  persisted approval-gated and fail closed.

## Documents

- [Security](../SECURITY.md) — authoritative security boundary.
- [Contributing](../CONTRIBUTING.md) — development workflow.
- [Architecture decisions](adr/) — focused decisions that still describe the
  current implementation.
- [Research report](../deep-research-report.md) — historical product and
  architecture research; it contains proposals that were intentionally removed
  from the current MVP.
- [ChatGPT integration](chatgpt.md) — connect ChatGPT Scheduled tasks to a Mac
  or Linux Taskrail host.
- [Acceptance checklist](ACCEPTANCE.md) — reproducible release-gate commands
  and evidence requirements.
- [Native integration architecture](adr/0031-native-integration-semantic-layer.md)
  — shared plan, policy, parsing, and verification boundary.

The removed Codex App Server, privileged-helper, and generic remote policy-engine
documents are no longer part of the current product contract. The MCP adapter
and local approval records described in `chatgpt.md` are the supported ChatGPT
integration boundary.
