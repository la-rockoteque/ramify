//! `edit_file`: replace an exact string in one file of the worktree.

use crate::search::read_utf8;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path};
use std::sync::atomic::{AtomicU64, Ordering};

static SEQ: AtomicU64 = AtomicU64::new(0);

/// Replace `old` with `new` in the file at `path`, which `Tools::resolve` already checked.
/// `relative` is the path the model gave, for the `.git` check and the output.
pub fn edit_file(
    path: &Path,
    relative: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, String> {
    if Path::new(relative)
        .components()
        .any(|c| matches!(c, Component::Normal(name) if name == ".git"))
    {
        return Err(format!("{relative} is inside .git, which is not edited"));
    }
    if old.is_empty() {
        return Err("old_string is empty; use write_file to create a file".into());
    }
    if old == new {
        return Err("old_string and new_string are the same; nothing to change".into());
    }
    let text = read_utf8(path)?;

    // A CRLF file gets CRLF in the strings, so an LF edit matches and keeps the file's endings.
    // A file with mixed endings is matched as it is first, and with CRLF only if that fails.
    let crlf = text.contains("\r\n");
    let bare_lf = text.replace("\r\n", "").contains('\n');
    let converted = (to_crlf(old), to_crlf(new));
    let (old, new) = if crlf && (!bare_lf || !text.contains(old)) {
        converted
    } else {
        (old.to_string(), new.to_string())
    };

    let starts: Vec<usize> = text.match_indices(&old).map(|(at, _)| at).collect();
    match starts.len() {
        0 => {
            return Err(format!(
                "old_string does not occur in {relative}. Read the file with read_file or find \
                 the text with grep, then copy it exactly."
            ))
        }
        1 => {}
        count if !replace_all => {
            let lines: Vec<String> = starts
                .iter()
                .map(|&at| line_of(&text, at).to_string())
                .collect();
            return Err(format!(
                "old_string occurs {count} times in {relative}, on lines {}. Add context so it \
                 occurs once, or set replace_all.",
                lines.join(", ")
            ));
        }
        _ => {}
    }

    let first_line = line_of(&text, starts[0]);
    let edited = if replace_all {
        text.replace(&old, &new)
    } else {
        text.replacen(&old, &new, 1)
    };
    write_over(path, &edited)?;

    let count = starts.len();
    if count > 1 {
        return Ok(format!("replaced {count} occurrences in {relative}"));
    }
    let new_lines = new.matches('\n').count();
    Ok(format!(
        "edited {relative}: lines {first_line}-{}",
        first_line + new_lines
    ))
}

fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// The 1-based line of a byte offset.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// Write a sibling file with the original mode, then rename it over the file, so a failed write
/// never leaves the file half written.
fn write_over(path: &Path, text: &str) -> Result<(), String> {
    let shown = path.display();
    let mode = std::fs::metadata(path)
        .map_err(|e| format!("could not read {shown}: {e}"))?
        .permissions()
        .mode();
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .ok_or_else(|| format!("{shown} has no file name"))?;
    let partial = path.with_file_name(format!(
        ".{name}.ram-edit-{}-{}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::SeqCst)
    ));
    let result = std::fs::write(&partial, text)
        .and_then(|_| std::fs::set_permissions(&partial, std::fs::Permissions::from_mode(mode)))
        .and_then(|_| std::fs::rename(&partial, path));
    if let Err(error) = result {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("could not write {shown}: {error}"));
    }
    Ok(())
}
