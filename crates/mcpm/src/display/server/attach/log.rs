//! Frontend-only recognition of Minecraft Log4j prefixes; message bodies are never rewritten.

use crate::theme::{self, Role, widgets::style};
use ratatui::text::{Line, Span};
use regex::Regex;
use std::{
    io::{self, Write},
    sync::LazyLock,
};

// Vanilla/Sponge: [%d{HH:mm:ss}] [%t/%level]; Forge also uses ddMMMyyyy timestamps.
// https://docs.spongepowered.org/stable/en/server/spongineer/logs.html
static PREFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(concat!(
    r"^\[(?:[0-9]{2}:[0-9]{2}:[0-9]{2}(?:[.,][0-9]{1,9})?|",
    r"[0-9]{4}-[0-9]{2}-[0-9]{2}[ T][0-9]{2}:[0-9]{2}:[0-9]{2}(?:[.,][0-9]{1,9})?(?:Z|[+-][0-9]{2}:?[0-9]{2})?|",
    r"[0-9]{2}[A-Za-z]{3}[0-9]{4} [0-9]{2}:[0-9]{2}:[0-9]{2}(?:[.,][0-9]{1,9})?)\]",
    r"[ \t]+(?P<block>\[(?:[^\]\r\n]+/)?(?P<level>TRACE|DEBUG|INFO|WARN|WARNING|ERROR|FATAL)\])"
)).expect("static Minecraft prefix expression")
});

pub(super) fn prefix(text: &str) -> Option<(usize, usize, Role, bool)> {
    let captures = PREFIX.captures(text)?;
    let (role, muted_body) = match captures.name("level")?.as_str() {
        "INFO" => (Role::Success, true),
        "WARN" | "WARNING" => (Role::Warning, true),
        "ERROR" | "FATAL" => (Role::Danger, false),
        "DEBUG" => (Role::Secondary, true),
        "TRACE" => (Role::Muted, true),
        _ => return None,
    };
    let block = captures.name("block")?;
    Some((block.start(), block.end(), role, muted_body))
}

/// Styling state belongs to one source and resets at each logical newline.
#[derive(Default)]
pub(super) struct Formatting {
    muted_body: bool,
}
pub(super) struct Fragment {
    prefix: Option<(usize, usize, Role)>,
    muted_body: bool,
}
impl Formatting {
    pub fn fragment(&mut self, text: &str, first: bool, newline: bool) -> Fragment {
        let prefix = if first {
            let parsed = prefix(text);
            self.muted_body = parsed.is_some_and(|(_, _, _, muted)| muted);
            parsed.map(|(start, end, role, _)| (start, end, role))
        } else {
            None
        };
        let result = Fragment {
            prefix,
            muted_body: self.muted_body,
        };
        if newline {
            self.muted_body = false;
        }
        result
    }
}

pub(super) fn line<'a>(text: &'a str, fragment: &Fragment) -> Line<'a> {
    let body = |text: &'a str| {
        if fragment.muted_body {
            Span::styled(text, style(Role::Muted))
        } else {
            Span::raw(text)
        }
    };
    match fragment.prefix {
        Some((start, end, role)) => Line::from(vec![
            Span::styled(&text[..start], style(Role::Muted)),
            Span::styled(&text[start..end], style(role)),
            body(&text[end..]),
        ]),
        None => Line::from(body(text)),
    }
}

pub(super) fn write(
    output: &mut impl Write,
    text: &str,
    fragment: &Fragment,
    color: bool,
) -> io::Result<()> {
    if !color {
        return output.write_all(text.as_bytes());
    }
    let end = if let Some((start, end, role)) = fragment.prefix {
        write!(
            output,
            "{}{}",
            theme::console(Role::Muted).style(&text[..start]),
            theme::console(role).style(&text[start..end])
        )?;
        end
    } else {
        0
    };
    if fragment.muted_body {
        // The library resets styling after this fragment, including at a logical line boundary.
        write!(
            output,
            "{}",
            theme::console(Role::Muted).style(&text[end..])
        )
    } else {
        output.write_all(text[end..].as_bytes())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn recognizes_optional_threads_dates_and_only_the_prefix() {
        for (text, head) in [
            (
                "[12:34:56] [Server thread/INFO]: Done (1.2s)!",
                "[12:34:56] [Server thread/INFO]",
            ),
            ("[12:34:56.001] [WARN] warning", "[12:34:56.001] [WARN]"),
            (
                "[2026-11-06T12:34:56] [Worker-Main-1/ERROR] [mod/logger]: 中文",
                "[2026-11-06T12:34:56] [Worker-Main-1/ERROR]",
            ),
            (
                "[06Nov2026 12:34:56.123] [main/DEBUG] [loader/]: loading",
                "[06Nov2026 12:34:56.123] [main/DEBUG]",
            ),
        ] {
            let (_, end, _, _) = prefix(text).expect("valid prefix");
            assert_eq!(&text[..end], head);
            let mut bytes = Vec::new();
            let fragment = Formatting::default().fragment(text, true, true);
            write(&mut bytes, text, &fragment, false).unwrap();
            assert_eq!(bytes, text.as_bytes());
        }
    }
    #[test]
    fn leaves_nonmatching_and_embedded_prefixes_untouched() {
        for text in [
            "ordinary output",
            " [12:34:56] [INFO] indented",
            "[not a date] [WARN] text",
            "[12:34:56] [CUSTOM] text",
            "body [12:34:56] [INFO] text",
            "[12:34:56] [/INFO] empty thread",
        ] {
            assert!(prefix(text).is_none(), "{text}");
            let mut bytes = Vec::new();
            let fragment = Formatting::default().fragment(text, true, true);
            write(&mut bytes, text, &fragment, true).unwrap();
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
        for level in ["ERROR", "FATAL"] {
            let text = format!("[12:34:56] [main/{level}] text");
            let fragment = formatting.fragment(&text, true, true);
            assert!(fragment.prefix.is_some());
            assert!(!fragment.muted_body);
        }
    }
    #[test]
    fn timestamp_and_warning_alias_have_separate_style_ranges() {
        let text = "[12:34:56] [Server thread/WARNING] message";
        let (start, end, _, muted) = prefix(text).expect("WARNING prefix");
        assert_eq!(&text[..start], "[12:34:56] ");
        assert_eq!(&text[start..end], "[Server thread/WARNING]");
        assert!(muted);
    }
}
