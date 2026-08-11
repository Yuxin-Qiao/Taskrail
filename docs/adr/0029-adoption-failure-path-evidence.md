# ADR 0029: Adoption failure paths preserve a single owner

## Decision

Adoption tests inject failures after native disable, during disabled-state
verification, during rollback restore, and at the ownership proof. The expected
outcomes are explicit:

- a verified rollback restores the native source and leaves the automation
  `observed` plus `needs_attention`;
- a failed rollback returns an error and records `needs_attention`, never a
  false `committed` state;
- a duplicate fingerprint is rejected by the exactly-one-owner proof and the
  native source is restored.

These tests model the transaction checkpoints without touching a real user's
launchd or crontab. Real controllers retain the same snapshot, disable,
verify, restore, and journal sequence. Any future adoption checkpoint must add
the corresponding injected failure-path assertion before it can be enabled.
