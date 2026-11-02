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

#[test]
fn label_matches_the_shell() {
    let fixture = temp("label");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("label-state");
    let registry = temp("label-registry").join("projects");
    let repo = repo_root();

    for args in [vec!["label"], vec!["label", "my-slug"]] {
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
fn lane_matches_the_shell() {
    let make = |name: &str| {
        let fixture = temp(name);
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
        fs::create_dir_all(fixture.join(".ramify/work/demo")).unwrap();
        fs::write(
            fixture.join(".ramify/work/demo/order.md"),
            "---\nworkflow: feature\nlane: light\nbranch: main\ntags: []\ncreated: 2026-11-01\nsteps:\n- {step: spec, date: '2026-11-01'}\n---\n\n# demo\n",
        )
        .unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        fixture.canonicalize().unwrap()
    };
    let shell_fixture = make("lane-shell");
    let rust_fixture = make("lane-rust");
    let repo = repo_root();
    let shell_state = temp("lane-shell-state");
    let rust_state = temp("lane-rust-state");
    let shell_registry = temp("lane-shell-registry").join("projects");
    let rust_registry = temp("lane-rust-registry").join("projects");

    for args in [vec!["lane", "standard"], vec!["lane", "light"], vec!["step"]] {
        let shell = run_args(
            &repo.join("bin/ramify"),
            &args,
            &shell_fixture,
            &shell_state,
            &shell_registry,
            true,
        );
        let rust = run_args(
            Path::new(env!("CARGO_BIN_EXE_ramify")),
            &args,
            &rust_fixture,
            &rust_state,
            &rust_registry,
            false,
        );
        assert_eq!(shell.status.code(), rust.status.code(), "{args:?} exit");
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{args:?} stdout differs"
        );
        assert_eq!(
            String::from_utf8_lossy(&shell.stderr),
            String::from_utf8_lossy(&rust.stderr),
            "{args:?} stderr differs"
        );
    }
}

#[test]
fn ping_matches_the_shell() {
    let fixture = temp("ping");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=3000\nweb_health=/health\nweb_cmd='true'\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("ping-state");
    let registry = temp("ping-registry").join("projects");
    let repo = repo_root();

    let shell = run_args(
        &repo.join("bin/ramify"),
        &["ping"],
        &fixture,
        &state,
        &registry,
        true,
    );
    let rust = run_args(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &["ping"],
        &fixture,
        &state,
        &registry,
        false,
    );
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout)
    );
    assert_eq!(
        String::from_utf8_lossy(&shell.stderr),
        String::from_utf8_lossy(&rust.stderr)
    );
}

#[test]
fn down_and_shared_down_match_the_shell() {
    let make = |name: &str| {
        let base = temp(name);
        let fixture = base.join("demo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(
            fixture.join(".ramify.conf"),
            "RAMIFY_SERVICES=\"web\"\nweb_port=3000\nweb_health=/health\nweb_cmd='true'\nweb_shared_port=3100\n",
        )
        .unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        let fixture = fixture.canonicalize().unwrap();
        let state = temp(&format!("{name}-state"));
        fs::create_dir_all(state.join("demo").join("slots")).unwrap();
        fs::write(
            state.join("demo").join("demo.env"),
            "SLOT=2\nWEB_PORT=3000\nWEB_SHARED=0\n",
        )
        .unwrap();
        fs::write(state.join("demo").join("slots").join("2"), "demo 999999\n").unwrap();
        let registry = temp(&format!("{name}-registry")).join("projects");
        (fixture, state, registry, "demo".to_string())
    };

    for args in [vec!["down"], vec!["shared-down"]] {
        let (shell_fixture, shell_state, shell_registry, _) = make(&format!("{args:?}-shell"));
        let (rust_fixture, rust_state, rust_registry, _) = make(&format!("{args:?}-rust"));
        let repo = repo_root();
        let shell = run_args(
            &repo.join("bin/ramify"),
            &args,
            &shell_fixture,
            &shell_state,
            &shell_registry,
            true,
        );
        let rust = run_args(
            Path::new(env!("CARGO_BIN_EXE_ramify")),
            &args,
            &rust_fixture,
            &rust_state,
            &rust_registry,
            false,
        );
        assert_eq!(shell.status.code(), rust.status.code(), "{args:?} exit");
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{args:?} stdout differs"
        );
        assert_eq!(
            String::from_utf8_lossy(&shell.stderr),
            String::from_utf8_lossy(&rust.stderr),
            "{args:?} stderr differs"
        );
    }
}

#[test]
fn up_starts_a_service_and_down_stops_it() {
    let base = 47000 + (std::process::id() % 500) * 10;
    let fixture = temp("up");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        format!(
            "RAMIFY_SERVICES=\"web\"\nweb_port={base}\nweb_cmd='sleep 30'\nweb_optional=1\nweb_timeout=1\nweb_health=/\n"
        ),
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
    let state = temp("up-state");
    let registry = temp("up-registry").join("projects");
    let binary = Path::new(env!("CARGO_BIN_EXE_ramify"));

    let started = run_args(binary, &["up"], &fixture, &state, &registry, false);
    assert_eq!(started.status.code(), Some(0), "up failed: {}", String::from_utf8_lossy(&started.stderr));
    let env = state.join(&name).join(format!("{name}.env"));
    let text = fs::read_to_string(&env).expect("env file");
    assert!(text.contains("SLOT=1"), "{text}");
    assert!(text.contains(&format!("WEB_PORT={}", base + 1)), "{text}");
    let pidfile = state.join(&name).join(format!("{name}.web.pid"));
    let pid = fs::read_to_string(&pidfile).expect("pid file");
    let pid = pid.trim().to_string();
    let alive = Command::new("ps").args(["-o", "pid=", "-p", &pid]).output().unwrap();
    assert!(!String::from_utf8_lossy(&alive.stdout).trim().is_empty(), "service not running");

    let stopped = run_args(binary, &["down"], &fixture, &state, &registry, false);
    assert_eq!(stopped.status.code(), Some(0), "down failed");
    assert!(!env.exists(), "env file left behind");
    assert!(!state.join(&name).join("slots").join("1").exists(), "slot not released");

    let _ = Command::new("kill").args(["-9", &pid]).status();
}

#[test]
fn write_set_matches_the_shell() {
    let fixture = temp("write-set");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    fs::create_dir_all(fixture.join("src")).unwrap();
    fs::create_dir_all(fixture.join("docs/adr")).unwrap();
    fs::write(fixture.join("src/app.rs"), "one\n").unwrap();
    fs::write(fixture.join("docs/adr/0001.md"), "one\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    fs::write(fixture.join("src/app.rs"), "two\n").unwrap();
    fs::write(fixture.join("docs/adr/0001.md"), "two\n").unwrap();

    let sets = [
        ("pass.txt", "src/*\ndocs/adr/*\n"),
        ("omit.txt", "src/*\n"),
        ("immutable.txt", "src/*\ndocs/adr/*\n!docs/adr/*\n"),
    ];
    for (name, content) in sets {
        fs::write(fixture.join(name), content).unwrap();
    }
    let repo = repo_root();
    for name in ["pass.txt", "omit.txt", "immutable.txt", "missing.txt"] {
        let path = fixture.join(name);
        let shell = Command::new(repo.join("bin/ramify"))
            .args(["write-set", &path.to_string_lossy()])
            .current_dir(&fixture)
            .env("BASE_REF", "HEAD")
            .env("RAMIFY_SHELL", "1")
            .env("RAMIFY_HOME", &repo)
            .output()
            .unwrap();
        let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
            .args(["write-set", &path.to_string_lossy()])
            .current_dir(&fixture)
            .env("BASE_REF", "HEAD")
            .env_remove("RAMIFY_SHELL")
            .env("RAMIFY_HOME", &repo)
            .output()
            .unwrap();
        assert_eq!(shell.status.code(), rust.status.code(), "{name} exit");
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{name} stdout"
        );
        assert_eq!(
            String::from_utf8_lossy(&shell.stderr),
            String::from_utf8_lossy(&rust.stderr),
            "{name} stderr"
        );
    }

    let shell = Command::new(repo.join("bin/ramify"))
        .args(["write-set", "--self-test"])
        .env("RAMIFY_SHELL", "1")
        .env("RAMIFY_HOME", &repo)
        .output()
        .unwrap();
    let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
        .args(["write-set", "--self-test"])
        .env_remove("RAMIFY_SHELL")
        .env("RAMIFY_HOME", &repo)
        .output()
        .unwrap();
    assert_eq!(shell.status.code(), rust.status.code(), "self-test exit");
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "self-test stdout"
    );
}

#[test]
fn mem_stacks_match_the_shell() {
    let fixture = temp("mem");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=47300\nweb_health=/\nweb_cmd='true'\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("mem-state");
    let registry = temp("mem-registry").join("projects");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, format!("{}\n", fixture.display())).unwrap();
    let repo = repo_root();

    let shell = Command::new(repo.join("bin/ramify"))
        .args(["mem"])
        .env("RAMIFY_SHELL", "1")
        .env("RAMIFY_STATE_DIR", &state)
        .env("RAMIFY_REGISTRY", &registry)
        .env("RAMIFY_HOME", &repo)
        .output()
        .unwrap();
    let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
        .args(["mem"])
        .env_remove("RAMIFY_SHELL")
        .env("RAMIFY_STATE_DIR", &state)
        .env("RAMIFY_REGISTRY", &registry)
        .env("RAMIFY_HOME", &repo)
        .output()
        .unwrap();
    assert_eq!(shell.status.code(), rust.status.code());
    let shell_text = String::from_utf8_lossy(&shell.stdout).into_owned();
    let rust_text = String::from_utf8_lossy(&rust.stdout).into_owned();
    assert!(rust_text.starts_with("machine"), "{rust_text}");
    assert_eq!(stacks_section(&shell_text), stacks_section(&rust_text));
}

fn stacks_section(text: &str) -> String {
    text.lines()
        .skip_while(|line| *line != "stacks")
        .take_while(|line| !line.is_empty())
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn reap_matches_the_shell() {
    use std::os::unix::process::CommandExt;
    let fixture = temp("reap");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=47400\nweb_health=/\nweb_cmd='true'\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let name = fixture.file_name().unwrap().to_string_lossy().into_owned();
    let registry = temp("reap-registry").join("projects");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, format!("{}\n", fixture.display())).unwrap();
    let repo = repo_root();

    let mut child = Command::new("sleep")
        .arg("30")
        .process_group(0)
        .spawn()
        .expect("sleep");
    let pid = child.id();
    let make_state = |suffix: &str| {
        let state = temp(suffix);
        let directory = state.join(&name);
        fs::create_dir_all(&directory).unwrap();
        fs::write(
            directory.join("gone.env"),
            "SLOT=1\nWEB_PORT=47401\nWEB_SHARED=0\n",
        )
        .unwrap();
        fs::write(directory.join("gone.web.pid"), format!("{pid}\n")).unwrap();
        state
    };
    let shell_state = make_state("reap-shell-state");
    let rust_state = make_state("reap-rust-state");

    let shell = Command::new(repo.join("bin/ramify"))
        .args(["reap"])
        .env("RAMIFY_SHELL", "1")
        .env("RAMIFY_STATE_DIR", &shell_state)
        .env("RAMIFY_REGISTRY", &registry)
        .env("RAMIFY_HOME", &repo)
        .output()
        .unwrap();
    let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
        .args(["reap"])
        .env_remove("RAMIFY_SHELL")
        .env("RAMIFY_STATE_DIR", &rust_state)
        .env("RAMIFY_REGISTRY", &registry)
        .env("RAMIFY_HOME", &repo)
        .output()
        .unwrap();
    let _ = child.kill();
    let _ = child.wait();

    assert_eq!(shell.status.code(), rust.status.code());
    let strip = |text: &str| {
        text.lines()
            .map(|line| line.chars().skip(19).collect::<String>())
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        strip(&String::from_utf8_lossy(&shell.stdout)),
        strip(&String::from_utf8_lossy(&rust.stdout)),
        "stdout differs"
    );
}

#[test]
fn new_matches_the_shell() {
    let make = |name: &str| {
        let base = temp(name);
        let bare = base.join("bare.git");
        git(&["init", "-q", "--bare", &bare.to_string_lossy()], &base);
        let fixture = base.join("demo-repo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        git(
            &["remote", "add", "origin", &bare.to_string_lossy()],
            &fixture,
        );
        git(&["push", "-q", "-u", "origin", "main"], &fixture);
        let fixture = fixture.canonicalize().unwrap();
        let worktrees = base.join("wt");
        let state = temp(&format!("{name}-state"));
        let registry = temp(&format!("{name}-registry")).join("projects");
        (fixture, worktrees, state, registry)
    };
    let (shell_fixture, shell_wt, shell_state, shell_registry) = make("new-shell");
    let (rust_fixture, rust_wt, rust_state, rust_registry) = make("new-rust");
    let repo = repo_root();

    let run = |binary: &Path,
               fixture: &Path,
               worktrees: &Path,
               state: &Path,
               registry: &Path,
               shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(["new", "demo", "--workflow", "feature"])
            .env("RAMIFY_ROOT", fixture)
            .env("RAMIFY_WORKTREES", worktrees)
            .env("RAMIFY_STATE_DIR", state)
            .env("RAMIFY_REGISTRY", registry)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_AUTOSTART", "0");
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    let shell = run(
        &repo.join("bin/ramify"),
        &shell_fixture,
        &shell_wt,
        &shell_state,
        &shell_registry,
        true,
    );
    let rust = run(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &rust_fixture,
        &rust_wt,
        &rust_state,
        &rust_registry,
        false,
    );
    assert_eq!(shell.status.code(), rust.status.code());
    let normalize = |text: &str, fixture: &Path, worktrees: &Path| {
        let mut text = text.replace(&fixture.to_string_lossy().into_owned(), "<FIXTURE>");
        let canonical = worktrees
            .canonicalize()
            .unwrap_or_else(|_| worktrees.to_path_buf());
        text = text.replace(&canonical.to_string_lossy().into_owned(), "<WT>");
        text.replace(&worktrees.to_string_lossy().into_owned(), "<WT>")
    };
    assert_eq!(
        normalize(
            &String::from_utf8_lossy(&shell.stdout),
            &shell_fixture,
            &shell_wt
        ),
        normalize(
            &String::from_utf8_lossy(&rust.stdout),
            &rust_fixture,
            &rust_wt
        ),
        "stdout differs"
    );
    let shell_order = fs::read_to_string(shell_wt.join("demo/.ramify/work/demo/order.md")).unwrap();
    let rust_order = fs::read_to_string(rust_wt.join("demo/.ramify/work/demo/order.md")).unwrap();
    assert_eq!(shell_order, rust_order, "order differs");
    let _ = Command::new("git")
        .args(["-C", &shell_fixture.to_string_lossy(), "worktree", "remove", "--force"])
        .arg(shell_wt.join("demo"))
        .status();
    let _ = Command::new("git")
        .args(["-C", &rust_fixture.to_string_lossy(), "worktree", "remove", "--force"])
        .arg(rust_wt.join("demo"))
        .status();
}

#[test]
fn cleanup_matches_the_shell() {
    let base_port = 47500;
    let make = |name: &str| {
        let base = temp(name);
        let fixture = base.join("demo-repo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(
            fixture.join(".ramify.conf"),
            format!("RAMIFY_SERVICES=\"web\"\nweb_port={base_port}\nweb_health=/\nweb_cmd='true'\n"),
        )
        .unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        let fixture = fixture.canonicalize().unwrap();
        let state = temp(&format!("{name}-state"));
        let directory = state.join("demo-repo");
        fs::create_dir_all(directory.join("slots")).unwrap();
        fs::write(directory.join("slots/1"), "ghost 999999\n").unwrap();
        fs::write(directory.join("slots/2"), "gone 999999\n").unwrap();
        fs::write(directory.join("slots/3"), "missing 999999\n").unwrap();
        fs::write(directory.join("gone.env"), "SLOT=1\nWEB_PORT=47502\nWEB_SHARED=0\n").unwrap();
        fs::write(directory.join("missing.env"), "SLOT=3\nWEB_PORT=47503\nWEB_SHARED=0\n").unwrap();
        fs::write(directory.join("dead.env"), "SLOT=4\nWEB_PORT=47504\nWEB_SHARED=0\n").unwrap();
        fs::write(directory.join("dead.web.pid"), "999999\n").unwrap();
        let registry = temp(&format!("{name}-registry")).join("projects");
        (fixture, state, registry)
    };
    let (shell_fixture, shell_state, shell_registry) = make("cleanup-shell");
    let (rust_fixture, rust_state, rust_registry) = make("cleanup-rust");
    let repo = repo_root();
    let run = |binary: &Path, fixture: &Path, state: &Path, registry: &Path, shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(["cleanup"])
            .env("RAMIFY_ROOT", fixture)
            .env("RAMIFY_STATE_DIR", state)
            .env("RAMIFY_REGISTRY", registry)
            .env("RAMIFY_HOME", &repo);
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    let shell = run(
        &repo.join("bin/ramify"),
        &shell_fixture,
        &shell_state,
        &shell_registry,
        true,
    );
    let rust = run(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &rust_fixture,
        &rust_state,
        &rust_registry,
        false,
    );
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
    assert_eq!(
        String::from_utf8_lossy(&shell.stderr),
        String::from_utf8_lossy(&rust.stderr),
        "stderr differs"
    );
}

#[test]
fn prune_and_delete_match_the_shell() {
    let make = |name: &str| {
        let base = temp(name);
        let bare = base.join("bare.git");
        git(&["init", "-q", "--bare", &bare.to_string_lossy()], &base);
        let fixture = base.join("demo-repo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
        fs::write(fixture.join("file.txt"), "one\n").unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        git(&["remote", "add", "origin", &bare.to_string_lossy()], &fixture);
        git(&["push", "-q", "-u", "origin", "main"], &fixture);
        git(&["checkout", "-q", "-b", "feat/old"], &fixture);
        fs::write(fixture.join("file.txt"), "two\n").unwrap();
        git(&["commit", "-qam", "old"], &fixture);
        git(&["checkout", "-q", "main"], &fixture);
        git(&["merge", "-q", "--no-ff", "feat/old", "-m", "merge old"], &fixture);
        git(&["push", "-q", "origin", "main"], &fixture);
        let fixture = fixture.canonicalize().unwrap();
        let worktrees = base.join("wt");
        fs::create_dir_all(worktrees.join("old")).unwrap();
        git(
            &["worktree", "add", "-q", &worktrees.join("old").to_string_lossy(), "feat/old"],
            &fixture,
        );
        let state = temp(&format!("{name}-state"));
        let registry = temp(&format!("{name}-registry")).join("projects");
        (fixture, worktrees, state, registry)
    };
    let (shell_fixture, shell_wt, shell_state, shell_registry) = make("prune-shell");
    let (rust_fixture, rust_wt, rust_state, rust_registry) = make("prune-rust");
    let repo = repo_root();
    let run = |binary: &Path, fixture: &Path, state: &Path, registry: &Path, shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(["prune"])
            .env("RAMIFY_ROOT", fixture)
            .env("RAMIFY_STATE_DIR", state)
            .env("RAMIFY_REGISTRY", registry)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_NO_DOCKER", "1");
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    let shell = run(&repo.join("bin/ramify"), &shell_fixture, &shell_state, &shell_registry, true);
    let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), &rust_fixture, &rust_state, &rust_registry, false);
    assert_eq!(shell.status.code(), rust.status.code());
    let normalize = |text: &str, worktrees: &Path| {
        let canonical = worktrees.canonicalize().unwrap_or_else(|_| worktrees.to_path_buf());
        let text = text.replace(&canonical.to_string_lossy().into_owned(), "<WT>");
        text.replace(&worktrees.to_string_lossy().into_owned(), "<WT>")
    };
    assert_eq!(
        normalize(&String::from_utf8_lossy(&shell.stdout), &shell_wt),
        normalize(&String::from_utf8_lossy(&rust.stdout), &rust_wt),
        "prune stdout differs"
    );

    for (fixture, worktrees) in [(&shell_fixture, &shell_wt), (&rust_fixture, &rust_wt)] {
        let _ = Command::new("git")
            .args(["-C", &fixture.to_string_lossy(), "worktree", "remove", "--force"])
            .arg(worktrees.join("old"))
            .status();
        let _ = Command::new("git")
            .args(["-C", &fixture.to_string_lossy(), "branch", "-D", "-q", "feat/old"])
            .status();
        fs::create_dir_all(worktrees.join("old")).unwrap();
        let _ = Command::new("git")
            .args(["-C", &fixture.to_string_lossy(), "worktree", "add", "-q"])
            .arg(worktrees.join("old"))
            .arg("--detach")
            .status();
    }

    let shell_delete = {
        let mut command = Command::new(repo.join("bin/ramify"));
        command
            .args(["delete", "old", "--yes"])
            .env("RAMIFY_ROOT", &shell_fixture)
            .env("RAMIFY_STATE_DIR", &shell_state)
            .env("RAMIFY_REGISTRY", &shell_registry)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_SHELL", "1");
        command.output().unwrap()
    };
    let rust_delete = {
        let mut command = Command::new(env!("CARGO_BIN_EXE_ramify"));
        command
            .args(["delete", "old", "--yes"])
            .env("RAMIFY_ROOT", &rust_fixture)
            .env("RAMIFY_STATE_DIR", &rust_state)
            .env("RAMIFY_REGISTRY", &rust_registry)
            .env("RAMIFY_HOME", &repo);
        command.env_remove("RAMIFY_SHELL");
        command.output().unwrap()
    };
    assert_eq!(shell_delete.status.code(), rust_delete.status.code(), "delete exit");
    assert_eq!(
        String::from_utf8_lossy(&shell_delete.stdout),
        String::from_utf8_lossy(&rust_delete.stdout),
        "delete stdout differs"
    );
}

#[test]
fn complete_matches_the_shell() {
    let make = |name: &str| {
        let base = temp(name);
        let bare = base.join("bare.git");
        git(&["init", "-q", "--bare", &bare.to_string_lossy()], &base);
        let fixture = base.join("demo-repo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
        fs::write(fixture.join("file.txt"), "one\n").unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        git(&["remote", "add", "origin", &bare.to_string_lossy()], &fixture);
        git(&["push", "-q", "-u", "origin", "main"], &fixture);
        git(&["branch", "feat/done"], &fixture);
        let fixture = fixture.canonicalize().unwrap();
        let worktrees = base.join("wt");
        fs::create_dir_all(&worktrees).unwrap();
        git(
            &["worktree", "add", "-q", &worktrees.join("done").to_string_lossy(), "feat/done"],
            &fixture,
        );
        let state = temp(&format!("{name}-state"));
        let registry = temp(&format!("{name}-registry")).join("projects");
        (fixture, worktrees, state, registry)
    };
    let (shell_fixture, _shell_wt, shell_state, shell_registry) = make("complete-shell");
    let (rust_fixture, _rust_wt, rust_state, rust_registry) = make("complete-rust");
    let repo = repo_root();
    let run = |binary: &Path, fixture: &Path, state: &Path, registry: &Path, shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(["complete", "done", "--yes"])
            .env("RAMIFY_ROOT", fixture)
            .env("RAMIFY_STATE_DIR", state)
            .env("RAMIFY_REGISTRY", registry)
            .env("RAMIFY_HOME", &repo);
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    let shell = run(&repo.join("bin/ramify"), &shell_fixture, &shell_state, &shell_registry, true);
    let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), &rust_fixture, &rust_state, &rust_registry, false);
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        String::from_utf8_lossy(&shell.stdout),
        String::from_utf8_lossy(&rust.stdout),
        "stdout differs"
    );
    assert_eq!(
        String::from_utf8_lossy(&shell.stderr),
        String::from_utf8_lossy(&rust.stderr),
        "stderr differs"
    );
}

#[test]
fn statusline_matches_the_shell() {
    use std::io::Write;
    let fixture = temp("statusline");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    fs::create_dir_all(fixture.join(".ramify/work/demo")).unwrap();
    fs::write(
        fixture.join(".ramify/work/demo/order.md"),
        "---\nworkflow: feature\nlane: light\nbranch: main\ntags: []\ncreated: 2026-11-01\nsteps: []\n---\n\n# demo\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("statusline-state");
    let repo = repo_root();
    let input = format!(
        "{{\"session_id\":\"test\",\"workspace\":{{\"current_dir\":\"{}\"}}}}",
        fixture.display()
    );

    let run = |binary: &Path, args: &[&str], shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(args)
            .env("RAMIFY_STATE_DIR", &state)
            .env("RAMIFY_HOME", &repo)
            .stdin(std::process::Stdio::piped())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped());
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        let mut child = command.spawn().unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(input.as_bytes())
            .unwrap();
        child.wait_with_output().unwrap()
    };
    for args in [vec!["statusline"], vec!["statusline", "--json"]] {
        let shell = run(&repo.join("bin/ramify"), &args, true);
        let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), &args, false);
        assert_eq!(shell.status.code(), rust.status.code(), "{args:?} exit");
        let shell_text = String::from_utf8_lossy(&shell.stdout)
            .replace(&fixture.to_string_lossy().into_owned(), "<F>");
        let rust_text = String::from_utf8_lossy(&rust.stdout)
            .replace(&fixture.to_string_lossy().into_owned(), "<F>");
        assert_eq!(shell_text, rust_text, "{args:?} stdout differs");
    }
}

fn normalize_run(text: &str, directory: &Path) -> String {
    let regex = regex::Regex::new(r"[0-9]{8}-[0-9]{6}-[0-9]+").unwrap();
    let text = regex.replace_all(text, "<ID>").into_owned();
    let canonical = directory.canonicalize().unwrap_or_else(|_| directory.to_path_buf());
    text.replace(&canonical.to_string_lossy().into_owned(), "<DIR>")
        .replace(&directory.to_string_lossy().into_owned(), "<DIR>")
}

#[test]
fn scratch_matches_the_shell() {
    let repo = repo_root();
    let shell_dir = temp("scratch-shell");
    let rust_dir = temp("scratch-rust");
    let text = temp("scratch-text").join("note.txt");
    fs::write(&text, "a message to copy\n").unwrap();

    for args in [
        vec!["scratch".to_string(), "--title".into(), "Note".into(), text.to_string_lossy().into_owned()],
        vec!["scratch".into(), "--list".into()],
        vec!["scratch".into(), "--clear".into()],
        vec!["scratch".into(), "--list".into()],
    ] {
        let shell = Command::new(repo.join("bin/ramify"))
            .args(&args)
            .env("RAMIFY_SCRATCH_DIR", &shell_dir)
            .env("RAMIFY_POPUP", "0")
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_SHELL", "1")
            .output()
            .unwrap();
        let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
            .args(&args)
            .env("RAMIFY_SCRATCH_DIR", &rust_dir)
            .env("RAMIFY_POPUP", "0")
            .env("RAMIFY_HOME", &repo)
            .env_remove("RAMIFY_SHELL")
            .output()
            .unwrap();
        assert_eq!(shell.status.code(), rust.status.code(), "{args:?} exit");
        assert_eq!(
            normalize_run(&String::from_utf8_lossy(&shell.stdout), &shell_dir),
            normalize_run(&String::from_utf8_lossy(&rust.stdout), &rust_dir),
            "{args:?} stdout differs"
        );
    }
}

#[test]
fn run_matches_the_shell() {
    let repo = repo_root();
    let shell_dir = temp("run-shell");
    let rust_dir = temp("run-rust");
    let script = temp("run-script").join("task.sh");
    fs::write(&script, "echo hello\n").unwrap();

    let ask = |binary: &Path, directory: &Path, shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(["run", "ask", &script.to_string_lossy(), "--note", "try it"])
            .env("RAMIFY_RUNS_DIR", directory)
            .env("RAMIFY_POPUP", "0")
            .env("RAMIFY_HOME", &repo);
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    let id_of = |output: &std::process::Output| {
        let text = String::from_utf8_lossy(&output.stdout).into_owned();
        let regex = regex::Regex::new(r"[0-9]{8}-[0-9]{6}-[0-9]+").unwrap();
        regex.find(&text).map(|found| found.as_str().to_string())
    };
    let shell = ask(&repo.join("bin/ramify"), &shell_dir, true);
    let rust = ask(Path::new(env!("CARGO_BIN_EXE_ramify")), &rust_dir, false);
    assert_eq!(shell.status.code(), rust.status.code());
    assert_eq!(
        normalize_run(&String::from_utf8_lossy(&shell.stdout), &shell_dir),
        normalize_run(&String::from_utf8_lossy(&rust.stdout), &rust_dir)
    );
    let shell_id = id_of(&shell).expect("shell id");
    let rust_id = id_of(&rust).expect("rust id");

    let sub = |binary: &Path,
               directory: &Path,
               id: &str,
               args: &[&str],
               shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(args)
            .arg(id)
            .env("RAMIFY_RUNS_DIR", directory)
            .env("RAMIFY_POPUP", "0")
            .env("RAMIFY_HOME", &repo);
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    for args in [vec!["run", "go"], vec!["run", "wait"]] {
        let shell_out = sub(
            &repo.join("bin/ramify"),
            &shell_dir,
            &shell_id,
            &args,
            true,
        );
        let rust_out = sub(
            Path::new(env!("CARGO_BIN_EXE_ramify")),
            &rust_dir,
            &rust_id,
            &args,
            false,
        );
        assert_eq!(shell_out.status.code(), rust_out.status.code(), "{args:?} exit");
        assert_eq!(
            normalize_run(&String::from_utf8_lossy(&shell_out.stdout), &shell_dir),
            normalize_run(&String::from_utf8_lossy(&rust_out.stdout), &rust_dir),
            "{args:?} stdout differs"
        );
    }

    let shell = ask(&repo.join("bin/ramify"), &shell_dir, true);
    let rust = ask(Path::new(env!("CARGO_BIN_EXE_ramify")), &rust_dir, false);
    let shell_id = id_of(&shell).expect("shell id");
    let rust_id = id_of(&rust).expect("rust id");
    let shell_out = sub(
        &repo.join("bin/ramify"),
        &shell_dir,
        &shell_id,
        &["run", "block", "--note", "not now"],
        true,
    );
    let rust_out = sub(
        Path::new(env!("CARGO_BIN_EXE_ramify")),
        &rust_dir,
        &rust_id,
        &["run", "block", "--note", "not now"],
        false,
    );
    assert_eq!(shell_out.status.code(), rust_out.status.code());
    assert_eq!(
        normalize_run(&String::from_utf8_lossy(&shell_out.stdout), &shell_dir),
        normalize_run(&String::from_utf8_lossy(&rust_out.stdout), &rust_dir),
        "block stdout differs"
    );
}

#[test]
fn dash_human_matches_the_shell() {
    let fixture = temp("dash-human");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(
        fixture.join(".ramify.conf"),
        "RAMIFY_SERVICES=\"web\"\nweb_port=47600\nweb_health=/\nweb_cmd='true'\nweb_shared_port=47610\n",
    )
    .unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let state = temp("dash-human-state");
    let registry = temp("dash-human-registry").join("projects");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, format!("{}\n", fixture.display())).unwrap();
    let repo = repo_root();
    let run = |binary: &Path, shell_mode: bool| {
        let mut command = Command::new(binary);
        command
            .args(["dash"])
            .env("RAMIFY_STATE_DIR", &state)
            .env("RAMIFY_REGISTRY", &registry)
            .env("RAMIFY_HOME", &repo);
        if shell_mode {
            command.env("RAMIFY_SHELL", "1");
        } else {
            command.env_remove("RAMIFY_SHELL");
        }
        command.output().unwrap()
    };
    let shell = run(&repo.join("bin/ramify"), true);
    let rust = run(Path::new(env!("CARGO_BIN_EXE_ramify")), false);
    assert_eq!(shell.status.code(), rust.status.code());
    let mask = |text: &str| {
        text.lines()
            .filter(|line| !line.starts_with("build servers"))
            .collect::<Vec<_>>()
            .join("\n")
    };
    assert_eq!(
        mask(&String::from_utf8_lossy(&shell.stdout)),
        mask(&String::from_utf8_lossy(&rust.stdout)),
        "stdout differs"
    );
}

#[test]
fn jira_errors_match_the_shell() {
    let fixture = temp("jira");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let repo = repo_root();
    for url in [None, Some("https://example.atlassian.net")] {
        let mut shell = Command::new(repo.join("bin/ramify"));
        shell
            .args(["jira", "check"])
            .env("RAMIFY_ROOT", &fixture)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_SHELL", "1")
            .env_remove("RAMIFY_JIRA_URL")
            .env_remove("RAMIFY_JIRA_EMAIL")
            .env_remove("RAMIFY_JIRA_TOKEN");
        let mut rust = Command::new(env!("CARGO_BIN_EXE_ramify"));
        rust.args(["jira", "check"])
            .env("RAMIFY_ROOT", &fixture)
            .env("RAMIFY_HOME", &repo)
            .env_remove("RAMIFY_SHELL")
            .env_remove("RAMIFY_JIRA_URL")
            .env_remove("RAMIFY_JIRA_EMAIL")
            .env_remove("RAMIFY_JIRA_TOKEN");
        if let Some(url) = url {
            shell.env("RAMIFY_JIRA_URL", url);
            rust.env("RAMIFY_JIRA_URL", url);
        }
        let shell = shell.output().unwrap();
        let rust = rust.output().unwrap();
        assert_eq!(shell.status.code(), rust.status.code(), "{url:?} exit");
        assert_eq!(
            String::from_utf8_lossy(&shell.stderr),
            String::from_utf8_lossy(&rust.stderr),
            "{url:?} stderr"
        );
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{url:?} stdout"
        );
    }
}

#[test]
fn ticket_without_jira_exits_three() {
    let fixture = temp("ticket");
    git(&["init", "-q", "-b", "main"], &fixture);
    fs::write(fixture.join(".ramify.conf"), "RAMIFY_SERVICES=\"\"\n").unwrap();
    git(&["add", "."], &fixture);
    git(&["commit", "-qm", "init"], &fixture);
    let fixture = fixture.canonicalize().unwrap();
    let repo = repo_root();
    for action in ["start", "sync"] {
        let shell = Command::new(repo.join("bin/ramify"))
            .args(["ticket", action])
            .env("RAMIFY_ROOT", &fixture)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_SHELL", "1")
            .env_remove("RAMIFY_JIRA_URL")
            .output()
            .unwrap();
        let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
            .args(["ticket", action])
            .env("RAMIFY_ROOT", &fixture)
            .env("RAMIFY_HOME", &repo)
            .env_remove("RAMIFY_SHELL")
            .env_remove("RAMIFY_JIRA_URL")
            .output()
            .unwrap();
        assert_eq!(shell.status.code(), rust.status.code(), "{action} exit");
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{action} stdout"
        );
    }
}

#[test]
fn setup_draft_matches_the_shell() {
    let repo = repo_root();
    let registry = temp("setup-registry").join("projects");
    fs::create_dir_all(registry.parent().unwrap()).unwrap();
    fs::write(&registry, "").unwrap();

    let make_plain = |name: &str| {
        let base = temp(name);
        let fixture = base.join("plain-repo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(fixture.join(".gitignore"), ".env\n").unwrap();
        fs::write(fixture.join(".env"), "SECRET=1\n").unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        fixture.canonicalize().unwrap()
    };
    let make_vite = |name: &str| {
        let base = temp(name);
        let fixture = base.join("vite-repo");
        fs::create_dir_all(&fixture).unwrap();
        git(&["init", "-q", "-b", "main"], &fixture);
        fs::write(
            fixture.join("package.json"),
            "{\"scripts\":{\"dev\":\"vite\"},\"devDependencies\":{\"vite\":\"5.0.0\"}}\n",
        )
        .unwrap();
        fs::write(fixture.join("package-lock.json"), "").unwrap();
        fs::write(fixture.join(".gitignore"), ".env\n").unwrap();
        fs::write(fixture.join(".env"), "SECRET=1\n").unwrap();
        git(&["add", "."], &fixture);
        git(&["commit", "-qm", "init"], &fixture);
        git(&["branch", "feat/foo"], &fixture);
        fixture.canonicalize().unwrap()
    };

    for fixture in [make_plain("setup-plain"), make_vite("setup-vite")] {
        let shell = Command::new(repo.join("bin/ramify"))
            .args(["setup", "--print"])
            .env("RAMIFY_ROOT", &fixture)
            .env("RAMIFY_REGISTRY", &registry)
            .env("RAMIFY_HOME", &repo)
            .env("RAMIFY_SHELL", "1")
            .output()
            .unwrap();
        let rust = Command::new(env!("CARGO_BIN_EXE_ramify"))
            .args(["setup", "--print"])
            .env("RAMIFY_ROOT", &fixture)
            .env("RAMIFY_REGISTRY", &registry)
            .env("RAMIFY_HOME", &repo)
            .env_remove("RAMIFY_SHELL")
            .output()
            .unwrap();
        assert_eq!(shell.status.code(), rust.status.code(), "{} exit", fixture.display());
        assert_eq!(
            String::from_utf8_lossy(&shell.stdout),
            String::from_utf8_lossy(&rust.stdout),
            "{} stdout differs",
            fixture.display()
        );
        assert_eq!(
            String::from_utf8_lossy(&shell.stderr),
            String::from_utf8_lossy(&rust.stderr),
            "{} stderr differs",
            fixture.display()
        );
    }
}
