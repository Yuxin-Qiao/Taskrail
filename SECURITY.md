# Security

Taskrail manages local schedulers, commands, filesystem paths, and developer
repositories. Treat Registry data, command output, launchd definitions, GitHub
content, and AI output as untrusted input.

## Defaults

- Commands use an executable plus an argv vector. Shell strings are not accepted.
- Explicit shell invocations such as `sh -c` are rejected by the executor.
- Discovered sources that invoke a shell are not promoted to runnable observed
  automations.
- `taskrail scan` is read-only.
- Existing native jobs remain observation-only until an explicit adoption command.
- Native adoption requires a matching fingerprint, a persisted snapshot, disable
  verification, and a rollback record.
- System `/Library/LaunchDaemons` remain observation-only. User launchd adoption
  is limited to `~/Library/LaunchAgents`.
- `taskrail daemon --install` writes only the current user's Taskrail LaunchAgent.
- Secrets must not be placed in YAML, SQLite event payloads, logs, or fixtures.
- Captured command output and immutable run snapshots redact configured environment
  values.
- Cancellation is accepted only for an active process owned by the current daemon.
- GitHub integration uses fixed read-only `gh --json` queries and never writes to
  GitHub.
- Homebrew integration only observes `brew services list --json`.
- Codex workspace-write runs must target an explicit worktree; the main checkout
  is not silently treated as a write target.
- Responses-compatible API keys are read by environment-variable name and are
  never stored in an automation definition.
- There is no privileged helper, remote executor, MCP server, or generic root
  command path.

## Reporting

Do not disclose vulnerabilities in public issues. Until a private reporting
endpoint is configured, contact the project maintainer directly with a minimal
reproduction. Do not include credentials or private automation definitions.
