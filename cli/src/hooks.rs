//! Claude Code's hook mechanics, for Ram Code and for ramify's workflow bridge.
//!
//! The ramify CLI and Ram Code are two crates; Ram Code includes this file with `#[path]`, so it
//! may use only `std`, `serde_json` and `regex`.

use regex::Regex;
use serde_json::{json, Value};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// One registered command.
#[derive(Clone, Debug)]
pub struct Hook {
    pub event: String,
    matcher: Option<Regex>,
    pub command: String,
    pub timeout: Duration,
    pub plugin_root: Option<PathBuf>,
    pub env: Vec<(String, String)>,
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

const DEFAULT_TIMEOUT: u64 = 60;

impl HookSet {
    /// Reads the sources in the order of the spec: Ram Code's files, Claude Code's files, the
    /// enabled Claude Code plugins, then ramify's `hooks/ram.json`.
    pub fn load(home: &Path, project: &Path, ramify_home: Option<&Path>, sources: Sources) -> Self {
        let mut set = HookSet::default();
        set.file(&home.join(".ram/settings.json"), None, &[]);
        set.file(&project.join(".ram/settings.json"), None, &[]);
        set.file(&project.join(".ram/settings.local.json"), None, &[]);
        if sources == Sources::RamOnly {
            return set;
        }
        set.file(&home.join(".claude/settings.json"), None, &[]);
        set.file(&project.join(".claude/settings.json"), None, &[]);
        set.file(&project.join(".claude/settings.local.json"), None, &[]);
        // Ram Code ships with ramify, so ramify's hooks for it come from `ramify_home` when the
        // installed ramify plugin has none: its Claude Code hooks match `Bash` and run without
        // RAMIFY_AGENT=ram. Either way, ramify's hooks are read once.
        let ram_home = ramify_home.map(|home| home.join("hooks/ram.json")).filter(|path| path.is_file());
        let mut ramify_loaded = false;
        for (name, root) in enabled_plugins(home) {
            let is_ramify = name.starts_with("ramify@");
            // A plugin that ships hooks for Ram Code uses them, not its Claude Code ones: their
            // matchers name Claude Code's tools.
            let ram = root.join("hooks/ram.json");
            if ram.is_file() {
                set.file(&ram, Some(&root), &[]);
                ramify_loaded |= is_ramify;
            } else if is_ramify && ram_home.is_some() {
                continue;
            } else {
                let manifest = root.join(".claude-plugin/plugin.json");
                let hooks = root.join("hooks/hooks.json");
                if !set.manifest(&manifest, &root) && hooks.is_file() {
                    set.file(&hooks, Some(&root), &[]);
                }
            }
        }
        if let (Some(ram), Some(ramify), false) = (&ram_home, ramify_home, ramify_loaded) {
            set.file(ram, Some(ramify), &[]);
        }
        set
    }

    /// Whether any hook is registered for the event.
    pub fn has(&self, event: &str) -> bool {
        self.hooks.iter().any(|hook| hook.event == event)
    }

    fn file(&mut self, path: &Path, plugin_root: Option<&Path>, env: &[(String, String)]) {
        let Ok(text) = std::fs::read_to_string(path) else {
            return;
        };
        match serde_json::from_str::<Value>(&text) {
            Ok(value) => self.add(value.get("hooks").unwrap_or(&Value::Null), plugin_root, env),
            Err(error) => self
                .warnings
                .push(format!("hooks: skipped {}: {error}", path.display())),
        }
    }

    /// A plugin manifest holds its hooks inline, or names a file. Returns whether it had any.
    fn manifest(&mut self, path: &Path, root: &Path) -> bool {
        let Ok(text) = std::fs::read_to_string(path) else {
            return false;
        };
        let Ok(value) = serde_json::from_str::<Value>(&text) else {
            self.warnings
                .push(format!("hooks: skipped {}: not JSON", path.display()));
            return true;
        };
        match value.get("hooks") {
            Some(Value::String(relative)) => {
                self.file(
                    &root.join(relative.trim_start_matches("./")),
                    Some(root),
                    &[],
                );
                true
            }
            Some(hooks @ Value::Object(_)) => {
                self.add(hooks, Some(root), &[]);
                true
            }
            _ => false,
        }
    }

    fn add(&mut self, hooks: &Value, plugin_root: Option<&Path>, env: &[(String, String)]) {
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
                        env: env.to_vec(),
                    });
                }
            }
        }
    }

    /// The hooks of `event` whose matcher takes `subject`, each command once.
    pub fn matching(&self, event: &str, subject: &str) -> Vec<&Hook> {
        let mut seen: Vec<(&str, Option<&Path>)> = Vec::new();
        self.hooks
            .iter()
            .filter(|hook| hook.event == event)
            .filter(|hook| {
                hook.matcher
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

    /// Runs the matching hooks of `event` in parallel, with `input` on stdin, in `cwd`.
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
        let payload = input.to_string();
        let started: Vec<_> = hooks
            .iter()
            .map(|hook| (hook, start(hook, &payload, cwd, project)))
            .collect();
        for (hook, child) in started {
            let result = match child {
                Ok(child) => finish(child, hook.timeout),
                Err(error) => Finished::Failed(format!("could not start: {error}")),
            };
            merge(event, &hook.command, result, &mut outcome);
        }
        outcome
    }
}

/// The installed plugins that `~/.claude/settings.json` enables, as (name, folder).
fn enabled_plugins(home: &Path) -> Vec<(String, PathBuf)> {
    let read = |path: PathBuf| {
        std::fs::read_to_string(path)
            .ok()
            .and_then(|text| serde_json::from_str::<Value>(&text).ok())
            .unwrap_or(Value::Null)
    };
    let settings = read(home.join(".claude/settings.json"));
    let installed = read(home.join(".claude/plugins/installed_plugins.json"));
    let Some(enabled) = settings.get("enabledPlugins").and_then(Value::as_object) else {
        return Vec::new();
    };
    let mut plugins: Vec<(String, PathBuf)> = enabled
        .iter()
        .filter(|(_, on)| on.as_bool() == Some(true))
        .filter_map(|(name, _)| {
            let entries = installed.get("plugins")?.get(name)?.as_array()?;
            let path = entries.first()?.get("installPath")?.as_str()?;
            Some((name.clone(), PathBuf::from(path)))
        })
        .collect();
    plugins.sort();
    plugins
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

fn start(
    hook: &Hook,
    payload: &str,
    cwd: &Path,
    project: &Path,
) -> std::io::Result<std::process::Child> {
    let mut command = Command::new("sh");
    command
        .arg("-c")
        .arg(&hook.command)
        .current_dir(cwd)
        .env("CLAUDE_PROJECT_DIR", project)
        .env("RAM_CODE", "1")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(root) = &hook.plugin_root {
        command
            .env("CLAUDE_PLUGIN_ROOT", root)
            .env("PLUGIN_ROOT", root);
    }
    for (key, value) in &hook.env {
        command.env(key, value);
    }
    let mut child = command.spawn()?;
    if let Some(mut stdin) = child.stdin.take() {
        let _ = stdin.write_all(payload.as_bytes());
    }
    Ok(child)
}

/// Waits for the hook up to its timeout, reading its output on threads so a full pipe cannot
/// stall it.
fn finish(mut child: std::process::Child, timeout: Duration) -> Finished {
    let pipe = |stream: Option<Box<dyn Read + Send>>| {
        std::thread::spawn(move || {
            let mut text = String::new();
            if let Some(mut stream) = stream {
                let _ = stream.read_to_string(&mut text);
            }
            text
        })
    };
    let stdout = pipe(
        child
            .stdout
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let stderr = pipe(
        child
            .stderr
            .take()
            .map(|s| Box::new(s) as Box<dyn Read + Send>),
    );
    let deadline = Instant::now() + timeout;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                return Finished::Exited {
                    code: status.code().unwrap_or(-1),
                    stdout: stdout.join().unwrap_or_default(),
                    stderr: stderr.join().unwrap_or_default(),
                }
            }
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Finished::TimedOut;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(10)),
            Err(error) => return Finished::Failed(error.to_string()),
        }
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

/// The JSON every event starts with.
pub fn common(session: &str, cwd: &Path, event: &str) -> Value {
    json!({
        "session_id": session,
        "transcript_path": "",
        "cwd": cwd,
        "hook_event_name": event,
    })
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
