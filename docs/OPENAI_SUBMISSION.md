# OpenAI app submission checklist

This repository contains the implementation and review artifacts for a
Taskrail ChatGPT app. The target public app is the **public read-only profile**
enforced by `taskrail mcp-http`, not the full private local profile. The
`TASKRAIL_MCP_PROFILE=public taskrail mcp` form remains available for local
stdio review tests.

## Completed in this checkout

- MCP tool annotations are explicit for read-only, destructive, idempotent, and
  open-world behavior, and every descriptor includes an output schema.
- `automation.discover` is a true read-only RPC path; it does not reconcile or
  write the local Registry. The MCP discovery and scan tools use that path.
- Public-profile tool calls are allowlisted to 19 read-only tools, including a
  ChatGPT MCP Apps dashboard render tool. The private Fleet gateway also
  exposes a separate read-only multi-host dashboard resource. Write,
  delete, adoption, approval, cancellation, and execution tools are both
  omitted from `tools/list` and rejected if called directly.
- MCP responses omit native raw definitions and environment values, redact
  configured environment values in automation/run snapshots, redact event
  `raw`/`env` fields, and hide the current home-directory prefix in paths.
- `chatgpt-app-submission.json` contains the app information, all 19 public
  tools, five positive tests, and three negative tests.
- `OPENAI_RELEASE_NOTES.md` contains the portal-ready initial-submission
  release notes.
- `.codex-plugin/plugin.json` and `.mcp.json` describe a repository-local,
  read-only Codex plugin development connection.
- [Privacy](PRIVACY.md), [terms](TERMS.md), and [support](SUPPORT.md) pages are
  included and linked from the plugin metadata.

## Public profile

Start the public-facing MCP process with a secret injected by the deployment
environment, not committed to the repository:

```bash
export TASKRAIL_MCP_BEARER_TOKEN="$(secret-manager read taskrail/mcp-bearer-token)"
export TASKRAIL_MCP_ALLOWED_ORIGINS="https://your-approved-chatgpt-origin.example"
taskrail mcp-http \
  --profile public-read-only \
  --bind 127.0.0.1:8787 \
  --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
```

`taskrail mcp-http` defaults to and, unless explicitly passed
`--profile private`, exposes the public read-only profile. The public profile
exposes
`POST /mcp` and `GET /healthz`, requires a constant-time Bearer token, bounds
request bodies, rejects chunked requests, validates allowed origins, emits
request logs and an authenticated internal `/metrics` endpoint, and is
intended to sit behind a production TLS-terminating reverse proxy. The proxy
or hosting layer still must provide user authentication and per-user host
binding; the static bearer token is a process-to-proxy boundary, not an
end-user identity system.

The repository's private Secure MCP Tunnel instructions are for development/
testing connections only. Do not submit a localhost address, a private-network
address, a tunnel-only address, or the default full local profile.

The private HTTP profile is for a single protected Fleet target, not for public
app review. Enable it only with `--profile private`, a non-empty bearer token,
and a private TLS/authenticated edge. Do not route a public reviewer or a
shared tenant to this profile.

## External gates before submission

The following require the repository owner or deployment operator and cannot
be completed by a local source change:

- [ ] Push this exact review snapshot, including the policy pages, to the
      public default branch and verify every linked URL in an incognito window.
- [ ] Deploy a stable production HTTPS MCP endpoint that proxies `/mcp` to
      `taskrail mcp-http`, authenticates users, binds each request to an
      authorized host, and has no private-network dependency.
- [ ] Configure OpenAI-managed mTLS for ChatGPT client authentication and
      OAuth 2.1/OIDC when user authentication is required; do not use the
      internal process bearer as the end-user auth mechanism.
- [ ] Prepare a non-MFA test account/fixture that can connect to the public
      endpoint and produce deterministic local read-only data without exposing
      credentials or private source content.
- [ ] Verify the developer/business identity in OpenAI Platform and confirm
      the project has the required app-management read/write permissions.
- [ ] In the submission form, use the exact endpoint metadata and upload the
      Taskrail logo from `docs/assets/taskrail-mark.svg`.
- [ ] Run all five positive and three negative tests from
      `chatgpt-app-submission.json` against the production endpoint. A test is
      not complete merely because the process starts locally.
- [ ] Submit for review. After approval, use the separate Publish action; an
      approval is not itself a public release.

## Reviewer-facing boundaries

The public profile can inspect local automation inventory, native scheduler
observations on an ARM64 macOS or Linux agent, adoption journal state, read-only
GitHub observations, local
package/security findings, run history/logs, attention items, and audit events.
It cannot create or run commands, change scheduler ownership, change files,
approve an action, or send a write to GitHub or another public service.

The read-only GitHub tool uses the user's local `gh` authentication when that
integration is explicitly requested. Returned run logs are user-controlled
command output and are disclosed in the privacy policy; users should not put
secrets into command output.
