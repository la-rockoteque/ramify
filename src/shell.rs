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
    Text,
    Variable,
    Keyword,
    Risky,
}

fn style(kind: Kind) -> Style {
    match kind {
        Kind::Plain => Style::default(),
        Kind::Comment => Style::default().fg(Color::DarkGray),
        Kind::Text => Style::default().fg(Color::Green),
        Kind::Variable => Style::default().fg(Color::Cyan),
        Kind::Keyword => Style::default().fg(Color::Magenta),
        Kind::Risky => Style::default().fg(Color::Red).add_modifier(Modifier::BOLD),
    }
}

/// One script line as styled spans.
pub fn highlight(line: &str) -> Line<'static> {
    let mut kinds = vec![Kind::Plain; line.len()];
    let bytes = line.as_bytes();

    // Strings and comments, in one pass so that `#` inside a string is not a comment.
    let mut index = 0;
    while index < bytes.len() {
        match bytes[index] {
            b'\'' | b'"' => {
                let quote = bytes[index];
                let start = index;
                index += 1;
                while index < bytes.len() && bytes[index] != quote {
                    if quote == b'"' && bytes[index] == b'\\' {
                        index += 1;
                    }
                    index += 1;
                }
                let end = (index + 1).min(bytes.len());
                kinds[start..end].fill(Kind::Text);
                index = end;
            }
            b'#' if index == 0 || bytes[index - 1].is_ascii_whitespace() => {
                kinds[index..].fill(Kind::Comment);
                break;
            }
            _ => index += 1,
        }
    }
    let free = |kinds: &[Kind], start: usize, end: usize| kinds[start..end].iter().all(|k| *k == Kind::Plain);

    for found in word().find_iter(line) {
        if KEYWORDS.contains(&found.as_str()) && free(&kinds, found.start(), found.end()) {
            kinds[found.start()..found.end()].fill(Kind::Keyword);
        }
    }
    // Variables show inside double-quoted strings too, but not in comments or single quotes.
    for found in variable().find_iter(line) {
        let inside_single = line[..found.start()].matches('\'').count() % 2 == 1;
        if kinds[found.start()] != Kind::Comment && !inside_single {
            kinds[found.start()..found.end()].fill(Kind::Variable);
        }
    }
    for found in risky().find_iter(line) {
        if kinds[found.start()] != Kind::Comment {
            kinds[found.start()..found.end()].fill(Kind::Risky);
        }
    }

    let mut spans = Vec::new();
    let mut start = 0;
    for position in 1..=line.len() {
        let changes = position == line.len() || kinds[position] != kinds[start];
        if changes && line.is_char_boundary(start) && line.is_char_boundary(position) {
            spans.push(Span::styled(line[start..position].to_string(), style(kinds[start])));
            start = position;
        }
    }
    Line::from(spans)
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
}
