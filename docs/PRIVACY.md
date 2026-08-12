# Taskrail privacy policy

Last updated: 2026-08-12

Taskrail is a local-first automation control plane. This policy describes the
repository software, its local MCP adapter, and the read-only profile prepared
for a future public ChatGPT app submission. The current repository does not
operate a hosted Taskrail service or collect telemetry by default.

## Data handled

When a user asks Taskrail to inspect a host, the local daemon may read and
store:

- automation metadata such as an identifier, name, scheduler/provider,
  executable, arguments, working directory, trigger, enabled state, and a
  native source path;
- a stable random `host_id` generated and stored in the local Registry, host
  operating-system and architecture information, plus the optional
  `TASKRAIL_HOST_LABEL` chosen by the user;
- run status, timestamps, exit codes, bounded stdout/stderr, audit events, and
  normalized integration findings;
- read-only GitHub observations returned by the user's local `gh` client, or
  local package and security scanner results, when the user explicitly asks
  for those checks.

The MCP adapter does not return native raw definitions, raw scanner matches,
or environment values. It redacts the current home-directory prefix in paths
and redacts configured environment values in automation snapshots. Inputs such
as `repository_env`, `password_env`, and `config_env` are environment-variable
names only; they are not credential fields.

Some typed integrations need the local process to read a credential from the
environment in order to perform a user-requested operation. The credential
value is not stored in the Registry or returned as MCP structured content.
Users should still avoid commands that echo secrets: arbitrary command output
is user-controlled, and the local run-log model cannot guarantee removal of a
secret that a command deliberately prints.

## Why and where data is used

Taskrail uses this data to discover local jobs, show status, explain failures,
produce typed integration summaries, and maintain a local audit trail. The
default local profile communicates with the daemon through a user-only Unix
socket and does not open a network listener.

When a user connects Taskrail to ChatGPT, selected tool inputs and outputs are
processed by ChatGPT/OpenAI as part of that user's conversation and according
to the applicable OpenAI terms and privacy controls. Taskrail does not sell
data, use it for advertising, or add a separate analytics service.

The optional `taskrail mcp-fleet` gateway keeps a local inventory of named MCP
endpoints and token environment-variable names. It never stores token values.
Each remote host is queried explicitly by `host_id`; remote Registry data,
policies, approvals, and run results remain authoritative on that host.

The future public deployment described in the submission checklist must add
authentication and per-user host binding before proxying to a user's daemon.
That deployment may receive account/host-binding information and the data
needed to answer an authenticated tool call; its operator must publish any
additional retention or subprocessors before launch. A public unauthenticated
relay is not a supported Taskrail deployment.

## Retention and controls

The local Registry, run logs, and audit events remain on the user's machine
until the user removes them or changes the configured XDG data directory. Stop
the daemon before deleting the Registry directory. The local socket and its
runtime directory are created with restrictive user-only permissions where the
platform supports them.

Users control which host is connected, which automations are stored, and which
logs are requested. Native scheduler definitions remain observation-only until
the user explicitly performs a local adoption operation. The public review
profile is read-only and cannot create, modify, delete, adopt, pause, resume,
run, cancel, or approve work.

## Restricted data

Taskrail is not designed to request payment-card numbers, health information,
government identifiers, MFA codes, private keys, or service passwords as tool
arguments. Do not put such values in an automation definition, issue report, or
chat prompt.

## Contact

For privacy questions, use [Taskrail support](SUPPORT.md). Security reports
should follow the private process in [SECURITY.md](../SECURITY.md) and must not
include credentials or private automation definitions in a public issue.
