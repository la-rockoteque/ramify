use crate::config::{git, Config};
use crate::workflow;
use serde_json::value::RawValue;
use serde::Serialize;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

#[derive(Serialize)]
pub struct Project {
    pub name: String,
    pub primary: String,
    pub stacks: bool,
    pub jira: Option<String>,
    pub containers: Vec<Container>,
    pub shared: Vec<Shared>,
    pub build_servers: Vec<BuildServer>,
    pub strays: Vec<Stray>,
    pub orphans: Vec<Orphan>,
    pub worktrees: Vec<WorktreeJson>,
}

#[derive(Serialize)]
pub struct Container {
    pub name: String,
    pub up: bool,
    pub mb: Option<u64>,
}

#[derive(Serialize)]
pub struct Shared {
    pub service: String,
    pub port: u32,
    pub up: bool,
    pub mb: Option<u64>,
}

#[derive(Serialize)]
pub struct BuildServer {
    pub kind: String,
    pub count: u64,
    pub mb: u64,
}

#[derive(Serialize)]
pub struct Stray {
    pub pgid: String,
    pub worktree: String,
    pub service: String,
}

#[derive(Serialize)]
pub struct Orphan {
    pub slot: u32,
    pub worktree: String,
    pub moved_to: String,
}

#[derive(Serialize)]
pub struct WorktreeJson {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub ticket: Option<String>,
    pub workflow: Option<Box<RawValue>>,
    pub primary: bool,
    pub dirty: bool,
    pub slot: Option<u32>,
    pub uptime: Option<String>,
    pub errors: Vec<ErrorJson>,
    pub services: Vec<ServiceJson>,
}

#[derive(Serialize)]
pub struct ErrorJson {
    pub service: String,
    pub log: Option<String>,
    pub message: String,
}

#[derive(Serialize)]
pub struct ServiceJson {
    pub name: String,
    pub port: u32,
    pub kind: String,
    pub state: String,
    pub log: Option<String>,
    pub mb: Option<u64>,
}

pub fn project_json() -> Result<String, String> {
    let config = Config::resolve()?;
    let project = project(&config)?;
    serde_json::to_string(&project).map_err(|error| error.to_string())
}

pub fn all_json() -> Result<String, String> {
    let mut text = String::from("[");
    let mut first = true;
    for root in projects() {
        let Ok(config) = Config::for_root(&root) else {
            continue;
        };
        let Ok(project) = project(&config) else {
            continue;
        };
        if !first {
            text.push(',');
        }
        first = false;
        text.push_str(&serde_json::to_string(&project).map_err(|e| e.to_string())?);
    }
    text.push(']');
    Ok(text)
}

fn projects() -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir = std::env::var("CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|_| format!("{home}/.claude"));
    let registry = std::env::var("RAMIFY_REGISTRY")
        .unwrap_or_else(|_| format!("{config_dir}/ramify/projects"));
    if let Ok(text) = std::fs::read_to_string(registry) {
        for line in text.lines() {
            let path = PathBuf::from(line.trim());
            if line.trim().is_empty() || !path.is_dir() {
                continue;
            }
            roots.insert(path);
        }
    }
    let state_root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    if let Ok(entries) = std::fs::read_dir(state_root) {
        for entry in entries.flatten() {
            let primary = entry.path().join("primary");
            if let Ok(text) = std::fs::read_to_string(primary) {
                let path = PathBuf::from(text.trim());
                if path.is_dir() {
                    roots.insert(path);
                }
            }
        }
    }
    roots.into_iter().collect()
}

fn project(config: &Config) -> Result<Project, String> {
    let text = git(&["worktree", "list", "--porcelain"], &config.primary)?;
    let mut worktrees = Vec::new();
    for path in text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|line| canonical(Path::new(line)))
    {
        let name = basename(&path);
        let env_file = config.state_dir.join(format!("{name}.env"));
        let slot = env_get(&env_file, "SLOT").and_then(|value| value.parse().ok());
        let branch = branch_of(&path);
        let ticket = ticket_of(&branch, config);
        let workflow = workflow::report(config, &path, &branch)
            .map(|progress| RawValue::from_string(workflow::progress_json(&progress)).unwrap());
        let dirty = !git(&["status", "--porcelain"], &path)
            .unwrap_or_default()
            .trim()
            .is_empty();
        let uptime = if env_file.is_file() {
            uptime_of(&env_file)
        } else {
            None
        };
        let errors = errors(&config.state_dir.join(format!("{name}.errors")));
        worktrees.push(WorktreeJson {
            name,
            path: path.to_string_lossy().into_owned(),
            branch,
            ticket,
            workflow,
            primary: path == config.primary,
            dirty,
            slot,
            uptime,
            errors,
            services: Vec::new(),
        });
    }
    Ok(Project {
        name: config.project.clone(),
        primary: config.primary.to_string_lossy().into_owned(),
        stacks: !config.services.is_empty(),
        jira: (!config.jira_url.is_empty())
            .then(|| config.jira_url.trim_end_matches('/').to_string()),
        containers: Vec::new(),
        shared: Vec::new(),
        build_servers: Vec::new(),
        strays: Vec::new(),
        orphans: Vec::new(),
        worktrees,
    })
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn branch_of(path: &Path) -> String {
    git(&["rev-parse", "--abbrev-ref", "HEAD"], path)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "?".into())
}

fn env_get(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .map(str::to_string)
}

fn ticket_of(branch: &str, config: &Config) -> Option<String> {
    if config.jira_url.is_empty() {
        return None;
    }
    let pattern = std::env::var("RAMIFY_TICKET_PATTERN")
        .unwrap_or_else(|_| "[A-Za-z][A-Za-z0-9]*-[0-9]+".into());
    let regex = regex::Regex::new(&format!("-({pattern})-")).ok()?;
    let text = format!("-{}-", branch.replace('/', "-"));
    regex
        .captures(&text)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_uppercase())
}

fn uptime_of(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let elapsed = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?
        .as_secs();
    if elapsed < 3600 {
        Some(format!("{}m", elapsed / 60))
    } else {
        Some(format!("{}h{:02}m", elapsed / 3600, (elapsed % 3600) / 60))
    }
}

fn errors(path: &Path) -> Vec<ErrorJson> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let service = parts.next()?.to_string();
            let log = parts.next().unwrap_or("").to_string();
            let message = parts.next().unwrap_or("").to_string();
            let log = (!log.is_empty() && Path::new(&log).is_file()).then_some(log);
            Some(ErrorJson {
                service,
                log,
                message,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_slot_line() {
        let dir = std::env::temp_dir().join(format!("ramify-env-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("x.env");
        std::fs::write(&file, "SLOT=2\nWEB_PORT=3002\nSLOT=3\n").unwrap();
        assert_eq!(env_get(&file, "SLOT").as_deref(), Some("3"));
    }

    #[test]
    fn formats_uptime() {
        let dir = std::env::temp_dir().join(format!("ramify-up-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("x");
        std::fs::write(&file, "").unwrap();
        let text = uptime_of(&file).expect("uptime");
        assert!(text.ends_with('m'), "{text}");
    }
}
