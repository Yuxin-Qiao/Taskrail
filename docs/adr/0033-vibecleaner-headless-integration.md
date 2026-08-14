# ADR 0033: VibeCleaner headless scan integration

## Status

Accepted

## Context

[VibeCleaner](https://vibecleaner.app/) is a local macOS developer-cache
cleaner. Its public distribution is a GUI app that scans regenerable developer
artifacts, labels them `safe` or `verify`, and lets the user decide what to
move to the Trash. The GUI does not provide a stable Taskrail automation
boundary.

The [public source implementation](https://github.com/pooran/vibecleaner)
documents a machine-readable scan contract:

```text
python source/vibecleaner.py --cli <directory>... --json
```

It reports `total_folders`, `total_bytes`, and folder records with paths,
ecosystems, categories, and risk labels. Taskrail needs to preserve that
meaning without turning a cache cleaner into an arbitrary filesystem command.

## Decision

Taskrail adds a `vibecleaner` semantic integration with one action, `scan`.
The adapter:

- accepts an explicit, bounded `directories` array and optional `min_size_mb`;
- invokes a configured `vibecleaner` wrapper, or a Python interpreter plus the
  source script when `TASKRAIL_VIBECLEANER_SCRIPT` is set, through direct argv
  only; the interpreter defaults to `python3` and can be overridden with
  `TASKRAIL_VIBECLEANER_PYTHON`;
- requests `--cli --json` and parses the bounded JSON report into reclaimable
  byte/count metrics and bounded `verify`/unknown-risk findings;
- treats all scans as read-only and never exposes a cleanup action, GUI click
  path, or arbitrary folder deletion capability;
- reports a missing headless CLI as unavailable instead of claiming that an
  installed GUI DMG is automation-ready;
- exposes the same typed action through the local RPC, CLI, MCP, and Fleet
  surfaces, with the public MCP profile limited to this read-only scan tool.

## Consequences

Users can schedule or invoke VibeCleaner scans from Taskrail and inspect the
same run, event, metric, and log records used by other integrations. A scan
does not free space by itself; cleanup remains an explicit action in the
VibeCleaner GUI, preserving the upstream product's user confirmation and
avoiding an invented destructive CLI contract. If VibeCleaner later publishes
a stable headless cleanup API, it must receive a separate decision covering
path safety, approvals, post-state verification, and audit semantics.
