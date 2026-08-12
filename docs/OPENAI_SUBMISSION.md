# OpenAI app submission checklist

This repository contains the implementation and review artifacts for a
Taskrail ChatGPT app. The target public app is the **public read-only profile**
(`TASKRAIL_MCP_PROFILE=public`), not the full private local profile.

## Completed in this checkout

- MCP tool annotations are explicit for read-only, destructive, idempotent, and
  open-world behavior, and every descriptor includes an output schema.
- `automation.discover` is a true read-only RPC path; it does not reconcile or
  write the local Registry. The MCP discovery and scan tools use that path.
- Public-profile tool calls are allowlisted to 17 read-only tools. Write,
  delete, adoption, approval, cancellation, and execution tools are both
  omitted from `tools/list` and rejected if called directly.
- MCP responses omit native raw definitions and environment values, redact
  configured environment values in automation/run snapshots, redact event
  `raw`/`env` fields, and hide the current home-directory prefix in paths.
- `chatgpt-app-submission.json` contains the app information, all 17 public
  tools, five positive tests, and three negative tests.
- `.codex-plugin/plugin.json` and `.mcp.json` describe a repository-local,
  read-only Codex plugin development connection.
- [Privacy](PRIVACY.md), [terms](TERMS.md), and [support](SUPPORT.md) pages are
  included and linked from the plugin metadata.

## Public profile

Start the public-facing MCP process with:

```bash
TASKRAIL_MCP_PROFILE=public taskrail mcp \
  --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
```

The process must run behind a production HTTPS MCP endpoint with user
authentication and per-user host binding. The repository's private Secure MCP
Tunnel instructions are for development/testing connections only. Do not
submit a localhost address, a private-network address, a tunnel-only address,
or the default full local profile.

## External gates before submission

The following require the repository owner or deployment operator and cannot
be completed by a local source change:

- [ ] Push this exact review snapshot, including the policy pages, to the
      public default branch and verify every linked URL in an incognito window.
- [ ] Deploy a stable production HTTPS MCP endpoint that launches the public
      profile, authenticates users, binds each request to an authorized host,
      and has no private-network dependency.
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
observations, adoption journal state, read-only GitHub observations, local
package/security findings, run history/logs, attention items, and audit events.
It cannot create or run commands, change scheduler ownership, change files,
approve an action, or send a write to GitHub or another public service.

The read-only GitHub tool uses the user's local `gh` authentication when that
integration is explicitly requested. Returned run logs are user-controlled
command output and are disclosed in the privacy policy; users should not put
secrets into command output.
