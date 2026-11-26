//! Session-local help catalog, command completion and opt-in argument assistance.

use super::command_hints::{Diagnostic, Handler, Query};
use crate::theme::widgets::style;
use mcpm_rich::Role;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use std::{borrow::Cow, ops::Range};

const LIMIT: usize = 5;
const HELP_TIP: &str =
    "Tip: Use the <tab> key while typing a command to auto-complete the command or its arguments";

/// Extracts the total page count from the first line of a paginated `help` response.
pub(crate) fn help_page_count(output: &str) -> Option<usize> {
    strip_help_header(output).1
}

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
                handler: None,
            }));
        self.builtin_count = self.catalog.len();
    }
    pub fn help_result(&mut self, pages: Option<&[String]>) {
        let mut commands = pages.map(parse_pages).unwrap_or_default();
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

    fn argument_query<'a, 'input>(
        &'a self,
        draft: &'input str,
    ) -> Option<ArgumentQuery<'a, 'input>> {
        let name = input(draft);
        let command = self.catalog.iter().find(|command| command.name == name)?;
        let handler = command.handler.as_ref()?;
        let trimmed = draft.trim_start();
        let start = draft.len() - trimmed.len() + usize::from(trimmed.starts_with('/'));
        let arguments = draft[start + name.len()..].trim_start();
        Some(ArgumentQuery {
            command,
            offset: draft.len() - arguments.len(),
            query: handler.query(arguments),
        })
    }

    pub fn navigate(&mut self, draft: &str, down: bool) {
        let count = self.argument_query(draft).map_or_else(
            || self.matches(draft).count(),
            |arguments| arguments.query.matches().count(),
        );
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

    /// Completion replaces the active command or argument token, preserving surrounding input.
    pub fn accept(&mut self, draft: &mut String) -> Option<usize> {
        if draft.trim().is_empty() {
            return None;
        }
        let (range, candidate, leading_space) = if let Some(arguments) = self.argument_query(draft)
        {
            let candidate = arguments.query.matches().nth(self.selected.unwrap_or(0))?;
            let range = arguments.query.replacement;
            let range = arguments.offset + range.start..arguments.offset + range.end;
            let leading_space = range.is_empty()
                && range.start > 0
                && !draft[..range.start].ends_with(char::is_whitespace);
            (range, candidate, leading_space)
        } else {
            let command = self.matches(draft).nth(self.selected.unwrap_or(0))?;
            (command_range(draft), command.name.as_ref(), false)
        };
        let append_space = range.end == draft.len() && !candidate.starts_with('@');
        let extra = usize::from(leading_space) + usize::from(append_space);
        if draft.len() - range.len() + candidate.len() + extra > 16 * 1024 {
            return None;
        }
        let start = range.start;
        let cursor = start + candidate.len() + extra;
        draft.replace_range(range, candidate);
        if leading_space {
            draft.insert(start, ' ');
        }
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
                diagnostic: None,
            };
        }
        if let Some(arguments) = self.argument_query(draft) {
            return self.argument_view(arguments);
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
            diagnostic: None,
        }
    }

    fn argument_view<'a>(&self, arguments: ArgumentQuery<'a, '_>) -> View<'a> {
        let query = arguments.query;
        let count = query.matches().count();
        let selected = self.selected.filter(|index| *index < count);
        let start = selected.unwrap_or(0).saturating_sub(LIMIT - 1);
        let title = if count == 0 {
            " Arguments ".to_owned()
        } else if let Some(index) = selected {
            format!(" Arguments {}/{count} · ↑/↓ select · Tab fill ", index + 1)
        } else {
            format!(
                " Arguments 1–{}/{count} · ↑/↓ select · Tab fill ",
                count.min(LIMIT)
            )
        };
        let primary = style(Role::Primary);
        let regular = primary.remove_modifier(Modifier::BOLD);
        let output = Style::default().remove_modifier(Modifier::BOLD);
        let mut rows = vec![Line::from(vec![
            Span::raw("  /"),
            Span::styled(arguments.command.name.as_ref(), primary),
            Span::raw(" "),
            Span::raw(arguments.command.arguments.as_ref()),
        ])];
        rows.extend(query.matches().enumerate().skip(start).take(LIMIT).map(
            |(index, candidate)| {
                Line::from(vec![
                    Span::raw(if selected == Some(index) {
                        "› "
                    } else {
                        "  "
                    }),
                    Span::styled(
                        candidate,
                        if (query.validates_value)(candidate) {
                            regular
                        } else {
                            output
                        },
                    ),
                ])
            },
        ));
        View {
            rows,
            title,
            selected: selected.map(|index| index - start + 1),
            diagnostic: query.diagnostic.map(|mut diagnostic| {
                diagnostic.range.start += arguments.offset;
                diagnostic.range.end += arguments.offset;
                diagnostic
            }),
        }
    }
}

struct Command {
    name: Cow<'static, str>,
    arguments: Cow<'static, str>,
    alias: Option<usize>,
    handler: Option<Handler>,
}

pub(super) struct View<'a> {
    pub rows: Vec<Line<'a>>,
    pub title: String,
    pub selected: Option<usize>,
    pub diagnostic: Option<Diagnostic>,
}

struct ArgumentQuery<'a, 'input> {
    command: &'a Command,
    offset: usize,
    query: Query<'a, 'input>,
}

fn command_range(draft: &str) -> Range<usize> {
    let trimmed = draft.trim_start();
    let start = draft.len() - trimmed.len() + usize::from(trimmed.starts_with('/'));
    start..start + input(draft).len()
}

fn input(draft: &str) -> &str {
    let draft = draft.trim_start();
    let draft = draft.strip_prefix('/').unwrap_or(draft);
    let end = draft.find(char::is_whitespace).unwrap_or(draft.len());
    &draft[..end]
}

fn parse_pages(pages: &[String]) -> Vec<Command> {
    let mut commands: Vec<Command> = Vec::new();
    for page in pages {
        parse_page(page, &mut commands);
    }
    for index in 0..commands.len() {
        let command = &mut commands[index];
        command.handler = Handler::from_help(&command.name, &command.arguments);
        if let Some(handler) = &command.handler {
            command.arguments = Cow::Borrowed(handler.syntax());
        }
        if let Some(target) = commands[index].arguments.strip_prefix("->").map(str::trim) {
            commands[index].alias = commands.iter().position(|command| command.name == target);
        }
    }
    commands
}

fn parse_page(page: &str, commands: &mut Vec<Command>) {
    let (body, page_count) = strip_help_header(page);
    let body = if page_count.is_some() {
        strip_paginated_tip(body)
    } else {
        body
    };
    let page_start = commands.len();
    for line in body.lines() {
        for segment in split_help_segments(line) {
            if let Some(entry) = segment.strip_prefix('/') {
                let end = entry.find(char::is_whitespace).unwrap_or(entry.len());
                let name = &entry[..end];
                if name.is_empty() || name.chars().any(char::is_control) {
                    continue;
                }
                if let Some(previous) = commands[page_start..].last_mut()
                    && previous.name == name
                    && (previous.arguments == "OR" || previous.arguments.ends_with(" OR"))
                {
                    previous.arguments.to_mut().push_str(" /");
                    previous.arguments.to_mut().push_str(entry);
                } else if !commands.iter().any(|command| command.name == name) {
                    commands.push(Command {
                        name: Cow::Owned(name.to_owned()),
                        arguments: Cow::Owned(entry[end..].trim().to_owned()),
                        alias: None,
                        handler: None,
                    });
                }
            } else if let Some(command) = commands[page_start..].last_mut() {
                if !command.arguments.is_empty() {
                    command.arguments.to_mut().push(' ');
                }
                command.arguments.to_mut().push_str(segment);
            }
        }
    }
}

fn strip_help_header(page: &str) -> (&str, Option<usize>) {
    let first_line_len = page.find(['\r', '\n']).unwrap_or(page.len());
    let first_line = &page[..first_line_len];
    let trimmed = first_line.trim_start();
    let leading_ws = first_line.len() - trimmed.len();
    let open = trimmed.bytes().take_while(|&byte| byte == b'-').count();
    if open < 3 {
        return (page, None);
    }
    let after_open = &trimmed[open..];
    let Some(close) = after_open.find("---") else {
        return (page, None);
    };
    let Some(total) = parse_header_total_pages(&after_open[..close]) else {
        return (page, None);
    };
    let close_dashes = after_open[close..]
        .bytes()
        .take_while(|&byte| byte == b'-')
        .count();
    let end = leading_ws + open + close + close_dashes;
    (&page[end..], Some(total))
}

fn parse_header_total_pages(header: &str) -> Option<usize> {
    let tokens: Vec<&str> = header.split_whitespace().collect();
    for window in tokens.windows(3) {
        if window[1].eq_ignore_ascii_case("of")
            && let Ok(current) = window[0].parse::<usize>()
            && let Ok(total) = window[2].parse::<usize>()
            && current > 0
            && total > 0
        {
            return Some(total);
        }
    }
    for token in tokens {
        let inner = token.trim_matches(|ch| matches!(ch, '(' | ')' | '[' | ']'));
        if let Some((current, total)) = inner.split_once('/')
            && let Ok(current) = current.parse::<usize>()
            && let Ok(total) = total.parse::<usize>()
            && current > 0
            && total > 0
        {
            return Some(total);
        }
    }
    None
}

fn strip_paginated_tip(text: &str) -> &str {
    text.trim_end()
        .strip_suffix(HELP_TIP)
        .map(str::trim_end)
        .unwrap_or(text)
}

fn split_help_segments(line: &str) -> Vec<&str> {
    let mut segments = Vec::new();
    let mut stack = Vec::new();
    let mut start = 0;
    let mut has_content = false;
    let mut consecutive_slash = false;

    for (index, ch) in line.char_indices() {
        match ch {
            '(' | '[' | '<' | '{' => {
                stack.push(ch);
                has_content = true;
                consecutive_slash = false;
            }
            ')' | ']' | '>' | '}' => {
                let expected = match ch {
                    ')' => '(',
                    ']' => '[',
                    '>' => '<',
                    '}' => '{',
                    _ => unreachable!(),
                };
                if let Some(pos) = stack.iter().rposition(|&open| open == expected) {
                    stack.truncate(pos);
                }
                has_content = true;
                consecutive_slash = false;
            }
            '/' if stack.is_empty() && has_content && !consecutive_slash => {
                let segment = line[start..index].trim();
                if !segment.is_empty() {
                    segments.push(segment);
                }
                start = index;
                consecutive_slash = true;
            }
            '/' => {
                has_content = true;
                consecutive_slash = stack.is_empty();
            }
            ch if ch.is_whitespace() => {
                consecutive_slash = false;
            }
            _ => {
                has_content = true;
                consecutive_slash = false;
            }
        }
    }

    let tail = line[start..].trim();
    if !tail.is_empty() {
        segments.push(tail);
    }
    segments
}

#[cfg(test)]
mod tests {
    use super::*;

    fn set_help(suggestions: &mut Suggestions, pages: &[&str]) {
        let pages: Vec<String> = pages.iter().map(|page| (*page).to_owned()).collect();
        suggestions.help_result(Some(&pages));
    }

    #[test]
    fn argument_completion_preserves_surrounding_input_and_stops_at_value() {
        let mut suggestions = Suggestions::default();
        set_help(
            &mut suggestions,
            &["/gamerule\n(keepInventory | randomTickSpeed | 規則) <value>"],
        );
        let mut draft = "/gamerul".to_owned();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/gamerule ");
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/gamerule keepInventory ");
        assert!(suggestions.accept(&mut draft).is_none());
        draft = "  /gamerule  ra  -7".into();
        let cursor = suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "  /gamerule  randomTickSpeed  -7");
        assert_eq!(&draft[..cursor], "  /gamerule  randomTickSpeed");
        draft = "gamerule 規".into();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "gamerule 規則 ");
        let invalid = suggestions.view("  /gamerule  規則  anything");
        assert!(invalid.diagnostic.is_none());
        let draft = "  /gamerule  randomTickSpeed  wrong";
        let invalid = suggestions.view(draft).diagnostic.unwrap();
        assert_eq!(&draft[invalid.range], "wrong");
    }

    #[test]
    fn argument_navigation_uses_all_server_choices_and_help_replacement_clears_handlers() {
        let mut suggestions = Suggestions::default();
        set_help(&mut suggestions, &["/gamerule (a | b | c | d | e | f | g)"]);
        let mut draft = "/gamerule".to_owned();
        suggestions.navigate(&draft, false);
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/gamerule g ");
        set_help(&mut suggestions, &["/gamerule <rule> [value]"]);
        assert!(suggestions.argument_query("/gamerule bad true").is_none());
        assert!(suggestions.view("/gamerule bad true").diagnostic.is_none());
        suggestions.help_result(None);
        assert!(suggestions.argument_query("/gamerule").is_none());
    }

    #[test]
    fn selection_traverses_all_matches_and_completion_preserves_arguments_and_unicode() {
        let mut suggestions = Suggestions::default();
        set_help(
            &mut suggestions,
            &[
                "/sélection <cible>\n/selective [arg]\n/selecta\n/selectb\n/selectc\n/selectd\n/selecte\n/selectf",
            ],
        );
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
        set_help(
            &mut suggestions,
            &["/waypoint (create|delete)\n/wp -> waypoint\n/w -> wp\n/missing -> absent\n/a -> b\n/b -> a"],
        );
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
        set_help(&mut suggestions, &["/waypoint <name>\n/wp -> waypoint"]);
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[3]),
            Some("<name>")
        );
        assert_eq!(suggestions.matches("@").count(), 2);
        let mut empty = "   ".to_owned();
        assert!(suggestions.accept(&mut empty).is_none());
        assert_eq!(empty, "   ");
    }

    #[test]
    fn splits_glued_rcon_help_with_bracket_stack_without_validation() {
        let mut suggestions = Suggestions::default();
        set_help(
            &mut suggestions,
            &[concat!(
                "/advancement (grant|revoke)",
                "/custom <player/uuid> [on/off] {a/b} ★ (<unclosed/path)",
                "/gamerule (keepInventory|randomTickSpeed)",
                "/experience (add|set|query)",
                "/xp -> experience",
                "//wand",
                "//set <pattern>",
                "/stop",
            )],
        );
        let entries: Vec<(&str, &str)> = suggestions
            .catalog
            .iter()
            .map(|command| (command.name.as_ref(), command.arguments.as_ref()))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("advancement", "(grant|revoke)"),
                (
                    "custom",
                    "<player/uuid> [on/off] {a/b} ★ (<unclosed/path)",
                ),
                ("gamerule", "<type> <value>"),
                ("experience", "(add|set|query)"),
                ("xp", "-> experience"),
                ("/wand", ""),
                ("/set", "<pattern>"),
                ("stop", ""),
            ]
        );
        assert!(suggestions.argument_query("/gamerule keep").is_some());
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[4]),
            Some("(add|set|query)")
        );
    }

    #[test]
    fn parses_paginated_help_headers_tips_and_or_usages() {
        let page1 = concat!(
            "--- Showing help page 1 of 10 (/help <page>) ---",
            "/effect <player> <effect> [seconds] [amplifier] [hideParticles] OR /effect <player> clear",
            "/clone <x1> <y1> <z1> <x2> <y2> <z2> <x> <y> <z> [maskMode] [cloneMode]",
            "Tip: Use the <tab> key while typing a command to auto-complete the command or its arguments",
        );
        let page2 = concat!(
            "--- Showing help page 2 of 10 (/help <page>) ---",
            "/list",
            "/spawnpoint OR /spawnpoint <player> OR /spawnpoint <player> <x> <y> <z>",
            "/list",
            "/gamerule <rule name> <value> OR /gamerule <rule name>",
        );
        assert_eq!(help_page_count(page1), Some(10));
        assert_eq!(
            help_page_count("--------- Help: Index (1/120) ---------------------------\n/stop"),
            Some(120)
        );
        assert_eq!(help_page_count("/advancement (grant|revoke)/stop"), None);

        let mut suggestions = Suggestions::default();
        set_help(&mut suggestions, &[page1, page2]);
        let entries: Vec<(&str, &str)> = suggestions
            .catalog
            .iter()
            .map(|command| (command.name.as_ref(), command.arguments.as_ref()))
            .collect();
        assert_eq!(
            entries,
            vec![
                (
                    "effect",
                    "<player> <effect> [seconds] [amplifier] [hideParticles] OR /effect <player> clear",
                ),
                (
                    "clone",
                    "<x1> <y1> <z1> <x2> <y2> <z2> <x> <y> <z> [maskMode] [cloneMode]",
                ),
                ("list", ""),
                (
                    "spawnpoint",
                    "OR /spawnpoint <player> OR /spawnpoint <player> <x> <y> <z>",
                ),
                (
                    "gamerule",
                    "<rule name> <value> OR /gamerule <rule name>",
                ),
            ]
        );
    }
}
