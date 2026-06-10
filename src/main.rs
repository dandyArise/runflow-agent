mod agent;
mod audit;
mod cli;
mod commands;
mod config;
mod json;
mod model;
mod runflow;
mod strict_json;
mod util;

use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run(std::env::args().collect()) {
        Ok(result) => {
            if !result.output.is_empty() {
                println!("{}", result.output);
            }
            if result.audit {
                let _ = audit::record(
                    &result.command,
                    result.status,
                    &result.changed_files,
                    &result.warnings,
                );
            }
            ExitCode::SUCCESS
        }
        Err(err) => {
            eprintln!("error: {}", err);
            ExitCode::from(1)
        }
    }
}
