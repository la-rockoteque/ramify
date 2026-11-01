use crate::config::Config;
use serde::{Deserialize, Serialize};
use serde_yaml_ng::Value;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
struct Definitions {
    #[serde(default)]
    work_dir: Option<String>,
    #[serde(default)]
    record: Option<RecordDef>,
    #[serde(default)]
    workflows: std::collections::HashMap<String, WorkflowDef>,
    #[serde(default)]
    steps: std::collections::HashMap<String, StepDef>,
}

#[derive(Deserialize, Default, Clone)]
struct RecordDef {
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    workflow_field: Option<String>,
    #[serde(default)]
    slug_field: Option<String>,
}

#[derive(Deserialize, Default)]
struct WorkflowDef {
    #[serde(default)]
    branch: Option<String>,
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

struct OrderMeta {
    workflow: String,
    lane: String,
    branch: Option<String>,
    story: Option<String>,
    steps: Vec<OrderStep>,
    ask: Option<(String, Option<String>)>,
}

struct OrderStep {
    step: String,
    na: bool,
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
    let (path, meta) = find_order(root, &work_dir, branch, &definitions)?;
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
    let asked = meta.ask.as_ref().and_then(|(step, decision)| match decision.as_deref() {
        None | Some("open") => Some(step.clone()),
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

fn find_order(
    root: &Path,
    work_dir: &str,
    branch: &str,
    definitions: &Definitions,
) -> Option<(PathBuf, OrderMeta)> {
    let record = definitions.record.clone().unwrap_or_default();
    let filename = record
        .filename
        .clone()
        .unwrap_or_else(|| "order.md".into());
    for directory in directories(root, work_dir) {
        let path = directory.join(&filename);
        if !path.is_file() {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(value) = frontmatter(&text) else {
            continue;
        };
        let Some(meta) = extract(&value, &record) else {
            continue;
        };
        if matches_branch(&meta, branch, definitions) {
            return Some((path, meta));
        }
    }
    None
}

/// The record lives one level below `work_dir`, and `work_dir` can itself carry a `*`:
/// `docs/stories/*` means `docs/stories/*/*/story.md`.
fn directories(root: &Path, work_dir: &str) -> Vec<PathBuf> {
    let bases: Vec<PathBuf> = match work_dir.split_once('*') {
        Some((before, after)) => {
            let after = after.trim_start_matches('/');
            let Ok(entries) = std::fs::read_dir(root.join(before)) else {
                return Vec::new();
            };
            entries
                .flatten()
                .map(|entry| entry.path().join(after))
                .collect()
        }
        None => vec![root.join(work_dir)],
    };
    let mut directories = Vec::new();
    for base in bases {
        let Ok(entries) = std::fs::read_dir(base) else {
            continue;
        };
        directories.extend(entries.flatten().map(|entry| entry.path()));
    }
    directories
}

fn extract(value: &Value, record: &RecordDef) -> Option<OrderMeta> {
    let workflow_field = record
        .workflow_field
        .clone()
        .unwrap_or_else(|| "workflow".into());
    let slug_field = record
        .slug_field
        .clone()
        .unwrap_or_else(|| "story".into());
    let workflow = value.get(workflow_field.as_str())?.as_str()?.to_string();
    let lane = value.get("lane")?.as_str()?.to_string();
    let branch = value
        .get("branch")
        .and_then(Value::as_str)
        .map(str::to_string);
    let story = value
        .get(slug_field.as_str())
        .and_then(Value::as_str)
        .map(str::to_string);
    let steps = value
        .get("steps")
        .and_then(Value::as_sequence)
        .map(|sequence| {
            sequence
                .iter()
                .filter_map(|item| {
                    let step = item.get("step")?.as_str()?.to_string();
                    let na = item.get("na").and_then(Value::as_bool).unwrap_or(false);
                    Some(OrderStep { step, na })
                })
                .collect()
        })
        .unwrap_or_default();
    let ask = value.get("ask").and_then(|ask| {
        let step = ask.get("step")?.as_str()?.to_string();
        let decision = ask
            .get("decision")
            .and_then(Value::as_str)
            .map(str::to_string);
        Some((step, decision))
    });
    Some(OrderMeta {
        workflow,
        lane,
        branch,
        story,
        steps,
        ask,
    })
}

fn matches_branch(meta: &OrderMeta, branch: &str, definitions: &Definitions) -> bool {
    if let Some(recorded) = &meta.branch {
        return recorded == branch;
    }
    let Some(story) = &meta.story else {
        return false;
    };
    let Some(pattern) = definitions
        .workflows
        .get(&meta.workflow)
        .and_then(|workflow| workflow.branch.clone())
    else {
        return false;
    };
    pattern.replace("{slug}", story) == branch
}

fn frontmatter(text: &str) -> Option<Value> {
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

#[allow(clippy::items_after_test_module)]
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_frontmatter_only() {
        let text = "---\nworkflow: feature\nlane: standard\nbranch: feat/x\nsteps:\n- {step: spec}\n---\n\n# X\n";
        let value = frontmatter(text).expect("frontmatter");
        let meta = extract(&value, &RecordDef::default()).expect("meta");
        assert_eq!(meta.workflow, "feature");
        assert_eq!(meta.lane, "standard");
        assert_eq!(meta.steps.len(), 1);
    }

    #[test]
    fn reads_a_native_record() {
        let record = RecordDef {
            filename: Some("story.md".into()),
            workflow_field: Some("kind".into()),
            slug_field: Some("story".into()),
        };
        let text = "---\nstory: sse-preview\nlane: light\nkind: feature\nsteps:\n  - { step: qa-plan, on: 2026-10-24 }\n---\n";
        let value = frontmatter(text).expect("frontmatter");
        let meta = extract(&value, &record).expect("meta");
        assert_eq!(meta.workflow, "feature");
        assert_eq!(meta.story.as_deref(), Some("sse-preview"));
    }

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

/// Python's `json.dumps` separates with ", " and ": "; the shell embeds its output verbatim.
pub fn progress_json(progress: &Progress) -> String {
    let mut bytes = Vec::new();
    let mut serializer = serde_json::Serializer::with_formatter(&mut bytes, PythonFormatter);
    progress
        .serialize(&mut serializer)
        .expect("progress serializes");
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
