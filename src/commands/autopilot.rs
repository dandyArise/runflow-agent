use std::fs;
use std::path::{Component, Path, PathBuf};

use serde_json::Value;

use crate::audit;
use crate::cli::CliResult;
use crate::commands::inspect_workspace;
use crate::json;

const DEFAULT_LIMIT: usize = 100;
const APPLY_ACTION: &str = "write_local_draft";

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("autopilot requires a subcommand: plan or apply".to_string());
    };

    match subcommand {
        "plan" => run_plan(&args[1..]),
        "apply" => run_apply(&args[1..]),
        other => Err(format!(
            "unknown autopilot subcommand '{other}'; available: plan, apply"
        )),
    }
}

fn run_plan(args: &[String]) -> Result<CliResult, String> {
    let options = PlanOptions::from_args(args)?;
    let proposal = build_proposal(&options)?;
    let output = if options.format_json {
        proposal.json()
    } else {
        proposal.text()
    };

    let mut changed_files = Vec::new();
    if let Some(path) = &options.output {
        write_output(path, &output, "autopilot proposal")?;
        changed_files.push(path.display().to_string());
    }

    let warnings = proposal
        .proposals
        .iter()
        .map(|proposal| format!("{} requires approval", proposal.id))
        .chain(
            proposal
                .blocked
                .iter()
                .map(|blocked| format!("blocked {}", blocked.reason)),
        )
        .collect::<Vec<_>>();
    let _ = audit::record_at(
        &options.root,
        "autopilot plan",
        "success",
        &changed_files,
        &warnings,
    );

    Ok(CliResult {
        command: "autopilot plan".to_string(),
        output,
        status: "success",
        changed_files,
        warnings,
        audit: false,
    })
}

fn run_apply(args: &[String]) -> Result<CliResult, String> {
    let options = ApplyOptions::from_args(args)?;
    let result = apply_proposal(&options)?;
    let output = if options.format_json {
        result.json()
    } else {
        result.text()
    };
    let warnings = vec![format!("{} applied", result.proposal_id)];
    let _ = audit::record_at(
        &options.root,
        "autopilot apply",
        "success",
        &result.changed_files,
        &warnings,
    );

    Ok(CliResult {
        command: "autopilot apply".to_string(),
        output,
        status: "success",
        changed_files: result.changed_files,
        warnings,
        audit: false,
    })
}

struct PlanOptions {
    root: PathBuf,
    limit: usize,
    from_watch: Option<PathBuf>,
    from_oncall: Option<PathBuf>,
    format_json: bool,
    output: Option<PathBuf>,
}

impl PlanOptions {
    fn from_args(args: &[String]) -> Result<Self, String> {
        let root = value_after(args, "--root").map(PathBuf::from).unwrap_or(
            std::env::current_dir().map_err(|e| format!("cannot read current dir: {e}"))?,
        );
        let limit = value_after(args, "--limit")
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| "--limit must be an integer".to_string())?
            .unwrap_or(DEFAULT_LIMIT);

        Ok(Self {
            root,
            limit,
            from_watch: value_after(args, "--from-watch").map(PathBuf::from),
            from_oncall: value_after(args, "--from-oncall").map(PathBuf::from),
            format_json: wants_json(args),
            output: value_after(args, "--output").map(PathBuf::from),
        })
    }
}

struct ApplyOptions {
    root: PathBuf,
    proposal: PathBuf,
    proposal_id: String,
    confirm: String,
    format_json: bool,
}

impl ApplyOptions {
    fn from_args(args: &[String]) -> Result<Self, String> {
        let root = value_after(args, "--root").map(PathBuf::from).unwrap_or(
            std::env::current_dir().map_err(|e| format!("cannot read current dir: {e}"))?,
        );
        let proposal = value_after(args, "--proposal")
            .map(PathBuf::from)
            .ok_or_else(|| "autopilot apply requires --proposal <path>".to_string())?;
        let proposal_id = value_after(args, "--proposal-id")
            .map(ToString::to_string)
            .ok_or_else(|| "autopilot apply requires --proposal-id <id>".to_string())?;
        let confirm = value_after(args, "--confirm")
            .map(ToString::to_string)
            .ok_or_else(|| "autopilot apply requires --confirm <token>".to_string())?;

        Ok(Self {
            root,
            proposal,
            proposal_id,
            confirm,
            format_json: wants_json(args),
        })
    }
}

struct AutopilotProposal {
    root: String,
    generated_at: String,
    source_watch: Option<String>,
    source_oncall: Option<String>,
    proposals: Vec<ProposalItem>,
    blocked: Vec<BlockedItem>,
}

struct ProposalItem {
    id: String,
    action: &'static str,
    target: String,
    status: &'static str,
    risk: &'static str,
    confidence: f32,
    reason: String,
    evidence: Vec<String>,
    would_change: Vec<String>,
    draft_path: String,
    draft_content: String,
    approval_token: String,
    rollback: Vec<String>,
}

struct BlockedItem {
    action: String,
    target: String,
    reason: String,
}

struct ApplyResult {
    proposal_id: String,
    action: String,
    changed_files: Vec<String>,
}

fn build_proposal(options: &PlanOptions) -> Result<AutopilotProposal, String> {
    let inspection = inspect_workspace::inspect(&options.root, options.limit)?;
    let watch_source = read_source_json(&options.from_watch, "watch")?;
    let oncall_source = read_source_json(&options.from_oncall, "oncall")?;
    let mut evidence_runs = runs_from_oncall(&oncall_source);
    evidence_runs.extend(runs_from_watch(&watch_source));
    evidence_runs.extend(
        inspection
            .runs
            .iter()
            .filter(|run| run.status == "FAILED" || run.status == "ERROR")
            .map(|run| EvidenceRun {
                id: run.id.clone(),
                job: run.job.clone(),
                status: run.status.clone(),
                failed_step: run.failed_step.clone(),
                evidence: vec![format!("workspace status {}", run.status)],
            }),
    );
    evidence_runs.sort_by(|a, b| a.id.cmp(&b.id).then_with(|| a.job.cmp(&b.job)));
    evidence_runs.dedup_by(|a, b| a.id == b.id);

    let proposals = evidence_runs
        .iter()
        .map(ProposalItem::from_run)
        .collect::<Vec<_>>();
    let mut blocked = blocked_from_source(&watch_source);
    blocked.extend(blocked_from_source(&oncall_source));

    Ok(AutopilotProposal {
        root: options.root.display().to_string(),
        generated_at: audit::timestamp_seconds(),
        source_watch: options
            .from_watch
            .as_ref()
            .map(|path| path.display().to_string()),
        source_oncall: options
            .from_oncall
            .as_ref()
            .map(|path| path.display().to_string()),
        proposals,
        blocked,
    })
}

fn apply_proposal(options: &ApplyOptions) -> Result<ApplyResult, String> {
    let raw = fs::read_to_string(&options.proposal)
        .map_err(|e| format!("cannot read proposal '{}': {e}", options.proposal.display()))?;
    let proposal = serde_json::from_str::<Value>(&raw).map_err(|e| {
        format!(
            "invalid proposal JSON '{}': {e}",
            options.proposal.display()
        )
    })?;
    if proposal.get("kind").and_then(Value::as_str) != Some("autopilot_proposal") {
        return Err("proposal kind must be autopilot_proposal".to_string());
    }
    if proposal.get("mode").and_then(Value::as_str) != Some("dry_run") {
        return Err("proposal mode must be dry_run".to_string());
    }

    let item = find_proposal(&proposal, &options.proposal_id)?;
    let action = json_field(item, "action").unwrap_or_default();
    if action != APPLY_ACTION {
        return Err(format!("unsupported autopilot apply action '{action}'"));
    }
    let expected_token = approval_token(&options.proposal_id);
    if options.confirm != expected_token {
        return Err("invalid approval token for proposal".to_string());
    }
    let would_run = item
        .get("preview")
        .and_then(|preview| preview.get("would_run"))
        .and_then(Value::as_array)
        .map(Vec::len)
        .unwrap_or(0);
    if would_run != 0 {
        return Err("proposal would_run must be empty for apply".to_string());
    }

    let draft = item
        .get("draft")
        .ok_or_else(|| "proposal is missing draft payload".to_string())?;
    let draft_path =
        json_field(draft, "path").ok_or_else(|| "draft path is missing".to_string())?;
    let draft_content =
        json_field(draft, "content").ok_or_else(|| "draft content is missing".to_string())?;
    validate_draft_path(&draft_path)?;
    let output = options.root.join(&draft_path);
    if output.exists() {
        return Err(format!(
            "draft '{}' already exists; refusing to overwrite",
            output.display()
        ));
    }
    if let Some(parent) = output.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create draft directory '{}': {e}", parent.display()))?;
    }
    fs::write(&output, draft_content)
        .map_err(|e| format!("cannot write draft '{}': {e}", output.display()))?;

    Ok(ApplyResult {
        proposal_id: options.proposal_id.clone(),
        action,
        changed_files: vec![output.display().to_string()],
    })
}

#[derive(Clone)]
struct EvidenceRun {
    id: String,
    job: String,
    status: String,
    failed_step: String,
    evidence: Vec<String>,
}

impl ProposalItem {
    fn from_run(run: &EvidenceRun) -> Self {
        let step = if run.failed_step.is_empty() {
            "unknown step"
        } else {
            &run.failed_step
        };
        let mut evidence = run.evidence.clone();
        evidence.push(format!("job {}", run.job));
        evidence.push(format!("status {}", run.status));
        if !run.failed_step.is_empty() {
            evidence.push(format!("failed_step {}", run.failed_step));
        }
        evidence.sort();
        evidence.dedup();

        let reason = format!("{} {} at {}", run.job, run.status, step);
        let id = stable_proposal_id(APPLY_ACTION, &run.id, &reason);
        let draft_path = format!(".flow/agent/drafts/autopilot-{id}.json");
        let draft_content = local_draft_content(&id, run, &reason, &evidence);

        Self {
            id: id.clone(),
            action: APPLY_ACTION,
            target: run.id.clone(),
            status: "requires_approval",
            risk: "low",
            confidence: confidence_for(run),
            reason,
            evidence,
            would_change: vec![draft_path.clone()],
            draft_path,
            draft_content,
            approval_token: approval_token(&id),
            rollback: vec!["Delete the generated local draft file.".to_string()],
        }
    }

    fn json(&self) -> String {
        format!(
            "{{\"id\":\"{}\",\"action\":\"{}\",\"target\":\"{}\",\"status\":\"{}\",\"risk\":\"{}\",\"confidence\":{},\"reason\":\"{}\",\"evidence\":[{}],\"preview\":{{\"would_change\":[{}],\"would_run\":[]}},\"approval\":{{\"required\":true,\"token_hint\":\"{}\"}},\"draft\":{{\"path\":\"{}\",\"content\":\"{}\"}},\"rollback\":[{}]}}",
            json::escape(&self.id),
            json::escape(self.action),
            json::escape(&self.target),
            json::escape(self.status),
            json::escape(self.risk),
            confidence_json(self.confidence),
            json::escape(&self.reason),
            json::string_array(&self.evidence),
            json::string_array(&self.would_change),
            json::escape(&self.approval_token),
            json::escape(&self.draft_path),
            json::escape(&self.draft_content),
            json::string_array(&self.rollback)
        )
    }
}

impl AutopilotProposal {
    fn text(&self) -> String {
        let mut out = vec![
            "kind: autopilot_proposal".to_string(),
            format!("root: {}", self.root),
            format!("generated_at: {}", self.generated_at),
            "mode: dry_run".to_string(),
            format!("from_watch: {}", option_or_null(&self.source_watch)),
            format!("from_oncall: {}", option_or_null(&self.source_oncall)),
            format!("proposals: {}", self.proposals.len()),
            format!("requires_approval: {}", self.proposals.len()),
            format!("blocked: {}", self.blocked.len()),
            "proposal_details:".to_string(),
        ];
        if self.proposals.is_empty() {
            out.push("- none".to_string());
        } else {
            for proposal in &self.proposals {
                out.push(format!(
                    "- {} action={} target={} status={} risk={} confidence={} reason={}",
                    proposal.id,
                    proposal.action,
                    proposal.target,
                    proposal.status,
                    proposal.risk,
                    confidence_json(proposal.confidence),
                    proposal.reason
                ));
                out.push(format!("  confirm: {}", proposal.approval_token));
                out.push(format!(
                    "  would_change: {}",
                    proposal.would_change.join(", ")
                ));
            }
        }
        if !self.blocked.is_empty() {
            out.push("blocked_details:".to_string());
            for blocked in &self.blocked {
                out.push(format!(
                    "- action={} target={} reason={}",
                    blocked.action, blocked.target, blocked.reason
                ));
            }
        }
        out.join("\n")
    }

    fn json(&self) -> String {
        let proposals = self
            .proposals
            .iter()
            .map(ProposalItem::json)
            .collect::<Vec<_>>()
            .join(",");
        let blocked = self
            .blocked
            .iter()
            .map(BlockedItem::json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"kind\":\"autopilot_proposal\",\"root\":\"{}\",\"generated_at\":\"{}\",\"mode\":\"dry_run\",\"source\":{{\"watch_snapshot\":{},\"oncall_handoff\":{}}},\"summary\":{{\"proposals\":{},\"requires_approval\":{},\"blocked\":{}}},\"proposals\":[{}],\"blocked\":[{}]}}",
            json::escape(&self.root),
            json::escape(&self.generated_at),
            json_option(&self.source_watch),
            json_option(&self.source_oncall),
            self.proposals.len(),
            self.proposals.len(),
            self.blocked.len(),
            proposals,
            blocked
        )
    }
}

impl BlockedItem {
    fn json(&self) -> String {
        format!(
            "{{\"action\":\"{}\",\"target\":\"{}\",\"reason\":\"{}\"}}",
            json::escape(&self.action),
            json::escape(&self.target),
            json::escape(&self.reason)
        )
    }
}

impl ApplyResult {
    fn text(&self) -> String {
        [
            "kind: autopilot_apply".to_string(),
            "status: success".to_string(),
            format!("proposal_id: {}", self.proposal_id),
            format!("action: {}", self.action),
            format!("changed_files: {}", self.changed_files.join(", ")),
        ]
        .join("\n")
    }

    fn json(&self) -> String {
        format!(
            "{{\"kind\":\"autopilot_apply\",\"status\":\"success\",\"proposal_id\":\"{}\",\"action\":\"{}\",\"changed_files\":[{}]}}",
            json::escape(&self.proposal_id),
            json::escape(&self.action),
            json::string_array(&self.changed_files)
        )
    }
}

fn read_source_json(path: &Option<PathBuf>, label: &str) -> Result<Option<Value>, String> {
    let Some(path) = path else {
        return Ok(None);
    };
    let raw = fs::read_to_string(path)
        .map_err(|e| format!("cannot read {label} source '{}': {e}", path.display()))?;
    let value = serde_json::from_str::<Value>(&raw)
        .map_err(|e| format!("invalid {label} source JSON '{}': {e}", path.display()))?;
    Ok(Some(value))
}

fn runs_from_watch(source: &Option<Value>) -> Vec<EvidenceRun> {
    source
        .as_ref()
        .and_then(|value| value.get("incidents"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    Some(EvidenceRun {
                        id: json_field(item, "run_id")?,
                        job: json_field(item, "job").unwrap_or_else(|| "unknown".to_string()),
                        status: json_field(item, "status").unwrap_or_else(|| "FAILED".to_string()),
                        failed_step: json_field(item, "failed_step").unwrap_or_default(),
                        evidence: vec!["watch incident".to_string()],
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn runs_from_oncall(source: &Option<Value>) -> Vec<EvidenceRun> {
    source
        .as_ref()
        .and_then(|value| value.get("incidents"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let evidence = item
                        .get("evidence")
                        .and_then(Value::as_array)
                        .map(|values| {
                            values
                                .iter()
                                .filter_map(Value::as_str)
                                .map(ToString::to_string)
                                .collect::<Vec<_>>()
                        })
                        .unwrap_or_else(|| vec!["oncall incident".to_string()]);
                    Some(EvidenceRun {
                        id: json_field(item, "id")?,
                        job: json_field(item, "job").unwrap_or_else(|| "unknown".to_string()),
                        status: json_field(item, "status").unwrap_or_else(|| "FAILED".to_string()),
                        failed_step: json_field(item, "failed_step").unwrap_or_default(),
                        evidence,
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn blocked_from_source(source: &Option<Value>) -> Vec<BlockedItem> {
    source
        .as_ref()
        .and_then(|value| value.get("proposals"))
        .and_then(Value::as_array)
        .map(|items| {
            items
                .iter()
                .filter_map(|item| {
                    let action = json_field(item, "action")?;
                    if allowed_plan_action(&action) {
                        return None;
                    }
                    Some(BlockedItem {
                        target: json_field(item, "target").unwrap_or_else(|| "unknown".to_string()),
                        action,
                        reason: "action is outside the autopilot allowlist".to_string(),
                    })
                })
                .collect()
        })
        .unwrap_or_default()
}

fn allowed_plan_action(action: &str) -> bool {
    action == APPLY_ACTION || action == "review_failed_run" || action == "inspect_workflow"
}

fn find_proposal<'a>(proposal: &'a Value, proposal_id: &str) -> Result<&'a Value, String> {
    proposal
        .get("proposals")
        .and_then(Value::as_array)
        .and_then(|items| {
            items
                .iter()
                .find(|item| item.get("id").and_then(Value::as_str) == Some(proposal_id))
        })
        .ok_or_else(|| format!("proposal '{proposal_id}' not found"))
}

fn local_draft_content(
    proposal_id: &str,
    run: &EvidenceRun,
    reason: &str,
    evidence: &[String],
) -> String {
    format!(
        "{{\"kind\":\"autopilot_local_draft\",\"proposal_id\":\"{}\",\"run_id\":\"{}\",\"job\":\"{}\",\"status\":\"{}\",\"failed_step\":\"{}\",\"reason\":\"{}\",\"evidence\":[{}],\"allowed_next_step\":\"Human review only; no job execution was performed.\"}}\n",
        json::escape(proposal_id),
        json::escape(&run.id),
        json::escape(&run.job),
        json::escape(&run.status),
        json::escape(&run.failed_step),
        json::escape(reason),
        json::string_array(evidence)
    )
}

fn stable_proposal_id(action: &str, target: &str, reason: &str) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in format!("{action}\0{target}\0{reason}").bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(0x100000001b3);
    }
    format!("proposal-{hash:012x}")
}

fn approval_token(proposal_id: &str) -> String {
    format!("approve:{proposal_id}:{APPLY_ACTION}")
}

fn confidence_for(run: &EvidenceRun) -> f32 {
    if !run.failed_step.is_empty() && run.status == "FAILED" {
        0.9
    } else if run.status == "FAILED" || run.status == "ERROR" {
        0.8
    } else {
        0.5
    }
}

fn confidence_json(value: f32) -> String {
    format!("{value:.2}")
}

fn validate_draft_path(value: &str) -> Result<(), String> {
    let path = Path::new(value);
    if path.is_absolute() {
        return Err("draft path must be relative".to_string());
    }
    if !value.starts_with(".flow/agent/drafts/") {
        return Err("draft path must stay under .flow/agent/drafts".to_string());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        return Err("draft path must not contain parent directory segments".to_string());
    }
    Ok(())
}

fn write_output(path: &Path, output: &str, label: &str) -> Result<(), String> {
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent)
            .map_err(|e| format!("cannot create output directory '{}': {e}", parent.display()))?;
    }
    fs::write(path, output).map_err(|e| format!("cannot write {label} '{}': {e}", path.display()))
}

fn json_field(value: &Value, field: &str) -> Option<String> {
    value
        .get(field)
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn option_or_null(value: &Option<String>) -> &str {
    value.as_deref().unwrap_or("null")
}

fn json_option(value: &Option<String>) -> String {
    value
        .as_ref()
        .map(|value| format!("\"{}\"", json::escape(value)))
        .unwrap_or_else(|| "null".to_string())
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn wants_json(args: &[String]) -> bool {
    args.iter().any(|arg| arg == "--format") && args.iter().any(|arg| arg == "json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_workspace_returns_valid_dry_run() {
        let root = unique_temp_dir("autopilot-empty");
        fs::create_dir_all(&root).unwrap();

        let result = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        let parsed: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(parsed["kind"], "autopilot_proposal");
        assert_eq!(parsed["mode"], "dry_run");
        assert_eq!(parsed["summary"]["proposals"], 0);
        assert!(parsed["proposals"].as_array().unwrap().is_empty());
    }

    #[test]
    fn failed_run_produces_applyable_local_draft_proposal() {
        let root = workspace_with_run("autopilot-run", "run-1", "backup", "FAILED", "upload");

        let result = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        let parsed: Value = serde_json::from_str(&result.output).unwrap();
        let proposal = &parsed["proposals"][0];
        assert_eq!(proposal["action"], APPLY_ACTION);
        assert_eq!(proposal["target"], "run-1");
        assert_eq!(proposal["confidence"], 0.9);
        assert!(proposal["id"].as_str().unwrap().starts_with("proposal-"));
        assert!(proposal["preview"]["would_run"]
            .as_array()
            .unwrap()
            .is_empty());
        assert!(proposal["preview"]["would_change"][0]
            .as_str()
            .unwrap()
            .starts_with(".flow/agent/drafts/autopilot-proposal-"));
    }

    #[test]
    fn proposal_ids_are_deterministic() {
        let run = EvidenceRun {
            id: "run-1".to_string(),
            job: "backup".to_string(),
            status: "FAILED".to_string(),
            failed_step: "upload".to_string(),
            evidence: vec!["one".to_string()],
        };

        let first = ProposalItem::from_run(&run);
        let second = ProposalItem::from_run(&run);

        assert_eq!(first.id, second.id);
        assert_eq!(first.approval_token, second.approval_token);
    }

    #[test]
    fn oncall_source_produces_proposal() {
        let root = unique_temp_dir("autopilot-oncall");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("handoff.json");
        fs::write(
            &source,
            "{\"kind\":\"oncall_handoff\",\"incidents\":[{\"id\":\"run-7\",\"job\":\"deploy\",\"status\":\"ERROR\",\"failed_step\":\"build\",\"evidence\":[\"oncall severity high\"]}]}",
        )
        .unwrap();

        let result = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--from-oncall".to_string(),
            source.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(result.output.contains("\"target\":\"run-7\""));
        assert!(result.output.contains("oncall severity high"));
    }

    #[test]
    fn mutation_actions_from_sources_are_blocked() {
        let root = unique_temp_dir("autopilot-blocked");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("bad-proposal.json");
        fs::write(
            &source,
            "{\"kind\":\"autopilot_proposal\",\"proposals\":[{\"id\":\"p1\",\"action\":\"run_job\",\"target\":\"backup\"}]}",
        )
        .unwrap();

        let result = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--from-oncall".to_string(),
            source.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        let parsed: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(parsed["summary"]["blocked"], 1);
        assert_eq!(parsed["blocked"][0]["action"], "run_job");
    }

    #[test]
    fn invalid_source_json_fails_closed() {
        let root = unique_temp_dir("autopilot-invalid");
        fs::create_dir_all(&root).unwrap();
        let source = root.join("bad.json");
        fs::write(&source, "not json").unwrap();

        let err = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--from-watch".to_string(),
            source.to_string_lossy().to_string(),
        ])
        .unwrap_err();

        fs::remove_dir_all(root).unwrap();
        assert!(err.contains("invalid watch source JSON"));
    }

    #[test]
    fn output_writes_requested_file_and_audit() {
        let root = workspace_with_run("autopilot-output", "run-1", "backup", "FAILED", "upload");
        let output = root.join("proposal").join("latest.json");

        let result = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--output".to_string(),
            output.to_string_lossy().to_string(),
        ])
        .unwrap();

        let audit =
            fs::read_to_string(root.join(".flow").join("agent").join("audit.jsonl")).unwrap();
        assert!(output.is_file());
        assert_eq!(result.changed_files, vec![output.display().to_string()]);
        assert!(audit.contains("requires approval"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apply_writes_only_local_draft_with_valid_token() {
        let root = workspace_with_run("autopilot-apply", "run-1", "backup", "FAILED", "upload");
        let proposal_path = root.join("proposal.json");
        let plan = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--output".to_string(),
            proposal_path.to_string_lossy().to_string(),
        ])
        .unwrap();
        let parsed: Value = serde_json::from_str(&plan.output).unwrap();
        let proposal_id = parsed["proposals"][0]["id"].as_str().unwrap();
        let token = parsed["proposals"][0]["approval"]["token_hint"]
            .as_str()
            .unwrap();

        let result = run(&[
            "apply".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--proposal".to_string(),
            proposal_path.to_string_lossy().to_string(),
            "--proposal-id".to_string(),
            proposal_id.to_string(),
            "--confirm".to_string(),
            token.to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        let parsed_apply: Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(parsed_apply["kind"], "autopilot_apply");
        assert_eq!(parsed_apply["action"], APPLY_ACTION);
        assert_eq!(result.changed_files.len(), 1);
        assert!(Path::new(&result.changed_files[0]).is_file());
        assert!(result.changed_files[0].contains(".flow"));
        assert!(result.changed_files[0].contains("agent"));
        assert!(result.changed_files[0].contains("drafts"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apply_rejects_invalid_token_and_refuses_overwrite() {
        let root = workspace_with_run(
            "autopilot-apply-token",
            "run-1",
            "backup",
            "FAILED",
            "upload",
        );
        let proposal_path = root.join("proposal.json");
        let plan = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--output".to_string(),
            proposal_path.to_string_lossy().to_string(),
        ])
        .unwrap();
        let parsed: Value = serde_json::from_str(&plan.output).unwrap();
        let proposal_id = parsed["proposals"][0]["id"].as_str().unwrap();
        let token = parsed["proposals"][0]["approval"]["token_hint"]
            .as_str()
            .unwrap();

        let bad_token = run(&[
            "apply".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--proposal".to_string(),
            proposal_path.to_string_lossy().to_string(),
            "--proposal-id".to_string(),
            proposal_id.to_string(),
            "--confirm".to_string(),
            "bad-token".to_string(),
        ])
        .unwrap_err();
        assert!(bad_token.contains("invalid approval token"));

        run(&[
            "apply".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--proposal".to_string(),
            proposal_path.to_string_lossy().to_string(),
            "--proposal-id".to_string(),
            proposal_id.to_string(),
            "--confirm".to_string(),
            token.to_string(),
        ])
        .unwrap();
        let overwrite = run(&[
            "apply".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--proposal".to_string(),
            proposal_path.to_string_lossy().to_string(),
            "--proposal-id".to_string(),
            proposal_id.to_string(),
            "--confirm".to_string(),
            token.to_string(),
        ])
        .unwrap_err();

        fs::remove_dir_all(root).unwrap();
        assert!(overwrite.contains("refusing to overwrite"));
    }

    #[test]
    fn apply_rejects_non_allowlisted_action() {
        let root = unique_temp_dir("autopilot-apply-denied");
        fs::create_dir_all(&root).unwrap();
        let proposal = root.join("proposal.json");
        fs::write(
            &proposal,
            "{\"kind\":\"autopilot_proposal\",\"mode\":\"dry_run\",\"proposals\":[{\"id\":\"proposal-x\",\"action\":\"run_job\",\"preview\":{\"would_run\":[]},\"draft\":{\"path\":\".flow/agent/drafts/x.json\",\"content\":\"{}\"}}]}",
        )
        .unwrap();

        let err = run(&[
            "apply".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--proposal".to_string(),
            proposal.to_string_lossy().to_string(),
            "--proposal-id".to_string(),
            "proposal-x".to_string(),
            "--confirm".to_string(),
            "approve:proposal-x:write_local_draft".to_string(),
        ])
        .unwrap_err();

        fs::remove_dir_all(root).unwrap();
        assert!(err.contains("unsupported autopilot apply action"));
    }

    #[test]
    fn proposals_do_not_include_dangerous_apply_actions() {
        let root = workspace_with_run("autopilot-denied", "run-1", "backup", "FAILED", "upload");

        let result = run(&[
            "plan".to_string(),
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        for denied in [
            "run_job",
            "cancel_run",
            "rerun_step",
            "execute_shell",
            "call_webhook",
        ] {
            assert!(!result.output.contains(denied));
        }
    }

    fn workspace_with_run(
        prefix: &str,
        run_id: &str,
        job: &str,
        status: &str,
        failed_step: &str,
    ) -> PathBuf {
        let root = unique_temp_dir(prefix);
        let jobs = root.join(".flow").join("jobs");
        let run_dir = root.join(".flow").join("runs").join(run_id);
        fs::create_dir_all(&jobs).unwrap();
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(
            jobs.join(format!("{job}.yml")),
            format!("name: {job}\nsteps: []\n"),
        )
        .unwrap();
        fs::write(
            run_dir.join("manifest.json"),
            format!(
                "{{\"run_id\":\"{run_id}\",\"job_name\":\"{job}\",\"status\":\"{status}\",\"failed_step\":\"{failed_step}\",\"started_at\":\"{}\"}}",
                audit::timestamp_seconds()
            ),
        )
        .unwrap();
        root
    }

    fn unique_temp_dir(prefix: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "runflow-agent-{prefix}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }
}
