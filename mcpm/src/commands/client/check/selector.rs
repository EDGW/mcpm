use std::io::{self, IsTerminal, Write};

use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Attribute, Color, ContentStyle, Print, PrintStyledContent},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use kako_mods_lib::local::mc::client::MinecraftVersion;
use thiserror::Error;
use unicode_width::UnicodeWidthChar;

use super::super::output::text;
use crate::terminal::{self as policy, Stream};

#[derive(Debug, Error)]
enum SelectError {
    #[error("no client versions found in the selected directory")]
    NoVersions,
    #[error(
        "interactive version selection requires terminal input and stderr; specify version IDs or use --all"
    )]
    Unavailable,
}

pub(super) fn select(versions: &[MinecraftVersion]) -> Result<Option<usize>> {
    if versions.is_empty() {
        return Err(SelectError::NoVersions.into());
    }
    if !io::stdin().is_terminal() || !policy::is_tty(Stream::Stderr) {
        return Err(SelectError::Unavailable.into());
    }
    let mut screen = Screen::new().context("failed to open version selector")?;
    let mut selected = 0;
    loop {
        screen
            .render(versions, selected)
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

    fn render(&mut self, versions: &[MinecraftVersion], selected: usize) -> io::Result<()> {
        let (width, height) = terminal::size().unwrap_or((80, 24));
        let color = policy::color_enabled(Stream::Stderr);
        let title = if color {
            content_style(Color::Cyan, true)
        } else {
            ContentStyle::new()
        };
        queue!(
            self.output,
            Clear(ClearType::All),
            MoveTo(0, 0),
            PrintStyledContent(title.apply(clip("Select a client version to check", width)))
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
            let style = if color {
                content_style(
                    if index == selected {
                        Color::Cyan
                    } else if broken {
                        Color::Red
                    } else {
                        Color::White
                    },
                    index == selected,
                )
            } else {
                ContentStyle::new()
            };
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
                "Up/Down: move | Enter: check | Esc/Ctrl-C: cancel",
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

fn content_style(color: Color, bold: bool) -> ContentStyle {
    let mut style = ContentStyle::new();
    style.foreground_color = Some(color);
    if bold {
        style.attributes.set(Attribute::Bold);
    }
    style
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
