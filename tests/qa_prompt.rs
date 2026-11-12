//! Ram Code shows the ramify prompt line, keeps a session record of its own, and lists the live
//! session records in the Sessions pane.

use ram_code::app::{App, Screen};
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use ram_code::state::{Project, Worktree};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::Duration;

/// The tests share RAMIFY_STATE_DIR, a process-wide variable: one at a time.
static STATE: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    STATE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ram-prompt-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

fn out(text: &str) -> CommandOutput {
    CommandOutput { status: 0, stdout: text.into(), stderr: String::new() }
}

/// Canned outputs per (root, args), in order.
type Responses = HashMap<(PathBuf, Vec<String>), VecDeque<CommandOutput>>;

struct FakeRunner {
    responses: Mutex<Responses>,
    specs: Mutex<Vec<CommandSpec>>,
}

impl FakeRunner {
    fn new() -> Arc<Self> {
        Arc::new(Self { responses: Mutex::new(HashMap::new()), specs: Mutex::new(Vec::new()) })
    }
    fn push(&self, root: &Path, args: &[&str], output: CommandOutput) {
        let key = (root.to_path_buf(), args.iter().map(|arg| arg.to_string()).collect());
        self.responses.lock().unwrap().entry(key).or_default().push_back(output);
    }
}

impl Runner for FakeRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        self.specs.lock().unwrap().push(spec.clone());
        let mut responses = self.responses.lock().unwrap();
        let queue = responses.entry((spec.root.clone(), spec.args.clone())).or_default();
        Ok(queue.front().cloned().map(|_| queue.pop_front().unwrap()).unwrap_or_else(|| out("")))
    }
}

fn worktree(name: &str, path: &Path) -> Worktree {
    Worktree {
        name: name.into(),
        path: path.to_path_buf(),
        branch: format!("feat/{name}"),
        primary: false,
        dirty: false,
        slot: None,
        services: Vec::new(),
        workflow: None,
    }
}

fn app_with(runner: Arc<FakeRunner>, base: &Path) -> App {
    let mut app = App::new(runner, base.to_path_buf());
    app.screen = Screen::Main;
    app.project = Some(Project {
        name: "demo".into(),
        primary: base.to_path_buf(),
        worktrees: vec![worktree("wt-1", &base.join("wt-1")), worktree("wt-2", &base.join("wt-2"))],
    });
    app
}

fn render_text(app: &App) -> String {
    use ratatui::backend::TestBackend;
    let backend = TestBackend::new(160, 40);
    let mut terminal = ratatui::Terminal::new(backend).unwrap();
    terminal.draw(|frame| frame.render_widget(app, frame.area())).unwrap();
    let buffer = terminal.backend().buffer();
    let mut text = String::new();
    for y in 0..buffer.area.height {
        for x in 0..buffer.area.width {
            text.push_str(buffer[(x, y)].symbol());
        }
        text.push('\n');
    }
    text
}

/// The prompt line comes from a background thread: tick until it shows, for at most 2 s.
fn settle(app: &mut App, wanted: &str) -> String {
    let mut text = String::new();
    for _ in 0..200 {
        app.tick();
        text = render_text(app);
        if text.contains(wanted) {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    text
}

#[test]
fn header_shows_the_prompt_line() {
    let _guard = serial();
    let base = tmp("header");
    std::env::set_var("RAMIFY_STATE_DIR", base.join("state"));
    let runner = FakeRunner::new();
    runner.push(&base.join("wt-1"), &["prompt"], out("𖣂 wt-1 ⎇ feat/wt-1 · ✻ birch\n"));
    runner.push(&base.join("wt-2"), &["prompt"], out("𖣂 wt-2 ⎇ feat/wt-2 · ◇ ash ⏸\n"));
    let mut app = app_with(runner, &base);
    let text = settle(&mut app, "wt-1 ⎇ feat/wt-1 · ✻ birch");
    assert!(text.contains("wt-1 ⎇ feat/wt-1 · ✻ birch"), "{text}");
    app.select(1);
    app.tick();
    let text = render_text(&app);
    assert!(!text.contains("✻ birch"), "the line of the other worktree stays: {text}");
    let text = settle(&mut app, "wt-2 ⎇ feat/wt-2 · ◇ ash ⏸");
    assert!(text.contains("wt-2 ⎇ feat/wt-2 · ◇ ash ⏸"), "{text}");
}

#[test]
fn sessions_pane_lists_live_records() {
    let _guard = serial();
    let base = tmp("pane");
    let state = base.join("state");
    std::env::set_var("RAMIFY_STATE_DIR", &state);
    let sessions = state.join("sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let wt = base.join("wt-1");
    std::fs::create_dir_all(&wt).unwrap();
    let pid = std::process::id();
    let write = |id: &str, agent: &str, status: &str, since: u64| {
        let record = serde_json::json!({"agent": agent, "root": wt, "pid": pid, "state": status, "since": since});
        std::fs::write(sessions.join(format!("{id}.session")), record.to_string()).unwrap();
    };
    write("c1", "claude", "working", 1);
    write("x1", "codex", "idle", 2);
    std::fs::write(sessions.join("c1.name"), "birch\n").unwrap();
    let mut app = app_with(FakeRunner::new(), &base);
    app.panel.open = true;
    app.on_key(KeyEvent::new(KeyCode::Char('5'), KeyModifiers::NONE));
    app.tick();
    let text = render_text(&app);
    assert!(text.contains("✻ birch"), "{text}");
    assert!(text.contains("✻ birch ") && !text.contains("✻ birch ⏸"), "{text}");
    assert!(text.contains("◇ codex ⏸"), "{text}");
}

#[test]
fn ram_code_refreshes_the_health_cache() {
    let _guard = serial();
    let base = tmp("health");
    std::env::set_var("RAMIFY_STATE_DIR", base.join("state"));
    let runner = FakeRunner::new();
    let mut app = app_with(runner.clone(), &base);
    app.tick();
    let ran = || {
        runner
            .specs
            .lock()
            .unwrap()
            .iter()
            .filter(|spec| spec.args == ["statusline"] && spec.root == base.join("wt-1"))
            .count()
    };
    for _ in 0..100 {
        if ran() > 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(ran(), 1, "statusline did not run for the selected worktree");
    app.tick();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(ran(), 1, "statusline ran again before 30 seconds");
}
