use crate::tools::Tools;
use serde_json::{json, Value};
use std::path::Path;

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:15721";
pub const DEFAULT_MODEL: &str = "deepseek-v4.1-flash";
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

const MAX_TURNS: usize = 32;

const SYSTEM_PROMPT: &str = "You are Ram Code, a coding agent working inside one git worktree. \
Use the tools to inspect and change files. Keep changes small and focused on the task. \
Every path is relative to the worktree root. When the task is done, reply with a short summary \
of what changed and why. Do not ask for permission; finish the task.";

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub base_url: String,
    pub token: String,
    pub model: String,
    pub max_tokens: u32,
}

impl AgentConfig {
    pub fn from_env(f: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let get = |key: &str| f(key).filter(|value| !value.trim().is_empty());
        let token = get("ANTHROPIC_AUTH_TOKEN").ok_or_else(|| {
            "ANTHROPIC_AUTH_TOKEN is not set. Export it, then start the task again.".to_string()
        })?;
        let base_url =
            normalize_base(&get("ANTHROPIC_BASE_URL").unwrap_or_else(|| DEFAULT_BASE_URL.to_string()));
        let model = get("RAM_CODE_MODEL")
            .or_else(|| get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
            .or_else(|| get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"))
            .unwrap_or_else(|| DEFAULT_MODEL.to_string());
        let max_tokens = get("RAM_CODE_MAX_TOKENS")
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_MAX_TOKENS);
        Ok(Self {
            base_url,
            token,
            model,
            max_tokens,
        })
    }

    pub fn from_process_env() -> Result<Self, String> {
        Self::from_env(|key| std::env::var(key).ok())
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum AgentEvent {
    Text(String),
    ToolCall { name: String, input: String },
    ToolResult(String),
    Error(String),
}

pub trait Llm: Send + Sync {
    fn complete(&self, request: &Value) -> Result<Value, String>;

    fn model(&self) -> &str {
        DEFAULT_MODEL
    }

    fn max_tokens(&self) -> u32 {
        DEFAULT_MAX_TOKENS
    }
}

pub struct AnthropicLlm {
    pub config: AgentConfig,
}

impl AnthropicLlm {
    pub fn new(config: AgentConfig) -> Self {
        Self { config }
    }
}

impl Llm for AnthropicLlm {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        let url = messages_url(&self.config.base_url);
        let body = serde_json::to_string(request)
            .map_err(|e| format!("could not encode the model request: {e}"))?;
        let agent = ureq::Agent::new_with_config(
            ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(std::time::Duration::from_secs(120)))
                .build(),
        );
        let mut response = agent
            .post(&url)
            .header("authorization", format!("Bearer {}", self.config.token))
            .header("x-api-key", &self.config.token)
            .header("anthropic-version", "2023-06-01")
            .header("content-type", "application/json")
            .send(body.as_str())
            .map_err(|e| format!("model API request to {url} failed: {e}"))?;
        let status = response.status().as_u16();
        let text = response
            .body_mut()
            .read_to_string()
            .map_err(|e| format!("could not read the model response: {e}"))?;
        if status >= 400 {
            return Err(format!(
                "model API returned {status}: {}",
                shorten(&text, 2000)
            ));
        }
        serde_json::from_str(&text)
            .map_err(|e| format!("model API returned invalid JSON: {e}"))
    }

    fn model(&self) -> &str {
        &self.config.model
    }

    fn max_tokens(&self) -> u32 {
        self.config.max_tokens
    }
}

pub fn run_task(
    llm: &dyn Llm,
    root: &Path,
    task: &str,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<String, String> {
    run_turn(llm, root, &[], task, on_event).map(|(answer, _)| answer)
}

/// Run one task with the earlier turns, and return the answer with the new history.
pub fn run_turn(
    llm: &dyn Llm,
    root: &Path,
    history: &[Value],
    task: &str,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<(String, Vec<Value>), String> {
    let tools = Tools::new(root);
    let definitions = Tools::definitions();
    let mut messages: Vec<Value> = history.to_vec();
    messages.push(json!({"role": "user", "content": task}));
    let mut answer = String::new();

    for _ in 0..MAX_TURNS {
        let request = json!({
            "model": llm.model(),
            "max_tokens": llm.max_tokens(),
            "system": SYSTEM_PROMPT,
            "messages": messages,
            "tools": definitions,
        });
        let response = llm.complete(&request)?;

        if let Some(error) = response.get("error") {
            let message = error
                .get("message")
                .and_then(Value::as_str)
                .unwrap_or("unknown error");
            return Err(format!("model API error: {message}"));
        }

        let content = response
            .get("content")
            .and_then(Value::as_array)
            .ok_or_else(|| format!("model response has no content: {response}"))?;

        let mut assistant_blocks = Vec::new();
        let mut tool_results = Vec::new();

        for block in content {
            match block.get("type").and_then(Value::as_str) {
                Some("text") => {
                    let text = block.get("text").and_then(Value::as_str).unwrap_or("");
                    if !text.is_empty() {
                        on_event(AgentEvent::Text(text.to_string()));
                        if !answer.is_empty() {
                            answer.push('\n');
                        }
                        answer.push_str(text);
                    }
                    assistant_blocks.push(block.clone());
                }
                Some("tool_use") => {
                    let name = block
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or("")
                        .to_string();
                    let input = block.get("input").cloned().unwrap_or_else(|| json!({}));
                    on_event(AgentEvent::ToolCall {
                        name: name.clone(),
                        input: input.to_string(),
                    });
                    let result = match tools.call(&name, &input) {
                        Ok(output) => output,
                        Err(error) => format!("error: {error}"),
                    };
                    on_event(AgentEvent::ToolResult(result.clone()));
                    assistant_blocks.push(block.clone());
                    tool_results.push(json!({
                        "type": "tool_result",
                        "tool_use_id": block.get("id").cloned().unwrap_or(Value::Null),
                        "content": result,
                    }));
                }
                _ => assistant_blocks.push(block.clone()),
            }
        }

        if tool_results.is_empty() {
            messages.push(json!({"role": "assistant", "content": content.clone()}));
            return Ok((answer.trim().to_string(), messages));
        }

        messages.push(json!({"role": "assistant", "content": assistant_blocks}));
        messages.push(json!({"role": "user", "content": tool_results}));
    }

    Err(format!("the task did not finish within {MAX_TURNS} turns"))
}

fn shorten(text: &str, limit: usize) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    format!("{}…", &text[..cut])
}

pub fn normalize_base(raw: &str) -> String {
    let trimmed = raw.trim();
    let unquoted = trimmed
        .strip_prefix('"')
        .and_then(|value| value.strip_suffix('"'))
        .or_else(|| {
            trimmed
                .strip_prefix('\'')
                .and_then(|value| value.strip_suffix('\''))
        })
        .unwrap_or(trimmed);

    let (scheme, authority) = match unquoted.rfind("://") {
        Some(index) => {
            let before = &unquoted[..index];
            let scheme = before
                .rsplit(|c: char| !(c.is_ascii_alphanumeric() || c == '+' || c == '-' || c == '.'))
                .next()
                .unwrap_or("");
            (scheme.to_ascii_lowercase(), &unquoted[index + 3..])
        }
        None => ("http".to_string(), unquoted),
    };

    let host = authority.split('/').next().unwrap_or("");
    if host.is_empty() {
        return DEFAULT_BASE_URL.to_string();
    }
    format!("{scheme}://{authority}")
        .trim_end_matches('/')
        .to_string()
}

pub fn messages_url(base_url: &str) -> String {
    let base = normalize_base(base_url);
    let base = base.strip_suffix("/v1/messages").unwrap_or(&base);
    let base = base.strip_suffix("/v1").unwrap_or(base);
    format!("{base}/v1/messages")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn config(base: &str) -> AgentConfig {
        AgentConfig::from_env(|key| match key {
            "ANTHROPIC_AUTH_TOKEN" => Some("token".into()),
            "ANTHROPIC_BASE_URL" => Some(base.into()),
            _ => None,
        })
        .expect("config")
    }

    #[test]
    fn base_url_without_scheme_gets_http() {
        let config = config("127.0.0.1:15721");
        assert_eq!(
            messages_url(&config.base_url),
            "http://127.0.0.1:15721/v1/messages"
        );
    }

    #[test]
    fn a_trailing_v1_is_not_duplicated() {
        assert_eq!(
            messages_url("http://127.0.0.1:15721/v1/"),
            "http://127.0.0.1:15721/v1/messages"
        );
        assert_eq!(
            messages_url("http://127.0.0.1:15721/v1/messages"),
            "http://127.0.0.1:15721/v1/messages"
        );
    }

    #[test]
    fn a_doubled_scheme_keeps_the_last_one() {
        assert_eq!(
            messages_url("http:https://llms.nexapp.dev"),
            "https://llms.nexapp.dev/v1/messages"
        );
        assert_eq!(
            messages_url("  \"https://llms.nexapp.dev/\"  "),
            "https://llms.nexapp.dev/v1/messages"
        );
    }

    #[test]
    fn an_empty_authority_falls_back_to_the_default() {
        assert_eq!(
            messages_url("http://"),
            format!("{DEFAULT_BASE_URL}/v1/messages")
        );
    }
}
