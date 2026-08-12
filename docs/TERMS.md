# Taskrail terms of use

Last updated: 2026-08-12

Taskrail is open-source software licensed under the [Apache License 2.0](../LICENSE).
These terms apply to use of the Taskrail repository, the local CLI/daemon/MCP
adapter, and any future Taskrail-hosted deployment.

## Use and responsibility

Taskrail runs commands and reads local scheduler and repository state on a
machine chosen by the user. You are responsible for the host, credentials,
files, repositories, and automations that you connect to it. Use only hosts
and third-party accounts you are authorized to access.

The local profile is intended for a private, user-owned connection. The public
review profile is read-only. Any future hosted endpoint must authenticate the
user and bind each request to an authorized host before exposing tool results.
Do not expose a local full-profile MCP process through an unauthenticated
public URL.

Taskrail does not authorize bypassing operating-system permissions, scheduler
controls, repository access controls, or third-party terms. Typed actions and
approval records are safety boundaries, not a guarantee that a command or
third-party tool is harmless. Review the target, arguments, dry-run result,
and affected data before approving a write or destructive action.

## Availability and changes

The software is provided as-is. Interfaces, integrations, and future hosted
services may change. The repository's [security policy](../SECURITY.md) and
[support page](SUPPORT.md) describe how to report problems and request help.

## Governing documents

Your use of ChatGPT or OpenAI services is also subject to the applicable OpenAI
terms and policies. The [privacy policy](PRIVACY.md) explains the data handled
by Taskrail. If these repository terms conflict with a separately agreed
written service agreement for a future hosted deployment, that agreement
controls for that deployment.
