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

// ── audit fixes ──────────────────────────────────────────────────────────────

fn write(root: &Path, input: Value) -> Result<String, String> {
    Tools::new(root).call("write_file", &input)
}

#[test]
fn file_tools_refuse_git_metadata_through_a_symlink() {
    let root = tmp("a1");
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(root.join(".git/config"), "[core]\n").unwrap();
    symlink(root.join(".git"), root.join("gitlink")).unwrap();
    let edited = edit(
        &root,
        json!({"path": "gitlink/config", "old_string": "[core]", "new_string": "[core]\n\tfsmonitor = x"}),
    );
    assert!(edited.is_err(), "edit_file wrote .git through a link: {edited:?}");
    let written = write(&root, json!({"path": "gitlink/config", "content": "x"}));
    assert!(written.is_err(), "write_file wrote .git through a link: {written:?}");
    assert_eq!(fs::read_to_string(root.join(".git/config")).unwrap(), "[core]\n");
}

#[test]
fn write_file_refuses_git_metadata() {
    let root = tmp("a2");
    // A linked worktree has a `.git` file that points at its Git folder.
    fs::write(root.join(".git"), "gitdir: /elsewhere\n").unwrap();
    fs::create_dir_all(root.join("sub/.git/hooks")).unwrap();
    for path in [".git", "sub/.git/hooks/pre-commit", "sub/.git/config"] {
        let written = write(&root, json!({"path": path, "content": "x"}));
        assert!(written.is_err(), "write_file wrote {path}: {written:?}");
    }
    assert_eq!(fs::read_to_string(root.join(".git")).unwrap(), "gitdir: /elsewhere\n");
    assert!(!root.join("sub/.git/hooks/pre-commit").exists());
}

#[test]
fn edit_matches_a_mixed_file_as_is() {
    let root = tmp("a3");
    fs::write(root.join("m.txt"), "a\r\nb\nc\r\n").unwrap();
    let out = edit(&root, json!({"path": "m.txt", "old_string": "b\nc", "new_string": "B\nc"}));
    assert!(out.is_ok(), "{out:?}");
    assert_eq!(fs::read(root.join("m.txt")).unwrap(), b"a\r\nB\nc\r\n");
}

#[test]
fn edit_through_a_symlink_keeps_the_link() {
    let root = tmp("a4");
    fs::write(root.join("real.txt"), "one\n").unwrap();
    symlink(root.join("real.txt"), root.join("alias.txt")).unwrap();
    edit(&root, json!({"path": "alias.txt", "old_string": "one", "new_string": "two"})).unwrap();
    assert!(fs::symlink_metadata(root.join("alias.txt")).unwrap().file_type().is_symlink());
    assert_eq!(fs::read_to_string(root.join("real.txt")).unwrap(), "two\n");
}

// ── review fixes ─────────────────────────────────────────────────────────────

fn repo(name: &str) -> PathBuf {
    let root = tmp(name);
    fs::create_dir_all(root.join(".git/hooks")).unwrap();
    root
}

#[test]
fn write_through_a_dangling_symlink_is_refused() {
    let root = repo("r1");
    symlink(".git/hooks/pre-commit", root.join("innocent")).unwrap();
    let out = write(&root, json!({"path": "innocent", "content": "#!/bin/sh\necho pwned\n"}));
    assert!(out.is_err(), "{out:?}");
    assert!(!root.join(".git/hooks/pre-commit").exists());
    let edited = edit(&root, json!({"path": "innocent", "old_string": "a", "new_string": "b"}));
    assert!(edited.is_err(), "{edited:?}");

    let outside = std::env::temp_dir().join(format!("ramcode-edit-outside-{}", std::process::id()));
    let _ = fs::remove_file(&outside);
    symlink(&outside, root.join("x")).unwrap();
    let out = write(&root, json!({"path": "x", "content": "escaped"}));
    assert!(out.is_err(), "{out:?}");
    assert!(!outside.exists(), "a file was written outside the worktree");

    // A dangling link as a folder on the way.
    symlink(".git/hooks", root.join("hooks-link-gone")).unwrap();
    fs::remove_dir_all(root.join(".git/hooks")).unwrap();
    let out = write(&root, json!({"path": "hooks-link-gone/pre-push", "content": "x"}));
    assert!(out.is_err(), "{out:?}");
    assert!(!root.join(".git/hooks").exists());
}

#[test]
fn planted_symlink_at_the_temp_path_is_harmless() {
    let root = repo("r2");
    fs::write(root.join("run.sh"), "echo hi\n").unwrap();
    fs::set_permissions(root.join("run.sh"), fs::Permissions::from_mode(0o755)).unwrap();
    let pid = std::process::id();
    for seq in 0..64 {
        let _ = symlink(
            ".git/hooks/post-checkout",
            root.join(format!(".run.sh.ram-edit-{pid}-{seq}")),
        );
    }
    let out = edit(&root, json!({"path": "run.sh", "old_string": "hi", "new_string": "bye"}));
    assert!(out.is_ok(), "{out:?}");
    assert!(!root.join(".git/hooks/post-checkout").exists(), "the edit wrote through a planted link");
    let meta = fs::symlink_metadata(root.join("run.sh")).unwrap();
    assert!(meta.file_type().is_file(), "run.sh is no longer a regular file");
    assert_eq!(fs::read_to_string(root.join("run.sh")).unwrap(), "echo bye\n");
    assert_eq!(meta.permissions().mode() & 0o777, 0o755);
}

#[test]
fn hard_linked_target_is_refused() {
    let root = repo("r3");
    fs::write(root.join(".git/config"), "[core]\n").unwrap();
    fs::hard_link(root.join(".git/config"), root.join("cfg")).unwrap();
    let out = write(&root, json!({"path": "cfg", "content": "[core]\n\tfsmonitor = evil\n"}));
    assert!(out.is_err(), "{out:?}");
    let edited = edit(
        &root,
        json!({"path": "cfg", "old_string": "[core]", "new_string": "[core]\n\tfsmonitor = evil"}),
    );
    assert!(edited.is_err(), "{edited:?}");
    assert_eq!(fs::read_to_string(root.join(".git/config")).unwrap(), "[core]\n");
}

#[test]
fn git_folder_name_is_matched_without_case() {
    let root = tmp("r4");
    fs::create_dir_all(root.join("sub")).unwrap();
    let out = write(&root, json!({"path": "sub/.GIT/config", "content": "x"}));
    assert!(out.is_err(), "{out:?}");
    assert!(!root.join("sub/.GIT").exists());
}

#[test]
fn write_file_is_atomic() {
    let root = tmp("r5");
    let file = root.join("f.txt");
    fs::write(&file, "old\n").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o640)).unwrap();
    let inode = fs::metadata(&file).unwrap().ino();
    write(&root, json!({"path": "f.txt", "content": "new\n"})).unwrap();
    assert_eq!(fs::read_to_string(&file).unwrap(), "new\n");
    assert_ne!(fs::metadata(&file).unwrap().ino(), inode, "write_file wrote in place");
    assert_eq!(fs::metadata(&file).unwrap().permissions().mode() & 0o777, 0o640);

    // A failed write leaves the old content and no temp file.
    fs::create_dir_all(root.join("ro")).unwrap();
    fs::write(root.join("ro/g.txt"), "keep\n").unwrap();
    fs::set_permissions(root.join("ro"), fs::Permissions::from_mode(0o555)).unwrap();
    let out = write(&root, json!({"path": "ro/g.txt", "content": "lost\n"}));
    fs::set_permissions(root.join("ro"), fs::Permissions::from_mode(0o755)).unwrap();
    assert!(out.is_err(), "{out:?}");
    assert_eq!(fs::read_to_string(root.join("ro/g.txt")).unwrap(), "keep\n");
    let left: Vec<_> = fs::read_dir(&root)
        .unwrap()
        .chain(fs::read_dir(root.join("ro")).unwrap())
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with('.'))
        .collect();
    assert!(left.is_empty(), "temp files left: {left:?}");
}

#[test]
fn mixed_endings_follow_the_line_around_the_match() {
    let root = tmp("r6");
    fs::write(root.join("m.txt"), "a\r\nb\r\nc\n").unwrap();
    edit(&root, json!({"path": "m.txt", "old_string": "b", "new_string": "b1\nb2"})).unwrap();
    assert_eq!(fs::read(root.join("m.txt")).unwrap(), b"a\r\nb1\r\nb2\r\nc\n");
}

#[test]
fn line_range_for_trailing_newline() {
    let root = tmp("r7");
    fs::write(root.join("t.txt"), "a\nb\nc\n").unwrap();
    let out = edit(&root, json!({"path": "t.txt", "old_string": "b\n", "new_string": "x\n"})).unwrap();
    assert!(out.contains("lines 2-2"), "{out}");
}

#[test]
fn ambiguous_error_lists_at_most_ten_lines() {
    let root = tmp("r8");
    fs::write(root.join("t.txt"), "x\n".repeat(25)).unwrap();
    let error = edit(&root, json!({"path": "t.txt", "old_string": "x", "new_string": "y"})).unwrap_err();
    assert!(error.contains("25"), "{error}");
    assert!(error.contains("1, 2, 3, 4, 5, 6, 7, 8, 9, 10"), "{error}");
    assert!(!error.contains("11,") && !error.contains(", 11"), "{error}");
    assert!(error.contains("and 15 more"), "{error}");
}

#[test]
fn replace_all_reports_first_and_last_line() {
    let root = tmp("r9");
    fs::write(root.join("t.txt"), "x\na\nx\nb\nx\n").unwrap();
    let out = edit(
        &root,
        json!({"path": "t.txt", "old_string": "x", "new_string": "y", "replace_all": true}),
    )
    .unwrap();
    assert!(out.contains("replaced 3 occurrences"), "{out}");
    assert!(out.contains("lines 1-5"), "{out}");
}
