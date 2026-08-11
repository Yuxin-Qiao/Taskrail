# ADR 0017: Pause and resume are control-plane-only lifecycle transitions

## Decision

Managed and adopted automations expose explicit `pause` and `resume` actions.
These actions change the local scheduler's `RuntimeState` and append an audit
event; they do not mutate an observed native scheduler source.

Observed automations remain read-only and must be paused through their native
provider. A drifted or otherwise `needs_attention` automation cannot have its
runtime state changed until its attention state is repaired explicitly.

## Interfaces

```text
taskrail pause <automation-id>
taskrail resume <automation-id>
automation.pause { id }
automation.resume { id }
```

The transitions are idempotent when the requested state is already current.
