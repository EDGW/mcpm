//! Console confirmation before displaying an unusually large cache inventory.

use anyhow::{Context, Result};

use crate::{display::style::OutputStyle, terminal};

pub(super) fn reading(count: usize) -> Result<bool> {
    if count <= 80 || !terminal::stdout_is_tty() {
        return Ok(true);
    }
    // Read from the console rather than consuming a command's redirected stdin.
    let term = console::Term::stdout();
    let style = OutputStyle::new();
    loop {
        term.write_str(&format!(
            "{} [Y/n] ",
            style.warning(format!(
                "There are {count} cache entries. Do you want to read all of them?"
            ))
        ))
        .context("failed to write cache list confirmation")?;
        let answer = read_answer(&term).context("failed to read cache list confirmation")?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => term.write_line("Please enter Y or n.")?,
        }
    }
}

#[cfg(not(windows))]
fn read_answer(term: &console::Term) -> std::io::Result<String> {
    term.read_line()
}

/// Crossterm reads CONIN$ on Windows even when the process stdin is a pipe.
#[cfg(windows)]
fn read_answer(term: &console::Term) -> std::io::Result<String> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, read};

    let mut answer = String::new();
    loop {
        let Event::Key(key) = read()? else { continue };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        match key.code {
            KeyCode::Enter => {
                term.write_line("")?;
                return Ok(answer);
            }
            KeyCode::Char(ch) if ch.is_ascii_alphabetic() => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            KeyCode::Backspace if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
}
