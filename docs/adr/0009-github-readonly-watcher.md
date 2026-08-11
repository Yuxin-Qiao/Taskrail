# ADR 0009: GitHub watchers are durable read-only snapshots

## Status

Accepted for the V0.2/V0.3 implementation.

## Decision

The GitHub watcher reuses the structured `gh` adapter and supports one-shot or
interval polling. Each successful observation is normalized and fingerprinted
by `(repository, query kind, pull number)`:

- the full latest snapshot is stored in the local SQLite Registry;
- the first observation and every changed fingerprint append a
  `github.snapshot.changed` event;
- unchanged observations update the latest snapshot but do not append a new
  event;
- query arguments remain fixed and read-only; no user-provided shell fragment is
  accepted.

## Consequences

The watcher can feed a later inbox or agent triage step without treating GitHub
content as instructions. It does not post comments, create issues, mutate pull
requests, or infer that a changed snapshot requires remediation.
