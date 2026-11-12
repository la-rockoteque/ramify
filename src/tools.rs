use serde_json::json;
use std::path::{Component, Path, PathBuf};

const MAX_READ: usize = 200_000;
const MAX_LIST: usize = 300;
const MAX_COMMAND: usize = 40_000;

#[derive(Debug, Clone)]
pub struct Tools {
    pub root: PathBuf,
}

impl Tools {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
        }
    }

    pub fn definitions() -> Vec<serde_json::Value> {
        vec![
            json!({
                "name": "read_file",
                "description": "Read a UTF-8 file inside the worktree.",
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Relative path"}},
                    "required": ["path"]
                }
            }),
            json!({
                "name": "write_file",
                "description": "Write a UTF-8 file inside the worktree. Creates parent folders.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Relative path"},
                        "content": {"type": "string", "description": "Full file content"}
                    },
                    "required": ["path", "content"]
                }
            }),
            json!({
                "name": "list_files",
                "description": "List the entries of a folder inside the worktree.",
                "input_schema": {
                    "type": "object",
                    "properties": {"path": {"type": "string", "description": "Relative folder, default '.'"}},
                    "required": []
                }
            }),
            json!({
                "name": "grep",
                "description": "Search file contents in the worktree with a regular expression. \
Skips .gitignore'd, .git and binary files, and files over 10 MB. A path under .git or ignored by \
.gitignore is an error. Returns `path:line: text`, at most 200 matches.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Regular expression (Rust regex syntax)"},
                        "path": {"type": "string", "description": "Relative file or folder, default '.'"},
                        "glob": {"type": "string", "description": "Only files matching this glob. Without '/', it matches file names at any depth, such as '*.rs'. With '/', it matches the path from `path`, such as 'src/**/*.rs'."},
                        "ignore_case": {"type": "boolean", "description": "Match without case, default false"}
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "glob",
                "description": "Find files in the worktree by a glob pattern. A pattern without '/' \
matches file names at any depth, such as '*.rs'. A pattern with '/' matches the path from `path`, \
such as 'src/**/*.rs'. Skips .gitignore'd files and .git. Returns sorted relative paths, at most 500.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "pattern": {"type": "string", "description": "Glob pattern"},
                        "path": {"type": "string", "description": "Relative folder to search from, default '.'. A file is an error."}
                    },
                    "required": ["pattern"]
                }
            }),
            json!({
                "name": "code_map",
                "description": "Outline the code in a file or folder: each class, type, function and \
method with its line, without bodies. Covers Rust, TypeScript, JavaScript, Python and Java; \
other files show their line count. Files over 10 MB are listed as skipped. A path under .git or \
ignored by .gitignore is an error. Use it before reading whole files.",
                "input_schema": {
                    "type": "object",
                    "properties": {
                        "path": {"type": "string", "description": "Relative file or folder, default '.'"}
                    },
                    "required": []
                }
            }),
            json!({
                "name": "run_command",
                "description": "Run a shell command in the worktree root and return its output.",
                "input_schema": {
                    "type": "object",
                    "properties": {"command": {"type": "string", "description": "Shell command"}},
                    "required": ["command"]
                }
            }),
        ]
    }

    pub fn call(&self, name: &str, input: &serde_json::Value) -> Result<String, String> {
        match name {
            "read_file" => {
                let path = self.resolve(required(input, "path")?)?;
                let content = std::fs::read_to_string(&path)
                    .map_err(|e| format!("could not read {}: {e}", path.display()))?;
                Ok(truncate(content, MAX_READ))
            }
            "write_file" => {
                let path = self.resolve(required(input, "path")?)?;
                let content = input
                    .get("content")
                    .and_then(|v| v.as_str())
                    .ok_or("write_file needs a string content")?;
                if let Some(parent) = path.parent() {
                    std::fs::create_dir_all(parent)
                        .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
                }
                std::fs::write(&path, content)
                    .map_err(|e| format!("could not write {}: {e}", path.display()))?;
                Ok(format!("wrote {} bytes to {}", content.len(), path.display()))
            }
            "list_files" => {
                let raw = input.get("path").and_then(|v| v.as_str()).unwrap_or(".");
                let path = self.resolve(raw)?;
                let mut entries: Vec<String> = Vec::new();
                for entry in std::fs::read_dir(&path)
                    .map_err(|e| format!("could not list {}: {e}", path.display()))?
                {
                    let entry = entry.map_err(|e| format!("could not list: {e}"))?;
                    let mut line = entry.file_name().to_string_lossy().into_owned();
                    if entry.path().is_dir() {
                        line.push('/');
                    }
                    entries.push(line);
                }
                entries.sort();
                entries.truncate(MAX_LIST);
                Ok(entries.join("\n"))
            }
            "grep" => {
                let base = self.resolve(optional(input, "path").unwrap_or("."))?;
                crate::search::grep(
                    &self.root,
                    &base,
                    required(input, "pattern")?,
                    optional(input, "glob"),
                    input
                        .get("ignore_case")
                        .and_then(|v| v.as_bool())
                        .unwrap_or(false),
                )
            }
            "glob" => {
                let base = self.resolve(optional(input, "path").unwrap_or("."))?;
                crate::search::glob(&self.root, &base, required(input, "pattern")?)
            }
            "code_map" => {
                let base = self.resolve(optional(input, "path").unwrap_or("."))?;
                crate::search::code_map(&self.root, &base)
            }
            "run_command" => {
                let command = required(input, "command")?;
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&self.root)
                    .env_remove("ANTHROPIC_AUTH_TOKEN")
                    .env_remove("ANTHROPIC_BASE_URL")
                    .env_remove("RAM_CODE_MODEL")
                    .output()
                    .map_err(|e| format!("could not run command: {e}"))?;
                let mut text = String::new();
                text.push_str(&String::from_utf8_lossy(&output.stdout));
                text.push_str(&String::from_utf8_lossy(&output.stderr));
                let code = output.status.code().unwrap_or(-1);
                Ok(truncate(format!("exit {code}\n{text}"), MAX_COMMAND))
            }
            other => Err(format!("unknown tool: {other}")),
        }
    }

    fn resolve(&self, raw: &str) -> Result<PathBuf, String> {
        let path = Path::new(raw);
        if path.is_absolute() {
            return Err("path must be relative to the worktree".into());
        }
        if path
            .components()
            .any(|c| matches!(c, Component::ParentDir | Component::Prefix(_)))
        {
            return Err("path must stay inside the worktree".into());
        }
        let root = self
            .root
            .canonicalize()
            .unwrap_or_else(|_| self.root.clone());
        let joined = self.root.join(path);
        let mut probe = joined.clone();
        let mut suffix: Vec<std::ffi::OsString> = Vec::new();
        loop {
            if let Ok(canonical) = probe.canonicalize() {
                let mut full = canonical;
                for part in suffix.iter().rev() {
                    full.push(part);
                }
                if full.starts_with(&root) {
                    return Ok(joined);
                }
                return Err("path must stay inside the worktree".into());
            }
            match (probe.parent(), probe.file_name()) {
                (Some(parent), Some(name)) => {
                    suffix.push(name.to_os_string());
                    probe = parent.to_path_buf();
                }
                _ => return Err("path must stay inside the worktree".into()),
            }
        }
    }
}

fn required<'a>(input: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string field: {key}"))
}

/// A non-empty string field, or `None`.
fn optional<'a>(input: &'a serde_json::Value, key: &str) -> Option<&'a str> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .filter(|v| !v.trim().is_empty())
}

fn truncate(mut text: String, limit: usize) -> String {
    if text.len() <= limit {
        return text;
    }
    let mut cut = limit;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    text.truncate(cut);
    text.push_str("\n[truncated]");
    text
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn temp(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ram-code-tools-{}-{name}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn truncate_keeps_a_char_boundary() {
        let text = "é".repeat(10);
        let cut = truncate(text.clone(), 3);
        assert!(cut.starts_with("é"));
        assert!(cut.ends_with("[truncated]"));
        assert_eq!(cut.trim_end_matches("\n[truncated]"), "é");
    }

    #[test]
    fn a_symlink_outside_the_root_is_rejected() {
        let outside = temp("outside");
        std::fs::write(outside.join("secret.txt"), "secret").unwrap();
        let root = temp("root");
        symlink(&outside, root.join("link")).unwrap();

        let tools = Tools::new(&root);
        assert!(tools.resolve("link/secret.txt").is_err());
        assert!(tools.call("read_file", &json!({"path": "link/secret.txt"})).is_err());
    }

    #[test]
    fn a_parent_path_is_rejected() {
        let root = temp("parent");
        let tools = Tools::new(&root);
        assert!(tools.resolve("../x").is_err());
        assert!(tools.resolve("/etc/passwd").is_err());
    }
}
