# Autopilot Apply Design

Status: minimal local-draft apply implemented.

`autopilot apply` is intentionally narrow. It can apply only one safe action: write a local draft file under `.flow/agent/drafts/`. It must not execute RunFlow jobs, rerun steps, cancel runs, execute shell commands, send notifications, call webhooks, access secrets, or mutate registered workflows.

## Current Scope

Implemented action:

- `write_local_draft`

Denied actions:

- `run_job`;
- `cancel_run`;
- `rerun_step`;
- `edit_workflow`;
- `write_config`;
- `send_notification`;
- `call_webhook`;
- `execute_shell`;
- any unknown action.

## CLI

```powershell
runflow-agent autopilot apply --root . --proposal .\.flow\agent\autopilot\proposal.json --proposal-id <id> --confirm <token>
runflow-agent autopilot apply --root . --proposal .\.flow\agent\autopilot\proposal.json --proposal-id <id> --confirm <token> --format json
```

`--proposal-id` is required. `--confirm` is required.

The current confirmation token format is:

```text
approve:<proposal-id>:write_local_draft
```

The token is explicit human confirmation, not a secret. It is scoped to one proposal ID and one action. Reuse is prevented by refusing to overwrite the draft file once it exists.

## Proposal Requirements

The proposal file must be JSON with:

- `kind: "autopilot_proposal"`;
- `mode: "dry_run"`;
- a matching proposal in `proposals[]`;
- `action: "write_local_draft"`;
- `preview.would_run: []`;
- `draft.path` under `.flow/agent/drafts/`;
- `draft.content`.

Any proposal that does not meet those requirements fails closed.

## Write Rules

Allowed write:

- one local draft file under `.flow/agent/drafts/`.

Denied writes:

- absolute paths;
- paths with `..`;
- paths outside `.flow/agent/drafts/`;
- overwriting an existing draft;
- writing registered workflow files under `.flow/jobs/`;
- writing config, secrets, scripts, or logs.

## Audit

Every successful apply appends:

```json
{
  "command": "autopilot apply",
  "status": "success",
  "changed_files": [".flow/agent/drafts/autopilot-<id>.json"],
  "warnings": ["<id> applied"]
}
```

Failures do not write drafts.

## Rollback

Rollback for the current action is manual and simple:

```text
Delete the generated local draft file.
```

No RunFlow state is executed or changed, so no job rollback is required.

## Tests

Required coverage:

- valid token writes one draft;
- invalid token is rejected;
- second apply refuses overwrite;
- non-allowlisted action is rejected;
- proposal must be `kind=autopilot_proposal`;
- proposal must be `mode=dry_run`;
- `preview.would_run` must be empty;
- draft path must stay under `.flow/agent/drafts/`;
- audit records changed file.

## Future Work

Before any stronger apply action exists, add a new design and tests for:

- approval token generation;
- token expiration;
- per-action allowlist;
- before/after audit;
- rollback plan;
- operator review UX.

No job execution, rerun, cancel, shell, webhook, notification, or registered workflow mutation should be added without a separate design branch.
