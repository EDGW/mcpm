//! Interactive client selection rendered through the shared TUI theme.

use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Print, PrintStyledContent},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use mcpm_backend::local::mc::client::MinecraftVersion;
use thiserror::Error;
use unicode_width::UnicodeWidthChar;

use super::output::text;
use crate::terminal as policy;

#[derive(Debug, Error)]
enum SelectError {
    #[error("no client versions found in the selected directory")]
    NoVersions,
    #[error(
        "interactive version selection requires terminal stdin, stdout, and stderr without --raw; {hint}"
    )]
    Unavailable { hint: &'static str },
}

pub(super) enum Purpose {
    Check,
    Launch,
}

impl Purpose {
    fn action(&self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Launch => "launch",
        }
    }
    fn hint(&self) -> &'static str {
        match self {
            Self::Check => "specify version IDs or use --all",
            Self::Launch => "specify version IDs",
        }
    }
}

pub(super) fn select(versions: &[MinecraftVersion], purpose: Purpose) -> Result<Option<usize>> {
    if versions.is_empty() {
        return Err(SelectError::NoVersions.into());
    }
    if !policy::stdout_is_tty() || !io::stdin().is_terminal() {
        return Err(SelectError::Unavailable {
            hint: purpose.hint(),
        }
        .into());
    }
    let mut screen = Screen::new().context("failed to open version selector")?;
    let mut selected = 0;
    loop {
        screen
            .render(versions, selected, purpose.action())
            .context("failed to draw version selector")?;
        if let Event::Key(key) = event::read().context("failed to read version selection")? {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            match key.code {
                KeyCode::Up => selected = (selected + versions.len() - 1) % versions.len(),
                KeyCode::Down => selected = (selected + 1) % versions.len(),
                KeyCode::Home => selected = 0,
                KeyCode::End => selected = versions.len() - 1,
                KeyCode::Enter => return Ok(Some(selected)),
                KeyCode::Esc => return Ok(None),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(None);
                }
                _ => {}
            }
        }
    }
}

#[derive(Debug)]
struct Screen {
    output: io::Stderr,
}

impl Screen {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut screen = Self {
            output: io::stderr(),
        };
        execute!(screen.output, EnterAlternateScreen, Hide)?;
        Ok(screen)
    }

    fn render(
        &mut self,
        versions: &[MinecraftVersion],
        selected: usize,
        action: &str,
    ) -> io::Result<()> {
        let (width, height) = terminal::size().unwrap_or((80, 24));
        let title = crate::theme::tui(crate::theme::Role::Primary);
        queue!(
            self.output,
            Clear(ClearType::All),
            MoveTo(0, 0),
            PrintStyledContent(
                title.apply(clip(&format!("Select a client version to {action}"), width))
            )
        )?;
        let visible = usize::from(height.saturating_sub(4).max(1));
        let start = selected
            .saturating_sub(visible / 2)
            .min(versions.len().saturating_sub(visible));
        for (offset, version) in versions.iter().skip(start).take(visible).enumerate() {
            let index = start + offset;
            let broken = matches!(version, MinecraftVersion::Broken(_));
            let label = format!(
                "{} {} [{}]",
                if index == selected { ">" } else { " " },
                text(version.name()),
                if broken { "broken" } else { "normal" }
            );
            let style = crate::theme::tui(if index == selected {
                crate::theme::Role::Primary
            } else if broken {
                crate::theme::Role::Danger
            } else {
                crate::theme::Role::Normal
            });
            queue!(
                self.output,
                MoveTo(0, (offset + 2) as u16),
                PrintStyledContent(style.apply(clip(&label, width)))
            )?;
        }
        queue!(
            self.output,
            MoveTo(0, height.saturating_sub(1)),
            Print(clip(
                &format!("Up/Down: move | Enter: {action} | Esc/Ctrl-C: cancel"),
                width
            ))
        )?;
        self.output.flush()
    }
}

// Keep all terminal state changes behind crossterm, including error/cancel cleanup.
impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(self.output, Show, LeaveAlternateScreen);
        let _ = terminal::disable_raw_mode();
    }
}

fn clip(value: &str, width: u16) -> String {
    let mut used = 0;
    value
        .chars()
        .take_while(|ch| {
            used += ch.width().unwrap_or(0);
            used <= usize::from(width.saturating_sub(1))
        })
        .collect()
}
