# Taskrail

Taskrail is a local automation manager for developers: one place to organize,
run, and monitor the scripts, CLI tools, and AI jobs your computer runs
automatically.

It answers four simple questions:

- What automations do I have?
- What is running now?
- What failed?
- What will run next?

Taskrail is deliberately small at its center. A local daemon owns the Registry
and scheduler; the CLI and TUI let you inspect and operate it. Optional tools
such as Codex and GitHub are integrations, not the product itself.

```text
                 CLI / TUI / local RPC
                         │
                         ▼
                 Taskrail local daemon
                 Registry + scheduler
                         │
          ┌──────────────┼──────────────┐
          ▼              ▼              ▼
       Commands       launchd/cron     Codex/GitHub
       and scripts    discovery       integrations
                         │
                         ▼
                  runs · logs · events
```

## Quick start

Install the binary from a checkout:

```bash
cargo install --path crates/taskrail
```

Add a command without writing a configuration file:

```bash
taskrail add hello /bin/echo --arg "hello from Taskrail"
taskrail list
taskrail run hello
taskrail runs
taskrail logs <run-id>
```

Add a recurring task:

```bash
taskrail add mole-cleanup mo --arg clean \
  --every-seconds 604800 --name "Mole cleanup"
```

On macOS, install the user LaunchAgent so the scheduler stays running:

```bash
taskrail daemon --install
taskrail status
```

Open the live terminal dashboard:

```bash
taskrail tui
```

For definitions that need more fields, use YAML:

```bash
taskrail register examples/hello.yaml
taskrail explain hello
taskrail run hello
```

The command executor uses direct argv. It does not turn a string into a shell
command.

## What it manages

Taskrail can manage commands and scripts you already use:

- one-shot commands and recurring interval or cron jobs;
- local run history, stdout, stderr, and operational events;
- launchd, cron, systemd user services, and Homebrew service discovery;
- explicit adoption of supported user-native jobs, with rollback records;
- optional Codex and Responses-compatible AI executions;
- read-only GitHub issue, pull request, check, and failed-run observations.

Native jobs are observed before Taskrail is asked to adopt them. Discovery does
not change the machine. Adoption is currently limited to supported user-level
sources and always requires an explicit command.

## The TUI is the main view

The TUI is designed for a small, always-available local tool rather than a
browser dashboard. It shows each automation's name, ownership, runtime state,
next run, and attention items. Runs, logs, events, and metrics remain available
from the CLI.

```text
NAME              OWNERSHIP   STATE       NEXT RUN
Mole cleanup      managed     enabled     2026-08-18T...
GitHub watch      observed    paused      manual

Needs attention
failed run        run_failure  high
```

## AI is an executor, not the product

Simple work should remain a command:

```text
every Sunday → mo clean
```

An AI executor is useful when the task needs interpretation:

```text
every two hours → inspect GitHub state → summarize what needs attention
```

The current repository includes optional Codex CLI and Responses-compatible
executors. A native ChatGPT desktop connection is not implemented yet; it is a
future interface to the same local manager, not a replacement for it.

## Local-first behavior

- The Registry is local SQLite.
- Runs, logs, and events are recorded locally.
- Commands are executed as argv; arbitrary shell strings are not accepted.
- Environment values are redacted in persisted automation snapshots.
- Existing native jobs remain observation-only until explicit adoption.
- There is no MCP server, Codex App Server adapter, approval inbox, or generic
  policy engine in the current product surface.

## Current status

The current package is `0.1.x` and is early but usable for local command
automation. The stable center is:

```text
add/register → list → daemon → run → history/logs → tui
```

The following are optional integrations or still future work:

| Area | Status |
| --- | --- |
| Registry, scheduler, runs, logs, events | Core |
| CLI and TUI | Core |
| launchd / cron / systemd / Homebrew discovery | Integration |
| User-level native adoption | Integration |
| Codex CLI and Responses executor | Optional integration |
| GitHub read-only watcher | Optional integration |
| ChatGPT natural-language management | Future |
| Packaged releases and Homebrew formula | Future |

## Documentation

- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [Architecture decisions](docs/adr/)
- [Research notes](deep-research-report.md)
- [Example automation](examples/hello.yaml)

The ADRs preserve historical decisions. They are not required to use the core
CLI and TUI.

## Contributing

Start with the core user journey and keep integrations at the edge. Before
opening a change, run:

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace
```

## License

Apache-2.0. See [LICENSE](LICENSE).
