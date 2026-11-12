use crate::config::{git, Config};
use crate::dash;
use std::collections::{BTreeSet, HashMap};
use std::path::{Path, PathBuf};
use std::process::Command;

const DEV_RE: &str = "vite|next dev|next-server|webpack serve|webpack-dev-server|vue-cli-service serve|react-scripts start|nest start|dotnet watch|uvicorn|nodemon|tsx watch|bun run dev|storybook dev|ng serve|expo start|react-native start";
const RUNNER_RE: &str = "(^|[/ ])(jest|vitest|mocha|playwright)( |$)|jest-worker|node --test";

pub fn mem(args: &[String]) -> i32 {
    let apply = match args.first().map(String::as_str) {
        None => false,
        Some("--apply") => true,
        Some(_) => {
            eprintln!("usage: ramify mem [--apply]");
            return 2;
        }
    };
    print!("{}", machine_line());
    let footprints = footprints();
    let top_ok = Command::new("top")
        .args(["-l", "1", "-n", "0"])
        .output()
        .map(|output| output.status.success())
        .unwrap_or(false);
    if !top_ok {
        println!("footprints are RSS: without macOS top, compressed and swapped pages are not counted");
        println!();
    }

    let mut rows = Vec::new();
    for config in service_projects() {
        rows.extend(mem_project(&config, apply, "apply", &footprints));
    }
    println!("stacks");
    if rows.is_empty() {
        println!("  none running");
    }
    let mut reclaim = false;
    for row in &rows {
        let note = if row.note.is_empty() {
            String::new()
        } else {
            format!("  → {}", row.note)
        };
        println!(
            "  {:<16} {:<32} {:>6} MB  up {:<7} {}{note}",
            row.project, row.worktree, row.memory, row.uptime, row.verdict
        );
        if !matches!(row.verdict.as_str(), "ok" | "pr-open" | "unreadable") && row.note.is_empty() {
            reclaim = true;
        }
    }
    println!();

    let outside = outside_ramify(&footprints);
    if !outside.is_empty() {
        println!("outside ramify (ramify never stops these)");
        for row in &outside {
            println!(
                "  {:<7} {:<6} {:>6} MB  up {:<7} {}",
                row.pid,
                row.kind,
                row.memory,
                dur(row.age),
                cwd_of(row.pid)
            );
            println!("          {}", row.command);
            println!("          kill {}", row.pid);
        }
        println!();
        reclaim = true;
    }
    build_servers_line();
    if !reclaim {
        println!("nothing to reclaim");
    } else if !apply {
        println!("ramify mem --apply stops the zombie, merged and orphan stacks");
    }
    0
}

struct Row {
    project: String,
    worktree: String,
    verdict: String,
    memory: u64,
    uptime: String,
    note: String,
}

fn machine_line() -> String {
    let ram = Command::new("sysctl")
        .args(["-n", "hw.memsize"])
        .output()
        .ok()
        .and_then(|output| String::from_utf8_lossy(&output.stdout).trim().parse::<u64>().ok())
        .map(|bytes| format!("{} GB", bytes / 1_073_741_824))
        .unwrap_or_else(|| "?".into());
    let swap = Command::new("sysctl")
        .args(["-n", "vm.swapusage"])
        .output()
        .ok()
        .map(|output| {
            let text = String::from_utf8_lossy(&output.stdout).into_owned();
            let fields: Vec<&str> = text.split_whitespace().collect();
            format!(
                "{} used of {}",
                fields.get(5).copied().unwrap_or("?"),
                fields.get(2).copied().unwrap_or("?")
            )
        })
        .unwrap_or_else(|| "?".into());
    let free = Command::new("memory_pressure")
        .output()
        .ok()
        .and_then(|output| {
            String::from_utf8_lossy(&output.stdout).lines().find_map(|line| {
                line.split_once("free percentage: ")
                    .map(|(_, rest)| rest.trim().to_string())
            })
        })
        .unwrap_or_else(|| "?".into());
    format!("machine        {ram} RAM   swap {swap}   {free} free\n\n")
}

fn footprints() -> HashMap<u64, f64> {
    let mut map = HashMap::new();
    if let Ok(output) = Command::new("top").args(["-l", "1", "-stats", "pid,mem"]).output() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split_whitespace();
            let Some(pid) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
                continue;
            };
            let Some(value) = parts.next() else {
                continue;
            };
            if !value.chars().next().is_some_and(|c| c.is_ascii_digit()) {
                continue;
            }
            let value = value.trim_end_matches(['+', '-']);
            let unit = value.chars().last().unwrap_or('M');
            let number: String = value
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == '.')
                .collect();
            let Ok(number) = number.parse::<f64>() else {
                continue;
            };
            let factor = match unit {
                'G' => 1024.0,
                'T' => 1_048_576.0,
                'K' => 1.0 / 1024.0,
                'B' => 1.0 / 1_048_576.0,
                _ => 1.0,
            };
            map.insert(pid, number * factor);
        }
    }
    if !map.is_empty() {
        return map;
    }
    if let Ok(output) = Command::new("ps").args(["-axo", "pid=,rss="]).output() {
        for line in String::from_utf8_lossy(&output.stdout).lines() {
            let mut parts = line.split_whitespace();
            let Some(pid) = parts.next().and_then(|value| value.parse::<u64>().ok()) else {
                continue;
            };
            let Some(rss) = parts.next().and_then(|value| value.parse::<f64>().ok()) else {
                continue;
            };
            map.insert(pid, rss / 1024.0);
        }
    }
    map
}

struct Process {
    pid: u64,
    pgid: u64,
    command: String,
}

fn process_rows() -> Vec<Process> {
    let Ok(output) = Command::new("ps")
        .args(["-axww", "-o", "pid=,pgid=,command="])
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&output.stdout)
        .lines()
        .filter_map(|line| {
            let row = split_fields(line, 2)?;
            Some(Process {
                pid: row[0].parse().ok()?,
                pgid: row[1].parse().ok()?,
                command: row[2].clone(),
            })
        })
        .collect()
}

/// Split a `ps` line into `count` fields, leaving the rest of the line as the last field.
fn split_fields(line: &str, count: usize) -> Option<Vec<String>> {
    let line = line.trim_start();
    let bytes = line.as_bytes();
    let mut index = 0;
    let mut fields = Vec::new();
    while fields.len() < count {
        while index < bytes.len() && bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        let start = index;
        while index < bytes.len() && !bytes[index].is_ascii_whitespace() {
            index += 1;
        }
        if start == index {
            return None;
        }
        fields.push(line[start..index].to_string());
    }
    fields.push(line.get(index..).unwrap_or("").trim_start().to_string());
    Some(fields)
}

fn is_build_server(command: &str) -> bool {
    (command.contains("MSBuild.dll")
        && (command.contains("nodeReuse:true") || command.contains("nodemode:8")))
        || command.contains("VBCSCompiler.dll")
        || (command.contains("rzc.dll") && command.contains(" server"))
}

fn group_members(group: u64, processes: &[Process]) -> Vec<u64> {
    processes
        .iter()
        .filter(|process| process.pgid == group && !is_build_server(&process.command))
        .map(|process| process.pid)
        .collect()
}

fn group_mb(group: u64, processes: &[Process], footprints: &HashMap<u64, f64>) -> u64 {
    let total: f64 = processes
        .iter()
        .filter(|process| process.pgid == group && !is_build_server(&process.command))
        .map(|process| footprints.get(&process.pid).copied().unwrap_or(0.0))
        .sum();
    total as u64
}

fn env_get(path: &Path, key: &str) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix(&format!("{key}=")))
        .next_back()
        .map(str::to_string)
}

fn listening(port: u32) -> bool {
    dash::listening(port)
}

fn dur(seconds: u64) -> String {
    if seconds < 3600 {
        format!("{}m", seconds / 60)
    } else {
        format!("{}h{:02}m", seconds / 3600, (seconds % 3600) / 60)
    }
}

fn age_of(path: &Path) -> u64 {
    std::fs::metadata(path)
        .and_then(|meta| meta.modified())
        .ok()
        .and_then(|modified| std::time::SystemTime::now().duration_since(modified).ok())
        .map(|duration| duration.as_secs())
        .unwrap_or(0)
}

fn zombie_after() -> u64 {
    std::env::var("RAMIFY_ZOMBIE_AFTER")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(3600)
}

fn worktree_root(primary: &Path, worktree: &str) -> Option<PathBuf> {
    let text = git(&["worktree", "list", "--porcelain"], primary).ok()?;
    text.lines()
        .filter_map(|line| line.strip_prefix("worktree "))
        .map(PathBuf::from)
        .find(|path| {
            path.file_name()
                .map(|name| name.to_string_lossy() == worktree)
                .unwrap_or(false)
        })
}

fn branch_of(path: &Path) -> String {
    git(&["rev-parse", "--abbrev-ref", "HEAD"], path)
        .map(|text| text.trim().to_string())
        .unwrap_or_else(|_| "?".into())
}

pub(crate) fn merged_reason(config: &Config, branch: &str) -> Option<String> {
    let main = format!("{}/{}", config.remote, config.main);
    let track = Command::new("git")
        .args([
            "-C",
            &config.primary.to_string_lossy(),
            "for-each-ref",
            "--format=%(upstream:track)",
            &format!("refs/heads/{branch}"),
        ])
        .output()
        .ok()
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
        .unwrap_or_default();
    let ancestor = Command::new("git")
        .args(["-C", &config.primary.to_string_lossy(), "merge-base", "--is-ancestor", branch, &main])
        .status()
        .map(|status| status.success())
        .unwrap_or(false);
    if ancestor && track == "[gone]" {
        return Some("ancestor of main, remote gone".into());
    }
    if !ancestor {
        let tree = Command::new("git")
            .args(["-C", &config.primary.to_string_lossy(), "merge-tree", "--write-tree", &main, branch])
            .output()
            .ok()
            .map(|output| {
                String::from_utf8_lossy(&output.stdout)
                    .lines()
                    .next()
                    .unwrap_or("")
                    .to_string()
            })
            .unwrap_or_default();
        let main_tree = git(&["rev-parse", &format!("{main}^{{tree}}")], &config.primary).unwrap_or_default();
        if tree == main_tree.trim() {
            return Some("content already in main".into());
        }
    }
    let escaped = regex::escape(branch);
    let mut merge: Option<u64> = None;
    for pattern in [
        format!("^Merged in {escaped} \\(pull request"),
        format!("^Merge pull request #[0-9]+ from [^ ]+/{escaped}$"),
    ] {
        if let Ok(text) = git(
            &["log", &config.main, "-1", "--format=%ct", "-E", &format!("--grep={pattern}")],
            &config.primary,
        ) {
            if let Ok(seconds) = text.trim().parse::<u64>() {
                merge = Some(merge.map_or(seconds, |old| old.max(seconds)));
            }
        }
    }
    let merge = merge?;
    let last = git(&["log", "-1", "--format=%ct", branch], &config.primary)
        .ok()
        .and_then(|text| text.trim().parse::<u64>().ok())?;
    (last <= merge).then(|| "PR merged, nothing newer on it".into())
}

fn at_finale(config: &Config, root: &Path) -> bool {
    let branch = branch_of(root);
    crate::workflow::report(config, root, &branch)
        .and_then(|progress| progress.next.map(|step| step.step == "finale"))
        .unwrap_or(false)
}

struct Outside {
    pid: u64,
    kind: String,
    memory: u64,
    age: u64,
    command: String,
}

fn outside_ramify(footprints: &HashMap<u64, f64>) -> Vec<Outside> {
    let tagged = tagged_groups();
    let Ok(output) = Command::new("ps")
        .args(["-axww", "-o", "pid=,ppid=,pgid=,etime=,command="])
        .output()
    else {
        return Vec::new();
    };
    let dev = regex::Regex::new(DEV_RE).ok();
    let runner = regex::Regex::new(RUNNER_RE).ok();
    let mut rows = Vec::new();
    let mut entries = Vec::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(row) = split_fields(line, 4) else {
            continue;
        };
        let (Ok(pid), Ok(ppid), Ok(pgid)) = (
            row[0].parse::<u64>(),
            row[1].parse::<u64>(),
            row[2].parse::<u64>(),
        ) else {
            continue;
        };
        let age = etime(&row[3]);
        let command = row[4].clone();
        let kind = if is_build_server(&command) || tagged.contains(&pgid) {
            String::new()
        } else if runner.as_ref().is_some_and(|regex| regex.is_match(&command)) {
            "runner".into()
        } else if dev.as_ref().is_some_and(|regex| regex.is_match(&command)) {
            "dev".into()
        } else {
            String::new()
        };
        entries.push((pid, ppid, pgid, command, age, kind));
    }
    let after: u64 = std::env::var("RAMIFY_RUNNER_AFTER")
        .ok()
        .and_then(|value| value.parse().ok())
        .unwrap_or(1800);
    let kinds: HashMap<u64, String> = entries
        .iter()
        .map(|entry| (entry.0, entry.5.clone()))
        .collect();
    let mut group_memory: HashMap<u64, f64> = HashMap::new();
    for entry in &entries {
        *group_memory.entry(entry.2).or_insert(0.0) +=
            footprints.get(&entry.0).copied().unwrap_or(0.0);
    }
    for (pid, ppid, pgid, command, age, kind) in entries {
        if kind.is_empty() || kinds.get(&ppid).map(|parent| !parent.is_empty()).unwrap_or(false) {
            continue;
        }
        if ppid != 1 && !(kind == "runner" && age >= after) {
            continue;
        }
        let memory = group_memory.get(&pgid).copied().unwrap_or(0.0);
        rows.push(Outside {
            pid,
            kind,
            memory: memory as u64,
            age,
            command: command.chars().take(90).collect(),
        });
    }
    rows
}

fn tagged_groups() -> BTreeSet<u64> {
    let Ok(output) = Command::new("ps")
        .args(["-axwwE", "-o", "pgid=,command="])
        .output()
    else {
        return BTreeSet::new();
    };
    let regex = regex::Regex::new(r"RAMIFY_TAG=[^ ]+@[0-9a-f]+/").ok();
    let Some(regex) = regex else {
        return BTreeSet::new();
    };
    let mut groups = BTreeSet::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        if !regex.is_match(line) {
            continue;
        }
        if let Some(pgid) = line.split_whitespace().next().and_then(|v| v.parse().ok()) {
            groups.insert(pgid);
        }
    }
    groups
}

fn etime(text: &str) -> u64 {
    let (days, rest) = match text.split_once('-') {
        Some((days, rest)) => (days.parse::<u64>().unwrap_or(0), rest),
        None => (0, text),
    };
    let mut seconds = 0u64;
    for part in rest.split(':') {
        seconds = seconds * 60 + part.parse::<u64>().unwrap_or(0);
    }
    days * 86_400 + seconds
}

fn cwd_of(pid: u64) -> String {
    Command::new("lsof")
        .args(["-a", "-p", &pid.to_string(), "-d", "cwd", "-Fn"])
        .output()
        .ok()
        .map(|output| {
            String::from_utf8_lossy(&output.stdout)
                .lines()
                .find_map(|line| line.strip_prefix('n').map(str::to_string))
                .unwrap_or_default()
        })
        .map(|cwd| {
            let home = std::env::var("HOME").unwrap_or_default();
            if !home.is_empty() {
                cwd.replacen(&home, "~", 1)
            } else {
                cwd
            }
        })
        .filter(|cwd| !cwd.is_empty())
        .unwrap_or_else(|| "?".into())
}

pub(crate) fn build_servers_line() {
    let Ok(output) = Command::new("ps")
        .args(["-axww", "-o", "rss=,command="])
        .output()
    else {
        println!("build servers  none");
        println!();
        return;
    };
    let mut kinds: std::collections::BTreeMap<&str, (u64, u64)> = std::collections::BTreeMap::new();
    for line in String::from_utf8_lossy(&output.stdout).lines() {
        let Some(row) = split_fields(line, 1) else {
            continue;
        };
        let Ok(rss) = row[0].parse::<u64>() else {
            continue;
        };
        let command = &row[1];
        let kind = if command.contains("MSBuild.dll") && command.contains("nodeReuse:true") {
            "msbuild"
        } else if command.contains("MSBuild.dll") && command.contains("nodemode:8") {
            "msbuild-server"
        } else if command.contains("VBCSCompiler.dll") {
            "roslyn"
        } else if command.contains("rzc.dll") && command.contains(" server") {
            "razor"
        } else {
            continue;
        };
        let entry = kinds.entry(kind).or_insert((0, 0));
        entry.0 += 1;
        entry.1 += rss;
    }
    if kinds.is_empty() {
        println!("build servers  none");
        println!();
        return;
    }
    let mut line = String::new();
    for (kind, (count, rss)) in kinds {
        line.push_str(&format!("{kind} ×{count} {} MB   ", rss / 1024));
    }
    println!("build servers  {line}(dotnet build-server shutdown stops them)");
    println!();
}

fn service_projects() -> Vec<Config> {
    let mut projects = Vec::new();
    for root in dash::projects() {
        if let Ok(config) = Config::for_root(&root) {
            if !config.services.is_empty() {
                projects.push(config);
            }
        }
    }
    projects
}

fn mem_project(
    config: &Config,
    apply: bool,
    mode: &str,
    footprints: &HashMap<u64, f64>,
) -> Vec<Row> {
    let mut rows = Vec::new();
    let worktrees = match git(&["worktree", "list", "--porcelain"], &config.primary) {
        Ok(text) => text
            .lines()
            .filter_map(|line| line.strip_prefix("worktree "))
            .map(str::to_string)
            .collect::<Vec<_>>(),
        Err(_) => Vec::new(),
    };
    if worktrees.is_empty() {
        rows.push(Row {
            project: config.project.clone(),
            worktree: "-".into(),
            verdict: "unreadable".into(),
            memory: 0,
            uptime: "-".into(),
            note: format!(
                "cannot read the worktree list of {}, its stacks left alone",
                config.primary.display()
            ),
        });
        return rows;
    }
    let processes = process_rows();
    let tagged = dash::tagged_groups(config);
    let mut envs: Vec<PathBuf> = std::fs::read_dir(&config.state_dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|entry| entry.path())
                .filter(|path| path.extension().map(|ext| ext == "env").unwrap_or(false))
                .collect()
        })
        .unwrap_or_default();
    envs.sort();
    for env in envs {
        let Some(worktree) = env.file_stem().map(|name| name.to_string_lossy().into_owned()) else {
            continue;
        };
        let mut groups = BTreeSet::new();
        if let Ok(entries) = std::fs::read_dir(&config.state_dir) {
            for entry in entries.flatten() {
                let name = entry.file_name().to_string_lossy().into_owned();
                if name.starts_with(&format!("{worktree}.")) && name.ends_with(".pid") {
                    if let Ok(text) = std::fs::read_to_string(entry.path()) {
                        if let Ok(group) = text.trim().parse::<u64>() {
                            groups.insert(group);
                        }
                    }
                }
            }
        }
        for (group, tagged_worktree, _) in &tagged {
            if tagged_worktree == &worktree {
                groups.insert(*group);
            }
        }
        let mut memory = 0;
        let mut alive = false;
        let mut live_groups = Vec::new();
        for group in &groups {
            if *group <= 1 || group_members(*group, &processes).is_empty() {
                continue;
            }
            alive = true;
            live_groups.push(*group);
            memory += group_mb(*group, &processes, footprints);
        }
        if !alive {
            continue;
        }
        let mut answers = false;
        for service in &config.services {
            let upper = service.to_uppercase();
            if env_get(&env, &format!("{upper}_SHARED")).as_deref() == Some("1") {
                continue;
            }
            if let Some(port) = env_get(&env, &format!("{upper}_PORT")).and_then(|p| p.parse().ok()) {
                if listening(port) {
                    answers = true;
                }
            }
        }
        let root = worktree_root(&config.primary, &worktree);
        let age = age_of(&env);
        if mode == "reap" && root.is_some() {
            continue;
        }
        let verdict = if root.is_none() {
            "orphan"
        } else if !answers && age >= zombie_after() {
            "zombie"
        } else if merged_reason(config, &branch_of(&config.repo_root)).is_some() {
            "merged"
        } else if at_finale(config, root.as_deref().unwrap_or(&config.repo_root)) {
            "pr-open"
        } else {
            "ok"
        };
        let mut note = String::new();
        if (apply || mode == "reap") && verdict != "ok" && verdict != "pr-open" {
            crate::commands::kill_worktree_public(config, &worktree);
            note = "stopped".into();
            let now = process_rows();
            for group in &live_groups {
                if !group_members(*group, &now).is_empty() {
                    note = format!("left alone: nothing proves group {group} is ours");
                }
            }
            if verdict == "orphan" {
                remove_worktree_state(config, &worktree);
            }
        }
        rows.push(Row {
            project: config.project.clone(),
            worktree,
            verdict: verdict.into(),
            memory,
            uptime: dur(age),
            note,
        });
    }
    rows
}

pub fn reap() -> i32 {
    reap_sessions();
    let footprints = HashMap::new();
    for config in service_projects() {
        let rows = mem_project(&config, false, "reap", &footprints);
        for row in rows {
            if row.verdict != "orphan" {
                continue;
            }
            let now = Command::new("date")
                .arg("+%F %T")
                .output()
                .ok()
                .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
                .unwrap_or_default();
            println!(
                "{now}  {}/{}  {} — its worktree no longer exists",
                row.project,
                row.worktree,
                if row.note.is_empty() { "stopped" } else { &row.note }
            );
        }
    }
    0
}

fn remove_worktree_state(config: &Config, worktree: &str) {
    if let Ok(entries) = std::fs::read_dir(&config.state_dir) {
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with(&format!("{worktree}.")) && name.ends_with(".pid") {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
    let _ = std::fs::remove_file(config.state_dir.join(format!("{worktree}.errors")));
    let _ = std::fs::remove_dir_all(config.state_dir.join(format!("{worktree}.up.lock")));
}

/// A session that crashed never ran its end hook: its record names a process that is gone.
fn reap_sessions() {
    let state_root = std::env::var("RAMIFY_STATE_DIR").unwrap_or_else(|_| "/tmp/ramify".into());
    let folder = std::path::Path::new(&state_root).join("sessions");
    for agent in crate::statusline::records(&state_root) {
        if !crate::statusline::running(agent.pid) {
            let _ = std::fs::remove_file(folder.join(format!("{}.session", agent.id)));
        }
    }
}
