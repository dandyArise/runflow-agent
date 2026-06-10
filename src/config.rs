use std::time::Duration;

#[derive(Clone, Debug)]
pub struct ModelConfig {
    pub provider: String,
    pub base_url: String,
    pub model: String,
    pub api_key: Option<String>,
    pub timeout: Duration,
}

impl ModelConfig {
    pub fn from_args(args: &[String]) -> Result<Self, String> {
        let provider = value_after(args, "--provider")
            .map(ToString::to_string)
            .or_else(|| std::env::var("RUNFLOW_AGENT_PROVIDER").ok())
            .unwrap_or_else(|| "mock".to_string());

        let default_base = match provider.as_str() {
            "ollama" => "http://localhost:11434",
            "openai-compatible" => "http://localhost:1234/v1",
            "mock" => "",
            other => return Err(format!("unsupported provider '{other}'")),
        };

        let base_url = value_after(args, "--base-url")
            .map(ToString::to_string)
            .or_else(|| std::env::var("RUNFLOW_AGENT_BASE_URL").ok())
            .unwrap_or_else(|| default_base.to_string());

        let model = value_after(args, "--model")
            .map(ToString::to_string)
            .or_else(|| std::env::var("RUNFLOW_AGENT_MODEL").ok())
            .unwrap_or_else(|| match provider.as_str() {
                "ollama" => "qwen2.5-coder:1.5b".to_string(),
                "openai-compatible" => "local-model".to_string(),
                _ => "deterministic-local".to_string(),
            });

        let api_key = value_after(args, "--api-key-env")
            .and_then(|name| std::env::var(name).ok())
            .or_else(|| std::env::var("RUNFLOW_AGENT_API_KEY").ok());

        let timeout_seconds = value_after(args, "--timeout-seconds")
            .map(str::parse::<u64>)
            .transpose()
            .map_err(|_| "--timeout-seconds must be an integer".to_string())?
            .or_else(|| {
                std::env::var("RUNFLOW_AGENT_TIMEOUT_SECONDS")
                    .ok()
                    .and_then(|value| value.parse::<u64>().ok())
            })
            .unwrap_or(30);

        Ok(Self {
            provider,
            base_url,
            model,
            api_key,
            timeout: Duration::from_secs(timeout_seconds),
        })
    }

    pub fn is_mock(&self) -> bool {
        self.provider == "mock"
    }
}

pub fn strip_model_flags(args: &[String]) -> Vec<String> {
    let mut out = Vec::new();
    let mut skip_next = false;
    for arg in args {
        if skip_next {
            skip_next = false;
            continue;
        }
        if matches!(
            arg.as_str(),
            "--provider" | "--base-url" | "--model" | "--api-key-env" | "--timeout-seconds"
        ) {
            skip_next = true;
            continue;
        }
        out.push(arg.clone());
    }
    out
}

fn value_after<'a>(args: &'a [String], flag: &str) -> Option<&'a str> {
    args.windows(2)
        .find(|pair| pair[0] == flag)
        .map(|pair| pair[1].as_str())
}
