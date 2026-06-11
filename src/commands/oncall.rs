use std::collections::{BTreeMap, BTreeSet};
use std::fs;
use std::path::PathBuf;

use crate::audit;
use crate::cli::CliResult;
use crate::commands::inspect_workspace::{self, HealthIssue, RunSummary, WorkspaceInspection};
use crate::json;

const DEFAULT_LIMIT: usize = 100;
const DEFAULT_WINDOW_HOURS: u64 = 24;

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let options = OncallOptions::from_args(args)?;
    let handoff = build_handoff(&options)?;
    let output = if options.format_json {
        handoff.json()
    } else {
        handoff.text()
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
            .map_err(|e| format!("cannot write oncall output '{}': {e}", path.display()))?;
        changed_files.push(path.display().to_string());
    }

    let warnings = handoff
        .incidents
        .iter()
        .map(|incident| format!("{} {}", incident.id, incident.status))
        .chain(
            handoff
                .health
                .iter()
                .filter(|issue| issue.severity != "ok")
                .map(|issue| format!("{}: {}", issue.path, issue.message)),
        )
        .collect::<Vec<_>>();
    let _ = audit::record_at(
        &options.root,
        "oncall",
        "success",
        &changed_files,
        &warnings,
    );

    Ok(CliResult {
        command: "oncall".to_string(),
        output,
        status: "success",
        changed_files,
        warnings,
        audit: false,
    })
}

struct OncallOptions {
    root: PathBuf,
    limit: usize,
    window_hours: u64,
    run_id: Option<String>,
    job: Option<String>,
    format_json: bool,
    output: Option<PathBuf>,
}

impl OncallOptions {
    fn from_args(args: &[String]) -> Result<Self, String> {
        let root = value_after(args, "--root").map(PathBuf::from).unwrap_or(
            std::env::current_dir().map_err(|e| format!("cannot read current dir: {e}"))?,
        );
        let limit = value_after(args, "--limit")
            .map(str::parse::<usize>)
            .transpose()
            .map_err(|_| "--limit must be an integer".to_string())?
            .unwrap_or(DEFAULT_LIMIT);
        let window_hours = value_after(args, "--window-hours")
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| "--window-hours must be a positive integer".to_string())?
            .unwrap_or(DEFAULT_WINDOW_HOURS);
        if window_hours == 0 {
            return Err("--window-hours must be greater than zero".to_string());
        }

        Ok(Self {
            root,
            limit,
            window_hours,
            run_id: value_after(args, "--run-id").map(ToString::to_string),
            job: value_after(args, "--job").map(ToString::to_string),
            format_json: wants_json(args),
            output: value_after(args, "--output").map(PathBuf::from),
        })
    }
}

struct OncallHandoff {
    root: String,
    generated_at: String,
    window_hours: u64,
    run_id: Option<String>,
    job: Option<String>,
    incidents: Vec<OncallIncident>,
    health: Vec<HealthSnapshot>,
    title: String,
    body: String,
}

struct OncallIncident {
    id: String,
    job: String,
    status: String,
    started_at: String,
    failed_step: String,
    severity: &'static str,
    evidence: Vec<String>,
    manual_next_steps: Vec<String>,
}

struct HealthSnapshot {
    severity: &'static str,
    path: String,
    message: String,
}

fn build_handoff(options: &OncallOptions) -> Result<OncallHandoff, String> {
    let inspection = inspect_workspace::inspect(&options.root, options.limit)?;
    Ok(OncallHandoff::from_inspection(options, &inspection))
}

impl OncallHandoff {
    fn from_inspection(options: &OncallOptions, inspection: &WorkspaceInspection) -> Self {
        let candidate_runs = inspection
            .runs
            .iter()
            .filter(|run| run.status == "FAILED" || run.status == "ERROR")
            .filter(|run| options.run_id.as_ref().is_none_or(|id| run.id == *id))
            .filter(|run| options.job.as_ref().is_none_or(|job| run.job == *job))
            .filter(|run| is_within_window(&run.started_at, options.window_hours))
            .collect::<Vec<_>>();
        let mut counts_by_job = BTreeMap::new();
        for run in &candidate_runs {
            *counts_by_job.entry(run.job.as_str()).or_insert(0usize) += 1;
        }
        let incidents = candidate_runs
            .into_iter()
            .map(|run| OncallIncident::from_run(run, counts_by_job[run.job.as_str()]))
            .collect::<Vec<_>>();
        let health = inspection
            .health
            .iter()
            .map(HealthSnapshot::from)
            .collect::<Vec<_>>();
        let title = handoff_title(&incidents);
        let body = handoff_body(&incidents);

        Self {
            root: inspection.root.display().to_string(),
            generated_at: audit::timestamp_seconds(),
            window_hours: options.window_hours,
            run_id: options.run_id.clone(),
            job: options.job.clone(),
            incidents,
            health,
            title,
            body,
        }
    }

    fn text(&self) -> String {
        let mut out = vec![
            "kind: oncall_handoff".to_string(),
            format!("root: {}", self.root),
            format!("generated_at: {}", self.generated_at),
            format!("window_hours: {}", self.window_hours),
            format!("run_id: {}", option_or_null(&self.run_id)),
            format!("job: {}", option_or_null(&self.job)),
            format!("incidents: {}", self.incidents.len()),
            format!("failed_runs: {}", self.failed_runs()),
            format!("error_runs: {}", self.error_runs()),
            format!("affected_jobs: {}", self.affected_jobs()),
            format!("health_warnings: {}", self.health_warnings()),
            format!("handoff_title: {}", self.title),
            format!("handoff_body: {}", self.body),
            "incident_details:".to_string(),
        ];
        if self.incidents.is_empty() {
            out.push("- none".to_string());
        } else {
            for incident in &self.incidents {
                out.push(format!(
                    "- {} severity={} status={} job={} failed_step={}",
                    incident.id,
                    incident.severity,
                    incident.status,
                    incident.job,
                    empty_as_none(&incident.failed_step)
                ));
            }
        }
        out.push("health:".to_string());
        for issue in &self.health {
            out.push(format!(
                "- [{}] {}: {}",
                issue.severity, issue.path, issue.message
            ));
        }
        out.join("\n")
    }

    fn json(&self) -> String {
        let incidents = self
            .incidents
            .iter()
            .map(OncallIncident::json)
            .collect::<Vec<_>>()
            .join(",");
        let health = self
            .health
            .iter()
            .map(HealthSnapshot::json)
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"kind\":\"oncall_handoff\",\"root\":\"{}\",\"generated_at\":\"{}\",\"scope\":{{\"window_hours\":{},\"run_id\":{},\"job\":{}}},\"summary\":{{\"incidents\":{},\"failed_runs\":{},\"error_runs\":{},\"affected_jobs\":{},\"health_warnings\":{}}},\"incidents\":[{}],\"health\":[{}],\"handoff\":{{\"title\":\"{}\",\"body\":\"{}\"}}}}",
            json::escape(&self.root),
            json::escape(&self.generated_at),
            self.window_hours,
            json_option(&self.run_id),
            json_option(&self.job),
            self.incidents.len(),
            self.failed_runs(),
            self.error_runs(),
            self.affected_jobs(),
            self.health_warnings(),
            incidents,
            health,
            json::escape(&self.title),
            json::escape(&self.body)
        )
    }

    fn failed_runs(&self) -> usize {
        self.incidents
            .iter()
            .filter(|incident| incident.status == "FAILED")
            .count()
    }

    fn error_runs(&self) -> usize {
        self.incidents
            .iter()
            .filter(|incident| incident.status == "ERROR")
            .count()
    }

    fn affected_jobs(&self) -> usize {
        self.incidents
            .iter()
            .map(|incident| incident.job.as_str())
            .collect::<BTreeSet<_>>()
            .len()
    }

    fn health_warnings(&self) -> usize {
        self.health
            .iter()
            .filter(|issue| issue.severity != "ok")
            .count()
    }
}

impl OncallIncident {
    fn from_run(run: &RunSummary, job_incidents: usize) -> Self {
        let severity = if job_incidents > 1 {
            "critical"
        } else if !run.failed_step.is_empty() {
            "high"
        } else {
            "medium"
        };
        let mut evidence = vec![format!("manifest status {}", run.status)];
        if !run.failed_step.is_empty() {
            evidence.push(format!("failed_step {}", run.failed_step));
        }
        if !run.started_at.is_empty() {
            evidence.push(format!("started_at {}", run.started_at));
        }

        Self {
            id: run.id.clone(),
            job: run.job.clone(),
            status: run.status.clone(),
            started_at: run.started_at.clone(),
            failed_step: run.failed_step.clone(),
            severity,
            evidence,
            manual_next_steps: vec![
                format!("Review logs for incident {}.", run.id),
                "Inspect the workflow definition before changing anything.".to_string(),
                "Prepare a human escalation note with the evidence above.".to_string(),
            ],
        }
    }

    fn json(&self) -> String {
        format!(
            "{{\"id\":\"{}\",\"job\":\"{}\",\"status\":\"{}\",\"started_at\":\"{}\",\"failed_step\":\"{}\",\"severity\":\"{}\",\"evidence\":[{}],\"manual_next_steps\":[{}]}}",
            json::escape(&self.id),
            json::escape(&self.job),
            json::escape(&self.status),
            json::escape(&self.started_at),
            json::escape(&self.failed_step),
            json::escape(self.severity),
            json::string_array(&self.evidence),
            json::string_array(&self.manual_next_steps)
        )
    }
}

impl From<&HealthIssue> for HealthSnapshot {
    fn from(issue: &HealthIssue) -> Self {
        Self {
            severity: issue.severity,
            path: issue.path.clone(),
            message: issue.message.clone(),
        }
    }
}

impl HealthSnapshot {
    fn json(&self) -> String {
        format!(
            "{{\"severity\":\"{}\",\"path\":\"{}\",\"message\":\"{}\"}}",
            json::escape(self.severity),
            json::escape(&self.path),
            json::escape(&self.message)
        )
    }
}

fn handoff_title(incidents: &[OncallIncident]) -> String {
    match incidents.len() {
        0 => "No failed RunFlow incidents need review".to_string(),
        1 => "1 RunFlow incident needs review".to_string(),
        count => format!("{count} RunFlow incidents need review"),
    }
}

fn handoff_body(incidents: &[OncallIncident]) -> String {
    let Some(first) = incidents.first() else {
        return "No failed or error runs matched the selected scope.".to_string();
    };
    let step = if first.failed_step.is_empty() {
        "unknown step"
    } else {
        &first.failed_step
    };
    if incidents.len() == 1 {
        format!(
            "{} {} at {}. Review incident {} evidence before deciding next action.",
            first.job, first.status, step, first.id
        )
    } else {
        format!(
            "{} incidents matched. Start with {} {} at {}.",
            incidents.len(),
            first.job,
            first.status,
            step
        )
    }
}

fn is_within_window(started_at: &str, window_hours: u64) -> bool {
    let Some(started) = parse_timestamp_seconds(started_at) else {
        return true;
    };
    let Ok(now) = audit::timestamp_seconds().parse::<u64>() else {
        return true;
    };
    started.saturating_add(window_hours.saturating_mul(3600)) >= now
}

fn parse_timestamp_seconds(value: &str) -> Option<u64> {
    if value.is_empty() {
        return None;
    }
    if let Ok(seconds) = value.parse::<u64>() {
        return Some(seconds);
    }
    parse_rfc3339_utc_seconds(value)
}

fn parse_rfc3339_utc_seconds(value: &str) -> Option<u64> {
    let trimmed = value.strip_suffix('Z')?;
    let (date, time) = trimmed.split_once('T')?;
    let mut date_parts = date.split('-');
    let year = date_parts.next()?.parse::<i64>().ok()?;
    let month = date_parts.next()?.parse::<u32>().ok()?;
    let day = date_parts.next()?.parse::<u32>().ok()?;
    let mut time_parts = time.split(':');
    let hour = time_parts.next()?.parse::<u32>().ok()?;
    let minute = time_parts.next()?.parse::<u32>().ok()?;
    let second = time_parts.next()?.parse::<u32>().ok()?;
    if !(1..=12).contains(&month)
        || !(1..=31).contains(&day)
        || hour > 23
        || minute > 59
        || second > 59
    {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    Some(days * 86_400 + hour as u64 * 3600 + minute as u64 * 60 + second as u64)
}

fn days_from_civil(year: i64, month: u32, day: u32) -> Option<u64> {
    let year = year - i64::from(month <= 2);
    let era = if year >= 0 { year } else { year - 399 } / 400;
    let yoe = year - era * 400;
    let month = month as i64;
    let day = day as i64;
    let doy = (153 * (month + if month > 2 { -3 } else { 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    u64::try_from(days).ok()
}

fn empty_as_none(value: &str) -> &str {
    if value.is_empty() {
        "none"
    } else {
        value
    }
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
    fn empty_workspace_returns_valid_handoff() {
        let root = unique_temp_dir("oncall-empty");
        fs::create_dir_all(&root).unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(parsed["kind"], "oncall_handoff");
        assert_eq!(parsed["summary"]["incidents"], 0);
        assert!(parsed["incidents"].as_array().unwrap().is_empty());
    }

    #[test]
    fn failed_run_is_reported_as_incident() {
        let root = workspace_with_run("oncall-failed", "run-1", "backup", "FAILED", "upload");

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(result.output.contains("\"id\":\"run-1\""));
        assert!(result.output.contains("\"severity\":\"high\""));
        assert!(result.output.contains("\"failed_step\":\"upload\""));
        assert!(result.output.contains("Review logs for incident run-1."));
    }

    #[test]
    fn repeated_job_failures_are_critical() {
        let root = unique_temp_dir("oncall-critical");
        write_run(&root, "run-1", "backup", "FAILED", "upload");
        write_run(&root, "run-2", "backup", "ERROR", "verify");

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert_eq!(
            result.output.matches("\"severity\":\"critical\"").count(),
            2
        );
        assert!(result.output.contains("\"affected_jobs\":1"));
    }

    #[test]
    fn run_id_and_job_filters_limit_incidents() {
        let root = unique_temp_dir("oncall-filter");
        write_run(&root, "run-1", "backup", "FAILED", "upload");
        write_run(&root, "run-2", "deploy", "FAILED", "build");

        let by_run = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--run-id".to_string(),
            "run-1".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();
        let by_job = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--job".to_string(),
            "deploy".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(by_run.output.contains("\"id\":\"run-1\""));
        assert!(!by_run.output.contains("\"id\":\"run-2\""));
        assert!(by_job.output.contains("\"id\":\"run-2\""));
        assert!(!by_job.output.contains("\"id\":\"run-1\""));
    }

    #[test]
    fn output_writes_requested_file_and_audit() {
        let root = workspace_with_run("oncall-output", "run-1", "backup", "FAILED", "upload");
        let output = root.join("handoff").join("latest.json");

        let result = run(&[
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
        assert!(audit.contains("run-1 FAILED"));
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn window_hours_must_be_positive() {
        let err = run(&["--window-hours".to_string(), "0".to_string()]).unwrap_err();
        assert!(err.contains("greater than zero"));
    }

    #[test]
    fn manual_steps_do_not_include_denied_action_language() {
        let root = workspace_with_run("oncall-denied", "run-1", "backup", "FAILED", "upload");

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        for denied in [
            "rerun",
            "cancel",
            "execute",
            "webhook",
            "send alert",
            "page user",
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
        write_run(&root, run_id, job, status, failed_step);
        root
    }

    fn write_run(root: &std::path::Path, run_id: &str, job: &str, status: &str, failed_step: &str) {
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
