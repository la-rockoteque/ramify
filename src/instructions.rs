//! The instruction files every task reads: `AGENTS.md`, as Codex reads it, and `CLAUDE.md`, as
//! Claude Code reads it, with its `@path` imports.

use std::path::{Path, PathBuf};
use std::process::Command;

/// Each file is cut here.
pub const FILE_LIMIT: usize = 32 * 1024;
/// All the files together are cut here.
pub const TOTAL_LIMIT: usize = 128 * 1024;
/// How deep imports go below the file that starts them.
const IMPORT_DEPTH: usize = 5;
/// The files of each project folder, in order.
const PROJECT_FILES: [&str; 4] = [
    "AGENTS.md",
    "CLAUDE.md",
    ".claude/CLAUDE.md",
    "CLAUDE.local.md",
];

/// One file read, for `/instructions`.
#[derive(Debug, Clone, PartialEq)]
pub struct Read {
    /// `~/…` for the user's files, else relative to the git top level.
    pub name: String,
    pub bytes: usize,
    /// Imported by another file.
    pub imported: bool,
}

/// What the instruction files hold.
#[derive(Debug, Default)]
pub struct Instructions {
    pub files: Vec<Read>,
    /// One `Instructions from <name>:` section per top file, in order.
    pub sections: Vec<String>,
    /// One line per file that could not be used.
    pub notes: Vec<String>,
}

/// `RAM_CODE_INSTRUCTIONS_HOME`, else `HOME`.
pub fn home() -> PathBuf {
    std::env::var_os("RAM_CODE_INSTRUCTIONS_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .unwrap_or_default()
}

/// The git top level of `root`, or `root` outside git.
fn top_level(root: &Path) -> PathBuf {
    Command::new("git")
        .arg("-C")
        .arg(root)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| PathBuf::from(String::from_utf8_lossy(&output.stdout).trim()))
        .unwrap_or_else(|| root.to_path_buf())
}

/// The folders from the git top level down to `root`.
fn folders(root: &Path) -> Vec<PathBuf> {
    let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let top = top_level(&root)
        .canonicalize()
        .unwrap_or_else(|_| root.clone());
    let mut chain: Vec<PathBuf> = root
        .ancestors()
        .take_while(|folder| folder.starts_with(&top))
        .map(Path::to_path_buf)
        .collect();
    if chain.is_empty() {
        chain.push(root);
    }
    chain.reverse();
    chain
}

struct Reader<'a> {
    home: &'a Path,
    top: PathBuf,
    seen: Vec<PathBuf>,
    out: Instructions,
}

impl Reader<'_> {
    fn name(&self, path: &Path) -> String {
        if let Ok(rest) = path.strip_prefix(self.home) {
            return format!("~/{}", rest.display());
        }
        path.strip_prefix(&self.top)
            .unwrap_or(path)
            .display()
            .to_string()
    }

    /// Reads a file once: None when it is missing, already read, or not UTF-8.
    fn read(&mut self, path: &Path, imported: bool) -> Option<String> {
        let real = path.canonicalize().ok()?;
        if !real.is_file() || self.seen.contains(&real) {
            return None;
        }
        self.seen.push(real);
        let bytes = std::fs::read(path).ok()?;
        let name = self.name(path);
        match String::from_utf8(bytes) {
            Ok(text) => {
                self.out.files.push(Read {
                    name,
                    bytes: text.len(),
                    imported,
                });
                Some(text)
            }
            Err(_) => {
                self.out
                    .notes
                    .push(format!("instructions: skipped {name}: not UTF-8"));
                None
            }
        }
    }

    /// One top file: its text, its imports when it is a `CLAUDE.md`, cut at the file limit.
    fn top_file(&mut self, path: &Path) {
        let Some(text) = self.read(path, false) else {
            return;
        };
        let claude = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("CLAUDE"));
        let text = if claude {
            self.expand(&text, path.parent().unwrap_or(Path::new(".")), 1)
        } else {
            text
        };
        let name = self.name(path);
        self.out.sections.push(format!(
            "Instructions from {name}:\n\n{}",
            cut(&text, FILE_LIMIT, "[cut at 32 KiB]")
        ));
    }

    /// Replaces each `@path` outside code with the text of that file.
    fn expand(&mut self, text: &str, folder: &Path, depth: usize) -> String {
        let mut out = Vec::new();
        let mut fenced = false;
        for line in text.split('\n') {
            if line.trim_start().starts_with("```") || line.trim_start().starts_with("~~~") {
                fenced = !fenced;
                out.push(line.to_string());
                continue;
            }
            if fenced {
                out.push(line.to_string());
                continue;
            }
            // Between backticks is a code span: odd parts stay as they are.
            let parts: Vec<String> = line
                .split('`')
                .enumerate()
                .map(|(index, part)| {
                    if index % 2 == 1 {
                        part.to_string()
                    } else {
                        self.imports(part, folder, depth)
                    }
                })
                .collect();
            out.push(parts.join("`"));
        }
        out.join("\n")
    }

    fn imports(&mut self, part: &str, folder: &Path, depth: usize) -> String {
        let mut out = String::new();
        let mut rest = part;
        while let Some(at) = rest.find('@') {
            let starts_word = at == 0 || rest[..at].ends_with(char::is_whitespace);
            out.push_str(&rest[..at]);
            let after = &rest[at + 1..];
            let end = after.find(char::is_whitespace).unwrap_or(after.len());
            let word = &after[..end];
            let path = word.trim_end_matches(['.', ',', ';', ':', ')', '!', '?']);
            let tail = &word[path.len()..];
            let text = (starts_word && !path.is_empty() && depth <= IMPORT_DEPTH)
                .then(|| self.resolve(path, folder))
                .flatten()
                .and_then(|file| {
                    let text = self.read(&file, true)?;
                    Some(self.expand(&text, file.parent().unwrap_or(Path::new(".")), depth + 1))
                });
            match text {
                Some(text) => {
                    out.push_str(&text);
                    out.push_str(tail);
                }
                None => {
                    out.push('@');
                    out.push_str(word);
                }
            }
            rest = &after[end..];
        }
        out.push_str(rest);
        out
    }

    fn resolve(&self, path: &str, folder: &Path) -> Option<PathBuf> {
        let file = match path.strip_prefix("~/") {
            Some(rest) => self.home.join(rest),
            None if path.starts_with('/') => PathBuf::from(path),
            None => folder.join(path),
        };
        file.is_file().then_some(file)
    }
}

/// Cuts `text` at `limit` bytes, on a character boundary, with `note`.
fn cut(text: &str, limit: usize, note: &str) -> String {
    if text.len() <= limit {
        return text.to_string();
    }
    let mut end = limit;
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}\n{note}", &text[..end])
}

/// Reads the user's files, then the project's, for a task that works in `root`.
pub fn load(root: &Path, home: &Path) -> Instructions {
    let chain = folders(root);
    let mut reader = Reader {
        home,
        top: chain.first().cloned().unwrap_or_else(|| root.to_path_buf()),
        seen: Vec::new(),
        out: Instructions::default(),
    };
    reader.top_file(&home.join(".ram/AGENTS.md"));
    reader.top_file(&home.join(".claude/CLAUDE.md"));
    for folder in &chain {
        for file in PROJECT_FILES {
            reader.top_file(&folder.join(file));
        }
    }
    reader.out
}

impl Instructions {
    /// Ram Code's own prompt, then each file's section, cut at the total limit.
    pub fn system_prompt(&self, base: &str) -> String {
        if self.sections.is_empty() {
            return base.to_string();
        }
        let all = self.sections.join("\n\n");
        format!("{base}\n\n{}", cut(&all, TOTAL_LIMIT, "[cut at 128 KiB]"))
    }

    /// The lines of `/instructions`.
    pub fn listing(&self) -> Vec<String> {
        if self.files.is_empty() {
            return vec!["no AGENTS.md or CLAUDE.md".into()];
        }
        self.files
            .iter()
            .map(|file| {
                let mark = if file.imported { "  ↳ " } else { "" };
                format!("{mark}{} ({})", file.name, size(file.bytes))
            })
            .collect()
    }
}

fn size(bytes: usize) -> String {
    if bytes < 1024 {
        format!("{bytes} B")
    } else {
        format!("{:.1} KiB", bytes as f64 / 1024.0)
    }
}
