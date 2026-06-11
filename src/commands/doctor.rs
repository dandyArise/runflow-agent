use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::CliResult;
use crate::config::{self, ModelConfig};
use crate::json;
use crate::model;
use crate::runflow;
use crate::strict_json::{decode_model_json, ModelKind};

#[derive(Debug)]
struct Check {
    name: &'static str,
    status: &'static str,
    message: String,
}

#[derive(Debug, Deserialize)]
struct DoctorModelResponse {
    kind: String,
    status: String,
}

impl ModelKind for DoctorModelResponse {
    fn kind(&self) -> &str {
        &self.kind
    }
}

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let model_config = ModelConfig::from_args(args)?;
    let command_args = config::strip_model_flags(args);
    let format_json = command_args.iter().any(|arg| arg == "--format")
        && command_args.iter().any(|arg| arg == "json");
    let root = value_after(&command_args, "--root")
        .map(PathBuf::from)
        .unwrap_or(std::env::current_dir().map_err(|e| format!("cannot read current dir: {e}"))?);

    let mut checks = Vec::new();
    checks.push(model_check(&model_config));
    checks.extend(workspace_checks(&root));

    let failed = checks.iter().any(|check| check.status == "fail");
    let output = if format_json {
        doctor_json(&root, &model_config, &checks, failed)
    } else {
        doctor_text(&root, &model_config, &checks, failed)
    };

    let warnings = checks
        .iter()
        .filter(|check| check.status != "ok")
        .map(|check| check.message.clone())
        .collect();

    Ok(CliResult {
        command: "doctor".to_string(),
        output,
        status: if failed { "failed" } else { "success" },
        changed_files: Vec::new(),
        warnings,
        audit: true,
    })
}

fn model_check(config: &ModelConfig) -> Check {
    if config.is_mock() {
        return Check {
            name: "model",
            status: "ok",
            message: "mock provider is available".to_string(),
        };
    }

    let system = "You are a RunFlow Agent health check endpoint. Return raw JSON only.";
    let user =
        "Return exactly this JSON object and nothing else: {\"kind\":\"doctor\",\"status\":\"ok\"}";
    match model::chat(config, system, user)
        .and_then(|raw| decode_model_json::<DoctorModelResponse>(&raw, "doctor"))
    {
        Ok(response) if response.status == "ok" => Check {
            name: "model",
            status: "ok",
            message: format!("{} provider answered with valid JSON", config.provider),
        },
        Ok(response) => Check {
            name: "model",
            status: "fail",
            message: format!("model returned unexpected status '{}'", response.status),
        },
        Err(err) => Check {
            name: "model",
            status: "fail",
            message: err,
        },
    }
}

fn workspace_checks(root: &Path) -> Vec<Check> {
    let flow = root.join(".flow");
    let agent = flow.join("agent");
    let jobs = flow.join("jobs");
    let runs = flow.join("runs");
    let drafts = agent.join("drafts");
    vec![
        dir_check("root", root, true),
        dir_check("flow_state", &flow, false),
        dir_count_check("jobs", &jobs),
        workflow_dir_check("jobs_valid", &jobs),
        dir_count_check("runs", &runs),
        run_manifest_check("run_manifests", &runs),
        dir_count_check("agent_drafts", &drafts),
        dir_check("agent_state", &agent, false),
        file_check("audit_file", &agent.join("audit.jsonl"), false),
        dir_count_check("logs", &root.join("logs")),
        policy_check(),
    ]
}

fn dir_check(name: &'static str, path: &Path, required: bool) -> Check {
    if path.is_dir() {
        Check {
            name,
            status: "ok",
            message: format!("{} exists", path.display()),
        }
    } else {
        Check {
            name,
            status: if required { "fail" } else { "warn" },
            message: format!("{} is missing", path.display()),
        }
    }
}

fn dir_count_check(name: &'static str, path: &Path) -> Check {
    if !path.is_dir() {
        return Check {
            name,
            status: "warn",
            message: format!("{} is missing", path.display()),
        };
    }

    match fs::read_dir(path) {
        Ok(entries) => Check {
            name,
            status: "ok",
            message: format!(
                "{} exists with {} item(s)",
                path.display(),
                entries.filter_map(Result::ok).count()
            ),
        },
        Err(err) => Check {
            name,
            status: "fail",
            message: format!("cannot read {}: {err}", path.display()),
        },
    }
}

fn file_check(name: &'static str, path: &Path, required: bool) -> Check {
    if path.is_file() {
        Check {
            name,
            status: "ok",
            message: format!("{} exists", path.display()),
        }
    } else {
        Check {
            name,
            status: if required { "fail" } else { "warn" },
            message: format!("{} is missing", path.display()),
        }
    }
}

fn workflow_dir_check(name: &'static str, path: &Path) -> Check {
    if !path.is_dir() {
        return Check {
            name,
            status: "warn",
            message: format!("{} is missing", path.display()),
        };
    }

    let mut checked = 0usize;
    let mut invalid = Vec::new();
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.filter_map(Result::ok) {
                let workflow_path = entry.path();
                if !is_yaml(&workflow_path) {
                    continue;
                }
                checked += 1;
                let yaml = match fs::read_to_string(&workflow_path) {
                    Ok(yaml) => yaml,
                    Err(err) => {
                        invalid.push(format!("{}: cannot read: {err}", workflow_path.display()));
                        continue;
                    }
                };
                let validation = runflow::validate_workflow(&yaml);
                if !validation.valid {
                    invalid.push(format!(
                        "{}: {}",
                        workflow_path.display(),
                        validation.messages.join("; ")
                    ));
                }
            }
        }
        Err(err) => {
            return Check {
                name,
                status: "fail",
                message: format!("cannot read {}: {err}", path.display()),
            };
        }
    }

    if invalid.is_empty() {
        Check {
            name,
            status: "ok",
            message: format!("{} valid workflow file(s)", checked),
        }
    } else {
        Check {
            name,
            status: "fail",
            message: format!("invalid workflow file(s): {}", invalid.join(" | ")),
        }
    }
}

fn run_manifest_check(name: &'static str, path: &Path) -> Check {
    if !path.is_dir() {
        return Check {
            name,
            status: "warn",
            message: format!("{} is missing", path.display()),
        };
    }

    let mut checked = 0usize;
    let mut invalid = Vec::new();
    match fs::read_dir(path) {
        Ok(entries) => {
            for entry in entries.filter_map(Result::ok) {
                let run_dir = entry.path();
                if !run_dir.is_dir() {
                    continue;
                }
                checked += 1;
                let manifest_path = run_dir.join("manifest.json");
                if !manifest_path.is_file() {
                    invalid.push(format!("{} is missing", manifest_path.display()));
                    continue;
                }
                match fs::read_to_string(&manifest_path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<serde_json::Value>(&text).ok())
                {
                    Some(_) => {}
                    None => invalid.push(format!("{} is invalid JSON", manifest_path.display())),
                }
            }
        }
        Err(err) => {
            return Check {
                name,
                status: "fail",
                message: format!("cannot read {}: {err}", path.display()),
            };
        }
    }

    if invalid.is_empty() {
        Check {
            name,
            status: "ok",
            message: format!("{} run manifest(s) valid", checked),
        }
    } else {
        Check {
            name,
            status: "fail",
            message: invalid.join(" | "),
        }
    }
}

fn policy_check() -> Check {
    Check {
        name: "permissions",
        status: "ok",
        message: "deny-by-default: no run, cancel, rerun, shell, secrets, notifications, or external APIs".to_string(),
    }
}

fn is_yaml(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| ext.eq_ignore_ascii_case("yml") || ext.eq_ignore_ascii_case("yaml"))
        .unwrap_or(false)
}

fn doctor_text(root: &Path, config: &ModelConfig, checks: &[Check], failed: bool) -> String {
    let base_url = if config.base_url.is_empty() {
        "n/a".to_string()
    } else {
        config.base_url.clone()
    };
    let mut out = vec![
        format!("status: {}", if failed { "failed" } else { "success" }),
        format!("root: {}", root.display()),
        format!("provider: {}", config.provider),
        format!("model: {}", config.model),
        format!("base_url: {base_url}"),
        "checks:".to_string(),
    ];
    for check in checks {
        out.push(format!(
            "- [{}] {}: {}",
            check.status, check.name, check.message
        ));
    }
    out.join("\n")
}

fn doctor_json(root: &Path, config: &ModelConfig, checks: &[Check], failed: bool) -> String {
    let checks_json = checks
        .iter()
        .map(|check| {
            format!(
                "{{\"name\":\"{}\",\"status\":\"{}\",\"message\":\"{}\"}}",
                json::escape(check.name),
                json::escape(check.status),
                json::escape(&check.message)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!(
        "{{\"kind\":\"doctor\",\"status\":\"{}\",\"root\":\"{}\",\"provider\":\"{}\",\"model\":\"{}\",\"base_url\":\"{}\",\"checks\":[{}]}}",
        if failed { "failed" } else { "success" },
        json::escape(&root.display().to_string()),
        json::escape(&config.provider),
        json::escape(&config.model),
        json::escape(&config.base_url),
        checks_json
    )
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn doctor_mock_returns_success_json() {
        let root = unique_temp_dir("doctor-json");
        fs::create_dir_all(root.join(".flow").join("agent").join("drafts")).unwrap();
        fs::create_dir_all(root.join(".flow").join("jobs")).unwrap();
        fs::create_dir_all(root.join(".flow").join("runs")).unwrap();
        fs::create_dir_all(root.join("logs")).unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.output.contains("\"kind\":\"doctor\""));
        assert!(result.output.contains("\"provider\":\"mock\""));
    }

    #[test]
    fn doctor_missing_optional_runflow_dirs_are_warnings() {
        let root = unique_temp_dir("doctor-warn");
        fs::create_dir_all(&root).unwrap();

        let result = run(&["--root".to_string(), root.to_string_lossy().to_string()]).unwrap();

        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.output.contains("[warn] flow_state"));
    }

    #[test]
    fn doctor_reports_workspace_integrity_and_policy() {
        let root = unique_temp_dir("doctor-integrity");
        let jobs = root.join(".flow").join("jobs");
        let runs = root.join(".flow").join("runs").join("bad-run");
        let agent = root.join(".flow").join("agent");
        fs::create_dir_all(&jobs).unwrap();
        fs::create_dir_all(&runs).unwrap();
        fs::create_dir_all(agent.join("drafts")).unwrap();
        fs::write(agent.join("audit.jsonl"), "{}\n").unwrap();
        fs::write(jobs.join("bad.yml"), "name: Bad Name\nsteps: []\n").unwrap();
        fs::write(runs.join("manifest.json"), "{not-json").unwrap();

        let result = run(&[
            "--root".to_string(),
            root.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();

        fs::remove_dir_all(root).unwrap();
        assert_eq!(result.status, "failed");
        assert!(result.output.contains("\"name\":\"jobs_valid\""));
        assert!(result.output.contains("\"name\":\"run_manifests\""));
        assert!(result.output.contains("\"name\":\"audit_file\""));
        assert!(result.output.contains("\"name\":\"permissions\""));
        assert!(result.output.contains("deny-by-default"));
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
