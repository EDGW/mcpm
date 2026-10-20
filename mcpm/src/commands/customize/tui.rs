//! Scheme-first customization controller with inline editing and live previews.

use super::{preview, settings};
use crate::{
    terminal as policy,
    theme::{Role, config::Config},
};
use anyhow::{Context, Result};
use crossterm::{
    cursor::{Hide, Show},
    event::{self, Event, KeyCode, KeyEventKind, KeyModifiers},
    execute,
    terminal::{self, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    Terminal,
    backend::CrosstermBackend,
    layout::{Constraint, Layout},
    text::{Line, Span},
    widgets::{Paragraph, Wrap},
};
use std::{io, path::Path};
use unicode_width::UnicodeWidthStr;

pub(super) fn run(config: Config, directory: &Path) -> Result<()> {
    let mut screen = Screen {
        terminal: Terminal::new(CrosstermBackend::new(io::stderr()))
            .context("failed to create customize terminal")?,
        raw: false,
        alternate: false,
    };
    terminal::enable_raw_mode().context("failed to enable customize TUI")?;
    screen.raw = true;
    screen.alternate = true;
    execute!(screen.terminal.backend_mut(), EnterAlternateScreen, Hide)?;
    let mut saved = config.clone();
    let mut draft = config;
    let mut selected = 0;
    let mut editing: Option<Editor> = None;
    let mut category = 0usize;
    let mut prompt: Option<Prompt> = None;
    let mut confirming_exit = false;
    let mut status = Status::warning("Select a color scheme to preview it. Enter applies it.");
    loop {
        let mut fields = if let Some(edit) = &editing {
            settings::edit_fields(&draft, &edit.id, edit.type_cursor)
        } else {
            settings::fields(&draft, false)
        };
        if let Some(edit) = &editing {
            fields[0].value = edit.name.clone();
        }
        selected = selected.min(fields.len() - 1);
        screen.render(
            &draft,
            &fields,
            selected,
            &status,
            editing.as_ref(),
            prompt.as_ref(),
        )?;
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
            continue;
        }
        if confirming_exit {
            match key.code {
                KeyCode::Char('y' | 'Y') => match save(&mut draft, editing.as_ref(), directory) {
                    Ok(_) => return Ok(()),
                    Err(error) => {
                        confirming_exit = false;
                        status = Status::error(format!("Save failed: {error:#}"));
                    }
                },
                KeyCode::Char('n' | 'N') => return Ok(()),
                KeyCode::Esc => {
                    confirming_exit = false;
                    status = Status::warning("Exit cancelled · Unsaved changes retained");
                }
                _ => {}
            }
            continue;
        }
        let inline_name = editing.is_some() && selected == 0 && prompt.is_none();
        let quit = (key.code == KeyCode::Char('c')
            && key.modifiers.contains(KeyModifiers::CONTROL))
            || (key.code == KeyCode::Char('q') && !inline_name && prompt.is_none());
        if quit {
            let name_changed = editing
                .as_ref()
                .is_some_and(|edit| edit.name != saved.palette_name(&edit.id));
            if !draft.same_values(&saved) || name_changed {
                confirming_exit = true;
                status = Status::warning(
                    "Unsaved changes. Y: Save and quit · N: Discard and quit · Esc: Cancel",
                );
                prompt = None;
            } else {
                return Ok(());
            }
            continue;
        }
        if let Some(mut input) = prompt.take() {
            match key.code {
                KeyCode::Esc => {}
                KeyCode::Backspace => {
                    input.value.pop();
                    prompt = Some(input);
                }
                KeyCode::Char(ch) if !ch.is_control() => {
                    input.value.push(ch);
                    prompt = Some(input);
                }
                KeyCode::Enter => {
                    let mut next = draft.clone();
                    let id = settings::selected_id(&draft, settings::CATEGORY_KEYS[category]);
                    let result = match input.action {
                        Action::Create => next
                            .create_palette(settings::CATEGORY_KEYS[category], input.value.trim())
                            .map(|_| ()),
                        Action::Rename => next.rename_palette(&id, input.value.trim()),
                        Action::Delete if input.value == "DELETE" => next.delete_palette(&id),
                        Action::Delete => Err(anyhow::anyhow!("Deletion cancelled")),
                    }
                    .and_then(|()| next.save_styles(directory));
                    match result {
                        Ok(()) => {
                            saved = next.clone();
                            draft = next;
                            if matches!(input.action, Action::Create) {
                                editing = Some(Editor::new(input.value.trim().to_owned(), &draft));
                                selected = 0;
                            }
                            status = Status::saved(directory);
                        }
                        Err(error) => status = Status::error(format!("{error:#}")),
                    }
                }
                _ => prompt = Some(input),
            }
            continue;
        }
        if inline_name {
            let edit = editing.as_mut().unwrap();
            let previous = edit.name.clone();
            if edit.input(key.code, key.modifiers) {
                if edit.name != previous {
                    status = Status::warning("Unsaved changes · Enter saves · Ctrl-C quits");
                }
                continue;
            }
        }
        // Validate the name before leaving its row; Enter always saves the whole draft.
        if inline_name && matches!(key.code, KeyCode::Up | KeyCode::Down) {
            let edit = editing.as_ref().unwrap();
            if let Err(error) = draft.rename_palette(&edit.id, &edit.name) {
                status = Status::error(error.to_string());
                continue;
            }
        }
        let field = &fields[selected];
        if let Some(index) = settings::CATEGORY_KEYS
            .iter()
            .position(|key| *key == field.key)
        {
            category = index;
        }
        match key.code {
            KeyCode::Char('n') if editing.is_none() => {
                prompt = Some(Prompt {
                    action: Action::Create,
                    value: String::new(),
                })
            }
            KeyCode::Char('r') if editing.is_none() => {
                prompt = Some(Prompt {
                    action: Action::Rename,
                    value: String::new(),
                })
            }
            KeyCode::Char('d') if editing.is_none() => {
                prompt = Some(Prompt {
                    action: Action::Delete,
                    value: String::new(),
                })
            }
            KeyCode::Char('e') => {
                if editing.is_some() {
                    editing = None;
                    selected = category;
                } else {
                    let id = settings::selected_id(&draft, settings::CATEGORY_KEYS[category]);
                    draft.customize_palette(&id)?;
                    editing = Some(Editor::new(id, &draft));
                    selected = 0;
                    status = Status::warning("Type to edit Name · Enter saves · Ctrl-C quits");
                }
            }
            KeyCode::Esc => {
                draft = saved.clone();
                editing = None;
                selected = category;
                status = Status::warning("Unsaved changes reverted");
            }
            KeyCode::Up => selected = (selected + fields.len() - 1) % fields.len(),
            KeyCode::Down => selected = (selected + 1) % fields.len(),
            KeyCode::Home => selected = 0,
            KeyCode::End => selected = fields.len() - 1,
            KeyCode::Left | KeyCode::Right if field.label == "Types" => {
                let edit = editing.as_mut().unwrap();
                edit.type_cursor = 1 - edit.type_cursor;
            }
            KeyCode::Char(' ') if field.label == "Types" => {
                let edit = editing.as_ref().unwrap();
                let kind = ["tui", "console"][edit.type_cursor];
                let mut types = draft.palette_types(&edit.id).to_vec();
                if types.iter().any(|value| value == kind) {
                    types.retain(|value| value != kind);
                } else {
                    types.push(kind.into());
                }
                match draft.set(&field.key, &types.join(",")) {
                    Ok(()) => {
                        status = Status::warning("Unsaved changes · Preview follows selected types")
                    }
                    Err(error) => status = Status::error(error.to_string()),
                }
            }
            KeyCode::Left | KeyCode::Right if field.label != "Name" => {
                let choices = draft.choices(&field.key);
                let value = if matches!(field.label.as_str(), "Primary" | "Secondary") {
                    field.value.clone()
                } else {
                    draft
                        .entries()
                        .into_iter()
                        .find(|(key, _)| key == &field.key)
                        .unwrap()
                        .1
                };
                let position = choices
                    .iter()
                    .position(|choice| choice == &value)
                    .unwrap_or(0);
                let next = if key.code == KeyCode::Left {
                    (position + choices.len() - 1) % choices.len()
                } else {
                    (position + 1) % choices.len()
                };
                match draft.set(&field.key, &choices[next]) {
                    Ok(()) => {
                        status = Status::warning(format!(
                            "Preview: {} · Enter saves changes",
                            field.label
                        ))
                    }
                    Err(error) => status = Status::error(error.to_string()),
                }
            }
            KeyCode::Enter => match save(&mut draft, editing.as_ref(), directory) {
                Ok(message) => {
                    saved = draft.clone();
                    status = message;
                }
                Err(error) => status = Status::error(format!("Save failed: {error:#}")),
            },
            _ => {}
        }
    }
}

fn save(config: &mut Config, editor: Option<&Editor>, directory: &Path) -> Result<Status> {
    let mut next = config.clone();
    if let Some(edit) = editor {
        next.rename_palette(&edit.id, &edit.name)?;
    }
    next.save_styles(directory)?;
    *config = next;
    Ok(Status::saved(directory))
}

struct Editor {
    id: String,
    type_cursor: usize,
    name: String,
    cursor: usize,
}
impl Editor {
    fn new(id: String, config: &Config) -> Self {
        let name = config.palette_name(&id);
        let cursor = name.len();
        Self {
            id,
            type_cursor: 0,
            name,
            cursor,
        }
    }

    /// Keep a UTF-8 byte cursor at character boundaries for inline name editing.
    fn input(&mut self, key: KeyCode, modifiers: KeyModifiers) -> bool {
        match key {
            KeyCode::Char(ch)
                if !ch.is_control()
                    && !modifiers.intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
            {
                self.name.insert(self.cursor, ch);
                self.cursor += ch.len_utf8();
            }
            KeyCode::Backspace if self.cursor > 0 => {
                let previous = self.name[..self.cursor]
                    .char_indices()
                    .next_back()
                    .unwrap()
                    .0;
                self.name.drain(previous..self.cursor);
                self.cursor = previous;
            }
            KeyCode::Delete if self.cursor < self.name.len() => {
                self.name.remove(self.cursor);
            }
            KeyCode::Left if self.cursor > 0 => {
                self.cursor = self.name[..self.cursor]
                    .char_indices()
                    .next_back()
                    .unwrap()
                    .0;
            }
            KeyCode::Right if self.cursor < self.name.len() => {
                self.cursor += self.name[self.cursor..].chars().next().unwrap().len_utf8();
            }
            KeyCode::Home => self.cursor = 0,
            KeyCode::End => self.cursor = self.name.len(),
            KeyCode::Backspace | KeyCode::Delete | KeyCode::Left | KeyCode::Right => {}
            _ => return false,
        }
        true
    }
}

struct Status {
    message: String,
    role: Role,
}
impl Status {
    fn warning(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            role: Role::Warning,
        }
    }
    fn error(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            role: Role::Danger,
        }
    }
    fn saved(directory: &Path) -> Self {
        let files = if directory.join("styles/custom.toml").exists() {
            "styles/custom.toml, settings.toml, loader_colors.toml"
        } else {
            "settings.toml, loader_colors.toml"
        };
        Self {
            message: format!("Saved to {}: {files}", directory.display()),
            role: Role::Success,
        }
    }
}

struct Prompt {
    action: Action,
    value: String,
}
enum Action {
    Create,
    Rename,
    Delete,
}
impl Prompt {
    fn label(&self) -> &'static str {
        match self.action {
            Action::Create => "New Color Scheme ID (immutable)",
            Action::Rename => "Rename Color Scheme",
            Action::Delete => "Delete Color Scheme? Type DELETE to confirm",
        }
    }
}

struct Screen {
    terminal: Terminal<CrosstermBackend<io::Stderr>>,
    raw: bool,
    alternate: bool,
}
impl Screen {
    fn render(
        &mut self,
        config: &Config,
        fields: &[settings::Field],
        selected: usize,
        status: &Status,
        editing: Option<&Editor>,
        prompt: Option<&Prompt>,
    ) -> io::Result<()> {
        let enabled = policy::stderr_color_enabled();
        self.terminal.draw(|frame| {
            let area = frame.area();
            let visible = usize::from(area.height.saturating_sub(12).clamp(1, 4));
            let [title, fields_area, previews, footer] = Layout::vertical([
                Constraint::Length(2),
                Constraint::Length(visible as u16 + 1),
                Constraint::Min(0),
                Constraint::Length(6),
            ])
            .areas(area);
            let style = |role| preview::interface_style(role, enabled);
            frame.render_widget(
                Paragraph::new(
                    editing
                        .map(|edit| format!("Editing {}", edit.id))
                        .unwrap_or_else(|| "Customize · Color Schemes".into()),
                )
                .style(style(Role::Primary)),
                title,
            );
            let start = selected
                .saturating_sub(visible / 2)
                .min(fields.len().saturating_sub(visible));
            let lines: Vec<_> = fields
                .iter()
                .skip(start)
                .take(visible)
                .enumerate()
                .map(|(offset, field)| {
                    let active = start + offset == selected;
                    let label_style = style(if active {
                        Role::Important
                    } else {
                        Role::Normal
                    });
                    let value_style = if settings::CATEGORY_KEYS.contains(&field.key.as_str())
                        && enabled
                    {
                        let id = settings::selected_id(config, &field.key);
                        ratatui::style::Style::default()
                            .fg(preview::color(config.palette_primary(&id).tui()))
                            .add_modifier(ratatui::style::Modifier::BOLD)
                    } else if enabled && matches!(field.label.as_str(), "Primary" | "Secondary") {
                        ratatui::style::Style::default().fg(preview::color(
                            crate::theme::Color::parse(&field.value).unwrap().tui(),
                        ))
                    } else {
                        label_style
                    };
                    Line::from(vec![
                        Span::styled(
                            super::text(&format!(
                                "{} {}: ",
                                if active { ">" } else { " " },
                                field.label
                            )),
                            label_style,
                        ),
                        Span::styled(super::text(&field.value), value_style),
                    ])
                })
                .collect();
            frame.render_widget(Paragraph::new(lines), fields_area);
            if prompt.is_none()
                && selected == 0
                && let Some(edit) = editing
            {
                let prefix = UnicodeWidthStr::width(&edit.name[..edit.cursor]);
                let mut name_area = fields_area;
                name_area.x = name_area.x.saturating_add(8).min(fields_area.right());
                name_area.width = fields_area.right().saturating_sub(name_area.x);
                name_area.height = name_area.height.min(1);
                let scroll = prefix.saturating_sub(usize::from(name_area.width.saturating_sub(1)));
                frame.render_widget(
                    Paragraph::new(super::text(&edit.name))
                        .style(style(Role::Important))
                        .scroll((0, scroll.min(u16::MAX as usize) as u16)),
                    name_area,
                );
                if name_area.width > 0 && name_area.height > 0 {
                    frame
                        .set_cursor_position((name_area.x + (prefix - scroll) as u16, name_area.y));
                }
            }
            let full = if let Some(edit) = editing {
                preview::draw_edit(frame, previews, config, enabled, &edit.id)
            } else {
                preview::draw(frame, previews, config, enabled)
            };
            let message = prompt
                .map(|prompt| format!("{}: {}_", prompt.label(), prompt.value))
                .unwrap_or_else(|| status.message.clone());
            let [height_hint, message_area, help_area] = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(3),
                Constraint::Length(2),
            ])
            .areas(footer);
            frame.render_widget(
                Paragraph::new(if full {
                    ""
                } else {
                    "Increase terminal height to display the full Preview."
                })
                .style(style(Role::Warning)),
                height_hint,
            );
            frame.render_widget(
                Paragraph::new(super::text(&message))
                    .style(style(if prompt.is_some() {
                        Role::Warning
                    } else {
                        status.role
                    }))
                    .wrap(Wrap { trim: false }),
                message_area,
            );
            frame.render_widget(
                Paragraph::new(vec![
                    Line::from(if editing.is_some() && selected == 0 {
                        "←/→ Cursor · Home/End · Backspace/Delete · Enter Save"
                    } else if editing.is_some() {
                        "←/→ Choose · Space Toggle Type · Enter Save · e Back"
                    } else {
                        "←/→ Preview · Enter Apply · n New · r Rename · d Delete · e Edit"
                    }),
                    Line::from(if editing.is_some() && selected == 0 {
                        "↑/↓ Select · Esc Revert · Ctrl-C Quit"
                    } else {
                        "↑/↓ Select · Esc Revert / Cancel · q / Ctrl-C Quit"
                    }),
                ])
                .style(style(Role::Muted)),
                help_area,
            );
        })?;
        Ok(())
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        if self.alternate {
            let _ = execute!(self.terminal.backend_mut(), Show, LeaveAlternateScreen);
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inline_name_handles_unicode_and_shortcut_letters_as_text() {
        let mut config = Config::defaults();
        config
            .create_palette(settings::CATEGORY_KEYS[0], "new")
            .unwrap();
        let mut editor = Editor::new("new".into(), &config);
        for _ in 0..3 {
            editor.input(KeyCode::Backspace, KeyModifiers::NONE);
        }
        for ch in "青qe".chars() {
            assert!(editor.input(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        editor.input(KeyCode::Home, KeyModifiers::NONE);
        editor.input(KeyCode::Right, KeyModifiers::NONE);
        editor.input(KeyCode::Delete, KeyModifiers::NONE);
        editor.input(KeyCode::Backspace, KeyModifiers::NONE);
        assert_eq!(editor.name, "e");
        assert_eq!(editor.cursor, 0);
        assert!(!editor.input(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!editor.input(KeyCode::Char('c'), KeyModifiers::CONTROL));
    }

    #[test]
    fn save_validates_inline_name_and_reports_written_files() {
        let directory = tempfile::tempdir().unwrap();
        let mut config = Config::defaults();
        let saved = config.clone();
        let id = settings::selected_id(&config, settings::CATEGORY_KEYS[0]);
        config.customize_palette(&id).unwrap();
        assert!(config.same_values(&saved));
        let mut editor = Editor::new(id, &config);
        editor.name.clear();
        assert!(save(&mut config, Some(&editor), directory.path()).is_err());
        assert!(config.same_values(&saved));
        assert!(!directory.path().join("styles/custom.toml").exists());
        editor.name = "New Name".into();
        let status = save(&mut config, Some(&editor), directory.path()).unwrap();
        assert!(matches!(status.role, Role::Success));
        assert!(status.message.contains("styles/custom.toml"));
        assert!(
            status
                .message
                .contains(&directory.path().display().to_string())
        );
        assert!(!config.same_values(&saved));
        let loaded = Config::load(directory.path()).unwrap();
        assert_eq!(loaded.palette_name(&editor.id), "New Name");
        assert!(config.same_values(&loaded));
    }
}
