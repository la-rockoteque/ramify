//! The sandbox around the model's shell commands: what it may write, no network, and an
//! environment without provider keys.

use std::path::{Path, PathBuf};
use std::process::Command;

/// The program that confines a command, or none on this machine.
#[derive(Debug, Clone, PartialEq)]
pub enum Sandbox {
    /// macOS `sandbox-exec` with a Seatbelt profile.
    Seatbelt(PathBuf),
    /// Linux bubblewrap.
    Bwrap(PathBuf),
    /// No sandbox: every command goes to the human.
    None,
}

impl Sandbox {
    pub fn detect() -> Self {
        if cfg!(target_os = "macos") {
            let program = PathBuf::from("/usr/bin/sandbox-exec");
            if program.is_file() {
                return Sandbox::Seatbelt(program);
            }
        }
        if cfg!(target_os = "linux") {
            if let Some(program) = on_path("bwrap") {
                return Sandbox::Bwrap(program);
            }
        }
        Sandbox::None
    }

    /// `sh -c command` inside the sandbox. Writes go only to `root` and `temp`.
    pub fn command(&self, root: &Path, temp: &Path, command: &str) -> Option<Command> {
        match self {
            Sandbox::Seatbelt(program) => {
                let mut cmd = Command::new(program);
                cmd.arg("-p").arg(seatbelt_profile(root, temp));
                cmd.args(["sh", "-c", command]);
                Some(cmd)
            }
            Sandbox::Bwrap(program) => {
                let mut cmd = Command::new(program);
                cmd.args(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
                cmd.arg("--bind").arg(root).arg(root);
                cmd.arg("--bind").arg(temp).arg(temp);
                cmd.args(["--unshare-net", "--die-with-parent", "sh", "-c", command]);
                Some(cmd)
            }
            Sandbox::None => None,
        }
    }
}

fn on_path(name: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|path| {
        std::env::split_paths(&path)
            .map(|dir| dir.join(name))
            .find(|candidate| candidate.is_file())
    })
}

fn quote(path: &Path) -> String {
    path.display()
        .to_string()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

/// Reads anywhere. Writes only under `root`, `temp` and the terminal devices. No network,
/// apart from local Unix sockets, which tools use to talk to their own helpers.
pub fn seatbelt_profile(root: &Path, temp: &Path) -> String {
    format!(
        r#"(version 1)
(allow default)
(deny network*)
(allow network* (local unix-socket) (remote unix-socket))
(deny file-write*)
(allow file-write*
  (subpath "{root}")
  (subpath "{temp}")
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/dtracehelper")
  (regex #"^/dev/tty")
  (regex #"^/dev/fd/"))
"#,
        root = quote(root),
        temp = quote(temp),
    )
}

/// True for a variable that holds a key, a token or a password.
fn is_secret(name: &str, key_envs: &[String]) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("ANTHROPIC_")
        || upper.starts_with("RAM_CODE_")
        || upper == "OPENAI_API_KEY"
        || upper == "DEEPSEEK_API_KEY"
        || key_envs.iter().any(|key| key == name)
        || upper.ends_with("_KEY")
        || upper.ends_with("_TOKEN")
        || upper.ends_with("_SECRET")
        || upper.contains("PASSWORD")
}

/// `env` without the provider keys, the `key_env` variables, and anything that looks like a secret.
pub fn scrub(env: &[(String, String)], key_envs: &[String]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(name, _)| !is_secret(name, key_envs))
        .cloned()
        .collect()
}
