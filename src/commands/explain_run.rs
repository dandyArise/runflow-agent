use crate::agent;
use crate::cli::CliResult;
use crate::config::{self, ModelConfig};
use crate::json;

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let model_config = ModelConfig::from_args(args)?;
    let command_args = config::strip_model_flags(args);
    let Some(run_id) = command_args.first() else {
        return Err("explain-run requires <run_id>".to_string());
    };
    if run_id.starts_with("--") {
        return Err("explain-run requires <run_id> before flags".to_string());
    }
    let format_json = command_args.iter().any(|arg| arg == "--format")
        && command_args.iter().any(|arg| arg == "json");
    let explanation = agent::explain_run_with_model(run_id, &model_config)?;

    let output = if format_json {
        format!(
            "{{\"kind\":\"run_explanation\",\"run_id\":\"{}\",\"status\":\"{}\",\"summary\":\"{}\",\"failed_step\":\"{}\",\"evidence\":[{}],\"suggested_next_steps\":[{}]}}",
            json::escape(&explanation.run_id),
            json::escape(&explanation.status),
            json::escape(&explanation.summary),
            json::escape(explanation.failed_step.as_deref().unwrap_or("")),
            json::string_array(&explanation.evidence),
            json::string_array(&explanation.suggested_next_steps)
        )
    } else {
        let mut out = vec![
            format!("run_id: {}", explanation.run_id),
            format!("status: {}", explanation.status),
            format!("summary: {}", explanation.summary),
        ];
        if let Some(step) = explanation.failed_step {
            out.push(format!("failed_step: {step}"));
        }
        out.push("evidence:".to_string());
        out.extend(explanation.evidence.iter().map(|item| format!("- {item}")));
        out.push("suggested_next_steps:".to_string());
        out.extend(
            explanation
                .suggested_next_steps
                .iter()
                .map(|item| format!("- {item}")),
        );
        out.join("\n")
    };

    Ok(CliResult {
        command: "explain-run".to_string(),
        output,
        status: "success",
        changed_files: Vec::new(),
        warnings: Vec::new(),
        audit: true,
    })
}
