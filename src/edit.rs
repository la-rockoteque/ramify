//! `edit_file`: replace an exact string in one file of the worktree.

use crate::search::read_utf8;
use crate::write::replace;
use std::path::Path;

/// The most line numbers an ambiguous-match error lists.
const MAX_LINES_LISTED: usize = 10;

/// Replace `old` with `new` in the file at `path`, which `write::target` returned.
/// `relative` is the path the model gave, for the messages.
pub fn edit_file(
    path: &Path,
    relative: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> Result<String, String> {
    if old.is_empty() {
        return Err("old_string is empty; use write_file to create a file".into());
    }
    if old == new {
        return Err("old_string and new_string are the same; nothing to change".into());
    }
    let text = read_utf8(path)?;

    // A CRLF file gets CRLF in old_string, so an LF edit matches. A file with mixed endings is
    // matched as it is first, and with CRLF only if that fails.
    let crlf = text.contains("\r\n");
    let bare_lf = text.replace("\r\n", "").contains('\n');
    let old = if crlf && (!bare_lf || !text.contains(old)) {
        to_crlf(old)
    } else {
        old.to_string()
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
            let listed: Vec<String> = starts
                .iter()
                .take(MAX_LINES_LISTED)
                .map(|&at| line_of(&text, at).to_string())
                .collect();
            let mut list = listed.join(", ");
            if count > listed.len() {
                list.push_str(&format!(", and {} more", count - listed.len()));
            }
            return Err(format!(
                "old_string occurs {count} times in {relative}, on lines {list}. Add context so \
                 it occurs once, or set replace_all."
            ));
        }
        _ => {}
    }

    // Each replacement takes the line ending of the line where its match starts, so new lines
    // in a CRLF part of the file get CRLF and new lines in an LF part get LF.
    let mut edited = String::with_capacity(text.len());
    let mut from = 0;
    let mut first_range = (0, 0);
    let mut last_range = (0, 0);
    for (index, &at) in starts.iter().enumerate() {
        edited.push_str(&text[from..at]);
        let piece = if line_is_crlf(&text, at) {
            to_crlf(new)
        } else {
            new.replace("\r\n", "\n")
        };
        let start_line = line_of(&edited, edited.len());
        let end_line = start_line + lines_in(&piece) - 1;
        if index == 0 {
            first_range = (start_line, end_line);
        }
        last_range = (start_line, end_line);
        edited.push_str(&piece);
        from = at + old.len();
    }
    edited.push_str(&text[from..]);
    replace(path, edited.as_bytes())?;

    if starts.len() > 1 {
        return Ok(format!(
            "replaced {} occurrences in {relative}, lines {}-{}",
            starts.len(),
            first_range.0,
            last_range.1
        ));
    }
    Ok(format!(
        "edited {relative}: lines {}-{}",
        first_range.0, first_range.1
    ))
}

fn to_crlf(text: &str) -> String {
    text.replace("\r\n", "\n").replace('\n', "\r\n")
}

/// The 1-based line of a byte offset.
fn line_of(text: &str, at: usize) -> usize {
    text[..at].matches('\n').count() + 1
}

/// True when the line that holds byte `at` ends in CRLF. A last line with no ending takes the
/// ending of the line before it.
fn line_is_crlf(text: &str, at: usize) -> bool {
    match text[at..].find('\n') {
        Some(offset) => text[..at + offset].ends_with('\r'),
        None => text[..at]
            .rfind('\n')
            .is_some_and(|end| text[..end].ends_with('\r')),
    }
}

/// The lines a replacement fills. A final line ending does not start another line.
fn lines_in(piece: &str) -> usize {
    let body = piece
        .strip_suffix("\r\n")
        .or_else(|| piece.strip_suffix('\n'))
        .unwrap_or(piece);
    body.matches('\n').count() + 1
}
