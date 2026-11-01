use crate::config::Config;
use crate::dash::{env_get, health_ok, listening};
use std::io::IsTerminal;
use std::path::{Path, PathBuf};
use std::process::Command;

pub fn config(args: &[String]) -> i32 {
    if args.first().map(String::as_str) == Some("-q") {
        return if Config::resolve().is_ok() { 0 } else { 1 };
    }
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    if args.first().map(String::as_str) == Some("get") {
        if let Some(name) = args.get(1) {
            if is_name(name) {
                if let Some(value) = config.value(name) {
                    println!("{value}");
                }
            }
        }
        return 0;
    }

    println!("config      {}", config.config_path.display());
    println!(
        "project     {}   primary {}",
        config.project,
        config.primary.display()
    );
    let branch = branch_of(&config.repo_root);
    println!("worktree    {} ({branch})", config.worktree);
    println!(
        "main        {}/{}   new trees in {}, branches '{}<slug>'",
        config.remote, config.main, config.worktrees, config.branch_prefix
    );
    println!("services    {}", or_none(&config.services.join(" ")));
    println!("qa plan     {}", or_none(&config.qa_plan));
    if config.jira_url.is_empty() {
        println!("jira        (none)");
    } else {
        println!(
            "jira        {}   tickets matching {}",
            config.jira_url, config.ticket_pattern
        );
    }
    if config.workflow_docs.is_empty() {
        println!("workflow    {}", config.workflow_source);
    } else {
        println!(
            "workflow    {}   {}",
            config.workflow_source, config.workflow_docs
        );
    }
    println!("names       {}", config.session_names);
    if !matches!(config.session_names.as_str(), "off")
        && !config.session_names.contains(char::is_whitespace)
    {
        let theme = config
            .home
            .join("assets")
            .join("names")
            .join(format!("{}.txt", config.session_names));
        if !theme.is_file() {
            println!(
                "  warning: no theme named '{}' in {}/assets/names",
                config.session_names,
                config.home.display()
            );
        }
    }
    println!("state       {}", config.state_dir.display());
    0
}

pub fn status() -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    if let Err(error) = check_services(&config) {
        eprintln!("ramify: {error}");
        return 1;
    }
    if !config.containers.is_empty() {
        println!("containers (shared):");
        for container in &config.containers {
            if let Ok(output) = Command::new("docker")
                .args([
                    "ps",
                    "--filter",
                    &format!("name=^{container}$"),
                    "--format",
                    "  {{.Names}}  {{.Status}}",
                ])
                .output()
            {
                print!("{}", String::from_utf8_lossy(&output.stdout));
            }
        }
    }
    for service in &config.services {
        let Some(port) = config.service(service, "shared_port") else {
            continue;
        };
        let parsed = port.parse().unwrap_or(0);
        let state = if health_ok(&config, service, parsed) {
            "up"
        } else {
            "not running"
        };
        println!("shared {service} :{port}  {state}");
    }
    println!("port slots ({}):", config.slots.display());
    for slot in 1..=config.max_slot {
        let path = config.slots.join(slot.to_string());
        if let Ok(holder) = std::fs::read_to_string(&path) {
            let holder = holder.split_whitespace().next().unwrap_or("").to_string();
            let live = if stack_is_down(&config, &holder) {
                "STALE (nothing on its ports)"
            } else {
                "up"
            };
            println!("  {slot:<3} {holder:<30} {live}");
        } else {
            let ports: String = slot_ports(&config, slot)
                .iter()
                .map(|port| format!(":{port} "))
                .collect();
            println!("  {slot:<3} {:<30} free ({ports})", "");
        }
    }
    0
}

fn check_services(config: &Config) -> Result<(), String> {
    if config.services.is_empty() {
        return Err(format!(
            "{} declares no RAMIFY_SERVICES — no QA stack to run here (new, prune and delete still work)",
            config.config_path.display()
        ));
    }
    for service in &config.services {
        if !valid_service(service) {
            return Err(format!(
                "service name '{service}' must match [a-z_][a-z0-9_]*"
            ));
        }
        if config.service(service, "port").is_none() || config.service(service, "cmd").is_none() {
            return Err(format!("service '{service}' needs {service}_port and {service}_cmd"));
        }
    }
    Ok(())
}

fn valid_service(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_lowercase() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '_')
}

fn is_name(name: &str) -> bool {
    let mut chars = name.chars();
    match chars.next() {
        Some(first) if first.is_ascii_alphabetic() || first == '_' => {}
        _ => return false,
    }
    chars.all(|c| c.is_ascii_alphanumeric() || c == '_')
}

fn or_none(value: &str) -> &str {
    if value.is_empty() {
        "(none)"
    } else {
        value
    }
}

fn slot_ports(config: &Config, slot: u32) -> Vec<u32> {
    config
        .services
        .iter()
        .filter_map(|service| {
            let base: u32 = config.service(service, "port")?.parse().ok()?;
            Some(base + slot)
        })
        .collect()
}

fn stack_is_down(config: &Config, worktree: &str) -> bool {
    let env = config.state_dir.join(format!("{worktree}.env"));
    if !env.is_file() {
        return true;
    }
    for service in &config.services {
        let key = format!("{}_PORT", service.to_uppercase());
        if let Some(port) = super::dash::env_get(&env, &key).and_then(|p| p.parse().ok()) {
            if listening(port) {
                return false;
            }
        }
    }
    true
}

fn branch_of(path: &Path) -> String {
    crate::config::git(&["rev-parse", "--abbrev-ref", "HEAD"], path)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "?".into())
}


pub fn workflow(args: &[String]) -> i32 {
    match crate::workflow::workflow_text(args) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("ramify: {error}");
            1
        }
    }
}

pub fn step() -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    match crate::workflow::order_text(&config) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("ramify: {error}");
            1
        }
    }
}


pub fn runs(args: &[String]) -> i32 {
    let json = args.first().map(String::as_str) == Some("--json");
    let directory = std::env::var("RAMIFY_RUNS_DIR").unwrap_or_else(|_| {
        let base = std::env::var("CLAUDE_CONFIG_DIR")
            .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()));
        format!("{base}/ramify/runs")
    });
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names.reverse();
    let metas: Vec<serde_json::Value> = names
        .iter()
        .filter_map(|name| {
            let text = std::fs::read_to_string(Path::new(&directory).join(name).join("meta.json")).ok()?;
            serde_json::from_str(&text).ok()
        })
        .collect();
    if json {
        println!("{}", crate::pyjson::to_string(&metas));
        return 0;
    }
    if metas.is_empty() {
        println!("no run yet");
    }
    for run in metas.iter().take(20) {
        let status = run_str(run, "status");
        let state = match run.get("exit").and_then(|exit| exit.as_i64()) {
            Some(exit) => format!("{status} exit {exit}"),
            None => status,
        };
        let project = run_str(run, "project");
        let worktree = run_str(run, "worktree");
        let cwd = run_str(run, "cwd");
        let where_ = if !project.is_empty() || !worktree.is_empty() {
            [project, worktree]
                .into_iter()
                .filter(|part| !part.is_empty())
                .collect::<Vec<_>>()
                .join("/")
        } else {
            cwd
        };
        let note = run_str(run, "note");
        let label = if note.is_empty() {
            run_str(run, "source")
        } else {
            note
        };
        let id = run_str(run, "id");
        println!("  {id}  {state:<12} {where_}  {label}");
    }
    0
}

fn run_str(run: &serde_json::Value, key: &str) -> String {
    run.get(key)
        .and_then(|value| value.as_str())
        .unwrap_or("")
        .to_string()
}

const BOX_WIDTH: usize = 64;

pub fn card() -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    if let Err(error) = check_services(&config) {
        eprintln!("ramify: {error}");
        return 1;
    }
    let worktree = &config.worktree;
    let env = config.state_dir.join(format!("{worktree}.env"));
    if !env.is_file() {
        println!("no stack for {worktree} — run 'ramify up'");
        return 1;
    }
    let branch = branch_of(&config.repo_root);
    let mut shown_branch = branch.clone();
    if !crate::config::git(&["status", "--porcelain"], &config.repo_root)
        .unwrap_or_default()
        .trim()
        .is_empty()
    {
        shown_branch.push_str(" *");
    }
    let style = CardStyle::new(&config.project, branch == config.main, color_on());
    style.box_line('┌', '┐');
    style.row2(
        worktree,
        &format!("slot {}", env_get(&env, "SLOT").unwrap_or_default()),
        "",
    );
    style.row2(
        &shown_branch,
        &format!("up {}", crate::dash::uptime_of(&env).unwrap_or_else(|| "—".into())),
        "",
    );
    if let Some(text) = workflow_line(&config, &branch) {
        style.row(&text);
    }
    for text in run_lines(&config.repo_root) {
        style.row(&text);
    }
    style.box_line('├', '┤');
    let processes = crate::dash::processes();
    for service in &config.services {
        let upper = service.to_uppercase();
        let Some(port) = env_get(&env, &format!("{upper}_PORT")) else {
            continue;
        };
        let Some(parsed) = port.parse().ok() else {
            continue;
        };
        let shared = config.service(service, "shared_port").is_some();
        let kind = if shared {
            if env_get(&env, &format!("{upper}_SHARED")).as_deref() == Some("1") {
                format!("{service} shared")
            } else {
                format!("{service} private")
            }
        } else {
            service.clone()
        };
        let memory = if kind == format!("{service} shared") {
            String::new()
        } else {
            crate::dash::service_mb(
                &config.state_dir.join(format!("{worktree}.{service}.pid")),
                &processes,
            )
            .map(|mb| format!(" {mb} MB"))
            .unwrap_or_default()
        };
        let left = format!(
            "{kind:<12} {:<22}{memory}",
            format!("http://localhost:{port}")
        );
        let color = if kind == format!("{service} shared") {
            style.service_color(service)
        } else {
            String::new()
        };
        style.row2(&left, crate::dash::probe(&config, service, parsed), &color);
        if let Some(spec) = config.service(service, "smoke") {
            if let Some(failure) = smoke(&config, &spec, parsed) {
                style.row(&format!("  smoke {}", failure));
            }
        }
    }
    style.box_line('└', '┘');
    0
}

struct CardStyle {
    on: bool,
    rc: String,
    end: String,
    bc: String,
}

impl CardStyle {
    fn new(project: &str, main_branch: bool, on: bool) -> Self {
        if !on {
            return Self {
                on,
                rc: String::new(),
                end: String::new(),
                bc: String::new(),
            };
        }
        let colors = ["34", "35", "36", "33", "94", "95", "96", "93"];
        let color = colors[(cksum(project.as_bytes()) % 8) as usize];
        let bc = format!("\x1b[{color}m");
        let end = "\x1b[0m".to_string();
        if main_branch {
            Self {
                on,
                rc: format!("{end}{bc}\x1b[7m"),
                end: end.clone(),
                bc: format!("{bc}\x1b[7m"),
            }
        } else {
            Self {
                on,
                rc: end.clone(),
                end,
                bc,
            }
        }
    }

    fn box_line(&self, left: char, right: char) {
        println!(
            "{}{left}{}{right}{}",
            self.bc,
            "─".repeat(BOX_WIDTH + 2),
            self.end
        );
    }

    fn row(&self, text: &str) {
        let padding = BOX_WIDTH.saturating_sub(text.chars().count());
        println!(
            "{bc}│{rc} {text}{pad} {bc2}│{end}",
            bc = self.bc,
            rc = self.rc,
            text = text,
            pad = " ".repeat(padding),
            bc2 = self.bc,
            end = self.end
        );
    }

    fn row2(&self, left: &str, right: &str, left_color: &str) {
        let width = BOX_WIDTH
            .saturating_sub(left.chars().count())
            .saturating_sub(right.chars().count())
            .saturating_sub(1);
        let reset = if left_color.is_empty() { "" } else { self.rc.as_str() };
        println!(
            "{bc}│{rc} {lc}{left}{reset}{pad} {paint} {bc2}│{end}",
            bc = self.bc,
            rc = self.rc,
            lc = left_color,
            left = left,
            reset = reset,
            pad = " ".repeat(width),
            paint = self.paint(right),
            bc2 = self.bc,
            end = self.end
        );
    }

    fn paint(&self, word: &str) -> String {
        if !self.on {
            return word.to_string();
        }
        let color = match word {
            "ready" | "up" => "\x1b[32m",
            "down" | "DOWN" | "not running" => "\x1b[31m",
            "starting" => "\x1b[33m",
            _ => return word.to_string(),
        };
        format!("{color}{word}{}", self.rc)
    }

    fn service_color(&self, service: &str) -> String {
        if !self.on {
            return String::new();
        }
        const COLORS: [&str; 8] = ["208", "213", "141", "117", "222", "180", "75", "177"];
        format!(
            "\x1b[38;5;{}m",
            COLORS[(cksum(service.as_bytes()) % 8) as usize]
        )
    }
}

fn color_on() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && (std::env::var("RAMIFY_COLOR").ok().as_deref() == Some("1")
            || std::io::stdout().is_terminal())
}

/// POSIX `cksum`: CRC-32 (polynomial 0x04C11DB7) over the bytes, then over the length.
fn cksum(bytes: &[u8]) -> u32 {
    let mut table = [0u32; 256];
    for (index, entry) in table.iter_mut().enumerate() {
        let mut value = (index as u32) << 24;
        for _ in 0..8 {
            value = if value & 0x8000_0000 != 0 {
                (value << 1) ^ 0x04C1_1DB7
            } else {
                value << 1
            };
        }
        *entry = value;
    }
    let mut crc: u32 = 0;
    for byte in bytes {
        crc = (crc << 8) ^ table[(((crc >> 24) ^ u32::from(*byte)) & 0xFF) as usize];
    }
    let mut length = bytes.len() as u64;
    while length != 0 {
        crc = (crc << 8) ^ table[(((crc >> 24) ^ (length & 0xFF) as u32) & 0xFF) as usize];
        length >>= 8;
    }
    !crc
}

fn workflow_line(config: &Config, branch: &str) -> Option<String> {
    let progress = crate::workflow::report(config, &config.repo_root, branch)?;
    let where_ = match &progress.next {
        Some(_) if progress.asked.is_some() => {
            format!("⏳ {} waits on you", progress.asked.clone().unwrap())
        }
        Some(step) => format!(
            "→ {}{}",
            step.step,
            if step.gate == "human" {
                " (human)"
            } else {
                ""
            }
        ),
        None => "✓ done".into(),
    };
    Some(format!(
        "{}/{} {}/{} {}",
        progress.workflow, progress.lane, progress.done, progress.total, where_
    ))
}

fn run_lines(root: &Path) -> Vec<String> {
    let directory = std::env::var("RAMIFY_RUNS_DIR").unwrap_or_else(|_| {
        let base = std::env::var("CLAUDE_CONFIG_DIR")
            .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()));
        format!("{base}/ramify/runs")
    });
    let mut names: Vec<String> = std::fs::read_dir(&directory)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.file_name().to_string_lossy().into_owned())
                .collect()
        })
        .unwrap_or_default();
    names.sort();
    names
        .iter()
        .filter_map(|name| {
            let text =
                std::fs::read_to_string(Path::new(&directory).join(name).join("meta.json")).ok()?;
            let meta: serde_json::Value = serde_json::from_str(&text).ok()?;
            if meta.get("status").and_then(|value| value.as_str()) != Some("pending") {
                return None;
            }
            if run_str(&meta, "root") != root.to_string_lossy() {
                return None;
            }
            Some(format!("▶ run {} waits on you", run_str(&meta, "id")))
        })
        .collect()
}

fn smoke(_config: &Config, spec: &str, port: u32) -> Option<String> {
    let (path, codes) = spec.split_once(' ').unwrap_or((spec, ""));
    let code = crate::dash::http_status_path(port, path)?;
    let matches = regex::Regex::new(&format!("^({codes})$"))
        .map(|regex| regex.is_match(&code.to_string()))
        .unwrap_or(false);
    if matches {
        None
    } else {
        Some(format!("{path} returned {code}"))
    }
}

pub fn step_asked() -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    match crate::workflow::asked_json(&config) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("ramify: {error}");
            1
        }
    }
}

pub fn gates(args: &[String]) -> i32 {
    let root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    let directory = Path::new(&root).join("gates");
    match args.first().map(String::as_str) {
        Some("--scan") => {
            scan(&directory);
            return 0;
        }
        Some("--check") => {
            if let Some(path) = args.get(1) {
                check(&directory, Path::new(path));
            }
            return 0;
        }
        Some("--json") => {
            if !directory.join(".scanned").is_file() {
                scan(&directory);
            }
            let entries = gate_entries(&directory);
            let mut text = String::from("[");
            for (index, (path, ask)) in entries.iter().enumerate() {
                if index > 0 {
                    text.push(',');
                }
                let project = project_of(path);
                text.push_str(&format!(
                    "{{\"project\":{},\"path\":{},\"worktree\":{},\"ask\":{ask}}}",
                    serde_json::to_string(&project).unwrap(),
                    serde_json::to_string(&path.to_string_lossy().into_owned()).unwrap(),
                    serde_json::to_string(&worktree_of(path)).unwrap()
                ));
            }
            text.push(']');
            println!("{text}");
            return 0;
        }
        _ => {}
    }
    if !directory.join(".scanned").is_file() {
        scan(&directory);
    }
    let entries = gate_entries(&directory);
    if entries.is_empty() {
        println!("nothing waits on a decision");
        return 0;
    }
    for (path, ask) in entries {
        let value: serde_json::Value = serde_json::from_str(&ask).unwrap_or_default();
        let step = value.get("step").and_then(|value| value.as_str()).unwrap_or("");
        let asked = value.get("asked").and_then(|value| value.as_str()).unwrap_or("");
        let files: Vec<&str> = value
            .get("files")
            .and_then(|files| files.as_array())
            .map(|files| {
                files
                    .iter()
                    .filter_map(|file| file.get("path").and_then(|path| path.as_str()))
                    .collect()
            })
            .unwrap_or_default();
        println!(
            "  {}/{}  {step:<9} asked {asked}  {}",
            project_of(&path),
            worktree_of(&path),
            files.join(", ")
        );
    }
    0
}

fn scan(directory: &Path) {
    let _ = std::fs::create_dir_all(directory);
    for root in crate::dash::projects() {
        let Ok(config) = Config::for_root(&root) else {
            continue;
        };
        let Ok(text) = crate::config::git(&["worktree", "list", "--porcelain"], &config.primary)
        else {
            continue;
        };
        for path in text
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(Path::new)
        {
            let has = path.join(".ramify").is_dir() || config.primary.join(".ramify").is_dir();
            if has {
                check(directory, path);
            }
        }
    }
    let _ = std::fs::write(directory.join(".scanned"), "");
}

fn check(directory: &Path, path: &Path) {
    let Ok(real) = path.canonicalize() else {
        return;
    };
    let entry = directory.join(entry_name(&real));
    let Ok(config) = Config::for_root(&real) else {
        return;
    };
    match crate::workflow::asked_json(&config) {
        Ok(text) if text.starts_with('{') => {
            let _ = std::fs::create_dir_all(directory);
            let temporary = entry.with_extension(format!("{}", std::process::id()));
            if std::fs::write(&temporary, format!("{}\n", real.display())).is_ok() {
                let _ = std::fs::rename(&temporary, &entry);
            }
        }
        _ => {
            let _ = std::fs::remove_file(&entry);
        }
    }
}

fn entry_name(path: &Path) -> String {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(path.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    digest[..8].iter().map(|byte| format!("{byte:02x}")).collect()
}

fn gate_entries(directory: &Path) -> Vec<(PathBuf, String)> {
    let mut entries = Vec::new();
    let Ok(read) = std::fs::read_dir(directory) else {
        return entries;
    };
    for entry in read.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') || name.ends_with(".tmp") || name.contains('.') {
            // A hash name has no extension; a PID-suffixed temp file does.
            if name.starts_with('.') || name.ends_with(".tmp") {
                continue;
            }
        }
        let Ok(text) = std::fs::read_to_string(entry.path()) else {
            continue;
        };
        let Some(first) = text.lines().next() else {
            continue;
        };
        let path = PathBuf::from(first);
        if !path.is_dir() {
            let _ = std::fs::remove_file(entry.path());
            continue;
        }
        let Ok(config) = Config::for_root(&path) else {
            continue;
        };
        match crate::workflow::asked_json(&config) {
            Ok(ask) if ask.starts_with('{') => entries.push((path, ask)),
            _ => {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    entries
}

fn project_of(path: &Path) -> String {
    let primary = crate::config::git(&["worktree", "list", "--porcelain"], path)
        .ok()
        .and_then(|text| {
            text.lines()
                .find_map(|line| line.strip_prefix("worktree "))
                .map(str::to_string)
        })
        .unwrap_or_else(|| path.to_string_lossy().into_owned());
    Path::new(&primary)
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or(primary)
}

fn worktree_of(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}


pub fn label(args: &[String]) -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    let slug = args
        .first()
        .cloned()
        .unwrap_or_else(|| config.worktree.clone());
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(slug.as_bytes());
    let digest = hasher.finalize();
    let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let index = u32::from_str_radix(&hash[..8], 16).unwrap_or(0) % 8;
    let colors = ["red", "blue", "green", "yellow", "purple", "orange", "pink", "cyan"];
    println!("/rename {slug}");
    println!("/color {}", colors[index as usize]);
    0
}

pub fn lane(args: &[String]) -> i32 {
    match crate::workflow::lane_text(args) {
        Ok(text) => {
            println!("{text}");
            0
        }
        Err(error) => {
            eprintln!("ramify: {error}");
            1
        }
    }
}


pub fn ping() -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    if let Err(error) = check_services(&config) {
        eprintln!("ramify: {error}");
        return 1;
    }
    let env = config.state_dir.join(format!("{}.env", config.worktree));
    if !env.is_file() {
        return 1;
    }
    for service in &config.services {
        if config.service(service, "optional").is_some() {
            continue;
        }
        let key = format!("{}_PORT", service.to_uppercase());
        let Some(port) = env_get(&env, &key).and_then(|port| port.parse().ok()) else {
            continue;
        };
        if !health_ok(&config, service, port) {
            return 1;
        }
    }
    0
}
