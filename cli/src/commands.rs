use crate::config::Config;
use crate::dash::{health_ok, listening};
use std::path::Path;
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
