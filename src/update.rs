//! Ram Code finds its updates and installs them: a newer GitHub release for a release install,
//! new commits on `main` for a checkout.

use serde_json::{json, Value};
use std::os::unix::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

/// How old a cached check may be before Ram Code checks again.
pub const CHECK_EVERY: Duration = Duration::from_secs(24 * 60 * 60);
/// The longest a check may take, on the network or in `git fetch`.
pub const CHECK_TIMEOUT: Duration = Duration::from_secs(10);
const RELEASES: &str = "https://github.com/la-rockoteque/ramify/releases";

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
}

impl Found {
    /// The header line.
    pub fn line(&self) -> String {
        match self {
            Found::Release(tag) => format!("update {tag} → ram update"),
            Found::Commits(1) => "update: 1 commit on main → ram update".into(),
            Found::Commits(count) => format!("update: {count} commits on main → ram update"),
        }
    }

    fn to_json(&self) -> Value {
        match self {
            Found::Release(tag) => json!({"release": tag}),
            Found::Commits(count) => json!({"commits": count}),
        }
    }

    fn from_json(value: &Value) -> Option<Self> {
        if let Some(tag) = value.get("release").and_then(Value::as_str) {
            return Some(Found::Release(tag.to_string()));
        }
        value
            .get("commits")
            .and_then(Value::as_u64)
            .map(|count| Found::Commits(count as usize))
    }
}

/// The folder Ram Code is installed in: `RAM_CODE_HOME`, else the folder above
/// `target/<profile>/ram-code`.
pub fn home() -> Option<PathBuf> {
    if let Some(home) = std::env::var_os("RAM_CODE_HOME") {
        return Some(PathBuf::from(home));
    }
    let exe = std::env::current_exe().ok()?.canonicalize().ok()?;
    Some(exe.parent()?.parent()?.parent()?.to_path_buf())
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
    /// The version after the crate version: the tag, or `main@<commit>` for a checkout.
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

    /// What identifies this version in the cache: a result for another version is stale.
    fn key(&self) -> String {
        match self {
            Install::Release { home, tag } => format!("{}#{tag}", home.display()),
            Install::Checkout { home, .. } => {
                let head = git(home, &["rev-parse", "HEAD"]).unwrap_or_default();
                format!("{}#{head}", home.display())
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

/// The update for `install`, from the cache when it is younger than a day.
pub fn check(install: &Install, cache: &Path, releases: &str) -> Option<Found> {
    if *install == Install::Unknown {
        return None;
    }
    let file = cache.join("update.json");
    let key = install.key();
    let cached = std::fs::read_to_string(&file)
        .ok()
        .and_then(|text| serde_json::from_str::<Value>(&text).ok());
    if let Some(cached) = cached {
        let at = cached
            .get("checked_at")
            .and_then(Value::as_u64)
            .unwrap_or(0);
        let same = cached.get("key").and_then(Value::as_str) == Some(key.as_str());
        if same && now().saturating_sub(at) < CHECK_EVERY.as_secs() {
            return cached.get("found").and_then(Found::from_json);
        }
    }
    let found = match look(install, releases) {
        Ok(found) => found,
        // A failed check is retried at the next start, and shows nothing.
        Err(_) => return None,
    };
    let record = json!({
        "checked_at": now(),
        "key": key,
        "found": found.as_ref().map(Found::to_json),
    });
    let _ = std::fs::create_dir_all(cache);
    let _ = std::fs::write(&file, record.to_string());
    found
}

/// Asks the network or `git`, with no cache.
fn look(install: &Install, releases: &str) -> Result<Option<Found>, String> {
    match install {
        Install::Release { tag, .. } => {
            let latest = latest_tag(releases)?;
            Ok(newer(&latest, tag).then_some(Found::Release(latest)))
        }
        Install::Checkout { home, .. } => {
            fetch(home)?;
            let behind = count(home, "main..origin/main")?;
            Ok((behind > 0).then_some(Found::Commits(behind)))
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
    location
        .rsplit_once("/tag/")
        .map(|(_, tag)| tag.trim_end_matches('/').to_string())
        .filter(|tag| !tag.is_empty())
        .ok_or_else(|| format!("{url} redirects to {location}, not to a tag"))
}

/// Whether `latest` is newer than `current`, by the numbers of `vX.Y.Z`. Tags that do not
/// parse are newer when they differ.
fn newer(latest: &str, current: &str) -> bool {
    let parse = |tag: &str| -> Option<Vec<u64>> {
        tag.trim_start_matches('v')
            .split(['.', '-'])
            .take(3)
            .map(|part| part.parse().ok())
            .collect()
    };
    match (parse(latest), parse(current)) {
        (Some(latest), Some(current)) => latest > current,
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

/// `git fetch origin main`, killed after the check timeout.
fn fetch(home: &Path) -> Result<(), String> {
    let mut child = Command::new("git")
        .arg("-C")
        .arg(home)
        .args(["fetch", "--quiet", "origin", "main"])
        .env("GIT_TERMINAL_PROMPT", "0")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let deadline = Instant::now() + CHECK_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Ok(()),
            Ok(Some(_)) => return Err("git fetch failed".into()),
            Ok(None) if Instant::now() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return Err("git fetch timed out".into());
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(error) => return Err(error.to_string()),
        }
    }
}

/// `ram update`: installs the update and says what it did. Errors leave the install as it was.
pub fn run(install: &Install, releases: &str) -> Result<String, String> {
    match install {
        Install::Release { home, tag } => {
            let latest = latest_tag(releases)?;
            if !newer(&latest, tag) {
                return Ok(format!("ram is up to date ({tag})"));
            }
            update_release(home, releases)?;
            Ok(format!("ram updated from {tag} to {latest}"))
        }
        Install::Checkout { home, commit } => update_checkout(home, commit),
        Install::Unknown => Err("ram cannot tell how it is installed: run install.sh again".into()),
    }
}

/// Runs the install folder's own installer, from a copy: it swaps the folder it lives in.
fn update_release(home: &Path, releases: &str) -> Result<(), String> {
    let copy = std::env::temp_dir().join(format!("ram-install-{}.sh", std::process::id()));
    std::fs::copy(home.join("install.sh"), &copy)
        .map_err(|e| format!("could not copy {}: {e}", home.join("install.sh").display()))?;
    let status = Command::new("bash")
        .arg(&copy)
        .arg("--home")
        .arg(home)
        .env("RAMIFY_RELEASES", releases)
        .status();
    let _ = std::fs::remove_file(&copy);
    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(format!(
            "the installer failed ({status}); the old release stays"
        )),
        Err(error) => Err(format!("could not run the installer: {error}")),
    }
}

/// Fast-forwards a clean `main` to `origin/main`, then builds both binaries.
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
    if count(home, "main..origin/main")? == 0 {
        return Ok(format!("ram is up to date (main@{commit})"));
    }
    git(home, &["merge", "--quiet", "--ff-only", "origin/main"])
        .ok_or("git merge --ff-only origin/main failed")?;
    for manifest in ["cli/Cargo.toml", "Cargo.toml"] {
        build(home, manifest)?;
    }
    let now = git(home, &["rev-parse", "--short", "HEAD"]).unwrap_or_default();
    Ok(format!("ram updated from main@{commit} to main@{now}"))
}

/// `cargo build --release` of one manifest. `RAM_CODE_BUILD` replaces cargo: tests set it.
fn build(home: &Path, manifest: &str) -> Result<(), String> {
    let mut command = match std::env::var("RAM_CODE_BUILD") {
        Ok(script) => {
            let mut command = Command::new("sh");
            command.arg("-c").arg(script).arg("ram-build").arg(manifest);
            command
        }
        Err(_) => {
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
            "the build of {manifest} failed: the code is updated, the old binary stays"
        ))
    }
}

/// Starts `ram-code update` on its own, with its output in `update.log`. `RAM_CODE_UPDATER`
/// replaces the program: tests set it.
pub fn start_detached(cache: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(cache)?;
    let log = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(cache.join("update.log"))?;
    let program = std::env::var_os("RAM_CODE_UPDATER")
        .map(PathBuf::from)
        .or_else(|| std::env::current_exe().ok())
        .ok_or_else(|| std::io::Error::other("no ram-code binary"))?;
    Command::new(program)
        .arg("update")
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()
        .map(|_| ())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_higher_number_is_newer() {
        assert!(newer("v0.3.0", "v0.2.9"));
        assert!(newer("v1.0.0", "v0.10.0"));
        assert!(!newer("v0.2.0", "v0.2.0"));
        assert!(!newer("v0.1.0", "v0.2.0"));
        assert!(newer("nightly-2", "nightly-1"));
    }
}
