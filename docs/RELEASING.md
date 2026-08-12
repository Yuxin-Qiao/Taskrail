# Releasing Taskrail

## Local verification

Run the checks in [ACCEPTANCE.md](ACCEPTANCE.md), then inspect the staged
diff and confirm that no credentials, runtime state, or machine-specific
private paths are present.

## GitHub release

Pushing a tag such as `v0.1.6` runs `.github/workflows/release.yml`. The tag
must match the `taskrail` Cargo package version. The workflow builds ARM64 CLI
archives for `aarch64-unknown-linux-gnu` and `aarch64-apple-darwin`. The CLI
archives contain the daemon-hosted browser dashboard and MCP Apps assets. The
workflow publishes SHA-256 checksums, SPDX SBOMs, and GitHub artifact
attestations for the CLI archives.

The final publish job is attached to the `release` GitHub Environment. The
repository currently restricts that environment to `v*` tags and applies a
15-minute wait timer before publishing. A maintainer should inspect the
workflow run and tag before that timer elapses; add an independent reviewer
once the repository has more than one release maintainer.

Before tagging a release, update the crate version, run the full Rust validation
commands in `docs/ACCEPTANCE.md`, and verify that `cargo package` contains the
browser dashboard and both MCP Apps resources: `gui/index.html`, `gui/app.js`,
`gui/styles.css`, `gui/favicon.svg`, `gui/mcp-app.html`, and
`gui/mcp-fleet-app.html`.

The supported release contract is ARM64 macOS (Apple Silicon) and ARM64 Linux.
x86_64 and Windows are not supported release targets. The Rust crate fails
closed at compile time on other targets so an accidental unsupported build
cannot be mistaken for a supported artifact.

The current release surface does not include a native desktop bundle. The
daemon-hosted browser dashboard is packaged into the CLI and remains
loopback-only; it must not be mistaken for a public deployment endpoint.
