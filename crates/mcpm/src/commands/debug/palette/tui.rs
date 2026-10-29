//! Responsive crossterm palette with scrolling and terminal state restoration.

use std::io::{self, Write};

use crossterm::{
    cursor::{Hide, MoveTo, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute, queue,
    style::{Attribute, Color, ContentStyle, PrintStyledContent, ResetColor, SetAttribute},
    terminal::{self, Clear, ClearType, EnterAlternateScreen, LeaveAlternateScreen},
};

use crate::terminal as policy;

pub(super) fn run() -> io::Result<()> {
    let mut screen = Screen::new()?;
    let mut scroll: usize = 0;
    loop {
        let visible = screen.render(&mut scroll)?;
        match event::read()? {
            Event::Key(key) if key.kind != KeyEventKind::Release => match key.code {
                KeyCode::Esc | KeyCode::Char('q') => return Ok(()),
                KeyCode::Char('c') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    return Ok(());
                }
                KeyCode::Up | KeyCode::Char('k') => scroll = scroll.saturating_sub(1),
                KeyCode::Down | KeyCode::Char('j') => scroll = scroll.saturating_add(1),
                KeyCode::PageUp => scroll = scroll.saturating_sub(visible.max(1)),
                KeyCode::PageDown => scroll = scroll.saturating_add(visible.max(1)),
                KeyCode::Home => scroll = 0,
                KeyCode::End => scroll = usize::MAX,
                _ => {}
            },
            Event::Resize(_, _) => scroll = 0,
            _ => {}
        }
    }
}

#[derive(Debug)]
struct Screen {
    output: io::Stdout,
}

impl Screen {
    fn new() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        let mut screen = Self {
            output: io::stdout(),
        };
        execute!(screen.output, EnterAlternateScreen, Hide)?;
        Ok(screen)
    }

    fn render(&mut self, scroll: &mut usize) -> io::Result<usize> {
        let (width, height) = terminal::size()?;
        queue!(
            self.output,
            ResetColor,
            SetAttribute(Attribute::Reset),
            Clear(ClearType::All)
        )?;
        if width == 0 || height == 0 {
            self.output.flush()?;
            return Ok(0);
        }
        if height < 5 {
            self.plain(0, width, "Enlarge terminal; q/Esc: quit")?;
            self.output.flush()?;
            return Ok(0);
        }
        self.plain(
            0,
            width,
            "mcpm palette · crossterm · 16 colors + default foreground",
        )?;
        let columns = width >= 66;
        let colors = colors();
        let total = colors.len() * if columns { 1 } else { 3 };
        let visible = usize::from(height - 4);
        *scroll = (*scroll).min(total.saturating_sub(visible));
        if columns {
            let column_width = width / 3;
            for (column, heading) in ["Normal", "Bold (Attribute::Bold)", "Dim (Attribute::Dim)"]
                .iter()
                .enumerate()
            {
                self.cell(
                    column_width * column as u16,
                    1,
                    column_width,
                    heading,
                    ContentStyle::new(),
                )?;
            }
        } else {
            self.plain(
                1,
                width,
                "Normal / Bold / Dim (Attribute::Bold / Attribute::Dim)",
            )?;
        }
        for (offset, index) in (*scroll..total).take(visible).enumerate() {
            let row = offset as u16 + 2;
            if columns {
                let color = colors[index];
                for variant in 0..3 {
                    self.cell(
                        width / 3 * variant,
                        row,
                        width / 3,
                        &format!("Color::{color:?}"),
                        style(color, variant),
                    )?;
                }
            } else {
                let color = colors[index / 3];
                let variant = (index % 3) as u16;
                let label = match variant {
                    0 => format!("Color::{color:?}"),
                    1 => format!("Color::{color:?} + Attribute::Bold"),
                    _ => format!("Color::{color:?} + Attribute::Dim"),
                };
                self.cell(0, row, width, &label, style(color, variant))?;
            }
        }
        self.plain(
            height - 2,
            width,
            &format!(
                "Rows {}-{} / {total} · colors follow terminal theme; NO_COLOR respected",
                *scroll + 1,
                (*scroll + visible).min(total)
            ),
        )?;
        self.plain(
            height - 1,
            width,
            "q/Esc: quit · Up/Down: scroll · PgUp/PgDn · Home/End",
        )?;
        self.output.flush()?;
        Ok(visible)
    }

    fn plain(&mut self, row: u16, width: u16, text: &str) -> io::Result<()> {
        self.cell(0, row, width, text, ContentStyle::new())
    }

    fn cell(
        &mut self,
        column: u16,
        row: u16,
        width: u16,
        text: &str,
        style: ContentStyle,
    ) -> io::Result<()> {
        let text: String = text.chars().take(usize::from(width)).collect();
        queue!(
            self.output,
            MoveTo(column, row),
            PrintStyledContent(style.apply(text))
        )
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = execute!(
            self.output,
            ResetColor,
            SetAttribute(Attribute::Reset),
            Show,
            LeaveAlternateScreen
        );
        let _ = terminal::disable_raw_mode();
    }
}

fn colors() -> [Color; 17] {
    [
        Color::Black,
        Color::DarkGrey,
        Color::DarkRed,
        Color::Red,
        Color::DarkGreen,
        Color::Green,
        Color::DarkYellow,
        Color::Yellow,
        Color::DarkBlue,
        Color::Blue,
        Color::DarkMagenta,
        Color::Magenta,
        Color::DarkCyan,
        Color::Cyan,
        Color::Grey,
        Color::White,
        Color::Reset,
    ]
}

fn style(color: Color, variant: u16) -> ContentStyle {
    let mut style = ContentStyle::new();
    if policy::stdout_color_enabled() {
        style.foreground_color = Some(color);
    }
    match variant {
        1 => style.attributes.set(Attribute::Bold),
        2 => style.attributes.set(Attribute::Dim),
        _ => {}
    }
    style
}
