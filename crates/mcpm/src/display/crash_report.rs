//! Console and Raw presentation for crash report analysis results.

use std::{
    io::{self, Write},
    path::Path,
};

use chrono::{NaiveDateTime, Timelike};
use mcpm_backend::operations::crashreport::{CrashReportAnalysis, CrashReportType};

use super::{
    console::ConsoleWriter,
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

/// Writes a crash report analysis in Console (TTY) or Raw TSV (non-TTY / `--raw`) format.
pub(crate) fn write_analysis(
    output: &mut impl Write,
    path: &Path,
    analysis: &CrashReportAnalysis,
) -> io::Result<()> {
    let type_raw = match analysis.report_type {
        CrashReportType::Watchdog => "watchdog",
        CrashReportType::Others => "others",
        CrashReportType::Unknown => "unknown",
    };
    let formatted_time = analysis.time.map(format_time);

    if !terminal::stdout_is_tty() {
        return writeln!(
            output,
            "{type_raw}\t{}\t{}",
            formatted_time.as_deref().unwrap_or(""),
            analysis.desc.as_deref().map(tsv).unwrap_or_default(),
        );
    }

    let style = OutputStyle::new();
    let mut console = ConsoleWriter::new(output);
    writeln!(
        console,
        "{} {} {}",
        style.primary("Crash Report"),
        style.normal("at"),
        style.muted(text(&path.to_string_lossy()))
    )?;

    let mut fields = console.subwriter();
    let type_display = match analysis.report_type {
        CrashReportType::Watchdog => style.danger("Watchdog"),
        CrashReportType::Others => style.important("Others"),
        CrashReportType::Unknown => style.muted("Unknown"),
    };
    writeln!(fields, "{}{}", style.muted("Type: "), type_display)?;

    let time_display = match formatted_time {
        Some(value) => style.secondary(value),
        None => style.muted("unknown"),
    };
    writeln!(fields, "{}{}", style.muted("Time: "), time_display)?;

    let desc_display = match analysis.desc.as_deref() {
        Some(value) => style.normal(text(value)),
        None => style.muted("unknown"),
    };
    writeln!(fields, "{}{}", style.muted("Description: "), desc_display)?;

    Ok(())
}

fn format_time(time: NaiveDateTime) -> String {
    if time.nanosecond() == 0 {
        time.format("%Y-%m-%d %H:%M:%S").to_string()
    } else {
        let frac = format!("{:09}", time.nanosecond());
        let frac = frac.trim_end_matches('0');
        format!("{}.{frac}", time.format("%Y-%m-%d %H:%M:%S"))
    }
}
