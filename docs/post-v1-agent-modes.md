# Post-V1 Agent Modes

This page opens the post-V1 track. It is intentionally design-only for now: no runtime behavior, no permissions change, and no autonomous action loop.

## Safety Gate

Before any post-V1 mode ships, the project needs explicit safety contracts for:

- allowed local reads;
- allowed writes;
- human approval points;
- audit events;
- timeout and rate limits;
- provider failure behavior;
- hard-denied actions.

V1 restrictions remain the default: no job execution, no cancellation, no rerun, no shell execution, no secrets, no notifications, and no external APIs by default.

## Watch

Goal: periodically inspect local RunFlow state and produce passive summaries.

Detailed design: [watch-design.md](watch-design.md).

Initial scope:

- read `.flow/jobs`, `.flow/runs`, `.flow/agent/drafts`, and `logs/`;
- reuse `inspect-workspace --health`;
- write local summary files only when explicitly configured;
- never trigger RunFlow actions.

Open decisions:

- polling interval;
- summary retention;
- whether watch output belongs in `.flow/agent/` or stdout only.

## Oncall

Goal: prepare human handoff material for failed or unstable runs.

Detailed design: [oncall-design.md](oncall-design.md).

Initial scope:

- group incidents by job and status;
- reuse `explain-run` evidence;
- produce manual next steps;
- no Slack, email, webhook, or paging by default.

Open decisions:

- incident grouping window;
- severity mapping;
- export format.

## Autopilot

Goal: not approved yet.

Autopilot would require a separate permission model, explicit user opt-in, dry-run previews, audit hardening, and rollback design. Until those exist, autopilot remains out of scope.

Hard requirements before implementation:

- explicit config gate;
- per-action allowlist;
- dry-run first;
- human confirmation for mutations;
- test coverage for denied actions;
- visible audit trail for every proposed and approved action.
