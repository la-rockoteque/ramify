//! The read-only search tools: `grep`, `glob` and `code_map`.
//!
//! Every walk starts at the worktree root, honours `.gitignore`, skips `.git`, binary files and
//! symlinks, and keeps only the paths under the folder the model asked for.

use globset::GlobBuilder;
use ignore::WalkBuilder;
use regex::RegexBuilder;
use std::path::{Path, PathBuf};
use tree_sitter::{Language, Node, Parser};

const MAX_MATCHES: usize = 200;
const MAX_LINE: usize = 300;
const MAX_FILES: usize = 500;
const MAX_MAP: usize = 40_000;
/// A NUL byte in this many first bytes marks a file as binary, as git does.
const BINARY_PROBE: usize = 8192;

/// The regular files under `base`, relative to `root`, sorted. `base` is already checked to stay
/// inside `root`.
fn files(root: &Path, base: &Path) -> Vec<PathBuf> {
    if base.is_file() {
        return base
            .strip_prefix(root)
            .map(|p| vec![p.to_path_buf()])
            .unwrap_or_default();
    }
    let canonical_root = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut found: Vec<PathBuf> = WalkBuilder::new(root)
        .hidden(false)
        .parents(false)
        .require_git(false)
        .follow_links(false)
        .filter_entry(|entry| entry.file_name() != ".git")
        .build()
        .flatten()
        .filter(|entry| entry.file_type().is_some_and(|t| t.is_file()))
        .map(|entry| entry.into_path())
        .filter(|path| path.starts_with(base))
        .filter(|path| {
            path.canonicalize()
                .is_ok_and(|real| real.starts_with(&canonical_root))
        })
        .filter_map(|path| path.strip_prefix(root).ok().map(Path::to_path_buf))
        .collect();
    found.sort();
    found
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes[..bytes.len().min(BINARY_PROBE)].contains(&0)
}

/// The text of a file, or `None` when it is binary or unreadable.
fn text_of(path: &Path) -> Option<String> {
    let bytes = std::fs::read(path).ok()?;
    if is_binary(&bytes) {
        return None;
    }
    Some(String::from_utf8_lossy(&bytes).into_owned())
}

fn display(path: &Path) -> String {
    path.to_string_lossy().into_owned()
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
    let filter = match glob {
        Some(glob) => Some(
            GlobBuilder::new(glob)
                .literal_separator(false)
                .build()
                .map_err(|e| format!("bad glob pattern: {e}"))?
                .compile_matcher(),
        ),
        None => None,
    };
    let mut lines = Vec::new();
    for relative in files(root, base) {
        if let Some(filter) = &filter {
            let name = relative.file_name().map(Path::new).unwrap_or(&relative);
            if !filter.is_match(&relative) && !filter.is_match(name) {
                continue;
            }
        }
        let Some(text) = text_of(&root.join(&relative)) else {
            continue;
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
            lines.push(format!("{}:{}: {}", display(&relative), index + 1, cut_line(line)));
        }
    }
    if lines.is_empty() {
        return Ok("no matches".into());
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
    let matcher = GlobBuilder::new(pattern)
        .literal_separator(true)
        .build()
        .map_err(|e| format!("bad glob pattern: {e}"))?
        .compile_matcher();
    let base_relative = base.strip_prefix(root).unwrap_or(Path::new(""));
    let mut lines = Vec::new();
    for relative in files(root, base) {
        let from_base = relative.strip_prefix(base_relative).unwrap_or(&relative);
        if !matcher.is_match(from_base) {
            continue;
        }
        if lines.len() == MAX_FILES {
            lines.push(format!("[cut: more than {MAX_FILES} files; narrow the pattern or path]"));
            break;
        }
        lines.push(display(&relative));
    }
    if lines.is_empty() {
        return Ok("no files".into());
    }
    Ok(lines.join("\n"))
}

pub fn code_map(root: &Path, base: &Path) -> Result<String, String> {
    let mut out = String::new();
    for relative in files(root, base) {
        let Some(text) = text_of(&root.join(&relative)) else {
            continue;
        };
        let block = match language_of(&relative) {
            Some(language) => {
                let mut block = display(&relative);
                for line in outline(language, &text) {
                    block.push('\n');
                    block.push_str(&line);
                }
                block
            }
            None => format!("{} ({} lines)", display(&relative), text.lines().count()),
        };
        let separator = usize::from(!out.is_empty());
        if out.len() + separator + block.len() > MAX_MAP {
            out.push_str(&format!(
                "\n[cut: the outline passed {MAX_MAP} characters; map a smaller path]"
            ));
            break;
        }
        if separator == 1 {
            out.push('\n');
        }
        out.push_str(&block);
    }
    if out.is_empty() {
        return Ok("no files".into());
    }
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
    let mut lines = Vec::new();
    visit(lang, tree.root_node(), text.as_bytes(), 1, &mut lines);
    lines
}

fn visit(lang: Lang, node: Node, source: &[u8], depth: usize, lines: &mut Vec<String>) {
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
    for child in node.named_children(&mut cursor) {
        visit(lang, child, source, child_depth, lines);
    }
}

fn field_text(node: Node, field: &str, source: &[u8]) -> Option<String> {
    let text = node.child_by_field_name(field)?.utf8_text(source).ok()?;
    Some(text.split_whitespace().collect::<Vec<_>>().join(" "))
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
            "class_declaration" | "abstract_class_declaration" | "class" => named("class"),
            "interface_declaration" => named("interface"),
            "type_alias_declaration" => named("type"),
            "enum_declaration" => named("enum"),
            "function_declaration" | "generator_function_declaration" => named("function"),
            "method_definition" | "method_signature" | "abstract_method_signature" => {
                named("method")
            }
            "variable_declarator" => {
                let value = node.child_by_field_name("value")?;
                matches!(
                    value.kind(),
                    "arrow_function" | "function_expression" | "function"
                )
                .then(|| named("function"))
                .flatten()
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
