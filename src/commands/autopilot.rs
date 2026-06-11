use std::fs;
use std::path::PathBuf;

use serde_json::Value;

use crate::audit;
use crate::cli::CliResult;
use crate::commands::inspect_workspace;
use crate::json;

const DEFAULT_LIMIT: usize = 100;

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let Some(subcommand) = args.first().map(String::as_str) else {
        return Err("autopilot requires a subcommand: plan".to_string());
    };
    if subcommand != "plan" {
        return Err(format!(
            "unknown autopilot subcommand '{subcommand}'; only plan is available"
        ));
    }

    let options = PlanOptions::from_args(&args[1..])?;
    let proposal = build_proposal(&options)?;
    let output = if options.format_json {
        proposal.json()
    } else {
        proposal.text()
    };

    let mut changed_files = Vec::new();
    if let Some(path) = &options.output {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|e| {
                format!("cannot create output directory '{}': {e}", parent.display())
            })?;
        }
        fs::write(path, &output)
            .map_err(|e| format!("cannot write autopilot proposal '{}': {e}", path.display()))?;
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
    reason: String,
    evidence: Vec<String>,
    rollback: Vec<String>,
}

struct BlockedItem {
    reason: String,
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
        .enumerate()
        .map(|(index, run)| ProposalItem::from_run(index + 1, run))
        .collect::<Vec<_>>();

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
        blocked: Vec::new(),
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
    fn from_run(index: usize, run: &EvidenceRun) -> Self {
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

        Self {
            id: format!("proposal-{index:03}"),
            action: "review_failed_run",
            target: run.id.clone(),
            status: "requires_approval",
            risk: "low",
            reason: format!("{} {} at {}", run.job, run.status, step),
            evidence,
            rollback: vec!["No rollback needed for review-only proposal.".to_string()],
        }
    }

    fn json(&self) -> String {
        format!(
            "{{\"id\":\"{}\",\"action\":\"{}\",\"target\":\"{}\",\"status\":\"{}\",\"risk\":\"{}\",\"reason\":\"{}\",\"evidence\":[{}],\"preview\":{{\"would_change\":[],\"would_run\":[]}},\"approval\":{{\"required\":true,\"token_hint\":\"generated by future approval command\"}},\"rollback\":[{}]}}",
            json::escape(&self.id),
            json::escape(self.action),
            json::escape(&self.target),
            json::escape(self.status),
            json::escape(self.risk),
            json::escape(&self.reason),
            json::string_array(&self.evidence),
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
                    "- {} action={} target={} status={} risk={} reason={}",
                    proposal.id,
                    proposal.action,
                    proposal.target,
                    proposal.status,
                    proposal.risk,
                    proposal.reason
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
            .map(|blocked| format!("{{\"reason\":\"{}\"}}", json::escape(&blocked.reason)))
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
    fn failed_run_produces_read_only_proposal() {
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
        assert!(result.output.contains("\"action\":\"review_failed_run\""));
        assert!(result.output.contains("\"target\":\"run-1\""));
        assert!(result.output.contains("\"would_change\":[]"));
        assert!(result.output.contains("\"would_run\":[]"));
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
        assert!(audit.contains("proposal-001 requires approval"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn apply_subcommand_does_not_exist() {
        let err = run(&["apply".to_string()]).unwrap_err();
        assert!(err.contains("only plan is available"));
    }

    #[test]
    fn proposals_do_not_include_mutation_actions() {
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
