use serde_json::Value;
use sha2::{Digest, Sha256};
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

    let mut child = launch(&binary, &target, &cache, kind, &log, &ready)?;

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

fn launch(
    binary: &Path,
    target: &Path,
    cache: &Path,
    kind: &str,
    log: &Path,
    ready: &Path,
) -> Result<std::process::Child, String> {
    if cfg!(target_os = "macos") {
        let mut bundle_path = bundle(binary, target, cache, kind)?;
        if let Some(newer) = retire_other_builds(cache, kind, &bundle_path) {
            bundle_path = newer;
        }
        let request = ready.with_file_name("environment.json");
        let env: std::collections::BTreeMap<String, String> = std::env::vars()
            .filter(|(key, _)| key != "NODE_OPTIONS" && key != "ELECTRON_RUN_AS_NODE")
            .chain(std::iter::once((
                "RAMIFY_LAUNCH_READY".to_string(),
                ready.to_string_lossy().into_owned(),
            )))
            .collect();
        std::fs::write(
            &request,
            serde_json::to_string(&env).map_err(|error| error.to_string())?,
        )
        .map_err(|error| format!("could not write {}: {error}", request.display()))?;
        let _ = std::fs::set_permissions(
            &request,
            std::os::unix::fs::PermissionsExt::from_mode(0o600),
        );
        // `open` returns before the window is ready; the ready file is the real signal.
        let mut command = Command::new("open");
        command.args([
            "-n",
            "-a",
            &bundle_path.to_string_lossy(),
            "--stdout",
            &log.to_string_lossy(),
            "--stderr",
            &log.to_string_lossy(),
            "--args",
            &format!("--ramify-launch={}", request.display()),
        ]);
        command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null());
        command
            .spawn()
            .map_err(|error| format!("could not open {}: {error}", bundle_path.display()))
    } else {
        let mut command = Command::new(binary);
        command
            .arg(target)
            .env_remove("NODE_OPTIONS")
            .env_remove("ELECTRON_RUN_AS_NODE")
            .env("RAMIFY_LAUNCH_READY", ready)
            .stdin(Stdio::null());
        use std::os::unix::process::CommandExt;
        command.process_group(0);
        command
            .spawn()
            .map_err(|error| format!("could not start Electron: {error}"))
    }
}

fn bundle(binary: &Path, target: &Path, cache: &Path, kind: &str) -> Result<PathBuf, String> {
    let source = binary
        .parent()
        .and_then(Path::parent)
        .and_then(Path::parent)
        .filter(|path| path.is_dir())
        .ok_or_else(|| "Electron's app bundle is missing".to_string())?;
    let name = if kind == "app" {
        "Ramify".to_string()
    } else {
        format!("Ramify {}", title(kind))
    };
    let root = if kind == "app" {
        target.parent()
    } else {
        target.parent().and_then(Path::parent)
    }
    .ok_or_else(|| "Ramify source is incomplete".to_string())?;
    let current = std::env::current_exe().map_err(|error| error.to_string())?;
    let files = source_files(root);
    let mut hasher = Sha256::new();
    hasher.update(std::fs::read(source.join("Contents/Info.plist")).map_err(|error| error.to_string())?);
    hasher.update(kind.as_bytes());
    for file in &files {
        let relative = file.strip_prefix(root).unwrap_or(file);
        hasher.update(relative.to_string_lossy().as_bytes());
        hasher.update(std::fs::read(file).unwrap_or_default());
    }
    hasher.update(std::fs::read(&current).unwrap_or_default());
    let signature: String = hasher.finalize().iter().map(|byte| format!("{byte:02x}")).collect();
    let bundle = cache.join("apps").join(kind).join(&signature).join(format!("{name}.app"));
    let marker = bundle.join("Contents/Resources/ramify-source");
    if marker.is_file() && std::fs::read_to_string(&marker).ok().as_deref() == Some(signature.as_str()) {
        alias(cache, &name, &bundle)?;
        return Ok(bundle);
    }
    bundle.parent()
        .map(std::fs::create_dir_all)
        .transpose()
        .map_err(|error| error.to_string())?;
    let staging = cache.join("apps").join(kind).join(format!(
        ".ramify-{}-{}",
        std::process::id(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|duration| duration.as_nanos())
            .unwrap_or(0)
    ));
    std::fs::create_dir_all(&staging).map_err(|error| error.to_string())?;
    let draft = staging.join(format!("{name}.app"));
    let result = build_bundle(source, &draft, target, root, &files, &current, &signature, kind);
    if let Err(error) = result {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(error);
    }
    if bundle.exists() {
        let previous = staging.join("previous.app");
        let _ = std::fs::rename(&bundle, &previous);
        if let Err(error) = std::fs::rename(&draft, &bundle) {
            let _ = std::fs::rename(&previous, &bundle);
            let _ = std::fs::remove_dir_all(&staging);
            return Err(format!("could not replace the bundle: {error}"));
        }
    } else if let Err(error) = std::fs::rename(&draft, &bundle) {
        let _ = std::fs::remove_dir_all(&staging);
        return Err(format!("could not place the bundle: {error}"));
    }
    let _ = std::fs::remove_dir_all(&staging);
    alias(cache, &name, &bundle)?;
    Ok(bundle)
}

#[allow(clippy::too_many_arguments)]
fn build_bundle(
    source: &Path,
    draft: &Path,
    target: &Path,
    root: &Path,
    files: &[PathBuf],
    current: &Path,
    signature: &str,
    kind: &str,
) -> Result<(), String> {
    copy_tree(source, draft)?;
    let info_path = draft.join("Contents/Info.plist");
    let info = std::fs::read_to_string(&info_path).map_err(|error| error.to_string())?;
    let info = set_plist(&info, "CFBundleIdentifier", &format!("dev.ramify.{kind}"));
    let name = if kind == "app" {
        "Ramify".to_string()
    } else {
        format!("Ramify {}", title(kind))
    };
    let info = set_plist(&info, "CFBundleName", &name);
    let info = set_plist(&info, "CFBundleDisplayName", &name);
    std::fs::write(&info_path, info).map_err(|error| error.to_string())?;

    let entry = draft.join("Contents/Resources/app");
    std::fs::create_dir_all(&entry).map_err(|error| error.to_string())?;
    let package = std::fs::read_to_string(target.join("package.json"))
        .map_err(|error| format!("could not read {}/package.json: {error}", target.display()))?;
    let mut package: Value = serde_json::from_str(&package).map_err(|error| error.to_string())?;
    package["main"] = Value::String("main.js".into());
    std::fs::write(
        entry.join("package.json"),
        serde_json::to_string(&package).map_err(|error| error.to_string())?,
    )
    .map_err(|error| error.to_string())?;

    let snapshot = draft.join("Contents/Resources/ramify");
    for file in files {
        let relative = file.strip_prefix(root).unwrap_or(file);
        let copy = snapshot.join(relative);
        if let Some(parent) = copy.parent() {
            std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
        }
        std::fs::copy(file, &copy).map_err(|error| {
            format!("could not copy {}: {error}", file.display())
        })?;
    }
    let binary_dir = snapshot.join("cli/target/release");
    std::fs::create_dir_all(&binary_dir).map_err(|error| error.to_string())?;
    let binary_copy = binary_dir.join("ramify");
    std::fs::copy(current, &binary_copy).map_err(|error| error.to_string())?;
    let _ = std::fs::set_permissions(
        &binary_copy,
        std::os::unix::fs::PermissionsExt::from_mode(0o755),
    );

    let main_relative = target
        .join("main.js")
        .strip_prefix(root)
        .map(|path| path.to_string_lossy().into_owned())
        .unwrap_or_else(|_| "app/main.js".into());
    let entry_js = format!(
        "const request = process.argv.find(arg => arg.startsWith('--ramify-launch='));\n\
         if (request) Object.assign(process.env, JSON.parse(require('node:fs').readFileSync(request.slice(16), 'utf8')));\n\
         process.env.RAMIFY_APP_BUILD = {};\n\
         require({});\n",
        serde_json::to_string(signature).unwrap_or_default(),
        serde_json::to_string(&format!("../ramify/{main_relative}")).unwrap_or_default()
    );
    std::fs::write(entry.join("main.js"), entry_js).map_err(|error| error.to_string())?;
    std::fs::write(draft.join("Contents/Resources/ramify-source"), signature)
        .map_err(|error| error.to_string())?;

    let status = Command::new("codesign")
        .args(["--force", "--sign", "-", "--preserve-metadata=entitlements"])
        .arg(draft)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map_err(|error| format!("codesign failed: {error}"))?;
    if !status.success() {
        return Err("codesign failed".into());
    }
    Ok(())
}

fn source_files(root: &Path) -> Vec<PathBuf> {
    let mut files = Vec::new();
    for name in ["app", "bin", "assets", "workflows.yml"] {
        let path = root.join(name);
        if !path.exists() {
            continue;
        }
        collect_files(&path, &mut files);
    }
    files.sort();
    files
}

fn collect_files(path: &Path, files: &mut Vec<PathBuf>) {
    if path.is_dir() {
        let Ok(entries) = std::fs::read_dir(path) else {
            return;
        };
        for entry in entries.flatten() {
            let child = entry.path();
            let name = entry.file_name().to_string_lossy().into_owned();
            if name == "node_modules" || name == "__pycache__" || name == ".DS_Store" {
                continue;
            }
            collect_files(&child, files);
        }
    } else if path.is_file() {
        files.push(path.to_path_buf());
    }
}

fn copy_tree(source: &Path, target: &Path) -> Result<(), String> {
    if source.is_symlink() {
        let link = std::fs::read_link(source).map_err(|error| error.to_string())?;
        use std::os::unix::fs::symlink;
        symlink(link, target).map_err(|error| error.to_string())?;
        return Ok(());
    }
    if source.is_dir() {
        std::fs::create_dir_all(target).map_err(|error| error.to_string())?;
        let entries = std::fs::read_dir(source).map_err(|error| error.to_string())?;
        for entry in entries.flatten() {
            copy_tree(&entry.path(), &target.join(entry.file_name()))?;
        }
        return Ok(());
    }
    std::fs::copy(source, target).map_err(|error| error.to_string())?;
    Ok(())
}

fn alias(cache: &Path, name: &str, bundle: &Path) -> Result<(), String> {
    let apps = cache.join("apps");
    std::fs::create_dir_all(&apps).map_err(|error| error.to_string())?;
    let alias = apps.join(format!("{name}.app"));
    if alias.exists() && !alias.is_symlink() {
        let previous = apps.join(format!(
            "{name}.app.previous-{}",
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|duration| duration.as_nanos())
                .unwrap_or(0)
        ));
        let _ = std::fs::rename(&alias, previous);
    }
    let _ = std::fs::remove_file(&alias);
    use std::os::unix::fs::symlink;
    symlink(bundle, &alias).map_err(|error| format!("could not link the app: {error}"))
}

fn retire_other_builds(cache: &Path, kind: &str, bundle: &Path) -> Option<PathBuf> {
    let prefix = cache.join("apps").join(kind).to_string_lossy().into_owned();
    let ours = build_age(bundle.parent()?);
    let output = Command::new("ps").args(["-axo", "pid=,command="]).output().ok()?;
    let mut newest: Option<PathBuf> = None;
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.trim().splitn(2, ' ');
        let pid: u32 = parts.next()?.parse().ok()?;
        let command = parts.next().unwrap_or("");
        let rest = command.strip_prefix(&format!("{prefix}/"))?;
        let mut segments = rest.split('/');
        let signature = segments.next()?;
        let app_name = segments.next()?;
        if !app_name.ends_with(".app") || command.contains(&bundle.to_string_lossy().into_owned()) {
            continue;
        }
        let other = cache
            .join("apps")
            .join(kind)
            .join(signature)
            .join(app_name);
        let age = build_age(other.parent().unwrap_or(&other));
        if age > ours {
            if newest.as_ref().map(|current| age > build_age(current.parent().unwrap_or(current))).unwrap_or(true) {
                newest = Some(other);
            }
            continue;
        }
        let _ = Command::new("kill")
            .args(["-TERM", &pid.to_string()])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
    newest
}

fn build_age(path: &Path) -> SystemTime {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .unwrap_or(UNIX_EPOCH)
}

fn title(kind: &str) -> String {
    let mut chars = kind.chars();
    match chars.next() {
        Some(first) => first.to_uppercase().collect::<String>() + chars.as_str(),
        None => String::new(),
    }
}

fn set_plist(text: &str, key: &str, value: &str) -> String {
    let pattern = format!(r"(<key>{}</key>\s*<string>)[^<]*(</string>)", regex::escape(key));
    match regex::Regex::new(&pattern) {
        Ok(regex) => regex
            .replace(text, |captures: &regex::Captures| {
                format!("{}{}{}", &captures[1], value, &captures[2])
            })
            .into_owned(),
        Err(_) => text.to_string(),
    }
}
