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

/// Each command of a shell line, read through `cd DIR &&`, `env X=Y`, `bash -c "…"` and a full
/// path to the program.
fn commands(line: &str) -> Vec<Value> {
    let Ok(tokens) = split_shell(line) else {
        return Vec::new();
    };
    let mut actions = Vec::new();
    for group in shell_groups(&tokens) {
        let mut words: Vec<String> = group;
        loop {
            match words.first().map(String::as_str) {
                Some("env") => {
                    words.remove(0);
                    while words.first().is_some_and(|word| word.contains('=') || word.starts_with('-')) {
                        words.remove(0);
                    }
                }
                Some(word) if word.contains('=') && !word.starts_with('-') => {
                    words.remove(0);
                }
                Some("bash" | "sh" | "zsh") if words.get(1).map(String::as_str) == Some("-c") => {
                    let inner = words.get(2).cloned().unwrap_or_default();
                    actions.extend(commands(&inner));
                    words.clear();
                }
                Some("command" | "exec" | "time" | "nohup") => {
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

fn program(word: &str) -> &str {
    word.rsplit('/').next().unwrap_or(word)
}

/// Positional words after `skip` words, without flags or their values.
fn positionals(words: &[String], skip: usize) -> Vec<&str> {
    words.iter().skip(skip).map(String::as_str).filter(|word| !word.starts_with('-')).collect()
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
                if let Some(step) = positionals(words, 3).first() {
                    details.insert("step".into(), json!(step));
                }
                action("workitem.update", details)
            }
            ("step", "ask") => {
                if let Some(step) = positionals(words, 3).first() {
                    details.insert("step".into(), json!(step));
                }
                action("gate.ask", details)
            }
            ("step", "wait") => {
                if let Some(step) = positionals(words, 3).first() {
                    details.insert("step".into(), json!(step));
                }
                action("gate.wait", details)
            }
            ("lane", _) | ("ticket", "start" | "sync") => action("workitem.update", details),
            ("ticket", "") => action("workitem.read", details),
            ("new", _) => {
                if let Some(slug) = positionals(words, 2).first() {
                    details.insert("branch".into(), json!(slug));
                }
                action("branch.create", details)
            }
            ("up", _) => action("stack.up", details),
            ("down", _) => action("stack.down", details),
            _ => None,
        },
        "gh" => {
            // `gh -R owner/repo pr merge`: skip the global flags.
            let rest: Vec<String> = {
                let mut out = Vec::new();
                let mut index = 1;
                while index < words.len() {
                    let item = &words[index];
                    if (item == "-R" || item == "--repo") && out.is_empty() {
                        index += 2;
                        continue;
                    }
                    out.push(item.clone());
                    index += 1;
                }
                out
            };
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
                            details.insert("draft".into(), json!(rest.iter().any(|w| w == "--draft" || w == "-d")));
                            if let Some(base) = flag_value(&rest, &["--base", "-B"]) {
                                details.insert("branch".into(), json!(base));
                            }
                            action("pr.open", details)
                        }
                        "ready" => action("pr.open", details),
                        "merge" => action("pr.merge", details),
                        _ => action("pr.review", details),
                    }
                }
                ("issue", verb @ ("view" | "edit" | "comment" | "close")) => {
                    if let Some(item) = positionals(&rest, 2).first() {
                        details.insert("work_item".into(), json!(item));
                    }
                    action(if verb == "view" { "workitem.read" } else { "workitem.update" }, details)
                }
                _ => None,
            }
        }
        "acli" if word(1) == "jira" && word(2) == "workitem" => {
            let verb = word(3);
            if let Some(key) = flag_value(words, &["--key", "-k"]).or_else(|| positionals(words, 4).first().copied()) {
                details.insert("work_item".into(), json!(key));
            }
            match verb {
                "view" => action("workitem.read", details),
                "edit" | "transition" | "comment" => action("workitem.update", details),
                _ => None,
            }
        }
        "git" => {
            // `git -C dir push`: skip the global flags.
            let mut index = 1;
            while word(index) == "-C" || word(index) == "-c" {
                index += 2;
            }
            let rest = &words[index.min(words.len())..];
            let sub = rest.first().map(String::as_str).unwrap_or("");
            match sub {
                "commit" => action("commit", details),
                "push" => {
                    let names = positionals(rest, 1);
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
