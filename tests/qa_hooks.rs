//! Claude Code's hook mechanics in Ram Code: the sources, the events, the decisions, and the
//! workflow events.

use ram_code::agent::{run_turn_hooked, AgentEvent, Llm};
use ram_code::hooked::Hooked;
use ram_code::hooks::{trust, HookSet, Sources};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::{Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// RAMIFY_HOME, RAMIFY_STATE_DIR and PATH are process-wide: one test at a time. HOME stays the
/// user's: hook scripts need it (asdf shims find their versions there). The hooks' own home is
/// the fixture's, through `Hooked::load_from`.
static ENV: Mutex<()> = Mutex::new(());

struct Fixture {
    _guard: MutexGuard<'static, ()>,
    base: PathBuf,
    home: PathBuf,
    project: PathBuf,
    log: PathBuf,
}

fn git(args: &[&str], cwd: &Path) {
    let ok = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap()
        .status
        .success();
    assert!(ok, "git {args:?}");
}

impl Fixture {
    fn new(name: &str) -> Self {
        let guard = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
        let base = std::env::temp_dir().join(format!("ram-hooks-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let home = base.join("home");
        let project = base.join("project");
        std::fs::create_dir_all(&home).unwrap();
        std::fs::create_dir_all(&project).unwrap();
        git(&["init", "-q", "-b", "main"], &project);
        let base = base.canonicalize().unwrap();
        let (home, project) = (base.join("home"), base.join("project"));
        std::env::set_var("RAMIFY_HOME", base.join("no-ramify"));
        std::env::set_var("RAMIFY_STATE_DIR", base.join("state"));
        let log = base.join("log");
        std::fs::create_dir_all(&log).unwrap();
        Fixture {
            _guard: guard,
            base,
            home,
            project,
            log,
        }
    }

    /// A hook script: it logs its stdin and `RAM_CODE` to `log/<name>`, then runs `body`.
    fn script(&self, name: &str, body: &str) -> String {
        let path = self.base.join(format!("{name}.sh"));
        let log = self.log.join(name);
        std::fs::write(
            &path,
            format!(
                "#!/bin/sh\ninput=$(cat)\nprintf '%s\\n' \"$input\" >> '{0}'\nprintf 'RAM_CODE=%s PLUGIN=%s\\n' \"$RAM_CODE\" \"$CLAUDE_PLUGIN_ROOT\" >> '{0}.env'\n{body}\n",
                log.display()
            ),
        )
        .unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
        path.display().to_string()
    }

    fn logged(&self, name: &str) -> Vec<Value> {
        std::fs::read_to_string(self.log.join(name))
            .unwrap_or_default()
            .lines()
            .filter_map(|line| serde_json::from_str(line).ok())
            .collect()
    }

    fn settings(&self, path: &Path, hooks: Value) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, json!({"hooks": hooks}).to_string()).unwrap();
    }

    /// `~/.ram/settings.json`.
    fn ram(&self, hooks: Value) {
        self.settings(&self.home.join(".ram/settings.json"), hooks);
    }

    fn hooked(&self) -> Hooked {
        Hooked::load_from(&self.project, "ram-test", &self.home)
    }
}

fn group(matcher: &str, command: &str) -> Value {
    json!([{"matcher": matcher, "hooks": [{"type": "command", "command": command}]}])
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
    fn requests(&self) -> Vec<Value> {
        self.requests.lock().unwrap().clone()
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

fn tool(name: &str, input: Value) -> Value {
    json!({"content": [{"type": "tool_use", "id": "t1", "name": name, "input": input}]})
}

fn text(words: &str) -> Value {
    json!({"content": [{"type": "text", "text": words}]})
}

/// Runs one task, and returns the model's requests and the transcript lines from hooks.
fn turn(fixture: &Fixture, hooks: &Hooked, responses: Vec<Value>) -> (Vec<Value>, Vec<String>) {
    let llm = FakeLlm::new(responses);
    let mut notes = Vec::new();
    let _ = run_turn_hooked(
        &llm,
        &fixture.project,
        &[],
        "the task",
        hooks,
        &mut |event| {
            if let AgentEvent::Hook(note) = event {
                notes.push(note);
            }
        },
    );
    (llm.requests(), notes)
}

/// The tool result the model got back in its second request.
fn tool_result(requests: &[Value]) -> String {
    let messages = requests[1]["messages"].as_array().unwrap();
    messages.last().unwrap()["content"][0]["content"]
        .as_str()
        .unwrap_or("")
        .to_string()
}

const DENY: &str = r#"echo '{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":"not now"}}'"#;

#[test]
fn pre_tool_deny_blocks_the_command() {
    let fixture = Fixture::new("deny");
    fixture.ram(json!({"PreToolUse": group("run_command", &fixture.script("pre", DENY))}));
    let (requests, notes) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"}))],
    );
    assert!(!fixture.project.join("made").exists(), "the command ran");
    assert_eq!(tool_result(&requests), "blocked by a hook: not now");
    assert_eq!(notes, ["hook PreToolUse: not now"]);
}

#[test]
fn pre_tool_ask_counts_as_deny() {
    let fixture = Fixture::new("ask");
    let ask = r#"echo '{"hookSpecificOutput":{"permissionDecision":"ask","permissionDecisionReason":"sure?"}}'"#;
    fixture.ram(json!({"PreToolUse": group("", &fixture.script("pre", ask))}));
    let (requests, _) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"}))],
    );
    assert!(!fixture.project.join("made").exists());
    assert_eq!(tool_result(&requests), "blocked by a hook: sure?");
}

#[test]
fn exit_two_blocks_with_stderr() {
    let fixture = Fixture::new("exit2");
    fixture.ram(json!({"PreToolUse": group("", &fixture.script("pre", "echo nope >&2; exit 2"))}));
    let (requests, _) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"}))],
    );
    assert!(!fixture.project.join("made").exists());
    assert_eq!(tool_result(&requests), "blocked by a hook: nope");
}

#[test]
fn matcher_selects_the_tool() {
    let fixture = Fixture::new("matcher");
    let hooks = json!([
        {"matcher": "write_file", "hooks": [{"type": "command", "command": fixture.script("w", "")}]},
        {"matcher": "*", "hooks": [{"type": "command", "command": fixture.script("star", "")}]},
        {"hooks": [{"type": "command", "command": fixture.script("empty", "")}]},
        {"matcher": "run_.*", "hooks": [{"type": "command", "command": fixture.script("re", "")}]},
    ]);
    fixture.ram(json!({"PreToolUse": hooks}));
    turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "true"}))],
    );
    let counts = |names: [&str; 4]| names.map(|name| fixture.logged(name).len());
    assert_eq!(counts(["w", "star", "empty", "re"]), [0, 1, 1, 1]);
    turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("write_file", json!({"path": "a.txt", "content": "x"}))],
    );
    assert_eq!(counts(["w", "star", "empty", "re"]), [1, 2, 2, 1]);
}

#[test]
fn post_tool_context_reaches_the_model() {
    let fixture = Fixture::new("post");
    let context = r#"echo '{"hookSpecificOutput":{"hookEventName":"PostToolUse","additionalContext":"remember the QA"}}'"#;
    fixture.ram(json!({"PostToolUse": group("run_command", &fixture.script("post", context))}));
    let (requests, _) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "echo hi"}))],
    );
    let result = tool_result(&requests);
    assert!(result.starts_with("exit 0\nhi"), "{result}");
    assert!(result.ends_with("\n\nremember the QA"), "{result}");
}

#[test]
fn prompt_block_sends_nothing() {
    let fixture = Fixture::new("prompt-block");
    let block = r#"echo '{"decision":"block","reason":"not on a Friday"}'"#;
    fixture.ram(json!({"UserPromptSubmit": group("", &fixture.script("prompt", block))}));
    let (requests, notes) = turn(&fixture, &fixture.hooked(), vec![text("never")]);
    assert!(requests.is_empty(), "the model got a request");
    assert!(
        notes.iter().any(|note| note.contains("not on a Friday")),
        "{notes:?}"
    );
}

#[test]
fn session_and_prompt_context_come_first() {
    let fixture = Fixture::new("context");
    fixture.ram(json!({
        "SessionStart": group("", &fixture.script("start", "echo from the session")),
        "UserPromptSubmit": group("", &fixture.script("prompt", "echo from the prompt")),
    }));
    let mut hooks = fixture.hooked();
    hooks.start_context = hooks
        .fire("SessionStart", "startup", json!({"source": "startup"}))
        .contexts;
    let (requests, _) = turn(&fixture, &hooks, vec![text("ok")]);
    let first = requests[0]["messages"][0]["content"]
        .as_str()
        .unwrap()
        .to_string();
    assert_eq!(first, "from the session\n\nfrom the prompt\n\nthe task");
}

#[test]
fn stop_block_gives_one_more_turn() {
    let fixture = Fixture::new("stop-once");
    let once = r#"case "$input" in *'"stop_hook_active":false'*) echo '{"decision":"block","reason":"go on"}' ;; esac"#;
    fixture.ram(json!({"Stop": group("", &fixture.script("stop", once))}));
    let (requests, _) = turn(
        &fixture,
        &fixture.hooked(),
        vec![text("first"), text("second")],
    );
    assert_eq!(requests.len(), 2);
    let messages = requests[1]["messages"].as_array().unwrap();
    assert_eq!(messages.last().unwrap()["content"], "go on");
    let stops = fixture.logged("stop");
    assert_eq!(stops.len(), 2);
    assert_eq!(
        (
            stops[0]["stop_hook_active"].clone(),
            stops[1]["stop_hook_active"].clone()
        ),
        (json!(false), json!(true))
    );
}

#[test]
fn stop_blocks_at_most_three_times() {
    let fixture = Fixture::new("stop-always");
    let always = r#"echo '{"decision":"block","reason":"again"}'"#;
    fixture.ram(json!({"Stop": group("", &fixture.script("stop", always))}));
    let (requests, _) = turn(&fixture, &fixture.hooked(), vec![]);
    assert_eq!(requests.len(), 4, "the first answer, then 3 blocks");
}

#[test]
fn a_hook_past_its_timeout_is_killed() {
    let fixture = Fixture::new("timeout");
    let pid = fixture.base.join("child.pid");
    let body = format!("sleep 30 & echo $! > '{}'; wait", pid.display());
    let hooks = json!([{"hooks": [{"type": "command", "command": fixture.script("slow", &body), "timeout": 1}]}]);
    fixture.ram(json!({"PreToolUse": hooks}));
    let started = Instant::now();
    let (_, notes) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"}))],
    );
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "{:?}",
        started.elapsed()
    );
    assert!(
        fixture.project.join("made").exists(),
        "the tool did not run"
    );
    assert_eq!(
        notes
            .iter()
            .filter(|note| note.contains("timed out"))
            .count(),
        1,
        "{notes:?}"
    );
    let child = std::fs::read_to_string(&pid).unwrap();
    assert!(!alive(child.trim()), "the hook's child outlives the timeout");
}

fn alive(pid: &str) -> bool {
    Command::new("kill").args(["-0", pid]).stderr(std::process::Stdio::null()).status().unwrap().success()
}

#[test]
fn a_failing_hook_does_not_stop_the_turn() {
    let fixture = Fixture::new("failing");
    fixture
        .ram(json!({"PreToolUse": group("", &fixture.script("bad", "echo broken >&2; exit 1"))}));
    let (_, notes) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"}))],
    );
    assert!(fixture.project.join("made").exists());
    assert_eq!(notes.len(), 1);
    assert!(notes[0].contains("failed: broken"), "{notes:?}");
}

#[test]
fn the_same_command_runs_once() {
    let fixture = Fixture::new("once");
    let command = fixture.script("same", "");
    fixture.ram(json!({"PreToolUse": group("", &command)}));
    fixture.settings(
        &fixture.home.join(".claude/settings.json"),
        json!({"PreToolUse": group("", &command)}),
    );
    turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "true"}))],
    );
    assert_eq!(fixture.logged("same").len(), 1);
}

/// One plugin, installed and enabled the way Claude Code records it.
fn plugin(fixture: &Fixture, name: &str, enabled: bool, hooks: Value) -> PathBuf {
    let root = fixture.base.join(format!("plugins/{name}"));
    std::fs::create_dir_all(root.join(".claude-plugin")).unwrap();
    std::fs::write(
        root.join(".claude-plugin/plugin.json"),
        json!({"name": name, "hooks": hooks}).to_string(),
    )
    .unwrap();
    let installed_path = fixture.home.join(".claude/plugins/installed_plugins.json");
    let mut installed: Value = std::fs::read_to_string(&installed_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({"version": 2, "plugins": {}}));
    installed["plugins"][format!("{name}@m")] = json!([{"installPath": root}]);
    std::fs::create_dir_all(installed_path.parent().unwrap()).unwrap();
    std::fs::write(&installed_path, installed.to_string()).unwrap();
    let settings_path = fixture.home.join(".claude/settings.json");
    let mut settings: Value = std::fs::read_to_string(&settings_path)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| json!({}));
    settings["enabledPlugins"][format!("{name}@m")] = json!(enabled);
    std::fs::write(&settings_path, settings.to_string()).unwrap();
    root
}

#[test]
fn every_source_runs_in_order() {
    let fixture = Fixture::new("order");
    let mark = |name: &str| group("", &format!("echo {name}"));
    fixture.ram(json!({"Stop": mark("home-ram")}));
    fixture.settings(
        &fixture.project.join(".ram/settings.json"),
        json!({"Stop": mark("project-ram")}),
    );
    fixture.settings(
        &fixture.project.join(".ram/settings.local.json"),
        json!({"Stop": mark("project-ram-local")}),
    );
    fixture.settings(
        &fixture.home.join(".claude/settings.json"),
        json!({"Stop": mark("home-claude")}),
    );
    fixture.settings(
        &fixture.project.join(".claude/settings.json"),
        json!({"Stop": mark("project-claude")}),
    );
    fixture.settings(
        &fixture.project.join(".claude/settings.local.json"),
        json!({"Stop": mark("project-claude-local")}),
    );
    plugin(&fixture, "a", true, json!({"Stop": mark("plugin")}));
    let ramify = fixture.base.join("ramify");
    fixture.settings(
        &ramify.join("hooks/ram.json"),
        json!({"Stop": mark("ramify")}),
    );
    trust(&fixture.home, &fixture.project, true).unwrap();
    let set = HookSet::load(&fixture.home, &fixture.project, Some(&ramify), Sources::All);
    let order: Vec<&str> = set
        .matching("Stop", "")
        .iter()
        .map(|hook| hook.command.trim_start_matches("echo "))
        .collect();
    assert_eq!(
        order,
        [
            "home-ram",
            "project-ram",
            "project-ram-local",
            "home-claude",
            "project-claude",
            "project-claude-local",
            "plugin",
            "ramify"
        ]
    );
}

#[test]
fn a_broken_source_is_skipped() {
    let fixture = Fixture::new("broken");
    std::fs::create_dir_all(fixture.home.join(".ram")).unwrap();
    std::fs::write(fixture.home.join(".ram/settings.json"), "not json").unwrap();
    fixture.settings(
        &fixture.home.join(".claude/settings.json"),
        json!({"Stop": group("", "echo fine")}),
    );
    let set = HookSet::load(&fixture.home, &fixture.project, None, Sources::All);
    assert_eq!(set.warnings.len(), 1, "{:?}", set.warnings);
    assert!(set.has("Stop"));
}

#[test]
fn enabled_plugin_hooks_run_with_plugin_root() {
    let fixture = Fixture::new("plugins");
    let on = plugin(
        &fixture,
        "on",
        true,
        json!({"PreToolUse": group("", &fixture.script("on", ""))}),
    );
    plugin(
        &fixture,
        "off",
        false,
        json!({"PreToolUse": group("", &fixture.script("off", ""))}),
    );
    turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "true"}))],
    );
    assert_eq!(fixture.logged("on").len(), 1);
    assert_eq!(fixture.logged("off").len(), 0);
    let env = std::fs::read_to_string(fixture.log.join("on.env")).unwrap();
    assert!(env.contains(&format!("PLUGIN={}", on.display())), "{env}");
}

#[test]
fn hook_input_has_the_common_and_event_fields() {
    let fixture = Fixture::new("fields");
    let mut hooks = json!({});
    for event in [
        "SessionStart",
        "UserPromptSubmit",
        "PreToolUse",
        "PostToolUse",
        "Stop",
        "SessionEnd",
    ] {
        hooks[event] = group("", &fixture.script(event, ""));
    }
    fixture.ram(hooks);
    let mut hooked = fixture.hooked();
    hooked.start_context = hooked
        .fire("SessionStart", "startup", json!({"source": "startup"}))
        .contexts;
    turn(
        &fixture,
        &hooked,
        vec![tool("run_command", json!({"command": "echo hi"}))],
    );
    hooked.fire("SessionEnd", "", json!({"reason": "exit"}));
    let expected = [
        ("SessionStart", "source", json!("startup")),
        ("UserPromptSubmit", "prompt", json!("the task")),
        ("PreToolUse", "tool_input", json!({"command": "echo hi"})),
        ("PostToolUse", "tool_response", json!("exit 0\nhi\n")),
        ("Stop", "stop_hook_active", json!(false)),
        ("SessionEnd", "reason", json!("exit")),
    ];
    for (event, field, value) in expected {
        let input = &fixture.logged(event)[0];
        assert_eq!(input["session_id"], "ram-test", "{event}");
        assert_eq!(input["cwd"], json!(fixture.project), "{event}");
        assert_eq!(input["hook_event_name"], event);
        assert_eq!(input[field], value, "{event}.{field}");
        let env = std::fs::read_to_string(fixture.log.join(format!("{event}.env"))).unwrap();
        assert!(env.contains("RAM_CODE=1"), "{event}: {env}");
    }
}

#[test]
fn a_deny_wins_over_an_allow() {
    let fixture = Fixture::new("wins");
    let allow = r#"echo '{"hookSpecificOutput":{"permissionDecision":"allow"}}'"#;
    let hooks = json!([{"hooks": [
        {"type": "command", "command": fixture.script("allow", allow)},
        {"type": "command", "command": fixture.script("deny", DENY)},
    ]}]);
    fixture.ram(json!({"PreToolUse": hooks}));
    let (requests, _) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"}))],
    );
    assert!(!fixture.project.join("made").exists());
    assert_eq!(tool_result(&requests), "blocked by a hook: not now");
}

#[test]
fn no_hooks_no_process() {
    let fixture = Fixture::new("none");
    // An `sh` that logs each start: hooks run through `sh -c`.
    let bin = fixture.base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    let starts = fixture.base.join("sh-starts");
    std::fs::write(
        bin.join("sh"),
        format!("#!/bin/bash\necho x >> '{}'\nexec /bin/sh \"$@\"\n", starts.display()),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin.join("sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    std::fs::write(fixture.project.join("a.txt"), "x").unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    let hooks = fixture.hooked();
    turn(&fixture, &hooks, vec![tool("read_file", json!({"path": "a.txt"}))]);
    // The probe works: with one hook, `sh` starts.
    fixture.ram(json!({"Stop": group("", "true")}));
    turn(&fixture, &fixture.hooked(), vec![text("ok")]);
    std::env::set_var("PATH", path);
    assert_eq!(hooks.spawned.get(), 0);
    let count = std::fs::read_to_string(&starts).unwrap_or_default().lines().count();
    assert_eq!(count, 1, "only the turn with a hook starts a process");
}

/// The ramify CLI from this repository. The hooks run it through `bin/ramify`, which prefers
/// the release build: build that one, so that a stale build cannot answer.
fn ramify_cli() -> PathBuf {
    let repo = Path::new(env!("CARGO_MANIFEST_DIR"));
    let status = Command::new("cargo")
        .args(["build", "--release", "--quiet", "--manifest-path"])
        .arg(repo.join("cli/Cargo.toml"))
        .status()
        .unwrap();
    assert!(status.success(), "the ramify CLI did not build");
    repo.join("cli/target/release/ramify")
}

/// A configured project on `feat/demo` with a work order whose steps before `pr` are missing.
fn ramify_project(fixture: &Fixture) {
    std::fs::write(
        fixture.project.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"\"\n",
    )
    .unwrap();
    std::fs::create_dir_all(fixture.project.join(".ramify/work/demo")).unwrap();
    std::fs::write(
        fixture.project.join(".ramify/work/demo/order.md"),
        "---\nworkflow: feature\nlane: light\nbranch: feat/demo\ntags: []\ncreated: 2026-11-12\nsteps: []\n---\n\n# demo\n",
    )
    .unwrap();
    git(&["checkout", "-q", "-b", "feat/demo"], &fixture.project);
    git(&["add", "."], &fixture.project);
    git(&["commit", "-qm", "init"], &fixture.project);
    std::env::set_var("RAMIFY_HOME", Path::new(env!("CARGO_MANIFEST_DIR")));
}

#[test]
fn ramify_pr_gate_denies_in_ram_code() {
    let fixture = Fixture::new("pr-gate");
    ramify_cli();
    ramify_project(&fixture);
    // A `gh` that leaves a trace if it ever runs.
    let bin = fixture.base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("gh"),
        format!(
            "#!/bin/sh\ntouch '{}'\n",
            fixture.base.join("gh-ran").display()
        ),
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin.join("gh"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    let (requests, _) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool(
            "run_command",
            json!({"command": "gh pr create --title x --body y"}),
        )],
    );
    std::env::set_var("PATH", path);
    assert!(!fixture.base.join("gh-ran").exists(), "gh ran");
    let result = tool_result(&requests);
    assert!(
        result.starts_with("blocked by a hook: The work order is not ready for a pull request"),
        "{result}"
    );
}

#[test]
fn ramify_session_start_writes_the_ram_record() {
    let fixture = Fixture::new("record");
    ramify_cli();
    ramify_project(&fixture);
    let hooks = Hooked::load_from(&fixture.project, "ram-record1", &fixture.home);
    let started = hooks.fire("SessionStart", "startup", json!({"source": "startup"}));
    let record_path = fixture.base.join("state/sessions/ram-record1.session");
    let record: Value =
        serde_json::from_str(&std::fs::read_to_string(&record_path).expect("a record")).unwrap();
    assert_eq!(record["agent"], "ram");
    assert!(
        started.contexts.join("\n").contains("work order"),
        "{:?}",
        started.contexts
    );
    hooks.fire("SessionEnd", "", json!({"reason": "exit"}));
    assert!(!record_path.exists(), "the record outlives the session");
}

#[test]
fn ram_code_no_longer_writes_its_own_record() {
    let fixture = Fixture::new("own-record");
    turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "true"}))],
    );
    let sessions = fixture.base.join("state/sessions");
    let records = std::fs::read_dir(&sessions)
        .map(|entries| entries.count())
        .unwrap_or(0);
    assert_eq!(records, 0);
}

#[test]
fn pre_workflow_deny_blocks_the_merge() {
    let fixture = Fixture::new("workflow-deny");
    let deny = r#"echo '{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"merges wait for the release"}}'"#;
    fixture.ram(json!({"PreWorkflow": group("pr.merge", &fixture.script("merge", deny))}));
    let mut hooks = fixture.hooked();
    hooks.ramify = ramify_cli().display().to_string();
    let (requests, _) = turn(
        &fixture,
        &hooks,
        vec![tool(
            "run_command",
            json!({"command": "gh pr merge 50 --squash; touch merged"}),
        )],
    );
    assert!(!fixture.project.join("merged").exists(), "the command ran");
    assert!(tool_result(&requests).contains("merges wait for the release"));
    let input = &fixture.logged("merge")[0];
    assert_eq!(
        (input["action"].clone(), input["details"]["pr"].clone()),
        (json!("pr.merge"), json!(50))
    );
}

#[test]
fn post_workflow_gets_details_and_output() {
    let fixture = Fixture::new("workflow-post");
    fixture.ram(json!({"PostWorkflow": group("workitem.update", &fixture.script("update", ""))}));
    // A `ramify` on the PATH that answers like `step done`.
    let bin = fixture.base.join("bin");
    std::fs::create_dir_all(&bin).unwrap();
    std::fs::write(
        bin.join("ramify"),
        "#!/bin/sh\necho '  recorded build — next: audit'\n",
    )
    .unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(bin.join("ramify"), std::fs::Permissions::from_mode(0o755)).unwrap();
    let path = std::env::var("PATH").unwrap_or_default();
    std::env::set_var("PATH", format!("{}:{path}", bin.display()));
    let mut hooks = fixture.hooked();
    hooks.ramify = ramify_cli().display().to_string();
    turn(
        &fixture,
        &hooks,
        vec![tool(
            "run_command",
            json!({"command": "ramify step done build"}),
        )],
    );
    std::env::set_var("PATH", path);
    let input = &fixture.logged("update")[0];
    assert_eq!(input["details"]["step"], "build");
    assert_eq!(input["succeeded"], true);
    assert!(input["tool_response"]
        .as_str()
        .unwrap()
        .contains("recorded build"));
}

#[test]
fn no_workflow_hooks_no_classifier() {
    let fixture = Fixture::new("no-classifier");
    let classifier = fixture.script("classify", "echo '[]'");
    fixture.ram(json!({"PreToolUse": group("", "true")}));
    let mut hooks = fixture.hooked();
    hooks.ramify = classifier.clone();
    turn(
        &fixture,
        &hooks,
        vec![tool("run_command", json!({"command": "git push"}))],
    );
    assert_eq!(
        fixture.logged("classify").len(),
        0,
        "the classifier ran with no workflow hook"
    );
    fixture.ram(json!({"PreWorkflow": group("push", "true")}));
    let mut hooks = fixture.hooked();
    hooks.ramify = classifier;
    turn(
        &fixture,
        &hooks,
        vec![tool("run_command", json!({"command": "git push"}))],
    );
    assert_eq!(fixture.logged("classify").len(), 1);
}

#[test]
fn continue_false_ends_the_task() {
    let fixture = Fixture::new("continue");
    let stop = r#"echo '{"continue":false,"stopReason":"out of budget"}'"#;
    fixture.ram(json!({"PostToolUse": group("run_command", &fixture.script("post", stop))}));
    let (requests, notes) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "true"})), text("never asked")],
    );
    assert_eq!(requests.len(), 1, "the model got another turn");
    assert!(notes.iter().any(|note| note.contains("out of budget")), "{notes:?}");
}

#[test]
fn system_message_shows_in_the_transcript() {
    let fixture = Fixture::new("system");
    let message = r#"echo '{"systemMessage":"lint is slow today"}'"#;
    fixture.ram(json!({"PreToolUse": group("", &fixture.script("pre", message))}));
    let (_, notes) = turn(&fixture, &fixture.hooked(), vec![tool("run_command", json!({"command": "true"}))]);
    assert_eq!(notes, ["hook PreToolUse: lint is slow today"]);
}

#[test]
fn an_old_ramify_plugin_gives_way_to_ram_codes_own_hooks() {
    // The installed ramify plugin predates hooks/ram.json: its Claude Code hooks match `Bash` and
    // run without RAMIFY_AGENT=ram. Ram Code's own ram.json takes its place.
    let fixture = Fixture::new("old-ramify");
    let old = fixture.base.join("plugins/ramify");
    std::fs::create_dir_all(old.join(".claude-plugin")).unwrap();
    std::fs::write(
        old.join(".claude-plugin/plugin.json"),
        json!({"hooks": {"Stop": group("", "echo old-claude-hook")}}).to_string(),
    )
    .unwrap();
    let installed = fixture.home.join(".claude/plugins/installed_plugins.json");
    std::fs::create_dir_all(installed.parent().unwrap()).unwrap();
    std::fs::write(&installed, json!({"plugins": {"ramify@ramify": [{"installPath": old}]}}).to_string()).unwrap();
    std::fs::write(
        fixture.home.join(".claude/settings.json"),
        json!({"enabledPlugins": {"ramify@ramify": true}}).to_string(),
    )
    .unwrap();
    let ramify = fixture.base.join("ramify-home");
    fixture.settings(&ramify.join("hooks/ram.json"), json!({"Stop": group("", "echo ram-hook")}));
    let set = HookSet::load(&fixture.home, &fixture.project, Some(&ramify), Sources::All);
    let commands: Vec<&str> = set.matching("Stop", "").iter().map(|hook| hook.command.as_str()).collect();
    assert_eq!(commands, ["echo ram-hook"]);
    assert_eq!(set.matching("Stop", "")[0].plugin_root.as_deref(), Some(ramify.as_path()));
}

#[test]
fn an_untrusted_project_runs_no_hook_of_its_own() {
    let fixture = Fixture::new("untrusted");
    fixture.ram(json!({"Stop": group("", "echo home")}));
    fixture.settings(&fixture.project.join(".ram/settings.json"), json!({"Stop": group("", "echo ram")}));
    fixture.settings(&fixture.project.join(".claude/settings.json"), json!({"Stop": group("", "echo claude")}));
    let commands = |set: &HookSet| set.matching("Stop", "").iter().map(|hook| hook.command.clone()).collect::<Vec<_>>();
    let set = HookSet::load(&fixture.home, &fixture.project, None, Sources::All);
    assert_eq!(commands(&set), ["echo home"]);
    assert_eq!(set.warnings.len(), 2, "{:?}", set.warnings);
    assert!(set.warnings[0].contains("ramify hooks trust"), "{:?}", set.warnings);
    trust(&fixture.home, &fixture.project, true).unwrap();
    let set = HookSet::load(&fixture.home, &fixture.project, None, Sources::All);
    assert_eq!(commands(&set), ["echo home", "echo ram", "echo claude"]);
    assert!(set.warnings.is_empty(), "{:?}", set.warnings);
    trust(&fixture.home, &fixture.project, false).unwrap();
    let set = HookSet::load(&fixture.home, &fixture.project, None, Sources::All);
    assert_eq!(commands(&set), ["echo home"]);
}

#[test]
fn a_hook_that_leaves_a_child_answers_in_time() {
    let fixture = Fixture::new("child");
    let body = format!("sleep 6 &\n{DENY}");
    let hooks = json!([{"hooks": [{"type": "command", "command": fixture.script("pre", &body), "timeout": 5}]}]);
    fixture.ram(json!({"PreToolUse": hooks}));
    let started = Instant::now();
    let (requests, _) = turn(&fixture, &fixture.hooked(), vec![tool("run_command", json!({"command": "touch made"}))]);
    assert!(started.elapsed() < Duration::from_secs(2), "{:?}", started.elapsed());
    assert_eq!(tool_result(&requests), "blocked by a hook: not now");
}

#[test]
fn a_hook_that_does_not_read_a_large_input_times_out() {
    let fixture = Fixture::new("large");
    std::fs::write(fixture.project.join("big.txt"), "x".repeat(200_000)).unwrap();
    let slow = json!({"type": "command", "command": "sleep 6", "timeout": 1});
    let fast = json!({"type": "command", "command": fixture.script("fast", "echo seen"), "timeout": 1});
    fixture.ram(json!({"PostToolUse": [{"hooks": [slow, fast]}]}));
    let started = Instant::now();
    let (_, notes) = turn(&fixture, &fixture.hooked(), vec![tool("read_file", json!({"path": "big.txt"}))]);
    assert!(started.elapsed() < Duration::from_secs(4), "{:?}", started.elapsed());
    assert_eq!(notes.iter().filter(|note| note.contains("timed out")).count(), 1, "{notes:?}");
    assert_eq!(fixture.logged("fast").len(), 1, "the other hook did not run");
}

#[test]
fn pre_tool_continue_false_stops_before_the_tool() {
    let fixture = Fixture::new("pre-continue");
    let halt = r#"echo '{"continue":false,"stopReason":"halt"}'"#;
    fixture.ram(json!({"PreToolUse": group("", &fixture.script("pre", halt))}));
    let (requests, notes) = turn(
        &fixture,
        &fixture.hooked(),
        vec![tool("run_command", json!({"command": "touch made"})), text("never")],
    );
    assert!(!fixture.project.join("made").exists(), "the command ran");
    assert_eq!(requests.len(), 1, "the task went on");
    assert!(notes.iter().any(|note| note.contains("the task was stopped: halt")), "{notes:?}");
}

#[test]
fn a_matcher_on_an_event_with_no_subject_is_ignored() {
    let fixture = Fixture::new("stop-matcher");
    fixture.ram(json!({"Stop": group("anything", &fixture.script("stop", ""))}));
    turn(&fixture, &fixture.hooked(), vec![text("ok")]);
    assert_eq!(fixture.logged("stop").len(), 1);
}

#[test]
fn stop_blocks_count_in_a_row() {
    let fixture = Fixture::new("stop-row");
    let block = r#"echo '{"decision":"block","reason":"go on"}'"#;
    fixture.ram(json!({"Stop": group("", &fixture.script("stop", block))}));
    let responses = vec![
        text("a"),
        tool("run_command", json!({"command": "true"})),
        text("b"),
        text("c"),
        text("d"),
        text("e"),
    ];
    let (requests, _) = turn(&fixture, &fixture.hooked(), responses);
    // a: block 1; the tool call starts the count again; b, c, d: blocks 1 to 3; e ends.
    assert_eq!(requests.len(), 6);
}

#[test]
fn a_task_waits_for_the_snapshot_of_session_start() {
    let fixture = Fixture::new("snapshot");
    fixture.ram(json!({"Stop": group("", "echo at-start")}));
    let start = fixture.hooked();
    // The model writes a hook file after the session starts: it must not run in this session.
    fixture.ram(json!({"Stop": group("", "echo written-later")}));
    let dir = fixture.base.join("start");
    let writer = {
        let dir = dir.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            start.leave_start(&dir, &["the context".to_string()]).unwrap();
        })
    };
    let task = Hooked::for_task_from(&fixture.project, "ram-test", Some(&dir), &fixture.home);
    writer.join().unwrap();
    let commands: Vec<_> = task.set.matching("Stop", "").iter().map(|hook| hook.command.clone()).collect();
    assert_eq!(commands, ["echo at-start"]);
    assert_eq!(std::fs::read_to_string(dir.join("context.txt")).unwrap(), "the context");
}

#[test]
fn a_plugin_installed_for_another_project_is_left_out() {
    let fixture = Fixture::new("plugin-scope");
    let root = plugin(&fixture, "p", true, json!({"Stop": group("", "echo plugin")}));
    let installed_path = fixture.home.join(".claude/plugins/installed_plugins.json");
    let install = |project: &Path| {
        let entry = json!([{"scope": "project", "projectPath": project, "installPath": root}]);
        std::fs::write(&installed_path, json!({"version": 2, "plugins": {"p@m": entry}}).to_string()).unwrap();
        HookSet::load(&fixture.home, &fixture.project, None, Sources::All).matching("Stop", "").len()
    };
    assert_eq!(install(Path::new("/elsewhere")), 0);
    assert_eq!(install(&fixture.project), 1);
}

#[test]
fn a_task_that_ends_without_stop_leaves_the_record_idle() {
    let fixture = Fixture::new("idle");
    let sessions = fixture.base.join("state/sessions");
    std::fs::create_dir_all(&sessions).unwrap();
    let record = sessions.join("ram-9.session");
    std::fs::write(&record, r#"{"agent":"ram","state":"working","since":1,"pid":9}"#).unwrap();
    ram_code::hooked::idle_record(&fixture.base.join("state"), "ram-9");
    let value: Value = serde_json::from_str(&std::fs::read_to_string(&record).unwrap()).unwrap();
    assert_eq!((value["state"].as_str(), value["agent"].as_str()), (Some("idle"), Some("ram")));
    assert!(value["since"].as_u64().unwrap() > 1);
}
