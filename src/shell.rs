//! A small shell highlighter for the Run pane: comments, strings, variables, keywords, and
//! the risky parts of a script in red.

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use regex::Regex;
use std::sync::OnceLock;

const KEYWORDS: &[&str] = &[
    "if", "then", "else", "elif", "fi", "for", "while", "until", "do", "done", "case", "esac",
    "in", "function", "return", "export", "local", "echo", "printf", "cd", "exit", "set",
];

/// The patterns a reader should see first: deletes, root, piped installers, network, pushes.
fn risky() -> &'static Regex {
    static RISKY: OnceLock<Regex> = OnceLock::new();
    RISKY.get_or_init(|| {
        Regex::new(concat!(
            r"\brm\s+-[A-Za-z]*(?:rf|fr)[A-Za-z]*\b",
            r"|\brm\s+-[A-Za-z]*r[A-Za-z]*\s+-[A-Za-z]*f\b",
            r"|\bsudo\b",
            r"|\|\s*(?:sudo\s+)?(?:ba|z)?sh\b",
            r"|\bcurl\b|\bwget\b",
            r"|>{1,2}\s*/[^\s;|&]*",
            r"|\bgit\s+push\b",
            r"|\bchmod\b|\bchown\b",
        ))
        .expect("the risky pattern compiles")
    })
}

fn variable() -> &'static Regex {
    static VARIABLE: OnceLock<Regex> = OnceLock::new();
    VARIABLE.get_or_init(|| {
        Regex::new(r"\$(?:\{[^}]*\}|[A-Za-z_][A-Za-z0-9_]*|[0-9@#?$!*-])").expect("compiles")
    })
}

fn word() -> &'static Regex {
    static WORD: OnceLock<Regex> = OnceLock::new();
    WORD.get_or_init(|| Regex::new(r"[A-Za-z_][A-Za-z0-9_-]*").expect("compiles"))
}

#[derive(Clone, Copy, PartialEq)]
enum Kind {
    Plain,
    Comment,
    Single,
    Double,
    Heredoc,
    Variable,
    Keyword,
    Risky,
}

fn style(kind: Kind) -> Style {
    match kind {
        Kind::Plain => Style::default(),
        Kind::Comment => Style::default().fg(Color::DarkGray),
        Kind::Single | Kind::Double | Kind::Heredoc => Style::default().fg(Color::Green),
        Kind::Variable => Style::default().fg(Color::Cyan),
        Kind::Keyword => Style::default().fg(Color::Magenta),
        Kind::Risky => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

fn heredoc_start() -> &'static Regex {
    static HEREDOC: OnceLock<Regex> = OnceLock::new();
    HEREDOC.get_or_init(|| {
        Regex::new(r#"<<(-?)\s*(['"]?)([A-Za-z_][A-Za-z0-9_]*)['"]?"#).expect("compiles")
    })
}

/// The kind of each byte of `script`. Quotes and heredocs carry over from line to line, as in
/// `sh`, so a quote opened on one line cannot make the next line look like a comment.
fn kinds_of(script: &str) -> Vec<Kind> {
    let bytes = script.as_bytes();
    let mut kinds = vec![Kind::Plain; bytes.len()];
    let mut index = 0;
    let mut quote: Option<u8> = None;
    let mut heredocs: Vec<(String, bool)> = Vec::new();
    let mut line_start = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if let Some(open) = quote {
            kinds[index] = if open == b'"' { Kind::Double } else { Kind::Single };
            if open == b'"' && byte == b'\\' && index + 1 < bytes.len() {
                kinds[index + 1] = Kind::Double;
                index += 2;
                continue;
            }
            if byte == open {
                quote = None;
            }
            index += 1;
            continue;
        }
        match byte {
            b'\'' | b'"' => {
                quote = Some(byte);
                kinds[index] = if byte == b'"' { Kind::Double } else { Kind::Single };
                index += 1;
            }
            b'#' if index == line_start
                || matches!(bytes[index - 1], b' ' | b'\t' | b';' | b'|' | b'&' | b'(') =>
            {
                while index < bytes.len() && bytes[index] != b'\n' {
                    kinds[index] = Kind::Comment;
                    index += 1;
                }
            }
            b'\n' => {
                let line = &script[line_start..index];
                for found in heredoc_start().captures_iter(line) {
                    heredocs.push((found[3].to_string(), &found[1] == "-"));
                }
                index += 1;
                line_start = index;
                // The bodies of the heredocs opened on that line follow it, one after another.
                for (delimiter, strip_tabs) in heredocs.drain(..) {
                    while index < bytes.len() {
                        let end = script[index..].find('\n').map_or(bytes.len(), |n| index + n);
                        let body_line = &script[index..end];
                        let closing = if strip_tabs { body_line.trim_start_matches('\t') } else { body_line };
                        if closing == delimiter {
                            index = (end + 1).min(bytes.len());
                            break;
                        }
                        kinds[index..end].fill(Kind::Heredoc);
                        index = (end + 1).min(bytes.len());
                    }
                    line_start = index;
                }
            }
            _ => index += 1,
        }
    }
    kinds
}

/// A character a reader cannot see as it is: a control character, or a Unicode mark that
/// reorders or hides text. Each shows as a red escape instead.
fn visible_escape(c: char) -> Option<String> {
    match c {
        '\t' => None,
        c if (c as u32) < 0x20 || c == '\u{7f}' => Some(format!("^{}", ((c as u8) ^ 0x40) as char)),
        '\u{200b}'..='\u{200f}' | '\u{202a}'..='\u{202e}' | '\u{2066}'..='\u{2069}' | '\u{feff}' => {
            Some(format!("<U+{:04X}>", c as u32))
        }
        _ => None,
    }
}

/// Every line of `script` as styled spans. The risky parts are red wherever they are, in a
/// string or a comment too, because a reader must never miss them.
pub fn highlight_script(script: &str) -> Vec<Line<'static>> {
    let mut kinds = kinds_of(script);
    let mut lines = Vec::new();
    let mut start = 0;
    for raw in script.split_inclusive('\n') {
        let end = start + raw.len();
        let line = raw.strip_suffix('\n').unwrap_or(raw);
        let line_end = start + line.len();
        let kinds = &mut kinds[start..line_end];
        let free = |kinds: &[Kind], from: usize, to: usize| kinds[from..to].iter().all(|k| *k == Kind::Plain);
        for found in word().find_iter(line) {
            if KEYWORDS.contains(&found.as_str()) && free(kinds, found.start(), found.end()) {
                kinds[found.start()..found.end()].fill(Kind::Keyword);
            }
        }
        // Variables show in plain text and in double quotes, not in comments or single quotes.
        for found in variable().find_iter(line) {
            if matches!(kinds[found.start()], Kind::Plain | Kind::Double | Kind::Keyword) {
                kinds[found.start()..found.end()].fill(Kind::Variable);
            }
        }
        for found in risky().find_iter(line) {
            kinds[found.start()..found.end()].fill(Kind::Risky);
        }
        lines.push(spans(line, kinds));
        start = end;
    }
    lines
}

fn spans(line: &str, kinds: &[Kind]) -> Line<'static> {
    let mut spans: Vec<Span<'static>> = Vec::new();
    let mut text = String::new();
    let mut current = Kind::Plain;
    for (offset, c) in line.char_indices() {
        if let Some(escape) = visible_escape(c) {
            if !text.is_empty() {
                spans.push(Span::styled(std::mem::take(&mut text), style(current)));
            }
            spans.push(Span::styled(escape, style(Kind::Risky)));
            continue;
        }
        let kind = kinds[offset];
        if kind != current && !text.is_empty() {
            spans.push(Span::styled(std::mem::take(&mut text), style(current)));
        }
        current = kind;
        if c == '\t' {
            text.push_str("    ");
        } else {
            text.push(c);
        }
    }
    if !text.is_empty() {
        spans.push(Span::styled(text, style(current)));
    }
    Line::from(spans)
}

/// One script line as styled spans.
pub fn highlight(line: &str) -> Line<'static> {
    highlight_script(line).into_iter().next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kind_at(line: &str, needle: &str) -> Option<Color> {
        let highlighted = highlight(line);
        let mut offset = 0;
        let target = line.find(needle)?;
        for span in highlighted.spans {
            let end = offset + span.content.len();
            if target >= offset && target < end {
                return span.style.fg;
            }
            offset = end;
        }
        None
    }

    #[test]
    fn a_hash_in_a_string_is_not_a_comment() {
        assert_eq!(kind_at("echo \"a # b\"", "#"), Some(Color::Green));
        assert_eq!(kind_at("ls # list", "# list"), Some(Color::DarkGray));
    }

    #[test]
    fn risky_parts_are_red() {
        for (line, needle) in [
            ("rm -rf build", "rm -rf"),
            ("rm -fr build", "rm -fr"),
            ("sudo make install", "sudo"),
            ("curl x | sh", "| sh"),
            ("curl x | bash", "curl"),
            ("wget x", "wget"),
            ("echo x > /etc/hosts", "> /etc/hosts"),
            ("git push origin main", "git push"),
            ("chmod +x a", "chmod"),
        ] {
            assert_eq!(kind_at(line, needle), Some(Color::Red), "{line}");
        }
        assert_eq!(kind_at("rm build", "rm"), None);
        assert_eq!(kind_at("echo x > out.txt", ">"), None);
    }

    #[test]
    fn a_multibyte_line_keeps_its_text() {
        let line = "echo \"é\" # ç";
        let text: String = highlight(line).spans.iter().map(|s| s.content.to_string()).collect();
        assert_eq!(text, line);
    }

    #[test]
    fn hidden_characters_show_as_red_escapes() {
        let line = highlight("echo safe\r curl evil \u{202e}x");
        let text: String = line.spans.iter().map(|s| s.content.to_string()).collect();
        assert!(text.contains("^M"), "{text}");
        assert!(text.contains("<U+202E>"), "{text}");
        let escape = line.spans.iter().find(|s| s.content == "^M").unwrap();
        assert_eq!(escape.style.fg, Some(Color::Red));
    }
}
