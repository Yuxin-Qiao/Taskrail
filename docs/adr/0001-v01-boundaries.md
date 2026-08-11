# ADR 0001: V0.1 boundaries

## Decision

V0.1 uses one local Registry and explicit ownership states. Native jobs are
discovered first and remain `observed` until an adoption transaction proves
that the native source is disabled and exactly one internal owner is active.

The command executor accepts an executable and argv vector, never an implicit
shell string. A `sh -c` style invocation is rejected even when it arrives as a
native launchd argument vector.

## Consequences

This leaves SwiftUI, AI executors, MCP, systemd and privileged helpers for later
phases. It makes the first milestone testable with local fixtures and prevents
the control plane from silently becoming a generic arbitrary-command root shell.
