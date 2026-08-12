# ADR 0032: Mole native integration

## Status

Accepted for Phase B.

## Decision

Mole is the first semantic integration. It uses typed actions for detection,
version, analyze, status, history, and cleanup. Structured Mole actions use
Mole's JSON flags where available; history is bounded to 200 sessions.

Mole produces an `ExecutionPlan` and never starts a subprocess itself. The
existing Taskrail service creates the auditable Run/Event records, calls the
existing argv executor, parses bounded output, records normalized metrics, and
performs adapter verification.

`clean --dry-run` is a read-only plan. Real `clean` is classified as
`Destructive`; the policy requires a persisted, expiring approval bound to the
exact plan fingerprint before execution. The adapter does not fabricate reclaimable bytes: only
explicit numeric byte fields or parseable reported size strings become
metrics.

## Limitations

Mole's current CLI does not provide a deterministic post-clean state contract
that Taskrail can verify without running another potentially expensive scan.
Therefore a successful real clean remains `NotConfigured` for post-action
verification. The action is exposed only through the explicit approval flow;
the default path never starts it.
