# ADR 0003: GitHub integration starts read-only

## Decision

The first GitHub adapter shells out only to `gh` commands that return
structured JSON: open issues, open pull requests, failed workflow runs and PR
checks. Repository identifiers are validated before process launch, and no
comment, issue, PR, merge or workflow mutation is exposed.

## Consequences

The adapter is small and uses the user's existing `gh` authentication without
duplicating a GitHub SDK. Returned issue bodies, titles, logs and check names
must remain untrusted data when passed to a later AI executor. External write
operations require a separate capability and approval design.
