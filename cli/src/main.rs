mod commands;
mod config;
mod pyjson;
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
