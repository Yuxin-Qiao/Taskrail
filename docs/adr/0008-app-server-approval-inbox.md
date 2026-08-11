# ADR 0008: App Server approvals use the local Registry Inbox

## Status

Accepted for the V0.3 implementation.

## Decision

Dynamic App Server approval requests are normalized into the existing
`ApprovalRequest` model:

- command execution and file changes map to `R1_WORKSPACE_WRITE`;
- MCP elicitation maps to `R2_EXTERNAL_WRITE`;
- permission changes map to `R3_SYSTEM_WRITE`.

The `RegistryApprovalHandler` stores a redacted scope plus a request fingerprint,
then waits for the existing `auto approve` or `auto reject` command. A request
above the handler's configured risk ceiling is immediately recorded as rejected.
Timeouts become `expired` and respond with `decline`. The default handler remains
`AutoDeclineApprovalHandler`, so no caller silently gains write access.

## Consequences

The CLI and future SwiftUI client can share one auditable approval source. A
pending App Server turn is intentionally coupled to a live local process; the
Registry records the decision and audit trail, but does not attempt to resume a
dead process or grant approval to a different request.
