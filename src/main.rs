use crossterm::event::{self, Event, KeyEventKind};
use ram_code::agent::{run_turn_hooked, AgentEvent, Llm};
use ram_code::hooked::Hooked;
use ram_code::providers::resolve_from_env;
use ram_code::app::{App, Screen};
use ram_code::panel::Pane;
use ram_code::ramify::RealRunner;
use ram_code::update;
use ratatui::DefaultTerminal;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    if args.iter().skip(1).any(|arg| arg == "-h" || arg == "--help") {
        println!(
            "Ram Code — a terminal coding harness for Ramify worktrees\n\n\
             usage: ram [--panel <pane>] [--task <text>] | ram update\n\n  \
             no flags     open the chat\n  \
             update       install the newer release, or the new commits on main\n  \
             --panel P    open the side panel on dashboard, approvals, scratch, run,\n               \
             sessions, agents or artifacts\n  \
             --task T     run one task without the interface\n\n\
             keys: enter send · tab worktrees · ctrl+p panel · ctrl+c quit"
        );
        return Ok(());
    }
    if args.iter().skip(1).any(|arg| arg == "-V" || arg == "--version") {
        let install = update::home().map(|home| update::install_of(&home));
        match install.and_then(|install| install.version()) {
            Some(version) => println!("ram-code {} ({version})", env!("CARGO_PKG_VERSION")),
            None => println!("ram-code {}", env!("CARGO_PKG_VERSION")),
        }
        return Ok(());
    }
    if args.get(1).map(String::as_str) == Some("update") {
        let install = update::home()
            .map(|home| update::install_of(&home))
            .unwrap_or(update::Install::Unknown);
        match update::run(&install, &update::releases(), &update::cache_dir()) {
            Ok(done) => {
                println!("{done}");
                return Ok(());
            }
            Err(error) => {
                eprintln!("ram update: {error}");
                std::process::exit(1);
            }
        }
    }
    let root = std::env::var_os("RAMIFY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    if let Some(position) = args.iter().position(|arg| arg == "--task") {
        let task = args
            .get(position + 1)
            .cloned()
            .unwrap_or_else(|| "--task needs a task text".into());
        let events = argument_path(&args, "--events");
        let history = argument_path(&args, "--history");
        return match (events, history) {
            (Some(events), Some(history)) => run_worker(
                &root,
                &task,
                &events,
                &history,
                argument_path(&args, "--context").as_deref(),
            ),
            _ => run_headless(&root, &task),
        };
    }

    let runner = Arc::new(RealRunner::default());
    let mut app = App::new(runner, root);
    if let Some(position) = args.iter().position(|arg| arg == "--panel") {
        let name = args.get(position + 1).map(String::as_str).unwrap_or("dashboard");
        let pane = Pane::from_name(name).unwrap_or(Pane::Dashboard);
        app.panel.open_on(pane);
    } else if let Some(name) = args
        .iter()
        .find_map(|arg| arg.strip_prefix("--panel="))
    {
        let pane = Pane::from_name(name).unwrap_or(Pane::Dashboard);
        app.panel.open_on(pane);
    }

    app.start_session("startup");
    app.start_update_check();
    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    app.end_session();
    if app.auto_update() {
        println!(
            "ram: an update runs in the background; its log is {}",
            update::cache_dir().join("update.log").display()
        );
    }
    result
}

fn argument_path(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|position| args.get(position + 1))
        .map(PathBuf::from)
}

fn run_headless(root: &Path, task: &str) -> std::io::Result<()> {
    let resolved = match resolve_from_env(&|key| std::env::var(key).ok()) {
        Ok(resolved) => resolved,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let llm = resolved.llm();
    // One task is one session: it starts and ends here.
    let mut hooks = Hooked::load(root, &format!("ram-{}", std::process::id()));
    let started = hooks.fire("SessionStart", "startup", json!({"source": "startup"}));
    for note in &started.notes {
        eprintln!("{note}");
    }
    hooks.start_context = started.contexts;
    let result = run_headless_task(llm.as_ref(), root, task, &mut |event| match event {
        AgentEvent::Text(text) => println!("{text}"),
        AgentEvent::ToolCall { name, input } => println!("→ {name} {input}"),
        AgentEvent::ToolResult(result) => {
            if let Some(first) = result.lines().next() {
                println!("  {first}");
            }
        }
        AgentEvent::Error(error) => eprintln!("error: {error}"),
        AgentEvent::Hook(note) => eprintln!("{note}"),
    }, &hooks);
    let _ = hooks.fire("SessionEnd", "", json!({"reason": "exit"}));
    match result {
        Ok(answer) => {
            println!("{answer}");
            Ok(())
        }
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    }
}

/// The worker writes one JSON event per line, then the new history, then `finished`.
fn run_worker(
    root: &Path,
    task: &str,
    events: &Path,
    history: &Path,
    context: Option<&Path>,
) -> std::io::Result<()> {
    let mut out = OpenOptions::new().create(true).append(true).open(events)?;
    let _ = writeln!(out, "{}", json!({"type": "trace", "text": "worker start"}));
    let _ = out.flush();
    let resolved = match resolve_from_env(&|key| std::env::var(key).ok()) {
        Ok(resolved) => resolved,
        Err(error) => {
            let _ = writeln!(out, "{}", json!({"type": "error", "text": error}));
            return Ok(());
        }
    };
    let previous: Vec<Value> = std::fs::read_to_string(history)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let _ = writeln!(out, "{}", json!({"type": "trace", "text": format!("worker {} {} ({})", resolved.name, resolved.config.base_url, resolved.config.credential.label())}));
    let _ = out.flush();
    let llm = resolved.llm();
    // The interface owns the session: it passes its id, and the folder where SessionStart
    // leaves the hook snapshot and, for the first task, its context.
    let session = std::env::var("RAM_CODE_SESSION").unwrap_or_else(|_| format!("ram-{}", std::process::id()));
    let start = std::env::var_os("RAM_CODE_START").map(PathBuf::from);
    let mut hooks = Hooked::for_task(root, &session, start.as_deref());
    if let Some(path) = context {
        if let Ok(text) = std::fs::read_to_string(path) {
            if !text.trim().is_empty() {
                hooks.start_context = vec![text];
            }
        }
    }
    let result = run_turn_hooked(llm.as_ref(), root, &previous, task, &hooks, &mut |event| {
        let value = match event {
            AgentEvent::Text(text) => json!({"type": "text", "text": text}),
            AgentEvent::ToolCall { name, input } => {
                json!({"type": "tool_call", "name": name, "input": input})
            }
            AgentEvent::ToolResult(text) => json!({"type": "tool_result", "text": text}),
            AgentEvent::Error(text) => json!({"type": "error", "text": text}),
            AgentEvent::Hook(text) => json!({"type": "hook", "text": text}),
        };
        let _ = writeln!(out, "{value}");
        let _ = out.flush();
    });
    match result {
        Ok((answer, new_history)) => {
            if let Ok(text) = serde_json::to_string(&new_history) {
                let _ = std::fs::write(history, text);
            }
            let _ = writeln!(out, "{}", json!({"type": "finished", "answer": answer}));
            let _ = out.flush();
        }
        Err(error) => {
            let _ = writeln!(out, "{}", json!({"type": "error", "text": error}));
            let _ = out.flush();
        }
    }
    Ok(())
}

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut splash_from = std::time::Instant::now();
    loop {
        terminal.draw(|frame| frame.render_widget(&*app, frame.area()))?;
        app.poll_events();
        app.tick();
        if app.quit {
            break;
        }
        if app.screen == Screen::Splash && splash_from.elapsed() >= Duration::from_millis(1500) {
            app.load_async();
            splash_from = std::time::Instant::now();
        }
        if app.screen == Screen::Loading {
            // Keep typed keys in the terminal buffer until the loader finishes.
            std::thread::sleep(Duration::from_millis(50));
            continue;
        }
        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.screen == Screen::Splash {
                        app.load_async();
                        splash_from = std::time::Instant::now();
                    } else {
                        app.on_key(key);
                    }
                }
            }
        }
    }
    Ok(())
}

/// `run_task` with the session's hooks: the headless run is its own session.
fn run_headless_task(
    llm: &dyn Llm,
    root: &Path,
    task: &str,
    on_event: &mut dyn FnMut(AgentEvent),
    hooks: &Hooked,
) -> Result<String, String> {
    run_turn_hooked(llm, root, &[], task, hooks, on_event).map(|(answer, _)| answer)
}
