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
    let open = classify(
        "run_command",
        json!({"command": "gh pr create --draft --title x --body y"}),
        Path::new("/"),
    );
    assert_eq!(open[0]["details"], json!({"draft": true}));
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

fn bridge(home: &Path, phase: &str, input: &Value) -> String {
    let mut command = Command::new("bash");
    command
        .arg(repo_root().join("hooks/ramify-hook"))
        .arg(format!("workflow-{phase}"))
        .env("RAMIFY_HOOKS_HOME", home);
    piped(command, input)
}

#[test]
fn bridge_denies_for_claude_code() {
    let home = temp("bridge-home");
    let project = temp("bridge-project");
    assert!(Command::new("git")
        .args(["init", "-q"])
        .current_dir(&project)
        .status()
        .unwrap()
        .success());
    std::fs::create_dir_all(home.join(".ram")).unwrap();
    let deny = r#"echo '{"hookSpecificOutput":{"permissionDecision":"deny","permissionDecisionReason":"no PR on Fridays"}}'"#;
    let hooks = json!({"hooks": {"PreWorkflow": [{"matcher": "pr.open", "hooks": [{"type": "command", "command": deny}]}]}});
    std::fs::write(home.join(".ram/settings.json"), hooks.to_string()).unwrap();
    let input = json!({
        "session_id": "s1", "cwd": project, "hook_event_name": "PreToolUse",
        "tool_name": "Bash", "tool_input": {"command": "gh pr create --title x"},
    });
    let out: Value = serde_json::from_str(&bridge(&home, "pre", &input)).expect("a decision");
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
