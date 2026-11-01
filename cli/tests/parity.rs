use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

fn temp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ramify-parity-{}-{name}", std::process::id()));
    let _ = fs::remove_dir_all(&dir);
    fs::create_dir_all(&dir).unwrap();
    dir
}

fn git(args: &[&str], cwd: &Path) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "test")
        .env("GIT_AUTHOR_EMAIL", "test@example.com")
        .env("GIT_COMMITTER_NAME", "test")
        .env("GIT_COMMITTER_EMAIL", "test@example.com")
        .status()
        .expect("git runs");
    assert!(status.success(), "git {args:?} failed");
}

fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("repo root")
        .to_path_buf()
}

fn run(binary: &Path, fixture: &Path, state: &Path, registry: &Path, shell_mode: bool) -> Output {
    run_args(
        binary,
        &["dash-project", "--json"],
        fixture,
        state,
        registry,
        shell_mode,
    )
}

fn run_args(
    binary: &Path,
    args: &[&str],
    fixture: &Path,
    state: &Path,
    registry: &Path,
    shell_mode: bool,
) -> Output {
    let mut command = Command::new(binary);
    command
        .args(args)
        .env("RAMIFY_ROOT", fixture)
        .env("RAMIFY_STATE_DIR", state)
        .env("RAMIFY_REGISTRY", registry)
        .env("RAMIFY_HOME", repo_root())
        .env("RAMIFY_COLOR", "1");
    if shell_mode {
        command.env("RAMIFY_SHELL", "1");
    } else {
        command.env_remove("RAMIFY_SHELL");
    }
    command.output().expect("command runs")
}

#[test]
fn dash_project_json_matches_the_shell() {
    let fixture = temp("fixture");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("state");
    let registry = temp("registry").join("projects");
    let repo = repo_root();

    let shell = run(&repo.join("bin/ramify"), &fixture, &state, &registry, true);
    let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), &fixture, &state, &registry, false);

    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
}

#[test]
fn a_missing_config_matches_the_shell() {
    let fixture = temp("no-config");
    git(&["init", "-q", "-b", "main"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("no-config-state");
    let registry = temp("no-config-registry").join("projects");
    let repo = repo_root();

    let shell = run(&repo.join("bin/ramify"), &fixture, &state, &registry, true);
    let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), &fixture, &state, &registry, false);

    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stderr),
        String::from_utf8_lossy(&rust.stderr),
        "stderr differs"
    );
}

#[test]
fn dash_project_with_services_matches_the_shell() {
    let fixture = temp("services");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=3000\nweb_health=/health\nweb_cmd='true'\nweb_shared_port=3100\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();

    let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
    let state = temp("services-state");
    fs::create_dir_all(state.join(&name)).unwrap();
    fs::write(
        state.join(&name).join(format!("{name}.env")),
        "SLOT=2\nWEB_PORT=3000\nWEB_SHARED=0\n",
    )
    .unwrap();
    let registry = temp("services-registry").join("projects");
    let repo = repo_root();

    let shell = run(&repo.join("bin/ramify"), &fixture, &state, &registry, true);
    let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), &fixture, &state, &registry, false);

    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
}

#[test]
fn config_matches_the_shell() {
    let fixture = temp("config");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("config-state");
    let registry = temp("config-registry").join("projects");
    let repo = repo_root();

    for args in [vec!["config"], vec!["config", "get", "RAMIFY_PROJECT"]] {
        let shell = run_args(&repo.join("bin/ramify"), &args, &fixture, &state, &registry, true);
        let rust = run_args(
            Path::new(env!("CARGO_BIN_EXE_ramify")),
            &args,
            &fixture,
            &state,
            &registry,
            false,
        );
        assert_eq!(shell.status.code(), rust.status.code());
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{args:?} differs"
        );
    }
}

#[test]
fn status_matches_the_shell() {
    let fixture = temp("status");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=3000\nweb_health=/health\nweb_cmd='true'\nweb_shared_port=3100\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
    let state = temp("status-state");
    fs::create_dir_all(state.join(&name).join("slots")).unwrap();
    fs::write(
        state.join(&name).join("slots").join("1"),
        format!("{name} 999999\n"),
    )
    .unwrap();
    let registry = temp("status-registry").join("projects");
    let repo = repo_root();

    let shell = run_args(
        &repo.join("bin/ramify"),
        &["status"],
        &fixture,
        &state,
        &registry,
        true,
    );
    let rust = run_args(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &["status"],
        &fixture,
        &state,
        &registry,
        false,
    );
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
}

#[test]
fn workflow_list_and_show_match_the_shell() {
    let fixture = temp("workflow");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("workflow-state");
    let registry = temp("workflow-registry").join("projects");
    let repo = repo_root();

    for args in [
        vec!["workflow"],
        vec!["workflow", "show", "feature"],
        vec!["workflow", "show", "bug"],
    ] {
        let shell = run_args(&repo.join("bin/ramify"), &args, &fixture, &state, &registry, true);
        let rust = run_args(
            Path::new(env!("CARGO_BIN_EXE_ramify")),
            &args,
            &fixture,
            &state,
            &registry,
            false,
        );
        assert_eq!(shell.status.code(), rust.status.code());
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{args:?} differs"
        );
    }
}

#[test]
fn step_display_matches_the_shell() {
    let fixture = temp("step");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    fs::create_dir_all(fixture.join(".ramify/work/demo")).unwrap();
    fs::write(
        fixture.join(".ramify/work/demo/order.md"),
        "---\nworkflow: feature\nlane: light\nbranch: main\ntags: []\ncreated: 2026-11-01\nsteps:\n- {step: spec, date: '2026-11-01'}\n- {step: qa-plan, date: '2026-11-01'}\n---\n\n# demo\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("step-state");
    let registry = temp("step-registry").join("projects");
    let repo = repo_root();

    let shell = run_args(
        &repo.join("bin/ramify"),
        &["step"],
        &fixture,
        &state,
        &registry,
        true,
    );
    let rust = run_args(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &["step"],
        &fixture,
        &state,
        &registry,
        false,
    );
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
}

#[test]
fn runs_match_the_shell() {
    let runs = temp("runs");
    fs::create_dir_all(runs.join("20260101-1")).unwrap();
    fs::write(
        runs.join("20260101-1").join("meta.json"),
        r#"{"id": "20260101-1", "status": "pending", "source": "echo one", "note": "", "cwd": "/tmp/x", "created": "2026-01-01T00:00:00"}
"#,
    )
    .unwrap();
    fs::create_dir_all(runs.join("20260102-2")).unwrap();
    fs::write(
        runs.join("20260102-2").join("meta.json"),
        "{\"id\": \"20260102-2\", \"status\": \"ran\", \"exit\": 0, \"source\": \"script\", \"note\": \"do it\", \"cwd\": \"/tmp/y\", \"created\": \"2026-01-02T00:00:00\"}\n",
    )
    .unwrap();
    let repo = repo_root();

    for args in [vec!["runs"], vec!["runs", "--json"]] {
        let shell = Command::new(repo.join("bin/ramify"))
            .args(&args)
            .env("RAMIFY_SHELL", "1")
            .env("RAMIFY_RUNS_DIR", &runs)
            .env("RAMIFY_HOME", &repo)
            .output()
            .unwrap();
        let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
            .args(&args)
            .env_remove("RAMIFY_SHELL")
            .env("RAMIFY_RUNS_DIR", &runs)
            .env("RAMIFY_HOME", &repo)
            .output()
            .unwrap();
        assert_eq!(shell.status.code(), rust.status.code());
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{args:?} differs"
        );
    }
}

#[test]
fn card_matches_the_shell() {
    let fixture = temp("card");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=3000\nweb_health=/health\nweb_cmd='true'\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
    let state = temp("card-state");
    fs::create_dir_all(state.join(&name)).unwrap();
    fs::write(
        state.join(&name).join(format!("{name}.env")),
        "SLOT=2\nWEB_PORT=3000\nWEB_SHARED=0\n",
    )
    .unwrap();
    let registry = temp("card-registry").join("projects");
    let repo = repo_root();

    let shell = run_args(
        &repo.join("bin/ramify"),
        &["card"],
        &fixture,
        &state,
        &registry,
        true,
    );
    let rust = run_args(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &["card"],
        &fixture,
        &state,
        &registry,
        false,
    );
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
}

#[test]
fn gates_match_the_shell() {
    let fixture = temp("gates");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    fs::create_dir_all(fixture.join(".ramify/work/demo")).unwrap();
    fs::write(
        fixture.join(".ramify/work/demo/order.md"),
        "---\nworkflow: feature\nlane: light\nbranch: main\ntags: []\ncreated: 2026-11-01\nsteps: []\nask: {step: spec, asked: '2026-11-01', files: [spec.md]}\n---\n\n# demo\n",
    )
    .unwrap();
    fs::write(fixture.join("spec.md"), "The spec.\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let registry = temp("gates-registry").join("projects");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, format!("{}\n", fixture.display())).unwrap();
    let repo = repo_root();

    let shell_state = temp("gates-shell-state");
    let rust_state = temp("gates-rust-state");
    let shell = |args: &[&str]| {
        Command::new(repo.join("bin/ramify"))
            .args(args)
            .env("RAMIFY_SHELL", "1")
            .env("RAMIFY_STATE_DIR", &shell_state)
            .env("RAMIFY_REGISTRY", &registry)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_COLOR", "1")
            .output()
            .unwrap()
    };
    let rust = |args: &[&str]| {
        Command::new(env!("CARGO_BIN_EXE_ramify"))
            .args(args)
            .env_remove("RAMIFY_SHELL")
            .env("RAMIFY_STATE_DIR", &rust_state)
            .env("RAMIFY_REGISTRY", &registry)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_COLOR", "1")
            .output()
            .unwrap()
    };

    for args in [vec!["gates", "--scan"], vec!["gates", "--json"], vec!["gates"]] {
        let shell_out = shell(&args);
        let rust_out = rust(&args);
        assert_eq!(shell_out.status.code(), rust_out.status.code(), "{args:?} exit");
        assert_eq!(
            String::from_utf8_lossy(&shell_out.stdout),
            String::from_utf8_lossy(&rust_out.stdout),
            "{args:?} stdout differs"
        );
    }
}
