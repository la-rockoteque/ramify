//! Named model providers: four presets, plus custom ones from `providers.json`.

use crate::agent::{
    credential_from_env, normalize_base, AgentConfig, AnthropicLlm, Credential, Llm,
    ANTHROPIC_BASE_URL, ANTHROPIC_MODEL, DEFAULT_MAX_TOKENS, DEFAULT_MODEL, NO_CREDENTIAL,
};
use crate::openai::OpenAiLlm;
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub const PRESETS: [&str; 4] = ["claude", "openai", "deepseek", "env"];
pub const ENV: &str = "env";
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
    /// The environment variable that holds the key. `None` means the Anthropic credential order.
    pub key_env: Option<String>,
    pub model: Option<String>,
}

fn preset(name: &str) -> Option<Provider> {
    let make = |format, base: Option<&str>, key: Option<&str>, model: Option<&str>| Provider {
        name: name.to_string(),
        format,
        base_url: base.map(String::from),
        key_env: key.map(String::from),
        model: model.map(String::from),
    };
    match name {
        "claude" => Some(make(Format::Anthropic, Some(ANTHROPIC_BASE_URL), None, Some(ANTHROPIC_MODEL))),
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
    pub custom: Vec<Provider>,
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
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                return Ok(Self::empty(dir))
            }
            Err(error) => return Err(format!("could not read {}: {error}", path.display())),
        };
        let value: Value = serde_json::from_str(&text)
            .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?;
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

    /// The presets, each replaced by a custom entry of the same name, then the other custom ones.
    pub fn all(&self) -> Vec<Provider> {
        let mut list: Vec<Provider> = PRESETS
            .iter()
            .map(|name| self.custom_named(name).unwrap_or_else(|| preset(name).unwrap()))
            .collect();
        for provider in &self.custom {
            if !PRESETS.contains(&provider.name.as_str()) {
                list.push(provider.clone());
            }
        }
        list
    }

    pub fn find(&self, name: &str) -> Option<Provider> {
        self.custom_named(name).or_else(|| preset(name))
    }

    fn custom_named(&self, name: &str) -> Option<Provider> {
        self.custom.iter().find(|p| p.name == name).cloned()
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
        let mut value = match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str::<Value>(&text)
                .map_err(|e| format!("{} is not valid JSON: {e}", path.display()))?,
            Err(_) => json!({}),
        };
        if !value.is_object() {
            return Err(format!("{} must hold a JSON object", path.display()));
        }
        value["active"] = json!(name);
        std::fs::create_dir_all(&self.dir)
            .map_err(|e| format!("could not create {}: {e}", self.dir.display()))?;
        let text = serde_json::to_string_pretty(&value).unwrap_or_default();
        std::fs::write(&path, text + "\n")
            .map_err(|e| format!("could not write {}: {e}", path.display()))?;
        self.active = Some(name.to_string());
        Ok(())
    }
}

fn parse_entry(entry: &Value) -> Result<Provider, String> {
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
        None | Some("anthropic") => Format::Anthropic,
        Some("openai") => Format::OpenAi,
        Some(other) => {
            return Err(format!(
                "provider {name} has format {other}; use anthropic or openai"
            ))
        }
    };
    Ok(Provider {
        name,
        format,
        base_url: field("base_url"),
        key_env: field("key_env"),
        model: field("model"),
    })
}

/// What a provider lacks to run: the key it needs, or `None` when it has one.
pub fn missing_key(provider: &Provider, get: &dyn Fn(&str) -> Option<String>) -> Option<String> {
    match &provider.key_env {
        Some(var) => get(var)
            .filter(|v| !v.trim().is_empty())
            .is_none()
            .then(|| var.clone()),
        None => credential_from_env(get)
            .is_none()
            .then(|| ANTHROPIC_KEYS.to_string()),
    }
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
    let is_env = provider.base_url.is_none() && provider.key_env.is_none();
    let mut config = if is_env {
        AgentConfig::from_env(get)?
    } else {
        let credential = match &provider.key_env {
            Some(var) => Credential::ApiKey(
                get(var)
                    .map(|key| key.trim().to_string())
                    .ok_or_else(|| {
                        format!(
                            "Provider {} needs {var}. Export it, then start the task again.",
                            provider.name
                        )
                    })?,
            ),
            None => credential_from_env(&get).ok_or_else(|| NO_CREDENTIAL.to_string())?,
        };
        let base_url = provider
            .base_url
            .as_deref()
            .ok_or_else(|| format!("Provider {} has no base_url.", provider.name))?;
        let model = get("RAM_CODE_MODEL")
            .or_else(|| provider.model.clone())
            .ok_or_else(|| {
                format!(
                    "Provider {} has no default model. Run /model <id> or export RAM_CODE_MODEL.",
                    provider.name
                )
            })?;
        AgentConfig {
            base_url: normalize_base(base_url),
            credential,
            model,
            max_tokens: get("RAM_CODE_MAX_TOKENS")
                .and_then(|v| v.parse().ok())
                .unwrap_or(DEFAULT_MAX_TOKENS),
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
}
