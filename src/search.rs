//! The read-only search tools: `grep`, `glob` and `code_map`.
//!
//! A walk starts at the worktree root, so the root `.gitignore` applies to every folder, but it
//! enters only the folders on the way to the requested path and under it. It honours
//! `.gitignore`, skips `.git`, binary files, files over 10 MB and symlinks.

use globset::{GlobBuilder, GlobMatcher};
use ignore::WalkBuilder;
use regex::RegexBuilder;
use std::fs::File;
use std::io::{BufReader, Read};
use std::path::{Component, Path, PathBuf};
use tree_sitter::{Language, Node, Parser};

const MAX_MATCHES: usize = 200;
const MAX_LINE: usize = 300;
const MAX_FILES: usize = 500;
/// The most characters `code_map` returns, its cut note included.
const MAX_MAP: usize = 40_000;
/// A NUL byte in this many first bytes marks a file as binary, as git does.
const BINARY_PROBE: usize = 8192;
/// A larger file is skipped, so one huge file cannot fill the memory.
const MAX_FILE: u64 = 10 * 1024 * 1024;

/// The worktree and the requested path, both by their real path, so a path typed in another case
/// still matches on a file system that ignores case.
struct Scope {
    root: PathBuf,
    base: PathBuf,
}

impl Scope {
    /// `None` when the requested path does not exist.
    fn new(root: &Path, base: &Path) -> Result<Option<Self>, String> {
        let root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
        let Ok(base) = base.canonicalize() else {
            return Ok(None);
        };
        let relative = base.strip_prefix(&root).unwrap_or(Path::new(""));
        if relative
            .components()
            .any(|c| matches!(c, Component::Normal(name) if name == ".git"))
        {
            return Err(format!("{} is inside .git, which is not searched", relative.display()));
        }
        Ok(Some(Self { root, base }))
    }

    fn relative_base(&self) -> &Path {
        self.base.strip_prefix(&self.root).unwrap_or(Path::new(""))
    }

    /// The regular files under the base, relative to the root, sorted. A base that `.gitignore`
    /// ignores is an error, so a file path cannot reach what a folder walk skips.
    fn files(&self) -> Result<Vec<PathBuf>, String> {
        let base = self.base.clone();
        let mut seen_base = self.base == self.root;
        let mut found = Vec::new();
        let walk = WalkBuilder::new(&self.root)
            .hidden(false)
            .parents(false)
            .require_git(false)
            .follow_links(false)
            .filter_entry(move |entry| {
                let path = entry.path();
                entry.file_name() != ".git" && (path.starts_with(&base) || base.starts_with(path))
            })
            .build();
        for entry in walk.flatten() {
            let path = entry.path();
            if path == self.base {
                seen_base = true;
            }
            if entry.file_type().is_some_and(|t| t.is_file()) && path.starts_with(&self.base) {
                if let Ok(relative) = path.strip_prefix(&self.root) {
                    found.push(relative.to_path_buf());
                }
            }
        }
        if !seen_base {
            return Err(format!(
                "{} is ignored by .gitignore, so it is not searched",
                self.relative_base().display()
            ));
        }
        found.sort();
        Ok(found)
    }
}

enum Content {
    Text(String),
    Binary,
    TooLarge,
    Unreadable,
}

/// The text of a file. It reads the first bytes before the rest, so a binary file costs one
/// small read.
fn read_text(path: &Path) -> Content {
    let Ok(file) = File::open(path) else {
        return Content::Unreadable;
    };
    if file.metadata().map_or(true, |m| m.len() > MAX_FILE) {
        return Content::TooLarge;
    }
    let mut reader = BufReader::new(file);
    let mut bytes = Vec::new();
    if (&mut reader).take(BINARY_PROBE as u64).read_to_end(&mut bytes).is_err() {
        return Content::Unreadable;
    }
    if bytes.contains(&0) {
        return Content::Binary;
    }
    if reader.read_to_end(&mut bytes).is_err() {
        return Content::Unreadable;
    }
    match String::from_utf8(bytes) {
        Ok(text) => Content::Text(text),
        Err(error) => Content::Text(String::from_utf8_lossy(error.as_bytes()).into_owned()),
    }
}

/// The line count of a text file, read in chunks. `None` for a binary or unreadable file.
fn count_lines(path: &Path) -> Option<usize> {
    let mut reader = BufReader::new(File::open(path).ok()?);
    let mut chunk = vec![0; 64 * 1024];
    let (mut lines, mut first, mut last) = (0, true, b'\n');
    loop {
        let read = reader.read(&mut chunk).ok()?;
        if read == 0 {
            break;
        }
        let part = &chunk[..read];
        if first && part[..read.min(BINARY_PROBE)].contains(&0) {
            return None;
        }
        first = false;
        lines += part.iter().filter(|&&b| b == b'\n').count();
        last = part[read - 1];
    }
    Some(lines + usize::from(last != b'\n'))
}

/// One rule for `grep`'s filter and the `glob` tool. A pattern without `/` matches the file name
/// at any depth. A pattern with `/` matches the path from the requested folder.
struct PathFilter {
    matcher: GlobMatcher,
    by_name: bool,
}

impl PathFilter {
    fn new(pattern: &str) -> Result<Self, String> {
        let pattern = pattern.trim().trim_start_matches("./");
        let matcher = GlobBuilder::new(pattern)
            .literal_separator(true)
            .build()
            .map_err(|e| format!("bad glob pattern: {e}"))?
            .compile_matcher();
        Ok(Self {
            matcher,
            by_name: !pattern.contains('/'),
        })
    }

    fn is_match(&self, relative: &Path, base: &Path) -> bool {
        if self.by_name {
            return relative
                .file_name()
                .is_some_and(|name| self.matcher.is_match(Path::new(name)));
        }
        self.matcher
            .is_match(relative.strip_prefix(base).unwrap_or(relative))
    }
}

pub fn grep(
    root: &Path,
    base: &Path,
    pattern: &str,
    glob: Option<&str>,
    ignore_case: bool,
) -> Result<String, String> {
    let regex = RegexBuilder::new(pattern)
        .case_insensitive(ignore_case)
        .build()
        .map_err(|e| format!("bad regex: {e}"))?;
    let filter = glob.map(PathFilter::new).transpose()?;
    let Some(scope) = Scope::new(root, base)? else {
        return Ok("no matches".into());
    };
    let mut lines = Vec::new();
    let mut too_large = Vec::new();
    for relative in scope.files()? {
        if let Some(filter) = &filter {
            if !filter.is_match(&relative, scope.relative_base()) {
                continue;
            }
        }
        let text = match read_text(&scope.root.join(&relative)) {
            Content::Text(text) => text,
            Content::TooLarge => {
                too_large.push(relative.display().to_string());
                continue;
            }
            Content::Binary | Content::Unreadable => continue,
        };
        for (index, line) in text.lines().enumerate() {
            if !regex.is_match(line) {
                continue;
            }
            if lines.len() == MAX_MATCHES {
                lines.push(format!(
                    "[cut: more than {MAX_MATCHES} matches; narrow the pattern, path or glob]"
                ));
                return Ok(lines.join("\n"));
            }
            lines.push(format!("{}:{}: {}", relative.display(), index + 1, cut_line(line)));
        }
    }
    if lines.is_empty() {
        lines.push("no matches".into());
    }
    if !too_large.is_empty() {
        lines.push(format!(
            "[skipped {} file(s) over 10 MB: {}]",
            too_large.len(),
            too_large.join(", ")
        ));
    }
    Ok(lines.join("\n"))
}

fn cut_line(line: &str) -> String {
    if line.chars().count() <= MAX_LINE {
        return line.to_string();
    }
    let kept: String = line.chars().take(MAX_LINE).collect();
    format!("{kept}…[cut]")
}

pub fn glob(root: &Path, base: &Path, pattern: &str) -> Result<String, String> {
    let filter = PathFilter::new(pattern)?;
    let Some(scope) = Scope::new(root, base)? else {
        return Ok("no files".into());
    };
    if scope.base.is_file() {
        return Err(format!(
            "glob needs a folder path; {} is a file",
            scope.relative_base().display()
        ));
    }
    let mut lines = Vec::new();
    for relative in scope.files()? {
        if !filter.is_match(&relative, scope.relative_base()) {
            continue;
        }
        if lines.len() == MAX_FILES {
            lines.push(format!("[cut: more than {MAX_FILES} files; narrow the pattern or path]"));
            break;
        }
        lines.push(relative.display().to_string());
    }
    if lines.is_empty() {
        return Ok("no files".into());
    }
    Ok(lines.join("\n"))
}

pub fn code_map(root: &Path, base: &Path) -> Result<String, String> {
    let Some(scope) = Scope::new(root, base)? else {
        return Ok("no files".into());
    };
    let note = format!("[cut: the outline passed {MAX_MAP} characters; map a smaller path]");
    // Room for the content, so the note still fits under the cap.
    let budget = MAX_MAP - note.chars().count() - 1;
    let mut out = String::new();
    let mut used = 0;
    for relative in scope.files()? {
        let path = scope.root.join(&relative);
        let block: Vec<String> = match language_of(&relative) {
            Some(language) => match read_text(&path) {
                Content::Text(text) => {
                    let mut block = vec![relative.display().to_string()];
                    block.extend(outline(language, &text));
                    block
                }
                Content::TooLarge => vec![format!("{} (skipped: over 10 MB)", relative.display())],
                Content::Binary | Content::Unreadable => continue,
            },
            None => match std::fs::metadata(&path) {
                Ok(meta) if meta.len() > MAX_FILE => {
                    vec![format!("{} (skipped: over 10 MB)", relative.display())]
                }
                _ => match count_lines(&path) {
                    Some(count) => vec![format!("{} ({count} lines)", relative.display())],
                    None => continue,
                },
            },
        };
        let size: usize = block.iter().map(|line| line.chars().count() + 1).sum();
        if used + size > budget {
            // The first file always shows, cut at a line, so one big file still gives an outline.
            if out.is_empty() {
                for line in block {
                    let width = line.chars().count() + 1;
                    if used + width > budget {
                        break;
                    }
                    out.push_str(&line);
                    out.push('\n');
                    used += width;
                }
            }
            out.push_str(&note);
            return Ok(out);
        }
        for line in block {
            out.push_str(&line);
            out.push('\n');
        }
        used += size;
    }
    if out.is_empty() {
        return Ok("no files".into());
    }
    out.pop();
    Ok(out)
}

#[derive(Clone, Copy)]
enum Lang {
    Rust,
    TypeScript,
    Tsx,
    JavaScript,
    Python,
    Java,
}

fn language_of(path: &Path) -> Option<Lang> {
    match path.extension()?.to_str()? {
        "rs" => Some(Lang::Rust),
        "ts" | "mts" | "cts" => Some(Lang::TypeScript),
        "tsx" => Some(Lang::Tsx),
        "js" | "jsx" | "mjs" | "cjs" => Some(Lang::JavaScript),
        "py" => Some(Lang::Python),
        "java" => Some(Lang::Java),
        _ => None,
    }
}

fn grammar(lang: Lang) -> Language {
    match lang {
        Lang::Rust => tree_sitter_rust::LANGUAGE.into(),
        Lang::TypeScript => tree_sitter_typescript::LANGUAGE_TYPESCRIPT.into(),
        Lang::Tsx => tree_sitter_typescript::LANGUAGE_TSX.into(),
        Lang::JavaScript => tree_sitter_javascript::LANGUAGE.into(),
        Lang::Python => tree_sitter_python::LANGUAGE.into(),
        Lang::Java => tree_sitter_java::LANGUAGE.into(),
    }
}

/// One line per symbol: indent, kind, name and line. A parse error keeps what the parser found.
fn outline(lang: Lang, text: &str) -> Vec<String> {
    let mut parser = Parser::new();
    if parser.set_language(&grammar(lang)).is_err() {
        return Vec::new();
    }
    let Some(tree) = parser.parse(text, None) else {
        return Vec::new();
    };
    let source = text.as_bytes();
    let mut lines = Vec::new();
    // An explicit stack, not recursion: a deeply nested expression must not overflow the
    // thread's stack, because a stack overflow ends the whole process.
    let mut stack = vec![(tree.root_node(), 1usize)];
    while let Some((node, depth)) = stack.pop() {
        let child_depth = match symbol(lang, node, source) {
            Some((kind, name)) => {
                lines.push(format!(
                    "{}{kind} {name} :{}",
                    "  ".repeat(depth),
                    node.start_position().row + 1
                ));
                depth + 1
            }
            None => depth,
        };
        let mut cursor = node.walk();
        let children: Vec<Node> = node.named_children(&mut cursor).collect();
        stack.extend(children.into_iter().rev().map(|child| (child, child_depth)));
    }
    lines
}

fn field_text(node: Node, field: &str, source: &[u8]) -> Option<String> {
    let text = node.child_by_field_name(field)?.utf8_text(source).ok()?;
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
}

fn is_function(node: Node) -> bool {
    matches!(
        node.kind(),
        "arrow_function" | "function_expression" | "function" | "generator_function"
    )
}

/// `export default class {}` and `export default function () {}` have no name.
fn default_export(node: Node) -> bool {
    node.parent().is_some_and(|parent| parent.kind() == "export_statement")
}

fn symbol(lang: Lang, node: Node, source: &[u8]) -> Option<(&'static str, String)> {
    let named = |kind: &'static str| field_text(node, "name", source).map(|name| (kind, name));
    match lang {
        Lang::Rust => match node.kind() {
            "struct_item" => named("struct"),
            "enum_item" => named("enum"),
            "union_item" => named("union"),
            "trait_item" => named("trait"),
            "mod_item" => named("mod"),
            "type_item" => named("type"),
            "const_item" => named("const"),
            "static_item" => named("static"),
            "macro_definition" => named("macro"),
            "function_item" | "function_signature_item" => named("fn"),
            "impl_item" => {
                let target = field_text(node, "type", source)?;
                Some(match field_text(node, "trait", source) {
                    Some(t) => ("impl", format!("{t} for {target}")),
                    None => ("impl", target),
                })
            }
            _ => None,
        },
        Lang::TypeScript | Lang::Tsx | Lang::JavaScript => match node.kind() {
            "class_declaration" | "abstract_class_declaration" | "class" => named("class")
                .or_else(|| default_export(node).then(|| ("class", "default".to_string()))),
            "interface_declaration" => named("interface"),
            "type_alias_declaration" => named("type"),
            "enum_declaration" => named("enum"),
            "function_declaration" | "generator_function_declaration" => named("function"),
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                named("method")
            }
            // A class field that holds a function, such as `handle = () => {}`.
            "public_field_definition" | "field_definition" => {
                let value = node.child_by_field_name("value")?;
                if !is_function(value) {
                    return None;
                }
                field_text(node, "name", source)
                    .or_else(|| field_text(node, "property", source))
                    .map(|name| ("method", name))
            }
            "variable_declarator" => {
                let value = node.child_by_field_name("value")?;
                is_function(value).then(|| named("function")).flatten()
            }
            kind if default_export(node) && is_function(node) && kind != "arrow_function" => {
                Some(("function", "default".to_string()))
            }
            _ => None,
        },
        Lang::Python => match node.kind() {
            "class_definition" => named("class"),
            "function_definition" => named("def"),
            _ => None,
        },
        Lang::Java => match node.kind() {
            "class_declaration" => named("class"),
            "interface_declaration" => named("interface"),
            "enum_declaration" => named("enum"),
            "record_declaration" => named("record"),
            "annotation_type_declaration" => named("annotation"),
            "method_declaration" => named("method"),
            "constructor_declaration" => named("constructor"),
            _ => None,
        },
    }
}
