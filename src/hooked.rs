//! The hooks one Ram Code session runs: the loaded sources, its session id, and where the
//! workflow classifier lives.

use crate::hooks::{common, project_of, HookSet, Outcome, Sources};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};

pub struct Hooked {
    pub set: HookSet,
    pub session: String,
    pub cwd: PathBuf,
    pub project: PathBuf,
    /// The `ramify` program that classifies workflow actions.
    pub ramify: String,
    /// What SessionStart said, for the first task.
    pub start_context: Vec<String>,
    /// How many processes the hooks started: tests read it.
    pub spawned: std::cell::Cell<usize>,
}

impl Hooked {
    /// No hook at all: the tool loop runs as it did before hooks.
    pub fn none(cwd: &Path) -> Self {
        Hooked {
            set: HookSet::default(),
            session: String::new(),
            cwd: cwd.to_path_buf(),
            project: cwd.to_path_buf(),
            ramify: "ramify".into(),
            start_context: Vec::new(),
            spawned: std::cell::Cell::new(0),
        }
    }

    /// Every source, for the worktree at `cwd`.
    pub fn load(cwd: &Path, session: &str) -> Self {
        // As in the ramify CLI: RAMIFY_HOOKS_HOME, else HOME.
        let home = std::env::var("RAMIFY_HOOKS_HOME").or_else(|_| std::env::var("HOME"));
        Self::load_from(cwd, session, &PathBuf::from(home.unwrap_or_default()))
    }

    /// Every source, with `home` in place of the user's home folder.
    pub fn load_from(cwd: &Path, session: &str, home: &Path) -> Self {
        let project = project_of(cwd);
        let set = HookSet::load(home, &project, ramify_home().as_deref(), Sources::All);
        Hooked {
            set,
            session: session.to_string(),
            cwd: cwd.to_path_buf(),
            project,
            ramify: std::env::var("RAM_CODE_RAMIFY").unwrap_or_else(|_| "ramify".into()),
            start_context: Vec::new(),
            spawned: std::cell::Cell::new(0),
        }
    }

    pub fn has(&self, event: &str) -> bool {
        self.set.has(event)
    }

    /// Fires `event`, with `fields` added to the common ones. `subject` is what the matcher
    /// tests: the tool name, the action or the source.
    pub fn fire(&self, event: &str, subject: &str, fields: Value) -> Outcome {
        if !self.set.has(event) {
            return Outcome::default();
        }
        let mut input = common(&self.session, &self.cwd, event);
        if let (Some(target), Some(extra)) = (input.as_object_mut(), fields.as_object()) {
            for (key, value) in extra {
                target.insert(key.clone(), value.clone());
            }
        }
        let outcome = self.set.run(event, subject, &input, &self.cwd, &self.project);
        self.spawned.set(self.spawned.get() + outcome.spawned);
        outcome
    }

    /// The workflow actions of a tool call, from `ramify classify`. It runs only when a
    /// workflow hook is configured.
    pub fn actions(&self, tool: &str, input: &Value) -> Vec<Value> {
        if !self.has("PreWorkflow") && !self.has("PostWorkflow") {
            return Vec::new();
        }
        let call = json!({"tool_name": tool, "tool_input": input, "cwd": self.cwd});
        let output = std::process::Command::new(&self.ramify)
            .arg("classify")
            .current_dir(&self.cwd)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .spawn()
            .and_then(|mut child| {
                use std::io::Write;
                if let Some(mut stdin) = child.stdin.take() {
                    let _ = stdin.write_all(call.to_string().as_bytes());
                }
                child.wait_with_output()
            });
        output
            .ok()
            .and_then(|output| serde_json::from_slice::<Value>(&output.stdout).ok())
            .and_then(|value| value.as_array().cloned())
            .unwrap_or_default()
    }
}

/// Ramify's install: `RAMIFY_HOME`, else the folder above this binary that holds
/// `hooks/ram.json` (Ram Code ships in the ramify repository).
pub fn ramify_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("RAMIFY_HOME") {
        return Some(PathBuf::from(home));
    }
    let mut current = std::env::current_exe().ok()?;
    while current.pop() {
        if current.join("hooks/ram.json").is_file() {
            return Some(current);
        }
    }
    None
}

/// Merges `more` into `into`: reasons join, a deny or a block wins.
pub fn absorb(into: &mut Outcome, more: Outcome) {
    into.contexts.extend(more.contexts);
    into.notes.extend(more.notes);
    into.spawned += more.spawned;
    for (slot, reason) in [
        (&mut into.deny, more.deny),
        (&mut into.block, more.block),
        (&mut into.stop, more.stop),
    ] {
        if let Some(reason) = reason {
            *slot = Some(match slot.take() {
                Some(earlier) => format!("{earlier}\n{reason}"),
                None => reason,
            });
        }
    }
}
