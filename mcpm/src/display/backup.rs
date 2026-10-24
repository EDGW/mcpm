//! Reusable backup progress, Console field groups and Raw TSV summaries.

use std::{
    io::{self, Write},
    path::Path,
};

use anyhow::Result;
use kako_mods_lib::local::{
    cache::TransferMethod,
    mcmod::backup::{
        ChecksumMismatch, Event, FileRecord, Operation, Outcome, Stage, VerificationError,
        VerifiedBackup,
    },
};

use super::{
    console::ConsoleWriter,
    size::{binary, size},
    style::{OutputStyle, text, tsv},
};
use crate::{progress::Progress, terminal};

pub(crate) struct Presentation<W: Write> {
    output: W,
    details: bool,
    headers: bool,
    progress: Option<(Stage, Progress)>,
    printed_result: bool,
}

impl<W: Write> Presentation<W> {
    pub(crate) fn new(output: W, details: bool, headers: bool) -> Self {
        Self {
            output,
            details,
            headers,
            progress: None,
            printed_result: false,
        }
    }

    pub(crate) fn event(&mut self, event: Event) -> Result<()> {
        match event {
            Event::Progress {
                stage,
                completed,
                total,
            } => {
                if self.progress.as_ref().map(|(current, _)| *current) != Some(stage) {
                    self.progress = None;
                    if terminal::stderr_is_tty() {
                        let label = match stage {
                            Stage::Cache => "Caching source files",
                            Stage::Verify => "Verifying file contents",
                            Stage::Transfer => "Transferring files",
                        };
                        let progress = Progress::new(label, label);
                        progress.set_total(total);
                        self.progress = Some((stage, progress));
                    }
                }
                if let Some((_, progress)) = &self.progress {
                    progress.set_position(completed);
                }
            }
            Event::StageDone(stage) => {
                self.progress = None;
                if terminal::stdout_is_tty() {
                    let message = match stage {
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
        self.progress = None;
        summary(&mut self.output, outcome, self.headers)?;
        if terminal::stdout_is_tty() && self.details {
            let mut console = ConsoleWriter::new(&mut self.output);
            for (record, method) in outcome.files() {
                writeln!(console)?;
                file(&mut console, record, Some((method, outcome.operation())))?;
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
        self.progress = None;
        verification_result(
            &mut self.output,
            path,
            verified,
            self.headers && !self.printed_result,
        )?;
        self.printed_result = true;
        if terminal::stdout_is_tty() && self.details {
            if let Some(verified) = verified {
                let mut console = ConsoleWriter::new(&mut self.output);
                for record in verified.manifest().files() {
                    writeln!(console)?;
                    file(&mut console, record, None)?;
                }
            }
        }
        self.output.flush()?;
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

pub(crate) fn summary(out: &mut impl Write, outcome: &Outcome, headers: bool) -> Result<()> {
    let manifest = outcome.manifest();
    let count = manifest.files().len();
    let (total, shared, linked) = outcome.files().fold(
        (u128::from(outcome.manifest_size()), 0_u128, 0_usize),
        |(total, shared, linked), (file, method)| {
            let bytes = u128::from(file.size());
            match method {
                TransferMethod::Copy => (total + bytes, shared, linked),
                TransferMethod::HardLink => (total + bytes, shared + bytes, linked + 1),
            }
        },
    );
    let copied = count - linked;
    let actual = total - shared;
    let operation = match outcome.operation() {
        Operation::Create => "create",
        Operation::Apply => "apply",
    };
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        let message = match outcome.operation() {
            Operation::Create => "The backup was created successfully.",
            Operation::Apply => "The backup was applied successfully.",
        };
        writeln!(console, "\n{}", style.success(message))?;
        {
            let mut fields = console.subwriter();
            writeln!(
                fields,
                "Destination: {}",
                style.muted(text(&outcome.destination().to_string_lossy()))
            )?;
            writeln!(fields, "Files: {}", style.important(count))?;
            sizes(&mut fields, total, shared, outcome.operation(), &style)?;
            writeln!(fields, "Copied: {copied}")?;
            writeln!(fields, "Hard linked: {linked}")?;
            writeln!(fields, "Checksum: {}", style.muted(manifest.checksum()))?;
        }
        if linked > 0 {
            hard_link_tip(&mut console, shared, outcome.operation(), &style)?;
        }
        if outcome.operation() == Operation::Create {
            let protection = if cfg!(unix) {
                " Its default permissions prevent ordinary recursive deletion."
            } else {
                " This command handles the backup's directory protection."
            };
            tip(
                &mut console,
                &format!(
                    "Tip: Use mcpm mod backup delete <BACKUP> to delete this backup.{protection}"
                ),
                &style,
            )?;
        }
    } else {
        if headers {
            writeln!(
                out,
                "operation\tdestination\tfiles\tsize\tactual_size\tshared_size\tchecksum\tcopied\thard_linked"
            )?;
        }
        writeln!(
            out,
            "{operation}\t{}\t{count}\t{total}\t{actual}\t{shared}\t{}\t{copied}\t{linked}",
            tsv(&outcome.destination().to_string_lossy()),
            manifest.checksum()
        )?;
    }
    Ok(())
}

pub(crate) fn file(
    out: &mut ConsoleWriter<'_>,
    record: &FileRecord,
    transfer: Option<(TransferMethod, Operation)>,
) -> Result<()> {
    let style = OutputStyle::new();
    writeln!(out, "File: {}", style.primary(text(record.path())))?;
    let mut fields = out.subwriter();
    writeln!(fields, "BLAKE3: {}", style.muted(record.blake3()))?;
    let total = u128::from(record.size());
    if let Some((method, operation)) = transfer {
        let shared = if method == TransferMethod::HardLink {
            total
        } else {
            0
        };
        sizes(&mut fields, total, shared, operation, &style)?;
        writeln!(
            fields,
            "Method: {}",
            super::cache::operations::method(method)
        )?;
    } else {
        writeln!(fields, "Size: {}", size(total, &style))?;
    }
    Ok(())
}

fn sizes(
    out: &mut ConsoleWriter<'_>,
    total: u128,
    shared: u128,
    operation: Operation,
    style: &OutputStyle,
) -> Result<()> {
    writeln!(out, "Size: {}", size(total, style))?;
    write!(out, "Actual Size: {}", size(total - shared, style))?;
    if shared > 0 {
        let label = match operation {
            Operation::Create => "cached",
            Operation::Apply => "shared",
        };
        write!(
            out,
            " {}",
            style.muted(format!("({} {label})", binary(shared)))
        )?;
    }
    writeln!(out)?;
    Ok(())
}

fn hard_link_tip(
    out: &mut ConsoleWriter<'_>,
    shared: u128,
    operation: Operation,
    style: &OutputStyle,
) -> Result<()> {
    let (source, object, option) = match operation {
        Operation::Create => ("the cache", "this backup", "when creating the backup"),
        Operation::Apply => ("the backup", "these files", "when applying the backup"),
    };
    let message = format!(
        "Tip: {} of files are hard-linked to {source} and use no additional disk space; \
         copying or transferring {object}, or moving {object} to a location that cannot preserve \
         those hard links, will require that space again; use --mode copy {option} to store independent copies.",
        binary(shared)
    );
    tip(out, &message, style)
}

fn tip(out: &mut ConsoleWriter<'_>, message: &str, style: &OutputStyle) -> Result<()> {
    writeln!(out)?;
    let options = textwrap::Options::new(textwrap::termwidth().max(1))
        .word_splitter(textwrap::WordSplitter::NoHyphenation);
    for line in textwrap::wrap(message, options) {
        writeln!(out, "{}", style.muted(line))?;
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
