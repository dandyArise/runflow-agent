use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cli::CliResult;
use crate::json;
use crate::runflow;

#[derive(Debug)]
struct WorkspaceInspection {
    root: PathBuf,
    jobs: Vec<String>,
    drafts: Vec<String>,
    runs: Vec<RunSummary>,
    health: Vec<HealthIssue>,
    recommendations: Vec<String>,
}

#[derive(Debug)]
struct RunSummary {
    id: String,
    job: String,
    status: String,
    started_at: String,
    ended_at: String,
}

#[derive(Debug)]
struct HealthIssue {
    severity: &'static str,
    path: String,
    message: String,
}

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let root = value_after(args, "--root")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().map_err(|e| format!("cannot read current dir: {e}"))?);
    let limit = value_after(args, "--limit")
        .map(str::parse::<usize>)
        .transpose()
        .map_err(|_| "--limit must be an integer".to_string())?
        .unwrap_or(10);
    let format_json = wants_json(args);
    let include_health = args.iter().any(|arg| arg == "--health");

    let inspection = inspect(&root, limit)?;
    let mut warnings = inspection
        .runs
        .iter()
        .filter(|run| run.status == "FAILED" || run.status == "ERROR")
        .map(|run| run.id.clone())
        .collect::<Vec<_>>();
    if include_health {
        warnings.extend(
            inspection
                .health
                .iter()
                .filter(|issue| issue.severity != "info")
                .map(|issue| format!("{}: {}", issue.path, issue.message)),
        );
    }

    let output = if format_json {
        inspection_json(&inspection, include_health)
    } else {
        inspection_text(&inspection, include_health)
    };

    Ok(CliResult {
        command: "inspect-workspace".to_string(),
        output,
        status: "success",
        changed_files: Vec::new(),
        warnings,
        audit: true,
    })
}

fn inspect(root: &Path, limit: usize) -> Result<WorkspaceInspection, String> {
    if !root.is_dir() {
        return Err(format!(
            "workspace root '{}' does not exist",
            root.display()
        ));
    }

    let mut jobs = list_yaml_stems(&root.join(".flow").join("jobs"))?;
    let mut drafts = list_yaml_paths(&root.join(".flow").join("agent").join("drafts"))?;
    let mut runs = list_runs(&root.join(".flow").join("runs"))?;
    let mut health = workspace_health(root, &jobs, &runs)?;

    jobs.sort();
    drafts.sort();
    health.sort_by(|a, b| {
        a.severity
            .cmp(b.severity)
            .then_with(|| a.path.cmp(&b.path))
            .then_with(|| a.message.cmp(&b.message))
    });
    runs.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| b.ended_at.cmp(&a.ended_at))
            .then_with(|| b.id.cmp(&a.id))
    });
    runs.truncate(limit);

    let failed_count = runs
        .iter()
        .filter(|run| run.status == "FAILED" || run.status == "ERROR")
        .count();
    let mut recommendations = Vec::new();
    if jobs.is_empty() {
        recommendations.push("No registered jobs found under .flow/jobs.".to_string());
    }
    if drafts.is_empty() {
        recommendations.push("No agent drafts found under .flow/agent/drafts.".to_string());
    }
    if runs.is_empty() {
        recommendations.push("No runs found under .flow/runs.".to_string());
    }
    if failed_count > 0 {
        recommendations
            .push("Review failed runs with runflow-agent explain-run <run_id>.".to_string());
    }
    if recommendations.is_empty() {
        recommendations.push("Workspace has jobs, drafts, and recent run history.".to_string());
    }

    Ok(WorkspaceInspection {
        root: root.to_path_buf(),
        jobs,
        drafts,
        runs,
        health,
        recommendations,
    })
}

fn list_yaml_stems(dir: &Path) -> Result<Vec<String>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if is_yaml(&path) {
            if let Some(stem) = path.file_stem().and_then(|stem| stem.to_str()) {
                out.push(stem.to_string());
            }
        }
    }
    Ok(out)
}

fn list_yaml_paths(dir: &Path) -> Result<Vec<String>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if is_yaml(&path) {
            out.push(path.display().to_string());
        }
    }
    Ok(out)
}

fn list_yaml_file_paths(dir: &Path) -> Result<Vec<PathBuf>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if is_yaml(&path) {
            out.push(path);
        }
    }
    Ok(out)
}

fn list_runs(dir: &Path) -> Result<Vec<RunSummary>, String> {
    if !dir.exists() {
        return Ok(Vec::new());
    }
    let mut out = Vec::new();
    for entry in fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let id = entry.file_name().to_string_lossy().to_string();
        let manifest = fs::read_to_string(path.join("manifest.json")).unwrap_or_default();
        out.push(run_summary(&id, &manifest));
    }
    Ok(out)
}

fn workspace_health(
    root: &Path,
    jobs: &[String],
    runs: &[RunSummary],
) -> Result<Vec<HealthIssue>, String> {
    let mut issues = Vec::new();
    let jobs_dir = root.join(".flow").join("jobs");
    for path in list_yaml_file_paths(&jobs_dir)? {
        let yaml = fs::read_to_string(&path)
            .map_err(|e| format!("cannot read '{}': {e}", path.display()))?;
        let validation = runflow::validate_workflow(&yaml);
        if !validation.valid {
            issues.push(HealthIssue {
                severity: "error",
                path: path.display().to_string(),
                message: format!(
                    "job workflow is invalid: {}",
                    validation.messages.join("; ")
                ),
            });
        }
    }

    let drafts_dir = root.join(".flow").join("agent").join("drafts");
    for path in list_yaml_file_paths(&drafts_dir)? {
        let stem = path
            .file_stem()
            .and_then(|stem| stem.to_str())
            .unwrap_or_default();
        if !jobs.iter().any(|job| job == stem) {
            issues.push(HealthIssue {
                severity: "warn",
                path: path.display().to_string(),
                message: "draft has no matching registered job name".to_string(),
            });
        }
    }

    let runs_dir = root.join(".flow").join("runs");
    let logs_dir = root.join("logs");
    if runs_dir.exists() {
        for entry in fs::read_dir(&runs_dir)
            .map_err(|e| format!("cannot read '{}': {e}", runs_dir.display()))?
        {
            let entry = entry.map_err(|e| format!("cannot read '{}': {e}", runs_dir.display()))?;
            let run_dir = entry.path();
            if !run_dir.is_dir() {
                continue;
            }
            let run_id = entry.file_name().to_string_lossy().to_string();
            let manifest_path = run_dir.join("manifest.json");
            if !manifest_path.is_file() {
                issues.push(HealthIssue {
                    severity: "error",
                    path: manifest_path.display().to_string(),
                    message: "run manifest is missing".to_string(),
                });
                continue;
            }
            let manifest = fs::read_to_string(&manifest_path)
                .map_err(|e| format!("cannot read '{}': {e}", manifest_path.display()))?;
            let parsed = match serde_json::from_str::<Value>(&manifest) {
                Ok(value) => Some(value),
                Err(error) => {
                    issues.push(HealthIssue {
                        severity: "error",
                        path: manifest_path.display().to_string(),
                        message: format!("run manifest is invalid JSON: {error}"),
                    });
                    continue;
                }
            };
            match json_string(&parsed, "job_name") {
                Some(job) if !jobs.is_empty() && !jobs.iter().any(|name| name == &job) => {
                    issues.push(HealthIssue {
                        severity: "warn",
                        path: manifest_path.display().to_string(),
                        message: format!("run references unknown job '{job}'"),
                    });
                }
                Some(_) => {}
                None => {
                    issues.push(HealthIssue {
                        severity: "warn",
                        path: manifest_path.display().to_string(),
                        message: "run manifest has no job_name".to_string(),
                    });
                }
            }

            let run_logs = logs_dir.join(&run_id);
            if !run_logs.is_dir() {
                issues.push(HealthIssue {
                    severity: "warn",
                    path: run_logs.display().to_string(),
                    message: "run logs directory is missing".to_string(),
                });
                continue;
            }
            if runs
                .iter()
                .any(|run| run.id == run_id && (run.status == "FAILED" || run.status == "ERROR"))
                && !has_log_file(&run_logs)?
            {
                issues.push(HealthIssue {
                    severity: "warn",
                    path: run_logs.display().to_string(),
                    message: "failed run has no stdout/stderr log".to_string(),
                });
            }
        }
    }

    if issues.is_empty() {
        issues.push(HealthIssue {
            severity: "ok",
            path: root.display().to_string(),
            message: "workspace health checks passed".to_string(),
        });
    }
    Ok(issues)
}

fn has_log_file(dir: &Path) -> Result<bool, String> {
    for entry in fs::read_dir(dir).map_err(|e| format!("cannot read '{}': {e}", dir.display()))? {
        let entry = entry.map_err(|e| format!("cannot read '{}': {e}", dir.display()))?;
        let path = entry.path();
        if path.is_dir() {
            if has_log_file(&path)? {
                return Ok(true);
            }
        } else if path
            .file_name()
            .and_then(|name| name.to_str())
            .map(|name| name == "stdout.log" || name == "stderr.log")
            .unwrap_or(false)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

fn run_summary(id: &str, manifest: &str) -> RunSummary {
    let parsed = serde_json::from_str::<Value>(manifest).ok();
    RunSummary {
        id: id.to_string(),
        job: json_string(&parsed, "job_name").unwrap_or_else(|| "unknown".to_string()),
        status: json_string(&parsed, "status").unwrap_or_else(|| "UNKNOWN".to_string()),
        started_at: json_string(&parsed, "started_at").unwrap_or_default(),
        ended_at: json_string(&parsed, "ended_at").unwrap_or_default(),
    }
}

fn json_string(value: &Option<Value>, key: &str) -> Option<String> {
    value
        .as_ref()
        .and_then(|value| value.get(key))
        .and_then(Value::as_str)
        .map(ToString::to_string)
}

fn is_yaml(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"))
        .unwrap_or(false)
}

fn inspection_text(inspection: &WorkspaceInspection, include_health: bool) -> String {
    let mut out = vec![
        format!("root: {}", inspection.root.display()),
        format!("jobs: {}", inspection.jobs.len()),
        format!("drafts: {}", inspection.drafts.len()),
        format!("recent_runs: {}", inspection.runs.len()),
    ];

    out.push(format!("job_names: {}", list_or_none(&inspection.jobs)));
    out.push("runs:".to_string());
    if inspection.runs.is_empty() {
        out.push("- none".to_string());
    } else {
        for run in &inspection.runs {
            out.push(format!(
                "- {} status={} job={} started_at={}",
                run.id,
                run.status,
                run.job,
                empty_as_dash(&run.started_at)
            ));
        }
    }
    out.push(format!(
        "recommendations: {}",
        list_or_none(&inspection.recommendations)
    ));
    if include_health {
        out.push("health:".to_string());
        for issue in &inspection.health {
            out.push(format!(
                "- [{}] {}: {}",
                issue.severity, issue.path, issue.message
            ));
        }
    }
    out.join("\n")
}

fn inspection_json(inspection: &WorkspaceInspection, include_health: bool) -> String {
    let runs = inspection
        .runs
        .iter()
        .map(|run| {
            format!(
                "{{\"id\":\"{}\",\"job\":\"{}\",\"status\":\"{}\",\"started_at\":\"{}\",\"ended_at\":\"{}\"}}",
                json::escape(&run.id),
                json::escape(&run.job),
                json::escape(&run.status),
                json::escape(&run.started_at),
                json::escape(&run.ended_at)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    let health = if include_health {
        let issues = inspection
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
        format!(",\"health\":[{issues}]")
    } else {
        String::new()
    };
    format!(
        "{{\"kind\":\"workspace_inspection\",\"root\":\"{}\",\"jobs\":[{}],\"drafts\":[{}],\"runs\":[{}],\"recommendations\":[{}]{}}}",
        json::escape(&inspection.root.display().to_string()),
        json::string_array(&inspection.jobs),
        json::string_array(&inspection.drafts),
        runs,
        json::string_array(&inspection.recommendations),
        health
    )
}

fn empty_as_dash(value: &str) -> &str {
    if value.is_empty() {
        "-"
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
    fn inspects_workspace_counts_and_failed_runs() {
        let root = unique_temp_dir("inspect");
        let jobs = root.join(".flow").join("jobs");
        let drafts = root.join(".flow").join("agent").join("drafts");
        let run_dir = root.join(".flow").join("runs").join("run-1");
        fs::create_dir_all(&jobs).unwrap();
        fs::create_dir_all(&drafts).unwrap();
        fs::create_dir_all(&run_dir).unwrap();
        fs::write(jobs.join("ping.yml"), "name: ping\n").unwrap();
        fs::write(drafts.join("draft.yml"), "name: draft\n").unwrap();
        fs::write(
            run_dir.join("manifest.json"),
            "{\"run_id\":\"run-1\",\"job_name\":\"ping\",\"status\":\"FAILED\",\"started_at\":\"2026-06-10T00:00:00Z\"}",
        )
        .unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.status, "success");
        assert_eq!(result.warnings, vec!["run-1".to_string()]);
        assert!(result.output.contains("\"kind\":\"workspace_inspection\""));
        assert!(result.output.contains("\"jobs\":[\"ping\"]"));
        assert!(result.output.contains("\"status\":\"FAILED\""));
    }

    #[test]
    fn missing_dirs_are_not_failures() {
        let root = unique_temp_dir("inspect-empty");
        fs::create_dir_all(&root).unwrap();

        let inspection = inspect(&root, 10).unwrap();

        fs::remove_dir_all(root).unwrap();
        assert!(inspection.jobs.is_empty());
        assert!(inspection.drafts.is_empty());
        assert!(inspection.runs.is_empty());
        assert!(inspection
            .recommendations
            .iter()
            .any(|item| item.contains("No registered jobs")));
    }

    #[test]
    fn health_reports_workspace_anomalies() {
        let root = unique_temp_dir("inspect-health");
        let jobs = root.join(".flow").join("jobs");
        let drafts = root.join(".flow").join("agent").join("drafts");
        let runs = root.join(".flow").join("runs");
        fs::create_dir_all(&jobs).unwrap();
        fs::create_dir_all(&drafts).unwrap();
        fs::create_dir_all(runs.join("missing-manifest")).unwrap();
        fs::create_dir_all(runs.join("unknown-job")).unwrap();
        fs::write(jobs.join("broken.yml"), "name: Bad Name\nsteps: []\n").unwrap();
        fs::write(drafts.join("orphan.yml"), "name: orphan\n").unwrap();
        fs::write(
            runs.join("unknown-job").join("manifest.json"),
            "{\"job_name\":\"ghost\",\"status\":\"FAILED\"}",
        )
        .unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--health".to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.output.contains("\"health\":["));
        assert!(result.output.contains("job workflow is invalid"));
        assert!(result.output.contains("run manifest is missing"));
        assert!(result
            .output
            .contains("draft has no matching registered job name"));
        assert!(result.output.contains("run references unknown job"));
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
