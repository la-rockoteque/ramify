//! Ram Code finds its updates, shows them, and installs them with `ram update`.

use ram_code::app::{App, Screen};
use ram_code::ramify::{CommandOutput, CommandSpec, Runner};
use ram_code::update::{check, install_of, Found, Install};
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};
use std::time::{Duration, Instant};

/// The variables below are process-wide: one test at a time.
static ENV: Mutex<()> = Mutex::new(());

fn serial() -> MutexGuard<'static, ()> {
    let guard = ENV.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    for key in [
        "RAM_CODE_HOME",
        "RAM_CODE_CACHE_DIR",
        "RAM_CODE_CONFIG_DIR",
        "RAM_CODE_NO_UPDATE",
        "RAM_CODE_UPDATER",
        "RAM_CODE_BUILD",
        "RAMIFY_RELEASES",
    ] {
        std::env::remove_var(key);
    }
    guard
}

fn tmp(name: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("ram-update-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.canonicalize().unwrap()
}

fn sh(script: &str, cwd: &Path) -> String {
    let output = Command::new("sh")
        .arg("-c")
        .arg(script)
        .current_dir(cwd)
        .env("GIT_AUTHOR_NAME", "t")
        .env("GIT_AUTHOR_EMAIL", "t@t")
        .env("GIT_COMMITTER_NAME", "t")
        .env("GIT_COMMITTER_EMAIL", "t@t")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{script}: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// A local releases server. `/latest` redirects to `/tag/<latest>`; other paths serve `files`.
/// With `silent`, it accepts and never answers.
struct Server {
    url: String,
    requests: Arc<AtomicUsize>,
}

fn server(latest: &str, files: Vec<(String, Vec<u8>)>, silent: bool) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}/releases", listener.local_addr().unwrap());
    let requests = Arc::new(AtomicUsize::new(0));
    let (count, latest, base) = (requests.clone(), latest.to_string(), url.clone());
    std::thread::spawn(move || {
        let mut held = Vec::new();
        for stream in listener.incoming().flatten() {
            count.fetch_add(1, Ordering::SeqCst);
            if silent {
                held.push(stream);
                continue;
            }
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut first = String::new();
            let _ = reader.read_line(&mut first);
            loop {
                let mut line = String::new();
                if reader.read_line(&mut line).unwrap_or(0) == 0 || line == "\r\n" {
                    break;
                }
            }
            let path = first.split_whitespace().nth(1).unwrap_or("").to_string();
            let mut stream = stream;
            if path.ends_with("/releases/latest") {
                let head = format!(
                    "HTTP/1.1 302 Found\r\nLocation: {base}/tag/{latest}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n"
                );
                let _ = stream.write_all(head.as_bytes());
                continue;
            }
            match files.iter().find(|(name, _)| path.ends_with(name.as_str())) {
                Some((_, body)) => {
                    let head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len()
                    );
                    let _ = stream.write_all(head.as_bytes());
                    let _ = stream.write_all(body);
                }
                None => {
                    let _ = stream.write_all(
                        b"HTTP/1.1 404 Not Found\r\nContent-Length: 0\r\nConnection: close\r\n\r\n",
                    );
                }
            }
        }
    });
    Server { url, requests }
}

/// A release install folder: the launchers, the binaries, `RELEASE` and the real installer.
fn release_tree(dir: &Path, tag: &str) {
    for file in [
        "bin/ram",
        "bin/ramify",
        "target/release/ram-code",
        "cli/target/release/ramify",
    ] {
        let path = dir.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, "#!/bin/sh\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    }
    std::fs::write(dir.join("Cargo.toml"), "[package]\n").unwrap();
    std::fs::write(dir.join("RELEASE"), format!("{tag}\n")).unwrap();
    std::fs::copy(
        Path::new(env!("CARGO_MANIFEST_DIR")).join("install.sh"),
        dir.join("install.sh"),
    )
    .unwrap();
}

/// The archive name `install.sh` asks for on this machine.
fn asset() -> String {
    let os = if cfg!(target_os = "macos") {
        "apple-darwin"
    } else {
        "unknown-linux-gnu"
    };
    let arch = if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86_64"
    };
    format!("ramify-{arch}-{os}.tar.gz")
}

/// The files of a release `tag`: its archive and its checksum, the checksum wrong when `bad`.
fn release_files(base: &Path, tag: &str, bad: bool) -> Vec<(String, Vec<u8>)> {
    let tree = base.join(format!("tree-{tag}"));
    std::fs::create_dir_all(&tree).unwrap();
    release_tree(&tree, tag);
    let archive = base.join(asset());
    sh(
        &format!("tar -C '{}' -czf '{}' .", tree.display(), archive.display()),
        base,
    );
    let sum = sh(
        &format!("shasum -a 256 '{}' | cut -d' ' -f1", archive.display()),
        base,
    );
    let sum = if bad { "0".repeat(64) } else { sum };
    vec![
        (
            format!("/latest/download/{}", asset()),
            std::fs::read(&archive).unwrap(),
        ),
        (
            format!("/latest/download/{}.sha256", asset()),
            format!("{sum}  {}\n", asset()).into_bytes(),
        ),
    ]
}

/// An `origin` repo with `main`, and a clone of it. Returns (origin, clone).
fn checkout(base: &Path) -> (PathBuf, PathBuf) {
    let origin = base.join("origin.git");
    sh(
        &format!("git init -q --bare -b main '{}'", origin.display()),
        base,
    );
    sh(&format!("git clone -q '{}' seed 2>/dev/null; cd seed && git checkout -q -b main && git commit -q --allow-empty -m one && git push -q origin main", origin.display()), base);
    sh(&format!("git clone -q '{}' clone", origin.display()), base);
    (origin, base.join("clone"))
}

/// Adds `n` commits to `origin/main`.
fn advance(base: &Path, n: usize) {
    for index in 0..n {
        sh(
            &format!(
                "cd seed && git commit -q --allow-empty -m more{index} && git push -q origin main"
            ),
            base,
        );
    }
}

#[test]
fn install_kind_is_read_from_the_folder() {
    let _guard = serial();
    let base = tmp("kind");
    let release = base.join("release");
    release_tree(&release, "v0.2.0");
    assert_eq!(
        install_of(&release),
        Install::Release {
            home: release.clone(),
            tag: "v0.2.0".into()
        }
    );
    let (_, clone) = checkout(&base);
    let commit = sh("git rev-parse --short HEAD", &clone);
    assert_eq!(
        install_of(&clone),
        Install::Checkout {
            home: clone.clone(),
            commit
        }
    );
    let empty = base.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    assert_eq!(install_of(&empty), Install::Unknown);
    let server = server("v9.0.0", Vec::new(), false);
    assert_eq!(
        check(&Install::Unknown, &base.join("cache"), &server.url),
        None
    );
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
}

#[test]
fn a_newer_release_is_an_update() {
    let _guard = serial();
    let base = tmp("newer");
    release_tree(&base.join("home"), "v0.2.0");
    let server = server("v0.3.0", Vec::new(), false);
    let found = check(
        &install_of(&base.join("home")),
        &base.join("cache"),
        &server.url,
    );
    assert_eq!(found, Some(Found::Release("v0.3.0".into())));
}

#[test]
fn the_same_release_is_no_update() {
    let _guard = serial();
    let base = tmp("same");
    release_tree(&base.join("home"), "v0.2.0");
    let server = server("v0.2.0", Vec::new(), false);
    assert_eq!(
        check(
            &install_of(&base.join("home")),
            &base.join("cache"),
            &server.url
        ),
        None
    );
}

#[test]
fn a_checkout_behind_main_counts_the_commits() {
    let _guard = serial();
    let base = tmp("behind");
    let (_, clone) = checkout(&base);
    advance(&base, 4);
    let found = check(
        &install_of(&clone),
        &base.join("cache"),
        "http://127.0.0.1:1",
    );
    assert_eq!(found, Some(Found::Commits(4)));
}

#[test]
fn a_fresh_cache_makes_no_request() {
    let _guard = serial();
    let base = tmp("fresh");
    release_tree(&base.join("home"), "v0.2.0");
    let server = server("v0.3.0", Vec::new(), false);
    let install = install_of(&base.join("home"));
    let cache = base.join("cache");
    check(&install, &cache, &server.url);
    let found = check(&install, &cache, &server.url);
    assert_eq!(found, Some(Found::Release("v0.3.0".into())));
    assert_eq!(server.requests.load(Ordering::SeqCst), 1);
    // A checkout answers from the cache with no `git fetch`: its origin is gone.
    let (_, clone) = checkout(&base);
    advance(&base, 1);
    let install = install_of(&clone);
    let cache = base.join("cache-checkout");
    assert_eq!(check(&install, &cache, ""), Some(Found::Commits(1)));
    sh("git remote remove origin", &clone);
    assert_eq!(check(&install, &cache, ""), Some(Found::Commits(1)));
}

#[test]
fn an_old_cache_checks_again() {
    let _guard = serial();
    let base = tmp("old");
    release_tree(&base.join("home"), "v0.2.0");
    let server = server("v0.3.0", Vec::new(), false);
    let install = install_of(&base.join("home"));
    let cache = base.join("cache");
    check(&install, &cache, &server.url);
    let file = cache.join("update.json");
    let mut record: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(&file).unwrap()).unwrap();
    record["checked_at"] = serde_json::json!(record["checked_at"].as_u64().unwrap() - 25 * 3600);
    std::fs::write(&file, record.to_string()).unwrap();
    check(&install, &cache, &server.url);
    assert_eq!(server.requests.load(Ordering::SeqCst), 2);
}

#[test]
fn a_silent_server_ends_the_check_in_time() {
    let _guard = serial();
    let base = tmp("silent");
    release_tree(&base.join("home"), "v0.2.0");
    let server = server("v0.3.0", Vec::new(), true);
    let started = Instant::now();
    let found = check(
        &install_of(&base.join("home")),
        &base.join("cache"),
        &server.url,
    );
    assert_eq!(found, None);
    assert!(
        started.elapsed() <= Duration::from_secs(11),
        "{:?}",
        started.elapsed()
    );
    assert!(
        !base.join("cache/update.json").exists(),
        "a failed check was cached"
    );
}

struct NoRunner;

impl Runner for NoRunner {
    fn run(&self, _spec: &CommandSpec) -> Result<CommandOutput, String> {
        Err("no runner in this test".into())
    }
}

fn app(base: &Path) -> App {
    std::env::set_var("RAM_CODE_CONFIG_DIR", base.join("config"));
    let mut app = App::new(Arc::new(NoRunner), base.to_path_buf());
    app.screen = Screen::Main;
    app
}

fn render_text(app: &App) -> String {
    use ratatui::backend::TestBackend;
    let mut terminal = ratatui::Terminal::new(TestBackend::new(100, 20)).unwrap();
    terminal
        .draw(|frame| frame.render_widget(app, frame.area()))
        .unwrap();
    let buffer = terminal.backend().buffer();
    (0..buffer.area.height)
        .map(|y| {
            (0..buffer.area.width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
                + "\n"
        })
        .collect()
}

#[test]
fn the_header_shows_the_update() {
    let _guard = serial();
    let base = tmp("header");
    let mut app = app(&base);
    assert!(!render_text(&app)
        .lines()
        .any(|line| line.trim_start().starts_with("update")));
    app.update_found = Some(Found::Release("v0.3.0".into()));
    assert!(
        render_text(&app).contains("update v0.3.0 → ram update"),
        "{}",
        render_text(&app)
    );
    app.update_found = Some(Found::Commits(4));
    assert!(render_text(&app).contains("update: 4 commits on main → ram update"));
}

fn ram(args: &[&str], envs: &[(&str, &Path)], extra: &[(&str, &str)]) -> (i32, String) {
    let mut command = Command::new(env!("CARGO_BIN_EXE_ram-code"));
    command.args(args);
    for (key, value) in envs {
        command.env(key, value);
    }
    for (key, value) in extra {
        command.env(key, value);
    }
    let output = command.output().unwrap();
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    (output.status.code().unwrap_or(-1), text)
}

#[test]
fn ram_update_installs_a_newer_release() {
    let _guard = serial();
    let base = tmp("install");
    let home = base.join("home");
    release_tree(&home, "v0.2.0");
    let server = server("v0.3.0", release_files(&base, "v0.3.0", false), false);
    let (code, text) = ram(
        &["update"],
        &[
            ("RAM_CODE_HOME", &home),
            ("RAMIFY_BIN_DIR", &base.join("bin")),
            ("RAM_CODE_CACHE_DIR", &base.join("cache")),
        ],
        &[("RAMIFY_RELEASES", &server.url)],
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        std::fs::read_to_string(home.join("RELEASE"))
            .unwrap()
            .trim(),
        "v0.3.0"
    );
    assert!(text.contains("ram updated from v0.2.0 to v0.3.0"), "{text}");
}

#[test]
fn a_bad_checksum_keeps_the_old_release() {
    let _guard = serial();
    let base = tmp("checksum");
    let home = base.join("home");
    release_tree(&home, "v0.2.0");
    let server = server("v0.3.0", release_files(&base, "v0.3.0", true), false);
    let (code, text) = ram(
        &["update"],
        &[
            ("RAM_CODE_HOME", &home),
            ("RAMIFY_BIN_DIR", &base.join("bin")),
            ("RAM_CODE_CACHE_DIR", &base.join("cache")),
        ],
        &[("RAMIFY_RELEASES", &server.url)],
    );
    assert_eq!(code, 1, "{text}");
    assert_eq!(
        std::fs::read_to_string(home.join("RELEASE"))
            .unwrap()
            .trim(),
        "v0.2.0"
    );
}

#[test]
fn ram_update_fast_forwards_a_checkout() {
    let _guard = serial();
    let base = tmp("forward");
    let (_, clone) = checkout(&base);
    advance(&base, 2);
    let log = base.join("build.log");
    let build = format!("echo \"$1\" >> '{}'", log.display());
    let (code, text) = ram(
        &["update"],
        &[("RAM_CODE_HOME", &clone)],
        &[("RAM_CODE_BUILD", &build)],
    );
    assert_eq!(code, 0, "{text}");
    assert_eq!(
        sh("git rev-parse main", &clone),
        sh("git rev-parse origin/main", &clone)
    );
    assert_eq!(
        std::fs::read_to_string(&log).unwrap(),
        "cli/Cargo.toml\nCargo.toml\n"
    );
}

#[test]
fn ram_update_refuses_a_checkout_it_must_not_move() {
    let _guard = serial();
    let base = tmp("refuse");
    let (_, clone) = checkout(&base);
    advance(&base, 1);
    let log = base.join("build.log");
    let build = format!("echo \"$1\" >> '{}'", log.display());
    let attempt = |reason: &str| {
        let head = sh("git rev-parse HEAD", &clone);
        let (code, text) = ram(
            &["update"],
            &[("RAM_CODE_HOME", &clone)],
            &[("RAM_CODE_BUILD", &build)],
        );
        assert_eq!(code, 1, "{text}");
        assert!(text.contains(reason), "{text}");
        assert_eq!(sh("git rev-parse HEAD", &clone), head);
    };
    sh("git checkout -q -b feat/x", &clone);
    attempt("not on main");
    sh(
        "git checkout -q main && echo x > tracked && git add tracked",
        &clone,
    );
    attempt("uncommitted changes");
    sh("git commit -q -m local", &clone);
    attempt("only fast-forwards");
    assert!(!log.exists(), "a refused update built");
}

#[test]
fn ram_update_with_no_update_says_so() {
    let _guard = serial();
    let base = tmp("current");
    let home = base.join("home");
    release_tree(&home, "v0.2.0");
    let server = server("v0.2.0", Vec::new(), false);
    let (code, text) = ram(
        &["update"],
        &[("RAM_CODE_HOME", &home)],
        &[("RAMIFY_RELEASES", &server.url)],
    );
    assert_eq!(code, 0, "{text}");
    assert!(text.contains("ram is up to date (v0.2.0)"), "{text}");
}

/// A fake `ram-code` for auto-update: it says that it started.
fn updater(base: &Path) -> PathBuf {
    let path = base.join("updater");
    std::fs::write(&path, "#!/bin/sh\necho \"updater $1\"\n").unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}

fn logged(cache: &Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        let text = std::fs::read_to_string(cache.join("update.log")).unwrap_or_default();
        if !text.is_empty() || Instant::now() > deadline {
            return text;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn auto_update_starts_at_quit() {
    let _guard = serial();
    let base = tmp("auto");
    let cache = base.join("cache");
    std::env::set_var("RAM_CODE_CACHE_DIR", &cache);
    std::env::set_var("RAM_CODE_UPDATER", updater(&base));
    let mut off = app(&base);
    off.update_found = Some(Found::Commits(1));
    assert!(!off.auto_update());
    std::fs::create_dir_all(base.join("config")).unwrap();
    std::fs::write(
        base.join("config/settings.json"),
        r#"{"auto_update": true}"#,
    )
    .unwrap();
    let mut on = app(&base);
    assert!(!on.auto_update(), "it started with no update found");
    on.update_found = Some(Found::Commits(1));
    assert!(on.auto_update());
    assert_eq!(logged(&cache), "updater update\n");
}

#[test]
fn no_update_turns_everything_off() {
    let _guard = serial();
    let base = tmp("off");
    let home = base.join("home");
    release_tree(&home, "v0.2.0");
    let server = server("v0.3.0", Vec::new(), false);
    let cache = base.join("cache");
    std::env::set_var("RAM_CODE_HOME", &home);
    std::env::set_var("RAMIFY_RELEASES", &server.url);
    std::env::set_var("RAM_CODE_CACHE_DIR", &cache);
    std::env::set_var("RAM_CODE_UPDATER", updater(&base));
    std::env::set_var("RAM_CODE_NO_UPDATE", "1");
    std::fs::create_dir_all(base.join("config")).unwrap();
    std::fs::write(
        base.join("config/settings.json"),
        r#"{"auto_update": true}"#,
    )
    .unwrap();
    let mut app = app(&base);
    app.start_update_check();
    std::thread::sleep(Duration::from_millis(500));
    app.poll_events();
    assert_eq!(server.requests.load(Ordering::SeqCst), 0);
    assert!(!render_text(&app).contains("ram update"));
    app.update_found = Some(Found::Release("v0.3.0".into()));
    assert!(!app.auto_update());
    // The probe works: with the variable unset, the same app checks and finds the update.
    std::env::remove_var("RAM_CODE_NO_UPDATE");
    app.update_found = None;
    app.start_update_check();
    let deadline = Instant::now() + Duration::from_secs(5);
    while app.update_found.is_none() && Instant::now() < deadline {
        app.poll_events();
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(app.update_found, Some(Found::Release("v0.3.0".into())));
}

#[test]
fn version_names_the_install() {
    let _guard = serial();
    let base = tmp("version");
    let home = base.join("home");
    release_tree(&home, "v0.2.0");
    let (_, text) = ram(&["--version"], &[("RAM_CODE_HOME", &home)], &[]);
    assert_eq!(
        text.trim(),
        format!("ram-code {} (v0.2.0)", env!("CARGO_PKG_VERSION"))
    );
    let (_, clone) = checkout(&base);
    let commit = sh("git rev-parse --short HEAD", &clone);
    let (_, text) = ram(&["--version"], &[("RAM_CODE_HOME", &clone)], &[]);
    assert_eq!(
        text.trim(),
        format!("ram-code {} (main@{commit})", env!("CARGO_PKG_VERSION"))
    );
}
