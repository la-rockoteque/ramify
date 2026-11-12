use crate::hooked::Hooked;
use crate::hooks::{workflow_fields, Outcome};
use crate::tools::Tools;
use serde_json::{json, Value};
use std::path::Path;

pub const DEFAULT_BASE_URL: &str = "http://127.0.0.1:15721";
pub const DEFAULT_MODEL: &str = "deepseek-v4.1-flash";
pub const DEFAULT_MAX_TOKENS: u32 = 8192;

const MAX_TURNS: usize = 32;

/// A `tool_use` input under this key holds arguments that did not parse as JSON.
pub const INVALID_ARGUMENTS: &str = "__invalid_arguments";

pub const SYSTEM_PROMPT: &str = "You are Ram Code, a coding agent working inside one git worktree. \
Use the tools to inspect and change files. To find code, use grep, glob and code_map first, \
then read only the files you need. Keep changes small and focused on the task. \
Every path is relative to the worktree root. \
run_command runs in a sandbox: it can read files, write only inside the worktree and $TMPDIR, \
and has no network, and keys and tokens are not in its environment. A command stops after \
timeout_seconds (default 120, at most 600); set a longer one for a slow build or test run. \
When a command must reach the network or write outside the worktree, set outside_sandbox \
and give a short reason: the human reads the command and runs or blocks it. Ask only when \
the sandbox cannot do the job. If the human blocks a command, do not ask for it again; \
find another way or say what you need. When the task is done, reply with a short summary \
of what changed and why.";

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
        let credential = credential_from_env(&get).ok_or_else(|| NO_CREDENTIAL.to_string())?;
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
        let max_tokens = max_tokens_from_env(&get);
        Ok(Self {
            base_url,
            credential,
            model,
            max_tokens,
        })
    }

}

/// Remove thinking blocks from earlier turns. Their signatures are valid only for the model
/// that wrote them, so another provider rejects them.
pub fn strip_thinking(history: &mut [Value]) {
    for message in history.iter_mut() {
        if let Some(blocks) = message.get_mut("content").and_then(Value::as_array_mut) {
            blocks.retain(|block| {
                !matches!(
                    block.get("type").and_then(Value::as_str),
                    Some("thinking" | "redacted_thinking")
                )
            });
        }
    }
}

/// `RAM_CODE_MAX_TOKENS`, else the default.
pub fn max_tokens_from_env(get: &dyn Fn(&str) -> Option<String>) -> u32 {
    get("RAM_CODE_MAX_TOKENS")
        .and_then(|value| value.trim().parse().ok())
        .unwrap_or(DEFAULT_MAX_TOKENS)
}

pub const NO_CREDENTIAL: &str = "No credential found. Export ANTHROPIC_API_KEY or ANTHROPIC_AUTH_TOKEN, \
or sign in with `ant auth login`, then start the task again.";

/// The first Anthropic credential found: `ANTHROPIC_API_KEY`, `ANTHROPIC_AUTH_TOKEN`, then the login.
pub fn credential_from_env(get: &dyn Fn(&str) -> Option<String>) -> Option<Credential> {
    let get = |key: &str| get(key).filter(|value| !value.trim().is_empty());
    if let Some(key) = get("ANTHROPIC_API_KEY") {
        Some(Credential::ApiKey(key.trim().to_string()))
    } else if let Some(token) = get("ANTHROPIC_AUTH_TOKEN") {
        Some(Credential::AuthToken(token.trim().to_string()))
    } else if has_login(&get) {
        Some(Credential::Login)
    } else {
        None
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
    /// A hook blocked, denied, failed or sent a message: one transcript line.
    Hook(String),
    /// A command waits on the human in `ramify run`: the run's id.
    RunPending(String),
}

/// Tools that come from outside Ram Code, such as MCP servers. Their calls go through the same
/// hooks as the built-in tools.
pub trait ToolSource {
    fn definitions(&self) -> Vec<Value>;
    fn handles(&self, name: &str) -> bool;
    fn call(&self, name: &str, input: &Value) -> Result<String, String>;
}

/// No outside tools.
pub struct NoTools;

impl ToolSource for NoTools {
    fn definitions(&self) -> Vec<Value> {
        Vec::new()
    }
    fn handles(&self, _name: &str) -> bool {
        false
    }
    fn call(&self, name: &str, _input: &Value) -> Result<String, String> {
        Err(format!("unknown tool: {name}"))
    }
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

/// POST a JSON body and return the JSON reply. A 4xx or 5xx reply is an error.
pub fn post_json(url: &str, headers: &[(&str, String)], request: &Value) -> Result<Value, String> {
    let body = serde_json::to_string(request)
        .map_err(|e| format!("could not encode the model request: {e}"))?;
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(std::time::Duration::from_secs(120)))
            .build(),
    );
    let mut post = agent.post(url);
    for (name, value) in headers {
        post = post.header(*name, value);
    }
    let mut response = post
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
    serde_json::from_str(&text).map_err(|e| format!("model API returned invalid JSON: {e}"))
}

impl Llm for AnthropicLlm {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        let mut headers = auth_headers(&self.config.credential)?;
        headers.push(("anthropic-version", "2023-06-01".to_string()));
        post_json(&messages_url(&self.config.base_url), &headers, request)
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
    run_turn_hooked(llm, root, history, task, &Hooked::none(root), on_event)
}

/// A Stop hook can send the model back to work this many times in a row, then the task ends.
const MAX_STOP_BLOCKS: usize = 3;

fn notes(outcome: &Outcome, on_event: &mut dyn FnMut(AgentEvent)) {
    for note in &outcome.notes {
        on_event(AgentEvent::Hook(note.clone()));
    }
}

/// `run_turn`, with the session's hooks around the prompt, each tool call and the stop.
pub fn run_turn_hooked(
    llm: &dyn Llm,
    root: &Path,
    history: &[Value],
    task: &str,
    hooks: &Hooked,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<(String, Vec<Value>), String> {
    run_turn_with(llm, root, history, task, hooks, &NoTools, on_event)
}

/// `run_turn_hooked`, with the tools of `extra` beside the built-in ones.
pub fn run_turn_with(
    llm: &dyn Llm,
    root: &Path,
    history: &[Value],
    task: &str,
    hooks: &Hooked,
    extra: &dyn ToolSource,
    on_event: &mut dyn FnMut(AgentEvent),
) -> Result<(String, Vec<Value>), String> {
    let tools = Tools::new(root);
    let mut definitions = Tools::definitions();
    definitions.extend(extra.definitions());
    let mut messages: Vec<Value> = history.to_vec();
    let instructions = crate::instructions::load(root, crate::instructions::home().as_deref());
    for note in &instructions.notes {
        on_event(AgentEvent::Hook(note.clone()));
    }
    let system = instructions.system_prompt(SYSTEM_PROMPT);

    let submitted = hooks.fire("UserPromptSubmit", "", json!({"prompt": task}));
    notes(&submitted, on_event);
    if let Some(reason) = submitted.block.or(submitted.stop) {
        on_event(AgentEvent::Hook(format!("the message was not sent: {reason}")));
        return Ok((String::new(), messages));
    }
    let mut context: Vec<String> = hooks.start_context.clone();
    context.extend(submitted.contexts);
    let content = if context.is_empty() {
        task.to_string()
    } else {
        format!("{}\n\n{task}", context.join("\n\n"))
    };
    messages.push(json!({"role": "user", "content": content}));
    let mut answer = String::new();
    let mut stop_blocks = 0;

    for _ in 0..MAX_TURNS {
        let request = json!({
            "model": llm.model(),
            "max_tokens": llm.max_tokens(),
            "system": system,
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
        let mut ended: Option<String> = None;

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
                    let result = if ended.is_some() {
                        "error: the task was stopped by a hook".to_string()
                    } else if let Some(raw) = input.get(INVALID_ARGUMENTS) {
                        format!("error: the tool arguments are not valid JSON: {raw}")
                    } else {
                        call_tool(&tools, extra, hooks, &name, &input, &mut ended, on_event)
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

        if let Some(reason) = ended {
            on_event(AgentEvent::Hook(format!("the task was stopped: {reason}")));
            messages.push(json!({"role": "assistant", "content": assistant_blocks}));
            if !tool_results.is_empty() {
                messages.push(json!({"role": "user", "content": tool_results}));
            }
            return Ok((answer.trim().to_string(), messages));
        }

        if tool_results.is_empty() {
            messages.push(json!({"role": "assistant", "content": content.clone()}));
            let stopped = hooks.fire("Stop", "", json!({"stop_hook_active": stop_blocks > 0}));
            notes(&stopped, on_event);
            match stopped.block {
                Some(reason) if stopped.stop.is_none() && stop_blocks < MAX_STOP_BLOCKS => {
                    stop_blocks += 1;
                    messages.push(json!({"role": "user", "content": reason}));
                    continue;
                }
                _ => return Ok((answer.trim().to_string(), messages)),
            }
        }

        messages.push(json!({"role": "assistant", "content": assistant_blocks}));
        messages.push(json!({"role": "user", "content": tool_results}));
        // The limit is on Stop blocks in a row: a turn with tool calls starts the count again.
        stop_blocks = 0;
    }

    Err(format!("the task did not finish within {MAX_TURNS} turns"))
}

/// One tool call between its PreToolUse/PreWorkflow and PostToolUse/PostWorkflow hooks.
fn call_tool(
    tools: &Tools,
    extra: &dyn ToolSource,
    hooks: &Hooked,
    name: &str,
    input: &Value,
    ended: &mut Option<String>,
    on_event: &mut dyn FnMut(AgentEvent),
) -> String {
    let actions = hooks.actions(name, input);
    let tool = json!(name);
    let mut before = hooks.fire(
        "PreToolUse",
        name,
        json!({"tool_name": name, "tool_input": input}),
    );
    for found in &actions {
        let action = found["action"].as_str().unwrap_or("");
        before.absorb(hooks.fire("PreWorkflow", action, workflow_fields(found, &tool, input, None)));
    }
    notes(&before, on_event);
    if let Some(reason) = before.deny {
        return format!("blocked by a hook: {reason}");
    }
    // `continue: false` before the tool: the tool does not run, and the task ends.
    if let Some(reason) = before.stop {
        *ended = Some(reason.clone());
        return format!("stopped by a hook: {reason}");
    }
    let called = if extra.handles(name) {
        extra.call(name, input)
    } else {
        tools.call_with(name, input, &mut |id| {
            on_event(AgentEvent::RunPending(id.to_string()))
        })
    };
    let mut result = match called {
        Ok(output) => output,
        Err(error) => format!("error: {error}"),
    };
    let response = json!(result);
    let mut after = hooks.fire(
        "PostToolUse",
        name,
        json!({"tool_name": name, "tool_input": input, "tool_response": response}),
    );
    for found in &actions {
        let action = found["action"].as_str().unwrap_or("");
        after.absorb(hooks.fire(
            "PostWorkflow",
            action,
            workflow_fields(found, &tool, input, Some(&response)),
        ));
    }
    notes(&after, on_event);
    for context in after.contexts {
        result.push_str(&format!("\n\n{context}"));
    }
    if let Some(reason) = after.block {
        result.push_str(&format!("\n\n{reason}"));
    }
    if let Some(reason) = after.stop {
        *ended = Some(reason);
    }
    result
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
