pub struct Validation {
    pub valid: bool,
    pub messages: Vec<String>,
}

const WORKFLOW_SCHEMA: &str = include_str!("schema_defs/v1/workflow.schema.json");

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
    if yaml.trim().is_empty() {
        return Validation {
            valid: false,
            messages: vec!["workflow is empty".to_string()],
        };
    }

    let instance = match serde_yaml::from_str::<serde_json::Value>(yaml) {
        Ok(value) => value,
        Err(error) => {
            return Validation {
                valid: false,
                messages: vec![format!("failed to parse workflow YAML: {error}")],
            };
        }
    };

    let schema = match serde_json::from_str::<serde_json::Value>(WORKFLOW_SCHEMA) {
        Ok(value) => value,
        Err(error) => {
            return Validation {
                valid: false,
                messages: vec![format!("failed to parse embedded workflow schema: {error}")],
            };
        }
    };

    let validator = match jsonschema::validator_for(&schema) {
        Ok(validator) => validator,
        Err(error) => {
            return Validation {
                valid: false,
                messages: vec![format!(
                    "failed to compile embedded workflow schema: {error}"
                )],
            };
        }
    };

    let messages = validator
        .iter_errors(&instance)
        .map(|error| format!("{}: {}", error.instance_path(), error))
        .collect::<Vec<_>>();

    Validation {
        valid: messages.is_empty(),
        messages,
    }
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

    #[test]
    fn rejects_invalid_name_and_version() {
        let yaml = "name: Bad Name\nversion: 0\nschema_version: 1\nsteps: []\n";
        let validation = validate_workflow(yaml);
        assert!(!validation.valid);
        assert!(validation
            .messages
            .iter()
            .any(|item| item.contains("/name")));
        assert!(validation
            .messages
            .iter()
            .any(|item| item.contains("/version")));
    }

    #[test]
    fn rejects_invalid_step_type() {
        let yaml = "name: demo\nsteps:\n  - name: bad\n    type: unknown\n";
        let validation = validate_workflow(yaml);
        assert!(!validation.valid);
        assert!(validation.messages.iter().any(|item| item.contains("type")));
    }

    #[test]
    fn rejects_invalid_yaml() {
        let yaml = "name: demo\nsteps:\n  - name: bad\n    type: command\n   nope";
        let validation = validate_workflow(yaml);
        assert!(!validation.valid);
        assert!(validation
            .messages
            .iter()
            .any(|item| item.contains("failed to parse workflow YAML")));
    }
}
