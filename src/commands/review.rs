use std::fs;

use crate::agent;
use crate::cli::CliResult;
use crate::config::{self, ModelConfig};
use crate::json;
use crate::runflow::{self, Finding};

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let model_config = ModelConfig::from_args(args)?;
    let command_args = config::strip_model_flags(args);
    let Some(path) = command_args.first() else {
        return Err("review requires <workflow.yml>".to_string());
    };
    if path.starts_with("--") {
        return Err("review requires <workflow.yml> before flags".to_string());
    }

    let format_json = command_args.iter().any(|arg| arg == "--format")
        && command_args.iter().any(|arg| arg == "json");
    let yaml =
        fs::read_to_string(path).map_err(|e| format!("cannot read workflow '{path}': {e}"))?;
    let validation = runflow::validate_workflow(&yaml);
    let mut findings = validation
        .messages
        .iter()
        .map(|message| {
            Finding::error(
                "/",
                message,
                "Fix the workflow before registering or running it.",
            )
        })
        .collect::<Vec<_>>();
    findings.extend(agent::review_workflow_with_model(&yaml, &model_config)?);

    let output = if format_json {
        review_json(validation.valid, &findings)
    } else {
        review_text(validation.valid, &findings)
    };

    let warnings = findings
        .iter()
        .filter(|f| f.severity != "info")
        .map(|f| f.message.clone())
        .collect();

    Ok(CliResult {
        command: "review".to_string(),
        output,
        status: if validation.valid {
            "success"
        } else {
            "failed"
        },
        changed_files: Vec::new(),
        warnings,
        audit: true,
    })
}

fn review_text(valid: bool, findings: &[Finding]) -> String {
    let mut out = vec![format!("valid: {valid}")];
    if findings.is_empty() {
        out.push("findings: none".to_string());
    } else {
        out.push("findings:".to_string());
        for finding in findings {
            out.push(format!(
                "- [{}] {} {} -> {}",
                finding.severity, finding.path, finding.message, finding.suggestion
            ));
        }
    }
    out.join("\n")
}

fn review_json(valid: bool, findings: &[Finding]) -> String {
    let items = findings
        .iter()
        .map(|finding| {
            format!(
                "{{\"severity\":\"{}\",\"path\":\"{}\",\"message\":\"{}\",\"suggestion\":\"{}\"}}",
                json::escape(&finding.severity),
                json::escape(&finding.path),
                json::escape(&finding.message),
                json::escape(&finding.suggestion)
            )
        })
        .collect::<Vec<_>>()
        .join(",");
    format!("{{\"kind\":\"workflow_review\",\"valid\":{valid},\"findings\":[{items}]}}")
}
