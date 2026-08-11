# Security

This project manages local schedulers, commands, filesystem paths and potentially
developer repositories. Treat Registry data, command output, launchd definitions,
GitHub content and future agent output as untrusted input.

## V0.1 defaults

- `shell` is disabled; commands use an executable plus an argv vector.
- Explicit shell invocations such as `sh -c` are rejected by the executor.
- Discovered sources invoking a shell are not promoted to runnable observed
  automations and adoption refuses them before native disable.
- System `/Library/LaunchDaemons` are observation-only.
- launchd adoption is limited to the current user's `~/Library/LaunchAgents`;
  it snapshots the plist, verifies the current loaded state, uses the user GUI
  domain, and restores only a previously loaded agent.
- launchd runtime discovery queries `launchctl print` only for current-user
  LaunchAgents; system LaunchDaemons are not probed through the user domain.
- `auto scan` is read-only.
- Native adoption requires `--apply`, a matching fingerprint, a persisted
  snapshot, disable verification, and an exactly-one-owner proof.
- Observation scans reconcile against existing ownership: adopted/managed
  definitions are retained, fingerprint drift is recorded once and marked
  `needs_attention`, and a later scan cannot silently downgrade ownership.
- `pause`/`resume` operate only on managed/adopted automations; observed sources
  remain controlled by their native provider, and `needs_attention` cannot be
  cleared by a lifecycle shortcut.
- Drift acknowledgement requires an explicit dry-run/apply choice, updates the
  baseline only after apply, records an audit event, and leaves the automation
  paused until a separate resume.
- Secrets are not accepted as a special capability and must not be placed in
  YAML, SQLite event payloads, logs or fixtures.
- Executor-captured stdout/stderr redacts configured environment values, and
  immutable Run snapshots retain environment key names while replacing values
  with `[REDACTED]`.
- Cancellation is only accepted for an active process owned by the current
  daemon; a completed Run cannot be rewritten as cancelled.
- MCP `run_cancel` uses the same active-run registry and cannot target an
  arbitrary PID or another daemon's Run.
- The MCP server exposes only fixed Registry tools; it cannot run arbitrary
  shell/argv or resolve approvals.
- Policy `budget.max_steps` is enforced before a Run starts; exceeding it never
  partially executes the definition.
- Policy retries are bounded by `retry.max_attempts` and
  `retry.max_backoff_seconds`; the default is one attempt with no delay.
  Cancellation, approval failures, and policy failures are fail-closed and are
  not retried.
- GitHub polling uses fixed `gh --json` argv templates, validates `owner/name`,
  and records only local snapshots/events. It has no GitHub write operation.
- Homebrew observation uses only `brew services list --json`; it never calls
  `start`, `stop`, `restart`, `cleanup`, or `--sudo`, and matched plist paths
  reuse the existing launchd identity.
- `systemd --user` discovery scan is observation-only; the macOS binary treats a
  missing `systemctl` as an empty provider rather than attempting a fallback.
  Adoption is limited to explicitly enabled `.service` units and still requires
  the explicit `--apply` gate, fingerprint match, disable verification, and
  rollback journal.
- The Codex App Server CLI defaults to a read-only sandbox and declines approval
  requests. Workspace-write requires `--interactive-approvals`; requests are
  persisted in the Registry, bounded by risk, and fail closed on timeout.
- The desktop client can resolve only pending approvals through the explicit
  `approval.approve` and `approval.reject` RPC methods; it cannot set arbitrary
  approval states or execute commands directly.
- Event history is exposed read-only with bounded `events.list` results; event
  payloads remain local audit data and are not treated as agent instructions.
- Approval requests and resolutions append local audit events so the decision
  chain is visible without granting agents approval authority.
- `NoPrivilegedHelper` refuses all system-write operations. A future privileged
  implementation must remain typed and allowlisted; generic root command
  execution is out of scope.

## Reporting

Do not disclose vulnerabilities in public issues. Until a private reporting
endpoint is configured, contact the project maintainer directly with a minimal
reproduction and do not include credentials or private automation definitions.
