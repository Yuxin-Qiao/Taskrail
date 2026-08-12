# Public Taskrail deployment

This directory provides a reproducible single-host deployment for the public
read-only MCP profile. It runs the local daemon and the stateless HTTP adapter
as separate, non-root containers sharing only the registry volume and Unix
socket. The container deployment is ARM64-only (`linux/arm64`), matching the
official Taskrail release targets.

## Local deployment smoke test

From the repository root:

```bash
export TASKRAIL_MCP_BEARER_TOKEN="$(openssl rand -hex 32)"
docker compose -f deploy/docker-compose.public.yml up -d --build
docker compose -f deploy/docker-compose.public.yml --profile smoke run --rm taskrail-healthcheck
```

The compose file intentionally uses `expose`, not `ports`, so the HTTP
adapter is not directly published to the host. The smoke profile runs a
temporary in-network curl container. Attach a TLS reverse proxy to the
compose network and use `deploy/Caddyfile.example` as a starting point.
Docker is not required for the local Rust validation suite; this compose sample
must be validated with Docker Compose on the deployment host before it is used.

## Production requirements

The sample is one isolated Taskrail host, not a shared multi-tenant service.
For a public ChatGPT submission, the deployment operator must additionally:

- expose a stable HTTPS URL whose `/mcp` path reaches the adapter;
- terminate TLS and configure OAuth 2.1/OIDC or the selected MCP-compatible
  authentication layer at the edge;
- bind an authenticated reviewer/user to the intended host and never route a
  user's request to another user's registry or Unix socket;
- keep `TASKRAIL_MCP_BEARER_TOKEN` in a secret manager and inject it into both
  the edge and the HTTP container without logging it;
- retain `/data` backups, restrict network access to the daemon, and collect
  request/error logs and scrape the authenticated internal `/metrics` endpoint
  without request bodies or authorization headers; and
- give reviewers a non-MFA test account or fixture that produces deterministic
  read-only data.

Do not expose port 8787 directly, use a localhost URL in the submission, or
treat a development tunnel as the production endpoint. The static bearer
token is only the reverse-proxy-to-process boundary; it is not sufficient
end-user authentication by itself.

For a private Fleet target that needs write or run tools, deploy a separate
single-host adapter with `mcp-http --profile private`, its own bearer secret,
and a private TLS/authenticated edge. Do not change the public compose sample
to private mode and do not share one private endpoint across tenants.

The repository's OpenAI checklist is in
[`docs/OPENAI_SUBMISSION.md`](../docs/OPENAI_SUBMISSION.md). The official
deployment guidance is in the [OpenAI MCP server documentation](https://developers.openai.com/plugins/build/mcp-server).
