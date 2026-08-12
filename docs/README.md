# Taskrail documentation

[简体中文文档](README.zh-CN.md) · [中文 ChatGPT 集成指南](chatgpt.zh-CN.md)

Taskrail is a local automation manager for developers. The product center is
small:

```text
add/register → schedule → run → history/logs → browser dashboard
```

## Current product

- The Rust crate and CLI are named `taskrail`.
- The local Registry stores automations, runs, logs, events, and metrics.
- The daemon evaluates interval and cron triggers.
- The daemon hosts the primary loopback browser dashboard at
  `http://127.0.0.1:10100` by default; `taskrail gui` opens it, while `taskrail tui` is the
  terminal fallback.
- If `10100` is occupied, the daemon tries the bounded loopback range through
  `10110`, and `taskrail gui` discovers the active Taskrail endpoint instead of
  opening an unrelated local service.
- The browser dashboard is a thin client over the daemon's local RPC handlers;
  it is a local convenience surface, not a public or Tunnel-exposed web
  service.
- The browser dashboard supports English, Simplified Chinese, Japanese, and
  Korean. It detects the browser language on first load and stores manual
  language changes only in browser local storage.
- The ChatGPT MCP app can render the same read-only overview inside the
  conversation through a versioned MCP Apps resource; its widget calls only
  typed MCP tools and never the local browser HTTP endpoint.
- The Rust CLI, daemon, TUI, browser dashboard, and local MCP adapter are
  supported only on ARM64 macOS and ARM64 Linux. The daemon uses a restricted
  Unix socket for the control plane and loopback-only HTTP for the dashboard.
- A connected ChatGPT client can call the local MCP adapter through an OpenAI
  Secure MCP Tunnel; an observed future ChatGPT Scheduled trigger is a separate
  external verification gate. Public App review and hosted deployment are also
  separate release gates.
- `taskrail mcp-fleet` can aggregate multiple explicitly configured hosts into
  one MCP app; host endpoints and token environment references remain local,
  and write routing is opt-in and read-only by default. Its private host-targeted
  surface includes a versioned read-only MCP Apps fleet dashboard plus native
  adoption, drift acknowledgement, typed integrations, and durable approvals.
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
- [ChatGPT integration](chatgpt.md) — connect ChatGPT Scheduled tasks to an
  ARM64 macOS or Linux Taskrail host.
- [Fleet example](../examples/fleet.yaml) — local multi-host endpoint metadata
  template; copy it outside the repository before enabling hosts.
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
