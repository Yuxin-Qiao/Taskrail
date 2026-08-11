# ADR 0004: Local JSON-RPC control plane

## Decision

The daemon exposes a versioned JSON-RPC 2.0 protocol over a per-user Unix
domain socket. The socket and its parent directory are restricted to the
current user. The optional desktop view uses the same protocol instead of
reimplementing Registry and execution logic.

The Registry is opened for short synchronous operations. It is never borrowed
across an async subprocess wait, because the SQLite connection is not safe to
hold across Tokio worker threads. RPC handlers therefore pass a Registry path
to the shared service layer, which reopens the database around each audit/event
write.

## Current methods

```text
daemon.ping
automation.list
automation.inspect { id }
automation.pause { id }
automation.resume { id }
source.acknowledge_drift { id }
adoptions.list { limit? }
adoption.inspect { tx_id }
automation.run { id, allow_observed? }
run.cancel { run_id }
run.logs { run_id }
events.list { limit? }
runs.list { limit?, automation_id? }
metrics.list
```

Arbitrary shell and arbitrary command RPCs are not exposed.
