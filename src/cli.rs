use crate::commands;

#[derive(Debug)]
pub struct CliResult {
    pub command: String,
    pub output: String,
    pub status: &'static str,
    pub changed_files: Vec<String>,
    pub warnings: Vec<String>,
    pub audit: bool,
}

pub fn run(args: Vec<String>) -> Result<CliResult, String> {
    let Some(command) = args.get(1).map(String::as_str) else {
        return Ok(help());
    };

    match command {
        "-h" | "--help" | "help" => Ok(help()),
        "doctor" => commands::doctor::run(&args[2..]),
        "draft" => commands::draft::run(&args[2..]),
        "review" => commands::review::run(&args[2..]),
        "explain-run" => commands::explain_run::run(&args[2..]),
        "inspect-workspace" => commands::inspect_workspace::run(&args[2..]),
        "report" => commands::report::run(&args[2..]),
        "self" => commands::self_update::run(&args[2..]),
        "watch" => commands::watch::run(&args[2..]),
        other => Err(format!(
            "unknown command '{other}'. Run `runflow-agent --help`."
        )),
    }
}

fn help() -> CliResult {
    CliResult {
        command: "help".to_string(),
        status: "success",
        changed_files: Vec::new(),
        warnings: Vec::new(),
        audit: false,
        output: [
            "RunFlow Agent",
            "",
            "Usage:",
            "  runflow-agent doctor [--root <path>] [--format json]",
            "  runflow-agent draft --prompt <text> [--output <workflow.yml>]",
            "  runflow-agent draft --input <request.txt> [--output <workflow.yml>]",
            "  runflow-agent review <workflow.yml> [--format json]",
            "  runflow-agent explain-run <run_id> [--format json]",
            "  runflow-agent inspect-workspace [--root <path>] [--limit <n>] [--health] [--format json]",
            "  runflow-agent report daily [--from <iso>] [--to <iso>] [--format json]",
            "  runflow-agent self version [--format json]",
            "  runflow-agent self update [--version <tag>] [--install-dir <path>] [--dry-run] [--format json]",
            "  runflow-agent watch --once [--root <path>] [--format json] [--output <path>]",
            "",
            "Model options:",
            "  --provider mock|ollama|openai-compatible",
            "  --base-url <http-url>",
            "  --model <name>",
            "  --api-key-env <ENV_NAME>",
            "  --timeout-seconds <n>",
            "",
            "Safety: assist-only; no job execution, cancellation, shell execution, alerts, secrets, or external API calls.",
        ]
        .join("\n"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn review_invalid_workflow_returns_failed_status() {
        let path = std::env::temp_dir().join(format!(
            "runflow-agent-invalid-{}.yml",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(
            &path,
            "name: Bad Name\nversion: 0\nunknown: true\nsteps: []\n",
        )
        .unwrap();
        let result = run(vec![
            "runflow-agent".to_string(),
            "review".to_string(),
            path.to_string_lossy().to_string(),
            "--format".to_string(),
            "json".to_string(),
        ])
        .unwrap();
        let _ = std::fs::remove_file(path);
        assert_eq!(result.status, "failed");
        assert!(result.output.contains("\"valid\":false"));
    }
}
