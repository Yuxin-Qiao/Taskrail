# ADR 0024: Run logs are bounded Registry read models

## Decision

Recorded stdout/stderr are exposed only for a known Run ID:

```text
auto logs <run-id>
run.logs { run_id }
run_logs { run_id }
```

The executor bounds captured output before persistence and redacts configured
environment values. These interfaces never accept arbitrary paths and never
read files outside the Registry.
