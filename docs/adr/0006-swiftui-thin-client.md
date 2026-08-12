# ADR 0006: SwiftUI remains a thin client

## Decision

The macOS desktop app is a SwiftUI executable under `macos/DesktopApp`. It
connects to the local Unix JSON-RPC socket and displays Registry automations,
runs, logs, events, attention items, and metrics. A run button sends only a
registered automation ID.

The app does not open SQLite, parse launchd, schedule jobs, or execute commands.
Those responsibilities remain in the Rust daemon so the CLI, TUI, browser
dashboard, and optional desktop view have one source of truth.

## Verification

`swift build --package-path macos/DesktopApp` and `swift test --package-path
macos/DesktopApp` remain the desktop compile gates. The browser dashboard adds
Rust route tests, crate packaging asset checks, and a loopback HTTP smoke test.
