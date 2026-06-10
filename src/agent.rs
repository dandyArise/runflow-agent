use std::fs;
use std::path::{Path, PathBuf};

use crate::config::ModelConfig;
use crate::model;
use crate::runflow::Finding;
use crate::strict_json::{
    decode_model_json, DailyReportModelResponse, DraftModelResponse, ModelKind,
    ReviewModelResponse, RunExplanationModelResponse,
};
use crate::util;
use serde::de::DeserializeOwned;
use serde_json::Value;

const MAX_REQUEST_CHARS: usize = 4_000;
const MAX_WORKFLOW_CHARS: usize = 24_000;
const MAX_EVIDENCE_CHARS: usize = 8_000;
const MAX_REPORT_CHARS: usize = 4_000;

#[derive(Debug)]
pub struct Draft {
    pub workflow_yaml: String,
    pub warnings: Vec<String>,
}

#[derive(Debug)]
pub struct RunExplanation {
    pub run_id: String,
    pub status: String,
    pub summary: String,
    pub failed_step: Option<String>,
    pub evidence: Vec<String>,
    pub suggested_next_steps: Vec<String>,
}

#[derive(Debug)]
pub struct DailyReport {
    pub from: String,
    pub to: String,
    pub total: usize,
    pub success: usize,
    pub failed: usize,
    pub cancelled: usize,
    pub unstable_jobs: Vec<String>,
    pub incidents: Vec<String>,
    pub recommendations: Vec<String>,
}

struct RunEvidence {
    manifest: String,
    events: String,
    step_metadata: String,
    stdout: String,
    stderr: String,
    sources: Vec<String>,
}

#[derive(Clone, Copy, Debug)]
struct HostContext {
    os: &'static str,
    family: &'static str,
}

impl HostContext {
    fn current() -> Self {
        Self {
            os: std::env::consts::OS,
            family: std::env::consts::FAMILY,
        }
    }

    fn command_target(&self) -> String {
        format!("{} ({})", self.os, self.family)
    }

    fn ping_count_flag(&self) -> &'static str {
        match self.family {
            "windows" => "-n",
            _ => "-c",
        }
    }

    fn wrong_ping_count_flag(&self) -> &'static str {
        match self.family {
            "windows" => "-c",
            _ => "-n",
        }
    }
}

impl RunEvidence {
    fn is_empty(&self) -> bool {
        self.manifest.is_empty()
            && self.events.is_empty()
            && self.step_metadata.is_empty()
            && self.stdout.is_empty()
            && self.stderr.is_empty()
            && self.sources.is_empty()
    }
}

pub fn draft_workflow(request: &str) -> Draft {
    let lower = request.to_lowercase();
    let name = util::slugify(request, "draft-workflow");
    let mut warnings = Vec::new();
    let mut yaml = format!("name: {name}\nversion: 1\nschema_version: 1\n");

    if let Some(minutes) = extract_every_minutes(&lower) {
        yaml.push_str(&format!(
            "schedule:\n  cron: \"0 */{minutes} * * * * *\"\n  timezone: UTC\n  enabled: true\n"
        ));
    } else {
        yaml.push_str("schedule: false\n");
    }

    yaml.push_str("\nsteps:\n");
    if lower.contains("ping") {
        let host = extract_ipv4(request).unwrap_or_else(|| "1.1.1.1".to_string());
        yaml.push_str("  - name: ping\n    type: command\n    timeout: 30s\n    run:\n      command: ping\n      args: [\"-n\", \"4\", \"");
        yaml.push_str(&host);
        yaml.push_str("\"]\n");
        warnings.push("On Linux/macOS, replace ping args with [\"-c\", \"4\", host].".to_string());
    } else if lower.contains("backup") {
        yaml.push_str("  - name: backup\n    type: command\n    timeout: 60s\n    run:\n      command: echo\n      args: [\"TODO: replace with backup command\"]\n");
        warnings.push(
            "Draft uses echo placeholder; replace it before running with RunFlow.".to_string(),
        );
    } else if lower.contains("sleep") || lower.contains("wait") {
        yaml.push_str("  - name: wait\n    type: sleep\n    duration: 2s\n");
    } else {
        yaml.push_str("  - name: task\n    type: command\n    timeout: 30s\n    run:\n      command: echo\n      args: [\"TODO: replace with command\"]\n");
        warnings.push("Request was ambiguous; generated a safe echo placeholder.".to_string());
    }

    Draft {
        workflow_yaml: yaml,
        warnings,
    }
}

pub fn draft_workflow_with_model(request: &str, config: &ModelConfig) -> Result<Draft, String> {
    if config.is_mock() {
        return Ok(draft_workflow(request));
    }

    let system = strict_json_system("Return only a draft_workflow JSON object.");
    let user = draft_workflow_prompt(request);
    let raw = model::chat(config, system, &user)?;
    let parsed: DraftModelResponse = decode_model_json(&raw, "draft_workflow")?;
    let mut warnings = parsed.warnings;
    let workflow_yaml = adapt_workflow_for_host(parsed.workflow_yaml, &mut warnings);
    Ok(Draft {
        workflow_yaml,
        warnings,
    })
}

pub fn repair_draft_workflow_with_model(
    request: &str,
    draft: &Draft,
    validation_errors: &[String],
    config: &ModelConfig,
) -> Result<Draft, String> {
    if config.is_mock() {
        return Ok(draft_workflow(request));
    }

    let system = strict_json_system("Return only a repaired draft_workflow JSON object.");
    let user = repair_draft_workflow_prompt(request, &draft.workflow_yaml, validation_errors);
    let raw = model::chat(config, system, &user)?;
    let parsed: DraftModelResponse = decode_model_json(&raw, "draft_workflow")?;
    let mut warnings = parsed.warnings;
    let workflow_yaml = adapt_workflow_for_host(parsed.workflow_yaml, &mut warnings);
    Ok(Draft {
        workflow_yaml,
        warnings,
    })
}

fn draft_workflow_prompt(request: &str) -> String {
    let host = HostContext::current();
    format!(
        "Create a RunFlow workflow draft for this request:\n\n{}\n\nHost context:\n- Target host OS: {}\n- Target host family: {}\n- Ping count flag for this host: {}\n- Generate commands and arguments compatible with this host.\n\nSchema constraints:\n- Return only JSON, no markdown and no prose.\n- workflow_yaml must be valid YAML for the embedded RunFlow workflow schema.\n- Prefer a minimal workflow with only name, version, schema_version, schedule, and steps unless the request explicitly requires more.\n- Do not add optional top-level fields unless needed by the request.\n- Top-level name must be kebab-case: lowercase letters, digits, and hyphens only; no spaces, underscores, dots, or uppercase.\n- Step names must use lowercase letters, digits, hyphens, or underscores; no dots, spaces, or uppercase.\n- schedule must be false, a cron string, or an object with cron/timezone/enabled. Never use schedule: true.\n- Allowed top-level fields: name, version, schema_version, schedule, failure_policy, concurrency, limits, locks, secrets, notifications, retention, steps, tests.\n- If concurrency is required, policy must be one of allow, forbid, queue, replace.\n- Allowed step types: command, plugin, sleep, wait_until.\n- For command steps, put timeout at the step level as a duration string such as timeout: 30s.\n- Never put timeout inside run.\n- Prefer structured run.command and run.args. Do not inline secrets. Do not run anything.\n\nValid YAML example for this host:\nname: ping-monitor\nversion: 1\nschema_version: 1\nschedule: false\nsteps:\n  - name: ping\n    type: command\n    timeout: 30s\n    run:\n      command: ping\n      args: [\"{}\", \"4\", \"1.1.1.1\"]\n\nReturn exactly this JSON shape:\n{{\"kind\":\"draft_workflow\",\"workflow_yaml\":\"<yaml>\",\"warnings\":[\"...\"]}}",
        util::truncate(request, MAX_REQUEST_CHARS),
        host.os,
        host.family,
        host.ping_count_flag(),
        host.ping_count_flag()
    )
}

fn repair_draft_workflow_prompt(
    request: &str,
    workflow_yaml: &str,
    validation_errors: &[String],
) -> String {
    format!(
        "{}\n\nThe previous workflow_yaml failed schema validation. Repair only the workflow_yaml and keep the original intent.\n\nOriginal request:\n{}\n\nValidation errors:\n{}\n\nInvalid workflow_yaml:\n{}",
        draft_workflow_prompt(request),
        util::truncate(request, MAX_REQUEST_CHARS),
        util::truncate(&validation_errors.join("\n"), MAX_REPORT_CHARS),
        util::truncate(workflow_yaml, MAX_WORKFLOW_CHARS)
    )
}

fn adapt_workflow_for_host(workflow_yaml: String, warnings: &mut Vec<String>) -> String {
    let host = HostContext::current();
    let Ok(mut value) = serde_yaml::from_str::<serde_yaml::Value>(&workflow_yaml) else {
        return workflow_yaml;
    };
    let mut changed = false;

    if let Some(steps) = value
        .as_mapping_mut()
        .and_then(|root| root.get_mut(yaml_key("steps")))
        .and_then(serde_yaml::Value::as_sequence_mut)
    {
        for step in steps {
            if adapt_ping_step_for_host(step, host) {
                changed = true;
            }
        }
    }

    if !changed {
        return workflow_yaml;
    }

    warnings.push(format!(
        "Adjusted ping args for {}: use {} for count.",
        host.command_target(),
        host.ping_count_flag()
    ));
    serde_yaml::to_string(&value).unwrap_or(workflow_yaml)
}

fn adapt_ping_step_for_host(step: &mut serde_yaml::Value, host: HostContext) -> bool {
    let Some(run) = step
        .as_mapping_mut()
        .and_then(|step_map| step_map.get_mut(yaml_key("run")))
        .and_then(serde_yaml::Value::as_mapping_mut)
    else {
        return false;
    };
    let command_is_ping = run
        .get(yaml_key("command"))
        .and_then(serde_yaml::Value::as_str)
        .map(|command| command.eq_ignore_ascii_case("ping"))
        .unwrap_or(false);
    if !command_is_ping {
        return false;
    }
    let Some(args) = run
        .get_mut(yaml_key("args"))
        .and_then(serde_yaml::Value::as_sequence_mut)
    else {
        return false;
    };
    let Some(first) = args.first_mut() else {
        return false;
    };
    let Some(flag) = first.as_str() else {
        return false;
    };
    if flag == host.wrong_ping_count_flag() {
        *first = serde_yaml::Value::String(host.ping_count_flag().to_string());
        true
    } else {
        false
    }
}

fn review_host_command_args(yaml: &str) -> Vec<Finding> {
    let host = HostContext::current();
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml) else {
        return Vec::new();
    };
    let Some(steps) = value
        .as_mapping()
        .and_then(|root| root.get(yaml_key("steps")))
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return Vec::new();
    };

    let mut findings = Vec::new();
    for (index, step) in steps.iter().enumerate() {
        let Some(run) = step
            .as_mapping()
            .and_then(|step_map| step_map.get(yaml_key("run")))
            .and_then(serde_yaml::Value::as_mapping)
        else {
            continue;
        };
        let command_is_ping = run
            .get(yaml_key("command"))
            .and_then(serde_yaml::Value::as_str)
            .map(|command| command.eq_ignore_ascii_case("ping"))
            .unwrap_or(false);
        if !command_is_ping {
            continue;
        }
        let first_arg = run
            .get(yaml_key("args"))
            .and_then(serde_yaml::Value::as_sequence)
            .and_then(|args| args.first())
            .and_then(serde_yaml::Value::as_str);
        if first_arg == Some(host.wrong_ping_count_flag()) {
            findings.push(Finding::warning(
                &format!("/steps/{index}/run/args/0"),
                &format!(
                    "Ping count flag '{}' does not match target host OS '{}'.",
                    host.wrong_ping_count_flag(),
                    host.command_target()
                ),
                &format!(
                    "Use '{}' for ping count on this host before running with RunFlow.",
                    host.ping_count_flag()
                ),
            ));
        }
    }
    findings
}

fn yaml_key(key: &str) -> serde_yaml::Value {
    serde_yaml::Value::String(key.to_string())
}

pub fn review_workflow(yaml: &str) -> Vec<Finding> {
    let mut findings = Vec::new();
    let lower = yaml.to_lowercase();

    if lower.contains("run:") && !lower.contains("timeout:") {
        findings.push(Finding::warning(
            "/steps",
            "Command step may be missing timeout.",
            "Add timeout: 30s or another bounded duration.",
        ));
    }
    for token in [">", ">>", "2>", "|", "&&", "||", ";"] {
        if yaml.contains(token) {
            findings.push(Finding::warning(
                "/steps/*/run",
                &format!("Command text contains shell control token '{token}'."),
                "Prefer structured run.command and run.args unless shell behavior is intentional.",
            ));
            break;
        }
    }
    if lower.contains("curl ") || lower.contains("wget ") || lower.contains("invoke-webrequest") {
        findings.push(Finding::warning(
            "/steps/*/run",
            "Workflow may call an external network endpoint.",
            "Confirm this is expected and keep credentials out of the workflow.",
        ));
    }
    if lower.contains("password:") || lower.contains("token:") || lower.contains("secret:") {
        findings.push(Finding::error(
            "/",
            "Workflow may contain inline secret-like values.",
            "Use secrets.<name>.from_env references instead.",
        ));
    }
    if lower.contains("env:") && !lower.contains("from_env") {
        findings.push(Finding::info(
            "/env",
            "Workflow contains environment configuration.",
            "Review env scope and avoid broad or sensitive values.",
        ));
    }
    findings.extend(review_host_command_args(yaml));
    findings
}

pub fn review_workflow_with_model(
    yaml: &str,
    config: &ModelConfig,
) -> Result<Vec<Finding>, String> {
    let mut findings = review_workflow(yaml);
    if config.is_mock() {
        return Ok(findings);
    }

    let system = strict_json_system("Return only a workflow_review JSON object.");
    let user = review_workflow_prompt(yaml);
    let raw = model::chat(config, system, &user)?;
    findings.extend(
        parse_findings_with_repair(&raw, config)?
            .into_iter()
            .filter(|finding| !is_spurious_host_ping_finding(yaml, finding)),
    );
    Ok(findings)
}

fn review_workflow_prompt(yaml: &str) -> String {
    let host = HostContext::current();
    format!(
        "Review this RunFlow workflow YAML. Do not edit the file.\n\nHost context:\n- Target host OS: {}\n- Target host family: {}\n- Ping count flag for this host: {}\n\nFocus on:\n- schema issues and unknown fields;\n- missing command step timeouts;\n- command arguments that do not match the target host;\n- risky shell control tokens, broad env usage, suspicious commands, raw secrets, and unbounded output;\n- findings that are actionable for a human reviewer.\n\nRules:\n- Return strict JSON only.\n- Severity must be one of error, warning, info.\n- Path must be a JSON-pointer-like path such as /steps/0/run.\n- Suggest manual review or an explicit YAML change; never suggest automatic execution.\n- Prefer commands compatible with the target host.\n- Do not flag ping args [\"{}\", \"4\", \"<host>\"] as incompatible on this host.\n- Do not invent alternate ping flags for this host.\n- Do not suggest target metadata; this workflow schema has no target OS metadata field.\n\nWorkflow YAML:\n{}\n\nReturn exactly this JSON shape:\n{{\"kind\":\"workflow_review\",\"valid\":true,\"findings\":[{{\"severity\":\"warning\",\"path\":\"/steps/0/run\",\"message\":\"...\",\"suggestion\":\"...\"}}]}}",
        host.os,
        host.family,
        host.ping_count_flag(),
        host.ping_count_flag(),
        util::truncate(yaml, MAX_WORKFLOW_CHARS)
    )
}

fn is_spurious_host_ping_finding(yaml: &str, finding: &Finding) -> bool {
    if !workflow_has_host_compatible_ping(yaml) {
        return false;
    }
    let text = format!(
        "{}\n{}\n{}",
        finding.path, finding.message, finding.suggestion
    )
    .to_lowercase();
    text.contains("ping")
        && (text.contains("target host")
            || text.contains("target context")
            || text.contains("target metadata")
            || text.contains("windows-specific")
            || text.contains("target host family")
            || text.contains("fully compatible")
            || text.contains("execution context")
            || text.contains("supported on the target"))
}

fn workflow_has_host_compatible_ping(yaml: &str) -> bool {
    let host = HostContext::current();
    let Ok(value) = serde_yaml::from_str::<serde_yaml::Value>(yaml) else {
        return false;
    };
    let Some(steps) = value
        .as_mapping()
        .and_then(|root| root.get(yaml_key("steps")))
        .and_then(serde_yaml::Value::as_sequence)
    else {
        return false;
    };

    steps.iter().any(|step| {
        let Some(run) = step
            .as_mapping()
            .and_then(|step_map| step_map.get(yaml_key("run")))
            .and_then(serde_yaml::Value::as_mapping)
        else {
            return false;
        };
        let command_is_ping = run
            .get(yaml_key("command"))
            .and_then(serde_yaml::Value::as_str)
            .map(|command| command.eq_ignore_ascii_case("ping"))
            .unwrap_or(false);
        let first_arg = run
            .get(yaml_key("args"))
            .and_then(serde_yaml::Value::as_sequence)
            .and_then(|args| args.first())
            .and_then(serde_yaml::Value::as_str);
        command_is_ping && first_arg == Some(host.ping_count_flag())
    })
}

pub fn explain_run(run_id: &str) -> Result<RunExplanation, String> {
    let evidence_data = collect_run_evidence(run_id);
    if evidence_data.is_empty() {
        return Err(format!("run '{run_id}' not found under .flow/runs or logs"));
    }

    let combined_metadata = format!(
        "{}\n{}\n{}",
        evidence_data.manifest, evidence_data.events, evidence_data.step_metadata
    );
    let status = detect_status(
        &evidence_data.manifest,
        &evidence_data.events,
        &evidence_data.stderr,
    );
    let failed_step = if matches!(status.as_str(), "FAILED" | "ERROR") {
        detect_failed_step(&evidence_data, &combined_metadata)
    } else {
        None
    };
    let exit_code = extract_exit_code(&combined_metadata);
    let probable_cause = infer_probable_cause(
        exit_code.as_deref(),
        &evidence_data.stderr,
        &evidence_data.events,
    );
    let mut evidence = Vec::new();

    if !evidence_data.sources.is_empty() {
        evidence.push(format!("sources: {}", evidence_data.sources.join(", ")));
    }
    if let Some(code) = &exit_code {
        evidence.push(format!("exit_code={code}"));
    }
    if !evidence_data.events.trim().is_empty() {
        for item in summarize_recent_events(&evidence_data.events, 3) {
            evidence.push(item);
        }
    }
    if let Some(line) = first_non_empty_line(&evidence_data.stderr) {
        evidence.push(format!(
            "stderr excerpt: {}",
            util::truncate(line.trim(), 180)
        ));
    }
    if let Some(line) = first_non_empty_line(&evidence_data.stdout) {
        evidence.push(format!(
            "stdout excerpt: {}",
            util::truncate(line.trim(), 180)
        ));
    }
    if let Some(cause) = &probable_cause {
        evidence.push(format!("probable_cause: {cause}"));
    }
    if evidence.is_empty() {
        evidence.push(
            "no detailed evidence found; only run directory presence was confirmed".to_string(),
        );
    }

    let step_text = failed_step.as_deref().unwrap_or("a step");
    let summary = if status == "FAILED" {
        if let Some(cause) = &probable_cause {
            format!("{step_text} appears to have failed: {cause}. Review the evidence before retrying manually.")
        } else {
            format!("{step_text} appears to have failed. Review the evidence and RunFlow logs before retrying manually.")
        }
    } else {
        format!("Run status detected as {status}. No automatic action was taken.")
    };

    let suggested_next_steps = suggested_run_steps(status.as_str(), probable_cause.as_deref());

    Ok(RunExplanation {
        run_id: run_id.to_string(),
        status,
        summary,
        failed_step,
        evidence,
        suggested_next_steps,
    })
}

pub fn explain_run_with_model(
    run_id: &str,
    config: &ModelConfig,
) -> Result<RunExplanation, String> {
    let mut explanation = explain_run(run_id)?;
    if config.is_mock() {
        return Ok(explanation);
    }

    let user = explain_run_prompt(&explanation);
    let raw = model::chat(
        config,
        strict_json_system("Return only a run_explanation JSON object."),
        &user,
    )?;
    let parsed: RunExplanationModelResponse = decode_model_json_with_repair(
        &raw,
        "run_explanation",
        "{\"kind\":\"run_explanation\",\"summary\":\"...\",\"suggested_next_steps\":[\"...\"]}",
        config,
    )?;
    explanation.summary = parsed.summary;
    let steps = parsed.suggested_next_steps;
    if !steps.is_empty() {
        explanation.suggested_next_steps = steps;
    }
    Ok(explanation)
}

fn explain_run_prompt(explanation: &RunExplanation) -> String {
    format!(
        "Explain this RunFlow run from structured local evidence. Do not invent missing logs. Do not propose automatic retry, cancellation, notification, or command execution.\n\nRules:\n- Return strict JSON only.\n- summary must be concise and evidence-based.\n- suggested_next_steps must be manual, bounded, and safe.\n- Prefer checking workflow YAML, failed step metadata, stdout/stderr excerpts, and RunFlow logs.\n\nrun_id: {}\nstatus: {}\nfailed_step: {}\nevidence:\n{}\n\nReturn exactly this JSON shape:\n{{\"kind\":\"run_explanation\",\"summary\":\"...\",\"suggested_next_steps\":[\"...\"]}}",
        explanation.run_id,
        explanation.status,
        explanation.failed_step.as_deref().unwrap_or(""),
        util::truncate(
            &explanation
                .evidence
                .iter()
                .map(|item| format!("- {item}"))
                .collect::<Vec<_>>()
                .join("\n"),
            MAX_EVIDENCE_CHARS
        )
    )
}

pub fn daily_report(from: &str, to: &str) -> Result<DailyReport, String> {
    let mut total = 0;
    let mut success = 0;
    let mut failed = 0;
    let mut cancelled = 0;
    let mut incidents = Vec::new();

    let runs_root = Path::new(".flow").join("runs");
    if runs_root.exists() {
        for entry in fs::read_dir(&runs_root).map_err(|e| format!("cannot read .flow/runs: {e}"))? {
            let entry = entry.map_err(|e| format!("cannot read run directory: {e}"))?;
            if !entry.path().is_dir() {
                continue;
            }
            total += 1;
            let run_id = entry.file_name().to_string_lossy().to_string();
            let manifest = read_optional(entry.path().join("manifest.json")).unwrap_or_default();
            let events = read_optional(entry.path().join("events.jsonl")).unwrap_or_default();
            let status = detect_status(&manifest, &events, "");
            match status.as_str() {
                "SUCCESS" | "SUCCEEDED" | "COMPLETED" => success += 1,
                "FAILED" | "ERROR" => {
                    failed += 1;
                    incidents.push(run_id);
                }
                "CANCELLED" | "CANCELED" => cancelled += 1,
                _ => {}
            }
        }
    }

    let mut recommendations = Vec::new();
    if failed > 0 {
        recommendations
            .push("Review failed runs with runflow-agent explain-run <run_id>.".to_string());
    }
    if total == 0 {
        recommendations.push("No local runs found in .flow/runs for this workspace.".to_string());
    }

    Ok(DailyReport {
        from: from.to_string(),
        to: to.to_string(),
        total,
        success,
        failed,
        cancelled,
        unstable_jobs: Vec::new(),
        incidents,
        recommendations,
    })
}

pub fn daily_report_with_model(
    from: &str,
    to: &str,
    config: &ModelConfig,
) -> Result<DailyReport, String> {
    let mut report = daily_report(from, to)?;
    if config.is_mock() {
        return Ok(report);
    }

    let user = daily_report_prompt(&report);
    let raw = model::chat(
        config,
        strict_json_system("Return only a daily_report JSON object."),
        &user,
    )?;
    let parsed: DailyReportModelResponse = decode_model_json_with_repair(
        &raw,
        "daily_report",
        "{\"kind\":\"daily_report\",\"recommendations\":[\"...\"]}",
        config,
    )?;
    let recommendations = parsed.recommendations;
    if !recommendations.is_empty() {
        report.recommendations = recommendations;
    }
    Ok(report)
}

fn daily_report_prompt(report: &DailyReport) -> String {
    format!(
        "Create manual recommendations for this local RunFlow daily report.\n\nRules:\n- Return strict JSON only.\n- Do not suggest sending alerts, editing secrets, mutating workflows, or automatically running commands.\n- Recommendations must be concrete and based only on the provided counts/incidents.\n- If no runs exist, say what local data should be checked next.\n\n{}\n\nReturn exactly this JSON shape:\n{{\"kind\":\"daily_report\",\"recommendations\":[\"...\"]}}",
        util::truncate(
            &format!(
                "period: {} -> {}\nruns: total={} success={} failed={} cancelled={}\nincidents: {}",
                report.from,
                report.to,
                report.total,
                report.success,
                report.failed,
                report.cancelled,
                if report.incidents.is_empty() {
                    "none".to_string()
                } else {
                    report.incidents.join(", ")
                }
            ),
            MAX_REPORT_CHARS
        )
    )
}

fn strict_json_system(task: &str) -> &'static str {
    let _ = task;
    "You are RunFlow Agent. You are assist-only. Return strict JSON only. No markdown, no prose, no tool calls. Never execute, cancel, rerun, schedule, notify, edit secrets, call external APIs, or mutate project state."
}

fn parse_findings_with_repair(raw: &str, config: &ModelConfig) -> Result<Vec<Finding>, String> {
    let parsed: ReviewModelResponse = decode_model_json_with_repair(
        raw,
        "workflow_review",
        "{\"kind\":\"workflow_review\",\"valid\":true,\"findings\":[{\"severity\":\"warning\",\"path\":\"/steps/0/run\",\"message\":\"...\",\"suggestion\":\"...\"}]}",
        config,
    )?;
    Ok(parsed.findings.into_iter().map(Finding::from).collect())
}

fn decode_model_json_with_repair<T>(
    raw: &str,
    expected_kind: &str,
    expected_shape: &str,
    config: &ModelConfig,
) -> Result<T, String>
where
    T: DeserializeOwned + ModelKind,
{
    match decode_model_json(raw, expected_kind) {
        Ok(parsed) => Ok(parsed),
        Err(original_error) => {
            let user =
                repair_model_json_prompt(raw, &original_error, expected_kind, expected_shape);
            let repaired_raw = model::chat(
                config,
                strict_json_system("Repair model JSON output."),
                &user,
            )
            .map_err(|repair_error| {
                format!("{original_error}; failed to repair model output: {repair_error}")
            })?;
            decode_model_json(&repaired_raw, expected_kind).map_err(|repair_error| {
                format!("{original_error}; repaired model output is still invalid: {repair_error}")
            })
        }
    }
}

fn repair_model_json_prompt(
    raw: &str,
    decode_error: &str,
    expected_kind: &str,
    expected_shape: &str,
) -> String {
    format!(
        "Repair the previous model output so it is valid strict JSON for RunFlow Agent.\n\nRules:\n- Return one JSON object only.\n- No markdown, prose, comments, or trailing text.\n- Preserve the useful intent from the invalid output when possible.\n- The kind field must be exactly \"{}\".\n- Use this exact shape:\n{}\n\nDecode error:\n{}\n\nInvalid output:\n{}",
        expected_kind,
        expected_shape,
        util::truncate(decode_error, MAX_REPORT_CHARS),
        util::truncate(raw, MAX_REPORT_CHARS)
    )
}

fn extract_every_minutes(text: &str) -> Option<u32> {
    let marker = "every ";
    let idx = text.find(marker)?;
    let rest = &text[idx + marker.len()..];
    let number = rest.split_whitespace().next()?.parse::<u32>().ok()?;
    if rest.contains("minute") && number > 0 {
        Some(number)
    } else {
        None
    }
}

fn extract_ipv4(text: &str) -> Option<String> {
    text.split_whitespace()
        .map(|part| part.trim_matches(|c: char| !c.is_ascii_digit() && c != '.'))
        .find(|part| {
            let pieces = part.split('.').collect::<Vec<_>>();
            pieces.len() == 4 && pieces.iter().all(|p| p.parse::<u8>().is_ok())
        })
        .map(ToString::to_string)
}

fn read_optional(path: PathBuf) -> Option<String> {
    fs::read_to_string(path).ok()
}

fn collect_run_evidence(run_id: &str) -> RunEvidence {
    let run_dir = Path::new(".flow").join("runs").join(run_id);
    let logs_dir = Path::new("logs").join(run_id);
    let mut evidence = RunEvidence {
        manifest: String::new(),
        events: String::new(),
        step_metadata: String::new(),
        stdout: String::new(),
        stderr: String::new(),
        sources: Vec::new(),
    };

    read_source_into(
        run_dir.join("manifest.json"),
        &mut evidence.manifest,
        &mut evidence.sources,
    );
    read_source_into(
        logs_dir.join("workflow.metadata.json"),
        &mut evidence.manifest,
        &mut evidence.sources,
    );
    read_source_into(
        run_dir.join("events.jsonl"),
        &mut evidence.events,
        &mut evidence.sources,
    );
    read_source_into(
        run_dir.join("stdout.log"),
        &mut evidence.stdout,
        &mut evidence.sources,
    );
    read_source_into(
        run_dir.join("stderr.log"),
        &mut evidence.stderr,
        &mut evidence.sources,
    );

    let (step_metadata, step_sources) = collect_named_files(&logs_dir, "step.metadata.json");
    append_text(&mut evidence.step_metadata, &step_metadata);
    evidence.sources.extend(step_sources);

    let (stdout, stdout_sources) = collect_named_files(&logs_dir, "stdout.log");
    append_text(&mut evidence.stdout, &stdout);
    evidence.sources.extend(stdout_sources);

    let (stderr, stderr_sources) = collect_named_files(&logs_dir, "stderr.log");
    append_text(&mut evidence.stderr, &stderr);
    evidence.sources.extend(stderr_sources);

    evidence.sources.sort();
    evidence.sources.dedup();
    evidence
}

fn read_source_into(path: PathBuf, out: &mut String, sources: &mut Vec<String>) {
    if let Ok(text) = read_text_lossy(&path) {
        append_text(out, &text);
        sources.push(path.display().to_string());
    }
}

fn collect_named_files(root: &Path, file_name: &str) -> (String, Vec<String>) {
    let mut out = String::new();
    let mut sources = Vec::new();
    collect_named_files_inner(root, file_name, 0, &mut out, &mut sources);
    (out, sources)
}

fn collect_named_files_inner(
    root: &Path,
    file_name: &str,
    depth: usize,
    out: &mut String,
    sources: &mut Vec<String>,
) {
    if depth > 3 || !root.exists() {
        return;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            let candidate = path.join(file_name);
            if let Ok(text) = read_text_lossy(&candidate) {
                append_text(out, &text);
                sources.push(candidate.display().to_string());
            }
            collect_named_files_inner(&path, file_name, depth + 1, out, sources);
        }
    }
}

fn append_text(out: &mut String, text: &str) {
    if text.is_empty() {
        return;
    }
    if !out.is_empty() && !out.ends_with('\n') {
        out.push('\n');
    }
    out.push_str(text);
    if !out.ends_with('\n') {
        out.push('\n');
    }
}

fn read_text_lossy(path: &Path) -> Result<String, std::io::Error> {
    fs::read(path).map(|bytes| decode_log_bytes(&bytes))
}

fn decode_log_bytes(bytes: &[u8]) -> String {
    if let Ok(text) = std::str::from_utf8(bytes) {
        return text.to_string();
    }
    bytes.iter().map(|byte| decode_oem_byte(*byte)).collect()
}

fn decode_oem_byte(byte: u8) -> char {
    if byte.is_ascii() {
        return byte as char;
    }
    match byte {
        0x80 => 'Ç',
        0x81 => 'ü',
        0x82 => 'é',
        0x83 => 'â',
        0x84 => 'ä',
        0x85 => 'à',
        0x86 => 'å',
        0x87 => 'ç',
        0x88 => 'ê',
        0x89 => 'ë',
        0x8a => 'è',
        0x8b => 'ï',
        0x8c => 'î',
        0x8d => 'ì',
        0x8e => 'Ä',
        0x8f => 'Å',
        0x90 => 'É',
        0x91 => 'æ',
        0x92 => 'Æ',
        0x93 => 'ô',
        0x94 => 'ö',
        0x95 => 'ò',
        0x96 => 'û',
        0x97 => 'ù',
        0x98 => 'ÿ',
        0x99 => 'Ö',
        0x9a => 'Ü',
        0xa0 => 'á',
        0xa1 => 'í',
        0xa2 => 'ó',
        0xa3 => 'ú',
        0xa4 => 'ñ',
        0xa5 => 'Ñ',
        0xff => '\u{00a0}',
        _ => '�',
    }
}

fn detect_status(manifest: &str, events: &str, stderr: &str) -> String {
    if let Some(status) =
        json_field_value_from_text(manifest, &["status", "state", "outcome", "result"]).or_else(
            || json_field_value_from_jsonl(events, &["status", "state", "outcome", "result"], true),
        )
    {
        let normalized = normalize_status(&status);
        if !normalized.is_empty() {
            return normalized;
        }
    }

    let combined = format!("{manifest}\n{events}").to_uppercase();
    for status in [
        "FAILED",
        "ERROR",
        "CANCELLED",
        "CANCELED",
        "SUCCESS",
        "SUCCEEDED",
        "COMPLETED",
        "RUNNING",
    ] {
        if combined.contains(status) {
            return if status == "CANCELED" {
                "CANCELLED".to_string()
            } else {
                status.to_string()
            };
        }
    }
    if !stderr.trim().is_empty() {
        "FAILED".to_string()
    } else {
        "UNKNOWN".to_string()
    }
}

fn detect_failed_step(evidence: &RunEvidence, combined_metadata: &str) -> Option<String> {
    if let Some(value) = json_field_value_from_text(
        combined_metadata,
        &[
            "failed_step",
            "failedStep",
            "step_name",
            "stepName",
            "step_id",
            "stepId",
            "step",
        ],
    )
    .or_else(|| {
        json_field_value_from_jsonl(
            &evidence.events,
            &[
                "failed_step",
                "failedStep",
                "step_name",
                "stepName",
                "step_id",
                "stepId",
                "step",
            ],
            true,
        )
    }) {
        return Some(value);
    }
    infer_step_from_stderr_source(&evidence.sources)
}

fn extract_exit_code(text: &str) -> Option<String> {
    json_field_value_from_text(text, &["exit_code", "exitCode", "code"])
        .or_else(|| util::extract_json_number(text, "exit_code"))
        .or_else(|| util::extract_json_number(text, "code"))
}

fn normalize_status(status: &str) -> String {
    let value = status.trim().to_uppercase();
    match value.as_str() {
        "CANCELED" => "CANCELLED".to_string(),
        "FAILURE" => "FAILED".to_string(),
        "OK" => "SUCCESS".to_string(),
        _ => value,
    }
}

fn summarize_recent_events(events: &str, max: usize) -> Vec<String> {
    events
        .lines()
        .filter(|line| !line.trim().is_empty())
        .rev()
        .take(max)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .map(format_event_excerpt)
        .collect()
}

fn format_event_excerpt(line: &str) -> String {
    let line = clean_log_line(line);
    if let Ok(value) = serde_json::from_str::<Value>(line) {
        let mut parts = Vec::new();
        for (label, keys) in [
            ("time", ["timestamp", "time", "ts"].as_slice()),
            ("event", ["event", "type", "kind"].as_slice()),
            (
                "status",
                ["status", "state", "outcome", "result"].as_slice(),
            ),
            (
                "step",
                ["step", "step_name", "stepName", "step_id", "stepId"].as_slice(),
            ),
            ("message", ["message", "error", "reason"].as_slice()),
        ] {
            if let Some(found) = json_field_value(&value, keys) {
                parts.push(format!("{label}={}", util::truncate(&found, 80)));
            }
        }
        if !parts.is_empty() {
            return format!("event: {}", parts.join(" "));
        }
    }
    format!("event: {}", util::truncate(line.trim(), 180))
}

fn first_non_empty_line(text: &str) -> Option<&str> {
    text.lines()
        .map(clean_log_line)
        .find(|line| !line.trim().is_empty())
}

fn clean_log_line(line: &str) -> &str {
    line.trim_start_matches('\u{feff}')
}

fn infer_probable_cause(exit_code: Option<&str>, stderr: &str, events: &str) -> Option<String> {
    let combined = format!("{stderr}\n{events}").to_lowercase();
    if combined.contains("timed out") || combined.contains("timeout") {
        return Some("the step appears to have timed out".to_string());
    }
    if combined.contains("permission denied") || combined.contains("access is denied") {
        return Some("the step hit a permissions error".to_string());
    }
    if combined.contains("not found")
        || combined.contains("is not recognized")
        || combined.contains("no such file")
    {
        return Some("a command or file path was not found".to_string());
    }
    if combined.contains("connection refused") || combined.contains("could not connect") {
        return Some("a dependency endpoint refused the connection".to_string());
    }
    if combined.contains("authentication") || combined.contains("unauthorized") {
        return Some("authentication or authorization failed".to_string());
    }
    if let Some(code) = exit_code {
        if code != "0" {
            return Some(format!("the failed process exited with code {code}"));
        }
    }
    None
}

fn suggested_run_steps(status: &str, probable_cause: Option<&str>) -> Vec<String> {
    if !matches!(status, "FAILED" | "ERROR") {
        return vec![
            "Review run logs if you need execution details.".to_string(),
            "No automatic action was taken.".to_string(),
        ];
    }

    let mut steps = vec![
        "Inspect the workflow YAML and failed step metadata.".to_string(),
        "Check bounded stdout/stderr excerpts for the first concrete error.".to_string(),
    ];
    if let Some(cause) = probable_cause {
        steps.push(format!(
            "Verify this likely cause before retrying manually: {cause}."
        ));
    }
    if status == "FAILED" || status == "ERROR" {
        steps
            .push("Run or retry manually with RunFlow only after reviewing the cause.".to_string());
    } else {
        steps.push("No automatic action was taken.".to_string());
    }
    steps
}

fn infer_step_from_stderr_source(sources: &[String]) -> Option<String> {
    for source in sources {
        if !source.ends_with("stderr.log") {
            continue;
        }
        let path = Path::new(source);
        if let Some(parent) = path.parent().and_then(Path::file_name) {
            let name = parent.to_string_lossy().to_string();
            if !name.is_empty() {
                return Some(name);
            }
        }
    }
    None
}

fn json_field_value_from_text(text: &str, keys: &[&str]) -> Option<String> {
    if let Ok(value) = serde_json::from_str::<Value>(text) {
        return json_field_value(&value, keys);
    }
    json_field_value_from_jsonl(text, keys, false)
}

fn json_field_value_from_jsonl(text: &str, keys: &[&str], reverse: bool) -> Option<String> {
    let lines = text.lines().filter(|line| !line.trim().is_empty());
    if reverse {
        for line in lines.collect::<Vec<_>>().into_iter().rev() {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                if let Some(found) = json_field_value(&value, keys) {
                    return Some(found);
                }
            }
        }
    } else {
        for line in lines {
            if let Ok(value) = serde_json::from_str::<Value>(line) {
                if let Some(found) = json_field_value(&value, keys) {
                    return Some(found);
                }
            }
        }
    }
    None
}

fn json_field_value(value: &Value, keys: &[&str]) -> Option<String> {
    match value {
        Value::Object(map) => {
            for key in keys {
                if let Some(value) = map.get(*key).and_then(json_scalar_to_string) {
                    return Some(value);
                }
            }
            for nested in map.values() {
                if let Some(value) = json_field_value(nested, keys) {
                    return Some(value);
                }
            }
            None
        }
        Value::Array(items) => items.iter().find_map(|item| json_field_value(item, keys)),
        _ => None,
    }
}

fn json_scalar_to_string(value: &Value) -> Option<String> {
    match value {
        Value::String(value) => Some(value.clone()),
        Value::Number(value) => Some(value.to_string()),
        Value::Bool(value) => Some(value.to_string()),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::{Arc, Mutex};
    use std::thread;
    use std::time::Duration;

    static CWD_LOCK: Mutex<()> = Mutex::new(());

    #[test]
    fn drafts_ping_workflow() {
        let draft = draft_workflow("Ping 1.1.1.1 every 5 minutes");
        assert!(draft
            .workflow_yaml
            .contains("name: ping-1-1-1-1-every-5-minutes"));
        assert!(draft.workflow_yaml.contains("command: ping"));
        assert!(draft.workflow_yaml.contains("*/5"));
    }

    #[test]
    fn review_flags_missing_timeout() {
        let findings = review_workflow(
            "name: demo\nsteps:\n  - name: x\n    type: command\n    run:\n      command: echo\n",
        );
        assert!(findings.iter().any(|f| f.message.contains("timeout")));
    }

    #[test]
    fn draft_prompt_documents_schema_constraints() {
        let host = HostContext::current();
        let prompt = draft_workflow_prompt("Ping 1.1.1.1");
        assert!(prompt.contains("Top-level name must be kebab-case"));
        assert!(prompt.contains("Never put timeout inside run"));
        assert!(prompt.contains("Never use schedule: true"));
        assert!(prompt.contains("allow, forbid, queue, replace"));
        assert!(prompt.contains("timeout: 30s\n    run:"));
        assert!(prompt.contains("Target host OS"));
        assert!(prompt.contains("Target host family"));
        assert!(prompt.contains(host.ping_count_flag()));
    }

    #[test]
    fn model_draft_adapts_ping_args_for_host() {
        let host = HostContext::current();
        let config = test_provider(&format!(
            "{{\"kind\":\"draft_workflow\",\"workflow_yaml\":\"name: ping-monitor\\nversion: 1\\nschema_version: 1\\nschedule: false\\nsteps:\\n  - name: ping\\n    type: command\\n    timeout: 30s\\n    run:\\n      command: ping\\n      args: [\\\"{}\\\", \\\"4\\\", \\\"1.1.1.1\\\"]\\n\",\"warnings\":[]}}",
            host.wrong_ping_count_flag()
        ));
        let draft = draft_workflow_with_model("Ping 1.1.1.1", &config).unwrap();

        assert!(draft.workflow_yaml.contains(host.ping_count_flag()));
        assert!(!draft
            .workflow_yaml
            .contains(&format!("\"{}\"", host.wrong_ping_count_flag())));
        assert!(draft
            .warnings
            .iter()
            .any(|warning| warning.contains("Adjusted ping args")));
    }

    #[test]
    fn review_flags_ping_args_for_wrong_host() {
        let host = HostContext::current();
        let yaml = format!(
            "name: ping-monitor\nversion: 1\nschema_version: 1\nschedule: false\nsteps:\n  - name: ping\n    type: command\n    timeout: 30s\n    run:\n      command: ping\n      args: [\"{}\", \"4\", \"1.1.1.1\"]\n",
            host.wrong_ping_count_flag()
        );
        let findings = review_workflow(&yaml);

        assert!(findings
            .iter()
            .any(|finding| finding.message.contains("does not match target host OS")));
    }

    #[test]
    fn review_filters_spurious_host_ping_metadata_finding() {
        let host = HostContext::current();
        let yaml = format!(
            "name: ping-monitor\nversion: 1\nschema_version: 1\nschedule: false\nsteps:\n  - name: ping\n    type: command\n    timeout: 30s\n    run:\n      command: ping\n      args: [\"{}\", \"4\", \"1.1.1.1\"]\n",
            host.ping_count_flag()
        );
        let config = test_provider(
            "{\"kind\":\"workflow_review\",\"valid\":true,\"findings\":[{\"severity\":\"warning\",\"path\":\"/steps/0/run\",\"message\":\"The ping command is Windows-specific and needs target metadata.\",\"suggestion\":\"Add target host family metadata.\"}]}",
        );
        let findings = review_workflow_with_model(&yaml, &config).unwrap();

        assert!(findings.is_empty());
    }

    #[test]
    fn repair_prompt_includes_validation_errors() {
        let prompt = repair_draft_workflow_prompt(
            "Ping 1.1.1.1",
            "name: Bad Name\nschedule: true\n",
            &["/schedule: true is not valid".to_string()],
        );
        assert!(prompt.contains("failed schema validation"));
        assert!(prompt.contains("/schedule: true is not valid"));
        assert!(prompt.contains("Invalid workflow_yaml"));
    }

    #[test]
    fn review_prompt_documents_safety_constraints() {
        let prompt = review_workflow_prompt("name: demo\nsteps: []\n");
        assert!(prompt.contains("Severity must be one of error, warning, info"));
        assert!(prompt.contains("never suggest automatic execution"));
        assert!(prompt.contains("command arguments that do not match"));
        assert!(prompt.contains("workflow_review"));
    }

    #[test]
    fn explain_prompt_documents_safety_constraints() {
        let prompt = explain_run_prompt(&RunExplanation {
            run_id: "run-1".to_string(),
            status: "FAILED".to_string(),
            summary: "failed".to_string(),
            failed_step: Some("task".to_string()),
            evidence: vec!["stderr excerpt: nope".to_string()],
            suggested_next_steps: Vec::new(),
        });
        assert!(prompt.contains("Do not invent missing logs"));
        assert!(prompt.contains("manual, bounded, and safe"));
        assert!(prompt.contains("run_explanation"));
    }

    #[test]
    fn daily_report_prompt_documents_safety_constraints() {
        let prompt = daily_report_prompt(&DailyReport {
            from: "2026-06-10".to_string(),
            to: "2026-06-11".to_string(),
            total: 1,
            success: 0,
            failed: 1,
            cancelled: 0,
            unstable_jobs: Vec::new(),
            incidents: vec!["run-1".to_string()],
            recommendations: Vec::new(),
        });
        assert!(prompt.contains("Do not suggest sending alerts"));
        assert!(prompt.contains("based only on the provided counts"));
        assert!(prompt.contains("daily_report"));
    }

    #[test]
    fn review_repairs_invalid_json_output() {
        let config = test_provider_sequence(vec![
            "```json\n{\"kind\":\"workflow_review\",\"valid\":true,\"findings\":[]}\n```",
            "{\"kind\":\"workflow_review\",\"valid\":true,\"findings\":[{\"severity\":\"warning\",\"path\":\"/steps/0/run\",\"message\":\"Command step has no timeout.\",\"suggestion\":\"Add timeout: 30s.\"}]}",
        ]);
        let findings = review_workflow_with_model(
            "name: demo\nsteps:\n  - name: task\n    type: command\n    run:\n      command: echo\n",
            &config,
        )
        .unwrap();
        assert!(findings
            .iter()
            .any(|finding| finding.message.contains("Command step has no timeout")));
    }

    #[test]
    fn explain_run_repairs_invalid_json_output() {
        let config = test_provider_sequence(vec![
            "{\"kind\":\"run_explanation\",\"suggested_next_steps\":[]}",
            "{\"kind\":\"run_explanation\",\"summary\":\"The task failed with exit code 1.\",\"suggested_next_steps\":[\"Inspect stderr before retrying manually.\"]}",
        ]);
        let explanation = explain_run_with_model_for_test(&config).unwrap();
        assert!(explanation.summary.contains("exit code 1"));
        assert!(explanation.suggested_next_steps[0].contains("manually"));
    }

    #[test]
    fn daily_report_repairs_invalid_json_output() {
        let config = test_provider_sequence(vec![
            "Recommendations: inspect failed runs",
            "{\"kind\":\"daily_report\",\"recommendations\":[\"Inspect failed runs with explain-run before manual retry.\"]}",
        ]);
        let report = daily_report_with_model_for_test(&config).unwrap();
        assert_eq!(
            report.recommendations,
            vec!["Inspect failed runs with explain-run before manual retry.".to_string()]
        );
    }

    #[test]
    fn explain_run_reads_events_metadata_and_step_logs() {
        let _guard = CWD_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "runflow-agent-evidence-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let run_id = "run-with-logs";
        let run_dir = root.join(".flow").join("runs").join(run_id);
        let step_dir = root.join("logs").join(run_id).join("build");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::create_dir_all(&step_dir).unwrap();
        std::fs::write(run_dir.join("manifest.json"), "{\"status\":\"FAILED\"}").unwrap();
        std::fs::write(
            run_dir.join("events.jsonl"),
            "{\"event\":\"step_started\",\"step\":\"build\",\"status\":\"RUNNING\"}\n{\"event\":\"step_failed\",\"step\":\"build\",\"status\":\"FAILED\",\"message\":\"command failed\"}\n",
        )
        .unwrap();
        std::fs::write(step_dir.join("step.metadata.json"), "{\"exitCode\":127}").unwrap();
        std::fs::write(step_dir.join("stdout.log"), "installing deps\n").unwrap();
        std::fs::write(
            step_dir.join("stderr.log"),
            "command not found: cargo-nextest\n",
        )
        .unwrap();

        let old_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let explanation = explain_run(run_id).unwrap();
        std::env::set_current_dir(old_dir).unwrap();
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(explanation.status, "FAILED");
        assert_eq!(explanation.failed_step.as_deref(), Some("build"));
        assert!(explanation
            .evidence
            .iter()
            .any(|item| item.contains("exit_code=127")));
        assert!(explanation
            .evidence
            .iter()
            .any(|item| item.contains("event:") && item.contains("step=build")));
        assert!(explanation
            .evidence
            .iter()
            .any(|item| item.contains("stdout excerpt: installing deps")));
        assert!(explanation
            .summary
            .contains("command or file path was not found"));
    }

    #[test]
    fn explain_run_success_has_no_failed_step() {
        let _guard = CWD_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "runflow-agent-success-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let run_id = "run-success";
        let run_dir = root.join(".flow").join("runs").join(run_id);
        let step_dir = root.join("logs").join(run_id).join("ping");
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::create_dir_all(&step_dir).unwrap();
        std::fs::write(run_dir.join("manifest.json"), "{\"status\":\"SUCCESS\"}").unwrap();
        std::fs::write(
            run_dir.join("events.jsonl"),
            "{\"event_type\":\"RUN_FINISHED\",\"payload\":{\"status\":\"Success\"}}\n",
        )
        .unwrap();
        std::fs::write(
            step_dir.join("step.metadata.json"),
            "{\"step\":\"ping\",\"exit_code\":0}",
        )
        .unwrap();
        std::fs::write(step_dir.join("stdout.log"), "Reply from 1.1.1.1").unwrap();

        let old_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let explanation = explain_run(run_id).unwrap();
        std::env::set_current_dir(old_dir).unwrap();
        let _ = std::fs::remove_dir_all(root);

        assert_eq!(explanation.status, "SUCCESS");
        assert_eq!(explanation.failed_step, None);
        assert!(explanation.summary.contains("SUCCESS"));
        assert!(explanation
            .suggested_next_steps
            .iter()
            .any(|step| step.contains("No automatic action")));
        assert!(explanation
            .evidence
            .iter()
            .any(|item| item.contains("stdout excerpt: Reply from 1.1.1.1")));
    }

    #[test]
    fn decodes_common_windows_oem_log_bytes() {
        let text = decode_log_bytes(&[
            b'A', b'c', b'c', 0x8a, b's', b' ', b'r', b'e', b'f', b'u', b's', 0x82, b'.', b' ',
            b'1', 0xff, b':',
        ]);

        assert_eq!(text, "Accès refusé. 1\u{00a0}:");
    }

    #[test]
    fn model_markdown_output_is_rejected() {
        let config = test_provider(
            "```json\n{\"kind\":\"draft_workflow\",\"workflow_yaml\":\"name: x\"}\n```",
        );
        let err = draft_workflow_with_model("demo", &config).unwrap_err();
        assert!(err.contains("raw JSON"));
    }

    #[test]
    fn model_wrong_kind_is_rejected() {
        let config = test_provider("{\"kind\":\"workflow_review\",\"workflow_yaml\":\"name: x\"}");
        let err = draft_workflow_with_model("demo", &config).unwrap_err();
        assert!(err.contains("kind mismatch"));
    }

    #[test]
    fn model_missing_required_field_is_rejected() {
        let config = test_provider("{\"kind\":\"run_explanation\",\"suggested_next_steps\":[]}");
        let err = explain_run_with_model_for_test(&config).unwrap_err();
        assert!(err.contains("summary"));
    }

    fn explain_run_with_model_for_test(config: &ModelConfig) -> Result<RunExplanation, String> {
        let _guard = CWD_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "runflow-agent-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let run_id = "missing-summary";
        let run_dir = root.join(".flow").join("runs").join(run_id);
        std::fs::create_dir_all(&run_dir).unwrap();
        std::fs::write(
            run_dir.join("manifest.json"),
            "{\"status\":\"FAILED\",\"failed_step\":\"task\",\"exit_code\":1}",
        )
        .unwrap();
        let old_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let result = explain_run_with_model(run_id, config);
        std::env::set_current_dir(old_dir).unwrap();
        let _ = std::fs::remove_dir_all(root);
        result
    }

    fn daily_report_with_model_for_test(config: &ModelConfig) -> Result<DailyReport, String> {
        let _guard = CWD_LOCK.lock().unwrap();
        let root = std::env::temp_dir().join(format!(
            "runflow-agent-report-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        let old_dir = std::env::current_dir().unwrap();
        std::env::set_current_dir(&root).unwrap();
        let result = daily_report_with_model("2026-06-10", "2026-06-11", config);
        std::env::set_current_dir(old_dir).unwrap();
        let _ = std::fs::remove_dir_all(root);
        result
    }

    fn test_provider(model_content: &str) -> ModelConfig {
        test_provider_sequence(vec![model_content])
    }

    fn test_provider_sequence(model_contents: Vec<&str>) -> ModelConfig {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let responses = Arc::new(Mutex::new(
            model_contents
                .into_iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>(),
        ));
        let server_responses = Arc::clone(&responses);
        thread::spawn(move || loop {
            let model_content = {
                let mut locked = server_responses.lock().unwrap();
                if locked.is_empty() {
                    break;
                }
                locked.remove(0)
            };
            let Ok((mut stream, _)) = listener.accept() else {
                break;
            };
            let mut buf = [0u8; 4096];
            let _ = stream.read(&mut buf);
            let escaped = crate::json::escape(&model_content);
            let body = format!(
                "{{\"choices\":[{{\"message\":{{\"role\":\"assistant\",\"content\":\"{}\"}}}}]}}",
                escaped
            );
            let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                    body.len(),
                    body
                );
            let _ = stream.write_all(response.as_bytes());
        });
        ModelConfig {
            provider: "openai-compatible".to_string(),
            base_url: format!("http://127.0.0.1:{port}/v1"),
            model: "test".to_string(),
            api_key: None,
            timeout: Duration::from_secs(5),
        }
    }
}
