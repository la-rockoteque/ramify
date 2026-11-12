//! The hooks one Ram Code session runs: the loaded sources, its session id, and where the
//! workflow classifier lives.

use crate::hooks::{common, project_of, ramify_home, HookSet, Outcome, Sources};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

/// How long a task waits for SessionStart to leave its snapshot.
const START_WAIT: Duration = Duration::from_secs(10);

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
        Self::load_from(cwd, session, &hooks_home())
    }

    /// Every source, with `home` in place of the user's home folder.
    pub fn load_from(cwd: &Path, session: &str, home: &Path) -> Self {
        Self::build(cwd, session, home, None)
    }

    /// The hooks of one task of the interface: the files as SessionStart read them into `dir`,
    /// so that a hook file the model writes runs only in a later session. The task waits for
    /// the snapshot at most 10 s, then reads the files as they are.
    pub fn for_task(cwd: &Path, session: &str, dir: Option<&Path>) -> Self {
        Self::for_task_from(cwd, session, dir, &hooks_home())
    }

    /// `for_task`, with `home` in place of the user's home folder.
    pub fn for_task_from(cwd: &Path, session: &str, dir: Option<&Path>, home: &Path) -> Self {
        let Some(dir) = dir else {
            return Self::load_from(cwd, session, home);
        };
        let deadline = Instant::now() + START_WAIT;
        while !dir.join(STARTED).exists() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(20));
        }
        let snapshot = std::fs::read_to_string(dir.join(SNAPSHOT))
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok());
        Self::build(cwd, session, home, snapshot.as_ref())
    }

    /// Writes what a task reads from `dir`: the snapshot, the SessionStart context, and last
    /// the marker that says both are there.
    pub fn leave_start(&self, dir: &Path, contexts: &[String]) -> std::io::Result<()> {
        std::fs::create_dir_all(dir)?;
        std::fs::write(dir.join(SNAPSHOT), self.set.snapshot().to_string())?;
        std::fs::write(dir.join(CONTEXT), contexts.join("\n\n"))?;
        std::fs::write(dir.join(STARTED), "")
    }

    fn build(cwd: &Path, session: &str, home: &Path, snapshot: Option<&Value>) -> Self {
        let project = project_of(cwd);
        let ramify = ramify_home();
        let set = match snapshot {
            Some(snapshot) => HookSet::load_snapshot(snapshot, home, &project, ramify.as_deref(), Sources::All),
            None => HookSet::load(home, &project, ramify.as_deref(), Sources::All),
        };
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
        let input = common(&self.session, &self.cwd, event, fields);
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

/// The files a task reads from the start folder.
pub const SNAPSHOT: &str = "hooks.json";
pub const CONTEXT: &str = "context.txt";
const STARTED: &str = "started";

/// As in the ramify CLI: RAMIFY_HOOKS_HOME, else HOME.
fn hooks_home() -> PathBuf {
    let home = std::env::var("RAMIFY_HOOKS_HOME").or_else(|_| std::env::var("HOME"));
    PathBuf::from(home.unwrap_or_default())
}

/// Sets the session's record to `idle` when it says `working`.
pub fn idle_record(state_root: &Path, session: &str) {
    let path = state_root.join("sessions").join(format!("{session}.session"));
    let Some(mut record) = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
    else {
        return;
    };
    if record.get("state").and_then(Value::as_str) != Some("working") {
        return;
    }
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0);
    record["state"] = json!("idle");
    record["since"] = json!(now);
    let temporary = path.with_extension(format!("session.{}", std::process::id()));
    if std::fs::write(&temporary, record.to_string()).is_ok() {
        let _ = std::fs::rename(&temporary, &path);
    }
}
