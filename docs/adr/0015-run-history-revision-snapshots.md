# ADR 0015: Run history stores an immutable automation snapshot

## Decision

Every persisted run stores the full serialized `Automation` definition that was
used when the run started, alongside its revision number. The read-only
`runs.list` surface returns bounded recent records and supports filtering by
automation ID.

The snapshot is retained in SQLite as evidence. It is not rewritten when the
current automation definition changes, so historical runs remain explainable.
Run output remains separate from the history read model and is not returned by
`runs.list`.

## Interfaces

```text
runs.list { limit?, automation_id? }
taskrail runs --limit 100
taskrail runs --automation weekly-clean
```

The limit is bounded to 1–500. The browser dashboard presents the same
read-only records through its Runs section.

## Migration

Existing registries receive `runs.automation_snapshot_json` with an empty JSON
object default. New runs always write a complete snapshot; old runs remain
queryable without inventing historical definitions.
