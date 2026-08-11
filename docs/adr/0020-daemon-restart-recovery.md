# ADR 0020: Daemon startup recovers orphaned running records

## Decision

At daemon startup, persisted runs still in `running` state are treated as
orphaned by the previous daemon instance. They are changed to `interrupted`,
receive an `run.interrupted` event with reason `daemon_restart`, and no longer
block `ForbidOverlap` admission. The recovery runs once per daemon startup,
before the first scheduler pass.

The recovery does not fabricate success, replay the old run, or alter native
scheduler sources. A subsequent scheduler pass evaluates the automation's
normal misfire policy.
