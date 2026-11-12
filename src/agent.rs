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

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
pub const ANTHROPIC_MODEL: &str = "claude-opus-5-5";
const OAUTH_BETA: &str = "oauth-2025-04-20";

/// How Ram Code authenticates to the model API.
#[derive(Debug, Clone, PartialEq)]
pub enum Credential {
    /// `ANTHROPIC_API_KEY`, sent as `x-api-key`.
    ApiKey(String),
    /// `ANTHROPIC_AUTH_TOKEN`, sent as a bearer token and as `x-api-key` for proxies.
    AuthToken(String),
    /// An `ant auth login` profile. Each request asks `ant` for a fresh access token.
    Login,
}

impl Credential {
    pub fn label(&self) -> &'static str {
        match self {
            Credential::ApiKey(_) => "API key",
            Credential::AuthToken(_) => "auth token",
            Credential::Login => "Anthropic login",
        }
    }
}

#[derive(Debug, Clone)]
pub struct AgentConfig {
    pub base_url: String,
    pub credential: Credential,
    pub model: String,
    pub max_tokens: u32,
}

impl AgentConfig {
    pub fn from_env(f: impl Fn(&str) -> Option<String>) -> Result<Self, String> {
        let get = |key: &str| f(key).filter(|value| !value.trim().is_empty());
        let credential = if let Some(key) = get("ANTHROPIC_API_KEY") {
            Credential::ApiKey(key.trim().to_string())
        } else if let Some(token) = get("ANTHROPIC_AUTH_TOKEN") {
            Credential::AuthToken(token.trim().to_string())
        } else if has_login(&get) {
            Credential::Login
        } else {
            return Err("No credential found. Export ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN, \
or sign in with `ant auth login`, then start the task again."
                .to_string());
        };
        // A token usually points at a proxy; a key or a login points at Anthropic.
        let (default_base, default_model) = match credential {
            Credential::AuthToken(_) => (DEFAULT_BASE_URL, DEFAULT_MODEL),
            _ => (ANTHROPIC_BASE_URL, ANTHROPIC_MODEL),
        };
        let base_url =
            normalize_base(&get("ANTHROPIC_BASE_URL").unwrap_or_else(|| default_base.to_string()));
        let model = get("RAM_CODE_MODEL")
            .or_else(|| get("ANTHROPIC_DEFAULT_SONNET_MODEL"))
            .or_else(|| get("ANTHROPIC_DEFAULT_SONNET_MODEL_NAME"))
            .unwrap_or_else(|| default_model.to_string());
        let max_tokens = get("RAM_CODE_MAX_TOKENS")
            .and_then(|value| value.parse().ok())
            .unwrap_or(DEFAULT_MAX_TOKENS);
        Ok(Self {
            base_url,
            credential,
            model,
            max_tokens,
        })
    }

    pub fn from_process_env() -> Result<Self, String> {
        Self::from_env(|key| std::env::var(key).ok())
    }
}

/// True when `ANTHROPIC_PROFILE` is set or `ant auth login` left a credentials file.
fn has_login(get: &dyn Fn(&str) -> Option<String>) -> bool {
    if get("ANTHROPIC_PROFILE").is_some() {
        return true;
    }
    let config_dir = match get("ANTHROPIC_CONFIG_DIR") {
        Some(dir) => std::path::PathBuf::from(dir),
        None => match get("HOME") {
            Some(home) => Path::new(&home).join(".config").join("anthropic"),
            None => return false,
        },
    };
    std::fs::read_dir(config_dir.join("credentials"))
        .map(|entries| {
            entries.flatten().any(|entry| {
                entry.path().extension().and_then(|ext| ext.to_str()) == Some("json")
            })
        })
        .unwrap_or(false)
}

/// Ask `ant` for the access token of the active profile. `ant` refreshes it when needed.
fn login_token() -> Result<String, String> {
    let output = std::process::Command::new("ant")
        .args(["auth", "print-credentials", "--access-token"])
        .output()
        .map_err(|e| {
            format!("could not run `ant` for the Anthropic login: {e}. Install the ant CLI, or export ANTHROPIC_API_KEY.")
        })?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        return Err(format!(
            "`ant auth print-credentials` failed: {}. Run `ant auth login` again.",
            stderr.trim()
        ));
    }
    let token = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if token.is_empty() {
        return Err("`ant auth print-credentials` printed no token. Run `ant auth login` again.".into());
    }
    Ok(token)
}

/// The authentication headers for one request.
pub fn auth_headers(credential: &Credential) -> Result<Vec<(&'static str, String)>, String> {
    Ok(match credential {
        Credential::ApiKey(key) => vec![("x-api-key", key.clone())],
        Credential::AuthToken(token) => vec![
            ("authorization", format!("Bearer {token}")),
            ("x-api-key", token.clone()),
        ],
        Credential::Login => vec![
            ("authorization", format!("Bearer {}", login_token()?)),
            ("anthropic-beta", OAUTH_BETA.to_string()),
        ],
    })
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
        let mut post = agent.post(&url);
        for (name, value) in auth_headers(&self.config.credential)? {
            post = post.header(name, value);
        }
        let mut response = post
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

    fn env(pairs: &[(&str, &str)]) -> Result<AgentConfig, String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        AgentConfig::from_env(move |key| {
            pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
        })
    }

    #[test]
    fn an_api_key_goes_to_anthropic_as_x_api_key() {
        let config = env(&[("ANTHROPIC_API_KEY", "sk-ant-key")]).expect("config");
        assert_eq!(config.credential, Credential::ApiKey("sk-ant-key".into()));
        assert_eq!(config.base_url, ANTHROPIC_BASE_URL);
        assert_eq!(config.model, ANTHROPIC_MODEL);
        assert_eq!(
            auth_headers(&config.credential).unwrap(),
            vec![("x-api-key", "sk-ant-key".to_string())]
        );
    }

    #[test]
    fn an_api_key_wins_over_a_token() {
        let config = env(&[
            ("ANTHROPIC_API_KEY", "sk-ant-key"),
            ("ANTHROPIC_AUTH_TOKEN", "proxy-token"),
        ])
        .expect("config");
        assert_eq!(config.credential, Credential::ApiKey("sk-ant-key".into()));
    }

    #[test]
    fn a_token_keeps_the_proxy_defaults() {
        let config = env(&[("ANTHROPIC_AUTH_TOKEN", "proxy-token")]).expect("config");
        assert_eq!(config.credential, Credential::AuthToken("proxy-token".into()));
        assert_eq!(config.base_url, DEFAULT_BASE_URL);
        assert_eq!(config.model, DEFAULT_MODEL);
        assert_eq!(
            auth_headers(&config.credential).unwrap(),
            vec![
                ("authorization", "Bearer proxy-token".to_string()),
                ("x-api-key", "proxy-token".to_string()),
            ]
        );
    }

    #[test]
    fn a_login_credentials_file_selects_the_login() {
        let dir = std::env::temp_dir().join(format!("ram-code-login-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("credentials")).unwrap();
        std::fs::write(dir.join("credentials").join("default.json"), "{}").unwrap();
        let config = env(&[("ANTHROPIC_CONFIG_DIR", dir.to_str().unwrap())]).expect("config");
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(config.credential, Credential::Login);
        assert_eq!(config.base_url, ANTHROPIC_BASE_URL);
        assert_eq!(config.model, ANTHROPIC_MODEL);
    }

    #[test]
    fn a_profile_selects_the_login() {
        let config = env(&[("ANTHROPIC_PROFILE", "work")]).expect("config");
        assert_eq!(config.credential, Credential::Login);
    }

    #[test]
    fn no_credential_names_every_option() {
        let error = env(&[]).expect_err("no credential");
        for option in ["ANTHROPIC_API_KEY", "ANTHROPIC_AUTH_TOKEN", "ant auth login"] {
            assert!(error.contains(option), "{option} missing from: {error}");
        }
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
