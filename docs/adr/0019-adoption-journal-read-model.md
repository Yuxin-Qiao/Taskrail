# ADR 0019: Adoption journals have bounded read-only inspection surfaces

## Decision

Adoption transactions are exposed through read-only, bounded views:

```text
taskrail adoptions --limit 100
taskrail adoption-inspect <tx-id>
taskrail doctor adoption
adoptions.list { limit? }
adoption.inspect { tx_id }
```

The views include transaction state, checkpoint, error, source ID, native
snapshot, and update time. They never invoke a native controller or alter the
transaction state. Rollback remains a separate explicit mutation and may be
used for committed, interrupted, or needs-attention transactions. A successful
rollback restores the native snapshot and converges the Registry automation to
`observed` plus `needs_attention`; a failed restore records
`adoption.rollback_failed` and never reports a commit.
