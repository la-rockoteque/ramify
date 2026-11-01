use crossterm::event::{self, Event, KeyEventKind};
use ram_code::agent::{run_task, AgentConfig, AgentEvent, AnthropicLlm};
use ram_code::app::{App, Screen};
use ram_code::ramify::RealRunner;
use ratatui::DefaultTerminal;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

fn main() -> std::io::Result<()> {
    let root = std::env::var_os("RAMIFY_ROOT")
        .map(PathBuf::from)
        .unwrap_or_else(|| std::env::current_dir().unwrap_or_default());

    let args: Vec<String> = std::env::args().collect();
    if let Some(position) = args.iter().position(|arg| arg == "--task") {
        let task = args
            .get(position + 1)
            .cloned()
            .unwrap_or_else(|| "--task needs a task text".into());
        return run_headless(&root, &task);
    }

    let runner = Arc::new(RealRunner::default());
    let mut app = App::new(runner, root);

    let mut terminal = ratatui::init();
    let result = run(&mut terminal, &mut app);
    ratatui::restore();
    result
}

fn run_headless(root: &std::path::Path, task: &str) -> std::io::Result<()> {
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
        AgentEvent::Done(_) => {}
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

fn run(terminal: &mut DefaultTerminal, app: &mut App) -> std::io::Result<()> {
    let mut splash_from = std::time::Instant::now();
    loop {
        terminal.draw(|frame| frame.render_widget(&*app, frame.area()))?;
        app.poll_events();
        if app.quit {
            break;
        }
        if app.screen == Screen::Splash && splash_from.elapsed() >= Duration::from_millis(1500) {
            app.load();
            splash_from = std::time::Instant::now();
        }
        if event::poll(Duration::from_millis(120))? {
            if let Event::Key(key) = event::read()? {
                if key.kind == KeyEventKind::Press {
                    if app.screen == Screen::Splash {
                        app.load();
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
