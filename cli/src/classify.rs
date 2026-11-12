//! `ramify classify`: the workflow actions a tool call does, for the PreWorkflow and
//! PostWorkflow hooks. One classifier serves Ram Code, Claude Code and Codex.

use crate::guard::{shell_groups, split_shell};
use serde_json::{json, Map, Value};
use std::path::Path;

/// Reads `{tool_name, tool_input, cwd}` on stdin and prints the actions as a JSON array.
pub fn classify_command() -> i32 {
    let mut input = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut input);
    let value: Value = serde_json::from_str(&input).unwrap_or(Value::Null);
    println!("{}", Value::Array(classify(&value)));
    0
}

/// The actions of one tool call: `{"action": …, "details": {…}}`, in the order they run.
pub fn classify(call: &Value) -> Vec<Value> {
    let tool = call.get("tool_name").and_then(Value::as_str).unwrap_or("");
    let input = call.get("tool_input").cloned().unwrap_or(Value::Null);
    let text = |key: &str| input.get(key).and_then(Value::as_str).unwrap_or("").to_string();
    match tool {
        "run_command" | "Bash" | "exec_command" => commands(&text("command")),
        "read_file" | "Read" => file_action("workitem.read", &first(&[text("file_path"), text("path")]), call),
        "write_file" | "Write" | "Edit" | "MultiEdit" => {
            file_action("workitem.update", &first(&[text("file_path"), text("path")]), call)
        }
        _ => Vec::new(),
    }
}

fn first(values: &[String]) -> String {
    values.iter().find(|value| !value.is_empty()).cloned().unwrap_or_default()
}

/// A file under the work directory is a work item: `.ramify/work/**`, or the `work_dir` of the
/// repository's workflows (`docs/stories/*` and the like).
fn file_action(action: &str, path: &str, call: &Value) -> Vec<Value> {
    if path.is_empty() {
        return Vec::new();
    }
    let cwd = call.get("cwd").and_then(Value::as_str).unwrap_or(".");
    let full = Path::new(cwd).join(path);
    let root = git_root(&full).unwrap_or_else(|| Path::new(cwd).to_path_buf());
    let relative = full.strip_prefix(&root).unwrap_or(Path::new(path)).to_string_lossy().into_owned();
    let mut dirs = vec![".ramify/work".to_string()];
    if let Some(dir) = work_dir(&root) {
        dirs.push(dir);
    }
    let inside = dirs.iter().any(|dir| {
        let pattern = regex::escape(dir.trim_end_matches('/')).replace(r"\*", "[^/]+");
        regex::Regex::new(&format!("^{pattern}/")).is_ok_and(|re| re.is_match(&relative))
    });
    if !inside {
        return Vec::new();
    }
    vec![json!({"action": action, "details": {"work_item": relative}})]
}

fn git_root(path: &Path) -> Option<std::path::PathBuf> {
    let mut current = if path.is_dir() { path.to_path_buf() } else { path.parent()?.to_path_buf() };
    loop {
        if current.join(".git").exists() {
            return Some(current);
        }
        if !current.pop() {
            return None;
        }
    }
}

fn work_dir(root: &Path) -> Option<String> {
    let text = std::fs::read_to_string(root.join(".ramify/workflows.yml")).ok()?;
    let value: serde_yaml_ng::Value = serde_yaml_ng::from_str(&text).ok()?;
    value.get("work_dir")?.as_str().map(str::to_string)
}

/// Each command of a shell line, read through `cd DIR &&`, `env X=Y`, `sudo`, `xargs`,
/// `bash -c "…"` and a full path to the program.
fn commands(line: &str) -> Vec<Value> {
    let Ok(tokens) = split_shell(line) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    for group in shell_groups(&tokens) {
        let mut words: Vec<String> = group;
        while let Some(raw) = words.first().cloned() {
            match program(&raw) {
                // A wrapper: skip it and its flags, then read the command it runs.
                "env" => drop_wrapper(&mut words, &["-u", "--unset", "-C", "--chdir", "-S", "--split-string"]),
                "sudo" => drop_wrapper(&mut words, &["-u", "-g", "-h", "-p", "-C", "-D", "-R", "-T", "-U"]),
                "xargs" => drop_wrapper(&mut words, &["-I", "-L", "-n", "-P", "-s", "-d", "-E", "-a"]),
                "command" | "exec" | "time" | "nohup" | "nice" => drop_wrapper(&mut words, &["-n"]),
                "bash" | "sh" | "zsh" => {
                    // `bash -c`, `bash -lc`, `bash -l -c`: the word after the flags is the line.
                    let flags: Vec<&String> = words[1..].iter().take_while(|word| word.starts_with('-')).collect();
                    let runs = flags.iter().any(|flag| !flag.starts_with("--") && flag.contains('c'));
                    if runs {
                        let inner = words.get(1 + flags.len()).cloned().unwrap_or_default();
                        actions.extend(commands(&inner));
                    }
                    words.clear();
                }
                _ if raw.contains('=') && !raw.starts_with('-') => {
                    words.remove(0);
                }
                _ => break,
            }
        }
        if let Some(action) = command(&words) {
            actions.push(action);
        }
    }
    actions
}

/// Removes a wrapper program, its flags (with the value of each flag in `valued`) and the
/// `NAME=value` words after it.
fn drop_wrapper(words: &mut Vec<String>, valued: &[&str]) {
    let mut index = skip_flags(words, 1, valued);
    while words.get(index).is_some_and(|word| word.contains('=') && !word.starts_with('-')) {
        index += 1;
    }
    words.drain(..index.min(words.len()));
}

/// The index of the first word from `start` on that is not a flag. The flags in `valued` take
/// the next word as their value; `--flag=value` is one word.
fn skip_flags(words: &[String], start: usize, valued: &[&str]) -> usize {
    let mut index = start;
    while let Some(word) = words.get(index) {
        if word == "--" {
            return index + 1;
        }
        if !word.starts_with('-') {
            break;
        }
        index += if valued.contains(&word.as_str()) { 2 } else { 1 };
    }
    index
}

fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Positional words after `skip` words, without flags or the values of the flags in `valued`.
fn positionals<'a>(words: &'a [String], skip: usize, valued: &[&str]) -> Vec<&'a str> {
    let mut out = Vec::new();
    let mut index = skip;
    while let Some(word) = words.get(index) {
        if valued.contains(&word.as_str()) {
            index += 2;
            continue;
        }
        if !word.starts_with('-') {
            out.push(word.as_str());
        }
        index += 1;
    }
    out
}

/// The first positional word from `skip` on, past the value of each flag in `valued`.
fn target<'a>(words: &'a [String], skip: usize, valued: &[&str]) -> Option<&'a str> {
    let mut index = skip;
    while index < words.len() {
        let word = words[index].as_str();
        if valued.contains(&word) {
            index += 2;
        } else if word.starts_with('-') {
            index += 1;
        } else {
            return Some(word);
        }
    }
    None
}

fn flag_value<'a>(words: &'a [String], names: &[&str]) -> Option<&'a str> {
    words.iter().enumerate().find_map(|(index, word)| {
        names.iter().find_map(|name| {
            if word == name {
                words.get(index + 1).map(String::as_str)
            } else {
                word.strip_prefix(&format!("{name}="))
            }
        })
    })
}

/// The flags of `ramify step` that take a value.
const STEP_VALUED: &[&str] = &["--note", "--file"];

fn action(name: &str, details: Map<String, Value>) -> Option<Value> {
    Some(json!({"action": name, "details": Value::Object(details)}))
}

fn number(text: &str) -> Value {
    let digits = text.trim_start_matches('#');
    digits.parse::<u64>().map(Value::from).unwrap_or_else(|_| Value::String(text.to_string()))
}

fn command(words: &[String]) -> Option<Value> {
    let first = program(words.first()?);
    let word = |index: usize| words.get(index).map(String::as_str).unwrap_or("");
    let mut details = Map::new();
    match first {
        "ramify" => match (word(1), word(2)) {
            ("step", "") => action("workitem.read", details),
            ("step", "done" | "na") => {
                if let Some(step) = positionals(words, 3, STEP_VALUED).first() {
                    details.insert("step".into(), json!(step));
                }
                action("workitem.update", details)
            }
            ("step", "ask") => {
                if let Some(step) = positionals(words, 3, STEP_VALUED).first() {
                    details.insert("step".into(), json!(step));
                }
                action("gate.ask", details)
            }
            ("step", "wait") => {
                if let Some(step) = positionals(words, 3, STEP_VALUED).first() {
                    details.insert("step".into(), json!(step));
                }
                action("gate.wait", details)
            }
            ("lane", _) | ("ticket", "start" | "sync") => action("workitem.update", details),
            ("ticket", "") => action("workitem.read", details),
            ("new", _) => {
                if let Some(slug) = positionals(words, 2, &[]).first() {
                    details.insert("branch".into(), json!(slug));
                }
                action("branch.create", details)
            }
            ("up", _) => action("stack.up", details),
            ("down", _) => action("stack.down", details),
            _ => None,
        },
        "gh" => {
            // `gh -R owner/repo pr merge`, `gh --repo=owner/repo pr merge`: skip the global flags.
            let rest: Vec<String> = words[skip_flags(words, 1, &["-R", "--repo"]).min(words.len())..].to_vec();
            let sub = |index: usize| rest.get(index).map(String::as_str).unwrap_or("");
            match (sub(0), sub(1)) {
                ("pr", verb @ ("create" | "ready" | "merge" | "review" | "comment")) => {
                    // The flags of each verb that take a value: the word after one is not the PR.
                    let valued: &[&str] = match verb {
                        "merge" => &["--subject", "-t", "--body", "-b", "--body-file", "-F", "--match-head-commit", "--author-email", "-A"],
                        "review" | "comment" => &["--body", "-b", "--body-file", "-F"],
                        _ => &[],
                    };
                    if verb != "create" {
                        if let Some(pr) = target(&rest, 2, valued) {
                            details.insert("pr".into(), number(pr));
                        }
                    }
                    match verb {
                        "create" => {
                            let draft = rest.iter().any(|w| w == "--draft" || w == "-d")
                                || flag_value(&rest, &["--draft"]).is_some_and(|value| value != "false");
                            details.insert("draft".into(), json!(draft));
                            if let Some(base) = flag_value(&rest, &["--base", "-B"]) {
                                details.insert("base".into(), json!(base));
                            }
                            action("pr.open", details)
                        }
                        "ready" => action("pr.open", details),
                        "merge" => action("pr.merge", details),
                        _ => action("pr.review", details),
                    }
                }
                ("issue", verb @ ("view" | "edit" | "comment" | "close")) => {
                    if let Some(item) = positionals(&rest, 2, &["--body", "-b", "--body-file", "-F", "--add-label", "--remove-label", "--title", "-t", "--reason", "-r", "--comment", "-c"]).first() {
                        details.insert("work_item".into(), json!(item));
                    }
                    action(if verb == "view" { "workitem.read" } else { "workitem.update" }, details)
                }
                _ => None,
            }
        }
        "acli" if word(1) == "jira" && word(2) == "workitem" => {
            let verb = word(3);
            if let Some(key) = flag_value(words, &["--key", "-k"]).or_else(|| positionals(words, 4, &["--key", "-k", "--status", "-s", "--body", "-b"]).first().copied()) {
                details.insert("work_item".into(), json!(key));
            }
            match verb {
                "view" => action("workitem.read", details),
                "edit" | "transition" | "comment" => action("workitem.update", details),
                _ => None,
            }
        }
        "git" => {
            // `git -C dir push`, `git --no-pager push`, `git --git-dir=x push`: skip the global
            // flags.
            let index = skip_flags(words, 1, &["-C", "-c", "--git-dir", "--work-tree", "--namespace"]);
            let rest = &words[index.min(words.len())..];
            let sub = rest.first().map(String::as_str).unwrap_or("");
            match sub {
                "commit" => action("commit", details),
                "push" => {
                    let names = positionals(rest, 1, &["-o", "--push-option", "--repo", "--receive-pack", "--exec"]);
                    if let Some(branch) = names.get(1) {
                        details.insert("branch".into(), json!(branch));
                    }
                    action("push", details)
                }
                "worktree" if rest.get(1).map(String::as_str) == Some("add") => {
                    if let Some(branch) = flag_value(rest, &["-b", "-B"]) {
                        details.insert("branch".into(), json!(branch));
                    }
                    action("branch.create", details)
                }
                "switch" | "checkout" if rest.iter().any(|w| w == "-c" || w == "-b" || w == "-C" || w == "-B") => {
                    if let Some(branch) = flag_value(rest, &["-c", "-b", "-C", "-B"]) {
                        details.insert("branch".into(), json!(branch));
                    }
                    action("branch.create", details)
                }
                _ => None,
            }
        }
        "cargo" | "go" | "dotnet" => match word(1) {
            "test" => action("test.run", details),
            "build" => action("build.run", details),
            _ => None,
        },
        "npm" | "pnpm" | "yarn" => {
            let script = if word(1) == "run" { word(2) } else { word(1) };
            if script == "test" || script.starts_with("test:") || script.starts_with("test-") {
                action("test.run", details)
            } else if script == "build" || script.starts_with("build:") {
                action("build.run", details)
            } else {
                None
            }
        }
        "pytest" => action("test.run", details),
        "python" | "python3" if word(1) == "-m" && word(2) == "pytest" => action("test.run", details),
        _ => None,
    }
}
