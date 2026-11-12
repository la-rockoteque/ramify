//! Ram Code finds its updates and installs them: a newer GitHub release for a release install,
//! new commits on `main` for a checkout.

use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How old a cached check may be before Ram Code checks again.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest a check may take, on the network or in `git fetch`.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
/// The longest the installer may run.
const INSTALL_TIMEOUT: Duration = Duration::from_secs(10 * 60);
/// `update.log` starts again past this size.
const LOG_LIMIT: u64 = 1024 * 1024;
const RELEASES: &str = "https://github.com/la-rockoteque/ramify/releases";
/// The commit of the last successful `ram update` build of a checkout.
const BUILT: &str = "target/release/.ram-built";

/// How this Ram Code is installed.
#[derive(Clone, Debug, PartialEq)]
pub enum Install {
    /// A release archive: the tag in its `RELEASE` file.
    Release {
        home: PathBuf,
        tag: String,
    },
    /// A git checkout, at this commit.
    Checkout {
        home: PathBuf,
        commit: String,
    },
    Unknown,
}

/// What a check found.
#[derive(Clone, Debug, PartialEq)]
pub enum Found {
    Release(String),
    Commits(usize),
    /// The checkout is on `origin/main`, but the binaries were not built from it.
    Rebuild,
}

impl Found {
    /// The header line.
    pub fn line(&self) -> String {
        match self {
            Found::Release(tag) => format!("update {tag} → ram update"),
            Found::Commits(1) => "update: 1 commit on main → ram update".into(),
            Found::Commits(count) => format!("update: {count} commits on main → ram update"),
            Found::Rebuild => "update: the build is older than main → ram update".into(),
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Found::Release(tag) => json!({"release": tag}),
            Found::Commits(count) => json!({"commits": count}),
            Found::Rebuild => json!({"rebuild": true}),
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        if let Some(tag) = value.get("release").and_then(Value::as_str) {
            return Some(Found::Release(tag.to_string()));
        }
        if value.get("rebuild").and_then(Value::as_bool) == Some(true) {
            return Some(Found::Rebuild);
        }
        value
            .get("commits")
            .and_then(Value::as_u64)
            .map(|count| Found::Commits(count as usize))
    }
}

/// The folder Ram Code is installed in: `RAM_CODE_HOME`, else the folder above
/// `target/<profile>/ram-code` when it holds Ram Code. A shared `CARGO_TARGET_DIR` puts the
/// binary elsewhere: then there is no install folder.
pub fn home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("RAM_CODE_HOME") {
        return Some(PathBuf::from(home));
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    let home = exe.parent()?.parent()?.parent()?.to_path_buf();
    is_ram_code(&home).then_some(home)
}

fn is_ram_code(home: &Path) -> bool {
    home.join("bin/ram").is_file()
        && std::fs::read_to_string(home.join("Cargo.toml")).is_ok_and(|text| {
            text.lines()
                .any(|line| line.replace(' ', "") == "name=\"ram-code\"")
        })
}

/// What kind of install `home` holds.
pub fn install_of(home: &Path) -> Install {
    if let Ok(text) = std::fs::read_to_string(home.join("RELEASE")) {
        let tag = text.trim().to_string();
        if !tag.is_empty() {
            return Install::Release {
                home: home.to_path_buf(),
                tag,
            };
        }
    }
    if home.join(".git").exists() {
        if let Some(commit) = git(home, &["rev-parse", "--short", "HEAD"]) {
            return Install::Checkout {
                home: home.to_path_buf(),
                commit,
            };
        }
    }
    Install::Unknown
}

impl Install {
    /// The version after the crate version: the tag, or `<branch>@<commit>` for a checkout.
    pub fn version(&self) -> Option<String> {
        match self {
            Install::Release { tag, .. } => Some(tag.clone()),
            Install::Checkout { home, commit } => {
                let branch = git(home, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
                Some(format!("{branch}@{commit}"))
            }
            Install::Unknown => None,
        }
    }

    fn home(&self) -> Option<&Path> {
        match self {
            Install::Release { home, .. } | Install::Checkout { home, .. } => Some(home),
            Install::Unknown => None,
        }
    }

    /// What identifies this version in the cache: a result for another version is stale.
    fn key(&self) -> String {
        match self {
            Install::Release { tag, .. } => tag.clone(),
            Install::Checkout { home, .. } => {
                let head = git(home, &["rev-parse", "HEAD"]).unwrap_or_default();
                format!("{head}#{}", built(home).unwrap_or_default())
            }
            Install::Unknown => String::new(),
        }
    }
}

/// Where the check and the log are kept: `RAM_CODE_CACHE_DIR`, else `~/.cache/ram-code`.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("RAM_CODE_CACHE_DIR") {
        return PathBuf::from(dir);
    }
    let base = std::env::var_os("XDG_CACHE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            PathBuf::from(std::env::var_os("HOME").unwrap_or_default()).join(".cache")
        });
    base.join("ram-code")
}

/// The releases URL: `RAMIFY_RELEASES`, as for the installer.
pub fn releases() -> String {
    std::env::var("RAMIFY_RELEASES").unwrap_or_else(|_| RELEASES.into())
}

/// Whether `RAM_CODE_NO_UPDATE` turns the check and auto-update off.
pub fn disabled() -> bool {
    std::env::var("RAM_CODE_NO_UPDATE").is_ok_and(|value| !value.is_empty() && value != "0")
}

/// Whether `settings.json` in the config folder turns auto-update on.
pub fn auto_update(config_dir: &Path) -> bool {
    std::fs::read_to_string(config_dir.join("settings.json"))
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.get("auto_update").and_then(Value::as_bool))
        .unwrap_or(false)
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_secs())
        .unwrap_or(0)
}

/// The update for `install`, from the cache when it is younger than a day. The cache keeps one
/// entry per install folder.
pub fn check(install: &Install, cache: &Path, releases: &str) -> Option<Found> {
    let home = install.home()?.display().to_string();
    let file = cache.join("update.json");
    let key = install.key();
    let mut entries = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok())
        .and_then(|value| value.as_object().cloned())
        .unwrap_or_default();
    if let Some(cached) = entries.get(&home) {
        let at = cached
            .get("checked_at")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let same = cached.get("key").and_then(Value::as_str) == Some(key.as_str());
        if same && now().saturating_sub(at) < CHECK_EVERY.as_secs() {
            return cached.get("found").and_then(Found::from_json);
        }
    }
    // A failed check is retried at the next start, and shows nothing.
    let found = look(install, releases).ok()?;
    entries.insert(
        home,
        json!({"checked_at": now(), "key": key, "found": found.as_ref().map(Found::to_json)}),
    );
    write_atomic(&file, &Value::Object(entries).to_string());
    found
}

/// Writes a temp file, then renames it: a reader never sees half a file.
fn write_atomic(file: &Path, text: &str) {
    let Some(dir) = file.parent() else {
        return;
    };
    let _ = std::fs::create_dir_all(dir);
    let temporary = dir.join(format!(".update.{}.json", std::process::id()));
    if std::fs::write(&temporary, text).is_ok() {
        let _ = std::fs::rename(&temporary, file);
    }
}

/// Asks the network or `git`, with no cache.
fn look(install: &Install, releases: &str) -> Result<Option<Found>, String> {
    match install {
        Install::Release { tag, .. } => {
            let latest = latest_tag(releases)?;
            Ok(newer(&latest, tag).then_some(Found::Release(latest)))
        }
        Install::Checkout { home, .. } => {
            // `ram update` moves only `main`: on another branch there is nothing to offer.
            if git(home, &["rev-parse", "--abbrev-ref", "HEAD"]).as_deref() != Some("main") {
                return Ok(None);
            }
            fetch(home)?;
            let behind = count(home, "main..origin/main")?;
            if behind > 0 {
                return Ok(Some(Found::Commits(behind)));
            }
            Ok(stale_build(home).then_some(Found::Rebuild))
        }
        Install::Unknown => Ok(None),
    }
}

/// The tag of the latest release. GitHub answers `/latest` with a redirect to `/tag/<tag>`.
fn latest_tag(releases: &str) -> Result<String, String> {
    let agent = ureq::Agent::new_with_config(
        ureq::Agent::config_builder()
            .http_status_as_error(false)
            .max_redirects(0)
            .timeout_global(Some(CHECK_TIMEOUT))
            .build(),
    );
    let url = format!("{}/latest", releases.trim_end_matches('/'));
    let response = agent.get(&url).call().map_err(|e| e.to_string())?;
    let location = response
        .headers()
        .get("location")
        .and_then(|value| value.to_str().ok())
        .ok_or_else(|| format!("{url} gave no redirect"))?;
    let tag = location
        .rsplit_once("/tag/")
        .map(|(_, tag)| tag.trim_end_matches('/'))
        .unwrap_or("");
    // The tag reaches the terminal and a command line: only plain characters.
    let plain = |c: char| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '+' | '-');
    if tag.is_empty() || tag.len() > 64 || !tag.chars().all(plain) {
        return Err(format!("{url} redirects to {location}, not to a tag"));
    }
    Ok(tag.to_string())
}

/// Whether `latest` is newer than `current`, as semantic versions: `v1.2` is `v1.2.0`, and a
/// pre-release (`v0.3.0-rc.1`) comes before its release. Tags that do not parse are newer when
/// they differ.
fn newer(latest: &str, current: &str) -> bool {
    fn parse(tag: &str) -> Option<(Vec<u64>, Option<String>)> {
        let tag = tag.trim_start_matches('v');
        let (numbers, pre) = match tag.split_once('-') {
            Some((numbers, pre)) => (numbers, Some(pre.to_string())),
            None => (tag, None),
        };
        let mut parts: Vec<u64> = numbers
            .split('.')
            .map(|part| part.parse().ok())
            .collect::<Option<_>>()?;
        if parts.is_empty() || parts.len() > 3 {
            return None;
        }
        parts.resize(3, 0);
        Some((parts, pre))
    }
    match (parse(latest), parse(current)) {
        (Some((a, a_pre)), Some((b, b_pre))) => match a.cmp(&b) {
            std::cmp::Ordering::Equal => match (a_pre, b_pre) {
                (None, Some(_)) => true,
                (Some(a_pre), Some(b_pre)) => a_pre > b_pre,
                _ => false,
            },
            order => order.is_gt(),
        },
        _ => latest != current,
    }
}

fn git(home: &Path, args: &[&str]) -> Option<String> {
    Command::new("git")
        .arg("-C")
        .arg(home)
        .args(args)
        .stderr(Stdio::null())
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn count(home: &Path, range: &str) -> Result<usize, String> {
    git(home, &["rev-list", "--count", range])
        .and_then(|text| text.parse().ok())
        .ok_or_else(|| format!("git rev-list {range} failed"))
}

/// The commit of the last `ram update` build. None when `ram update` never built here.
fn built(home: &Path) -> Option<String> {
    std::fs::read_to_string(home.join(BUILT))
        .ok()
        .map(|text| text.trim().to_string())
}

/// Whether the last `ram update` build is not HEAD: a build that failed half way.
fn stale_build(home: &Path) -> bool {
    built(home).is_some_and(|commit| Some(commit) != git(home, &["rev-parse", "HEAD"]))
}

/// Starts `command` in a new session: it has no terminal, so `ssh` cannot ask for a
/// passphrase or a host key over the interface, and a kill reaches its whole group.
fn detach(command: &mut Command) -> &mut Command {
    // SAFETY: setsid only changes the session of the child, between fork and exec.
    unsafe {
        command.pre_exec(|| {
            if libc::setsid() == -1 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(())
        })
    }
}

/// Waits for `child` up to `limit`, then kills its group.
fn wait_for(mut child: Child, limit: Duration, what: &str) -> Result<(), String> {
    let deadline = Instant::now() + limit;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(status)) => return Err(format!("{what} failed ({status})")),
            Ok(None) if Instant::now() >= deadline => {
                // SAFETY: killpg only sends a signal; the child leads its own group (detach).
                unsafe {
                    libc::killpg(child.id() as libc::pid_t, libc::SIGKILL);
                }
                let _ = child.wait();
                return Err(format!("{what} timed out"));
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// `git fetch origin main`, with no prompt, killed with its children after the check timeout.
fn fetch(home: &Path) -> Result<(), String> {
    let mut command = Command::new("git");
    command
        .arg("-C")
        .arg(home)
        .args(["fetch", "--quiet", "origin", "main"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    if std::env::var_os("GIT_SSH_COMMAND").is_none() {
        command.env("GIT_SSH_COMMAND", "ssh -o BatchMode=yes");
    }
    let child = detach(&mut command).spawn().map_err(|e| e.to_string())?;
    wait_for(child, CHECK_TIMEOUT, "git fetch")
}

/// Holds `update.lock` while `ram update` runs: two updates at once would mix their folders.
struct Lock {
    /// The lock ends when this file closes.
    _file: std::fs::File,
}

impl Lock {
    fn take(cache: &Path) -> Result<Self, String> {
        use std::os::fd::AsRawFd;
        let _ = std::fs::create_dir_all(cache);
        let file = std::fs::OpenOptions::new()
            .create(true)
            .truncate(false)
            .write(true)
            .open(cache.join("update.lock"))
            .map_err(|e| format!("could not open the update lock: {e}"))?;
        // SAFETY: flock on a file this process owns; the lock ends when the file closes.
        let taken = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } == 0;
        if taken {
            Ok(Lock { _file: file })
        } else {
            Err("another ram update is running".into())
        }
    }
}

/// `ram update`: installs the update and says what it did. Errors leave the install as it was.
pub fn run(install: &Install, releases: &str, cache: &Path) -> Result<String, String> {
    let _lock = Lock::take(cache)?;
    let done = match install {
        Install::Release { home, tag } => {
            let latest = latest_tag(releases)?;
            if !newer(&latest, tag) {
                return Ok(format!("ram is up to date ({tag})"));
            }
            update_release(home, releases, &latest)?;
            format!("ram updated from {tag} to {latest}")
        }
        Install::Checkout { home, commit } => update_checkout(home, commit)?,
        Install::Unknown => {
            return Err("ram cannot tell how it is installed: run install.sh again".into())
        }
    };
    // The next start must not offer the update that this run installed.
    let _ = std::fs::remove_file(cache.join("update.json"));
    Ok(done)
}

/// The folder of the `ram` link on PATH that points into `home`: the installer links there
/// again. None when no such link exists.
fn link_dir(home: &Path) -> Option<PathBuf> {
    let target = home.join("bin/ram").canonicalize().ok()?;
    std::env::split_paths(&std::env::var_os("PATH")?).find(|dir| {
        dir.join("ram")
            .canonicalize()
            .is_ok_and(|found| found == target)
    })
}

/// Runs the install folder's own installer for the tag the check found. bash keeps the script
/// open, so the installer may swap the folder that holds it.
fn update_release(home: &Path, releases: &str, latest: &str) -> Result<(), String> {
    let mut command = Command::new("bash");
    command
        .arg(home.join("install.sh"))
        .arg("--home")
        .arg(home)
        .arg("--version")
        .arg(latest)
        .env("RAMIFY_RELEASES", releases)
        .env_remove("RAMIFY_VERSION")
        .stdin(Stdio::null());
    if std::env::var_os("RAMIFY_BIN_DIR").is_none() {
        if let Some(dir) = link_dir(home) {
            command.arg("--bin-dir").arg(dir);
        }
    }
    let child = detach(&mut command)
        .spawn()
        .map_err(|e| format!("could not run the installer: {e}"))?;
    match wait_for(child, INSTALL_TIMEOUT, "the installer") {
        Ok(()) => Ok(()),
        Err(error) => {
            // The installer can fail after the swap, when it links: say what is installed.
            let installed = std::fs::read_to_string(home.join("RELEASE")).unwrap_or_default();
            if installed.trim() == latest {
                Err(format!(
                    "{error}: {latest} is installed, but its links may be missing; run install.sh"
                ))
            } else {
                Err(format!("{error}: the old release stays"))
            }
        }
    }
}

/// Fast-forwards a clean `main` to `origin/main`, then builds both binaries. A build that fails
/// is built again by the next `ram update`.
fn update_checkout(home: &Path, commit: &str) -> Result<String, String> {
    let branch = git(home, &["rev-parse", "--abbrev-ref", "HEAD"]).unwrap_or_default();
    if branch != "main" {
        return Err(format!(
            "{} is on {branch}, not on main: ram update moves only main",
            home.display()
        ));
    }
    let changes = git(home, &["status", "--porcelain", "--untracked-files=no"]).unwrap_or_default();
    if !changes.is_empty() {
        return Err(format!(
            "{} has uncommitted changes: commit or stash them first",
            home.display()
        ));
    }
    fetch(home)?;
    let ahead = count(home, "origin/main..main")?;
    if ahead > 0 {
        return Err(format!(
            "main has {ahead} commit(s) that origin/main does not have: ram update only fast-forwards"
        ));
    }
    let behind = count(home, "main..origin/main")?;
    if behind == 0 && !stale_build(home) {
        return Ok(format!("ram is up to date (main@{commit})"));
    }
    if behind > 0 {
        git(home, &["merge", "--quiet", "--ff-only", "origin/main"])
            .ok_or("git merge --ff-only origin/main failed")?;
    }
    let head = git(home, &["rev-parse", "HEAD"]).unwrap_or_default();
    // Before the build: a build that stops half way leaves a stamp that is not HEAD.
    let _ = std::fs::create_dir_all(home.join("target/release"));
    let _ = std::fs::write(home.join(BUILT), "building\n");
    for manifest in ["cli/Cargo.toml", "Cargo.toml"] {
        build(home, manifest)?;
    }
    let _ = std::fs::write(home.join(BUILT), format!("{head}\n"));
    // `bin/ram` runs a debug build that is newer than the release build: make the release newer.
    touch(&home.join("target/release/ram-code"));
    let now = git(home, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    Ok(format!("ram updated from main@{commit} to main@{now}"))
}

fn touch(path: &Path) {
    if let Ok(file) = std::fs::OpenOptions::new().write(true).open(path) {
        let _ = file.set_modified(SystemTime::now());
    }
}

/// `cargo build --release` of one manifest. In a debug build, `RAM_CODE_BUILD` replaces cargo
/// for the tests.
fn build(home: &Path, manifest: &str) -> Result<(), String> {
    let fake = cfg!(debug_assertions)
        .then(|| std::env::var("RAM_CODE_BUILD").ok())
        .flatten();
    let mut command = match fake {
        Some(script) => {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script).arg("ram-build").arg(manifest);
            command
        }
        None => {
            let mut command = Command::new("cargo");
            command
                .args(["build", "--release", "--quiet", "--manifest-path"])
                .arg(manifest);
            command
        }
    };
    let status = command
        .current_dir(home)
        .status()
        .map_err(|e| format!("could not build {manifest}: {e}"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "the build of {manifest} failed: the code is updated, the old binary stays; run ram update again"
        ))
    }
}

/// Starts `ram-code update` in its own session, with its output in `update.log`. Returns the
/// log. In a debug build, `RAM_CODE_UPDATER` replaces the program for the tests.
pub fn start_detached(cache: &Path) -> std::io::Result<PathBuf> {
    std::fs::create_dir_all(cache)?;
    let path = cache.join("update.log");
    if std::fs::metadata(&path).is_ok_and(|meta| meta.len() > LOG_LIMIT) {
        let _ = std::fs::remove_file(&path);
    }
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)?;
    let fake = cfg!(debug_assertions)
        .then(|| std::env::var_os("RAM_CODE_UPDATER"))
        .flatten();
    let program = match fake {
        Some(program) => PathBuf::from(program),
        None => std::env::current_exe()?,
    };
    detach(
        Command::new(program)
            .arg("update")
            .stdin(Stdio::null())
            .stdout(log.try_clone()?)
            .stderr(log),
    )
    .spawn()?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn versions_compare_as_semver() {
        assert!(newer("v0.3.0", "v0.2.9"));
        assert!(newer("v1.0.0", "v0.10.0"));
        assert!(!newer("v0.2.0", "v0.2.0"));
        assert!(!newer("v0.1.0", "v0.2.0"));
        assert!(!newer("v1.2", "v1.2.0"));
        assert!(newer("v0.3.0", "v0.3.0-rc.1"));
        assert!(!newer("v0.3.0-rc.1", "v0.3.0"));
        assert!(newer("v0.3.0-rc.2", "v0.3.0-rc.1"));
        assert!(newer("nightly-2", "nightly-1"));
    }
}
