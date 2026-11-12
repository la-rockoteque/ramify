//! `ramify hooks run <Event>` and `ramify hooks bridge pre|post`.

use crate::classify::classify;
use crate::hooks::{common, project_of, to_claude, HookSet, Outcome, Sources};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

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
        (Some("run"), Some(event)) => run(event),
        (Some("list"), _) => list(),
        (Some("bridge"), Some(phase @ ("pre" | "post"))) => bridge(phase),
        _ => {
            eprintln!("ramify: usage: ramify hooks run <Event> | hooks list | hooks bridge pre|post");
            2
        }
    }
}

/// Every hook the sources hold for the current directory, in source order.
fn list() -> i32 {
    let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
    let project = project_of(&cwd);
    let ramify = crate::launch::home().ok();
    let set = HookSet::load(&home(), &project, ramify.as_deref(), Sources::All);
    for warning in &set.warnings {
        eprintln!("{warning}");
    }
    for hook in &set.hooks {
        let root = hook.plugin_root.as_ref().map(|root| format!("  [{}]", root.display())).unwrap_or_default();
        println!("{:<17} {:<12} {}{root}", hook.event, hook.matcher_text(), hook.command);
    }
    0
}

/// Runs every source's hooks for one event, and prints the merged decision.
fn run(event: &str) -> i32 {
    let input = stdin_json();
    let cwd = cwd_of(&input);
    let project = project_of(&cwd);
    let ramify = crate::launch::home().ok();
    let set = HookSet::load(&home(), &project, ramify.as_deref(), Sources::All);
    let subject = ["tool_name", "action", "source"]
        .iter()
        .find_map(|key| input.get(*key).and_then(Value::as_str))
        .unwrap_or("")
        .to_string();
    let outcome = set.run(event, &subject, &input, &cwd, &project);
    print(&outcome, event);
    0
}

fn print(outcome: &Outcome, event: &str) {
    for note in &outcome.notes {
        eprintln!("{note}");
    }
    if let Some(value) = to_claude(outcome, event) {
        println!("{value}");
    }
}

/// Claude Code and Codex have no workflow events: this hook gives them PreWorkflow and
/// PostWorkflow, from Ram Code's own files (`~/.ram`, `.ram`), and answers in their format.
fn bridge(phase: &str) -> i32 {
    let input = stdin_json();
    let cwd = cwd_of(&input);
    let project = project_of(&cwd);
    let set = HookSet::load(&home(), &project, None, Sources::RamOnly);
    let (event, answer_as) = if phase == "pre" {
        ("PreWorkflow", "PreToolUse")
    } else {
        ("PostWorkflow", "PostToolUse")
    };
    if !set.has(event) {
        return 0;
    }
    let outcome = workflow(&set, event, &input, &cwd, &project);
    print(&outcome, answer_as);
    0
}

/// Fires `event` once per workflow action of the tool call, and merges the outcomes.
pub fn workflow(set: &HookSet, event: &str, call: &Value, cwd: &Path, project: &Path) -> Outcome {
    let mut merged = Outcome::default();
    let session = call.get("session_id").and_then(Value::as_str).unwrap_or("");
    for found in classify(call) {
        let name = found["action"].as_str().unwrap_or("").to_string();
        let mut input = common(session, cwd, event);
        input["action"] = json!(name);
        input["tool_name"] = call.get("tool_name").cloned().unwrap_or(Value::Null);
        input["tool_input"] = call.get("tool_input").cloned().unwrap_or(Value::Null);
        input["details"] = found["details"].clone();
        if event == "PostWorkflow" {
            let response = call.get("tool_response").cloned().unwrap_or(Value::Null);
            input["succeeded"] = succeeded(&response);
            input["tool_response"] = response;
        }
        let outcome = set.run(event, &name, &input, cwd, project);
        merged.spawned += outcome.spawned;
        merged.contexts.extend(outcome.contexts);
        merged.notes.extend(outcome.notes);
        for (slot, reason) in [
            (&mut merged.deny, outcome.deny),
            (&mut merged.block, outcome.block),
            (&mut merged.stop, outcome.stop),
        ] {
            if let Some(reason) = reason {
                *slot = Some(match slot.take() {
                    Some(earlier) => format!("{earlier}\n{reason}"),
                    None => reason,
                });
            }
        }
    }
    merged
}

/// Ram Code's `run_command` result starts with `exit <code>`. Claude Code's Bash response has
/// no exit code: null there means unknown.
fn succeeded(response: &Value) -> Value {
    let text = match response {
        Value::String(text) => text.clone(),
        other => other.get("output").and_then(Value::as_str).unwrap_or("").to_string(),
    };
    match text.strip_prefix("exit ").and_then(|rest| rest.split_whitespace().next()) {
        Some(code) => json!(code == "0"),
        None => Value::Null,
    }
}
