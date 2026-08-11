# ADR 0013: Event history is a bounded read model

## Status

Accepted for the local audit surface.

## Decision

The Registry exposes `events.list` with a caller-supplied limit from 1 to 500,
defaulting to 100. Events are returned newest first with their sequence number,
run ID, timestamp, type, and structured payload. The CLI and SwiftUI client use
the same read model through the daemon.

## Consequences

Run, adoption, approval, and watcher changes can be inspected without opening
SQLite directly. Executor failures, timeouts, and cancellations include the
step and attempt in `executor.command.failed` or
`executor.command.cancelled` events. The API is intentionally read-only and
bounded; it does not allow event deletion, payload rewriting, or arbitrary SQL.
