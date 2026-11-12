//! Named model providers: four presets, plus custom ones from `providers.json`.

use crate::agent::{
    credential_from_env, idle_timeout_from_env, max_tokens_from_env, normalize_base,
    stream_from_env, AgentConfig, AnthropicLlm,
    Credential, Llm, ANTHROPIC_BASE_URL, ANTHROPIC_MODEL, DEFAULT_MODEL, NO_CREDENTIAL,
};
use crate::openai::OpenAiLlm;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const PRESETS: [&str; 4] = ["claude", "openai", "deepseek", "env"];
pub const ENV: &str = "env";
const CLAUDE: &str = "claude";
const FILE: &str = "providers.json";
const ANTHROPIC_KEYS: &str = "ANTHROPIC_API_KEY, ANTHROPIC_AUTH_TOKEN or `ant auth login`";

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Format {
    Anthropic,
    OpenAi,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Provider {
    pub name: String,
    pub format: Format,
    pub base_url: Option<String>,
    /// The environment variable that holds the key. Only `claude` and `env` may leave it out:
    /// they use the Anthropic credential order.
    pub key_env: Option<String>,
    pub model: Option<String>,
    /// The default `max_tokens`. `RAM_CODE_MAX_TOKENS` overrides it.
    pub max_tokens: Option<u32>,
    /// `Some(false)` turns streaming off for a server that does not stream.
    pub stream: Option<bool>,
}

/// One entry of `providers.json`. A field it leaves out comes from the preset of the same name.
#[derive(Debug, Clone, PartialEq)]
struct Entry {
    name: String,
    format: Option<Format>,
    base_url: Option<String>,
    key_env: Option<String>,
    model: Option<String>,
    max_tokens: Option<u32>,
    stream: Option<bool>,
}

/// `claude` streams, so a long reply hits no request timeout. Other providers keep the
/// smaller default, which every compatible server accepts.
const CLAUDE_MAX_TOKENS: u32 = 64_000;

fn preset(name: &str) -> Option<Provider> {
    let make = |format, base: Option<&str>, key: Option<&str>, model: Option<&str>| Provider {
        name: name.to_string(),
        format,
        base_url: base.map(String::from),
        key_env: key.map(String::from),
        model: model.map(String::from),
        max_tokens: (name == CLAUDE).then_some(CLAUDE_MAX_TOKENS),
        stream: None,
    };
    match name {
        CLAUDE => Some(make(Format::Anthropic, Some(ANTHROPIC_BASE_URL), None, Some(ANTHROPIC_MODEL))),
        "openai" => Some(make(Format::OpenAi, Some("https://api.openai.com/v1"), Some("OPENAI_API_KEY"), None)),
        "deepseek" => Some(make(
            Format::Anthropic,
            Some("https://api.deepseek.com/anthropic"),
            Some("DEEPSEEK_API_KEY"),
            Some(DEFAULT_MODEL),
        )),
        // Today's environment setup; `AgentConfig::from_env` resolves it.
        ENV => Some(make(Format::Anthropic, None, None, None)),
        _ => None,
    }
}

/// The saved choice and the custom providers, read from `<dir>/providers.json`.
#[derive(Debug, Clone)]
pub struct Providers {
    pub dir: PathBuf,
    pub active: Option<String>,
    custom: Vec<Entry>,
}

impl Providers {
    /// `RAM_CODE_CONFIG_DIR`, else `~/.config/ram-code`.
    pub fn default_dir(get: &dyn Fn(&str) -> Option<String>) -> PathBuf {
        match get("RAM_CODE_CONFIG_DIR").filter(|dir| !dir.trim().is_empty()) {
            Some(dir) => PathBuf::from(dir),
            None => Path::new(&get("HOME").unwrap_or_default())
                .join(".config")
                .join("ram-code"),
        }
    }

    pub fn empty(dir: PathBuf) -> Self {
        Self {
            dir,
            active: None,
            custom: Vec::new(),
        }
    }

    pub fn path(&self) -> PathBuf {
        self.dir.join(FILE)
    }

    /// A missing file is the empty state. An unreadable file is an error.
    pub fn load(dir: PathBuf) -> Result<Self, String> {
        let path = dir.join(FILE);
        let Some(value) = read_file(&path)? else {
            return Ok(Self::empty(dir));
        };
        let active = value.get("active").and_then(Value::as_str).map(String::from);
        let mut custom = Vec::new();
        for entry in value
            .get("providers")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            custom.push(parse_entry(entry).map_err(|e| format!("{}: {e}", path.display()))?);
        }
        Ok(Self { dir, active, custom })
    }

    /// The presets, each merged with a custom entry of the same name, then the other custom ones.
    pub fn all(&self) -> Vec<Provider> {
        let mut list: Vec<Provider> = PRESETS.iter().filter_map(|name| self.find(name)).collect();
        for entry in &self.custom {
            if !PRESETS.contains(&entry.name.as_str()) {
                list.extend(self.find(&entry.name));
            }
        }
        list
    }

    pub fn find(&self, name: &str) -> Option<Provider> {
        let entry = self.custom.iter().find(|e| e.name == name);
        match (preset(name), entry) {
            (Some(base), Some(entry)) => Some(Provider {
                name: base.name,
                format: entry.format.unwrap_or(base.format),
                base_url: entry.base_url.clone().or(base.base_url),
                key_env: entry.key_env.clone().or(base.key_env),
                model: entry.model.clone().or(base.model),
                max_tokens: entry.max_tokens.or(base.max_tokens),
                stream: entry.stream.or(base.stream),
            }),
            (Some(base), None) => Some(base),
            (None, Some(entry)) => Some(Provider {
                name: entry.name.clone(),
                format: entry.format.unwrap_or(Format::Anthropic),
                base_url: entry.base_url.clone(),
                key_env: entry.key_env.clone(),
                model: entry.model.clone(),
                max_tokens: entry.max_tokens,
                stream: entry.stream,
            }),
            (None, None) => None,
        }
    }

    pub fn names(&self) -> Vec<String> {
        self.all().into_iter().map(|p| p.name).collect()
    }

    /// `RAM_CODE_PROVIDER`, else the saved choice, else `env`.
    pub fn active_name(&self, get: &dyn Fn(&str) -> Option<String>) -> String {
        get("RAM_CODE_PROVIDER")
            .filter(|name| !name.trim().is_empty())
            .map(|name| name.trim().to_string())
            .or_else(|| self.active.clone())
            .unwrap_or_else(|| ENV.to_string())
    }

    /// Save `name` as the active provider. Other fields in the file are kept as they are.
    pub fn save_active(&mut self, name: &str) -> Result<(), String> {
        let path = self.path();
        let mut value = read_file(&path)?.unwrap_or_else(|| json!({}));
        if !value.is_object() {
            return Err(format!("{} must hold a JSON object", path.display()));
        }
        value["active"] = json!(name);
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("could not create {}: {e}", self.dir.display()))?;
        // Write a sibling file, then rename it, so a failed write never truncates the file.
        let partial = self.dir.join(format!("{FILE}.partial"));
        let text = serde_json::to_string_pretty(&value).unwrap_or_default() + "\n";
        std::fs::write(&partial, text)
            .and_then(|_| std::fs::rename(&partial, &path))
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        self.active = Some(name.to_string());
        Ok(())
    }
}

/// `None` when the file does not exist.
fn read_file(path: &Path) -> Result<Option<Value>, String> {
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    serde_json::from_str(&text)
        .map(Some)
        .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))
}

fn parse_entry(entry: &Value) -> Result<Entry, String> {
    let field = |key: &str| {
        entry
            .get(key)
            .and_then(Value::as_str)
            .map(str::trim)
            .filter(|v| !v.is_empty())
            .map(String::from)
    };
    let name = field("name").ok_or("a provider has no name")?;
    let format = match field("format").as_deref() {
        None => None,
        Some("anthropic") => Some(Format::Anthropic),
        Some("openai") => Some(Format::OpenAi),
        Some(other) => {
            return Err(format!(
                "provider {name} has format {other}; use anthropic or openai"
            ))
        }
    };
    Ok(Entry {
        name,
        format,
        base_url: field("base_url"),
        key_env: field("key_env"),
        model: field("model"),
        max_tokens: entry
            .get("max_tokens")
            .and_then(Value::as_u64)
            .and_then(|tokens| u32::try_from(tokens).ok()),
        stream: entry.get("stream").and_then(Value::as_bool),
    })
}

/// True when the provider sends the Anthropic credential order: `env`, or `claude` at Anthropic.
/// Any other host must name its own key, so an Anthropic key or login never leaves for it.
fn uses_anthropic_credential(provider: &Provider) -> bool {
    provider.key_env.is_none()
        && (provider.name == ENV
            || (provider.name == CLAUDE
                && provider.base_url.as_deref() == Some(ANTHROPIC_BASE_URL)))
}

/// What a provider lacks to run: the key it needs, or `None` when it has one.
pub fn missing_key(provider: &Provider, get: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    match &provider.key_env {
        Some(var) => get(var)
            .filter(|v| !v.trim().is_empty())
            .is_none()
            .then(|| var.clone()),
        None if uses_anthropic_credential(provider) => credential_from_env(get)
            .is_none()
            .then(|| ANTHROPIC_KEYS.to_string()),
        None => Some("key_env in providers.json".to_string()),
    }
}

/// A key may travel only over https, or over http to this machine.
fn check_base_url(name: &str, raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    let Some((scheme, rest)) = trimmed.split_once("://") else {
        return Err(format!(
            "Provider {name} has base_url {raw}; write it in full, such as https://host"
        ));
    };
    let authority = rest.split('/').next().unwrap_or("");
    let host = match authority.rsplit_once(':') {
        Some((host, port)) if port.chars().all(|c| c.is_ascii_digit()) => host,
        _ => authority,
    };
    if host.is_empty() {
        return Err(format!(
            "Provider {name} has base_url {raw}; write it in full, such as https://host"
        ));
    }
    let loopback = matches!(host, "127.0.0.1" | "localhost" | "[::1]");
    let scheme = scheme.to_ascii_lowercase();
    if scheme != "https" && !(scheme == "http" && loopback) {
        return Err(format!(
            "Provider {name} has base_url {raw}; use https, or http only to localhost"
        ));
    }
    Ok(normalize_base(trimmed))
}

/// A provider with its credential, URL and model, ready for a request.
#[derive(Debug, Clone)]
pub struct Resolved {
    pub name: String,
    pub format: Format,
    pub config: AgentConfig,
}

impl Resolved {
    pub fn llm(self) -> Box<dyn Llm> {
        match self.format {
            Format::Anthropic => Box::new(AnthropicLlm::new(self.config)),
            Format::OpenAi => Box::new(OpenAiLlm::new(self.config)),
        }
    }
}

/// Resolve `provider` from the environment. `session_model` is the `/model` choice.
pub fn resolve(
    provider: &Provider,
    get: &dyn Fn(&str) -> Option<String>,
    session_model: Option<&str>,
) -> Result<Resolved, String> {
    let get = |key: &str| get(key).filter(|v| !v.trim().is_empty());
    let name = &provider.name;
    let is_env = provider.name == ENV && provider.base_url.is_none() && provider.key_env.is_none();
    let mut config = if is_env {
        let mut config = AgentConfig::from_env(get)?;
        if let (Some(model), None) = (&provider.model, get("RAM_CODE_MODEL")) {
            config.model = model.clone();
        }
        if let (Some(tokens), None) = (provider.max_tokens, get("RAM_CODE_MAX_TOKENS")) {
            config.max_tokens = tokens;
        }
        config.stream &= provider.stream.unwrap_or(true);
        config
    } else {
        let credential = match &provider.key_env {
            Some(var) => Credential::ApiKey(
                get(var)
                    .map(|key| key.trim().to_string())
                    .ok_or_else(|| {
                        format!("Provider {name} needs {var}. Export it, then start the task again.")
                    })?,
            ),
            None if uses_anthropic_credential(provider) => {
                credential_from_env(&get).ok_or_else(|| NO_CREDENTIAL.to_string())?
            }
            None => {
                return Err(format!(
                    "Provider {name} has no key_env. Name the variable that holds its key in providers.json."
                ))
            }
        };
        let raw = provider
            .base_url
            .as_deref()
            .ok_or_else(|| format!("Provider {name} has no base_url."))?;
        let base_url = check_base_url(name, raw)?;
        let model = get("RAM_CODE_MODEL")
            .or_else(|| provider.model.clone())
            .ok_or_else(|| {
                format!(
                    "Provider {name} has no default model. Run /model <id> or export RAM_CODE_MODEL."
                )
            })?;
        AgentConfig {
            base_url,
            credential,
            model,
            max_tokens: match (get("RAM_CODE_MAX_TOKENS"), provider.max_tokens) {
                (None, Some(tokens)) => tokens,
                _ => max_tokens_from_env(&get),
            },
            stream: stream_from_env(&get) && provider.stream.unwrap_or(true),
            idle_timeout: idle_timeout_from_env(&get),
        }
    };
    if let Some(model) = session_model {
        config.model = model.to_string();
    }
    Ok(Resolved {
        name: provider.name.clone(),
        format: provider.format,
        config,
    })
}

/// The worker's and the headless run's provider: the saved choice, overridden by the environment.
pub fn resolve_from_env(get: &dyn Fn(&str) -> Option<String>) -> Result<Resolved, String> {
    let dir = Providers::default_dir(get);
    // An unreadable file leaves the presets, as in the app. A custom provider then fails with the file's error.
    let (providers, load_error) = match Providers::load(dir.clone()) {
        Ok(providers) => (providers, None),
        Err(error) => (Providers::empty(dir), Some(error)),
    };
    let name = providers.active_name(get);
    let provider = providers
        .find(&name)
        .ok_or_else(|| load_error.unwrap_or_else(|| unknown(&name, &providers)))?;
    resolve(&provider, get, None)
}

pub fn unknown(name: &str, providers: &Providers) -> String {
    format!(
        "Unknown provider {name}. Choose one of: {}.",
        providers.names().join(", ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worker_uses_presets_when_the_file_is_unreadable() {
        let dir = std::env::temp_dir().join(format!("ram-code-worker-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join(FILE), "{not json").unwrap();
        let dir_text = dir.to_str().unwrap().to_string();
        let get = |provider: &'static str| {
            let dir_text = dir_text.clone();
            move |key: &str| match key {
                "RAM_CODE_CONFIG_DIR" => Some(dir_text.clone()),
                "RAM_CODE_PROVIDER" => Some(provider.to_string()),
                "DEEPSEEK_API_KEY" => Some("ds".to_string()),
                _ => None,
            }
        };
        let resolved = resolve_from_env(&get("deepseek")).expect("a preset still resolves");
        assert_eq!(resolved.name, "deepseek");
        let error = resolve_from_env(&get("litellm")).expect_err("a custom provider needs the file");
        let _ = std::fs::remove_dir_all(&dir);
        assert!(error.contains("providers.json"), "{error}");
    }

    #[test]
    fn a_base_url_must_be_https_or_loopback() {
        assert!(check_base_url("p", "https://llm.example.com").is_ok());
        assert!(check_base_url("p", "http://127.0.0.1:4000").is_ok());
        assert!(check_base_url("p", "http://localhost:4000/v1").is_ok());
        assert!(check_base_url("p", "http://llm.example.com").is_err());
        assert!(check_base_url("p", "llm.example.com").is_err());
        assert!(check_base_url("p", "https://").is_err());
        assert!(check_base_url("p", "http://localhost.evil.com").is_err());
    }
}
