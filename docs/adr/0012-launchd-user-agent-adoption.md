# ADR 0012: Adopt only current-user launchd agents

## Status

Accepted for the local adoption engine.

## Decision

`LaunchdController` supports only plist files under the current user's
`~/Library/LaunchAgents`:

- snapshot the plist bytes and fingerprint, verify the label, and record whether
  `launchctl print gui/<uid>/<label>` succeeds;
- use `launchctl bootout` only for an agent observed as loaded;
- verify the target is no longer printable before committing ownership;
- restore only a previously loaded agent with `launchctl bootstrap`.

System LaunchDaemons, `/Library/LaunchAgents`, arbitrary plist paths, and generic
launchctl arguments remain outside the controller boundary.

## Consequences

The existing transactional adoption engine now covers cron, user launchd
agents, and systemd user services. A failed disable or ownership proof still
uses the same rollback path; no root or privileged helper is implied.

Discovery uses `launchctl print` for current-user LaunchAgents so `enabled`
reflects loaded runtime state. System-level roots retain their observation-only
plist state and are not queried through the user GUI domain.

As a cross-provider invariant, a discovered command containing an explicit shell
invocation is neither promoted to a runnable observed automation nor adopted;
this check runs before any native scheduler mutation.
