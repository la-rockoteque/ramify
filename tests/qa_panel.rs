#![allow(unused_variables)]
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::app::{App, Screen};
use ram_code::panel::Pane;
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-panel-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn out(text: &str) -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: text.into(),
        stderr: String::new(),
    }
}

struct FakeRunner {
    responses: Mutex<HashMap<Vec<String>, VecDeque<CommandOutput>>>,
    specs: Mutex<Vec<CommandSpec>>,
}

impl FakeRunner {
    fn new() -> Arc<Self> {
        Arc::new(Self {
            responses: Mutex::new(HashMap::new()),
            specs: Mutex::new(Vec::new()),
        })
    }

    fn push(&self, args: &[&str], output: CommandOutput) {
        let key: Vec<String> = args.iter().map(|arg| arg.to_string()).collect();
        self.responses
            .lock()
            .unwrap()
            .entry(key)
            .or_default()
            .push_back(output);
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
    let backend = TestBackend::new(140, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app, frame.area()))
        .unwrap();
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

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn panel_toggles_and_closes() {
    let root = tmp("q1");
    let runner = FakeRunner::new();
    let mut app = app_with(runner, &root);
    app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(app.panel.open);
    assert_eq!(app.panel.pane, Pane::Dashboard);
    app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(!app.panel.open);
    app.on_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
    assert!(app.panel.open);
    app.on_key(key(KeyCode::Esc));
    assert!(!app.panel.open);
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn dashboard_lists_worktrees() {
    let root = tmp("q2");
    let runner = FakeRunner::new();
    let dash = serde_json::json!([{
        "name": "demo",
        "primary": root,
        "worktrees": [{
            "name": "alpha",
            "branch": "feat/alpha",
            "slot": 1,
            "services": [{"name": "web", "port": 3101, "state": "ready"}]
        }]
    }])
    .to_string();
    runner.push(&["dash", "--json"], out(&dash));
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Dashboard);
    app.panel.refresh(&*runner, &root);
    let text = render_text(&app);
    for expected in ["demo", "alpha", "feat/alpha", "web", "3101", "ready", "slot 1"] {
        assert!(text.contains(expected), "{expected} missing:\n{text}");
    }

    let changed = serde_json::json!([{
        "name": "demo",
        "primary": root,
        "worktrees": [{
            "name": "alpha",
            "branch": "feat/alpha",
            "slot": 1,
            "services": [{"name": "web", "port": 3101, "state": "down"}]
        }]
    }])
    .to_string();
    runner.push(&["dash", "--json"], out(&changed));
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("down"));
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn approvals_approve_and_reject() {
    let root = tmp("q3");
    let worktree = root.join("alpha");
    std::fs::create_dir_all(&worktree).unwrap();
    let runner = FakeRunner::new();
    let gates = serde_json::json!([{
        "project": "demo",
        "path": worktree,
        "worktree": "alpha",
        "ask": {"step": "spec", "files": [{"path": "spec.md"}]}
    }])
    .to_string();
    runner.push(&["gates", "--json"], out(&gates));
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Approvals);
    app.panel.refresh(&*runner, &root);
    let text = render_text(&app);
    assert!(text.contains("demo/alpha"));
    assert!(text.contains("spec"));

    runner.push(&["gates", "--json"], out("[]"));
    runner.push(&["approve", "spec"], out("  spec approved"));
    app.on_key(key(KeyCode::Char('a')));
    let calls = runner.calls();
    assert!(calls
        .iter()
        .any(|spec| spec.args == vec!["approve", "spec"] && spec.root == worktree));

    runner.push(&["gates", "--json"], out(&gates));
    app.panel.refresh(&*runner, &root);
    runner.push(&["gates", "--json"], out("[]"));
    runner.push(&["reject", "spec", "--note", "needs work"], out("  spec rejected"));
    app.on_key(key(KeyCode::Char('r')));
    for character in "needs work".chars() {
        app.on_key(key(KeyCode::Char(character)));
    }
    app.on_key(key(KeyCode::Enter));
    let calls = runner.calls();
    assert!(calls.iter().any(|spec| spec.args
        == vec!["reject", "spec", "--note", "needs work"]));
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn scratch_lists_copies_and_removes() {
    let root = tmp("q4");
    let scratch = tmp("q4-scratch");
    std::env::set_var("RAMIFY_SCRATCH_DIR", &scratch);
    let copy = scratch.join("copied.txt");
    let clipboard = scratch.join("clipboard.sh");
    std::fs::write(
        &clipboard,
        format!("#!/bin/sh\ncat > '{}'\n", copy.display()),
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = std::fs::metadata(&clipboard).unwrap().permissions();
        perms.set_mode(0o755);
        std::fs::set_permissions(&clipboard, perms).unwrap();
    }
    std::env::set_var("RAM_CODE_CLIPBOARD", &clipboard);
    std::fs::write(scratch.join("20260101-1.txt"), "a message to copy\n").unwrap();
    std::fs::write(scratch.join("20260101-1.title"), "Note").unwrap();

    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Scratch);
    app.panel.refresh(&*runner, &root);
    let text = render_text(&app);
    assert!(text.contains("20260101-1"));
    assert!(text.contains("Note"));
    assert!(text.contains("a message to copy"));

    app.on_key(key(KeyCode::Char('c')));
    assert_eq!(std::fs::read_to_string(&copy).unwrap(), "a message to copy\n");
    app.on_key(key(KeyCode::Char('d')));
    assert!(!scratch.join("20260101-1.txt").exists());
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn run_go_and_block() {
    let root = tmp("q5");
    let runner = FakeRunner::new();
    let runs = serde_json::json!([
        {"id": "20260101-1", "status": "pending", "note": "try it", "root": root, "worktree": "demo"},
        {"id": "20260102-2", "status": "pending", "note": "", "root": root, "worktree": "demo"}
    ])
    .to_string();
    runner.push(&["runs", "--json"], out(&runs));
    let mut app = app_with(runner.clone(), &root);
    app.panel.open_on(Pane::Run);
    app.panel.refresh(&*runner, &root);
    let text = render_text(&app);
    assert!(text.contains("20260101-1"));
    assert!(text.contains("try it"));

    runner.push(&["runs", "--json"], out(&runs));
    runner.push(&["run", "go", "20260101-1"], out("hello\nrun 20260101-1 ran: exit 0"));
    app.on_key(key(KeyCode::Char('g')));
    assert!(runner
        .calls()
        .iter()
        .any(|spec| spec.args == vec!["run", "go", "20260101-1"]));
    assert!(render_text(&app).contains("ran: exit 0"));

    runner.push(&["runs", "--json"], out(&runs));
    runner.push(
        &["run", "block", "20260101-1", "--note", "not now"],
        out("run 20260101-1 blocked: not now"),
    );
    app.on_key(key(KeyCode::Char('b')));
    for character in "not now".chars() {
        app.on_key(key(KeyCode::Char(character)));
    }
    app.on_key(key(KeyCode::Enter));
    assert!(runner.calls().iter().any(|spec| spec.args
        == vec!["run", "block", "20260101-1", "--note", "not now"]));
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn panel_flag_opens_pane() {
    let root = tmp("q6");
    let runner = FakeRunner::new();
    let mut app = app_with(runner, &root);
    let pane = Pane::from_name("run").unwrap();
    app.panel.open_on(pane);
    assert!(app.panel.open);
    assert_eq!(app.panel.pane, Pane::Run);
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[test]
fn empty_states_name_the_command() {
    let root = tmp("q7");
    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &root);

    app.panel.open_on(Pane::Dashboard);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify up"));

    runner.push(&["gates", "--json"], out("[]"));
    app.panel.open_on(Pane::Approvals);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify step ask"));

    std::env::set_var("RAMIFY_SCRATCH_DIR", tmp("q7-scratch"));
    app.panel.open_on(Pane::Scratch);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify scratch"));

    runner.push(&["runs", "--json"], out("[]"));
    app.panel.open_on(Pane::Run);
    app.panel.refresh(&*runner, &root);
    assert!(render_text(&app).contains("ramify run ask"));
}
