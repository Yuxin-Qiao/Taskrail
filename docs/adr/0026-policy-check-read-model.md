# ADR 0026: Policy check is a side-effect-free execution preflight

## Decision

The supervisor exposes a read-only policy preflight:

```text
auto policy-check <automation-id>
automation.policy_check { id }
```

It reports `pass`, `warn`, or `fail` for shell use, executable presence, risk
ceilings, approval requirements, runtime attention state, timeout,
`budget.max_steps`, retry bounds, and trigger validity. It never starts a
process, creates a Run, changes runtime state, or mutates a native source.
