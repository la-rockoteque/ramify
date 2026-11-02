#![allow(unused_variables)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::app::{App, Screen};
use ram_code::panel::{session_color, Pane};
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};

static ENV_LOCK: Mutex<()> = Mutex::new(());

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-panes-{}-{name}", std::process::id()));
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
    let status = Command::new("git")
        .args(["init", "-q", "-b", branch])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
    std::fs::write(root.join("file.txt"), "x\n").unwrap();
    let status = Command::new("git")
        .args(["add", "."])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
    let status = Command::new("git")
        .args(["-c", "user.email=a@b", "-c", "user.name=t", "commit", "-qm", "init"])
        .current_dir(root)
        .status()
        .unwrap();
    assert!(status.success());
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn sessions_list_names_roots_branches() {
    let _env = ENV_LOCK.lock().unwrap();
    let state = tmp("q1-state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let root = tmp("q1-root");
    git_branch(&root, "feat/birch");
    std::fs::create_dir_all(state.join("sessions")).unwrap();
    std::fs::write(state.join("sessions/abc.root"), format!("{}\n", root.display())).unwrap();
    std::fs::write(state.join("sessions/abc.name"), "birch\n").unwrap();

    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Sessions);
    app.panel.refresh(&*runner, &root);
    let text = render_text(&app);
    for expected in ["abc", "birch", "feat/birch", "q1-root"] {
        assert!(text.contains(expected), "{expected} missing:\n{text}");
    }
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn selected_session_colour_themes_panel() {
    let _env = ENV_LOCK.lock().unwrap();
    let state = tmp("q2-state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let root = tmp("q2-root");
    std::fs::create_dir_all(state.join("sessions")).unwrap();
    std::fs::write(state.join("sessions/one.root"), format!("{}\n", root.display())).unwrap();
    std::fs::write(state.join("sessions/one.name"), "birch\n").unwrap();

    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Sessions);
    app.panel.refresh(&*runner, &root);
    app.panel.select_next();
    assert_eq!(app.panel.border_color(), Some(session_color("birch")));
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn enter_opens_session_root() {
    let _env = ENV_LOCK.lock().unwrap();
    let state = tmp("q3-state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let other = tmp("q3-other");
    git_branch(&other, "feat/other");
    std::fs::create_dir_all(state.join("sessions")).unwrap();
    std::fs::write(state.join("sessions/one.root"), format!("{}\n", other.display())).unwrap();
    std::fs::write(state.join("sessions/one.name"), "birch\n").unwrap();
    let project = serde_json::json!({
        "name": "demo", "primary": other,
        "worktrees": [{"name": "other", "path": other, "branch": "feat/other", "primary": true}]
    }).to_string();

    let runner = FakeRunner::new();
    runner.push(&["dash-project", "--json"], out(&project));
    let mut app = app_with(runner.clone(), &tmp("q3-start"));
    app.panel.open_on(Pane::Sessions);
    app.panel.refresh(&*runner, &app.root.clone());
    app.panel.select_next();
    app.on_key(key(KeyCode::Enter));
    assert_eq!(app.root, other);
    assert_eq!(app.screen, Screen::Main);
    assert_eq!(app.project.as_ref().map(|project| project.name.as_str()), Some("demo"));
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn agents_list_the_worktree_services() {
    let _env = ENV_LOCK.lock().unwrap();
    let state = tmp("q4-state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let worktree = tmp("q4-root");
    let directory = state.join("demo");
    std::fs::create_dir_all(&directory).unwrap();
    std::fs::write(directory.join(format!("{}.web.pid", worktree.file_name().unwrap().to_string_lossy())), "1234\n").unwrap();

    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &worktree);
    app.panel.open_on(Pane::Agents);
    app.panel.set_worktree(Some(worktree.clone()));
    app.panel.refresh(&*runner, &worktree);
    let text = render_text(&app);
    assert!(text.contains("web"), "{text}");
    assert!(text.contains("1234"), "{text}");

    app.panel.set_worktree(None);
    app.panel.refresh(&*runner, &worktree);
    assert!(render_text(&app).contains("ramify up"));
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn artifacts_list_and_preview() {
    let _env = ENV_LOCK.lock().unwrap();
    let worktree = tmp("q5-root");
    git_branch(&worktree, "feat/x");
    std::fs::create_dir_all(worktree.join(".ramify/work/demo")).unwrap();
    std::fs::write(
        worktree.join(".ramify/work/demo/order.md"),
        "---\nworkflow: feature\nlane: standard\nbranch: feat/x\n---\n",
    )
    .unwrap();
    std::fs::write(worktree.join(".ramify/work/demo/spec.md"), "The spec.\n").unwrap();

    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &worktree);
    app.panel.open_on(Pane::Artifacts);
    app.panel.set_worktree(Some(worktree.clone()));
    app.panel.refresh(&*runner, &worktree);
    let text = render_text(&app);
    assert!(text.contains("spec.md"), "{text}");
    assert!(text.contains("The spec."), "{text}");

    let empty = tmp("q5-empty");
    git_branch(&empty, "feat/y");
    app.panel.set_worktree(Some(empty.clone()));
    app.panel.refresh(&*runner, &empty);
    assert!(render_text(&app).contains("ramify new"));
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn existing_panes_still_work() {
    let root = tmp("q6");
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
