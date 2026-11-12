use crate::cancel::Cancel;
use crate::providers::Providers;
use crate::sandbox::{scrub, Sandbox};
use serde_json::{json, Value};
use std::io::Read;
use std::os::unix::process::CommandExt;
use std::path::{Component, Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

const MAX_READ: usize = 200_000;
const MAX_LIST: usize = 300;
const MAX_COMMAND: usize = 40_000;
pub const DEFAULT_TIMEOUT: u64 = 120;
pub const MAX_TIMEOUT: u64 = 600;
const DEFAULT_RUN_WAIT: u64 = 1800;
pub const NO_DECISION: &str = "no decision; the run stays pending in the Run pane";

static TEMP_SEQ: AtomicUsize = AtomicUsize::new(0);

#[derive(Debug, Clone)]
pub struct Tools {
    pub root: PathBuf,
    pub sandbox: Sandbox,
    /// The environment Ram Code got. Commands get it without the secrets.
    pub env: Vec<(String, String)>,
    /// The `key_env` variables of `providers.json`.
    pub key_envs: Vec<String>,
    /// The `ramify` program that asks the human to run a command.
    pub ramify: String,
    /// True under the TUI: its Run pane shows the run, so the run window stays closed.
    pub no_window: bool,
    /// How long a command outside the sandbox waits for the human, in seconds.
    pub run_wait: u64,
    /// Set when the user stops the task: a running command or wait ends at once.
    pub cancel: Cancel,
    temp: PathBuf,
}

impl Tools {
    pub fn new(root: &Path) -> Self {
        Self::with_env(root, std::env::vars().collect())
    }

    pub fn with_env(root: &Path, env: Vec<(String, String)>) -> Self {
        let get = |key: &str| {
            env.iter()
                .find(|(name, _)| name == key)
                .map(|(_, value)| value.clone())
                .filter(|value| !value.trim().is_empty())
        };
        let key_envs = Providers::load(Providers::default_dir(&get))
            .map(|providers| {
                providers
                    .all()
                    .into_iter()
                    .filter_map(|provider| provider.key_env)
                    .collect()
            })
            .unwrap_or_default();
        let temp = std::env::temp_dir().join(format!(
            "ram-code-task-{}-{}",
            std::process::id(),
            TEMP_SEQ.fetch_add(1, Ordering::SeqCst)
        ));
        Self {
            root: root.to_path_buf(),
            sandbox: Sandbox::detect(),
            key_envs,
            ramify: get("RAM_CODE_RAMIFY").unwrap_or_else(|| "ramify".into()),
            no_window: get("RAM_CODE_NO_WINDOW").is_some(),
            run_wait: get("RAM_CODE_RUN_WAIT")
                .and_then(|value| value.trim().parse().ok())
                .unwrap_or(DEFAULT_RUN_WAIT),
            env,
            cancel: Cancel::new(),
            temp,
        }
    }

    /// These tools, stopped by `cancel`.
    pub fn with_cancel(mut self, cancel: Cancel) -> Self {
        self.cancel = cancel;
        self
    }

    /// The task's temp folder, created on first use. `TMPDIR` points here in a command.
    pub fn temp_dir(&self) -> Result<PathBuf, String> {
        std::fs::create_dir_all(&self.temp)
            .map_err(|e| format!("could not create {}: {e}", self.temp.display()))?;
        self.temp
            .canonicalize()
            .map_err(|e| format!("could not resolve {}: {e}", self.temp.display()))
    }

    pub fn definitions() -> Vec<serde_json::Value> {
        vec![
            json!({
                "name": "read_file",
                "description": "Read a UTF-8 file inside the worktree.",
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Relative path"}},
                    "required": ["path"]
                }
            }),
            json!({
                "name": "write_file",
                "description": "Write a UTF-8 file inside the worktree. Creates parent folders.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Relative path"},
                        "content": {"type": "string", "description": "Full file content"}
                    },
                    "required": ["path", "content"]
                }
            }),
            json!({
                "name": "list_files",
                "description": "List the entries of a folder inside the worktree.",
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Relative folder, default '.'"}},
                    "required": []
                }
            }),
            json!({
                "name": "grep",
                "description": "Search file contents in the worktree with a regular expression. \
Skips .gitignore'd, .git and binary files, and files over 10 MB. A path under .git or ignored by \
.gitignore is an error. Returns `path:line: text`, at most 200 matches.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Regular expression (Rust regex syntax)"},
                        "path": {"type": "string", "description": "Relative file or folder, default '.'"},
                        "glob": {"type": "string", "description": "Only files matching this glob. Without '/', it matches file names at any depth, such as '*.rs'. With '/', it matches the path from `path`, such as 'src/**/*.rs'."},
                        "ignore_case": {"type": "boolean", "description": "Match without case, default false"}
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "glob",
                "description": "Find files in the worktree by a glob pattern. A pattern without '/' \
matches file names at any depth, such as '*.rs'. A pattern with '/' matches the path from `path`, \
such as 'src/**/*.rs'. Skips .gitignore'd files and .git. Returns sorted relative paths, at most 500.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Glob pattern"},
                        "path": {"type": "string", "description": "Relative folder to search from, default '.'. A file is an error."}
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "code_map",
                "description": "Outline the code in a file or folder: each class, type, function and \
method with its line, without bodies. Covers Rust, TypeScript, JavaScript, Python and Java; \
other files show their line count. Files over 10 MB are listed as skipped. A path under .git or \
ignored by .gitignore is an error. Use it before reading whole files.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Relative file or folder, default '.'"}
                    },
                    "required": []
                }
            }),
            json!({
                "name": "run_command",
                "description": "Run a shell command in the worktree root and return its output. \
The command runs in a sandbox: it can read files, write only inside the worktree and $TMPDIR, \
and has no network. Keys and tokens are not in its environment. \
To fetch dependencies, reach the network or write elsewhere, set outside_sandbox with a reason: \
the human reads the command and decides whether it runs.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "command": {"type": "string", "description": "Shell command"},
                        "timeout_seconds": {"type": "integer", "description": "Default 120, at most 600"},
                        "outside_sandbox": {"type": "boolean", "description": "Ask the human to run it outside the sandbox"},
                        "reason": {"type": "string", "description": "Why it must run outside the sandbox"}
                    },
                    "required": ["command"]
                }
            }),
        ]
    }

    pub fn call(&self, name: &str, input: &Value) -> Result<String, String> {
        self.call_with(name, input, &mut |_| {})
    }

    /// `call`, with `on_run` told the id of a run that waits on the human.
    pub fn call_with(
        &self,
        name: &str,
        input: &Value,
        on_run: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        match name {
            "read_file" => {
                let path = self.resolve(required(input, "path")?)?;
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                Ok(truncate(content, MAX_READ))
            }
            "write_file" => {
                let path = self.resolve(required(input, "path")?)?;
                let content = input
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("write_file needs a string content")?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
                }
                std::fs::write(&path, content)
                    .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                Ok(format!("wrote {} bytes to {}", content.len(), path.display()))
            }
            "list_files" => {
                let raw = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                let path = self.resolve(raw)?;
                let mut entries: Vec<String> = Vec::new();
                for entry in std::fs::read_dir(&path)
                    .map_err(|e| format!("could not list {}: {e}", path.display()))?
                {
                    let entry = entry.map_err(|e| format!("could not list: {e}"))?;
                    let mut line = entry.file_name().to_string_lossy().into_owned();
                    if entry.path().is_dir() {
                        line.push('/');
                    }
                    entries.push(line);
                }
                entries.sort();
                entries.truncate(MAX_LIST);
                Ok(entries.join("\n"))
            }
            "grep" => {
                let base = self.resolve(optional(input, "path").unwrap_or("."))?;
                crate::search::grep(
                    &self.root,
                    &base,
                    required(input, "pattern")?,
                    optional(input, "glob"),
                    input
                        .get("ignore_case")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                )
            }
            "glob" => {
                let base = self.resolve(optional(input, "path").unwrap_or("."))?;
                crate::search::glob(&self.root, &base, required(input, "pattern")?)
            }
            "code_map" => {
                let base = self.resolve(optional(input, "path").unwrap_or("."))?;
                crate::search::code_map(&self.root, &base)
            }
            "run_command" => {
                let command = required(input, "command")?;
                let outside = input.get("outside_sandbox").and_then(Value::as_bool) == Some(true);
                let reason = input
                    .get("reason")
                    .and_then(Value::as_str)
                    .map(str::trim)
                    .filter(|reason| !reason.is_empty());
                if outside {
                    let reason = reason.ok_or("outside_sandbox needs a reason for the human")?;
                    return self.ask_human(command, reason, on_run);
                }
                if self.sandbox == Sandbox::None {
                    let reason = reason.unwrap_or("Ram Code has no sandbox on this machine");
                    return self.ask_human(command, reason, on_run);
                }
                self.run_sandboxed(command, timeout_of(input))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    fn home(&self) -> Option<PathBuf> {
        self.env
            .iter()
            .find(|(name, _)| name == "HOME")
            .map(|(_, value)| PathBuf::from(value))
            .filter(|path| path.is_absolute())
    }

    fn run_sandboxed(&self, command: &str, timeout: u64) -> Result<String, String> {
        let temp = self.temp_dir()?;
        let root = self
            .root
            .canonicalize()
            .map_err(|e| format!("could not resolve {}: {e}", self.root.display()))?;
        let mut cmd = self
            .sandbox
            .command(&root, &temp, self.home().as_deref(), command)
            .ok_or("no sandbox on this machine")?;
        cmd.current_dir(&root)
            .env_clear()
            .envs(scrub(&self.env, &self.key_envs))
            .env("TMPDIR", &temp);
        run_with_timeout(cmd, timeout, &self.cancel)
    }

    /// Hand the command to the human through `ramify run`, and wait for the decision.
    fn ask_human(
        &self,
        command: &str,
        reason: &str,
        on_run: &mut dyn FnMut(&str),
    ) -> Result<String, String> {
        let temp = self.temp_dir()?;
        let script = temp.join(format!("run-{}.sh", TEMP_SEQ.fetch_add(1, Ordering::SeqCst)));
        let header: String = reason
            .lines()
            .map(|line| format!("# {line}\n"))
            .collect();
        std::fs::write(
            &script,
            format!("#!/bin/sh\n# Ram Code asks to run this outside the sandbox.\n{header}\n{command}\n"),
        )
        .map_err(|e| format!("could not write {}: {e}", script.display()))?;
        let mut ask = vec!["run".to_string(), "ask".into(), script.display().to_string()];
        ask.extend(["--cwd".into(), self.root.display().to_string(), "--note".into(), reason.into()]);
        if self.no_window {
            ask.push("--no-window".into());
        }
        let asked = self.ramify_output(&ask)?;
        let id = asked
            .1
            .lines()
            .find_map(|line| line.strip_prefix("run "))
            .and_then(|rest| rest.split_whitespace().next())
            .map(str::to_string)
            .ok_or_else(|| format!("ramify run ask did not create a run: {}", asked.1.trim()))?;
        on_run(&id);
        let wait = [
            "run".to_string(),
            "wait".into(),
            id,
            "--timeout".into(),
            self.run_wait.to_string(),
        ];
        let Some((ok, text)) = self.ramify_wait(&wait)? else {
            return Ok(format!(
                "{}; the run stays pending in the Run pane",
                crate::agent::INTERRUPTED
            ));
        };
        if !ok && text.contains("is still") {
            return Ok(NO_DECISION.to_string());
        }
        Ok(truncate(text, MAX_COMMAND))
    }

    /// `ramify run wait`, stopped when the user stops the task. `None` means stopped: the wait
    /// ends, and the run stays pending, since neither `run go` nor `run block` is called.
    fn ramify_wait(&self, args: &[String]) -> Result<Option<(bool, String)>, String> {
        let mut child = Command::new(&self.ramify)
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .envs(scrub(&self.env, &self.key_envs))
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .map_err(|e| format!("could not run {}: {e}", self.ramify))?;
        let stop = Arc::new(AtomicBool::new(false));
        let (stdout, out_thread) = drain(child.stdout.take(), MAX_COMMAND, stop.clone());
        let (stderr, err_thread) = drain(child.stderr.take(), MAX_COMMAND, stop.clone());
        let status = loop {
            match child.try_wait() {
                Ok(Some(status)) => break Some(status),
                Ok(None) if self.cancel.is_set() => {
                    let _ = child.kill();
                    let _ = child.wait();
                    break None;
                }
                Ok(None) => std::thread::sleep(Duration::from_millis(20)),
                Err(error) => return Err(format!("could not wait for {}: {error}", self.ramify)),
            }
        };
        stop.store(true, Ordering::SeqCst);
        let _ = out_thread.join();
        let _ = err_thread.join();
        let Some(status) = status else {
            return Ok(None);
        };
        let take = |buffer: &Captured| {
            buffer
                .lock()
                .map(|b| String::from_utf8_lossy(&b.0).into_owned())
                .unwrap_or_default()
        };
        let mut text = take(&stdout);
        text.push_str(&take(&stderr));
        Ok(Some((status.success(), text)))
    }

    fn ramify_output(&self, args: &[String]) -> Result<(bool, String), String> {
        let output = Command::new(&self.ramify)
            .args(args)
            .current_dir(&self.root)
            .env_clear()
            .envs(scrub(&self.env, &self.key_envs))
            .stdin(Stdio::null())
            .output()
            .map_err(|e| format!("could not run {}: {e}", self.ramify))?;
        let mut text = String::from_utf8_lossy(&output.stdout).into_owned();
        text.push_str(&String::from_utf8_lossy(&output.stderr));
        Ok((output.status.success(), text))
    }

    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let path = Path::new(raw);
        if path.is_absolute() {
            return Err("path must be relative to the worktree".into());
        }
        if path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err("path must stay inside the worktree".into());
        }
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let joined = self.root.join(path);
        let mut probe = joined.clone();
        let mut suffix: Vec<std::ffi::OsString> = Vec::new();
        loop {
            if let Ok(canonical) = probe.canonicalize() {
                let mut full = canonical;
                for part in suffix.iter().rev() {
                    full.push(part);
                }
                if full.starts_with(&root) {
                    return Ok(joined);
                }
                return Err("path must stay inside the worktree".into());
            }
            match (probe.parent(), probe.file_name()) {
                (Some(parent), Some(name)) => {
                    suffix.push(name.to_os_string());
                    probe = parent.to_path_buf();
                }
                _ => return Err("path must stay inside the worktree".into()),
            }
        }
    }
}

/// `timeout_seconds`, between 1 and 600, else 120.
pub fn timeout_of(input: &Value) -> u64 {
    input
        .get("timeout_seconds")
        .and_then(Value::as_u64)
        .map_or(DEFAULT_TIMEOUT, |seconds| seconds.clamp(1, MAX_TIMEOUT))
}

/// How long to keep reading after the command ends, for output a background job still writes.
const DRAIN: Duration = Duration::from_secs(1);

/// The bytes kept from one pipe, and whether more came than the cap allows.
type Captured = Arc<Mutex<(Vec<u8>, bool)>>;

/// Read `pipe` until it closes or `stop` is set. Keep at most `cap` bytes; read and drop the
/// rest, so the writer never blocks on a full pipe. The pipe is non-blocking, so the thread
/// sees `stop` even when a descendant that left the group keeps the pipe open.
fn drain<R: Read + std::os::fd::AsRawFd + Send + 'static>(
    pipe: Option<R>,
    cap: usize,
    stop: Arc<AtomicBool>,
) -> (Captured, std::thread::JoinHandle<()>) {
    let buffer = Arc::new(Mutex::new((Vec::new(), false)));
    let sink = buffer.clone();
    let handle = std::thread::spawn(move || {
        let Some(mut pipe) = pipe else {
            return;
        };
        // SAFETY: fcntl on a file descriptor this thread owns.
        unsafe {
            let fd = pipe.as_raw_fd();
            let flags = libc::fcntl(fd, libc::F_GETFL);
            libc::fcntl(fd, libc::F_SETFL, flags | libc::O_NONBLOCK);
        }
        let mut chunk = [0u8; 8192];
        loop {
            match pipe.read(&mut chunk) {
                Ok(0) => break,
                Ok(read) => {
                    if let Ok(mut sink) = sink.lock() {
                        let room = cap.saturating_sub(sink.0.len());
                        sink.0.extend_from_slice(&chunk[..read.min(room)]);
                        if read > room {
                            sink.1 = true;
                        }
                    }
                }
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                    if stop.load(Ordering::SeqCst) {
                        break;
                    }
                    std::thread::sleep(Duration::from_millis(10));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {}
                Err(_) => break,
            }
        }
    });
    (buffer, handle)
}

/// Run `cmd` in a new session: its own process group, with no controlling terminal. On a
/// timeout, kill the whole group and keep the output.
fn run_with_timeout(mut cmd: Command, timeout: u64, cancel: &Cancel) -> Result<String, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    // SAFETY: setsid is async-signal-safe, which is all pre_exec may call.
    unsafe {
        cmd.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        });
    }
    let mut child = cmd.spawn().map_err(|e| format!("could not run command: {e}"))?;
    let group = child.id() as i32;
    let stop = Arc::new(AtomicBool::new(false));
    let (stdout, out_thread) = drain(child.stdout.take(), MAX_COMMAND, stop.clone());
    let (stderr, err_thread) = drain(child.stderr.take(), MAX_COMMAND, stop.clone());
    let deadline = Instant::now() + Duration::from_secs(timeout);
    let mut interrupted = false;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break Some(status),
            Ok(None) if cancel.is_set() => {
                interrupted = true;
                break None;
            }
            Ok(None) if Instant::now() >= deadline => break None,
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
            Err(error) => return Err(format!("could not wait for the command: {error}")),
        }
    };
    // Kill what is left of the group: a timed-out command, or a background job of a finished one.
    // SAFETY: killpg only sends a signal to the group this function created.
    unsafe {
        libc::killpg(group, libc::SIGKILL);
    }
    if status.is_none() {
        let _ = child.wait();
    }
    // A descendant that left the group can hold the pipes open. Stop reading after a short drain.
    let drained = Instant::now() + DRAIN;
    while !(out_thread.is_finished() && err_thread.is_finished()) && Instant::now() < drained {
        std::thread::sleep(Duration::from_millis(10));
    }
    stop.store(true, Ordering::SeqCst);
    let _ = out_thread.join();
    let _ = err_thread.join();
    let take = |buffer: &Captured| {
        buffer
            .lock()
            .map(|b| (String::from_utf8_lossy(&b.0).into_owned(), b.1))
            .unwrap_or_default()
    };
    let (mut text, out_cut) = take(&stdout);
    let (err_text, err_cut) = take(&stderr);
    text.push_str(&err_text);
    if out_cut || err_cut {
        text.push_str("\n[truncated]");
    }
    let text = match status {
        Some(status) => format!("exit {}\n{text}", status.code().unwrap_or(-1)),
        None if interrupted => format!("{text}\n{}", crate::agent::INTERRUPTED),
        None => format!("{text}\ntimed out after {timeout} s"),
    };
    Ok(truncate(text, MAX_COMMAND))
}

fn required<'a>(input: &'a Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string field: {key}"))
}

/// A non-empty string field, or `None`.
fn optional<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
}

fn truncate(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
    text.push_str("\n[truncated]");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ram-code-tools-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn truncate_keeps_a_char_boundary() {
        let text = "é".repeat(10);
        let cut = truncate(text.clone(), 3);
        assert!(cut.starts_with("é"));
        assert!(cut.ends_with("[truncated]"));
        assert_eq!(cut.trim_end_matches("\n[truncated]"), "é");
    }

    #[test]
    fn a_symlink_outside_the_root_is_rejected() {
        let outside = temp("outside");
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        let root = temp("root");
        symlink(&outside, root.join("link")).unwrap();

        let tools = Tools::new(&root);
        assert!(tools.resolve("link/secret.txt").is_err());
        assert!(tools.call("read_file", &json!({"path": "link/secret.txt"})).is_err());
    }

    #[test]
    fn a_parent_path_is_rejected() {
        let root = temp("parent");
        let tools = Tools::new(&root);
        assert!(tools.resolve("../x").is_err());
        assert!(tools.resolve("/etc/passwd").is_err());
    }
}
