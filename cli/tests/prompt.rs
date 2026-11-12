//! The session record the hooks write, `ramify prompt`, and the zsh plugin that shows it.

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Output, Stdio};
use std::time::{Duration, Instant};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramify-prompt-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ramify")
}

/// A configured repo with `count` linked worktrees, `wt-1` to `wt-<count>`, and its state folder.
struct Fixture {
    base: PathBuf,
    primary: PathBuf,
    state: PathBuf,
}

impl Fixture {
    fn new(name: &str, conf: &str, count: usize) -> Self {
        let base = temp(name);
        let primary = base.join("demo");
        fs::create_dir_all(&primary).unwrap();
        git(&["init", "-q", "-b", "main"], &primary);
        fs::write(primary.join(".ramify.conf"), conf).unwrap();
        git(&["add", "."], &primary);
        git(&["commit", "-qm", "init"], &primary);
        for index in 1..=count {
            let path = base.join(format!("wt-{index}"));
            git(
                &[
                    "worktree",
                    "add",
                    "-q",
                    "-b",
                    &format!("feat/wt-{index}"),
                    path.to_str().unwrap(),
                ],
                &primary,
            );
        }
        let state = base.join("state");
        fs::create_dir_all(state.join("sessions")).unwrap();
        Fixture {
            base,
            primary,
            state,
        }
    }

    fn plain(name: &str) -> Self {
        Fixture::new(name, "RAMIFY_SERVICES=\"\"\n", 2)
    }

    fn worktree(&self, index: usize) -> PathBuf {
        self.base.join(format!("wt-{index}"))
    }

    fn sessions(&self) -> PathBuf {
        self.state.join("sessions")
    }

    fn hook(&self, event: &str, input: Value, agent: Option<&str>) -> Output {
        let mut command = Command::new(repo_root().join("hooks/ramify-hook"));
        command
            .arg(event)
            .env("RAMIFY_STATE_DIR", &self.state)
            .env("RAMIFY_REGISTRY", self.base.join("registry"))
            .env("RAMIFY_REAP", "0")
            .env("RAMIFY_AUTOSTART", "0")
            .env_remove("RAMIFY_ROOT")
            .env_remove("RAMIFY_AGENT")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        if let Some(agent) = agent {
            command.env("RAMIFY_AGENT", agent);
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    }

    fn start(&self, session: &str, cwd: &Path, agent: Option<&str>) {
        self.hook(
            "session-start",
            json!({"session_id": session, "source": "startup", "cwd": cwd}),
            agent,
        );
    }

    fn record(&self, session: &str) -> Option<Value> {
        let text = fs::read_to_string(self.sessions().join(format!("{session}.session"))).ok()?;
        serde_json::from_str(&text).ok()
    }

    /// Writes a record and a name the way the hooks do, without the hooks.
    #[allow(clippy::too_many_arguments)]
    fn put(
        &self,
        session: &str,
        root: &Path,
        agent: &str,
        state: &str,
        since: u64,
        pid: u32,
        name: Option<&str>,
    ) {
        let record =
            json!({"agent": agent, "root": root, "pid": pid, "state": state, "since": since});
        fs::write(
            self.sessions().join(format!("{session}.session")),
            record.to_string(),
        )
        .unwrap();
        if let Some(name) = name {
            fs::write(
                self.sessions().join(format!("{session}.name")),
                format!("{name}\n"),
            )
            .unwrap();
        }
    }

    fn run(&self, args: &[&str], cwd: &Path) -> Output {
        Command::new(binary())
            .args(args)
            .current_dir(cwd)
            .env("RAMIFY_STATE_DIR", &self.state)
            .env("RAMIFY_REGISTRY", self.base.join("registry"))
            .env("RAMIFY_RUNS_DIR", self.base.join("runs"))
            .env_remove("RAMIFY_ROOT")
            .env_remove("RAMIFY_STATUSLINE_ICON")
            .stdin(Stdio::null())
            .output()
            .unwrap()
    }

    fn prompt(&self, cwd: &Path) -> String {
        let output = self.run(&["prompt"], cwd);
        assert_eq!(output.status.code(), Some(0), "prompt exit");
        String::from_utf8_lossy(&output.stdout)
            .trim_end()
            .to_string()
    }

    fn statusline(&self, cwd: &Path, json_out: bool) -> String {
        let mut child = Command::new(binary())
            .arg("statusline")
            .args(if json_out { vec!["--json"] } else { vec![] })
            .env("RAMIFY_STATE_DIR", &self.state)
            .env("RAMIFY_REGISTRY", self.base.join("registry"))
            .env("RAMIFY_RUNS_DIR", self.base.join("runs"))
            .env_remove("RAMIFY_ROOT")
            .env_remove("RAMIFY_STATUSLINE_ICON")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .unwrap();
        let input = json!({"session_id": "none", "workspace": {"current_dir": cwd}});
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.to_string().as_bytes())
            .unwrap();
        String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout)
            .trim_end()
            .to_string()
    }
}

fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
}

fn dead_pid() -> u32 {
    let mut child = Command::new("true").spawn().unwrap();
    let pid = child.id();
    child.wait().unwrap();
    pid
}

fn alive(pid: u64) -> bool {
    Command::new("kill")
        .args(["-0", &pid.to_string()])
        .status()
        .unwrap()
        .success()
}

#[test]
fn session_start_writes_a_claude_record() {
    let fixture = Fixture::plain("start-claude");
    fixture.start("s1", &fixture.worktree(1), None);
    let record = fixture.record("s1").expect("a record");
    assert_eq!(record["agent"], "claude");
    assert_eq!(
        Path::new(record["root"].as_str().unwrap())
            .canonicalize()
            .unwrap(),
        fixture.worktree(1)
    );
    assert!(alive(record["pid"].as_u64().unwrap()), "the pid runs");
    assert_eq!(record["state"], "idle");
    assert!(record["since"].as_u64().unwrap() >= now() - 60);
}

#[test]
fn session_start_with_codex_writes_a_codex_record() {
    let fixture = Fixture::plain("start-codex");
    fixture.start("s1", &fixture.worktree(1), Some("codex"));
    assert_eq!(fixture.record("s1").expect("a record")["agent"], "codex");
}

#[test]
fn prompt_submit_and_stop_switch_the_state() {
    let fixture = Fixture::plain("state");
    fixture.start("s1", &fixture.worktree(1), None);
    let since = fixture.record("s1").unwrap()["since"].clone();
    let input = json!({"session_id": "s1", "cwd": fixture.worktree(1)});
    fixture.hook("user-prompt-submit", input.clone(), None);
    assert_eq!(fixture.record("s1").unwrap()["state"], "working");
    fixture.hook("stop", input, None);
    let record = fixture.record("s1").unwrap();
    assert_eq!(record["state"], "idle");
    assert_eq!(record["since"], since);
}

#[test]
fn state_change_without_a_record_writes_nothing() {
    let fixture = Fixture::plain("state-none");
    fixture.hook(
        "user-prompt-submit",
        json!({"session_id": "ghost", "cwd": fixture.worktree(1)}),
        None,
    );
    assert!(fixture.record("ghost").is_none());
}

#[test]
fn session_end_removes_the_record() {
    let fixture = Fixture::plain("end");
    for reason in ["logout", "clear", "other"] {
        fixture.start("s1", &fixture.worktree(1), None);
        assert!(fixture.record("s1").is_some());
        fixture.hook(
            "session-end",
            json!({"session_id": "s1", "cwd": fixture.worktree(1), "reason": reason}),
            None,
        );
        assert!(fixture.record("s1").is_none(), "{reason} leaves the record");
    }
}

#[test]
fn prompt_input_exit_removes_the_record() {
    // The session is over on prompt_input_exit too: a record left behind would show a dead
    // session, or a stranger's once the pid is reused.
    let fixture = Fixture::plain("end-escape");
    fixture.start("s1", &fixture.worktree(1), None);
    fixture.hook(
        "session-end",
        json!({"session_id": "s1", "cwd": fixture.worktree(1), "reason": "prompt_input_exit"}),
        None,
    );
    assert!(fixture.record("s1").is_none());
}

#[test]
fn compact_and_resume_keep_the_state() {
    let fixture = Fixture::plain("compact");
    let wt = fixture.worktree(1);
    fixture.start("s1", &wt, None);
    fixture.hook("user-prompt-submit", json!({"session_id": "s1", "cwd": wt}), None);
    for source in ["compact", "resume"] {
        fixture.hook("session-start", json!({"session_id": "s1", "source": source, "cwd": wt}), None);
        assert_eq!(fixture.record("s1").unwrap()["state"], "working", "{source} reset the state");
    }
    fixture.hook("session-start", json!({"session_id": "s1", "source": "startup", "cwd": wt}), None);
    assert_eq!(fixture.record("s1").unwrap()["state"], "idle");
}

#[test]
fn a_move_to_a_new_worktree_moves_the_record() {
    let fixture = Fixture::plain("move");
    fixture.start("s1", &fixture.primary, None);
    let input = json!({
        "session_id": "s1",
        "cwd": fixture.primary,
        "tool_name": "Bash",
        "tool_input": {"command": "ramify new wt-2"},
        "tool_response": {"stdout": format!("ramify: worktree {}\n", fixture.worktree(2).display())},
    });
    fixture.hook("worktree-add", input, None);
    let root = fixture.record("s1").unwrap()["root"].as_str().unwrap().to_string();
    assert_eq!(Path::new(&root).canonicalize().unwrap(), fixture.worktree(2));
}

#[test]
fn a_late_state_change_does_not_bring_back_a_record() {
    let fixture = Fixture::plain("late");
    let wt = fixture.worktree(1);
    fixture.start("s1", &wt, None);
    fixture.hook("session-end", json!({"session_id": "s1", "cwd": wt, "reason": "logout"}), None);
    fixture.hook("stop", json!({"session_id": "s1", "cwd": wt}), None);
    assert!(fixture.record("s1").is_none());
    let leftovers: Vec<_> = fs::read_dir(fixture.sessions())
        .unwrap()
        .flatten()
        .filter(|entry| entry.file_name().to_string_lossy().ends_with(".tmp"))
        .collect();
    assert!(leftovers.is_empty(), "a temp file was left behind");
}

#[test]
fn session_end_ignores_a_session_id_with_a_path() {
    let fixture = Fixture::plain("end-path");
    let outside = fixture.state.join("x.session");
    fs::write(&outside, "{}").unwrap();
    fixture.hook(
        "session-end",
        json!({"session_id": "../x", "cwd": fixture.worktree(1), "reason": "logout"}),
        None,
    );
    assert!(outside.exists(), "session-end removed a file outside the sessions folder");
}

#[test]
fn a_dead_pid_is_skipped() {
    let fixture = Fixture::plain("dead");
    let wt = fixture.worktree(1);
    fixture.put("s1", &wt, "claude", "working", 1, dead_pid(), Some("birch"));
    assert!(!fixture.prompt(&wt).contains("birch"));
}

#[test]
fn reap_removes_dead_session_records() {
    let fixture = Fixture::plain("reap");
    let wt = fixture.worktree(1);
    fixture.put("dead", &wt, "claude", "idle", 1, dead_pid(), None);
    fixture.put("live", &wt, "claude", "idle", 1, std::process::id(), None);
    let output = fixture.run(&["reap"], &fixture.base);
    assert_eq!(output.status.code(), Some(0));
    assert!(fixture.record("dead").is_none(), "the dead record stays");
    assert!(fixture.record("live").is_some(), "the live record is gone");
}

#[test]
fn no_record_without_a_ramify_config() {
    let fixture = Fixture::plain("noconf");
    let bare = fixture.base.join("bare");
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "-q", "-b", "main"], &bare);
    fixture.start("s1", &bare, None);
    assert!(fixture.record("s1").is_none());
}

#[test]
fn prompt_matches_statusline_without_sessions() {
    let fixture = Fixture::plain("match");
    let wt = fixture.worktree(1);
    let prompt = fixture.prompt(&wt);
    assert_eq!(prompt, fixture.statusline(&wt, false));
    assert_eq!(prompt, "𖣂 wt-1 ⎇ feat/wt-1");
}

#[test]
fn prompt_in_the_primary_checkout() {
    let fixture = Fixture::plain("primary");
    assert_eq!(fixture.prompt(&fixture.primary), "𖣂 main");
}

#[test]
fn two_sessions_in_since_order() {
    let fixture = Fixture::plain("two");
    let wt = fixture.worktree(1);
    let pid = std::process::id();
    fixture.put("b", &wt, "codex", "idle", 200, pid, Some("ash"));
    fixture.put("a", &wt, "claude", "working", 100, pid, Some("birch"));
    assert_eq!(fixture.prompt(&wt), "𖣂 wt-1 ⎇ feat/wt-1 · ✻ birch, ◇ ash ⏸");
}

#[test]
fn a_session_in_another_worktree_is_not_shown() {
    let fixture = Fixture::plain("other");
    fixture.put(
        "a",
        &fixture.worktree(2),
        "claude",
        "working",
        1,
        std::process::id(),
        Some("birch"),
    );
    assert_eq!(fixture.prompt(&fixture.worktree(1)), "𖣂 wt-1 ⎇ feat/wt-1");
}

#[test]
fn a_session_without_a_name_shows_its_agent() {
    let fixture = Fixture::plain("unnamed");
    let wt = fixture.worktree(1);
    let pid = std::process::id();
    fixture.put("a", &wt, "claude", "working", 1, pid, None);
    fixture.put("b", &wt, "codex", "working", 2, pid, None);
    assert_eq!(
        fixture.prompt(&wt),
        "𖣂 wt-1 ⎇ feat/wt-1 · ✻ claude, ◇ codex"
    );
}

#[test]
fn prompt_is_silent_outside_git() {
    let fixture = Fixture::plain("outside");
    let plain = fixture.base.join("plain");
    fs::create_dir_all(&plain).unwrap();
    let output = fixture.run(&["prompt"], &plain);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[test]
fn prompt_is_silent_without_a_config() {
    let fixture = Fixture::plain("bare");
    let bare = fixture.base.join("bare");
    fs::create_dir_all(&bare).unwrap();
    git(&["init", "-q", "-b", "main"], &bare);
    let output = fixture.run(&["prompt"], &bare);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
}

#[test]
fn prompt_survives_a_broken_record() {
    let fixture = Fixture::plain("broken");
    let wt = fixture.worktree(1);
    fs::write(fixture.sessions().join("bad.session"), "not json").unwrap();
    let output = fixture.run(&["prompt"], &wt);
    assert_eq!(output.status.code(), Some(0));
    assert!(output.stderr.is_empty());
    assert_eq!(
        String::from_utf8_lossy(&output.stdout).trim_end(),
        "𖣂 wt-1 ⎇ feat/wt-1"
    );
}

#[test]
fn prompt_json_has_statusline_fields_and_sessions() {
    let fixture = Fixture::plain("json");
    let wt = fixture.worktree(1);
    fixture.put(
        "a",
        &wt,
        "codex",
        "idle",
        7,
        std::process::id(),
        Some("ash"),
    );
    let output = fixture.run(&["prompt", "--json"], &wt);
    let prompt: Value = serde_json::from_slice(&output.stdout).expect("json");
    let status: Value =
        serde_json::from_str(&fixture.statusline(&wt, true)).expect("statusline json");
    for key in status.as_object().unwrap().keys() {
        assert!(prompt.get(key).is_some(), "prompt --json lacks {key}");
    }
    assert_eq!(
        prompt["sessions"],
        json!([{"name": "ash", "agent": "codex", "state": "idle", "since": 7}])
    );
}

fn served(name: &str, port: u16) -> (Fixture, PathBuf) {
    let fixture = Fixture::new(
        name,
        "RAMIFY_SERVICES=\"web\"\nweb_port=17990\nweb_cmd='true'\n",
        1,
    );
    let wt = fixture.worktree(1);
    let directory = fixture.state.join("demo");
    fs::create_dir_all(&directory).unwrap();
    fs::write(
        directory.join("wt-1.env"),
        format!("SLOT=1\nWEB_PORT={port}\n"),
    )
    .unwrap();
    (fixture, wt)
}

#[test]
fn prompt_makes_no_http_request() {
    // A socket that accepts and never answers: a probe would wait for its timeout.
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (fixture, wt) = served("nohttp", port);
    let started = Instant::now();
    fixture.prompt(&wt);
    assert!(
        started.elapsed() < Duration::from_millis(1500),
        "prompt waited on the service"
    );
    drop(listener);
}

#[test]
fn prompt_reads_the_health_cache() {
    let (fixture, wt) = served("cache", dead_port());
    assert_eq!(
        fixture.prompt(&wt),
        "𖣂 wt-1 ⎇ feat/wt-1",
        "no cache, no health"
    );
    assert!(fixture.statusline(&wt, false).ends_with("· stack down"));
    assert_eq!(
        fixture.prompt(&wt),
        "𖣂 wt-1 ⎇ feat/wt-1 · stack down",
        "a fresh cache shows"
    );
    let cache = fixture.state.join("demo/wt-1.health");
    let text = fs::read_to_string(&cache).expect("statusline writes the cache");
    let fields: Vec<&str> = text.split_whitespace().collect();
    fs::write(
        &cache,
        format!("{} {} {}\n", fields[0], fields[1], now() - 61),
    )
    .unwrap();
    assert_eq!(
        fixture.prompt(&wt),
        "𖣂 wt-1 ⎇ feat/wt-1",
        "a stale cache does not show"
    );
}

fn dead_port() -> u16 {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    listener.local_addr().unwrap().port()
}

#[test]
fn prompt_is_fast_with_twenty_worktrees() {
    let fixture = Fixture::new("fast", "RAMIFY_SERVICES=\"\"\n", 20);
    let wt = fixture.worktree(10);
    let pid = std::process::id();
    for index in 1..=20 {
        fixture.put(
            &format!("s{index}"),
            &fixture.worktree(index),
            "claude",
            "idle",
            index as u64,
            pid,
            None,
        );
    }
    fixture.prompt(&wt);
    let mut times: Vec<Duration> = (0..10)
        .map(|_| {
            let started = Instant::now();
            fixture.prompt(&wt);
            started.elapsed()
        })
        .collect();
    times.sort();
    let median = times[5];
    assert!(median <= Duration::from_millis(50), "median {median:?}");
}

fn zsh(fixture: &Fixture, script: &str) -> String {
    let output = Command::new("zsh")
        .args(["-fc", script])
        .env("RAMIFY_BIN", binary())
        .env("RAMIFY_STATE_DIR", &fixture.state)
        .env("RAMIFY_REGISTRY", fixture.base.join("registry"))
        .env("RAMIFY_RUNS_DIR", fixture.base.join("runs"))
        .env_remove("RAMIFY_ROOT")
        .output()
        .unwrap();
    String::from_utf8_lossy(&output.stdout)
        .trim_end()
        .to_string()
}

#[test]
fn zsh_plugin_prints_the_prompt_line() {
    let fixture = Fixture::plain("zsh");
    let plugin = repo_root().join("shell/ramify.plugin.zsh");
    let script = format!(
        "source {}; cd {}; _ramify_precmd; ramify_prompt_info",
        plugin.display(),
        fixture.worktree(1).display()
    );
    assert_eq!(zsh(&fixture, &script), "𖣂 wt-1 ⎇ feat/wt-1");
}

#[test]
fn zsh_plugin_refreshes_on_cd() {
    let fixture = Fixture::plain("zsh-cd");
    let plugin = repo_root().join("shell/ramify.plugin.zsh");
    let script = format!(
        "source {}; cd {}; _ramify_precmd; cd {}; _ramify_precmd; ramify_prompt_info; cd /; _ramify_precmd; print -r -- \"[$(ramify_prompt_info)]\"",
        plugin.display(),
        fixture.worktree(1).display(),
        fixture.worktree(2).display()
    );
    assert_eq!(zsh(&fixture, &script), "𖣂 wt-2 ⎇ feat/wt-2\n[]");
}

#[test]
fn p10k_segment_draws_the_line_and_nothing_outside() {
    let fixture = Fixture::plain("p10k");
    let shell = repo_root().join("shell");
    // A stand-in for powerlevel10k's `p10k segment`.
    let script = format!(
        "p10k() {{ print -r -- \"$*\"; }}; source {}/ramify.plugin.zsh; cd {}; _ramify_precmd; prompt_ramify; cd /; _ramify_precmd; prompt_ramify; print end",
        shell.display(),
        fixture.worktree(1).display()
    );
    assert_eq!(zsh(&fixture, &script), "segment -t 𖣂 wt-1 ⎇ feat/wt-1\nend");
}

#[test]
fn prompt_install_links_the_plugin() {
    let base = temp("install");
    let custom = base.join("custom");
    let zshrc = base.join(".zshrc");
    fs::write(&zshrc, "plugins=(git)\n").unwrap();
    let output = Command::new(binary())
        .args(["prompt", "install"])
        .env("HOME", &base)
        .env("ZSH_CUSTOM", &custom)
        .env("RAMIFY_HOME", repo_root())
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let link = custom.join("plugins/ramify");
    assert_eq!(fs::read_link(&link).unwrap(), repo_root().join("shell"));
    assert!(link.join("ramify.plugin.zsh").is_file());
    assert_eq!(fs::read_to_string(&zshrc).unwrap(), "plugins=(git)\n");
    let text = String::from_utf8_lossy(&output.stdout);
    assert!(
        text.contains("plugins=(") && text.contains("POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS"),
        "{text}"
    );
}

#[test]
fn a_ram_session_shows_as_ram() {
    let fixture = Fixture::plain("ram");
    let wt = fixture.worktree(1);
    fixture.put("ram-1", &wt, "ram", "working", 1, std::process::id(), None);
    assert_eq!(fixture.prompt(&wt), "𖣂 wt-1 ⎇ feat/wt-1 · ◆ ram");
}

#[test]
fn prompt_json_has_no_stack_without_a_fresh_cache() {
    let (fixture, wt) = served("json-stale", dead_port());
    let output = fixture.run(&["prompt", "--json"], &wt);
    let value: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["stack"], Value::Null, "{value}");
}

#[test]
fn a_control_character_in_a_record_is_dropped() {
    let fixture = Fixture::plain("control");
    let wt = fixture.worktree(1);
    fixture.put("a", &wt, "claude", "working", 1, std::process::id(), Some("bir\u{1b}[31mch"));
    let line = fixture.prompt(&wt);
    assert!(!line.contains('\u{1b}'), "{line:?}");
    assert!(line.ends_with("✻ bir[31mch"), "{line:?}");
}

fn install(base: &Path, home: &Path, extra: &[&str]) -> Output {
    Command::new(binary())
        .args(["prompt", "install"])
        .args(extra)
        .env("HOME", base)
        .env("ZSH_CUSTOM", base.join("custom"))
        .env("RAMIFY_HOME", home)
        .output()
        .unwrap()
}

#[test]
fn prompt_install_refuses_a_home_without_the_plugin() {
    let base = temp("install-nohome");
    let output = install(&base, &base.join("nohome"), &[]);
    assert_eq!(output.status.code(), Some(1));
    assert!(fs::symlink_metadata(base.join("custom/plugins/ramify")).is_err(), "a link was made");
}

#[test]
fn prompt_install_keeps_a_foreign_link_without_force() {
    let base = temp("install-foreign");
    let plugins = base.join("custom/plugins");
    fs::create_dir_all(&plugins).unwrap();
    std::os::unix::fs::symlink("/elsewhere", plugins.join("ramify")).unwrap();
    assert_eq!(install(&base, &repo_root(), &[]).status.code(), Some(1));
    assert_eq!(fs::read_link(plugins.join("ramify")).unwrap(), Path::new("/elsewhere"));
    assert_eq!(install(&base, &repo_root(), &["--force"]).status.code(), Some(0));
    assert_eq!(fs::read_link(plugins.join("ramify")).unwrap(), repo_root().join("shell"));
}

#[test]
fn zsh_plugin_escapes_percent_for_any_theme() {
    let fixture = Fixture::plain("percent");
    let wt = fixture.worktree(1);
    git(&["checkout", "-q", "-b", "feat/a%Fb"], &wt);
    let plugin = repo_root().join("shell/ramify.plugin.zsh");
    let script = format!(
        "source {}; cd {}; _ramify_precmd; print -rP -- \"$(ramify_prompt_info)\"",
        plugin.display(),
        wt.display()
    );
    assert!(zsh(&fixture, &script).ends_with("⎇ feat/a%Fb"));
}

#[test]
fn zsh_plugin_skips_an_unbuilt_ramify() {
    let fixture = Fixture::plain("unbuilt");
    let fake = fixture.base.join("fakehome");
    fs::create_dir_all(fake.join("bin")).unwrap();
    fs::create_dir_all(fake.join("cli")).unwrap();
    let wrapper = fake.join("bin/ramify");
    fs::write(&wrapper, "#!/bin/sh\ntouch \"$(dirname \"$0\")/ran\"\necho built\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(&wrapper, fs::Permissions::from_mode(0o755)).unwrap();
    let plugin = repo_root().join("shell/ramify.plugin.zsh");
    let output = Command::new("zsh")
        .args([
            "-fc",
            &format!(
                "source {}; cd {}; _ramify_precmd; print -r -- \"[$(ramify_prompt_info)]\"",
                plugin.display(),
                fixture.worktree(1).display()
            ),
        ])
        .env("RAMIFY_BIN", &wrapper)
        .output()
        .unwrap();
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim_end(), "[]");
    assert!(!fake.join("bin/ran").exists(), "the plugin ran an unbuilt ramify");
}
