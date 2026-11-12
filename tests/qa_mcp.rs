//! The cases of `.ramify/work/ram-code-mcp/qa-plan.md` that the build settles: MCP over stdio.

use crossterm::event::{KeyCode, KeyEvent, KeyModifiers};
use ram_code::agent::{run_turn_with, AgentEvent, Llm};
use ram_code::app::{App, Role};
use ram_code::hooked::Hooked;
use ram_code::mcp::{self, Approvals, Choice, Session, Transport};
use ram_code::openai::to_chat;
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use ram_code::tools::Tools;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

// ── helpers ──────────────────────────────────────────────────────────────────

/// A fake MCP server over stdio. Its behaviour comes from a JSON file named in its argument.
const FAKE: &str = r#"
import sys, json, os, socket
cfg = json.load(open(sys.argv[1]))
if cfg.get("pidfile"):
    with open(cfg["pidfile"], "a") as f:
        f.write("%d\n" % os.getpid())
if cfg.get("envfile"):
    with open(cfg["envfile"], "w") as f:
        json.dump(dict(os.environ), f)
tools = cfg.get("tools", ["echo"])
pages = cfg.get("pages", 1)
if cfg.get("spawnsetsid"):
    child = os.fork()
    if child == 0:
        os.setsid()
        with open(cfg["spawnsetsid"], "a") as f:
            f.write("%d\n" % os.getpid())
        import time
        time.sleep(60)
        os._exit(0)

def send(obj):
    sys.stdout.write(json.dumps(obj) + "\n")
    sys.stdout.flush()

def log(name):
    if cfg.get("calllog"):
        with open(cfg["calllog"], "a") as f:
            f.write(name + "\n")

def probe(args):
    out = []
    try:
        with open(args["outside"], "w") as f:
            f.write("x")
        out.append("write ok")
    except Exception as e:
        out.append("write denied")
    try:
        socket.create_connection(("127.0.0.1", int(args["port"])), 2).close()
        out.append("connect ok")
    except Exception as e:
        out.append("connect denied")
    return ", ".join(out)

for line in sys.stdin:
    line = line.strip()
    if not line:
        continue
    msg = json.loads(line)
    method = msg.get("method")
    ident = msg.get("id")
    if method == "initialize":
        send({"jsonrpc": "2.0", "id": ident, "result": {
            "protocolVersion": "2025-06-18",
            "capabilities": {"tools": {}},
            "serverInfo": {"name": "fake", "version": "1"}}})
    elif method == "tools/list":
        cursor = (msg.get("params") or {}).get("cursor")
        page = int(cursor) if cursor else 0
        size = (len(tools) + pages - 1) // pages
        chunk = tools[page * size:(page + 1) * size]
        result = {"tools": [{"name": n, "description": "the " + n + " tool",
            "inputSchema": {"type": "object", "properties": {"text": {"type": "string"}}}}
            for n in chunk]}
        if cfg.get("rawtools") is not None:
            result = {"tools": cfg["rawtools"]}
        if page + 1 < pages:
            result["nextCursor"] = str(page + 1)
        send({"jsonrpc": "2.0", "id": ident, "result": result})
    elif method == "tools/call":
        params = msg.get("params") or {}
        name = params.get("name")
        args = params.get("arguments") or {}
        log(name)
        if name == "hang":
            continue
        if name == "crash":
            os._exit(1)
        if name == "probe":
            result = {"content": [{"type": "text", "text": probe(args)}]}
        elif name == "mixed":
            result = {"isError": True, "content": [
                {"type": "text", "text": "partly broken"},
                {"type": "image", "mimeType": "image/png", "data": "aGVsbG8="},
                {"type": "resource_link", "uri": "file:///tmp/x.txt", "name": "x"}]}
        elif name == "ansi":
            result = {"content": [{"type": "text", "text": "\u001b[2J\u001b[31mred\u0007bell\rback\u009b1m"}]}
        elif name == "big":
            result = {"content": [{"type": "text", "text": "y" * 50000}]}
        elif name == "huge":
            result = {"content": [{"type": "text", "text": "z" * (9 * 1024 * 1024)}]}
        else:
            result = {"content": [{"type": "text", "text": "echo: " + str(args.get("text", ""))}]}
        send({"jsonrpc": "2.0", "id": ident, "result": result})
    elif method == "ping":
        send({"jsonrpc": "2.0", "id": ident, "result": {}})
    elif ident is not None:
        send({"jsonrpc": "2.0", "id": ident, "error": {"code": -32601, "message": "no such method"}})
if cfg.get("ignoreeof"):
    import time
    time.sleep(30)
"#;

struct Fixture {
    base: PathBuf,
    /// The worktree: a git repository, where the server may write.
    root: PathBuf,
    /// `RAM_CODE_CONFIG_DIR`: the user's `mcp.json` and `mcp-approvals.json`.
    config: PathBuf,
    script: PathBuf,
}

impl Fixture {
    fn new(name: &str) -> Self {
        let base = std::env::temp_dir().join(format!("ram-mcp-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&base);
        fs::create_dir_all(base.join("wt")).unwrap();
        fs::create_dir_all(base.join("config")).unwrap();
        let base = base.canonicalize().unwrap();
        let root = base.join("wt");
        let ok = Command::new("git")
            .args(["init", "-q"])
            .current_dir(&root)
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let script = base.join("fake.py");
        fs::write(&script, FAKE).unwrap();
        Fixture {
            config: base.join("config"),
            base,
            root,
            script,
        }
    }

    /// A server config file in the worktree, so the sandboxed server can read and write next to it.
    fn server(&self, name: &str, behaviour: Value) -> Value {
        let path = self.root.join(format!("{name}.json"));
        fs::write(&path, behaviour.to_string()).unwrap();
        json!({"command": "python3", "args": [self.script.display().to_string(), path.display().to_string()]})
    }

    fn user(&self, servers: Value) {
        fs::write(self.config.join("mcp.json"), json!({"mcpServers": servers}).to_string()).unwrap();
    }

    fn project(&self, servers: Value) {
        fs::write(self.root.join(".mcp.json"), json!({"mcpServers": servers}).to_string()).unwrap();
    }

    fn env(&self, extra: &[(&str, &str)]) -> Vec<(String, String)> {
        let mut env: Vec<(String, String)> = ["PATH", "HOME", "USER", "LANG"]
            .iter()
            .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
            .collect();
        env.push(("RAM_CODE_CONFIG_DIR".into(), self.config.display().to_string()));
        env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.to_string())));
        env
    }

    fn session(&self, extra: &[(&str, &str)]) -> Session {
        Session::start(&self.root, self.env(extra))
    }

    fn path(&self, name: &str) -> PathBuf {
        self.root.join(name)
    }

    fn approve(&self, name: &str, choice: Choice) {
        let config = mcp::load(&self.root, &getter(self.env(&[])));
        let entry = config.servers.iter().find(|s| s.name == name).unwrap();
        let mut approvals = Approvals::load(&self.config, &self.root);
        approvals.set(entry, choice).unwrap();
    }
}

fn getter(env: Vec<(String, String)>) -> impl Fn(&str) -> Option<String> {
    move |key| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
}

fn pids(path: &Path) -> Vec<i32> {
    fs::read_to_string(path)
        .unwrap_or_default()
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

fn alive(pid: i32) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .map(|s| s.success())
        .unwrap_or(false)
}

fn names(session: &Session) -> Vec<String> {
    use ram_code::agent::ToolSource;
    session
        .definitions()
        .iter()
        .filter_map(|d| d["name"].as_str().map(String::from))
        .collect()
}

fn call(session: &Session, name: &str, input: Value) -> Result<String, String> {
    use ram_code::agent::ToolSource;
    session.call(name, &input)
}

struct FakeLlm {
    responses: Mutex<Vec<Value>>,
    requests: Mutex<Vec<Value>>,
}

impl FakeLlm {
    fn new(responses: Vec<Value>) -> Self {
        FakeLlm {
            responses: Mutex::new(responses),
            requests: Mutex::new(Vec::new()),
        }
    }
}

impl Llm for FakeLlm {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        self.requests.lock().unwrap().push(request.clone());
        let mut responses = self.responses.lock().unwrap();
        if responses.is_empty() {
            Ok(json!({"content": [{"type": "text", "text": "done"}]}))
        } else {
            Ok(responses.remove(0))
        }
    }
}

struct FakeRunner {
    dash: String,
}

impl Runner for FakeRunner {
    fn run(&self, spec: &CommandSpec) -> Result<CommandOutput, String> {
        let stdout = if spec.args.first().map(String::as_str) == Some("dash-project") {
            self.dash.clone()
        } else {
            String::new()
        };
        Ok(CommandOutput {
            status: 0,
            stdout,
            stderr: String::new(),
        })
    }
}

/// The TUI on the fixture's worktree, with a worker that only says it finished.
fn app(fixture: &Fixture) -> App {
    let dash = json!({
        "name": "demo", "primary": fixture.root, "stacks": true, "jira": null,
        "containers": [], "shared": [], "build_servers": [], "strays": [], "orphans": [],
        "worktrees": [{"name": "demo", "path": fixture.root, "branch": "main", "ticket": null,
            "workflow": null, "primary": true, "dirty": false, "slot": 0, "uptime": null,
            "errors": [], "services": []}],
    })
    .to_string();
    let mut app = App::new(Arc::new(FakeRunner { dash }), fixture.root.clone());
    let env = fixture.env(&[("ANTHROPIC_AUTH_TOKEN", "t")]);
    app.env = Arc::new(move |key| env.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()));
    app.entries.clear();
    app.load_providers(fixture.config.clone());
    app.load();
    let worker = fixture.base.join("worker.sh");
    fs::write(
        &worker,
        "#!/bin/sh\nevents=\"$4\"\nhistory=\"$6\"\nprintf '[]' > \"$history\"\n\
         printf '{\"type\":\"finished\",\"answer\":\"ok\"}\\n' >> \"$events\"\n",
    )
    .unwrap();
    fs::set_permissions(&worker, fs::Permissions::from_mode(0o755)).unwrap();
    app.worker_binary = Some(worker);
    app
}

fn key(app: &mut App, code: KeyCode) {
    app.on_key(KeyEvent::new(code, KeyModifiers::NONE));
}

fn send(app: &mut App, text: &str) {
    app.input = text.to_string();
    key(app, KeyCode::Enter);
}

fn transcript(app: &App) -> String {
    app.entries
        .iter()
        .map(|e| e.text.clone())
        .collect::<Vec<_>>()
        .join("\n")
}

fn finish(app: &mut App) {
    for _ in 0..300 {
        app.poll_events();
        if !app.running {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the worker never finished");
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn stdio_server_tools_are_offered_and_called() {
    let fx = Fixture::new("q1");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["echo"]}))}));
    let mut session = fx.session(&[]);
    assert!(names(&session).contains(&"mcp__fake__echo".to_string()), "{:?}", session.notes());
    assert_eq!(call(&session, "mcp__fake__echo", json!({"text": "hi"})).unwrap(), "echo: hi");
    session.shutdown();
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn user_file_wins_over_project_file() {
    let fx = Fixture::new("q2");
    fx.user(json!({"fake": fx.server("user", json!({"tools": ["from_user"]}))}));
    fx.project(json!({"fake": fx.server("proj", json!({"tools": ["from_project"]}))}));
    let mut session = fx.session(&[]);
    let offered = names(&session);
    assert!(offered.contains(&"mcp__fake__from_user".to_string()), "{offered:?}");
    assert!(!offered.iter().any(|n| n.ends_with("from_project")), "{offered:?}");
    session.shutdown();
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn config_variables_expand() {
    let fx = Fixture::new("q3");
    fx.user(json!({
        "s": {"command": "${CMD}", "args": ["${ARG:-fallback}", "x${ARG2}y"],
              "env": {"TOKEN": "${TOK}"}},
        "h": {"type": "http", "url": "https://${HOST}/mcp", "headers": {"X-Team": "${TEAM:-core}"}}
    }));
    let env = fx.env(&[("CMD", "python3"), ("ARG2", "mid"), ("TOK", "t0k"), ("HOST", "mcp.example.com")]);
    let config = mcp::load(&fx.root, &getter(env));
    let s = config.servers.iter().find(|s| s.name == "s").unwrap();
    match &s.transport {
        Transport::Stdio { command, args, env, .. } => {
            assert_eq!(command, "python3");
            assert_eq!(args, &vec!["fallback".to_string(), "xmidy".to_string()]);
            assert_eq!(env, &vec![("TOKEN".to_string(), "t0k".to_string())]);
        }
        other => panic!("not stdio: {other:?}"),
    }
    let h = config.servers.iter().find(|s| s.name == "h").unwrap();
    match &h.transport {
        Transport::Http { url, headers } => {
            assert_eq!(url, "https://mcp.example.com/mcp");
            assert_eq!(headers, &vec![("X-Team".to_string(), "core".to_string())]);
        }
        other => panic!("not http: {other:?}"),
    }
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn unset_variable_fails_only_that_server() {
    let fx = Fixture::new("q4");
    fx.user(json!({
        "fake": fx.server("fake", json!({"tools": ["echo"]})),
        "broken": {"command": "python3", "args": ["${NOT_SET_ANYWHERE}"]}
    }));
    let mut session = fx.session(&[]);
    assert!(names(&session).contains(&"mcp__fake__echo".to_string()));
    let notes = session.notes().join("\n");
    assert!(notes.contains("broken") && notes.contains("NOT_SET_ANYWHERE"), "{notes}");
    session.shutdown();
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn project_server_waits_for_approval() {
    let fx = Fixture::new("q5");
    let pidfile = fx.path("pid");
    fx.project(json!({"fake": fx.server("fake", json!({"pidfile": pidfile}))}));
    let mut app = app(&fx);
    send(&mut app, "do it");
    assert!(!app.running, "the task started before the approval");
    let shown = transcript(&app);
    assert!(shown.contains("fake") && shown.contains("python3"), "{shown}");
    assert!(!pidfile.exists());
    key(&mut app, KeyCode::Char('a'));
    assert!(app.running, "the task did not start after the approval");
    finish(&mut app);
    assert!(!pidfile.exists(), "the TUI started the server itself");
    let approvals = Approvals::load(&fx.config, &fx.root);
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    assert_eq!(approvals.get(&config.servers[0]), Some(Choice::Sandbox));
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn denied_project_server_never_starts() {
    let fx = Fixture::new("q6");
    let pidfile = fx.path("pid");
    fx.project(json!({"fake": fx.server("fake", json!({"pidfile": pidfile}))}));
    let mut app = app(&fx);
    send(&mut app, "do it");
    key(&mut app, KeyCode::Char('d'));
    finish(&mut app);
    let mut session = fx.session(&[]);
    assert!(!names(&session).iter().any(|n| n.starts_with("mcp__")));
    session.shutdown();
    assert!(!pidfile.exists());
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[test]
fn changed_entry_asks_again() {
    let fx = Fixture::new("q7");
    fx.project(json!({"fake": fx.server("fake", json!({}))}));
    let mut app = app(&fx);
    send(&mut app, "first");
    key(&mut app, KeyCode::Char('a'));
    finish(&mut app);
    send(&mut app, "second");
    assert!(app.running, "an unchanged entry asked again");
    finish(&mut app);
    let mut entry = fx.server("fake", json!({}));
    entry["args"].as_array_mut().unwrap().push(json!("--changed"));
    fx.project(json!({"fake": entry}));
    app.entries.clear();
    send(&mut app, "third");
    assert!(!app.running, "a changed entry did not ask again");
    assert!(transcript(&app).contains("--changed"));
}

// ── Q8 ───────────────────────────────────────────────────────────────────────

#[test]
fn headless_skips_unapproved_servers() {
    let fx = Fixture::new("q8");
    let pidfile = fx.path("pid");
    fx.project(json!({"fake": fx.server("fake", json!({"pidfile": pidfile}))}));
    let mut session = fx.session(&[]);
    let notes = session.notes().join("\n");
    assert_eq!(notes.lines().filter(|l| l.contains("fake")).count(), 1, "{notes}");
    assert!(notes.contains("approv"), "{notes}");
    assert!(!names(&session).iter().any(|n| n.starts_with("mcp__")));
    session.shutdown();
    assert!(!pidfile.exists());
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[test]
fn approval_is_stored_by_hash() {
    let fx = Fixture::new("q9");
    let mut entry = fx.server("fake", json!({}));
    entry["env"] = json!({"SERVICE_X": "${MY_VALUE}"});
    fx.project(json!({"fake": entry}));
    let env = fx.env(&[("MY_VALUE", "supersecretvalue")]);
    let config = mcp::load(&fx.root, &getter(env));
    let mut approvals = Approvals::load(&fx.config, &fx.root);
    approvals.set(&config.servers[0], Choice::Network).unwrap();
    let text = fs::read_to_string(fx.config.join("mcp-approvals.json")).unwrap();
    assert!(text.contains(&fx.root.display().to_string()), "{text}");
    let hex = regex::Regex::new(r"[0-9a-f]{64}").unwrap();
    assert!(hex.is_match(&text), "{text}");
    assert!(!text.contains("supersecretvalue"), "{text}");
}

// ── Q10, Q11 ─────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
fn probe_with(choice: Choice, name: &str) -> String {
    let fx = Fixture::new(name);
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port().to_string();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            drop(stream);
        }
    });
    fx.project(json!({"fake": fx.server("fake", json!({"tools": ["probe"]}))}));
    fx.approve("fake", choice);
    let outside = fx.base.join("outside.txt");
    let mut session = fx.session(&[]);
    let out = call(
        &session,
        "mcp__fake__probe",
        json!({"outside": outside.display().to_string(), "port": port}),
    )
    .unwrap_or_else(|e| e);
    session.shutdown();
    assert!(!outside.exists(), "the server wrote outside the worktree");
    out
}

#[cfg(target_os = "macos")]
#[test]
fn approved_server_is_sandboxed() {
    assert_eq!(probe_with(Choice::Sandbox, "q10"), "write denied, connect denied");
}

#[cfg(target_os = "macos")]
#[test]
fn network_approval_keeps_other_rules() {
    assert_eq!(probe_with(Choice::Network, "q11"), "write denied, connect ok");
}

// ── Q12 ──────────────────────────────────────────────────────────────────────

#[test]
fn npx_command_suggests_network() {
    let fx = Fixture::new("q12");
    fx.project(json!({"tracker": {"command": "npx", "args": ["-y", "tracker-mcp"]}}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let prompt = mcp::prompt_text(&config.servers[0]);
    assert!(prompt.contains("npx -y tracker-mcp"), "{prompt}");
    assert!(prompt.contains("`n`") && prompt.contains("network"), "{prompt}");
}

// ── Q13 ──────────────────────────────────────────────────────────────────────

#[test]
fn server_env_is_scrubbed() {
    let fx = Fixture::new("q13");
    let envfile = fx.path("env.json");
    let mut entry = fx.server("fake", json!({"envfile": envfile}));
    entry["env"] = json!({"TRACKER_TOKEN": "${TRACKER_TOKEN}"});
    fx.user(json!({"fake": entry}));
    let mut session = fx.session(&[("OPENAI_API_KEY", "sk-leak"), ("TRACKER_TOKEN", "trk")]);
    session.shutdown();
    let seen: Value = serde_json::from_str(&fs::read_to_string(&envfile).unwrap()).unwrap();
    assert_eq!(seen["TRACKER_TOKEN"], "trk");
    assert!(seen.get("OPENAI_API_KEY").is_none(), "{seen}");
}

// ── Q14 ──────────────────────────────────────────────────────────────────────

#[test]
fn tools_list_follows_pages() {
    let fx = Fixture::new("q14");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["a", "b", "c", "d"], "pages": 2}))}));
    let mut session = fx.session(&[]);
    let offered = names(&session);
    for tool in ["a", "b", "c", "d"] {
        assert!(offered.contains(&format!("mcp__fake__{tool}")), "{offered:?}");
    }
    session.shutdown();
}

// ── Q15 ──────────────────────────────────────────────────────────────────────

#[test]
fn tool_names_are_valid_and_unique() {
    let fx = Fixture::new("q15");
    let long_a = format!("{}a", "x".repeat(70));
    let long_b = format!("{}b", "x".repeat(70));
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["has space", long_a, long_b]}))}));
    let mut session = fx.session(&[]);
    let offered: Vec<String> = names(&session)
        .into_iter()
        .filter(|n| n.starts_with("mcp__"))
        .collect();
    assert_eq!(offered.len(), 3, "{offered:?}");
    let valid = regex::Regex::new(r"^[A-Za-z0-9_-]{1,64}$").unwrap();
    for name in &offered {
        assert!(valid.is_match(name), "{name}");
    }
    let mut unique = offered.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 3, "{offered:?}");
    let spaced = offered.iter().find(|n| n.contains("has_space")).unwrap();
    assert_eq!(call(&session, spaced, json!({"text": "z"})).unwrap(), "echo: z");
    session.shutdown();
}

// ── Q16 ──────────────────────────────────────────────────────────────────────

#[test]
fn results_are_converted() {
    let fx = Fixture::new("q16");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["mixed"]}))}));
    let mut session = fx.session(&[]);
    let error = call(&session, "mcp__fake__mixed", json!({})).unwrap_err();
    assert!(error.contains("partly broken"), "{error}");
    assert!(error.contains("[image: image/png, 5 bytes]"), "{error}");
    assert!(error.contains("[resource: file:///tmp/x.txt]"), "{error}");
    session.shutdown();
}

// ── Q17 ──────────────────────────────────────────────────────────────────────

#[test]
fn large_result_is_cut() {
    let fx = Fixture::new("q17");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["big"]}))}));
    let mut session = fx.session(&[]);
    let out = call(&session, "mcp__fake__big", json!({})).unwrap();
    assert!(out.starts_with(&"y".repeat(40_000)), "too short");
    assert!(out.chars().filter(|c| *c == 'y').count() == 40_000, "not cut at 40,000");
    assert!(out.contains("cut"), "no note");
    session.shutdown();
}

// ── Q18 ──────────────────────────────────────────────────────────────────────

#[test]
fn hung_call_times_out() {
    let fx = Fixture::new("q18");
    let mut entry = fx.server("fake", json!({"tools": ["hang", "echo"]}));
    entry["timeout_seconds"] = json!(1);
    fx.user(json!({"fake": entry}));
    let mut session = fx.session(&[]);
    let started = Instant::now();
    let error = call(&session, "mcp__fake__hang", json!({})).unwrap_err();
    assert!(started.elapsed() < Duration::from_secs(3), "{:?}", started.elapsed());
    assert!(error.contains("timed out"), "{error}");
    assert_eq!(call(&session, "mcp__fake__echo", json!({"text": "after"})).unwrap(), "echo: after");
    session.shutdown();
}

// ── Q19 ──────────────────────────────────────────────────────────────────────

#[test]
fn crashed_server_restarts_once() {
    let fx = Fixture::new("q19");
    let pidfile = fx.path("pid");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["crash", "echo"], "pidfile": pidfile}))}));
    let mut session = fx.session(&[]);
    assert!(call(&session, "mcp__fake__crash", json!({})).is_err());
    assert_eq!(call(&session, "mcp__fake__echo", json!({"text": "back"})).unwrap(), "echo: back");
    assert_eq!(pids(&pidfile).len(), 2, "the server did not restart");
    assert!(call(&session, "mcp__fake__crash", json!({})).is_err());
    let error = call(&session, "mcp__fake__echo", json!({"text": "x"})).unwrap_err();
    assert!(error.contains("failed"), "{error}");
    assert_eq!(pids(&pidfile).len(), 2, "the server restarted a second time");
    session.shutdown();
}

// ── Q20 ──────────────────────────────────────────────────────────────────────

#[test]
fn no_server_outlives_the_task() {
    let fx = Fixture::new("q20");
    let pidfile = fx.path("pid");
    fx.user(json!({
        "one": fx.server("one", json!({"pidfile": pidfile})),
        "two": fx.server("two", json!({"pidfile": pidfile, "tools": ["hang"]}))
    }));
    let mut session = fx.session(&[]);
    let started = pids(&pidfile);
    assert_eq!(started.len(), 2, "{:?}", session.notes());
    session.shutdown();
    for pid in started {
        assert!(!alive(pid), "server {pid} still runs");
    }
}

// ── Q21 ──────────────────────────────────────────────────────────────────────

#[test]
fn pretooluse_hook_can_deny_an_mcp_call() {
    let _lock = HOOK_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("q21");
    let calllog = fx.path("calls");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["delete"], "calllog": calllog}))}));
    let home = fx.base.join("home");
    fs::create_dir_all(home.join(".ram")).unwrap();
    let deny = fx.base.join("deny.sh");
    fs::write(
        &deny,
        "#!/bin/sh\ncat >/dev/null\necho '{\"hookSpecificOutput\":{\"hookEventName\":\"PreToolUse\",\"permissionDecision\":\"deny\",\"permissionDecisionReason\":\"no deletes\"}}'\n",
    )
    .unwrap();
    fs::set_permissions(&deny, fs::Permissions::from_mode(0o755)).unwrap();
    fs::write(
        home.join(".ram/settings.json"),
        json!({"hooks": {"PreToolUse": [{"matcher": "mcp__fake__delete",
            "hooks": [{"type": "command", "command": deny.display().to_string()}]}]}})
        .to_string(),
    )
    .unwrap();
    std::env::set_var("RAMIFY_HOME", fx.base.join("no-ramify"));
    std::env::set_var("RAMIFY_STATE_DIR", fx.base.join("state"));
    std::env::set_var("RAM_CODE_INSTRUCTIONS_HOME", &home);
    let hooks = Hooked::load_from(&fx.root, "ram-test", &home);
    let mut session = fx.session(&[]);
    let llm = FakeLlm::new(vec![json!({"content": [{"type": "tool_use", "id": "t1",
        "name": "mcp__fake__delete", "input": {}}]})]);
    let mut results = Vec::new();
    run_turn_with(&llm, &fx.root, &[], "go", &hooks, &session, &mut |event| {
        if let AgentEvent::ToolResult(text) = event {
            results.push(text);
        }
    })
    .unwrap();
    session.shutdown();
    assert!(results[0].contains("no deletes"), "{results:?}");
    assert!(!calllog.exists(), "the call reached the server");
}

// ── Q22 ──────────────────────────────────────────────────────────────────────

#[test]
fn no_config_changes_nothing() {
    let fx = Fixture::new("q22");
    let mut session = fx.session(&[]);
    let llm = FakeLlm::new(vec![]);
    run_turn_with(&llm, &fx.root, &[], "go", &Hooked::none(&fx.root), &session, &mut |_| {}).unwrap();
    session.shutdown();
    let request = llm.requests.lock().unwrap()[0].clone();
    assert_eq!(request["tools"], json!(Tools::definitions()));
    let mut app = app(&fx);
    send(&mut app, "/mcp");
    let shown = transcript(&app);
    assert!(shown.contains("no MCP server"), "{shown}");
    assert!(shown.contains("mcp.json"), "{shown}");
}

// ── Q23 ──────────────────────────────────────────────────────────────────────

#[test]
fn both_formats_get_mcp_tools() {
    let fx = Fixture::new("q23");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["echo"]}))}));
    let mut session = fx.session(&[]);
    let llm = FakeLlm::new(vec![]);
    run_turn_with(&llm, &fx.root, &[], "go", &Hooked::none(&fx.root), &session, &mut |_| {}).unwrap();
    session.shutdown();
    let request = llm.requests.lock().unwrap()[0].clone();
    let tool = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["name"] == "mcp__fake__echo")
        .cloned()
        .expect("Anthropic tools");
    assert_eq!(tool["input_schema"]["properties"]["text"]["type"], "string");
    let chat = to_chat(&request);
    let function = chat["tools"]
        .as_array()
        .unwrap()
        .iter()
        .find(|t| t["function"]["name"] == "mcp__fake__echo")
        .cloned()
        .expect("Chat Completions tools");
    assert_eq!(function["function"]["parameters"]["properties"]["text"]["type"], "string");
}

// ── Q24 ──────────────────────────────────────────────────────────────────────

#[test]
fn http_entry_is_not_supported_yet() {
    let fx = Fixture::new("q24");
    fx.user(json!({"docs": {"type": "http", "url": "https://mcp.example.com/mcp"}}));
    let mut session = fx.session(&[]);
    assert!(!names(&session).iter().any(|n| n.starts_with("mcp__docs")));
    session.shutdown();
    let mut app = app(&fx);
    send(&mut app, "/mcp");
    let shown = transcript(&app);
    assert!(shown.contains("docs") && shown.contains("not supported yet"), "{shown}");
    let _ = Role::Info;
}

// ── audit ────────────────────────────────────────────────────────────────────

/// Q21 and the hook test below set process-wide variables; they must not run at once.
static HOOK_ENV: Mutex<()> = Mutex::new(());

#[test]
fn project_entry_cannot_read_secret_variables() {
    let fx = Fixture::new("a1");
    let mut entry = fx.server("fake", json!({}));
    entry["env"] = json!({"LEAK": "${ANTHROPIC_API_KEY}"});
    let mut by_arg = fx.server("fake2", json!({}));
    by_arg["args"].as_array_mut().unwrap().push(json!("${MY_SERVICE_TOKEN}"));
    fx.project(json!({"fake": entry, "fake2": by_arg}));
    let env = fx.env(&[("ANTHROPIC_API_KEY", "sk-ant-secret-1"), ("MY_SERVICE_TOKEN", "tok-secret-2")]);
    let config = mcp::load(&fx.root, &getter(env.clone()));
    for server in &config.servers {
        let error = server.error.clone().unwrap_or_default();
        assert!(error.contains("secret"), "{} was not refused: {error:?}", server.name);
        let prompt = mcp::prompt_text(server);
        assert!(!prompt.contains("secret-1") && !prompt.contains("secret-2"), "{prompt}");
    }
    fx.approve("fake", Choice::Network);
    let mut session = Session::start(&fx.root, env);
    assert!(!names(&session).iter().any(|n| n.starts_with("mcp__")));
    session.shutdown();
}

#[test]
fn user_entry_may_read_its_own_secrets() {
    let fx = Fixture::new("a2");
    let envfile = fx.path("env.json");
    let mut entry = fx.server("fake", json!({"envfile": envfile}));
    entry["env"] = json!({"SERVICE_TOKEN": "${MY_SERVICE_TOKEN}"});
    fx.user(json!({"fake": entry}));
    let mut session = fx.session(&[("MY_SERVICE_TOKEN", "tok-user")]);
    session.shutdown();
    let seen: Value = serde_json::from_str(&fs::read_to_string(&envfile).unwrap()).unwrap();
    assert_eq!(seen["SERVICE_TOKEN"], "tok-user");
}

#[test]
fn project_approval_binds_expanded_values() {
    let fx = Fixture::new("a3");
    let mut entry = fx.server("fake", json!({}));
    entry["args"].as_array_mut().unwrap().push(json!("${TARGET}"));
    fx.project(json!({"fake": entry}));
    let first = mcp::load(&fx.root, &getter(fx.env(&[("TARGET", "one")])));
    let mut approvals = Approvals::load(&fx.config, &fx.root);
    approvals.set(&first.servers[0], Choice::Sandbox).unwrap();
    let same = mcp::load(&fx.root, &getter(fx.env(&[("TARGET", "one")])));
    assert_eq!(approvals.get(&same.servers[0]), Some(Choice::Sandbox));
    let changed = mcp::load(&fx.root, &getter(fx.env(&[("TARGET", "two")])));
    assert_eq!(approvals.get(&changed.servers[0]), None, "a new expansion kept the approval");
}

#[test]
fn prompt_shows_env_as_written() {
    let fx = Fixture::new("a4");
    let mut entry = fx.server("fake", json!({}));
    entry["env"] = json!({"REGION": "${MY_REGION:-eu}"});
    fx.project(json!({"fake": entry}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let prompt = mcp::prompt_text(&config.servers[0]);
    assert!(prompt.contains("REGION=${MY_REGION:-eu}"), "{prompt}");
}

#[test]
fn mcp_results_escape_control_characters() {
    let fx = Fixture::new("a5");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["ansi"]}))}));
    let mut session = fx.session(&[]);
    let out = call(&session, "mcp__fake__ansi", json!({})).unwrap();
    session.shutdown();
    assert!(
        !out.chars().any(|c| c.is_control() && c != '\n' && c != '\t'),
        "a control character reached the model and the transcript: {out:?}"
    );
    assert!(out.contains("red") && out.contains("bell") && out.contains("back"), "{out:?}");
}

#[test]
fn hooks_see_mcp_tool_name_and_input() {
    let _lock = HOOK_ENV.lock().unwrap_or_else(|e| e.into_inner());
    let fx = Fixture::new("a6");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["echo"]}))}));
    let home = fx.base.join("home");
    fs::create_dir_all(home.join(".ram")).unwrap();
    let pre = fx.base.join("pre.json");
    let post = fx.base.join("post.json");
    let record = |out: &Path, name: &str| {
        let script = fx.base.join(name);
        fs::write(&script, format!("#!/bin/sh\ncat > '{}'\n", out.display())).unwrap();
        fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
        script.display().to_string()
    };
    let hooks_json = json!({"hooks": {
        "PreToolUse": [{"matcher": "mcp__fake__echo", "hooks": [{"type": "command", "command": record(&pre, "pre.sh")}]}],
        "PostToolUse": [{"matcher": "mcp__fake__echo", "hooks": [{"type": "command", "command": record(&post, "post.sh")}]}]
    }});
    fs::write(home.join(".ram/settings.json"), hooks_json.to_string()).unwrap();
    std::env::set_var("RAMIFY_HOME", fx.base.join("no-ramify"));
    std::env::set_var("RAMIFY_STATE_DIR", fx.base.join("state"));
    std::env::set_var("RAM_CODE_INSTRUCTIONS_HOME", &home);
    let hooks = Hooked::load_from(&fx.root, "ram-test", &home);
    let mut session = fx.session(&[]);
    let llm = FakeLlm::new(vec![json!({"content": [{"type": "tool_use", "id": "t1",
        "name": "mcp__fake__echo", "input": {"text": "hello-hook"}}]})]);
    run_turn_with(&llm, &fx.root, &[], "go", &hooks, &session, &mut |_| {}).unwrap();
    session.shutdown();
    let pre: Value = serde_json::from_str(&fs::read_to_string(&pre).expect("no PreToolUse input")).unwrap();
    assert_eq!(pre["tool_name"], "mcp__fake__echo");
    assert_eq!(pre["tool_input"]["text"], "hello-hook");
    let post: Value = serde_json::from_str(&fs::read_to_string(&post).expect("no PostToolUse input")).unwrap();
    assert_eq!(post["tool_name"], "mcp__fake__echo");
    assert!(post.to_string().contains("echo: hello-hook"), "{post}");
}

/// The Keychain daemon stays out of reach in both sandbox variants. A throwaway keychain in the
/// worktree holds one dummy item; the user's keychains and search list are not touched.
#[cfg(target_os = "macos")]
#[test]
fn keychain_is_out_of_reach_for_servers() {
    use ram_code::sandbox::Sandbox;
    let fx = Fixture::new("a7");
    let keychain = fx.root.join("probe.keychain-db");
    let path = keychain.to_str().unwrap().to_string();
    let security = |args: &[&str]| {
        assert!(Command::new("/usr/bin/security").args(args).status().unwrap().success(), "{args:?}");
    };
    security(&["create-keychain", "-p", "probe-pw", &path]);
    security(&["unlock-keychain", "-p", "probe-pw", &path]);
    security(&["add-generic-password", "-a", "probe", "-s", "ram-mcp-probe", "-w", "dummy-value-456", &path]);
    let temp = fx.base.join("tmp");
    fs::create_dir_all(&temp).unwrap();
    let mut outputs = Vec::new();
    for network in [false, true] {
        let mut cmd = Sandbox::detect()
            .command_with_network(
                &fx.root,
                &temp,
                None,
                &format!("/usr/bin/security find-generic-password -s ram-mcp-probe -w '{path}'"),
                network,
            )
            .expect("a sandbox");
        let out = cmd.current_dir(&fx.root).output().unwrap();
        outputs.push((network, String::from_utf8_lossy(&out.stdout).to_string()));
    }
    let _ = Command::new("/usr/bin/security").args(["delete-keychain", &path]).status();
    for (network, out) in outputs {
        assert!(!out.contains("dummy-value-456"), "network={network}: the sandbox read a keychain item");
    }
}

// ── review fixes ─────────────────────────────────────────────────────────────

/// A copy of the fake server inside the worktree, run by a relative path.
fn repo_server(fx: &Fixture, behaviour: Value) -> Value {
    fs::create_dir_all(fx.root.join("tools")).unwrap();
    fs::write(fx.root.join("tools/mcp.py"), FAKE).unwrap();
    fs::write(fx.root.join("tools/cfg.json"), behaviour.to_string()).unwrap();
    json!({"command": "python3", "args": ["tools/mcp.py", "tools/cfg.json"]})
}

#[test]
fn project_server_cannot_run_outside_the_sandbox() {
    let fx = Fixture::new("r1a");
    fx.project(json!({"fake": fx.server("fake", json!({}))}));
    let mut app = app(&fx);
    send(&mut app, "do it");
    let shown = transcript(&app);
    assert!(!shown.contains("u: approve"), "the prompt offers u for a project server:\n{shown}");
    key(&mut app, KeyCode::Char('u'));
    assert!(!app.running, "u started the task");
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let approvals = Approvals::load(&fx.config, &fx.root);
    assert_eq!(approvals.get(&config.servers[0]), None, "u was stored for a project server");
    assert!(transcript(&app).contains("mcp.json"), "{}", transcript(&app));
    app.entries.clear();
    key(&mut app, KeyCode::Esc);
    send(&mut app, "/mcp approve fake u");
    let shown = transcript(&app);
    assert!(shown.contains("~/.config/ram-code/mcp.json"), "{shown}");
    let approvals = Approvals::load(&fx.config, &fx.root);
    assert_eq!(approvals.get(&config.servers[0]), None);
    let mut approvals = Approvals::load(&fx.config, &fx.root);
    assert!(approvals.set(&config.servers[0], Choice::Outside).is_err());
    // A user server keeps `u`.
    let fx = Fixture::new("r1b");
    fx.user(json!({"mine": fx.server("mine", json!({}))}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let mut approvals = Approvals::load(&fx.config, &fx.root);
    assert!(approvals.set(&config.servers[0], Choice::Outside).is_ok());
}

#[test]
fn approval_covers_repo_files_the_server_runs() {
    let fx = Fixture::new("r1c");
    fx.project(json!({"fake": repo_server(&fx, json!({}))}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let prompt = mcp::prompt_text(&config.servers[0]);
    assert!(
        prompt.contains("runs a file from this repository: tools/mcp.py"),
        "{prompt}"
    );
    fx.approve("fake", Choice::Sandbox);
    let mut session = fx.session(&[]);
    assert!(names(&session).contains(&"mcp__fake__echo".to_string()), "{:?}", session.notes());
    session.shutdown();
    let mut script = fs::read_to_string(fx.root.join("tools/mcp.py")).unwrap();
    script.push_str("\n# changed by the model\n");
    fs::write(fx.root.join("tools/mcp.py"), script).unwrap();
    let mut session = fx.session(&[]);
    assert!(
        !names(&session).iter().any(|n| n.starts_with("mcp__")),
        "a changed repo file ran under the old approval"
    );
    assert!(session.notes().join("\n").contains("approval"), "{:?}", session.notes());
    session.shutdown();
}

#[test]
fn approval_records_what_the_human_saw() {
    let fx = Fixture::new("r2");
    fx.project(json!({"fake": fx.server("fake", json!({}))}));
    let mut app = app(&fx);
    send(&mut app, "do it");
    // The file changes while the human reads the prompt.
    let mut entry = fx.server("fake", json!({}));
    entry["args"].as_array_mut().unwrap().push(json!("--swapped"));
    fx.project(json!({"fake": entry}));
    app.entries.clear();
    key(&mut app, KeyCode::Char('a'));
    assert!(!app.running, "the task started with an entry the human never saw");
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let approvals = Approvals::load(&fx.config, &fx.root);
    assert_eq!(approvals.get(&config.servers[0]), None, "the unseen entry was approved");
    assert!(transcript(&app).contains("--swapped"), "{}", transcript(&app));
}

#[test]
fn prompt_shows_the_resolved_command_and_flags_risky_env() {
    let fx = Fixture::new("r3");
    fx.project(json!({"fake": fx.server("fake", json!({}))}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let prompt = mcp::prompt_text(&config.servers[0]);
    let python = Command::new("sh").args(["-c", "command -v python3"]).output().unwrap();
    let python = String::from_utf8_lossy(&python.stdout).trim().to_string();
    assert!(python.starts_with('/'));
    assert!(prompt.contains(&format!("runs: {python}")), "{prompt}");
    for key in [
        "PATH",
        "NODE_OPTIONS",
        "NODE_PATH",
        "PYTHONPATH",
        "PYTHONSTARTUP",
        "RUBYOPT",
        "PERL5OPT",
        "MY_LIBPATH",
        "DYLD_INSERT_LIBRARIES",
        "LD_PRELOAD",
        "npm_config_registry",
        "BASH_ENV",
        "ENV",
    ] {
        let mut entry = fx.server("fake", json!({}));
        entry["env"] = json!({key: "x"});
        fx.project(json!({"fake": entry}));
        let config = mcp::load(&fx.root, &getter(fx.env(&[])));
        let error = config.servers[0].error.clone().unwrap_or_default();
        assert!(error.contains(key) && error.contains("mcp.json"), "{key}: {error:?}");
    }
    // The user's own file may set them.
    let mut entry = fx.server("fake", json!({}));
    entry["env"] = json!({"NODE_OPTIONS": "x"});
    fx.user(json!({"mine": entry}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let mine = config.servers.iter().find(|s| s.name == "mine").unwrap();
    assert!(mine.error.is_none(), "{:?}", mine.error);
}

#[test]
fn bad_tool_schemas_are_dropped_and_lists_capped() {
    let fx = Fixture::new("r4");
    let schema = json!({"type": "object", "properties": {"text": {"type": "string"}}});
    let mut tools: Vec<Value> = (0..130)
        .map(|i| json!({"name": format!("t{i}"), "description": "d", "inputSchema": schema}))
        .collect();
    tools.insert(0, json!({"name": "foo_type", "description": "d", "inputSchema": {"type": "foo"}}));
    tools.insert(0, json!({"name": "no_type", "description": "d", "inputSchema": {"properties": {}}}));
    let long = format!("{}\u{202e}{}", "w".repeat(3000), "\u{1b}[31m");
    tools.insert(0, json!({"name": "long", "description": long, "inputSchema": schema}));
    fx.user(json!({"fake": fx.server("fake", json!({"rawtools": tools}))}));
    let mut session = fx.session(&[]);
    use ram_code::agent::ToolSource;
    let definitions = session.definitions();
    let offered: Vec<&Value> = definitions
        .iter()
        .filter(|d| d["name"].as_str().unwrap_or("").starts_with("mcp__"))
        .collect();
    assert_eq!(offered.len(), 128, "{:?}", session.notes());
    for definition in &offered {
        assert_eq!(definition["input_schema"]["type"], "object", "{definition}");
        let description = definition["description"].as_str().unwrap();
        assert!(description.chars().count() <= 2_000, "{}", description.len());
        assert!(!description.contains('\u{202e}') && !description.contains('\u{1b}'));
    }
    let names: Vec<&str> = offered.iter().filter_map(|d| d["name"].as_str()).collect();
    assert!(!names.contains(&"mcp__fake__foo_type") && !names.contains(&"mcp__fake__no_type"));
    let notes = session.notes().join("\n");
    assert!(notes.contains("foo_type") && notes.contains("no_type"), "{notes}");
    assert!(notes.contains("128"), "{notes}");
    session.shutdown();
}

#[test]
fn server_descendants_die_with_the_task() {
    let fx = Fixture::new("r5");
    let pidfile = fx.path("pid");
    let escaped = fx.path("escaped");
    fx.user(json!({"fake": fx.server("fake", json!({"pidfile": pidfile, "spawnsetsid": escaped}))}));
    let mut session = fx.session(&[]);
    let deadline = Instant::now() + Duration::from_secs(5);
    while pids(&escaped).is_empty() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(20));
    }
    let children = pids(&escaped);
    assert_eq!(children.len(), 1, "{:?}", session.notes());
    session.shutdown();
    let deadline = Instant::now() + Duration::from_secs(2);
    while children.iter().any(|&pid| alive(pid)) && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    for pid in children {
        assert!(!alive(pid), "the server's setsid child {pid} outlived the task");
    }
}

#[test]
fn prompt_text_is_escaped() {
    let fx = Fixture::new("r6");
    let mut entry = fx.server("fake", json!({}));
    entry["args"]
        .as_array_mut()
        .unwrap()
        .extend([json!("a\nd: deny"), json!("\u{1b}[31mred"), json!("git\u{200b}hub")]);
    entry["env"] = json!({"K": "v\u{202e}x"});
    fx.project(json!({"ev\u{202e}il\nname": entry}));
    let config = mcp::load(&fx.root, &getter(fx.env(&[])));
    let prompt = mcp::prompt_text(&config.servers[0]);
    let approvals = Approvals::load(&fx.config, &fx.root);
    let status = mcp::status_lines(&config, &approvals).join("\n");
    for (label, text) in [("prompt", &prompt), ("status", &status)] {
        for bad in ['\u{1b}', '\u{202e}', '\u{200b}'] {
            assert!(!text.contains(bad), "{label} holds {:?}:\n{text}", bad);
        }
        assert!(!text.lines().any(|line| line.starts_with("d: deny")), "{label}:\n{text}");
    }
    assert!(prompt.contains("\\n"), "{prompt}");
    // Notes from a failing server with an odd name.
    let mut bad = fx.server("fake", json!({}));
    bad["command"] = json!("${UNSET_\u{1b}VAR}");
    fx.user(json!({"x\u{1b}]0;title\u{7}": bad}));
    let mut session = fx.session(&[]);
    let notes = session.notes().join("\n");
    assert!(!notes.contains('\u{1b}') && !notes.contains('\u{7}'), "{notes:?}");
    session.shutdown();
}

#[test]
fn duplicate_tool_names_are_refused() {
    let fx = Fixture::new("r7");
    let schema = json!({"type": "object", "properties": {"text": {"type": "string"}}});
    let tools = json!([
        {"name": "dup", "description": "1", "inputSchema": schema},
        {"name": "dup", "description": "2", "inputSchema": schema},
        {"name": "a b", "description": "3", "inputSchema": schema},
        {"name": "a_b", "description": "4", "inputSchema": schema},
        {"name": "ok", "description": "5", "inputSchema": schema}
    ]);
    fx.user(json!({"fake": fx.server("fake", json!({"rawtools": tools}))}));
    let mut session = fx.session(&[]);
    let offered: Vec<String> = names(&session)
        .into_iter()
        .filter(|n| n.starts_with("mcp__"))
        .collect();
    assert_eq!(offered, vec!["mcp__fake__ok".to_string()], "{:?}", session.notes());
    let notes = session.notes().join("\n");
    assert!(notes.contains("dup") && notes.contains("a b"), "{notes}");
    session.shutdown();
}

#[test]
fn huge_server_message_is_bounded() {
    let fx = Fixture::new("r8a");
    fx.user(json!({"fake": fx.server("fake", json!({"tools": ["huge", "echo"]}))}));
    let mut session = fx.session(&[]);
    let out = call(&session, "mcp__fake__huge", json!({}));
    assert!(out.is_err(), "a 9 MiB message was read whole: {:?}", out.map(|t| t.len()));
    session.shutdown();
}

#[test]
fn shutdown_stops_servers_in_parallel() {
    let fx = Fixture::new("r8b");
    fx.user(json!({
        "one": fx.server("one", json!({"ignoreeof": true})),
        "two": fx.server("two", json!({"ignoreeof": true}))
    }));
    let mut session = fx.session(&[]);
    assert_eq!(names(&session).iter().filter(|n| n.starts_with("mcp__")).count(), 2);
    let started = Instant::now();
    session.shutdown();
    let took = started.elapsed();
    assert!(took < Duration::from_secs(3), "shutdown took {took:?}");
}

// ── merge with streaming ─────────────────────────────────────────────────────

#[test]
fn esc_during_an_mcp_call_interrupts_promptly() {
    use ram_code::agent::{run_turn_cancellable_with, TurnEnd, INTERRUPTED};
    use ram_code::cancel::Cancel;
    let fx = Fixture::new("esc");
    let pidfile = fx.path("pid");
    let mut entry = fx.server("fake", json!({"tools": ["hang"], "pidfile": pidfile}));
    entry["timeout_seconds"] = json!(30);
    fx.user(json!({"fake": entry}));
    let mut session = fx.session(&[]);
    let started = pids(&pidfile);
    assert_eq!(started.len(), 1, "{:?}", session.notes());
    let llm = FakeLlm::new(vec![json!({
        "content": [{"type": "tool_use", "id": "t1", "name": "mcp__fake__hang", "input": {}}],
        "stop_reason": "tool_use"
    })]);
    let cancel = Cancel::new();
    let stopper = cancel.clone();
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        stopper.cancel();
    });
    let begun = Instant::now();
    let turn = run_turn_cancellable_with(
        &llm,
        &fx.root,
        &[],
        "go",
        &Hooked::none(&fx.root),
        &session,
        &cancel,
        &mut |_| {},
    );
    assert!(begun.elapsed() < Duration::from_secs(2), "took {:?}", begun.elapsed());
    assert_eq!(turn.end, TurnEnd::Interrupted);
    let results: Vec<&Value> = turn
        .history
        .iter()
        .filter_map(|m| m["content"].as_array())
        .flatten()
        .filter(|b| b["type"] == "tool_result")
        .collect();
    assert_eq!(results.len(), 1, "{:?}", turn.history);
    assert_eq!(results[0]["content"], INTERRUPTED);
    std::thread::sleep(Duration::from_millis(300));
    assert!(!alive(started[0]), "the server still runs after the interrupt");
    session.shutdown();
}
