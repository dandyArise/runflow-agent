use std::fs;
use std::path::PathBuf;

use crate::audit;
use crate::cli::CliResult;
use crate::commands::inspect_workspace::{self, HealthIssue, WorkspaceInspection};
use crate::json;

const DEFAULT_LIMIT: usize = 10;

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let root = value_after(args, "--root")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().map_err(|e| format!("cannot read current dir: {e}"))?);
    let limit = value_after(args, "--limit")
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| "--limit must be an integer".to_string())?
        .unwrap_or(DEFAULT_LIMIT);
    if let Some(interval) = value_after(args, "--interval-seconds") {
        let interval = interval
            .parse::<u64>()
            .map_err(|_| "--interval-seconds must be a positive integer".to_string())?;
        if interval == 0 {
            return Err("--interval-seconds must be greater than zero".to_string());
        }
    }
    if !args.iter().any(|arg| arg == "--once") {
        return Err(
            "watch currently supports --once only; continuous mode is not implemented".to_string(),
        );
    }

    let inspection = inspect_workspace::inspect(&root, limit)?;
    let format_json = wants_json(args);
    let snapshot = WatchSnapshot::from_inspection(&inspection);
    let output = if format_json {
        snapshot.json()
    } else {
        snapshot.text()
    };

    let mut changed_files = Vec::new();
    if let Some(path) = value_after(args, "--output").map(PathBuf::from) {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent).map_err(|e| {
                format!("cannot create output directory '{}': {e}", parent.display())
            })?;
        }
        fs::write(&path, &output)
            .map_err(|e| format!("cannot write watch output '{}': {e}", path.display()))?;
        changed_files.push(path.display().to_string());
    }

    let warnings = snapshot
        .incidents
        .iter()
        .map(|incident| format!("{} {}", incident.run_id, incident.status))
        .chain(
            snapshot
                .health
                .iter()
                .filter(|issue| issue.severity != "ok")
                .map(|issue| format!("{}: {}", issue.path, issue.message)),
        )
        .collect::<Vec<_>>();
    let _ = audit::record_at(&root, "watch", "success", &changed_files, &warnings);

    Ok(CliResult {
        command: "watch".to_string(),
        output,
        status: "success",
        changed_files,
        warnings,
        audit: false,
    })
}

struct WatchSnapshot {
    root: String,
    generated_at: String,
    jobs: usize,
    drafts: usize,
    runs: usize,
    failed_runs: usize,
    health_warnings: usize,
    incidents: Vec<Incident>,
    health: Vec<HealthSnapshot>,
    recommendations: Vec<String>,
}

struct Incident {
    run_id: String,
    job: String,
    status: String,
    failed_step: String,
    hint: String,
}

struct HealthSnapshot {
    severity: &'static str,
    path: String,
    message: String,
}

impl WatchSnapshot {
    fn from_inspection(inspection: &WorkspaceInspection) -> Self {
        let incidents = inspection
            .runs
            .iter()
            .filter(|run| run.status == "FAILED" || run.status == "ERROR")
            .map(|run| Incident {
                run_id: run.id.clone(),
                job: run.job.clone(),
                status: run.status.clone(),
                failed_step: run.failed_step.clone(),
                hint: format!("Review with runflow-agent explain-run {}", run.id),
            })
            .collect::<Vec<_>>();
        let health = inspection
            .health
            .iter()
            .map(HealthSnapshot::from)
            .collect::<Vec<_>>();
        let health_warnings = health.iter().filter(|issue| issue.severity != "ok").count();

        Self {
            root: inspection.root.display().to_string(),
            generated_at: audit::timestamp_seconds(),
            jobs: inspection.jobs.len(),
            drafts: inspection.drafts.len(),
            runs: inspection.runs.len(),
            failed_runs: incidents.len(),
            health_warnings,
            incidents,
            health,
            recommendations: inspection.recommendations.clone(),
        }
    }

    fn text(&self) -> String {
        let mut out = vec![
            "kind: watch_snapshot".to_string(),
            format!("root: {}", self.root),
            format!("generated_at: {}", self.generated_at),
            format!("jobs: {}", self.jobs),
            format!("drafts: {}", self.drafts),
            format!("runs: {}", self.runs),
            format!("failed_runs: {}", self.failed_runs),
            format!("health_warnings: {}", self.health_warnings),
            "incidents:".to_string(),
        ];
        if self.incidents.is_empty() {
            out.push("- none".to_string());
        } else {
            for incident in &self.incidents {
                out.push(format!(
                    "- {} status={} job={} failed_step={}",
                    incident.run_id,
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
        out.push(format!(
            "recommendations: {}",
            list_or_none(&self.recommendations)
        ));
        out.join("\n")
    }

    fn json(&self) -> String {
        let incidents = self
            .incidents
            .iter()
            .map(|incident| {
                format!(
                    "{{\"run_id\":\"{}\",\"job\":\"{}\",\"status\":\"{}\",\"failed_step\":\"{}\",\"hint\":\"{}\"}}",
                    json::escape(&incident.run_id),
                    json::escape(&incident.job),
                    json::escape(&incident.status),
                    json::escape(&incident.failed_step),
                    json::escape(&incident.hint)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        let health = self
            .health
            .iter()
            .map(|issue| {
                format!(
                    "{{\"severity\":\"{}\",\"path\":\"{}\",\"message\":\"{}\"}}",
                    json::escape(issue.severity),
                    json::escape(&issue.path),
                    json::escape(&issue.message)
                )
            })
            .collect::<Vec<_>>()
            .join(",");
        format!(
            "{{\"kind\":\"watch_snapshot\",\"root\":\"{}\",\"generated_at\":\"{}\",\"summary\":{{\"jobs\":{},\"drafts\":{},\"runs\":{},\"failed_runs\":{},\"health_warnings\":{}}},\"incidents\":[{}],\"health\":[{}],\"recommendations\":[{}]}}",
            json::escape(&self.root),
            json::escape(&self.generated_at),
            self.jobs,
            self.drafts,
            self.runs,
            self.failed_runs,
            self.health_warnings,
            incidents,
            health,
            json::string_array(&self.recommendations)
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

fn empty_as_none(value: &str) -> &str {
    if value.is_empty() {
        "none"
    } else {
        value
    }
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
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
    fn empty_workspace_returns_valid_json_snapshot() {
        let root = unique_temp_dir("watch-empty");
        fs::create_dir_all(&root).unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--once".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&result.output).unwrap();
        assert_eq!(parsed["kind"], "watch_snapshot");
        assert_eq!(parsed["summary"]["jobs"], 0);
        assert_eq!(parsed["summary"]["failed_runs"], 0);
        assert!(parsed["incidents"].as_array().unwrap().is_empty());
    }

    #[test]
    fn failed_run_is_reported_as_incident() {
        let root = unique_temp_dir("watch-failed");
        let jobs = root.join(".flow").join("jobs");
        let run_dir = root.join(".flow").join("runs").join("run-1");
        fs::create_dir_all(&jobs).unwrap();
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(jobs.join("backup.yml"), "name: backup\nsteps: []\n").unwrap();
        fs::write(
            run_dir.join("manifest.json"),
            "{\"run_id\":\"run-1\",\"job_name\":\"backup\",\"status\":\"FAILED\",\"failed_step\":\"upload\"}",
        )
        .unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--once".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(result.output.contains("\"run_id\":\"run-1\""));
        assert!(result.output.contains("\"failed_step\":\"upload\""));
        assert!(result
            .output
            .contains("Review with runflow-agent explain-run run-1"));
    }

    #[test]
    fn health_warning_is_included() {
        let root = unique_temp_dir("watch-health");
        fs::create_dir_all(&root).unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--once".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(result.output.contains("\"health\":["));
        assert!(result.output.contains(".flow/jobs"));
        assert!(result.output.contains("directory is missing"));
    }

    #[test]
    fn output_writes_requested_file_without_watch_state() {
        let root = unique_temp_dir("watch-output");
        fs::create_dir_all(&root).unwrap();
        let output = root.join("snapshots").join("latest.json");

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--once".to_string(),
            "--format".to_string(),
            "json".to_string(),
            "--output".to_string(),
            output.to_string_lossy().to_string(),
        ])
        .unwrap();

        assert!(output.is_file());
        assert!(root
            .join(".flow")
            .join("agent")
            .join("audit.jsonl")
            .is_file());
        assert!(!root.join(".flow").join("agent").join("watch").exists());
        assert_eq!(result.changed_files, vec![output.display().to_string()]);
        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn continuous_mode_rejects_zero_interval() {
        let err = run(&[
            "--once".to_string(),
            "--interval-seconds".to_string(),
            "0".to_string(),
        ])
        .unwrap_err();
        assert!(err.contains("greater than zero"));
    }

    #[test]
    fn continuous_mode_is_not_implemented_yet() {
        let err = run(&["--interval-seconds".to_string(), "60".to_string()]).unwrap_err();
        assert!(err.contains("--once only"));
    }

    #[test]
    fn recommendations_do_not_suggest_mutations() {
        let root = unique_temp_dir("watch-recommendations");
        fs::create_dir_all(&root).unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--once".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        for denied in ["cancel", "rerun", "execute shell", "webhook"] {
            assert!(!result.output.contains(denied));
        }
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
