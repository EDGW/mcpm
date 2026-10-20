//! Scoped terminal lifecycle for the mod list, restoring state on every exit path.

use crossterm::{
    cursor::{Hide, Show},
    event::{DisableMouseCapture, EnableMouseCapture},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{Terminal, backend::CrosstermBackend};
use std::io;

pub(super) struct Screen {
    pub terminal: Terminal<CrosstermBackend<io::Stderr>>,
    raw: bool,
    alternate: bool,
}
impl Screen {
    pub fn new() -> io::Result<Self> {
        let mut screen = Self {
            terminal: Terminal::new(CrosstermBackend::new(io::stderr()))?,
            raw: false,
            alternate: false,
        };
        terminal::enable_raw_mode()?;
        screen.raw = true;
        screen.alternate = true;
        execute!(
            screen.terminal.backend_mut(),
            EnterAlternateScreen,
            EnableMouseCapture,
            Hide
        )?;
        Ok(screen)
    }
}
impl Drop for Screen {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(
                self.terminal.backend_mut(),
                DisableMouseCapture,
                Show,
                LeaveAlternateScreen
            );
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}
