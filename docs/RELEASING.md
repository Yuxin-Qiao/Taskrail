# Releasing Taskrail

## Local verification

Run the checks in [ACCEPTANCE.md](ACCEPTANCE.md), then inspect the staged
diff and confirm that no credentials, runtime state, or machine-specific
private paths are present.

## GitHub release

Pushing a tag such as `v0.1.0` runs `.github/workflows/release.yml`. It builds
the Rust CLI for Linux and macOS and packages an unsigned `Taskrail.app` for
macOS. The workflow then creates the GitHub release and uploads the assets.

The desktop bundle is intentionally unsigned in the public workflow. Apple
Developer signing identities, entitlements, provisioning, and notarization
credentials are deployment secrets and must be added to a private release
environment before a signed/notarized distribution is enabled. They must never
be committed to this repository or printed in CI logs.
