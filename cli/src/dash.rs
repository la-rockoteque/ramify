use crate::config::{git, Config};
use crate::workflow;
use serde::Serialize;
use serde_json::value::RawValue;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::io::{Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

#[derive(Serialize)]
pub struct Project {
    pub name: String,
    pub primary: String,
    pub stacks: bool,
    pub jira: Option<String>,
    pub containers: Vec<Container>,
    pub shared: Vec<Shared>,
    pub build_servers: Vec<BuildServer>,
    pub strays: Vec<Stray>,
    pub orphans: Vec<Orphan>,
    pub worktrees: Vec<WorktreeJson>,
}

#[derive(Serialize)]
pub struct Container {
    pub name: String,
    pub up: bool,
    pub mb: Option<u64>,
}

#[derive(Serialize)]
pub struct Shared {
    pub service: String,
    pub port: u32,
    pub up: bool,
    pub mb: Option<u64>,
}

#[derive(Serialize)]
pub struct BuildServer {
    pub kind: String,
    pub count: u64,
    pub mb: u64,
}

#[derive(Serialize)]
pub struct Stray {
    pub pgid: String,
    pub worktree: String,
    pub service: String,
}

#[derive(Serialize)]
pub struct Orphan {
    pub slot: u32,
    pub worktree: String,
    pub moved_to: String,
}

#[derive(Serialize)]
pub struct WorktreeJson {
    pub name: String,
    pub path: String,
    pub branch: String,
    pub ticket: Option<String>,
    pub workflow: Option<Box<RawValue>>,
    pub primary: bool,
    pub dirty: bool,
    pub slot: Option<u32>,
    pub uptime: Option<String>,
    pub errors: Vec<ErrorJson>,
    pub services: Vec<ServiceJson>,
}

#[derive(Serialize)]
pub struct ErrorJson {
    pub service: String,
    pub log: Option<String>,
    pub message: String,
}

#[derive(Serialize)]
pub struct ServiceJson {
    pub name: String,
    pub port: u32,
    pub kind: String,
    pub state: String,
    pub log: Option<String>,
    pub mb: Option<u64>,
}

pub fn project_json() -> Result<String, String> {
    let config = Config::resolve()?;
    let project = project(&config)?;
    serde_json::to_string(&project).map_err(|error| error.to_string())
}

pub fn all_json() -> Result<String, String> {
    let roots = projects();
    let mut projects = Vec::new();
    std::thread::scope(|scope| {
        let handles: Vec<_> = roots
            .iter()
            .map(|root| {
                scope.spawn(move || {
                    let config = Config::for_root(root).ok()?;
                    project(&config).ok()
                })
            })
            .collect();
        for handle in handles {
            if let Ok(Some(project)) = handle.join() {
                projects.push(project);
            }
        }
    });
    let mut text = String::from("[");
    for (index, project) in projects.iter().enumerate() {
        if index > 0 {
            text.push(',');
        }
        text.push_str(&serde_json::to_string(project).map_err(|e| e.to_string())?);
    }
    text.push(']');
    Ok(text)
}

pub(crate) fn projects() -> Vec<PathBuf> {
    let mut roots = BTreeSet::new();
    let mut unsorted: Vec<String> = Vec::new();
    let home = std::env::var("HOME").unwrap_or_default();
    let config_dir =
        std::env::var("CLAUDE_CONFIG_DIR").unwrap_or_else(|_| format!("{home}/.claude"));
    let registry =
        std::env::var("RAMIFY_REGISTRY").unwrap_or_else(|_| format!("{config_dir}/ramify/projects"));
    if let Ok(text) = std::fs::read_to_string(registry) {
        for line in text.lines() {
            let path = PathBuf::from(line.trim());
            if !line.trim().is_empty() && path.is_dir() {
                roots.insert(path.clone());
                unsorted.push(path.to_string_lossy().into_owned());
            }
        }
    }
    let state_root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    if let Ok(entries) = std::fs::read_dir(state_root) {
        for entry in entries.flatten() {
            if let Ok(text) = std::fs::read_to_string(entry.path().join("primary")) {
                let path = PathBuf::from(text.trim());
                if path.is_dir() {
                    roots.insert(path.clone());
                    unsorted.push(path.to_string_lossy().into_owned());
                }
            }
        }
    }
    // `projects all` sorts the paths as strings; path ordering is not the same thing.
    if let Some(mut sorted) = sort_lines(&unsorted) {
        return sorted.drain(..).map(PathBuf::from).collect();
    }
    roots.into_iter().collect()
}

fn sort_lines(lines: &[String]) -> Option<Vec<String>> {
    let mut child = Command::new("sort")
        .arg("-u")
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .ok()?;
    {
        let mut stdin = child.stdin.take()?;
        for line in lines {
            let _ = writeln!(stdin, "{line}");
        }
    }
    let output = child.wait_with_output().ok()?;
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_string)
            .collect(),
    )
}

fn project(config: &Config) -> Result<Project, String> {
    let processes = processes();
    let text = git(&["worktree", "list", "--porcelain"], &config.primary)?;
    let mut worktrees = Vec::new();
    for path in text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(|line| canonical(Path::new(line)))
    {
        let name = basename(&path);
        let env_file = config.state_dir.join(format!("{name}.env"));
        let slot = env_get(&env_file, "SLOT").and_then(|value| value.parse().ok());
        let branch = branch_of(&path);
        let ticket = ticket_of(&branch, config);
        let workflow = workflow::report(config, &path, &branch)
            .map(|progress| RawValue::from_string(workflow::progress_json(&progress)).unwrap());
        let dirty = !git(&["status", "--porcelain"], &path)
            .unwrap_or_default()
            .trim()
            .is_empty();
        let uptime = if env_file.is_file() {
            uptime_of(&env_file)
        } else {
            None
        };
        let errors = errors(&config.state_dir.join(format!("{name}.errors")));
        let services = services(config, &name, &env_file, &processes);
        worktrees.push(WorktreeJson {
            name,
            path: path.to_string_lossy().into_owned(),
            branch,
            ticket,
            workflow,
            primary: path == config.primary,
            dirty,
            slot,
            uptime,
            errors,
            services,
        });
    }
    Ok(Project {
        name: config.project.clone(),
        primary: config.primary.to_string_lossy().into_owned(),
        stacks: !config.services.is_empty(),
        jira: (!config.jira_url.is_empty())
            .then(|| config.jira_url.trim_end_matches('/').to_string()),
        containers: containers(config),
        shared: shared(config, &processes),
        build_servers: build_servers(&processes),
        strays: Vec::new(),
        orphans: orphans(config),
        worktrees,
    })
}

fn services(
    config: &Config,
    worktree: &str,
    env_file: &Path,
    processes: &[Process],
) -> Vec<ServiceJson> {
    let mut services = Vec::new();
    for service in &config.services {
        let upper = service.to_uppercase();
        let Some(port) = env_get(env_file, &format!("{upper}_PORT")) else {
            continue;
        };
        let Some(port) = port.parse().ok() else {
            continue;
        };
        let shared = config.service(service, "shared_port").is_some();
        let kind = if shared {
            if env_get(env_file, &format!("{upper}_SHARED")).as_deref() == Some("1") {
                "shared"
            } else {
                "private"
            }
        } else {
            "own"
        };
        let log = if kind == "shared" {
            config.state_dir.join("shared").join(format!("{service}.log"))
        } else {
            config.state_dir.join(format!("{worktree}.{service}.log"))
        };
        let mb = if kind == "shared" {
            None
        } else {
            service_mb(
                &config.state_dir.join(format!("{worktree}.{service}.pid")),
                processes,
            )
        };
        services.push(ServiceJson {
            name: service.clone(),
            port,
            kind: kind.to_string(),
            state: probe(config, service, port).to_string(),
            log: log.is_file().then(|| log.to_string_lossy().into_owned()),
            mb,
        });
    }
    services
}

fn shared(config: &Config, processes: &[Process]) -> Vec<Shared> {
    let mut shared = Vec::new();
    for service in &config.services {
        let Some(shared_port) = config.service(service, "shared_port") else {
            continue;
        };
        let Some(port) = shared_port.parse().ok() else {
            continue;
        };
        shared.push(Shared {
            service: service.clone(),
            port,
            up: health_ok(config, service, port),
            mb: service_mb(
                &config
                    .state_dir
                    .join("shared")
                    .join(format!("{service}.pid")),
                processes,
            ),
        });
    }
    shared
}

fn containers(config: &Config) -> Vec<Container> {
    if config.containers.is_empty() {
        return Vec::new();
    }
    let running: Vec<&String> = config
        .containers
        .iter()
        .filter(|name| docker_running(name))
        .collect();
    let memory = docker_memory(&running);
    config
        .containers
        .iter()
        .map(|name| Container {
            name: name.clone(),
            up: running.contains(&name),
            mb: memory.get(name).copied(),
        })
        .collect()
}

fn docker_running(name: &str) -> bool {
    Command::new("docker")
        .args([
            "ps",
            "--filter",
            &format!("name=^{name}$"),
            "--filter",
            "status=running",
            "-q",
        ])
        .output()
        .map(|output| !String::from_utf8_lossy(&output.stdout).trim().is_empty())
        .unwrap_or(false)
}

fn docker_memory(names: &[&String]) -> HashMap<String, u64> {
    let mut memory = HashMap::new();
    if names.is_empty() {
        return memory;
    }
    let mut command = Command::new("docker");
    command.args([
        "stats",
        "--no-stream",
        "--format",
        "{{.Name}} {{.MemUsage}}",
    ]);
    command.args(names.iter().map(|name| name.as_str()));
    let Ok(output) = command.output() else {
        return memory;
    };
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let mut parts = line.split_whitespace();
        let Some(name) = parts.next() else {
            continue;
        };
        let Some(value) = parts.next() else {
            continue;
        };
        let digits: String = value
            .chars()
            .take_while(|c| c.is_ascii_digit() || *c == '.')
            .collect();
        let unit: String = value.chars().skip(digits.len()).collect();
        let Ok(number) = digits.parse::<f64>() else {
            continue;
        };
        let factor = match unit.as_str() {
            "GiB" | "GB" => 1024.0,
            "KiB" | "kB" => 1.0 / 1024.0,
            "B" => 1.0 / 1_048_576.0,
            _ => 1.0,
        };
        memory.insert(name.to_string(), (number * factor) as u64);
    }
    memory
}

fn orphans(config: &Config) -> Vec<Orphan> {
    let mut orphans = Vec::new();
    for slot in 1..=config.max_slot {
        let path = config.slots.join(slot.to_string());
        let Ok(holder) = std::fs::read_to_string(&path) else {
            continue;
        };
        let Some(worktree) = holder.split_whitespace().next() else {
            continue;
        };
        let held = env_get(&config.state_dir.join(format!("{worktree}.env")), "SLOT")
            .unwrap_or_default();
        if held.is_empty() || held == slot.to_string() {
            continue;
        }
        orphans.push(Orphan {
            slot,
            worktree: worktree.to_string(),
            moved_to: held,
        });
    }
    orphans
}

pub(crate) struct Process {
    pub(crate) rss: u64,
    pub(crate) pgid: u64,
    pub(crate) command: String,
}

pub(crate) fn processes() -> Vec<Process> {
    let Ok(output) = Command::new("ps")
        .args(["-axww", "-o", "rss=,pgid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(parse_ps)
        .collect()
}

fn parse_ps(line: &str) -> Option<Process> {
    let line = line.trim_start();
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut tokens: Vec<&str> = Vec::new();
    while tokens.len() < 2 {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if start == index {
            break;
        }
        tokens.push(&line[start..index]);
    }
    let rss = tokens.first()?.parse().ok()?;
    let pgid = tokens.get(1)?.parse().ok()?;
    Some(Process {
        rss,
        pgid,
        command: line[index..].trim_start().to_string(),
    })
}

fn is_build_server(command: &str) -> bool {
    (command.contains("MSBuild.dll")
        && (command.contains("nodeReuse:true") || command.contains("nodemode:8")))
        || command.contains("VBCSCompiler.dll")
        || (command.contains("rzc.dll") && command.contains(" server"))
}

pub(crate) fn service_mb(pidfile: &Path, processes: &[Process]) -> Option<u64> {
    let group: u64 = std::fs::read_to_string(pidfile).ok()?.trim().parse().ok()?;
    let mut total = 0u64;
    let mut count = 0u64;
    for process in processes {
        if process.pgid == group && !is_build_server(&process.command) {
            total += process.rss;
            count += 1;
        }
    }
    (count > 0).then_some(total / 1024)
}

fn build_servers(processes: &[Process]) -> Vec<BuildServer> {
    let mut kinds: BTreeMap<&str, (u64, u64)> = BTreeMap::new();
    for process in processes {
        let kind = if process.command.contains("MSBuild.dll")
            && process.command.contains("nodeReuse:true")
        {
            "msbuild"
        } else if process.command.contains("MSBuild.dll") && process.command.contains("nodemode:8")
        {
            "msbuild-server"
        } else if process.command.contains("VBCSCompiler.dll") {
            "roslyn"
        } else if process.command.contains("rzc.dll") && process.command.contains(" server") {
            "razor"
        } else {
            continue;
        };
        let entry = kinds.entry(kind).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += process.rss;
    }
    kinds
        .into_iter()
        .map(|(kind, (count, rss))| BuildServer {
            kind: kind.to_string(),
            count,
            mb: rss / 1024,
        })
        .collect()
}

pub(crate) fn health_ok(config: &Config, service: &str, port: u32) -> bool {
    http_status(config, service, port)
        .map(|status| status < 400)
        .unwrap_or(false)
}

pub(crate) fn probe(config: &Config, service: &str, port: u32) -> &'static str {
    if !listening(port) {
        return "down";
    }
    if http_status(config, service, port) == Some(200) {
        "ready"
    } else {
        "starting"
    }
}

fn health_path(config: &Config, service: &str) -> String {
    let health = config.service(service, "health").unwrap_or_default();
    if health.starts_with('/') {
        health
    } else if health.is_empty() {
        "/".into()
    } else {
        format!("/{health}")
    }
}

pub(crate) fn listening(port: u32) -> bool {
    for host in ["127.0.0.1", "::1"] {
        if let Ok(mut addresses) = (host, port as u16).to_socket_addrs() {
            if let Some(address) = addresses.next() {
                if TcpStream::connect_timeout(&address, Duration::from_millis(200)).is_ok() {
                    return true;
                }
            }
        }
    }
    false
}

fn http_status(config: &Config, service: &str, port: u32) -> Option<u16> {
    http_status_path(port, &health_path(config, service))
}

/// The HTTP status code for a path on a local port, trying IPv4 then IPv6.
pub(crate) fn http_status_path(port: u32, path: &str) -> Option<u16> {
    for host in ["127.0.0.1", "::1"] {
        if let Some(status) = http_code(host, port, path) {
            return Some(status);
        }
    }
    None
}

fn http_code(host: &str, port: u32, path: &str) -> Option<u16> {
    let address = (host, port as u16).to_socket_addrs().ok()?.next()?;
    let mut stream = TcpStream::connect_timeout(&address, Duration::from_secs(3)).ok()?;
    stream.set_read_timeout(Some(Duration::from_secs(3))).ok()?;
    stream
        .set_write_timeout(Some(Duration::from_secs(3)))
        .ok()?;
    let request = format!("GET {path} HTTP/1.1\r\nHost: localhost\r\nConnection: close\r\n\r\n");
    stream.write_all(request.as_bytes()).ok()?;
    let mut buffer = [0u8; 512];
    let read = stream.read(&mut buffer).ok()?;
    let head = String::from_utf8_lossy(&buffer[..read]);
    head.lines()
        .next()?
        .split_whitespace()
        .nth(1)?
        .parse()
        .ok()
}

fn basename(path: &Path) -> String {
    path.file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn canonical(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn branch_of(path: &Path) -> String {
    git(&["rev-parse", "--abbrev-ref", "HEAD"], path)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "?".into())
}

pub(crate) fn env_get(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .map(str::to_string)
}

fn ticket_of(branch: &str, config: &Config) -> Option<String> {
    if config.jira_url.is_empty() {
        return None;
    }
    let pattern = std::env::var("RAMIFY_TICKET_PATTERN")
        .unwrap_or_else(|_| "[A-Za-z][A-Za-z0-9]*-[0-9]+".into());
    let regex = regex::Regex::new(&format!("-({pattern})-")).ok()?;
    let text = format!("-{}-", branch.replace('/', "-"));
    regex
        .captures(&text)
        .and_then(|captures| captures.get(1))
        .map(|found| found.as_str().to_uppercase())
}

pub(crate) fn uptime_of(path: &Path) -> Option<String> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    let elapsed = std::time::SystemTime::now()
        .duration_since(modified)
        .ok()?
        .as_secs();
    if elapsed < 3600 {
        Some(format!("{}m", elapsed / 60))
    } else {
        Some(format!("{}h{:02}m", elapsed / 3600, (elapsed % 3600) / 60))
    }
}

fn errors(path: &Path) -> Vec<ErrorJson> {
    let Ok(text) = std::fs::read_to_string(path) else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            let mut parts = line.splitn(3, '\t');
            let service = parts.next()?.to_string();
            let log = parts.next().unwrap_or("").to_string();
            let message = parts.next().unwrap_or("").to_string();
            let log = (!log.is_empty() && Path::new(&log).is_file()).then_some(log);
            Some(ErrorJson {
                service,
                log,
                message,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_last_slot_line() {
        let dir = std::env::temp_dir().join(format!("ramify-env-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("x.env");
        std::fs::write(&file, "SLOT=2\nWEB_PORT=3002\nSLOT=3\n").unwrap();
        assert_eq!(env_get(&file, "SLOT").as_deref(), Some("3"));
    }

    #[test]
    fn formats_uptime() {
        let dir = std::env::temp_dir().join(format!("ramify-up-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("x");
        std::fs::write(&file, "").unwrap();
        let text = uptime_of(&file).expect("uptime");
        assert!(text.ends_with('m'), "{text}");
    }
}
