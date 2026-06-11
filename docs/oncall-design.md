# Oncall Mode Design

Status: implemented post-V1 baseline.

`oncall` prepares human handoff material for failed or unstable RunFlow activity. It is a local evidence pack generator, not an incident responder. It must not execute jobs, cancel runs, rerun steps, execute shell commands, send notifications, call webhooks, or contact external APIs by default.

## Goals

- Turn failed and unstable run data into a compact handoff for a human.
- Reuse existing local readers, `inspect-workspace --health`, `explain-run`, and report evidence.
- Group related incidents by job, status, and recent time window.
- Produce manual next steps that never mutate RunFlow state.
- Keep all oncall activity auditable under `.flow/agent/audit.jsonl`.
- Keep output deterministic enough for tests and review.

## Non-Goals

- No paging.
- No Slack, email, webhook, or external notification delivery.
- No automatic remediation.
- No job execution, cancellation, or rerun.
- No shell execution.
- No secrets extraction.
- No background daemon installation.
- No hidden state outside the selected workspace.

## Proposed CLI

Design target only:

```powershell
runflow-agent oncall --root . --window-hours 24
runflow-agent oncall --root . --run-id run-123
runflow-agent oncall --root . --job backup --window-hours 24
runflow-agent oncall --root . --format json
runflow-agent oncall --root . --output .\.flow\agent\oncall\handoff.json
```

Current implementation ships a single-shot command only. If continuous oncall monitoring is needed later, it should build on `watch` snapshots rather than adding a second polling loop.

## Inputs

Local reads only:

- `.flow/jobs`;
- `.flow/runs`;
- `.flow/agent/drafts`;
- `logs/`;
- `.flow/agent/audit.jsonl`;
- explicit `--run-id`, `--job`, and `--window-hours` filters.

No provider or network access is required for the baseline. If a model-assisted summary is added later, it must use the same strict JSON repair and safety contract as existing agent commands.

## Output Contract

JSON output should use this shape:

```json
{
  "kind": "oncall_handoff",
  "root": ".",
  "generated_at": "unix-seconds",
  "scope": {
    "window_hours": 24,
    "run_id": null,
    "job": null
  },
  "summary": {
    "incidents": 1,
    "failed_runs": 1,
    "error_runs": 0,
    "affected_jobs": 1,
    "health_warnings": 2
  },
  "incidents": [
    {
      "id": "run-123",
      "job": "backup",
      "status": "FAILED",
      "started_at": "2026-06-10T00:00:00Z",
      "failed_step": "upload",
      "severity": "high",
      "evidence": [
        "manifest status FAILED",
        "failed_step upload",
        "stderr.log present"
      ],
      "manual_next_steps": [
        "Review logs for run run-123.",
        "Inspect the workflow definition before any rerun."
      ]
    }
  ],
  "health": [
    {
      "severity": "warn",
      "path": ".flow/runs/run-123/manifest.json",
      "message": "failed run manifest has no failed_step"
    }
  ],
  "handoff": {
    "title": "1 failed RunFlow run needs review",
    "body": "backup failed at upload. Review run run-123 logs and workflow before taking action."
  }
}
```

Text output should be a compact equivalent of the same data.

## Severity Mapping

Baseline mapping:

- `critical`: repeated failures for the same job inside the selected window.
- `high`: `FAILED` or `ERROR` run with failed step evidence.
- `medium`: failed or error run without failed step evidence.
- `low`: health warning without a failed run.
- `info`: clean handoff with no incidents.

Severity is advisory only. It must not trigger notifications or actions.

## Manual Next Steps

Allowed language:

- review logs;
- inspect manifest;
- compare recent runs;
- validate workflow definition;
- prepare a human escalation note.

Denied language:

- run;
- rerun;
- cancel;
- restart;
- execute;
- call webhook;
- send alert;
- page user.

If an output needs to mention a future mutation, it must phrase it as a human decision outside the agent, for example: "Ask the operator whether a rerun is appropriate."

## Audit

Every oncall handoff should append one audit line:

```json
{
  "command": "oncall",
  "status": "success",
  "changed_files": [],
  "warnings": ["run-123 FAILED"]
}
```

If `--output` writes a file, that path should appear in `changed_files`.

## Safety Rules

Hard deny:

- any automatic RunFlow action;
- shell execution;
- secrets access;
- outbound network calls;
- notification delivery;
- background monitoring.

Allowed:

- bounded local reads;
- stdout output;
- explicit `--output` writes under user-selected paths;
- audit writes under `.flow/agent/audit.jsonl`.

## Implementation Status

- Done: `oncall` command routing and help text.
- Done: reuse `inspect_workspace::inspect` as the baseline evidence source.
- Done: incident builder for failed and error runs.
- Done: optional filters: `--run-id`, `--job`, `--window-hours`.
- Done: text and JSON output.
- Done: explicit `--output` support and audit writes.
- Done: tests for grouping, severity, filters, strict JSON, denied language, and output writes.
- Later: continuous oncall mode, only if built on `watch` snapshots.

## Tests

Required tests:

- empty workspace returns a valid handoff with no incidents;
- failed run appears in `incidents`;
- repeated job failures produce `critical` severity;
- `--run-id` filters to one incident;
- `--job` filters by job name;
- `--format json` returns strict JSON;
- `--output` writes only the requested file;
- audit includes warnings for failed or error runs;
- denied action language never appears in manual next steps.

## Open Decisions

- Default `--window-hours` value.
- Whether `ERROR` should always outrank `FAILED`.
- Whether handoff output belongs under `.flow/agent/oncall/` by convention or stdout only.
- Whether a later model-assisted handoff summary is worth the extra provider dependency.
