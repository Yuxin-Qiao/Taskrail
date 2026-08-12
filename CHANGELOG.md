# Changelog

## [0.1.3] - 2026-08-12

- Harden CI with a pinned Rust toolchain, MSRV coverage, dependency review, and
  Rust supply-chain audits.
- Make tagged releases version-checked, checksummed, SBOM-backed, and
  provenance-attested.
- Upgrade the cron parser to 0.17 and refresh the release-time GitHub Actions.

## [Unreleased]

- Persist typed native integration steps as managed, schedulable Automations.
- Add integration catalog/doctor status, adoption lifecycle, drift, deletion,
  and approval controls to JSON-RPC and MCP.
- Add macOS desktop discovery, integration health, and approval views.
- Keep plan-only Topgrade actions semantic and never spawn a placeholder command.

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
