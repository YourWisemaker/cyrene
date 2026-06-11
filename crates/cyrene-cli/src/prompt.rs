//! Interactive line reader with a live slash-command menu.
//!
//! The chat REPL reads a line at a time. On a real terminal this reader runs in
//! raw mode so it can show a Claude-style command menu *beneath the prompt* the
//! moment the line starts with `/`, filtering as you type. The menu is drawn
//! only in the area below the input line and wiped on submit, so the welcome
//! card and earlier transcript above are never cleared.
//!
//! When there is no TTY (piped input, CI, `CYRENE_NO_MENU=1`) — or if raw mode
//! can't be enabled — it falls back to a plain buffered `read_line`, so scripts
//! and tests behave exactly as before.

use std::io::{stdout, IsTerminal, Stdout, Write};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    queue,
    style::Print,
    terminal::{self, disable_raw_mode, enable_raw_mode, Clear, ClearType},
};

use crate::slash;

/// The outcome of reading one line.
pub enum Read {
    /// A submitted line (without the trailing newline). May be empty.
    Line(String),
    /// End of input — Ctrl-D on an empty line, or a closed stdin.
    Eof,
}

/// Maximum command rows shown in the live menu before a "+N more" hint.
const MENU_MAX: usize = 8;

/// Reads one line, using the live menu on a TTY and a plain read otherwise.
pub fn read(prompt: &str) -> Read {
    let interactive = std::env::var_os("CYRENE_NO_MENU").is_none()
        && std::io::stdin().is_terminal()
        && stdout().is_terminal();
    if !interactive {
        return read_plain(prompt);
    }
    match read_interactive(prompt) {
        Ok(r) => r,
        Err(_) => {
            // Best-effort: never leave the terminal in raw mode on error.
            let _ = disable_raw_mode();
            read_plain(prompt)
        }
    }
}

/// Plain buffered read — the non-TTY / fallback path.
fn read_plain(prompt: &str) -> Read {
    print!("{prompt}");
    let _ = stdout().flush();
    let mut line = String::new();
    match std::io::stdin().read_line(&mut line) {
        Ok(0) => Read::Eof,
        Ok(_) => Read::Line(line.trim_end_matches(['\n', '\r']).to_owned()),
        Err(_) => Read::Eof,
    }
}

/// The command rows to show for the current buffer, or empty when the menu
/// should be hidden (not a `/`-line, an argument already typed, or dismissed).
fn menu(buf: &str, suppressed: bool) -> Vec<(String, &'static str)> {
    if suppressed {
        return Vec::new();
    }
    let Some(rest) = buf.strip_prefix('/') else {
        return Vec::new();
    };
    if rest.contains(char::is_whitespace) {
        return Vec::new();
    }
    slash::suggestions(rest)
        .into_iter()
        .map(|c| (format!("/{}", c.name), c.summary))
        .collect()
}

/// The raw-mode reader with the live menu.
fn read_interactive(prompt: &str) -> std::io::Result<Read> {
    enable_raw_mode()?;
    let mut out = stdout();
    let mut buf = String::new();
    let mut cursor = 0usize; // char index within buf
    let mut sel = 0usize; // selected menu row
    let mut suppressed = false; // menu hidden (via Esc) until the next edit

    render(&mut out, prompt, &buf, cursor, sel, suppressed)?;

    let result = loop {
        let Event::Key(key) = event::read()? else {
            continue;
        };
        // Windows emits key-release events too; act on press/repeat only.
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match (key.code, key.modifiers) {
            (KeyCode::Char('c'), KeyModifiers::CONTROL) => {
                queue!(out, Print("^C"))?;
                break Read::Line(String::new());
            }
            (KeyCode::Char('d'), KeyModifiers::CONTROL) if buf.is_empty() => {
                break Read::Eof;
            }
            (KeyCode::Char('u'), KeyModifiers::CONTROL) => {
                buf.clear();
                cursor = 0;
                sel = 0;
                suppressed = false;
            }
            (KeyCode::Enter, _) => break Read::Line(buf.clone()),
            (KeyCode::Tab, _) => {
                let rows = menu(&buf, suppressed);
                if let Some((label, _)) = rows.get(sel) {
                    buf = format!("{label} ");
                    cursor = buf.chars().count();
                }
            }
            (KeyCode::Esc, _) => suppressed = true,
            (KeyCode::Up, _) => sel = sel.saturating_sub(1),
            (KeyCode::Down, _) => sel = sel.saturating_add(1),
            (KeyCode::Left, _) => cursor = cursor.saturating_sub(1),
            (KeyCode::Right, _) => {
                if cursor < buf.chars().count() {
                    cursor += 1;
                }
            }
            (KeyCode::Home, _) => cursor = 0,
            (KeyCode::End, _) => cursor = buf.chars().count(),
            (KeyCode::Backspace, _) => {
                if cursor > 0 {
                    let idx = char_byte(&buf, cursor - 1);
                    buf.remove(idx);
                    cursor -= 1;
                    suppressed = false;
                    sel = 0;
                }
            }
            (KeyCode::Char(c), m) if m.is_empty() || m == KeyModifiers::SHIFT => {
                let idx = char_byte(&buf, cursor);
                buf.insert(idx, c);
                cursor += 1;
                suppressed = false;
                sel = 0;
            }
            _ => {}
        }
        // Clamp the selection to the (possibly changed) row count.
        let rows = menu(&buf, suppressed).len().min(MENU_MAX);
        if sel >= rows {
            sel = rows.saturating_sub(1);
        }
        render(&mut out, prompt, &buf, cursor, sel, suppressed)?;
    };

    finalize(&mut out, prompt, &buf)?;
    disable_raw_mode()?;
    Ok(result)
}

/// Redraws the input line plus the menu beneath it, leaving the terminal cursor
/// at the editing position. Only the area from the input line down is cleared,
/// so content above (the welcome card, transcript) is preserved.
fn render(
    out: &mut Stdout,
    prompt: &str,
    buf: &str,
    cursor: usize,
    sel: usize,
    suppressed: bool,
) -> std::io::Result<()> {
    let width = terminal::size().map(|(c, _)| c as usize).unwrap_or(80);
    let rows = menu(buf, suppressed);

    queue!(
        out,
        cursor::MoveToColumn(0),
        Clear(ClearType::FromCursorDown),
        Print(prompt),
        Print(buf),
    )?;

    let mut printed = 0u16;
    for (i, (label, summary)) in rows.iter().take(MENU_MAX).enumerate() {
        let marker = if i == sel { '>' } else { ' ' };
        let row = format!("  {marker} {label:<16} {summary}");
        queue!(
            out,
            Print("\r\n"),
            Print(truncate(&row, width.saturating_sub(1)))
        )?;
        printed += 1;
    }
    if rows.len() > MENU_MAX {
        let more = format!(
            "    … +{} more (keep typing to filter)",
            rows.len() - MENU_MAX
        );
        queue!(
            out,
            Print("\r\n"),
            Print(truncate(&more, width.saturating_sub(1)))
        )?;
        printed += 1;
    }

    // Move back up to the input line and place the cursor at the edit column.
    if printed > 0 {
        queue!(out, cursor::MoveToPreviousLine(printed))?;
    } else {
        queue!(out, cursor::MoveToColumn(0))?;
    }
    let col = prompt.chars().count() + cursor;
    queue!(out, cursor::MoveToColumn(col as u16))?;
    out.flush()
}

/// Clears the menu, leaves the submitted line on screen, and drops to the next
/// line so the conversation continues normally below it.
fn finalize(out: &mut Stdout, prompt: &str, buf: &str) -> std::io::Result<()> {
    queue!(
        out,
        cursor::MoveToColumn(0),
        Clear(ClearType::FromCursorDown),
        Print(prompt),
        Print(buf),
        Print("\r\n"),
    )?;
    out.flush()
}

/// Truncates `s` to at most `max` characters (display columns for ASCII).
fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        s.chars().take(max).collect()
    }
}

/// Byte offset of the `idx`-th char (or the end of the string).
fn char_byte(s: &str, idx: usize) -> usize {
    s.char_indices().nth(idx).map(|(b, _)| b).unwrap_or(s.len())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_only_for_slash_prefix_without_args() {
        assert!(menu("", false).is_empty());
        assert!(menu("hello", false).is_empty());
        assert!(menu("/model x", false).is_empty()); // arg already typed
        assert!(menu("/", false).len() >= 5); // bare slash lists commands
        assert!(menu("/mo", false).iter().any(|(n, _)| n == "/model"));
    }

    #[test]
    fn esc_suppresses_menu() {
        assert!(menu("/mo", true).is_empty());
    }

    #[test]
    fn truncate_caps_width() {
        assert_eq!(truncate("hello", 3), "hel");
        assert_eq!(truncate("hi", 5), "hi");
    }

    #[test]
    fn char_byte_handles_multibyte() {
        assert_eq!(char_byte("a√b", 0), 0);
        assert_eq!(char_byte("a√b", 2), 4); // '√' is 3 bytes
        assert_eq!(char_byte("ab", 5), 2); // past end → len
    }
}
