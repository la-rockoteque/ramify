//! MCP servers over stdio: their configuration, the human's approval, and the servers one task
//! runs. Remote (`http`) servers are listed but not started yet.

use crate::agent::ToolSource;
use crate::providers::Providers;
use crate::sandbox::{checked_temp, load_extras, scrub, Paths, Sandbox};
use crate::tools::Tools;
use rmcp::model::{CallToolRequestParams, ClientCapabilities, Implementation, InitializeRequestParams};
use rmcp::service::RunningService;
use rmcp::{RoleClient, ServiceExt};
use serde_json::{json, Map, Value};
use sha2::{Digest, Sha256};
use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::Duration;

const USER_FILE: &str = "mcp.json";
const PROJECT_FILE: &str = ".mcp.json";
const APPROVALS_FILE: &str = "mcp-approvals.json";
/// A server that is not ready after this long is marked failed for the task.
const START_TIMEOUT: Duration = Duration::from_secs(30);
const DEFAULT_CALL_TIMEOUT: u64 = 120;
const MAX_CALL_TIMEOUT: u64 = 600;
const MAX_RESULT: usize = 40_000;
/// The longest tool name both provider formats accept.
const MAX_NAME: usize = 64;
/// The most tools one server may offer, and the longest description each may have. Every
/// request carries them, so a server cannot inflate each turn.
const MAX_TOOLS: usize = 128;
const MAX_DESCRIPTION: usize = 2_000;
/// The longest single message a server may send. A longer line ends its transport.
const MAX_MESSAGE: usize = 8 * 1024 * 1024;
/// How long a server gets to exit after its input closes, before its process group is killed.
const EXIT_GRACE: Duration = Duration::from_secs(2);

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    User,
    Project,
}

/// The human's decision for a server.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Choice {
    /// `a`: inside the sandbox, with no network.
    Sandbox,
    /// `n`: inside the sandbox, with network.
    Network,
    /// `u`: outside the sandbox.
    Outside,
    /// `d`: the server never starts.
    Deny,
}

impl Choice {
    pub fn from_key(key: char) -> Option<Choice> {
        match key {
            'a' => Some(Choice::Sandbox),
            'n' => Some(Choice::Network),
            'u' => Some(Choice::Outside),
            'd' => Some(Choice::Deny),
            _ => None,
        }
    }

    pub fn key(self) -> &'static str {
        match self {
            Choice::Sandbox => "a",
            Choice::Network => "n",
            Choice::Outside => "u",
            Choice::Deny => "d",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Choice::Sandbox => "sandbox, no network",
            Choice::Network => "sandbox with network",
            Choice::Outside => "outside the sandbox",
            Choice::Deny => "denied",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Transport {
    Stdio {
        command: String,
        args: Vec<String>,
        env: Vec<(String, String)>,
        timeout: u64,
    },
    Http {
        url: String,
        headers: Vec<(String, String)>,
    },
    /// The entry could not be read: `ServerEntry::error` says why.
    Invalid,
}

#[derive(Debug, Clone)]
pub struct ServerEntry {
    pub name: String,
    pub source: Source,
    pub transport: Transport,
    /// SHA-256 of the entry as written, before `${VAR}` expansion. An approval holds this hash,
    /// so a changed entry asks again, and no expanded secret reaches the approvals file.
    pub hash: String,
    pub error: Option<String>,
    /// `env` as written in the file, before expansion, for the approval prompt.
    pub written_env: Vec<(String, String)>,
    /// The program `command` runs, found on `PATH` the way the server's shell finds it.
    pub resolved: Option<PathBuf>,
    /// Files in the repository that the server runs, relative to the top level. A project
    /// approval holds their content, so a changed file asks again.
    pub repo_files: Vec<String>,
}

impl ServerEntry {
    /// The command line, for the approval prompt and `/mcp`. Each word is escaped, and quoted
    /// when it holds more than plain characters, so one word cannot pass for several.
    pub fn command_line(&self) -> String {
        match &self.transport {
            Transport::Stdio { command, args, .. } => {
                let mut parts = vec![shown_word(command)];
                parts.extend(args.iter().map(|arg| shown_word(arg)));
                parts.join(" ")
            }
            Transport::Http { url, .. } => visible_line(url),
            Transport::Invalid => String::new(),
        }
    }
}

#[derive(Debug, Clone, Default)]
pub struct Config {
    pub servers: Vec<ServerEntry>,
    /// Files that could not be read.
    pub errors: Vec<String>,
}

/// The user file, then the project file. On a name clash the user file wins, so a project
/// cannot replace a server the user trusts.
pub fn load(root: &Path, get: &dyn Fn(&str) -> Option<String>) -> Config {
    let mut config = Config::default();
    let user = Providers::default_dir(get).join(USER_FILE);
    let project = crate::instructions::top_level(root).join(PROJECT_FILE);
    for (path, source) in [(user, Source::User), (project, Source::Project)] {
        let text = match std::fs::read_to_string(&path) {
            Ok(text) => text,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                config.errors.push(format!("could not read {}: {error}", path.display()));
                continue;
            }
        };
        let value: Value = match serde_json::from_str(&text) {
            Ok(value) => value,
            Err(error) => {
                config
                    .errors
                    .push(format!("{} is not valid JSON: {error}", path.display()));
                continue;
            }
        };
        let Some(servers) = value.get("mcpServers").and_then(Value::as_object) else {
            continue;
        };
        for (name, raw) in servers {
            if config.servers.iter().any(|server| &server.name == name) {
                continue;
            }
            config.servers.push(parse_entry(name, raw, source, root, get));
        }
    }
    config
}

fn parse_entry(
    name: &str,
    raw: &Value,
    source: Source,
    root: &Path,
    get: &dyn Fn(&str) -> Option<String>,
) -> ServerEntry {
    let written_env = raw
        .get("env")
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .map(|(k, v)| (k.clone(), v.as_str().unwrap_or_default().to_string()))
                .collect()
        })
        .unwrap_or_default();
    let mut entry = ServerEntry {
        name: name.to_string(),
        source,
        transport: Transport::Invalid,
        hash: hex(&Sha256::digest(raw.to_string().as_bytes())),
        error: None,
        written_env,
        resolved: None,
        repo_files: Vec::new(),
    };
    // A project file comes with the repository, so it may not pull a secret out of the user's
    // environment into a server, its arguments or the prompt. The user's own file may.
    let key_envs = project_key_envs(get);
    let refuse = |name: &str, value: &str| {
        source == Source::Project && crate::sandbox::is_secret(name, value, &key_envs)
    };
    let transport = match read_transport(raw, get, &refuse) {
        Ok(transport) => transport,
        Err(error) => {
            entry.error = Some(error);
            return entry;
        }
    };
    if let Transport::Stdio { command, args, env, .. } = &transport {
        // These change what an approved command runs without changing the command a human
        // reads, so a repository may not set them.
        if source == Source::Project {
            if let Some((key, _)) = env.iter().find(|(key, _)| risky_env(key)) {
                entry.error = Some(format!(
                    "a project server may not set {key}, which changes what the command runs; \
                     move the server to ~/.config/ram-code/mcp.json"
                ));
                entry.transport = transport;
                return entry;
            }
        }
        let cwd = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let top = crate::instructions::top_level(root);
        let top = top.canonicalize().unwrap_or(top);
        let path = env
            .iter()
            .find(|(key, _)| key == "PATH")
            .map(|(_, value)| value.clone())
            .or_else(|| get("PATH"))
            .unwrap_or_default();
        entry.resolved = resolve_command(command, &path, &cwd);
        let mut files = Vec::new();
        let mut candidates: Vec<PathBuf> = entry.resolved.iter().cloned().collect();
        candidates.extend(args.iter().map(|arg| cwd.join(arg)));
        candidates.extend(npx_bin(command, args, &cwd));
        for candidate in candidates {
            let Ok(real) = candidate.canonicalize() else {
                continue;
            };
            let Ok(relative) = real.strip_prefix(&top) else {
                continue;
            };
            if !real.is_file() {
                continue;
            }
            let content = std::fs::read(&real).unwrap_or_default();
            let relative = relative.display().to_string();
            if !files.iter().any(|(name, _)| name == &relative) {
                files.push((relative, hex(&Sha256::digest(&content))));
            }
        }
        entry.repo_files = files.iter().map(|(name, _)| name.clone()).collect();
        // A project approval holds what the entry expands to, the program it resolves to,
        // and the repository files it runs, so a change to any of them asks again.
        if source == Source::Project {
            let bound = format!(
                "{}\0{:?}\0{:?}\0{:?}",
                raw,
                expanded_of(&transport),
                entry.resolved,
                files
            );
            entry.hash = hex(&Sha256::digest(bound.as_bytes()));
        }
    } else if source == Source::Project {
        let expanded = format!("{}\0{:?}", raw, expanded_of(&transport));
        entry.hash = hex(&Sha256::digest(expanded.as_bytes()));
    }
    entry.transport = transport;
    entry
}

/// Environment variables that load code or change which program runs.
fn risky_env(key: &str) -> bool {
    let upper = key.to_ascii_uppercase();
    matches!(
        upper.as_str(),
        "NODE_OPTIONS"
            | "PYTHONSTARTUP"
            | "PYTHONHOME"
            | "RUBYOPT"
            | "RUBYLIB"
            | "PERL5OPT"
            | "PERL5LIB"
            | "BASH_ENV"
            | "ENV"
            | "JAVA_TOOL_OPTIONS"
            | "_JAVA_OPTIONS"
            | "JDK_JAVA_OPTIONS"
    ) || upper.ends_with("PATH")
        || upper.starts_with("DYLD_")
        || upper.starts_with("LD_")
        || upper.starts_with("NPM_CONFIG_")
}

/// The program `sh` runs for `command`: a path as written, else the first match on `path`.
/// A relative entry on `path` counts from `cwd`, as it does for the server.
fn resolve_command(command: &str, path: &str, cwd: &Path) -> Option<PathBuf> {
    use std::os::unix::fs::PermissionsExt;
    let runnable = |candidate: &Path| {
        std::fs::metadata(candidate)
            .map(|meta| meta.is_file() && meta.permissions().mode() & 0o111 != 0)
            .unwrap_or(false)
    };
    if command.contains('/') {
        let candidate = cwd.join(command);
        return runnable(&candidate).then(|| candidate.canonicalize().unwrap_or(candidate));
    }
    path.split(':').find_map(|dir| {
        let dir = if dir.is_empty() { Path::new(".") } else { Path::new(dir) };
        let candidate = cwd.join(dir).join(command);
        runnable(&candidate).then(|| candidate.canonicalize().unwrap_or(candidate))
    })
}

/// `npx <package>` runs `node_modules/.bin/<name>` when the repository has one.
fn npx_bin(command: &str, args: &[String], cwd: &Path) -> Option<PathBuf> {
    let program = Path::new(command).file_name()?.to_str()?;
    if program != "npx" {
        return None;
    }
    let package = args.iter().find(|arg| !arg.starts_with('-'))?;
    let name = package.rsplit('/').next()?;
    let name = match name.rsplit_once('@') {
        Some((head, _)) if !head.is_empty() => head,
        _ => name,
    };
    Some(cwd.join("node_modules/.bin").join(name))
}

/// The `key_env` names in `providers.json`, which count as secrets too.
fn project_key_envs(get: &dyn Fn(&str) -> Option<String>) -> Vec<String> {
    Providers::load(Providers::default_dir(get))
        .map(|providers| {
            providers
                .all()
                .into_iter()
                .filter_map(|provider| provider.key_env)
                .collect()
        })
        .unwrap_or_default()
}

fn expanded_of(transport: &Transport) -> Vec<String> {
    match transport {
        Transport::Stdio { command, args, env, timeout } => {
            let mut out = vec![command.clone(), timeout.to_string()];
            out.extend(args.iter().cloned());
            out.extend(env.iter().map(|(k, v)| format!("{k}={v}")));
            out
        }
        Transport::Http { url, headers } => {
            let mut out = vec![url.clone()];
            out.extend(headers.iter().map(|(k, v)| format!("{k}={v}")));
            out
        }
        Transport::Invalid => Vec::new(),
    }
}

fn read_transport(
    raw: &Value,
    get: &dyn Fn(&str) -> Option<String>,
    refuse: &dyn Fn(&str, &str) -> bool,
) -> Result<Transport, String> {
    let expand = |text: &str, get: &dyn Fn(&str) -> Option<String>| expand_guarded(text, get, refuse);
    let pairs = |key: &str| -> Result<Vec<(String, String)>, String> {
        let mut out = Vec::new();
        if let Some(map) = raw.get(key).and_then(Value::as_object) {
            for (name, value) in map {
                let value = value
                    .as_str()
                    .ok_or_else(|| format!("{key}.{name} is not a string"))?;
                out.push((name.clone(), expand(value, get)?));
            }
        }
        Ok(out)
    };
    if let Some(command) = raw.get("command").and_then(Value::as_str) {
        let mut args = Vec::new();
        for arg in raw.get("args").and_then(Value::as_array).into_iter().flatten() {
            let arg = arg.as_str().ok_or("an argument is not a string")?;
            args.push(expand(arg, get)?);
        }
        let timeout = raw
            .get("timeout_seconds")
            .and_then(Value::as_u64)
            .unwrap_or(DEFAULT_CALL_TIMEOUT)
            .clamp(1, MAX_CALL_TIMEOUT);
        return Ok(Transport::Stdio {
            command: expand(command, get)?,
            args,
            env: pairs("env")?,
            timeout,
        });
    }
    let kind = raw.get("type").and_then(Value::as_str);
    if let (Some("http"), Some(url)) = (kind, raw.get("url").and_then(Value::as_str)) {
        return Ok(Transport::Http {
            url: expand(url, get)?,
            headers: pairs("headers")?,
        });
    }
    Err("the entry needs `command`, or `type: \"http\"` with `url`".into())
}

/// Expand `${VAR}` and `${VAR:-default}`. An unset variable with no default is an error that
/// names it.
pub fn expand(text: &str, get: &dyn Fn(&str) -> Option<String>) -> Result<String, String> {
    expand_guarded(text, get, &|_, _| false)
}

/// `expand`, refusing a variable that `refuse` names as a secret.
fn expand_guarded(
    text: &str,
    get: &dyn Fn(&str) -> Option<String>,
    refuse: &dyn Fn(&str, &str) -> bool,
) -> Result<String, String> {
    let mut out = String::new();
    let mut rest = text;
    while let Some(start) = rest.find("${") {
        out.push_str(&rest[..start]);
        let after = &rest[start + 2..];
        let Some(end) = after.find('}') else {
            out.push_str(&rest[start..]);
            return Ok(out);
        };
        let inner = &after[..end];
        let (name, default) = match inner.split_once(":-") {
            Some((name, default)) => (name, Some(default)),
            None => (inner, None),
        };
        let value = get(name).filter(|value| !value.is_empty());
        if refuse(name, value.as_deref().unwrap_or_default()) {
            return Err(format!(
                "a project server may not read the secret variable {name}; move the server to \
                 ~/.config/ram-code/mcp.json"
            ));
        }
        match value {
            Some(value) => out.push_str(&value),
            None => match default {
                Some(default) => out.push_str(default),
                None => return Err(format!("{name} is not set")),
            },
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

/// The human's decisions, in `mcp-approvals.json`, keyed by the git top level, the server
/// name, and the hash of its entry.
pub struct Approvals {
    path: PathBuf,
    top: String,
    data: Value,
}

impl Approvals {
    pub fn load(dir: &Path, root: &Path) -> Self {
        let path = dir.join(APPROVALS_FILE);
        let data = std::fs::read_to_string(&path)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .filter(Value::is_object)
            .unwrap_or_else(|| json!({}));
        Approvals {
            path,
            top: crate::instructions::top_level(root).display().to_string(),
            data,
        }
    }

    /// The stored choice, when the entry has not changed since. A project server never runs
    /// outside the sandbox, whatever an older approvals file says.
    pub fn get(&self, entry: &ServerEntry) -> Option<Choice> {
        let stored = self.data.get(&self.top)?.get(&entry.name)?;
        if stored.get("hash").and_then(Value::as_str) != Some(entry.hash.as_str()) {
            return None;
        }
        stored
            .get("choice")
            .and_then(Value::as_str)
            .and_then(|key| key.chars().next())
            .and_then(Choice::from_key)
            .filter(|choice| !(entry.source == Source::Project && *choice == Choice::Outside))
    }

    pub fn set(&mut self, entry: &ServerEntry, choice: Choice) -> Result<(), String> {
        if entry.source == Source::Project && choice == Choice::Outside {
            return Err(format!(
                "MCP server {} comes from the repository's .mcp.json, which cannot run outside \
                 the sandbox. To approve it with u, move it to ~/.config/ram-code/mcp.json.",
                visible_line(&entry.name)
            ));
        }
        let top = self.top.clone();
        let data = self
            .data
            .as_object_mut()
            .ok_or("the approvals are not a JSON object")?;
        let project = data.entry(top).or_insert_with(|| json!({}));
        if !project.is_object() {
            *project = json!({});
        }
        project[&entry.name] = json!({"hash": entry.hash, "choice": choice.key()});
        if let Some(dir) = self.path.parent() {
            std::fs::create_dir_all(dir)
                .map_err(|e| format!("could not create {}: {e}", dir.display()))?;
        }
        let partial = self.path.with_extension("json.partial");
        let text = serde_json::to_string_pretty(&self.data).unwrap_or_default() + "\n";
        std::fs::write(&partial, text)
            .and_then(|_| std::fs::rename(&partial, &self.path))
            .map_err(|e| format!("could not write {}: {e}", self.path.display()))
    }

    /// What applies to `entry` now: the stored choice, or for a user server, the sandbox.
    pub fn decision(&self, entry: &ServerEntry) -> Option<Choice> {
        self.get(entry)
            .or((entry.source == Source::User).then_some(Choice::Sandbox))
    }
}

/// The project servers that wait for the human: stdio entries with no current decision.
pub fn pending<'a>(config: &'a Config, approvals: &Approvals) -> Vec<&'a ServerEntry> {
    config
        .servers
        .iter()
        .filter(|server| server.source == Source::Project && server.error.is_none())
        .filter(|server| matches!(server.transport, Transport::Stdio { .. }))
        .filter(|server| approvals.get(server).is_none())
        .collect()
}

/// The approval prompt for one project server.
pub fn prompt_text(entry: &ServerEntry) -> String {
    let mut lines = vec![
        format!("MCP server `{}` from .mcp.json wants to start:", visible_line(&entry.name)),
        format!("  {}", entry.command_line()),
    ];
    if let Transport::Stdio { command, .. } = &entry.transport {
        match &entry.resolved {
            Some(path) => lines.push(format!("  runs: {}", visible_line(&path.display().to_string()))),
            None => lines.push(format!("  runs: {} (not found on PATH)", shown_word(command))),
        }
    }
    for file in &entry.repo_files {
        lines.push(format!(
            "  runs a file from this repository: {}. A change to it asks again.",
            visible_line(file)
        ));
    }
    if !entry.written_env.is_empty() {
        let pairs: Vec<String> = entry
            .written_env
            .iter()
            .map(|(name, value)| shown_word(&format!("{name}={value}")))
            .collect();
        lines.push(format!("  env: {}", pairs.join(" ")));
    }
    if let Transport::Stdio { command, .. } = &entry.transport {
        let program = Path::new(command)
            .file_name()
            .and_then(|name| name.to_str())
            .unwrap_or(command);
        if matches!(program, "npx" | "uvx") {
            lines.push(format!(
                "`{program}` downloads packages on its first run, so this server needs `n` \
                 (network) to start."
            ));
        }
    }
    lines.push(
        "a: approve, sandbox with no network · n: approve, sandbox with network · \
         d: deny · Esc: not now"
            .into(),
    );
    lines.join("\n")
}

/// `/mcp`: one line per server, or the empty state.
pub fn status_lines(config: &Config, approvals: &Approvals) -> Vec<String> {
    let mut lines: Vec<String> = config.errors.iter().map(|e| visible_line(e)).collect();
    if config.servers.is_empty() {
        lines.push(
            "no MCP server configured. Add one to ~/.config/ram-code/mcp.json, or to .mcp.json \
             at the project's top level, in Claude Code's format."
                .into(),
        );
        return lines;
    }
    for server in &config.servers {
        let source = match server.source {
            Source::User => "user",
            Source::Project => "project",
        };
        let (transport, state) = match (&server.transport, &server.error) {
            (_, Some(error)) => ("?", format!("error: {}", visible_line(error))),
            (Transport::Http { .. }, None) => ("http", "not supported yet".to_string()),
            (Transport::Stdio { .. }, None) => ("stdio", "starts with each task".to_string()),
            (Transport::Invalid, None) => ("?", "invalid".to_string()),
        };
        let approval = match approvals.decision(server) {
            Some(choice) => choice.label().to_string(),
            None => "needs approval".to_string(),
        };
        lines.push(format!(
            "{} · {source} · {transport} · {approval} · {state} · {}",
            visible_line(&server.name),
            server.command_line()
        ));
    }
    lines
}

/// `mcp__<server>__<tool>`, with characters outside `[A-Za-z0-9_-]` replaced, cut to 64
/// characters with a short hash, and unique among `used`.
fn offered_name(server: &str, tool: &str, used: &[String]) -> String {
    let full = format!("mcp__{}__{}", clean_name(server), clean_name(tool));
    if full.len() <= MAX_NAME && !used.contains(&full) {
        return full;
    }
    let mut salt = 0u32;
    loop {
        let digest = hex(&Sha256::digest(format!("{server}\0{tool}\0{salt}").as_bytes()));
        let keep = MAX_NAME - 9;
        let candidate = format!("{}_{}", &full[..keep.min(full.len())], &digest[..8]);
        if !used.contains(&candidate) {
            return candidate;
        }
        salt += 1;
    }
}

/// A name with characters outside `[A-Za-z0-9_-]` replaced by `_`.
fn clean_name(text: &str) -> String {
    text.chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' })
        .collect()
}

/// Everything needed to start one server, again after a crash too.
#[derive(Clone)]
struct Launch {
    name: String,
    command: String,
    args: Vec<String>,
    env: Vec<(String, String)>,
    base_env: Vec<(String, String)>,
    choice: Choice,
    root: PathBuf,
    temp: PathBuf,
    home: Option<PathBuf>,
    /// Ram Code's configuration folder, for `sandbox.json` and the write denial on it.
    config: PathBuf,
    sandbox: Sandbox,
}

struct Started {
    client: RunningService<RoleClient, InitializeRequestParams>,
    child: tokio::process::Child,
    tools: Vec<Value>,
}

fn quote(word: &str) -> String {
    format!("'{}'", word.replace('\'', "'\\''"))
}

impl Launch {
    fn command(&self) -> Result<std::process::Command, String> {
        let mut cmd = match self.choice {
            Choice::Outside => {
                let mut cmd = std::process::Command::new(&self.command);
                cmd.args(&self.args);
                cmd
            }
            Choice::Sandbox | Choice::Network => {
                let mut words = vec![quote(&self.command)];
                words.extend(self.args.iter().map(|arg| quote(arg)));
                // The same profile as `run_command`, checked again before each start, a restart
                // too: the server can write its temp folder and could swap it for a link.
                let temp = checked_temp(&self.temp)?;
                let mut paths = Paths::new(&self.root, &temp, self.home.as_deref());
                paths.extras = load_extras(&self.config, paths.home.as_deref())?;
                paths.config = Some(self.config.clone());
                paths.network = self.choice == Choice::Network;
                self.sandbox
                    .command(&paths, &format!("exec {}", words.join(" ")))
                    .ok_or(
                        "Ram Code has no sandbox on this machine; approve the server with `u` \
                         to run it outside the sandbox",
                    )?
            }
            Choice::Deny => return Err("the server is denied".into()),
        };
        cmd.current_dir(&self.root)
            .env_clear()
            .envs(self.base_env.iter().cloned())
            .envs(self.env.iter().cloned())
            .env("TMPDIR", &self.temp)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null());
        // Its own session: no terminal, and one process group to kill at the end.
        use std::os::unix::process::CommandExt;
        // SAFETY: setsid is async-signal-safe, which is all pre_exec may call.
        unsafe {
            cmd.pre_exec(|| {
                if libc::setsid() == -1 {
                    return Err(std::io::Error::last_os_error());
                }
                Ok(())
            });
        }
        Ok(cmd)
    }

    async fn start(self) -> Result<Started, String> {
        let command = self.command()?;
        let mut child = tokio::process::Command::from(command)
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| format!("could not run {}: {e}", self.command))?;
        let stdout = Bounded {
            inner: child.stdout.take().ok_or("no stdout")?,
            line: 0,
            max: MAX_MESSAGE,
        };
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let pid = child.id();
        let handshake = async {
            let info = InitializeRequestParams::new(
                ClientCapabilities::default(),
                Implementation::new("ram-code", env!("CARGO_PKG_VERSION")),
            );
            let client = info
                .serve((stdout, stdin))
                .await
                .map_err(|e| format!("the handshake failed: {e}"))?;
            let tools = client
                .peer()
                .list_all_tools()
                .await
                .map_err(|e| format!("tools/list failed: {e}"))?;
            let tools = tools
                .iter()
                .filter_map(|tool| serde_json::to_value(tool).ok())
                .collect();
            Ok::<_, String>((client, tools))
        };
        match tokio::time::timeout(START_TIMEOUT, handshake).await {
            Ok(Ok((client, tools))) => Ok(Started { client, child, tools }),
            Ok(Err(error)) => {
                kill_group(pid);
                Err(error)
            }
            Err(_) => {
                kill_group(pid);
                Err(format!("not ready after {} s", START_TIMEOUT.as_secs()))
            }
        }
    }
}

fn kill_group(pid: Option<u32>) {
    if let Some(pid) = pid {
        // SAFETY: kill has no memory preconditions. The negative pid names the process group
        // that setsid created, so the server's children go too.
        unsafe {
            libc::kill(-(pid as i32), libc::SIGKILL);
        }
    }
}

/// The server's output, with a cap on one message. rmcp reads a whole line before it parses
/// it, so a line longer than `max` ends the transport instead of filling memory.
struct Bounded<R> {
    inner: R,
    line: usize,
    max: usize,
}

impl<R: tokio::io::AsyncRead + Unpin> tokio::io::AsyncRead for Bounded<R> {
    fn poll_read(
        mut self: std::pin::Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
        buf: &mut tokio::io::ReadBuf<'_>,
    ) -> std::task::Poll<std::io::Result<()>> {
        let before = buf.filled().len();
        let this = &mut *self;
        let poll = std::pin::Pin::new(&mut this.inner).poll_read(cx, buf);
        if let std::task::Poll::Ready(Ok(())) = poll {
            for &byte in &buf.filled()[before..] {
                this.line = if byte == b'\n' { 0 } else { this.line + 1 };
            }
            if this.line > this.max {
                return std::task::Poll::Ready(Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("a message from the server passed {} MiB", this.max / 1024 / 1024),
                )));
            }
        }
        poll
    }
}

/// True when `pid` has exited. The exit stays to be reaped, so its process group id is not
/// free for reuse until then.
fn exited(pid: u32) -> bool {
    // SAFETY: waitid writes only into `info`, which lives on this stack.
    unsafe {
        let mut info: libc::siginfo_t = std::mem::zeroed();
        let done = libc::waitid(
            libc::P_PID,
            pid as libc::id_t,
            &mut info,
            libc::WEXITED | libc::WNOHANG | libc::WNOWAIT,
        );
        if done != 0 {
            // No such child: nothing to wait for.
            return true;
        }
        #[cfg(target_os = "linux")]
        let who = info.si_pid();
        #[cfg(not(target_os = "linux"))]
        let who = info.si_pid;
        who != 0
    }
}

/// The processes under `pid`, from one `ps` listing. A child that left the process group with
/// `setsid` is still under its parent until the parent exits.
fn descendants(pid: u32) -> Vec<i32> {
    let Ok(out) = std::process::Command::new("ps")
        .args(["-A", "-o", "pid=", "-o", "ppid="])
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    let pairs: Vec<(i32, i32)> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| {
            let mut words = line.split_whitespace();
            Some((words.next()?.parse().ok()?, words.next()?.parse().ok()?))
        })
        .collect();
    let mut found = Vec::new();
    let mut queue = vec![pid as i32];
    while let Some(parent) = queue.pop() {
        for &(child, of) in &pairs {
            if of == parent && !found.contains(&child) {
                found.push(child);
                queue.push(child);
            }
        }
    }
    found
}

/// Kill the server's group, then each descendant and its own group.
fn kill_tree(pid: Option<u32>, below: &[i32]) {
    kill_group(pid);
    for &child in below {
        // SAFETY: kill has no memory preconditions.
        unsafe {
            libc::kill(-child, libc::SIGKILL);
            libc::kill(child, libc::SIGKILL);
        }
    }
}

struct Server {
    launch: Launch,
    timeout: Duration,
    client: Option<RunningService<RoleClient, InitializeRequestParams>>,
    child: Option<tokio::process::Child>,
    crashes: u32,
    failed: Option<String>,
}

struct Offered {
    name: String,
    server: usize,
    tool: String,
    definition: Value,
}

struct Inner {
    rt: tokio::runtime::Runtime,
    servers: Vec<Server>,
    offered: Vec<Offered>,
}

/// The MCP servers of one task, started in the worker when the task starts.
pub struct Session {
    inner: RefCell<Option<Inner>>,
    notes: Vec<String>,
}

impl Session {
    /// Start the servers that may start. `env` is Ram Code's environment.
    pub fn start(root: &Path, env: Vec<(String, String)>) -> Self {
        let get = |key: &str| {
            env.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
        };
        let config = load(root, &get);
        let mut notes: Vec<String> = config.errors.iter().map(|e| format!("mcp: {e}")).collect();
        let empty = |notes: Vec<String>| Session {
            inner: RefCell::new(None),
            notes: notes.iter().map(|note| visible_line(note)).collect(),
        };
        if config.servers.is_empty() {
            return empty(notes);
        }
        let config_dir = Providers::default_dir(&get);
        let approvals = Approvals::load(&config_dir, root);
        let key_envs = Tools::with_env(root, env.clone()).key_envs;
        let base_env = scrub(&env, &key_envs);
        let home = get("HOME").map(PathBuf::from).filter(|home| home.is_absolute());
        let sandbox = Sandbox::detect();
        let rt = match tokio::runtime::Builder::new_current_thread().enable_all().build() {
            Ok(rt) => rt,
            Err(error) => {
                notes.push(format!("mcp: could not start the runtime: {error}"));
                return empty(notes);
            }
        };
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());

        let mut servers = Vec::new();
        for entry in &config.servers {
            if let Some(error) = &entry.error {
                notes.push(format!("mcp: server {} failed: {error}", entry.name));
                continue;
            }
            let (command, args, server_env, timeout) = match &entry.transport {
                Transport::Stdio { command, args, env, timeout } => {
                    (command.clone(), args.clone(), env.clone(), *timeout)
                }
                Transport::Http { .. } => {
                    notes.push(format!(
                        "mcp: server {} uses http, which is not supported yet",
                        entry.name
                    ));
                    continue;
                }
                Transport::Invalid => continue,
            };
            let choice = match approvals.decision(entry) {
                Some(Choice::Deny) => continue,
                Some(choice) => choice,
                None => {
                    notes.push(format!(
                        "mcp: server {} from .mcp.json needs approval, so it does not start. \
                         Approve it in Ram Code, or with /mcp approve {}.",
                        entry.name, entry.name
                    ));
                    continue;
                }
            };
            let temp = std::env::temp_dir().join(format!(
                "ram-code-mcp-{}-{}",
                std::process::id(),
                TEMP_SEQ.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = std::fs::create_dir_all(&temp);
            let temp = temp.canonicalize().unwrap_or(temp);
            servers.push(Server {
                launch: Launch {
                    name: entry.name.clone(),
                    command,
                    args,
                    env: server_env,
                    base_env: base_env.clone(),
                    choice,
                    root: root.clone(),
                    temp,
                    home: home.clone(),
                    config: config_dir.clone(),
                    sandbox: sandbox.clone(),
                },
                timeout: Duration::from_secs(timeout),
                client: None,
                child: None,
                crashes: 0,
                failed: None,
            });
        }

        // Start every server at once.
        let launches: Vec<Launch> = servers.iter().map(|s| s.launch.clone()).collect();
        let results = rt.block_on(async {
            let mut set = tokio::task::JoinSet::new();
            for (index, launch) in launches.into_iter().enumerate() {
                set.spawn(async move { (index, launch.start().await) });
            }
            let mut results = Vec::new();
            while let Some(joined) = set.join_next().await {
                if let Ok(result) = joined {
                    results.push(result);
                }
            }
            results
        });
        let mut tools_of: Vec<Vec<Value>> = vec![Vec::new(); servers.len()];
        for (index, result) in results {
            match result {
                Ok(started) => {
                    servers[index].client = Some(started.client);
                    servers[index].child = Some(started.child);
                    tools_of[index] = started.tools;
                }
                Err(error) => {
                    notes.push(format!(
                        "mcp: server {} failed: {error}",
                        servers[index].launch.name
                    ));
                    servers[index].failed = Some(error);
                }
            }
        }

        let mut offered: Vec<Offered> = Vec::new();
        for (index, tools) in tools_of.into_iter().enumerate() {
            let server_name = servers[index].launch.name.clone();
            let named: Vec<(String, Value)> = tools
                .into_iter()
                .filter_map(|tool| {
                    let name = tool.get("name").and_then(Value::as_str)?.to_string();
                    Some((name, tool))
                })
                .collect();
            // Two tools that read the same to a hook rule are both refused: an alias would let
            // one of them pass a rule written for the other.
            let mut refused: Vec<String> = Vec::new();
            for (i, (name, _)) in named.iter().enumerate() {
                let clashes = named
                    .iter()
                    .enumerate()
                    .any(|(j, (other, _))| i != j && clean_name(other) == clean_name(name));
                if clashes && !refused.contains(name) {
                    refused.push(name.clone());
                }
            }
            if !refused.is_empty() {
                notes.push(format!(
                    "mcp: server {server_name} lists tools whose names repeat or read the same \
                     ({}); Ram Code offers none of them",
                    refused.join(", ")
                ));
            }
            let mut count = 0;
            let mut over = 0;
            for (tool_name, tool) in named {
                if refused.contains(&tool_name) {
                    continue;
                }
                let schema = tool.get("inputSchema").cloned().unwrap_or(Value::Null);
                if schema.get("type").and_then(Value::as_str) != Some("object") {
                    notes.push(format!(
                        "mcp: server {server_name}: tool {tool_name} has an input schema that is \
                         not a JSON object schema, so Ram Code does not offer it"
                    ));
                    continue;
                }
                if count == MAX_TOOLS {
                    over += 1;
                    continue;
                }
                count += 1;
                let used: Vec<String> = offered.iter().map(|o| o.name.clone()).collect();
                let name = offered_name(&server_name, &tool_name, &used);
                let mut description: String = visible(
                    tool.get("description").and_then(Value::as_str).unwrap_or(""),
                );
                if description.chars().count() > MAX_DESCRIPTION {
                    description = description.chars().take(MAX_DESCRIPTION - 1).collect();
                    description.push('…');
                }
                let definition = json!({
                    "name": name,
                    "description": description,
                    "input_schema": schema,
                });
                offered.push(Offered {
                    name,
                    server: index,
                    tool: tool_name,
                    definition,
                });
            }
            if over > 0 {
                notes.push(format!(
                    "mcp: server {server_name} lists {} tools; Ram Code offers the first {MAX_TOOLS}",
                    MAX_TOOLS + over
                ));
            }
        }
        for (index, server) in servers.iter().enumerate() {
            if server.client.is_some() {
                let count = offered.iter().filter(|o| o.server == index).count();
                notes.push(format!(
                    "mcp: server {} ready ({}, {count} tools)",
                    server.launch.name,
                    server.launch.choice.label()
                ));
            }
        }
        Session {
            inner: RefCell::new(Some(Inner { rt, servers, offered })),
            notes: notes.iter().map(|note| visible_line(note)).collect(),
        }
    }

    /// Transcript lines from the start: failures, skipped servers, ready servers.
    pub fn notes(&self) -> &[String] {
        &self.notes
    }

    /// Close every server's input at once, give them two seconds together, then kill each
    /// process tree before the exits are reaped.
    pub fn shutdown(&mut self) {
        let Some(mut inner) = self.inner.borrow_mut().take() else {
            return;
        };
        let Inner { rt, servers, .. } = &mut inner;
        stop_all(rt, servers.iter_mut().collect());
        for server in servers.iter() {
            let _ = std::fs::remove_dir_all(&server.launch.temp);
        }
    }
}

fn stop(rt: &tokio::runtime::Runtime, server: &mut Server) {
    stop_all(rt, vec![server]);
}

/// Stop servers together. The process tree is listed while each server still runs, and killed
/// before its exit is reaped, so no group id or child id is reused in between.
fn stop_all(rt: &tokio::runtime::Runtime, servers: Vec<&mut Server>) {
    // Timers need the runtime's context when they are built, before `block_on` runs them.
    let _context = rt.enter();
    let trees: Vec<(Option<u32>, Vec<i32>)> = servers
        .iter()
        .map(|server| {
            let pid = server.child.as_ref().and_then(|child| child.id());
            (pid, pid.map(descendants).unwrap_or_default())
        })
        .collect();
    let mut servers = servers;
    let clients: Vec<_> = servers.iter_mut().filter_map(|s| s.client.take()).collect();
    let pids: Vec<u32> = trees.iter().filter_map(|(pid, _)| *pid).collect();
    rt.block_on(async {
        let mut set = tokio::task::JoinSet::new();
        for client in clients {
            set.spawn(async move {
                let _ = tokio::time::timeout(EXIT_GRACE, client.cancel()).await;
            });
        }
        while set.join_next().await.is_some() {}
        let deadline = tokio::time::Instant::now() + EXIT_GRACE;
        while !pids.iter().all(|&pid| exited(pid)) && tokio::time::Instant::now() < deadline {
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    for (pid, below) in &trees {
        kill_tree(*pid, below);
    }
    for server in servers {
        if let Some(mut child) = server.child.take() {
            let _ = rt.block_on(tokio::time::timeout(EXIT_GRACE, child.wait()));
        }
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shutdown();
    }
}

impl ToolSource for Session {
    fn definitions(&self) -> Vec<Value> {
        self.inner
            .borrow()
            .as_ref()
            .map(|inner| inner.offered.iter().map(|o| o.definition.clone()).collect())
            .unwrap_or_default()
    }

    fn handles(&self, name: &str) -> bool {
        self.inner
            .borrow()
            .as_ref()
            .is_some_and(|inner| inner.offered.iter().any(|o| o.name == name))
    }

    fn call(&self, name: &str, input: &Value) -> Result<String, String> {
        let mut guard = self.inner.borrow_mut();
        let inner = guard.as_mut().ok_or("no MCP server runs")?;
        let offered = inner
            .offered
            .iter()
            .find(|o| o.name == name)
            .ok_or_else(|| format!("unknown tool: {name}"))?;
        let (index, tool) = (offered.server, offered.tool.clone());
        let Inner { rt, servers, .. } = inner;
        let _context = rt.enter();
        let server = &mut servers[index];
        let label = server.launch.name.clone();
        if let Some(reason) = &server.failed {
            return Err(format!("MCP server {label} failed: {reason}"));
        }
        if server.client.is_none() {
            // The server crashed once in this task: start it one more time.
            match rt.block_on(server.launch.clone().start()) {
                Ok(started) => {
                    server.client = Some(started.client);
                    server.child = Some(started.child);
                }
                Err(error) => {
                    server.failed = Some(format!("it did not restart: {error}"));
                    return Err(format!("MCP server {label} failed: it did not restart: {error}"));
                }
            }
        }
        let arguments: Map<String, Value> = input.as_object().cloned().unwrap_or_default();
        let params = CallToolRequestParams::new(tool).with_arguments(arguments);
        let outcome = {
            let client = server.client.as_ref().ok_or("the server is not running")?;
            rt.block_on(tokio::time::timeout(
                server.timeout,
                client.peer().call_tool(params),
            ))
        };
        match outcome {
            Err(_) => Err(format!(
                "the call to MCP server {label} timed out after {} s",
                server.timeout.as_secs()
            )),
            Ok(Ok(result)) => convert(&serde_json::to_value(&result).unwrap_or(Value::Null)),
            Ok(Err(error)) => {
                let exited = server.child.as_mut().is_some_and(|child| {
                    rt.block_on(tokio::time::timeout(Duration::from_millis(500), child.wait()))
                        .is_ok()
                });
                if !exited && !matches!(error, rmcp::ServiceError::TransportClosed) {
                    return Err(format!("MCP server {label}: {error}"));
                }
                stop(rt, server);
                server.crashes += 1;
                if server.crashes >= 2 {
                    server.failed = Some("it crashed twice".into());
                    Err(format!("MCP server {label} crashed again and is marked failed"))
                } else {
                    Err(format!("MCP server {label} crashed; it starts again on the next call"))
                }
            }
        }
    }
}

/// A `tools/call` result as text for the model. `isError` gives an error.
fn convert(result: &Value) -> Result<String, String> {
    let mut parts: Vec<String> = Vec::new();
    let mut texts = 0;
    for item in result.get("content").and_then(Value::as_array).into_iter().flatten() {
        let kind = item.get("type").and_then(Value::as_str).unwrap_or("");
        let field = |key: &str| item.get(key).and_then(Value::as_str).unwrap_or("");
        match kind {
            "text" => {
                texts += 1;
                parts.push(field("text").to_string());
            }
            "image" | "audio" => parts.push(format!(
                "[{kind}: {}, {} bytes]",
                field("mimeType"),
                base64_len(field("data"))
            )),
            "resource_link" => parts.push(format!("[resource: {}]", field("uri"))),
            "resource" => {
                let resource = item.get("resource").cloned().unwrap_or(Value::Null);
                match resource.get("text").and_then(Value::as_str) {
                    Some(text) => {
                        texts += 1;
                        parts.push(text.to_string());
                    }
                    None => parts.push(format!(
                        "[resource: {}]",
                        resource.get("uri").and_then(Value::as_str).unwrap_or("")
                    )),
                }
            }
            other => parts.push(format!("[{other}]")),
        }
    }
    if texts == 0 {
        if let Some(structured) = result.get("structuredContent").filter(|v| !v.is_null()) {
            parts.push(structured.to_string());
        }
    }
    let mut text = visible(&parts.join("\n"));
    if text.chars().count() > MAX_RESULT {
        text = text.chars().take(MAX_RESULT).collect();
        text.push_str(&format!("\n[cut: the result passed {MAX_RESULT} characters]"));
    }
    if result.get("isError").and_then(Value::as_bool) == Some(true) {
        Err(text)
    } else {
        Ok(text)
    }
}

/// Control characters as caret or `\u{..}` text, so a server cannot drive the terminal or the
/// transcript. Zero-width and bidi marks show as `\u{..}` too, so text cannot hide or reorder.
/// New lines and tabs stay.
fn visible(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '\n' | '\t' => out.push(c),
            c if (c as u32) < 0x20 => {
                out.push('^');
                out.push(char::from(c as u8 + 0x40));
            }
            '\u{7f}' => out.push_str("^?"),
            c if c.is_control() || hidden_mark(c) => {
                out.push_str(&format!("\\u{{{:x}}}", c as u32))
            }
            c => out.push(c),
        }
    }
    out
}

/// Format characters that hide text or change its order on screen.
fn hidden_mark(c: char) -> bool {
    matches!(c,
        '\u{ad}' | '\u{61c}' | '\u{180e}'
        | '\u{200b}'..='\u{200f}'
        | '\u{202a}'..='\u{202e}'
        | '\u{2060}'..='\u{2064}'
        | '\u{2066}'..='\u{2069}'
        | '\u{feff}')
}

/// `visible` on one line: a new line or a tab shows as `\n` or `\t`, so one value cannot
/// start a line of its own in a prompt or a note.
pub fn visible_line(text: &str) -> String {
    visible(text).replace('\n', "\\n").replace('\t', "\\t")
}

/// One word of a command line: escaped, and quoted unless it holds only plain characters.
fn shown_word(word: &str) -> String {
    let plain = !word.is_empty()
        && word
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_-./:@=+,%".contains(c));
    let shown = visible_line(word);
    if plain {
        shown
    } else {
        quote(&shown)
    }
}

/// The decoded size of base64 text.
fn base64_len(data: &str) -> usize {
    let data = data.trim();
    let padding = data.chars().rev().take_while(|c| *c == '=').count();
    (data.len() / 4 * 3).saturating_sub(padding)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expand_handles_defaults_and_unset_names() {
        let get = |key: &str| (key == "A").then(|| "x".to_string());
        assert_eq!(expand("${A}-${B:-d}", &get).unwrap(), "x-d");
        assert_eq!(expand("plain $HOME", &get).unwrap(), "plain $HOME");
        assert_eq!(expand("${C}", &get).unwrap_err(), "C is not set");
    }

    #[test]
    fn base64_len_counts_padding() {
        assert_eq!(base64_len("aGVsbG8="), 5);
        assert_eq!(base64_len("aGVsbG8h"), 6);
    }

    #[test]
    fn long_and_clashing_names_stay_valid() {
        let long = "x".repeat(80);
        let first = offered_name("s", &long, &[]);
        assert!(first.len() <= MAX_NAME);
        let second = offered_name("s", &long, std::slice::from_ref(&first));
        assert_ne!(first, second);
        assert_eq!(offered_name("my server", "do it", &[]), "mcp__my_server__do_it");
    }
}
