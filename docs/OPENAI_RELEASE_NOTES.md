# Taskrail initial plugin submission release notes

Taskrail is an MCP-only developer tool that gives ChatGPT a bounded,
read-only view of a user-authorized macOS, Linux, or Windows automation host. The initial
submission includes native scheduler discovery, automation inventory, run
history and bounded logs, attention items, audit-event summaries, and
normalized local integration findings.

The public review profile is intentionally narrower than the private local
profile. It advertises 17 read-only tools, rejects hidden write/execution
tools, redacts configured environment values and native raw definitions, and
does not mutate the Registry during discovery. The repository now includes a
stateless Streamable HTTP `/mcp` adapter, bounded request parsing, origin and
Bearer boundary checks, request audit logs that omit authorization headers and
request bodies, a container deployment sample, and public privacy, terms, and
support pages.

Reviewers should use the production HTTPS endpoint and the non-MFA fixture
provided in the submission portal. The local Docker Compose sample is only a
single-host deployment reference; it is not the public endpoint and must be
placed behind the required TLS and user-authentication edge before review.
