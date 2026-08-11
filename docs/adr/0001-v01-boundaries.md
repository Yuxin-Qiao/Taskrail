# ADR 0001: V0.1 boundaries

## Decision

V0.1 uses one local Registry and explicit ownership states. Native jobs are
discovered first and remain `observed` until an adoption transaction proves
that the native source is disabled and exactly one internal owner is active.

The command executor accepts an executable and argv vector, never an implicit
shell string. A `sh -c` style invocation is rejected even when it arrives as a
native launchd argument vector.

## Consequences

The first milestone stays focused on the local Registry, scheduler, CLI, and
TUI. Optional integrations remain outside the core manager, and the executor
cannot silently become a generic arbitrary-command root shell.
