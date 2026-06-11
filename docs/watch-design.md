# Watch Mode Design

Status: implemented post-V1 baseline.

`watch` is a passive local monitoring mode. It repeatedly inspects RunFlow state and writes or prints summaries for a human. It does not run jobs, cancel runs, rerun steps, execute shell commands, send notifications, call webhooks, or call external APIs by default.

## Goals

- Reuse V1 local readers and safety policy.
- Detect local workspace changes without mutating RunFlow state.
- Produce bounded summaries that a human can review.
- Keep all watch activity auditable under `.flow/agent/audit.jsonl`.
- Keep failure behavior boring: clear errors, bounded retries, no panic.

## Non-Goals

- No autopilot.
- No remediation.
- No shell execution.
- No job execution, cancellation, or rerun.
- No Slack, email, webhook, or external API integration.
- No background daemon installation.
- No hidden state outside the selected workspace.

## Proposed CLI

Design target only:

```powershell
runflow-agent watch --root . --interval-seconds 60
runflow-agent watch --root . --once
runflow-agent watch --root . --once --format json
runflow-agent watch --root . --output .\.flow\agent\watch\latest.json
```

Current implementation supports `--once` and continuous polling. State-based deduplication remains out of scope until it is needed.

## Inputs

Local reads only:

- `.flow/jobs/*.yml`
- `.flow/agent/drafts/*.yml`
- `.flow/runs/<run_id>/manifest.json`
- `.flow/runs/<run_id>/events.jsonl`
- `logs/<run_id>/**/stdout.log`
- `logs/<run_id>/**/stderr.log`
- `logs/<run_id>/**/step.metadata.json`
- `.flow/agent/audit.jsonl`

All reads must be bounded. Large logs should reuse the same excerpt limits as `explain-run`.

## Output Contract

Proposed JSON shape:

```json
{
  "kind": "watch_snapshot",
  "root": ".",
  "generated_at": "unix-seconds",
  "summary": {
    "jobs": 0,
    "drafts": 0,
    "runs": 0,
    "failed_runs": 0,
    "health_warnings": 0
  },
  "incidents": [
    {
      "run_id": "run-123",
      "job": "backup",
      "status": "FAILED",
      "failed_step": "upload",
      "hint": "Review with runflow-agent explain-run run-123"
    }
  ],
  "health": [
    {
      "severity": "warn",
      "path": ".flow/runs/run-123/manifest.json",
      "message": "failed run manifest has no failed_step"
    }
  ],
  "recommendations": [
    "Review failed runs with runflow-agent explain-run <run_id>."
  ]
}
```

Text output should be a compact equivalent of the same data.

## Loop Behavior

For continuous mode:

1. Read workspace snapshot.
2. Run health checks.
3. Build incident summary.
4. Print or write output.
5. Record audit line.
6. Sleep until next interval.

The loop must stop cleanly on Ctrl+C. A failed read should produce one bounded error per interval and continue unless the root itself becomes unreadable.

## State and Deduplication

Initial `--once` mode needs no state.

Continuous mode may maintain a local cache under:

```text
.flow/agent/watch/state.json
```

The cache may store:

- last snapshot timestamp;
- last seen run IDs;
- last emitted incident IDs;
- output schema version.

State must be optional. Deleting it must not break watch mode.

## Audit

Every watch snapshot should append one audit line:

```json
{
  "command": "watch",
  "status": "success",
  "changed_files": [],
  "warnings": ["run-123 failed"]
}
```

If `--output` writes a file, that path should appear in `changed_files`.

## Safety Rules

Hard deny:

- any request to run, cancel, rerun, or mutate jobs;
- any shell command execution;
- any secret value extraction;
- any outbound network call;
- any notification delivery.

Allowed:

- bounded local reads;
- stdout output;
- explicit `--output` writes under user-selected paths;
- audit writes under `.flow/agent/audit.jsonl`.

## Implementation Status

- Done: shared snapshot builder around existing `inspect-workspace` logic.
- Done: `watch --once` with text and JSON output.
- Done: explicit `--output` write path.
- Done: audit writes for each snapshot.
- Done: continuous polling with interval validation.
- Later: state cache only if deduplication is needed.

## Tests

Required tests:

- empty workspace returns a valid snapshot;
- failed run appears in `incidents`;
- health warning appears in `health`;
- `--format json` returns strict JSON;
- `--output` writes only the requested file;
- continuous interval rejects zero or invalid values;
- continuous mode can run bounded iterations in tests;
- denied action language never appears in recommendations.

## Open Decisions

- Minimum allowed interval.
- Whether continuous mode should default to stdout or require `--output`.
- Whether model-backed recommendations are allowed in watch mode or deterministic-only is required.
- Whether watch state belongs in `.flow/agent/watch/` or should stay stateless.
