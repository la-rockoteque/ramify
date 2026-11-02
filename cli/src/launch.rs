use serde_json::Value;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

const DEFAULT_CACHE: &str = ".cache/ramify/electron";

pub fn app(args: &[String]) -> i32 {
    let kind = args
        .first()
        .map(String::as_str)
        .filter(|kind| !kind.is_empty())
        .unwrap_or("app");
    match open(kind) {
        Ok(()) => {
            println!("ramify {kind} ready");
            0
        }
        Err(error) => {
            eprintln!("ramify: {error}");
            1
        }
    }
}

pub fn open(kind: &str) -> Result<(), String> {
    let home = home()?;
    let app = home.join("app");
    let cache = cache_dir();
    let want = electron_version(&app)?;
    let mut package = app.join("node_modules/electron");
    if !package.join("path.txt").is_file() {
        package = cache.join("node_modules/electron");
    }
    if package == cache.join("node_modules/electron") {
        let installed = std::fs::read_to_string(package.join("package.json")).ok();
        let installed = installed.and_then(|text| {
            serde_json::from_str::<Value>(&text)
                .ok()
                .and_then(|value| value.get("version").and_then(Value::as_str).map(str::to_string))
        });
        if installed.as_deref() != Some(want.as_str()) {
            install(&cache, &want)?;
        }
    }
    let binary = electron_binary(&package)?;
    let target = match kind {
        "approvals" | "scratch" | "run" => app.join(kind),
        "app" => app.clone(),
        other => return Err(format!("no window named {other}")),
    };
    if !target.join("main.js").is_file() {
        return Err(format!("Ramify entry point is missing: {}", target.join("main.js").display()));
    }

    let logs = cache.join("launch");
    std::fs::create_dir_all(&logs).map_err(|error| error.to_string())?;
    let log = logs.join(format!("{kind}.log"));
    let ready_dir = logs.join(format!(
        "ready-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&ready_dir).map_err(|error| error.to_string())?;
    let ready = ready_dir.join("ready.json");

    let mut command = Command::new(&binary);
    command
        .arg(&target)
        .env_remove("NODE_OPTIONS")
        .env_remove("ELECTRON_RUN_AS_NODE")
        .env("RAMIFY_LAUNCH_READY", &ready)
        .stdin(Stdio::null());
    let output = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&log)
        .map_err(|error| format!("could not open {}: {error}", log.display()))?;
    let errors = output.try_clone().ok();
    command.stdout(output);
    if let Some(errors) = errors {
        command.stderr(errors);
    }
    use std::os::unix::process::CommandExt;
    command.process_group(0);
    let mut child = command
        .spawn()
        .map_err(|error| format!("could not start Electron: {error}"))?;

    let deadline = Instant::now() + Duration::from_secs(30);
    while Instant::now() < deadline {
        if let Ok(text) = std::fs::read_to_string(&ready) {
            if let Ok(value) = serde_json::from_str::<Value>(&text) {
                let visible = value.get("visible").and_then(Value::as_bool).unwrap_or(false);
                let title = value.get("title").and_then(Value::as_str).unwrap_or("");
                if visible && title.to_lowercase().starts_with("ramify") {
                    let _ = std::fs::remove_dir_all(&ready_dir);
                    return Ok(());
                }
            }
        }
        if let Ok(Some(status)) = child.try_wait() {
            if !status.success() {
                break;
            }
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let _ = std::fs::remove_dir_all(&ready_dir);
    let log_text = std::fs::read(&log).unwrap_or_default();
    let start = log_text.len().saturating_sub(2000);
    let detail = String::from_utf8_lossy(&log_text[start..]).trim().to_string();
    Err(format!(
        "Ramify exited or timed out before its window was ready. Log: {}{}",
        log.display(),
        if detail.is_empty() {
            String::new()
        } else {
            format!("\n{detail}")
        }
    ))
}

fn home() -> Result<PathBuf, String> {
    if let Some(home) = std::env::var_os("RAMIFY_HOME") {
        return Ok(PathBuf::from(home));
    }
    let mut current = std::env::current_exe().map_err(|error| error.to_string())?;
    while current.pop() {
        if current.join("bin/ramify").is_file() && current.join("app/main.js").is_file() {
            return Ok(current);
        }
    }
    Err("could not find RAMIFY_HOME".into())
}

fn cache_dir() -> PathBuf {
    if let Some(cache) = std::env::var_os("RAMIFY_APP_CACHE") {
        return PathBuf::from(cache);
    }
    if let Some(cache) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(cache).join("ramify/electron");
    }
    PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(DEFAULT_CACHE)
}

fn electron_version(app: &Path) -> Result<String, String> {
    let text = std::fs::read_to_string(app.join("package.json"))
        .map_err(|error| format!("could not read {}: {error}", app.join("package.json").display()))?;
    let regex = regex::Regex::new(r#""electron":\s*"[^0-9]*([0-9][0-9.]*)""#)
        .map_err(|error| error.to_string())?;
    regex
        .captures(&text)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_string())
        .ok_or_else(|| format!("no electron version in {}", app.join("package.json").display()))
}

fn electron_binary(package: &Path) -> Result<PathBuf, String> {
    let relative = std::fs::read_to_string(package.join("path.txt"))
        .map_err(|_| format!("Electron is incomplete in {} — delete the cache and run 'ramify app' again", package.display()))?;
    let binary = package.join("dist").join(relative.trim());
    if !binary.is_file() {
        return Err(format!(
            "Electron is incomplete in {} — delete the cache and run 'ramify app' again",
            package.display()
        ));
    }
    Ok(binary)
}

fn install(cache: &Path, want: &str) -> Result<(), String> {
    let node_ok = Command::new("node")
        .args([
            "-e",
            "const [a, b] = process.versions.node.split('.').map(Number); process.exit(a > 22 || (a === 22 && b >= 12) ? 0 : 1)",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !node_ok {
        if let Ok(output) = Command::new("asdf").args(["list", "nodejs"]).output() {
            let mut versions: Vec<String> = String::from_utf8_lossy(&output.stdout)
                .lines()
                .map(|line| line.trim_matches(|c: char| c == ' ' || c == '*').to_string())
                .filter(|line| line.starts_with(|c: char| c.is_ascii_digit()))
                .collect();
            versions.sort();
            if let Some(newest) = versions.last() {
                std::env::set_var("ASDF_NODEJS_VERSION", newest);
            }
        }
    }
    let node_now = Command::new("node").arg("-v").output().ok().map(|output| {
        String::from_utf8_lossy(&output.stdout).trim().to_string()
    });
    let npm_ok = Command::new("npm")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    let node_ok = Command::new("node")
        .args([
            "-e",
            "const [a, b] = process.versions.node.split('.').map(Number); process.exit(a > 22 || (a === 22 && b >= 12) ? 0 : 1)",
        ])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if !node_ok || !npm_ok {
        return Err(format!(
            "installing the app needs Node.js ≥ 22.12 and npm (found {})",
            node_now.unwrap_or_else(|| "none".into())
        ));
    }
    println!(
        "installing Electron {want} into {} (once, ~100 MB)…",
        cache.display()
    );
    std::fs::create_dir_all(cache).map_err(|error| error.to_string())?;
    let log = cache.join("install.log");
    let output = std::fs::File::create(&log).map_err(|error| error.to_string())?;
    let errors = output.try_clone().ok();
    let mut install = Command::new("npm");
    install.args([
        "install",
        "--prefix",
        &cache.to_string_lossy(),
        "--no-save",
        "--no-package-lock",
        "--no-audit",
        "--no-fund",
        &format!("electron@{want}"),
    ]);
    install.stdout(output);
    if let Some(errors) = errors {
        install.stderr(errors);
    } else {
        install.stderr(Stdio::null());
    }
    let install = install.status();
    if !matches!(install, Ok(status) if status.success()) {
        return Err(format!(
            "Electron install failed — see {}",
            log.display()
        ));
    }
    let script = cache.join("node_modules/electron/install.js");
    let fetch = Command::new("node")
        .arg(&script)
        .stdout(Stdio::from(
            std::fs::OpenOptions::new()
                .append(true)
                .open(&log)
                .map_err(|error| error.to_string())?,
        ))
        .status();
    if !matches!(fetch, Ok(status) if status.success()) {
        return Err(format!("Electron install failed — see {}", log.display()));
    }
    let _ = std::io::stdout().flush();
    Ok(())
}
