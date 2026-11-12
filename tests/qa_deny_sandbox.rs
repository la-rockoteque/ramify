//! The cases of `.ramify/work/ram-code-deny-sandbox/qa-plan.md` that the build settles.
//! Cases that run a real sandbox need `sandbox-exec` and run on macOS only.

use ram_code::tools::Tools;
use serde_json::json;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// A folder under `/private/tmp`, which is not a read root, so its other files are outside.
fn base(name: &str) -> PathBuf {
    let dir = PathBuf::from(format!(
        "/private/tmp/ramcode-deny-{}-{name}",
        std::process::id()
    ));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(dir.join("wt")).unwrap();
    fs::create_dir_all(dir.join("config")).unwrap();
    dir
}

fn env_with(base: &Path, extra: &[(&str, String)]) -> Vec<(String, String)> {
    let mut env: Vec<(String, String)> = ["PATH", "HOME", "USER", "LANG"]
        .iter()
        .filter_map(|key| std::env::var(key).ok().map(|value| (key.to_string(), value)))
        .filter(|(key, _)| !extra.iter().any(|(name, _)| name == key))
        .collect();
    if !extra.iter().any(|(name, _)| *name == "RAM_CODE_CONFIG_DIR") {
        env.push((
            "RAM_CODE_CONFIG_DIR".into(),
            base.join("config").display().to_string(),
        ));
    }
    env.extend(extra.iter().map(|(k, v)| (k.to_string(), v.clone())));
    env
}

fn tools(base: &Path, root: &Path) -> Tools {
    Tools::with_env(root, env_with(base, &[]))
}

fn run(tools: &Tools, command: &str) -> String {
    match tools.call("run_command", &json!({"command": command, "timeout_seconds": 600})) {
        Ok(text) | Err(text) => text,
    }
}

fn ok(out: &str) -> bool {
    out.starts_with("exit 0")
}

fn have(program: &str) -> bool {
    Command::new("sh")
        .args(["-c", &format!("command -v {program}")])
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

fn git(dir: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(["-c", "user.name=t", "-c", "user.email=t@t", "-c", "commit.gpgsign=false"])
        .args(args)
        .current_dir(dir)
        .output()
        .unwrap();
    assert!(status.status.success(), "git {args:?}: {:?}", status);
}

fn crate_at(dir: &Path, build_script: bool) {
    fs::create_dir_all(dir.join("src")).unwrap();
    let build = if build_script { "build = \"build.rs\"\n" } else { "" };
    fs::write(
        dir.join("Cargo.toml"),
        format!("[package]\nname = \"fixture\"\nversion = \"0.1.0\"\nedition = \"2021\"\n{build}\n[workspace]\n"),
    )
    .unwrap();
    if build_script {
        fs::write(dir.join("native.c"), "int seven(void){return 7;}\n").unwrap();
        fs::write(
            dir.join("build.rs"),
            r#"use std::process::Command;
fn main() {
    let out = std::env::var("OUT_DIR").unwrap();
    assert!(Command::new("cc").args(["-c", "native.c", "-o"]).arg(format!("{out}/native.o")).status().unwrap().success(), "cc failed");
    assert!(Command::new("ar").arg("crs").arg(format!("{out}/libnative.a")).arg(format!("{out}/native.o")).status().unwrap().success(), "ar failed");
    println!("cargo:rustc-link-search=native={out}");
    println!("cargo:rustc-link-lib=static=native");
}
"#,
        )
        .unwrap();
        fs::write(
            dir.join("src/lib.rs"),
            "extern \"C\" { fn seven() -> i32; }\npub fn get() -> i32 { unsafe { seven() } }\n#[cfg(test)] mod t { #[test] fn s() { assert_eq!(super::get(), 7); } }\n",
        )
        .unwrap();
    } else {
        fs::write(
            dir.join("src/lib.rs"),
            "pub fn add(a: i32, b: i32) -> i32 { a + b }\n#[cfg(test)] mod t { #[test] fn s() { assert_eq!(super::add(2, 2), 4); } }\n",
        )
        .unwrap();
    }
}

// ── Q1 ───────────────────────────────────────────────────────────────────────

#[test]
fn profile_starts_with_deny_default() {
    let root = PathBuf::from("/private/tmp");
    let profile = ram_code::sandbox::seatbelt_profile(&root, &root, &[]);
    let rules: Vec<&str> = profile
        .lines()
        .map(str::trim)
        .filter(|line| line.starts_with('('))
        .collect();
    assert_eq!(rules.first(), Some(&"(version 1)"), "{profile}");
    assert_eq!(rules.get(1), Some(&"(deny default)"), "{profile}");
    assert!(!profile.contains("(allow default)"), "{profile}");
}

// ── Q2 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn unlisted_operation_fails() {
    let b = base("q2");
    let out = run(&tools(&b, &b.join("wt")), "ls /Users");
    assert!(!ok(&out), "{out}");
    assert!(out.contains("Operation not permitted"), "{out}");
}

// ── Q3 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn toolchains_work_in_the_sandbox() {
    if !have("cargo") {
        eprintln!("skipped: cargo is not installed");
        return;
    }
    let b = base("q3");
    let root = b.join("wt");
    crate_at(&root, false);
    let out = run(&tools(&b, &root), "cargo build -q && cargo test -q --lib");
    assert!(ok(&out), "{out}");
    assert!(out.contains("test result: ok"), "{out}");
}

// ── Q4 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn c_build_script_works_in_the_sandbox() {
    if !have("cargo") || !have("cc") {
        eprintln!("skipped: cargo or cc is not installed");
        return;
    }
    let b = base("q4");
    let root = b.join("wt");
    crate_at(&root, true);
    let out = run(&tools(&b, &root), "cargo test -q --lib");
    assert!(ok(&out), "{out}");
    assert!(out.contains("test result: ok"), "{out}");
}

// ── Q5 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn git_works_in_the_sandbox() {
    let b = base("q5");
    let root = b.join("wt");
    git(&root, &["init", "-q"]);
    fs::write(root.join("a.txt"), "one\n").unwrap();
    git(&root, &["add", "."]);
    git(&root, &["commit", "-qm", "one"]);
    fs::write(root.join("a.txt"), "two\n").unwrap();
    git(&root, &["commit", "-qam", "two"]);
    let tools = tools(&b, &root);
    for command in ["git status", "git log -1", "git diff --stat HEAD~1"] {
        let out = run(&tools, command);
        assert!(ok(&out), "{command}: {out}");
    }
}

// ── Q6 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn common_tools_work_in_the_sandbox() {
    let b = base("q6");
    let root = b.join("wt");
    fs::write(root.join("f.txt"), "alpha\nbeta\n").unwrap();
    let tools = tools(&b, &root);
    let cases = [
        ("make", "make --version"),
        ("python3", "python3 -c 'print(1)'"),
        ("node", "node -e 'console.log(1)'"),
        ("npm", "npm --version"),
        ("grep", "grep -c alpha f.txt"),
        ("sed", "sed -n 2p f.txt"),
        ("awk", "awk 'NR==1' f.txt"),
        ("sh", "sh -c 'echo hi'"),
        ("bash", "bash -c 'echo hi'"),
        ("zsh", "zsh -c 'echo hi'"),
    ];
    for (program, command) in cases {
        if !have(program) {
            eprintln!("skipped {program}: not installed");
            continue;
        }
        let out = run(&tools, command);
        assert!(ok(&out), "{command}: {out}");
    }
}

// ── Q7 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn reads_outside_the_roots_fail() {
    let b = base("q7");
    let root = b.join("wt");
    fs::write(b.join("outside.txt"), "outside-secret").unwrap();
    fs::create_dir_all(b.join("sibling")).unwrap();
    fs::write(b.join("sibling/README"), "sibling-secret").unwrap();
    let home = b.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::write(home.join(".zshrc"), "zshrc-secret").unwrap();
    let tools = Tools::with_env(&root, env_with(&b, &[("HOME", home.display().to_string())]));
    for command in [
        format!("cat '{}'", b.join("outside.txt").display()),
        format!("cat '{}'", b.join("sibling/README").display()),
        "cat \"$HOME/.zshrc\"".to_string(),
        "ls /Users".to_string(),
    ] {
        let out = run(&tools, &command);
        assert!(!ok(&out), "{command} succeeded: {out}");
        assert!(!out.contains("-secret"), "{command} read it: {out}");
    }
}

// ── Q9 ───────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn cannot_signal_outside() {
    let b = base("q9");
    let mut sleeper = Command::new("sleep").arg("30").spawn().unwrap();
    let out = run(&tools(&b, &b.join("wt")), &format!("kill -0 {}", sleeper.id()));
    let alive = sleeper.try_wait().unwrap().is_none();
    let _ = sleeper.kill();
    let _ = sleeper.wait();
    assert!(!ok(&out), "the sandbox signalled an outside process: {out}");
    assert!(alive, "the outside process died");
}

// ── Q10 ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn linked_worktree_git_reads_common_dir() {
    let b = base("q10");
    let main = b.join("main");
    fs::create_dir_all(&main).unwrap();
    git(&main, &["init", "-q"]);
    fs::write(main.join("a.txt"), "one\n").unwrap();
    git(&main, &["add", "."]);
    git(&main, &["commit", "-qm", "one"]);
    let linked = b.join("linked");
    git(&main, &["worktree", "add", "-q", linked.to_str().unwrap()]);
    let exclude = main.join(".git/info/exclude");
    let before = fs::read_to_string(&exclude).unwrap_or_default();
    let tools = tools(&b, &linked);
    let out = run(&tools, "git status");
    assert!(ok(&out), "{out}");
    let out = run(&tools, &format!("echo planted >> '{}'", exclude.display()));
    assert!(!ok(&out), "the common Git folder was writable: {out}");
    assert_eq!(fs::read_to_string(&exclude).unwrap_or_default(), before);
}

// ── Q11 ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn sandbox_json_adds_paths() {
    let b = base("q11");
    let read = b.join("extra-read");
    let write = b.join("extra-write");
    fs::create_dir_all(&read).unwrap();
    fs::create_dir_all(&write).unwrap();
    fs::write(read.join("f.txt"), "readable").unwrap();
    fs::write(
        b.join("config/sandbox.json"),
        json!({"read": [read], "write": [write]}).to_string(),
    )
    .unwrap();
    let tools = tools(&b, &b.join("wt"));
    let out = run(&tools, &format!("cat '{}'", read.join("f.txt").display()));
    assert!(ok(&out) && out.contains("readable"), "{out}");
    let out = run(&tools, &format!("echo w > '{}'", write.join("g.txt").display()));
    assert!(ok(&out), "{out}");
    assert_eq!(fs::read_to_string(write.join("g.txt")).unwrap(), "w\n");
}

// ── Q12 ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn sandbox_json_cannot_unhide_secrets() {
    let b = base("q12");
    let root = b.join("wt");
    let home = b.join("home");
    fs::create_dir_all(home.join(".ssh")).unwrap();
    fs::write(home.join(".ssh/id_test"), "ssh-secret").unwrap();
    fs::create_dir_all(root.join(".git")).unwrap();
    fs::write(
        b.join("config/sandbox.json"),
        json!({"read": [home.join(".ssh")], "write": [home.join(".ssh"), root.join(".git")]})
            .to_string(),
    )
    .unwrap();
    let tools = Tools::with_env(&root, env_with(&b, &[("HOME", home.display().to_string())]));
    let out = run(&tools, "cat \"$HOME/.ssh/id_test\"");
    assert!(!ok(&out) && !out.contains("ssh-secret"), "{out}");
    let out = run(&tools, "echo key >> \"$HOME/.ssh/authorized_keys\"");
    assert!(!ok(&out), "{out}");
    assert!(!home.join(".ssh/authorized_keys").exists());
    let out = run(&tools, "echo x > .git/config");
    assert!(!ok(&out), "{out}");
    assert!(!root.join(".git/config").exists());
}

// ── Q13 ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn project_sandbox_json_is_ignored() {
    let b = base("q13");
    let root = b.join("wt");
    fs::write(b.join("outside.txt"), "outside-secret").unwrap();
    let opened = json!({"read": [b.clone()], "write": [b.clone()]}).to_string();
    fs::write(root.join("sandbox.json"), &opened).unwrap();
    fs::create_dir_all(root.join(".config/ram-code")).unwrap();
    fs::write(root.join(".config/ram-code/sandbox.json"), &opened).unwrap();
    let out = run(&tools(&b, &root), &format!("cat '{}'", b.join("outside.txt").display()));
    assert!(!ok(&out) && !out.contains("outside-secret"), "{out}");
}

// ── Q14 ──────────────────────────────────────────────────────────────────────

#[test]
fn invalid_sandbox_json_is_an_error() {
    let b = base("q14");
    let root = b.join("wt");
    fs::write(b.join("config/sandbox.json"), "{not json").unwrap();
    let result = tools(&b, &root).call(
        "run_command",
        &json!({"command": "touch marker", "timeout_seconds": 10}),
    );
    let text = match &result {
        Ok(text) | Err(text) => text.clone(),
    };
    assert!(result.is_err(), "{text}");
    assert!(text.contains("sandbox.json"), "{text}");
    assert!(!root.join("marker").exists(), "the command ran");
}

// ── Q15 ──────────────────────────────────────────────────────────────────────

#[cfg(target_os = "macos")]
#[test]
fn denied_command_gets_the_hint() {
    let b = base("q15");
    fs::write(b.join("outside.txt"), "x").unwrap();
    let tools = tools(&b, &b.join("wt"));
    let out = run(&tools, &format!("cat '{}'", b.join("outside.txt").display()));
    let last = out.trim_end().lines().last().unwrap_or("");
    assert!(last.contains("sandbox.json") && last.contains("outside_sandbox"), "{out}");
    let out = run(&tools, "echo hi");
    assert!(ok(&out), "{out}");
    assert!(!out.contains("sandbox.json"), "{out}");
}

// ── audit fix ────────────────────────────────────────────────────────────────

/// The Keychain daemon must be out of reach: items stored with the `security` command often let
/// that command read them without a prompt. The test makes a throwaway keychain in the worktree,
/// unlocked, with one dummy item, and never touches the user's keychains or search list.
#[cfg(target_os = "macos")]
#[test]
fn keychain_is_out_of_reach() {
    let base = base("kc");
    let root = base.join("wt");
    let keychain = root.join("probe.keychain-db");
    let security = |args: &[&str]| {
        let status = Command::new("/usr/bin/security").args(args).status().unwrap();
        assert!(status.success(), "security {args:?} failed");
    };
    let path = keychain.to_str().unwrap();
    security(&["create-keychain", "-p", "probe-pw", path]);
    security(&["unlock-keychain", "-p", "probe-pw", path]);
    security(&["add-generic-password", "-a", "probe", "-s", "ram-code-probe", "-w", "dummy-value-123", path]);
    let tools = tools(&base, &root);
    let out = run(
        &tools,
        &format!("/usr/bin/security find-generic-password -s ram-code-probe -w '{path}'"),
    );
    let _ = Command::new("/usr/bin/security").args(["delete-keychain", path]).status();
    assert!(!out.contains("dummy-value-123"), "the sandbox read a keychain item: {out}");
    assert!(!ok(&out), "{out}");
}

// ── review fixes ─────────────────────────────────────────────────────────────

/// A folder from `getconf`, by its real path.
#[cfg(target_os = "macos")]
fn getconf(name: &str) -> PathBuf {
    let out = Command::new("getconf").arg(name).output().unwrap();
    let text = String::from_utf8(out.stdout).unwrap();
    fs::canonicalize(text.trim()).unwrap()
}

/// A fake home folder under `base`, by its real path.
fn fake_home(base: &Path) -> PathBuf {
    let home = base.join("home");
    fs::create_dir_all(&home).unwrap();
    fs::canonicalize(&home).unwrap()
}

#[cfg(target_os = "macos")]
#[test]
fn temp_folder_cannot_be_repointed() {
    let b = base("r1");
    let outside = b.join("outside");
    fs::create_dir_all(&outside).unwrap();
    let tools = tools(&b, &b.join("wt"));
    let temp_out = run(&tools, "printf %s \"$TMPDIR\"");
    let temp = PathBuf::from(temp_out.lines().nth(1).unwrap_or("").trim());
    assert!(temp.is_dir(), "no task temp folder: {temp_out}");

    // From inside the sandbox, the swap fails.
    let first = run(
        &tools,
        &format!("rm -rf \"$TMPDIR\" && ln -s '{}' \"$TMPDIR\"", outside.display()),
    );
    let swapped = fs::symlink_metadata(&temp)
        .map(|m| m.file_type().is_symlink())
        .unwrap_or(false);
    assert!(!swapped, "a sandboxed command replaced the temp folder: {first}");
    let moved = run(&tools, "mv \"$TMPDIR\" \"$TMPDIR-moved\"");
    assert!(temp.is_dir() && !ok(&moved), "the temp folder was renamed away: {moved}");
    let root_moved = run(&tools, &format!("mv '{}' '{}'", b.join("wt").display(), b.join("wt2").display()));
    assert!(b.join("wt").is_dir() && !ok(&root_moved), "the worktree was renamed away: {root_moved}");

    // If something else swaps it, the next command refuses to run.
    fs::remove_dir_all(&temp).unwrap();
    std::os::unix::fs::symlink(&outside, &temp).unwrap();
    let second = run(&tools, "echo pwned > \"$TMPDIR/x\"");
    let _ = fs::remove_file(&temp);
    assert!(!outside.join("x").exists(), "a write reached the outside folder: {second}");
    assert!(!ok(&second), "the command ran in a replaced temp folder: {second}");
    assert!(second.contains("was replaced"), "{second}");
}

#[cfg(target_os = "macos")]
#[test]
fn git_and_cargo_credentials_stay_hidden() {
    let b = base("r2");
    let home = fake_home(&b);
    fs::create_dir_all(home.join(".config/git")).unwrap();
    fs::write(home.join(".config/git/credentials"), "https://u:git-secret@example.com\n").unwrap();
    fs::write(home.join(".config/git/config"), "[user]\n\tname = t\n").unwrap();
    fs::create_dir_all(home.join(".cargo/bin")).unwrap();
    fs::write(home.join(".cargo/config.toml"), "[term]\nverbose = false\n").unwrap();
    fs::write(home.join(".cargo/credentials.toml"), "[registry]\ntoken = \"cargo-secret\"\n").unwrap();
    let tools = Tools::with_env(
        &b.join("wt"),
        env_with(&b, &[("HOME", home.display().to_string())]),
    );
    let creds = run(&tools, "cat \"$HOME/.config/git/credentials\"");
    assert!(!creds.contains("git-secret"), "git credentials readable: {creds}");
    let cargo = run(&tools, "cat \"$HOME/.cargo/credentials.toml\"");
    assert!(!cargo.contains("cargo-secret"), "cargo credentials readable: {cargo}");
    // Cargo stops when config.toml exists and cannot be read, so it stays readable.
    let cargo_config = run(&tools, "cat \"$HOME/.cargo/config.toml\"");
    assert!(ok(&cargo_config), "cargo config should stay readable: {cargo_config}");
    let config = run(&tools, "cat \"$HOME/.config/git/config\"");
    assert!(ok(&config), "git config should stay readable: {config}");
}

#[cfg(target_os = "macos")]
#[test]
fn symlinked_secret_and_config_paths_stay_denied() {
    let b = base("r3");
    let home = fake_home(&b);
    let real_config = home.join("dotfiles/config");
    fs::create_dir_all(real_config.join("gh")).unwrap();
    fs::create_dir_all(real_config.join("ram-code")).unwrap();
    fs::write(real_config.join("gh/hosts.yml"), "oauth_token: gh-secret\n").unwrap();
    std::os::unix::fs::symlink(&real_config, home.join(".config")).unwrap();
    let sandbox_json = real_config.join("ram-code/sandbox.json");
    // A dotfile user lets commands read and write their dotfiles folder.
    let original = "{\"read\": [\"~/dotfiles\"], \"write\": [\"~/dotfiles\"]}\n";
    fs::write(&sandbox_json, original).unwrap();
    let env = env_with(
        &b,
        &[
            ("HOME", home.display().to_string()),
            ("RAM_CODE_CONFIG_DIR", home.join(".config/ram-code").display().to_string()),
        ],
    );
    let tools = Tools::with_env(&b.join("wt"), env);
    let read = run(&tools, "cat \"$HOME/.config/gh/hosts.yml\"");
    assert!(!read.contains("gh-secret"), "a symlinked secret was readable: {read}");
    let widen = run(
        &tools,
        "echo '{\"read\":[\"/\"],\"write\":[\"/\"]}' > \"$HOME/.config/ram-code/sandbox.json\"",
    );
    assert_eq!(
        fs::read_to_string(&sandbox_json).unwrap(),
        original,
        "the command rewrote sandbox.json: {widen}"
    );
}

#[cfg(target_os = "macos")]
#[test]
fn per_user_temp_is_create_only() {
    let t = getconf("DARWIN_USER_TEMP_DIR");
    let tag = format!("ramcode-probe-{}", std::process::id());
    let other = t.join(format!("{tag}-app"));
    let existing = t.join(format!("{tag}-existing"));
    let created = t.join(format!("{tag}-new"));
    let moved = t.join(format!("{tag}-moved"));
    let link = t.join(format!("{tag}-link"));
    let cleanup = || {
        let _ = fs::remove_dir_all(&other);
        for p in [&existing, &created, &moved, &link] {
            let _ = fs::remove_file(p);
        }
    };
    cleanup();
    fs::create_dir_all(&other).unwrap();
    fs::write(other.join("secret.txt"), "app-secret").unwrap();
    fs::write(&existing, "existing").unwrap();

    let b = base("r4");
    let tools = tools(&b, &b.join("wt"));
    let read_other = run(&tools, &format!("cat '{}'", other.join("secret.txt").display()));
    let read_existing = run(&tools, &format!("cat '{}'", existing.display()));
    let mv = run(&tools, &format!("mv '{}' '{}'", existing.display(), moved.display()));
    let rm = run(&tools, &format!("rm -f '{}'", existing.display()));
    let ln = run(&tools, &format!("ln -s '{}' '{}'", b.join("wt").display(), link.display()));
    let create = run(&tools, &format!("echo hi > '{}'", created.display()));
    let existing_left = existing.exists();
    let created_text = fs::read_to_string(&created).unwrap_or_default();
    let link_made = fs::symlink_metadata(&link).is_ok();
    cleanup();

    assert!(!read_other.contains("app-secret"), "another app's file was readable: {read_other}");
    assert!(!read_existing.contains("existing"), "an existing entry was readable: {read_existing}");
    assert!(existing_left, "an existing entry was moved or removed: mv={mv} rm={rm}");
    assert!(!link_made, "a link was planted in the user temp folder: {ln}");
    assert!(ok(&create) && created_text == "hi\n", "a new file could not be made: {create}");
}

#[cfg(target_os = "macos")]
#[test]
fn shared_clang_cache_is_not_writable() {
    let c = getconf("DARWIN_USER_CACHE_DIR");
    let probe = c.join(format!("clang/ramcode-probe-{}", std::process::id()));
    let b = base("r5");
    let tools = tools(&b, &b.join("wt"));
    let out = run(&tools, &format!("mkdir -p '{}' && echo x > '{}/f'", probe.display(), probe.display()));
    let made = probe.exists();
    let _ = fs::remove_dir_all(&probe);
    assert!(!made, "the shared clang cache was writable: {out}");
    let env = run(&tools, "printf %s \"$CLANG_MODULE_CACHE_PATH\"");
    let tmp = run(&tools, "printf %s \"$TMPDIR\"");
    let cache = env.lines().nth(1).unwrap_or("").to_string();
    let temp = tmp.lines().nth(1).unwrap_or("").to_string();
    assert!(!cache.is_empty() && cache.starts_with(&temp), "module cache is not per task: {env} / {tmp}");
}
