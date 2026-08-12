# ADR 0031: Native integration semantic layer

## Status

Accepted for Phase A.

## Decision

External automation-friendly CLI tools are represented by semantic
integrations. An integration describes detection, capabilities, risk,
deterministic argv planning, bounded-output parsing, and verification. It does
not spawn processes or write the Registry directly.

The integration boundary produces an `ExecutionPlan` containing a
`CommandSpec`, environment-variable references, risk classification, dry-run
support, timeout, and optional deterministic verification checks. Plans are
validated for direct argv and must mark every non-read action as approval
required.

The existing Taskrail executor remains the subprocess primitive. The existing
service, Run, Event, and Registry paths remain the persistence and audit
boundary. A conservative default policy allows read-only plans and holds all
writes for a future durable approval implementation; no approval bypass is
introduced in Phase A.

Integration parsers receive bounded `ProcessOutput` and return normalized
metrics, findings, changes, artifacts, and a human-readable summary. Raw
stdout/stderr is not part of the normalized Registry-facing model, and secret
values are never represented by the integration framework.

## Consequences

- Mole, restic, rclone, GitHub, and Homebrew adapters can share one contract.
- Existing GitHub and Homebrew implementations can be adapted incrementally
  without creating parallel executors or breaking discovery reconciliation.
- CI can test adapters with fixtures and does not need external tools installed.
- Durable approvals are still a required follow-up before write-capable native
  actions are exposed through CLI, MCP, or ChatGPT.
