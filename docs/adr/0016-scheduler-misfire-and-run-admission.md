# ADR 0016: Scheduler pass applies misfire policy before run admission

## Decision

The daemon scheduler evaluates each due managed/adopted automation with its
stored `MisfirePolicy` before starting work:

- `skip` advances to the next future occurrence;
- `run_once` records one run with the original `scheduled_at` and advances from
  the current time;
- `catch_up` executes at most the configured number of missed occurrences and
  leaves a still-due occurrence for a later pass.

`catch_up.max_runs` must be at least one. A zero value is rejected by
`policy-check` and by scheduler evaluation rather than being interpreted as a
silent skip.

A scheduler pass leaves a future `next_run_at` unchanged. It only advances the
schedule after a due occurrence has been evaluated, preventing idle polling
from pulling future runs forward.

Automations may set `misfire_max_age_seconds`. A due occurrence older than that
bound emits `scheduler.misfire_expired` and advances without creating a Run;
the default is unset for backward compatibility.

The pass reads persisted `runs.status = 'running'` rows before admission and
honors `ForbidOverlap`. Each scheduled run stores its intended `scheduled_at`,
so sleep/wake recovery remains auditable rather than looking like a manual run.

## Consequences

The daemon does not silently replay every missed interval after sleep, and a
second daemon pass cannot admit a forbidden overlap already recorded in the
Registry. The same overlap policy is enforced atomically at Run insertion, so
manual CLI, RPC, and concurrent daemon entry points cannot bypass it. A later
pass is responsible for remaining catch-up work. A skipped due occurrence emits
`scheduler.misfire_skipped` with its intended `scheduled_at`, so omission of a
Run remains auditable. A rejected overlap admission emits
`run.admission_rejected` without inventing a Run ID.
