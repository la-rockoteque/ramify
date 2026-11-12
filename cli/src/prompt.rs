//! `ramify prompt`: the status line for a shell prompt, from the current directory.

use crate::statusline::{render, Mode};
use std::path::PathBuf;

pub fn prompt(args: &[String]) -> i32 {
    match args.first().map(String::as_str) {
        Some("install") => install(args.get(1).map(String::as_str) == Some("--force")),
        None | Some("--json") => {
            let cwd = std::env::current_dir()
                .map(|cwd| cwd.to_string_lossy().into_owned())
                .unwrap_or_else(|_| ".".into());
            if let Some(text) = render("-", &cwd, !args.is_empty(), Mode::Prompt) {
                println!("{text}");
            }
            0
        }
        Some(_) => {
            eprintln!("ramify: usage: ramify prompt [--json | install [--force]]");
            2
        }
    }
}

/// Links `shell/` as the oh-my-zsh plugin `ramify`. It edits no rc file: it prints the lines.
/// It replaces a `ramify` link that points elsewhere only with `--force`.
fn install(force: bool) -> i32 {
    let home = match crate::launch::home() {
        Ok(home) => home,
        Err(error) => {
            eprintln!("ramify: {error}");
            return 1;
        }
    };
    let custom = std::env::var_os("ZSH_CUSTOM").map(PathBuf::from).unwrap_or_else(|| {
        PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".oh-my-zsh/custom")
    });
    let plugins = custom.join("plugins");
    let link = plugins.join("ramify");
    let source = home.join("shell");
    if !source.join("ramify.plugin.zsh").is_file() {
        eprintln!("ramify: {} has no ramify.plugin.zsh — set RAMIFY_HOME to the ramify install", source.display());
        return 1;
    }
    let text = source.to_string_lossy();
    if text.contains("/plugins/cache/") || text.contains("-wt/") {
        eprintln!(
            "ramify: warning: {} goes away when this version or worktree is removed; the link will then dangle",
            source.display()
        );
    }
    if let Err(error) = std::fs::create_dir_all(&plugins) {
        eprintln!("ramify: could not create {}: {error}", plugins.display());
        return 1;
    }
    match std::fs::read_link(&link) {
        Ok(target) if target == source => {}
        Ok(target) if !force => {
            eprintln!(
                "ramify: {} already points to {} — run 'ramify prompt install --force' to replace it",
                link.display(),
                target.display()
            );
            return 1;
        }
        Ok(_) => {
            let _ = std::fs::remove_file(&link);
        }
        Err(_) if link.exists() => {
            eprintln!("ramify: {} exists and is not a link — move it away first", link.display());
            return 1;
        }
        Err(_) => {}
    }
    if std::fs::read_link(&link).is_err() {
        if let Err(error) = std::os::unix::fs::symlink(&source, &link) {
            eprintln!("ramify: could not link {}: {error}", link.display());
            return 1;
        }
    }
    println!("  linked          {} → {}", link.display(), source.display());
    println!("  in ~/.zshrc     add ramify to the plugins list: plugins=(git ramify)");
    println!("  in ~/.p10k.zsh  add ramify to POWERLEVEL9K_RIGHT_PROMPT_ELEMENTS");
    println!("  then            exec zsh");
    0
}
