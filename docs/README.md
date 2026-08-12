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
- The Rust CLI, daemon, TUI, and local MCP adapter are supported on macOS, Linux,
  and Windows; macOS/Linux use a restricted Unix socket, Windows uses a named
  pipe and Task Scheduler, and the SwiftUI desktop view remains macOS-only.
- ChatGPT Scheduled tasks can call the local MCP adapter through a private
  OpenAI Secure MCP Tunnel.
- The public read-only `taskrail mcp-http` adapter is available for deployment
  behind a TLS/authentication edge; the container example under `deploy/` is
  single-host only and is not a hosted multi-tenant service.
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
- [ChatGPT integration](chatgpt.md) — connect ChatGPT Scheduled tasks to a
  macOS, Linux, or Windows Taskrail host.
- [OpenAI submission checklist](OPENAI_SUBMISSION.md) — public review profile,
  metadata, test cases, policy pages, and external launch gates.
- [OpenAI release notes](OPENAI_RELEASE_NOTES.md) — portal-ready initial
  submission summary.
- [Privacy policy](PRIVACY.md), [Terms](TERMS.md), and [Support](SUPPORT.md) —
  public app-review policy pages.
- [Acceptance checklist](ACCEPTANCE.md) — reproducible release-gate commands
  and evidence requirements.
- [Native integration architecture](adr/0031-native-integration-semantic-layer.md)
  — shared plan, policy, parsing, and verification boundary.

The removed Codex App Server, privileged-helper, and generic remote policy-engine
documents are no longer part of the current product contract. The MCP adapter
and local approval records described in `chatgpt.md` are the supported ChatGPT
integration boundary.
