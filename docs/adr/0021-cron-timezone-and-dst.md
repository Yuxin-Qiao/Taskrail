# ADR 0021: Cron evaluation uses the declared IANA timezone

## Decision

`Trigger::Cron.timezone` is part of scheduling semantics, not display-only
metadata. `next_run` evaluates cron expressions in `UTC`, `local`, or an IANA
timezone such as `America/New_York`, then persists the resulting instant in UTC.

Spring-forward nonexistent local times are skipped according to the timezone
database. The scheduler retains the original local schedule while Run records
store the unambiguous UTC `scheduled_at` instant.

## Consequences

The same automation has stable wall-clock behavior across machines and DST
transitions. Invalid or empty timezone names fail closed instead of silently
falling back to UTC.
