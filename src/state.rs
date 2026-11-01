use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub primary: PathBuf,
    pub worktrees: Vec<Worktree>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Worktree {
    pub name: String,
    pub path: PathBuf,
    pub branch: String,
    pub primary: bool,
    pub dirty: bool,
    pub slot: Option<u32>,
    pub services: Vec<Service>,
    pub workflow: Option<Workflow>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Service {
    pub service: String,
    pub port: u16,
    pub up: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Workflow {
    pub name: String,
    pub lane: String,
    pub next: Option<Step>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Step {
    pub step: String,
    pub skill: String,
    pub gate: Option<String>,
}

#[derive(Deserialize)]
struct RawProject {
    name: String,
    primary: PathBuf,
    #[serde(default)]
    worktrees: Vec<RawWorktree>,
}

#[derive(Deserialize)]
struct RawWorktree {
    name: String,
    path: PathBuf,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    primary: bool,
    #[serde(default)]
    dirty: bool,
    #[serde(default)]
    slot: Option<u32>,
    #[serde(default)]
    services: Vec<RawService>,
    #[serde(default)]
    workflow: Option<RawWorkflow>,
}

#[derive(Deserialize)]
struct RawService {
    #[serde(alias = "service")]
    name: String,
    #[serde(default)]
    port: Option<u16>,
    #[serde(default)]
    up: bool,
    #[serde(default)]
    state: Option<String>,
}

#[derive(Deserialize)]
struct RawWorkflow {
    workflow: String,
    #[serde(default)]
    lane: Option<String>,
    #[serde(default)]
    next: Option<RawStep>,
}

#[derive(Deserialize)]
struct RawStep {
    step: String,
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    gate: Option<String>,
}

pub fn parse_dash(json: &str) -> Result<Vec<Project>, String> {
    let raw: Vec<RawProject> =
        serde_json::from_str(json).map_err(|e| format!("invalid JSON: {e}"))?;
    Ok(raw
        .into_iter()
        .map(|project| Project {
            name: project.name,
            primary: project.primary,
            worktrees: project
                .worktrees
                .into_iter()
                .map(|wt| Worktree {
                    name: wt.name,
                    path: wt.path,
                    branch: wt.branch.unwrap_or_default(),
                    primary: wt.primary,
                    dirty: wt.dirty,
                    slot: wt.slot,
                    services: wt
                        .services
                        .into_iter()
                        .map(|s| Service {
                            service: s.name,
                            port: s.port.unwrap_or(0),
                            up: s.up || s.state.as_deref() == Some("ready"),
                        })
                        .collect(),
                    workflow: wt.workflow.map(|w| Workflow {
                        name: w.workflow,
                        lane: w.lane.unwrap_or_default(),
                        next: w.next.map(|s| Step {
                            step: s.step,
                            skill: s.skill.unwrap_or_default(),
                            gate: s.gate,
                        }),
                    }),
                })
                .collect(),
        })
        .collect())
}

pub fn project_for_root<'a>(projects: &'a [Project], root: &Path) -> Option<&'a Project> {
    let root = canonical(root);
    projects.iter().find(|project| {
        canonical(&project.primary) == root
            || project
                .worktrees
                .iter()
                .any(|wt| canonical(&wt.path) == root)
    })
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}
