//! The one write path for `write_file` and `edit_file`: find the real target, refuse Git
//! metadata, and replace the file through a new temp file and a rename.

use std::collections::hash_map::RandomState;
use std::ffi::OsString;
use std::hash::{BuildHasher, Hasher};
use std::io::Write;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// The real path of `joined`: the nearest existing ancestor by its canonical path, then the parts
/// that do not exist yet. It fails when that path leaves `root`, which is already canonical.
pub fn real_path(root: &Path, joined: &Path) -> Result<PathBuf, String> {
    let mut probe = joined.to_path_buf();
    let mut suffix: Vec<OsString> = Vec::new();
    loop {
        if let Ok(canonical) = probe.canonicalize() {
            let mut full = canonical;
            for part in suffix.iter().rev() {
                full.push(part);
            }
            if full.starts_with(root) {
                return Ok(full);
            }
            return Err("path must stay inside the worktree".into());
        }
        match (probe.parent(), probe.file_name()) {
            (Some(parent), Some(name)) => {
                suffix.push(name.to_os_string());
                probe = parent.to_path_buf();
            }
            _ => return Err("path must stay inside the worktree".into()),
        }
    }
}

/// A `.git` component, in any case: APFS matches names without case, so `.GIT` is `.git` to Git.
fn is_git(name: &std::ffi::OsStr) -> bool {
    name.to_str().is_some_and(|n| n.eq_ignore_ascii_case(".git"))
}

/// The file a write changes. `relative` is the path the model gave, already checked for `..`
/// and absolute paths.
///
/// Each existing part is followed one at a time. A symlink must resolve, stay inside `root` and
/// not lead into Git metadata. A dangling symlink is refused, because a write would create its
/// target wherever it points. An existing file with more than one hard link is refused, because
/// one of its other names may be in `.git`.
pub fn target(root: &Path, relative: &str) -> Result<PathBuf, String> {
    let root = root
        .canonicalize()
        .map_err(|e| format!("could not resolve the worktree: {e}"))?;
    let refuse_git = |path: &Path| -> Result<(), String> {
        let inside = path
            .strip_prefix(&root)
            .map_err(|_| "path must stay inside the worktree".to_string())?;
        if inside.components().any(|c| matches!(c, Component::Normal(n) if is_git(n))) {
            return Err(format!("{relative} is Git metadata, which Ram Code does not write"));
        }
        Ok(())
    };

    let parts: Vec<&std::ffi::OsStr> = Path::new(relative)
        .components()
        .filter_map(|c| match c {
            Component::Normal(name) => Some(name),
            _ => None,
        })
        .collect();
    if parts.is_empty() {
        return Err("write needs a file path".into());
    }
    let mut current = root.clone();
    let mut missing = false;
    for part in parts {
        let next = current.join(part);
        if missing {
            current = next;
            continue;
        }
        match std::fs::symlink_metadata(&next) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                missing = true;
                current = next;
            }
            Err(error) => return Err(format!("could not read {relative}: {error}")),
            Ok(meta) if meta.file_type().is_symlink() => {
                let resolved = next.canonicalize().map_err(|_| {
                    format!("{relative} goes through a symlink whose target does not exist")
                })?;
                if !resolved.starts_with(&root) {
                    return Err("path must stay inside the worktree".into());
                }
                refuse_git(&resolved)?;
                current = resolved;
            }
            Ok(_) => current = next,
        }
        refuse_git(&current)?;
    }
    if let Ok(meta) = std::fs::symlink_metadata(&current) {
        if meta.is_dir() {
            return Err(format!("{relative} is a folder"));
        }
        if meta.nlink() > 1 {
            return Err(format!(
                "{relative} has other hard links, so Ram Code does not write it"
            ));
        }
    }
    Ok(current)
}

/// A temp name nobody can guess, so a command cannot plant a symlink there first.
fn temp_name(name: &str) -> String {
    let mut hasher = RandomState::new().build_hasher();
    hasher.write_u64(SEQ.fetch_add(1, Ordering::SeqCst));
    hasher.write_u32(std::process::id());
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    hasher.write_u128(nanos);
    format!(".{name}.ram-{:016x}", hasher.finish())
}

/// Replace `path`, which `target` returned, with `content`. An existing file keeps its mode.
///
/// The temp file is new (`O_EXCL`) and never a symlink (`O_NOFOLLOW`), and the rename swaps the
/// name, so a failed write leaves the old file whole.
pub fn replace(path: &Path, content: &[u8]) -> Result<(), String> {
    let shown = path.display();
    let parent = path
        .parent()
        .ok_or_else(|| format!("{shown} has no folder"))?;
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{shown} has no file name"))?;
    let mode = std::fs::metadata(path).ok().map(|m| m.permissions().mode());
    std::fs::create_dir_all(parent)
        .map_err(|e| format!("could not create {}: {e}", parent.display()))?;
    let partial = parent.join(temp_name(name));
    let written = (|| -> std::io::Result<()> {
        let mut file = std::fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(libc::O_NOFOLLOW)
            .mode(0o666)
            .open(&partial)?;
        if let Some(mode) = mode {
            file.set_permissions(std::fs::Permissions::from_mode(mode))?;
        }
        file.write_all(content)?;
        file.sync_all()?;
        std::fs::rename(&partial, path)
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("could not write {shown}: {error}"));
    }
    Ok(())
}
