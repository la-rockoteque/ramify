use crate::agent::{run_task, AgentConfig, AgentEvent, AnthropicLlm};
use crate::ramify::{Action, CommandOutput, CommandSpec, Runner};
use crate::state::{parse_dash, project_for_root, Project, Worktree};
use crossterm::event::{KeyCode, KeyEvent};
use ratatui::buffer::Buffer;
use ratatui::layout::{Alignment, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Paragraph, Widget, Wrap};
use std::path::PathBuf;
use std::sync::mpsc;
use std::sync::Arc;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Screen {
    Splash,
    Loading,
    Main,
    Detail,
    Task,
    Message,
    Running,
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
pub enum Prompt {
    None,
    NewWorktree,
    Task,
}

pub struct App {
    pub runner: Arc<dyn Runner>,
    pub root: PathBuf,
    pub project: Option<Project>,
    pub selected: usize,
    pub screen: Screen,
    pub message: String,
    pub message_kind: MessageKind,
    pub detail: Option<Worktree>,
    pub task_input: String,
    pub log: Vec<String>,
    pub events: Option<mpsc::Receiver<AgentEvent>>,
    pub prompt: Prompt,
    pub quit: bool,
}

impl App {
    pub fn new(runner: Arc<dyn Runner>, root: PathBuf) -> Self {
        Self {
            runner,
            root,
            project: None,
            selected: 0,
            screen: Screen::Splash,
            message: String::new(),
            message_kind: MessageKind::Info,
            detail: None,
            task_input: String::new(),
            log: Vec::new(),
            events: None,
            prompt: Prompt::None,
            quit: false,
        }
    }

    pub fn load(&mut self) {
        self.screen = Screen::Loading;
        let runner = self.runner.clone();
        let spec = CommandSpec {
            root: self.root.clone(),
            args: vec!["dash".into(), "--json".into()],
        };
        let result = runner.run(&spec);
        match result {
            Err(error) => {
                self.screen = Screen::Error;
                self.message = format!("could not run Ramify: {error}");
                self.message_kind = MessageKind::Failure;
            }
            Ok(output) if !output.ok() => {
                self.screen = Screen::Error;
                self.message = format!("could not read Ramify state: {}", output.text());
                self.message_kind = MessageKind::Failure;
            }
            Ok(output) => match parse_dash(&output.stdout) {
                Err(error) => {
                    self.screen = Screen::Error;
                    self.message = format!("could not read Ramify state: {error}");
                    self.message_kind = MessageKind::Failure;
                }
                Ok(projects) => match project_for_root(&projects, &self.root) {
                    None => {
                        self.project = None;
                        self.screen = Screen::Empty;
                    }
                    Some(project) => {
                        self.project = Some(project.clone());
                        self.screen = Screen::Main;
                        self.clamp_selection();
                    }
                },
            },
        }
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
        let config = match config {
            Ok(config) => config,
            Err(error) => {
                self.message = error;
                self.message_kind = MessageKind::Failure;
                self.screen = Screen::Message;
                return;
            }
        };
        let task = self.task_input.trim().to_string();
        if task.is_empty() {
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

        let (tx, rx) = mpsc::channel();
        let root = worktree.path.clone();
        self.log.clear();
        self.log.push(format!("task for {}", worktree.name));
        self.log.push(task.clone());
        std::thread::spawn(move || {
            let llm = AnthropicLlm::new(config);
            let result = run_task(&llm, &root, &task, &mut |event| {
                let _ = tx.send(event);
            });
            let _ = match result {
                Ok(text) => tx.send(AgentEvent::Done(text)),
                Err(error) => tx.send(AgentEvent::Error(error)),
            };
        });
        self.events = Some(rx);
        self.screen = Screen::Running;
    }

    pub fn poll_events(&mut self) {
        let mut events = Vec::new();
        if let Some(rx) = &self.events {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }
        for event in events {
            match event {
                AgentEvent::Text(text) => self.log.push(text),
                AgentEvent::ToolCall { name, .. } => self.log.push(format!("→ {name}")),
                AgentEvent::ToolResult(result) => {
                    let first = result.lines().next().unwrap_or("").to_string();
                    self.log.push(format!("  {first}"));
                }
                AgentEvent::Done(text) => {
                    self.events = None;
                    self.message = if text.is_empty() {
                        "task finished".into()
                    } else {
                        text
                    };
                    self.message_kind = MessageKind::Success;
                    self.screen = Screen::Message;
                }
                AgentEvent::Error(error) => {
                    self.events = None;
                    self.message = error;
                    self.message_kind = MessageKind::Failure;
                    self.screen = Screen::Message;
                }
            }
        }
    }

    pub fn back(&mut self) {
        match self.screen {
            Screen::Detail => {
                self.detail = None;
                self.screen = Screen::Main;
            }
            Screen::Task => {
                self.prompt = Prompt::None;
                self.task_input.clear();
                self.screen = Screen::Main;
            }
            Screen::Running | Screen::Message => {
                self.prompt = Prompt::None;
                self.load();
            }
            _ => {}
        }
    }

    pub fn on_key(&mut self, key: KeyEvent) {
        if self.screen == Screen::Task {
            match key.code {
                KeyCode::Esc => self.back(),
                KeyCode::Enter => {
                    let value = self.task_input.trim().to_string();
                    if value.is_empty() {
                        return;
                    }
                    match self.prompt {
                        Prompt::NewWorktree => {
                            self.task_input.clear();
                            self.invoke(Action::CreateWorktree(value));
                        }
                        _ => self.start_task(AgentConfig::from_process_env()),
                    }
                }
                KeyCode::Backspace => {
                    self.task_input.pop();
                }
                KeyCode::Char(c) => self.task_input.push(c),
                _ => {}
            }
            return;
        }

        match key.code {
            KeyCode::Char('q') => self.quit = true,
            KeyCode::Esc => self.back(),
            KeyCode::Char('r') => self.load(),
            KeyCode::Char('j') | KeyCode::Down => self.next(),
            KeyCode::Char('k') | KeyCode::Up => self.previous(),
            KeyCode::Enter | KeyCode::Char('i') if self.screen == Screen::Main => {
                self.open_selected()
            }
            KeyCode::Char('c') if self.screen == Screen::Main || self.screen == Screen::Detail => {
                self.task_input.clear();
                self.prompt = Prompt::Task;
                self.screen = Screen::Task;
            }
            KeyCode::Char('n') if self.screen == Screen::Main => {
                self.task_input.clear();
                self.prompt = Prompt::NewWorktree;
                self.screen = Screen::Task;
            }
            KeyCode::Char('u') if self.screen == Screen::Main => self.invoke(Action::Up),
            KeyCode::Char('d') if self.screen == Screen::Main => self.invoke(Action::Down),
            KeyCode::Char('s') if self.screen == Screen::Main => self.invoke(Action::Step),
            _ => {}
        }
    }

    pub fn render(&self, area: Rect, buf: &mut Buffer) {
        let block = Block::bordered().title(self.title());
        let inner = block.inner(area);
        block.render(area, buf);
        let lines = self.lines();
        let paragraph = Paragraph::new(lines).wrap(Wrap { trim: false });
        let paragraph = if self.screen == Screen::Splash {
            paragraph.alignment(Alignment::Center)
        } else {
            paragraph
        };
        paragraph.render(inner, buf);
    }

    fn title(&self) -> String {
        match self.screen {
            Screen::Splash => " Ram Code ".into(),
            Screen::Main => format!(
                " Ram Code — {} ",
                self.project
                    .as_ref()
                    .map(|project| project.name.as_str())
                    .unwrap_or("ramify")
            ),
            Screen::Detail => format!(
                " Ram Code — {} ",
                self.detail
                    .as_ref()
                    .map(|worktree| worktree.name.as_str())
                    .unwrap_or("worktree")
            ),
            Screen::Task if self.prompt == Prompt::NewWorktree => " Ram Code — new worktree ".into(),
            Screen::Task => " Ram Code — task ".into(),
            Screen::Running => " Ram Code — working ".into(),
            Screen::Message => " Ram Code — result ".into(),
            Screen::Empty => " Ram Code — no project ".into(),
            Screen::Error => " Ram Code — error ".into(),
            Screen::Loading => " Ram Code ".into(),
        }
    }

    fn lines(&self) -> Vec<Line<'static>> {
        match self.screen {
            Screen::Splash => splash_lines(),
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
            Screen::Main => self.main_lines(),
            Screen::Detail => self.detail_lines(),
            Screen::Task => {
                let heading = match self.prompt {
                    Prompt::NewWorktree => "New worktree slug:".to_string(),
                    _ => format!(
                        "Task for {}:",
                        self.selected_worktree()
                            .map(|worktree| worktree.name.as_str())
                            .unwrap_or("worktree")
                    ),
                };
                vec![
                    Line::from(heading),
                    Line::from(self.task_input.clone()),
                    Line::from(""),
                    Line::from("enter run · esc back"),
                ]
            }
            Screen::Running => {
                let mut lines = vec![Line::from("Working…")];
                lines.push(Line::from(""));
                for entry in self.log.iter().rev().take(20).rev() {
                    lines.push(Line::from(entry.clone()));
                }
                lines.push(Line::from(""));
                lines.push(Line::from("esc back"));
                lines
            }
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
        }
    }

    fn main_lines(&self) -> Vec<Line<'static>> {
        let mut lines = Vec::new();
        if let Some(project) = &self.project {
            lines.push(Line::from(format!(
                "Project  {}  {}",
                project.name,
                project.primary.display()
            )));
        } else {
            lines.push(Line::from("Project  (none)"));
        }
        lines.push(Line::from(""));
        lines.push(Line::from("Worktrees"));
        if self.worktrees().is_empty() {
            lines.push(Line::from("  (none)"));
        }
        for (index, worktree) in self.worktrees().iter().enumerate() {
            let marker = if index == self.selected { ">" } else { " " };
            let dirty = if worktree.dirty { " dirty" } else { "" };
            let next = worktree
                .workflow
                .as_ref()
                .and_then(|workflow| workflow.next.as_ref())
                .map(|step| format!("  next: {}", step.step))
                .unwrap_or_default();
            lines.push(Line::from(format!(
                "{marker} {:<18} {:<24}{dirty}{next}",
                worktree.name, worktree.branch
            )));
        }
        lines.push(Line::from(""));
        lines.push(Line::from(
            "j/k move · enter open · c code · n new · u up · d down · s step · r refresh · q quit",
        ));
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
            "c code · u up · d down · s step · esc back · q quit",
        ));
        lines
    }
}

impl Widget for &App {
    fn render(self, area: Rect, buf: &mut Buffer) {
        App::render(self, area, buf);
    }
}

const RAM_LOGO: &[&str] = &[
    "      ╱╲                   ╱╲",
    "     ╱  ╲_________________╱  ╲",
    "    ╱   ◉                 ◉   ╲",
    "   ╱           ╲_______╱         ╲",
    "   ╲_____________________________╱",
];

const RAM_CODE_ART: &[&str] = &[
    "██████╗  █████╗ ███╗   ███╗    ██████╗  ██████╗ ██████╗ ███████╗",
    "██╔══██╗██╔══██╗████╗ ████║   ██╔════╝ ██╔═══██╗██╔══██╗██╔════╝",
    "██████╔╝███████║██╔████╔██║   ██║      ██║   ██║██║  ██║█████╗  ",
    "██╔══██╗██╔══██║██║╚██╔╝██║   ██║      ██║   ██║██║  ██║██╔══╝  ",
    "██║  ██║██║  ██║██║ ╚═╝ ██║   ╚██████╗ ╚██████╔╝██████╔╝███████╗",
    "╚═╝  ╚═╝╚═╝  ╚═╝╚═╝     ╚═╝    ╚═════╝  ╚═════╝ ╚═════╝ ╚══════╝",
];

fn splash_lines() -> Vec<Line<'static>> {
    let mut lines = Vec::new();
    for art in RAM_LOGO {
        lines.push(Line::from(Span::styled(
            *art,
            Style::default().fg(Color::Cyan),
        )));
    }
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
    vec![
        near.into_iter().collect(),
        far.into_iter().collect(),
    ]
}
