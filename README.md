# auto

`auto` is a local-first automation control plane for macOS. It discovers existing
`launchd` jobs, user crontab entries, and Linux `systemd --user` services,
records them in a local SQLite Registry, and keeps them observation-only until an
explicit adoption transaction is requested.

> Early development: V0.1 is intentionally conservative. System LaunchDaemons
> are observe-only, shell execution is rejected, and native mutation requires
> `--apply`.

## Core model

```text
Command → Automation → Run → Event
```

Ownership is explicit:

- `observed`: the native scheduler remains authoritative;
- `adopted`: `auto` has transactionally disabled the native source;
- `managed`: the definition exists only in the local Registry.

## Quick start

```bash
cargo run -p auto -- scan
cargo run -p auto -- list
cargo run -p auto -- doctor ownership
cargo run -p auto -- doctor drift
cargo run -p auto -- doctor adoption
cargo run -p auto -- inspect <automation-id>
```

Register and run a direct-argv automation:

```bash
cargo run -p auto -- register examples/hello.yaml
cargo run -p auto -- explain hello
cargo run -p auto -- run hello
cargo run -p auto -- policy-check hello
cargo run -p auto -- pause hello
cargo run -p auto -- resume hello
cargo run -p auto -- cancel <run-id>
cargo run -p auto -- logs <run-id>
cargo run -p auto -- inbox --limit 100

# Review a detected source drift before updating its baseline
auto acknowledge-drift launchd:com.example.agent --dry-run
auto acknowledge-drift launchd:com.example.agent --apply

# Diagnose optional integrations without running work
auto integration codex-doctor --cwd /path/to/repo
auto integration gh-doctor --hostname github.com
```

Adoption is deliberately two-step. The first command is read-only; the second
is the only command that may alter the selected native user source:

```bash
cargo run -p auto -- adopt cron:line-3 --dry-run
cargo run -p auto -- adopt cron:line-3 --apply
```

Every applied adoption records a native snapshot and journal. Use the printed
transaction ID with `rollback` to restore the snapshot. Rollback also accepts
an interrupted or `needs_attention` transaction and converges its Registry
automation back to `observed`/`needs_attention`, so a restart cannot leave a
stale internal owner claiming authority.

Inspect the journal without changing native state:

```bash
auto adoptions --limit 100
auto adoption-inspect <transaction-id>
```

Linux `systemd --user` services use the same transaction boundary:

```bash
auto scan --source systemd
auto adopt systemd:user:my-service.service --dry-run
auto adopt systemd:user:my-service.service --apply
auto rollback <transaction-id>
```

Only explicitly enabled `.service` units are adoptable. Static, indirect, or
non-service units remain observation-only.

User launchd agents are also adoptable with the same fingerprint and rollback
guarantees:

```bash
auto scan --source launchd
auto adopt launchd:com.example.agent --dry-run
auto adopt launchd:com.example.agent --apply
auto rollback <transaction-id>
```

Adoption is limited to the current user's `~/Library/LaunchAgents`. System
LaunchDaemons and other system-level paths remain observation-only.
For user agents, `scan` also consults `launchctl print` so the Registry's
enabled/paused state reflects current loaded runtime rather than plist presence.

## V0.1 scope

Implemented: Rust workspace, SQLite Registry, launchd/cron/systemd discovery, managed
YAML definitions, argv-only command execution, time/misfire/concurrency helpers,
manual scheduler pass, adoption fingerprinting, rollback journal, CLI and a
text dashboard. V0.2 adds a policy-backed Codex `exec` adapter, JSONL event
parsing, persistent approval requests, deterministic argv verification, Git
worktree lifecycle helpers, the local JSON-RPC daemon, MCP Registry tools, and a
thin SwiftUI client. V0.3 now includes a Codex App Server stdio client with
Registry-backed dynamic approvals, systemd discovery/adoption, Homebrew service
observation, launchd runtime-state discovery, source drift reconciliation, and
an experimental typed privileged boundary.

Managed/adopted scheduled runs now apply the configured misfire policy, record
their intended `scheduled_at`, and check persisted running rows before admitting
a forbidden overlap.
The overlap decision is also enforced atomically when a Run is inserted, so
manual CLI, RPC, and concurrent daemon entry points cannot bypass the policy.
Skipped misfires emit a `scheduler.misfire_skipped` audit event instead of
silently disappearing.
Definitions may set `misfire_max_age_seconds` to prevent replaying an
occurrence that is too old; those occurrences emit `scheduler.misfire_expired`
and advance without creating a Run.
Cron schedules evaluate their declared `UTC`, `local`, or IANA timezone and
cover DST transitions explicitly.
Policies also support fail-closed `budget.max_steps` and bounded retry guards.
Retries default to one attempt, only retry failed or timed-out steps, record
each retry as an event, and use a capped exponential backoff when configured;
cancelled, approval-gated, or policy-rejected work is never retried. Unknown
provider token or dollar cost is never fabricated.

An installed privileged helper is not implemented. The boundary remains
fail-closed: defining a typed interface does not grant root access.

Homebrew Services are observation-only and are reconciled with matching launchd
plist paths so one service is not shown twice:

```bash
auto scan --source homebrew
auto scan --source all
```

Only unmatched Homebrew services receive a `homebrew:<formula>` source ID;
matched services retain the canonical launchd identity with Homebrew metadata.

## Codex and verification

Read-only Codex runs are allowed only inside a Git repository and use the
documented non-interactive surface:

```bash
auto codex-run --cwd /path/to/repo --prompt-file prompts/triage.md --sandbox read-only
```

Workspace-write runs require an explicit worktree and a persistent approval:

```bash
auto codex-run --cwd /path/to/repo --prompt "make the smallest fix" \\
  --sandbox workspace-write --worktree-dir /tmp/auto-fix
# approve the printed request, then rerun with --approval-id <id>
auto approve <id>
auto codex-run --cwd /path/to/repo --prompt "make the smallest fix" \\
  --sandbox workspace-write --worktree-dir /tmp/auto-fix --approval-id <id>
```

Codex JSONL is parsed strictly; malformed events or a failed turn cannot be
reported as success. Verify the result independently with argv-only commands:

```bash
auto verify --cwd /tmp/auto-fix --executable cargo --arg test --arg --workspace
```

Codex usage is stored as local token metrics when the provider reports it:

```bash
auto metrics
```

Dollar cost is intentionally absent unless a separately maintained pricing
registry is configured; unknown subscription/provider cost is never fabricated.

## Codex App Server (experimental)

The App Server adapter uses the local `codex app-server --listen stdio://`
transport and requires the working directory to be a Git repository. The safe
default is read-only with automatic decline. Workspace-write requires the
explicit interactive approval bridge:

```bash
auto codex-app-server --cwd /path/to/repo --prompt "summarize the current changes"
auto codex-app-server --cwd /path/to/repo --sandbox workspace-write \
  --interactive-approvals --prompt "make the smallest fix"
```

When a dynamic request arrives, the command prints an approval ID on stderr and
waits. Resolve it from another terminal with `auto approve <id>` or
`auto reject <id>`. Requests above the sandbox risk ceiling are persisted as
rejected and never become approvable.

The typed `PrivilegedHelper` boundary currently has only a fail-closed
`NoPrivilegedHelper` implementation. It accepts known system-job identities and
explicit operations such as `query_system_job`, `enable_system_job`,
`disable_system_job`, and `read_known_plist`; it deliberately has no generic
command execution method.

## OpenAI Responses-compatible API

`auto responses-run` provides a narrow, read-only cloud AI executor for
classification, summarization, and decision support. It accepts any
OpenAI Responses-compatible base URL, never persists the API key, and sends
`store: false` unless `--store` is explicitly supplied.

The key is selected by environment-variable name rather than placed in a
definition or command argument:

```bash
export OPENCODE_API_KEY='<key supplied through a local secret mechanism>'
auto responses-run \
  --base-url https://opencode.ai/zen/go/v1 \
  --model deepseek-v4-flash \
  --api-key-env OPENCODE_API_KEY \
  --prompt 'Summarize this local status in one sentence.'
```

For OpenAI Platform, omit the provider-specific flags and provide
`OPENAI_API_KEY`; the default base URL is `https://api.openai.com/v1` and the
default model is `gpt-5`. Usage reported by the provider is recorded in the
local Metrics read model. Response reasoning items are never printed as
assistant output, and captured output is bounded at 1 MiB.

The same executor can be used inside a managed YAML automation. A Responses
step has no command and is validated by the same risk, budget, retry, event,
and cancellation paths as command steps:

```yaml
steps:
  - id: classify
    responses:
      prompt: "Classify the supplied state."
      base_url: https://opencode.ai/zen/go/v1
      model: deepseek-v4-flash
      api_key_env: OPENCODE_API_KEY
      store: false
```

## Local JSON-RPC daemon

Run the scheduler and Unix-socket control plane together:

```bash
auto daemon --socket "$HOME/.local/share/auto/automationd.sock"
```

On startup, orphaned `running` records from a prior daemon instance are marked
`interrupted` and audited before misfire scheduling resumes.

The socket is created with mode `0600` in a directory with mode `0700`. The
versioned methods currently include `daemon.ping`, `automation.list`,
`automation.inspect`, `automation.policy_check`, `automation.pause`, `automation.resume`,
`source.acknowledge_drift`, `adoptions.list`, `adoption.inspect`, `automation.run`, `run.cancel`, `run.logs`, `approvals.list`,
`approval.approve`, `approval.reject`, `events.list`, `runs.list`, and
`metrics.list`, and `inbox.list`. `runs.list` returns bounded recent runs with their immutable
automation revision snapshots and can filter by automation ID.

`auto inbox` and `inbox.list` are read-only aggregations of pending approvals,
automations needing attention, interrupted adoption transactions, and failed
or timed-out Runs. They never approve, resume, rollback, or retry anything.
All requests reopen the SQLite Registry for short synchronous database scopes;
subprocess awaits never hold a SQLite connection across worker threads.

## MCP control plane

Expose the same Registry to MCP clients through stdio:

```bash
auto mcp serve
```

The adapter supports current stateless `server/discover`, `tools/list`, and
`tools/call`, plus legacy `initialize` compatibility. Its tools are fixed and
deterministically ordered: list/inspect registered automations, run a read-only
policy preflight, read bounded runs/events, run or cancel a named local run,
read its bounded logs, and read approvals/metrics. There is no shell or
arbitrary-command tool.

## macOS desktop client

The SwiftUI client is intentionally thin and talks only to the Unix JSON-RPC
daemon:

```bash
auto daemon --socket "$HOME/.local/share/auto/automationd.sock"
swift run --package-path macos/DesktopApp
```

It displays automations, runs, a read-only Inbox, run logs, approvals, metrics and audit events,
can request or cancel a named run, and can approve or reject pending approval
requests through the explicit RPC methods.
Scheduler, policy, Registry and executor logic remain in `automationd`.

`auto tui` opens an interactive Ratatui dashboard on a terminal (`r` refreshes,
`q` exits) and falls back to a one-shot text dashboard when output is not a TTY.

`auto worktree remove` refuses dirty worktrees unless `--force` is explicit.

Read-only GitHub snapshots use `gh`'s structured fields and never accept a
free-form shell command. A watcher can persist a normalized fingerprint and
emit an event only when the snapshot changes:

```bash
auto github-watch --repo owner/repo --query pulls
auto github-watch --repo owner/repo --query pulls --interval-seconds 300
auto github-watch --repo owner/repo --query failed-runs
auto github-watch --repo owner/repo --query checks --pull-number 123
```

Watcher snapshots are stored locally by `(repo, query, pull number)`; array
ordering is normalized before hashing, so polling the same state does not flood
the event log.

Issue, PR, log and web content returned by GitHub remains data, not agent
instructions; a future Codex prompt adapter must preserve that boundary.

## Safety principles

1. Observe before owning.
2. Exactly one scheduler may own an adopted source.
3. Shell strings are not implicit commands.
4. Shell-invoking discovered sources are not promoted or adopted automatically.
5. Native definitions are fingerprinted and snapshotted before mutation.
6. Adopted/managed ownership is never overwritten by a later observation scan;
   drift is recorded and moves the automation to `needs_attention`.
7. Destructive or external-write risks require approval; unresolved approvals
   fail closed and remain auditable in the local Registry.
8. Deterministic evidence beats an executor's claim that it succeeded.

See [SECURITY.md](SECURITY.md) and [CONTRIBUTING.md](CONTRIBUTING.md).

## License

Licensed under the Apache License 2.0; see [LICENSE](LICENSE).
