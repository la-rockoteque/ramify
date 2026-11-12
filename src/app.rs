use crate::agent::AgentConfig;
use crate::providers::{missing_key, resolve, unknown, Providers};
use crate::bonsai_art;
use crate::panel::Panel;
use crate::hooked::Hooked;
use crate::ramify::{Action, CommandOutput, CommandSpec, Runner};
use crate::state::{parse_project, project_for_root, Project, Worktree};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Constraint, Layout, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};
use serde_json::Value;
use std::fs::{File, OpenOptions};
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::PathBuf;
use std::process::{Child, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Splash,
    Loading,
    Main,
    Detail,
    Picker,
    Message,
    Empty,
    Error,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MessageKind {
    Info,
    Success,
    Failure,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Role {
    User,
    Assistant,
    Tool,
    ToolResult,
    Error,
    Info,
}

#[derive(Debug, Clone)]
pub struct Entry {
    pub role: Role,
    pub text: String,
}

struct Worker {
    child: Child,
    events: PathBuf,
    history: PathBuf,
    stderr: PathBuf,
    offset: u64,
    pending: Vec<u8>,
    saw_text: bool,
}

impl Drop for Worker {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

struct Loader {
    child: Child,
    stdout: PathBuf,
    stderr: PathBuf,
}

impl Drop for Loader {
    fn drop(&mut self) {
        if self.child.try_wait().ok().flatten().is_none() {
            let _ = self.child.kill();
            let _ = self.child.wait();
        }
    }
}

/// Reads an environment variable.
pub type EnvFn = Arc<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct App {
    pub runner: Arc<dyn Runner>,
    pub root: PathBuf,
    pub project: Option<Project>,
    pub selected: usize,
    pub screen: Screen,
    pub message: String,
    pub message_kind: MessageKind,
    pub detail: Option<Worktree>,
    pub entries: Vec<Entry>,
    pub input: String,
    pub running: bool,
    pub scroll_up: usize,
    pub history: Vec<Value>,
    pub quit: bool,
    pub panel: Panel,
    pub ramify_bin: Option<PathBuf>,
    pub worker_binary: Option<PathBuf>,
    /// Reads an environment variable. Tests replace it.
    pub env: EnvFn,
    pub providers: Providers,
    /// The active provider's name.
    pub provider: String,
    /// The `/model` choice for this session.
    pub session_model: Option<String>,
    /// `provider · model` for the header, kept current so a frame does no file work.
    provider_line: String,
    worker: Option<Worker>,
    loader: Option<Loader>,
    task_seq: u64,
    picker_from: usize,
    /// The `ramify prompt` line of the selected worktree, and when and where it was read.
    status_line: String,
    status_root: Option<PathBuf>,
    status_at: Option<Instant>,
    /// Where the background `ramify prompt` run leaves its line, and whether one is running.
    status_slot: Arc<Mutex<Option<(PathBuf, String)>>>,
    status_busy: Arc<AtomicBool>,
    health_busy: Arc<AtomicBool>,
    /// This session's record, once a task has started: `ramify prompt` shows it as `◆ ram`.
    /// `ram-<pid>`: the session id every hook of this Ram Code gets.
    session_id: String,
    session_started: bool,
    /// The notes of SessionStart, for the transcript. It runs on a thread, so the slot fills
    /// when it is done.
    start_slot: Arc<Mutex<Vec<String>>>,
    /// Where SessionStart leaves the hook snapshot and its context for the tasks.
    start_dir: Option<PathBuf>,
    start_seq: usize,
    /// The next task is the first since SessionStart: it gets the context.
    context_pending: bool,
    /// When `ramify statusline` last refreshed the health cache, and for which worktree.
    health_root: Option<PathBuf>,
    health_at: Option<Instant>,
}

impl App {
    pub fn new(runner: Arc<dyn Runner>, root: PathBuf) -> Self {
        let mut app = Self {
            runner,
            root,
            project: None,
            selected: 0,
            screen: Screen::Splash,
            message: String::new(),
            message_kind: MessageKind::Info,
            detail: None,
            entries: Vec::new(),
            input: String::new(),
            running: false,
            scroll_up: 0,
            history: Vec::new(),
            quit: false,
            panel: Panel::new(),
            ramify_bin: None,
            worker_binary: None,
            env: Arc::new(|key| std::env::var(key).ok()),
            providers: Providers::empty(PathBuf::new()),
            provider: crate::providers::ENV.to_string(),
            session_model: None,
            provider_line: String::new(),
            worker: None,
            loader: None,
            task_seq: 0,
            picker_from: 0,
            status_line: String::new(),
            status_root: None,
            status_at: None,
            status_slot: Arc::new(Mutex::new(None)),
            status_busy: Arc::new(AtomicBool::new(false)),
            health_busy: Arc::new(AtomicBool::new(false)),
            session_id: format!("ram-{}", std::process::id()),
            session_started: false,
            start_slot: Arc::new(Mutex::new(Vec::new())),
            start_dir: None,
            start_seq: 0,
            context_pending: false,
            health_root: None,
            health_at: None,
        };
        let dir = Providers::default_dir(app.env.as_ref());
        app.load_providers(dir);
        app
    }

    /// Read `providers.json` from `dir` and pick the active provider.
    /// An unreadable file leaves the presets, with `env` active, and shows the error.
    pub fn load_providers(&mut self, dir: PathBuf) {
        self.providers = match Providers::load(dir.clone()) {
            Ok(providers) => providers,
            Err(error) => {
                self.push(Role::Error, error);
                Providers::empty(dir)
            }
        };
        self.provider = self.providers.active_name(self.env.as_ref());
        self.refresh_provider_line();
    }

    fn refresh_provider_line(&mut self) {
        self.provider_line = match self.resolve_config() {
            Ok(config) => format!("{} · {}", self.provider, config.model),
            Err(_) => format!("{} · not ready", self.provider),
        };
    }

    /// The active provider's configuration, or the fix when it cannot run.
    pub fn resolve_config(&self) -> Result<AgentConfig, String> {
        let provider = self
            .providers
            .find(&self.provider)
            .ok_or_else(|| unknown(&self.provider, &self.providers))?;
        resolve(&provider, self.env.as_ref(), self.session_model.as_deref())
            .map(|resolved| resolved.config)
    }

    fn list_providers(&mut self) {
        let mut lines = Vec::new();
        for provider in self.providers.all() {
            let mark = if provider.name == self.provider { "●" } else { " " };
            let mut line = format!("{mark} {}", provider.name);
            if let Some(var) = missing_key(&provider, self.env.as_ref()) {
                line.push_str(&format!("  (no key: {var})"));
            }
            lines.push(line);
        }
        lines.push("/provider <name> switches · /model <id> sets the model".into());
        self.push(Role::Info, lines.join("\n"));
    }

    fn switch_provider(&mut self, name: &str) {
        if self.providers.find(name).is_none() {
            self.push(Role::Error, unknown(name, &self.providers));
            return;
        }
        if let Err(error) = self.providers.save_active(name) {
            self.push(Role::Error, error);
            return;
        }
        if self.provider != name {
            crate::agent::strip_thinking(&mut self.history);
        }
        self.provider = name.to_string();
        self.session_model = None;
        self.refresh_provider_line();
        let detail = match self.resolve_config() {
            Ok(config) => format!("provider {name} · {}", config.model),
            Err(error) => format!("provider {name} · {error}"),
        };
        self.push(Role::Info, detail);
    }

    pub fn load(&mut self) {
        self.screen = Screen::Loading;
        let runner = self.runner.clone();
        let spec = CommandSpec {
            root: self.root.clone(),
            args: vec!["dash-project".into(), "--json".into()],
        };
        match runner.run(&spec) {
            Err(error) => self.fail_load(format!("could not run Ramify: {error}")),
            Ok(output) if !output.ok() => self.apply_dash_error(&output.text()),
            Ok(output) => self.apply_dashboard(&output.stdout),
        }
    }

    /// The interface uses this: the command runs in a child, so Loading is drawn.
    pub fn load_async(&mut self) {
        self.screen = Screen::Loading;
        let directory = match task_directory() {
            Ok(directory) => directory,
            Err(error) => {
                self.fail_load(error);
                return;
            }
        };
        let stdout = directory.join(format!("load-{}.json", self.task_seq));
        let stderr = directory.join(format!("load-{}.log", self.task_seq));
        self.task_seq += 1;
        let out = match File::create(&stdout) {
            Ok(file) => file,
            Err(error) => {
                self.fail_load(format!("could not open the load log: {error}"));
                return;
            }
        };
        let err = match File::create(&stderr) {
            Ok(file) => file,
            Err(error) => {
                self.fail_load(format!("could not open the load log: {error}"));
                return;
            }
        };
        let child = std::process::Command::new(self.ramify_binary())
            .args(["dash-project", "--json"])
            .current_dir(&self.root)
            .env("RAMIFY_ROOT", &self.root)
            .stdin(Stdio::null())
            .stdout(Stdio::from(out))
            .stderr(Stdio::from(err))
            .spawn();
        match child {
            Ok(child) => {
                self.loader = Some(Loader {
                    child,
                    stdout,
                    stderr,
                })
            }
            Err(error) => self.fail_load(format!("could not run Ramify: {error}")),
        }
    }

    fn ramify_binary(&self) -> PathBuf {
        self.ramify_bin
            .clone()
            .or_else(|| std::env::var_os("RAM_CODE_RAMIFY").map(PathBuf::from))
            .unwrap_or_else(|| PathBuf::from("ramify"))
    }

    fn apply_dashboard(&mut self, json: &str) {
        match parse_project(json) {
            Err(error) => self.fail_load(format!("could not read Ramify state: {error}")),
            Ok(project) => {
                if project_for_root(std::slice::from_ref(&project), &self.root).is_none() {
                    self.project = None;
                    self.screen = Screen::Empty;
                } else {
                    self.project = Some(project);
                    self.screen = Screen::Main;
                    self.clamp_selection();
                }
            }
        }
    }

    fn apply_dash_error(&mut self, text: &str) {
        if text.contains("ramify setup") || text.contains("not inside a git checkout") {
            self.project = None;
            self.screen = Screen::Empty;
        } else {
            self.fail_load(format!("could not read Ramify state: {text}"));
        }
    }

    fn fail_load(&mut self, message: String) {
        self.screen = Screen::Error;
        self.message = message;
        self.message_kind = MessageKind::Failure;
    }

    pub fn next(&mut self) {
        let len = self.worktrees().len();
        if len > 0 {
            self.selected = (self.selected + 1) % len;
        }
    }

    pub fn previous(&mut self) {
        let len = self.worktrees().len();
        if len > 0 {
            self.selected = (self.selected + len - 1) % len;
        }
    }

    fn clamp_selection(&mut self) {
        let len = self.worktrees().len();
        if self.selected >= len {
            self.selected = len.saturating_sub(1);
        }
    }

    pub fn worktrees(&self) -> &[Worktree] {
        self.project
            .as_ref()
            .map(|project| project.worktrees.as_slice())
            .unwrap_or(&[])
    }

    pub fn selected_worktree(&self) -> Option<&Worktree> {
        self.worktrees().get(self.selected)
    }

    pub fn select(&mut self, index: usize) {
        self.selected = index;
        self.clamp_selection();
    }

    pub fn open_selected(&mut self) {
        match self.selected_worktree() {
            Some(worktree) => {
                self.detail = Some(worktree.clone());
                self.screen = Screen::Detail;
            }
            None => {
                self.message = "select a worktree first".into();
                self.message_kind = MessageKind::Failure;
                self.screen = Screen::Message;
            }
        }
    }

    pub fn invoke(&mut self, action: Action) {
        let Some(worktree) = self.selected_worktree().cloned() else {
            self.message = "select a worktree first".into();
            self.message_kind = MessageKind::Failure;
            self.screen = Screen::Message;
            return;
        };
        let result = self.run_command(action, worktree.path.clone());
        match result {
            Err(error) => {
                self.message = error;
                self.message_kind = MessageKind::Failure;
            }
            Ok(output) if output.ok() => {
                let text = output.text();
                self.message = if text.is_empty() { "done".into() } else { text };
                self.message_kind = MessageKind::Success;
            }
            Ok(output) => {
                let text = output.text();
                self.message = if text.is_empty() {
                    format!("ramify exited {}", output.status)
                } else {
                    text
                };
                self.message_kind = MessageKind::Failure;
            }
        }
        self.screen = Screen::Message;
    }

    pub fn run_command(&mut self, action: Action, root: PathBuf) -> Result<CommandOutput, String> {
        self.runner.run(&CommandSpec {
            root,
            args: action.args(),
        })
    }

    pub fn start_task(&mut self, config: Result<AgentConfig, String>) {
        let task = self.input.trim().to_string();
        self.start_task_with_text(config, task);
    }

    pub fn start_task_with_text(&mut self, config: Result<AgentConfig, String>, task: String) {
        if let Err(error) = config {
            self.message = error;
            self.message_kind = MessageKind::Failure;
            self.screen = Screen::Message;
            return;
        }
        if self.running {
            self.push(Role::Info, "a task is still running");
            return;
        }
        if task.trim().is_empty() {
            self.message = "type a task first".into();
            self.message_kind = MessageKind::Failure;
            self.screen = Screen::Message;
            return;
        }
        let Some(worktree) = self.selected_worktree().cloned() else {
            self.message = "select a worktree first".into();
            self.message_kind = MessageKind::Failure;
            self.screen = Screen::Message;
            return;
        };

        self.push(Role::User, task.clone());
        self.running = true;
        self.input.clear();

        let directory = match task_directory() {
            Ok(directory) => directory,
            Err(error) => {
                self.push(Role::Error, error);
                self.running = false;
                return;
            }
        };
        let history = directory.join("history.json");
        let events = directory.join(format!("events-{}.jsonl", self.task_seq));
        let stderr = directory.join(format!("stderr-{}.log", self.task_seq));
        self.task_seq += 1;
        match OpenOptions::new().write(true).create_new(true).open(&history) {
            Ok(mut file) => {
                if let Ok(text) = serde_json::to_string(&self.history) {
                    let _ = file.write_all(text.as_bytes());
                }
            }
            Err(error) => {
                self.push(Role::Error, format!("could not write the history: {error}"));
                self.running = false;
                return;
            }
        }
        let stderr_file = match File::create(&stderr) {
            Ok(file) => file,
            Err(error) => {
                self.push(Role::Error, format!("could not open the task log: {error}"));
                self.running = false;
                return;
            }
        };
        let binary = self
            .worker_binary
            .clone()
            .or_else(|| std::env::current_exe().ok());
        let Some(binary) = binary else {
            self.push(Role::Error, "could not find the ram-code binary");
            self.running = false;
            return;
        };
        let mut command = std::process::Command::new(binary);
        command
            .arg("--task")
            .arg(&task)
            .arg("--events")
            .arg(&events)
            .arg("--history")
            .arg(&history)
            .current_dir(&worktree.path)
            .env("RAMIFY_ROOT", &worktree.path)
            .env("RAM_CODE_SESSION", &self.session_id)
            .env("RAM_CODE_PROVIDER", &self.provider)
            .env("RAM_CODE_CONFIG_DIR", &self.providers.dir)
            .envs(self.session_model.iter().map(|model| ("RAM_CODE_MODEL", model)))
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(stderr_file));
        if let Some(dir) = &self.start_dir {
            command.env("RAM_CODE_START", dir);
            if std::mem::take(&mut self.context_pending) {
                command.arg("--context").arg(dir.join(crate::hooked::CONTEXT));
            }
        }
        let child = command.spawn();
        match child {
            Ok(child) => {
                self.worker = Some(Worker {
                    child,
                    events,
                    history,
                    stderr,
                    offset: 0,
                    pending: Vec::new(),
                    saw_text: false,
                });
            }
            Err(error) => {
                self.push(Role::Error, format!("could not start the task: {error}"));
                self.running = false;
            }
        }
    }

    pub fn poll_events(&mut self) {
        self.poll_loader();
        let notes = self
            .start_slot
            .lock()
            .map(|mut slot| std::mem::take(&mut *slot))
            .unwrap_or_default();
        for note in notes {
            self.push(Role::Info, note);
        }

        let (mut parsed, mut saw_finished) = self.read_worker_events();
        let exit_status = self
            .worker
            .as_mut()
            .and_then(|worker| worker.child.try_wait().ok().flatten());
        if exit_status.is_some() && !saw_finished {
            let (more, finished) = self.read_worker_events();
            parsed.extend(more);
            saw_finished |= finished;
        }

        for value in parsed {
            self.apply_worker_value(value);
        }
        if saw_finished {
            self.finish_task();
        } else if let Some(status) = exit_status {
            let was_running = self.running;
            let stderr = self.worker.as_ref().map(|worker| worker.stderr.clone());
            self.worker = None;
            self.running = false;
            self.task_ended();
            if was_running {
                let tail = stderr
                    .and_then(|path| std::fs::read_to_string(path).ok())
                    .unwrap_or_default();
                let text = if tail.trim().is_empty() {
                    format!("the task process exited with {status}")
                } else {
                    tail
                };
                self.push(Role::Error, text);
            }
        }
    }

    fn poll_loader(&mut self) {
        let Some(loader) = self.loader.as_mut() else {
            return;
        };
        let status = match loader.child.try_wait() {
            Ok(Some(status)) => status,
            _ => return,
        };
        let stdout = std::fs::read_to_string(&loader.stdout).unwrap_or_default();
        let stderr = std::fs::read_to_string(&loader.stderr).unwrap_or_default();
        self.loader = None;
        if status.success() {
            self.apply_dashboard(&stdout);
        } else {
            self.apply_dash_error(&stderr);
        }
    }

    fn read_worker_events(&mut self) -> (Vec<Value>, bool) {
        let mut parsed = Vec::new();
        let mut finished = false;
        let Some(worker) = self.worker.as_mut() else {
            return (parsed, finished);
        };
        if let Ok(mut file) = File::open(&worker.events) {
            if file.seek(SeekFrom::Start(worker.offset)).is_ok() {
                let mut bytes = Vec::new();
                if file.read_to_end(&mut bytes).is_ok() {
                    worker.offset += bytes.len() as u64;
                    worker.pending.extend_from_slice(&bytes);
                }
            }
        }
        while let Some(index) = worker.pending.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = worker.pending.drain(..=index).collect();
            let text = String::from_utf8_lossy(&line[..line.len().saturating_sub(1)])
                .trim()
                .to_string();
            if text.is_empty() {
                continue;
            }
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                if value.get("type").and_then(Value::as_str) == Some("finished") {
                    finished = true;
                }
                parsed.push(value);
            }
        }
        (parsed, finished)
    }

    fn apply_worker_value(&mut self, value: Value) {
        match value.get("type").and_then(Value::as_str) {
            Some("text") => {
                let text = value
                    .get("text")
                    .and_then(Value::as_str)
                    .unwrap_or("")
                    .to_string();
                if let Some(worker) = self.worker.as_mut() {
                    worker.saw_text = true;
                }
                self.push(Role::Assistant, text);
            }
            Some("tool_call") => {
                let name = value.get("name").and_then(Value::as_str).unwrap_or("");
                let input = value.get("input").and_then(Value::as_str).unwrap_or("");
                self.push(Role::Tool, format!("{name} {input}"));
            }
            Some("hook") => {
                self.push(
                    Role::Info,
                    value.get("text").and_then(Value::as_str).unwrap_or(""),
                );
            }
            Some("tool_result") => {
                let text = value.get("text").and_then(Value::as_str).unwrap_or("");
                let head: Vec<&str> = text.lines().take(2).collect();
                self.push(Role::ToolResult, head.join("\n"));
            }
            Some("error") => {
                self.push(
                    Role::Error,
                    value.get("text").and_then(Value::as_str).unwrap_or(""),
                );
                self.running = false;
            }
            Some("finished") => {
                let answer = value.get("answer").and_then(Value::as_str).unwrap_or("");
                let saw_text = self
                    .worker
                    .as_ref()
                    .map(|worker| worker.saw_text)
                    .unwrap_or(false);
                if !saw_text && !answer.is_empty() {
                    self.push(Role::Assistant, answer);
                }
            }
            _ => {}
        }
    }

    fn finish_task(&mut self) {
        let Some(mut worker) = self.worker.take() else {
            return;
        };
        let _ = worker.child.wait();
        let directory = worker.history.parent().map(|path| path.to_path_buf());
        if let Ok(text) = std::fs::read_to_string(&worker.history) {
            if let Ok(history) = serde_json::from_str::<Vec<Value>>(&text) {
                self.history = history;
            }
        }
        if let Some(directory) = directory {
            let _ = std::fs::remove_dir_all(directory);
        }
        self.running = false;
        self.task_ended();
    }

    fn task_ended(&mut self) {
        // The task may have changed the branch or the stack: read the line again.
        self.status_at = None;
        self.mark_idle();
    }

    /// A task that ends on an error, a cancel or the turn limit fires no Stop, so ramify's hook
    /// leaves the record `working`. The task is over: the record says `idle`.
    fn mark_idle(&self) {
        let state = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
        crate::hooked::idle_record(std::path::Path::new(&state), &self.session_id);
    }

    fn hooks_cwd(&self) -> PathBuf {
        self.selected_worktree()
            .map(|worktree| worktree.path.clone())
            .unwrap_or_else(|| self.root.clone())
    }

    /// Fires SessionStart on a thread: Ram Code starts, or `/clear` starts afresh. The
    /// SessionStart hooks of ramify write this session's record. The thread leaves the hook
    /// snapshot and the context in a new start folder; each task waits for them there.
    pub fn start_session(&mut self, source: &str) {
        self.session_started = true;
        self.context_pending = true;
        if let Some(old) = self.start_dir.take() {
            let _ = std::fs::remove_dir_all(old);
        }
        self.start_seq += 1;
        let dir = std::env::temp_dir().join(format!("ram-code-{}-{}", self.session_id, self.start_seq));
        let _ = std::fs::remove_dir_all(&dir);
        self.start_dir = Some(dir.clone());
        let (cwd, session, slot, source) = (
            self.hooks_cwd(),
            self.session_id.clone(),
            self.start_slot.clone(),
            source.to_string(),
        );
        std::thread::spawn(move || {
            let hooks = Hooked::load(&cwd, &session);
            let mut outcome = hooks.fire("SessionStart", &source, serde_json::json!({"source": source}));
            outcome.notes.extend(hooks.set.warnings.iter().cloned());
            if let Err(error) = hooks.leave_start(&dir, &outcome.contexts) {
                outcome.notes.push(format!("hooks: could not write {}: {error}", dir.display()));
            }
            if let Ok(mut slot) = slot.lock() {
                slot.extend(outcome.notes);
            }
        });
    }

    /// Fires SessionEnd, and waits for it at most 5 seconds: quitting must not hang on a hook.
    pub fn end_session(&mut self) {
        if !std::mem::take(&mut self.session_started) {
            return;
        }
        let (cwd, session, dir) = (self.hooks_cwd(), self.session_id.clone(), self.start_dir.take());
        let (done, wait) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let hooks = Hooked::for_task(&cwd, &session, dir.as_deref());
            let _ = hooks.fire("SessionEnd", "", serde_json::json!({"reason": "exit"}));
            if let Some(dir) = dir {
                let _ = std::fs::remove_dir_all(dir);
            }
            let _ = done.send(());
        });
        let _ = wait.recv_timeout(Duration::from_secs(5));
    }

    /// `ramify prompt` takes tens of milliseconds, so it runs off the drawing thread, one run at
    /// a time. The line of another worktree is never shown: a new selection clears it.
    fn refresh_status(&mut self) {
        let Some(root) = self.selected_worktree().map(|worktree| worktree.path.clone()) else {
            self.status_line.clear();
            self.status_root = None;
            return;
        };
        if let Some((from, line)) = self.status_slot.lock().ok().and_then(|mut slot| slot.take()) {
            if from == root {
                self.status_line = line;
            }
        }
        if self.status_root.as_ref() != Some(&root) {
            self.status_line.clear();
            self.status_root = Some(root.clone());
            self.status_at = None;
        }
        let fresh = self
            .status_at
            .is_some_and(|at| at.elapsed() < Duration::from_secs(5));
        if fresh || self.status_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.status_at = Some(Instant::now());
        let (runner, slot, busy) = (self.runner.clone(), self.status_slot.clone(), self.status_busy.clone());
        std::thread::spawn(move || {
            let line = runner
                .run(&CommandSpec {
                    root: root.clone(),
                    args: vec!["prompt".into()],
                })
                .ok()
                .filter(|output| output.ok())
                .and_then(|output| output.stdout.lines().next().map(str::to_string))
                .unwrap_or_default();
            if let Ok(mut slot) = slot.lock() {
                *slot = Some((root, line));
            }
            busy.store(false, Ordering::SeqCst);
        });
    }

    fn push(&mut self, role: Role, text: impl Into<String>) {
        self.entries.push(Entry {
            role,
            text: text.into(),
        });
        self.scroll_up = 0;
    }

    pub fn back(&mut self) {
        match self.screen {
            Screen::Detail | Screen::Picker => {
                self.detail = None;
                self.screen = Screen::Main;
            }
            Screen::Message => {
                self.load();
            }
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('c'))
        {
            self.quit = true;
            return;
        }
        if self.screen == Screen::Main
            && key.modifiers.contains(KeyModifiers::CONTROL)
            && matches!(key.code, KeyCode::Char('p'))
        {
            self.panel.toggle();
            return;
        }
        if self.screen == Screen::Main && self.panel.open {
            if let Some(worktree) = self.selected_worktree() {
                self.panel.set_worktree(Some(worktree.path.clone()));
            }
            if self.panel.on_key(key, &*self.runner, &self.root) {
                if let Some(root) = self.panel.take_open() {
                    self.open_root(root);
                }
                return;
            }
        }
        match self.screen {
            Screen::Main => self.key_chat(key),
            Screen::Picker => self.key_picker(key),
            Screen::Message => match key.code {
                KeyCode::Enter | KeyCode::Esc => self.back(),
                KeyCode::Char('q') => self.quit = true,
                _ => {}
            },
            Screen::Empty | Screen::Error => match key.code {
                KeyCode::Char('r') => self.load_async(),
                KeyCode::Char('q') => self.quit = true,
                _ => {}
            },
            Screen::Detail => match key.code {
                KeyCode::Esc => self.back(),
                KeyCode::Char('q') => self.quit = true,
                KeyCode::Char('u') => self.invoke(Action::Up),
                KeyCode::Char('d') => self.invoke(Action::Down),
                KeyCode::Char('s') => self.invoke(Action::Step),
                _ => {}
            },
            _ => {}
        }
    }

    fn key_chat(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Enter => self.submit(),
            KeyCode::Backspace => {
                self.input.pop();
            }
            KeyCode::Tab if !self.running => {
                self.picker_from = self.selected;
                self.screen = Screen::Picker;
            }
            KeyCode::Up => self.scroll_up = self.scroll_up.saturating_add(1),
            KeyCode::Down => self.scroll_up = self.scroll_up.saturating_sub(1),
            KeyCode::Char('o')
                if key.modifiers.contains(KeyModifiers::CONTROL) && self.input.is_empty() =>
            {
                self.open_selected()
            }
            KeyCode::Char(character) if !self.running => self.input.push(character),
            _ => {}
        }
    }

    fn key_picker(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc | KeyCode::Tab => {
                self.selected = self.picker_from;
                self.clamp_selection();
                self.screen = Screen::Main;
            }
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            KeyCode::Enter => {
                let name = self
                    .selected_worktree()
                    .map(|worktree| worktree.name.clone())
                    .unwrap_or_default();
                self.history.clear();
                self.entries.clear();
                self.push(Role::Info, format!("session: {name}"));
                self.screen = Screen::Main;
            }
            KeyCode::Char('q') => self.quit = true,
            _ => {}
        }
    }

    fn submit(&mut self) {
        if self.running {
            return;
        }
        let text = self.input.trim().to_string();
        if text.is_empty() {
            return;
        }
        self.input.clear();
        if text.starts_with('/') {
            self.handle_command(&text);
            return;
        }
        let config = self.resolve_config();
        self.start_task_with_text(config, text);
    }

    fn handle_command(&mut self, text: &str) {
        let (command, rest) = match text.split_once(' ') {
            Some((command, rest)) => (command, rest.trim()),
            None => (text, ""),
        };
        match command {
            "/help" => self.push(
                Role::Info,
                "enter send · tab worktrees · i inspect · ↑/↓ scroll · ^C quit\n\
                 /up /down /step /status /workflow /new <slug> /refresh /worktrees\n\
                 /provider [name] /model <id> /clear /quit",
            ),
            "/up" => self.run_action_inline(Action::Up),
            "/down" => self.run_action_inline(Action::Down),
            "/step" => self.run_action_inline(Action::Step),
            "/status" => self.run_action_inline(Action::Status),
            "/workflow" => self.run_action_inline(Action::Workflow),
            "/new" => {
                if rest.is_empty() {
                    self.push(Role::Info, "usage: /new <slug>");
                } else {
                    self.run_action_inline(Action::CreateWorktree(rest.to_string()));
                }
            }
            "/refresh" => self.load_async(),
            "/worktrees" => self.screen = Screen::Picker,
            "/clear" => {
                self.entries.clear();
                self.history.clear();
                self.scroll_up = 0;
                if self.session_started {
                    self.start_session("clear");
                }
            }
            "/provider" => {
                if rest.is_empty() {
                    self.list_providers();
                } else {
                    self.switch_provider(rest);
                }
            }
            "/model" => {
                if rest.is_empty() {
                    self.push(Role::Info, "usage: /model <id>");
                } else {
                    self.session_model = Some(rest.to_string());
                    self.refresh_provider_line();
                    self.push(Role::Info, format!("model {rest} for this session"));
                }
            }
            "/quit" => self.quit = true,
            other => self.push(Role::Info, format!("unknown command {other}; try /help")),
        }
    }

    fn run_action_inline(&mut self, action: Action) {
        let Some(worktree) = self.selected_worktree().cloned() else {
            self.push(Role::Info, "select a worktree first");
            return;
        };
        match self.run_command(action, worktree.path.clone()) {
            Err(error) => self.push(Role::Error, error),
            Ok(output) if output.ok() => {
                let text = output.text();
                self.push(Role::Info, if text.is_empty() { "done".into() } else { text });
                self.load_async();
            }
            Ok(output) => {
                let text = output.text();
                self.push(
                    Role::Error,
                    if text.is_empty() {
                        format!("ramify exited {}", output.status)
                    } else {
                        text
                    },
                );
            }
        }
    }

    pub fn tick(&mut self) {
        if let Some(worktree) = self.selected_worktree() {
            self.panel.set_worktree(Some(worktree.path.clone()));
        }
        self.panel.tick(&*self.runner, &self.root);
        self.refresh_health();
        self.refresh_status();
    }

    /// `ramify prompt` makes no HTTP request: it shows the health that `ramify statusline` last
    /// cached. Ram Code runs `statusline` every 30 seconds, off the drawing thread, so the
    /// header and the shell prompts in that worktree show a live stack.
    fn refresh_health(&mut self) {
        let Some(root) = self.selected_worktree().map(|worktree| worktree.path.clone()) else {
            return;
        };
        let fresh = self.health_root.as_ref() == Some(&root)
            && self
                .health_at
                .is_some_and(|at| at.elapsed() < Duration::from_secs(30));
        if fresh || self.health_busy.swap(true, Ordering::SeqCst) {
            return;
        }
        self.health_root = Some(root.clone());
        self.health_at = Some(Instant::now());
        let (runner, busy) = (self.runner.clone(), self.health_busy.clone());
        std::thread::spawn(move || {
            let _ = runner.run(&CommandSpec {
                root,
                args: vec!["statusline".into()],
            });
            busy.store(false, Ordering::SeqCst);
        });
    }

    pub fn open_root(&mut self, root: PathBuf) {
        self.root = root;
        self.history.clear();
        self.entries.clear();
        self.project = None;
        self.selected = 0;
        self.load();
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        match self.screen {
            Screen::Splash => self.render_splash(area, buf),
            Screen::Main if self.panel.open => {
                let chunks = Layout::horizontal([
                    Constraint::Min(1),
                    Constraint::Length(40),
                ])
                .split(area);
                self.render_chat(chunks[0], buf);
                self.panel.render(chunks[1], buf);
            }
            Screen::Main => self.render_chat(area, buf),
            Screen::Picker => self.render_picker(area, buf),
            _ => self.render_panel(area, buf),
        }
    }

    fn render_splash(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().title(" Ram Code ");
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(splash_lines(inner.height as usize, inner.width as usize))
            .alignment(Alignment::Center)
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }

    fn render_chat(&self, area: Rect, buf: &mut Buffer) {
        let chunks = Layout::vertical([
            Constraint::Length(4),
            Constraint::Min(1),
            Constraint::Length(3),
        ])
        .split(area);
        self.render_header(chunks[0], buf);
        self.render_transcript(chunks[1], buf);
        self.render_input(chunks[2], buf);
    }

    fn render_header(&self, area: Rect, buf: &mut Buffer) {
        let mut lines = Vec::new();
        let name = self
            .project
            .as_ref()
            .map(|project| project.name.as_str())
            .unwrap_or("no project");
        let path = self
            .project
            .as_ref()
            .map(|project| project.primary.display().to_string())
            .unwrap_or_default();
        lines.push(Line::from(vec![
            Span::styled(
                " Ram Code ",
                Style::default()
                    .fg(Color::Black)
                    .bg(accent())
                    .add_modifier(Modifier::BOLD),
            ),
            Span::raw(format!("  {name}  {path}")),
            Span::styled(
                format!("  {}", self.provider_line),
                Style::default().fg(Color::DarkGray),
            ),
        ]));
        let mut chips = Vec::new();
        if let Some(project) = &self.project {
            for (index, worktree) in project.worktrees.iter().enumerate() {
                if index > 0 {
                    chips.push(Span::raw(" "));
                }
                let style = if index == self.selected {
                    Style::default().fg(Color::Black).bg(accent())
                } else {
                    Style::default().fg(Color::DarkGray)
                };
                chips.push(Span::styled(format!(" {} ", worktree.name), style));
            }
        }
        lines.push(Line::from(chips));
        lines.push(Line::from(Span::raw(format!(" {}", self.status_line))));
        lines.push(Line::from(Span::styled(
            "─".repeat(area.width as usize),
            Style::default().fg(Color::DarkGray),
        )));
        Paragraph::new(lines).render(area, buf);
    }

    fn render_transcript(&self, area: Rect, buf: &mut Buffer) {
        let lines = self.transcript_lines();
        let height = area.height as usize;
        let base = lines.len().saturating_sub(height);
        let offset = base.saturating_sub(self.scroll_up);
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .scroll((offset as u16, 0))
            .render(area, buf);
    }

    fn render_input(&self, area: Rect, buf: &mut Buffer) {
        let hint = if self.running {
            " working… · ↑/↓ scroll · ^C quit "
        } else {
            " enter send · tab worktrees · ^O inspect · /help · ^C quit "
        };
        let block = Block::bordered()
            .border_style(Style::default().fg(accent()))
            .title(Span::styled(" message ", Style::default().fg(accent())))
            .title_bottom(Line::from(Span::styled(
                hint,
                Style::default().fg(Color::DarkGray),
            )));
        let inner = block.inner(area);
        block.render(area, buf);
        let line = if self.running {
            Line::from(vec![
                Span::styled("⏳ ", Style::default().fg(accent())),
                Span::styled("working…", Style::default().fg(Color::DarkGray)),
            ])
        } else {
            Line::from(vec![
                Span::styled("❯ ", Style::default().fg(accent())),
                Span::raw(self.input.clone()),
                Span::styled("▊", Style::default().fg(accent())),
            ])
        };
        Paragraph::new(line).render(inner, buf);
    }

    fn render_picker(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .title(" worktrees ")
            .title_bottom(Line::from(" enter select · j/k move · esc back "));
        let inner = block.inner(area);
        block.render(area, buf);
        let mut lines = Vec::new();
        for (index, worktree) in self.worktrees().iter().enumerate() {
            let marker = if index == self.selected { "❯" } else { " " };
            let dirty = if worktree.dirty { "dirty" } else { "" };
            let text = format!(
                "{marker} {:<20} {:<24} {dirty}",
                worktree.name, worktree.branch
            );
            let style = if index == self.selected {
                Style::default().fg(accent()).add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            lines.push(Line::from(Span::styled(text, style)));
        }
        Paragraph::new(lines).render(inner, buf);
    }

    fn render_panel(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().title(self.title());
        let inner = block.inner(area);
        block.render(area, buf);
        Paragraph::new(self.lines())
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }

    fn title(&self) -> String {
        match self.screen {
            Screen::Splash => " Ram Code ".into(),
            Screen::Main => " Ram Code ".into(),
            Screen::Detail => format!(
                " Ram Code — {} ",
                self.detail
                    .as_ref()
                    .map(|worktree| worktree.name.as_str())
                    .unwrap_or("worktree")
            ),
            Screen::Picker => " Ram Code — worktrees ".into(),
            Screen::Message => " Ram Code — result ".into(),
            Screen::Empty => " Ram Code — no project ".into(),
            Screen::Error => " Ram Code — error ".into(),
            Screen::Loading => " Ram Code ".into(),
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        match self.screen {
            Screen::Loading => vec![Line::from("Reading Ramify state…")],
            Screen::Empty => vec![
                Line::from("No Ramify project here."),
                Line::from("Run `ramify setup` in this repository, then press r."),
                Line::from(""),
                Line::from("r retry · q quit"),
            ],
            Screen::Error => vec![
                Line::from(self.message.clone()),
                Line::from(""),
                Line::from("r retry · q quit"),
            ],
            Screen::Detail => self.detail_lines(),
            Screen::Message => {
                let mut lines: Vec<Line<'static>> = self
                    .message
                    .lines()
                    .map(|line| Line::from(line.to_string()))
                    .collect();
                if lines.is_empty() {
                    lines.push(Line::from("done"));
                }
                let style = match self.message_kind {
                    MessageKind::Success => Style::default().fg(Color::Green),
                    MessageKind::Failure => Style::default().fg(Color::Red),
                    MessageKind::Info => Style::default(),
                };
                for line in &mut lines {
                    *line = line.clone().style(style);
                }
                lines.push(Line::from(""));
                lines.push(Line::from("enter or esc back · q quit"));
                lines
            }
            _ => vec![Line::from("")],
        }
    }

    fn transcript_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if self.entries.is_empty() {
            lines.push(Line::from(Span::styled(
                "Ask for a change in the selected worktree.",
                Style::default().fg(Color::Gray),
            )));
            lines.push(Line::from(""));
            lines.push(Line::from(Span::styled(
                "Press /help for commands, Tab to change the worktree.",
                Style::default().fg(Color::DarkGray),
            )));
        }
        for entry in &self.entries {
            let text = entry.text.lines().take(40);
            match entry.role {
                Role::User => lines.push(Line::from(vec![
                    Span::styled(
                        "❯ ",
                        Style::default().fg(accent()).add_modifier(Modifier::BOLD),
                    ),
                    Span::raw(text.collect::<Vec<_>>().join("\n")),
                ])),
                Role::Assistant => {
                    push_prefixed(&mut lines, text, "⏺ ", Style::default().fg(accent()))
                }
                Role::Tool => push_prefixed(
                    &mut lines,
                    text,
                    "⏺ ",
                    Style::default().fg(Color::DarkGray),
                ),
                Role::ToolResult => push_prefixed(
                    &mut lines,
                    text,
                    "  ⎿ ",
                    Style::default().fg(Color::DarkGray),
                ),
                Role::Error => push_prefixed(&mut lines, text, "⏺ ", Style::default().fg(Color::Red)),
                Role::Info => push_prefixed(
                    &mut lines,
                    text,
                    "· ",
                    Style::default().fg(Color::DarkGray),
                ),
            }
            lines.push(Line::from(""));
        }
        lines
    }

    fn detail_lines(&self) -> Vec<Line<'static>> {
        let Some(worktree) = &self.detail else {
            return vec![Line::from("no worktree selected")];
        };
        let mut lines = vec![
            Line::from(format!("Worktree  {}", worktree.name)),
            Line::from(format!("Path      {}", worktree.path.display())),
            Line::from(format!("Branch    {}", worktree.branch)),
            Line::from(format!(
                "State     {}",
                if worktree.dirty { "dirty" } else { "clean" }
            )),
        ];
        if let Some(slot) = worktree.slot {
            lines.push(Line::from(format!("Slot      {slot}")));
        }
        if worktree.services.is_empty() {
            lines.push(Line::from("Services  none"));
        } else {
            let services: Vec<String> = worktree
                .services
                .iter()
                .map(|service| {
                    format!(
                        "{} :{} {}",
                        service.service,
                        service.port,
                        if service.up { "up" } else { "down" }
                    )
                })
                .collect();
            lines.push(Line::from(format!("Services  {}", services.join(", "))));
        }
        match &worktree.workflow {
            Some(workflow) => {
                lines.push(Line::from(format!(
                    "Workflow  {} · {}",
                    workflow.name, workflow.lane
                )));
                match &workflow.next {
                    Some(step) => {
                        lines.push(Line::from(format!("Next step {}", step.step)));
                        lines.push(Line::from(format!("Skill     {}", step.skill)));
                    }
                    None => lines.push(Line::from("Next step none")),
                }
            }
            None => lines.push(Line::from("Workflow  none")),
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "u up · d down · s step · esc back · q quit",
        ));
        lines
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        App::render(self, area, buf);
    }
}

fn task_directory() -> Result<PathBuf, String> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    let path = std::env::temp_dir().join(format!(
        "ram-code-{}-{nanos}",
        std::process::id()
    ));
    let mut builder = std::fs::DirBuilder::new();
    #[cfg(unix)]
    {
        use std::os::unix::fs::DirBuilderExt;
        builder.mode(0o700);
    }
    builder
        .create(&path)
        .map_err(|error| format!("could not create the task folder: {error}"))?;
    Ok(path)
}

fn accent() -> Color {
    Color::Rgb(215, 119, 87)
}

fn push_prefixed<'a>(
    lines: &mut Vec<Line<'static>>,
    text: impl Iterator<Item = &'a str>,
    prefix: &'static str,
    style: Style,
) {
    let mut first = true;
    for part in text {
        let mark = if first {
            Span::styled(prefix, style)
        } else {
            Span::raw(" ".repeat(prefix.chars().count()))
        };
        lines.push(Line::from(vec![mark, Span::raw(part.to_string())]));
        first = false;
    }
}

const RAM_CODE_ART: &[&str] = &[
    "██████╗  █████╗ ███╗   ███╗    ██████╗  ██████╗ ██████╗ ███████╗",
    "██╔══██╗██╔══██╗████╗ ████║   ██╔════╝ ██╔═══██╗██╔══██╗██╔════╝",
    "██████╔╝███████║██╔████╔██║   ██║      ██║   ██║██║  ██║█████╗  ",
    "██╔══██╗██╔══██║██║╚██╔╝██║   ██║      ██║   ██║██║  ██║██╔══╝  ",
    "██║  ██║██║  ██║██║ ╚═╝ ██║   ╚██████╗ ╚██████╔╝██████╔╝███████╗",
    "╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝    ╚═════╝  ╚═════╝ ╚═════╝ ╚══════╝",
];

/// Builds the splash for an inner area of `height` x `width` cells. It shows the
/// largest bonsai that fits with the wordmark; failing that, the largest bonsai
/// alone; failing that, the wordmark alone; and the tagline always.
fn splash_lines(height: usize, width: usize) -> Vec<Line<'static>> {
    const FOOTER: usize = 4; // blank, tagline, blank, "press any key"
    let wordmark = RAM_CODE_ART.len() + shadow_rows(RAM_CODE_ART).len() + 1;
    let wordmark_fits = width >= 64;
    let fits = |art: bonsai_art::Art, below: usize| {
        bonsai_width(art) <= width && art.len() + below <= height
    };
    let with_wordmark = bonsai_art::ARTS
        .iter()
        .copied()
        .find(|art| wordmark_fits && fits(art, wordmark + FOOTER));
    let bonsai = with_wordmark.or_else(|| {
        bonsai_art::ARTS
            .iter()
            .copied()
            .find(|art| fits(art, FOOTER))
    });
    let show_wordmark = with_wordmark.is_some()
        || (bonsai.is_none() && wordmark_fits && height >= wordmark + FOOTER);

    let mut lines = Vec::new();
    if let Some(art) = bonsai {
        lines.extend(bonsai_lines(art));
    }
    if show_wordmark {
        lines.push(Line::from(""));
        for art in RAM_CODE_ART {
            lines.push(Line::from(Span::styled(
                *art,
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD),
            )));
        }
        for shade in shadow_rows(RAM_CODE_ART) {
            lines.push(Line::from(Span::styled(
                shade,
                Style::default().fg(Color::DarkGray),
            )));
        }
    }
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "terminal coding harness for ramify worktrees",
        Style::default().fg(Color::Gray),
    )));
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        "press any key",
        Style::default().fg(Color::DarkGray),
    )));
    lines
}

/// Widest row of a splash bonsai, in cells.
fn bonsai_width(art: bonsai_art::Art) -> usize {
    art.iter()
        .map(|(chars, _)| chars.chars().count())
        .max()
        .unwrap_or(0)
}

/// A splash bonsai, one styled span per run of the same ink and shade.
/// Rows are padded to one width so centring keeps the picture's shape.
fn bonsai_lines(art: bonsai_art::Art) -> Vec<Line<'static>> {
    let width = bonsai_width(art);
    art.iter()
        .map(|(chars, inks)| {
            let mut spans: Vec<Span<'static>> = Vec::new();
            let mut run = String::new();
            let mut run_style = Style::default();
            let cells = chars
                .chars()
                .zip(inks.chars().chain(std::iter::repeat(' ')));
            for (ch, ink) in cells {
                let style = bonsai_style(ink, ch);
                if style != run_style && !run.is_empty() {
                    spans.push(Span::styled(std::mem::take(&mut run), run_style));
                }
                run_style = style;
                run.push(ch);
            }
            run.push_str(&" ".repeat(width.saturating_sub(chars.chars().count())));
            spans.push(Span::styled(run, run_style));
            Line::from(spans)
        })
        .collect()
}

/// Colour of one bonsai cell: its ink's colour, dimmed by how dark its character is.
fn bonsai_style(ink: char, ch: char) -> Style {
    let (r, g, b) = match ink {
        't' => (150, 128, 108),
        'd' => (214, 204, 186),
        'l' => (70, 140, 100),
        'm' => (110, 150, 70),
        'g' => (120, 96, 72),
        's' => (200, 60, 50),
        _ => return Style::default(),
    };
    let steps = bonsai_art::RAMP.chars().count().saturating_sub(1).max(1);
    let level = bonsai_art::RAMP
        .chars()
        .position(|c| c == ch)
        .unwrap_or(steps);
    let dim = |v: u8| (f32::from(v) * (0.45 + 0.55 * level as f32 / steps as f32)) as u8;
    Style::default().fg(Color::Rgb(dim(r), dim(g), dim(b)))
}

fn shadow_rows(art: &[&str]) -> Vec<String> {
    let last = art.last().copied().unwrap_or("");
    let width = last.chars().count();
    let mut near: Vec<char> = vec![' '; width + 2];
    let mut far: Vec<char> = vec![' '; width + 2];
    for (index, ch) in last.chars().enumerate() {
        if ch != ' ' {
            near[index + 1] = '▓';
            far[index + 2] = '░';
        }
    }
    vec![near.into_iter().collect(), far.into_iter().collect()]
}

/// Quitting ends the session: SessionEnd runs, and ramify's hook removes the record.
impl Drop for App {
    fn drop(&mut self) {
        self.end_session();
    }
}
