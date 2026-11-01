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
        .env("RAMIFY_HOME", repo_root());
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
