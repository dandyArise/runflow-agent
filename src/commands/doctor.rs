use std::fs;
use std::path::{Path, PathBuf};

use serde::Deserialize;

use crate::cli::CliResult;
use crate::config::{self, ModelConfig};
use crate::json;
use crate::model;
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
    vec![
        dir_check("root", root, true),
        dir_check("flow_state", &root.join(".flow"), false),
        dir_count_check("jobs", &root.join(".flow").join("jobs")),
        dir_count_check("runs", &root.join(".flow").join("runs")),
        dir_count_check(
            "agent_drafts",
            &root.join(".flow").join("agent").join("drafts"),
        ),
        dir_count_check("logs", &root.join("logs")),
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
