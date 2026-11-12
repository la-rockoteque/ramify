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
    /// What the limits did to the file: `cut at 32 KiB`, `cut at 128 KiB` or `left out: over 128 KiB`.
    pub limit: Option<&'static str>,
}

/// What the instruction files hold.
#[derive(Debug, Default)]
pub struct Instructions {
    pub files: Vec<Read>,
    /// One `Instructions from <name>:` section per top file, in order, inside the total limit.
    pub sections: Vec<String>,
    /// One line per file or import that could not be used.
    pub notes: Vec<String>,
}

/// `RAM_CODE_INSTRUCTIONS_HOME`, else `HOME`. None when neither is set.
pub fn home() -> Option<PathBuf> {
    std::env::var_os("RAM_CODE_INSTRUCTIONS_HOME")
        .or_else(|| std::env::var_os("HOME"))
        .filter(|home| !home.is_empty())
        .map(PathBuf::from)
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

/// Where a file and its imports may read: a project file stays inside the project, so a cloned
/// repository cannot send the user's other files to the model.
#[derive(Clone, Copy)]
enum Scope<'a> {
    User,
    Project(&'a Path),
}

struct Reader<'a> {
    home: Option<&'a Path>,
    top: PathBuf,
    seen: Vec<PathBuf>,
    out: Instructions,
    /// The files of each section: its top file, then its imports.
    section_files: Vec<std::ops::Range<usize>>,
}

impl Reader<'_> {
    fn name(&self, path: &Path) -> String {
        if let Ok(rest) = path.strip_prefix(&self.top) {
            return rest.display().to_string();
        }
        if let Some(rest) = self.home.and_then(|home| path.strip_prefix(home).ok()) {
            return format!("~/{}", rest.display());
        }
        path.display().to_string()
    }

    /// Reads a file once, cut at the file limit: None when it is missing, already read, outside
    /// its scope, or not UTF-8.
    fn read(&mut self, path: &Path, imported: bool, scope: Scope) -> Option<String> {
        let real = path.canonicalize().ok()?;
        if !real.is_file() || self.seen.contains(&real) {
            return None;
        }
        let name = self.name(path);
        if let Scope::Project(top) = scope {
            if !real.starts_with(top) {
                self.out.notes.push(format!(
                    "instructions: skipped {name}: it is outside the project"
                ));
                return None;
            }
        }
        self.seen.push(real);
        let bytes = std::fs::read(path).ok()?;
        match String::from_utf8(bytes) {
            Ok(text) => {
                let limit = (text.len() > FILE_LIMIT).then_some("cut at 32 KiB");
                self.out.files.push(Read {
                    name,
                    bytes: text.len(),
                    imported,
                    limit,
                });
                Some(cut(&text, FILE_LIMIT, "[cut at 32 KiB]"))
            }
            Err(_) => {
                self.out
                    .notes
                    .push(format!("instructions: skipped {name}: not UTF-8"));
                None
            }
        }
    }

    /// One top file: its text, with its imports when it is a `CLAUDE.md`.
    fn top_file(&mut self, path: &Path, scope: Scope) {
        let first = self.out.files.len();
        let Some(text) = self.read(path, false, scope) else {
            return;
        };
        let claude = path
            .file_name()
            .is_some_and(|name| name.to_string_lossy().starts_with("CLAUDE"));
        let text = if claude {
            self.expand(&text, path.parent().unwrap_or(Path::new(".")), 1, scope)
        } else {
            text
        };
        let name = self.name(path);
        self.out
            .sections
            .push(format!("Instructions from {name}:\n\n{text}"));
        self.section_files.push(first..self.out.files.len());
    }

    /// Replaces each `@path` outside code with the text of that file.
    // ponytail: line-based markdown; a fence must close with the same marker, a code span ends at
    // the next backtick, and `\ ` escapes in paths are not read. A real parser if users need them.
    fn expand(&mut self, text: &str, folder: &Path, depth: usize, scope: Scope) -> String {
        let mut out = Vec::new();
        let mut fence: Option<&str> = None;
        for line in text.split('\n') {
            let trimmed = line.trim_start();
            let marker = ["```", "~~~"]
                .into_iter()
                .find(|marker| trimmed.starts_with(marker));
            match (fence, marker) {
                (None, Some(marker)) => fence = Some(marker),
                (Some(open), Some(marker)) if open == marker => fence = None,
                _ => {}
            }
            if fence.is_some() || marker.is_some() {
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
                        self.imports(part, folder, depth, scope)
                    }
                })
                .collect();
            out.push(parts.join("`"));
        }
        out.join("\n")
    }

    fn imports(&mut self, part: &str, folder: &Path, depth: usize, scope: Scope) -> String {
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
                .then(|| self.resolve(path, folder, scope))
                .flatten()
                .and_then(|file| {
                    let text = self.read(&file, true, scope)?;
                    Some(self.expand(
                        &text,
                        file.parent().unwrap_or(Path::new(".")),
                        depth + 1,
                        scope,
                    ))
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

    /// The file an import names. A project file may name only a path relative to itself; `~/`
    /// and absolute paths are for the user's own files.
    fn resolve(&mut self, path: &str, folder: &Path, scope: Scope) -> Option<PathBuf> {
        let outside = path.starts_with("~/") || path.starts_with('/');
        if outside && matches!(scope, Scope::Project(_)) {
            self.out
                .notes
                .push(format!("instructions: skipped the import @{path}: a project file imports only from the project"));
            return None;
        }
        let file = match path.strip_prefix("~/") {
            Some(rest) => self.home?.join(rest),
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
pub fn load(root: &Path, home: Option<&Path>) -> Instructions {
    let chain = folders(root);
    let top = chain.first().cloned().unwrap_or_else(|| root.to_path_buf());
    let mut reader = Reader {
        home,
        top: top.clone(),
        seen: Vec::new(),
        out: Instructions::default(),
        section_files: Vec::new(),
    };
    if let Some(home) = home {
        reader.top_file(&home.join(".ram/AGENTS.md"), Scope::User);
        reader.top_file(&home.join(".claude/CLAUDE.md"), Scope::User);
    }
    for folder in &chain {
        for file in PROJECT_FILES {
            reader.top_file(&folder.join(file), Scope::Project(&top));
        }
    }
    reader.fit();
    reader.out
}

impl Reader<'_> {
    /// Keeps the sections inside the total limit, in order. The section that crosses it is cut,
    /// and the later ones are left out; their files say so in `/instructions`.
    // ponytail: the nearest files go first when the total is over; they are the most specific,
    // but the user's own files are small. Drop from the far end if that ever bites.
    fn fit(&mut self) {
        let mut left = TOTAL_LIMIT;
        let mut kept = Vec::new();
        for (section, files) in self.out.sections.drain(..).zip(self.section_files.drain(..)) {
            let size = section.len() + 2;
            if size <= left {
                left -= size;
                kept.push(section);
                continue;
            }
            let top = &mut self.out.files[files.start];
            const NOTE: &str = "[cut at 128 KiB]";
            if left > 64 {
                // The note and its line break count inside the limit.
                kept.push(cut(&section, left - 2 - NOTE.len() - 1, NOTE));
                top.limit = Some("cut at 128 KiB");
                left = 0;
            } else {
                top.limit = Some("left out: over 128 KiB");
                self.out.notes.push(format!("instructions: left out {}: over 128 KiB", top.name));
            }
        }
        self.out.sections = kept;
    }
}

impl Instructions {
    /// Ram Code's own prompt, then each file's section.
    pub fn system_prompt(&self, base: &str) -> String {
        if self.sections.is_empty() {
            return base.to_string();
        }
        format!("{base}\n\n{}", self.sections.join("\n\n"))
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
                let limit = file.limit.map(|limit| format!(", {limit}")).unwrap_or_default();
                format!("{mark}{} ({}{limit})", file.name, size(file.bytes))
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
