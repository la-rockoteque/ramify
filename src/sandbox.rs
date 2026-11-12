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
                // SAFETY: getuid has no preconditions and cannot fail.
                let uid = unsafe { libc::getuid() };
                let exists = |path: &Path| -> Option<bool> {
                    std::fs::symlink_metadata(path).ok().map(|meta| meta.is_dir())
                };
                let (dirs, files) = linux_hidden(&secrets, uid, &exists);
                let mut cmd = Command::new(program);
                cmd.args(bwrap_args(root, temp, &dirs, &files, command));
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
    ".cargo/credentials.toml",
    ".cargo/credentials",
    ".vault-token",
    ".config/hub",
    ".terraform.d",
    ".orbstack",
    ".colima",
    ".zsh_history",
    ".bash_history",
    ".local/share/fish/fish_history",
    "Library/Keychains",
];

/// Daemon sockets on Linux that would let a command start work outside the sandbox.
const LINUX_SOCKETS: &[&str] = &["/var/run/docker.sock", "/run/docker.sock"];

/// The folders and files bwrap hides: the secrets, the user's runtime folder (session bus,
/// systemd), and the Docker sockets. `exists` gives `Some(is_dir)` for a path that exists.
pub fn linux_hidden(
    secrets: &[PathBuf],
    uid: u32,
    exists: &dyn Fn(&Path) -> Option<bool>,
) -> (Vec<PathBuf>, Vec<PathBuf>) {
    let mut dirs = Vec::new();
    let mut files = Vec::new();
    let runtime = PathBuf::from(format!("/run/user/{uid}"));
    let candidates = secrets
        .iter()
        .cloned()
        .chain(std::iter::once(runtime))
        .chain(LINUX_SOCKETS.iter().map(PathBuf::from));
    for path in candidates {
        match exists(&path) {
            Some(true) => dirs.push(path),
            Some(false) => files.push(path),
            None => {}
        }
    }
    (dirs, files)
}

/// The bwrap arguments: `/` read-only, the worktree and temp folder writable, the hidden paths
/// covered, and no network, no shared process or IPC namespace, no controlling terminal.
pub fn bwrap_args(
    root: &Path,
    temp: &Path,
    hidden_dirs: &[PathBuf],
    hidden_files: &[PathBuf],
    command: &str,
) -> Vec<std::ffi::OsString> {
    let mut args: Vec<std::ffi::OsString> = Vec::new();
    let mut push = |items: &[&std::ffi::OsStr]| args.extend(items.iter().map(|i| i.to_os_string()));
    push(&["--ro-bind".as_ref(), "/".as_ref(), "/".as_ref()]);
    push(&["--dev".as_ref(), "/dev".as_ref(), "--proc".as_ref(), "/proc".as_ref()]);
    push(&["--bind".as_ref(), root.as_os_str(), root.as_os_str()]);
    push(&["--bind".as_ref(), temp.as_os_str(), temp.as_os_str()]);
    for dir in hidden_dirs {
        push(&["--tmpfs".as_ref(), dir.as_os_str()]);
    }
    for file in hidden_files {
        push(&["--ro-bind".as_ref(), "/dev/null".as_ref(), file.as_os_str()]);
    }
    push(&[
        "--unshare-net".as_ref(),
        "--unshare-pid".as_ref(),
        "--unshare-ipc".as_ref(),
        "--new-session".as_ref(),
        "--die-with-parent".as_ref(),
        "sh".as_ref(),
        "-c".as_ref(),
        command.as_ref(),
    ]);
    args
}

/// The secret paths under `home`, by their real path, since Seatbelt matches real paths.
pub fn secret_paths(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    SECRETS.iter().map(|name| home.join(name)).collect()
}

/// Reads anywhere but the secret paths. Writes only under `root`, `temp` and a few devices,
/// and never to Git metadata, which Git runs later outside the sandbox (hooks, fsmonitor).
/// No network, Unix sockets included: Docker or an ssh-agent would be a way out. No terminal
/// ioctl, so a command cannot type into the TUI. No way to start a program outside the sandbox
/// through Launch Services, Apple events or launchd. Seatbelt applies the last matching rule.
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
(deny file-ioctl (regex #"^/dev/tty"))
(deny file-write*)
(allow file-write*
  (subpath "{root}")
  (subpath "{temp}")
  (literal "/dev/null")
  (literal "/dev/zero")
  (literal "/dev/dtracehelper")
  (regex #"^/dev/fd/"))
(deny file-write* (regex #"/\.git(/|$)"))
"#,
        root = quote(root),
        temp = quote(temp),
    )
}

/// Names that hold a credential, or a path or socket that leads to one.
const SECRET_NAMES: &[&str] = &[
    "OPENAI_API_KEY",
    "DEEPSEEK_API_KEY",
    "SSH_AUTH_SOCK",
    "DOCKER_HOST",
    "GOOGLE_APPLICATION_CREDENTIALS",
];

/// Name endings that mark a credential. `_PWD` does not match `PWD` or `OLDPWD`.
const SECRET_ENDINGS: &[&str] = &[
    "_KEY",
    "_TOKEN",
    "_SECRET",
    "_PAT",
    "_APIKEY",
    "_CREDENTIALS",
    "_PASS",
    "_PWD",
    "_AUTH",
    "_DSN",
];

/// A URL with a user and a password, such as `postgres://app:hunter2@db/x`.
fn has_userinfo(value: &str) -> bool {
    static USERINFO: std::sync::OnceLock<regex::Regex> = std::sync::OnceLock::new();
    USERINFO
        .get_or_init(|| {
            regex::Regex::new(r"[A-Za-z][A-Za-z0-9+.-]*://[^/@\s:]+:[^/@\s]+@").expect("compiles")
        })
        .is_match(value)
}

/// True for a variable that holds a key, a token, a password, or a way to reach one.
fn is_secret(name: &str, value: &str, key_envs: &[String]) -> bool {
    let upper = name.to_ascii_uppercase();
    upper.starts_with("ANTHROPIC_")
        || upper.starts_with("RAM_CODE_")
        || SECRET_NAMES.contains(&upper.as_str())
        || key_envs.iter().any(|key| key == name)
        || SECRET_ENDINGS.iter().any(|ending| upper.ends_with(ending))
        || upper.contains("PASSWORD")
        || has_userinfo(value)
}

/// `env` without the provider keys, the `key_env` variables, and anything that looks like a secret.
pub fn scrub(env: &[(String, String)], key_envs: &[String]) -> Vec<(String, String)> {
    env.iter()
        .filter(|(name, value)| !is_secret(name, value, key_envs))
        .cloned()
        .collect()
}
