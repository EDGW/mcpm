//! Minecraft server log header parsing and ANSI escape stripping shared across frontend and backend.

use std::{borrow::Cow, ops::Range};

const MAX_TIMESTAMP_BYTES: usize = 64;

/// Recognized severity level from a Minecraft server log header.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LogLevel {
    Trace,
    Debug,
    Info,
    Warn,
    Error,
    Fatal,
}

impl LogLevel {
    /// Returns `true` for severity levels Minecraft uses for crash notices (`ERROR`, `FATAL`, `SEVERE`).
    pub const fn is_error_or_fatal(self) -> bool {
        matches!(self, Self::Error | Self::Fatal)
    }

    fn parse(token: &str) -> Option<Self> {
        match token {
            "TRACE" => Some(Self::Trace),
            "DEBUG" => Some(Self::Debug),
            "INFO" => Some(Self::Info),
            "WARN" | "WARNING" => Some(Self::Warn),
            "ERROR" | "SEVERE" => Some(Self::Error),
            "FATAL" => Some(Self::Fatal),
            _ => None,
        }
    }
}

/// Parsed Minecraft server log header anchored at byte 0 of an ANSI-clean line.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LogHeader<'a> {
    /// Severity level parsed from the header.
    pub level: LogLevel,
    /// Byte range of the level segment to highlight:
    /// - `[Server thread/INFO]`, `[WARN]`, or `[SEVERE]` for standalone level blocks;
    /// - `INFO` / `ERROR` inside `[12:34:56 ERROR]` for combined Paper/Spigot headers.
    pub level_span: Range<usize>,
    /// End byte offset of the primary header bracket block (includes the closing `]`
    /// for combined `[time LEVEL]` headers, equals `level_span.end` otherwise).
    pub prefix_end: usize,
    /// Byte offset where the log message body begins after the header and its separator.
    pub body_start: usize,
    /// Log message body slice after the stripped header.
    pub body: &'a str,
}

/// Strips ANSI CSI, OSC, and ESC terminal escape sequences, avoiding allocation when none exist.
pub fn strip_ansi(input: &str) -> Cow<'_, str> {
    if !input.as_bytes().contains(&0x1b) {
        return Cow::Borrowed(input);
    }
    let mut output = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(ch) = chars.next() {
        if ch != '\x1b' {
            output.push(ch);
            continue;
        }
        match chars.peek().copied() {
            Some('[') => {
                chars.next();
                for next in chars.by_ref() {
                    if ('@'..='~').contains(&next) {
                        break;
                    }
                }
            }
            Some(']') => {
                chars.next();
                while let Some(next) = chars.next() {
                    if next == '\x07' {
                        break;
                    }
                    if next == '\x1b' && chars.peek() == Some(&'\\') {
                        chars.next();
                        break;
                    }
                }
            }
            Some(_) => {
                // Skip optional nF intermediate bytes (e.g. `\x1b(B`) and final byte.
                while let Some(&next) = chars.peek() {
                    chars.next();
                    if !(' '..='/').contains(&next) {
                        break;
                    }
                }
            }
            None => {}
        }
    }
    Cow::Owned(output)
}

/// Parses a Minecraft server log header from an ANSI-clean logical line.
///
/// Recognizes Vanilla/Fabric/Quilt (`[time] [thread/LEVEL]:`), Paper/Spigot/Purpur
/// (`[time LEVEL]:`), Forge/NeoForge/Sponge (`[date time] [thread/LEVEL] [logger]:`),
/// and legacy 1.6.4 JUL (`YYYY-MM-DD HH:MM:SS [SEVERE]`).
///
/// Intentionally conservative: unanchored lines, missing timestamps, unknown levels,
/// and anything past the header's `:` boundary are never treated as header tokens.
pub fn parse_header(line: &str) -> Option<LogHeader<'_>> {
    let first = *line.as_bytes().first()?;
    if first == b'[' {
        parse_bracketed_timestamp(line)
    } else if first.is_ascii_digit() {
        parse_unbracketed_timestamp(line)
    } else {
        None
    }
}

fn parse_bracketed_timestamp(line: &str) -> Option<LogHeader<'_>> {
    let close = find_bracket_close(line, 0)?;
    let inner = &line[1..close];

    // Paper/Spigot combined header: `[12:34:56 ERROR]: ...`
    if let Some((ts, level_str)) = inner.rsplit_once(' ')
        && let Some(level) = LogLevel::parse(level_str)
        && is_timestamp(ts)
    {
        let level_start = 1 + ts.len() + 1;
        let prefix_end = close + 1;
        let body_start = consume_simple_separator(line, prefix_end)?;
        return Some(LogHeader {
            level,
            level_span: level_start..close,
            prefix_end,
            body_start,
            body: &line[body_start..],
        });
    }

    if !is_timestamp(inner) {
        return None;
    }

    let after_ts = skip_horizontal_ws(line, close + 1)?;
    if !line[after_ts..].starts_with('[') {
        return None;
    }
    let level_close = find_bracket_close(line, after_ts)?;
    let (level, has_thread) = parse_level_block(&line[after_ts + 1..level_close])?;
    let prefix_end = level_close + 1;
    let body_start = consume_header_tail(line, prefix_end, has_thread)?;
    Some(LogHeader {
        level,
        level_span: after_ts..prefix_end,
        prefix_end,
        body_start,
        body: &line[body_start..],
    })
}

fn parse_unbracketed_timestamp(line: &str) -> Option<LogHeader<'_>> {
    let limit = line.floor_char_boundary(line.len().min(MAX_TIMESTAMP_BYTES + 2));
    let bracket_offset = line[..limit].find(" [")?;
    let ts = &line[..bracket_offset];
    if !is_timestamp(ts) {
        return None;
    }
    let block_start = bracket_offset + 1;
    let level_close = find_bracket_close(line, block_start)?;
    let (level, _) = parse_level_block(&line[block_start + 1..level_close])?;
    let prefix_end = level_close + 1;
    // JUL headers never carry a trailing `[logger]:` block.
    let body_start = consume_simple_separator(line, prefix_end)?;
    Some(LogHeader {
        level,
        level_span: block_start..prefix_end,
        prefix_end,
        body_start,
        body: &line[body_start..],
    })
}

/// Finds the matching `']'` for a `'['` at `open`, rejecting nested `'['` or line breaks.
fn find_bracket_close(line: &str, open: usize) -> Option<usize> {
    let bytes = line.as_bytes();
    for (offset, &byte) in bytes[open + 1..].iter().enumerate() {
        match byte {
            b']' => return Some(open + 1 + offset),
            b'[' | b'\r' | b'\n' => return None,
            _ => {}
        }
    }
    None
}

fn skip_horizontal_ws(line: &str, start: usize) -> Option<usize> {
    let rest = &line.as_bytes()[start..];
    let count = rest
        .iter()
        .take_while(|&&byte| byte == b' ' || byte == b'\t')
        .count();
    (count > 0).then_some(start + count)
}

fn parse_level_block(inner: &str) -> Option<(LogLevel, bool)> {
    if let Some((thread, level_str)) = inner.rsplit_once('/') {
        if thread.is_empty() {
            return None;
        }
        Some((LogLevel::parse(level_str)?, true))
    } else {
        Some((LogLevel::parse(inner)?, false))
    }
}

/// Consumes a direct separator (`": "`, trailing `":"`, single space/tab, or EOL).
fn consume_simple_separator(line: &str, pos: usize) -> Option<usize> {
    let rest = &line[pos..];
    if rest.is_empty() {
        Some(pos)
    } else if rest.starts_with(": ") || rest.starts_with(":\t") {
        Some(pos + 2)
    } else if rest == ":" {
        Some(pos + 1)
    } else if rest.starts_with(' ') || rest.starts_with('\t') {
        Some(pos + 1)
    } else {
        None
    }
}

/// Consumes the separator after `[thread/LEVEL]` or `[LEVEL]`, permitting at most one
/// Forge/NeoForge/Sponge/FML `[logger]:` block only before any `:` boundary.
fn consume_header_tail(line: &str, pos: usize, allow_logger: bool) -> Option<usize> {
    let rest = &line[pos..];
    if rest.is_empty() {
        return Some(pos);
    }
    if rest.starts_with(": ") || rest.starts_with(":\t") {
        return Some(pos + 2);
    }
    if rest == ":" {
        return Some(pos + 1);
    }
    if !(rest.starts_with(' ') || rest.starts_with('\t')) {
        return None;
    }

    if allow_logger
        && let Some(after_ws) = skip_horizontal_ws(line, pos)
        && line[after_ws..].starts_with('[')
        && let Some(logger_close) = find_bracket_close(line, after_ws)
    {
        let logger = &line[after_ws + 1..logger_close];
        let after_logger = &line[logger_close + 1..];
        let valid_logger = !logger.is_empty() && !logger.chars().any(char::is_whitespace);
        if valid_logger {
            if after_logger.starts_with(": ") || after_logger.starts_with(":\t") {
                return Some(logger_close + 3);
            }
            if after_logger == ":" {
                return Some(logger_close + 2);
            }
        }
    }

    Some(pos + 1)
}

/// Checks whether `s` is a bounded timestamp starting with a digit and ending with
/// `HH:MM:SS` (with optional fractional seconds and timezone suffix).
fn is_timestamp(s: &str) -> bool {
    if s.is_empty() || s.len() > MAX_TIMESTAMP_BYTES {
        return false;
    }
    let bytes = s.as_bytes();
    if !bytes[0].is_ascii_digit() {
        return false;
    }
    if bytes
        .iter()
        .any(|&b| matches!(b, b'[' | b']' | b'<' | b'>' | b'\r' | b'\n'))
    {
        return false;
    }

    let without_tz = strip_timezone_suffix(s);
    let without_frac = strip_fractional_seconds(without_tz);
    if without_frac.len() < 8 {
        return false;
    }
    let time_start = without_frac.len() - 8;
    if !without_frac.is_char_boundary(time_start) {
        return false;
    }
    let (date_prefix, hms) = without_frac.split_at(time_start);
    if !is_hms(hms.as_bytes()) {
        return false;
    }
    date_prefix.is_empty() || date_prefix.ends_with(' ') || date_prefix.ends_with('T')
}

fn strip_timezone_suffix(s: &str) -> &str {
    if let Some(rest) = s.strip_suffix('Z') {
        return rest;
    }
    let bytes = s.as_bytes();
    if bytes.len() >= 6 {
        let tail = &bytes[bytes.len() - 6..];
        if matches!(tail[0], b'+' | b'-')
            && tail[1].is_ascii_digit()
            && tail[2].is_ascii_digit()
            && tail[3] == b':'
            && tail[4].is_ascii_digit()
            && tail[5].is_ascii_digit()
        {
            return &s[..bytes.len() - 6];
        }
    }
    if bytes.len() >= 5 {
        let tail = &bytes[bytes.len() - 5..];
        if matches!(tail[0], b'+' | b'-')
            && tail[1..].iter().all(u8::is_ascii_digit)
        {
            return &s[..bytes.len() - 5];
        }
    }
    s
}

fn strip_fractional_seconds(s: &str) -> &str {
    let bytes = s.as_bytes();
    let digits = bytes
        .iter()
        .rev()
        .take_while(|b| b.is_ascii_digit())
        .count();
    if (1..=9).contains(&digits) && bytes.len() > digits {
        let sep = bytes[bytes.len() - digits - 1];
        if sep == b'.' || sep == b',' {
            return &s[..bytes.len() - digits - 1];
        }
    }
    s
}

fn is_hms(bytes: &[u8]) -> bool {
    if bytes.len() != 8 || bytes[2] != b':' || bytes[5] != b':' {
        return false;
    }
    let Some(hour) = parse_two_digits(bytes[0], bytes[1]) else {
        return false;
    };
    let Some(minute) = parse_two_digits(bytes[3], bytes[4]) else {
        return false;
    };
    let Some(second) = parse_two_digits(bytes[6], bytes[7]) else {
        return false;
    };
    hour <= 23 && minute <= 59 && second <= 60
}

fn parse_two_digits(a: u8, b: u8) -> Option<u8> {
    if a.is_ascii_digit() && b.is_ascii_digit() {
        Some((a - b'0') * 10 + (b - b'0'))
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_minecraft_server_headers_across_loaders_and_locales() {
        let cases = [
            (
                "[12:34:56] [Server thread/INFO]: Done (1.2s)!",
                LogLevel::Info,
                "[Server thread/INFO]",
                "Done (1.2s)!",
            ),
            (
                "[12:34:56.001] [WARN] warning",
                LogLevel::Warn,
                "[WARN]",
                "warning",
            ),
            (
                "[12:34:56 ERROR]: This crash report has been saved to: crash.txt",
                LogLevel::Error,
                "ERROR",
                "This crash report has been saved to: crash.txt",
            ),
            (
                "[06Nov2026 12:34:56.123] [Server Watchdog/FATAL] [net.minecraft.server.dedicated.ServerWatchdog/]: boom",
                LogLevel::Fatal,
                "[Server Watchdog/FATAL]",
                "boom",
            ),
            (
                "[2811月2026 12:34:56.123] [Server thread/ERROR] [net.minecraft.server.MinecraftServer/]: localized",
                LogLevel::Error,
                "[Server thread/ERROR]",
                "localized",
            ),
            (
                "[2026-11-06T12:34:56.123+08:00] [Worker-Main-1/ERROR] [mod/logger]: 中文",
                LogLevel::Error,
                "[Worker-Main-1/ERROR]",
                "中文",
            ),
            (
                "2013-10-15 16:20:00 [SEVERE] This crash report has been saved to: crash.txt",
                LogLevel::Error,
                "[SEVERE]",
                "This crash report has been saved to: crash.txt",
            ),
        ];

        for (line, expected_level, expected_span, expected_body) in cases {
            let parsed = parse_header(line).unwrap_or_else(|| panic!("expected header for {line}"));
            assert_eq!(parsed.level, expected_level, "{line}");
            assert_eq!(&line[parsed.level_span.clone()], expected_span, "{line}");
            assert_eq!(parsed.body, expected_body, "{line}");
        }
    }

    #[test]
    fn never_eats_player_prefixes_or_recognizes_unanchored_lines() {
        // Player prefixes after `:` remain in the body.
        for (line, expected_body) in [
            (
                "[12:34:56] [Server thread/INFO]: <Steve> This crash report has been saved to: x",
                "<Steve> This crash report has been saved to: x",
            ),
            (
                "[12:34:56 INFO]: [Steve]: This crash report has been saved to: x",
                "[Steve]: This crash report has been saved to: x",
            ),
            (
                "[12:34:56] [Server thread/ERROR]: [NotSecure] This crash report has been saved to: x",
                "[NotSecure] This crash report has been saved to: x",
            ),
            (
                "2013-10-15 16:20:00 [SEVERE] [Steve]: This crash report has been saved to: x",
                "[Steve]: This crash report has been saved to: x",
            ),
        ] {
            let parsed = parse_header(line).expect("header parsed");
            assert_eq!(parsed.body, expected_body, "{line}");
        }

        // Unrecognized lines must return None even when containing INFO/WARN/ERROR.
        for line in [
            "ordinary output with [INFO] inside",
            " [12:34:56] [INFO] indented",
            "[not a date] [WARN] text",
            "[12:34:56] [CUSTOM] text",
            "body [12:34:56] [INFO] text",
            "[12:34:56] [/INFO] empty thread",
            "[99:99:99] [ERROR]: impossible time",
            "[12:34:56] [INFO]glued",
            "Server 12:34:56 [INFO] unbracketed prose",
        ] {
            assert!(parse_header(line).is_none(), "should not match: {line}");
        }
    }

    #[test]
    fn strip_ansi_handles_csi_osc_and_charset_escapes() {
        let plain = "[12:34:56] [INFO]: clean";
        assert!(matches!(strip_ansi(plain), Cow::Borrowed(_)));

        let colored =
            "\x1b(B\x1b[31m[12:34:56 ERROR]\x1b[0m: \x1b]0;title\x07This crash report\x1b[m";
        assert_eq!(
            strip_ansi(colored),
            "[12:34:56 ERROR]: This crash report"
        );
    }
}
