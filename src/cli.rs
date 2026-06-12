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

    if is_help_arg(command) {
        return Ok(help());
    }
    if command == "help" {
        return help_topic(&args[2..]);
    }
    if command == "-V" || command == "--version" {
        return Ok(success_result(
            "version",
            format!("runflow-agent {}", env!("CARGO_PKG_VERSION")),
        ));
    }
    if args[2..].iter().any(|arg| is_help_arg(arg)) {
        return help_topic(&args[1..]);
    }

    match command {
        "autopilot" => commands::autopilot::run(&args[2..]),
        "doctor" => commands::doctor::run(&args[2..]),
        "draft" => commands::draft::run(&args[2..]),
        "review" => commands::review::run(&args[2..]),
        "explain-run" => commands::explain_run::run(&args[2..]),
        "inspect-workspace" => commands::inspect_workspace::run(&args[2..]),
        "oncall" => commands::oncall::run(&args[2..]),
        "report" => commands::report::run(&args[2..]),
        "self" => commands::self_update::run(&args[2..]),
        "watch" => commands::watch::run(&args[2..]),
        other => Err(format!(
            "unknown command '{other}'. Run `runflow-agent --help`."
        )),
    }
}

fn help() -> CliResult {
    success_result(
        "help",
        [
            "RunFlow Agent",
            "",
            "Usage:",
            "  runflow-agent <command> [options]",
            "  runflow-agent help [command]",
            "",
            "Commands:",
            "  autopilot          Plan or apply local assist-only proposals",
            "  doctor             Check runtime configuration and workspace policy",
            "  draft              Draft a workflow from a prompt or input file",
            "  review             Validate and review a workflow file",
            "  explain-run        Explain a recorded run",
            "  inspect-workspace  Inspect workspace runs, jobs, and health",
            "  oncall             Build an incident handoff",
            "  report             Generate reports",
            "  self               Show version or update the binary",
            "  watch              Emit or stream workspace snapshots",
            "",
            "Options:",
            "  -h, --help         Show help",
            "  -V, --version      Show version",
            "",
            "Run `runflow-agent help <command>` for command-specific help.",
            "",
            "Safety: assist-only; no job execution, cancellation, shell execution, alerts, secrets, or external API calls.",
        ]
        .join("\n"),
    )
}

fn help_topic(topic: &[String]) -> Result<CliResult, String> {
    let topic = topic
        .iter()
        .map(String::as_str)
        .filter(|arg| !is_help_arg(arg))
        .collect::<Vec<_>>();
    if topic.is_empty() {
        return Ok(help());
    }

    let command = topic[0];
    let subcommand = topic.get(1).copied();
    let output = match (command, subcommand) {
        ("autopilot", Some("plan")) => [
            "Usage:",
            "  runflow-agent autopilot plan [options]",
            "",
            "Options:",
            "  --root <path>          Workspace root",
            "  --limit <n>            Maximum items to inspect",
            "  --from-watch <json>    Use a watch snapshot as source",
            "  --from-oncall <json>   Use an oncall handoff as source",
            "  --format json          Emit JSON",
            "  --output <path>        Write output to file",
            "  -h, --help             Show help",
        ]
        .join("\n"),
        ("autopilot", Some("apply")) => [
            "Usage:",
            "  runflow-agent autopilot apply --proposal <json> --proposal-id <id> --confirm <token> [options]",
            "",
            "Options:",
            "  --proposal <json>      Proposal file",
            "  --proposal-id <id>     Proposal id to apply",
            "  --confirm <token>      Confirmation token",
            "  --root <path>          Workspace root",
            "  --format json          Emit JSON",
            "  -h, --help             Show help",
        ]
        .join("\n"),
        ("autopilot", None) => [
            "Usage:",
            "  runflow-agent autopilot <command> [options]",
            "",
            "Commands:",
            "  plan   Build assist-only proposals",
            "  apply  Apply an allowlisted local draft proposal",
            "",
            "Run `runflow-agent help autopilot <command>` for more information.",
        ]
        .join("\n"),
        ("doctor", None) => [
            "Usage:",
            "  runflow-agent doctor [options]",
            "",
            "Options:",
            "  --root <path>    Workspace root",
            "  --format json    Emit JSON",
            "  -h, --help       Show help",
        ]
        .join("\n"),
        ("draft", None) => [
            "Usage:",
            "  runflow-agent draft --prompt <text> [options]",
            "  runflow-agent draft --input <request.txt> [options]",
            "",
            "Options:",
            "  --prompt <text>          Request text",
            "  --input <request.txt>    Request file",
            "  --output <workflow.yml>  Write workflow to file",
            "  -h, --help               Show help",
            "",
            "Model options:",
            "  --provider mock|ollama|openai-compatible",
            "  --base-url <http-url>",
            "  --model <name>",
            "  --api-key-env <ENV_NAME>",
            "  --timeout-seconds <n>",
        ]
        .join("\n"),
        ("review", None) => [
            "Usage:",
            "  runflow-agent review <workflow.yml> [options]",
            "",
            "Options:",
            "  --format json    Emit JSON",
            "  -h, --help       Show help",
        ]
        .join("\n"),
        ("explain-run", None) => [
            "Usage:",
            "  runflow-agent explain-run <run_id> [options]",
            "",
            "Options:",
            "  --format json    Emit JSON",
            "  -h, --help       Show help",
            "",
            "Model options are supported.",
        ]
        .join("\n"),
        ("inspect-workspace", None) => [
            "Usage:",
            "  runflow-agent inspect-workspace [options]",
            "",
            "Options:",
            "  --root <path>    Workspace root",
            "  --limit <n>      Maximum items to inspect",
            "  --health         Include health diagnostics",
            "  --format json    Emit JSON",
            "  -h, --help       Show help",
        ]
        .join("\n"),
        ("oncall", None) => [
            "Usage:",
            "  runflow-agent oncall [options]",
            "",
            "Options:",
            "  --root <path>        Workspace root",
            "  --window-hours <n>   Incident window",
            "  --run-id <id>        Filter by run id",
            "  --job <name>         Filter by job name",
            "  --format json        Emit JSON",
            "  --output <path>      Write output to file",
            "  -h, --help           Show help",
        ]
        .join("\n"),
        ("report", Some("daily")) => [
            "Usage:",
            "  runflow-agent report daily [options]",
            "",
            "Options:",
            "  --from <iso>     Start time",
            "  --to <iso>       End time",
            "  --format json    Emit JSON",
            "  -h, --help       Show help",
            "",
            "Model options are supported.",
        ]
        .join("\n"),
        ("report", None) => [
            "Usage:",
            "  runflow-agent report <command> [options]",
            "",
            "Commands:",
            "  daily  Generate a daily report",
        ]
        .join("\n"),
        ("self", Some("version")) => [
            "Usage:",
            "  runflow-agent self version [options]",
            "",
            "Options:",
            "  --format json    Emit JSON",
            "  -h, --help       Show help",
        ]
        .join("\n"),
        ("self", Some("update")) => [
            "Usage:",
            "  runflow-agent self update [options]",
            "",
            "Options:",
            "  --version <tag>        Version tag to install",
            "  --install-dir <path>   Install directory",
            "  --dry-run              Print planned update without installing",
            "  --format json          Emit JSON",
            "  -h, --help             Show help",
        ]
        .join("\n"),
        ("self", None) => [
            "Usage:",
            "  runflow-agent self <command> [options]",
            "",
            "Commands:",
            "  version  Show installed version",
            "  update   Update the binary",
        ]
        .join("\n"),
        ("watch", None) => [
            "Usage:",
            "  runflow-agent watch [options]",
            "  runflow-agent watch --once [options]",
            "",
            "Options:",
            "  --root <path>              Workspace root",
            "  --limit <n>                Maximum items to inspect",
            "  --interval-seconds <n>     Polling interval",
            "  --once                     Emit one snapshot and exit",
            "  --format json              Emit JSON",
            "  --output <path>            Write output to file",
            "  -h, --help                 Show help",
        ]
        .join("\n"),
        (known, Some(unknown)) if is_known_command(known) => {
            return Err(format!(
                "unknown help topic '{known} {unknown}'. Run `runflow-agent help {known}`."
            ));
        }
        (unknown, _) => {
            return Err(format!(
                "unknown help topic '{unknown}'. Run `runflow-agent help`."
            ));
        }
    };

    Ok(success_result("help", output))
}

fn is_help_arg(arg: &str) -> bool {
    arg == "-h" || arg == "--help"
}

fn is_known_command(command: &str) -> bool {
    matches!(
        command,
        "autopilot"
            | "doctor"
            | "draft"
            | "review"
            | "explain-run"
            | "inspect-workspace"
            | "oncall"
            | "report"
            | "self"
            | "watch"
    )
}

fn success_result(command: &str, output: String) -> CliResult {
    CliResult {
        command: command.to_string(),
        output,
        status: "success",
        changed_files: Vec::new(),
        warnings: Vec::new(),
        audit: false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn top_level_help_uses_standard_flags() {
        let result = run(vec!["runflow-agent".to_string(), "--help".to_string()]).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.output.contains("Usage:"));
        assert!(result.output.contains("runflow-agent <command> [options]"));
        assert!(result.output.contains("-h, --help"));
    }

    #[test]
    fn command_help_does_not_execute_command() {
        let result = run(vec![
            "runflow-agent".to_string(),
            "draft".to_string(),
            "--help".to_string(),
        ])
        .unwrap();
        assert_eq!(result.status, "success");
        assert!(result
            .output
            .contains("runflow-agent draft --prompt <text>"));
        assert!(result.output.contains("--input <request.txt>"));
    }

    #[test]
    fn help_command_accepts_nested_topics() {
        let result = run(vec![
            "runflow-agent".to_string(),
            "help".to_string(),
            "autopilot".to_string(),
            "plan".to_string(),
        ])
        .unwrap();
        assert_eq!(result.status, "success");
        assert!(result
            .output
            .contains("runflow-agent autopilot plan [options]"));
        assert!(result.output.contains("--from-watch <json>"));
    }

    #[test]
    fn version_flag_is_successful() {
        let result = run(vec!["runflow-agent".to_string(), "--version".to_string()]).unwrap();
        assert_eq!(result.status, "success");
        assert!(result.output.starts_with("runflow-agent "));
    }

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
