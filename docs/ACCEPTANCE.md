# Taskrail Acceptance Checklist

This checklist is the release-gate for the current Taskrail workspace. It is
intentionally evidence-based: a feature is accepted only when the stated
command, test, or runtime observation succeeds.

Execution date: 2026-08-13

## Scope and safety

- Tests that create or run automations use a temporary Registry.
- Native discovery is read-only; it must not modify launchd, cron, systemd,
  Homebrew, or application-owned automation definitions.
- No real user automation is created, paused, resumed, adopted, or deleted by
  this checklist.
- Secrets must not appear in source, logs, test output, or this document.

## Acceptance matrix

| ID | Area | Acceptance criterion | Evidence | Status |
| --- | --- | --- | --- | --- |
| A1 | Metadata | Rust workspace, package metadata, license, README, and OSS files are present | `Cargo.toml`, `crates/taskrail/Cargo.toml`, `LICENSE`, `README.md`, `CONTRIBUTING.md`, `SECURITY.md` | PASS |
| A2 | Repository hygiene | Formatting diff is clean and generated/runtime artifacts are ignored | `git diff --check`, `.gitignore`, tracked-artifact scan | PASS |
| A3 | Secret safety | No obvious API key, token, private key, or credential marker is tracked or present in the project | repository secret scan | PASS |
| B1 | Rust quality | Workspace formatting, Clippy, tests, doc-tests, and build pass | Release validation: `cargo fmt --all -- --check`; strict workspace Clippy; 184 taskrail tests including Shortcuts freshness/confirmation coverage; doc-tests and build | PASS |
| B2 | Browser dashboard | Dashboard source is embedded in the published crate and its HTTP route/origin mapping tests pass | `cargo package --locked --package taskrail`; `cargo test --locked --workspace --lib web`; loopback browser smoke | PASS |
| B3 | Desktop client | The current branch intentionally uses the daemon-hosted browser dashboard as its local UI; the historical SwiftUI client is not part of this release surface | Not applicable to the current CLI/browser release; dashboard coverage is recorded in B2 and F1 | N/A |
| B4 | ARM64 Linux build | ARM64 Linux target produces an ELF binary without unsupported-target warnings/errors | GitHub ARM64 CI run `31626953386`: `cargo +stable build --locked --workspace --target aarch64-unknown-linux-gnu` passed | PASS |
| C1 | CLI lifecycle | Add/register, list, inspect, delete, explain, run, runs, logs, pause, resume, inbox, metrics, events, doctor, and verify work | temporary-Registry CLI smoke | PASS |
| C2 | Scheduler | Interval scheduling runs repeatedly; cron/misfire/overlap behavior is covered | 3 interval runs succeeded; scheduler tests passed | PASS |
| C3 | Native discovery | launchd, cron, systemd services/timers, Homebrew, and supported macOS application sources execute read-only discovery without native mutation; app-owned sources remain observe-only | Apple Silicon isolated scan: launchd=26, Shortcuts=14, Automator=6; Keyboard Maestro/Raycast/Alfred/Hazel absent on this host; fixtures cover all application providers, Alfred metadata, systemd timer schedules, safe summaries, optional-provider failure handling, and Registry reconciliation; Linux ARM64 CI systemd/cron smoke; Homebrew/provider fixtures | PASS |
| C4 | Adoption safety | Dry-run, transaction journal, verification failure, rollback, and shell boundary are fail-closed | adoption tests; shell creation now rejected before Registry write | PASS |
| C5 | Daemon/RPC | ARM64 macOS/Linux Unix-socket daemons expose lifecycle/log/run APIs with local-only boundaries | Apple Silicon temporary daemon/MCP smoke; Linux ARM64 CI build/test and XDG/runtime smoke | PASS |
| D1 | MCP contract | MCP initializes, advertises valid schemas/annotations, handles invalid requests, exposes the read-only Taskrail Apps dashboard resource, and exposes overview, discovery, native integrations, typed scheduling, adoption, drift, deletion, and approval tools | 40 local tools including the dashboard render tool and typed Shortcuts tool; 19 public read-only tools; resource read/origin tests; stdio and authenticated private/public HTTP probes completed | PASS |
| D2 | Local automation discovery | A fresh MCP discovery call returns local native and app-owned tasks as safe summaries and reports no native definition mutation | isolated local scan returned 46 sources (launchd=26, Shortcuts=14, Automator=6); app-owned entries are marked `execution=observe_only`; `native_definitions_changed=false`; Registry contained 21 updated observed entries and no app-source attention noise | PASS |
| D3 | ChatGPT connection | Tunnel runtime and ChatGPT integration doctor are ready; the MCP connection can return Taskrail data | Code path and local MCP probes pass; current managed Tunnel runtime is stopped because the protected `CONTROL_PLANE_API_KEY` is absent from this shell/LaunchAgent environment; no credential was written or exposed | BLOCKED_EXTERNAL |
| D4 | ChatGPT app call | A logged-in ChatGPT client can call the connected Taskrail app and report a completed read-only result | In the logged-in ChatGPT web client, the connected Taskrail app called `taskrail_status` followed by `taskrail_scan_native`; the response identified `Yuxin-MacBook.local` (macOS/aarch64), reported the observed host inventory, and confirmed `native_definitions_changed=false`; no write operation was requested | PASS |
| D5 | Scheduled trigger | A future ChatGPT Scheduled task invocation is observed calling the connected Taskrail app and returning a completed result | The connected app and interactive call are verified, but no future Scheduled trigger was created and observed in this release run; ChatGPT's Scheduled page and Taskrail's local scheduler remain separate layers | NOT VERIFIED |
| D6 | Fleet routing | A local fleet MCP gateway loads endpoint metadata without credentials, reports disabled/offline hosts, and routes a named host operation without ambiguity | 40 fleet tools including the read-only dashboard render tool; explicit `host_id` schemas; localhost MCP routing test; Shortcuts typed route, adoption, typed integration, audit, and approval routes covered; token values are never stored | PASS |
| D7 | Fleet control plane | A private Fleet host exposes the same native adoption, drift, typed integration, approval, lifecycle, and run boundaries as a local MCP host | 40 Fleet descriptors (38 require `host_id`); versioned read-only Fleet Apps resource; route-completeness contract; Shortcuts route and action-aware write gate preserve read-only default | PASS |
| E1 | Codex | Codex doctor and real `codex-run` succeed without exposing credentials; incompatible local catalog is handled ephemerally | `ACCEPT_CODEX_OK`; doctor ready | PASS |
| E2 | Responses API | Fake Responses-compatible success/error paths work and redact API key output | responses tests and fake-server smoke | PASS |
| E3 | GitHub watcher | Read-only pulls/issues/checks/failed-runs snapshots work and deduplicate unchanged snapshots | pulls 2, issues 0, failed runs 7, checks 8; watcher tests passed | PASS |
| F1 | Local runtime | Installed daemon serves the loopback dashboard, the local MCP adapter is runnable, and the local socket is user-only | Shortcuts doctor and fresh source scan pass; strict MCP/RPC contract tests pass; preferred dashboard `10100` fell back to `10101` because another local service occupied it; dashboard/MCP Apps render native and app-owned rows with observe-only labels; socket mode 0600; the managed Tunnel process is currently stopped only because its protected runtime key is absent | PASS |
| F2 | CI definition | GitHub Actions YAML parses and contains ARM64 Linux/macOS Rust, ARM64 MSRV, crate packaging with embedded browser and MCP Apps assets, dependency review, and supply-chain audit jobs | CI workflow and package asset checks inspected; browser and both MCP Apps resources are checked; security workflows retained | PASS |
| F3 | Release packaging | A matching version tag produces ARM64 Linux/macOS CLI archives containing the browser dashboard, SHA-256 checksums, SPDX SBOMs, and signed release/SBOM attestations | Published GitHub Release `v0.1.7` from `7f30706`; release workflow `31658493582` passed Linux ARM64 and macOS ARM64 builds, SBOM generation, attestations, checksums, and asset upload | PASS |
| F4 | OSS governance | Ownership, issue intake, dependency updates, CodeQL, dependency review, cargo audit/deny, and contribution checks are configured | governance files inspected | PASS |
| G1 | Mole integration | Mole detect/doctor/version/analyze/status/history/clean planning use typed argv and shared semantic boundaries | fixture tests; Mole CLI/RPC/MCP path; real clean held by policy | PASS |
| G2 | Mole safety | Dry-run is read-only, real clean is destructive and fail-closed, output is bounded and normalized | policy test; parser fixtures; no real cleanup executed | PASS |
| G3 | Backup/sync integrations | restic and rclone expose typed snapshots, backup, check, copy, and sync dry-run semantics with secret-safe parsing | fixture tests; write paths approval-gated | PASS |
| G4 | Host/package integrations | GitHub/Homebrew/mas/Topgrade adapters use the shared layer without arbitrary writes or sudo | fixture tests; existing discovery preserved | PASS |
| G5 | Security integrations | OSV-Scanner, Gitleaks, and Trivy normalize findings without retaining secret/match values | fixture tests; malformed and missing-tool paths fail closed | PASS |
| G6 | Durable approval | Write plans are persisted with expiry, exact plan fingerprints, one-time consumption, audit events, and RPC/MCP/CLI controls | 184 tests; approval lifecycle, replay rejection, and Shortcuts approval-boundary tests passed | PASS |
| G7 | Typed scheduling | Read-only/dry-run native integration actions persist as typed Automation steps and re-plan at execution time; recurring writes are refused | RPC/service tests; 184 tests passed | PASS |
| G8 | Secret-safe persistence | Integration parameters reject direct secret values; scanner and referenced-environment output is redacted before run persistence | core/service tests; 184 tests passed | PASS |

The previously recorded x86_64 and Windows evidence no longer satisfies the
ARM64-only release contract. The updated ARM64 runner evidence and the
matching `v0.1.7` GitHub Release are the current release baseline. The release
workflow's attestation steps recorded signed in-toto SBOM attestations in
GitHub and the Sigstore transparency log; the published release contains both
ARM64 archives, SHA-256 files, and SPDX SBOM JSON files.

## External release gates

The local repository validation gates are complete for the current host. The
following remain intentionally remote or external release gates:

- Docker/container smoke testing, a stable public HTTPS MCP deployment, and
  ChatGPT app review/publication remain external deployment gates.
- The current managed Tunnel runtime is stopped because the protected
  `CONTROL_PLANE_API_KEY` is absent from this shell/LaunchAgent environment;
  no credential was written to the repository, profile, Registry, or logs.
  Restoring it through the operator's protected environment mechanism is an
  external deployment step.
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
cargo package --locked --package taskrail --list | grep -F 'gui/index.html'
cargo package --locked --package taskrail --list | grep -F 'gui/app.js'
cargo package --locked --package taskrail --list | grep -F 'gui/styles.css'
cargo package --locked --package taskrail --list | grep -F 'gui/favicon.svg'
cargo package --locked --package taskrail --list | grep -F 'gui/mcp-app.html'
cargo package --locked --package taskrail --list | grep -F 'gui/mcp-fleet-app.html'

cargo +1.88.0 build --locked --workspace --target aarch64-unknown-linux-gnu
git diff --check
```

The black-box checks must use a temporary Registry and must include at least:

1. direct-argv command execution and log retrieval;
2. interval scheduling;
3. pause/resume and active-run cancellation;
4. native scan;
5. MCP initialize, tools/list, discovery, error, and shell-rejection paths;
6. MCP overview plus Codex, Responses, and GitHub read-only integrations;
7. daemon restart/status, dashboard health, browser route, same-origin write rejection, and socket permission checks.

## Execution record

This section is filled only from the current execution. Platform-specific
remote evidence is called out explicitly rather than being presented as local
execution.

### Results

The current host passes the Rust full suite (184 tests on this release
candidate, including Shortcuts confirmation/freshness and dashboard route
tests), strict Clippy, formatting check, build, package verification, OpenAI
submission validation, cargo audit/deny, and the Fleet contract now covers 40
host-routing descriptors;
remote status, integration catalog, and a plan-only Topgrade route succeeded,
while an unavailable Mole executable returned its real remote error and a
destructive Mole clean was blocked before network access on a read-only host.
The real local MCP stdio probe completed initialize, tools/list, overview,
fresh discovery, and automation listing; the authenticated private HTTP probe
completed initialize, tools/list, and overview, while the public HTTP probe
confirmed 19 read-only tools and rejected execution calls. The local and Fleet
MCP Apps resources are embedded, versioned, and attached only to their explicit
read-only render tools. The dashboard HTTP
smoke covers health, embedded assets, API reads, same-origin writes, and run
logs; the in-app browser loaded the connected dashboard and returned the
connected host's native and app-owned discovery rows. The public HTTP adapter
unit tests cover health, authentication, origin,
MCP headers, public-profile allowlisting, private profile authentication, and
protocol version boundaries.
The `v0.1.7` pull request and protected-main merge passed the Rust, MSRV,
packaging, audit, deny, dependency-review, and CodeQL checks. Release workflow
`31658493582` then published the matching `v0.1.7` GitHub Release with Linux
ARM64 and macOS ARM64 archives, checksums, SPDX SBOMs, and attestations. The
MCP Apps implementation,
HTTP resource-route coverage, and native-task-list Widget are included in the
validated branch history.
Docker Compose execution remains an external
deployment-host check because Docker is not installed on this host.

The local MCP contract and native scan are validated, but the current Tunnel
runtime re-check is externally blocked by the missing protected key described
above. The previously observed logged-in ChatGPT web call proves the
interactive app path for the prior release, not this candidate's new
Shortcuts write path and not a future Scheduled trigger. No credential value
was printed or persisted, and no destructive operation was requested.

## Release decision

The repository-level control-plane implementation is published as `v0.1.7`.
The local repository gates and typed Shortcuts safety boundaries are complete.
A future ChatGPT Scheduled trigger, current Tunnel runtime, public HTTPS
hosting, OpenAI review/publication, and real destructive/adoption operations
remain explicit external or approval-gated steps.
