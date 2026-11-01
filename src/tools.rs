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
            "run_command" => {
                let command = required(input, "command")?;
                let output = std::process::Command::new("sh")
                    .arg("-c")
                    .arg(command)
                    .current_dir(&self.root)
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
        Ok(self.root.join(path))
    }
}

fn required<'a>(input: &'a serde_json::Value, key: &str) -> Result<&'a str, String> {
    input
        .get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| format!("missing string field: {key}"))
}

fn truncate(mut text: String, limit: usize) -> String {
    if text.len() > limit {
        text.truncate(limit);
        text.push_str("\n[truncated]");
    }
    text
}
