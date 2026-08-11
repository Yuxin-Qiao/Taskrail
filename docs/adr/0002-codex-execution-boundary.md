# ADR 0002: Codex execution boundary

## Decision

Codex integration uses the local `codex exec` non-interactive interface with
`--json`, `--ephemeral`, explicit `--sandbox`, and optional `--output-schema`.
The supervisor does not pass arbitrary shell strings or use the dangerous
bypass flag. A Codex run must start inside a Git repository; write-capable runs
must target an explicit Git worktree and have a persistent approval whose scope
matches the exact cwd, prompt digest, sandbox, model, schema and worktree path.

## Verification

Codex JSONL is parsed as evidence, and malformed events, failed turns, non-zero
exit codes and timeouts are failures. A successful Codex result is not itself
task acceptance; `auto verify` runs deterministic argv commands separately.
Command executor output redacts configured environment values before persistence;
Run revision snapshots retain environment keys but replace values with
`[REDACTED]`.

## Consequences

The current workspace is not a Git repository, so this integration can be
compiled and tested here but cannot start a real Codex run until the project is
placed in Git. The approval and worktree paths remain usable for later phases.
