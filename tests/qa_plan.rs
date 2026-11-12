use ram_code::agent::{run_task, run_turn, AgentConfig, AgentEvent, Credential, Llm};
use ram_code::app::{App, MessageKind, Role, Screen};
use ram_code::ramify::{Action, CommandOutput, CommandSpec, Runner};
use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use std::collections::HashMap;
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ── helpers ──────────────────────────────────────────────────────────────────

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-test-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn out(stdout: &str) -> CommandOutput {
    CommandOutput {
        status: 0,
        stdout: stdout.into(),
        stderr: String::new(),
    }
}

fn fail(stderr: &str) -> CommandOutput {
    CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: stderr.into(),
    }
}

struct FakeRunner {
    responses: Mutex<HashMap<Vec<String>, CommandOutput>>,
    specs: Mutex<Vec<CommandSpec>>,
}

impl FakeRunner {
    fn new(responses: Vec<(Vec<&str>, CommandOutput)>) -> Arc<Self> {
        let map = responses
            .into_iter()
            .map(|(k, v)| (k.into_iter().map(str::to_string).collect(), v))
            .collect();
        Arc::new(Self {
            responses: Mutex::new(map),
            specs: Mutex::new(Vec::new()),
        })
    }

    fn calls(&self) -> Vec<CommandSpec> {
        self.specs.lock().unwrap().clone()
    }
}

impl Runner for FakeRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        self.specs.lock().unwrap().push(spec.clone());
        Ok(self
            .responses
            .lock()
            .unwrap()
            .get(&spec.args)
            .cloned()
            .unwrap_or_else(|| out("")))
    }
}

fn dash_json(project: &str, primary: &Path, worktrees: &[(&str, &Path, &str)]) -> String {
    let list: Vec<serde_json::Value> = worktrees
        .iter()
        .enumerate()
        .map(|(index, (name, path, branch))| {
            let workflow = if *name == "beta" {
                serde_json::json!({
                    "workflow": "feature",
                    "lane": "standard",
                    "order": ".ramify/work/beta/order.md",
                    "done": 2,
                    "total": 8,
                    "next": {"step": "build", "skill": "ramify:build", "gate": "auto"},
                    "asked": null,
                    "steps": []
                })
            } else {
                serde_json::Value::Null
            };
            serde_json::json!({
                "name": name,
                "path": path,
                "branch": branch,
                "ticket": null,
                "workflow": workflow,
                "primary": index == 0,
                "dirty": *name == "beta",
                "slot": if *name == "beta" { 1 } else { 0 },
                "uptime": null,
                "errors": [],
                "services": if *name == "beta" {
                    serde_json::json!([{"service": "web", "port": 5300, "up": true}])
                } else {
                    serde_json::json!([])
                }
            })
        })
        .collect();
    serde_json::json!({
        "name": project,
        "primary": primary,
        "stacks": true,
        "jira": null,
        "containers": [],
        "shared": [],
        "build_servers": [],
        "strays": [],
        "orphans": [],
        "worktrees": list,
    })
    .to_string()
}

fn render_text(app: &App) -> String {
    use ratatui::backend::TestBackend;
    let backend = TestBackend::new(120, 40);
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

fn app_with(responses: Vec<(Vec<&str>, CommandOutput)>, root: &Path) -> (App, Arc<FakeRunner>) {
    let runner = FakeRunner::new(responses);
    (App::new(runner.clone(), root.to_path_buf()), runner)
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn startup_lists_project_and_worktrees() {
    let root = tmp("q1");
    let alpha = root.join("alpha");
    fs::create_dir_all(&alpha).unwrap();
    let json = dash_json(
        "poc-app",
        &root,
        &[("alpha", &root, "main"), ("alpha-wt", &alpha, "feat/alpha")],
    );
    let (mut app, _) = app_with(
        vec![(vec!["dash-project", "--json"], out(&json))],
        &root,
    );
    app.load();

    assert_eq!(app.screen, Screen::Main);
    let text = render_text(&app);
    assert!(text.contains("poc-app"), "project name missing:\n{text}");
    assert!(text.contains("alpha-wt"), "worktree name missing:\n{text}");
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn selecting_worktree_shows_status_and_workflow() {
    let root = tmp("q2");
    let beta = root.join("beta");
    fs::create_dir_all(&beta).unwrap();
    let json = dash_json("demo",
        &root,
        &[("demo", &root, "main"), ("beta", &beta, "feat/beta")],
    );
    let (mut app, _) = app_with(vec![(vec!["dash-project", "--json"], out(&json))], &root);
    app.load();
    app.select(1);
    app.open_selected();

    assert_eq!(app.screen, Screen::Detail);
    let text = render_text(&app);
    for expected in ["feat/beta", "dirty", "web", "5300", "feature", "standard", "build"] {
        assert!(text.contains(expected), "{expected} missing:\n{text}");
    }
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn actions_call_ramify_with_selected_root() {
    let root = tmp("q3");
    let beta = root.join("beta");
    fs::create_dir_all(&beta).unwrap();
    let json = dash_json("demo",
        &root,
        &[("demo", &root, "main"), ("beta", &beta, "feat/beta")],
    );
    let (mut app, runner) = app_with(
        vec![
            (vec!["dash-project", "--json"], out(&json)),
            (vec!["up"], out("stack up")),
            (vec!["down"], out("stack down")),
            (vec!["step"], out("→ build")),
            (vec!["new", "demo"], out("created demo")),
        ],
        &root,
    );
    app.load();
    app.select(1);

    for action in [
        Action::Up,
        Action::Down,
        Action::Step,
        Action::CreateWorktree("demo".into()),
    ] {
        app.invoke(action.clone());
        let text = render_text(&app);
        let expected = match action {
            Action::Up => "stack up",
            Action::Down => "stack down",
            Action::Step => "→ build",
            Action::CreateWorktree(_) => "created demo",
            _ => unreachable!(),
        };
        assert!(text.contains(expected), "output missing for {action:?}:\n{text}");
    }

    let calls = runner.calls();
    let actions: Vec<&CommandSpec> = calls
        .iter()
        .filter(|spec| spec.args != vec!["dash-project".to_string(), "--json".to_string()])
        .collect();
    assert_eq!(actions.len(), 4, "expected four actions: {actions:?}");
    for spec in &actions {
        assert_eq!(spec.root, beta, "wrong root for {:?}", spec.args);
    }
    assert_eq!(actions[0].args, vec!["up"]);
    assert_eq!(actions[1].args, vec!["down"]);
    assert_eq!(actions[2].args, vec!["step"]);
    assert_eq!(actions[3].args, vec!["new", "demo"]);
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn missing_project_state_shows_recovery() {
    let root = tmp("q4-empty");
    let missing = CommandOutput {
        status: 1,
        stdout: String::new(),
        stderr: "ramify: no .ramify.conf or .claude/ramify.conf in /tmp/x — run 'ramify setup'"
            .into(),
    };
    let (mut empty, _) = app_with(vec![(vec!["dash-project", "--json"], missing)], &root);
    empty.load();
    assert_eq!(empty.screen, Screen::Empty);
    let text = render_text(&empty);
    assert!(text.contains("No Ramify project"), "empty state text missing:\n{text}");
    assert!(text.contains("ramify setup"), "recovery hint missing:\n{text}");

    let root = tmp("q4-invalid");
    let (mut broken, _) = app_with(
        vec![(vec!["dash-project", "--json"], out("not json"))],
        &root,
    );
    broken.load();
    assert_eq!(broken.screen, Screen::Error);
    let text = render_text(&broken);
    assert!(
        text.contains("could not read Ramify state"),
        "error state text missing:\n{text}"
    );
    assert!(text.contains("r"), "retry hint missing:\n{text}");
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn command_failure_returns_to_interface() {
    let root = tmp("q5");
    let json = dash_json("demo", &root, &[("demo", &root, "main")]);
    let (mut app, _) = app_with(
        vec![
            (vec!["dash-project", "--json"], out(&json)),
            (vec!["up"], fail("stack failed: port busy")),
        ],
        &root,
    );
    app.load();
    app.invoke(Action::Up);

    assert_eq!(app.screen, Screen::Message);
    assert_eq!(app.message_kind, MessageKind::Failure);
    let text = render_text(&app);
    assert!(text.contains("stack failed: port busy"), "failure missing:\n{text}");

    app.back();
    assert_eq!(app.screen, Screen::Main);
    let text = render_text(&app);
    assert!(text.contains("demo"), "list not usable after failure:\n{text}");
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn terminal_app_runs_without_electron() {
    let root = tmp("q6");
    let bin_dir = root.join("fake-bin");
    fs::create_dir_all(&bin_dir).unwrap();
    let json = dash_json("poc", &root, &[("poc", &root, "main")]);
    let escaped = json.replace('\'', "'\\''");
    let script = format!(
        "#!/bin/sh\nif [ \"$1\" = \"dash-project\" ]; then\n  printf '%s' '{escaped}'\n  exit 0\nfi\nprintf 'ok\\n'\nexit 0\n"
    );
    let fake = bin_dir.join("ramify");
    fs::write(&fake, script).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(&fake).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&fake, perms).unwrap();
    }

    let binary = env!("CARGO_BIN_EXE_ram-code");
    // A non-tty parent gives the pseudo-terminal a 0x0 window. Fix it before the TUI starts.
    let command = format!("stty rows 30 cols 100 2>/dev/null; exec '{binary}'");
    let (program, args): (&str, Vec<&str>) = if cfg!(target_os = "macos") {
        ("script", vec!["-q", "/dev/null", "/bin/sh", "-c", &command])
    } else {
        ("script", vec!["-qec", &command, "/dev/null"])
    };
    let mut child = Command::new(program)
        .args(&args)
        .current_dir(&root)
        .env(
            "PATH",
            format!("{}:/usr/bin:/bin", bin_dir.display()),
        )
        .env("TERM", "xterm-256color")
        .env("HOME", &root)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("spawn under a pseudo-terminal");

    let mut stdin = child.stdin.take().unwrap();
    let writer = std::thread::spawn(move || {
        // Dismiss the splash, then wait for the loader before quitting.
        std::thread::sleep(Duration::from_millis(1200));
        let _ = stdin.write_all(b"\x03");
        let _ = stdin.flush();
        std::thread::sleep(Duration::from_millis(1000));
        for _ in 0..30 {
            if stdin.write_all(b"\x03").is_err() {
                break;
            }
            let _ = stdin.flush();
            std::thread::sleep(Duration::from_millis(100));
        }
    });

    let deadline = Instant::now() + Duration::from_secs(10);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break status;
        }
        if Instant::now() > deadline {
            let _ = child.kill();
            panic!("the TUI did not exit after 'q'");
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    let output = child.wait_with_output().unwrap();
    let _ = writer.join();
    let stdout = String::from_utf8_lossy(&output.stdout);

    assert!(status.success(), "the TUI exited with {status}:\n{stdout}");
    assert!(stdout.contains("poc"), "the TUI did not start:\n{stdout}");
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

struct FakeLlm {
    responses: Mutex<Vec<serde_json::Value>>,
    requests: Mutex<Vec<serde_json::Value>>,
}

impl FakeLlm {
    fn new(responses: Vec<serde_json::Value>) -> Self {
        Self {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Llm for FakeLlm {
    fn complete(&self, request: &serde_json::Value) -> Result<serde_json::Value, String> {
        self.requests.lock().unwrap().push(request.clone());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Err("no more fake responses".into())
        } else {
            Ok(responses.remove(0))
        }
    }
}

#[test]
fn coding_task_sends_tools_to_worktree() {
    let root = tmp("q7");
    let llm = FakeLlm::new(vec![
        serde_json::json!({
            "content": [
                {"type": "tool_use", "id": "t1", "name": "write_file",
                 "input": {"path": "hello.txt", "content": "hi from deepseek"}}
            ],
            "stop_reason": "tool_use"
        }),
        serde_json::json!({
            "content": [
                {"type": "text", "text": "done: wrote hello.txt"}
            ],
            "stop_reason": "end_turn"
        }),
    ]);

    let mut events = Vec::new();
    let final_text = run_task(&llm, &root, "write hello.txt", &mut |event| {
        events.push(event);
    })
    .expect("the task runs");

    let written = fs::read_to_string(root.join("hello.txt")).unwrap();
    assert_eq!(written, "hi from deepseek");
    assert!(
        events.iter().any(|e| matches!(
            e,
            AgentEvent::ToolCall { name, .. } if name == "write_file"
        )),
        "the tool call was not shown: {events:?}"
    );
    assert!(final_text.contains("done: wrote hello.txt"), "{final_text}");

    let requests = llm.requests.lock().unwrap();
    assert!(!requests.is_empty(), "the model was not called");
    let tools = requests[0]["tools"].as_array().expect("tools sent");
    assert!(
        tools.iter().any(|t| t["name"] == "write_file"),
        "write_file not offered: {tools:?}"
    );
}

// ── Q8 ───────────────────────────────────────────────────────────────────────

#[test]
fn missing_api_token_shows_recovery() {
    let root = tmp("q8");
    let json = dash_json("demo", &root, &[("demo", &root, "main")]);
    let (mut app, _) = app_with(vec![(vec!["dash-project", "--json"], out(&json))], &root);
    app.load();

    let config = AgentConfig::from_env(|_| None);
    assert!(config.is_err(), "a missing token must be an error");
    app.start_task(config);

    assert_eq!(app.screen, Screen::Message);
    assert_eq!(app.message_kind, MessageKind::Failure);
    let text = render_text(&app);
    assert!(
        text.contains("ANTHROPIC_AUTH_TOKEN"),
        "the fix is not named:\n{text}"
    );

    app.back();
    assert_eq!(app.screen, Screen::Main);
    assert!(render_text(&app).contains("demo"));
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[test]
fn follow_up_task_keeps_early_context() {
    let root = tmp("q9");
    let llm = FakeLlm::new(vec![
        serde_json::json!({
            "content": [{"type": "text", "text": "first answer"}],
            "stop_reason": "end_turn"
        }),
        serde_json::json!({
            "content": [{"type": "text", "text": "second answer"}],
            "stop_reason": "end_turn"
        }),
    ]);

    let (first, history) = run_turn(&llm, &root, &[], "first task", &mut |_| {}).expect("first");
    assert_eq!(first, "first answer");
    let (second, _) = run_turn(&llm, &root, &history, "second task", &mut |_| {}).expect("second");
    assert_eq!(second, "second answer");

    let requests = llm.requests.lock().unwrap();
    let sent = requests[1]["messages"].to_string();
    assert!(sent.contains("first task"), "first task missing: {sent}");
    assert!(sent.contains("first answer"), "first answer missing: {sent}");
    assert!(sent.contains("second task"), "second task missing: {sent}");
}

// ── Delivered-path coverage ──────────────────────────────────────────────────

fn write_script(path: &Path, body: &str) {
    fs::write(path, body).unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut perms = fs::metadata(path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(path, perms).unwrap();
    }
}

#[test]
fn slash_action_runs_in_the_selected_root() {
    let root = tmp("slash");
    let beta = root.join("beta");
    fs::create_dir_all(&beta).unwrap();
    let json = dash_json(
        "demo",
        &root,
        &[("demo", &root, "main"), ("beta", &beta, "feat/beta")],
    );
    let (mut app, runner) = app_with(
        vec![
            (vec!["dash-project", "--json"], out(&json)),
            (vec!["up"], out("stack up")),
        ],
        &root,
    );
    app.load();
    app.select(1);

    let bin_dir = tmp("slash-bin");
    let fake = bin_dir.join("ramify");
    let escaped = json.replace('\'', "'\\''");
    write_script(&fake, &format!("#!/bin/sh\nprintf '%s' '{escaped}'\n"));
    app.ramify_bin = Some(fake);

    app.input = "/up".into();
    app.on_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));

    let calls = runner.calls();
    assert!(
        calls
            .iter()
            .any(|spec| spec.args == vec!["up"] && spec.root == beta),
        "the slash command did not run ramify in the selected root: {calls:?}"
    );
    assert!(
        app.entries
            .iter()
            .any(|entry| entry.role == Role::Info && entry.text.contains("stack up")),
        "the command output is not in the transcript: {:?}",
        app.entries
    );
}

#[test]
fn worker_events_reach_the_transcript() {
    let root = tmp("worker");
    // A task writes Ram Code's session record: keep it out of the machine's state folder.
    std::env::set_var("RAMIFY_STATE_DIR", tmp("worker-state"));
    let json = dash_json("demo", &root, &[("demo", &root, "main")]);
    let (mut app, _) = app_with(vec![(vec!["dash-project", "--json"], out(&json))], &root);
    app.load();

    let script_dir = tmp("worker-script");
    let script = script_dir.join("worker.sh");
    write_script(
        &script,
        "#!/bin/sh\nevents=\"$4\"\nhistory=\"$6\"\n\
         printf '{\"type\":\"text\",\"text\":\"hello\"}\\n' >> \"$events\"\n\
         printf '{\"type\":\"tool_call\",\"name\":\"read_file\",\"input\":\"{}\"}\\n' >> \"$events\"\n\
         printf '[]' > \"$history\"\n\
         printf '{\"type\":\"finished\",\"answer\":\"hello\"}\\n' >> \"$events\"\n",
    );
    app.worker_binary = Some(script);

    let config = AgentConfig {
        base_url: "http://127.0.0.1:1".into(),
        credential: Credential::AuthToken("test".into()),
        model: "test".into(),
        max_tokens: 10,
    };
    app.start_task_with_text(Ok(config), "go".into());
    for _ in 0..200 {
        app.poll_events();
        if !app.running {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }

    assert!(!app.running, "the worker never finished");
    assert!(app.entries.iter().any(|e| e.role == Role::User && e.text == "go"));
    assert!(app
        .entries
        .iter()
        .any(|e| e.role == Role::Assistant && e.text == "hello"));
    assert!(app
        .entries
        .iter()
        .any(|e| e.role == Role::Tool && e.text.contains("read_file")));
    assert!(app.history.is_empty(), "the history file was not read back");
}

#[test]
fn async_load_lists_worktrees() {
    let root = tmp("async-load");
    let json = dash_json("poc-app", &root, &[("poc-app", &root, "main")]);
    let bin_dir = tmp("async-bin");
    let fake = bin_dir.join("ramify");
    let escaped = json.replace('\'', "'\\''");
    write_script(&fake, &format!("#!/bin/sh\nprintf '%s' '{escaped}'\n"));

    let (mut app, _) = app_with(vec![], &root);
    app.ramify_bin = Some(fake);
    app.load_async();
    for _ in 0..200 {
        app.poll_events();
        if app.screen != Screen::Loading {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(app.screen, Screen::Main);
    assert!(render_text(&app).contains("poc-app"));
}
