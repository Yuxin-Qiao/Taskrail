# ADR 0028: Retries are bounded, observable, and cancellation-aware

## Decision

Automation definitions may configure a retry policy:

```yaml
policy:
  retry:
    max_attempts: 1
    initial_backoff_seconds: 0
    max_backoff_seconds: 600
```

The default is one attempt, so existing definitions do not change behavior.
Only failed and timed-out executor steps may be retried. Each retry emits an
`executor.command.retrying` event containing the step, current attempt, next
attempt, status, and selected backoff. Backoff is exponential from the
configured initial value and is capped by `max_backoff_seconds`.

Cancellation interrupts a retry wait and prevents another attempt. Policy
preflight failures and approval-gated work do not enter the retry loop. A zero
attempt count or an initial backoff greater than its cap is rejected before a
Run starts. This keeps retries bounded and auditable without treating repeated
execution as an implicit escalation of authority.
