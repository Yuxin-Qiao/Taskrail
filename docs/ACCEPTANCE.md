# Taskrail Acceptance Checklist

This checklist is the release-gate for the current Taskrail workspace. It is
intentionally evidence-based: a feature is accepted only when the stated
command, test, or runtime observation succeeds.

Execution date: 2026-08-13

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
| B1 | Rust quality | Workspace formatting, Clippy, tests, doc-tests, and build pass | `cargo +1.88.0 fmt --all -- --check`; `cargo +1.88.0 clippy --locked --workspace --all-targets --all-features -- -D warnings`; `cargo +1.88.0 test --locked --workspace --all-features`; 171 tests passed | PASS |
| B2 | Browser dashboard | Dashboard source is embedded in the published crate and its HTTP route/origin mapping tests pass | `cargo package --locked --package taskrail`; `cargo test --locked --workspace --lib web`; loopback browser smoke | PASS |
| B3 | Desktop client | The current branch intentionally uses the daemon-hosted browser dashboard as its local UI; the historical SwiftUI client is not part of this release surface | Not applicable to the current CLI/browser release; dashboard coverage is recorded in B2 and F1 | N/A |
| B4 | ARM64 Linux build | ARM64 Linux target produces an ELF binary without unsupported-target warnings/errors | GitHub ARM64 CI run `31626953386`: `cargo +stable build --locked --workspace --target aarch64-unknown-linux-gnu` passed | PASS |
| C1 | CLI lifecycle | Add/register, list, inspect, delete, explain, run, runs, logs, pause, resume, inbox, metrics, events, doctor, and verify work | temporary-Registry CLI smoke | PASS |
| C2 | Scheduler | Interval scheduling runs repeatedly; cron/misfire/overlap behavior is covered | 3 interval runs succeeded; scheduler tests passed | PASS |
| C3 | Native discovery | launchd, cron, systemd, and Homebrew discovery paths execute without native mutation on supported ARM64 hosts | Apple Silicon live `overview`/discovery; Linux ARM64 CI systemd/cron smoke; Homebrew/provider fixtures; background reconciliation and missing-source tests | PASS |
| C4 | Adoption safety | Dry-run, transaction journal, verification failure, rollback, and shell boundary are fail-closed | adoption tests; shell creation now rejected before Registry write | PASS |
| C5 | Daemon/RPC | ARM64 macOS/Linux Unix-socket daemons expose lifecycle/log/run APIs with local-only boundaries | Apple Silicon temporary daemon/MCP smoke; Linux ARM64 CI build/test and XDG/runtime smoke | PASS |
| D1 | MCP contract | MCP initializes, advertises valid schemas/annotations, handles invalid requests, exposes the read-only Taskrail Apps dashboard resource, and exposes overview, discovery, native integrations, typed scheduling, adoption, drift, deletion, and approval tools | 39 local tools including the dashboard render tool; 19 public read-only tools; resource read/origin tests; real stdio and authenticated private/public HTTP probes completed | PASS |
| D2 | Local automation discovery | A fresh MCP discovery call returns local native tasks as safe summaries and reports no native definition mutation | live call returned 26 sources, `native_definitions_changed=false` | PASS |
| D3 | ChatGPT connection | Tunnel runtime and ChatGPT integration doctor are ready; the MCP connection can return Taskrail data | Managed runtime `taskrail-local` reports `runtime_state=ready`, `process_running=true`, `healthy=true`, `ready=true`; `taskrail integration chatgpt-doctor` is ready; direct MCP stdio initialize, `taskrail_overview`, and `taskrail_discover_local_automations` calls succeeded | PASS |
| D4 | Scheduled workflow | ChatGPT Scheduled task can call the connected Taskrail app and report a completed read-only result | The runtime and MCP contract are ready, but the final ChatGPT desktop/UI call is not yet rechecked because the Mac was locked during this run | BLOCKED (desktop UI unavailable) |
| D5 | Fleet routing | A local fleet MCP gateway loads endpoint metadata without credentials, reports disabled/offline hosts, and routes a named host operation without ambiguity | 39 fleet tools including the read-only dashboard render tool; explicit `host_id` schemas; localhost MCP routing test; adoption, typed integration, audit, and approval routes covered; token values are never stored | PASS |
| D6 | Fleet control plane | A private Fleet host exposes the same native adoption, drift, typed integration, approval, lifecycle, and run boundaries as a local MCP host | 39 Fleet descriptors (37 require `host_id`); versioned read-only Fleet Apps resource; route-completeness contract; private HTTP target exposed the same local data/action boundaries; action-aware write gate and read-only default preserved | PASS |
| E1 | Codex | Codex doctor and real `codex-run` succeed without exposing credentials; incompatible local catalog is handled ephemerally | `ACCEPT_CODEX_OK`; doctor ready | PASS |
| E2 | Responses API | Fake Responses-compatible success/error paths work and redact API key output | responses tests and fake-server smoke | PASS |
| E3 | GitHub watcher | Read-only pulls/issues/checks/failed-runs snapshots work and deduplicate unchanged snapshots | pulls 2, issues 0, failed runs 7, checks 8; watcher tests passed | PASS |
| F1 | Local runtime | Installed daemon serves the loopback dashboard, MCP runtime is healthy/ready, and local socket is user-only | doctor ready; preferred dashboard `10100` fell back to `10101` because another local service occupied it; `/healthz` ready; browser shows connected host and 26 native rows; the MCP Apps resource now renders the discovered native-task list; socket mode 0600; host identity falls back to a bounded system hostname when no label is configured | PASS |
| F2 | CI definition | GitHub Actions YAML parses and contains ARM64 Linux/macOS Rust, ARM64 MSRV, crate packaging with embedded browser and MCP Apps assets, dependency review, and supply-chain audit jobs | CI workflow and package asset checks inspected; browser and both MCP Apps resources are checked; security workflows retained | PASS |
| F3 | Release packaging | A matching version tag produces ARM64 Linux/macOS CLI archives containing the browser dashboard, SHA-256 checksums, SPDX SBOMs, and signed release/SBOM attestations | Tag `v0.1.6` points to `a553be0`; release workflow run `31635879714` completed successfully; both ARM64 archives, checksums, SPDX SBOMs, and signed GitHub SBOM attestations were published and independently downloaded/checked | PASS |
| F4 | OSS governance | Ownership, issue intake, dependency updates, CodeQL, dependency review, cargo audit/deny, and contribution checks are configured | governance files inspected | PASS |
| G1 | Mole integration | Mole detect/doctor/version/analyze/status/history/clean planning use typed argv and shared semantic boundaries | fixture tests; Mole CLI/RPC/MCP path; real clean held by policy | PASS |
| G2 | Mole safety | Dry-run is read-only, real clean is destructive and fail-closed, output is bounded and normalized | policy test; parser fixtures; no real cleanup executed | PASS |
| G3 | Backup/sync integrations | restic and rclone expose typed snapshots, backup, check, copy, and sync dry-run semantics with secret-safe parsing | fixture tests; write paths approval-gated | PASS |
| G4 | Host/package integrations | GitHub/Homebrew/mas/Topgrade adapters use the shared layer without arbitrary writes or sudo | fixture tests; existing discovery preserved | PASS |
| G5 | Security integrations | OSV-Scanner, Gitleaks, and Trivy normalize findings without retaining secret/match values | fixture tests; malformed and missing-tool paths fail closed | PASS |
| G6 | Durable approval | Write plans are persisted with expiry, exact plan fingerprints, one-time consumption, audit events, and RPC/MCP/CLI controls | 171 tests; approval lifecycle and replay rejection passed | PASS |
| G7 | Typed scheduling | Read-only/dry-run native integration actions persist as typed Automation steps and re-plan at execution time; recurring writes are refused | RPC/service tests; 171 tests passed | PASS |
| G8 | Secret-safe persistence | Integration parameters reject direct secret values; scanner and referenced-environment output is redacted before run persistence | core/service tests; 171 tests passed | PASS |

The previously recorded x86_64 and Windows evidence no longer satisfies the
ARM64-only release contract. The updated ARM64 runner evidence and the
matching `v0.1.6` release assets are now complete. The release workflow's
attestation step recorded signed in-toto SBOM attestations in GitHub and the
Sigstore transparency log; the local asset check verified both SHA-256 files,
archive contents, and SBOM JSON structure.

## External release gates

The local repository validation gates are complete for the current host. The
following remain intentionally remote or external release gates:

- Docker/container smoke testing, a stable public HTTPS MCP deployment, and
  ChatGPT app review/publication remain external deployment gates.
- The private Tunnel runtime is currently managed and healthy for this session;
  its key was supplied only to the short-lived connect process and was not
  written to the repository, profile, launchd environment, or logs. A
  production/unattended setup must still provision the key through the
  operator's protected environment mechanism.
- The validated feature branch was squash-merged into protected `main` as
  `a553be0`; the main-branch CI, security, CodeQL, and release workflows are
  green. Subsequent documentation-only corrections must continue through the
  protected pull-request path.
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

The current host passes the Rust full suite (171 tests, including the dashboard
route and origin tests), strict Clippy, formatting check, build, package verification,
OpenAI submission validator, and the Fleet localhost routing smoke. The Fleet
black-box exposed 38 host-routing tools;
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
logs; the in-app browser loaded the connected dashboard and returned 26 native
discovery rows. The public HTTP adapter unit tests cover health, authentication, origin,
MCP headers, public-profile allowlisting, private profile authentication, and
protocol version boundaries.
GitHub Actions runs `31635284025` (CI), `31635283861` (Security), and
`31635284140` (CodeQL) passed on main commit `a553be0`; the pull request's
dependency review also passed in run `31634056819`. Release workflow run
`31635879714` passed and published the `v0.1.6` assets described in F3. The
MCP Apps implementation,
HTTP resource-route coverage, and native-task-list Widget are included in the
validated branch history.
Docker Compose execution remains an external
deployment-host check because Docker is not installed on this host.

The current local Tunnel re-check is ready: `tunnel-client runtimes status
taskrail-local --json` reports `runtime_state=ready`, `process_running=true`,
`healthy=true`, and `ready=true`. `tunnel-client health --require-control-plane-poll`
returned HTTP 200 for both `/healthz` and `/readyz`. The Taskrail MCP stdio
probe completed initialize, overview, and fresh local discovery; the discovery
call returned 26 native sources. No credential value was printed or persisted.

## Release decision

The repository-level control-plane implementation is published as `v0.1.6`
for the ARM64-only contract, with the release archives and signed SBOM
attestations available from GitHub. The repository-controlled release gates
and the local MCP/Tunnel runtime gates are complete. A final ChatGPT Scheduled
task/UI call remains to be rechecked after the operator unlocks the Mac.
Public HTTPS hosting, OpenAI review/publication, and real destructive/adoption
operations remain explicit external or approval-gated steps.
