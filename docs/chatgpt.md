# ChatGPT integration

[简体中文](chatgpt.zh-CN.md)

Taskrail's ChatGPT integration is a tool-only MCP app. ChatGPT provides the
natural-language conversation, the **Scheduled** page, and notifications.
Taskrail provides the local daemon, scheduler, command execution, run history,
logs, and host-local audit events.

The connection can be per host, or you can use the explicit fleet gateway to
present several Taskrail hosts through one MCP app. Each host still owns its
own Registry, policy, approvals, and execution. Set a stable label so ChatGPT
can distinguish them:

```bash
export TASKRAIL_HOST_LABEL="macbook-pro"
```

For several hosts, copy `examples/fleet.yaml` to the local ignored config path,
replace the example endpoints, inject each token through its `token_env`, and
start the gateway:

```bash
taskrail mcp-fleet --config ~/.config/taskrail/fleet.yaml
```

The fleet app exposes `taskrail_fleet_overview` first, followed by explicit
`host_id`-targeted discovery, inventory, run history, logs, and lifecycle tools.
Fleet hosts are read-only by default. `allow_writes: true` is an explicit opt-in
for a trusted private endpoint; the remote Taskrail host still enforces its own
policy and approval boundary.

## Start the local backend

The daemon owns the SQLite Registry and listens on a user-scoped restricted Unix
socket on supported ARM64 macOS/Linux hosts.
After startup it performs a read-only native discovery pass every five minutes
by default. Use `--discovery-interval-seconds` to adjust the interval. The pass
reconciles observed jobs, records drift, and marks a source missing only when
that provider was successfully queried; unavailable providers are not treated
as empty.
On macOS, install the LaunchAgent:

```bash
taskrail daemon --install
taskrail status
```

On Linux, install a systemd user unit. For a headless host, enable lingering
first so the user manager survives logout:

```bash
loginctl enable-linger "$USER"
taskrail daemon --install
taskrail status
```

The unit is written to `~/.config/systemd/user/taskrail.service` (or the
directory selected by `XDG_CONFIG_HOME`). The Registry uses `XDG_DATA_HOME`
and the socket uses `XDG_RUNTIME_DIR` when those variables are absolute;
otherwise taskrail falls back to `~/.local/share/taskrail/`. The install command
fails closed if a systemd user manager is not available.

If you manage systemd units yourself, the explicit foreground form remains:

```bash
taskrail daemon --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
```

The MCP process is intentionally short-lived and speaks stdio. The tunnel
client starts it when ChatGPT needs a tool call:

```bash
taskrail mcp --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
```

Do not run the MCP process with its stdout redirected to a human-readable log;
stdout is the MCP protocol stream. Diagnostics belong on stderr.

The default `taskrail mcp` profile is the full local profile for a private
developer connection. It includes write and execution tools, but those tools
remain typed, direct-argv, and policy/approval controlled. Never expose that
profile through a public HTTP endpoint.

Before configuring the OpenAI side, check the local prerequisites without
printing any credential:

```bash
taskrail integration chatgpt-doctor
```

This reports daemon/socket, MCP adapter, tunnel-client, Tunnel ID, and runtime
key presence. It does not prove that ChatGPT can reach the Tunnel; that final
check is `tunnel-client doctor` after the Platform configuration exists.

After `CONTROL_PLANE_TUNNEL_ID` and `CONTROL_PLANE_API_KEY` are configured in
the local environment, Taskrail can start the managed tunnel runtime for you:

```bash
taskrail integration chatgpt-connect
tunnel-client runtimes status taskrail-local --json
```

The connect command passes only the reference `env:CONTROL_PLANE_API_KEY` to
`tunnel-client`; it does not put the key in a profile argument, Registry row,
log, or Git file.

For unattended reconnects on macOS, keep the value in the user launchd
environment instead of shell history or a repository file:

```bash
launchctl setenv CONTROL_PLANE_API_KEY '<runtime key>'
taskrail integration chatgpt-connect
```

Taskrail reads that value only to start the short-lived tunnel child and never
prints it. On Linux, use the environment mechanism of the user service
manager, such as a protected systemd `EnvironmentFile`.

## Private connection with Secure MCP Tunnel

Secure MCP Tunnel keeps the MCP server private and uses an outbound connection
from the host. Create a tunnel in OpenAI Platform and associate it with the
ChatGPT workspace that will use the app. Then configure the latest
`tunnel-client` release with a stdio profile:

```bash
export CONTROL_PLANE_API_KEY="<store this outside the repository>"

tunnel-client init \
  --sample sample_mcp_stdio_local \
  --profile taskrail-local \
  --tunnel-id "<tunnel_id>" \
  --mcp-command "taskrail mcp --socket $HOME/.local/share/taskrail/taskraild.sock"

tunnel-client doctor --profile taskrail-local --explain
tunnel-client run --profile taskrail-local
```

Keep `tunnel-client run` healthy while the app is being used. The tunnel
runtime key and tunnel id are deployment secrets; never commit them to this
repository or put them in an automation definition.

## Public review profile

OpenAI public app review requires a stable, production-hosted HTTPS MCP
endpoint. A local Secure MCP Tunnel is suitable for development connections,
not as the public submission endpoint. Use the built-in HTTP adapter behind a
TLS-terminating reverse proxy:

```bash
export TASKRAIL_MCP_BEARER_TOKEN="<inject from a secret manager>"
taskrail mcp-http \
  --profile public-read-only \
  --bind 127.0.0.1:8787 \
  --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
```

`taskrail mcp-http` defaults to the public read-only profile. It exposes
`POST /mcp` and `GET /healthz`, requires Bearer authentication, bounds request
bodies, and refuses chunked requests. The proxy/hosting layer must still add
end-user authentication and per-user host binding. The public profile exposes
only status, native discovery, inventory, adoption
journal inspection, read-only GitHub observations, local package/security
inspection, run history/logs, attention items, and audit events. It does not
expose automation creation, deletion, pause/resume, execution, cancellation,
native adoption, integration writes, or approval operations.

For a private, single-host Fleet target that must receive explicit write or
run requests, use the authenticated private profile instead:

```bash
export TASKRAIL_MCP_BEARER_TOKEN="<inject from a secret manager>"
taskrail mcp-http \
  --profile private \
  --bind 127.0.0.1:8788 \
  --socket "${XDG_RUNTIME_DIR:-$HOME/.local/share}/taskrail/taskraild.sock"
```

Private mode is never the default: keep it bound behind a private TLS/auth
edge, use one authorized host per endpoint, and do not expose it as a shared
public relay. The Fleet gateway's `allow_writes: true` hosts must point to
such an explicitly protected private endpoint; public-read-only endpoints
correctly reject Fleet write calls.

The public endpoint must add its own user authentication and host binding
before proxying to a user's daemon. Do not turn the read-only profile into a
shared unauthenticated relay, and do not submit a `localhost`, private-network,
or tunnel-only URL. See the [OpenAI submission checklist](OPENAI_SUBMISSION.md)
for the remaining portal steps and the exact test pack.

## Connect the app in ChatGPT

In ChatGPT:

1. Enable Developer mode in Settings → Security and login.
2. Open the Plugins/Apps developer connection screen and create an app.
3. Choose **Tunnel**, select the Taskrail tunnel, and review the discovered
   tools.
4. Refresh the app after changing tool descriptors or rebuilding Taskrail.

The app should be connected before creating a Scheduled task. Scheduled tasks
can then call the app at the requested time, for example:

```text
Every Sunday at 09:00, run the Taskrail automation "Mole cleanup" on the MacBook host.
If the run fails, get its logs and notify me with the exit status and the next action.
```

For multiple hosts without the fleet gateway, use a separate tunnel/profile and
host label for each machine. With the fleet gateway, call
`taskrail_fleet_overview` first and name the configured `host_id` explicitly.
Always call `taskrail_overview` or
`taskrail_fleet_overview` first when the user wants a complete host summary. It
returns the host identity, daemon state, fresh discovery, Taskrail inventory,
recent runs, and attention items in one read-only result. Use
`taskrail_status` for a lightweight connectivity check. When
the user asks what automation tasks already exist on the host, prefer
`taskrail_discover_local_automations` for a fresh native scan; a successful
ChatGPT response is not proof that a different host's daemon ran the task.
Daemon status also includes the last background discovery timestamp, complete
providers, drift count, and confirmed missing-source count.

## Tool surface

The adapter exposes focused tools rather than a generic shell endpoint:

- `taskrail_status` — verify daemon connectivity and identify the host.
- `taskrail_overview` — return one safe host summary combining identity,
  discovery, Taskrail automations, recent runs, and attention items.
- `taskrail_list_automations` / `taskrail_get_automation` — inspect the local
  inventory.
- `taskrail_discover_local_automations` — freshly scan launchd, cron, systemd,
  and Homebrew services and return safe observed-task summaries.
- `taskrail_scan_native` — perform a fresh read-only launchd, cron, systemd,
  or Homebrew scan without mutating native definitions
  or the Registry.
- `taskrail_list_integrations` — inspect the built-in integration catalog,
  executable detection, and doctor status on this host.
- `taskrail_schedule_integration` — persist a typed read-only or dry-run
  integration as a local Automation; recurring writes are refused.
- `taskrail_list_adoptions` / `taskrail_get_adoption` — inspect native adoption
  journal state.
- `taskrail_adopt_automation` / `taskrail_rollback_adoption` — preflight/apply
  or explicitly restore a native scheduler adoption transaction.
- `taskrail_acknowledge_drift` — accept a fresh external baseline while leaving
  the owned Automation paused.
- `taskrail_create_automation` — create a direct-argv manual, interval, or cron
  task.
- `taskrail_delete_automation` — delete only a managed Automation without run
  history; observed/adopted definitions remain protected.
- `taskrail_pause_automation` / `taskrail_resume_automation` — change managed
  runtime state.
- `taskrail_run_automation` / `taskrail_cancel_run` — explicitly start or stop
  a run.
- `taskrail_list_runs` / `taskrail_get_run_logs` — inspect outcomes.
- `taskrail_list_attention` / `taskrail_list_events` — review failures, drift,
  and recent activity.
- `taskrail_mole` — use typed Mole actions for detection, analysis, status,
  history, and cleanup dry-run planning. Real cleanup is destructive and held
  by Taskrail policy until an explicit, expiring approval is granted.
- `taskrail_restic` / `taskrail_rclone` — use typed snapshot, repository,
  transfer, and sync actions; backup/copy/real sync are policy-controlled.
- `taskrail_github` / `taskrail_homebrew` — use fixed read-only GitHub
  observations and typed Homebrew health/maintenance actions.
- `taskrail_mas`, `taskrail_osv_scanner`, `taskrail_gitleaks`, and
  `taskrail_trivy` — inspect local packages and security findings without
  exposing secret or match values.
- `taskrail_topgrade` — inspect or plan updates; run requires approval.
- `taskrail_list_approvals`, `taskrail_request_approval`, `taskrail_approve`,
  `taskrail_reject`, and `taskrail_execute_approved` — review and operate the
  persisted, plan-bound approval flow. Approval is one-time and never a shell
  grant.

The adapter does not accept arbitrary shell strings, expose the SQLite file, or
change observed native jobs. Native adoption remains an explicit local
operation.

For public review, only the read-only subset is advertised and enforced by
`TASKRAIL_MCP_PROFILE=public`. The full tool surface above is for a private,
user-owned connection; keeping those surfaces separate prevents a public
endpoint from becoming a general-purpose local command runner.

## What this integration does not claim

ChatGPT's Scheduled page is the scheduler for the ChatGPT prompt. Taskrail's
daemon is the scheduler for local Automations. A Scheduled task that calls
Taskrail at 09:00 is therefore a two-stage workflow: ChatGPT wakes up and calls
the local app; Taskrail then runs the selected local Automation according to
its persisted typed definition and records the result. The connected app does
not automatically import or control ChatGPT's own Scheduled-task list; that
list remains managed by ChatGPT's Scheduled page.

For unattended operation, keep the Taskrail daemon and tunnel client running,
and verify failures through the returned run status and logs.
