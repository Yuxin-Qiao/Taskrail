# Releasing Taskrail

## Local verification

Run the checks in [ACCEPTANCE.md](ACCEPTANCE.md), then inspect the staged
diff and confirm that no credentials, runtime state, or machine-specific
private paths are present.

## GitHub release

Pushing a tag such as `v0.1.6` runs `.github/workflows/release.yml`. The tag
must match the `taskrail` Cargo package version and the macOS bundle version.
The workflow builds only ARM64 artifacts: an `aarch64-unknown-linux-gnu` Linux
CLI, an `aarch64-apple-darwin` macOS CLI, and an Apple Silicon unsigned
`Taskrail.app`. It publishes SHA-256 checksums and SPDX SBOMs, and creates
GitHub artifact attestations for the CLI archives.

The final publish job is attached to the `release` GitHub Environment. The
repository currently restricts that environment to `v*` tags and applies a
15-minute wait timer before publishing. A maintainer should inspect the
workflow run and tag before that timer elapses; add an independent reviewer
once the repository has more than one release maintainer.

Before tagging a release, update the crate and desktop bundle versions together,
run the full Rust and Swift validation commands in `docs/ACCEPTANCE.md`, and
verify that the generated app executable matches `Info.plist` (`taskrail`).

The supported release contract is ARM64 macOS (Apple Silicon) and ARM64 Linux.
x86_64 and Windows are not supported release targets. The Rust crate fails
closed at compile time on other targets so an accidental unsupported build
cannot be mistaken for a supported artifact.

The desktop bundle is intentionally unsigned in the public workflow. Apple
Developer signing identities, entitlements, provisioning, and notarization
credentials are deployment secrets and must be added to a private release
environment before a signed/notarized distribution is enabled. They must never
be committed to this repository or printed in CI logs.
