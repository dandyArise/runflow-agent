# Model Providers

RunFlow Agent supports a small provider surface:

- `mock`: deterministic local behavior, no model call.
- `ollama`: native Ollama HTTP API.
- `openai-compatible`: OpenAI-compatible chat completions over local HTTP, including LM Studio.

The built-in HTTP client intentionally supports `http://` endpoints only. This covers local Ollama and LM Studio. Cloud HTTPS providers should be added later with an explicit TLS HTTP dependency and secret-handling review.

## Common Flags

```powershell
--provider mock|ollama|openai-compatible
--base-url <http-url>
--model <name>
--api-key-env <ENV_NAME>
--timeout-seconds <n>
```

Environment variables:

```text
RUNFLOW_AGENT_PROVIDER
RUNFLOW_AGENT_BASE_URL
RUNFLOW_AGENT_MODEL
RUNFLOW_AGENT_API_KEY
RUNFLOW_AGENT_TIMEOUT_SECONDS
```

## Ollama

Default base URL:

```text
http://localhost:11434
```

RunFlow Agent calls:

```text
POST /api/chat
```

Example:

```powershell
runflow-agent draft --prompt "Ping 1.1.1.1 every 5 minutes" --provider ollama --model qwen2.5-coder:1.5b
```

Reference: [Ollama chat API](https://docs.ollama.com/api/chat).

## LM Studio

LM Studio is used through `openai-compatible`.

Default LM Studio base URL:

```text
http://localhost:1234/v1
```

RunFlow Agent calls:

```text
POST /v1/chat/completions
```

Example:

```powershell
runflow-agent draft --prompt "Ping 1.1.1.1 every 5 minutes" --provider openai-compatible --base-url http://localhost:1234/v1 --model qwen/qwen3-coder-30b --timeout-seconds 120
```

LM Studio may take longer than the default 30 seconds when a model is loading or cold. Use `--timeout-seconds 120` for local large models.

Reference: [LM Studio OpenAI compatibility](https://lmstudio.ai/docs/developer/openai-compat).

## OpenAI-Compatible

Use this for local servers that expose OpenAI-style chat completions, such as LM Studio, vLLM, LocalAI, or llama.cpp server when configured with an OpenAI-compatible endpoint.

Required:

- `base_url`
- `model`

Optional:

- API key through `--api-key-env` or `RUNFLOW_AGENT_API_KEY`

Reference: [OpenAI Chat Completions](https://developers.openai.com/api/reference/chat-completions/overview/).

## Output Rules

All providers must return strict JSON only. RunFlow Agent rejects model output that does not contain the required fields for the active command.

The agent remains assist-only:

- no job execution;
- no cancellation;
- no rerun;
- no shell execution;
- no secret editing;
- no alerts or external API calls from generated actions.
