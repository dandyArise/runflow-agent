# RunFlow Agent

[![CI](https://github.com/dandyArise/runflow-agent/actions/workflows/ci.yml/badge.svg)](https://github.com/dandyArise/runflow-agent/actions/workflows/ci.yml)

Assist-only external agent for [RunFlow](https://github.com/dandyArise/runflow).

RunFlow Agent reads local RunFlow data, produces validated suggestions, and leaves execution under user control. It does not run jobs, cancel runs, rerun steps, send alerts, manage secrets, call external APIs, or execute shell commands.

## Features

- `runflow-agent draft`: generate a schema-shaped RunFlow workflow YAML draft from a short request.
- `runflow-agent review`: validate a workflow file against the embedded RunFlow schema and report risky patterns without editing it.
- `runflow-agent explain-run`: explain a run from `.flow/runs/<run_id>`, `logs/<run_id>`, events, metadata, stdout/stderr excerpts, and likely cause hints.
- `runflow-agent report daily`: summarize local RunFlow run activity for a time window.
- `runflow-agent doctor`: check the selected LLM provider and local RunFlow Agent workspace wiring.
- `runflow-agent inspect-workspace`: inventory local RunFlow jobs, agent drafts, recent runs, and recommended manual follow-ups.
- `runflow-agent self version` / `self update`: inspect and update the installed binary from GitHub releases.
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

From GitHub release:

```powershell
iwr https://github.com/dandyArise/runflow-agent/releases/latest/download/install.ps1 -UseBasicParsing | iex
```

Linux/macOS:

```sh
curl -fsSL https://github.com/dandyArise/runflow-agent/releases/latest/download/install.sh | sh
```

Update an installed binary:

```powershell
runflow-agent self update
```

From source:

```powershell
.\scripts\install-local.ps1
```

See [docs/install.md](docs/install.md).

## Demo

```powershell
.\demo\demo.ps1
```

See [docs/demo.md](docs/demo.md).
See [docs/v1-roadmap.md](docs/v1-roadmap.md) for V1 completion status and post-V1 boundaries.

## Usage

V1 command set:

- `doctor`: local/provider health checks.
- `inspect-workspace`: local workspace inventory and optional health checks.
- `draft`: generate a workflow draft without registering or running it.
- `review`: validate and review workflow YAML without editing it.
- `explain-run`: explain one local run from local evidence.
- `report daily`: summarize local run history.
- `self`: inspect or update the installed binary.

```powershell
runflow-agent doctor
runflow-agent doctor --provider openai-compatible --base-url http://localhost:1234/v1 --model qwen/qwen3-coder-30b --timeout-seconds 120
runflow-agent inspect-workspace
runflow-agent inspect-workspace --health
runflow-agent inspect-workspace --format json
runflow-agent draft --prompt "Ping 1.1.1.1 every 5 minutes"
runflow-agent draft --prompt "Backup logs" --output .\.flow\agent\drafts\backup-logs.yml
runflow-agent review .\.flow\agent\drafts\backup-logs.yml
runflow-agent review .\.flow\agent\drafts\backup-logs.yml --format json
runflow-agent explain-run <run_id>
runflow-agent report daily --format json
runflow-agent self version
runflow-agent self update --dry-run
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

This repository contains the V1 assist-only MVP with `mock`, `ollama`, and `openai-compatible` providers. Model outputs are decoded as strict JSON and rejected when the expected `kind` or required fields are missing. Generated workflow YAML is validated against the embedded RunFlow workflow schema. Local jobs, drafts, run history, workspace health, audit wiring, install scripts, and Windows release assets have smoke coverage.
