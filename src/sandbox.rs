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

    /// `sh -c command` inside the sandbox. Writes go only to `root` and `temp`. The secret
    /// files under `home` cannot be read.
    pub fn command(
        &self,
        root: &Path,
        temp: &Path,
        home: Option<&Path>,
        command: &str,
    ) -> Option<Command> {
        let secrets = secret_paths(home);
        match self {
            Sandbox::Seatbelt(program) => {
                let mut cmd = Command::new(program);
                cmd.arg("-p").arg(seatbelt_profile(root, temp, &secrets));
                cmd.args(["sh", "-c", command]);
                Some(cmd)
            }
            Sandbox::Bwrap(program) => {
                let mut cmd = Command::new(program);
                cmd.args(["--ro-bind", "/", "/", "--dev", "/dev", "--proc", "/proc"]);
                cmd.arg("--bind").arg(root).arg(root);
                cmd.arg("--bind").arg(temp).arg(temp);
                for secret in &secrets {
                    if secret.is_dir() {
                        cmd.arg("--tmpfs").arg(secret);
                    } else if secret.exists() {
                        cmd.arg("--ro-bind").arg("/dev/null").arg(secret);
                    }
                }
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

/// Files and folders under the home folder that hold credentials. A command may not read them,
/// because what it reads goes to the provider in the transcript.
const SECRETS: &[&str] = &[
    ".ssh",
    ".aws",
    ".azure",
    ".gnupg",
    ".kube",
    ".docker",
    ".config/gcloud",
    ".config/gh",
    ".config/anthropic",
    ".git-credentials",
    ".netrc",
    ".npmrc",
    ".pypirc",
    "Library/Keychains",
];

/// The secret paths under `home`, by their real path, since Seatbelt matches real paths.
pub fn secret_paths(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    SECRETS.iter().map(|name| home.join(name)).collect()
}

/// Reads anywhere but the secret paths. Writes only under `root`, `temp` and the terminal
/// devices. No network, apart from local Unix sockets, which tools use to talk to their own
/// helpers. No way to start a program outside the sandbox through Launch Services, Apple
/// events or launchd.
pub fn seatbelt_profile(root: &Path, temp: &Path, secrets: &[PathBuf]) -> String {
    let hidden = if secrets.is_empty() {
        String::new()
    } else {
        let rules: Vec<String> = secrets
            .iter()
            .map(|path| format!("  (subpath \"{}\")", quote(path)))
            .collect();
        format!("(deny file-read*\n{})\n", rules.join("\n"))
    };
    format!(
        r#"(version 1)
(allow default)
(deny appleevent-send)
(deny mach-lookup (global-name "com.apple.coreservices.launchservicesd"))
(deny process-exec
  (literal "/usr/bin/open")
  (literal "/usr/bin/osascript")
  (literal "/usr/bin/osacompile")
  (literal "/bin/launchctl"))
{hidden}(deny network*)
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
