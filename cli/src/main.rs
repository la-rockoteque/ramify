mod commands;
mod config;
mod mem;
mod launch;
mod pyjson;
mod run;
mod statusline;
mod dash;
mod workflow;

use std::io::Write;

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    let json = args.iter().any(|arg| arg == "--json");
    if command == "config" {
        std::process::exit(commands::config(&args[1..]));
    }
    if command == "status" {
        std::process::exit(commands::status());
    }
    if command == "prune" {
        std::process::exit(commands::prune(&args[1..]));
    }
    if command == "delete" {
        std::process::exit(commands::delete(&args[1..]));
    }
    if command == "complete" {
        std::process::exit(commands::complete(&args[1..]));
    }
    if command == "cleanup" {
        std::process::exit(commands::cleanup());
    }
    if command == "new" {
        std::process::exit(commands::new(&args[1..]));
    }
    if command == "reap" {
        std::process::exit(mem::reap());
    }
    if command == "scratch" {
        std::process::exit(run::scratch(&args[1..]));
    }
    if command == "run" {
        std::process::exit(run::run(&args[1..]));
    }
    if command == "app" || command == "approvals" {
        let kind = if command == "app" {
            args.get(1).map(String::as_str).unwrap_or("")
        } else {
            "approvals"
        };
        let kind_args: Vec<String> = if kind.is_empty() {
            Vec::new()
        } else {
            vec![kind.to_string()]
        };
        std::process::exit(launch::app(&kind_args));
    }
    if command == "statusline" {
        std::process::exit(statusline::statusline(&args[1..]));
    }
    if command == "mem" {
        std::process::exit(mem::mem(&args[1..]));
    }
    if command == "write-set" {
        std::process::exit(commands::write_set(&args[1..]));
    }
    if command == "up" {
        std::process::exit(commands::up());
    }
    if command == "down" {
        std::process::exit(commands::down());
    }
    if command == "shared-down" {
        std::process::exit(commands::shared_down());
    }
    if command == "ping" {
        std::process::exit(commands::ping());
    }
    if command == "label" {
        std::process::exit(commands::label(&args[1..]));
    }
    if command == "lane" {
        std::process::exit(commands::lane(&args[1..]));
    }
    if command == "gates" {
        std::process::exit(commands::gates(&args[1..]));
    }
    if command == "step" && args.get(1).map(String::as_str) == Some("asked") && json {
        std::process::exit(commands::step_asked());
    }
    if command == "card" {
        std::process::exit(commands::card());
    }
    if command == "runs" {
        std::process::exit(commands::runs(&args[1..]));
    }
    if command == "workflow" {
        std::process::exit(commands::workflow(&args[1..]));
    }
    if command == "step" && args.len() == 1 {
        std::process::exit(commands::step());
    }
    let result = match (command, json) {
        ("dash-project", true) => dash::project_json().map(|text| (text, false)),
        ("dash", true) => dash::all_json().map(|text| (text, true)),
        ("dash-project", false) => Err("usage: ramify dash-project --json".into()),
        (other, _) => Err(format!("ramify-rs: not ported yet: {other}")),
    };
    match result {
        Ok((text, newline)) => {
            let mut out = std::io::stdout().lock();
            let _ = if newline { writeln!(out, "{text}") } else { write!(out, "{text}") };
        }
        Err(error) => {
            let _ = writeln!(std::io::stderr(), "ramify: {error}");
            std::process::exit(1);
        }
    }
}
