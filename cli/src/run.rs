use crate::launch;
use serde_json::{Map, Value};
use sha2::{Digest, Sha256};
use std::io::{IsTerminal, Read};
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

pub fn scratch(args: &[String]) -> i32 {
    let directory = scratch_dir();
    let mut title = String::new();
    let mut source: Option<String> = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--title" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("ramify: --title needs a value");
                    return 1;
                };
                title = value.clone();
                index += 1;
            }
            "--list" => {
                let mut found = false;
                let mut entries = text_entries(&directory);
                entries.sort();
                for path in entries {
                    found = true;
                    let id = path.file_stem().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
                    let title = std::fs::read_to_string(path.with_extension("title")).unwrap_or_default();
                    println!("  {id}  {}", title.trim());
                }
                if !found {
                    println!("nothing in the scratch window");
                }
                return 0;
            }
            "--clear" => {
                for path in text_entries(&directory) {
                    let _ = std::fs::remove_file(&path);
                    let _ = std::fs::remove_file(path.with_extension("title"));
                }
                println!("scratch cleared");
                return 0;
            }
            "-" => source = Some("-".into()),
            other if other.starts_with('-') => {
                eprintln!("ramify: unknown option {other} — usage: ramify scratch [--title T] [FILE | -]");
                return 1;
            }
            other => {
                if source.is_some() {
                    eprintln!("ramify: one file at a time");
                    return 1;
                }
                source = Some(other.to_string());
            }
        }
        index += 1;
    }
    let _ = std::fs::create_dir_all(&directory);
    let id = format!("{}-{}", now_format("%Y%m%d-%H%M%S"), std::process::id());
    let text_path = directory.join(format!("{id}.txt"));
    let text = match source.as_deref() {
        None | Some("-") => {
            if std::io::stdin().is_terminal() {
                eprintln!("ramify: no text: pass a FILE, or pipe the text in");
                return 1;
            }
            let mut text = String::new();
            if std::io::stdin().read_to_string(&mut text).is_err() {
                return 1;
            }
            text
        }
        Some(path) => {
            let path = Path::new(path);
            if !path.is_file() {
                eprintln!("ramify: no such file: {}", path.display());
                return 1;
            }
            if title.is_empty() {
                title = path.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            }
            std::fs::read_to_string(path).unwrap_or_default()
        }
    };
    if text.is_empty() {
        eprintln!("ramify: the text is empty");
        return 1;
    }
    if std::fs::write(&text_path, &text).is_err() {
        eprintln!("ramify: cannot write {}", text_path.display());
        return 1;
    }
    let title = if title.is_empty() {
        text.lines().next().unwrap_or("").chars().take(60).collect()
    } else {
        title
    };
    let _ = std::fs::write(text_path.with_extension("title"), title);
    println!("scratch item {id} added");
    if std::env::var("RAMIFY_POPUP").ok().as_deref() == Some("0") {
        return 0;
    }
    if launch::open("scratch").is_err() {
        println!(
            "  the scratch window did not open — run 'ramify scratch --list', or read {}",
            text_path.display()
        );
    }
    0
}

fn scratch_dir() -> PathBuf {
    if let Some(directory) = std::env::var_os("RAMIFY_SCRATCH_DIR") {
        return PathBuf::from(directory);
    }
    let root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    Path::new(&root).join("scratch")
}

fn text_entries(directory: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().map(|ext| ext == "txt").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default()
}

pub fn run(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("ask") => ask(&args[1..]),
        Some("wait") => wait(&args[1..]),
        Some("go") => go(&args[1..]),
        Some("block") => block(&args[1..]),
        Some("line") => line(args.get(1).map(String::as_str).unwrap_or("")),
        Some("") | None => {
            if launch::open("run").is_err() {
                eprintln!("ramify: the run window did not open");
                return 1;
            }
            0
        }
        Some(_) => {
            eprintln!("ramify: usage: ramify run [ask <script> [--cwd DIR] [--note TEXT] | wait <id> | go <id> | block <id> [--note TEXT] | line [root]]");
            1
        }
    }
}

fn ask(args: &[String]) -> i32 {
    let mut source = String::new();
    let mut cwd = std::env::current_dir().unwrap_or_default();
    let mut note = String::new();
    let mut index = 0;
    while index < args.len() {
        match args[index].as_str() {
            "--cwd" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("ramify: --cwd needs a directory");
                    return 1;
                };
                cwd = PathBuf::from(value);
                index += 1;
            }
            "--note" => {
                let Some(value) = args.get(index + 1) else {
                    eprintln!("ramify: --note needs a text");
                    return 1;
                };
                note = value.clone();
                index += 1;
            }
            other if other.starts_with('-') => {
                eprintln!("ramify: usage: ramify run ask <script> [--cwd DIR] [--note TEXT]");
                return 1;
            }
            other => {
                if !source.is_empty() {
                    eprintln!("ramify: one script at a time");
                    return 1;
                }
                source = other.to_string();
            }
        }
        index += 1;
    }
    if source.is_empty() {
        eprintln!("ramify: usage: ramify run ask <script> [--cwd DIR] [--note TEXT]");
        return 1;
    }
    let source_path = Path::new(&source);
    if !source_path.is_file() {
        eprintln!("ramify: no such script: {source}");
        return 1;
    }
    let text = std::fs::read_to_string(source_path).unwrap_or_default();
    if text.is_empty() {
        eprintln!("ramify: the script is empty: {source}");
        return 1;
    }
    if !cwd.is_dir() {
        eprintln!("ramify: no such directory: {}", cwd.display());
        return 1;
    }
    let cwd = cwd.canonicalize().unwrap_or(cwd);
    let source = source_path
        .canonicalize()
        .unwrap_or_else(|_| source_path.to_path_buf());
    let directory = runs_dir();
    let _ = std::fs::create_dir_all(&directory);
    let _ = std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o700));
    let id = format!("{}-{}", now_format("%Y%m%d-%H%M%S"), std::process::id());
    let run_path = directory.join(&id);
    if std::fs::create_dir(&run_path).is_err() {
        eprintln!("ramify: cannot create {}", run_path.display());
        return 1;
    }
    let _ = std::fs::set_permissions(&run_path, std::fs::Permissions::from_mode(0o700));
    let script_path = run_path.join("script");
    if std::fs::copy(&source, &script_path).is_err() {
        eprintln!("ramify: cannot copy {}", source.display());
        return 1;
    }
    let _ = std::fs::set_permissions(&script_path, std::fs::Permissions::from_mode(0o500));
    let root = crate::config::git(&["rev-parse", "--show-toplevel"], &cwd).ok().map(|text| text.trim().to_string());
    let primary = root.as_ref().and_then(|root| {
        crate::config::git(&["worktree", "list", "--porcelain"], Path::new(root))
            .ok()
            .and_then(|text| text.lines().find_map(|line| line.strip_prefix("worktree ")).map(str::to_string))
    });
    let project = primary
        .as_deref()
        .and_then(|path| Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()));
    let worktree = root
        .as_deref()
        .and_then(|path| Path::new(path).file_name().map(|name| name.to_string_lossy().into_owned()));
    let mut meta = Map::new();
    meta.insert("id".into(), Value::String(id.clone()));
    meta.insert("created".into(), Value::String(now_format("%Y-%m-%dT%H:%M:%S")));
    meta.insert("cwd".into(), Value::String(cwd.to_string_lossy().into_owned()));
    meta.insert("note".into(), Value::String(note));
    meta.insert("sha256".into(), Value::String(sha256(&script_path)));
    meta.insert("source".into(), Value::String(source.to_string_lossy().into_owned()));
    meta.insert("root".into(), root.clone().map(Value::String).unwrap_or(Value::Null));
    meta.insert("project".into(), project.map(Value::String).unwrap_or(Value::Null));
    meta.insert("worktree".into(), worktree.map(Value::String).unwrap_or(Value::Null));
    meta.insert("status".into(), Value::String("pending".into()));
    write_meta(&run_path, &Value::Object(meta));
    println!("run {id} waits on the human — the run window shows {} (frozen at {})", source.display(), script_path.display());
    println!("  wait for the decision: ramify run wait {id}");
    if std::env::var("RAMIFY_POPUP").ok().as_deref() == Some("0") {
        return 0;
    }
    if launch::open("run").is_err() {
        println!("  the run window did not open — run 'ramify run', or decide with 'ramify run go|block {id}'");
    }
    0
}

fn wait(args: &[String]) -> i32 {
    let Some(id) = args.first().cloned() else {
        eprintln!("ramify: usage: ramify run wait <id> [--timeout S]");
        return 1;
    };
    let mut timeout: Option<u64> = None;
    if let Some(position) = args.iter().position(|arg| arg == "--timeout") {
        timeout = args.get(position + 1).and_then(|value| value.parse().ok());
    }
    let Some(run_path) = run_dir(&id) else {
        return 1;
    };
    let start = Instant::now();
    loop {
        let status = meta_string(&run_path, "status");
        match status.as_str() {
            "ran" => {
                println!("ran: exit {}", meta_number(&run_path, "exit").unwrap_or(0));
                if let Ok(output) = std::fs::read_to_string(run_path.join("output.log")) {
                    let lines: Vec<&str> = output.lines().collect();
                    let start = lines.len().saturating_sub(40);
                    for line in &lines[start..] {
                        println!("{line}");
                    }
                }
                return 0;
            }
            "blocked" => {
                let reason = meta_string(&run_path, "reason");
                println!("blocked: {}", if reason.is_empty() { "(no reason given)".into() } else { reason });
                return 0;
            }
            _ => {}
        }
        if let Some(timeout) = timeout {
            if start.elapsed() >= Duration::from_secs(timeout) {
                println!("run {id} is still {}", if status.is_empty() { "pending" } else { &status });
                return 1;
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    }
}

fn go(args: &[String]) -> i32 {
    let Some(id) = args.first() else {
        eprintln!("ramify: usage: ramify run go <id>");
        return 1;
    };
    let Some(run_path) = run_dir(id) else {
        return 1;
    };
    let status = meta_string(&run_path, "status");
    if status != "pending" {
        eprintln!("ramify: run {id} is no longer pending (status: {status})");
        return 1;
    }
    if std::fs::create_dir(run_path.join("lock")).is_err() {
        eprintln!("ramify: run {id} has already started");
        return 1;
    }
    let want = meta_string(&run_path, "sha256");
    let got = sha256(&run_path.join("script"));
    if want != got {
        let mut meta = read_meta(&run_path);
        set(&mut meta, "status", "blocked");
        set(&mut meta, "decided", &now_format("%Y-%m-%dT%H:%M:%S"));
        set(&mut meta, "reason", &format!("the frozen script changed after the ask (sha256 {got}, asked {want})"));
        write_meta(&run_path, &meta);
        eprintln!("ramify: the frozen script changed after the ask — blocked, nothing ran");
        return 1;
    }
    let cwd = meta_string(&run_path, "cwd");
    if !Path::new(&cwd).is_dir() {
        let mut meta = read_meta(&run_path);
        set(&mut meta, "status", "blocked");
        set(&mut meta, "decided", &now_format("%Y-%m-%dT%H:%M:%S"));
        set(&mut meta, "reason", &format!("the directory {cwd} is gone"));
        write_meta(&run_path, &meta);
        eprintln!("ramify: no such directory: {cwd} — blocked, nothing ran");
        return 1;
    }
    let bash = if Path::new("/opt/homebrew/bin/bash").is_file() {
        PathBuf::from("/opt/homebrew/bin/bash")
    } else {
        PathBuf::from("bash")
    };
    let mut meta = read_meta(&run_path);
    set(&mut meta, "status", "running");
    set(&mut meta, "started", &now_format("%Y-%m-%dT%H:%M:%S"));
    write_meta(&run_path, &meta);
    let output = Command::new(&bash)
        .arg(run_path.join("script"))
        .current_dir(&cwd)
        .stdin(Stdio::null())
        .output();
    let (code, text) = match output {
        Ok(output) => (
            output.status.code().unwrap_or(-1),
            [output.stdout, output.stderr].concat(),
        ),
        Err(error) => (-1, error.to_string().into_bytes()),
    };
    let _ = std::fs::write(run_path.join("output.log"), &text);
    print!("{}", String::from_utf8_lossy(&text));
    let mut meta = read_meta(&run_path);
    set(&mut meta, "status", "ran");
    set(&mut meta, "finished", &now_format("%Y-%m-%dT%H:%M:%S"));
    if let Some(map) = meta.as_object_mut() {
        map.insert("exit".into(), Value::from(code));
    }
    write_meta(&run_path, &meta);
    println!("run {id} ran: exit {code}");
    0
}

fn block(args: &[String]) -> i32 {
    let Some(id) = args.first().cloned() else {
        eprintln!("ramify: usage: ramify run block <id> [--note TEXT]");
        return 1;
    };
    let mut note = String::new();
    if let Some(position) = args.iter().position(|arg| arg == "--note") {
        note = args.get(position + 1).cloned().unwrap_or_default();
    }
    let Some(run_path) = run_dir(&id) else {
        return 1;
    };
    let status = meta_string(&run_path, "status");
    if status != "pending" {
        eprintln!("ramify: run {id} is no longer pending (status: {status})");
        return 1;
    }
    if std::fs::create_dir(run_path.join("lock")).is_err() {
        eprintln!("ramify: run {id} has already started");
        return 1;
    }
    let mut meta = read_meta(&run_path);
    set(&mut meta, "status", "blocked");
    set(&mut meta, "decided", &now_format("%Y-%m-%dT%H:%M:%S"));
    set(&mut meta, "reason", &note);
    write_meta(&run_path, &meta);
    println!("run {id} blocked{}", if note.is_empty() { String::new() } else { format!(": {note}") });
    0
}

fn line(root: &str) -> i32 {
    let directory = runs_dir();
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    for name in names {
        let path = Path::new(&directory).join(&name);
        if meta_string(&path, "status") != "pending" {
            continue;
        }
        if !root.is_empty() && meta_string(&path, "root") != root {
            continue;
        }
        println!("▶ run {name} waits on you");
        return 0;
    }
    0
}

fn runs_dir() -> PathBuf {
    if let Some(directory) = std::env::var_os("RAMIFY_RUNS_DIR") {
        return PathBuf::from(directory);
    }
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("ramify/runs")
}

fn run_dir(id: &str) -> Option<PathBuf> {
    let valid = regex::Regex::new(r"^[0-9]{8}-[0-9]{6}-[0-9]+$")
        .map(|regex| regex.is_match(id))
        .unwrap_or(false);
    if !valid {
        eprintln!("ramify: no run id {id} — 'ramify runs' lists them");
        return None;
    }
    let path = runs_dir().join(id);
    if !path.join("meta.json").is_file() {
        eprintln!("ramify: no run {id} — 'ramify runs' lists them");
        return None;
    }
    Some(path)
}

fn sha256(path: &Path) -> String {
    let Ok(bytes) = std::fs::read(path) else {
        return String::new();
    };
    let mut hasher = Sha256::new();
    hasher.update(&bytes);
    let digest = hasher.finalize();
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn read_meta(path: &Path) -> Value {
    std::fs::read_to_string(path.join("meta.json"))
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_else(|| Value::Object(Map::new()))
}

fn write_meta(path: &Path, value: &Value) {
    let text = serde_json::to_string_pretty(value).unwrap_or_default();
    let _ = std::fs::write(path.join("meta.json"), format!("{text}\n"));
}

fn meta_string(path: &Path, key: &str) -> String {
    read_meta(path)
        .get(key)
        .and_then(Value::as_str)
        .unwrap_or("")
        .to_string()
}

fn meta_number(path: &Path, key: &str) -> Option<i64> {
    read_meta(path).get(key).and_then(Value::as_i64)
}

fn set(meta: &mut Value, key: &str, value: &str) {
    if let Some(map) = meta.as_object_mut() {
        map.insert(key.to_string(), Value::String(value.to_string()));
    }
}

fn now_format(format: &str) -> String {
    Command::new("date")
        .arg(format!("+{format}"))
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default()
}
