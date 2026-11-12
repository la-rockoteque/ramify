use crate::config::{git, Config};
use serde_json::{json, Value};
use std::io::Read;
use std::path::{Path, PathBuf};

pub fn statusline(args: &[String]) -> i32 {
    let json_out = args.first().map(String::as_str) == Some("--json");
    let mut input = String::new();
    let _ = std::io::stdin().read_to_string(&mut input);
    let value: Value = serde_json::from_str(&input).unwrap_or_else(|_| json!({}));
    let session = value
        .get("session_id")
        .and_then(Value::as_str)
        .unwrap_or("-")
        .to_string();
    let cwd = value
        .get("workspace")
        .and_then(|workspace| workspace.get("current_dir"))
        .and_then(Value::as_str)
        .or_else(|| value.get("cwd").and_then(Value::as_str))
        .unwrap_or(".")
        .to_string();
    if let Some(text) = render(&session, &cwd, json_out, Mode::Statusline) {
        println!("{text}");
    }
    0
}

/// `Statusline` probes each service over HTTP and caches the result. `Prompt` runs before every
/// shell prompt, so it reads that cache instead, and adds the agent sessions of the worktree.
#[derive(PartialEq, Clone, Copy)]
pub(crate) enum Mode {
    Statusline,
    Prompt,
}

/// A health result older than this is not shown: the stack may have changed since.
const HEALTH_TTL: u64 = 60;

pub(crate) fn render(session: &str, cwd: &str, json_out: bool, mode: Mode) -> Option<String> {
    let state_root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    let sessions = Path::new(&state_root).join("sessions");
    let pin = sessions.join(format!("{session}.root"));
    let name = std::fs::read_to_string(sessions.join(format!("{session}.name")))
        .ok()
        .and_then(|text| text.lines().next().map(str::to_string))
        .unwrap_or_default();

    let mut root: Option<PathBuf> = None;
    if pin.is_file() {
        if let Ok(text) = std::fs::read_to_string(&pin) {
            let candidate = PathBuf::from(text.trim());
            if git(&["rev-parse", "--git-dir"], &candidate).is_ok() {
                root = Some(candidate);
            }
        }
    }
    if root.is_none() {
        if let Ok(text) = git(&["rev-parse", "--show-toplevel"], Path::new(&cwd)) {
            root = Some(PathBuf::from(text.trim()));
        }
    }
    let root = root?;
    let primary = git(&["worktree", "list", "--porcelain"], &root)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("worktree "))
                .map(PathBuf::from)
        })?;
    let conf = config_file(&root, &primary)?;
    let branch = git(&["rev-parse", "--abbrev-ref", "HEAD"], &root)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "?".into());
    let icon = std::env::var("RAMIFY_STATUSLINE_ICON").unwrap_or_else(|_| "𖣂".into());
    let worktree = root
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let conf_text = std::fs::read_to_string(&conf).unwrap_or_default();
    let project = conf_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("RAMIFY_PROJECT=")?;
            Some(rest.trim_matches(['\'', '"']).to_string())
        })
        .next_back()
        .filter(|project| !project.is_empty())
        .unwrap_or_else(|| {
            primary
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let directory = state_dir(&state_root, &project, &primary);
    let env = directory.join(format!("{worktree}.env"));
    let slot = env_get(&env, "SLOT");
    let services = conf_text
        .lines()
        .any(|line| line.trim_start().starts_with("RAMIFY_SERVICES=") && line.trim().len() > "RAMIFY_SERVICES=".len());

    let mut stack: Option<&str> = None;
    let mut answering = 0;
    let mut total = 0;
    if services {
        // Without a fresh measure, the prompt cannot tell a stopped stack from a running one,
        // so it says nothing rather than "stopped".
        if mode == Mode::Statusline || !env.is_file() {
            stack = Some("stopped");
        }
        let cache = directory.join(format!("{worktree}.health"));
        let measured = match mode {
            Mode::Statusline if env.is_file() => {
                let ports: Vec<u32> = env.lines_ports();
                let answered = ports
                    .iter()
                    .filter(|port| crate::dash::http_status_path(**port, "/").is_some())
                    .count();
                let _ = std::fs::write(&cache, format!("{answered} {} {}\n", ports.len(), epoch()));
                Some((answered, ports.len()))
            }
            Mode::Prompt if env.is_file() => cached_health(&cache),
            _ => None,
        };
        if let Some((answered, ports)) = measured {
            answering = answered;
            total = ports;
            stack = Some(if total == 0 || answering == 0 {
                "down"
            } else if answering == total {
                "up"
            } else {
                "partial"
            });
        }
    }

    let source_mode = conf_text
        .lines()
        .filter_map(|line| {
            let line = line.trim();
            let rest = line.strip_prefix("RAMIFY_WORKFLOW_SOURCE=")?;
            Some(rest.trim_matches(['\'', '"']).to_string())
        })
        .next_back()
        .unwrap_or_default();
    let mut flow = String::new();
    let mut flow_json: Option<String> = None;
    if root.join(".ramify").is_dir()
        || primary.join(".ramify").is_dir()
        || source_mode == "repo"
        || source_mode == "select"
    {
        if let Ok(config) = Config::for_root(&root) {
            if let Some(progress) = crate::workflow::report(&config, &root, &branch) {
                flow = crate::workflow::line(&progress);
                flow_json = Some(crate::workflow::progress_json(&progress));
            }
        }
    }
    let runs = run_line(&state_root, &root);
    let health = match stack {
        Some("up") | Some("partial") => format!(" · {answering}/{total} up"),
        Some("down") => " · stack down".into(),
        _ => String::new(),
    };

    if json_out {
        let ports = ports_map(&env);
        let ticket = if conf_text.contains("RAMIFY_JIRA_URL=") {
            crate::dash::ticket_of(&branch, &Config::for_root(&root).unwrap_or_else(|_| Config::for_root(&primary).unwrap()))
        } else {
            None
        };
        let value = json!({
            "root": root.to_string_lossy(),
            "worktree": worktree,
            "branch": branch,
            "name": if name.is_empty() { Value::Null } else { Value::String(name) },
            "primary": root == primary,
            "icon": icon,
            "slot": slot.as_deref().and_then(|slot| slot.parse::<u64>().ok()).map(Value::from).unwrap_or(Value::Null),
            "ticket": ticket.map(Value::String).unwrap_or(Value::Null),
            "stack": stack.map(Value::from).unwrap_or(Value::Null),
            "answering": answering,
            "services": total,
            "workflow": flow_json.map(|text| serde_json::from_str::<Value>(&text).unwrap_or(Value::Null)).unwrap_or(Value::Null),
            "ports": ports,
            "run": if runs.is_empty() { Value::Null } else { Value::String(runs) },
        });
        let mut value = value;
        if mode == Mode::Prompt {
            value["sessions"] = Value::Array(
                agents(&state_root, &root)
                    .into_iter()
                    .map(|agent| json!({"name": agent.name, "agent": agent.agent, "state": agent.state, "since": agent.since}))
                    .collect(),
            );
        }
        return Some(ascii_json(&crate::pyjson::to_string(&value)));
    }
    let prefix = if name.is_empty() {
        String::new()
    } else {
        format!("{name} · ")
    };
    let flow_part = if flow.is_empty() {
        String::new()
    } else {
        format!(" · {flow}")
    };
    let run_part = if runs.is_empty() {
        String::new()
    } else {
        format!(" · {runs}")
    };
    let agents_part = match mode {
        Mode::Prompt => agents_line(&agents(&state_root, &root)),
        Mode::Statusline => String::new(),
    };
    Some(if root == primary {
        format!("{icon} {prefix}{branch}{health}{flow_part}{run_part}{agents_part}")
    } else {
        format!("{icon} {prefix}{worktree} ⎇ {branch}{health}{flow_part}{run_part}{agents_part}")
    })
}

fn epoch() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// `<answering> <total> <epoch>`, as `statusline` wrote it, while it is fresh.
fn cached_health(cache: &Path) -> Option<(usize, usize)> {
    let text = std::fs::read_to_string(cache).ok()?;
    let mut fields = text.split_whitespace().map(|field| field.parse::<u64>().ok());
    let (answering, total, at) = (fields.next()??, fields.next()??, fields.next()??);
    (epoch().saturating_sub(at) <= HEALTH_TTL).then_some((answering as usize, total as usize))
}

/// A live agent session: its record, as the session hooks wrote it.
pub(crate) struct Agent {
    pub(crate) id: String,
    pub(crate) name: Option<String>,
    pub(crate) agent: String,
    pub(crate) state: String,
    pub(crate) since: u64,
    pub(crate) root: PathBuf,
    pub(crate) pid: i32,
}

/// Every session record in the state folder, live or not. A record that does not parse is skipped.
pub(crate) fn records(state_root: &str) -> Vec<Agent> {
    let folder = Path::new(state_root).join("sessions");
    let Ok(entries) = std::fs::read_dir(&folder) else {
        return Vec::new();
    };
    entries
        .flatten()
        .filter_map(|entry| {
            let file = entry.file_name().to_string_lossy().into_owned();
            let id = file.strip_suffix(".session")?.to_string();
            let value: Value = serde_json::from_str(&std::fs::read_to_string(entry.path()).ok()?).ok()?;
            let name = std::fs::read_to_string(folder.join(format!("{id}.name")))
                .ok()
                .and_then(|text| text.lines().next().map(printable))
                .filter(|name| !name.is_empty());
            Some(Agent {
                id,
                name,
                agent: printable(value.get("agent")?.as_str()?),
                state: printable(value.get("state").and_then(Value::as_str).unwrap_or("idle")),
                since: value.get("since").and_then(Value::as_u64).unwrap_or(0),
                root: PathBuf::from(value.get("root")?.as_str()?),
                pid: i32::try_from(value.get("pid")?.as_u64()?).ok()?,
            })
        })
        .collect()
}

/// A record is written by any process that can write the state folder: a control character in
/// it would reach the terminal as an escape sequence.
fn printable(text: &str) -> String {
    text.chars().filter(|c| !c.is_control()).collect()
}

/// Whether the process runs. EPERM means it runs under another user.
pub(crate) fn running(pid: i32) -> bool {
    if pid <= 0 {
        return false;
    }
    // SAFETY: signal 0 checks that the process exists and sends nothing.
    let result = unsafe { libc::kill(pid, 0) };
    result == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// The live sessions of the worktree at `root`, oldest first.
fn agents(state_root: &str, root: &Path) -> Vec<Agent> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut found: Vec<Agent> = records(state_root)
        .into_iter()
        .filter(|agent| agent.root.canonicalize().unwrap_or_else(|_| agent.root.clone()) == root)
        .filter(|agent| running(agent.pid))
        .collect();
    found.sort_by(|a, b| a.since.cmp(&b.since).then_with(|| a.id.cmp(&b.id)));
    found
}

/// ` · ✻ birch, ◇ ash ⏸`: Claude Code is ✻, Codex is ◇, Ram Code is ◆, and ⏸ follows an idle
/// session.
fn agents_line(agents: &[Agent]) -> String {
    if agents.is_empty() {
        return String::new();
    }
    let parts: Vec<String> = agents
        .iter()
        .map(|agent| {
            let mark = match agent.agent.as_str() {
                "codex" => "◇",
                "ram" => "◆",
                _ => "✻",
            };
            let label = agent.name.clone().unwrap_or_else(|| agent.agent.clone());
            let idle = if agent.state == "idle" { " ⏸" } else { "" };
            format!("{mark} {label}{idle}")
        })
        .collect();
    format!(" · {}", parts.join(", "))
}

fn config_file(root: &Path, primary: &Path) -> Option<PathBuf> {
    for base in [root, primary] {
        for name in [".ramify.conf", ".claude/ramify.conf"] {
            let path = base.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn state_dir(state_root: &str, project: &str, primary: &Path) -> PathBuf {
    let plain = Path::new(state_root).join(project);
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(primary.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let short: String = digest[..4].iter().map(|byte| format!("{byte:02x}")).collect();
    let twin = PathBuf::from(format!("{}-{short}", plain.display()));
    let claim = std::fs::read_to_string(plain.join("primary"))
        .ok()
        .map(|text| text.trim().to_string());
    let claim = claim.unwrap_or_default();
    if twin.join("primary").is_file() || (!claim.is_empty() && claim != primary.to_string_lossy() && Path::new(&claim).is_dir()) {
        twin
    } else {
        plain
    }
}

fn env_get(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .map(str::to_string)
}

trait Ports {
    fn lines_ports(&self) -> Vec<u32>;
}

impl Ports for PathBuf {
    fn lines_ports(&self) -> Vec<u32> {
        let Ok(text) = std::fs::read_to_string(self) else {
            return Vec::new();
        };
        text.lines()
            .filter_map(|line| {
                let (key, value) = line.trim().split_once('=')?;
                if key.ends_with("_PORT") {
                    value.parse().ok()
                } else {
                    None
                }
            })
            .collect()
    }
}

fn ports_map(env: &Path) -> Value {
    let mut map = serde_json::Map::new();
    if let Ok(text) = std::fs::read_to_string(env) {
        for line in text.lines() {
            let Some((key, value)) = line.trim().split_once('=') else {
                continue;
            };
            if let Some(name) = key.strip_suffix("_PORT") {
                if let Ok(port) = value.parse::<u64>() {
                    map.insert(name.to_lowercase(), Value::from(port));
                }
            }
        }
    }
    Value::Object(map)
}

fn run_line(state_root: &str, root: &Path) -> String {
    let _ = state_root;
    let directory = std::env::var("RAMIFY_RUNS_DIR").unwrap_or_else(|_| {
        let base = std::env::var("CLAUDE_CONFIG_DIR")
            .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()));
        format!("{base}/ramify/runs")
    });
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in names {
        let Ok(text) = std::fs::read_to_string(Path::new(&directory).join(&name).join("meta.json")) else {
            continue;
        };
        let Ok(meta) = serde_json::from_str::<Value>(&text) else {
            continue;
        };
        if meta.get("status").and_then(Value::as_str) != Some("pending") {
            continue;
        }
        if meta.get("root").and_then(Value::as_str) != Some(&root.to_string_lossy()) {
            continue;
        }
        if let Some(id) = meta.get("id").and_then(Value::as_str) {
            return format!("▶ run {id} waits on you");
        }
    }
    String::new()
}

/// Python's `json.dumps` escapes every non-ASCII character by default.
fn ascii_json(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for character in text.chars() {
        let code = character as u32;
        if code < 0x80 {
            out.push(character);
        } else if code <= 0xFFFF {
            out.push_str(&format!("\\u{code:04x}"));
        } else {
            let adjusted = code - 0x1_0000;
            let high = 0xD800 + (adjusted >> 10);
            let low = 0xDC00 + (adjusted & 0x3FF);
            out.push_str(&format!("\\u{high:04x}\\u{low:04x}"));
        }
    }
    out
}
