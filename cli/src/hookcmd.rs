//! `ramify hooks list | trust | untrust | snapshot | bridge pre|post`.

use crate::classify::classify;
use crate::hooks::{
    common, project_of, ramify_home, to_claude, trust, workflow_fields, HookSet, Outcome, Sources, TRUSTED,
};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::time::Duration;

/// The bridge runs under a 30 s timeout in Claude Code and Codex: its hooks must end before.
const BRIDGE_CAP: Duration = Duration::from_secs(25);

fn stdin_json() -> Value {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    serde_json::from_str(&input).unwrap_or(Value::Null)
}

/// Where `.ram` and `.claude` are read: `RAMIFY_HOOKS_HOME`, else `HOME`. Tests set the first, so
/// the hooks they run keep the real `HOME`.
fn home() -> PathBuf {
    let home = std::env::var("RAMIFY_HOOKS_HOME").or_else(|_| std::env::var("HOME"));
    PathBuf::from(home.unwrap_or_default())
}

fn cwd_of(input: &Value) -> PathBuf {
    input
        .get("cwd")
        .and_then(Value::as_str)
        .map(PathBuf::from)
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn hooks(args: &[String]) -> i32 {
    match (args.first().map(String::as_str), args.get(1).map(String::as_str)) {
        (Some("list"), _) => list(),
        (Some(verb @ ("trust" | "untrust")), dir) => trusted(verb == "trust", dir),
        (Some("snapshot"), _) => snapshot(),
        (Some("bridge"), Some(phase @ ("pre" | "post"))) => bridge(phase),
        _ => {
            eprintln!("ramify: usage: ramify hooks list | trust [dir] | untrust [dir] | snapshot | bridge pre|post");
            2
        }
    }
}

fn here() -> PathBuf {
    std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."))
}

/// Every hook the sources hold for the current directory, in source order.
fn list() -> i32 {
    let cwd = here();
    let project = project_of(&cwd);
    let set = HookSet::load(&home(), &project, ramify_home().as_deref(), Sources::All);
    for warning in &set.warnings {
        eprintln!("{warning}");
    }
    for hook in &set.hooks {
        let root = hook.plugin_root.as_ref().map(|root| format!("  [{}]", root.display())).unwrap_or_default();
        println!("{:<17} {:<12} {}{root}", hook.event, hook.matcher_text(), hook.command);
    }
    0
}

/// Lets the hook files of a project run, or stops them.
fn trusted(on: bool, dir: Option<&str>) -> i32 {
    let dir = dir.map(PathBuf::from).unwrap_or_else(here);
    match trust(&home(), &project_of(&dir), on) {
        Ok(project) => {
            let verb = if on { "trusted" } else { "no longer trusted" };
            println!("{}: {verb}", project.display());
            0
        }
        Err(error) => {
            eprintln!("ramify: could not write ~/{TRUSTED}: {error}");
            1
        }
    }
}

fn print(outcome: &Outcome, event: &str) {
    for note in &outcome.notes {
        eprintln!("{note}");
    }
    if let Some(value) = to_claude(outcome, event) {
        println!("{value}");
    }
}

/// Where the bridge keeps the hook files of one Claude Code or Codex session, as they were at
/// its start. None for a session id that is not a plain name.
fn snapshot_path(session: &str) -> Option<PathBuf> {
    let plain = !session.is_empty()
        && session.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    let state = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    plain.then(|| PathBuf::from(state).join("sessions").join(format!("{session}.hooks.json")))
}

/// The project of `cwd`. Without a trust list no project file runs, so `git` need not run.
fn project_for(home: &Path, cwd: &Path) -> PathBuf {
    if home.join(TRUSTED).is_file() {
        project_of(cwd)
    } else {
        cwd.to_path_buf()
    }
}

/// SessionStart: keeps the bridge's hook files as they are now, so that a file the model
/// writes later does not run in this session.
fn snapshot() -> i32 {
    let input = stdin_json();
    let Some(path) = snapshot_path(input.get("session_id").and_then(Value::as_str).unwrap_or("")) else {
        return 0;
    };
    let home = home();
    let cwd = cwd_of(&input);
    let set = HookSet::load(&home, &project_for(&home, &cwd), None, Sources::RamOnly);
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let _ = std::fs::write(&path, set.snapshot().to_string());
    0
}

/// Claude Code and Codex have no workflow events: this hook gives them PreWorkflow and
/// PostWorkflow, from Ram Code's own files (`~/.ram`, and `.ram` in a trusted project), and
/// answers in their format. The files come from the session's snapshot when it has one.
fn bridge(phase: &str) -> i32 {
    let input = stdin_json();
    let home = home();
    let cwd = cwd_of(&input);
    let project = project_for(&home, &cwd);
    let frozen = snapshot_path(input.get("session_id").and_then(Value::as_str).unwrap_or(""))
        .and_then(|path| std::fs::read_to_string(path).ok())
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    let mut set = match &frozen {
        Some(frozen) => HookSet::load_snapshot(frozen, &home, &project, None, Sources::RamOnly),
        None => HookSet::load(&home, &project, None, Sources::RamOnly),
    };
    let (event, answer_as) = if phase == "pre" {
        ("PreWorkflow", "PreToolUse")
    } else {
        ("PostWorkflow", "PostToolUse")
    };
    if !set.has(event) {
        return 0;
    }
    set.cap(BRIDGE_CAP);
    let outcome = workflow(&set, event, &input, &cwd, &project);
    print(&outcome, answer_as);
    0
}

/// Fires `event` once per workflow action of the tool call, and merges the outcomes.
pub fn workflow(set: &HookSet, event: &str, call: &Value, cwd: &Path, project: &Path) -> Outcome {
    let mut merged = Outcome::default();
    let session = call.get("session_id").and_then(Value::as_str).unwrap_or("");
    let response = (event == "PostWorkflow").then(|| call.get("tool_response").cloned().unwrap_or(Value::Null));
    for found in classify(call) {
        let name = found["action"].as_str().unwrap_or("");
        let fields = workflow_fields(&found, &call["tool_name"], &call["tool_input"], response.as_ref());
        let input = common(session, cwd, event, fields);
        merged.absorb(set.run(event, name, &input, cwd, project));
    }
    merged
}
