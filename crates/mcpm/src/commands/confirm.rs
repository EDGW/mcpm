//! Console confirmations for large inventories and destructive cache removal.

use anyhow::{Context, Result};

use crate::{display::style::OutputStyle, terminal};

pub(in crate::commands) fn reading(count: usize, noun: &str) -> Result<bool> {
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
                "There are {count} {noun}. Do you want to read all of them?"
            ))
        ))
        .context("failed to write list confirmation")?;
        let answer = read_answer(&term, false).context("failed to read list confirmation")?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "" | "y" | "yes" => return Ok(true),
            "n" | "no" => return Ok(false),
            _ => term.write_line("Please enter Y or n.")?,
        }
    }
}

/// Confirms a frozen remove-all selection without consuming the command's stdin.
pub(in crate::commands) fn cache_removal(
    path: &std::path::Path,
    target: &str,
    yes: bool,
) -> Result<bool> {
    if yes {
        return Ok(true);
    }
    anyhow::ensure!(
        terminal::stdout_is_tty() && terminal::stderr_is_tty(),
        "cache remove --all requires confirmation; use --yes in Raw or non-interactive mode"
    );
    let term = console::Term::stderr();
    let style = OutputStyle::stderr();
    term.write_line(
        &style
            .warning(format!(
                "You are about to remove all cache entries from \"{}\" on {target}.",
                crate::display::style::text(&path.to_string_lossy())
            ))
            .to_string(),
    )
    .context("failed to write cache removal confirmation")?;
    loop {
        term.write_str("Are you sure you want to continue? [y/N] ")
            .context("failed to write cache removal confirmation")?;
        let answer =
            read_answer(&term, true).context("failed to read cache removal confirmation")?;
        match answer.trim().to_ascii_lowercase().as_str() {
            "y" | "yes" => return Ok(true),
            "" | "n" | "no" => return Ok(false),
            _ => term.write_line("Please enter y or N.")?,
        }
    }
}

#[cfg(not(windows))]
fn read_answer(term: &console::Term, eof_declines: bool) -> std::io::Result<String> {
    if !eof_declines {
        return term.read_line();
    }
    let mut answer = String::new();
    loop {
        match term.read_key()? {
            console::Key::Enter => {
                term.write_line("")?;
                return Ok(answer);
            }
            console::Key::Char('\u{4}') => {
                term.write_line("")?;
                return Ok(String::new());
            }
            console::Key::Char(ch) if ch.is_ascii_alphabetic() => {
                answer.push(ch);
                term.write_str(&ch.to_string())?;
            }
            console::Key::Backspace if !answer.is_empty() => {
                answer.pop();
                term.clear_chars(1)?;
            }
            _ => {}
        }
    }
}

/// Crossterm reads CONIN$ on Windows even when the process stdin is a pipe.
#[cfg(windows)]
fn read_answer(term: &console::Term, eof_declines: bool) -> std::io::Result<String> {
    use crossterm::event::{Event, KeyCode, KeyEventKind, KeyModifiers, read};

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
            KeyCode::Char('d' | 'z')
                if eof_declines && key.modifiers.contains(KeyModifiers::CONTROL) =>
            {
                term.write_line("")?;
                return Ok(String::new());
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
