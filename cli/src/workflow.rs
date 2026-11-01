use crate::config::Config;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
struct Definitions {
    #[serde(default)]
    work_dir: Option<String>,
    #[serde(default)]
    workflows: std::collections::HashMap<String, WorkflowDef>,
    #[serde(default)]
    steps: std::collections::HashMap<String, StepDef>,
}

#[derive(Deserialize, Default)]
struct WorkflowDef {
    #[serde(default)]
    lanes: std::collections::HashMap<String, Vec<String>>,
}

#[derive(Deserialize, Default)]
struct StepDef {
    #[serde(default)]
    skill: Option<String>,
    #[serde(default)]
    gate: Option<String>,
}

#[derive(Deserialize)]
struct OrderMeta {
    workflow: String,
    lane: String,
    #[serde(default)]
    branch: Option<String>,
    #[serde(default)]
    steps: Vec<OrderStep>,
    #[serde(default)]
    ask: Option<Ask>,
}

#[derive(Deserialize)]
struct OrderStep {
    step: String,
    #[serde(default)]
    na: bool,
}

#[derive(Deserialize)]
struct Ask {
    step: String,
    #[serde(default)]
    decision: Option<String>,
}

#[derive(Serialize)]
pub struct NextStep {
    pub step: String,
    pub skill: Option<String>,
    pub gate: String,
}

#[derive(Serialize)]
pub struct StepState {
    pub step: String,
    pub state: String,
}

#[derive(Serialize)]
pub struct Progress {
    pub workflow: String,
    pub lane: String,
    pub order: String,
    pub done: usize,
    pub total: usize,
    pub next: Option<NextStep>,
    pub asked: Option<String>,
    pub steps: Vec<StepState>,
}

pub fn report(cfg: &Config, root: &Path, branch: &str) -> Option<Progress> {
    if !root.join(".ramify").is_dir() && !cfg.primary.join(".ramify").is_dir() {
        return None;
    }
    let definitions = definitions(cfg)?;
    let work_dir = definitions
        .work_dir
        .clone()
        .unwrap_or_else(|| ".ramify/work".into());
    let (path, meta) = find_order(root, &work_dir, branch)?;
    let workflow = definitions.workflows.get(&meta.workflow)?;
    let lane = workflow.lanes.get(&meta.lane)?;

    let recorded: Vec<&str> = meta.steps.iter().map(|entry| entry.step.as_str()).collect();
    let pending: Vec<&String> = lane
        .iter()
        .filter(|step| !recorded.contains(&step.as_str()))
        .collect();
    let next = pending.first().map(|step| step.as_str());
    let next_step = next.map(|step| {
        let definition = definitions.steps.get(step);
        NextStep {
            step: step.to_string(),
            skill: definition.and_then(|def| def.skill.clone()),
            gate: definition
                .and_then(|def| def.gate.clone())
                .unwrap_or_else(|| "auto".into()),
        }
    });
    let asked = meta.ask.as_ref().and_then(|ask| match ask.decision.as_deref() {
        None | Some("open") => Some(ask.step.clone()),
        _ => None,
    });
    let states = lane
        .iter()
        .map(|step| {
            let state = if let Some(entry) = meta.steps.iter().find(|entry| &entry.step == step) {
                if entry.na { "na" } else { "done" }
            } else if next == Some(step.as_str()) {
                "next"
            } else {
                "todo"
            };
            StepState {
                step: step.clone(),
                state: state.into(),
            }
        })
        .collect();
    let relative = path
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned());

    Some(Progress {
        workflow: meta.workflow.clone(),
        lane: meta.lane.clone(),
        order: relative,
        done: lane.len().saturating_sub(pending.len()),
        total: lane.len(),
        next: next_step,
        asked,
        steps: states,
    })
}

fn definitions(cfg: &Config) -> Option<Definitions> {
    let file = workflows_file(cfg)?;
    let text = std::fs::read_to_string(file).ok()?;
    serde_yaml_ng::from_str(&text).ok()
}

fn workflows_file(cfg: &Config) -> Option<PathBuf> {
    for base in [&cfg.repo_root, &cfg.primary] {
        let path = base.join(".ramify/workflows.yml");
        if path.is_file() {
            return Some(path);
        }
    }
    let path = cfg.home.join("workflows.yml");
    path.is_file().then_some(path)
}

fn find_order(root: &Path, work_dir: &str, branch: &str) -> Option<(PathBuf, OrderMeta)> {
    let directory = root.join(work_dir);
    let entries = std::fs::read_dir(directory).ok()?;
    for entry in entries.flatten() {
        let path = entry.path().join("order.md");
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(meta) = frontmatter(&text) else {
            continue;
        };
        if meta.branch.as_deref() == Some(branch) {
            return Some((path, meta));
        }
    }
    None
}

fn frontmatter(text: &str) -> Option<OrderMeta> {
    let mut lines = text.lines();
    if lines.next()?.trim() != "---" {
        return None;
    }
    let mut yaml = String::new();
    for line in lines {
        if line.trim() == "---" {
            return serde_yaml_ng::from_str(&yaml).ok();
        }
        yaml.push_str(line);
        yaml.push('\n');
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_frontmatter_only() {
        let text = "---\nworkflow: feature\nlane: standard\nbranch: feat/x\nsteps:\n- {step: spec}\n---\n\n# X\n";
        let meta = frontmatter(text).expect("frontmatter");
        assert_eq!(meta.workflow, "feature");
        assert_eq!(meta.lane, "standard");
        assert_eq!(meta.steps.len(), 1);
    }
}

#[cfg(test)]
mod repo_tests {
    use super::*;

    #[test]
    fn parses_the_repo_workflows_file() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../workflows.yml"
        ))
        .unwrap();
        let defs: Definitions = serde_yaml_ng::from_str(&text).expect("yaml");
        assert!(defs.workflows.contains_key("feature"), "no feature");
    }
}

#[cfg(test)]
mod order_tests {
    use super::*;

    #[test]
    fn parses_a_real_order() {
        let text = std::fs::read_to_string(concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../.ramify/work/rust-cli/order.md"
        ))
        .unwrap();
        let meta = frontmatter(&text).expect("frontmatter");
        assert_eq!(meta.branch.as_deref(), Some("feat/rust-cli"));
        assert_eq!(meta.workflow, "feature");
    }
}

/// Python's `json.dumps` separates with ", " and ": "; the shell embeds its output verbatim.
pub fn progress_json(progress: &Progress) -> String {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, PythonFormatter);
    progress.serialize(&mut serializer).expect("progress serializes");
    String::from_utf8(bytes).expect("json is UTF-8")
}

struct PythonFormatter;

impl serde_json::ser::Formatter for PythonFormatter {
    fn begin_array_value<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if !first {
            writer.write_all(b", ")?;
        }
        Ok(())
    }

    fn begin_object_key<W>(&mut self, writer: &mut W, first: bool) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        if !first {
            writer.write_all(b", ")?;
        }
        Ok(())
    }

    fn begin_object_value<W>(&mut self, writer: &mut W) -> std::io::Result<()>
    where
        W: ?Sized + std::io::Write,
    {
        writer.write_all(b": ")
    }
}
