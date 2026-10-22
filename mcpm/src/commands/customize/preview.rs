//! Ratatui widgets for responsive theme previews with independent card borders and content.

use ratatui::{
    Frame,
    buffer::Buffer,
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, BorderType, Padding, Paragraph, Widget, Wrap},
};

use crate::theme::{Role, Theme, config::Config};

const PANEL_HEIGHT: u16 = 13;

/// Render the visible part of a full-size preview, preserving card geometry when short.
pub(super) fn draw(frame: &mut Frame, area: Rect, config: &Config, enabled: bool) -> bool {
    draw_with_mode(
        frame,
        area,
        config,
        enabled,
        crate::terminal::light_background(),
    )
}

fn draw_with_mode(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    enabled: bool,
    terminal_light: Option<bool>,
) -> bool {
    draw_panels(
        frame,
        area,
        config,
        enabled,
        terminal_light,
        &[(true, false), (true, true), (false, false), (false, true)],
    )
}

pub(super) fn draw_edit(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    enabled: bool,
    id: &str,
) -> bool {
    let types = config.palette_types(id);
    let panels: Vec<_> = [(true, false), (true, true), (false, false), (false, true)]
        .into_iter()
        .filter(|(tui, _)| {
            types
                .iter()
                .any(|kind| kind == if *tui { "tui" } else { "console" })
        })
        .collect();
    let mut preview = config.clone();
    for (tui, light) in &panels {
        let key = match (tui, light) {
            (true, false) => "settings.tui_style_dark",
            (true, true) => "settings.tui_style_light",
            (false, false) => "settings.console_style_dark",
            (false, true) => "settings.console_style_light",
        };
        preview
            .set(key, id)
            .expect("edited palette supports selected preview type");
    }
    draw_panels(
        frame,
        area,
        &preview,
        enabled,
        crate::terminal::light_background(),
        &panels,
    )
}

fn draw_panels(
    frame: &mut Frame,
    area: Rect,
    config: &Config,
    enabled: bool,
    terminal_light: Option<bool>,
    panels: &[(bool, bool)],
) -> bool {
    let paired = area.width >= 92;
    let row_count = if paired {
        panels.len().div_ceil(2)
    } else {
        panels.len()
    };
    let height = PANEL_HEIGHT * row_count as u16;
    let canvas = Rect::new(0, 0, area.width, height);
    let mut buffer = Buffer::empty(canvas);
    let rows = Layout::vertical(vec![Constraint::Length(PANEL_HEIGHT); row_count]).split(canvas);
    let theme = Theme {
        config: config.clone(),
    };
    for (index, &(tui, light)) in panels.iter().enumerate() {
        let panel = if paired {
            Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)])
                .spacing(2)
                .split(rows[index / 2])[index % 2]
        } else {
            rows[index]
        };
        render_panel(
            &mut buffer,
            panel,
            &theme,
            tui,
            light,
            enabled,
            terminal_light,
        );
    }
    frame.render_widget(PreviewViewport(&buffer), area);
    area.height >= height
}

fn render_panel(
    buffer: &mut Buffer,
    area: Rect,
    theme: &Theme,
    tui: bool,
    light: bool,
    enabled: bool,
    terminal_light: Option<bool>,
) {
    let [heading, sample, hint, _] = Layout::vertical([
        Constraint::Length(1),
        Constraint::Length(9),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .areas(area);
    let role = |role| style(theme, role, tui, light, enabled);
    let mode = if tui { "TUI" } else { "Console" };
    let appearance = if light { "Light" } else { "Dark" };
    Paragraph::new(format!("{mode} · {appearance}"))
        .style(role(Role::Important))
        .render(heading, buffer);
    let loader = if enabled {
        let mut loader = role(Role::Normal);
        loader.fg = theme
            .config
            .loader_color(tui, "fabric")
            .map(|color| color.tui())
            .map(color);
        loader
    } else {
        Style::default()
    };
    let content = if tui {
        vec![
            Line::from(Span::styled("Create", role(Role::Primary))),
            Line::from(Span::styled("1.21.1 · 0.5.1", role(Role::Secondary))),
            Line::from(Span::styled("Fabric", loader)),
            Line::from(Span::styled("/mods/create.jar", role(Role::Muted))),
            Line::from(Span::styled("Ready to launch", role(Role::Success))),
        ]
    } else {
        vec![
            Line::from(Span::styled("$ mcpm mod list", role(Role::Muted))),
            Line::from(vec![
                Span::styled("[1]  ", role(Role::Muted)),
                Span::styled("Create", role(Role::Primary)),
            ]),
            Line::from(Span::styled(
                "Version 0.5.1 · Minecraft 1.21.1",
                role(Role::Secondary),
            )),
            Line::from(Span::styled("Fabric", loader)),
            Line::from(Span::styled("/mods/create.jar", role(Role::Muted))),
            Line::from(Span::styled(
                "Check passed. No problems found.",
                role(Role::Success),
            )),
            Line::from(Span::styled(
                "Optional dependency unavailable",
                role(Role::Warning),
            )),
        ]
    };
    let paragraph = Paragraph::new(content);
    if tui {
        let block = Block::bordered()
            .padding(Padding::new(2, 2, 1, 1))
            .border_type(BorderType::Rounded)
            .border_style(role(Role::Primary))
            .title(Line::from(Span::styled(
                " Example Mod ",
                role(Role::Primary),
            )));
        let inner = block.inner(sample);
        block.render(sample, buffer);
        let [content, swatches] = Layout::horizontal([Constraint::Min(0), Constraint::Length(8)])
            .spacing(2)
            .areas(inner);
        paragraph.render(content, buffer);
        // Terminal cells are typically twice as tall as they are wide: 4 × 2
        // cells form a visual square. Adjacent swatches share no gap.
        let rows = Layout::vertical([Constraint::Length(2), Constraint::Length(2)]).split(swatches);
        for (index, tone) in [Role::Primary, Role::Secondary, Role::Normal, Role::Muted]
            .into_iter()
            .enumerate()
        {
            let cells = Layout::horizontal([Constraint::Length(4), Constraint::Length(4)])
                .split(rows[index / 2]);
            Paragraph::new("████\n████")
                .style(role(tone))
                .render(cells[index % 2], buffer);
        }
    } else {
        paragraph
            .block(Block::bordered().border_style(role(Role::Muted)))
            .render(sample, buffer);
    }
    if terminal_light != Some(light) {
        Paragraph::new(format!(
            "Switch terminal to {} mode to preview correctly.",
            if light { "light" } else { "dark" }
        ))
        .wrap(Wrap { trim: false })
        .render(hint, buffer);
    }
}

/// Copy widget cells through a clipped viewport; no border characters are generated here.
#[derive(Debug)]
struct PreviewViewport<'a>(&'a Buffer);
impl Widget for PreviewViewport<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        for y in 0..area.height.min(self.0.area.height) {
            for x in 0..area.width.min(self.0.area.width) {
                if let (Some(source), Some(target)) = (
                    self.0.cell((x, y)),
                    buffer.cell_mut((area.x + x, area.y + y)),
                ) {
                    *target = source.clone();
                }
            }
        }
    }
}

fn style(theme: &Theme, role: Role, tui: bool, light: bool, enabled: bool) -> Style {
    if !enabled {
        return Style::default();
    }
    let content = theme.tui(role, true);
    let mut result = Style::default();
    if let Some(foreground) = content.foreground_color {
        result = result.fg(color(foreground));
    }
    let palette = theme.config.palette_for(tui, light);
    match role {
        Role::Primary => result = result.fg(color(palette.primary.tui())),
        Role::Secondary => result = result.fg(color(palette.secondary.tui())),
        _ => {}
    }
    result = result.add_modifier(match role {
        Role::Primary | Role::Important | Role::Warning | Role::Success | Role::Danger => {
            Modifier::BOLD
        }
        Role::Muted => Modifier::DIM,
        _ => Modifier::empty(),
    });
    result
}

pub(super) fn interface_style(role: Role, enabled: bool) -> Style {
    style(
        crate::theme::get(),
        role,
        true,
        crate::terminal::light_background().unwrap_or(false),
        enabled,
    )
}

pub(super) fn color(value: crossterm::style::Color) -> Color {
    crate::theme::widgets::color(value)
}
