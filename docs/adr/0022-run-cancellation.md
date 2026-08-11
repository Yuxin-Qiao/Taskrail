# ADR 0022: Run cancellation is an active-process operation

## Decision

Active runs register a local cancellation channel. `run.cancel` and the CLI
`cancel` command signal that channel; the executor drops the child process with
`kill_on_drop`, records a `cancelled` Run result, and appends
`run.cancel_requested` / `run.cancelled` events.

The JSON-RPC server handles client streams concurrently so a cancellation
request can be served while another request is waiting for a subprocess. A run
that is no longer active cannot be marked cancelled retroactively.

## Interfaces

```text
auto cancel <run-id>
run.cancel { run_id }
```
