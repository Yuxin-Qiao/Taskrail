# ADR 0007: V0.3 native and agent adapters remain narrow

## Status

Accepted for the V0.3 implementation.

## Decision

Add the following adapters behind existing Registry and policy boundaries:

- `SystemdProvider` discovers `systemd --user` service units on Linux and is
  harmless on macOS when `systemctl` is unavailable. `SystemdController` can
  adopt explicitly enabled `.service` units through `disable --now`, verify the
  disabled/inactive state, and restore the captured enabled/active state.
  Fixture input is supported so discovery parsing is testable without a Linux
  service manager.
- `AppServerClient` starts the local Codex App Server over stdio JSONL. It
  requires a Git repository and defaults to a read-only sandbox. Its approval
  handler can either decline immediately or persist a typed Registry approval,
  wait for `auto approve/reject`, and respond with the resulting decision.
  Requests above the configured risk ceiling are rejected by policy.
- `PrivilegedHelper` exposes only typed, known system-job operations. The default
  `NoPrivilegedHelper` refuses every operation. No generic root command executor
  is part of the interface.

## Consequences

The current CLI can observe Linux user services and run App Server turns with a
bounded approval loop, but it cannot silently mutate system services. A future
privileged helper must be separately reviewed, installed, authenticated, and
connected to the same approval and audit model before system-write operations
are enabled.
