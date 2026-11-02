use serde_json::Value;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};
use std::path::{Component, Path, PathBuf};
use std::process::Command;

const SEP: char = '\u{1f}';

pub fn setup(args: &[String]) -> i32 {
    let mut root: Option<PathBuf> = None;
    let mut mode = "write";
    let mut local = false;
    for argument in args {
        match argument.as_str() {
            "--print" => mode = "print",
            "--force" => mode = "force",
            "--local" => local = true,
            other if !other.starts_with('-') && root.is_none() => root = Some(PathBuf::from(other)),
            _ => {}
        }
    }
    let root = root
        .or_else(|| std::env::var_os("RAMIFY_ROOT").map(PathBuf::from))
        .or_else(|| std::env::var_os("RAMIFY_SETUP_ROOT").map(PathBuf::from))
        .or_else(|| crate::config::git(&["rev-parse", "--show-toplevel"], Path::new(".")).ok().map(|text| PathBuf::from(text.trim())));
    let Some(root) = root else {
        eprintln!("ramify: not inside a git checkout (or set RAMIFY_ROOT)");
        return 1;
    };
    let root = root.canonicalize().unwrap_or(root);
    let primary = crate::config::git(&["worktree", "list", "--porcelain"], &root)
        .ok()
        .and_then(|text| text.lines().find_map(|line| line.strip_prefix("worktree ")).map(PathBuf::from));
    let Some(primary) = primary else {
        eprintln!("ramify: {} is not a git checkout", root.display());
        return 1;
    };
    let target = if local {
        root.join(".claude/ramify.conf")
    } else {
        root.join(".ramify.conf")
    };
    match draft(&root, &primary) {
        Ok(out) => {
            if mode == "print" {
                print!("{out}");
                return 0;
            }
            if target.is_file() && mode != "force" {
                eprintln!(
                    "ramify: {} exists — --force to overwrite, --print to compare",
                    target.display()
                );
                return 1;
            }
            if let Some(parent) = target.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if std::fs::write(&target, &out).is_err() {
                eprintln!("ramify: could not write {}", target.display());
                return 1;
            }
            println!("wrote {}", target.display());
            println!("{}", summary(&out));
            0
        }
        Err(error) => {
            eprintln!("ramify: {error}");
            1
        }
    }
}

fn summary(out: &str) -> String {
    let services = field(out, "RAMIFY_SERVICES")
        .map(|value| value.trim_matches('"').to_string())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(none detected)".into());
    let main = field(out, "RAMIFY_MAIN").unwrap_or_default();
    let prefix = field(out, "RAMIFY_BRANCH_PREFIX")
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "(none)".into());
    let worktrees = field(out, "RAMIFY_WORKTREES").unwrap_or_default();
    format!("services: {services}   main: {main}   prefix: {prefix}   worktrees: {worktrees}")
}

fn field(out: &str, key: &str) -> Option<String> {
    out.lines().find_map(|line| {
        let value = line.strip_prefix(&format!("{key}="))?;
        Some(value.trim_matches(['\'', '"']).to_string())
    })
}

struct Parts {
    main: String,
    prefix: String,
    worktrees: String,
    copy: String,
    bootstrap: String,
    ready: String,
    qa_plan: String,
    notes: String,
    compose_block: String,
    services: String,
    body: String,
    name: String,
}

fn draft(root: &Path, primary: &Path) -> Result<String, String> {
    let name = primary
        .file_name()
        .map(|name| name.to_string_lossy().into_owned())
        .unwrap_or_default();
    let main = crate::config::git(&["symbolic-ref", "--short", "refs/remotes/origin/HEAD"], root)
        .ok()
        .map(|text| text.trim().trim_start_matches("origin/").to_string())
        .filter(|value| !value.is_empty())
        .or_else(|| {
            crate::config::git(&["rev-parse", "--abbrev-ref", "HEAD"], root)
                .ok()
                .map(|text| text.trim().to_string())
                .filter(|value| !value.is_empty())
        })
        .unwrap_or_else(|| "main".into());
    let prefix = branch_prefix(root, &main);
    let worktrees = worktree_dir(root, &name);

    let mut parts = Parts {
        main: main.clone(),
        prefix: prefix.clone(),
        worktrees: worktrees.clone(),
        copy: String::new(),
        bootstrap: String::new(),
        ready: String::new(),
        qa_plan: String::new(),
        notes: String::new(),
        compose_block: String::new(),
        services: String::new(),
        body: String::new(),
        name: name.clone(),
    };
    let registry = registry_path();
    let span: u32 = std::env::var("RAMIFY_MAX_SLOT").ok().and_then(|v| v.parse().ok()).unwrap_or(5);
    let mut held = held_ranges(root, &registry, span);
    let detected = detect_packages(root);
    let mut pkgdirs = Vec::new();
    let mut webdirs = Vec::new();
    let mut installs = Vec::new();
    let mut services: Vec<String> = Vec::new();
    for row in detected {
        let fields: Vec<&str> = row.split(SEP).collect();
        match fields.first().copied().unwrap_or("") {
            "install" => installs.push(fields.get(1..).unwrap_or(&[]).join("\u{1f}")),
            "svc" => {
                let service = fields.get(1).copied().unwrap_or("");
                let dir = fields.get(2).copied().unwrap_or("");
                let script = fields.get(3).copied().unwrap_or("other");
                let port: u32 = fields.get(4).and_then(|v| v.parse().ok()).unwrap_or(3000);
                let cmd = fields.get(5).copied().unwrap_or("");
                let picked = pick_port(&mut held, port, span);
                webdirs.push(if dir.is_empty() { String::new() } else { format!("{dir}/") });
                let _ = script;
                add_service(&mut parts, &mut services, service, picked, dir, "/", cmd, "");
            }
            "sb" => {
                let service = fields.get(1).copied().unwrap_or("");
                let dir = fields.get(2).copied().unwrap_or("");
                let cmd = fields.get(3).copied().unwrap_or("");
                let picked = pick_port(&mut held, 6006, span);
                add_service(
                    &mut parts,
                    &mut services,
                    service,
                    picked,
                    dir,
                    "/index.json",
                    cmd,
                    "# Boots last and nothing waits on it; RAMIFY_SKIP=$service leaves it out.\n",
                );
                let last = parts.body.len();
                let _ = last;
                parts.body.push_str(&format!("{service}_optional=1\n"));
            }
            "fixed" => {
                let dir = fields.get(1).copied().unwrap_or("");
                parts.body.push('\n');
                parts.body.push_str(&format!(
                    "# {}: NestJS listens on a fixed port in src/main.ts (port not configurable), so it is not a service.\n",
                    if dir.is_empty() { "." } else { dir }
                ));
            }
            "orchestrator" => {
                let dir = fields.get(1).copied().unwrap_or("");
                let script = fields.get(2).copied().unwrap_or("");
                parts.body.push('\n');
                parts.body.push_str(&format!(
                    "# {}: the {script} script runs other packages, so it is not a service. They are listed on their own.\n",
                    if dir.is_empty() { "." } else { dir }
                ));
            }
            "mobile" => {
                let notes = fields.get(2).copied().unwrap_or("");
                parts.notes = if parts.notes.is_empty() {
                    notes.to_string()
                } else {
                    format!("{} {}", parts.notes, notes)
                };
            }
            "pkg" => pkgdirs.push(if fields.get(1).copied().unwrap_or("").is_empty() {
                ".".to_string()
            } else {
                fields[1].to_string()
            }),
            _ => {}
        }
    }
    services.sort();
    services.dedup();
    // The shell prints "api first"; add_service appends, so move api to the front.
    if let Some(index) = services.iter().position(|service| service == "api") {
        let api = services.remove(index);
        services.insert(0, api);
    }
    parts.services = services.join(" ");
    for install in installs {
        let mut fields = install.split(SEP);
        let dir = fields.next().unwrap_or("");
        let cmd = fields.next().unwrap_or("");
        let ready = fields.next().unwrap_or("");
        let piece = if dir.is_empty() {
            cmd.to_string()
        } else {
            format!("(cd \"{dir}\" && {cmd})")
        };
        parts.bootstrap = if parts.bootstrap.is_empty() {
            piece
        } else {
            format!("{} && {piece}", parts.bootstrap)
        };
        if !ready.is_empty() {
            parts.ready = if dir.is_empty() {
                ready.to_string()
            } else {
                format!("{dir}/{ready}")
            };
        }
    }

    parts.copy = copy_paths(root, &pkgdirs);
    let (compose_block, compose, containers) = compose_block(root);
    parts.compose_block = compose_block;
    let _ = containers;
    parts.qa_plan = ["docs/stories", "docs/specs", "docs/qa", "qa"]
        .iter()
        .find(|dir| root.join(dir).is_dir())
        .map(|dir| format!("{dir}/*/<slug>/qa-plan.md"))
        .unwrap_or_default();
    if !parts.notes.is_empty() {
        parts.notes = format!(
            "# Examples. Replace them with the repo's own.\n# RAMIFY_QA_RESULTS='docs/stories/*/<slug>/qa-results.md'\nRAMIFY_QA_NOTES='{}'",
            sq(&parts.notes)
        );
    } else {
        parts.notes = "# Examples. Replace them with the repo's own.\n# RAMIFY_QA_RESULTS='docs/stories/*/<slug>/qa-results.md'\n# RAMIFY_QA_NOTES='Seed demo data with ./scripts/seed.sh before list screens.'".into();
    }

    // ASP.NET Core Web SDK project: shared API first.
    let csproj = crate::config::git(&["ls-files", "*.csproj"], root).unwrap_or_default();
    for cs in csproj.lines().filter(|cs| !cs.is_empty()) {
        let lowered = cs.to_lowercase();
        if lowered.contains("test") {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(cs)) else {
            continue;
        };
        if !text.contains("Sdk=\"Microsoft.NET.Sdk.Web\"") {
            continue;
        }
        let project = Path::new(cs).parent().map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
        let mut names: Vec<String> = webdirs
            .iter()
            .filter(|dir| !dir.is_empty())
            .cloned()
            .collect();
        names.push("docs/".into());
        let escaped: Vec<String> = names
            .iter()
            .map(|name| name.trim_end_matches('/').replace('.', "\\."))
            .collect();
        let shared_when = format!("^({})", escaped.join("|"));
        let api_port = pick_port(&mut held, 5000, span);
        let api_shared = pick_port(&mut held, 5100, 0);
        parts.body.push('\n');
        parts.body.push_str(&format!(
            "# Slot N → {api_port}+N. Slot 0 is never used, and a port someone else holds skips the slot.\napi_port={api_port}\n# Check it: the path that answers once the API really serves, not once it logs « listening ».\napi_health=/health\napi_timeout=180\n# --urls, not ASPNETCORE_URLS: the launch profile overrides the environment variable.\napi_cmd='DOTNET_WATCH_RESTART_ON_RUDE_EDIT=1 dotnet watch --project {project} --non-interactive run --urls http://localhost:\\$PORT'\napi_shared_port={api_shared}\napi_shared_cmd='dotnet run --project {project} --urls http://localhost:\\$PORT'\n# While every changed path matches, the branch reuses the shared API and skips api_prepare.\napi_shared_when='{shared_when}'\n# api_prepare='dotnet ef database update --project <Infrastructure> --startup-project {project}'\n"
        ));
        // Prefix every service command but storybook with the API target.
        parts.body = parts
            .body
            .lines()
            .map(|line| {
                if line.ends_with("_cmd='") {
                    let _ = line;
                    line.to_string()
                } else if line.contains("_cmd='") && !line.starts_with("storybook_cmd") && !line.starts_with("api_cmd") && !line.starts_with("api_shared_cmd") {
                    line.replacen("_cmd='", "_cmd='VITE_API_TARGET=http://localhost:$API_PORT ", 1)
                } else {
                    line.to_string()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        parts.body.push_str("# web_smoke='/api/health 200|401'\n");
        services.insert(0, "api".into());
        break;
    }
    services.sort();
    services.dedup();
    if let Some(index) = services.iter().position(|service| service == "api") {
        let api = services.remove(index);
        services.insert(0, api);
    }
    parts.services = services.join(" ");
    let _ = compose;

    let workflow = "# Workflow selection: no repository development workflow detected\nRAMIFY_WORKFLOW_SOURCE=auto\nRAMIFY_WORKFLOW_DOCS='[]'";
    let mut out = format!(
        "# ramify — {name}. Sourced as bash: single-quote commands so $PORT and $<SERVICE>_PORT\n# expand when the service starts, not now. Drafted by 'ramify setup'; review every line.\n\n# ── Branching ──\n# 'ramify new <slug>' cuts {prefix}<slug> from origin/{main} into {worktrees}/<slug>.\nRAMIFY_MAIN={main}\nRAMIFY_BRANCH_PREFIX={prefix}\nRAMIFY_WORKTREES={worktrees}\n# Gitignored paths every new worktree needs; git does not bring them.\nRAMIFY_COPY=\"{copy}\"\nRAMIFY_BOOTSTRAP='{bootstrap}'\n# Exists once the bootstrap is done; the worktree hook waits for it before 'up'.\nRAMIFY_READY={ready}\n{workflow}\n# Uncomment to let a branch that is behind main be pushed.\n# RAMIFY_GATE_BEHIND=0\n\n# ── QA recipe ──\n# The checklist ask-for-qa hands over. <slug> is the worktree name.\nRAMIFY_QA_PLAN='{qa_plan}'\n{notes}\n\n# ── Shared containers (never stopped by ramify) ──\n{compose_block}\n\n# ── Services, started in this order ──\nRAMIFY_SERVICES=\"{services}\"\n{body}",
        name = parts.name,
        prefix = parts.prefix,
        main = parts.main,
        worktrees = parts.worktrees,
        copy = parts.copy,
        bootstrap = parts.bootstrap,
        ready = parts.ready,
        qa_plan = parts.qa_plan,
        notes = parts.notes,
        compose_block = parts.compose_block,
        services = parts.services,
        body = parts.body,
    );
    if parts.services.is_empty() {
        out.push_str("\n# No dev server detected. Leave RAMIFY_SERVICES empty for branching only, or add a service:\n# RAMIFY_SERVICES=\"app\"\n# app_port=8000                 # slot N → 8000+N\n# app_dir=                      # relative to the worktree\n# app_health=/healthz\n# app_cmd='python -m uvicorn main:app --port $PORT'\n");
    }
    Ok(out)
}

fn add_service(
    parts: &mut Parts,
    services: &mut Vec<String>,
    service: &str,
    port: u32,
    dir: &str,
    health: &str,
    cmd: &str,
    prefixes: &str,
) {
    parts.body.push('\n');
    parts.body.push_str(prefixes);
    parts.body.push_str(&format!(
        "{service}_port={port}\n{service}_dir='{}'\n{service}_health={health}\n{service}_cmd='{}'\n",
        sq(dir),
        sq(cmd)
    ));
    services.push(service.to_string());
}

fn sq(text: &str) -> String {
    text.replace('\'', "'\\''")
}

fn branch_prefix(root: &Path, main: &str) -> String {
    let text = crate::config::git(
        &["for-each-ref", "--format=%(refname:lstrip=2)", "refs/heads", "refs/remotes/origin"],
        root,
    )
    .unwrap_or_default();
    let ticket = regex::Regex::new(r"^[A-Z][A-Z0-9]*-[0-9]+/").ok();
    let mut branches: Vec<String> = text
        .lines()
        .map(|line| line.trim_start_matches("origin/").to_string())
        .filter(|line| line != "HEAD" && line != main && !line.is_empty())
        .filter(|line| !ticket.as_ref().is_some_and(|regex| regex.is_match(line)))
        .collect();
    branches.sort();
    branches.dedup();
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for branch in &branches {
        if let Some((first, _)) = branch.split_once('/') {
            *counts.entry(format!("{first}/")).or_insert(0) += 1;
        }
    }
    let total = branches.len() as u32;
    let mut best: Option<(u32, String)> = None;
    for (prefix, count) in counts {
        if count * 3 >= total && best.as_ref().map(|(best_count, _)| count > *best_count).unwrap_or(true) {
            best = Some((count, prefix));
        }
    }
    best.map(|(_, prefix)| prefix).unwrap_or_default()
}

fn worktree_dir(root: &Path, name: &str) -> String {
    let text = crate::config::git(&["worktree", "list", "--porcelain"], root).unwrap_or_default();
    let mut counts: BTreeMap<String, u32> = BTreeMap::new();
    for path in text
        .lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .skip(1)
        .filter(|path| !path.contains("/.claude/worktrees/"))
    {
        if let Some(parent) = Path::new(path).parent() {
            *counts.entry(parent.to_string_lossy().into_owned()).or_insert(0) += 1;
        }
    }
    let mut best: Option<(u32, String)> = None;
    for (dir, count) in counts {
        if best.as_ref().map(|(best_count, _)| count > *best_count).unwrap_or(true) {
            best = Some((count, dir));
        }
    }
    match best {
        Some((_, dir)) => relative(Path::new(&dir), root),
        None => format!("../{name}-wt"),
    }
}

fn rel_repo(path: &Path, base: &Path) -> String {
    let relative = relative(path, base);
    if relative == "." {
        String::new()
    } else {
        relative
    }
}

fn relative(path: &Path, base: &Path) -> String {
    let path: Vec<Component> = path.components().collect();
    let base: Vec<Component> = base.components().collect();
    let common = path
        .iter()
        .zip(base.iter())
        .take_while(|(a, b)| a == b)
        .count();
    let mut parts: Vec<String> = Vec::new();
    for _ in common..base.len() {
        parts.push("..".into());
    }
    for component in &path[common..] {
        parts.push(component.as_os_str().to_string_lossy().into_owned());
    }
    if parts.is_empty() {
        ".".into()
    } else {
        parts.join("/")
    }
}

fn registry_path() -> PathBuf {
    if let Ok(value) = std::env::var("RAMIFY_REGISTRY") {
        return PathBuf::from(value);
    }
    let base = std::env::var("CLAUDE_CONFIG_DIR")
        .unwrap_or_else(|_| format!("{}/.claude", std::env::var("HOME").unwrap_or_default()));
    PathBuf::from(base).join("ramify/projects")
}

fn held_ranges(root: &Path, registry: &Path, span: u32) -> Vec<(u32, u32)> {
    let self_path = root.canonicalize().unwrap_or_else(|_| root.to_path_buf());
    let mut held = Vec::new();
    let Ok(text) = std::fs::read_to_string(registry) else {
        return held;
    };
    for project in text.lines().filter(|line| !line.is_empty()) {
        let path = Path::new(project);
        let canonical = path.canonicalize().unwrap_or_else(|_| path.to_path_buf());
        if canonical == self_path {
            continue;
        }
        let mut config = None;
        for name in [".ramify.conf", ".claude/ramify.conf"] {
            let candidate = path.join(name);
            if candidate.is_file() {
                config = Some(candidate);
                break;
            }
        }
        let Some(config) = config else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(config) else {
            continue;
        };
        let values = parse_config(&text);
        let max: u32 = values
            .get("RAMIFY_MAX_SLOT")
            .and_then(|value| value.parse().ok())
            .unwrap_or(5);
        for (key, value) in &values {
            if let Some(port) = value.parse::<u32>().ok().filter(|_| key.ends_with("_shared_port")) {
                held.push((port, port));
            } else if key.ends_with("_port") && !key.ends_with("_shared_port") {
                if let Some(port) = value.parse::<u32>().ok() {
                    held.push((port, port + max));
                }
            }
        }
    }
    let _ = span;
    held
}

fn parse_config(text: &str) -> HashMap<String, String> {
    let mut values = HashMap::new();
    for line in text.lines() {
        let line = line.trim();
        let line = line.strip_prefix("export ").unwrap_or(line);
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        if !key.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') || key.is_empty() {
            continue;
        }
        values.insert(key.to_string(), value.trim().trim_matches(['\'', '"']).to_string());
    }
    values
}

fn pick_port(held: &mut Vec<(u32, u32)>, base: u32, span: u32) -> u32 {
    let mut port = base;
    loop {
        let overlaps = held
            .iter()
            .any(|(low, high)| port <= *high && port + span >= *low);
        if !overlaps {
            break;
        }
        port += 10;
    }
    held.push((port, port + span));
    port
}

fn copy_paths(root: &Path, pkgdirs: &[String]) -> String {
    let mut copy: Vec<String> = Vec::new();
    let keep = |path: &str, copy: &mut Vec<String>| {
        if copy.iter().any(|item| item == path) {
            return;
        }
        let ignored = Command::new("git")
            .args(["-C", &root.to_string_lossy(), "check-ignore", "-q", path])
            .status()
            .map(|status| status.success())
            .unwrap_or(false);
        if ignored {
            copy.push(path.to_string());
        }
    };
    for path in [".claude", ".envrc", ".vscode/settings.json"] {
        if root.join(path).exists() {
            keep(path, &mut copy);
        }
    }
    let mut dirs = vec![".".to_string()];
    dirs.extend(pkgdirs.iter().cloned());
    for dir in dirs {
        let prefix = if dir == "." { String::new() } else { format!("{dir}/") };
        if let Ok(entries) = std::fs::read_dir(root.join(&prefix)) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if !name.starts_with(".env") {
                    continue;
                }
                if name.ends_with(".example") || name.ends_with(".sample") || name.ends_with(".template") {
                    continue;
                }
                keep(&format!("{prefix}{name}"), &mut copy);
            }
        }
        if root.join(format!("{prefix}.env.example")).is_file() {
            keep(&format!("{prefix}.env"), &mut copy);
            keep(&format!("{prefix}.env.local"), &mut copy);
        }
    }
    copy.join(" ")
}

fn compose_block(root: &Path) -> (String, String, String) {
    let mut patterns: Vec<String> = Vec::new();
    for pattern in [
        "docker-compose.y*ml",
        "compose.y*ml",
        "*/docker-compose.y*ml",
        "*/compose.y*ml",
        "*/*/docker-compose.y*ml",
        "*/*/compose.y*ml",
    ] {
        for entry in glob(root, pattern) {
            let relative = entry.strip_prefix(root).unwrap_or(&entry).to_string_lossy().into_owned();
            if relative.contains("node_modules") {
                continue;
            }
            patterns.push(relative);
        }
    }
    patterns.sort();
    let Some(compose) = patterns.first().cloned() else {
        return ("RAMIFY_COMPOSE_FILE=''\nRAMIFY_CONTAINERS=\"\"".into(), String::new(), String::new());
    };
    let text = std::fs::read_to_string(root.join(&compose)).unwrap_or_default();
    let regex = regex::Regex::new(r#"^\s*container_name:\s*["']?([^"'\s]*)"#).unwrap();
    let mut containers: Vec<String> = Vec::new();
    for line in text.lines() {
        if let Some(captures) = regex.captures(line) {
            containers.push(captures[1].to_string());
        }
    }
    let containers = containers.join(" ");
    if containers.is_empty() {
        (
            format!(
                "# {compose} names no container_name. Give each service one and list them here:\n# RAMIFY_COMPOSE_FILE='{compose}'\n# RAMIFY_CONTAINERS=\"\""
            ),
            compose,
            containers,
        )
    } else {
        (
            format!("RAMIFY_COMPOSE_FILE='{compose}'\nRAMIFY_CONTAINERS=\"{containers}\""),
            compose,
            containers,
        )
    }
}

fn glob(root: &Path, pattern: &str) -> Vec<PathBuf> {
    let single = !pattern.contains('*');
    if single {
        let path = root.join(pattern);
        return if path.is_file() { vec![path] } else { Vec::new() };
    }
    // Only the shapes setup.sh uses: `name`, `*/name` and `*/*/name` with a `y*ml` suffix.
    let parts: Vec<&str> = pattern.split('/').collect();
    let (dirs, file) = parts.split_at(parts.len() - 1);
    let file = file[0];
    let mut bases = vec![root.to_path_buf()];
    for part in dirs {
        let mut next = Vec::new();
        for base in bases {
            if *part == "*" {
                if let Ok(entries) = std::fs::read_dir(&base) {
                    for entry in entries.flatten() {
                        if entry.path().is_dir() {
                            next.push(entry.path());
                        }
                    }
                }
            } else {
                next.push(base.join(*part));
            }
        }
        bases = next;
    }
    let mut found = Vec::new();
    for base in bases {
        if let Ok(entries) = std::fs::read_dir(&base) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if matches_glob(&name, file) && entry.path().is_file() {
                    found.push(entry.path());
                }
            }
        }
    }
    found
}

fn matches_glob(name: &str, pattern: &str) -> bool {
    let regex = regex::Regex::new(&format!(
        "^{}$",
        regex::escape(pattern).replace("\\*", ".*").replace("y.*ml", "y.*ml")
    ));
    regex.map(|regex| regex.is_match(name)).unwrap_or(false)
}

fn detect_packages(root: &Path) -> Vec<String> {
    let root = root.to_path_buf();
    let top = subdirs(&root);
    let mut packages: Vec<PathBuf> = vec![root.clone()];
    packages.extend(top.iter().cloned());
    for dir in &top {
        packages.extend(subdirs(dir));
    }
    packages.retain(|dir| dir.join("package.json").is_file());
    packages.sort_by(|a, b| {
        let ra = relative(a, &root);
        let rb = relative(b, &root);
        let key = |r: &str| (r.matches('/').count() + usize::from(!r.is_empty()), r.to_string());
        key(&ra).cmp(&key(&rb))
    });
    let mut out = Vec::new();
    let mut roots: Vec<PathBuf> = Vec::new();
    let mut bins: HashMap<PathBuf, String> = HashMap::new();
    let mut members: HashMap<PathBuf, Vec<HashMap<String, Value>>> = HashMap::new();
    let mut names: HashSet<String> = HashSet::new();
    let mut ui_named = false;
    let mut service_dirs: Vec<String> = Vec::new();
    for dir in &packages {
        let rel = rel_repo(dir, &root);
        let package = load_package(dir);
        let Some(package) = package else {
            continue;
        };
        let scripts = table(&package, "scripts");
        let deps = dependencies(&package);
        let install_root = install_root(dir, &root);
        let manager = manager_for(&install_root);
        members.entry(install_root.clone()).or_default().push(deps.clone());
        out.push(format!("pkg{SEP}{rel}"));
        if !roots.contains(&install_root) {
            roots.push(install_root.clone());
        }
        let mobile = deps.contains_key("react-native")
            || deps.contains_key("expo")
            || (dir.join("ios").is_dir() && dir.join("android").is_dir());
        if mobile {
            let runs: Vec<String> = ["ios", "android"]
                .iter()
                .filter(|script| scripts.contains_key(**script))
                .map(|script| run(&manager, script, ""))
                .collect();
            let checks: Vec<String> = ["test", "lint"]
                .iter()
                .filter(|script| scripts.contains_key(**script))
                .map(|script| run(&manager, script, ""))
                .collect();
            let mut notes = format!("Mobile app in {}, no QA stack.", if rel.is_empty() { "the repo root" } else { &rel });
            if !runs.is_empty() {
                notes.push_str(&format!(" Run it: {}.", runs.join(", ")));
            }
            if !checks.is_empty() {
                notes.push_str(&format!(" Check it: {}.", checks.join(", ")));
            }
            out.push(format!("mobile{SEP}{rel}{SEP}{notes}"));
            continue;
        }
        if deps.contains_key("@nestjs/core") {
            let main_ts = std::fs::read_to_string(dir.join("src/main.ts")).unwrap_or_default();
            if regex::Regex::new(r"\.listen\(\s*\d").map(|r| r.is_match(&main_ts)).unwrap_or(false) {
                out.push(format!("fixed{SEP}{rel}"));
                continue;
            }
        }
        let Some(script) = ["dev", "start:dev", "start", "serve"]
            .iter()
            .find(|script| scripts.contains_key(**script))
            .copied()
        else {
            continue;
        };
        let command = scripts.get(script).cloned().unwrap_or_default();
        let orchestrator = regex::Regex::new(r"\b(turbo|nx|lerna|concurrently)\b|--workspaces?\b|--filter\b|(^|\s)-r(\s|$)")
            .map(|regex| regex.is_match(&command))
            .unwrap_or(false);
        if orchestrator {
            out.push(format!("orchestrator{SEP}{rel}{SEP}{script}"));
            continue;
        }
        let word = |name: &str| {
            regex::Regex::new(&format!(r"(^|[\s;&|(]){}(\s|$)", regex::escape(name)))
                .map(|regex| regex.is_match(&command))
                .unwrap_or(false)
        };
        let (kind, cmd, base, binary) = if word("next") {
            ("ui", run(&manager, script, "-p $PORT"), 3000, Some("next"))
        } else if word("vite") || word("astro") {
            let binary = if word("vite") { "vite" } else { "astro" };
            ("ui", run(&manager, script, "--port $PORT"), 5173, Some(binary))
        } else if word("webpack serve") || word("webpack-dev-server") {
            let binary = if word("webpack-dev-server") { "webpack-dev-server" } else { "webpack" };
            ("ui", run(&manager, script, "--port $PORT"), 8080, Some(binary))
        } else if word("react-scripts start") {
            ("ui", format!("BROWSER=none PORT=$PORT {}", run(&manager, script, "")), 3000, Some("react-scripts"))
        } else {
            ("other", format!("PORT=$PORT {}", run(&manager, script, "")), 3000, None)
        };
        if let Some(binary) = binary {
            bins.entry(install_root.clone()).or_insert_with(|| binary.to_string());
        }
        let mut name = if kind == "ui" && !ui_named {
            ui_named = true;
            "web".to_string()
        } else {
            let sanitized: String = (if rel.is_empty() { "app".to_string() } else { rel.to_lowercase() })
                .chars()
                .map(|c| if c.is_ascii_lowercase() || c.is_ascii_digit() { c } else { '_' })
                .collect();
            let sanitized = if sanitized.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
                format!("_{sanitized}")
            } else {
                sanitized
            };
            if names.contains(&sanitized) || sanitized == "web" {
                format!("{}_{}", sanitized, script.replace(':', "_"))
            } else {
                sanitized
            }
        };
        if name.is_empty() {
            name = "app".into();
        }
        names.insert(name.clone());
        out.push(format!("svc{SEP}{name}{SEP}{rel}{SEP}{kind}{SEP}{base}{SEP}{cmd}"));
        service_dirs.push(rel);
    }
    for root_dir in roots {
        let rel = rel_repo(&root_dir, &root);
        let has_members = members.get(&root_dir).map(|list| list.iter().any(|deps| !deps.is_empty())).unwrap_or(false);
        if !is_root(&root_dir) && !has_members {
            continue;
        }
        let manager = manager_for(&root_dir);
        let (cmd, ready): (String, String) = match manager.as_str() {
            "bun" => {
                let first = members
                    .get(&root_dir)
                    .map(|list| {
                        let mut keys: Vec<String> = list.iter().flat_map(|deps| deps.keys().cloned()).collect();
                        keys.sort();
                        keys.first().cloned()
                    })
                    .flatten();
                let ready = if let Some(binary) = bins.get(&root_dir) {
                    format!("node_modules/.bin/{binary}")
                } else {
                    first.map(|first| format!("node_modules/{first}/package.json")).unwrap_or_default()
                };
                ("bun install".into(), ready)
            }
            "pnpm" => ("pnpm install".into(), "node_modules/.modules.yaml".into()),
            "yarn" => ("yarn install --frozen-lockfile".into(), "node_modules/.yarn-integrity".into()),
            _ => (
                if root_dir.join("package-lock.json").is_file() {
                    "npm ci".into()
                } else {
                    "npm install".into()
                },
                "node_modules/.package-lock.json".into(),
            ),
        };
        out.push(format!("install{SEP}{rel}{SEP}{cmd}{SEP}{ready}"));
    }
    let mut dirs = vec![root.clone()];
    dirs.extend(top);
    for dir in dirs {
        if dir.join("uv.lock").is_file() {
            out.push(format!("install{SEP}{}{SEP}uv sync{SEP}.venv/pyvenv.cfg", rel_repo(&dir, &root)));
        }
    }
    out
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut dirs: Vec<PathBuf> = entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| {
            path.is_dir()
                && path
                    .file_name()
                    .map(|name| !name.to_string_lossy().starts_with('.') && name.to_string_lossy() != "node_modules")
                    .unwrap_or(false)
        })
        .collect();
    dirs.sort();
    dirs
}

fn load_package(dir: &Path) -> Option<Value> {
    let text = std::fs::read_to_string(dir.join("package.json")).ok()?;
    serde_json::from_str(&text).ok()
}

fn table(package: &Value, key: &str) -> HashMap<String, String> {
    package
        .get(key)
        .and_then(Value::as_object)
        .map(|map| {
            map.iter()
                .filter_map(|(key, value)| value.as_str().map(|value| (key.clone(), value.to_string())))
                .collect()
        })
        .unwrap_or_default()
}

fn dependencies(package: &Value) -> HashMap<String, Value> {
    let mut deps = HashMap::new();
    for key in ["dependencies", "devDependencies"] {
        if let Some(map) = package.get(key).and_then(Value::as_object) {
            deps.extend(map.iter().map(|(key, value)| (key.clone(), value.clone())));
        }
    }
    deps
}

const LOCKS: [(&str, &str); 5] = [
    ("bun.lock", "bun"),
    ("bun.lockb", "bun"),
    ("pnpm-lock.yaml", "pnpm"),
    ("yarn.lock", "yarn"),
    ("package-lock.json", "npm"),
];

fn is_root(dir: &Path) -> bool {
    let package = load_package(dir).unwrap_or(Value::Null);
    LOCKS.iter().any(|(lock, _)| dir.join(lock).is_file())
        || dir.join("pnpm-workspace.yaml").is_file()
        || package.get("packageManager").is_some()
        || package.get("workspaces").is_some()
}

fn install_root(dir: &Path, root: &Path) -> PathBuf {
    let mut current = dir.to_path_buf();
    loop {
        if is_root(&current) {
            return current;
        }
        if current == root {
            return dir.to_path_buf();
        }
        let Some(parent) = current.parent() else {
            return dir.to_path_buf();
        };
        current = parent.to_path_buf();
    }
}

fn manager_for(root: &Path) -> String {
    let package = load_package(root).unwrap_or(Value::Null);
    let manager = package
        .get("packageManager")
        .and_then(Value::as_str)
        .unwrap_or("")
        .split('@')
        .next()
        .unwrap_or("");
    if matches!(manager, "bun" | "pnpm" | "yarn" | "npm") {
        return manager.to_string();
    }
    for (lock, manager) in LOCKS {
        if root.join(lock).is_file() {
            return manager.to_string();
        }
    }
    if root.join("pnpm-workspace.yaml").is_file() {
        "pnpm".into()
    } else {
        "npm".into()
    }
}

fn run(manager: &str, script: &str, args: &str) -> String {
    if args.is_empty() {
        return format!("{manager} run {script}");
    }
    if manager == "npm" {
        format!("npm run {script} -- {args}")
    } else {
        format!("{manager} run {script} {args}")
    }
}

#[allow(dead_code)]
fn unused(_: BTreeSet<String>) {}
