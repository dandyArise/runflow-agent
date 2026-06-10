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
    let workflow_yaml = parsed.workflow_yaml;
    let warnings = parsed.warnings;
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
    Ok(Draft {
        workflow_yaml: parsed.workflow_yaml,
        warnings: parsed.warnings,
    })
}

fn draft_workflow_prompt(request: &str) -> String {
    format!(
        "Create a RunFlow workflow draft for this request:\n\n{}\n\nSchema constraints:\n- Return only JSON, no markdown and no prose.\n- workflow_yaml must be valid YAML for the embedded RunFlow workflow schema.\n- Prefer a minimal workflow with only name, version, schema_version, schedule, and steps unless the request explicitly requires more.\n- Do not add optional top-level fields unless needed by the request.\n- Top-level name must be kebab-case: lowercase letters, digits, and hyphens only; no spaces, underscores, dots, or uppercase.\n- Step names must use lowercase letters, digits, hyphens, or underscores; no dots, spaces, or uppercase.\n- schedule must be false, a cron string, or an object with cron/timezone/enabled. Never use schedule: true.\n- Allowed top-level fields: name, version, schema_version, schedule, failure_policy, concurrency, limits, locks, secrets, notifications, retention, steps, tests.\n- If concurrency is required, policy must be one of allow, forbid, queue, replace.\n- Allowed step types: command, plugin, sleep, wait_until.\n- For command steps, put timeout at the step level as a duration string such as timeout: 30s.\n- Never put timeout inside run.\n- Prefer structured run.command and run.args. Do not inline secrets. Do not run anything.\n\nValid YAML example:\nname: ping-monitor\nversion: 1\nschema_version: 1\nschedule: false\nsteps:\n  - name: ping\n    type: command\n    timeout: 30s\n    run:\n      command: ping\n      args: [\"-n\", \"4\", \"1.1.1.1\"]\n\nReturn exactly this JSON shape:\n{{\"kind\":\"draft_workflow\",\"workflow_yaml\":\"<yaml>\",\"warnings\":[\"...\"]}}",
        util::truncate(request, MAX_REQUEST_CHARS)
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
    findings.extend(parse_findings_with_repair(&raw, config)?);
    Ok(findings)
}

fn review_workflow_prompt(yaml: &str) -> String {
    format!(
        "Review this RunFlow workflow YAML. Do not edit the file.\n\nFocus on:\n- schema issues and unknown fields;\n- missing command step timeouts;\n- risky shell control tokens, broad env usage, suspicious commands, raw secrets, and unbounded output;\n- findings that are actionable for a human reviewer.\n\nRules:\n- Return strict JSON only.\n- Severity must be one of error, warning, info.\n- Path must be a JSON-pointer-like path such as /steps/0/run.\n- Suggest manual review or an explicit YAML change; never suggest automatic execution.\n\nWorkflow YAML:\n{}\n\nReturn exactly this JSON shape:\n{{\"kind\":\"workflow_review\",\"valid\":true,\"findings\":[{{\"severity\":\"warning\",\"path\":\"/steps/0/run\",\"message\":\"...\",\"suggestion\":\"...\"}}]}}",
        util::truncate(yaml, MAX_WORKFLOW_CHARS)
    )
}

pub fn explain_run(run_id: &str) -> Result<RunExplanation, String> {
    let run_dir = Path::new(".flow").join("runs").join(run_id);
    let logs_dir = Path::new("logs").join(run_id);
    if !run_dir.exists() && !logs_dir.exists() {
        return Err(format!("run '{run_id}' not found under .flow/runs or logs"));
    }

    let manifest = read_optional(run_dir.join("manifest.json"))
        .or_else(|| read_optional(logs_dir.join("workflow.metadata.json")))
        .unwrap_or_default();
    let events = read_optional(run_dir.join("events.jsonl")).unwrap_or_default();
    let stderr = collect_named_logs(&logs_dir, "stderr.log");
    let status = detect_status(&manifest, &events, &stderr);
    let failed_step = detect_failed_step(&manifest, &events, &logs_dir);
    let mut evidence = Vec::new();

    if !manifest.is_empty() {
        evidence.push("manifest found".to_string());
    }
    if !events.is_empty() {
        evidence.push("events.jsonl found".to_string());
    }
    if let Some(code) = extract_exit_code(&manifest).or_else(|| extract_exit_code(&events)) {
        evidence.push(format!("exit_code={code}"));
    }
    if let Some(line) = stderr.lines().find(|line| !line.trim().is_empty()) {
        evidence.push(format!(
            "stderr excerpt: {}",
            util::truncate(line.trim(), 180)
        ));
    }
    if evidence.is_empty() {
        evidence.push(
            "no detailed evidence found; only run directory presence was confirmed".to_string(),
        );
    }

    let step_text = failed_step.as_deref().unwrap_or("a step");
    let summary = if status == "FAILED" {
        format!("{step_text} appears to have failed. Review the evidence and RunFlow logs before retrying manually.")
    } else {
        format!("Run status detected as {status}. No automatic action was taken.")
    };

    Ok(RunExplanation {
        run_id: run_id.to_string(),
        status,
        summary,
        failed_step,
        evidence,
        suggested_next_steps: vec![
            "Inspect the workflow YAML and failed step metadata.".to_string(),
            "Check bounded stdout/stderr excerpts for the first concrete error.".to_string(),
            "Run or retry manually with RunFlow only after reviewing the cause.".to_string(),
        ],
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

fn collect_named_logs(root: &Path, file_name: &str) -> String {
    let mut out = String::new();
    if !root.exists() {
        return out;
    }
    let Ok(entries) = fs::read_dir(root) else {
        return out;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            if let Ok(text) = fs::read_to_string(path.join(file_name)) {
                out.push_str(&text);
                out.push('\n');
            }
        }
    }
    out
}

fn detect_status(manifest: &str, events: &str, stderr: &str) -> String {
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

fn detect_failed_step(manifest: &str, events: &str, logs_dir: &Path) -> Option<String> {
    for key in ["failed_step", "step_name", "step"] {
        if let Some(value) = util::extract_json_string(manifest, key)
            .or_else(|| util::extract_json_string(events, key))
        {
            return Some(value);
        }
    }
    if logs_dir.exists() {
        for entry in fs::read_dir(logs_dir).ok()?.flatten() {
            if entry.path().join("stderr.log").exists() {
                return Some(entry.file_name().to_string_lossy().to_string());
            }
        }
    }
    None
}

fn extract_exit_code(text: &str) -> Option<String> {
    util::extract_json_number(text, "exit_code").or_else(|| util::extract_json_number(text, "code"))
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
        let prompt = draft_workflow_prompt("Ping 1.1.1.1");
        assert!(prompt.contains("Top-level name must be kebab-case"));
        assert!(prompt.contains("Never put timeout inside run"));
        assert!(prompt.contains("Never use schedule: true"));
        assert!(prompt.contains("allow, forbid, queue, replace"));
        assert!(prompt.contains("timeout: 30s\n    run:"));
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

    fn test_provider(model_content: &'static str) -> ModelConfig {
        test_provider_sequence(vec![model_content])
    }

    fn test_provider_sequence(model_contents: Vec<&'static str>) -> ModelConfig {
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
