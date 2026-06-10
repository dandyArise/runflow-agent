use std::io::{Read, Write};
use std::net::TcpStream;
use std::time::Duration;

use crate::config::ModelConfig;
use crate::json;
use crate::util;
use serde::Deserialize;

pub fn chat(config: &ModelConfig, system: &str, user: &str) -> Result<String, String> {
    match config.provider.as_str() {
        "mock" => Err("mock provider does not call a model".to_string()),
        "ollama" => chat_ollama(config, system, user),
        "openai-compatible" => chat_openai_compatible(config, system, user),
        other => Err(format!("unsupported provider '{other}'")),
    }
}

fn chat_ollama(config: &ModelConfig, system: &str, user: &str) -> Result<String, String> {
    let url = format!("{}/api/chat", config.base_url.trim_end_matches('/'));
    let body = format!(
        "{{\"model\":\"{}\",\"stream\":false,\"options\":{{\"temperature\":0.1}},\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}]}}",
        json::escape(&config.model),
        json::escape(system),
        json::escape(user)
    );
    let response = http_post_json(&url, &[], &body, config.timeout)?;
    let decoded: OllamaChatResponse = serde_json::from_str(&response)
        .map_err(|e| format!("invalid Ollama JSON response: {e}"))?;
    Ok(decoded.message.content)
}

fn chat_openai_compatible(
    config: &ModelConfig,
    system: &str,
    user: &str,
) -> Result<String, String> {
    let url = format!("{}/chat/completions", config.base_url.trim_end_matches('/'));
    let body = format!(
        "{{\"model\":\"{}\",\"temperature\":0.1,\"messages\":[{{\"role\":\"system\",\"content\":\"{}\"}},{{\"role\":\"user\",\"content\":\"{}\"}}]}}",
        json::escape(&config.model),
        json::escape(system),
        json::escape(user)
    );
    let mut headers = Vec::new();
    if let Some(api_key) = &config.api_key {
        headers.push(("Authorization".to_string(), format!("Bearer {api_key}")));
    }
    let response = http_post_json(&url, &headers, &body, config.timeout)?;
    let decoded: OpenAiChatResponse = serde_json::from_str(&response)
        .map_err(|e| format!("invalid OpenAI-compatible JSON response: {e}"))?;
    decoded
        .choices
        .into_iter()
        .next()
        .map(|choice| choice.message.content)
        .ok_or_else(|| {
            "OpenAI-compatible response did not contain choices[0].message.content".to_string()
        })
}

#[derive(Debug, Deserialize)]
struct OllamaChatResponse {
    message: OllamaMessage,
}

#[derive(Debug, Deserialize)]
struct OllamaMessage {
    content: String,
}

#[derive(Debug, Deserialize)]
struct OpenAiChatResponse {
    choices: Vec<OpenAiChoice>,
}

#[derive(Debug, Deserialize)]
struct OpenAiChoice {
    message: OpenAiMessage,
}

#[derive(Debug, Deserialize)]
struct OpenAiMessage {
    content: String,
}

pub(crate) fn http_post_json(
    url: &str,
    headers: &[(String, String)],
    body: &str,
    timeout: Duration,
) -> Result<String, String> {
    let parsed = HttpUrl::parse(url)?;
    let mut stream = TcpStream::connect((&*parsed.host, parsed.port))
        .map_err(|e| format!("cannot connect to {}:{}: {e}", parsed.host, parsed.port))?;
    stream
        .set_read_timeout(Some(timeout))
        .map_err(|e| format!("cannot set read timeout: {e}"))?;
    stream
        .set_write_timeout(Some(timeout))
        .map_err(|e| format!("cannot set write timeout: {e}"))?;

    let mut request = format!(
        "POST {} HTTP/1.1\r\nHost: {}\r\nContent-Type: application/json\r\nAccept: application/json\r\nContent-Length: {}\r\nConnection: close\r\n",
        parsed.path,
        parsed.host,
        body.as_bytes().len()
    );
    for (name, value) in headers {
        request.push_str(name);
        request.push_str(": ");
        request.push_str(value);
        request.push_str("\r\n");
    }
    request.push_str("\r\n");
    request.push_str(body);

    stream
        .write_all(request.as_bytes())
        .map_err(|e| format!("cannot write request: {e}"))?;

    let mut raw = Vec::new();
    stream
        .read_to_end(&mut raw)
        .map_err(|e| format!("cannot read response: {e}"))?;
    let text = String::from_utf8_lossy(&raw).to_string();
    let (head, body) = text
        .split_once("\r\n\r\n")
        .ok_or_else(|| "invalid HTTP response".to_string())?;
    let status = head.lines().next().unwrap_or_default();
    if !(status.contains(" 200 ") || status.contains(" 201 ")) {
        return Err(format!(
            "model HTTP request failed: {status}; body={}",
            util::truncate(body, 500)
        ));
    }
    if head.to_lowercase().contains("transfer-encoding: chunked") {
        decode_chunked(body)
    } else {
        Ok(body.to_string())
    }
}

struct HttpUrl {
    host: String,
    port: u16,
    path: String,
}

impl HttpUrl {
    fn parse(url: &str) -> Result<Self, String> {
        let rest = url.strip_prefix("http://").ok_or_else(|| {
            "only http:// model endpoints are supported by the built-in client".to_string()
        })?;
        let (host_port, path) = rest
            .split_once('/')
            .map(|(host, path)| (host, format!("/{path}")))
            .unwrap_or((rest, "/".to_string()));
        let (host, port) = host_port
            .split_once(':')
            .map(|(host, port)| {
                let port = port
                    .parse::<u16>()
                    .map_err(|_| format!("invalid port in URL '{url}'"))?;
                Ok::<_, String>((host.to_string(), port))
            })
            .transpose()?
            .unwrap_or_else(|| (host_port.to_string(), 80));

        Ok(Self { host, port, path })
    }
}

fn decode_chunked(body: &str) -> Result<String, String> {
    let mut rest = body;
    let mut out = String::new();
    loop {
        let Some((size_hex, after_size)) = rest.split_once("\r\n") else {
            return Err("invalid chunked response".to_string());
        };
        let size = usize::from_str_radix(size_hex.trim(), 16)
            .map_err(|_| "invalid chunk size".to_string())?;
        if size == 0 {
            return Ok(out);
        }
        if after_size.len() < size + 2 {
            return Err("truncated chunked response".to_string());
        }
        out.push_str(&after_size[..size]);
        rest = &after_size[size + 2..];
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_http_url_with_port() {
        let url = HttpUrl::parse("http://localhost:1234/v1/chat/completions").unwrap();
        assert_eq!(url.host, "localhost");
        assert_eq!(url.port, 1234);
        assert_eq!(url.path, "/v1/chat/completions");
    }

    #[test]
    fn rejects_https_for_builtin_client() {
        assert!(HttpUrl::parse("https://api.openai.com/v1").is_err());
    }
}
