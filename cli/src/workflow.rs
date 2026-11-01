use crate::config::Config;
use serde::{Deserialize, Serialize};
use indexmap::IndexMap;
use serde_yaml_ng::Value;
use std::path::{Path, PathBuf};

#[derive(Deserialize, Default)]
pub(crate) struct Definitions {
    #[serde(default)]
    pub(crate) work_dir: Option<String>,
    #[serde(default)]
    pub(crate) record: Option<RecordDef>,
    #[serde(default)]
    pub(crate) workflows: IndexMap<String, WorkflowDef>,
    #[serde(default)]
    pub(crate) steps: std::collections::HashMap<String, StepDef>,
}

#[derive(Deserialize, Default, Clone)]
pub(crate) struct RecordDef {
    #[serde(default)]
    filename: Option<String>,
    #[serde(default)]
    workflow_field: Option<String>,
    #[serde(default)]
    slug_field: Option<String>,
    #[serde(default)]
    date_field: Option<String>,
    #[serde(default)]
    note_field: Option<String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct WorkflowDef {
    #[serde(default)]
    pub(crate) does: Option<String>,
    #[serde(default)]
    pub(crate) branch: Option<String>,
    #[serde(default)]
    pub(crate) default_lane: Option<String>,
    #[serde(default)]
    pub(crate) lanes: IndexMap<String, Vec<String>>,
    #[serde(default)]
    pub(crate) min_lane: IndexMap<String, String>,
}

#[derive(Deserialize, Default)]
pub(crate) struct StepDef {
    #[serde(default)]
    pub(crate) skill: Option<String>,
    #[serde(default)]
    pub(crate) gate: Option<String>,
    #[serde(default)]
    pub(crate) does: Option<String>,
    #[serde(default)]
    pub(crate) files: Option<Vec<String>>,
}

struct OrderMeta {
    workflow: String,
    lane: String,
    branch: Option<String>,
    story: Option<String>,
    steps: Vec<OrderStep>,
    ask: Option<Ask>,
}

struct OrderStep {
    step: String,
    na: bool,
    date: Option<String>,
    note: Option<String>,
}

struct Ask {
    step: String,
    asked: Option<String>,
    files: Vec<String>,
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
    let asked = meta
        .ask
        .as_ref()
        .filter(|ask| ask.decision.is_none())
        .map(|ask| ask.step.clone());
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

pub(crate) fn definitions(cfg: &Config) -> Option<Definitions> {
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
    let date_field = record.date_field.clone().unwrap_or_else(|| "date".into());
    let note_field = record.note_field.clone().unwrap_or_else(|| "note".into());
    let steps = value
        .get("steps")
        .and_then(Value::as_sequence)
        .map(|sequence| {
            sequence
                .iter()
                .filter_map(|item| {
                    let step = item.get("step")?.as_str()?.to_string();
                    let na = item.get("na").and_then(Value::as_bool).unwrap_or(false);
                    let date = item
                        .get(date_field.as_str())
                        .map(scalar)
                        .filter(|value| !value.is_empty());
                    let note = item
                        .get(note_field.as_str())
                        .map(scalar)
                        .filter(|value| !value.is_empty());
                    Some(OrderStep {
                        step,
                        na,
                        date,
                        note,
                    })
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
        let asked = ask.get("asked").map(scalar).filter(|value| !value.is_empty());
        let files = ask
            .get("files")
            .and_then(Value::as_sequence)
            .map(|files| {
                files
                    .iter()
                    .filter_map(Value::as_str)
                    .map(str::to_string)
                    .collect()
            })
            .unwrap_or_default();
        Some(Ask {
            step,
            asked,
            files,
            decision,
        })
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

fn scalar(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        Value::Number(number) => number.to_string(),
        Value::Bool(flag) => flag.to_string(),
        _ => String::new(),
    }
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
pub fn order_text(cfg: &Config) -> Result<String, String> {
    let definitions = definitions(cfg).ok_or("no workflows are defined")?;
    let work_dir = definitions
        .work_dir
        .clone()
        .unwrap_or_else(|| ".ramify/work".into());
    let branch = std::env::var("WF_BRANCH")
        .ok()
        .or_else(|| crate::config::git(&["rev-parse", "--abbrev-ref", "HEAD"], &cfg.repo_root).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    let (path, meta) = find_order(&cfg.repo_root, &work_dir, &branch, &definitions)
        .ok_or_else(|| format!("no work order for branch {branch}"))?;
    let workflow = definitions
        .workflows
        .get(&meta.workflow)
        .ok_or_else(|| format!("no workflow '{}'", meta.workflow))?;
    let lane = workflow
        .lanes
        .get(&meta.lane)
        .ok_or_else(|| format!("lane '{}' is not one of {}'s", meta.lane, meta.workflow))?;

    let recorded: Vec<&str> = meta.steps.iter().map(|entry| entry.step.as_str()).collect();
    let pending: Vec<&String> = lane
        .iter()
        .filter(|step| !recorded.contains(&step.as_str()))
        .collect();
    let next = pending.first().map(|step| step.as_str());
    let relative = path
        .strip_prefix(&cfg.repo_root)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
    let prefix = if definitions.record.is_some() {
        "repository "
    } else {
        ""
    };
    let mut lines = vec![format!(
        "{prefix}{} · {} lane · {relative}",
        meta.workflow, meta.lane
    )];
    for step in lane {
        if let Some(entry) = meta.steps.iter().find(|entry| &entry.step == step) {
            let date = entry.date.clone().unwrap_or_default();
            let na = if entry.na { "  n/a" } else { "" };
            let note = entry
                .note
                .as_ref()
                .map(|note| format!("  {note}"))
                .unwrap_or_default();
            lines.push(format!("  ✓ {step:<10} {date}{na}{note}"));
        } else if next == Some(step.as_str()) {
            let definition = definitions.steps.get(step);
            let skill = definition
                .and_then(|def| def.skill.clone())
                .unwrap_or_default();
            let does = definition
                .and_then(|def| def.does.clone())
                .unwrap_or_default();
            let folder = path.parent().unwrap_or(&cfg.repo_root);
            let gate = if definition.and_then(|def| def.gate.as_deref()) == Some("human") {
                gate_note(definition, &meta, step, folder, &cfg.repo_root)
            } else {
                String::new()
            };
            lines.push(format!("  → {step:<10} {skill} — {does}{gate}"));
        } else {
            lines.push(format!("  · {step}"));
        }
    }
    if next.is_none() {
        lines.push("  every step is recorded".into());
    }
    Ok(lines.join("\n"))
}

fn gate_note(
    definition: Option<&StepDef>,
    meta: &OrderMeta,
    step: &str,
    folder: &Path,
    root: &Path,
) -> String {
    if meta
        .ask
        .as_ref()
        .is_some_and(|ask| ask.step == step && ask.decision.is_none())
    {
        return format!(
            "  (human gate: asked, waits on `ramify approve {step}` or the approvals window)"
        );
    }
    let names = definition
        .and_then(|def| def.files.clone())
        .unwrap_or_else(|| vec![format!("{step}.md")]);
    let missing: Vec<String> = names
        .iter()
        .filter(|file| !folder.join(file).is_file())
        .map(|file| {
            folder
                .join(file)
                .strip_prefix(root)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| file.clone())
        })
        .collect();
    if !missing.is_empty() {
        return format!(
            "  (human gate: write {}, then Ramify asks)",
            missing.join(", ")
        );
    }
    "  (human gate: record it only after the user's explicit OK)".into()
}

pub fn asked_json(cfg: &Config) -> Result<String, String> {
    let definitions = definitions(cfg).ok_or("no workflows are defined")?;
    let work_dir = definitions
        .work_dir
        .clone()
        .unwrap_or_else(|| ".ramify/work".into());
    let branch = std::env::var("WF_BRANCH")
        .ok()
        .or_else(|| crate::config::git(&["rev-parse", "--abbrev-ref", "HEAD"], &cfg.repo_root).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    let Some((_path, meta)) = find_order(&cfg.repo_root, &work_dir, &branch, &definitions) else {
        return Ok("null".into());
    };
    let Some(ask) = meta.ask.as_ref().filter(|ask| ask.decision.is_none()) else {
        return Ok("null".into());
    };
    let step = ask.step.clone();
    let does = definitions
        .steps
        .get(&step)
        .and_then(|definition| definition.does.clone());
    let order_branch = meta.branch.clone().or_else(|| {
        let story = meta.story.clone()?;
        let pattern = definitions
            .workflows
            .get(&meta.workflow)?
            .branch
            .clone()?;
        Some(pattern.replace("{slug}", &story))
    });
    let root = cfg.repo_root.canonicalize().unwrap_or_else(|_| cfg.repo_root.clone());
    let files: Vec<serde_json::Value> = ask
        .files
        .iter()
        .map(|relative| {
            let full = cfg.repo_root.join(relative);
            let canonical = full.canonicalize().unwrap_or_else(|_| full.clone());
            let text = if !canonical.starts_with(&root) {
                format!("(cannot read {relative}: outside the worktree)")
            } else {
                match std::fs::read(&canonical) {
                    Ok(bytes) => {
                        let limit = 512 << 10;
                        let slice = &bytes[..bytes.len().min(limit)];
                        String::from_utf8_lossy(slice).into_owned()
                    }
                    Err(error) => format!("(cannot read {relative}: {error})"),
                }
            };
            serde_json::json!({"path": relative, "text": text})
        })
        .collect();
    let value = serde_json::json!({
        "step": step,
        "does": does,
        "asked": ask.asked,
        "workflow": meta.workflow,
        "lane": meta.lane,
        "branch": order_branch,
        "files": files,
    });
    Ok(crate::pyjson::to_string(&value))
}

pub struct BranchPlan {
    pub branch: String,
    pub lane: String,
    pub notes: Vec<String>,
}

pub fn plan_branch(
    config: &Config,
    flow: &str,
    slug: &str,
    lane: Option<&str>,
    tags: &[String],
) -> Result<BranchPlan, String> {
    if !valid_slug(slug) {
        return Err(format!(
            "'{slug}' is not a slug: letters, digits, '.', '_' and '-' only"
        ));
    }
    let definitions = definitions(config).ok_or("no workflows are defined")?;
    let workflow = definitions
        .workflows
        .get(flow)
        .ok_or_else(|| {
            format!(
                "no workflow '{flow}' — one of: {}",
                definitions
                    .workflows
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ")
            )
        })?;
    let branch = workflow
        .branch
        .clone()
        .unwrap_or_else(|| "{slug}".into())
        .replace("{slug}", slug);
    let mut lane = lane
        .map(str::to_string)
        .or_else(|| workflow.default_lane.clone())
        .or_else(|| workflow.lanes.keys().next().cloned())
        .unwrap_or_default();
    if !workflow.lanes.contains_key(&lane) {
        return Err(format!(
            "no lane '{lane}' in {flow} — one of: {}",
            workflow.lanes.keys().cloned().collect::<Vec<_>>().join(", ")
        ));
    }
    let mut notes = Vec::new();
    let order: Vec<&String> = workflow.lanes.keys().collect();
    for tag in tags {
        let lower = tag.to_lowercase();
        match workflow.min_lane.get(&lower) {
            Some(floor) if floor != &lane => {
                let floor_rank = order.iter().position(|name| *name == floor).unwrap_or(0);
                let lane_rank = order.iter().position(|name| name == &&lane).unwrap_or(0);
                if floor_rank > lane_rank {
                    lane = floor.clone();
                }
            }
            None => {
                let known = workflow
                    .min_lane
                    .keys()
                    .cloned()
                    .collect::<Vec<_>>()
                    .join(", ");
                notes.push(format!(
                    "  tag             '{lower}' sets no lane floor in {flow} (those that do: {})",
                    if known.is_empty() { "none".to_string() } else { known }
                ));
            }
            _ => {}
        }
    }
    check_slug(config, &definitions, slug)?;
    Ok(BranchPlan {
        branch,
        lane,
        notes,
    })
}

fn valid_slug(slug: &str) -> bool {
    let mut chars = slug.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphanumeric() => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_' || c == '-')
}

fn check_slug(config: &Config, definitions: &Definitions, slug: &str) -> Result<(), String> {
    let work_dir = definitions
        .work_dir
        .clone()
        .unwrap_or_else(|| ".ramify/work".into());
    let filename = definitions
        .record
        .as_ref()
        .and_then(|record| record.filename.clone())
        .unwrap_or_else(|| "order.md".into());
    let relative = format!("{work_dir}/{slug}/{filename}");
    let spec = format!("{}/{}:{relative}", config.remote, config.main);
    let status = std::process::Command::new("git")
        .args(["-C", &config.primary.to_string_lossy(), "cat-file", "-e", &spec])
        .status();
    if matches!(status, Ok(status) if status.success()) {
        return Err(format!(
            "{}/{} already has {relative} — pick another slug",
            config.remote, config.main
        ));
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub fn start_order(
    root: &Path,
    definitions_root: &Config,
    flow: &str,
    slug: &str,
    lane: &str,
    tags: &[String],
    notes: &[String],
    branch: &str,
) -> Result<Vec<String>, String> {
    let config = config_for(root, definitions_root)?;
    let definitions = definitions(&config).ok_or("no workflows are defined")?;
    if definitions.record.is_some() {
        return Err("this repository starts native records itself".into());
    }
    let work_dir = definitions
        .work_dir
        .clone()
        .unwrap_or_else(|| ".ramify/work".into());
    let folder = root.join(&work_dir).join(slug);
    let path = folder.join("order.md");
    if path.exists() {
        return Err(format!(
            "{} already exists — pick another slug",
            path.strip_prefix(root)
                .map(|path| path.to_string_lossy().into_owned())
                .unwrap_or_else(|_| path.to_string_lossy().into_owned())
        ));
    }
    std::fs::create_dir_all(&folder)
        .map_err(|error| format!("could not create {}: {error}", folder.display()))?;
    let created = std::process::Command::new("date")
        .arg("+%F")
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    let tags_text = if tags.is_empty() {
        "[]".to_string()
    } else {
        format!("[{}]", tags.join(", "))
    };
    let body = format!(
        "---\nworkflow: {flow}\nlane: {lane}\nbranch: {branch}\ntags: {tags_text}\ncreated: {created}\nsteps: []\n---\n\n# {slug}\n\nWhat this changes and why.\n"
    );
    std::fs::write(&path, body)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    let relative = path
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| path.to_string_lossy().into_owned());
    let mut lines = vec![format!(
        "  work order      {relative} ({flow}, {lane} lane)"
    )];
    lines.extend(notes.iter().cloned());
    Ok(lines)
}

fn config_for(root: &Path, fallback: &Config) -> Result<Config, String> {
    Config::for_root(root).or_else(|_| Ok::<Config, String>(fallback.clone()))
}

pub fn lane_text(args: &[String]) -> Result<String, String> {
    let Some(lane) = args.first() else {
        return Err("usage: ramify lane <lane>".into());
    };
    let config = Config::resolve()?;
    let definitions = definitions(&config).ok_or("no workflows are defined")?;
    let work_dir = definitions
        .work_dir
        .clone()
        .unwrap_or_else(|| ".ramify/work".into());
    let branch = std::env::var("WF_BRANCH")
        .ok()
        .or_else(|| crate::config::git(&["rev-parse", "--abbrev-ref", "HEAD"], &config.repo_root).ok())
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    let (path, meta) = find_order(&config.repo_root, &work_dir, &branch, &definitions)
        .ok_or_else(|| format!("no work order for branch {branch}"))?;
    let workflow = definitions
        .workflows
        .get(&meta.workflow)
        .ok_or_else(|| format!("no workflow '{}'", meta.workflow))?;
    if !workflow.lanes.contains_key(lane) {
        let names: Vec<&str> = workflow.lanes.keys().map(String::as_str).collect();
        return Err(format!(
            "no lane '{lane}' in {} — one of: {}",
            meta.workflow,
            names.join(", ")
        ));
    }
    let order: Vec<&String> = workflow.lanes.keys().collect();
    let current = order
        .iter()
        .position(|name| *name == &meta.lane)
        .unwrap_or(0);
    let wanted = order
        .iter()
        .position(|name| *name == lane)
        .unwrap_or(0);
    if wanted < current {
        return Err(format!(
            "{lane} is lighter than {}: a lane is raised, never lowered",
            meta.lane
        ));
    }
    if lane == &meta.lane {
        return Ok(format!("  already on the {lane} lane"));
    }
    let text = std::fs::read_to_string(&path)
        .map_err(|error| format!("could not read {}: {error}", path.display()))?;
    let updated = written_lane(&text, lane)
        .ok_or_else(|| format!("{} has no lane line", path.display()))?;
    std::fs::write(&path, updated)
        .map_err(|error| format!("could not write {}: {error}", path.display()))?;
    let lane_steps = workflow.lanes.get(lane).cloned().unwrap_or_default();
    let recorded: Vec<&str> = meta.steps.iter().map(|entry| entry.step.as_str()).collect();
    let next = lane_steps
        .iter()
        .find(|step| !recorded.contains(&step.as_str()))
        .cloned()
        .unwrap_or_else(|| "nothing".into());
    Ok(format!("  lane raised to {lane} — next: {next}"))
}

fn written_lane(text: &str, lane: &str) -> Option<String> {
    let mut lines: Vec<String> = text.lines().map(str::to_string).collect();
    let mut fences = 0;
    let mut replaced = false;
    for line in &mut lines {
        if line.trim() == "---" {
            fences += 1;
            if fences == 2 {
                break;
            }
            continue;
        }
        if fences == 1 && line.starts_with("lane:") && !replaced {
            *line = format!("lane: {lane}");
            replaced = true;
        }
    }
    if !replaced {
        return None;
    }
    Some(format!("{}\n", lines.join("\n")))
}

pub fn workflow_text(args: &[String]) -> Result<String, String> {
    let config = Config::resolve()?;
    let definitions = definitions(&config).ok_or("no workflows are defined")?;
    match args.first().map(String::as_str) {
        Some("show") => {
            let Some(name) = args.get(1) else {
                return Err("usage: ramify workflow show <workflow>".into());
            };
            let workflow = definitions.workflows.get(name).ok_or_else(|| {
                format!(
                    "no workflow '{name}' — one of: {}",
                    definitions
                        .workflows
                        .keys()
                        .cloned()
                        .collect::<Vec<_>>()
                        .join(", ")
                )
            })?;
            let default = default_lane(workflow);
            let mut lines = vec![format!(
                "{name} — {}   branch {}",
                workflow.does.clone().unwrap_or_default(),
                workflow.branch.clone().unwrap_or_else(|| "{slug}".into())
            )];
            for (lane, steps) in &workflow.lanes {
                lines.push(String::new());
                lines.push(format!(
                    "  {lane}{}",
                    if Some(lane) == default.as_ref() {
                        " (default)"
                    } else {
                        ""
                    }
                ));
                for step in steps {
                    let definition = definitions.steps.get(step);
                    let skill = definition
                        .and_then(|def| def.skill.clone())
                        .unwrap_or_default();
                    let gate = if definition.and_then(|def| def.gate.as_deref()) == Some("human") {
                        "human gate"
                    } else {
                        ""
                    };
                    let does = definition
                        .and_then(|def| def.does.clone())
                        .unwrap_or_default();
                    lines.push(format!("    {step:<10} {skill:<18} {gate:<10} {does}"));
                }
            }
            for (tag, lane) in &workflow.min_lane {
                lines.push(format!("  tag {tag} → at least {lane}"));
            }
            Ok(lines.join("\n"))
        }
        _ => {
            let mut lines = Vec::new();
            for (name, workflow) in &definitions.workflows {
                let default = default_lane(workflow);
                let lanes: Vec<String> = workflow
                    .lanes
                    .keys()
                    .map(|lane| {
                        if Some(lane) == default.as_ref() {
                            format!("[{lane}]")
                        } else {
                            lane.clone()
                        }
                    })
                    .collect();
                lines.push(format!(
                    "  {name:<10} {:<28} {}",
                    lanes.join(" "),
                    workflow.does.clone().unwrap_or_default()
                ));
            }
            Ok(lines.join("\n"))
        }
    }
}

fn default_lane(workflow: &WorkflowDef) -> Option<String> {
    workflow
        .default_lane
        .clone()
        .or_else(|| workflow.lanes.keys().next().cloned())
}

/// Python's `json.dumps` separates with ", " and ": "; the shell embeds its output verbatim.
pub fn progress_json(progress: &Progress) -> String {
    crate::pyjson::to_string(progress)
}

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
            ..Default::default()
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

