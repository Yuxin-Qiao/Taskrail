# ADR 0030: Inbox is a bounded read-only attention aggregation

## Decision

Taskrail exposes `taskrail inbox --limit N` and RPC `inbox.list`. The read model
aggregates existing Registry state:

- automations in `needs_attention`;
- non-terminal adoption journal transactions;
- failed, timed-out, or interrupted Runs.

Items are severity-ordered and bounded to 1--500 entries. The Inbox has no
mutation authority: it cannot approve, resume, rollback, cancel, or retry an
item. Operators must use the existing explicit action for each item after
inspecting its evidence.
