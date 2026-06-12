use serde::de::DeserializeOwned;
use serde::Deserialize;

use crate::runflow::Finding;

#[derive(Debug, Deserialize)]
pub struct DraftModelResponse {
    pub kind: String,
    pub workflow_yaml: String,
    #[serde(default)]
    pub needs_tool: Option<String>,
    #[serde(default)]
    pub warnings: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct ReviewModelResponse {
    pub kind: String,
    #[allow(dead_code)]
    pub valid: bool,
    #[serde(default)]
    pub findings: Vec<ModelFinding>,
}

#[derive(Debug, Deserialize)]
pub struct ModelFinding {
    pub severity: String,
    pub path: String,
    pub message: String,
    #[serde(default)]
    pub suggestion: String,
}

#[derive(Debug, Deserialize)]
pub struct RunExplanationModelResponse {
    pub kind: String,
    pub summary: String,
    #[serde(default)]
    pub suggested_next_steps: Vec<String>,
}

#[derive(Debug, Deserialize)]
pub struct DailyReportModelResponse {
    pub kind: String,
    #[serde(default)]
    pub recommendations: Vec<String>,
}

pub trait ModelKind {
    fn kind(&self) -> &str;
}

impl ModelKind for DraftModelResponse {
    fn kind(&self) -> &str {
        &self.kind
    }
}

impl ModelKind for ReviewModelResponse {
    fn kind(&self) -> &str {
        &self.kind
    }
}

impl ModelKind for RunExplanationModelResponse {
    fn kind(&self) -> &str {
        &self.kind
    }
}

impl ModelKind for DailyReportModelResponse {
    fn kind(&self) -> &str {
        &self.kind
    }
}

impl From<ModelFinding> for Finding {
    fn from(value: ModelFinding) -> Self {
        Self {
            severity: value.severity,
            path: value.path,
            message: value.message,
            suggestion: value.suggestion,
        }
    }
}

pub fn decode_model_json<T>(raw: &str, expected_kind: &str) -> Result<T, String>
where
    T: DeserializeOwned + ModelKind,
{
    let source = extract_single_object(raw)?;
    let decoded: T = serde_json::from_str(source)
        .map_err(|e| format!("model output is not valid JSON for {expected_kind}: {e}"))?;
    if decoded.kind() != expected_kind {
        return Err(format!(
            "model output kind mismatch: expected '{expected_kind}', got '{}'",
            decoded.kind()
        ));
    }
    Ok(decoded)
}

fn extract_single_object(raw: &str) -> Result<&str, String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return Err("model output is empty".to_string());
    }
    if trimmed.starts_with("```") || trimmed.ends_with("```") || trimmed.contains("```") {
        return Err("model output must be raw JSON, not markdown".to_string());
    }
    if !trimmed.starts_with('{') || !trimmed.ends_with('}') {
        return Err(
            "model output must be a single JSON object with no surrounding prose".to_string(),
        );
    }
    Ok(trimmed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_single_object_with_kind() {
        let parsed: DraftModelResponse = decode_model_json(
            "{\"kind\":\"draft_workflow\",\"workflow_yaml\":\"x\"}",
            "draft_workflow",
        )
        .unwrap();
        assert_eq!(parsed.workflow_yaml, "x");
    }

    #[test]
    fn rejects_markdown_fence() {
        let result: Result<DraftModelResponse, String> = decode_model_json(
            "```json\n{\"kind\":\"draft_workflow\"}\n```",
            "draft_workflow",
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_prose_prefix() {
        let result: Result<DraftModelResponse, String> = decode_model_json(
            "Here is JSON: {\"kind\":\"draft_workflow\"}",
            "draft_workflow",
        );
        assert!(result.is_err());
    }

    #[test]
    fn rejects_wrong_kind() {
        let result: Result<DraftModelResponse, String> =
            decode_model_json("{\"kind\":\"workflow_review\"}", "draft_workflow");
        assert!(result.is_err());
    }

    #[test]
    fn extracts_arrays() {
        let parsed: DailyReportModelResponse = decode_model_json(
            "{\"kind\":\"daily_report\",\"recommendations\":[\"a\",\"b\"]}",
            "daily_report",
        )
        .unwrap();
        assert_eq!(parsed.recommendations, vec!["a", "b"]);
    }

    #[test]
    fn rejects_missing_required_field() {
        let result: Result<RunExplanationModelResponse, String> =
            decode_model_json("{\"kind\":\"run_explanation\"}", "run_explanation");
        assert!(result.is_err());
    }
}
