# RunFlow Agent

Assist-only external agent for [RunFlow](https://github.com/dandyArise/runflow).

RunFlow Agent reads local RunFlow data, produces validated suggestions, and leaves execution under user control. It does not run jobs, cancel runs, rerun steps, send alerts, manage secrets, call external APIs, or execute shell commands.

## Features

- `runflow-agent draft`: generate a schema-shaped RunFlow workflow YAML draft from a short request.
- `runflow-agent review`: validate a workflow file against the embedded RunFlow schema and report risky patterns without editing it.
- `runflow-agent explain-run`: explain a run from `.flow/runs/<run_id>`, `logs/<run_id>`, events, metadata, stdout/stderr excerpts, and likely cause hints.
- `runflow-agent report daily`: summarize local RunFlow run activity for a time window.
- `runflow-agent doctor`: check the selected LLM provider and local RunFlow Agent workspace wiring.
- Local audit trail in `.flow/agent/audit.jsonl`.
- Output contract files under `schemas/`.
- Strict model-output decoding with bounded JSON repair for LLM-backed commands.
- Draft validation and bounded repair pass when an LLM returns schema-invalid YAML.

## Providers

Default provider is `mock`, which is deterministic and does not call a model.

```powershell
runflow-agent draft --prompt "Ping 1.1.1.1" --provider ollama --model qwen2.5-coder:1.5b
runflow-agent draft --prompt "Ping 1.1.1.1 every 5 minutes" --provider openai-compatible --base-url http://localhost:1234/v1 --model qwen/qwen3-coder-30b --timeout-seconds 120
```

See [docs/providers.md](docs/providers.md).

## Build

```powershell
cargo build
```

## Install

```powershell
.\scripts\install-local.ps1
```

See [docs/install.md](docs/install.md).

## Demo

```powershell
.\demo\demo.ps1
```

See [docs/demo.md](docs/demo.md).

## Usage

```powershell
runflow-agent doctor
runflow-agent doctor --provider openai-compatible --base-url http://localhost:1234/v1 --model qwen/qwen3-coder-30b --timeout-seconds 120
runflow-agent draft --prompt "Ping 1.1.1.1 every 5 minutes"
runflow-agent draft --prompt "Backup logs" --output .\.flow\agent\drafts\backup-logs.yml
runflow-agent review .\.flow\agent\drafts\backup-logs.yml
runflow-agent review .\.flow\agent\drafts\backup-logs.yml --format json
runflow-agent explain-run <run_id>
runflow-agent report daily --format json
```

## Safety

V1 is deny-by-default:

- no autopilot;
- no automatic remediation;
- no direct shell execution;
- no job execution, cancellation, or rerun;
- no webhooks, email, Slack, or external API calls by default;
- no secret management.

## Status

This repository currently contains a local MVP with `mock`, `ollama`, and `openai-compatible` providers. Model outputs are decoded as strict JSON and rejected when the expected `kind` or required fields are missing. Generated workflow YAML is validated against the embedded RunFlow workflow schema.
