//! Interactive server launch selection with shared TUI colors and loader styles.

use crate::{commands::output_style::text, terminal as policy};
use anyhow::{Context, Result, ensure};
use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Print, PrintStyledContent},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};
use kako_mods_lib::local::mc::server::identify::ServerCandidate;
use std::io::{self, IsTerminal, Write};
use unicode_width::UnicodeWidthChar;

pub(super) fn select(candidates: &[ServerCandidate]) -> Result<Option<usize>> {
    ensure!(
        policy::stdout_is_tty() && io::stdin().is_terminal(),
        "multiple identified server launches found; TUI selection requires terminal stdin, stdout and stderr without --raw; use a dedicated --start-script or manual version/loader options to disambiguate"
    );
    let mut screen = Screen {
        output: io::stderr(),
        raw: false,
        alternate: false,
    };
    terminal::enable_raw_mode().context("failed to enable server selector")?;
    screen.raw = true;
    screen.alternate = true;
    execute!(screen.output, EnterAlternateScreen, Hide)?;
    let mut selected = 0;
    loop {
        screen.render(candidates, selected)?;
        if let Event::Key(key) = event::read()? {
            if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
                continue;
            }
            match key.code {
                KeyCode::Up => selected = (selected + candidates.len() - 1) % candidates.len(),
                KeyCode::Down => selected = (selected + 1) % candidates.len(),
                KeyCode::Home => selected = 0,
                KeyCode::End => selected = candidates.len() - 1,
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
    raw: bool,
    alternate: bool,
}
impl Screen {
    fn render(&mut self, candidates: &[ServerCandidate], selected: usize) -> io::Result<()> {
        let (width, height) = terminal::size().unwrap_or((80, 24));
        let theme = crate::theme::get();
        let enabled = policy::stderr_color_enabled();
        queue!(
            self.output,
            Clear(ClearType::All),
            MoveTo(0, 0),
            PrintStyledContent(theme.tui(crate::theme::Role::Primary, enabled).apply(clip(
                "Select an identified server launch to initialize",
                width
            )))
        )?;
        let visible = usize::from(height.saturating_sub(4).max(1));
        let start = selected
            .saturating_sub(visible / 2)
            .min(candidates.len().saturating_sub(visible));
        for (offset, candidate) in candidates.iter().skip(start).take(visible).enumerate() {
            let mut remaining = width;
            let prefix = clip(
                &format!(
                    "{} {}  ",
                    if start + offset == selected { ">" } else { " " },
                    text(&candidate.metadata.minecraft_version)
                ),
                remaining,
            );
            remaining = remaining.saturating_sub(display_width(&prefix));
            queue!(
                self.output,
                MoveTo(0, (offset + 2) as u16),
                PrintStyledContent(
                    theme
                        .tui(
                            if start + offset == selected {
                                crate::theme::Role::Primary
                            } else {
                                crate::theme::Role::Important
                            },
                            enabled
                        )
                        .apply(prefix)
                )
            )?;
            let (id, name, version) = match &candidate.metadata.loader {
                Some(loader) => {
                    let name = kako_mods_lib::local::modloader::BUILTIN_LOADERS
                        .get(loader.loader_type.as_str())
                        .map(|m| m.name())
                        .unwrap_or(&loader.loader_type);
                    (loader.loader_type.as_str(), name, loader.version.as_str())
                }
                None => ("vanilla", "Vanilla", ""),
            };
            let loader = clip(&text(name), remaining);
            remaining = remaining.saturating_sub(display_width(&loader));
            queue!(
                self.output,
                PrintStyledContent(theme.tui_loader(id, enabled).apply(loader))
            )?;
            let detail = clip(
                &format!(
                    " {}  {}:{} [{}]",
                    text(version),
                    text(&candidate.launch.script.display().to_string()),
                    candidate.launch.line,
                    text(&candidate.launch.main_class)
                ),
                remaining,
            );
            queue!(
                self.output,
                PrintStyledContent(theme.tui(crate::theme::Role::Muted, enabled).apply(detail))
            )?;
        }
        queue!(
            self.output,
            MoveTo(0, height.saturating_sub(1)),
            Print(clip(
                "Up/Down: move | Enter: initialize | Esc/Ctrl-C: cancel",
                width
            ))
        )?;
        self.output.flush()
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(self.output, Show, LeaveAlternateScreen);
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}
fn display_width(value: &str) -> u16 {
    value
        .chars()
        .map(|ch| ch.width().unwrap_or(0))
        .sum::<usize>()
        .min(u16::MAX as usize) as u16
}
fn clip(value: &str, width: u16) -> String {
    let mut used = 0;
    value
        .chars()
        .take_while(|ch| {
            used += ch.width().unwrap_or(0);
            used < usize::from(width)
        })
        .collect()
}
