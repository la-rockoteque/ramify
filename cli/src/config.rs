use sha1::{Digest, Sha1};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct Config {
    pub repo_root: PathBuf,
    pub primary: PathBuf,
    pub worktree: String,
    pub project: String,
    pub state_dir: PathBuf,
    pub slots: PathBuf,
    pub max_slot: u32,
    pub services: Vec<String>,
    pub containers: Vec<String>,
    pub jira_url: String,
    pub main: String,
    pub remote: String,
    pub branch_prefix: String,
    pub workflow_source: String,
    pub home: PathBuf,
    pub config_path: PathBuf,
    pub worktrees: String,
    pub qa_plan: String,
    pub ticket_pattern: String,
    pub workflow_docs: String,
    pub session_names: String,
    pub values: HashMap<String, String>,
}

impl Config {
    pub fn resolve() -> Result<Self, String> {
        let repo_root = match std::env::var_os("RAMIFY_ROOT") {
            Some(root) => PathBuf::from(root),
            None => detect_repo_root()?,
        };
        Self::for_root(&repo_root)
    }

    pub fn value(&self, key: &str) -> Option<String> {
        if let Some(value) = self.values.get(key) {
            return Some(value.clone());
        }
        if let Ok(value) = std::env::var(key) {
            return Some(value);
        }
        match key {
            "RAMIFY_PROJECT" => Some(self.project.clone()),
            "RAMIFY_REMOTE" => Some(self.remote.clone()),
            "RAMIFY_MAIN" => Some(self.main.clone()),
            "RAMIFY_WORKTREES" => Some(self.worktrees.clone()),
            "RAMIFY_BRANCH_PREFIX" => Some(self.branch_prefix.clone()),
            "RAMIFY_SERVICES" => Some(self.services.join(" ")),
            "RAMIFY_CONTAINERS" => Some(self.containers.join(" ")),
            "RAMIFY_QA_PLAN" => Some(self.qa_plan.clone()),
            "RAMIFY_JIRA_URL" => Some(self.jira_url.clone()),
            "RAMIFY_JIRA_DONE" => Some(String::new()),
            "RAMIFY_JIRA_PROGRESS" => Some("In Progress".into()),
            "RAMIFY_JIRA_REVIEW" => Some(String::new()),
            "RAMIFY_JIRA_SYNC" => Some("1".into()),
            "RAMIFY_TICKET_PATTERN" => Some(self.ticket_pattern.clone()),
            "RAMIFY_WORKFLOW_SOURCE" => Some(self.workflow_source.clone()),
            "RAMIFY_WORKFLOW_DOCS" => Some(self.workflow_docs.clone()),
            "RAMIFY_SESSION_NAMES" => Some(self.session_names.clone()),
            "RAMIFY_MAX_SLOT" => Some(self.max_slot.to_string()),
            _ => None,
        }
    }

    /// A per-service setting, such as `web_port` or `web_health`.
    pub fn service(&self, service: &str, key: &str) -> Option<String> {
        let name = format!("{service}_{key}");
        self.values
            .get(&name)
            .cloned()
            .or_else(|| std::env::var(&name).ok())
            .filter(|value| !value.is_empty())
    }

    pub fn for_root(root: &Path) -> Result<Self, String> {
        let repo_root = canonical(root);
        let primary = primary_of(&repo_root)?;
        let worktree = basename(&repo_root);
        let file = match config_file(&repo_root, &primary) {
            Some(file) => file,
            None => {
                let second = if repo_root != primary {
                    format!(" or {}", repo_root.display())
                } else {
                    String::new()
                };
                return Err(format!(
                    "no .ramify.conf or .claude/ramify.conf in {}{second} — run 'ramify setup'",
                    primary.display()
                ));
            }
        };
        let text = std::fs::read_to_string(&file)
            .map_err(|error| format!("could not read {}: {error}", file.display()))?;
        let values = parse_conf(&text);
        let get = |key: &str| -> Option<String> {
            values
                .get(key)
                .cloned()
                .or_else(|| std::env::var(key).ok())
        };
        let get_or = |key: &str, default: &str| get(key).unwrap_or_else(|| default.to_string());

        let project = get("RAMIFY_PROJECT").unwrap_or_else(|| basename(&primary));
        let worktrees = get_or("RAMIFY_WORKTREES", &format!("../{}-wt", basename(&primary)));
        let state_root = get("RAMIFY_STATE_DIR").unwrap_or_else(|| "/tmp/ramify".into());
        let mut state_dir = PathBuf::from(&state_root).join(&project);
        let twin = PathBuf::from(format!(
            "{}-{}",
            state_dir.display(),
            short_hash(&primary)
        ));
        let claim = std::fs::read_to_string(state_dir.join("primary"))
            .ok()
            .map(|text| text.trim().to_string());
        let claimed_elsewhere = claim
            .as_deref()
            .is_some_and(|claim| claim != primary.to_string_lossy() && Path::new(claim).is_dir());
        if twin.join("primary").is_file() || claimed_elsewhere {
            state_dir = twin;
        }
        let slots = state_dir.join("slots");
        let home = match std::env::var_os("RAMIFY_HOME") {
            Some(home) => PathBuf::from(home),
            None => detect_home()?,
        };
        let values = values.clone();
        Ok(Self {
            repo_root,
            primary,
            worktree,
            project,
            state_dir,
            slots,
            max_slot: get_or("RAMIFY_MAX_SLOT", "5").parse().unwrap_or(5),
            services: words(&get_or("RAMIFY_SERVICES", "")),
            containers: words(&get_or("RAMIFY_CONTAINERS", "")),
            jira_url: get_or("RAMIFY_JIRA_URL", ""),
            main: get_or("RAMIFY_MAIN", "main"),
            remote: get_or("RAMIFY_REMOTE", "origin"),
            branch_prefix: get_or("RAMIFY_BRANCH_PREFIX", ""),
            workflow_source: get_or("RAMIFY_WORKFLOW_SOURCE", "auto"),
            home,
            config_path: file.clone(),
            worktrees,
            qa_plan: get_or("RAMIFY_QA_PLAN", ""),
            ticket_pattern: get_or("RAMIFY_TICKET_PATTERN", "[A-Za-z][A-Za-z0-9]*-[0-9]+"),
            workflow_docs: get_or("RAMIFY_WORKFLOW_DOCS", ""),
            session_names: get_or("RAMIFY_SESSION_NAMES", "trees"),
            values,
        })
    }
}

fn words(value: &str) -> Vec<String> {
    value.split_whitespace().map(str::to_string).collect()
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn short_hash(path: &Path) -> String {
    let mut hasher = Sha1::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest[..4].iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn git(args: &[&str], cwd: &Path) -> Result<String, String> {
    let output = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .map_err(|error| format!("could not run git: {error}"))?;
    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).trim().to_string());
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

fn detect_repo_root() -> Result<PathBuf, String> {
    let cwd = std::env::current_dir().map_err(|error| error.to_string())?;
    git(&["rev-parse", "--show-toplevel"], &cwd)
        .map(|text| PathBuf::from(text.trim()))
        .map_err(|_| "not inside a git checkout (or set RAMIFY_ROOT)".to_string())
}

fn primary_of(repo_root: &Path) -> Result<PathBuf, String> {
    let text = git(&["worktree", "list", "--porcelain"], repo_root)?;
    text.lines()
        .find_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .ok_or_else(|| format!("{} is not a git checkout", repo_root.display()))
}

fn config_file(repo_root: &Path, primary: &Path) -> Option<PathBuf> {
    for root in [repo_root, primary] {
        for name in [".ramify.conf", ".claude/ramify.conf"] {
            let path = root.join(name);
            if path.is_file() {
                return Some(path);
            }
        }
    }
    None
}

fn detect_home() -> Result<PathBuf, String> {
    let mut current = std::env::current_exe().map_err(|error| error.to_string())?;
    while current.pop() {
        if current.join("bin/ramify").is_file() && current.join("workflows.yml").is_file() {
            return Ok(current);
        }
    }
    Err("could not find RAMIFY_HOME".into())
}

/// Read the `KEY=value` lines of a bash config. Single quotes keep the text as it is.
fn parse_conf(text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        let key = key.trim();
        if key.is_empty() || !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            continue;
        }
        let value = value.trim();
        let value = value
            .strip_prefix('\'')
            .and_then(|inner| inner.strip_suffix('\''))
            .or_else(|| {
                value
                    .strip_prefix('"')
                    .and_then(|inner| inner.strip_suffix('"'))
            })
            .unwrap_or(value);
        values.insert(key.to_string(), value.to_string());
    }
    values
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_assignments_and_quotes() {
        let text = "# comment\nRAMIFY_SERVICES=\"web api\"\nRAMIFY_BOOTSTRAP='(cd app && npm ci)'\napp_port=3010\n";
        let values = parse_conf(text);
        assert_eq!(values["RAMIFY_SERVICES"], "web api");
        assert_eq!(values["RAMIFY_BOOTSTRAP"], "(cd app && npm ci)");
        assert_eq!(values["app_port"], "3010");
        assert_eq!(words("web  api"), vec!["web", "api"]);
    }
}
