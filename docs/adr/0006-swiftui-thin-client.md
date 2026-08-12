# ADR 0006: SwiftUI remains a thin client

**Status: Superseded by the daemon-hosted browser dashboard release surface.**

This document records the historical desktop-client direction. The SwiftUI
client is not included in the current branch or release workflow; the Rust
daemon-hosted browser dashboard is now the supported local UI.

## Decision

The historical macOS desktop app was a SwiftUI executable under
`macos/DesktopApp`. It
connects to the local Unix JSON-RPC socket and displays Registry automations,
runs, logs, events, attention items, and metrics. A run button sends only a
registered automation ID.

The app does not open SQLite, parse launchd, schedule jobs, or execute commands.
Those responsibilities remained in the Rust daemon so the CLI, TUI, browser
dashboard, and optional desktop view had one source of truth.

## Verification

The historical desktop compile gates were `swift build --package-path
macos/DesktopApp` and `swift test --package-path macos/DesktopApp`. The current
release uses Rust route tests, crate packaging asset checks, and a loopback
HTTP smoke test for the browser dashboard instead.
