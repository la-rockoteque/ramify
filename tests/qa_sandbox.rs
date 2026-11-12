//! The cases of `.ramify/work/ram-code-sandbox/qa-plan.md` that the build settles.

use ram_code::app::App;
use ram_code::panel::Pane;
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use ram_code::sandbox::{scrub, Sandbox};
use ram_code::tools::{timeout_of, Tools};
use ratatui::style::Color;
use serde_json::json;
use std::collections::HashMap;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ── helpers ──────────────────────────────────────────────────────────────────

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-sbx-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

/// A worktree inside its own folder, so a sibling path is outside it.
fn worktree(name: &str) -> (PathBuf, PathBuf) {
    let base = tmp(name);
    let root = base.join("wt");
    fs::create_dir_all(&root).unwrap();
    (base, root)
}

fn base_env() -> Vec<(String, String)> {
    ["PATH", "HOME", "USER", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
        .collect()
}

fn tools(root: &Path, extra: &[(&str, &str)]) -> Tools {
    let mut env = base_env();
    env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
    Tools::with_env(root, env)
}

fn run(tools: &Tools, input: serde_json::Value) -> String {
    match tools.call("run_command", &input) {
        Ok(text) | Err(text) => text,
    }
}

fn executable(path: &Path, text: &str) {
    fs::write(path, text).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

/// A fake `ramify` that records each call and answers `run ask` and `run wait` from files.
fn fake_ramify(dir: &Path, wait_reply: &str, wait_exit: i32) -> PathBuf {
    let program = dir.join("ramify");
    fs::write(dir.join("wait-reply.txt"), wait_reply).unwrap();
    executable(
        &program,
        &format!(
            "#!/bin/sh\n\
             printf '%s\\n' \"$*\" >> '{log}'\n\
             if [ \"$1 $2\" = 'run ask' ]; then cp \"$3\" '{copy}'; \
             echo 'run 20261112-120000-42 waits on the human — the run window shows x (frozen at y)'; \
             echo '  wait for the decision: ramify run wait 20261112-120000-42'; exit 0; fi\n\
             if [ \"$1 $2\" = 'run wait' ]; then cat '{reply}'; exit {code}; fi\n\
             exit 3\n",
            log = dir.join("calls.log").display(),
            copy = dir.join("asked-script.sh").display(),
            reply = dir.join("wait-reply.txt").display(),
            code = wait_exit,
        ),
    );
    program
}

fn calls(dir: &Path) -> Vec<String> {
    fs::read_to_string(dir.join("calls.log"))
        .unwrap_or_default()
        .lines()
        .map(String::from)
        .collect()
}

// ── Q1–Q5: the sandbox on macOS ──────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn sandbox_allows_writes_in_the_worktree() {
    let (_base, root) = worktree("q1");
    let tools = tools(&root, &[]);
    let out = run(&tools, json!({"command": "echo hi > out.txt"}));
    assert!(out.starts_with("exit 0"), "{out}");
    assert_eq!(fs::read_to_string(root.join("out.txt")).unwrap(), "hi\n");
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_allows_the_task_temp_folder() {
    let (_base, root) = worktree("q2");
    let tools = tools(&root, &[]);
    let out = run(&tools, json!({"command": "touch \"$TMPDIR/x\" && ls \"$TMPDIR\""}));
    assert!(out.starts_with("exit 0"), "{out}");
    assert!(out.contains('x'), "{out}");
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_denies_writes_outside() {
    let (base, root) = worktree("q3");
    let tools = tools(&root, &[]);
    let target = base.join("outside.txt");
    let out = run(
        &tools,
        json!({"command": format!("echo no > '{}'", target.display())}),
    );
    assert!(!out.starts_with("exit 0"), "{out}");
    assert!(!target.exists());
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_denies_writes_through_a_symlink() {
    let (base, root) = worktree("q4");
    let outside = base.join("elsewhere");
    fs::create_dir_all(&outside).unwrap();
    std::os::unix::fs::symlink(&outside, root.join("link")).unwrap();
    let tools = tools(&root, &[]);
    let out = run(&tools, json!({"command": "echo no > link/file.txt"}));
    assert!(!out.starts_with("exit 0"), "{out}");
    assert!(!outside.join("file.txt").exists());
}

#[cfg(target_os = "macos")]
#[test]
fn sandbox_denies_the_network() {
    use std::net::TcpListener;
    let (_base, root) = worktree("q5");
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    listener.set_nonblocking(true).unwrap();
    let port = listener.local_addr().unwrap().port();
    let tools = tools(&root, &[]);
    let out = run(
        &tools,
        json!({"command": format!("curl -s --max-time 3 http://127.0.0.1:{port}/"), "timeout_seconds": 10}),
    );
    assert!(!out.starts_with("exit 0"), "{out}");
    assert!(listener.accept().is_err(), "the listener saw a connection");
}

// ── Q6, Q7: keys leave the environment ───────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn sandbox_strips_keys() {
    let (base, root) = worktree("q6");
    let config = base.join("config");
    fs::create_dir_all(&config).unwrap();
    fs::write(
        config.join("providers.json"),
        json!({"providers": [{"name": "team", "base_url": "https://llm.example.com",
            "key_env": "TEAM_LLM", "model": "m"}]})
        .to_string(),
    )
    .unwrap();
    let secrets = [
        ("ANTHROPIC_API_KEY", "s-anthropic"),
        ("OPENAI_API_KEY", "s-openai"),
        ("DEEPSEEK_API_KEY", "s-deepseek"),
        ("TEAM_LLM", "s-team"),
        ("MY_SERVICE_TOKEN", "s-token"),
        ("A_SECRET", "s-secret"),
        ("DB_PASSWORD_FILE", "s-password"),
    ];
    let mut extra: Vec<(&str, &str)> = secrets.to_vec();
    let config_text = config.display().to_string();
    extra.push(("RAM_CODE_CONFIG_DIR", &config_text));
    let tools = tools(&root, &extra);
    let out = run(&tools, json!({"command": "env"}));
    assert!(out.starts_with("exit 0"), "{out}");
    for (name, value) in secrets {
        assert!(!out.contains(value), "{name} reached the command:\n{out}");
    }
    assert!(out.contains("\nPATH=") || out.contains("PATH="), "{out}");
    assert!(out.contains("HOME="), "{out}");
}

#[test]
fn scrub_list_covers_every_provider_key() {
    let names = [
        "ANTHROPIC_API_KEY",
        "ANTHROPIC_AUTH_TOKEN",
        "ANTHROPIC_BASE_URL",
        "OPENAI_API_KEY",
        "DEEPSEEK_API_KEY",
        "TEAM_LLM",
        "MY_SERVICE_TOKEN",
        "A_SECRET",
        "DB_PASSWORD_FILE",
        "PATH",
        "HOME",
        "KEYBOARD",
        "TOKENIZERS_PARALLELISM",
    ];
    let env: Vec<(String, String)> = names.iter().map(|n| (n.to_string(), "v".into())).collect();
    let kept: Vec<String> = scrub(&env, &["TEAM_LLM".to_string()])
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    assert_eq!(kept, vec!["PATH", "HOME", "KEYBOARD", "TOKENIZERS_PARALLELISM"]);
}

// ── Q8–Q10: timeouts ─────────────────────────────────────────────────────────

#[test]
fn timeout_kills_the_process_group() {
    let (_base, root) = worktree("q8");
    let tools = tools(&root, &[]);
    let started = Instant::now();
    let out = run(
        &tools,
        json!({"command": "echo $$ > pgid.txt; sleep 30 & sleep 30", "timeout_seconds": 1}),
    );
    assert!(started.elapsed() < Duration::from_secs(3), "took {:?}", started.elapsed());
    assert!(out.contains("timed out after 1 s"), "{out}");
    let pgid: i32 = fs::read_to_string(root.join("pgid.txt")).unwrap().trim().parse().unwrap();
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        // SAFETY: signal 0 only checks that the group exists.
        let alive = unsafe { libc::kill(-pgid, 0) } == 0;
        if !alive {
            break;
        }
        assert!(Instant::now() < deadline, "a process of group {pgid} still runs");
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn timeout_is_clamped() {
    assert_eq!(timeout_of(&json!({"timeout_seconds": 5000})), 600);
    assert_eq!(timeout_of(&json!({})), 120);
    assert_eq!(timeout_of(&json!({"timeout_seconds": 0})), 1);
    assert_eq!(timeout_of(&json!({"timeout_seconds": 30})), 30);
}

#[test]
fn timeout_keeps_partial_output() {
    let (_base, root) = worktree("q10");
    let tools = tools(&root, &[]);
    let out = run(&tools, json!({"command": "echo start; sleep 30", "timeout_seconds": 1}));
    let start = out.find("start").expect("partial output");
    let note = out.find("timed out after 1 s").expect("timeout note");
    assert!(start < note, "{out}");
}

// ── Q11–Q13, Q16–Q18: commands outside the sandbox ───────────────────────────

#[test]
fn outside_sandbox_asks_ramify_run() {
    let (base, root) = worktree("q11");
    let program = fake_ramify(&base, "ran: exit 0\nhello from outside\n", 0);
    let mut tools = tools(&root, &[]);
    tools.ramify = program.display().to_string();
    tools.no_window = false;
    let mut pending = Vec::new();
    let out = tools
        .call_with(
            "run_command",
            &json!({"command": "curl -sI https://example.com", "outside_sandbox": true,
                "reason": "fetch the headers"}),
            &mut |id| pending.push(id.to_string()),
        )
        .unwrap();
    assert_eq!(out.trim(), "ran: exit 0\nhello from outside");
    assert_eq!(pending, vec!["20261112-120000-42"]);
    let log = calls(&base);
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log[0].starts_with("run ask "), "{log:?}");
    assert!(
        log[0].ends_with(&format!("--cwd {} --note fetch the headers", root.display())),
        "{log:?}"
    );
    assert!(log[1].starts_with("run wait 20261112-120000-42 --timeout "), "{log:?}");
    let script = fs::read_to_string(base.join("asked-script.sh")).unwrap();
    assert!(script.contains("curl -sI https://example.com"), "{script}");
    assert!(script.contains("fetch the headers"), "{script}");
}

#[test]
fn tui_asks_without_the_window() {
    let (base, root) = worktree("q12");
    let program = fake_ramify(&base, "ran: exit 0\n", 0);
    let input = json!({"command": "true", "outside_sandbox": true, "reason": "r"});

    let program_text = program.display().to_string();
    let tui = tools(&root, &[("RAM_CODE_NO_WINDOW", "1"), ("RAM_CODE_RAMIFY", &program_text)]);
    tui.call("run_command", &input).unwrap();
    let headless = tools(&root, &[("RAM_CODE_RAMIFY", &program_text)]);
    headless.call("run_command", &input).unwrap();

    let asks: Vec<String> = calls(&base).into_iter().filter(|c| c.starts_with("run ask")).collect();
    assert_eq!(asks.len(), 2, "{asks:?}");
    assert!(asks[0].ends_with("--no-window"), "{asks:?}");
    assert!(!asks[1].contains("--no-window"), "{asks:?}");

    // The TUI sets the variable on its worker.
    let seen = base.join("worker-env.txt");
    let worker = base.join("worker.sh");
    executable(
        &worker,
        &format!(
            "#!/bin/sh\nprintf '%s' \"$RAM_CODE_NO_WINDOW\" > '{}'\n\
             printf '[]' > \"$6\"\nprintf '{{\"type\":\"finished\",\"answer\":\"ok\"}}\\n' >> \"$4\"\n",
            seen.display()
        ),
    );
    let runner = FakeRunner::new(&root);
    let mut app = task_app(runner, &root, &base);
    app.worker_binary = Some(worker);
    app.input = "go".into();
    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    wait_until(&mut app, |app| !app.running);
    assert_eq!(fs::read_to_string(&seen).unwrap(), "1");
}

#[test]
fn blocked_run_reaches_the_model() {
    let (base, root) = worktree("q16");
    let program = fake_ramify(&base, "blocked: too risky\n", 0);
    let mut tools = tools(&root, &[]);
    tools.ramify = program.display().to_string();
    let marker = root.join("ran.txt");
    let out = tools
        .call(
            "run_command",
            &json!({"command": format!("touch '{}'", marker.display()),
                "outside_sandbox": true, "reason": "r"}),
        )
        .unwrap();
    assert_eq!(out.trim(), "blocked: too risky");
    assert!(!marker.exists(), "the blocked command ran");
}

#[test]
fn no_sandbox_goes_to_the_human() {
    let (base, root) = worktree("q17");
    let program = fake_ramify(&base, "blocked: no\n", 0);
    let mut tools = tools(&root, &[]);
    tools.ramify = program.display().to_string();
    tools.sandbox = Sandbox::None;
    let marker = root.join("ran.txt");
    let out = tools
        .call("run_command", &json!({"command": format!("touch '{}'", marker.display())}))
        .unwrap();
    assert_eq!(out.trim(), "blocked: no");
    assert!(!marker.exists(), "the command ran unconfined");
    let log = calls(&base);
    assert!(log[0].starts_with("run ask "), "{log:?}");
}

#[test]
fn no_decision_leaves_the_run_pending() {
    let (base, root) = worktree("q18");
    let program = fake_ramify(&base, "run 20261112-120000-42 is still pending\n", 1);
    let mut tools = tools(&root, &[]);
    tools.ramify = program.display().to_string();
    tools.run_wait = 1;
    let out = tools
        .call("run_command", &json!({"command": "true", "outside_sandbox": true, "reason": "r"}))
        .unwrap();
    assert_eq!(out, "no decision; the run stays pending in the Run pane");
    let log = calls(&base);
    assert_eq!(log.len(), 2, "{log:?}");
    assert!(log.iter().all(|c| !c.starts_with("run go") && !c.starts_with("run block")));
    assert_eq!(log[1], "run wait 20261112-120000-42 --timeout 1");
}

#[test]
fn outside_sandbox_needs_a_reason() {
    let (_base, root) = worktree("reason");
    let tools = tools(&root, &[]);
    let error = tools
        .call("run_command", &json!({"command": "true", "outside_sandbox": true}))
        .unwrap_err();
    assert!(error.contains("reason"), "{error}");
}

// ── Q14, Q15: the Run pane ───────────────────────────────────────────────────

struct FakeRunner {
    root: PathBuf,
    replies: Mutex<HashMap<Vec<String>, CommandOutput>>,
}

impl FakeRunner {
    fn new(root: &Path) -> Arc<Self> {
        Arc::new(Self {
            root: root.to_path_buf(),
            replies: Mutex::new(HashMap::new()),
        })
    }

    fn reply(&self, args: &[&str], stdout: String) {
        self.replies.lock().unwrap().insert(
            args.iter().map(|a| a.to_string()).collect(),
            CommandOutput { status: 0, stdout, stderr: String::new() },
        );
    }
}

impl Runner for FakeRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        if let Some(found) = self.replies.lock().unwrap().get(&spec.args) {
            return Ok(found.clone());
        }
        let stdout = if spec.args.first().map(String::as_str) == Some("dash-project") {
            json!({
                "name": "demo", "primary": self.root, "stacks": true, "jira": null,
                "containers": [], "shared": [], "build_servers": [], "strays": [], "orphans": [],
                "worktrees": [{"name": "demo", "path": self.root, "branch": "main", "ticket": null,
                    "workflow": null, "primary": true, "dirty": false, "slot": 0, "uptime": null,
                    "errors": [], "services": []}],
            })
            .to_string()
        } else {
            String::new()
        };
        Ok(CommandOutput { status: 0, stdout, stderr: String::new() })
    }
}

/// An app on `root` with a test credential and an empty providers folder, so a task starts.
fn task_app(runner: Arc<FakeRunner>, root: &Path, base: &Path) -> App {
    let mut app = App::new(runner, root.to_path_buf());
    let config = base.join("ram-code-config");
    fs::create_dir_all(&config).unwrap();
    app.env = Arc::new(|key| (key == "ANTHROPIC_AUTH_TOKEN").then(|| "test".to_string()));
    app.load_providers(config);
    app.load();
    app
}

fn wait_until(app: &mut App, done: impl Fn(&App) -> bool) {
    for _ in 0..400 {
        app.poll_events();
        if done(app) {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the condition never held");
}

fn runs_json(ids: &[&str]) -> String {
    serde_json::Value::Array(
        ids.iter()
            .map(|id| json!({"id": id, "status": "pending", "note": format!("note {id}"),
                "worktree": "demo", "root": null}))
            .collect(),
    )
    .to_string()
}

fn buffer(app: &App) -> ratatui::buffer::Buffer {
    use ratatui::backend::TestBackend;
    let mut terminal = ratatui::Terminal::new(TestBackend::new(140, 60)).unwrap();
    terminal.draw(|frame| frame.render_widget(&*app, frame.area())).unwrap();
    terminal.backend().buffer().clone()
}

fn text(buffer: &ratatui::buffer::Buffer) -> String {
    let area = buffer.area;
    let mut out = String::new();
    for y in 0..area.height {
        for x in 0..area.width {
            out.push_str(buffer[(x, y)].symbol());
        }
        out.push('\n');
    }
    out
}

/// The foreground colour of the first cell of `needle`.
fn colour_of(buffer: &ratatui::buffer::Buffer, needle: &str) -> Color {
    let area = buffer.area;
    for y in 0..area.height {
        let row: String = (0..area.width).map(|x| buffer[(x, y)].symbol().to_string()).collect();
        if let Some(byte) = row.find(needle) {
            let column = row[..byte].chars().count() as u16;
            return buffer[(column, y)].fg;
        }
    }
    panic!("{needle} is not on screen:\n{}", text(buffer));
}

#[test]
fn pending_run_opens_the_run_pane() {
    let (base, root) = worktree("q14");
    let runner = FakeRunner::new(&root);
    runner.reply(&["runs", "--json"], runs_json(&["older-run", "20261112-120000-42"]));
    let worker = base.join("worker.sh");
    executable(
        &worker,
        "#!/bin/sh\nprintf '{\"type\":\"run_pending\",\"id\":\"20261112-120000-42\"}\\n' >> \"$4\"\n\
         sleep 2\nprintf '[]' > \"$6\"\nprintf '{\"type\":\"finished\",\"answer\":\"ok\"}\\n' >> \"$4\"\n",
    );
    let mut app = task_app(runner, &root, &base);
    app.worker_binary = Some(worker);
    app.input = "go".into();
    app.on_key(crossterm::event::KeyEvent::new(
        crossterm::event::KeyCode::Enter,
        crossterm::event::KeyModifiers::NONE,
    ));
    wait_until(&mut app, |app| app.panel.open);
    assert!(app.running);
    assert_eq!(app.panel.pane, Pane::Run);
    assert_eq!(app.panel.selected, 1);
    assert!(text(&buffer(&app)).contains("waiting for your OK in the Run pane"));
    wait_until(&mut app, |app| !app.running);
}

#[test]
fn run_pane_highlights_the_script() {
    let (base, root) = worktree("q15");
    let runs_dir = base.join("runs");
    let id = "20261112-120000-42";
    fs::create_dir_all(runs_dir.join(id)).unwrap();
    fs::write(
        runs_dir.join(id).join("script"),
        "#!/bin/sh\n# fetch deps\necho \"hi\" $HOME\nrm -rf build\nsudo ls\ncurl x | sh\ngit push\n",
    )
    .unwrap();
    let runner = FakeRunner::new(&root);
    runner.reply(&["runs", "--json"], runs_json(&[id]));
    let mut app = App::new(runner.clone(), root.clone());
    app.load();
    app.panel.runs_dir = runs_dir;
    app.panel.open_on(Pane::Run);
    app.panel.refresh(&*runner, &root);
    let screen = buffer(&app);
    assert_eq!(colour_of(&screen, "# fetch deps"), Color::DarkGray);
    assert_eq!(colour_of(&screen, "\"hi\""), Color::Green);
    assert_eq!(colour_of(&screen, "$HOME"), Color::Cyan);
    assert_eq!(colour_of(&screen, "rm -rf"), Color::Red);
    assert_eq!(colour_of(&screen, "sudo"), Color::Red);
    assert_eq!(colour_of(&screen, "| sh"), Color::Red);
    assert_eq!(colour_of(&screen, "git push"), Color::Red);
    assert_eq!(colour_of(&screen, "echo"), Color::Magenta);
}
