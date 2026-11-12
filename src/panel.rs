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
    Sessions,
    Agents,
    Artifacts,
}

impl Pane {
    fn all() -> [Pane; 7] {
        [
            Pane::Dashboard,
            Pane::Approvals,
            Pane::Scratch,
            Pane::Run,
            Pane::Sessions,
            Pane::Agents,
            Pane::Artifacts,
        ]
    }

    fn title(self) -> &'static str {
        match self {
            Pane::Dashboard => "dashboard",
            Pane::Approvals => "approvals",
            Pane::Scratch => "scratch",
            Pane::Run => "run",
            Pane::Sessions => "sessions",
            Pane::Agents => "agents",
            Pane::Artifacts => "artifacts",
        }
    }

    pub fn from_name(name: &str) -> Option<Pane> {
        match name {
            "dashboard" | "dash" => Some(Pane::Dashboard),
            "approvals" => Some(Pane::Approvals),
            "scratch" => Some(Pane::Scratch),
            "run" => Some(Pane::Run),
            "sessions" => Some(Pane::Sessions),
            "agents" => Some(Pane::Agents),
            "artifacts" => Some(Pane::Artifacts),
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

#[derive(Debug, Clone)]
pub struct SessionEntry {
    pub id: String,
    pub name: String,
    pub root: Option<PathBuf>,
    pub branch: String,
    pub project: String,
    /// From the session record: `claude`, `codex` or `ram`. Empty for a pinned session without one.
    pub agent: String,
    pub state: String,
}

enum Visible {
    Header { project: String, count: usize },
    Session { index: usize },
}

#[derive(Debug, Clone)]
pub struct AgentEntry {
    pub service: String,
    pub pid: String,
}

#[derive(Debug, Clone)]
pub struct ArtifactEntry {
    pub name: String,
    pub path: PathBuf,
}

pub struct Panel {
    pub open: bool,
    pub pane: Pane,
    pub selected: usize,
    projects: Vec<Value>,
    gates: Vec<Gate>,
    scratch: Vec<ScratchItem>,
    runs: Vec<RunEntry>,
    sessions: Vec<SessionEntry>,
    open_group: Option<String>,
    agents: Vec<AgentEntry>,
    artifacts: Vec<ArtifactEntry>,
    worktree: Option<PathBuf>,
    pending_open: Option<PathBuf>,
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
            sessions: Vec::new(),
            open_group: None,
            agents: Vec::new(),
            artifacts: Vec::new(),
            worktree: None,
            pending_open: None,
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

    pub fn set_worktree(&mut self, worktree: Option<PathBuf>) {
        if self.worktree != worktree {
            self.worktree = worktree;
            self.last_refresh = None;
        }
    }

    pub fn take_open(&mut self) -> Option<PathBuf> {
        self.pending_open.take()
    }

    pub fn refresh(&mut self, runner: &dyn Runner, root: &Path) {
        self.last_refresh = Some(Instant::now());
        match self.pane {
            Pane::Dashboard => self.projects = self.json(runner, root, &["dash", "--json"]),
            Pane::Approvals => self.gates = gates(self.json(runner, root, &["gates", "--json"])),
            Pane::Scratch => self.scratch = scratch_items(&scratch_dir()),
            Pane::Run => self.runs = runs(self.json(runner, root, &["runs", "--json"])),
            Pane::Sessions => {
                self.sessions = sessions();
                let projects: Vec<String> = self
                    .sessions
                    .iter()
                    .map(|session| session.project.clone())
                    .collect();
                let current = self
                    .worktree
                    .as_deref()
                    .and_then(primary_of)
                    .filter(|project| projects.contains(project));
                if self
                    .open_group
                    .as_ref()
                    .map(|group| !projects.contains(group))
                    .unwrap_or(true)
                {
                    self.open_group = current.or_else(|| projects.first().cloned());
                }
            }
            Pane::Agents => {
                self.agents = self
                    .worktree
                    .as_deref()
                    .map(agent_entries)
                    .unwrap_or_default()
            }
            Pane::Artifacts => {
                self.artifacts = self
                    .worktree
                    .as_deref()
                    .map(artifact_entries)
                    .unwrap_or_default()
            }
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
            Pane::Sessions => self.visible_rows().len(),
            Pane::Agents => self.agents.len(),
            Pane::Artifacts => self.artifacts.len(),
        }
    }

    fn visible_rows(&self) -> Vec<Visible> {
        let mut rows = Vec::new();
        let mut groups: Vec<String> = Vec::new();
        for session in &self.sessions {
            if !groups.contains(&session.project) {
                groups.push(session.project.clone());
            }
        }
        for group in groups {
            let indexes: Vec<usize> = self
                .sessions
                .iter()
                .enumerate()
                .filter(|(_, session)| session.project == group)
                .map(|(index, _)| index)
                .collect();
            let count = indexes.len();
            let open = self.open_group.as_deref() == Some(group.as_str());
            rows.push(Visible::Header {
                project: if group.is_empty() { "other".into() } else { group },
                count,
            });
            if open {
                for index in indexes {
                    rows.push(Visible::Session { index });
                }
            }
        }
        rows
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
            KeyCode::Char('5') => {
                self.switch(Pane::Sessions, runner, root);
                true
            }
            KeyCode::Char('6') => {
                self.switch(Pane::Agents, runner, root);
                true
            }
            KeyCode::Char('7') => {
                self.switch(Pane::Artifacts, runner, root);
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
            KeyCode::Enter | KeyCode::Char(' ') if self.pane == Pane::Sessions => {
                match self.visible_rows().get(self.selected) {
                    Some(Visible::Header { project, .. }) => {
                        self.open_group = Some(project.clone());
                        self.selected = 0;
                    }
                    Some(Visible::Session { index }) => {
                        if let Some(session) = self.sessions.get(*index) {
                            self.pending_open = session.root.clone();
                        }
                    }
                    None => {}
                }
                true
            }
            KeyCode::Char('x') if self.pane == Pane::Agents => {
                self.stop_agents(runner, root);
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

    pub fn border_color(&self) -> Option<Color> {
        if self.pane != Pane::Sessions {
            return None;
        }
        match self.visible_rows().get(self.selected) {
            Some(Visible::Header { project, .. }) => Some(session_color(project)),
            Some(Visible::Session { index }) => self
                .sessions
                .get(*index)
                .map(|session| session_color(&session.name)),
            None => None,
        }
    }

    fn stop_agents(&mut self, runner: &dyn Runner, root: &Path) {
        let Some(worktree) = self.worktree.clone() else {
            self.message = "no worktree selected".into();
            return;
        };
        let spec = CommandSpec {
            root: worktree,
            args: vec!["down".into()],
        };
        self.message = match runner.run(&spec) {
            Ok(output) => output.text(),
            Err(error) => error,
        };
        self.refresh(runner, root);
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let mut block = Block::bordered()
            .title(self.title())
            .title_bottom(Line::from(" ^P close · 1-7 panes · j/k · esc "));
        if let Some(color) = self.border_color() {
            block = block.border_style(Style::default().fg(color));
        }
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
            Pane::Sessions => {
                if self.sessions.is_empty() {
                    lines.push(Line::from("no session — start one in Claude Code or Codex"));
                }
                for (row_index, row) in self.visible_rows().iter().enumerate() {
                    let selected = row_index == self.selected;
                    match row {
                        Visible::Header { project, count } => {
                            let open = self.open_group.as_deref() == Some(project.as_str());
                            let color = session_color(project);
                            let style = if selected {
                                Style::default()
                                    .fg(color)
                                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                            } else {
                                Style::default().fg(color).add_modifier(Modifier::BOLD)
                            };
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "{} {} {} ({count})",
                                    if selected { ">" } else { " " },
                                    if open { "▾" } else { "▸" },
                                    project
                                ),
                                style,
                            )));
                        }
                        Visible::Session { index } => {
                            let Some(session) = self.sessions.get(*index) else {
                                continue;
                            };
                            let color = session_color(&session.name);
                            let style = if selected {
                                Style::default()
                                    .fg(color)
                                    .add_modifier(Modifier::BOLD | Modifier::REVERSED)
                            } else {
                                Style::default().fg(color)
                            };
                            let who = if session.agent.is_empty() {
                                format!("{} {}", session.id, session.name)
                            } else {
                                session_label(session)
                            };
                            lines.push(Line::from(Span::styled(
                                format!(
                                    "  {} {} {}",
                                    who,
                                    session.branch,
                                    session
                                        .root
                                        .as_ref()
                                        .map(|root| root.display().to_string())
                                        .unwrap_or_default()
                                ),
                                style,
                            )));
                        }
                    }
                }
            }
            Pane::Agents => {
                if self.agents.is_empty() {
                    lines.push(Line::from("no agent work here — 'ramify up' starts a stack"));
                }
                for (index, agent) in self.agents.iter().enumerate() {
                    let selected = index == self.selected;
                    lines.push(self.row(selected, &format!("{}  pid {}", agent.service, agent.pid)));
                }
            }
            Pane::Artifacts => {
                if self.artifacts.is_empty() {
                    lines.push(Line::from(
                        "no work order — 'ramify new <slug> --workflow <name>' writes one",
                    ));
                }
                for (index, artifact) in self.artifacts.iter().enumerate() {
                    let selected = index == self.selected;
                    lines.push(self.row(selected, &artifact.name));
                    if selected {
                        if let Ok(text) = std::fs::read_to_string(&artifact.path) {
                            for line in text.lines().take(20) {
                                lines.push(Line::from(format!("    {line}")));
                            }
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

pub fn session_color(name: &str) -> Color {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let index = u32::from_str_radix(&hash[..8], 16).unwrap_or(0) % 8;
    const COLORS: [Color; 8] = [
        Color::Red,
        Color::Blue,
        Color::Green,
        Color::Yellow,
        Color::Magenta,
        Color::Rgb(255, 165, 0),
        Color::Rgb(255, 105, 180),
        Color::Cyan,
    ];
    COLORS[index as usize]
}

fn primary_of(path: &Path) -> Option<String> {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "worktree", "list", "--porcelain"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| line.strip_prefix("worktree ").map(PathBuf::from))
        })
        .and_then(|primary| {
            primary
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
}

fn git_branch(path: &Path) -> Option<String> {
    Command::new("git")
        .args(["-C", &path.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .filter(|branch| !branch.is_empty())
}

fn state_root() -> PathBuf {
    PathBuf::from(std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into()))
}

/// Every live session: the records the session hooks write (`<id>.session`), and the sessions
/// that `ramify new` pinned to a worktree (`<id>.root`) without a live record.
fn sessions() -> Vec<SessionEntry> {
    let directory = state_root().join("sessions");
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut records: Vec<(String, Value)> = Vec::new();
    let mut pins: Vec<(String, PathBuf)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        let Some(id) = path.file_stem().map(|stem| stem.to_string_lossy().into_owned()) else {
            continue;
        };
        match path.extension().and_then(|ext| ext.to_str()) {
            Some("session") => {
                let record = std::fs::read_to_string(&path)
                    .ok()
                    .and_then(|text| serde_json::from_str::<Value>(&text).ok())
                    .filter(|record| record["pid"].as_u64().is_some_and(running));
                if let Some(record) = record {
                    records.push((id, record));
                }
            }
            Some("root") => pins.push((id, path)),
            _ => {}
        }
    }
    let name_of = |id: &str| {
        std::fs::read_to_string(directory.join(format!("{id}.name")))
            .ok()
            .and_then(|text| text.lines().next().map(str::to_string))
            .unwrap_or_default()
    };
    let entry = |id: String, root: Option<PathBuf>, agent: String, state: String| SessionEntry {
        name: name_of(&id),
        branch: root.as_deref().and_then(git_branch).unwrap_or_default(),
        project: root.as_deref().and_then(primary_of).unwrap_or_default(),
        id,
        root,
        agent,
        state,
    };
    let mut sessions: Vec<SessionEntry> = Vec::new();
    for (id, path) in pins {
        if records.iter().any(|(record, _)| *record == id) {
            continue;
        }
        let root = std::fs::read_to_string(&path)
            .ok()
            .map(|text| PathBuf::from(text.trim()))
            .filter(|path| path.is_dir());
        sessions.push(entry(id, root, String::new(), String::new()));
    }
    for (id, record) in records {
        let root = record["root"].as_str().map(PathBuf::from).filter(|path| path.is_dir());
        let agent = record["agent"].as_str().unwrap_or("").to_string();
        let state = record["state"].as_str().unwrap_or("idle").to_string();
        sessions.push(entry(id, root, agent, state));
    }
    sessions.sort_by(|a, b| a.project.cmp(&b.project).then(b.id.cmp(&a.id)));
    sessions
}

/// Whether the process runs. `kill -0` sends no signal; the crate has no libc binding.
fn running(pid: u64) -> bool {
    std::process::Command::new("kill")
        .args(["-0", &pid.to_string()])
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

/// `✻ birch`, `◇ codex ⏸`, `◆ ram`: the marks and labels of `ramify prompt`.
fn session_label(session: &SessionEntry) -> String {
    let mark = match session.agent.as_str() {
        "codex" => "◇",
        "ram" => "◆",
        _ => "✻",
    };
    let label = if session.name.is_empty() { session.agent.as_str() } else { session.name.as_str() };
    let idle = if session.state == "idle" { " ⏸" } else { "" };
    format!("{mark} {label}{idle}")
}

fn agent_entries(worktree: &Path) -> Vec<AgentEntry> {
    let Some(name) = worktree.file_name().map(|name| name.to_string_lossy().into_owned()) else {
        return Vec::new();
    };
    let root = state_root();
    let Ok(directories) = std::fs::read_dir(&root) else {
        return Vec::new();
    };
    let mut agents = Vec::new();
    for directory in directories.flatten() {
        if !directory.path().is_dir() {
            continue;
        }
        let Ok(entries) = std::fs::read_dir(directory.path()) else {
            continue;
        };
        for entry in entries.flatten() {
            let file = entry.file_name().to_string_lossy().into_owned();
            let Some(rest) = file.strip_prefix(&format!("{name}.")) else {
                continue;
            };
            let Some(service) = rest.strip_suffix(".pid") else {
                continue;
            };
            if service.contains('.') {
                continue;
            }
            let pid = std::fs::read_to_string(entry.path())
                .unwrap_or_default()
                .trim()
                .to_string();
            agents.push(AgentEntry {
                service: service.to_string(),
                pid,
            });
        }
    }
    agents.sort_by(|a, b| a.service.cmp(&b.service));
    agents
}

fn artifact_entries(worktree: &Path) -> Vec<ArtifactEntry> {
    let branch = git_branch(worktree).unwrap_or_default();
    let directory = worktree.join(".ramify/work");
    let Ok(entries) = std::fs::read_dir(&directory) else {
        return Vec::new();
    };
    let mut order_folder = None;
    for entry in entries.flatten() {
        let path = entry.path().join("order.md");
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        let recorded = text.lines().find_map(|line| line.strip_prefix("branch:")).map(str::trim);
        if recorded == Some(branch.as_str()) {
            order_folder = Some(entry.path());
            break;
        }
    }
    let Some(folder) = order_folder else {
        return Vec::new();
    };
    ["spec.md", "qa-plan.md", "audit.md", "qa-results.md", "review.md"]
        .iter()
        .filter_map(|name| {
            let path = folder.join(name);
            path.is_file().then(|| ArtifactEntry {
                name: name.to_string(),
                path,
            })
        })
        .collect()
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
