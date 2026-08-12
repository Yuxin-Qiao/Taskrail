# Changelog

## [Unreleased]

## [0.1.6] - 2026-08-12

- Make ARM64 macOS (Apple Silicon) and ARM64 Linux the only supported release
  targets; x86_64 and Windows builds are no longer published or tested.
- Add a local multi-host MCP fleet gateway with stable host routing, read-only
  defaults, explicit host-targeted tools, and environment-only token references.
- Extend Fleet with host-targeted native adoption, drift acknowledgement, typed
  integration scheduling, and durable approval lifecycle tools.
- Add a read-only MCP host overview that combines local identity, native
  discovery, Taskrail automations, recent runs, and attention items.
- Add periodic read-only native-discovery supervision, safe missing-source
  detection, and persisted discovery status for daemon and MCP summaries.
- Add an explicit authenticated private HTTP MCP profile for single-host Fleet
  write/run routing; the public HTTP profile remains read-only by default.
- Persist typed native integration steps as managed, schedulable Automations and
  expose integration catalog, adoption, drift, deletion, and approval controls
  through JSON-RPC and MCP.
- Add macOS desktop discovery, integration health, and approval views.

## [0.1.5] - 2026-08-12

- Make macOS App checksum manifests directly verifiable after download.

## [0.1.4] - 2026-08-12

- Upgrade `rusqlite` to 0.40.2 with checked revision conversion at the SQLite boundary.
- Upgrade `sha2` to 0.11.0 while preserving stable SHA-256 fingerprints.

## [0.1.3] - 2026-08-12

- Harden CI with a pinned Rust toolchain, MSRV coverage, dependency review, and
  Rust supply-chain audits.
- Make tagged releases version-checked, checksummed, SBOM-backed, and
  provenance-attested.
- Upgrade the cron parser to 0.17 and refresh the release-time GitHub Actions.

## [0.1.0] - 2026-08-12

Initial open-source release.

- Local Registry, daemon, CLI, TUI, scheduling, runs, logs, and audit events.
- Read-only discovery for launchd, cron, systemd user services, and Homebrew.
- Explicit, fail-closed native adoption boundary.
- MCP tools for ChatGPT, including fresh local automation discovery.
- Optional Codex, Responses-compatible, and read-only GitHub integrations.
- macOS desktop client model decoding and CI coverage.

## [0.1.1] - 2026-08-12

Patch release for the reproducible macOS release workflow, using a runner with
Swift 6 support for the desktop bundle.

## [0.1.2] - 2026-08-12

Patch release for the GitHub Release publisher, which now passes the repository
explicitly in the artifact-only publish job.
