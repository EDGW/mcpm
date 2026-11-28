//! Session-local command catalog, completion, and unified plain/checked syntax assistance.

use crate::theme::widgets::style;
use mcpm_backend::operations::server::shim::ServerStatus;
use mcpm_rich::Role;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use std::{borrow::Cow, ops::Range};

// Private Modules

mod debug;
mod gamerule;
mod help_parser;
mod syntax;

pub(crate) use help_parser::help_page_count;
pub(super) use syntax::{Diagnostic, Recommendation, Syntax, ValidityPolicy};

const LIMIT: usize = 5;

/// Constructs the checked syntax module for `@debug`.
pub(super) fn debug_syntax() -> Syntax {
    Syntax::checked(debug::DebugSyntax)
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
        definitions: impl Iterator<Item = (&'static str, &'static str, Syntax, ValidityPolicy)>,
    ) {
        self.catalog.extend(
            definitions.map(|(name, description, syntax, validity)| Command {
                kind: CommandKind::Builtin,
                name: Cow::Borrowed(name),
                description: (!description.is_empty()).then_some(description),
                syntax,
                validity,
                alias: None,
            }),
        );
        self.builtin_count = self.catalog.len();
    }

    pub fn builtin_recommendation(&self, name: &str, status: ServerStatus) -> Recommendation {
        self.catalog[..self.builtin_count]
            .iter()
            .find(|command| command.name == name)
            .map_or(ValidityPolicy::RequiresServerDone, |command| {
                command.validity
            })
            .evaluate(status)
    }

    pub fn server_command_recommendation(status: ServerStatus) -> Recommendation {
        ValidityPolicy::RequiresServerDone.evaluate(status)
    }

    pub fn help_result(&mut self, pages: Option<&[String]>) {
        let mut commands = pages.map(help_parser::parse_pages).unwrap_or_default();
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
        let is_builtin = draft.starts_with('@');
        let matches = self.catalog.iter().filter(move |command| {
            command.is_builtin() == is_builtin && command.name.starts_with(name)
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
        let trimmed = draft.trim_start();
        let is_builtin = draft.starts_with('@');
        let command = self
            .catalog
            .iter()
            .find(|command| command.is_builtin() == is_builtin && command.name == name)?;
        let checker = command.syntax.checker()?;
        let start = draft.len() - trimmed.len() + usize::from(trimmed.starts_with('/'));
        let after_name = &draft[start + name.len()..];
        if after_name.is_empty()
            && self.catalog.iter().any(|other| {
                other.is_builtin() == is_builtin
                    && other.name.len() > name.len()
                    && other.name.starts_with(name)
            })
        {
            return None;
        }
        let arguments = after_name.trim_start();
        Some(ArgumentQuery {
            command,
            offset: draft.len() - arguments.len(),
            query: checker.query(arguments),
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
        let (range, replacement, leading_space, append_space) =
            if let Some(arguments) = self.argument_query(draft) {
                let candidate = arguments.query.matches().nth(self.selected.unwrap_or(0))?;
                let range = arguments.query.replacement;
                let range = arguments.offset + range.start..arguments.offset + range.end;
                let leading_space = range.is_empty()
                    && range.start > 0
                    && !draft[..range.start].ends_with(char::is_whitespace);
                let append_space = range.end == draft.len() && candidate.append_space;
                (range, candidate.text, leading_space, append_space)
            } else {
                let command = self.matches(draft).nth(self.selected.unwrap_or(0))?;
                let range = command_range(draft);
                let append_space = range.end == draft.len()
                    && (command.kind == CommandKind::Minecraft || command.syntax.has_arguments());
                (range, command.name.as_ref(), false, append_space)
            };
        let extra = usize::from(leading_space) + usize::from(append_space);
        if draft.len() - range.len() + replacement.len() + extra > 16 * 1024 {
            return None;
        }
        let start = range.start;
        let cursor = start + replacement.len() + extra;
        draft.replace_range(range, replacement);
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
                None => return Some(command.syntax.summary()),
            }
        }
        None // Cyclic aliases keep their original arrow text without a derived syntax hint.
    }

    pub fn view<'a>(&'a self, draft: &'a str, server_status: ServerStatus) -> View<'a> {
        if draft.trim().is_empty() {
            return View {
                rows: Vec::new(),
                title: String::new(),
                selected: None,
                diagnostic: None,
            };
        }
        if let Some(arguments) = self.argument_query(draft) {
            return self.argument_view(arguments, server_status);
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
        let muted = style(Role::Muted).remove_modifier(Modifier::BOLD);
        let warning = style(Role::Warning).remove_modifier(Modifier::BOLD);
        let mut rows = self
            .matches(draft)
            .enumerate()
            .skip(start)
            .take(LIMIT)
            .map(|(index, command)| {
                let matched = prefix.len();
                let summary = command.syntax.summary();
                let recommendation = command.validity.evaluate(server_status);
                let discouraged = matches!(recommendation, Recommendation::Invalid { .. });
                let (prefix_style, matched_style, rest_style, arg_style) = if discouraged {
                    (muted, muted, muted, muted)
                } else {
                    (output, primary, regular, output)
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
                        match command.kind {
                            CommandKind::Builtin => "",
                            CommandKind::Minecraft => "/",
                        },
                        prefix_style,
                    ),
                    Span::styled(&command.name[..matched], matched_style),
                    Span::styled(&command.name[matched..], rest_style),
                ];
                if !summary.is_empty() {
                    spans.push(Span::styled(" ", arg_style));
                    spans.push(Span::styled(summary, arg_style));
                }
                if let Some(arguments) = self
                    .alias_arguments(command)
                    .filter(|args| !args.is_empty())
                {
                    spans.push(Span::styled(" ", arg_style));
                    spans.push(Span::styled(arguments, arg_style));
                }
                if let Some(description) = command.description {
                    spans.push(Span::styled(
                        if summary.is_empty() { " " } else { " · " },
                        muted,
                    ));
                    spans.push(Span::styled(description, muted));
                }
                if let Recommendation::Invalid { reason } = recommendation {
                    spans.push(Span::styled(" · ! ", warning));
                    spans.push(Span::styled(reason, warning));
                }
                Line::from(spans)
            })
            .collect::<Vec<_>>();
        if self.failed && !draft.starts_with('@') {
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

    fn argument_view<'a>(
        &self,
        arguments: ArgumentQuery<'a, '_>,
        server_status: ServerStatus,
    ) -> View<'a> {
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
        let muted = style(Role::Muted).remove_modifier(Modifier::BOLD);
        let warning = style(Role::Warning).remove_modifier(Modifier::BOLD);
        let command_prefix = match arguments.command.kind {
            CommandKind::Builtin => "  ",
            CommandKind::Minecraft => "  /",
        };
        let summary = arguments.command.syntax.summary();
        let mut header_spans = vec![
            Span::raw(command_prefix),
            Span::styled(arguments.command.name.as_ref(), primary),
        ];
        if !summary.is_empty() {
            header_spans.push(Span::raw(" "));
            header_spans.push(Span::raw(summary));
        }
        if let Recommendation::Invalid { reason } =
            arguments.command.validity.evaluate(server_status)
        {
            header_spans.push(Span::styled(" · ! ", warning));
            header_spans.push(Span::styled(reason, warning));
        }
        let mut rows = vec![Line::from(header_spans)];
        let matched = query.prefix.len();
        rows.extend(query.matches().enumerate().skip(start).take(LIMIT).map(
            |(index, candidate)| {
                let mut spans = vec![Span::raw(if selected == Some(index) {
                    "› "
                } else {
                    "  "
                })];
                match candidate.emphasis {
                    syntax::CandidateEmphasis::Primary => {
                        if matched > 0 {
                            spans.push(Span::styled(&candidate.text[..matched], primary));
                        }
                        if matched < candidate.text.len() {
                            spans.push(Span::styled(&candidate.text[matched..], regular));
                        }
                    }
                    syntax::CandidateEmphasis::Normal => {
                        spans.push(Span::styled(candidate.text, output));
                    }
                }
                if let Some(description) = candidate.description {
                    spans.push(Span::styled(" ", muted));
                    spans.push(Span::styled(description, muted));
                }
                Line::from(spans)
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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum CommandKind {
    Builtin,
    Minecraft,
}

pub(super) struct Command {
    pub kind: CommandKind,
    pub name: Cow<'static, str>,
    pub description: Option<&'static str>,
    pub syntax: Syntax,
    pub validity: ValidityPolicy,
    pub alias: Option<usize>,
}

impl Command {
    fn is_builtin(&self) -> bool {
        self.kind == CommandKind::Builtin
    }
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
    query: syntax::SyntaxQuery<'a, 'input>,
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

#[cfg(test)]
mod tests {
    use super::*;

    fn set_help(suggestions: &mut Suggestions, pages: &[&str]) {
        let pages: Vec<String> = pages.iter().map(|page| (*page).to_owned()).collect();
        suggestions.help_result(Some(&pages));
    }

    #[test]
    fn argument_completion_preserves_surrounding_input_and_completes_boolean_values() {
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
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/gamerule keepInventory true");
        draft = "/gamerule keepInventory f".into();
        assert!(
            suggestions
                .view(&draft, ServerStatus::Done)
                .diagnostic
                .is_none()
        );
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/gamerule keepInventory false");

        draft = "  /gamerule  ra  -7".into();
        let cursor = suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "  /gamerule  randomTickSpeed  -7");
        assert_eq!(&draft[..cursor], "  /gamerule  randomTickSpeed");

        draft = "gamerule 規".into();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "gamerule 規則 ");
        assert!(suggestions.accept(&mut draft).is_none());
        let invalid = suggestions.view("  /gamerule  規則  anything", ServerStatus::Done);
        assert!(invalid.diagnostic.is_none());

        let draft = "  /gamerule  randomTickSpeed  wrong";
        let invalid = suggestions
            .view(draft, ServerStatus::Done)
            .diagnostic
            .unwrap();
        assert_eq!(&draft[invalid.range], "wrong");
    }

    #[test]
    fn argument_navigation_uses_all_server_choices_and_help_replacement_clears_checkers() {
        let mut suggestions = Suggestions::default();
        set_help(&mut suggestions, &["/gamerule (a | b | c | d | e | f | g)"]);
        let mut draft = "/gamerule".to_owned();
        suggestions.navigate(&draft, false);
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/gamerule g ");
        set_help(&mut suggestions, &["/gamerule <rule> [value]"]);
        assert!(suggestions.argument_query("/gamerule bad true").is_none());
        assert!(
            suggestions
                .view("/gamerule bad true", ServerStatus::Done)
                .diagnostic
                .is_none()
        );
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
            &[
                "/waypoint (create|delete)\n/wp -> waypoint\n/w -> wp\n/missing -> absent\n/a -> b\n/b -> a",
            ],
        );
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[1]),
            Some("(create|delete)")
        );
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[2]),
            Some("(create|delete)")
        );
        assert_eq!(suggestions.catalog[1].syntax.summary(), "-> waypoint");
        assert_eq!(suggestions.alias_arguments(&suggestions.catalog[3]), None);
        assert_eq!(suggestions.alias_arguments(&suggestions.catalog[4]), None);
        let mut draft = "/wp".to_owned();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "/wp ");
    }

    #[test]
    fn builtin_hints_support_plain_and_checked_syntax_without_rcon_help() {
        let mut suggestions = Suggestions::default();
        suggestions.builtin_hints(
            [
                (
                    "@stop",
                    "stop",
                    Syntax::plain(""),
                    ValidityPolicy::RequiresServerDone,
                ),
                ("@kill", "kill", Syntax::plain(""), ValidityPolicy::Always),
                (
                    "@debug",
                    "set a frontend debug variable",
                    debug_syntax(),
                    ValidityPolicy::Always,
                ),
            ]
            .into_iter(),
        );
        suggestions.help_result(None);

        let mut draft = "@ki".to_owned();
        suggestions.accept(&mut draft).unwrap();
        assert_eq!(draft, "@kill");

        let mut debug_draft = "@de".to_owned();
        suggestions.accept(&mut debug_draft).unwrap();
        assert_eq!(debug_draft, "@debug ");
        suggestions.accept(&mut debug_draft).unwrap();
        assert_eq!(debug_draft, "@debug display_content_range ");
        suggestions.navigate(&debug_draft, false);
        suggestions.accept(&mut debug_draft).unwrap();
        assert_eq!(debug_draft, "@debug display_content_range false");

        let view = suggestions.view(
            "@debug display_content_range",
            ServerStatus::StartingOrUnknown,
        );
        assert_eq!(view.rows[0].to_string(), "  @debug <var_name> <true|false>");
        assert!(view.diagnostic.is_none());

        // @stop is only recommended when ServerStatus == Done; @kill/@debug stay valid.
        assert_eq!(
            suggestions.builtin_recommendation("@stop", ServerStatus::Done),
            Recommendation::Valid
        );
        assert!(matches!(
            suggestions.builtin_recommendation("@stop", ServerStatus::StartingOrUnknown),
            Recommendation::Invalid { .. }
        ));
        assert!(matches!(
            suggestions.builtin_recommendation("@stop", ServerStatus::Stopping),
            Recommendation::Invalid { .. }
        ));
        assert!(matches!(
            suggestions.builtin_recommendation("@stop", ServerStatus::Exited),
            Recommendation::Invalid { .. }
        ));
        assert_eq!(
            suggestions.builtin_recommendation("@kill", ServerStatus::StartingOrUnknown),
            Recommendation::Valid
        );
        let stop_view = suggestions.view("@st", ServerStatus::GeneratingChunks(42));
        assert!(stop_view.rows[0].to_string().contains("42%"));
        let expected_dimmed = style(Role::Muted).remove_modifier(Modifier::BOLD);
        assert_eq!(stop_view.rows[0].spans[2].style, expected_dimmed);
        assert_eq!(stop_view.rows[0].spans[3].style, expected_dimmed);
        let kill_view = suggestions.view("@ki", ServerStatus::GeneratingChunks(42));
        assert_ne!(kill_view.rows[0].spans[2].style, expected_dimmed);

        let invalid = "@debug display_content_range nope";
        let diagnostic = suggestions
            .view(invalid, ServerStatus::Done)
            .diagnostic
            .unwrap();
        assert_eq!(&invalid[diagnostic.range], "nope");

        set_help(&mut suggestions, &["/waypoint <name>\n/wp -> waypoint"]);
        assert_eq!(
            suggestions.alias_arguments(&suggestions.catalog[4]),
            Some("<name>")
        );
        assert_eq!(suggestions.matches("@").count(), 3);
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
            .map(|command| (command.name.as_ref(), command.syntax.summary()))
            .collect();
        assert_eq!(
            entries,
            vec![
                ("advancement", "(grant|revoke)"),
                ("custom", "<player/uuid> [on/off] {a/b} ★ (<unclosed/path)",),
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
            .map(|command| (command.name.as_ref(), command.syntax.summary()))
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
                ("gamerule", "<rule name> <value> OR /gamerule <rule name>",),
            ]
        );
    }
}
