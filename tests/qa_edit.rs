//! The cases of `.ramify/work/ram-code-edit-tool/qa-plan.md` that the build settles.

use ram_code::agent::{run_task, Llm, SYSTEM_PROMPT};
use ram_code::openai::to_chat;
use ram_code::tools::Tools;
use serde_json::{json, Value};
use std::fs;
use std::os::unix::fs::{symlink, MetadataExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramcode-edit-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn edit(root: &Path, input: Value) -> Result<String, String> {
    Tools::new(root).call("edit_file", &input)
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_replaces_one_match_and_nothing_else() {
    let root = tmp("q1");
    let before = "fn a() {\n    let x = 1;\n}\n\nfn b() {\n    let y = 2;\n}\n";
    fs::write(root.join("a.rs"), before).unwrap();
    edit(
        &root,
        json!({"path": "a.rs", "old_string": "let x = 1;", "new_string": "let x = 10;"}),
    )
    .unwrap();
    let after = fs::read_to_string(root.join("a.rs")).unwrap();
    assert_eq!(after, before.replace("let x = 1;", "let x = 10;"));
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_refuses_an_ambiguous_match() {
    let root = tmp("q2");
    let text = "x = 1\ny = 2\nx = 1\nz = 3\nx = 1\n";
    fs::write(root.join("f.txt"), text).unwrap();
    let error = edit(&root, json!({"path": "f.txt", "old_string": "x = 1", "new_string": "x = 9"}))
        .unwrap_err();
    assert!(error.contains('3'), "{error}");
    for line in ["1", "3", "5"] {
        assert!(error.contains(line), "line {line} missing: {error}");
    }
    assert_eq!(fs::read_to_string(root.join("f.txt")).unwrap(), text);
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_replace_all_reports_the_count() {
    let root = tmp("q3");
    fs::write(root.join("f.txt"), "x = 1\ny = 2\nx = 1\nz = 3\nx = 1\n").unwrap();
    let out = edit(
        &root,
        json!({"path": "f.txt", "old_string": "x = 1", "new_string": "x = 9", "replace_all": true}),
    )
    .unwrap();
    assert!(out.contains('3'), "{out}");
    assert_eq!(
        fs::read_to_string(root.join("f.txt")).unwrap(),
        "x = 9\ny = 2\nx = 9\nz = 3\nx = 9\n"
    );
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_reports_no_match() {
    let root = tmp("q4");
    fs::write(root.join("f.txt"), "hello\n").unwrap();
    let error = edit(&root, json!({"path": "f.txt", "old_string": "nope", "new_string": "x"}))
        .unwrap_err();
    assert!(error.contains("read_file") || error.contains("grep"), "{error}");
    assert_eq!(fs::read_to_string(root.join("f.txt")).unwrap(), "hello\n");
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_refuses_empty_or_identical_strings() {
    let root = tmp("q5");
    fs::write(root.join("f.txt"), "hello\n").unwrap();
    assert!(edit(&root, json!({"path": "f.txt", "old_string": "", "new_string": "x"})).is_err());
    assert!(
        edit(&root, json!({"path": "f.txt", "old_string": "hello", "new_string": "hello"}))
            .is_err()
    );
    assert_eq!(fs::read_to_string(root.join("f.txt")).unwrap(), "hello\n");
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_keeps_crlf_endings() {
    let root = tmp("q6");
    fs::write(root.join("w.txt"), "one\r\ntwo\r\nthree\r\n").unwrap();
    edit(
        &root,
        json!({"path": "w.txt", "old_string": "one\ntwo", "new_string": "one\ntwo and a half\nmore"}),
    )
    .unwrap();
    let after = fs::read_to_string(root.join("w.txt")).unwrap();
    assert_eq!(after, "one\r\ntwo and a half\r\nmore\r\nthree\r\n");
    assert!(!after.replace("\r\n", "").contains('\n'), "a bare LF is left: {after:?}");
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_keeps_the_trailing_newline_state() {
    let root = tmp("q7");
    fs::write(root.join("no.txt"), "a\nlast").unwrap();
    edit(&root, json!({"path": "no.txt", "old_string": "last", "new_string": "final"})).unwrap();
    assert_eq!(fs::read_to_string(root.join("no.txt")).unwrap(), "a\nfinal");

    fs::write(root.join("yes.txt"), "a\nlast\n").unwrap();
    edit(&root, json!({"path": "yes.txt", "old_string": "last", "new_string": "final"})).unwrap();
    assert_eq!(fs::read_to_string(root.join("yes.txt")).unwrap(), "a\nfinal\n");
}

// ── Q8 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_stays_in_the_worktree() {
    let base = tmp("q8");
    let root = base.join("wt");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "[core]\n").unwrap();
    let outside = base.join("outside");
    fs::create_dir_all(&outside).unwrap();
    fs::write(outside.join("o.txt"), "secret\n").unwrap();
    symlink(&outside, root.join("link")).unwrap();
    let absolute = outside.join("o.txt");
    for path in [
        "../outside/o.txt".to_string(),
        absolute.display().to_string(),
        "link/o.txt".to_string(),
        ".git/config".to_string(),
    ] {
        let result = edit(
            &root,
            json!({"path": path, "old_string": "secret", "new_string": "owned"}),
        );
        assert!(result.is_err(), "{path} was edited");
    }
    let config = edit(
        &root,
        json!({"path": ".git/config", "old_string": "[core]", "new_string": "[x]"}),
    );
    assert!(config.is_err());
    assert_eq!(fs::read_to_string(outside.join("o.txt")).unwrap(), "secret\n");
    assert_eq!(fs::read_to_string(root.join(".git/config")).unwrap(), "[core]\n");
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[test]
fn edit_refuses_missing_binary_and_large_files() {
    let root = tmp("q9");
    fs::write(root.join("bin.dat"), b"abc\0def").unwrap();
    fs::write(root.join("latin.txt"), [b'a', 0xe9, b'b', b'\n']).unwrap();
    let big = "abc\n".repeat(3 * 1024 * 1024);
    fs::write(root.join("big.txt"), &big).unwrap();
    for path in ["missing.txt", "bin.dat", "latin.txt", "big.txt"] {
        let result = edit(&root, json!({"path": path, "old_string": "a", "new_string": "z"}));
        assert!(result.is_err(), "{path} was edited");
    }
    assert_eq!(fs::read(root.join("bin.dat")).unwrap(), b"abc\0def");
    assert_eq!(fs::read(root.join("latin.txt")).unwrap(), [b'a', 0xe9, b'b', b'\n']);
    assert!(!root.join("missing.txt").exists());
}

// ── Q10 ──────────────────────────────────────────────────────────────────────

#[test]
fn edit_keeps_the_executable_bit() {
    let root = tmp("q10");
    let script = root.join("run.sh");
    fs::write(&script, "#!/bin/sh\necho old\n").unwrap();
    fs::set_permissions(&script, fs::Permissions::from_mode(0o755)).unwrap();
    edit(&root, json!({"path": "run.sh", "old_string": "old", "new_string": "new"})).unwrap();
    assert_eq!(fs::metadata(&script).unwrap().permissions().mode() & 0o777, 0o755);
}

// ── Q11 ──────────────────────────────────────────────────────────────────────

#[test]
fn edit_output_is_one_line_with_the_range() {
    let root = tmp("q11");
    fs::write(root.join("f.txt"), "a\nb\nsecret-body\nd\n").unwrap();
    let out = edit(
        &root,
        json!({"path": "f.txt", "old_string": "b\nsecret-body", "new_string": "b\nc1\nc2"}),
    )
    .unwrap();
    assert_eq!(out.lines().count(), 1, "{out}");
    assert!(out.contains("2-4"), "{out}");
    assert!(!out.contains("c1") && !out.contains("secret"), "{out}");
}

// ── Q12 ──────────────────────────────────────────────────────────────────────

struct Capture(Mutex<Vec<Value>>);

impl Llm for Capture {
    fn complete(&self, request: &Value) -> Result<Value, String> {
        self.0.lock().unwrap().push(request.clone());
        Ok(json!({"content": [{"type": "text", "text": "done"}], "stop_reason": "end_turn"}))
    }
}

#[test]
fn both_formats_get_edit_file() {
    let root = tmp("q12");
    let llm = Capture(Mutex::new(Vec::new()));
    run_task(&llm, &root, "hi", &mut |_| {}).unwrap();
    let request = llm.0.lock().unwrap()[0].clone();
    let tools = request["tools"].as_array().unwrap();
    assert!(tools.iter().any(|t| t["name"] == "edit_file"));
    let chat = to_chat(&request);
    assert!(chat["tools"]
        .as_array()
        .unwrap()
        .iter()
        .any(|t| t["function"]["name"] == "edit_file"));
    let write = tools.iter().find(|t| t["name"] == "write_file").unwrap();
    assert!(write["description"].as_str().unwrap().contains("edit_file"));
    assert!(SYSTEM_PROMPT.contains("edit_file"));
}

// ── Q13 ──────────────────────────────────────────────────────────────────────

#[test]
fn edit_writes_atomically() {
    let root = tmp("q13");
    let file = root.join("f.txt");
    fs::write(&file, "old\n").unwrap();
    let before = fs::metadata(&file).unwrap().ino();
    edit(&root, json!({"path": "f.txt", "old_string": "old", "new_string": "new"})).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");
    assert_ne!(fs::metadata(&file).unwrap().ino(), before, "the file was not replaced by a rename");
    let names: Vec<String> = fs::read_dir(&root)
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    assert_eq!(names, vec!["f.txt".to_string()], "a temporary file is left: {names:?}");
}
