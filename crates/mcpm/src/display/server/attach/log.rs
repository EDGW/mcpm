//! Frontend-only rendering of Minecraft server log headers parsed by `mcpm_lib::local::mc::log`;
//! message bodies are never rewritten, and unrecognized lines pass through unstyled.

use crate::theme::{self, widgets::style};
use mcpm_lib::local::mc::log::{LogLevel, parse_header};
use mcpm_rich::Role;
use ratatui::{
    style::{Modifier, Style},
    text::{Line, Span},
};
use std::io::{self, Write};

fn prefix(text: &str) -> Option<(PrefixSpan, Role, bool)> {
    let header = parse_header(text)?;
    let (role, muted_body) = match header.level {
        LogLevel::Info => (Role::Success, true),
        LogLevel::Warn => (Role::Warning, true),
        LogLevel::Error | LogLevel::Fatal => (Role::Danger, false),
        LogLevel::Debug => (Role::Secondary, true),
        LogLevel::Trace => (Role::Muted, true),
    };
    Some((
        PrefixSpan {
            level_start: header.level_span.start,
            level_end: header.level_span.end,
            prefix_end: header.prefix_end,
            body_start: header.body_start,
        },
        role,
        muted_body,
    ))
}

#[derive(Clone, Copy)]
struct PrefixSpan {
    level_start: usize,
    level_end: usize,
    prefix_end: usize,
    body_start: usize,
}

/// Styling state belongs to one source and resets at each logical newline.
#[derive(Default)]
pub(super) struct Formatting {
    muted_body: bool,
    recognized: bool,
}
pub(super) struct Fragment {
    prefix: Option<(PrefixSpan, Role)>,
    muted_body: bool,
    recognized: bool,
}
impl Formatting {
    pub fn fragment(&mut self, text: &str, first: bool, newline: bool) -> Fragment {
        let prefix = if first {
            let parsed = prefix(text);
            self.recognized = parsed.is_some();
            self.muted_body = parsed.is_some_and(|(_, _, muted)| muted);
            parsed.map(|(span, role, _)| (span, role))
        } else {
            None
        };
        let result = Fragment {
            prefix,
            muted_body: self.muted_body,
            recognized: self.recognized,
        };
        if newline {
            self.muted_body = false;
            self.recognized = false;
        }
        result
    }
}

pub(super) fn line<'a>(text: &'a str, fragment: &Fragment, underline_content: bool) -> Line<'a> {
    let body_span = |slice: &'a str, underlined: bool| match (fragment.muted_body, underlined) {
        (true, true) => Span::styled(slice, style(Role::Muted).add_modifier(Modifier::UNDERLINED)),
        (true, false) => Span::styled(slice, style(Role::Muted)),
        (false, true) => Span::styled(slice, Style::new().add_modifier(Modifier::UNDERLINED)),
        (false, false) => Span::raw(slice),
    };
    match fragment.prefix {
        Some((span, role)) => {
            let mut spans = Vec::with_capacity(5);
            spans.push(Span::styled(&text[..span.level_start], style(Role::Muted)));
            spans.push(Span::styled(
                &text[span.level_start..span.level_end],
                style(role),
            ));
            if span.level_end < span.prefix_end {
                spans.push(Span::styled(
                    &text[span.level_end..span.prefix_end],
                    style(Role::Muted),
                ));
            }
            if underline_content {
                if span.prefix_end < span.body_start {
                    spans.push(body_span(&text[span.prefix_end..span.body_start], false));
                }
                if span.body_start < text.len() {
                    spans.push(body_span(&text[span.body_start..], true));
                }
            } else {
                spans.push(body_span(&text[span.prefix_end..], false));
            }
            Line::from(spans)
        }
        None => Line::from(body_span(text, underline_content && fragment.recognized)),
    }
}

pub(super) fn write(
    output: &mut impl Write,
    text: &str,
    fragment: &Fragment,
    color: bool,
    underline_content: bool,
) -> io::Result<()> {
    if !color {
        return output.write_all(text.as_bytes());
    }
    let write_body = |output: &mut dyn Write, slice: &str, underlined: bool| -> io::Result<()> {
        if slice.is_empty() {
            return Ok(());
        }
        match (fragment.muted_body, underlined) {
            (true, true) => write!(
                output,
                "{}",
                theme::console(Role::Muted).underline().style(slice)
            ),
            (true, false) => write!(output, "{}", theme::console(Role::Muted).style(slice)),
            (false, true) => write!(
                output,
                "{}",
                owo_colors::Style::new().underline().style(slice)
            ),
            (false, false) => output.write_all(slice.as_bytes()),
        }
    };
    if let Some((span, role)) = fragment.prefix {
        write!(
            output,
            "{}{}",
            theme::console(Role::Muted).style(&text[..span.level_start]),
            theme::console(role).style(&text[span.level_start..span.level_end])
        )?;
        if span.level_end < span.prefix_end {
            write!(
                output,
                "{}",
                theme::console(Role::Muted).style(&text[span.level_end..span.prefix_end])
            )?;
        }
        if underline_content {
            write_body(output, &text[span.prefix_end..span.body_start], false)?;
            write_body(output, &text[span.body_start..], true)
        } else {
            write_body(output, &text[span.prefix_end..], false)
        }
    } else {
        write_body(output, text, underline_content && fragment.recognized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_optional_threads_dates_paper_jul_and_only_the_prefix() {
        for (text, head) in [
            (
                "[12:34:56] [Server thread/INFO]: Done (1.2s)!",
                "[12:34:56] [Server thread/INFO]",
            ),
            ("[12:34:56.001] [WARN] warning", "[12:34:56.001] [WARN]"),
            ("[12:34:56 ERROR]: Paper error", "[12:34:56 ERROR]"),
            (
                "2013-10-15 16:20:00 [SEVERE] legacy",
                "2013-10-15 16:20:00 [SEVERE]",
            ),
            (
                "[2026-11-06T12:34:56] [Worker-Main-1/ERROR] [mod/logger]: 中文",
                "[2026-11-06T12:34:56] [Worker-Main-1/ERROR]",
            ),
            (
                "[06Nov2026 12:34:56.123] [main/DEBUG] [loader/]: loading",
                "[06Nov2026 12:34:56.123] [main/DEBUG]",
            ),
            (
                "[2811月2026 12:34:56.123] [main/INFO] [loader/]: localized",
                "[2811月2026 12:34:56.123] [main/INFO]",
            ),
        ] {
            let (span, _, _) = prefix(text).expect("valid prefix");
            assert_eq!(&text[..span.prefix_end], head);
            let mut bytes = Vec::new();
            let fragment = Formatting::default().fragment(text, true, true);
            write(&mut bytes, text, &fragment, false, false).unwrap();
            assert_eq!(bytes, text.as_bytes());
        }
    }
    #[test]
    fn leaves_nonmatching_and_embedded_prefixes_untouched() {
        for text in [
            "ordinary output",
            "ordinary output with [INFO] token",
            " [12:34:56] [INFO] indented",
            "[not a date] [WARN] text",
            "[12:34:56] [CUSTOM] text",
            "body [12:34:56] [INFO] text",
            "[12:34:56] [/INFO] empty thread",
        ] {
            assert!(prefix(text).is_none(), "{text}");
            let mut bytes = Vec::new();
            let fragment = Formatting::default().fragment(text, true, true);
            assert!(fragment.prefix.is_none());
            assert!(!fragment.muted_body);
            write(&mut bytes, text, &fragment, true, true).unwrap();
            assert_eq!(bytes, text.as_bytes());
        }
    }
    #[test]
    fn muted_body_continues_only_until_its_own_newline() {
        let mut formatting = Formatting::default();
        for level in ["TRACE", "DEBUG", "INFO", "WARN", "WARNING"] {
            let text = format!("[12:34:56] [main/{level}] text");
            assert!(formatting.fragment(&text, true, false).muted_body);
            assert!(formatting.fragment("continuation", false, true).muted_body);
            assert!(
                !formatting
                    .fragment("unmatched next line", true, true)
                    .muted_body
            );
        }
        for level in ["ERROR", "FATAL", "SEVERE"] {
            let text = format!("[12:34:56] [main/{level}] text");
            let fragment = formatting.fragment(&text, true, true);
            assert!(fragment.prefix.is_some());
            assert!(!fragment.muted_body);
        }
    }
    #[test]
    fn timestamp_and_level_have_separate_style_ranges_including_paper() {
        let text = "[12:34:56] [Server thread/WARNING] message";
        let (span, _, muted) = prefix(text).expect("WARNING prefix");
        assert_eq!(&text[..span.level_start], "[12:34:56] ");
        assert_eq!(&text[span.level_start..span.level_end], "[Server thread/WARNING]");
        assert_eq!(span.level_end, span.prefix_end);
        assert!(muted);

        let paper = "[12:34:56 ERROR]: boom";
        let (span, role, muted) = prefix(paper).expect("Paper prefix");
        assert_eq!(&paper[..span.level_start], "[12:34:56 ");
        assert_eq!(&paper[span.level_start..span.level_end], "ERROR");
        assert_eq!(&paper[span.level_end..span.prefix_end], "]");
        assert_eq!(&paper[span.prefix_end..], ": boom");
        assert_eq!(role, Role::Danger);
        assert!(!muted);
    }

    #[test]
    fn underlines_only_stripped_body_range_on_recognized_lines_and_continuations() {
        let mut formatting = Formatting::default();
        for (text, expected_underlined) in [
            (
                "[12:34:56] [Server thread/INFO]: Done (1.2s)!",
                "Done (1.2s)!",
            ),
            (
                "[12:34:56] [Server thread/INFO] [minecraft/DedicatedServer]: Done (1.2s)!",
                "Done (1.2s)!",
            ),
            ("[12:34:56 ERROR]: <Steve> boom", "<Steve> boom"),
            ("2013-10-15 16:20:00 [INFO] legacy body", "legacy body"),
        ] {
            let fragment = formatting.fragment(text, true, true);
            let rendered = line(text, &fragment, true);
            let underlined: String = rendered
                .spans
                .iter()
                .filter(|span| span.style.add_modifier.contains(Modifier::UNDERLINED))
                .map(|span| span.content.as_ref())
                .collect();
            assert_eq!(underlined, expected_underlined, "{text}");
            // Unchanged when display_content_range is disabled.
            let plain = line(text, &fragment, false);
            assert!(
                plain
                    .spans
                    .iter()
                    .all(|span| !span.style.add_modifier.contains(Modifier::UNDERLINED))
            );
        }

        // Continuation fragment of a recognized line stays underlined until newline resets state.
        let first = formatting.fragment("[12:34:56] [Server thread/ERROR]: part1", true, false);
        assert_eq!(
            line("[12:34:56] [Server thread/ERROR]: part1", &first, true)
                .spans
                .last()
                .map(|s| (s.content.as_ref(), s.style.add_modifier.contains(Modifier::UNDERLINED))),
            Some(("part1", true))
        );
        let cont = formatting.fragment("part2", false, true);
        assert!(
            line("part2", &cont, true)
                .spans
                .iter()
                .all(|s| s.style.add_modifier.contains(Modifier::UNDERLINED))
        );
        let next_unrecognized = formatting.fragment("plain line", true, true);
        assert!(
            line("plain line", &next_unrecognized, true)
                .spans
                .iter()
                .all(|s| !s.style.add_modifier.contains(Modifier::UNDERLINED))
        );
    }
}
