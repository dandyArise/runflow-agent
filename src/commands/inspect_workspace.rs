use std::fs;
use std::path::{Path, PathBuf};

use serde_json::Value;

use crate::cli::CliResult;
use crate::json;

#[derive(Debug)]
struct WorkspaceInspection {
    root: PathBuf,
    jobs: Vec<String>,
    drafts: Vec<String>,
    runs: Vec<RunSummary>,
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

    let inspection = inspect(&root, limit)?;
    let failed = inspection
        .runs
        .iter()
        .filter(|run| run.status == "FAILED" || run.status == "ERROR")
        .map(|run| run.id.clone())
        .collect::<Vec<_>>();

    let output = if format_json {
        inspection_json(&inspection)
    } else {
        inspection_text(&inspection)
    };

    Ok(CliResult {
        command: "inspect-workspace".to_string(),
        output,
        status: "success",
        changed_files: Vec::new(),
        warnings: failed,
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

    jobs.sort();
    drafts.sort();
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

fn inspection_text(inspection: &WorkspaceInspection) -> String {
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
    out.join("\n")
}

fn inspection_json(inspection: &WorkspaceInspection) -> String {
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
    format!(
        "{{\"kind\":\"workspace_inspection\",\"root\":\"{}\",\"jobs\":[{}],\"drafts\":[{}],\"runs\":[{}],\"recommendations\":[{}]}}",
        json::escape(&inspection.root.display().to_string()),
        json::string_array(&inspection.jobs),
        json::string_array(&inspection.drafts),
        runs,
        json::string_array(&inspection.recommendations)
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
