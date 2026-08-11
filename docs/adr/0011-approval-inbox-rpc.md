# ADR 0011: Approval Inbox writes are explicit and narrow

## Status

Accepted for the desktop approval flow.

## Decision

The Unix JSON-RPC daemon exposes exactly two approval mutations:

- `approval.approve { id, actor? }`
- `approval.reject { id, actor? }`

Both delegate to `Registry::resolve_approval`, which only transitions a pending
request. The SwiftUI client enables these actions only for pending rows and
refreshes the Inbox after a successful response.

## Consequences

Approval decisions can be made from the desktop client without granting it a
general Registry write API. Expired, already resolved, malformed, or unknown
requests fail through the existing JSON-RPC error path.
