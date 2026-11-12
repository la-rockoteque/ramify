//! The cases of `.ramify/work/ram-code-search-tools/qa-plan.md` that the build settles.

use ram_code::agent::{run_task, Llm};
use ram_code::openai::to_chat;
use ram_code::tools::Tools;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::symlink;
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-search-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn write(root: &Path, path: &str, text: &str) {
    let full = root.join(path);
    fs::create_dir_all(full.parent().unwrap()).unwrap();
    fs::write(full, text).unwrap();
}

fn call(root: &Path, name: &str, input: Value) -> Result<String, String> {
    Tools::new(root).call(name, &input)
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn grep_skips_ignored_files() {
    let root = tmp("q1");
    write(&root, ".gitignore", "target/\n");
    write(&root, "src/a.rs", "fn one() {}\nlet needle = 1;\n");
    write(&root, "target/b.rs", "let needle = 2;\n");
    let out = call(&root, "grep", json!({"pattern": "needle"})).unwrap();
    assert_eq!(out, "src/a.rs:2: let needle = 1;");
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn grep_skips_the_git_folder() {
    let root = tmp("q2");
    write(&root, ".git/config", "needle\n");
    write(&root, "a.txt", "needle\n");
    let out = call(&root, "grep", json!({"pattern": "needle"})).unwrap();
    assert_eq!(out, "a.txt:1: needle");
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn grep_filters_by_glob_and_ignores_case() {
    let root = tmp("q3");
    write(&root, "a.rs", "struct Foo;\n");
    write(&root, "b.txt", "Foo\n");
    let out = call(
        &root,
        "grep",
        json!({"pattern": "foo", "glob": "*.rs", "ignore_case": true}),
    )
    .unwrap();
    assert_eq!(out, "a.rs:1: struct Foo;");
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn grep_caps_at_200_matches() {
    let root = tmp("q4");
    write(&root, "many.txt", &"hit\n".repeat(250));
    let out = call(&root, "grep", json!({"pattern": "hit"})).unwrap();
    let matches = out.lines().filter(|l| l.starts_with("many.txt:")).count();
    assert_eq!(matches, 200);
    assert!(out.contains("[cut"), "no cut note:\n{}", out.lines().last().unwrap());
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn grep_cuts_a_long_line() {
    let root = tmp("q5");
    write(&root, "long.txt", &format!("needle{}\n", "x".repeat(994)));
    let out = call(&root, "grep", json!({"pattern": "needle"})).unwrap();
    let text = out.strip_prefix("long.txt:1: ").unwrap();
    assert!(text.ends_with("…[cut]"), "{text}");
    assert_eq!(text.trim_end_matches("…[cut]").chars().count(), 300);
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn grep_rejects_a_bad_regex() {
    let root = tmp("q6");
    write(&root, "a.txt", "x\n");
    let error = call(&root, "grep", json!({"pattern": "("})).unwrap_err();
    assert!(error.contains("regex"), "{error}");
    assert!(error.contains("unclosed"), "the parser's message is not quoted: {error}");
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[test]
fn search_tools_stay_in_the_worktree() {
    let outside = tmp("q7-outside");
    write(&outside, "secret.rs", "fn secret() {}\n");
    let root = tmp("q7");
    symlink(&outside, root.join("link")).unwrap();
    for path in ["../", "/etc", "link", "link/secret.rs"] {
        for (tool, input) in [
            ("grep", json!({"pattern": "secret", "path": path})),
            ("glob", json!({"pattern": "*", "path": path})),
            ("code_map", json!({"path": path})),
        ] {
            let error = call(&root, tool, input).expect_err(&format!("{tool} accepted {path}"));
            assert!(error.contains("worktree"), "{tool} {path}: {error}");
        }
    }
}

// ── Q8 ───────────────────────────────────────────────────────────────────────

#[test]
fn walks_do_not_follow_a_symlink_out() {
    let outside = tmp("q8-outside");
    write(&outside, "secret.rs", "fn secret() {}\n");
    let root = tmp("q8");
    write(&root, "inside.rs", "fn inside() {}\n");
    symlink(&outside, root.join("link")).unwrap();
    symlink(outside.join("secret.rs"), root.join("file-link.rs")).unwrap();
    let grep = call(&root, "grep", json!({"pattern": "secret"})).unwrap();
    assert_eq!(grep, "no matches");
    let glob = call(&root, "glob", json!({"pattern": "**/*.rs"})).unwrap();
    assert_eq!(glob, "inside.rs");
    let map = call(&root, "code_map", json!({})).unwrap();
    assert!(!map.contains("secret"), "{map}");
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[test]
fn glob_finds_unignored_files_sorted() {
    let root = tmp("q9");
    write(&root, ".gitignore", "src/gen/\n");
    write(&root, "src/main.rs", "");
    write(&root, "src/b/lib.rs", "");
    write(&root, "src/a.rs", "");
    write(&root, "src/gen/out.rs", "");
    write(&root, "src/notes.md", "");
    write(&root, "build.rs", "");
    let out = call(&root, "glob", json!({"pattern": "src/**/*.rs"})).unwrap();
    assert_eq!(out, "src/a.rs\nsrc/b/lib.rs\nsrc/main.rs");
}

// ── Q10 ──────────────────────────────────────────────────────────────────────

#[test]
fn glob_reports_no_files_and_bad_patterns() {
    let root = tmp("q10");
    write(&root, "a.txt", "");
    assert_eq!(call(&root, "glob", json!({"pattern": "*.rs"})).unwrap(), "no files");
    let error = call(&root, "glob", json!({"pattern": "["})).unwrap_err();
    assert!(error.contains("pattern"), "{error}");
}

// ── Q11 ──────────────────────────────────────────────────────────────────────

#[test]
fn code_map_outlines_rust() {
    let root = tmp("q11");
    write(
        &root,
        "src/geo.rs",
        "pub struct Point {\n    x: i32,\n}\n\nimpl Point {\n    pub fn new() -> Self {\n        Point { x: 0 }\n    }\n\n    fn x(&self) -> i32 {\n        self.x\n    }\n}\n\nfn helper() {}\n",
    );
    let out = call(&root, "code_map", json!({"path": "src/geo.rs"})).unwrap();
    let expected = "src/geo.rs\n  struct Point :1\n  impl Point :5\n    fn new :6\n    fn x :10\n  fn helper :15";
    assert_eq!(out, expected);
}

// ── Q12 ──────────────────────────────────────────────────────────────────────

#[test]
fn code_map_covers_each_language() {
    let root = tmp("q12");
    write(&root, "a.ts", "export class Cart {\n  total(): number { return 0; }\n}\nfunction pay() {}\n");
    write(&root, "b.js", "class Shop {\n  open() {}\n}\nfunction greet() {}\n");
    write(&root, "c.py", "class Store:\n    def sell(self):\n        pass\n\ndef main():\n    pass\n");
    write(&root, "D.java", "public class Bank {\n  public void deposit() {}\n}\n");
    write(&root, "README.md", "# Title\n\ntext\n");
    let out = call(&root, "code_map", json!({})).unwrap();
    for line in [
        "  class Cart :1",
        "    method total :2",
        "  function pay :4",
        "  class Shop :1",
        "    method open :2",
        "  function greet :4",
        "  class Store :1",
        "    def sell :2",
        "  def main :5",
        "  class Bank :1",
        "    method deposit :2",
        "README.md (3 lines)",
    ] {
        assert!(out.lines().any(|l| l == line), "missing {line:?} in:\n{out}");
    }
}

// ── Q13 ──────────────────────────────────────────────────────────────────────

#[test]
fn code_map_caps_its_output() {
    let root = tmp("q13");
    let body: String = (0..400).map(|i| format!("fn function_number_{i}() {{}}\n")).collect();
    for i in 0..10 {
        write(&root, &format!("f{i:02}.rs"), &body);
    }
    let out = call(&root, "code_map", json!({})).unwrap();
    assert!(out.len() <= 40_000 + 200, "{} characters", out.len());
    let last = out.lines().last().unwrap();
    assert!(last.contains("[cut"), "no cut note: {last}");
    let body_lines: Vec<&str> = out.lines().filter(|l| !l.contains("[cut")).collect();
    let last_header = body_lines.iter().rposition(|l| !l.starts_with(' ')).unwrap();
    assert_eq!(
        body_lines.len() - last_header - 1,
        400,
        "the last file was cut inside its outline"
    );
}

// ── Q14 ──────────────────────────────────────────────────────────────────────

#[test]
fn code_map_survives_a_parse_error() {
    let root = tmp("q14");
    write(&root, "broken.rs", "fn good() {}\n\nfn bad( {\n\nstruct After;\n");
    let out = call(&root, "code_map", json!({"path": "broken.rs"})).unwrap();
    assert!(out.contains("fn good :1"), "{out}");
}

// ── Q15 ──────────────────────────────────────────────────────────────────────

#[test]
fn binary_files_are_skipped() {
    let root = tmp("q15");
    fs::write(root.join("blob.rs"), b"fn needle() {}\0\x01\x02").unwrap();
    write(&root, "text.rs", "fn other() {}\n");
    assert_eq!(
        call(&root, "grep", json!({"pattern": "needle"})).unwrap(),
        "no matches"
    );
    let map = call(&root, "code_map", json!({})).unwrap();
    assert!(!map.contains("blob.rs"), "{map}");
    assert!(map.contains("fn other :1"), "{map}");
}

// ── Q16 ──────────────────────────────────────────────────────────────────────

struct Capture(Mutex<Vec<Value>>);

impl Llm for Capture {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        self.0.lock().unwrap().push(request.clone());
        Ok(json!({"content": [{"type": "text", "text": "done"}], "stop_reason": "end_turn"}))
    }
}

#[test]
fn both_formats_get_the_new_tools() {
    let root = tmp("q16");
    let llm = Capture(Mutex::new(Vec::new()));
    run_task(&llm, &root, "hi", &mut |_| {}).unwrap();
    let request = llm.0.lock().unwrap()[0].clone();
    let names: Vec<&str> = request["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["name"].as_str())
        .collect();
    let chat = to_chat(&request);
    let chat_names: Vec<&str> = chat["tools"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|t| t["function"]["name"].as_str())
        .collect();
    for tool in ["grep", "glob", "code_map"] {
        assert!(names.contains(&tool), "Anthropic tools lack {tool}: {names:?}");
        assert!(chat_names.contains(&tool), "chat tools lack {tool}: {chat_names:?}");
    }
}
