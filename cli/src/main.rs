mod commands;
mod config;
mod mem;
mod jira;
mod launch;
mod pyjson;
mod run;
mod setup;
mod statusline;
mod prompt;
mod dash;
mod workflow;

use std::io::Write;

const COMMANDS: &[(&str, &str, &str)] = &[
    ("setup", "setup [--print | --force] [--local]", "write .ramify.conf for this repo"),
    ("config", "config [-q | get VAR]", "show the resolved config; -q tests that one exists"),
    ("new", "new <slug> [--workflow W [--lane L] [--tag T]...]", "cut a worktree and branch from the remote main, copy, bootstrap, start its stack, and write its work order"),
    ("workflow", "workflow [list | show <workflow>]", "the workflows this repo offers, or one workflow's lanes and steps"),
    ("step", "step [done <step> [--approved] | na <step> | gate <step>|--pr | ask <step> [--file F]... | wait <step>] [--note TEXT]", "this branch's work order and next step, record a step, exit 1 while <step> waits on earlier ones, put a human gate up in the approvals window, or wait for the decision"),
    ("approve", "approve <step> [--note TEXT]", "the human's OK on a gate the agent asked about: records the step"),
    ("reject", "reject <step> --note TEXT", "the human's no on a gate the agent asked about, with what to change"),
    ("gates", "gates [--json | --scan]", "every human gate on the machine that waits on a decision"),
    ("approvals", "approvals", "open the approvals window"),
    ("scratch", "scratch [--title T] [FILE | -] | --list | --clear", "hand the human a text to copy: it opens in the scratch window, with a Copy button"),
    ("run", "run [ask <script> [--cwd DIR] [--note TEXT] | wait <id> [--timeout S] | go <id> | block <id> [--note TEXT]]", "hand the human a script to run: it opens in the run window (bare: open the window), with a Run and a Block button; go and block are the human's"),
    ("runs", "runs [--json]", "every script handed to the human, newest first, with its state"),
    ("lane", "lane <lane>", "raise this branch's lane; a lane is never lowered"),
    ("label", "label [slug]", "print the /rename and /color lines for a session"),
    ("up", "up", "start this worktree's QA stack"),
    ("down", "down", "stop this worktree's own services"),
    ("ping", "ping", "exit 0 when this worktree's services answer their health check"),
    ("card", "card", "this worktree's URLs and health"),
    ("dash", "dash [--json]", "every stack on the machine"),
    ("reap", "reap", "stop the stacks whose worktree no longer exists, in every project (the SessionStart hook runs it)"),
    ("mem", "mem [--apply]", "where the memory goes: each stack's footprint and verdict, and dev servers and test runners left outside ramify; --apply stops the zombie, merged and orphan stacks"),
    ("watch", "watch [seconds]", "redraw the dashboard every few seconds (default 2)"),
    ("menu", "menu", "interactive menu for this project"),
    ("status", "status", "this worktree's slot, ports and processes"),
    ("shared-down", "shared-down", "stop the shared services"),
    ("cleanup", "cleanup", "release stale slots, kill orphans and strays, drop dead state"),
    ("prune", "prune [--all] [--apply]", "remove merged branches and their worktrees; dry run without --apply"),
    ("delete", "delete <worktree> [--yes]", "force-delete a worktree and its branch, merged or not"),
    ("complete", "complete <worktree> [--yes]", "close its ticket, stop its stack, remove the worktree and branch"),
    ("jira", "jira login | check", "store a Jira token in the Keychain, or test it"),
    ("ticket", "ticket start | sync | watch", "assign the story, synchronize its Jira status, or resume automatic synchronization"),
    ("statusline", "statusline [--json]", "status line for Claude Code, from its JSON on stdin"),
    ("prompt", "prompt [--json | install]", "the status line and the agent sessions of the current directory, for a zsh prompt; install links the oh-my-zsh plugin"),
    ("app", "app", "open the desktop dashboard"),
    ("write-set", "write-set <file> | --self-test", "check a branch's diff against the write set its QA plan declared"),
];

fn print_usage(command: Option<&str>) -> bool {
    match command {
        Some(command) => {
            let Some((_, usage, what)) = COMMANDS.iter().find(|(name, _, _)| *name == command) else {
                return false;
            };
            println!("usage: ramify {usage}\n  {what}");
            true
        }
        None => {
            println!("usage: ramify <command> [--help]");
            for (_, usage, what) in COMMANDS {
                println!("  {usage:<40} {what}");
            }
            true
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let command = args.first().map(String::as_str).unwrap_or("");
    let json = args.iter().any(|arg| arg == "--json");
    if command.is_empty() || command == "-h" || command == "--help" {
        print_usage(None);
        return;
    }
    if command == "help" {
        if print_usage(args.get(1).map(String::as_str)) {
            return;
        }
        print_usage(None);
        std::process::exit(2);
    }
    if args.iter().skip(1).any(|arg| arg == "-h" || arg == "--help") {
        if !print_usage(Some(command)) {
            print_usage(None);
            std::process::exit(2);
        }
        return;
    }
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
    if command == "dash" && !json {
        std::process::exit(commands::dash_human());
    }
    if command == "watch" {
        std::process::exit(commands::watch(&args[1..]));
    }
    if command == "menu" {
        std::process::exit(commands::menu());
    }
    if command == "setup" {
        std::process::exit(setup::setup(&args[1..]));
    }
    if command == "ticket" {
        std::process::exit(jira::ticket_command(&args[1..]));
    }
    if command == "jira" {
        std::process::exit(jira::jira_command(&args[1..]));
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
    if command == "prompt" {
        std::process::exit(prompt::prompt(&args[1..]));
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
        (_, _) => {
            print_usage(None);
            std::process::exit(2);
        }
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
