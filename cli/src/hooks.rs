//! Claude Code's hook mechanics, for Ram Code and for ramify's workflow bridge.
//!
//! The ramify CLI and Ram Code are two crates; Ram Code includes this file with `#[path]`, so it
//! may use only `std`, `libc`, `serde_json` and `regex`.

use regex::Regex;
use serde_json::{json, Map, Value};
use std::io::{ErrorKind, Read, Write};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{mpsc, Arc, Mutex};
use std::time::{Duration, Instant};

/// One registered command.
#[derive(Clone, Debug)]
pub struct Hook {
    pub event: String,
    matcher: Option<Regex>,
    pub command: String,
    pub timeout: Duration,
    pub plugin_root: Option<PathBuf>,
}

impl Hook {
    /// The matcher as written, `*` when it matches everything.
    pub fn matcher_text(&self) -> String {
        self.matcher
            .as_ref()
            .map(|regex| regex.as_str().trim_start_matches("^(?:").trim_end_matches(")$").to_string())
            .unwrap_or_else(|| "*".into())
    }
}

/// Every hook of every source, in source order, with the sources that did not parse.
#[derive(Default, Debug)]
pub struct HookSet {
    pub hooks: Vec<Hook>,
    pub warnings: Vec<String>,
    /// The text of each file read, by path: `snapshot` gives it, `load_snapshot` reads it back.
    texts: Map<String, Value>,
    /// When set, files come from this snapshot, not from the disk.
    frozen: Option<Map<String, Value>>,
}

/// Which sources to read. The workflow bridge reads only Ram Code's own files.
#[derive(Clone, Copy, PartialEq)]
pub enum Sources {
    All,
    RamOnly,
}

/// What the hooks of one event decided, merged: a deny or a block wins over an allow.
#[derive(Default, Debug, PartialEq)]
pub struct Outcome {
    /// PreToolUse and PreWorkflow: the reason the tool must not run.
    pub deny: Option<String>,
    /// UserPromptSubmit, PostToolUse, PostWorkflow and Stop: `decision: "block"` or exit 2.
    pub block: Option<String>,
    /// `continue: false`: end the task, with this reason.
    pub stop: Option<String>,
    pub contexts: Vec<String>,
    /// One line per hook that blocked, denied, failed or sent a `systemMessage`.
    pub notes: Vec<String>,
    /// How many processes ran: zero when no hook matched.
    pub spawned: usize,
}

impl Outcome {
    /// Merges `more` into this outcome: the reasons join, and a deny or a block wins.
    pub fn absorb(&mut self, more: Outcome) {
        self.contexts.extend(more.contexts);
        self.notes.extend(more.notes);
        self.spawned += more.spawned;
        for (slot, reason) in [
            (&mut self.deny, more.deny),
            (&mut self.block, more.block),
            (&mut self.stop, more.stop),
        ] {
            if let Some(reason) = reason {
                join(slot, reason);
            }
        }
    }
}

const DEFAULT_TIMEOUT: u64 = 60;
/// After a hook exits, how long its output may take to arrive: a child that the hook left
/// running can keep the pipes open.
const GRACE: Duration = Duration::from_millis(200);
/// The projects whose own hook files run, below the home folder.
pub const TRUSTED: &str = ".ram/trusted.json";

impl HookSet {
    /// Reads the sources in the order of the spec: Ram Code's files, Claude Code's files, the
    /// enabled Claude Code plugins, then ramify's `hooks/ram.json`. The files of the project
    /// itself are read only when the project is trusted.
    pub fn load(home: &Path, project: &Path, ramify_home: Option<&Path>, sources: Sources) -> Self {
        Self::build(home, project, ramify_home, sources, None)
    }

    /// The same sources, with each file as `snapshot` holds it.
    pub fn load_snapshot(
        snapshot: &Value,
        home: &Path,
        project: &Path,
        ramify_home: Option<&Path>,
        sources: Sources,
    ) -> Self {
        let frozen = snapshot.as_object().cloned().unwrap_or_default();
        Self::build(home, project, ramify_home, sources, Some(frozen))
    }

    /// The text of every file this set read, by path.
    pub fn snapshot(&self) -> Value {
        Value::Object(self.texts.clone())
    }

    fn build(
        home: &Path,
        project: &Path,
        ramify_home: Option<&Path>,
        sources: Sources,
        frozen: Option<Map<String, Value>>,
    ) -> Self {
        let mut set = HookSet {
            frozen,
            ..HookSet::default()
        };
        let trusted = set.trusted(home, project);
        let own = |folder: &str| {
            [
                project.join(folder).join("settings.json"),
                project.join(folder).join("settings.local.json"),
            ]
        };
        set.project_files(trusted, project, &own(".ram"), home.join(".ram/settings.json"));
        if sources == Sources::RamOnly {
            return set;
        }
        set.project_files(trusted, project, &own(".claude"), home.join(".claude/settings.json"));
        // Ram Code ships with ramify, so ramify's hooks for it come from `ramify_home` when the
        // installed ramify plugin has none: its Claude Code hooks match `Bash` and run without
        // RAMIFY_AGENT=ram. Either way, ramify's hooks are read once.
        let ram_home = ramify_home.map(|home| home.join("hooks/ram.json")).filter(|path| path.is_file());
        let mut ramify_loaded = false;
        for (name, root) in set.enabled_plugins(home, project) {
            let is_ramify = name.starts_with("ramify@");
            // A plugin that ships hooks for Ram Code uses them, not its Claude Code ones: their
            // matchers name Claude Code's tools.
            let ram = root.join("hooks/ram.json");
            if set.file(&ram, Some(&root)) {
                ramify_loaded |= is_ramify;
            } else if !(is_ramify && ram_home.is_some())
                && !set.manifest(&root.join(".claude-plugin/plugin.json"), &root)
            {
                set.file(&root.join("hooks/hooks.json"), Some(&root));
            }
        }
        if let (Some(ram), Some(ramify), false) = (&ram_home, ramify_home, ramify_loaded) {
            set.file(ram, Some(ramify));
        }
        set
    }

    /// The home file, then the project's files when the project is trusted. An untrusted
    /// project with hook files gets one warning.
    fn project_files(&mut self, trusted: bool, project: &Path, files: &[PathBuf], home_file: PathBuf) {
        self.file(&home_file, None);
        if trusted {
            for path in files {
                self.file(path, None);
            }
        } else if files.iter().any(|path| path.is_file()) {
            self.warnings.push(format!(
                "hooks: skipped the hooks of {}: the project is not trusted (ramify hooks trust)",
                project.display()
            ));
        }
    }

    /// Whether `~/.ram/trusted.json` lists the project.
    fn trusted(&mut self, home: &Path, project: &Path) -> bool {
        let list = self
            .read(&home.join(TRUSTED))
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null);
        let project = canonical(project);
        list.get("projects")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .any(|entry| canonical(Path::new(entry)) == project)
    }

    /// Whether any hook is registered for the event.
    pub fn has(&self, event: &str) -> bool {
        self.hooks.iter().any(|hook| hook.event == event)
    }

    /// Lowers each timeout to `most`: the workflow bridge must answer before its own timeout.
    pub fn cap(&mut self, most: Duration) {
        for hook in &mut self.hooks {
            hook.timeout = hook.timeout.min(most);
        }
    }

    fn read(&mut self, path: &Path) -> Option<String> {
        let key = path.display().to_string();
        let text = match &self.frozen {
            Some(frozen) => frozen.get(&key).and_then(Value::as_str).map(str::to_string),
            None => std::fs::read_to_string(path).ok(),
        }?;
        self.texts.insert(key, Value::String(text.clone()));
        Some(text)
    }

    /// Adds the hooks of one settings file. Returns whether the file exists.
    fn file(&mut self, path: &Path, plugin_root: Option<&Path>) -> bool {
        let Some(text) = self.read(path) else {
            return false;
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => self.add(value.get("hooks").unwrap_or(&Value::Null), plugin_root),
            Err(error) => self
                .warnings
                .push(format!("hooks: skipped {}: {error}", path.display())),
        }
        true
    }

    /// A plugin manifest holds its hooks inline, or names a file. Returns whether it had any.
    fn manifest(&mut self, path: &Path, root: &Path) -> bool {
        let Some(text) = self.read(path) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            self.warnings
                .push(format!("hooks: skipped {}: not JSON", path.display()));
            return true;
        };
        match value.get("hooks") {
            Some(Value::String(relative)) => {
                self.file(&root.join(relative.trim_start_matches("./")), Some(root));
                true
            }
            Some(hooks @ Value::Object(_)) => {
                self.add(hooks, Some(root));
                true
            }
            _ => false,
        }
    }

    fn add(&mut self, hooks: &Value, plugin_root: Option<&Path>) {
        let Some(events) = hooks.as_object() else {
            return;
        };
        for (event, groups) in events {
            for group in groups.as_array().into_iter().flatten() {
                let pattern = group.get("matcher").and_then(Value::as_str).unwrap_or("");
                let matcher = if pattern.is_empty() || pattern == "*" {
                    None
                } else {
                    match Regex::new(&format!("^(?:{pattern})$")) {
                        Ok(regex) => Some(regex),
                        Err(error) => {
                            self.warnings
                                .push(format!("hooks: bad matcher {pattern:?}: {error}"));
                            continue;
                        }
                    }
                };
                for hook in group
                    .get("hooks")
                    .and_then(Value::as_array)
                    .into_iter()
                    .flatten()
                {
                    if hook.get("type").and_then(Value::as_str) != Some("command") {
                        continue;
                    }
                    let Some(command) = hook.get("command").and_then(Value::as_str) else {
                        continue;
                    };
                    let timeout = hook
                        .get("timeout")
                        .and_then(Value::as_u64)
                        .unwrap_or(DEFAULT_TIMEOUT);
                    self.hooks.push(Hook {
                        event: event.clone(),
                        matcher: matcher.clone(),
                        command: command.to_string(),
                        timeout: Duration::from_secs(timeout.max(1)),
                        plugin_root: plugin_root.map(Path::to_path_buf),
                    });
                }
            }
        }
    }

    /// The hooks of `event` whose matcher takes `subject`, each command once. An event with no
    /// subject (UserPromptSubmit, Stop, SessionEnd) ignores the matchers, as Claude Code does.
    pub fn matching(&self, event: &str, subject: &str) -> Vec<&Hook> {
        let mut seen: Vec<(&str, Option<&Path>)> = Vec::new();
        self.hooks
            .iter()
            .filter(|hook| hook.event == event)
            .filter(|hook| {
                subject.is_empty()
                    || hook
                        .matcher
                        .as_ref()
                        .is_none_or(|regex| regex.is_match(subject))
            })
            .filter(|hook| {
                let key = (hook.command.as_str(), hook.plugin_root.as_deref());
                let new = !seen.contains(&key);
                seen.push(key);
                new
            })
            .collect()
    }

    /// Runs the matching hooks of `event` in parallel, with `input` on stdin, in `cwd`. Each
    /// timeout counts from the start of its hook.
    pub fn run(
        &self,
        event: &str,
        subject: &str,
        input: &Value,
        cwd: &Path,
        project: &Path,
    ) -> Outcome {
        let hooks = self.matching(event, subject);
        let mut outcome = Outcome {
            spawned: hooks.len(),
            ..Outcome::default()
        };
        let payload: Arc<[u8]> = input.to_string().into_bytes().into();
        let started: Vec<_> = hooks
            .iter()
            .map(|hook| (hook, start(hook, payload.clone(), cwd, project)))
            .collect();
        for (hook, running) in started {
            let result = match running {
                Ok(running) => running.finish(),
                Err(error) => Finished::Failed(format!("could not start: {error}")),
            };
            merge(event, &hook.command, result, &mut outcome);
        }
        outcome
    }

    /// The installed plugins that `~/.claude/settings.json` enables, as (name, folder). A
    /// plugin installed for another project is left out.
    fn enabled_plugins(&mut self, home: &Path, project: &Path) -> Vec<(String, PathBuf)> {
        let mut read = |path: PathBuf| {
            self.read(&path)
                .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                .unwrap_or(Value::Null)
        };
        let settings = read(home.join(".claude/settings.json"));
        let installed = read(home.join(".claude/plugins/installed_plugins.json"));
        let Some(enabled) = settings.get("enabledPlugins").and_then(Value::as_object) else {
            return Vec::new();
        };
        let project = canonical(project);
        let mut plugins: Vec<(String, PathBuf)> = enabled
            .iter()
            .filter(|(_, on)| on.as_bool() == Some(true))
            .filter_map(|(name, _)| {
                let entries = installed.get("plugins")?.get(name)?.as_array()?;
                let entry = entries.iter().find(|entry| {
                    let scope = entry.get("scope").and_then(Value::as_str);
                    let path = entry.get("projectPath").and_then(Value::as_str);
                    matches!(scope, None | Some("user"))
                        || path.is_some_and(|path| canonical(Path::new(path)) == project)
                })?;
                let path = entry.get("installPath")?.as_str()?;
                Some((name.clone(), PathBuf::from(path)))
            })
            .collect();
        plugins.sort();
        plugins
    }
}

fn canonical(path: &Path) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf())
}

/// Adds the project to `~/.ram/trusted.json`, or takes it out. Returns the project.
pub fn trust(home: &Path, project: &Path, on: bool) -> std::io::Result<PathBuf> {
    let path = home.join(TRUSTED);
    let mut list = std::fs::read_to_string(&path)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let project = canonical(project);
    let entry = project.display().to_string();
    let mut projects: Vec<String> = list
        .get("projects")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .filter_map(Value::as_str)
        .filter(|known| canonical(Path::new(known)) != project)
        .map(str::to_string)
        .collect();
    if on {
        projects.push(entry);
    }
    list["projects"] = json!(projects);
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(&path, format!("{list:#}\n"))?;
    Ok(project)
}

/// Ramify's install: `RAMIFY_HOME`, else the folder above the running binary that holds both
/// `bin/ramify` and `hooks/ram.json`.
pub fn ramify_home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("RAMIFY_HOME") {
        return Some(PathBuf::from(home));
    }
    let mut current = std::env::current_exe().ok()?;
    while current.pop() {
        if current.join("bin/ramify").is_file() && current.join("hooks/ram.json").is_file() {
            return Some(current);
        }
    }
    None
}

enum Finished {
    Exited {
        code: i32,
        stdout: String,
        stderr: String,
    },
    TimedOut,
    Failed(String),
}

/// A hook's output stream, read on a thread so that a full pipe cannot stall the hook.
struct Output {
    text: Arc<Mutex<Vec<u8>>>,
    done: mpsc::Receiver<()>,
}

fn collect(stream: Option<impl Read + Send + 'static>) -> Output {
    let text = Arc::new(Mutex::new(Vec::new()));
    let (done, wait) = mpsc::channel();
    let sink = text.clone();
    std::thread::spawn(move || {
        if let Some(mut stream) = stream {
            let mut chunk = [0u8; 8192];
            loop {
                match stream.read(&mut chunk) {
                    Ok(0) => break,
                    Ok(read) => {
                        if let Ok(mut sink) = sink.lock() {
                            sink.extend_from_slice(&chunk[..read]);
                        }
                    }
                    Err(error) if error.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break,
                }
            }
        }
        let _ = done.send(());
    });
    Output { text, done: wait }
}

impl Output {
    /// What the stream gave, once it closes or at `until`, whichever comes first.
    fn take(self, until: Instant) -> String {
        let _ = self
            .done
            .recv_timeout(until.saturating_duration_since(Instant::now()));
        let bytes = self.text.lock().map(|text| text.clone()).unwrap_or_default();
        String::from_utf8_lossy(&bytes).into_owned()
    }
}

struct Running {
    child: Child,
    stdout: Output,
    stderr: Output,
    deadline: Instant,
}

/// Starts one hook in its own process group, so that a timeout kills its children too. The
/// input goes to stdin on a thread: a hook that does not read cannot stall the others.
fn start(hook: &Hook, payload: Arc<[u8]>, cwd: &Path, project: &Path) -> std::io::Result<Running> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(&hook.command)
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", project)
        .env("RAM_CODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .process_group(0);
    if let Some(root) = &hook.plugin_root {
        command
            .env("CLAUDE_PLUGIN_ROOT", root)
            .env("PLUGIN_ROOT", root);
    }
    let mut child = command.spawn()?;
    let deadline = Instant::now() + hook.timeout;
    if let Some(mut stdin) = child.stdin.take() {
        std::thread::spawn(move || {
            let _ = stdin.write_all(&payload);
        });
    }
    let stdout = collect(child.stdout.take());
    let stderr = collect(child.stderr.take());
    Ok(Running {
        child,
        stdout,
        stderr,
        deadline,
    })
}

impl Running {
    /// Waits for the hook up to its deadline.
    fn finish(mut self) -> Finished {
        loop {
            match self.child.try_wait() {
                Ok(Some(status)) => {
                    let until = self.deadline.min(Instant::now() + GRACE);
                    return Finished::Exited {
                        code: status.code().unwrap_or(-1),
                        stdout: self.stdout.take(until),
                        stderr: self.stderr.take(until),
                    };
                }
                Ok(None) if Instant::now() >= self.deadline => {
                    self.kill();
                    return Finished::TimedOut;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(10)),
                Err(error) => {
                    self.kill();
                    return Finished::Failed(error.to_string());
                }
            }
        }
    }

    fn kill(&mut self) {
        // SAFETY: killpg only sends a signal; the group is the one this hook leads.
        unsafe {
            libc::killpg(self.child.id() as libc::pid_t, libc::SIGKILL);
        }
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn first_line(text: &str) -> String {
    text.lines()
        .find(|line| !line.trim().is_empty())
        .unwrap_or("")
        .trim()
        .to_string()
}

fn join(slot: &mut Option<String>, reason: String) {
    *slot = Some(match slot.take() {
        Some(earlier) => format!("{earlier}\n{reason}"),
        None => reason,
    });
}

fn merge(event: &str, command: &str, result: Finished, outcome: &mut Outcome) {
    let name = command.split_whitespace().last().unwrap_or(command);
    let (code, stdout, stderr) = match result {
        Finished::Exited {
            code,
            stdout,
            stderr,
        } => (code, stdout, stderr),
        Finished::TimedOut => {
            outcome
                .notes
                .push(format!("hook {event}: {name} timed out"));
            return;
        }
        Finished::Failed(error) => {
            outcome.notes.push(format!("hook {event}: {name}: {error}"));
            return;
        }
    };
    match code {
        0 => {}
        2 => {
            let reason = stderr.trim().to_string();
            outcome
                .notes
                .push(format!("hook {event}: {}", first_line(&reason)));
            match event {
                "PreToolUse" | "PreWorkflow" => join(&mut outcome.deny, reason),
                "UserPromptSubmit" | "Stop" | "PostToolUse" | "PostWorkflow" => {
                    join(&mut outcome.block, reason)
                }
                _ => {}
            }
            return;
        }
        _ => {
            outcome.notes.push(format!(
                "hook {event}: {name} failed: {}",
                first_line(&stderr)
            ));
            return;
        }
    }
    let text = stdout.trim();
    let Some(value) = text
        .starts_with('{')
        .then(|| serde_json::from_str::<Value>(text).ok())
        .flatten()
    else {
        if !text.is_empty() && matches!(event, "SessionStart" | "UserPromptSubmit") {
            outcome.contexts.push(text.to_string());
        }
        return;
    };
    let specific = value
        .get("hookSpecificOutput")
        .cloned()
        .unwrap_or(Value::Null);
    if let Some(context) = specific.get("additionalContext").and_then(Value::as_str) {
        if !context.is_empty() {
            outcome.contexts.push(context.to_string());
        }
    }
    if matches!(event, "PreToolUse" | "PreWorkflow") {
        let decision = specific
            .get("permissionDecision")
            .or_else(|| value.get("decision"))
            .and_then(Value::as_str);
        if matches!(decision, Some("deny") | Some("ask") | Some("block")) {
            let reason = specific
                .get("permissionDecisionReason")
                .or_else(|| value.get("reason"))
                .and_then(Value::as_str)
                .unwrap_or("denied")
                .to_string();
            outcome
                .notes
                .push(format!("hook {event}: {}", first_line(&reason)));
            join(&mut outcome.deny, reason);
        }
    } else if value.get("decision").and_then(Value::as_str) == Some("block") {
        let reason = value
            .get("reason")
            .and_then(Value::as_str)
            .unwrap_or("blocked")
            .to_string();
        outcome
            .notes
            .push(format!("hook {event}: {}", first_line(&reason)));
        join(&mut outcome.block, reason);
    }
    if value.get("continue").and_then(Value::as_bool) == Some(false) {
        let reason = value
            .get("stopReason")
            .and_then(Value::as_str)
            .unwrap_or("stopped by a hook");
        join(&mut outcome.stop, reason.to_string());
    }
    if let Some(message) = value.get("systemMessage").and_then(Value::as_str) {
        outcome.notes.push(format!("hook {event}: {message}"));
    }
}

/// The primary checkout of the repository at `cwd`: hooks read `.ram` and `.claude` there.
pub fn project_of(cwd: &Path) -> PathBuf {
    Command::new("git")
        .args(["rev-parse", "--path-format=absolute", "--git-common-dir"])
        .current_dir(cwd)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .and_then(|common| common.parent().map(Path::to_path_buf))
        .unwrap_or_else(|| cwd.to_path_buf())
}

/// An outcome in Claude Code's output format, for the hook that asked: `pre` for PreToolUse.
pub fn to_claude(outcome: &Outcome, hook_event: &str) -> Option<Value> {
    let mut specific = serde_json::Map::new();
    specific.insert("hookEventName".into(), json!(hook_event));
    let mut out = serde_json::Map::new();
    if let Some(reason) = &outcome.deny {
        specific.insert("permissionDecision".into(), json!("deny"));
        specific.insert("permissionDecisionReason".into(), json!(reason));
    }
    if !outcome.contexts.is_empty() {
        specific.insert("additionalContext".into(), json!(outcome.contexts.join("\n\n")));
    }
    if let Some(reason) = &outcome.block {
        out.insert("decision".into(), json!("block"));
        out.insert("reason".into(), json!(reason));
    }
    if let Some(reason) = &outcome.stop {
        out.insert("continue".into(), json!(false));
        out.insert("stopReason".into(), json!(reason));
    }
    if specific.len() > 1 {
        out.insert("hookSpecificOutput".into(), Value::Object(specific));
    }
    (!out.is_empty()).then_some(Value::Object(out))
}

/// The input of one event: the fields every event has, then `fields`.
pub fn common(session: &str, cwd: &Path, event: &str, fields: Value) -> Value {
    let mut input = json!({
        "session_id": session,
        "transcript_path": "",
        "cwd": cwd,
        "hook_event_name": event,
    });
    if let (Some(target), Value::Object(extra)) = (input.as_object_mut(), fields) {
        target.extend(extra);
    }
    input
}

/// The fields of a PreWorkflow input for one action of a tool call. With the tool's response,
/// the fields of a PostWorkflow input.
pub fn workflow_fields(found: &Value, tool: &Value, tool_input: &Value, response: Option<&Value>) -> Value {
    let mut fields = json!({
        "action": found["action"],
        "tool_name": tool,
        "tool_input": tool_input,
        "details": found["details"],
    });
    if let Some(response) = response {
        fields["succeeded"] = succeeded(response);
        fields["tool_response"] = response.clone();
    }
    fields
}

/// Whether the command exited 0. Ram Code's `run_command` result starts with `exit <code>`.
/// A tool with no exit code (`read_file`, `write_file`, Claude Code's Bash) gives null.
pub fn succeeded(response: &Value) -> Value {
    let text = match response {
        Value::String(text) => text.as_str(),
        other => other.get("output").and_then(Value::as_str).unwrap_or(""),
    };
    match text.strip_prefix("exit ").and_then(|rest| rest.split_whitespace().next()) {
        Some(code) => json!(code == "0"),
        None => Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn outcome(event: &str, code: i32, stdout: &str, stderr: &str) -> Outcome {
        let mut outcome = Outcome::default();
        let result = Finished::Exited {
            code,
            stdout: stdout.into(),
            stderr: stderr.into(),
        };
        merge(event, "h", result, &mut outcome);
        outcome
    }

    #[test]
    fn a_deny_and_an_ask_both_hold_the_tool() {
        let deny = r#"{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no"}}"#;
        assert_eq!(
            outcome("PreToolUse", 0, deny, "").deny.as_deref(),
            Some("no")
        );
        let ask = r#"{"hookSpecificOutput":{"permissionDecision":"ask","permissionDecisionReason":"sure?"}}"#;
        assert_eq!(
            outcome("PreToolUse", 0, ask, "").deny.as_deref(),
            Some("sure?")
        );
        let allow = r#"{"hookSpecificOutput":{"permissionDecision":"allow"}}"#;
        assert_eq!(outcome("PreToolUse", 0, allow, "").deny, None);
    }

    #[test]
    fn exit_two_blocks_and_other_codes_only_note() {
        assert_eq!(
            outcome("PreToolUse", 2, "", "nope\n").deny.as_deref(),
            Some("nope")
        );
        assert_eq!(
            outcome("Stop", 2, "", "go on").block.as_deref(),
            Some("go on")
        );
        let failed = outcome("PreToolUse", 1, "", "boom");
        assert_eq!((failed.deny, failed.notes.len()), (None, 1));
    }

    #[test]
    fn plain_stdout_is_context_only_at_the_start_of_a_turn() {
        assert_eq!(outcome("SessionStart", 0, "hello", "").contexts, ["hello"]);
        assert!(outcome("PostToolUse", 0, "hello", "").contexts.is_empty());
    }
}
