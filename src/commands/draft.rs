use std::fs;

use crate::agent;
use crate::cli::CliResult;
use crate::config::{self, ModelConfig};
use crate::runflow;

pub fn run(args: &[String]) -> Result<CliResult, String> {
    let model_config = ModelConfig::from_args(args)?;
    let command_args = config::strip_model_flags(args);
    let prompt = value_after(&command_args, "--prompt");
    let input = value_after(&command_args, "--input");
    let output = value_after(&command_args, "--output");

    let request = match (prompt, input) {
        (Some(text), None) => text.to_string(),
        (None, Some(path)) => {
            fs::read_to_string(path).map_err(|e| format!("cannot read input '{path}': {e}"))?
        }
        (Some(_), Some(_)) => return Err("use either --prompt or --input, not both".to_string()),
        (None, None) => return Err("draft requires --prompt <text> or --input <file>".to_string()),
    };

    let draft = agent::draft_workflow_with_model(&request, &model_config)?;
    let validation = runflow::validate_workflow(&draft.workflow_yaml);
    if !validation.valid {
        return Err(format!(
            "internal draft failed validation: {}",
            validation.messages.join("; ")
        ));
    }

    let mut changed_files = Vec::new();
    if let Some(path) = output {
        fs::write(path, &draft.workflow_yaml)
            .map_err(|e| format!("cannot write output '{path}': {e}"))?;
        changed_files.push(path.to_string());
    }

    Ok(CliResult {
        command: "draft".to_string(),
        output: draft.workflow_yaml,
        status: "success",
        changed_files,
        warnings: draft.warnings,
        audit: true,
    })
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
    fn draft_requires_prompt_or_input() {
        assert!(run(&[]).is_err());
    }
}
