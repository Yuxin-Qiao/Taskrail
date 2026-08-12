# Releasing Taskrail

## Local verification

Run the checks in [ACCEPTANCE.md](ACCEPTANCE.md), then inspect the staged
diff and confirm that no credentials, runtime state, or machine-specific
private paths are present.

## GitHub release

Pushing a tag such as `v0.1.3` runs `.github/workflows/release.yml`. The tag
must match the `taskrail` Cargo package version and the macOS bundle version.
The workflow builds the Rust CLI for Linux and macOS, packages an unsigned
`Taskrail.app`, publishes SHA-256 checksums and SPDX SBOMs, and creates GitHub
artifact attestations for the CLI archives.

The final publish job is attached to the `release` GitHub Environment. The
repository currently restricts that environment to `v*` tags and applies a
15-minute wait timer before publishing. A maintainer should inspect the
workflow run and tag before that timer elapses; add an independent reviewer
once the repository has more than one release maintainer.

The desktop bundle is intentionally unsigned in the public workflow. Apple
Developer signing identities, entitlements, provisioning, and notarization
credentials are deployment secrets and must be added to a private release
environment before a signed/notarized distribution is enabled. They must never
be committed to this repository or printed in CI logs.
