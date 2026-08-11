# ADR 0030: Inbox is a bounded read-only attention aggregation

## Decision

The control plane exposes `auto inbox --limit N`, RPC `inbox.list`, and MCP
`inbox_list`. The read model aggregates existing Registry state:

- pending approval requests;
- automations in `needs_attention`;
- non-terminal adoption journal transactions;
- failed, timed-out, or interrupted Runs.

Items are severity-ordered and bounded to 1--500 entries. The Inbox has no
mutation authority: it cannot approve, resume, rollback, cancel, or retry an
item. Operators must use the existing explicit action for each item after
inspecting its evidence.
