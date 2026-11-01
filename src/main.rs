use crossterm::event::{self, Event, KeyEventKind};
use ram_code::agent::{run_task, run_turn, AgentConfig, AgentEvent, AnthropicLlm};
use ram_code::app::{App, Screen};
use ram_code::ramify::RealRunner;
use ratatui::DefaultTerminal;
use serde_json::{json, Value};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
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
            (Some(events), Some(history)) => run_worker(&root, &task, &events, &history),
            _ => run_headless(&root, &task),
        };
    }

    let runner = Arc::new(RealRunner::default());
    let mut app = App::new(runner, root);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn argument_path(args: &[String], flag: &str) -> Option<PathBuf> {
    args.iter()
        .position(|arg| arg == flag)
        .and_then(|position| args.get(position + 1))
        .map(PathBuf::from)
}

fn run_headless(root: &Path, task: &str) -> std::io::Result<()> {
    let config = match AgentConfig::from_process_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    let llm = AnthropicLlm::new(config);
    let result = run_task(&llm, root, task, &mut |event| match event {
        AgentEvent::Text(text) => println!("{text}"),
        AgentEvent::ToolCall { name, input } => println!("→ {name} {input}"),
        AgentEvent::ToolResult(result) => {
            if let Some(first) = result.lines().next() {
                println!("  {first}");
            }
        }
        AgentEvent::Error(error) => eprintln!("error: {error}"),
    });
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
fn run_worker(root: &Path, task: &str, events: &Path, history: &Path) -> std::io::Result<()> {
    let mut out = OpenOptions::new().create(true).append(true).open(events)?;
    let _ = writeln!(out, "{}", json!({"type": "trace", "text": "worker start"}));
    let _ = out.flush();
    let config = match AgentConfig::from_process_env() {
        Ok(config) => config,
        Err(error) => {
            let _ = writeln!(out, "{}", json!({"type": "error", "text": error}));
            return Ok(());
        }
    };
    let previous: Vec<Value> = std::fs::read_to_string(history)
        .ok()
        .and_then(|text| serde_json::from_str(&text).ok())
        .unwrap_or_default();
    let _ = writeln!(out, "{}", json!({"type": "trace", "text": format!("worker url {}", config.base_url)}));
    let _ = out.flush();
    let llm = AnthropicLlm::new(config);
    let result = run_turn(&llm, root, &previous, task, &mut |event| {
        let value = match event {
            AgentEvent::Text(text) => json!({"type": "text", "text": text}),
            AgentEvent::ToolCall { name, input } => {
                json!({"type": "tool_call", "name": name, "input": input})
            }
            AgentEvent::ToolResult(text) => json!({"type": "tool_result", "text": text}),
            AgentEvent::Error(text) => json!({"type": "error", "text": text}),
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
