use crate::ramify::{CommandSpec, Runner};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::Rect;
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};
use serde_json::Value;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Pane {
    Dashboard,
    Approvals,
    Scratch,
    Run,
}

impl Pane {
    fn all() -> [Pane; 4] {
        [Pane::Dashboard, Pane::Approvals, Pane::Scratch, Pane::Run]
    }

    fn title(self) -> &'static str {
        match self {
            Pane::Dashboard => "dashboard",
            Pane::Approvals => "approvals",
            Pane::Scratch => "scratch",
            Pane::Run => "run",
        }
    }

    pub fn from_name(name: &str) -> Option<Pane> {
        match name {
            "dashboard" | "dash" => Some(Pane::Dashboard),
            "approvals" => Some(Pane::Approvals),
            "scratch" => Some(Pane::Scratch),
            "run" => Some(Pane::Run),
            _ => None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct Gate {
    pub project: String,
    pub worktree: String,
    pub path: PathBuf,
    pub step: String,
    pub files: Vec<String>,
}

#[derive(Debug, Clone)]
pub struct ScratchItem {
    pub id: String,
    pub title: String,
    pub text: String,
    pub path: PathBuf,
}

#[derive(Debug, Clone)]
pub struct RunEntry {
    pub id: String,
    pub status: String,
    pub note: String,
    pub root: Option<PathBuf>,
    pub worktree: String,
    pub output: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PromptKind {
    Reject(usize),
    Block(usize),
}

pub struct Panel {
    pub open: bool,
    pub pane: Pane,
    pub selected: usize,
    projects: Vec<Value>,
    gates: Vec<Gate>,
    scratch: Vec<ScratchItem>,
    runs: Vec<RunEntry>,
    prompt: Option<(PromptKind, String)>,
    pub message: String,
    last_refresh: Option<Instant>,
}

impl Default for Panel {
    fn default() -> Self {
        Self::new()
    }
}

impl Panel {
    pub fn new() -> Self {
        Self {
            open: false,
            pane: Pane::Dashboard,
            selected: 0,
            projects: Vec::new(),
            gates: Vec::new(),
            scratch: Vec::new(),
            runs: Vec::new(),
            prompt: None,
            message: String::new(),
            last_refresh: None,
        }
    }

    pub fn toggle(&mut self) {
        self.open = !self.open;
        if self.open {
            self.last_refresh = None;
        }
    }

    pub fn open_on(&mut self, pane: Pane) {
        self.open = true;
        self.pane = pane;
        self.selected = 0;
        self.last_refresh = None;
    }

    pub fn close(&mut self) {
        self.open = false;
        self.prompt = None;
    }

    pub fn tick(&mut self, runner: &dyn Runner, root: &Path) {
        if !self.open {
            return;
        }
        let due = self
            .last_refresh
            .map(|last| last.elapsed() >= Duration::from_secs(5))
            .unwrap_or(true);
        if due {
            self.refresh(runner, root);
        }
    }

    pub fn refresh(&mut self, runner: &dyn Runner, root: &Path) {
        self.last_refresh = Some(Instant::now());
        match self.pane {
            Pane::Dashboard => self.projects = self.json(runner, root, &["dash", "--json"]),
            Pane::Approvals => self.gates = gates(self.json(runner, root, &["gates", "--json"])),
            Pane::Scratch => self.scratch = scratch_items(&scratch_dir()),
            Pane::Run => self.runs = runs(self.json(runner, root, &["runs", "--json"])),
        }
        self.clamp();
    }

    fn json(&self, runner: &dyn Runner, root: &Path, args: &[&str]) -> Vec<Value> {
        let spec = CommandSpec {
            root: root.to_path_buf(),
            args: args.iter().map(|arg| arg.to_string()).collect(),
        };
        match runner.run(&spec) {
            Ok(output) if output.ok() => serde_json::from_str(&output.stdout).unwrap_or_default(),
            _ => Vec::new(),
        }
    }

    fn length(&self) -> usize {
        match self.pane {
            Pane::Dashboard => self.projects
                .iter()
                .map(|project| project["worktrees"].as_array().map(|w| w.len()).unwrap_or(0))
                .sum(),
            Pane::Approvals => self.gates.len(),
            Pane::Scratch => self.scratch.len(),
            Pane::Run => self.runs.len(),
        }
    }

    fn clamp(&mut self) {
        let length = self.length();
        if length == 0 {
            self.selected = 0;
        } else if self.selected >= length {
            self.selected = length - 1;
        }
    }

    pub fn select_next(&mut self) {
        let length = self.length();
        if length > 0 {
            self.selected = (self.selected + 1) % length;
        }
    }

    pub fn select_previous(&mut self) {
        let length = self.length();
        if length > 0 {
            self.selected = (self.selected + length - 1) % length;
        }
    }

    pub fn on_key(&mut self, key: KeyEvent, runner: &dyn Runner, root: &Path) -> bool {
        if let Some((kind, text)) = self.prompt.as_mut() {
            match key.code {
                KeyCode::Esc => self.prompt = None,
                KeyCode::Backspace => {
                    text.pop();
                }
                KeyCode::Enter => {
                    let (kind, note) = (*kind, text.clone());
                    self.prompt = None;
                    self.apply_prompt(kind, &note, runner, root);
                }
                KeyCode::Char(character) => text.push(character),
                _ => {}
            }
            return true;
        }
        match key.code {
            KeyCode::Char('p') if key.modifiers.contains(crossterm::event::KeyModifiers::CONTROL) => {
                self.close();
                true
            }
            KeyCode::Esc => {
                self.close();
                true
            }
            KeyCode::Char('1') => {
                self.switch(Pane::Dashboard, runner, root);
                true
            }
            KeyCode::Char('2') => {
                self.switch(Pane::Approvals, runner, root);
                true
            }
            KeyCode::Char('3') => {
                self.switch(Pane::Scratch, runner, root);
                true
            }
            KeyCode::Char('4') => {
                self.switch(Pane::Run, runner, root);
                true
            }
            KeyCode::Char(']') => {
                let all = Pane::all();
                let index = all.iter().position(|pane| *pane == self.pane).unwrap_or(0);
                self.switch(all[(index + 1) % all.len()], runner, root);
                true
            }
            KeyCode::Char('[') => {
                let all = Pane::all();
                let index = all.iter().position(|pane| *pane == self.pane).unwrap_or(0);
                self.switch(all[(index + all.len() - 1) % all.len()], runner, root);
                true
            }
            KeyCode::Char('j') | KeyCode::Down => {
                self.select_next();
                true
            }
            KeyCode::Char('k') | KeyCode::Up => {
                self.select_previous();
                true
            }
            KeyCode::Char('a') if self.pane == Pane::Approvals => {
                self.approve(runner, root);
                true
            }
            KeyCode::Char('r') if self.pane == Pane::Approvals => {
                self.prompt = Some((PromptKind::Reject(self.selected), String::new()));
                true
            }
            KeyCode::Char('c') if self.pane == Pane::Scratch => {
                self.copy();
                true
            }
            KeyCode::Char('d') if self.pane == Pane::Scratch => {
                self.delete_scratch();
                true
            }
            KeyCode::Char('g') if self.pane == Pane::Run => {
                self.go_run(runner, root);
                true
            }
            KeyCode::Char('b') if self.pane == Pane::Run => {
                self.prompt = Some((PromptKind::Block(self.selected), String::new()));
                true
            }
            _ => false,
        }
    }

    fn switch(&mut self, pane: Pane, runner: &dyn Runner, root: &Path) {
        self.pane = pane;
        self.selected = 0;
        self.message.clear();
        self.refresh(runner, root);
    }

    fn approve(&mut self, runner: &dyn Runner, root: &Path) {
        let Some(gate) = self.gates.get(self.selected).cloned() else {
            return;
        };
        let spec = CommandSpec {
            root: gate.path,
            args: vec!["approve".into(), gate.step],
        };
        self.message = match runner.run(&spec) {
            Ok(output) => output.text(),
            Err(error) => error,
        };
        self.refresh(runner, root);
    }

    fn apply_prompt(&mut self, kind: PromptKind, note: &str, runner: &dyn Runner, root: &Path) {
        match kind {
            PromptKind::Reject(index) => {
                let Some(gate) = self.gates.get(index).cloned() else {
                    return;
                };
                let spec = CommandSpec {
                    root: gate.path,
                    args: vec!["reject".into(), gate.step, "--note".into(), note.to_string()],
                };
                self.message = match runner.run(&spec) {
                    Ok(output) => output.text(),
                    Err(error) => error,
                };
            }
            PromptKind::Block(index) => {
                let Some(entry) = self.runs.get(index).cloned() else {
                    return;
                };
                let spec = CommandSpec {
                    root: entry.root.unwrap_or_else(|| root.to_path_buf()),
                    args: vec![
                        "run".into(),
                        "block".into(),
                        entry.id,
                        "--note".into(),
                        note.to_string(),
                    ],
                };
                self.message = match runner.run(&spec) {
                    Ok(output) => output.text(),
                    Err(error) => error,
                };
            }
        }
        self.refresh(runner, root);
    }

    fn copy(&mut self) {
        let Some(item) = self.scratch.get(self.selected) else {
            return;
        };
        let program = std::env::var("RAM_CODE_CLIPBOARD").unwrap_or_else(|_| "pbcopy".into());
        let mut child = match Command::new(&program)
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .spawn()
        {
            Ok(child) => child,
            Err(error) => {
                self.message = format!("could not copy: {error}");
                return;
            }
        };
        if let Some(mut stdin) = child.stdin.take() {
            let _ = stdin.write_all(item.text.as_bytes());
        }
        let _ = child.wait();
        self.message = format!("copied {}", item.id);
    }

    fn delete_scratch(&mut self) {
        let Some(item) = self.scratch.get(self.selected).cloned() else {
            return;
        };
        let _ = std::fs::remove_file(&item.path);
        let _ = std::fs::remove_file(item.path.with_extension("title"));
        self.message = format!("removed {}", item.id);
        self.refresh_scratch();
    }

    fn refresh_scratch(&mut self) {
        self.scratch = scratch_items(&scratch_dir());
        self.clamp();
    }

    fn go_run(&mut self, runner: &dyn Runner, root: &Path) {
        let Some(entry) = self.runs.get(self.selected).cloned() else {
            return;
        };
        if entry.status != "pending" {
            self.message = format!("run {} is {}", entry.id, entry.status);
            return;
        }
        let spec = CommandSpec {
            root: entry.root.unwrap_or_else(|| root.to_path_buf()),
            args: vec!["run".into(), "go".into(), entry.id.clone()],
        };
        self.message = match runner.run(&spec) {
            Ok(output) => tail(&output.text(), 8),
            Err(error) => error,
        };
        self.refresh(runner, root);
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered()
            .title(self.title())
            .title_bottom(Line::from(" ^P close · 1-4 panes · j/k · esc "));
        let inner = block.inner(area);
        block.render(area, buf);
        let lines = self.lines();
        Paragraph::new(lines)
            .wrap(Wrap { trim: false })
            .render(inner, buf);
    }

    fn title(&self) -> String {
        let mut title = String::from(" panel ");
        for (index, pane) in Pane::all().iter().enumerate() {
            let label = format!("{}{} {}", if index == 0 { "" } else { "  " }, index + 1, pane.title());
            title.push_str(&label);
        }
        title.push(' ');
        title
    }

    fn lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        match self.pane {
            Pane::Dashboard => {
                if self.projects.is_empty() {
                    lines.push(Line::from("no stack — run 'ramify up' in a worktree"));
                }
                let mut index = 0;
                for project in &self.projects {
                    let name = project["name"].as_str().unwrap_or("");
                    lines.push(Line::from(Span::styled(
                        name.to_string(),
                        Style::default().add_modifier(Modifier::BOLD),
                    )));
                    if let Some(worktrees) = project["worktrees"].as_array() {
                        for worktree in worktrees {
                            let title = format!(
                                "{} {}",
                                worktree["name"].as_str().unwrap_or(""),
                                worktree["branch"].as_str().unwrap_or("")
                            );
                            let selected = index == self.selected;
                            index += 1;
                            lines.push(self.row(selected, &title));
                            let slot = worktree["slot"]
                                .as_u64()
                                .map(|slot| format!("slot {slot}"))
                                .unwrap_or_else(|| "no slot".into());
                            if let Some(services) = worktree["services"].as_array() {
                                for service in services {
                                    lines.push(Line::from(format!(
                                        "    {} :{} {} ({slot})",
                                        service["name"].as_str().unwrap_or(""),
                                        service["port"].as_u64().unwrap_or(0),
                                        service["state"].as_str().unwrap_or("")
                                    )));
                                }
                            }
                        }
                    }
                }
            }
            Pane::Approvals => {
                if self.gates.is_empty() {
                    lines.push(Line::from("nothing waits — 'ramify step ask <step>' opens a gate"));
                }
                for (index, gate) in self.gates.iter().enumerate() {
                    let selected = index == self.selected;
                    lines.push(self.row(
                        selected,
                        &format!("{}/{}  {}", gate.project, gate.worktree, gate.step),
                    ));
                    if !gate.files.is_empty() {
                        lines.push(Line::from(format!("    {}", gate.files.join(", "))));
                    }
                }
            }
            Pane::Scratch => {
                if self.scratch.is_empty() {
                    lines.push(Line::from("nothing in the scratch window — 'ramify scratch' adds"));
                }
                for (index, item) in self.scratch.iter().enumerate() {
                    let selected = index == self.selected;
                    lines.push(self.row(selected, &format!("{}  {}", item.id, item.title)));
                    if selected {
                        for line in item.text.lines().take(8) {
                            lines.push(Line::from(format!("    {line}")));
                        }
                    }
                }
            }
            Pane::Run => {
                if self.runs.is_empty() {
                    lines.push(Line::from("no run yet — 'ramify run ask <script>' starts one"));
                }
                for (index, entry) in self.runs.iter().enumerate() {
                    let selected = index == self.selected;
                    lines.push(self.row(
                        selected,
                        &format!(
                            "{}  {}  {}",
                            entry.id,
                            entry.status,
                            if entry.note.is_empty() { entry.worktree.clone() } else { entry.note.clone() }
                        ),
                    ));
                }
            }
        }
        if let Some((kind, text)) = &self.prompt {
            let label = match kind {
                PromptKind::Reject(_) => "reject note",
                PromptKind::Block(_) => "block note",
            };
            lines.push(Line::from(""));
            lines.push(Line::from(format!("{label}: {text}▊")));
        }
        if !self.message.is_empty() {
            lines.push(Line::from(""));
            for line in tail(&self.message, 4).lines() {
                lines.push(Line::from(Span::styled(
                    line.to_string(),
                    Style::default().fg(Color::Gray),
                )));
            }
        }
        lines
    }

    fn row(&self, selected: bool, text: &str) -> Line<'static> {
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Rgb(215, 119, 87))
        } else {
            Style::default()
        };
        Line::from(Span::styled(format!("{} {text}", if selected { ">" } else { " " }), style))
    }
}

fn gates(values: Vec<Value>) -> Vec<Gate> {
    values
        .into_iter()
        .filter_map(|value| {
            let ask = &value["ask"];
            Some(Gate {
                project: value["project"].as_str().unwrap_or("").into(),
                worktree: value["worktree"].as_str().unwrap_or("").into(),
                path: PathBuf::from(value["path"].as_str()?),
                step: ask["step"].as_str()?.into(),
                files: ask["files"]
                    .as_array()
                    .map(|files| {
                        files
                            .iter()
                            .filter_map(|file| file["path"].as_str().map(str::to_string))
                            .collect()
                    })
                    .unwrap_or_default(),
            })
        })
        .collect()
}

fn runs(values: Vec<Value>) -> Vec<RunEntry> {
    values
        .into_iter()
        .map(|value| RunEntry {
            id: value["id"].as_str().unwrap_or("").into(),
            status: value["status"].as_str().unwrap_or("").into(),
            note: value["note"].as_str().unwrap_or("").into(),
            root: value["root"].as_str().map(PathBuf::from),
            worktree: value["worktree"].as_str().unwrap_or("").into(),
            output: value["output"].as_str().unwrap_or("").into(),
        })
        .collect()
}

fn scratch_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RAMIFY_SCRATCH_DIR") {
        return PathBuf::from(dir);
    }
    let root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    Path::new(&root).join("scratch")
}

fn scratch_items(directory: &Path) -> Vec<ScratchItem> {
    let Ok(entries) = std::fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut items: Vec<ScratchItem> = entries
        .flatten()
        .filter(|entry| entry.path().extension().map(|ext| ext == "txt").unwrap_or(false))
        .filter_map(|entry| {
            let path = entry.path();
            let id = path.file_stem()?.to_string_lossy().into_owned();
            let text = std::fs::read_to_string(&path).ok()?;
            let title = std::fs::read_to_string(path.with_extension("title"))
                .unwrap_or_default()
                .trim()
                .to_string();
            Some(ScratchItem {
                id,
                title,
                text,
                path,
            })
        })
        .collect();
    items.sort_by(|a, b| b.id.cmp(&a.id));
    items
}

fn tail(text: &str, count: usize) -> String {
    let lines: Vec<&str> = text.lines().collect();
    let start = lines.len().saturating_sub(count);
    lines[start..].join("\n")
}

#[allow(dead_code)]
fn read_stdin() -> String {
    let mut text = String::new();
    let _ = std::io::stdin().read_to_string(&mut text);
    text
}
