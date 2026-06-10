use std::collections::HashSet;

pub struct Validation {
    pub valid: bool,
    pub messages: Vec<String>,
}

#[derive(Clone)]
pub struct Finding {
    pub severity: String,
    pub path: String,
    pub message: String,
    pub suggestion: String,
}

impl Finding {
    pub fn error(path: &str, message: &str, suggestion: &str) -> Self {
        Self::new("error", path, message, suggestion)
    }

    pub fn warning(path: &str, message: &str, suggestion: &str) -> Self {
        Self::new("warning", path, message, suggestion)
    }

    pub fn info(path: &str, message: &str, suggestion: &str) -> Self {
        Self::new("info", path, message, suggestion)
    }

    fn new(severity: &str, path: &str, message: &str, suggestion: &str) -> Self {
        Self {
            severity: severity.to_string(),
            path: path.to_string(),
            message: message.to_string(),
            suggestion: suggestion.to_string(),
        }
    }
}

pub fn validate_workflow(yaml: &str) -> Validation {
    let mut messages = Vec::new();
    if yaml.trim().is_empty() {
        messages.push("workflow is empty".to_string());
        return Validation {
            valid: false,
            messages,
        };
    }

    let allowed = allowed_top_level_fields();
    let mut has_name = false;
    let mut has_steps = false;
    let mut has_bad_tabs = false;

    for (idx, line) in yaml.lines().enumerate() {
        if line.contains('\t') {
            has_bad_tabs = true;
        }
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || line.starts_with(' ')
            || line.starts_with('-')
        {
            continue;
        }
        if let Some((key, _)) = trimmed.split_once(':') {
            let key = key.trim();
            if key == "name" {
                has_name = true;
            }
            if key == "steps" {
                has_steps = true;
            }
            if !allowed.contains(key) {
                messages.push(format!(
                    "unknown top-level field '{key}' at line {}",
                    idx + 1
                ));
            }
        }
    }

    if !has_name {
        messages.push("missing required top-level field 'name'".to_string());
    }
    if !has_steps {
        messages.push("missing top-level field 'steps'".to_string());
    }
    if has_bad_tabs {
        messages.push("YAML contains tab indentation".to_string());
    }
    if yaml.contains("type: command") && !yaml.contains("run:") {
        messages.push("command step is missing run block".to_string());
    }
    if yaml.contains("run:") && !(yaml.contains("command:") || yaml.contains("args:")) {
        messages.push("run block should use structured command and optional args".to_string());
    }

    Validation {
        valid: messages.is_empty(),
        messages,
    }
}

fn allowed_top_level_fields() -> HashSet<&'static str> {
    [
        "name",
        "version",
        "schema_version",
        "schedule",
        "failure_policy",
        "concurrency",
        "limits",
        "locks",
        "secrets",
        "notifications",
        "retention",
        "steps",
        "tests",
    ]
    .into_iter()
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_generated_shape() {
        let yaml = "name: demo\nversion: 1\nschema_version: 1\nschedule: false\nsteps:\n  - name: task\n    type: command\n    run:\n      command: echo\n      args: [\"ok\"]\n";
        assert!(validate_workflow(yaml).valid);
    }

    #[test]
    fn rejects_unknown_field() {
        let yaml = "name: demo\nunknown: true\nsteps: []\n";
        assert!(!validate_workflow(yaml).valid);
    }
}
