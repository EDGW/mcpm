//! Session-local RCON help catalog; command names are matched, argument syntax remains opaque.

use crate::theme::{Role, widgets::style};
use ratatui::{
    style::Modifier,
    text::{Line, Span},
};
use std::borrow::Cow;

const LIMIT: usize = 5;

#[derive(Default)]
pub(super) struct Suggestions {
    catalog: Vec<Command>,
    builtin_count: usize,
    failed: bool,
    selected: Option<usize>,
}
impl Suggestions {
    pub fn builtin_hints(
        &mut self,
        definitions: impl Iterator<Item = (&'static str, &'static str)>,
    ) {
        self.catalog
            .extend(definitions.map(|(name, description)| Command {
                name: Cow::Borrowed(name),
                arguments: Cow::Borrowed(description),
                alias: None,
            }));
        self.builtin_count = self.catalog.len();
    }
    pub fn help_result(&mut self, output: Option<&str>) {
        let mut commands = output.map(parse).unwrap_or_default();
        self.failed = commands.is_empty();
        self.catalog.truncate(self.builtin_count);
        for command in &mut commands {
            if let Some(target) = &mut command.alias {
                *target += self.builtin_count;
            }
        }
        self.catalog.extend(commands);
        self.selected = None;
    }

    pub fn reset(&mut self) {
        self.selected = None;
    }

    fn matches<'a>(&'a self, draft: &str) -> impl Iterator<Item = &'a Command> {
        let name = input(draft);
        let draft = draft.trim_start();
        let matches = self.catalog.iter().filter(move |command| {
            let builtin = command.name.starts_with('@');
            (draft.is_empty() || builtin == draft.starts_with('@'))
                && command.name.starts_with(name)
        });
        matches
            .clone()
            .filter(move |command| command.name == name)
            .chain(matches.filter(move |command| command.name != name))
    }

    pub fn navigate(&mut self, draft: &str, down: bool) {
        let count = self.matches(draft).count();
        self.selected = if count == 0 {
            None
        } else {
            Some(match self.selected {
                None => {
                    if down {
                        0
                    } else {
                        count - 1
                    }
                }
                Some(index) => {
                    if down {
                        (index + 1) % count
                    } else {
                        (index + count - 1) % count
                    }
                }
            })
        };
    }

    /// Completion changes only the first token; existing arguments and slash choice survive.
    pub fn accept(&mut self, draft: &mut String) -> Option<usize> {
        if draft.trim().is_empty() {
            return None;
        }
        let command = self.matches(draft).nth(self.selected.unwrap_or(0))?;
        let start = draft.len() - draft.trim_start().len();
        let start = start + usize::from(draft[start..].starts_with('/'));
        let end = draft[start..]
            .find(char::is_whitespace)
            .map_or(draft.len(), |offset| start + offset);
        let append_space = end == draft.len() && !command.name.starts_with('@');
        let cursor = start + command.name.len() + usize::from(append_space);
        if draft.len() - (end - start) + command.name.len() + usize::from(append_space) > 16 * 1024
        {
            return None;
        }
        draft.replace_range(start..end, &command.name);
        if append_space {
            draft.push(' ');
        }
        self.selected = None;
        Some(cursor)
    }
    fn alias_arguments(&self, command: &Command) -> Option<&str> {
        let mut target = command.alias?;
        for _ in 0..self.catalog.len() {
            let command = &self.catalog[target];
            match command.alias {
                Some(next) => target = next,
                None => return Some(&command.arguments),
            }
        }
        None // Cyclic aliases keep their original arrow text without a derived syntax hint.
    }

    pub fn view<'a>(&'a self, draft: &'a str) -> View<'a> {
        if draft.trim().is_empty() {
            return View {
                rows: Vec::new(),
                title: String::new(),
                selected: None,
            };
        }
        let prefix = input(draft);
        let count = self.matches(draft).count();
        let start = self.selected.unwrap_or(0).saturating_sub(LIMIT - 1);
        let title = if let Some(index) = self.selected.filter(|index| *index < count) {
            format!(" Commands {}/{count} · ↑/↓ select · Tab fill ", index + 1)
        } else {
            format!(
                " Commands {}–{}/{count} · ↑/↓ select · Tab fill ",
                usize::from(count > 0),
                count.min(LIMIT)
            )
        };
        let primary = style(Role::Primary);
        let regular = primary.remove_modifier(Modifier::BOLD);
        let output = style(Role::Normal).remove_modifier(Modifier::BOLD);
        let mut rows = self
            .matches(draft)
            .enumerate()
            .skip(start)
            .take(LIMIT)
            .map(|(index, command)| {
                let matched = prefix.len();
                let description = if command.name.starts_with('@') {
                    style(Role::Muted).remove_modifier(Modifier::BOLD)
                } else {
                    output
                };
                let mut spans = vec![
                    Span::styled(
                        if self.selected == Some(index) {
                            "› "
                        } else {
                            "  "
                        },
                        output,
                    ),
                    Span::styled(
                        if command.name.starts_with('@') {
                            ""
                        } else {
                            "/"
                        },
                        output,
                    ),
                    Span::styled(&command.name[..matched], primary),
                    Span::styled(&command.name[matched..], regular),
                    Span::styled(
                        if command.arguments.is_empty() {
                            ""
                        } else {
                            " "
                        },
                        output,
                    ),
                    Span::styled(command.arguments.as_ref(), description),
                ];
                if let Some(arguments) = self
                    .alias_arguments(command)
                    .filter(|args| !args.is_empty())
                {
                    spans.push(Span::styled(" ", output));
                    spans.push(Span::styled(arguments, output));
                }
                Line::from(spans)
            })
            .collect::<Vec<_>>();
        if self.failed && !draft.trim_start().starts_with('@') {
            rows.insert(0, Line::styled(" Failed to fetch help", output));
            rows.truncate(LIMIT);
        }
        View {
            rows,
            title,
            selected: self.selected.map(|index| index - start),
        }
    }
}

struct Command {
    name: Cow<'static, str>,
    arguments: Cow<'static, str>,
    alias: Option<usize>,
}

pub(super) struct View<'a> {
    pub rows: Vec<Line<'a>>,
    pub title: String,
    pub selected: Option<usize>,
}

fn input(draft: &str) -> &str {
    let draft = draft.trim_start();
    let draft = draft.strip_prefix('/').unwrap_or(draft);
    let end = draft.find(char::is_whitespace).unwrap_or(draft.len());
    &draft[..end]
}

fn parse(output: &str) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();
    for line in output
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
    {
        if let Some(entry) = line.strip_prefix('/') {
            let end = entry.find(char::is_whitespace).unwrap_or(entry.len());
            let name = &entry[..end];
            if name.is_empty() || name.chars().any(char::is_control) {
                continue;
            }
            if !commands.iter().any(|command| command.name == name) {
                commands.push(Command {
                    name: Cow::Owned(name.to_owned()),
                    arguments: Cow::Owned(entry[end..].trim().to_owned()),
                    alias: None,
                });
            }
        } else if let Some(command) = commands.last_mut() {
            if !command.arguments.is_empty() {
                command.arguments.to_mut().push(' ');
            }
            command.arguments.to_mut().push_str(line);
        }
    }
    for index in 0..commands.len() {
        if let Some(target) = commands[index].arguments.strip_prefix("->").map(str::trim) {
            commands[index].alias = commands.iter().position(|command| command.name == target);
        }
    }
    commands
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opaque_multiline_help_aliases_and_name_only_matching() {
        let mut suggestions = Suggestions::default();
        suggestions.help_result(Some("Available commands:\n/gamerule\n(keepInventory|mod.setting)\n/gamemode <mode> [<target>]\n/gm -> gamemode\n/list [uuids]\n/gamemode duplicate"));
        let names = |draft: &str| {
            suggestions
                .matches(draft)
                .map(|command| command.name.to_string())
                .collect::<Vec<_>>()
        };
        assert_eq!(names("ga"), ["gamerule", "gamemode"]);
        assert_eq!(names("ga ignored arguments"), ["gamerule", "gamemode"]);
        assert_eq!(names("/gamemode nonsense"), ["gamemode"]);
        assert!(names("keepInventory").is_empty());
        assert_eq!(
            suggestions.catalog[0].arguments,
            "(keepInventory|mod.setting)"
        );
        assert_eq!(suggestions.catalog[2].arguments, "-> gamemode");
        assert!(names("@stop").is_empty());
    }

    #[test]
    fn selection_traverses_all_matches_and_completion_preserves_arguments_and_unicode() {
        let mut suggestions = Suggestions::default();
        suggestions.help_result(Some(
            "/sélection <cible>\n/selective [arg]\n/selecta\n/selectb\n/selectc\n/selectd\n/selecte\n/selectf",
        ));
        assert_eq!(suggestions.matches("/se").count(), 7);
        let mut draft = "/sé".to_owned();
        suggestions.navigate(&draft, true);
        let cursor = suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/sélection ");
        assert_eq!(cursor, draft.len());
        draft = "/se".into();
        for _ in 0..6 {
            suggestions.navigate(&draft, true);
        }
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/selecte ");
        draft = "  selecti  keep these args".into();
        suggestions.reset();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "  selective  keep these args");
        suggestions.help_result(None);
        assert!(suggestions.accept(&mut draft).is_none());
    }

    #[test]
    fn aliases_retain_names_and_resolve_chains_without_following_cycles() {
        let mut suggestions = Suggestions::default();
        suggestions.help_result(Some("/waypoint (create|delete)\n/wp -> waypoint\n/w -> wp\n/missing -> absent\n/a -> b\n/b -> a"));
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[1]),
            Some("(create|delete)")
        );
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[2]),
            Some("(create|delete)")
        );
        assert_eq!(suggestions.catalog[1].arguments, "-> waypoint");
        assert_eq!(suggestions.alias_arguments(&suggestions.catalog[3]), None);
        assert_eq!(suggestions.alias_arguments(&suggestions.catalog[4]), None);
        let mut draft = "/wp".to_owned();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/wp ");
    }

    #[test]
    fn builtin_hints_survive_help_failure_and_do_not_insert_argument_spaces() {
        let mut suggestions = Suggestions::default();
        suggestions.builtin_hints([("@stop", "stop"), ("@kill", "kill")].into_iter());
        suggestions.help_result(None);
        let mut draft = "@ki".to_owned();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "@kill");
        suggestions.help_result(Some("/waypoint <name>\n/wp -> waypoint"));
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[3]),
            Some("<name>")
        );
        assert_eq!(suggestions.matches("@").count(), 2);
        let mut empty = "   ".to_owned();
        assert!(suggestions.accept(&mut empty).is_none());
        assert_eq!(empty, "   ");
    }
}
