# ADR 0018: Drift acknowledgement updates the baseline and stays paused

## Decision

An operator may explicitly acknowledge an owned source drift after reviewing the
current source. The CLI requires a dry-run/apply choice:

```text
auto acknowledge-drift <source-id> --dry-run
auto acknowledge-drift <source-id> --apply
```

Applying the action updates the expected fingerprint, records a
`source.drift.acknowledged` event, and leaves the automation `paused`. It never
resumes execution implicitly; a separate `auto resume` is required.

Observed sources and sources without an active drift cannot use this action.
