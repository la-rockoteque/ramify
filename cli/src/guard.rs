//! Which tool calls a pending human gate lets through: reads, planning edits, and anything aimed
//! outside the gated worktree. A port of `libexec/native_guard.py`.

use crate::steps::{normalize, realpath, relpath};
use serde_json::{Map, Value};
use std::path::{Path, PathBuf};

const EDIT_TOOLS: &[&str] = &["write", "edit", "multiedit", "apply_patch"];
const COMMAND_TOOLS: &[&str] = &["bash", "exec_command", "exec"];
const READ_PROGRAMS: &[&str] = &["cat", "rg", "ls", "pwd", "head", "tail", "wc", "stat"];
const GIT_READS: &[&str] = &["status", "diff", "log", "show", "rev-parse", "ls-files", "check-ignore"];
const RAMIFY_READS: &[&str] = &["step", "workflow", "gates", "approvals", "card", "config", "status", "dash"];
const SHELLS: &[&str] = &["bash", "sh", "zsh"];
const PUNCTUATION: &str = ";&|()";

fn join(base: &str, path: &str) -> PathBuf {
    Path::new(base).join(path)
}

fn inside(path: &Path, root: &str) -> bool {
    realpath(path).starts_with(realpath(Path::new(root)))
}

fn tool(payload: &Value) -> String {
    let name = payload.get("tool_name").and_then(Value::as_str).unwrap_or("");
    name.rsplit('.').next().unwrap_or("").to_lowercase()
}

fn tool_values(payload: &Value) -> Map<String, Value> {
    match payload.get("tool_input") {
        Some(Value::Object(map)) => map.clone(),
        None | Some(Value::Null) => Map::new(),
        Some(Value::String(text)) if text.is_empty() => Map::new(),
        Some(other) => {
            let mut map = Map::new();
            map.insert("patch".into(), other.clone());
            map
        }
    }
}

fn first_string(values: &Map<String, Value>, keys: &[&str]) -> Option<String> {
    keys.iter()
        .find_map(|key| values.get(*key).and_then(Value::as_str).filter(|text| !text.is_empty()))
        .map(str::to_string)
}

fn edit_target(tool: &str, path: &Option<String>) -> bool {
    path.is_some() && EDIT_TOOLS.contains(&tool)
}

/// Whether the tool call lands in the gated worktree. A gate holds its own worktree only.
pub fn targets_root(payload: &Value, root: &str) -> bool {
    let values = tool_values(payload);
    let base = payload
        .get("cwd")
        .and_then(Value::as_str)
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or(root)
        .to_string();
    let path = first_string(&values, &["file_path", "path"]);
    let tool = tool(payload);
    if edit_target(&tool, &path) {
        return inside(&join(&base, path.as_deref().unwrap_or("")), root);
    }
    let Some(command) = first_string(&values, &["command", "cmd"]) else {
        return true;
    };
    if command.contains(&*realpath(Path::new(root)).to_string_lossy()) || command.contains(root) {
        return true;
    }
    match split_shell(&command) {
        Ok(tokens) => inside(Path::new(&command_dir(&tokens, &base)), root),
        Err(()) => true,
    }
}

/// The directory a command runs in: the session's, moved by each leading `cd DIR`.
fn command_dir(tokens: &[String], cwd: &str) -> String {
    let groups = shell_groups(tokens);
    let mut cwd = PathBuf::from(cwd);
    for group in groups.iter().take(groups.len().saturating_sub(1)) {
        if group[0] != "cd" || group.len() != 2 {
            break;
        }
        cwd = cwd.join(expand_user(&group[1]));
    }
    cwd.to_string_lossy().into_owned()
}

fn expand_user(path: &str) -> String {
    match (path.strip_prefix('~'), std::env::var("HOME")) {
        (Some(rest), Ok(home)) if rest.is_empty() || rest.starts_with('/') => format!("{home}{rest}"),
        _ => path.to_string(),
    }
}

/// `shlex.shlex(command, posix=True, punctuation_chars=";&|()")` with `whitespace_split`: words
/// split on blanks, quotes join, and a run of `;&|()` is a token of its own.
pub(crate) fn split_shell(command: &str) -> Result<Vec<String>, ()> {
    let mut tokens = Vec::new();
    let mut chars = command.chars().peekable();
    let mut current = String::new();
    let mut started = false;
    let flush = |current: &mut String, started: &mut bool, tokens: &mut Vec<String>| {
        if *started {
            tokens.push(std::mem::take(current));
            *started = false;
        }
    };
    while let Some(c) = chars.next() {
        match c {
            // A line break ends a command, as `;` does. native_guard.py read it as a blank, so
            // `cat x` and a write on the next line passed as one read.
            '\n' => {
                flush(&mut current, &mut started, &mut tokens);
                tokens.push(";".into());
            }
            c if c.is_whitespace() => flush(&mut current, &mut started, &mut tokens),
            '#' if !started => {
                // The line break stays: it ends the command.
                while chars.peek().is_some_and(|next| *next != '\n') {
                    chars.next();
                }
            }
            '\'' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('\'') => break,
                        Some(next) => current.push(next),
                        None => return Err(()),
                    }
                }
            }
            '"' => {
                started = true;
                loop {
                    match chars.next() {
                        Some('"') => break,
                        Some('\\') => match chars.next() {
                            Some(next) if "\\\"$`\n".contains(next) => current.push(next),
                            Some(next) => {
                                current.push('\\');
                                current.push(next);
                            }
                            None => return Err(()),
                        },
                        Some(next) => current.push(next),
                        None => return Err(()),
                    }
                }
            }
            '\\' => {
                started = true;
                match chars.next() {
                    Some(next) => current.push(next),
                    None => return Err(()),
                }
            }
            c if PUNCTUATION.contains(c) => {
                flush(&mut current, &mut started, &mut tokens);
                let mut run = String::from(c);
                while let Some(next) = chars.peek().copied().filter(|next| PUNCTUATION.contains(*next)) {
                    run.push(next);
                    chars.next();
                }
                tokens.push(run);
            }
            c => {
                started = true;
                current.push(c);
            }
        }
    }
    flush(&mut current, &mut started, &mut tokens);
    Ok(tokens)
}

pub(crate) fn shell_groups(tokens: &[String]) -> Vec<Vec<String>> {
    let mut groups = Vec::new();
    let mut current = Vec::new();
    for token in tokens {
        if !token.is_empty() && token.chars().all(|c| PUNCTUATION.contains(c)) {
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        } else {
            current.push(token.clone());
        }
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Python's `fnmatch.fnmatchcase`: `*` crosses `/`.
fn fnmatch(name: &str, pattern: &str) -> bool {
    let mut expression = String::from("(?s)^");
    let chars: Vec<char> = pattern.chars().collect();
    let mut index = 0;
    while index < chars.len() {
        let c = chars[index];
        index += 1;
        match c {
            '*' => expression.push_str(".*"),
            '?' => expression.push('.'),
            '[' => {
                let mut end = index;
                if end < chars.len() && chars[end] == '!' {
                    end += 1;
                }
                if end < chars.len() && chars[end] == ']' {
                    end += 1;
                }
                while end < chars.len() && chars[end] != ']' {
                    end += 1;
                }
                if end >= chars.len() {
                    expression.push_str("\\[");
                } else {
                    let inner: String = chars[index..end].iter().collect();
                    let inner = inner.replace('\\', "\\\\");
                    let inner = match inner.strip_prefix('!') {
                        Some(rest) => format!("^{rest}"),
                        None => inner,
                    };
                    expression.push_str(&format!("[{inner}]"));
                    index = end + 1;
                }
            }
            other => expression.push_str(&regex::escape(&other.to_string())),
        }
    }
    expression.push('$');
    regex::Regex::new(&expression).is_ok_and(|re| re.is_match(name))
}

fn planning_path(path: &str, root: &str, patterns: &[String]) -> bool {
    let full = realpath(&join(root, path));
    if !full.starts_with(realpath(Path::new(root))) {
        return false;
    }
    let relative = relpath(&full, &normalize(Path::new(root)));
    patterns.iter().any(|pattern| fnmatch(&relative, pattern))
}

fn staged_planning(root: &str, patterns: &[String]) -> bool {
    let Ok(output) = std::process::Command::new("git")
        .args(["-C", root, "diff", "--cached", "--name-only", "-z"])
        .output()
    else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .all(|path| planning_path(path, root, patterns))
}

fn read_command(tokens: &[String]) -> bool {
    if tokens[0] == "git" {
        return git_read(tokens);
    }
    if !READ_PROGRAMS.contains(&tokens[0].as_str()) {
        return false;
    }
    !tokens.iter().any(|token| token.starts_with("--pre"))
}

fn git_read(tokens: &[String]) -> bool {
    if tokens.len() < 2 || !GIT_READS.contains(&tokens[1].as_str()) {
        return false;
    }
    !tokens
        .iter()
        .any(|token| ["--output", "--ext-diff", "--textconv"].iter().any(|flag| token.starts_with(flag)))
}

/// Output thrown away or merged writes nothing: `2>&1`, `>/dev/null`, `2>/dev/null`. The regex
/// crate has no lookaround, so this walks `(?<![<>])\d?>(?:&\d|\s*/dev/null)(?![\w/])` by hand.
fn drop_harmless_redirects(command: &str) -> String {
    let chars: Vec<char> = command.chars().collect();
    let mut out = String::new();
    let mut index = 0;
    while index < chars.len() {
        match harmless_at(&chars, index) {
            Some(end) => {
                out.push(' ');
                index = end;
            }
            None => {
                out.push(chars[index]);
                index += 1;
            }
        }
    }
    out
}

fn harmless_at(chars: &[char], start: usize) -> Option<usize> {
    if start > 0 && (chars[start - 1] == '<' || chars[start - 1] == '>') {
        return None;
    }
    let arrow = if chars[start].is_ascii_digit() && chars.get(start + 1) == Some(&'>') {
        start + 1
    } else if chars[start] == '>' {
        start
    } else {
        return None;
    };
    let mut end = arrow + 1;
    if chars.get(end) == Some(&'&') && chars.get(end + 1).is_some_and(char::is_ascii_digit) {
        end += 2;
    } else {
        while chars.get(end).is_some_and(|c| c.is_whitespace()) {
            end += 1;
        }
        let target: Vec<char> = "/dev/null".chars().collect();
        if chars.get(end..end + target.len()) != Some(&target[..]) {
            return None;
        }
        end += target.len();
    }
    if chars.get(end).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == '/') {
        return None;
    }
    Some(end)
}

fn shell_planning(command: &str, root: &str, patterns: &[String]) -> bool {
    let command = drop_harmless_redirects(command);
    if ["$(", "`", ">", "<<"].iter().any(|part| command.contains(part)) {
        return false;
    }
    let Ok(tokens) = split_shell(&command) else {
        return false;
    };
    let groups = shell_groups(&tokens);
    !groups.is_empty() && groups.iter().all(|group| shell_group(group, root, patterns))
}

fn git_planning(tokens: &[String], root: &str, patterns: &[String]) -> bool {
    if tokens.len() < 2 {
        return false;
    }
    match tokens[1].as_str() {
        "commit" => staged_planning(root, patterns),
        "add" => {
            let paths: Vec<&String> = tokens[2..].iter().filter(|token| !token.starts_with('-')).collect();
            !paths.is_empty() && paths.iter().all(|path| planning_path(path, root, patterns))
        }
        _ => git_read(tokens),
    }
}

fn basename(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

fn shell_group(tokens: &[String], root: &str, patterns: &[String]) -> bool {
    let mut tokens = tokens.to_vec();
    let mut executable = basename(&tokens[0]).to_string();
    if SHELLS.contains(&executable.as_str()) && tokens.len() > 1 && basename(&tokens[1]) == "ramify" {
        // bash /path/to/bin/ramify step ask review
        tokens.remove(0);
        executable = "ramify".into();
    }
    let mut rest = vec![executable.clone()];
    rest.extend(tokens[1..].iter().cloned());
    match executable.as_str() {
        "ramify" => tokens.len() > 1 && RAMIFY_READS.contains(&tokens[1].as_str()),
        "cd" => tokens.len() <= 2,
        "git" => git_planning(&rest, root, patterns),
        _ => read_command(&rest),
    }
}

fn patch_paths(patch: &str) -> Vec<String> {
    let mut paths = Vec::new();
    for prefix in ["*** Update File: ", "*** Add File: ", "*** Delete File: "] {
        paths.extend(patch.lines().filter_map(|line| line.strip_prefix(prefix)).map(str::to_string));
    }
    paths.extend(patch.lines().filter_map(|line| line.strip_prefix("*** Move to: ")).map(str::to_string));
    paths.retain(|path| !path.is_empty());
    paths
}

/// Whether the call only reads or edits planning paths.
pub fn planning_tool(payload: &Value, root: &str, patterns: &[String]) -> bool {
    let tool = tool(payload);
    let values = tool_values(payload);
    let path = first_string(&values, &["file_path", "path"]);
    if edit_target(&tool, &path) {
        return planning_path(path.as_deref().unwrap_or(""), root, patterns);
    }
    if let Some(patch) = first_string(&values, &["patch", "input"]).filter(|patch| patch.contains("*** Begin Patch")) {
        let paths = patch_paths(&patch);
        return !paths.is_empty() && paths.iter().all(|path| planning_path(path, root, patterns));
    }
    if let Some(command) = first_string(&values, &["command", "cmd"]) {
        return shell_planning(&command, root, patterns);
    }
    !EDIT_TOOLS.contains(&tool.as_str()) && !COMMAND_TOOLS.contains(&tool.as_str())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_like_shlex_with_punctuation() {
        let tokens = split_shell("cd /a && git status; echo 'x y'|wc").unwrap();
        assert_eq!(tokens, ["cd", "/a", "&&", "git", "status", ";", "echo", "x y", "|", "wc"]);
        assert!(split_shell("echo 'open").is_err());
        let tokens = split_shell("cat x # read\ntouch src/x").unwrap();
        assert_eq!(tokens, ["cat", "x", ";", "touch", "src/x"]);
    }

    #[test]
    fn drops_only_harmless_redirects() {
        assert_eq!(drop_harmless_redirects("ls 2>&1"), "ls  ");
        assert_eq!(drop_harmless_redirects("ls >/dev/null"), "ls  ");
        assert_eq!(drop_harmless_redirects("ls > out"), "ls > out");
        assert_eq!(drop_harmless_redirects("ls >/dev/nullx"), "ls >/dev/nullx");
    }

    #[test]
    fn fnmatch_star_crosses_slashes() {
        assert!(fnmatch("docs/a/b.md", "docs/**"));
        assert!(!fnmatch("src/a.rs", "docs/**"));
        assert!(fnmatch(".ramify/work/x/spec.md", ".ramify/**"));
    }
}
