# ADR 0010: Homebrew Services enrich native identities

## Status

Accepted for the V0.3 implementation.

## Decision

`HomebrewProvider` calls only `brew services list --json` and parses service
metadata as observation data. It does not own service lifecycle. During a full
scan, a Homebrew entry whose plist path matches a discovered launchd source
enriches that source's raw observation and keeps the launchd `source_id`.

If no matching plist exists, the provider records a `homebrew:<formula>` service
source without an executable command. Such a source is inspectable but cannot
become an executable observed automation automatically.

## Consequences

Homebrew services do not appear twice merely because Homebrew and launchd expose
two views of the same plist. Adoption remains unavailable for Homebrew-only
sources, and no Homebrew lifecycle mutation is hidden behind `scan`.
