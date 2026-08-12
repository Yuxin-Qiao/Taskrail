# Taskrail Acceptance Checklist

This checklist is the release-gate for the current Taskrail workspace. It is
intentionally evidence-based: a feature is accepted only when the stated
command, test, or runtime observation succeeds.

Execution date: 2026-08-12

## Scope and safety

- Tests that create or run automations use a temporary Registry.
- Native discovery is read-only; it must not modify launchd, cron, systemd, or
  Homebrew definitions.
- No real user automation is created, paused, resumed, adopted, or deleted by
  this checklist.
- Secrets must not appear in source, logs, test output, or this document.

## Acceptance matrix

| ID | Area | Acceptance criterion | Evidence | Status |
| --- | --- | --- | --- | --- |
| A1 | Metadata | Rust workspace, package metadata, license, README, and OSS files are present | `Cargo.toml`, `crates/taskrail/Cargo.toml`, `LICENSE`, `README.md`, `CONTRIBUTING.md`, `SECURITY.md` | PASS |
| A2 | Repository hygiene | Formatting diff is clean and generated/runtime artifacts are ignored | `git diff --check`, `.gitignore`, tracked-artifact scan | PASS |
| A3 | Secret safety | No obvious API key, token, private key, or credential marker is tracked or present in the project | repository secret scan | PASS |
| B1 | Rust quality | Workspace formatting, Clippy, tests, doc-tests, and build pass | `cargo fmt --check`; `cargo clippy --workspace --all-targets --all-features -- -D warnings`; `cargo test --workspace --all-features`; 147 tests passed | PASS |
| B2 | Swift client | Desktop client builds and model-decoding tests pass | `swift build`, `swift test`; 2 tests passed | PASS |
| B3 | Linux build | Linux target produces an ELF binary without macOS-only warnings/errors | `cargo +1.88.0 zigbuild --locked --workspace --target x86_64-unknown-linux-gnu`; Linux workspace cross-build passed | PASS |
| C1 | CLI lifecycle | Add/register, list, inspect, delete, explain, run, runs, logs, pause, resume, inbox, metrics, events, doctor, and verify work | temporary-Registry CLI smoke | PASS |
| C2 | Scheduler | Interval scheduling runs repeatedly; cron/misfire/overlap behavior is covered | 3 interval runs succeeded; scheduler tests passed | PASS |
| C3 | Native discovery | launchd, cron, systemd, Windows Task Scheduler, and Homebrew discovery paths execute without native mutation | macOS/Linux scans and tests passed; Windows Task Scheduler parser and live `scan --source task-scheduler --json` smoke passed on the Windows CI runner | PASS |
| C4 | Adoption safety | Dry-run, transaction journal, verification failure, rollback, and shell boundary are fail-closed | adoption tests; shell creation now rejected before Registry write | PASS |
| C5 | Daemon/RPC | macOS/Linux Unix socket and Windows named-pipe daemons expose lifecycle/log/run APIs with local-only boundaries | Unix temporary daemon/MCP smoke passed; Windows named-pipe `daemon.ping` round-trip passed on the Windows CI runner | PASS |
| D1 | MCP contract | MCP initializes, advertises valid schemas/annotations, handles invalid requests, and exposes overview, discovery, native integrations, typed scheduling, adoption, drift, deletion, and approval tools | 38 local tools; 18 public read-only tools; MCP tests and negative paths passed | PASS |
| D2 | Local automation discovery | A fresh MCP discovery call returns local native tasks as safe summaries and reports no native definition mutation | live call returned 26 sources, `native_definitions_changed=false` | PASS |
| D3 | ChatGPT connection | Tunnel runtime and ChatGPT integration doctor are ready; ChatGPT can call Taskrail | doctor ready; ChatGPT session called Taskrail | PASS |
| D4 | Scheduled workflow | ChatGPT Scheduled task can call the connected Taskrail app and report a completed read-only result | Scheduled task history/detail showed completed Taskrail status call | PASS |
| E1 | Codex | Codex doctor and real `codex-run` succeed without exposing credentials; incompatible local catalog is handled ephemerally | `ACCEPT_CODEX_OK`; doctor ready | PASS |
| E2 | Responses API | Fake Responses-compatible success/error paths work and redact API key output | responses tests and fake-server smoke | PASS |
| E3 | GitHub watcher | Read-only pulls/issues/checks/failed-runs snapshots work and deduplicate unchanged snapshots | pulls 2, issues 0, failed runs 7, checks 8; watcher tests passed | PASS |
| F1 | Desktop runtime | Installed daemon is running, MCP runtime is healthy/ready, and local socket is user-only | doctor ready; Tunnel ready; socket mode 0600 | PASS |
| F2 | CI definition | GitHub Actions YAML parses and contains pinned Ubuntu/macOS/Windows Rust, MSRV, crate packaging, macOS Swift, dependency review, and supply-chain audit jobs | YAML inspected; Linux job includes XDG/default-path and native-scan smoke | PASS |
| F3 | Release packaging | A matching version tag produces Linux/macOS CLI archives, unsigned desktop bundle, SHA-256 checksums, SPDX SBOMs, and provenance attestations | local unsigned app bundle build passed; release workflow inspected | PASS |
| F4 | OSS governance | Ownership, issue intake, dependency updates, CodeQL, dependency review, cargo audit/deny, and contribution checks are configured | governance files inspected | PASS |
| G1 | Mole integration | Mole detect/doctor/version/analyze/status/history/clean planning use typed argv and shared semantic boundaries | fixture tests; Mole CLI/RPC/MCP path; real clean held by policy | PASS |
| G2 | Mole safety | Dry-run is read-only, real clean is destructive and fail-closed, output is bounded and normalized | policy test; parser fixtures; no real cleanup executed | PASS |
| G3 | Backup/sync integrations | restic and rclone expose typed snapshots, backup, check, copy, and sync dry-run semantics with secret-safe parsing | fixture tests; write paths approval-gated | PASS |
| G4 | Host/package integrations | GitHub/Homebrew/mas/Topgrade adapters use the shared layer without arbitrary writes or sudo | fixture tests; existing discovery preserved | PASS |
| G5 | Security integrations | OSV-Scanner, Gitleaks, and Trivy normalize findings without retaining secret/match values | fixture tests; malformed and missing-tool paths fail closed | PASS |
| G6 | Durable approval | Write plans are persisted with expiry, exact plan fingerprints, one-time consumption, audit events, and RPC/MCP/CLI controls | 147 tests; approval lifecycle and replay rejection passed | PASS |
| G7 | Typed scheduling | Read-only/dry-run native integration actions persist as typed Automation steps and re-plan at execution time; recurring writes are refused | RPC/service tests; 147 tests passed | PASS |
| G8 | Secret-safe persistence | Integration parameters reject direct secret values; scanner and referenced-environment output is redacted before run persistence | core/service tests; 147 tests passed | PASS |

All current acceptance items are `PASS`. Windows-specific evidence comes from
the configured GitHub Actions Windows runner; the macOS workspace records the
local evidence separately.

## External release gates

The repository-level validation gates are complete. The following remain
intentionally external release gates:

- Docker/container smoke testing, a stable public HTTPS MCP deployment, and
  ChatGPT app review/publication remain external deployment gates.
- Real destructive integration writes and native adoption remain approval- and
  host-specific; the checklist keeps them dry-run or fixture-only.

## Commands to execute

```bash
cargo +1.88.0 fmt --all -- --check
cargo +1.88.0 clippy --locked --workspace --all-targets --all-features -- -D warnings
cargo +1.88.0 test --locked --workspace --all-features
cargo +1.88.0 test --locked --workspace --doc
cargo +1.88.0 build --locked --workspace

cargo audit -D warnings
cargo deny check advisories bans licenses sources

cargo package --locked --package taskrail

cd macos/DesktopApp
swift build
swift test
cd ../..

cargo zigbuild --workspace --target x86_64-unknown-linux-gnu
git diff --check
```

The black-box checks must use a temporary Registry and must include at least:

1. direct-argv command execution and log retrieval;
2. interval scheduling;
3. pause/resume and active-run cancellation;
4. native scan;
5. MCP initialize, tools/list, discovery, error, and shell-rejection paths;
6. MCP overview plus Codex, Responses, and GitHub read-only integrations;
7. daemon restart/status and socket permission checks.

## Execution record

This section is filled only from the current execution. Platform-specific
remote evidence is called out explicitly rather than being presented as local
execution.

### Results

The current workspace passed the Rust full suite, strict Clippy, formatting
check, and a temporary-Registry end-to-end smoke for typed Topgrade plan
scheduling. The SwiftUI desktop client built and its 2 tests passed. The
public HTTP adapter unit tests cover health, authentication, origin, MCP
headers, public-profile allowlisting, and protocol-version boundaries. The
GitHub Actions matrix passed on macOS, Ubuntu, and Windows, including the
Windows Task Scheduler live scan and named-pipe RPC round-trip. CodeQL,
dependency review, cargo audit/deny, MSRV, and package validation also passed.
Docker Compose execution remains an external deployment-host check because
Docker is not installed on this host.

## Release decision

The repository-level control-plane implementation is release-candidate ready
for the covered local and CI surfaces. The macOS SwiftUI client remains
macOS-only; Linux and Windows support is the headless Rust CLI/daemon/TUI plus
MCP surface. Public HTTPS hosting, OpenAI review/publication, and real
destructive/adoption operations remain explicit external or approval-gated
steps.
