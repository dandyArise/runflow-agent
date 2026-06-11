# RunFlow Agent V1 Complete / Roadmap

## V1 Complete

V1 is assist-only and local-first. It can read local RunFlow state, produce validated suggestions, and write explicit user-requested outputs. It must not run jobs, cancel runs, rerun steps, execute shell commands, manage secrets, send notifications, or call external APIs by default.

Completed V1 surface:

- `doctor`: provider, workspace, audit, and deny-by-default checks.
- `inspect-workspace`: local job, draft, run, and workspace health inventory.
- `draft`: schema-valid workflow draft generation.
- `review`: workflow schema and risk review without editing files.
- `explain-run`: local failed-run explanation from manifests, events, metadata, stdout, and stderr.
- `report daily`: local run summary and manual recommendations.
- `self version` / `self update`: release-aware binary inspection and update.
- Release smoke coverage for Windows archives and install scripts.

## V1 Health Coverage

`inspect-workspace --health` checks:

- missing `.flow/jobs`, `.flow/agent/drafts`, and `.flow/runs` directories;
- missing `logs/` when runs exist;
- invalid registered job workflow YAML;
- invalid draft workflow YAML;
- drafts without matching registered jobs;
- missing or invalid run manifests;
- run manifests without `job_name` or `status`;
- failed/error runs without `failed_step`;
- run references to unknown jobs;
- failed/error runs without stdout/stderr logs;
- log directories without matching runs.

## Outside V1

These modes are intentionally not part of V1:

- `watch`: passive monitoring loop.
- `oncall`: escalation-oriented summaries and handoff workflows.
- `autopilot`: any autonomous remediation or action loop.
- direct `run`, `cancel`, or `rerun` actions by the agent.
- shell execution, notifications, webhooks, and external APIs by default.

## Next Branch

Use a separate branch for any post-V1 exploration:

```powershell
git switch -c codex/post-v1-agent-modes
```

Keep the first post-V1 work scoped to design docs and safety contracts before adding runtime behavior.
