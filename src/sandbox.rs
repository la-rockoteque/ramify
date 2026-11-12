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

    /// `sh -c command` inside the sandbox described by `paths`.
    pub fn command(&self, paths: &Paths, command: &str) -> Option<Command> {
        match self {
            Sandbox::Seatbelt(program) => {
                let mut cmd = Command::new(program);
                cmd.arg("-p").arg(seatbelt_profile_for(paths));
                cmd.args(["sh", "-c", command]);
                Some(cmd)
            }
            Sandbox::Bwrap(program) => {
                // SAFETY: getuid has no preconditions and cannot fail.
                let uid = unsafe { libc::getuid() };
                // A link is skipped: `secret_paths` lists its real path too, and bwrap cannot
                // mount over a link.
                let exists = |path: &Path| -> Option<bool> {
                    std::fs::symlink_metadata(path)
                        .ok()
                        .filter(|meta| !meta.file_type().is_symlink())
                        .map(|meta| meta.is_dir())
                };
                let (dirs, files) = linux_hidden(&paths.secrets, uid, &exists);
                let mut cmd = Command::new(program);
                cmd.args(bwrap_args(
                    &paths.root,
                    &paths.temp,
                    &dirs,
                    &files,
                    command,
                    paths.network,
                ));
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
    ".config/git/credentials",
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
/// covered, no shared process or IPC namespace, no controlling terminal, and no network unless
/// `network` is true.
pub fn bwrap_args(
    root: &Path,
    temp: &Path,
    hidden_dirs: &[PathBuf],
    hidden_files: &[PathBuf],
    command: &str,
    network: bool,
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
    if !network {
        push(&["--unshare-net".as_ref()]);
    }
    push(&[
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

/// The secret paths under `home`. Seatbelt matches real paths, and a dotfile manager often makes
/// `~/.config` a link, so each path comes as given and, when it differs, by its real path.
pub fn secret_paths(home: Option<&Path>) -> Vec<PathBuf> {
    let Some(home) = home else {
        return Vec::new();
    };
    let home = home.canonicalize().unwrap_or_else(|_| home.to_path_buf());
    SECRETS
        .iter()
        .flat_map(|name| given_and_real(&home.join(name)))
        .collect()
}

/// The path, and its real path when that differs.
fn given_and_real(path: &Path) -> Vec<PathBuf> {
    let resolved = real_through(path);
    if resolved == path {
        vec![path.to_path_buf()]
    } else {
        vec![path.to_path_buf(), resolved]
    }
}

/// The real path of the nearest existing ancestor, then the rest, so a path that does not exist
/// yet under a linked folder still resolves.
fn real_through(path: &Path) -> PathBuf {
    let mut probe = path.to_path_buf();
    let mut rest: Vec<std::ffi::OsString> = Vec::new();
    loop {
        if let Ok(found) = probe.canonicalize() {
            let mut full = found;
            for part in rest.iter().rev() {
                full.push(part);
            }
            return full;
        }
        match (probe.parent(), probe.file_name()) {
            (Some(parent), Some(name)) => {
                rest.push(name.to_os_string());
                probe = parent.to_path_buf();
            }
            _ => return path.to_path_buf(),
        }
    }
}

/// The paths one sandboxed command may use. Each path is a real path, because Seatbelt
/// matches real paths (`/var` is `/private/var`).
#[derive(Debug, Clone, Default)]
pub struct Paths {
    /// The worktree.
    pub root: PathBuf,
    /// The task's temp folder, which `TMPDIR` names.
    pub temp: PathBuf,
    /// The home folder, for the toolchains and Git settings in it.
    pub home: Option<PathBuf>,
    /// Credential paths that no rule can open.
    pub secrets: Vec<PathBuf>,
    /// Ram Code's configuration folder. A command may not write it, or it could widen its own
    /// sandbox through `sandbox.json`.
    pub config: Option<PathBuf>,
    /// What `sandbox.json` adds.
    pub extras: Extras,
    /// IP traffic and name lookups, for an MCP server the human approved with network.
    pub network: bool,
}

impl Paths {
    pub fn new(root: &Path, temp: &Path, home: Option<&Path>) -> Self {
        let home = home.map(real);
        Paths {
            root: real(root),
            temp: real(temp),
            secrets: secret_paths(home.as_deref()),
            home,
            config: None,
            extras: Extras::default(),
            network: false,
        }
    }
}

/// A temp folder a sandboxed process can write, created when missing. The process could replace
/// it with a link, which would widen the next profile, so it must still be a real folder that
/// the user owns.
pub fn checked_temp(path: &Path) -> Result<PathBuf, String> {
    use std::os::unix::fs::MetadataExt;
    let shown = path.display();
    let meta = match std::fs::symlink_metadata(path) {
        Ok(meta) => meta,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            std::fs::create_dir(path).map_err(|e| format!("could not create {shown}: {e}"))?;
            std::fs::symlink_metadata(path).map_err(|e| format!("could not read {shown}: {e}"))?
        }
        Err(error) => return Err(format!("could not read {shown}: {error}")),
    };
    // SAFETY: getuid has no preconditions and cannot fail.
    let uid = unsafe { libc::getuid() };
    if meta.file_type().is_symlink() || !meta.is_dir() || meta.uid() != uid {
        return Err(format!(
            "the temp folder {shown} was replaced; Ram Code runs no more commands in it"
        ));
    }
    Ok(path.to_path_buf())
}

/// The real path when the path exists, else the path as given.
fn real(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

/// Paths the user adds in `~/.config/ram-code/sandbox.json`.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Extras {
    pub read: Vec<PathBuf>,
    pub write: Vec<PathBuf>,
}

pub const SANDBOX_FILE: &str = "sandbox.json";

/// Read `sandbox.json` from Ram Code's configuration folder. A missing file adds nothing. A
/// file that is not valid is an error, so a command never runs with a sandbox the user did not
/// mean. Ram Code reads no sandbox file from the worktree.
pub fn load_extras(config: &Path, home: Option<&Path>) -> Result<Extras, String> {
    let path = config.join(SANDBOX_FILE);
    let text = match std::fs::read_to_string(&path) {
        Ok(text) => text,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Extras::default()),
        Err(error) => return Err(format!("could not read {}: {error}", path.display())),
    };
    let shown = path.display();
    let value: serde_json::Value =
        serde_json::from_str(&text).map_err(|e| format!("{shown} is not valid JSON: {e}"))?;
    let object = value
        .as_object()
        .ok_or_else(|| format!("{shown} must hold a JSON object with \"read\" and \"write\""))?;
    let mut extras = Extras::default();
    for (key, entries) in object {
        let list = match key.as_str() {
            "read" => &mut extras.read,
            "write" => &mut extras.write,
            other => return Err(format!("{shown} has an unknown key \"{other}\"; use read or write")),
        };
        let items = entries
            .as_array()
            .ok_or_else(|| format!("{shown}: \"{key}\" must be a list of paths"))?;
        for item in items {
            let raw = item
                .as_str()
                .ok_or_else(|| format!("{shown}: \"{key}\" holds a value that is not a path"))?;
            list.push(expand(raw, home).ok_or_else(|| {
                format!("{shown}: {raw} must be an absolute path or start with ~/")
            })?);
        }
    }
    Ok(extras)
}

fn expand(raw: &str, home: Option<&Path>) -> Option<PathBuf> {
    let path = match raw.strip_prefix("~/") {
        Some(rest) => home?.join(rest),
        None if Path::new(raw).is_absolute() => PathBuf::from(raw),
        None => return None,
    };
    Some(real(&path))
}

/// The Git folders of a linked worktree. Its `.git` is a file that names a folder under the
/// main repository's Git folder, which holds the objects and refs that Git reads.
fn git_folders(root: &Path) -> Vec<PathBuf> {
    let dot_git = root.join(".git");
    let Ok(text) = std::fs::read_to_string(&dot_git) else {
        return Vec::new();
    };
    let Some(target) = text.trim().strip_prefix("gitdir:") else {
        return Vec::new();
    };
    let gitdir = root.join(target.trim());
    let gitdir = real(&gitdir);
    let mut folders = vec![gitdir.clone()];
    if let Ok(common) = std::fs::read_to_string(gitdir.join("commondir")) {
        folders.push(real(&gitdir.join(common.trim())));
    }
    folders
}

/// The per-user temp and cache folders. `ar`, `xcrun` and `clang` use them and ignore `TMPDIR`.
#[cfg(target_os = "macos")]
fn user_folders() -> (Option<PathBuf>, Option<PathBuf>) {
    fn confstr(name: libc::c_int) -> Option<PathBuf> {
        let mut buffer = vec![0u8; 1024];
        // SAFETY: the buffer is valid for its length, and confstr writes at most that many bytes.
        let length = unsafe { libc::confstr(name, buffer.as_mut_ptr().cast(), buffer.len()) };
        if length == 0 || length > buffer.len() {
            return None;
        }
        buffer.truncate(length - 1);
        let path = PathBuf::from(String::from_utf8(buffer).ok()?);
        Some(real(&path))
    }
    (
        confstr(libc::_CS_DARWIN_USER_TEMP_DIR),
        confstr(libc::_CS_DARWIN_USER_CACHE_DIR),
    )
}

#[cfg(not(target_os = "macos"))]
fn user_folders() -> (Option<PathBuf>, Option<PathBuf>) {
    (None, None)
}

/// System folders that hold programs, libraries, compilers and their settings.
const SYSTEM_READS: &[&str] = &[
    "/usr",
    "/bin",
    "/sbin",
    "/System",
    "/Library",
    "/private/etc",
    "/private/var/db/timezone",
    "/private/var/select",
    "/dev",
    "/opt/homebrew",
    "/Applications/Xcode.app",
];

/// The links and folders on the way to the read roots. A process reads them to follow `/var`,
/// `/etc` and `/tmp` to `/private`.
const SYSTEM_LINKS: &[&str] = &["/", "/private", "/private/var", "/var", "/etc", "/tmp"];

/// Toolchains and their caches in the home folder, as folders. Secret files in them stay denied.
/// `~/.cargo` is listed by part, because its `config.toml` can hold a registry token.
const HOME_READS: &[&str] = &[
    ".cargo/bin",
    ".cargo/registry",
    ".cargo/git",
    ".rustup",
    ".asdf",
    ".nvm",
    ".pyenv",
    ".volta",
    ".local/share/mise",
    ".npm",
];

/// Single files in the home folder that tools read: rustup's `env` script, Cargo's settings
/// and Git's settings. Cargo stops when its `config.toml` exists and cannot be read, so it
/// stays readable; a registry token belongs in `credentials.toml`, which stays hidden. Git's
/// `credentials` file stays hidden too.
const HOME_FILES: &[&str] = &[
    ".cargo/env",
    ".cargo/config.toml",
    ".cargo/config",
    ".tool-versions",
    ".config/git/config",
    ".config/git/ignore",
    ".config/git/attributes",
];

/// The only name a command may create in the per-user temp folder. `xcrun` resets `TMPDIR` to
/// that folder, so `ar` makes `ar.XXXXXX` there. `xcrun` cannot write its cache, `xcrun_db`, and
/// says so on stderr; the build goes on. Tools outside the sandbox trust that cache to find
/// compilers, so it stays read-only. Other entries, and other programs' folders, stay out of reach.
const USER_TEMP_FILES: &str = r"ar\.[A-Za-z0-9]+";

/// Services a build needs: user and group lookups, logging, notifications and certificates.
/// Launch Services is not here.
const SERVICES: &[&str] = &[
    "com.apple.system.opendirectoryd.libinfo",
    "com.apple.system.DirectoryService.libinfo_v1",
    "com.apple.system.logger",
    "com.apple.logd",
    "com.apple.diagnosticd",
    "com.apple.system.notification_center",
    "com.apple.trustd.agent",
    "com.apple.bsd.dirhelper",
];

/// Keychain services, denied after the allows. An item stored with the `security` command often
/// lets that command read it without a prompt, so a sandboxed `security find-generic-password -w`
/// would put the secret in the transcript.
const KEYCHAIN_SERVICES: &[&str] = &[
    "com.apple.SecurityServer",
    "com.apple.securityd",
    "com.apple.securityd.xpc",
    "com.apple.secd",
    "com.apple.security.agent",
    "com.apple.security.agent.login",
    "com.apple.security.keychain-circle-notification",
    "com.apple.security.cloudkeychainproxy3",
];

fn subpath(path: &Path) -> String {
    format!("  (subpath \"{}\")", quote(path))
}

fn literal(path: &str) -> String {
    format!("  (literal \"{}\")", quote(Path::new(path)))
}

fn regex_of(path: &Path) -> String {
    regex::escape(&path.display().to_string()).replace('"', "\\\"")
}

/// The profile for `root` and `temp`, with no home folder and no `sandbox.json`.
pub fn seatbelt_profile(root: &Path, temp: &Path, secrets: &[PathBuf]) -> String {
    let mut paths = Paths::new(root, temp, None);
    paths.secrets = secrets.to_vec();
    seatbelt_profile_for(&paths)
}

/// Deny by default, then allow what builds, tests and Git need. The denials from the earlier
/// sandbox come last, because Seatbelt applies the last rule that matches: secrets stay
/// unreadable, Git metadata and the configuration folder stay unwritable, and no network,
/// terminal ioctl, Apple event or Launch Services lookup gets through.
pub fn seatbelt_profile_for(paths: &Paths) -> String {
    let (user_temp, user_cache) = user_folders();

    let mut reads: Vec<String> = SYSTEM_LINKS.iter().map(|p| literal(p)).collect();
    reads.extend(
        SYSTEM_READS
            .iter()
            .map(|p| subpath(Path::new(p))),
    );
    reads.push(subpath(&paths.root));
    reads.push(subpath(&paths.temp));
    reads.extend(git_folders(&paths.root).iter().map(|p| subpath(p)));
    reads.extend(user_cache.iter().map(|p| subpath(p)));
    if let Some(dir) = &user_temp {
        reads.push(format!("  (literal \"{}\")", quote(&dir.join("xcrun_db"))));
        reads.push(format!("  (regex #\"^{}/{}$\")", regex_of(dir), USER_TEMP_FILES));
    }
    if let Some(home) = &paths.home {
        reads.extend(HOME_READS.iter().map(|p| subpath(&home.join(p))));
        reads.extend(
            HOME_FILES
                .iter()
                .map(|p| format!("  (literal \"{}\")", quote(&home.join(p)))),
        );
        // `~/.gitconfig` and the files it includes, such as `~/.gitconfig-github`.
        reads.push(format!("  (regex #\"^{}/\\.gitconfig\")", regex_of(home)));
    }
    reads.extend(paths.extras.read.iter().map(|p| subpath(p)));

    let mut writes = vec![subpath(&paths.root), subpath(&paths.temp)];
    if let Some(dir) = &user_temp {
        writes.push(format!("  (regex #\"^{}/{}$\")", regex_of(dir), USER_TEMP_FILES));
    }
    // `ar` makes its temporary file directly in the user temp folder. Its subfolders, which
    // other programs own, stay unwritable.

    writes.extend(
        ["/dev/null", "/dev/zero", "/dev/dtracehelper"]
            .iter()
            .map(|p| literal(p)),
    );
    writes.push("  (regex #\"^/dev/fd/\")".to_string());
    writes.extend(paths.extras.write.iter().map(|p| subpath(p)));

    let services: Vec<String> = SERVICES
        .iter()
        .map(|name| format!("  (global-name \"{name}\")"))
        .collect();

    let mut denials = String::new();
    if !paths.secrets.is_empty() {
        let rules: Vec<String> = paths.secrets.iter().map(|p| subpath(p)).collect();
        denials.push_str(&format!("(deny file-read*\n{})\n", rules.join("\n")));
        denials.push_str(&format!("(deny file-write*\n{})\n", rules.join("\n")));
    }
    if let Some(config) = &paths.config {
        let rules: Vec<String> = given_and_real(config).iter().map(|p| subpath(p)).collect();
        denials.push_str(&format!("(deny file-write*\n{})\n", rules.join("\n")));
    }

    format!(
        r#"(version 1)
(deny default)
(allow process-exec process-fork)
(allow signal (target same-sandbox))
(allow process-info* (target same-sandbox))
(allow sysctl-read user-preference-read)
(allow file-read-metadata)
(allow ipc-posix-sem ipc-posix-shm)
(allow mach-lookup
{services})
(allow file-read*
{reads})
(allow file-write*
{writes})
(deny appleevent-send)
(deny mach-lookup (global-name "com.apple.coreservices.launchservicesd"))
(deny mach-lookup
{keychain})
(deny process-exec
  (literal "/usr/bin/open")
  (literal "/usr/bin/osascript")
  (literal "/usr/bin/osacompile")
  (literal "/bin/launchctl"))
{denials}(deny file-write-unlink
  (literal "{root}")
  (literal "{temp}"))
(deny network*)
{network}(deny file-ioctl (regex #"^/dev/tty"))
(deny file-write* (regex #"/\.git(/|$)"))
"#,
        services = services.join("\n"),
        keychain = KEYCHAIN_SERVICES
            .iter()
            .map(|name| format!("  (global-name \"{name}\")"))
            .collect::<Vec<_>>()
            .join("\n"),
        reads = reads.join("\n"),
        writes = writes.join("\n"),
        root = quote(&paths.root),
        temp = quote(&paths.temp),
        network = if paths.network { NETWORK } else { "" },
    )
}

/// The rules `network` adds, after `(deny network*)` so they win. Unix sockets other than the name
/// lookup daemon's stay denied, so Docker and an ssh-agent stay out of reach.
const NETWORK: &str = "(allow network-outbound (remote ip))
(allow network-inbound (local ip))
(allow network-bind (local ip))
(allow network-outbound (literal \"/private/var/run/mDNSResponder\"))
";

/// The line added to a command's output when the sandbox may have denied it.
pub const DENIED_HINT: &str = "[the sandbox may have denied this. For a lasting need, add the path to \
~/.config/ram-code/sandbox.json; for a one-time need, run the command with outside_sandbox and a reason]";

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
pub(crate) fn is_secret(name: &str, value: &str, key_envs: &[String]) -> bool {
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
