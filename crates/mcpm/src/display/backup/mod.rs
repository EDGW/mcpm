//! Reusable backup progress, Console field groups and Raw TSV summaries.

use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Result;
use mcpm_backend::local::mcmod::backup::{
    ChecksumMismatch, Comparison, DirectoryChecksum, Event, FileDisposition, FileSource, Manifest,
    Operation, Outcome, Stage, VerificationError, VerifiedBackup,
};

use super::{
    console::ConsoleWriter,
    size::size,
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

mod notice;
pub(crate) use notice::{cache_only_created, cache_only_warning};
mod summary;
pub(crate) use summary::{file, summary};

mod checksum;
pub(crate) use checksum::file_difference;
mod progress;
pub(crate) use progress::FileProgress;

pub(crate) struct Presentation<W: Write> {
    output: W,
    details: bool,
    headers: bool,
    progress: FileProgress,
    printed_result: bool,
}

impl<W: Write> Presentation<W> {
    pub(crate) fn new(output: W, details: bool, headers: bool) -> Self {
        Self {
            output,
            details,
            headers,
            progress: FileProgress::default(),
            printed_result: false,
        }
    }

    pub(crate) fn event(&mut self, event: Event) -> Result<()> {
        self.progress.event(event);
        match event {
            Event::Progress { .. } => {}
            Event::StageDone(stage) => {
                if terminal::stdout_is_tty() {
                    let message = match stage {
                        Stage::Checksum => "All file checksums were computed successfully.",
                        Stage::Cache => "All source files were cached successfully.",
                        Stage::Verify => "All files passed content verification.",
                        Stage::Transfer => "All files were transferred and verified successfully.",
                    };
                    writeln!(
                        ConsoleWriter::new(&mut self.output),
                        "{}",
                        OutputStyle::new().success(message)
                    )?;
                }
            }
        }
        Ok(())
    }

    pub(crate) fn finish(mut self, outcome: &Outcome) -> Result<()> {
        self.progress.clear();
        summary(&mut self.output, outcome, self.headers)?;
        if terminal::stdout_is_tty() && self.details {
            let mut console = ConsoleWriter::new(&mut self.output);
            for (record, disposition) in outcome.files() {
                writeln!(console)?;
                match disposition {
                    FileDisposition::CacheOnly => {
                        file(&mut console, record, None)?;
                        writeln!(console.subwriter(), "Method: cache-only")?;
                    }
                    FileDisposition::Transferred { source, method } => {
                        file(&mut console, record, Some((method, outcome.operation())))?;
                        if outcome.operation() == Operation::Apply {
                            let source = match source {
                                FileSource::Backup => "backup",
                                FileSource::Cache => "cache",
                            };
                            writeln!(console.subwriter(), "Source: {source}")?;
                        }
                    }
                }
            }
        }
        self.output.flush()?;
        Ok(())
    }

    pub(crate) fn verified(
        &mut self,
        path: &Path,
        verified: Option<&VerifiedBackup>,
    ) -> Result<()> {
        self.progress.clear();
        verification_result(
            &mut self.output,
            path,
            verified,
            self.headers && !self.printed_result,
        )?;
        self.printed_result = true;
        self.file_details(verified.map(VerifiedBackup::manifest))?;
        self.output.flush()?;
        Ok(())
    }

    pub(crate) fn checksummed(
        &mut self,
        path: &Path,
        computed: Option<&DirectoryChecksum>,
    ) -> Result<()> {
        self.progress.clear();
        checksum::summary(
            &mut self.output,
            path,
            computed,
            self.headers && !self.printed_result,
        )?;
        self.printed_result = true;
        self.file_details(computed.map(DirectoryChecksum::manifest))?;
        self.output.flush()?;
        Ok(())
    }

    pub(crate) fn compared(
        &mut self,
        target: (&str, &str),
        path: &Path,
        computed: Option<&DirectoryChecksum>,
        comparison: Option<&Comparison>,
    ) -> Result<()> {
        self.progress.clear();
        checksum::comparison(
            &mut self.output,
            target,
            path,
            computed,
            comparison,
            self.headers && !self.printed_result,
        )?;
        self.printed_result = true;
        self.file_details(computed.map(DirectoryChecksum::manifest))?;
        self.output.flush()?;
        Ok(())
    }

    fn file_details(&mut self, manifest: Option<&Manifest>) -> Result<()> {
        if terminal::stdout_is_tty()
            && self.details
            && let Some(manifest) = manifest
        {
            let mut console = ConsoleWriter::new(&mut self.output);
            for record in manifest.files() {
                writeln!(console)?;
                file(&mut console, record, None)?;
            }
        }
        Ok(())
    }
}

pub(crate) fn deletion_result(
    out: &mut impl Write,
    path: &Path,
    deleted: bool,
    headers: bool,
) -> Result<()> {
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        writeln!(
            console,
            "\nBackup: {}",
            style.primary(text(&path.to_string_lossy()))
        )?;
        let mut fields = console.subwriter();
        let message = if deleted {
            style.success("The backup was deleted successfully.")
        } else {
            style.danger("The backup could not be deleted.")
        };
        writeln!(fields, "{message}")?;
    } else {
        if headers {
            writeln!(out, "backup\tstatus")?;
        }
        let status = if deleted { "deleted" } else { "failed" };
        writeln!(out, "{}\t{status}", tsv(&path.to_string_lossy()))?;
    }
    Ok(())
}

pub(crate) fn verification_result(
    out: &mut impl Write,
    path: &Path,
    verified: Option<&VerifiedBackup>,
    headers: bool,
) -> Result<()> {
    let status = if verified.is_some() {
        "valid"
    } else {
        "invalid"
    };
    let total = verified.map(|verified| {
        u128::from(verified.manifest_size())
            + verified
                .manifest()
                .files()
                .iter()
                .map(|file| u128::from(file.size()))
                .sum::<u128>()
    });
    let directory = verified.map(VerifiedBackup::directory).unwrap_or(path);
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        writeln!(
            console,
            "\nBackup: {}",
            style.primary(text(&directory.to_string_lossy()))
        )?;
        let mut fields = console.subwriter();
        let state = if verified.is_some() {
            style.success(status)
        } else {
            style.danger(status)
        };
        writeln!(fields, "Status: {state}")?;
        if let Some(verified) = verified {
            writeln!(fields, "Files: {}", verified.manifest().files().len())?;
            writeln!(fields, "Size: {}", size(total.unwrap(), &style))?;
            writeln!(
                fields,
                "Checksum: {}",
                style.muted(verified.manifest().checksum())
            )?;
        }
    } else {
        if headers {
            writeln!(out, "backup\tstatus\tfiles\tsize\tchecksum")?;
        }
        writeln!(
            out,
            "{}\t{status}\t{}\t{}\t{}",
            tsv(&directory.to_string_lossy()),
            verified
                .map(|verified| verified.manifest().files().len().to_string())
                .unwrap_or_default(),
            total.map(|size| size.to_string()).unwrap_or_default(),
            verified
                .map(|verified| verified.manifest().checksum().to_string())
                .unwrap_or_default()
        )?;
    }
    Ok(())
}

/// Summaries are short in terminal diagnostics; library Display retains full log evidence.
pub(crate) fn verification_message(error: &(dyn std::error::Error + 'static)) -> Option<String> {
    if let Some(failure) = error.downcast_ref::<VerificationError>() {
        Some(match failure {
            VerificationError::FileSet {
                missing,
                unexpected,
                ..
            } => format!(
                "backup file verification failed: {} missing, {} unexpected",
                missing.len(),
                unexpected.len()
            ),
            VerificationError::MissingFile { .. } => "a required backup file is missing".into(),
            VerificationError::FileSize { .. } => "backup file size verification failed".into(),
            VerificationError::FileHash { .. } => "backup file BLAKE3 verification failed".into(),
        })
    } else if error.is::<ChecksumMismatch>() {
        Some("backup manifest checksum verification failed".into())
    } else {
        None
    }
}

/// Extra verification fields use the same diagnostic stream and indentation as causes.
pub(crate) fn verification(out: &mut ConsoleWriter<'_>, error: &anyhow::Error) -> io::Result<()> {
    let style = OutputStyle::stderr();
    if let Some(failure) = error.downcast_ref::<VerificationError>() {
        let mut fields = out.subwriter();
        match failure {
            VerificationError::FileSet {
                directory,
                missing,
                unexpected,
            } => {
                writeln!(
                    fields,
                    "Directory: {}",
                    style.muted(text(&directory.to_string_lossy()))
                )?;
                for (label, files) in [("Missing files", missing), ("Unexpected files", unexpected)]
                {
                    if !files.is_empty() {
                        writeln!(
                            fields,
                            "{}",
                            style.important(format!("{label} ({}):", files.len()))
                        )?;
                        let mut paths = fields.subwriter();
                        for path in files {
                            writeln!(paths, "{}", style.normal(text(path)))?;
                        }
                    }
                }
            }
            VerificationError::MissingFile { path } => {
                writeln!(
                    fields,
                    "File: {}",
                    style.normal(text(&path.to_string_lossy()))
                )?;
            }
            VerificationError::FileSize {
                path,
                expected,
                actual,
            } => {
                writeln!(
                    fields,
                    "File: {}",
                    style.normal(text(&path.to_string_lossy()))
                )?;
                writeln!(
                    fields,
                    "Expected size: {}",
                    size(u128::from(*expected), &style)
                )?;
                writeln!(fields, "Actual size: {}", size(u128::from(*actual), &style))?;
            }
            VerificationError::FileHash {
                path,
                expected,
                actual,
            } => {
                writeln!(
                    fields,
                    "File: {}",
                    style.normal(text(&path.to_string_lossy()))
                )?;
                writeln!(fields, "Expected BLAKE3: {}", style.muted(expected))?;
                writeln!(fields, "Actual BLAKE3: {}", style.muted(actual))?;
            }
        }
    } else if let Some(failure) = error.downcast_ref::<ChecksumMismatch>() {
        let mut fields = out.subwriter();
        writeln!(
            fields,
            "Expected checksum: {}",
            style.muted(failure.expected)
        )?;
        writeln!(fields, "Actual checksum: {}", style.muted(failure.actual))?;
    }
    Ok(())
}
