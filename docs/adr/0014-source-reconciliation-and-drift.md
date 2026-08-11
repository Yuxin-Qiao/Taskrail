# ADR 0014: Observation scans preserve ownership and surface drift

## Status

Accepted for every discovery provider.

## Decision

Scanning a native source always refreshes the source record, then reconciles its
automation:

- a new direct-argv source becomes `observed`;
- an existing observed source may refresh its derived definition;
- an adopted or managed source keeps its ownership, definition, and expected
  fingerprint;
- a changed fingerprint marks the owned automation `needs_attention` and emits
  one `source.drifted` event until it is repaired;
- shell-invoking or otherwise unrunnable sources are not promoted and old
  observed records are marked `needs_attention`.

Adoption requires an observed, healthy automation, so a drifted or already-owned
source cannot be re-adopted accidentally.

`auto doctor drift` provides a read-only audit of `needs_attention` states and
owned-source fingerprint mismatches. It never changes a source or baseline.

After review, `auto acknowledge-drift <source-id> --apply` may update the
baseline. The automation is deliberately left paused and requires a separate
resume action.

## Consequences

Repeated polling is idempotent and cannot erase the evidence needed to diagnose
native changes. Repair remains an explicit operator action; discovery never
silently rewrites an adopted definition.
