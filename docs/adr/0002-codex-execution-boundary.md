# ADR 0002: Codex execution boundary

## Decision

Codex integration uses the local `codex exec` non-interactive interface with
`--json`, `--ephemeral`, explicit `--sandbox`, and optional `--output-schema`.
When needed, Taskrail can pass an explicit `model_catalog_json` config override
for the run. For the known cc-switch catalog incompatibility, it can instead
create a short-lived 0600 compatibility copy that removes only the unsupported
`audio` modality; neither path mutates the user's global Codex configuration.
The integration does not pass arbitrary shell strings or use the dangerous
bypass flag. A Codex run must start inside a Git repository; write-capable runs
must target an explicit Git worktree rather than silently modifying the main
checkout.

## Verification

Codex JSONL is parsed as evidence, and malformed events, failed turns, non-zero
exit codes and timeouts are failures. A successful Codex result is not itself
task acceptance; `taskrail verify` runs deterministic argv commands separately.
Command executor output redacts configured environment values before persistence;
Run revision snapshots retain environment keys but replace values with
`[REDACTED]`.

## Consequences

Codex remains an optional executor. It is not the identity of the product and
does not own the Registry, scheduler, or run history.
