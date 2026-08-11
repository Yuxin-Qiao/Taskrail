# ADR 0023: Automation step count is a fail-closed policy budget

## Decision

Automation definitions may set:

```yaml
policy:
  budget:
    max_steps: 40
```

Before a Run is recorded or any command starts, the supervisor rejects a
definition whose step count exceeds `max_steps`. Older definitions omit the
field and receive a conservative default of 100. The budget is a policy guard,
not a cost estimate; provider token and dollar usage remain unknown unless the
provider reports reliable usage.
