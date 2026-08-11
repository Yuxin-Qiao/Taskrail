# ADR 0006: SwiftUI remains a thin client

## Decision

The macOS desktop app is a SwiftUI executable under `macos/DesktopApp`. It
connects to the local Unix JSON-RPC socket and displays Registry automations,
runs, logs, events, attention items, and metrics. A run button sends only a
registered automation ID.

The app does not open SQLite, parse launchd, schedule jobs, or execute commands.
Those responsibilities remain in the Rust daemon so the CLI, TUI, and optional
desktop view have one source of truth.

## Verification

`swift build --package-path macos/DesktopApp` is the desktop compile gate. A
daemon socket smoke test remains the runtime boundary for the client protocol.
