//! GitHub CLI (`gh`) style table formatter and terminal styling helpers.
//!
//! Features:
//! - Uppercase, bold headers with customizable alignment
//! - Width-aware padding using unicode display width (handles CJK and emoji properly)
//! - Clean spacing without ASCII table border lines
//! - Seamless fallback to clean tab-separated values (TSV) when stdout is not a TTY
//! - GH-style feedback helpers: `✓` success, `!` warning, `X` error

use std::io::IsTerminal;
use unicode_width::UnicodeWidthStr;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Right,
}

#[derive(Clone, Debug)]
pub struct Cell {
    pub display: String,
    pub raw: String,
    pub align: Align,
}

impl Cell {
    pub fn new(raw: impl Into<String>) -> Self {
        let raw = raw.into();
        Self {
            display: raw.clone(),
            raw,
            align: Align::Left,
        }
    }

    pub fn styled(display: impl Into<String>, raw: impl Into<String>) -> Self {
        Self {
            display: display.into(),
            raw: raw.into(),
            align: Align::Left,
        }
    }

    pub fn right(mut self) -> Self {
        self.align = Align::Right;
        self
    }
}

pub struct Header {
    pub title: String,
    pub align: Align,
}

pub struct Table {
    headers: Vec<Header>,
    rows: Vec<Vec<Cell>>,
}

impl Table {
    pub fn new(headers: Vec<(&str, Align)>) -> Self {
        Self {
            headers: headers
                .into_iter()
                .map(|(t, a)| Header {
                    title: t.to_uppercase(),
                    align: a,
                })
                .collect(),
            rows: Vec::new(),
        }
    }

    pub fn add_row(&mut self, row: Vec<Cell>) {
        self.rows.push(row);
    }

    #[allow(dead_code)]
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn render(&self) -> String {
        let is_tty = std::io::stdout().is_terminal();
        self.render_with_tty(is_tty)
    }

    pub fn render_with_tty(&self, is_tty: bool) -> String {
        if !is_tty {
            // When piped or redirected, GH CLI outputs raw tab-separated values with no header
            let mut out = String::new();
            for row in &self.rows {
                let line: Vec<&str> = row.iter().map(|c| c.raw.as_str()).collect();
                out.push_str(&line.join("\t"));
                out.push('\n');
            }
            return out;
        }

        if self.rows.is_empty() {
            return String::new();
        }

        let num_cols = self.headers.len();
        let mut col_widths = vec![0; num_cols];

        for (i, h) in self.headers.iter().enumerate() {
            col_widths[i] = UnicodeWidthStr::width(h.title.as_str());
        }

        for row in &self.rows {
            for (i, cell) in row.iter().enumerate() {
                if i < num_cols {
                    let w = UnicodeWidthStr::width(cell.raw.as_str());
                    if w > col_widths[i] {
                        col_widths[i] = w;
                    }
                }
            }
        }

        let mut out = String::new();

        // Header: bold
        let mut header_parts = Vec::new();
        for (i, h) in self.headers.iter().enumerate() {
            let w = col_widths[i];
            let title_w = UnicodeWidthStr::width(h.title.as_str());
            let pad = w.saturating_sub(title_w);
            let (display, is_last) = (&h.title, i == num_cols - 1);
            let cell_str = match h.align {
                Align::Left => {
                    if is_last {
                        display.to_string()
                    } else {
                        format!("{}{}", display, " ".repeat(pad))
                    }
                }
                Align::Right => format!("{}{}", " ".repeat(pad), display),
            };
            header_parts.push(cell_str);
        }
        out.push_str(&format!("\x1b[1m{}\x1b[0m\n", header_parts.join("  ")));

        // Rows
        for row in &self.rows {
            let mut row_parts = Vec::new();
            for (i, cell) in row.iter().enumerate() {
                if i >= num_cols {
                    break;
                }
                let w = col_widths[i];
                let cell_w = UnicodeWidthStr::width(cell.raw.as_str());
                let pad = w.saturating_sub(cell_w);
                let (display, is_last) = (&cell.display, i == num_cols - 1);

                let cell_str = match cell.align {
                    Align::Left => {
                        if is_last {
                            display.to_string()
                        } else {
                            format!("{}{}", display, " ".repeat(pad))
                        }
                    }
                    Align::Right => format!("{}{}", " ".repeat(pad), display),
                };
                row_parts.push(cell_str);
            }
            out.push_str(&row_parts.join("  "));
            out.push('\n');
        }

        out
    }
}

pub fn success(msg: &str) {
    if std::io::stdout().is_terminal() {
        println!("\x1b[32m✓\x1b[0m {msg}");
    } else {
        println!("✓ {msg}");
    }
}

#[allow(dead_code)]
pub fn warn(msg: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("\x1b[33m!\x1b[0m {msg}");
    } else {
        eprintln!("! {msg}");
    }
}

#[allow(dead_code)]
pub fn error(msg: &str) {
    if std::io::stderr().is_terminal() {
        eprintln!("\x1b[31mX\x1b[0m {msg}");
    } else {
        eprintln!("X {msg}");
    }
}

pub fn format_relative_time(epoch_secs: i64) -> (String, String) {
    if epoch_secs <= 0 {
        return ("never".to_string(), "never".to_string());
    }
    let now = chrono::Utc::now().timestamp();
    let diff = now.saturating_sub(epoch_secs);
    let raw = match chrono::DateTime::from_timestamp(epoch_secs, 0) {
        Some(dt) => dt.format("%Y-%m-%dT%H:%M:%SZ").to_string(),
        None => format!("{epoch_secs}"),
    };

    let human = if diff < 60 {
        "just now".to_string()
    } else if diff < 120 {
        "about 1 minute ago".to_string()
    } else if diff < 3600 {
        format!("about {} minutes ago", diff / 60)
    } else if diff < 7200 {
        "about 1 hour ago".to_string()
    } else if diff < 86400 {
        format!("about {} hours ago", diff / 3600)
    } else if diff < 172800 {
        "about 1 day ago".to_string()
    } else if diff < 30 * 86400 {
        format!("about {} days ago", diff / 86400)
    } else if diff < 60 * 86400 {
        "about 1 month ago".to_string()
    } else {
        format!("about {} months ago", diff / (30 * 86400))
    };

    let is_tty = std::io::stdout().is_terminal();
    let display = if is_tty {
        format!("\x1b[2m{human}\x1b[0m")
    } else {
        raw.clone()
    };

    (display, raw)
}

pub fn format_delay(delay: Option<u32>) -> (String, String) {
    let is_tty = std::io::stdout().is_terminal();
    match delay {
        Some(0) => {
            let raw = "timeout".to_string();
            let display = if is_tty {
                format!("\x1b[31m{raw}\x1b[0m")
            } else {
                raw.clone()
            };
            (display, raw)
        }
        Some(ms) => {
            let raw = format!("{ms}ms");
            let display = if is_tty {
                if ms <= 150 {
                    format!("\x1b[32m{raw}\x1b[0m")
                } else if ms <= 400 {
                    format!("\x1b[33m{raw}\x1b[0m")
                } else {
                    format!("\x1b[31m{raw}\x1b[0m")
                }
            } else {
                raw.clone()
            };
            (display, raw)
        }
        None => {
            let raw = "-".to_string();
            let display = if is_tty {
                format!("\x1b[2m{raw}\x1b[0m")
            } else {
                raw.clone()
            };
            (display, raw)
        }
    }
}

pub fn active_indicator(active: bool) -> (String, String) {
    let is_tty = std::io::stdout().is_terminal();
    if active {
        let raw = "*".to_string();
        let display = if is_tty {
            format!("\x1b[1;32m{raw}\x1b[0m")
        } else {
            raw.clone()
        };
        (display, raw)
    } else {
        ("".to_string(), "".to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_renders_tsv_when_not_tty() {
        let mut table = Table::new(vec![("Name", Align::Left), ("Delay", Align::Right)]);
        table.add_row(vec![Cell::new("Node A"), Cell::new("45ms").right()]);
        table.add_row(vec![Cell::new("Node B"), Cell::new("120ms").right()]);

        let output = table.render_with_tty(false);
        assert_eq!(output, "Node A\t45ms\nNode B\t120ms\n");
    }

    #[test]
    fn table_renders_aligned_gh_style_when_tty() {
        let mut table = Table::new(vec![("Name", Align::Left), ("Delay", Align::Right)]);
        table.add_row(vec![Cell::new("Node A"), Cell::new("45ms").right()]);
        table.add_row(vec![Cell::new("HongKong Fast"), Cell::new("120ms").right()]);

        let output = table.render_with_tty(true);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);
        assert!(lines[0].contains("NAME"));
        assert!(lines[0].contains("DELAY"));
        assert!(lines[1].contains("Node A"));
        assert!(lines[1].ends_with("45ms"));
    }

    #[test]
    fn table_cjk_unicode_width_alignment() {
        let mut table = Table::new(vec![("Name", Align::Left), ("Type", Align::Left)]);
        table.add_row(vec![Cell::new("香港高速"), Cell::new("Shadowsocks")]);
        table.add_row(vec![Cell::new("US-Node"), Cell::new("Vmess")]);

        let output = table.render_with_tty(true);
        let lines: Vec<&str> = output.lines().collect();
        assert_eq!(lines.len(), 3);
    }

    #[test]
    fn format_relative_time_formats_seconds() {
        let now = chrono::Utc::now().timestamp();
        let (_disp, raw) = format_relative_time(0);
        assert_eq!(raw, "never");

        let (_disp, raw) = format_relative_time(now - 10);
        assert!(!raw.is_empty());
    }

    #[test]
    fn format_delay_formats_thresholds() {
        let (_disp, raw) = format_delay(None);
        assert_eq!(raw, "-");

        let (_disp, raw) = format_delay(Some(0));
        assert_eq!(raw, "timeout");

        let (_disp, raw) = format_delay(Some(85));
        assert_eq!(raw, "85ms");
    }
}
