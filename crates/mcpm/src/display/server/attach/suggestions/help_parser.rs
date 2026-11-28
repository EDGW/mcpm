//! RCON `help` response parser: strips pagination framing, splits glued entries, resolves aliases,
//! and upgrades recognized command syntax into structured [`SyntaxChecker`] instances.

use super::{
    Command, CommandKind,
    gamerule::GameruleSyntax,
    syntax::{Syntax, ValidityPolicy},
};
use std::borrow::Cow;

const HELP_TIP: &str =
    "Tip: Use the <tab> key while typing a command to auto-complete the command or its arguments";

/// Extracts the total page count from the first line of a paginated `help` response.
pub(crate) fn help_page_count(output: &str) -> Option<usize> {
    strip_help_header(output).1
}

/// Parses one or more `help` response pages into Minecraft command suggestions.
pub(super) fn parse_pages(pages: &[String]) -> Vec<Command> {
    let mut entries: Vec<RawEntry> = Vec::new();
    for page in pages {
        parse_page(page, &mut entries);
    }
    let aliases: Vec<Option<usize>> = entries
        .iter()
        .map(|entry| {
            let target = entry.arguments.strip_prefix("->").map(str::trim)?;
            entries.iter().position(|other| other.name == target)
        })
        .collect();

    entries
        .into_iter()
        .zip(aliases)
        .map(|(entry, alias)| {
            let syntax = syntax_from_help(&entry.name, entry.arguments);
            Command {
                kind: CommandKind::Minecraft,
                name: Cow::Owned(entry.name),
                description: None,
                syntax,
                validity: ValidityPolicy::RequiresServerDone,
                alias,
            }
        })
        .collect()
}

struct RawEntry {
    name: String,
    arguments: String,
}

fn syntax_from_help(name: &str, arguments: String) -> Syntax {
    match name {
        "gamerule" => GameruleSyntax::from_help(&arguments)
            .map(Syntax::checked)
            .unwrap_or_else(|| Syntax::plain(arguments)),
        _ => Syntax::plain(arguments),
    }
}

fn parse_page(page: &str, entries: &mut Vec<RawEntry>) {
    let (body, page_count) = strip_help_header(page);
    let body = if page_count.is_some() {
        strip_paginated_tip(body)
    } else {
        body
    };
    let page_start = entries.len();
    for line in body.lines() {
        for segment in split_help_segments(line) {
            if let Some(entry) = segment.strip_prefix('/') {
                let end = entry.find(char::is_whitespace).unwrap_or(entry.len());
                let name = &entry[..end];
                if name.is_empty() || name.chars().any(char::is_control) {
                    continue;
                }
                if let Some(previous) = entries[page_start..].last_mut()
                    && previous.name == name
                    && (previous.arguments == "OR" || previous.arguments.ends_with(" OR"))
                {
                    previous.arguments.push_str(" /");
                    previous.arguments.push_str(entry);
                } else if !entries.iter().any(|existing| existing.name == name) {
                    entries.push(RawEntry {
                        name: name.to_owned(),
                        arguments: entry[end..].trim().to_owned(),
                    });
                }
            } else if let Some(entry) = entries[page_start..].last_mut() {
                if !entry.arguments.is_empty() {
                    entry.arguments.push(' ');
                }
                entry.arguments.push_str(segment);
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
