# ADR 0027: Integration doctor reports readiness without running work

## Decision

The CLI exposes read-only integration diagnostics:

```text
taskrail integration codex-doctor --cwd <path>
taskrail integration gh-doctor --hostname github.com
```

Codex diagnostics check the CLI version and Git-worktree precondition. GitHub
CLI diagnostics check the executable version and local authentication status.
The checks use fixed argv calls, do not execute an automation, and never print
authentication command output or credentials.
