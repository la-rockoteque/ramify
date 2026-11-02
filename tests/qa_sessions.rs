#![allow(unused_variables)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::app::{App, Screen};
use ram_code::panel::Pane;
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-sessions-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn out(text: &str) -> CommandOutput {
    CommandOutput { status: 0, stdout: text.into(), stderr: String::new() }
}

struct FakeRunner {
    responses: Mutex<HashMap<Vec<String>, VecDeque<CommandOutput>>>,
    specs: Mutex<Vec<CommandSpec>>,
}

impl FakeRunner {
    fn new() -> Arc<Self> {
        Arc::new(Self { responses: Mutex::new(HashMap::new()), specs: Mutex::new(Vec::new()) })
    }
    fn push(&self, args: &[&str], output: CommandOutput) {
        let key: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        self.responses.lock().unwrap().entry(key).or_default().push_back(output);
    }
    fn calls(&self) -> Vec<CommandSpec> {
        self.specs.lock().unwrap().clone()
    }
}

impl Runner for FakeRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        self.specs.lock().unwrap().push(spec.clone());
        let mut responses = self.responses.lock().unwrap();
        let queue = responses.entry(spec.args.clone()).or_default();
        Ok(queue.pop_front().unwrap_or_else(|| out("")))
    }
}

fn app_with(runner: Arc<FakeRunner>, root: &Path) -> App {
    let mut app = App::new(runner, root.to_path_buf());
    app.screen = Screen::Main;
    app
}

fn render_text(app: &App) -> String {
    use ratatui::backend::TestBackend;
    let backend = TestBackend::new(160, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| frame.render_widget(app, frame.area())).unwrap();
    let buffer = terminal.backend().buffer();
    let area = buffer.area;
    let mut text = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

fn key(code: KeyCode) -> KeyEvent {
    KeyEvent::new(code, KeyModifiers::NONE)
}

fn git_branch(root: &Path, branch: &str) {
    let status = Command::new("git").args(["init", "-q", "-b", branch]).current_dir(root).status().unwrap();
    assert!(status.success());
    std::fs::write(root.join("file.txt"), "x\n").unwrap();
    let status = Command::new("git").args(["add", "."]).current_dir(root).status().unwrap();
    assert!(status.success());
    let status = Command::new("git")
        .args(["-c", "user.email=a@b", "-c", "user.name=t", "commit", "-qm", "init"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
}

fn two_sessions() -> (PathBuf, PathBuf, PathBuf) {
    let state = tmp("state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let alpha = tmp("alpha");
    git_branch(&alpha, "feat/alpha");
    let beta = tmp("beta");
    git_branch(&beta, "feat/beta");
    std::fs::create_dir_all(state.join("sessions")).unwrap();
    std::fs::write(state.join("sessions/a.root"), format!("{}\n", alpha.display())).unwrap();
    std::fs::write(state.join("sessions/a.name"), "alpha\n").unwrap();
    std::fs::write(state.join("sessions/b.root"), format!("{}\n", beta.display())).unwrap();
    std::fs::write(state.join("sessions/b.name"), "beta\n").unwrap();
    (state, alpha, beta)
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn sessions_group_by_project() {
    let _env = ENV_LOCK.lock().unwrap();
    let (state, alpha, beta) = two_sessions();
    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &alpha);
    app.panel.open_on(Pane::Sessions);
    app.panel.set_worktree(Some(alpha.clone()));
    app.panel.refresh(&*runner, &alpha);
    let text = render_text(&app);
    assert!(text.contains("alpha"), "{text}");
    assert!(text.contains("(1)"), "count missing:\n{text}");
    let beta_project = beta.file_name().unwrap().to_string_lossy();
    assert!(text.contains(beta_project.as_ref()), "beta header missing:\n{text}");
    assert!(!text.contains(" feat/beta"), "closed group shows its session:\n{text}");
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn opening_a_group_closes_the_others() {
    let _env = ENV_LOCK.lock().unwrap();
    let (state, alpha, beta) = two_sessions();
    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &alpha);
    app.panel.open_on(Pane::Sessions);
    app.panel.set_worktree(Some(alpha.clone()));
    app.panel.refresh(&*runner, &alpha);

    app.panel.selected = 2;
    app.on_key(key(KeyCode::Enter));
    let text = render_text(&app);
    assert!(text.contains(" feat/beta"), "beta not opened:\n{text}");
    assert!(!text.contains(" feat/alpha"), "alpha still open:\n{text}");

    app.panel.selected = 0;
    app.on_key(key(KeyCode::Char(' ')));
    let text = render_text(&app);
    assert!(text.contains(" feat/alpha"), "space did not open alpha:\n{text}");
    assert!(!text.contains(" feat/beta"), "beta still open:\n{text}");
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn enter_on_a_session_opens_its_root() {
    let _env = ENV_LOCK.lock().unwrap();
    let (state, alpha, beta) = two_sessions();
    let project = serde_json::json!({
        "name": "demo", "primary": beta,
        "worktrees": [{"name": "beta", "path": beta, "branch": "feat/beta", "primary": true}]
    }).to_string();
    let runner = FakeRunner::new();
    runner.push(&["dash-project", "--json"], out(&project));
    let mut app = app_with(runner.clone(), &alpha);
    app.panel.open_on(Pane::Sessions);
    app.panel.set_worktree(Some(alpha.clone()));
    app.panel.refresh(&*runner, &alpha);
    app.panel.selected = 2;
    app.on_key(key(KeyCode::Enter));
    app.panel.selected = 2;
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.root, beta);
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn agent_stop_runs_down() {
    let _env = ENV_LOCK.lock().unwrap();
    let state = tmp("q4-state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let worktree = tmp("q4-worktree");
    let directory = state.join("demo");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(format!("{}.web.pid", worktree.file_name().unwrap().to_string_lossy())), "1234\n").unwrap();

    let runner = FakeRunner::new();
    runner.push(&["down"], out("demo stopped. Shared containers and instances left running on purpose."));
    let mut app = app_with(runner.clone(), &worktree);
    app.panel.open_on(Pane::Agents);
    app.panel.set_worktree(Some(worktree.clone()));
    app.panel.refresh(&*runner, &worktree);
    app.on_key(key(KeyCode::Char('x')));
    assert!(runner.calls().iter().any(|spec| spec.args == vec!["down"] && spec.root == worktree));
    assert!(render_text(&app).contains("stopped"));
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn other_panes_keep_working() {
    let root = tmp("q5");
    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Dashboard);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify up"));
    runner.push(&["gates", "--json"], out("[]"));
    app.panel.open_on(Pane::Approvals);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify step ask"));
    runner.push(&["runs", "--json"], out("[]"));
    app.panel.open_on(Pane::Run);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify run ask"));
}
