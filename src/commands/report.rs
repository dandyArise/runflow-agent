use crate::agent;
use crate::cli::CliResult;
use crate::config::{self, ModelConfig};
use crate::json;

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let model_config = ModelConfig::from_args(args)?;
    let command_args = config::strip_model_flags(args);
    let Some(subcommand) = command_args.first().map(String::as_str) else {
        return Err("report requires a subcommand: daily".to_string());
    };
    if subcommand != "daily" {
        return Err(format!("unknown report subcommand '{subcommand}'"));
    }

    let from = value_after(&command_args, "--from").unwrap_or("start");
    let to = value_after(&command_args, "--to").unwrap_or("now");
    let format_json = command_args.iter().any(|arg| arg == "--format")
        && command_args.iter().any(|arg| arg == "json");
    let report = agent::daily_report_with_model(from, to, &model_config)?;

    let output = if format_json {
        format!(
            "{{\"kind\":\"daily_report\",\"period\":{{\"from\":\"{}\",\"to\":\"{}\"}},\"runs\":{{\"total\":{},\"success\":{},\"failed\":{},\"cancelled\":{}}},\"unstable_jobs\":[{}],\"incidents\":[{}],\"recommendations\":[{}]}}",
            json::escape(&report.from),
            json::escape(&report.to),
            report.total,
            report.success,
            report.failed,
            report.cancelled,
            json::string_array(&report.unstable_jobs),
            json::string_array(&report.incidents),
            json::string_array(&report.recommendations)
        )
    } else {
        [
            format!("period: {} -> {}", report.from, report.to),
            format!(
                "runs: total={} success={} failed={} cancelled={}",
                report.total, report.success, report.failed, report.cancelled
            ),
            format!("unstable_jobs: {}", list_or_none(&report.unstable_jobs)),
            format!("incidents: {}", list_or_none(&report.incidents)),
            format!("recommendations: {}", list_or_none(&report.recommendations)),
        ]
        .join("\n")
    };

    Ok(CliResult {
        command: "report daily".to_string(),
        output,
        status: "success",
        changed_files: Vec::new(),
        warnings: Vec::new(),
        audit: true,
    })
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}

fn list_or_none(items: &[String]) -> String {
    if items.is_empty() {
        "none".to_string()
    } else {
        items.join(", ")
    }
}
