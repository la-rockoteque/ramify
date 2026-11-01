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
    card_with_smoke(true)
}

pub fn card_with_smoke(with_smoke: bool) -> i32 {
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
        if with_smoke {
            if let Some(spec) = config.service(service, "smoke") {
                if let Some(failure) = smoke(&config, &spec, parsed) {
                    style.row(&format!("  smoke {}", failure));
                }
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

pub fn down() -> i32 {
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
    kill_worktree(&config, &config.worktree);
    let _ = std::fs::remove_file(
        config
            .state_dir
            .join(format!("{}.errors", config.worktree)),
    );
    println!(
        "{} stopped. Shared containers and instances left running on purpose.",
        config.worktree
    );
    0
}

pub fn shared_down() -> i32 {
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
    for service in &config.services {
        let Some(port) = config.service(service, "shared_port") else {
            continue;
        };
        let pidfile = config
            .state_dir
            .join("shared")
            .join(format!("{service}.pid"));
        if pidfile.is_file() {
            stop_pidfile(&pidfile, port.parse().ok());
        }
        if let Ok(port) = port.parse() {
            kill_port(&config, port);
        }
        println!("shared {service} :{port} stopped — every worktree pointed at it now reaches nothing");
    }
    0
}

pub(crate) fn kill_worktree_public(config: &Config, worktree: &str) {
    kill_worktree(config, worktree)
}

fn kill_worktree(config: &Config, worktree: &str) {
    let env = config.state_dir.join(format!("{worktree}.env"));
    for service in &config.services {
        let upper = service.to_uppercase();
        let shared = env_get(&env, &format!("{upper}_SHARED")).as_deref() == Some("1");
        let port = if shared {
            None
        } else {
            env_get(&env, &format!("{upper}_PORT")).and_then(|port| port.parse().ok())
        };
        let pidfile = config
            .state_dir
            .join(format!("{worktree}.{service}.pid"));
        if pidfile.is_file() {
            stop_pidfile(&pidfile, port);
        }
    }
    if let Ok(entries) = std::fs::read_dir(&config.state_dir) {
        let prefix = format!("{worktree}.");
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&prefix) && name.ends_with(".pid") {
                stop_pidfile(&entry.path(), None);
            }
        }
    }
    for (group, tagged_worktree, _) in crate::dash::tagged_groups(config) {
        if tagged_worktree == worktree && our_group(config, group) {
            signal_group(group);
        }
    }
    for service in &config.services {
        let upper = service.to_uppercase();
        if env_get(&env, &format!("{upper}_SHARED")).as_deref() == Some("1") {
            continue;
        }
        if let Some(port) = env_get(&env, &format!("{upper}_PORT")).and_then(|port| port.parse().ok())
        {
            kill_port(config, port);
        }
    }
    release_slot(config, env_get(&env, "SLOT"), worktree);
    let _ = std::fs::remove_file(&env);
}

fn release_slot(config: &Config, slot: Option<String>, worktree: &str) {
    let Some(slot) = slot.filter(|slot| !slot.is_empty()) else {
        return;
    };
    let path = config.slots.join(&slot);
    let holder = std::fs::read_to_string(&path).unwrap_or_default();
    let holder = holder.split_whitespace().next().unwrap_or("");
    if holder != worktree {
        return;
    }
    let _ = std::fs::remove_file(&path);
    println!("  slot {slot} released");
}

fn group_members(group: u64) -> Vec<u64> {
    let Ok(output) = Command::new("ps")
        .args(["-axww", "-o", "pid=,pgid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let mut parts = line.split_whitespace();
            let pid: u64 = parts.next()?.parse().ok()?;
            let pgid: u64 = parts.next()?.parse().ok()?;
            let command = parts.collect::<Vec<_>>().join(" ");
            (pgid == group && !is_build_server(&command)).then_some(pid)
        })
        .collect()
}

fn is_build_server(command: &str) -> bool {
    (command.contains("MSBuild.dll")
        && (command.contains("nodeReuse:true") || command.contains("nodemode:8")))
        || command.contains("VBCSCompiler.dll")
        || (command.contains("rzc.dll") && command.contains(" server"))
}

fn our_group(config: &Config, group: u64) -> bool {
    crate::dash::tagged_groups(config)
        .iter()
        .any(|(tagged, _, _)| *tagged == group)
}

fn signal_group(group: u64) {
    if group <= 1 || crate::dash::protected(group) {
        return;
    }
    for round in 0..16 {
        let members: Vec<u64> = group_members(group)
            .into_iter()
            .filter(|pid| *pid > 1 && !crate::dash::protected(*pid))
            .collect();
        if members.is_empty() {
            return;
        }
        if round == 0 {
            for pid in members {
                let _ = Command::new("kill")
                    .args(["-TERM", &pid.to_string()])
                    .status();
            }
        } else if round == 15 {
            for pid in members {
                let _ = Command::new("kill")
                    .args(["-9", &pid.to_string()])
                    .status();
            }
        } else {
            std::thread::sleep(std::time::Duration::from_millis(200));
        }
    }
}

fn stop_pidfile(pidfile: &Path, port: Option<u32>) {
    let pid = std::fs::read_to_string(pidfile)
        .map(|text| text.trim().to_string())
        .unwrap_or_default();
    let _ = std::fs::remove_file(pidfile);
    let Ok(group) = pid.parse::<u64>() else {
        return;
    };
    if group <= 1 || group_members(group).is_empty() {
        return;
    }
    let config = Config::resolve();
    if let Ok(config) = &config {
        if our_group(config, group) {
            signal_group(group);
            return;
        }
    }
    if let Some(port) = port {
        for listener in listeners(port) {
            if listener_group(listener) == Some(group) {
                signal_group(group);
                return;
            }
        }
    }
    eprintln!(
        "ramify: left process group {group} alone — {} names it, but nothing proves it is ours",
        pidfile.display()
    );
}

fn kill_port(config: &Config, port: u32) {
    for listener in listeners(port) {
        let Some(group) = listener_group(listener) else {
            continue;
        };
        if our_group(config, group) {
            signal_group(group);
        } else {
            eprintln!("ramify: left pid {listener} alone on :{port} — it is not one of ours");
        }
    }
}

fn listeners(port: u32) -> Vec<u64> {
    let Ok(output) = Command::new("lsof")
        .args(["-ti", &format!("tcp:{port}"), "-sTCP:LISTEN"])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| line.trim().parse().ok())
        .collect()
}

fn listener_group(pid: u64) -> Option<u64> {
    Command::new("ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout)
                .trim()
                .parse()
                .ok()
        })
}

struct UpLock {
    path: PathBuf,
}

impl Drop for UpLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

pub fn up() -> i32 {
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
    let _ = std::fs::create_dir_all(config.state_dir.join("shared"));
    let _ = std::fs::create_dir_all(&config.slots);
    let lock = match take_up_lock(&config) {
        Ok(lock) => lock,
        Err(code) => return code,
    };
    let errors = config
        .state_dir
        .join(format!("{}.errors", config.worktree));
    let _ = std::fs::remove_file(&errors);
    let env = config
        .state_dir
        .join(format!("{}.env", config.worktree));
    if env.is_file() && ping_ports(&config, &env) {
        drop(lock);
        return card_with_smoke(true);
    }
    kill_worktree(&config, &config.worktree);
    println!(
        "Stack for {} ({})",
        config.worktree,
        branch_of(&config.repo_root)
    );
    if let Err(error) = ensure_containers(&config, &errors) {
        drop(lock);
        eprintln!("ramify: {error}");
        return 1;
    }
    let slot = match claim_slot(&config) {
        Some(slot) => slot,
        None => {
            drop(lock);
            note_error(
                &errors,
                "slots",
                "",
                &format!(
                    "all {} slots taken — 'ramify status' shows who holds them",
                    config.max_slot
                ),
            );
            eprintln!(
                "ramify: all {} slots taken — 'ramify status' shows who holds them",
                config.max_slot
            );
            return 1;
        }
    };
    println!("  slot            {slot}");
    let mut state = format!("SLOT={slot}\n");
    let mut ports: Vec<(String, String)> = Vec::new();
    let mut late: Vec<String> = Vec::new();
    let mut failed = false;

    for service in &config.services {
        if skipped(&config, service) {
            continue;
        }
        if unneeded(&config, service) {
            println!(
                "  {:<15} not needed — nothing changed matches {service}_when",
                service
            );
            continue;
        }
        let upper = service.to_uppercase();
        if is_shared(&config, service) {
            let port = config.service(service, "shared_port").unwrap_or_default();
            if let Err(()) = ensure_shared(&config, service, &errors) {
                failed = true;
                break;
            }
            state.push_str(&format!("{upper}_PORT={port}\n{upper}_SHARED=1\n"));
            ports.push((format!("{upper}_PORT"), port));
        } else {
            let Some(base) = config.service(service, "port").and_then(|port| port.parse::<u32>().ok())
            else {
                continue;
            };
            let port = base + slot;
            if let Some(prepare) = config.service(service, "prepare") {
                println!("  {:<15} preparing", service);
                let log = config
                    .state_dir
                    .join(format!("{}.{}.prepare.log", config.worktree, service));
                let directory = service_dir(&config, service);
                let status = Command::new("bash")
                    .args(["-c", &prepare])
                    .current_dir(&directory)
                    .stdout(std::fs::File::create(&log).map(std::process::Stdio::from).unwrap_or_else(|_| std::process::Stdio::null()))
                    .stderr(std::process::Stdio::null())
                    .status();
                match status {
                    Ok(status) if status.success() => {}
                    Ok(status) => {
                        println!(
                            "  {:<15} prepare FAILED — see {}",
                            service,
                            log.display()
                        );
                        note_error(&errors, service, &log.to_string_lossy(), &format!("prepare failed (exit {})", status.code().unwrap_or(-1)));
                        failed = true;
                        break;
                    }
                    Err(error) => {
                        println!("  {:<15} prepare FAILED — see {}", service, log.display());
                        note_error(&errors, service, &log.to_string_lossy(), &format!("prepare failed: {error}"));
                        failed = true;
                        break;
                    }
                }
            }
            let cmd = config.service(service, "cmd").unwrap_or_default();
            let pidfile = config
                .state_dir
                .join(format!("{}.{}.pid", config.worktree, service));
            let log = config
                .state_dir
                .join(format!("{}.{}.log", config.worktree, service));
            if let Err(error) = start_service(
                &config,
                service,
                port,
                &config.repo_root,
                &pidfile,
                &log,
                &cmd,
                &tag(&config, &config.worktree, service),
                &ports,
            ) {
                eprintln!("ramify: {error}");
                failed = true;
                break;
            }
            state.push_str(&format!("{upper}_PORT={port}\n{upper}_SHARED=0\n"));
            ports.push((format!("{upper}_PORT"), port.to_string()));
            if config.service(service, "optional").is_some() {
                late.push(service.clone());
            } else if wait_for(&config, service, port) {
                println!("  {:<15} http://localhost:{port}  (ready)", service);
            } else {
                println!(
                    "  {:<15} http://localhost:{port}  NOT READY — see {}",
                    service,
                    log.display()
                );
                note_error(
                    &errors,
                    service,
                    &log.to_string_lossy(),
                    &format!(
                        "did not answer http://localhost:{port}{} in time",
                        config.service(service, "health").unwrap_or_default()
                    ),
                );
            }
        }
    }

    // The state file is written before the services are waited on, as the shell does.
    let _ = std::fs::write(&env, state);

    if !failed {
        for service in &config.services {
            let key = format!("{}_PORT", service.to_uppercase());
            let Some(port) = env_get(&env, &key).and_then(|port| port.parse().ok()) else {
                continue;
            };
            if let Some(spec) = config.service(service, "smoke") {
                if let Some(failure) = smoke(&config, &spec, port) {
                    println!(
                        "  {:<15} smoke {}  <-- check the wiring",
                        service, failure
                    );
                }
            }
        }
        for service in &late {
            let key = format!("{}_PORT", service.to_uppercase());
            let Some(port) = env_get(&env, &key).and_then(|port| port.parse().ok()) else {
                continue;
            };
            if wait_for(&config, service, port) {
                println!("  {:<15} http://localhost:{port}  (ready)", service);
            } else {
                println!(
                    "  {:<15} http://localhost:{port}  still starting — see {}",
                    service,
                    config
                        .state_dir
                        .join(format!("{}.{}.log", config.worktree, service))
                        .display()
                );
            }
        }
        println!();
        let code = card_with_smoke(false);
        drop(lock);
        return code;
    }

    kill_worktree(&config, &config.worktree);
    drop(lock);
    1
}

fn take_up_lock(config: &Config) -> Result<UpLock, i32> {
    let path = config
        .state_dir
        .join(format!("{}.up.lock", config.worktree));
    let mut created = std::fs::create_dir(&path).is_ok();
    if !created {
        if crate::dash::up_running(config, &config.worktree) {
            let pid = std::fs::read_to_string(path.join("pid")).unwrap_or_default();
            println!(
                "{}: an up is already running (pid {}) — 'ramify card' shows how far it got",
                config.worktree,
                pid.trim()
            );
            return Err(0);
        }
        let stale = path.with_extension(format!("stale.{}", std::process::id()));
        let _ = std::fs::rename(&path, &stale);
        let _ = std::fs::remove_dir_all(&stale);
        created = std::fs::create_dir(&path).is_ok();
        if !created {
            println!("{}: another up took the lock first", config.worktree);
            return Err(0);
        }
    }
    let _ = std::fs::write(path.join("pid"), format!("{}\n", std::process::id()));
    Ok(UpLock { path })
}

fn claim_slot(config: &Config) -> Option<u32> {
    for slot in 1..=config.max_slot {
        let path = config.slots.join(slot.to_string());
        if path.exists() {
            if !slot_is_stale(config, slot) {
                continue;
            }
            let _ = std::fs::remove_file(&path);
        }
        let busy = config.services.iter().any(|service| {
            config
                .service(service, "port")
                .and_then(|port| port.parse::<u32>().ok())
                .map(|base| crate::dash::listening(base + slot))
                .unwrap_or(false)
        });
        if busy {
            continue;
        }
        if let Ok(mut file) = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
        {
            use std::io::Write;
            let _ = writeln!(file, "{} {}", config.worktree, std::process::id());
            return Some(slot);
        }
    }
    None
}

fn slot_is_stale(config: &Config, slot: u32) -> bool {
    let path = config.slots.join(slot.to_string());
    let holder = std::fs::read_to_string(&path).unwrap_or_default();
    let pid = holder.split_whitespace().nth(1).unwrap_or("");
    if !pid.is_empty() {
        if let Ok(status) = Command::new("kill")
            .args(["-0", pid])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
        {
            if status.success() {
                return false;
            }
        }
    }
    !config.services.iter().any(|service| {
        config
            .service(service, "port")
            .and_then(|port| port.parse::<u32>().ok())
            .map(|base| crate::dash::listening(base + slot))
            .unwrap_or(false)
    })
}

fn ensure_containers(config: &Config, errors: &Path) -> Result<(), String> {
    let compose = config.value("RAMIFY_COMPOSE_FILE").unwrap_or_default();
    if config.containers.is_empty() && compose.is_empty() {
        return Ok(());
    }
    if Command::new("docker").arg("info").output().map(|output| !output.status.success()).unwrap_or(true) {
        note_error(errors, "containers", "", "Docker is not running. Start it, then run this again.");
        return Err("Docker is not running. Start it, then run this again.".into());
    }
    let mut missing = false;
    for container in &config.containers {
        let running = Command::new("docker")
            .args([
                "ps",
                "--filter",
                &format!("name=^{container}$"),
                "--filter",
                "status=running",
                "-q",
            ])
            .output()
            .map(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
            .unwrap_or(false);
        if !running {
            missing = true;
        }
    }
    if config.containers.is_empty() {
        let project = config
            .value("RAMIFY_COMPOSE_PROJECT")
            .unwrap_or_else(|| config.project.clone());
        let file = config.primary.join(&compose);
        let running = Command::new("docker")
            .args(["compose", "-p", &project, "-f"])
            .arg(&file)
            .args(["ps", "--services", "--status", "running"])
            .output()
            .map(|output| output.stdout)
            .unwrap_or_default();
        let all = Command::new("docker")
            .args(["compose", "-f"])
            .arg(&file)
            .args(["config", "--services"])
            .output()
            .map(|output| output.stdout)
            .unwrap_or_default();
        missing = sorted(&running) != sorted(&all);
    }
    if !missing {
        println!("  containers      already up (shared)");
        return Ok(());
    }
    if compose.is_empty() {
        note_error(errors, "containers", "", "containers down and no RAMIFY_COMPOSE_FILE to start them");
        return Err("containers down and no RAMIFY_COMPOSE_FILE to start them".into());
    }
    let project = config
        .value("RAMIFY_COMPOSE_PROJECT")
        .unwrap_or_else(|| config.project.clone());
    println!("  containers      starting under project '{project}'…");
    let log = config
        .state_dir
        .join(format!("{}.compose.log", config.worktree));
    let output = Command::new("docker")
        .args(["compose", "-p", &project, "-f"])
        .arg(config.primary.join(&compose))
        .args(["up", "-d"])
        .output();
    match output {
        Ok(output) if output.status.success() => {
            let _ = std::fs::write(&log, &output.stdout);
            Ok(())
        }
        Ok(output) => {
            let _ = std::fs::write(&log, [output.stdout, output.stderr].concat());
            note_error(errors, "containers", &log.to_string_lossy(), "docker compose up failed");
            Err("docker compose up failed".into())
        }
        Err(error) => {
            note_error(errors, "containers", &log.to_string_lossy(), "docker compose up failed");
            Err(format!("docker compose up failed: {error}"))
        }
    }
}

fn sorted(bytes: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = String::from_utf8_lossy(bytes)
        .lines()
        .map(str::to_string)
        .filter(|line| !line.is_empty())
        .collect();
    lines.sort();
    lines
}

fn service_dir(config: &Config, service: &str) -> PathBuf {
    let dir = config.service(service, "dir").unwrap_or_default();
    config.repo_root.join(dir)
}

fn tag(config: &Config, worktree: &str, service: &str) -> String {
    use sha1::{Digest, Sha1};
    let mut hasher = Sha1::new();
    hasher.update(config.state_dir.to_string_lossy().as_bytes());
    let digest = hasher.finalize();
    let short: String = digest[..4].iter().map(|byte| format!("{byte:02x}")).collect();
    format!("{}@{short}/{worktree}/{service}", config.project)
}

#[allow(clippy::too_many_arguments)]
fn start_service(
    config: &Config,
    service: &str,
    port: u32,
    root: &Path,
    pidfile: &Path,
    log: &Path,
    cmd: &str,
    tag: &str,
    ports: &[(String, String)],
) -> Result<(), String> {
    use std::os::unix::process::CommandExt;
    let directory = root.join(config.service(service, "dir").unwrap_or_default());
    let stdout = std::fs::File::create(log).map_err(|error| format!("could not open {}: {error}", log.display()))?;
    let stderr = stdout
        .try_clone()
        .map_err(|error| format!("could not open {}: {error}", log.display()))?;
    let mut command = Command::new("bash");
    command
        .args(["-c", cmd])
        .current_dir(&directory)
        .env("PORT", port.to_string())
        .env("RAMIFY_TAG", tag)
        .stdin(std::process::Stdio::null())
        .stdout(stdout)
        .stderr(stderr)
        .process_group(0);
    for (key, value) in ports {
        command.env(key, value);
    }
    let child = command
        .spawn()
        .map_err(|error| format!("could not start {service}: {error}"))?;
    std::fs::write(pidfile, format!("{}\n", child.id()))
        .map_err(|error| format!("could not write {}: {error}", pidfile.display()))?;
    Ok(())
}

fn wait_for(config: &Config, service: &str, port: u32) -> bool {
    let timeout: u64 = config
        .service(service, "timeout")
        .and_then(|value| value.parse().ok())
        .unwrap_or(90);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(timeout);
    loop {
        if health_ok(config, service, port) {
            return true;
        }
        if std::time::Instant::now() >= deadline {
            return false;
        }
        std::thread::sleep(std::time::Duration::from_secs(2));
    }
}

fn ensure_shared(config: &Config, service: &str, errors: &Path) -> Result<(), ()> {
    let port = config
        .service(service, "shared_port")
        .and_then(|port| port.parse().ok())
        .unwrap_or(0);
    if health_ok(config, service, port) {
        println!(
            "  {:<15} http://localhost:{port}  (shared, already up)",
            service
        );
        return Ok(());
    }
    println!(
        "  {:<15} starting shared instance from {}…",
        service,
        config
            .primary
            .file_name()
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_default()
    );
    let pidfile = config.state_dir.join("shared").join(format!("{service}.pid"));
    let log = config.state_dir.join("shared").join(format!("{service}.log"));
    let cmd = config.service(service, "shared_cmd").unwrap_or_default();
    if start_service(
        config,
        service,
        port,
        &config.primary,
        &pidfile,
        &log,
        &cmd,
        &tag(config, "@shared", service),
        &[],
    )
    .is_err()
    {
        return Err(());
    }
    if wait_for(config, service, port) {
        println!(
            "  {:<15} http://localhost:{port}  (shared, ready)",
            service
        );
        Ok(())
    } else {
        println!(
            "  {:<15} shared instance NOT READY — see {}",
            service,
            log.display()
        );
        note_error(
            errors,
            service,
            &log.to_string_lossy(),
            &format!("shared instance did not answer http://localhost:{port}{}", config.service(service, "health").unwrap_or_default()),
        );
        Err(())
    }
}

fn changed_paths(config: &Config) -> Vec<String> {
    let base = crate::config::git(
        &["merge-base", "HEAD", &format!("{}/{}", config.remote, config.main)],
        &config.repo_root,
    )
    .or_else(|_| crate::config::git(&["merge-base", "HEAD", &config.main], &config.repo_root));
    let mut paths = Vec::new();
    if let Ok(base) = base {
        if let Ok(text) = crate::config::git(&["diff", "--name-only", &format!("{}...HEAD", base.trim())], &config.repo_root) {
            paths.extend(text.lines().map(str::to_string));
        }
    }
    if let Ok(text) = crate::config::git(&["status", "--porcelain"], &config.repo_root) {
        for line in text.lines() {
            let path = line.get(3..).unwrap_or("");
            let path = path.rsplit(" -> ").next().unwrap_or(path);
            paths.push(path.to_string());
        }
    }
    paths
}

fn is_shared(config: &Config, service: &str) -> bool {
    if config.service(service, "shared_port").is_none() {
        return false;
    }
    let Some(pattern) = config.service(service, "shared_when") else {
        return true;
    };
    let Ok(regex) = regex::Regex::new(&pattern) else {
        return true;
    };
    !changed_paths(config)
        .iter()
        .any(|path| !regex.is_match(path))
}

fn unneeded(config: &Config, service: &str) -> bool {
    let Some(pattern) = config.service(service, "when") else {
        return false;
    };
    let Ok(regex) = regex::Regex::new(&pattern) else {
        return false;
    };
    !changed_paths(config).iter().any(|path| regex.is_match(path))
}

fn skipped(config: &Config, service: &str) -> bool {
    config
        .value("RAMIFY_SKIP")
        .map(|value| value.split_whitespace().any(|item| item == service))
        .unwrap_or(false)
}

fn ping_ports(config: &Config, env: &Path) -> bool {
    if !env.is_file() {
        return false;
    }
    for service in &config.services {
        if config.service(service, "optional").is_some() {
            continue;
        }
        let key = format!("{}_PORT", service.to_uppercase());
        let Some(port) = env_get(env, &key).and_then(|port| port.parse().ok()) else {
            continue;
        };
        if !health_ok(config, service, port) {
            return false;
        }
    }
    true
}

fn note_error(errors: &Path, service: &str, log: &str, message: &str) {
    use std::io::Write;
    if let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(errors)
    {
        let _ = writeln!(file, "{service}\t{log}\t{message}");
    }
}

pub fn write_set(args: &[String]) -> i32 {
    let Some(argument) = args.first() else {
        eprintln!("usage: ramify write-set <write-set-file> | --self-test");
        return 2;
    };
    if argument == "--self-test" {
        return write_set_self_test();
    }
    let base = std::env::var("BASE_REF").unwrap_or_else(|_| "origin/main".into());
    let path = Path::new(argument);
    if !path.is_file() {
        eprintln!("check-write-set: no write set at {}", path.display());
        return 2;
    }
    let text = match std::fs::read_to_string(path) {
        Ok(text) => text,
        Err(error) => {
            eprintln!("check-write-set: could not read {}: {error}", path.display());
            return 2;
        }
    };
    let (allowed, immutable) = parse_write_set(&text);
    if allowed.is_empty() {
        eprintln!(
            "check-write-set: {} declares no allowed pattern — every change would be a violation.",
            path.display()
        );
        return 2;
    }
    let changed = match changed_paths_for_write_set(&base) {
        Ok(paths) => paths,
        Err(error) => {
            eprintln!("{error}");
            return 2;
        }
    };
    let mut violations = 0;
    for changed_path in &changed {
        if matches_any(changed_path, &immutable) {
            println!("IMMUTABLE  {changed_path}");
            violations += 1;
        } else if !matches_any(changed_path, &allowed) {
            println!("OUTSIDE    {changed_path}");
            violations += 1;
        }
    }
    if violations > 0 {
        println!();
        println!(
            "check-write-set: {violations} path(s) break the write set in {}.",
            path.display()
        );
        println!("A path that genuinely belongs to this branch is ADDED to the write set as a deviation,");
        println!("with the reason beside it. It is never waved through.");
        return 1;
    }
    println!(
        "check-write-set: the diff stays inside {}.",
        path.display()
    );
    0
}

fn parse_write_set(text: &str) -> (Vec<String>, Vec<String>) {
    let mut allowed = Vec::new();
    let mut immutable = Vec::new();
    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        if let Some(rest) = line.strip_prefix('!') {
            immutable.push(rest.to_string());
        } else {
            allowed.push(line.to_string());
        }
    }
    (allowed, immutable)
}

fn changed_paths_for_write_set(base: &str) -> Result<Vec<String>, String> {
    if let Ok(explicit) = std::env::var("CHECK_WRITE_SET_PATHS") {
        let mut paths: Vec<String> = explicit
            .lines()
            .map(str::trim)
            .filter(|line| !line.is_empty())
            .map(str::to_string)
            .collect();
        paths.sort();
        paths.dedup();
        return Ok(paths);
    }
    if Command::new("git")
        .args(["rev-parse", "--verify", "--quiet", base])
        .output()
        .map(|output| !output.status.success())
        .unwrap_or(true)
    {
        return Err(format!("check-write-set: no such base ref: {base}"));
    }
    let mut paths = Vec::new();
    if let Ok(text) = crate::config::git(
        &["diff", "--name-only", &format!("{base}...HEAD")],
        &std::env::current_dir().unwrap_or_default(),
    ) {
        paths.extend(text.lines().map(str::to_string));
    }
    if let Ok(text) = crate::config::git(
        &["status", "--porcelain", "--untracked-files=all"],
        &std::env::current_dir().unwrap_or_default(),
    ) {
        for line in text.lines() {
            let rest = line.get(3..).unwrap_or("");
            for part in rest.split(" -> ") {
                let part = part.trim();
                let part = part.strip_prefix('"').and_then(|p| p.strip_suffix('"')).unwrap_or(part);
                if !part.is_empty() {
                    paths.push(part.to_string());
                }
            }
        }
    }
    paths.sort();
    paths.dedup();
    Ok(paths)
}

fn matches_any(path: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|pattern| matches_pattern(path, pattern))
}

fn matches_pattern(path: &str, pattern: &str) -> bool {
    let mut regex = String::from("^");
    for character in pattern.chars() {
        match character {
            '*' => regex.push_str(".*"),
            '?' => regex.push('.'),
            c => regex.push_str(&regex::escape(&c.to_string())),
        }
    }
    regex.push('$');
    regex::Regex::new(&regex)
        .map(|regex| regex.is_match(path))
        .unwrap_or(false)
}

fn write_set_self_test() -> i32 {
    let directory = std::env::temp_dir().join(format!("ramify-write-set-{}", std::process::id()));
    let _ = std::fs::create_dir_all(&directory);
    let paths = "src/api/orders.ts\ndocs/adr/0001-pipeline.md";
    std::env::set_var("CHECK_WRITE_SET_PATHS", paths);
    let fixtures = [
        ("a covered diff passes", "src/*\ndocs/adr/*\n", 0, None),
        ("an omitted path fails", "src/*\n", 1, Some("docs/adr/0001-pipeline.md")),
        (
            "an immutable path fails",
            "src/*\ndocs/adr/*\n!docs/adr/*\n",
            1,
            Some("docs/adr/0001-pipeline.md"),
        ),
    ];
    let mut status = 0;
    for (name, content, expected, needle) in fixtures {
        let file = directory.join(name.replace(' ', "_"));
        let _ = std::fs::write(&file, content);
        let (allowed, immutable) = parse_write_set(content);
        let mut violations = 0;
        let mut output = String::new();
        for path in paths.lines() {
            if matches_any(path, &immutable) {
                output.push_str(&format!("IMMUTABLE  {path}\n"));
                violations += 1;
            } else if !matches_any(path, &allowed) {
                output.push_str(&format!("OUTSIDE    {path}\n"));
                violations += 1;
            }
        }
        let code = if violations > 0 { 1 } else { 0 };
        let named = needle.map(|needle| output.contains(needle)).unwrap_or(true);
        if code != expected || !named {
            println!(
                "self-test FAIL [{name}]: expected exit {expected}, got {code}"
            );
            println!("{output}");
            status = 1;
        } else {
            println!("self-test ok   [{name}]");
        }
    }
    std::env::remove_var("CHECK_WRITE_SET_PATHS");
    let _ = std::fs::remove_dir_all(&directory);
    if status == 0 {
        println!("check-write-set: self-test passed (pattern matching only — the diff half is not covered here).");
    }
    status
}

pub fn new(args: &[String]) -> i32 {
    let config = match Config::resolve() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    let mut slug: Option<String> = None;
    let mut flow: Option<String> = None;
    let mut lane: Option<String> = None;
    let mut tags: Vec<String> = Vec::new();
    let mut index = 0;
    while index < args.len() {
        let argument = &args[index];
        match argument.as_str() {
            "--workflow" => {
                let value = args.get(index + 1);
                if value.is_none() || value.is_some_and(|value| value.starts_with('-')) {
                    eprintln!("ramify: --workflow needs a value");
                    return 1;
                }
                flow = value.cloned();
                index += 1;
            }
            "--lane" | "--tag" => {
                let value = args.get(index + 1);
                if value.is_none() || value.is_some_and(|value| value.starts_with('-')) {
                    eprintln!("ramify: {argument} needs a value");
                    return 1;
                }
                if argument == "--lane" {
                    lane = value.cloned();
                } else {
                    tags.extend(value.cloned());
                }
                index += 1;
            }
            _ if argument.starts_with("--") && argument.contains('=') => {
                let (name, value) = argument.split_once('=').unwrap_or(("", ""));
                eprintln!("ramify: write '{name} {value}', not '{argument}'");
                return 1;
            }
            _ if argument.starts_with('-') => {
                eprintln!("ramify: '{argument}' is not a slug: it starts with a dash");
                return 1;
            }
            _ => {
                if slug.is_some() {
                    eprintln!("ramify: usage: ramify new <slug> [--workflow W]");
                    return 1;
                }
                slug = Some(argument.clone());
            }
        }
        index += 1;
    }
    let Some(slug) = slug else {
        eprintln!("ramify: usage: ramify new <slug> [--workflow W [--lane L] [--tag T]...]");
        return 1;
    };
    if flow.is_none() && (lane.is_some() || !tags.is_empty()) {
        eprintln!("ramify: --lane and --tag need --workflow");
        return 1;
    }
    let worktrees = config.value("RAMIFY_WORKTREES").unwrap_or_else(|| config.worktrees.clone());
    let parent = if worktrees.starts_with('/') {
        PathBuf::from(&worktrees)
    } else {
        config.primary.join(&worktrees)
    };
    if std::fs::create_dir_all(&parent).is_err() {
        eprintln!("ramify: could not create {}", parent.display());
        return 1;
    }
    let parent = parent.canonicalize().unwrap_or(parent);
    let destination = parent.join(&slug);
    let mut branch = format!("{}{slug}", config.branch_prefix);
    let mut plan: Option<crate::workflow::BranchPlan> = None;
    if let Some(flow) = &flow {
        match crate::workflow::plan_branch(&config, flow, &slug, lane.as_deref(), &tags) {
            Ok(planned) => {
                branch = planned.branch.clone();
                plan = Some(planned);
            }
            Err(error) => {
                eprintln!("ramify: {error}");
                return 1;
            }
        }
    } else if config.workflow_source == "repo" || config.workflow_source == "select" {
        let workflow_config = Config::for_root(&config.repo_root).unwrap_or_else(|_| config.clone());
        if crate::workflow::report(&workflow_config, &config.repo_root, &branch).is_none() {
            eprintln!("ramify: no work order for branch {branch}");
            return 1;
        }
    }

    let fetched = Command::new("git")
        .args(["-C", &config.primary.to_string_lossy(), "fetch", "-q", &config.remote, &config.main])
        .status();
    if !matches!(fetched, Ok(status) if status.success()) {
        println!(
            "  (fetch failed — cutting from the last known {}/{})",
            config.remote, config.main
        );
    }
    let added = Command::new("git")
        .args([
            "-C",
            &config.primary.to_string_lossy(),
            "worktree",
            "add",
            "-q",
            "--no-track",
            "-b",
            &branch,
            &destination.to_string_lossy(),
            &format!("{}/{}", config.remote, config.main),
        ])
        .status();
    if !matches!(added, Ok(status) if status.success()) {
        return 1;
    }

    let copy = config.value("RAMIFY_COPY").unwrap_or_default();
    let exclude = config
        .value("RAMIFY_COPY_EXCLUDE")
        .unwrap_or_else(|| "worktrees/".into());
    for entry in copy.split_whitespace() {
        let source = config.primary.join(entry);
        let target = destination.join(entry);
        if source.is_dir() {
            let _ = std::fs::create_dir_all(&target);
            let mut command = Command::new("rsync");
            command.args(["-a", "--ignore-existing"]);
            if !exclude.is_empty() {
                command.args(["--exclude", &exclude]);
            }
            let _ = command
                .arg(format!("{}/", source.display()))
                .arg(format!("{}/", target.display()))
                .status();
        } else if source.exists() && !target.exists() {
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            let _ = std::fs::copy(&source, &target);
        }
    }

    if let (Some(flow), Some(plan)) = (&flow, &plan) {
        match crate::workflow::start_order(
            &destination,
            &config,
            flow,
            &slug,
            &plan.lane,
            &tags,
            &plan.notes,
            &branch,
        ) {
            Ok(lines) => {
                for line in lines {
                    println!("{line}");
                }
            }
            Err(_) => println!("  (no work order written — 'ramify step' will say why)"),
        }
    } else if config.workflow_source == "repo" || config.workflow_source == "select" {
        let workflow_config = Config::for_root(&destination).unwrap_or_else(|_| config.clone());
        if crate::workflow::report(&workflow_config, &destination, &branch).is_none() {
            eprintln!("ramify: no work order for branch {branch}");
            return 1;
        }
    }

    let autostart = !config.services.is_empty()
        && config.value("RAMIFY_AUTOSTART").as_deref() != Some("0");
    let bootstrap = config.value("RAMIFY_BOOTSTRAP").unwrap_or_default();
    if !bootstrap.is_empty() || autostart {
        let _ = std::fs::create_dir_all(&config.state_dir);
        let state = config.state_dir.join(&slug);
        let _ = std::fs::create_dir_all(&state);
        let _ = std::fs::remove_file(state.join(format!("{slug}.errors")));
        let log = state.join(format!("{slug}.bootstrap.log"));
        let errors = state.join(format!("{slug}.errors"));
        let self_binary = std::env::current_exe().unwrap_or_else(|_| PathBuf::from("ramify"));
        let script = "eval \"${1:-true}\" || { printf 'bootstrap\\t%s\\tbootstrap failed (exit %s)\\n' \"$6\" \"$?\" >>\"$5\"; exit 1; }\n[ \"$3\" = 1 ] && RAMIFY_ROOT=\"$2\" \"$4\" up";
        let stdout = std::fs::File::create(&log);
        if let Ok(stdout) = stdout {
            let stderr = stdout.try_clone().ok();
            use std::os::unix::process::CommandExt;
            let mut command = Command::new("bash");
            command
                .args([
                    "-c",
                    script,
                    "_",
                    &bootstrap,
                    &destination.to_string_lossy(),
                    if autostart { "1" } else { "0" },
                    &self_binary.to_string_lossy(),
                    &errors.to_string_lossy(),
                    &log.to_string_lossy(),
                ])
                .env("RAMIFY_ROOT", &destination)
                .stdin(std::process::Stdio::null())
                .stdout(stdout)
                .process_group(0);
            if let Some(stderr) = stderr {
                command.stderr(stderr);
            }
            if command.spawn().is_ok() {
                println!(
                    "  bootstrap       running in the background — {}",
                    log.display()
                );
            }
        }
    }

    println!(
        "  branch          {branch} (from {}/{})",
        config.remote, config.main
    );
    println!("ramify: worktree {}", destination.display());
    print!("{}", label_output(&config, &slug));
    0
}

fn label_output(config: &Config, slug: &str) -> String {
    use md5::{Digest, Md5};
    let mut hasher = Md5::new();
    hasher.update(slug.as_bytes());
    let digest = hasher.finalize();
    let hash: String = digest.iter().map(|byte| format!("{byte:02x}")).collect();
    let index = u32::from_str_radix(&hash[..8], 16).unwrap_or(0) % 8;
    let colors = ["red", "blue", "green", "yellow", "purple", "orange", "pink", "cyan"];
    let _ = config;
    format!("/rename {slug}\n/color {}\n", colors[index as usize])
}

pub fn cleanup() -> i32 {
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
    let worktrees = match crate::config::git(&["worktree", "list", "--porcelain"], &config.primary) {
        Ok(text) if !text.trim().is_empty() => text
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(str::to_string)
            .collect::<Vec<_>>(),
        _ => {
            eprintln!(
                "cannot read the worktree list from {} — refusing to clean",
                config.primary.display()
            );
            return 1;
        }
    };
    let mut found = false;
    for slot in 1..=config.max_slot {
        let path = config.slots.join(slot.to_string());
        if !path.is_file() {
            continue;
        }
        let holder = std::fs::read_to_string(&path)
            .unwrap_or_default()
            .split_whitespace()
            .next()
            .unwrap_or("")
            .to_string();
        let held = env_get(&config.state_dir.join(format!("{holder}.env")), "SLOT")
            .unwrap_or_default();
        if slot_is_stale(&config, slot) {
            let _ = std::fs::remove_file(&path);
            found = true;
            println!("  slot {slot}        released — {holder} left it claimed with nothing running");
        } else if !held.is_empty() && held != slot.to_string() {
            for port in slot_ports(&config, slot) {
                kill_port(&config, port);
            }
            let _ = std::fs::remove_file(&path);
            found = true;
            println!("  slot {slot}        orphan of {holder} killed — its state had moved to slot {held}");
        } else if worktree_root(&config.primary, &holder).is_none() {
            println!("  slot {slot}        {holder} stopped — its worktree no longer exists");
            kill_worktree(&config, &holder);
            found = true;
        }
    }

    let mut worktree_names: Vec<String> = std::fs::read_dir(&config.state_dir)
        .map(|entries| {
            entries
                .flatten()
                .filter_map(|entry| state_worktree(&entry.file_name().to_string_lossy()))
                .collect()
        })
        .unwrap_or_default();
    worktree_names.sort();
    worktree_names.dedup();
    for worktree in worktree_names {
        if worktree_root(&config.primary, &worktree).is_none() {
            kill_worktree(&config, &worktree);
            remove_state_and_logs(&config, &worktree);
            println!("  state          {worktree} removed — its worktree no longer exists");
            found = true;
        } else if !holds_a_slot(&config, &worktree) && stack_is_down(&config, &worktree) {
            let mut removed = false;
            if let Ok(entries) = std::fs::read_dir(&config.state_dir) {
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    if name.starts_with(&format!("{worktree}.")) && name.ends_with(".pid") {
                        let pid = std::fs::read_to_string(entry.path())
                            .unwrap_or_default()
                            .trim()
                            .to_string();
                        let alive = Command::new("kill")
                            .args(["-0", &pid])
                            .stdout(std::process::Stdio::null())
                            .stderr(std::process::Stdio::null())
                            .status()
                            .map(|status| status.success())
                            .unwrap_or(false);
                        if alive {
                            continue;
                        }
                        let _ = std::fs::remove_file(entry.path());
                        removed = true;
                    }
                }
            }
            let env = config.state_dir.join(format!("{worktree}.env"));
            if env.is_file() {
                let _ = std::fs::remove_file(&env);
                removed = true;
            }
            if removed {
                println!("  state          {worktree} cleared — its stack is not running any more");
                found = true;
            }
        }
    }

    for stray in crate::dash::strays(&config) {
        let Ok(group) = stray.pgid.parse::<u64>() else {
            continue;
        };
        if our_group(&config, group) {
            signal_group(group);
        }
        found = true;
        println!(
            "  stray          {} {} killed (process group {}) — no pid file pointed at it",
            stray.worktree, stray.service, stray.pgid
        );
    }

    if !found {
        println!("nothing to clean — every claimed slot has a live stack behind it");
    }
    0
}

fn state_worktree(name: &str) -> Option<String> {
    if let Some(worktree) = name.strip_suffix(".env") {
        return (!worktree.is_empty()).then(|| worktree.to_string());
    }
    let regex = regex::Regex::new(r"^(.+?)\.(?:[a-z0-9_]+(?:\.prepare)?\.(?:log|pid))$").ok()?;
    regex
        .captures(name)
        .and_then(|captures| captures.get(1))
        .map(|worktree| worktree.as_str().to_string())
}

fn holds_a_slot(config: &Config, worktree: &str) -> bool {
    for slot in 1..=config.max_slot {
        let holder = std::fs::read_to_string(config.slots.join(slot.to_string()))
            .unwrap_or_default();
        if holder.split_whitespace().next() == Some(worktree) {
            return true;
        }
    }
    false
}

fn remove_state_and_logs(config: &Config, worktree: &str) {
    if let Ok(entries) = std::fs::read_dir(&config.state_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{worktree}.")) {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_dir_all(config.state_dir.join(format!("{worktree}.up.lock")));
}

fn worktree_root(primary: &Path, worktree: &str) -> Option<PathBuf> {
    let text = crate::config::git(&["worktree", "list", "--porcelain"], primary).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .find(|path| path.file_name().map(|name| name.to_string_lossy() == worktree).unwrap_or(false))
}
