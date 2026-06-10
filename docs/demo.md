# RunFlow Agent Demo

This demo is local and assist-only. It creates a temporary workspace with sample RunFlow run data, then runs:

- `draft`
- `review`
- `explain-run`
- `report daily`

## Run

```powershell
.\demo\demo.ps1
```

Use a specific binary:

```powershell
.\demo\demo.ps1 -Binary .\target\release\runflow-agent.exe
```

The demo keeps its temporary files and prints the workspace path at the end so the generated workflow, `.flow` data, logs, and saved agent outputs can be inspected.

## Expected Flow

1. A workflow draft is generated from `Ping 1.1.1.1 every 5 minutes` into `.flow/agent/drafts/ping-workflow.yml`.
2. The generated workflow is reviewed against the embedded RunFlow schema from that centralized draft path.
3. A simulated failed run is explained from:
   - `.flow/runs/demo-failed-run/manifest.json`
   - `.flow/runs/demo-failed-run/events.jsonl`
   - `logs/demo-failed-run/build/step.metadata.json`
   - `stdout.log`
   - `stderr.log`
4. A local daily report is printed as JSON.

The failed run intentionally contains `command not found: cargo-nextest`, so `explain-run` should surface a command/path likely-cause hint.

## Saved Outputs

The demo writes:

- `.flow/agent/drafts/ping-workflow.yml`: generated workflow draft.
- `demo-result.txt`: readable combined output.
- `demo-review.json`: structured review output.
- `demo-explain.json`: structured run explanation.
- `demo-report.json`: structured daily report.
