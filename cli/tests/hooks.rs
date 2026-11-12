//! `ramify classify` and the workflow bridge for Claude Code and Codex.

use serde_json::{json, Value};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

fn binary() -> &'static str {
    env!("CARGO_BIN_EXE_ramify")
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .to_path_buf()
}

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramify-hooks-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn piped(mut command: Command, input: &Value) -> String {
    let mut child = command
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(input.to_string().as_bytes())
        .unwrap();
    String::from_utf8_lossy(&child.wait_with_output().unwrap().stdout)
        .trim()
        .to_string()
}

fn classify(tool: &str, input: Value, cwd: &Path) -> Value {
    let mut command = Command::new(binary());
    command.arg("classify");
    let out = piped(
        command,
        &json!({"tool_name": tool, "tool_input": input, "cwd": cwd}),
    );
    serde_json::from_str(&out).unwrap_or(Value::Null)
}

fn actions(tool: &str, command: &str) -> Vec<String> {
    classify(tool, json!({"command": command}), Path::new("/"))
        .as_array()
        .unwrap()
        .iter()
        .map(|found| found["action"].as_str().unwrap().to_string())
        .collect()
}

#[test]
fn classify_each_action() {
    let table = [
        ("ramify step", "workitem.read"),
        ("ramify ticket", "workitem.read"),
        ("gh issue view 12", "workitem.read"),
        ("acli jira workitem view MFX-1", "workitem.read"),
        ("ramify step done build", "workitem.update"),
        ("ramify step na audit --note x", "workitem.update"),
        ("ramify lane standard", "workitem.update"),
        ("ramify ticket start", "workitem.update"),
        ("ramify ticket sync", "workitem.update"),
        ("gh issue edit 12 --add-label x", "workitem.update"),
        ("gh issue comment 12 --body hi", "workitem.update"),
        ("gh issue close 12", "workitem.update"),
        (
            "acli jira workitem transition --key MFX-1 --status Done",
            "workitem.update",
        ),
        (
            "acli jira workitem comment MFX-1 --body hi",
            "workitem.update",
        ),
        ("ramify step ask spec", "gate.ask"),
        ("ramify step wait spec", "gate.wait"),
        ("ramify new tm-1", "branch.create"),
        ("git worktree add ../wt -b feat/x", "branch.create"),
        ("git switch -c feat/x", "branch.create"),
        ("git checkout -b feat/x", "branch.create"),
        ("git commit -m x", "commit"),
        ("git push -u origin HEAD", "push"),
        ("gh pr create --title x", "pr.open"),
        ("gh pr ready 50", "pr.open"),
        ("gh pr review 50 --approve", "pr.review"),
        ("gh pr comment 50 --body ok", "pr.review"),
        ("gh pr merge 50 --squash", "pr.merge"),
        ("cargo test", "test.run"),
        ("npm test", "test.run"),
        ("npm run test:unit", "test.run"),
        ("pnpm test", "test.run"),
        ("yarn test", "test.run"),
        ("pytest -q", "test.run"),
        ("go test ./...", "test.run"),
        ("dotnet test", "test.run"),
        ("cargo build --release", "build.run"),
        ("npm run build", "build.run"),
        ("pnpm build", "build.run"),
        ("yarn build", "build.run"),
        ("go build ./...", "build.run"),
        ("dotnet build", "build.run"),
        ("ramify up", "stack.up"),
        ("ramify down", "stack.down"),
        ("/opt/bin/ramify step done build", "workitem.update"),
        ("env CI=1 gh pr merge 1", "pr.merge"),
        ("bash -c 'git push'", "push"),
        ("bash -lc 'git push'", "push"),
        ("/bin/sh -l -c 'gh pr merge 3'", "pr.merge"),
        ("gh --repo=o/r pr merge 5", "pr.merge"),
        ("gh -R o/r pr merge 5", "pr.merge"),
        ("git --no-pager push", "push"),
        ("git --git-dir=x push", "push"),
        ("git -C wt push", "push"),
        ("env -u FOO git push", "push"),
        ("sudo git push", "push"),
        ("sudo -u me git push", "push"),
        ("xargs git push", "push"),
        ("A=/x/y git commit -m x", "commit"),
    ];
    for (command, expected) in table {
        assert_eq!(actions("run_command", command), [expected], "{command}");
        assert_eq!(
            actions("Bash", command),
            [expected],
            "{command} as Claude Code's Bash"
        );
    }
    for command in ["ls -la", "git status", "gh pr view 50", "echo git push"] {
        assert!(actions("run_command", command).is_empty(), "{command}");
    }
}

#[test]
fn classify_a_chain() {
    let found = classify(
        "run_command",
        json!({"command": "cd wt && git commit -m x && git push origin HEAD"}),
        Path::new("/"),
    );
    assert_eq!(
        found,
        json!([{"action": "commit", "details": {}}, {"action": "push", "details": {"branch": "HEAD"}}])
    );
}

#[test]
fn classify_pr_details() {
    let details = |command: &str| classify("run_command", json!({"command": command}), Path::new("/"))[0]["details"].clone();
    assert_eq!(details("gh pr create --draft --title x --body y"), json!({"draft": true}));
    assert_eq!(details("gh pr create --draft=true --base dev"), json!({"draft": true, "base": "dev"}));
    assert_eq!(details("git push -o ci.skip origin main"), json!({"branch": "main"}));
    assert_eq!(details("ramify step done --note x build"), json!({"step": "build"}));
    let merge = classify(
        "run_command",
        json!({"command": "gh pr merge --subject 'a b' 50 --squash"}),
        Path::new("/"),
    );
    assert_eq!(merge[0]["details"], json!({"pr": 50}));
}

#[test]
fn classify_a_work_item_read() {
    let repo = temp("workitem");
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&repo)
        .status()
        .unwrap()
        .success());
    std::fs::create_dir_all(repo.join(".ramify/work/x")).unwrap();
    std::fs::write(repo.join(".ramify/work/x/spec.md"), "spec").unwrap();
    for tool in ["read_file", "Read"] {
        let key = if tool == "Read" { "file_path" } else { "path" };
        let found = classify(tool, json!({key: ".ramify/work/x/spec.md"}), &repo);
        assert_eq!(
            found,
            json!([{"action": "workitem.read", "details": {"work_item": ".ramify/work/x/spec.md"}}]),
            "{tool}"
        );
    }
    let found = classify(
        "write_file",
        json!({"path": ".ramify/work/x/spec.md"}),
        &repo,
    );
    assert_eq!(found[0]["action"], "workitem.update");
    assert_eq!(
        classify("read_file", json!({"path": "src/main.rs"}), &repo),
        json!([])
    );
}

/// A copy of `hooks/ramify-hook` whose `../bin/ramify` is the binary under test: the repo's
/// `bin/ramify` prefers a release build, which can be older.
fn hook_kit(home: &Path) -> PathBuf {
    let kit = home.join("kit");
    std::fs::create_dir_all(kit.join("hooks")).unwrap();
    std::fs::create_dir_all(kit.join("bin")).unwrap();
    std::fs::copy(repo_root().join("hooks/ramify-hook"), kit.join("hooks/ramify-hook")).unwrap();
    let _ = std::fs::remove_file(kit.join("bin/ramify"));
    std::os::unix::fs::symlink(binary(), kit.join("bin/ramify")).unwrap();
    kit.join("hooks/ramify-hook")
}

fn bridge(home: &Path, phase: &str, input: &Value) -> String {
    let mut command = Command::new("bash");
    command
        .arg(hook_kit(home))
        .arg(format!("workflow-{phase}"))
        .env("RAMIFY_HOOKS_HOME", home)
        .env("RAMIFY_STATE_DIR", home.join("state"));
    piped(command, input)
}

fn ramify(home: &Path, args: &[&str], input: &Value) -> String {
    let mut command = Command::new(binary());
    command
        .args(args)
        .env("RAMIFY_HOOKS_HOME", home)
        .env("RAMIFY_STATE_DIR", home.join("state"));
    piped(command, input)
}

fn git_project(name: &str) -> PathBuf {
    let project = temp(name);
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&project)
        .status()
        .unwrap()
        .success());
    project
}

const DENY: &str = r#"echo '{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no PR on Fridays"}}'"#;

fn deny_pr(path: &Path) {
    std::fs::create_dir_all(path.parent().unwrap()).unwrap();
    let hooks = json!({"hooks": {"PreWorkflow": [{"matcher": "pr.open", "hooks": [{"type": "command", "command": DENY}]}]}});
    std::fs::write(path, hooks.to_string()).unwrap();
}

fn pr_create(session: &str, project: &Path) -> Value {
    json!({
        "session_id": session, "cwd": project, "hook_event_name": "PreToolUse",
        "tool_name": "Bash", "tool_input": {"command": "gh pr create --title x"},
    })
}

#[test]
fn bridge_denies_for_claude_code() {
    let home = temp("bridge-home");
    let project = git_project("bridge-project");
    deny_pr(&home.join(".ram/settings.json"));
    let out: Value = serde_json::from_str(&bridge(&home, "pre", &pr_create("s1", &project))).expect("a decision");
    assert_eq!(out["hookSpecificOutput"]["hookEventName"], "PreToolUse");
    assert_eq!(out["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        out["hookSpecificOutput"]["permissionDecisionReason"],
        "no PR on Fridays"
    );
    let other = json!({"session_id": "s1", "cwd": project, "tool_name": "Bash", "tool_input": {"command": "ls"}});
    assert_eq!(bridge(&home, "pre", &other), "");
}

#[test]
fn bridge_without_workflow_hooks_says_nothing() {
    let home = temp("bridge-empty");
    let input = json!({"session_id": "s1", "cwd": "/", "tool_name": "Bash", "tool_input": {"command": "gh pr merge 1"}});
    assert_eq!(bridge(&home, "pre", &input), "");
    assert_eq!(bridge(&home, "post", &input), "");
}

#[test]
fn bridge_runs_project_hooks_only_when_trusted() {
    let home = temp("trust-home");
    let project = git_project("trust-project");
    deny_pr(&project.join(".ram/settings.json"));
    assert_eq!(bridge(&home, "pre", &pr_create("s2", &project)), "");
    let said = ramify(&home, &["hooks", "trust", project.to_str().unwrap()], &Value::Null);
    assert!(said.ends_with(": trusted"), "{said}");
    assert!(bridge(&home, "pre", &pr_create("s2", &project)).contains("no PR on Fridays"));
    ramify(&home, &["hooks", "untrust", project.to_str().unwrap()], &Value::Null);
    assert_eq!(bridge(&home, "pre", &pr_create("s2", &project)), "");
}

#[test]
fn bridge_reads_the_hooks_of_session_start() {
    let home = temp("snapshot-home");
    let project = git_project("snapshot-project");
    let start = json!({"session_id": "s3", "cwd": project, "hook_event_name": "SessionStart"});
    ramify(&home, &["hooks", "snapshot"], &start);
    assert!(home.join("state/sessions/s3.hooks.json").is_file());
    // A hook file written after the session started does not run in it.
    deny_pr(&home.join(".ram/settings.json"));
    assert_eq!(bridge(&home, "pre", &pr_create("s3", &project)), "");
    // A session with no snapshot reads the files as they are.
    assert!(bridge(&home, "pre", &pr_create("s4", &project)).contains("no PR on Fridays"));
}
