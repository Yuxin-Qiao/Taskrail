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
boundary. A conservative policy allows read-only plans and requires every write
plan to carry a persisted, expiring, plan-bound approval. Approval is consumed
atomically before the existing executor starts, so no adapter can bypass the
policy boundary or replay a grant.

Integration parsers receive bounded `ProcessOutput` and return normalized
metrics, findings, changes, artifacts, and a human-readable summary. Raw
stdout/stderr is not part of the normalized Registry-facing model, and secret
values are never represented by the integration framework.

## Consequences

- Mole, restic, rclone, GitHub, and Homebrew adapters can share one contract.
- Existing GitHub and Homebrew implementations can be adapted incrementally
  without creating parallel executors or breaking discovery reconciliation.
- CI can test adapters with fixtures and does not need external tools installed.
- Write-capable native actions are exposed only through typed CLI, MCP, and
  ChatGPT paths that support request, explicit decision, and one-time execute.
