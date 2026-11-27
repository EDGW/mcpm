//! Minecraft crash report header parsing and classification across 1.6.4–26.1 formats.

use chrono::{DateTime, NaiveDate, NaiveDateTime, NaiveTime};
use serde::{Deserialize, Serialize};
use std::{
    fs::File,
    io::{self, BufRead, BufReader},
    path::{Path, PathBuf},
};
use thiserror::Error;

const CRASH_REPORT_BANNER: &str = "---- Minecraft Crash Report ----";
const WALKTHROUGH_HEADER: &str =
    "A detailed walkthrough of the error, its code path and all known details is as follows:";
const WATCHDOG_DESCRIPTION: &str = "Watching Server";
const MAX_HEADER_LINES: usize = 64;
const MAX_HEADER_BYTES: usize = 64 * 1024;

/// Classified category of a Minecraft crash report.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CrashReportType {
    /// Dedicated server hang detected by `ServerWatchdog` (`Description: Watching Server`).
    Watchdog,
    /// Any other crash report with a recognized `Description:` header.
    Others,
    /// Valid crash report banner without a non-empty `Description:` header.
    Unknown,
}

/// Parsed header summary of a Minecraft crash report.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CrashReportAnalysis {
    #[serde(rename = "type")]
    pub report_type: CrashReportType,
    pub desc: Option<String>,
    pub time: Option<NaiveDateTime>,
}

/// Analyses a Minecraft crash report file on disk using a bounded header read.
pub fn analyse_file(path: &Path) -> Result<CrashReportAnalysis, CrashReportError> {
    let metadata = std::fs::metadata(path).map_err(|source| CrashReportError::Read {
        path: path.to_owned(),
        source,
    })?;
    if !metadata.is_file() {
        return Err(CrashReportError::NotAFile {
            path: path.to_owned(),
        });
    }
    let file = File::open(path).map_err(|source| CrashReportError::Read {
        path: path.to_owned(),
        source,
    })?;
    analyse_reader(BufReader::new(file), path)
}

/// Analyses a Minecraft crash report from any buffered reader without loading the full body.
pub fn analyse_reader(
    mut reader: impl BufRead,
    path: &Path,
) -> Result<CrashReportAnalysis, CrashReportError> {
    let mut raw = Vec::new();
    let mut total_bytes = 0usize;

    let first_len = read_bounded_line(&mut reader, &mut raw, MAX_HEADER_BYTES).map_err(|source| {
        CrashReportError::Read {
            path: path.to_owned(),
            source,
        }
    })?;
    if first_len == 0 {
        return Err(CrashReportError::InvalidFormat {
            path: path.to_owned(),
            reason: "file is empty",
        });
    }
    total_bytes += first_len;

    let first_line = decode_line(&raw);
    let first_line = first_line.strip_prefix('\u{feff}').unwrap_or(&first_line);
    if first_line != CRASH_REPORT_BANNER {
        return Err(CrashReportError::InvalidFormat {
            path: path.to_owned(),
            reason: "first line is not a Minecraft crash report banner",
        });
    }

    let mut desc: Option<String> = None;
    let mut desc_seen = false;
    let mut time: Option<NaiveDateTime> = None;
    let mut time_seen = false;

    for _ in 1..MAX_HEADER_LINES {
        if total_bytes >= MAX_HEADER_BYTES || (desc_seen && time_seen) {
            break;
        }
        let remaining = MAX_HEADER_BYTES - total_bytes;
        let read =
            read_bounded_line(&mut reader, &mut raw, remaining).map_err(|source| {
                CrashReportError::Read {
                    path: path.to_owned(),
                    source,
                }
            })?;
        if read == 0 {
            break;
        }
        total_bytes += read;

        let line = decode_line(&raw);
        if line == WALKTHROUGH_HEADER {
            break;
        }
        if !desc_seen
            && let Some(raw_desc) = line.strip_prefix("Description:")
        {
            desc_seen = true;
            let trimmed = raw_desc.trim();
            if !trimmed.is_empty() {
                desc = Some(trimmed.to_owned());
            }
            continue;
        }
        if !time_seen
            && let Some(raw_time) = line.strip_prefix("Time:")
        {
            time_seen = true;
            time = parse_crash_time(raw_time);
        }
    }

    let report_type = match desc.as_deref() {
        Some(WATCHDOG_DESCRIPTION) => CrashReportType::Watchdog,
        Some(_) => CrashReportType::Others,
        None => CrashReportType::Unknown,
    };

    Ok(CrashReportAnalysis {
        report_type,
        desc,
        time,
    })
}

/// Errors produced while reading or validating a crash report file.
#[derive(Debug, Error)]
pub enum CrashReportError {
    #[error("failed to read crash report {path:?}")]
    Read {
        path: PathBuf,
        #[source]
        source: io::Error,
    },
    #[error("crash report path {path:?} is not a regular file")]
    NotAFile { path: PathBuf },
    #[error("invalid Minecraft crash report {path:?}: {reason}")]
    InvalidFormat {
        path: PathBuf,
        reason: &'static str,
    },
}

fn read_bounded_line(
    reader: &mut impl BufRead,
    buffer: &mut Vec<u8>,
    max_bytes: usize,
) -> io::Result<usize> {
    buffer.clear();
    let mut read = 0usize;
    while read < max_bytes {
        let available = reader.fill_buf()?;
        if available.is_empty() {
            break;
        }
        let limit = available.len().min(max_bytes - read);
        let slice = &available[..limit];
        if let Some(pos) = slice.iter().position(|&b| b == b'\n') {
            let take = pos + 1;
            buffer.extend_from_slice(&slice[..take]);
            reader.consume(take);
            read += take;
            break;
        }
        buffer.extend_from_slice(slice);
        reader.consume(limit);
        read += limit;
    }
    Ok(read)
}

fn decode_line(bytes: &[u8]) -> String {
    let mut line = String::from_utf8_lossy(bytes).into_owned();
    while line.ends_with(['\n', '\r']) {
        line.pop();
    }
    line
}

// region: time-parsing

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Meridiem {
    Am,
    Pm,
}

/// Parses Minecraft `Time:` header values across modern ISO (`yyyy-MM-dd HH:mm:ss`) and
/// legacy Java `SimpleDateFormat` locale formats (US, EU, CJK, CLDR narrow-space, etc.).
pub(crate) fn parse_crash_time(raw: &str) -> Option<NaiveDateTime> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }

    // 1. Exact RFC 3339 / ISO with explicit timezone offset.
    if let Ok(dt) = DateTime::parse_from_rfc3339(trimmed) {
        return Some(dt.naive_local());
    }
    for fmt in [
        "%Y-%m-%d %H:%M:%S%.f %#z",
        "%Y-%m-%d %H:%M:%S %#z",
        "%Y-%m-%d %H:%M:%S%.f%#z",
        "%Y-%m-%d %H:%M:%S%#z",
        "%Y-%m-%dT%H:%M:%S%.f%#z",
        "%Y-%m-%dT%H:%M:%S%#z",
    ] {
        if let Ok(dt) = DateTime::parse_from_str(trimmed, fmt) {
            return Some(dt.naive_local());
        }
    }

    let normalized = normalize_time_string(trimmed);

    // 2. Named-month formats (e.g. "Oct 15, 2013 4:20:00 PM", "15 Oct 2013 16:20:00").
    if let Some(dt) = parse_named_month(&normalized) {
        return Some(dt);
    }

    // 3. Structural parser for numeric dates + 12h/24h times across locales.
    parse_numeric_datetime(&normalized)
}

fn normalize_time_string(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut prev_space = false;
    for ch in input.chars() {
        if ch.is_whitespace() || matches!(ch, '\u{202f}' | '\u{00a0}' | '\u{3000}') {
            if !prev_space {
                out.push(' ');
                prev_space = true;
            }
        } else {
            out.push(ch);
            prev_space = false;
        }
    }
    out.trim().to_owned()
}

fn parse_named_month(text: &str) -> Option<NaiveDateTime> {
    let cleaned = normalize_meridiem_text(text).0;
    for fmt in [
        "%b %e, %Y %I:%M:%S %p",
        "%b %e, %Y, %I:%M:%S %p",
        "%b %e, %Y %I:%M %p",
        "%b %e, %Y, %I:%M %p",
        "%b %e, %Y %H:%M:%S",
        "%b %e, %Y, %H:%M:%S",
        "%b %e, %Y %H:%M",
        "%b %e, %Y, %H:%M",
        "%B %e, %Y %I:%M:%S %p",
        "%B %e, %Y, %I:%M:%S %p",
        "%B %e, %Y %I:%M %p",
        "%B %e, %Y, %I:%M %p",
        "%B %e, %Y %H:%M:%S",
        "%B %e, %Y %H:%M",
        "%e %b %Y %H:%M:%S",
        "%e %b %Y, %H:%M:%S",
        "%e %b %Y %H:%M",
        "%e %b %Y, %H:%M",
        "%e %b %Y %I:%M:%S %p",
        "%e %b %Y %I:%M %p",
        "%e %B %Y %H:%M:%S",
        "%e %B %Y %H:%M",
        "%a %b %e %H:%M:%S %Y",
    ] {
        if let Ok(dt) = NaiveDateTime::parse_from_str(&cleaned, fmt) {
            return Some(dt);
        }
    }
    None
}

/// Replaces localized AM/PM tokens with padded ASCII ` AM ` / ` PM `.
fn normalize_meridiem_text(input: &str) -> (String, Option<Meridiem>, bool) {
    let mut text = input.to_owned();
    let mut meridiem = None;
    let mut cjk = false;

    for (marker, value) in [
        ("上午", Meridiem::Am),
        ("下午", Meridiem::Pm),
        ("午前", Meridiem::Am),
        ("午後", Meridiem::Pm),
        ("오전", Meridiem::Am),
        ("오후", Meridiem::Pm),
    ] {
        if text.contains(marker) {
            meridiem = Some(value);
            cjk = true;
            let replacement = if value == Meridiem::Am { " AM " } else { " PM " };
            text = text.replace(marker, replacement);
        }
    }

    // Handle Latin variants like `a.m.`, `p.m.`, `a. m.`, `p. m.` case-insensitively.
    let lower = text.to_ascii_lowercase();
    for (pattern, value) in [
        ("a. m.", Meridiem::Am),
        ("p. m.", Meridiem::Pm),
        ("a.m.", Meridiem::Am),
        ("p.m.", Meridiem::Pm),
    ] {
        if let Some(idx) = lower.find(pattern) {
            meridiem = Some(value);
            let replacement = if value == Meridiem::Am { " AM " } else { " PM " };
            text.replace_range(idx..idx + pattern.len(), replacement);
            break;
        }
    }

    // Detect standalone `AM`/`PM` tokens (including before a trailing timezone) or glued `4:20PM`.
    if meridiem.is_none() {
        let mut rebuilt = Vec::new();
        for token in text.split_whitespace() {
            let lower = token.to_ascii_lowercase();
            if lower == "am" {
                meridiem = Some(Meridiem::Am);
                rebuilt.push("AM".to_owned());
            } else if lower == "pm" {
                meridiem = Some(Meridiem::Pm);
                rebuilt.push("PM".to_owned());
            } else if lower.ends_with("am")
                && token[..token.len() - 2]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_digit())
            {
                meridiem = Some(Meridiem::Am);
                rebuilt.push(token[..token.len() - 2].to_owned());
                rebuilt.push("AM".to_owned());
            } else if lower.ends_with("pm")
                && token[..token.len() - 2]
                    .chars()
                    .next_back()
                    .is_some_and(|c| c.is_ascii_digit())
            {
                meridiem = Some(Meridiem::Pm);
                rebuilt.push(token[..token.len() - 2].to_owned());
                rebuilt.push("PM".to_owned());
            } else {
                rebuilt.push(token.to_owned());
            }
        }
        text = rebuilt.join(" ");
    }

    (normalize_time_string(&text), meridiem, cjk)
}

fn parse_numeric_datetime(input: &str) -> Option<NaiveDateTime> {
    let (mut text, meridiem, cjk_meridiem) = normalize_meridiem_text(input);

    // Strip the normalized AM/PM token so only date and time remain.
    if meridiem.is_some() {
        text = text
            .split_whitespace()
            .filter(|tok| !matches!(*tok, "AM" | "PM"))
            .collect::<Vec<_>>()
            .join(" ");
    }

    // Normalize CJK date separators (`2021年8月1日` -> `2021-8-1 `) and optional commas.
    let mut cjk_date = false;
    if text.contains('年') && text.contains('月') {
        cjk_date = true;
        text = text
            .replace('年', "-")
            .replace('月', "-")
            .replace('日', " ");
    }
    text = text
        .replace(',', " ")
        .replace(" a las ", " ")
        .replace(" klo ", " ")
        .replace(" à ", " ");
    text = normalize_time_string(&text);

    // Handle `YYYY-MM-DDTHH:MM:SS`, `YYYY-MM-DD_HH.MM.SS`, and space-separated date/time.
    let (date_str, time_str) = split_date_time(&text)?;
    let time = parse_time_part(time_str.trim(), meridiem)?;
    let date = parse_date_part(date_str.trim(), meridiem, cjk_meridiem || cjk_date)?;
    Some(NaiveDateTime::new(date, time))
}

fn split_date_time(text: &str) -> Option<(&str, &str)> {
    // 1. ISO `T` separator (`2026-05-01T14:30:00`).
    if let Some((d, t)) = text.split_once('T').or_else(|| text.split_once('t'))
        && !d.is_empty()
        && !t.is_empty()
        && d.chars().all(|c| c.is_ascii_digit() || matches!(c, '-' | '/' | '.'))
    {
        return Some((d, t));
    }

    // 2. Crash-report filename style (`2026-05-01_14.30.00`).
    if let Some((d, t)) = text.split_once('_')
        && !d.is_empty()
        && !t.is_empty()
    {
        return Some((d, t));
    }

    // 3. Space-separated: Korean `2021. 8. 1. 3:20` has spaces inside the date,
    //    so locate the final token after the date that looks like a time (`H:M...` or `H.M...`).
    let mut last_time_start = None;
    let mut offset = 0;
    for part in text.split(' ') {
        if offset > 0 && looks_like_time(part) {
            last_time_start = Some(offset);
        }
        offset += part.len() + 1;
    }
    let idx = last_time_start?;
    let date = text[..idx].trim();
    let time = text[idx..].trim();
    if date.is_empty() || time.is_empty() {
        return None;
    }
    Some((date, time))
}

fn looks_like_time(token: &str) -> bool {
    let sep = if token.contains(':') {
        ':'
    } else if matches!(token.matches('.').count(), 1 | 2) {
        '.'
    } else {
        return false;
    };
    let mut parts = token.split(sep);
    let Some(h) = parts.next() else { return false };
    let Some(m) = parts.next() else { return false };
    !h.is_empty()
        && h.len() <= 2
        && h.chars().all(|c| c.is_ascii_digit())
        && !m.is_empty()
        && m.len() <= 2
        && m.chars().all(|c| c.is_ascii_digit())
}

fn parse_time_part(raw: &str, meridiem: Option<Meridiem>) -> Option<NaiveTime> {
    // Strip trailing timezone abbreviations if any (e.g. "16:20:00 CST").
    let core = raw.split_whitespace().next()?;
    let sep = if core.contains(':') {
        ':'
    } else if matches!(core.matches('.').count(), 1 | 2) {
        '.'
    } else {
        return None;
    };

    let parts: Vec<&str> = core.split(sep).collect();
    if !(2..=3).contains(&parts.len()) {
        return None;
    }

    let raw_hour: u32 = parts[0].parse().ok()?;
    let minute: u32 = parts[1].parse().ok()?;
    let (second, nanos) = if parts.len() == 3 {
        if let Some((sec_str, frac_str)) = parts[2].split_once('.').or_else(|| parts[2].split_once(',')) {
            let sec: u32 = sec_str.parse().ok()?;
            let nanos = parse_nanos(frac_str)?;
            (sec, nanos)
        } else {
            (parts[2].parse().ok()?, 0)
        }
    } else {
        (0, 0)
    };

    let hour = match meridiem {
        Some(Meridiem::Am) => match raw_hour {
            0 => 0,
            1..=11 => raw_hour,
            12 => 0,
            _ => return None,
        },
        Some(Meridiem::Pm) => match raw_hour {
            0 => 12,
            1..=11 => raw_hour + 12,
            12 => 12,
            _ => return None,
        },
        None => raw_hour,
    };

    NaiveTime::from_hms_nano_opt(hour, minute, second, nanos)
}

fn parse_nanos(frac: &str) -> Option<u32> {
    if frac.is_empty() || !frac.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let mut digits = frac.chars().take(9).collect::<String>();
    while digits.len() < 9 {
        digits.push('0');
    }
    digits.parse().ok()
}

fn parse_date_part(
    raw: &str,
    meridiem: Option<Meridiem>,
    prefer_ymd: bool,
) -> Option<NaiveDate> {
    // Collapse spaces around dots and strip trailing dot (Korean `2021. 8. 1.` -> `2021.8.1`).
    let compact: String = raw
        .chars()
        .filter(|&c| c != ' ')
        .collect::<String>()
        .trim_end_matches('.')
        .to_owned();

    let sep = ['-', '/', '.']
        .into_iter()
        .find(|&s| compact.matches(s).count() == 2)?;
    let parts: Vec<&str> = compact.split(sep).collect();
    if parts.len() != 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return None;
    }

    let a_len = parts[0].len();
    let c_len = parts[2].len();
    let a: i32 = parts[0].parse().ok()?;
    let b: u32 = parts[1].parse().ok()?;
    let c: i32 = parts[2].parse().ok()?;

    let ymd = || make_date(a, a_len, b, c as u32);
    let dmy = || make_date(c, c_len, b, a as u32);
    let mdy = || make_date(c, c_len, a as u32, b);

    if a_len == 4 {
        return ymd();
    }
    if c_len == 4 {
        return match (sep, meridiem.is_some()) {
            ('/', true) => mdy().or_else(dmy),
            _ => dmy().or_else(mdy),
        };
    }

    // All three components are 1–2 digits (e.g. `13-10-15`, `10/15/13`, `15.10.13`).
    if prefer_ymd {
        return ymd().or_else(dmy).or_else(mdy);
    }
    match (sep, meridiem.is_some()) {
        ('.', _) => dmy().or_else(ymd).or_else(mdy),
        ('/', true) => mdy().or_else(dmy).or_else(ymd),
        ('/', false) => dmy().or_else(mdy).or_else(ymd),
        ('-', _) => ymd().or_else(dmy).or_else(mdy),
        _ => None,
    }
}

fn make_date(raw_year: i32, year_digits: usize, month: u32, day: u32) -> Option<NaiveDate> {
    let year = match year_digits {
        4 if (1970..=9999).contains(&raw_year) => raw_year,
        1..=2 => {
            if raw_year <= 69 {
                2000 + raw_year
            } else {
                1900 + raw_year
            }
        }
        _ => return None,
    };
    NaiveDate::from_ymd_opt(year, month, day)
}

// endregion

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn analyse_str(content: &str) -> Result<CrashReportAnalysis, CrashReportError> {
        analyse_reader(Cursor::new(content.as_bytes()), Path::new("crash.txt"))
    }

    #[test]
    fn classifies_watchdog_others_unknown_and_rejects_invalid_banner() {
        let watchdog = "\u{feff}---- Minecraft Crash Report ----\r\n// Who set us up the TNT?\r\n\r\nTime: 2026-05-01 14:30:00\r\nDescription: Watching Server\r\n\r\njava.lang.Error: Watchdog\r\n";
        let analysis = analyse_str(watchdog).unwrap();
        assert_eq!(analysis.report_type, CrashReportType::Watchdog);
        assert_eq!(analysis.desc.as_deref(), Some("Watching Server"));
        assert_eq!(
            analysis.time,
            Some(
                NaiveDate::from_ymd_opt(2026, 5, 1)
                    .unwrap()
                    .and_hms_opt(14, 30, 0)
                    .unwrap()
            )
        );

        let other = "---- Minecraft Crash Report ----\nwhatever garbage line\nTime: unparseable\nDescription: Ticking entity\n";
        let analysis = analyse_str(other).unwrap();
        assert_eq!(analysis.report_type, CrashReportType::Others);
        assert_eq!(analysis.desc.as_deref(), Some("Ticking entity"));
        assert_eq!(analysis.time, None);

        let unknown = "---- Minecraft Crash Report ----\n// truncated\nTime: 2024-06-01 08:00:00\n";
        let analysis = analyse_str(unknown).unwrap();
        assert_eq!(analysis.report_type, CrashReportType::Unknown);
        assert_eq!(analysis.desc, None);
        assert!(analysis.time.is_some());

        assert!(matches!(
            analyse_str("not a crash report\nDescription: Watching Server\n"),
            Err(CrashReportError::InvalidFormat { .. })
        ));
    }

    #[test]
    fn parses_crash_times_across_java_versions_and_locales() {
        let ymd_hms = |y, m, d, h, min, s| {
            NaiveDate::from_ymd_opt(y, m, d)
                .unwrap()
                .and_hms_opt(h, min, s)
                .unwrap()
        };

        let cases = [
            ("2026-05-01 14:30:00", ymd_hms(2026, 5, 1, 14, 30, 0)),
            ("2026-05-01T14:30:00+08:00", ymd_hms(2026, 5, 1, 14, 30, 0)),
            ("2026-05-01_14.30.00", ymd_hms(2026, 5, 1, 14, 30, 0)),
            ("10/15/13 4:20 PM", ymd_hms(2013, 10, 15, 16, 20, 0)),
            ("5/14/22, 9:32\u{202f}PM", ymd_hms(2022, 5, 14, 21, 32, 0)),
            ("10/15/2013, 12:05:30 AM", ymd_hms(2013, 10, 15, 0, 5, 30)),
            ("13-10-15 下午4:20", ymd_hms(2013, 10, 15, 16, 20, 0)),
            ("2021/8/1 下午3:20", ymd_hms(2021, 8, 1, 15, 20, 0)),
            ("2021/8/1 下午 3:20", ymd_hms(2021, 8, 1, 15, 20, 0)),
            ("2021年8月1日 上午9:05:12", ymd_hms(2021, 8, 1, 9, 5, 12)),
            ("15/10/13 16:20", ymd_hms(2013, 10, 15, 16, 20, 0)),
            ("15.10.2013, 16:20:30", ymd_hms(2013, 10, 15, 16, 20, 30)),
            ("15.10.2021 klo 16.20", ymd_hms(2021, 10, 15, 16, 20, 0)),
            ("15/10/2021 à 16:20", ymd_hms(2021, 10, 15, 16, 20, 0)),
            ("2021. 8. 1. 오후 3:20", ymd_hms(2021, 8, 1, 15, 20, 0)),
            ("Oct 15, 2013 4:20:00 PM", ymd_hms(2013, 10, 15, 16, 20, 0)),
        ];

        for (raw, expected) in cases {
            assert_eq!(parse_crash_time(raw), Some(expected), "failed for {raw:?}");
        }
        assert_eq!(parse_crash_time(""), None);
        assert_eq!(parse_crash_time("not a date"), None);
    }
}
