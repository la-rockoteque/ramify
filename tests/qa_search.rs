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

// ── review fixes ─────────────────────────────────────────────────────────────

fn lines_of(out: &str) -> Vec<String> {
    out.lines().map(String::from).collect()
}

/// Runs in a child process from `code_map_survives_deep_nesting`, so a stack overflow fails
/// that test and does not abort the whole suite.
#[test]
#[ignore]
fn deep_nesting_child() {
    let root = tmp("deep");
    let depth = 4_000;
    let source = format!(
        "let x = {}1{};\nfunction after() {{}}\n",
        "(".repeat(depth),
        ")".repeat(depth)
    );
    write(&root, "deep.js", &source);
    let out = std::thread::Builder::new()
        .stack_size(128 * 1024)
        .spawn(move || call(&root, "code_map", json!({"path": "deep.js"})))
        .unwrap()
        .join()
        .unwrap()
        .unwrap();
    assert!(out.contains("function after :2"), "{out}");
}

#[test]
fn code_map_survives_deep_nesting() {
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .args(["deep_nesting_child", "--exact", "--ignored", "--quiet"])
        .stdout(std::process::Stdio::null())
        .status()
        .unwrap();
    assert!(status.success(), "code_map crashed or failed on deep nesting: {status}");
}

#[test]
fn code_map_cuts_one_big_file_at_a_line() {
    let root = tmp("bigmap");
    let source: String = (0..3000)
        .map(|i| format!("fn function_number_{i}() {{}}\n"))
        .collect();
    write(&root, "big.rs", &source);
    let out = call(&root, "code_map", json!({"path": "big.rs"})).unwrap();
    assert!(out.starts_with("big.rs\n"), "{}", &out[..out.len().min(80)]);
    assert!(out.chars().count() <= 40_000, "{} characters", out.chars().count());
    let lines = lines_of(&out);
    let (note, body) = lines.split_last().unwrap();
    assert!(note.starts_with("[cut:"), "{note}");
    assert!(body.len() > 100, "only {} lines kept", body.len());
    for line in &body[1..] {
        assert!(line.starts_with("  fn function_number_"), "{line}");
        let number = line.rsplit(':').next().unwrap();
        assert!(number.chars().all(|c| c.is_ascii_digit()), "a cut line: {line}");
    }
}

#[test]
fn large_files_are_skipped() {
    let root = tmp("large");
    write(&root, "small.txt", "needle\n");
    write(&root, "big.txt", &"needle\n".repeat(11 * 1024 * 1024 / 7 + 1));
    let out = call(&root, "grep", json!({"pattern": "needle"})).unwrap();
    assert!(out.starts_with("small.txt:1: needle"), "{}", &out[..out.len().min(200)]);
    assert!(!out.contains("big.txt:"), "the big file was searched");
    assert!(out.contains("big.txt") && out.contains("10 MB"), "no skip note: {out}");
    let map = call(&root, "code_map", json!({})).unwrap();
    assert!(map.contains("big.txt (skipped: over 10 MB)"), "{map}");
    assert!(map.contains("small.txt (1 lines)"), "{map}");
}

#[test]
fn a_file_path_under_git_or_ignored_is_rejected() {
    let root = tmp("filepath");
    write(&root, ".git/config", "needle\n");
    write(&root, ".gitignore", ".env\n");
    write(&root, ".env", "needle=secret\n");
    write(&root, "a.rs", "fn needle() {}\n");
    for (tool, input) in [
        ("grep", json!({"pattern": "needle", "path": ".git/config"})),
        ("grep", json!({"pattern": "needle", "path": ".env"})),
        ("code_map", json!({"path": ".git/config"})),
        ("code_map", json!({"path": ".env"})),
        ("glob", json!({"pattern": "*", "path": ".git"})),
    ] {
        let error = call(&root, tool, input.clone()).expect_err(&format!("{tool} {input}"));
        assert!(
            error.contains(".git") || error.contains("ignored"),
            "{tool} {input}: {error}"
        );
    }
    assert_eq!(
        call(&root, "grep", json!({"pattern": "needle", "path": "a.rs"})).unwrap(),
        "a.rs:1: fn needle() {}"
    );
}

#[test]
fn paths_and_patterns_normalise() {
    let root = tmp("normalise");
    write(&root, "src/a.rs", "fn a() {}\n");
    assert_eq!(
        call(&root, "glob", json!({"pattern": "./src/*.rs"})).unwrap(),
        "src/a.rs"
    );
    let error = call(&root, "glob", json!({"pattern": "*", "path": "src/a.rs"})).unwrap_err();
    assert!(error.contains("folder"), "{error}");
    // Only on a file system that ignores case, as macOS does by default.
    if root.join("SRC").exists() {
        assert_eq!(
            call(&root, "glob", json!({"pattern": "*.rs", "path": "SRC"})).unwrap(),
            "src/a.rs"
        );
        assert_eq!(
            call(&root, "grep", json!({"pattern": "fn", "path": "SRC"})).unwrap(),
            "src/a.rs:1: fn a() {}"
        );
    }
}

#[test]
fn grep_glob_matches_like_the_glob_tool() {
    let root = tmp("globrule");
    for path in ["c.rs", "d.txt", "src/a.rs", "src/deep/b.rs"] {
        write(&root, path, "x\n");
    }
    for (path, pattern) in [
        (".", "*.rs"),
        (".", "src/*.rs"),
        (".", "src/**/*.rs"),
        ("src", "*.rs"),
        ("src", "deep/*.rs"),
    ] {
        let mut from_glob: Vec<String> =
            lines_of(&call(&root, "glob", json!({"pattern": pattern, "path": path})).unwrap());
        let mut from_grep: Vec<String> = lines_of(
            &call(&root, "grep", json!({"pattern": "x", "path": path, "glob": pattern})).unwrap(),
        )
        .into_iter()
        .map(|line| line.split(':').next().unwrap().to_string())
        .collect();
        from_glob.sort();
        from_grep.sort();
        assert_eq!(from_glob, from_grep, "path {path}, pattern {pattern}");
    }
    let all = call(&root, "glob", json!({"pattern": "*.rs"})).unwrap();
    assert_eq!(all, "c.rs\nsrc/a.rs\nsrc/deep/b.rs", "a name pattern matches at any depth");
}

#[test]
fn code_map_cap_counts_what_it_says() {
    let root = tmp("capcount");
    for i in 0..2000 {
        write(&root, &format!("f{i:04}.rs"), &format!("fn f{i}() {{}}\n"));
    }
    let out = call(&root, "code_map", json!({})).unwrap();
    assert!(out.chars().count() <= 40_000, "{} characters", out.chars().count());
    assert!(out.ends_with("characters; map a smaller path]"), "{}", &out[out.len() - 80..]);
}

#[test]
fn subfolder_walk_keeps_root_gitignore() {
    let root = tmp("subignore");
    write(&root, ".gitignore", "*.log\n");
    write(&root, "sub/a.log", "x\n");
    write(&root, "sub/b.rs", "x\n");
    write(&root, "other/c.rs", "x\n");
    assert_eq!(
        call(&root, "glob", json!({"pattern": "*", "path": "sub"})).unwrap(),
        "sub/b.rs"
    );
}

#[test]
fn ts_class_fields_and_default_exports_are_listed() {
    let root = tmp("tsfields");
    write(&root, "a.ts", "class Widget {\n  handle = () => {};\n  run() {}\n}\n");
    write(&root, "b.ts", "export default class {\n  go() {}\n}\n");
    write(&root, "c.js", "export default function () {}\n");
    write(&root, "d.js", "class W {\n  onClick = () => {};\n}\n");
    let out = call(&root, "code_map", json!({})).unwrap();
    for expected in [
        "method handle :2",
        "method run :3",
        "class default :1",
        "method go :2",
        "function default :1",
        "method onClick :2",
    ] {
        assert!(out.contains(expected), "missing {expected}:\n{out}");
    }
}
