//! `ramify run ask --no-window`: the run is created, and the run window is not opened.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramify-run-ask-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn ask(dir: &PathBuf, extra: &[&str]) -> String {
    let script = dir.join("script.sh");
    fs::write(&script, "#!/bin/sh\necho hi\n").unwrap();
    // An empty RAMIFY_HOME makes the window launcher fail at once, so no window opens here.
    let home = dir.join("home");
    fs::create_dir_all(&home).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_ramify"))
        .args(["run", "ask"])
        .arg(&script)
        .args(["--cwd"])
        .arg(dir)
        .args(["--note", "check"])
        .args(extra)
        .env("RAMIFY_RUNS_DIR", dir.join("runs"))
        .env("RAMIFY_HOME", &home)
        .env_remove("RAMIFY_POPUP")
        .output()
        .expect("ramify runs");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
fn run_ask_no_window_skips_the_window() {
    let dir = temp("no-window");
    let stdout = ask(&dir, &["--no-window"]);
    assert!(stdout.starts_with("run "), "{stdout}");
    assert!(!stdout.contains("did not open"), "the launcher ran:\n{stdout}");
    let runs: Vec<_> = fs::read_dir(dir.join("runs")).unwrap().flatten().collect();
    assert_eq!(runs.len(), 1);
    let meta = fs::read_to_string(runs[0].path().join("meta.json")).unwrap();
    assert!(meta.contains("\"pending\""), "{meta}");

    // Without the flag, the launcher runs (and fails here, with no app to open).
    let dir = temp("window");
    let stdout = ask(&dir, &[]);
    assert!(stdout.contains("did not open"), "{stdout}");
}
