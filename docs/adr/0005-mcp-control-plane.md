# ADR 0005: MCP control plane boundary

## Decision

The project exposes a stdio MCP server with the current stateless protocol
version `2026-07-28` and compatibility for `2025-11-25` initialization. It
supports `server/discover`, `tools/list`, and `tools/call`. Notifications are
accepted without a response, and the server does not rely on connection-local
session state.

The tool catalog is a fixed allowlist:

```text
automation_list
automation_inspect
runs_list
events_list
automation_run
run_cancel
approvals_list
metrics_list
```

`automation_run` accepts only a registered automation ID and delegates to the
same policy/service layer used by the CLI and Unix RPC daemon. `runs_list` and
`events_list` are bounded read-only views. `run_cancel` can signal only an
active run owned by the current daemon. Tool execution failures are returned as
`isError: true`; unknown tools and malformed request shapes are protocol errors.

## Security

The MCP boundary never accepts an executable, argv, shell string, file path
outside a registered definition, or approval decision from the model. Human
approval remains an application/UI responsibility. External issue, PR, web and
MCP content remains untrusted data.
