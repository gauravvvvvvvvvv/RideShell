//! Terminal output helpers. Colour is emitted through `anstream`, which
//! strips it when output is not a terminal or `NO_COLOR` is set, and
//! translates it for legacy Windows consoles.

use std::io::{BufRead, IsTerminal, Write};

use anstyle::{Ansi256Color, AnsiColor, Color, Style};

pub const ACCENT: Style = Style::new().fg_color(Some(Color::Ansi256(Ansi256Color(111)))).bold();
pub const DIM: Style = Style::new().fg_color(Some(Color::Ansi256(Ansi256Color(245))));
pub const OK: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Green)));
pub const WARN: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Yellow)));
pub const ERR: Style = Style::new().fg_color(Some(Color::Ansi(AnsiColor::Red))).bold();
pub const BOLD: Style = Style::new().bold();

pub fn paint(text: impl AsRef<str>, style: Style) -> String {
    format!("{}{}{}", style.render(), text.as_ref(), style.render_reset())
}

pub fn accent(text: impl AsRef<str>) -> String {
    paint(text, ACCENT)
}

pub fn dim(text: impl AsRef<str>) -> String {
    paint(text, DIM)
}

/// Unicode glyphs only where the terminal is known to render them.
fn fancy() -> bool {
    !cfg!(windows)
        || std::env::var_os("WT_SESSION").is_some()
        || std::env::var("TERM_PROGRAM").is_ok_and(|t| t == "vscode" || t == "RideShell")
}

pub fn success(msg: impl AsRef<str>) {
    let mark = if fancy() { "✓" } else { "+" };
    anstream::println!("{} {}", paint(mark, OK), msg.as_ref());
}

pub fn warn(msg: impl AsRef<str>) {
    anstream::eprintln!("{} {}", paint("warning:", WARN), msg.as_ref());
}

pub fn error(msg: impl AsRef<str>) {
    anstream::eprintln!("{} {}", paint("error:", ERR), msg.as_ref());
}

pub fn hint(msg: impl AsRef<str>) {
    anstream::println!("  {}", dim(msg));
}

/// Width of `s` as displayed, ignoring ANSI escape sequences.
pub fn visible_width(s: &str) -> usize {
    let mut width = 0;
    let mut in_escape = false;
    for c in s.chars() {
        if in_escape {
            if c.is_ascii_alphabetic() {
                in_escape = false;
            }
        } else if c == '\x1b' {
            in_escape = true;
        } else {
            width += 1;
        }
    }
    width
}

/// Print rows as aligned columns (two spaces between columns).
pub fn table(rows: &[Vec<String>]) {
    let cols = rows.iter().map(Vec::len).max().unwrap_or(0);
    let mut widths = vec![0; cols];
    for row in rows {
        for (i, cell) in row.iter().enumerate() {
            widths[i] = widths[i].max(visible_width(cell));
        }
    }
    for row in rows {
        let mut line = String::new();
        for (i, cell) in row.iter().enumerate() {
            line.push_str(cell);
            if i + 1 < row.len() {
                line.push_str(&" ".repeat(widths[i] - visible_width(cell) + 2));
            }
        }
        anstream::println!("{}", line.trim_end());
    }
}

/// A titled box of label/value rows, e.g. for `status`.
pub fn panel(title: &str, rows: &[(String, String)]) {
    let label_w = rows.iter().map(|(l, _)| visible_width(l)).max().unwrap_or(0);
    let lines: Vec<String> =
        rows.iter().map(|(l, v)| format!("{}{}  {}", dim(l), " ".repeat(label_w - visible_width(l)), v)).collect();
    let inner = lines.iter().map(|l| visible_width(l)).max().unwrap_or(0).max(visible_width(title) + 2);
    let (tl, tr, bl, br, h, v) =
        if fancy() { ("╭", "╮", "╰", "╯", "─", "│") } else { ("+", "+", "+", "+", "-", "|") };
    anstream::println!(
        "{}",
        dim(format!("{tl}{h} "))
            + &accent(title)
            + &dim(format!(" {}{tr}", h.repeat(inner + 1 - visible_width(title) - 2)))
    );
    for line in &lines {
        anstream::println!("{} {}{} {}", dim(v), line, " ".repeat(inner - visible_width(line)), dim(v));
    }
    anstream::println!("{}", dim(format!("{bl}{}{br}", h.repeat(inner + 2))));
}

pub fn stdin_is_interactive() -> bool {
    std::io::stdin().is_terminal()
}

/// Ask a yes/no question on stderr. Non-interactive sessions get `default`.
pub fn confirm(question: &str, default: bool) -> bool {
    if !stdin_is_interactive() {
        return default;
    }
    let suffix = if default { "[Y/n]" } else { "[y/N]" };
    anstream::eprint!("{} {} ", question, dim(suffix));
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return default;
    }
    match answer.trim().to_ascii_lowercase().as_str() {
        "" => default,
        "y" | "yes" => true,
        _ => false,
    }
}

/// Ask for a line of text with a default.
pub fn ask(question: &str, default: &str) -> String {
    if !stdin_is_interactive() {
        return default.to_string();
    }
    anstream::eprint!("{} {} ", question, dim(format!("[{default}]")));
    let _ = std::io::stderr().flush();
    let mut answer = String::new();
    if std::io::stdin().lock().read_line(&mut answer).is_err() {
        return default.to_string();
    }
    let answer = answer.trim();
    if answer.is_empty() {
        default.to_string()
    } else {
        answer.to_string()
    }
}

pub fn fmt_time(unix: i64) -> String {
    // Days since epoch -> civil date (UTC). Avoids a date-time dependency.
    let days = unix.div_euclid(86_400);
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates() {
        assert_eq!(fmt_time(0), "1970-01-01");
        assert_eq!(fmt_time(1_763_510_400), "2025-11-19");
    }

    #[test]
    fn width_ignores_escapes() {
        assert_eq!(visible_width(&accent("work")), 4);
    }
}
