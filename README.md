<div align="center">
  <img src="docs/assets/taskrail-topology.svg" alt="Taskrail connects VibeCleaner, Mole, Homebrew, restic, rclone, local jobs, and ChatGPT to one automation control plane for scheduling, safe execution, and audit history" width="960" />

  <p>
    <a href="https://github.com/Yuxin-Qiao/Taskrail/actions/workflows/ci.yml"><img src="https://img.shields.io/github/actions/workflow/status/Yuxin-Qiao/Taskrail/ci.yml?branch=main&style=flat-square&label=CI&logo=githubactions&logoColor=white" alt="CI status" /></a>
    <a href="https://github.com/Yuxin-Qiao/Taskrail/releases/latest"><img src="https://img.shields.io/github/v/release/Yuxin-Qiao/Taskrail?style=flat-square&color=2563eb" alt="Latest release" /></a>
    <a href="https://www.rust-lang.org/"><img src="https://img.shields.io/badge/Rust-1.88%2B-dea584?style=flat-square&logo=rust&logoColor=white" alt="Built with Rust 1.88+" /></a>
    <a href="LICENSE"><img src="https://img.shields.io/badge/license-Apache--2.0-f97316?style=flat-square" alt="Apache 2.0 license" /></a>
  </p>

  <p>
    <a href="#supported-runtime-targets"><img src="https://img.shields.io/badge/macOS-Apple%20Silicon%20(aarch64)-000000?style=flat-square&logo=apple&logoColor=white" alt="Apple Silicon macOS" /></a>
    <a href="#supported-runtime-targets"><img src="https://img.shields.io/badge/Linux-ARM64%20(glibc)-FCC624?style=flat-square&logo=linux&logoColor=black" alt="ARM64 Linux" /></a>
    <a href="#supported-runtime-targets"><img src="https://img.shields.io/badge/arch-aarch64-0091BD?style=flat-square&logo=arm&logoColor=white" alt="aarch64 architecture" /></a>
    <a href="#supported-runtime-targets"><img src="https://img.shields.io/badge/Windows%20%2F%20x86__64-unsupported-71717a?style=flat-square&logo=windows&logoColor=white" alt="Windows / x86_64 unsupported" /></a>
  </p>
  <p><sub>Core CLI/TUI needs no Node.js, Python, standalone SQLite, or OpenSSL. VibeCleaner, Homebrew, Mole, restic, rclone, <code>gh</code>, scanners, Codex, and ChatGPT Tunnel are optional integrations.</sub></p>

  <p><a href="#supported-platforms-and-prerequisites">Platform &amp; install</a> · <a href="docs/chatgpt.md">ChatGPT integration</a> · <a href="README.zh-CN.md">简体中文</a></p>

  <p><sub>Works with</sub> · <a href="https://vibecleaner.app/">VibeCleaner</a> · <a href="https://github.com/tw93/Mole">Mole</a> · <a href="https://github.com/Homebrew/brew">Homebrew</a> · <a href="https://github.com/restic/restic">restic</a> · <a href="https://github.com/rclone/rclone">rclone</a></p>
</div>

<p align="center">
  <sub>discover</sub> &nbsp;→&nbsp; <sub>schedule</sub> &nbsp;→&nbsp; <sub>execute</sub> &nbsp;→&nbsp; <sub>inspect</sub>
</p>

## Supported platforms and prerequisites

Taskrail is a local executable. There is no Taskrail server, hosted account,
database service, Node.js runtime, Python runtime, or separate SQLite/OpenSSL
installation required for the core CLI.

### Supported runtime targets

Official binaries, CI, and release verification cover only these targets:

- <img src="https://img.shields.io/badge/macOS-Apple%20Silicon%20(M1--M4)-000000?style=flat-square&logo=apple&logoColor=white" alt="macOS Apple Silicon" /> `aarch64-apple-darwin` — **Supported** (LaunchAgent daemon supervision)
- <img src="https://img.shields.io/badge/Linux-ARM64%20(glibc)-FCC624?style=flat-square&logo=linux&logoColor=black" alt="ARM64 Linux" /> `aarch64-unknown-linux-gnu` — **Supported** (`systemd --user` supervision)
- <img src="https://img.shields.io/badge/Windows%20%2F%20x86__64-unsupported-71717a?style=flat-square&logo=windows&logoColor=white" alt="Windows / x86_64 unsupported" /> `x86_64`, Windows, 32-bit ARM, Linux `musl`/Alpine — **Unsupported** (rejected at compile time)

Intel/AMD `x86_64`, Windows, 32-bit ARM, Linux `musl`/Alpine, and other
architectures or operating systems are not supported release targets. The Rust
crate intentionally fails at compile time for an unsupported target instead of
producing an untested binary. There is no native desktop app bundle: the local
UI is the CLI, TUI, and daemon-hosted loopback browser dashboard.

### Choose an installation path

#### Option A: Download a release (no Rust required)

Download the archive matching the host from the
[GitHub Releases](https://github.com/Yuxin-Qiao/Taskrail/releases) page:

- macOS Apple Silicon: `taskrail-<version>-aarch64-apple-darwin.tar.gz`;
- ARM64 Linux: `taskrail-<version>-aarch64-unknown-linux-gnu.tar.gz`.

Verify the matching `.sha256` file, extract the archive, and put the binary on
your `PATH` (replace `<target>` with the full Rust target shown above):

```bash
tar -xzf taskrail-<version>-<target>.tar.gz
mkdir -p "$HOME/.local/bin"
install -m 0755 taskrail "$HOME/.local/bin/taskrail"
export PATH="$HOME/.local/bin:$PATH"
taskrail --version
```

Persist the `PATH` change in your shell profile if `~/.local/bin` is not
already on it.

#### Option B: Build from the checkout

This path requires [Rustup](https://rustup.rs/), Cargo, Rust `1.88.0` or newer,
and a native ARM64 C compiler/linker for the bundled native dependencies. On
macOS, install Apple's Command Line Tools if needed; on Debian/Ubuntu ARM64,
install the distribution build tools:

```bash
# macOS, only if the Command Line Tools are not installed
xcode-select --install

# Debian/Ubuntu ARM64
sudo apt-get update
sudo apt-get install build-essential

rustup toolchain install 1.88.0
cargo +1.88.0 install --locked --path crates/taskrail
taskrail --version
```

You do not need to install Node.js, Python, a standalone SQLite server, or
OpenSSL to build the current crate. The repository pins Rust `1.88.0` in
`rust-toolchain.toml`; the same command can be run without the `+1.88.0`
override when Rustup is already selecting that toolchain.

### Platform-specific services and UI

| Feature | macOS Apple Silicon | ARM64 Linux |
| --- | --- | --- |
| Core CLI, TUI, foreground daemon, local Registry | No extra package | No extra package; use a glibc-based distribution |
| `taskrail daemon --install` | Installs a per-user LaunchAgent using the built-in `launchctl` | Installs a systemd user unit; `systemctl --user` must be available |
| Headless background service | LaunchAgent works in the logged-in user session | Run `loginctl enable-linger "$USER"` before installation when the service must survive logout |
| `taskrail gui` | Uses the built-in `open` command | Uses `xdg-open`; install the distribution's `xdg-utils` package or open the printed loopback URL manually |
| Browser dashboard | Any modern browser on the same host | Any modern browser on the same host; the dashboard remains loopback-only |

The browser and `taskrail gui` are optional: the CLI and `taskrail tui` work
without a graphical desktop. Linux containers and minimal distributions can
run the foreground CLI, but `daemon --install` needs a systemd user manager;
the release target is still GNU libc ARM64, not Alpine/musl.

### Optional integrations: install only what you use

The core commands (`add`, `register`, `list`, `run`, `daemon`, `tui`, the local
dashboard, and local MCP) do not require the tools in this table. Taskrail does
not install them for you. A missing tool makes only its integration unavailable;
check the result with `taskrail integrations` or the integration's `doctor`
command.

| Capability | External command or setup | Platform and notes |
| --- | --- | --- |
| Mole cleanup/analyze/status | `mo` (Mole) | macOS only; install Mole separately |
| VibeCleaner developer-cache scan | `vibecleaner` headless CLI or compatible wrapper | Read-only scan; the public GUI DMG is not driven by Taskrail |
| Homebrew inventory/services | `brew` (Homebrew) | macOS or Linux; optional |
| Backup and repository checks | `restic` | macOS or Linux; configure repository/password environment references for repository actions |
| Copy and sync | `rclone` | macOS or Linux; configure remotes separately |
| GitHub observations | `gh` (GitHub CLI) | macOS or Linux; authenticate `gh` when the target data requires it |
| Mac App Store inventory | `mas` | macOS only; optional |
| Apple Shortcuts | `shortcuts` | Included with macOS; no separate package, but running a Shortcut is approval-gated |
| Automator, Keyboard Maestro, Raycast, Alfred, Hazel discovery | Corresponding macOS app | macOS only; app-owned definitions are observed, not imported as arbitrary commands |
| Security scans | `osv-scanner`, `gitleaks`, `trivy` | macOS or Linux; install each scanner you plan to call |
| System update planning | `topgrade` | macOS or Linux; execution is approval-gated |
| Codex executor | `codex` CLI | Optional; needed only for `taskrail codex-run` |
| Responses executor | Network access and an API key such as `OPENAI_API_KEY` | Optional; no extra CLI is required |
| ChatGPT MCP/Tunnel connection | `tunnel-client`, an OpenAI Secure MCP Tunnel, and its local credentials | Optional; see [ChatGPT integration](docs/chatgpt.md) |

For example, installing Taskrail alone is enough for this first run:

```bash
taskrail add hello /bin/echo --arg "hello from Taskrail"
taskrail run hello
```

The optional container deployment is a separate path. The files under
[`deploy/`](deploy/) require an ARM64 Docker host and Docker Compose; Docker is
not required for local CLI/TUI use or for the Rust test suite. The sample is a
single-host, public-read-only MCP deployment and still needs an HTTPS/auth
edge; it is not a general hosted service.

## Quick start

If you are installing from a checkout, use the source-build command above:

```bash
cargo +1.88.0 install --locked --path crates/taskrail
```

Add a command without writing a configuration file:

```bash
taskrail add hello /bin/echo --arg "hello from Taskrail"
taskrail list
taskrail run hello
taskrail runs
taskrail logs <run-id>
# Delete only a managed definition with no recorded run history
taskrail delete hello
```

The short path is:

```text
add → run → inspect
```

Add a recurring task:

```bash
taskrail add weekly-hello /bin/echo --arg "weekly Taskrail run" \
  --every-seconds 604800 --name "Weekly hello"
```

On macOS, if Mole is installed separately, the same pattern can call
`mo clean`; see the optional integrations table below.

Keep the scheduler running with the per-user service for your platform:

```bash
taskrail daemon --install
taskrail status
```

On macOS this installs a LaunchAgent. On Linux this installs a systemd user
unit under `~/.config/systemd/user/`. The Registry is stored under
`$XDG_DATA_HOME/taskrail/` (or `~/.local/share/taskrail/`) on Linux; the Unix
daemon socket uses `$XDG_RUNTIME_DIR/taskrail/` when available. For a headless Linux host,
enable user lingering before installing:

```bash
loginctl enable-linger "$USER"
taskrail daemon --install
```

The daemon performs a read-only local-source inventory refresh every five
minutes by default. Use `--discovery-interval-seconds` to adjust it. Status and
overview report the last scan, provider completeness, drift, and confirmed
missing-source counts; an unavailable provider is not treated as an empty
provider, so Taskrail does not manufacture deletion alerts.

## ChatGPT Scheduled tasks

Taskrail can be connected to ChatGPT as an MCP app with typed tools and optional
read-only MCP Apps widgets. The connected ChatGPT client has been verified to
call the app interactively; a future Scheduled trigger still needs to be
observed in the target account. ChatGPT Web, Desktop, and Mobile use the same
MCP tool contract; ChatGPT's Scheduled page remains the natural-language
scheduler and notification surface; Taskrail is the local execution backend
that ChatGPT calls on the selected ARM64 macOS or Linux host.

Start the local MCP adapter after the Taskrail daemon is running:

```bash
taskrail daemon --install       # LaunchAgent/systemd by platform
taskrail mcp                    # MCP stdio adapter for the current host
taskrail integration chatgpt-doctor
```

The adapter exposes status, fresh native discovery, automation creation, pause and
resume, immediate runs, run history, logs, cancellation, attention items, and
audit events. The daemon also keeps a background read-only observation mirror;
status carries its safe supervision summary while overview still performs a
fresh scan. Commands remain direct argv; ChatGPT cannot turn a free-form string
into a shell pipeline through this interface.

For one private ARM64 macOS or Linux host, connect `taskrail mcp` through
OpenAI Secure MCP Tunnel, then add the tunnel as a ChatGPT developer-mode app.
For several hosts, use `taskrail mcp-fleet` with the local `examples/fleet.yaml`
shape and connect that one gateway. Copy the example outside the repository,
replace its endpoints and token environment-variable names, and enable only
the hosts you trust:

```bash
mkdir -p ~/.config/taskrail
cp examples/fleet.yaml ~/.config/taskrail/fleet.yaml
taskrail mcp-fleet --config ~/.config/taskrail/fleet.yaml
```

The checked-in example keeps its hosts disabled and uses placeholder endpoints;
it never makes an outbound request until you edit the local copy. Once the app
is connected, a supported account can use a Scheduled task prompt such as the
following; observe the first run before treating the workflow as verified:

```text
Every Sunday at 09:00, run the Taskrail automation named "Mole cleanup" on this host.
If it fails, inspect the run logs and tell me what needs attention.
```

See [ChatGPT integration](docs/chatgpt.md) for the tunnel, permissions, and
multi-host setup details. With the fleet gateway, always name the target
`host_id` in a request; do not rely on a display label alone.

For a public deployment, use the enforced read-only HTTP profile:

```bash
export TASKRAIL_MCP_BEARER_TOKEN="<inject from a secret manager>"
taskrail mcp-http --profile public-read-only --bind 127.0.0.1:8787
```

This endpoint defaults to the public read-only profile and omits creation,
deletion, execution, adoption, and approval tools. Put it behind a production
HTTPS proxy with end-user authentication and per-user host binding; a local
tunnel is only a developer connection. See the [OpenAI submission checklist](docs/OPENAI_SUBMISSION.md)
and the [single-host deployment example](deploy/README.md).

For a private, single-host Fleet target that needs explicit write or run
requests, use `taskrail mcp-http --profile private` with a separate bearer
secret and private TLS/authentication edge. Never expose that profile as a
shared public relay; Fleet `allow_writes: true` is intended only for this
explicitly protected endpoint.

For a local stdio review of the public read-only profile, use:

```bash
TASKRAIL_MCP_PROFILE=public taskrail mcp
```

This profile omits creation, deletion, execution, adoption, and approval
tools.

Open the live terminal dashboard:

```bash
taskrail tui
```

The daemon is also the local web server for the primary browser dashboard. It
listens on `127.0.0.1:10100` by default; open it with:

```bash
taskrail gui
```

The dashboard shows discovery, automations, runs, logs, integrations, inbox,
approvals, metrics, and audit events. Its write actions call the same local RPC
and policy boundary as the CLI/TUI. It is loopback-only, requires a same-origin
browser request for writes, and is never exposed through the ChatGPT MCP or
Tunnel endpoint. Use `taskrail daemon --http-bind 127.0.0.1:10100` to choose a
different local port. If another local service owns `10100`, Taskrail falls
back to the next loopback ports and `taskrail gui` discovers the active
Taskrail endpoint instead of opening the other service.

The browser dashboard supports English, Simplified Chinese, Japanese, and
Korean. It selects a supported browser language on first load; use the language
selector in the top-right corner to change it. The selection is stored only in
the browser's local storage.

The ChatGPT MCP app can render the same bounded overview through a versioned
MCP Apps resource. The optional Fleet gateway also exposes a read-only
multi-host view; both widgets call typed MCP tools and never the local browser
HTTP endpoint.

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
- launchd, cron, systemd user services/timers, Homebrew services, and supported
  macOS application automation discovery (Shortcuts, Automator, Keyboard
  Maestro, Raycast, Alfred, and Hazel); application-owned definitions remain
  observe-only during discovery, while Shortcuts has a separate typed,
  approval-gated run path;
- explicit adoption of supported user-native jobs, with rollback records;
- deletion of unused managed definitions without deleting immutable run history;
- optional Codex and Responses-compatible AI executions;
- typed semantic integrations for VibeCleaner (read-only developer-cache scan), Mole, restic, rclone, GitHub, Homebrew, mas,
  OSV-Scanner, Gitleaks, Trivy, Topgrade, and typed Apple Shortcuts runs;
- durable, typed integration Automations for read-only and dry-run schedules;
- normalized findings, metrics, changes, artifacts, run history, and inbox
  attention items from those integrations.

Native jobs are observed before Taskrail is asked to adopt them. Discovery does
not change the machine. Adoption is currently limited to supported user-level
sources and always requires an explicit command.

## Native integrations

Taskrail exposes one typed semantic layer for native tools. For example:

```bash
taskrail integration mole detect
taskrail integration mole doctor
taskrail integration mole analyze
taskrail integration mole status
taskrail integration mole history --limit 20
taskrail integration mole clean --dry-run
taskrail integration vibecleaner detect
taskrail integration vibecleaner doctor
taskrail integration vibecleaner scan "$HOME/Projects" --min-size-mb 500
taskrail integration restic snapshots
taskrail integration rclone sync ./data remote:backup --dry-run
taskrail integration github pulls Yuxin-Qiao/Taskrail
taskrail integration homebrew outdated
taskrail integration gitleaks scan .
taskrail integration topgrade plan
taskrail integration shortcuts doctor

# Persist a read-only native integration as a recurring Automation
taskrail schedule-integration homebrew-outdated homebrew outdated \
  --every-seconds 86400 --name "Daily Homebrew inventory"
```

These actions use typed argv plans, bounded parsing, normalized semantic
results, Run/Event/Metric records, and adapter verification. Writes and
destructive actions are bound to a persisted, expiring approval request:

VibeCleaner is intentionally scan-only here. Its public app is a local GUI;
Taskrail does not attempt to click the app or automate deletion. When a
headless `vibecleaner` wrapper (or the documented Python CLI source) is present,
the adapter invokes `--cli ... --json`, preserves the upstream `safe`/`verify`
risk distinction, and records reclaimable bytes without touching the scanned
directories. Set `TASKRAIL_VIBECLEANER_SCRIPT` to the Python source path (and
optionally `TASKRAIL_VIBECLEANER_PYTHON` to choose the interpreter) when using
the documented source CLI; otherwise the adapter looks for a `vibecleaner`
wrapper on `PATH`.

```bash
taskrail approval-request restic-prune
taskrail approval-request shortcuts-run <shortcut-uuid> --confirm
taskrail approvals
taskrail approval-decide <approval-id> --approve
taskrail approval-execute <approval-id>
```

The exact typed approval request subcommands are shown by
`taskrail approval-request --help`. A granted request is one-time and matched
to the exact typed plan fingerprint. Without that approval, the existing
policy boundary records the request and does not spawn a process. Use
`taskrail integrations` to inspect the complete built-in adapter catalog.

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
executors. ChatGPT is the natural-language control surface: a verified
interactive app call can use the Taskrail MCP adapter, while Taskrail owns local
discovery, typed Automation definitions, execution, approvals, history, and
logs. ChatGPT's Scheduled task and Taskrail's local schedule are intentionally
separate layers; the former is an external trigger that still needs an observed
run, and the latter runs a persisted local Automation.

For Codex installations with a model catalog generated by another tool,
Taskrail automatically uses a short-lived 0600 compatibility copy when the
known unsupported `audio` modality is present. The global Codex configuration
is not changed. An explicit catalog override is also available:

```bash
taskrail codex-run --cwd . --model-catalog-json /path/to/catalog.json \
  --prompt "inspect the repository"
```

## Local-first behavior

- The Registry is local SQLite.
- Runs, logs, and events are recorded locally.
- Commands are executed as argv; arbitrary shell strings are not accepted.
- Environment values are redacted in persisted automation snapshots.
- Existing native jobs remain observation-only until explicit adoption.
- The daemon refreshes native observations in the background and marks drift or
  confirmed missing sources as attention items; it never deletes a source from
  the Registry because a provider was unavailable.
- The ChatGPT MCP adapter reaches the daemon through the restricted local Unix
  socket; it does not expose the Registry directly.
- Approval requests are persisted locally, expire, are plan-bound, and are
  consumed once. They never contain secret values.

## Current status

The current package is `0.1.7` and is usable for local command automation and
interactive private ChatGPT app control; future Scheduled triggering remains
an unverified account-level workflow gate. The stable center is:

```text
add/register → list → daemon → run → history/logs → tui
```

The current implementation and remaining release gates are:

| Area | Status |
| --- | --- |
| Registry, scheduler, runs, logs, events | 🟢 Core |
| CLI and TUI | 🟢 Core |
| launchd / cron / systemd / Homebrew plus supported macOS app discovery and background supervision | 🔵 Integration; Shortcuts has typed, approval-gated run |
| User-level native adoption | 🔵 Integration (cron/launchd/systemd) |
| Codex CLI and Responses executor | 🟣 Optional integration |
| Native semantic integrations | 🟢 VibeCleaner (scan) / Mole / restic / rclone / GitHub / Homebrew / mas / security scanners / Topgrade / Shortcuts |
| Private ChatGPT MCP/Tunnel and interactive read-only ChatGPT app call | 🟢 Verified; future Scheduled trigger not yet observed |
| Read-only ChatGPT MCP Apps views for local and Fleet overviews | 🟢 Implemented (private MCP) |
| Multi-host fleet gateway with explicit host routing | 🟢 Implemented (private configuration) |
| Public ChatGPT App hosting, review, and publication | 🟡 External gate |
| ARM64 CLI releases | 🟢 [v0.1.7 published](https://github.com/Yuxin-Qiao/Taskrail/releases/tag/v0.1.7) |
| Homebrew formula | 🟡 Future |

## Documentation

- [简体中文 README](README.zh-CN.md)
- [中文文档索引](docs/README.zh-CN.md)
- [Contributing](CONTRIBUTING.md)
- [Security](SECURITY.md)
- [ChatGPT integration](docs/chatgpt.md)
- [中文 ChatGPT 集成指南](docs/chatgpt.zh-CN.md)
- [Acceptance checklist](docs/ACCEPTANCE.md)
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
cargo +1.88.0 clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo +1.88.0 test --locked --workspace --all-features
```

## License

Apache-2.0. See [LICENSE](LICENSE).
