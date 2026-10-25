//! Backup result summaries and file details, including content provenance and manifest-only sizes.

use std::io::Write;

use anyhow::Result;
use mcpm_backend::local::{
    cache::TransferMethod,
    mcmod::backup::{FileDisposition, FileRecord, FileSource, Operation, Outcome},
};

use super::super::{
    console::ConsoleWriter,
    size::{binary, size},
    style::{OutputStyle, text, tsv},
};
use crate::terminal;

pub(crate) fn summary(out: &mut impl Write, outcome: &Outcome, headers: bool) -> Result<()> {
    let manifest = outcome.manifest();
    let count = manifest.files().len();
    let mut total = u128::from(outcome.manifest_size());
    let (mut shared, mut linked, mut copied, mut from_backup, mut from_cache) =
        (0_u128, 0, 0, 0, 0);
    for (file, disposition) in outcome.files() {
        let bytes = u128::from(file.size());
        total += bytes;
        if let FileDisposition::Transferred { source, method } = disposition {
            match method {
                TransferMethod::Copy => copied += 1,
                TransferMethod::HardLink => {
                    shared += bytes;
                    linked += 1;
                }
            }
            match source {
                FileSource::Backup => from_backup += 1,
                FileSource::Cache => from_cache += 1,
            }
        }
    }
    let actual = if outcome.operation() == Operation::CreateCacheOnly {
        u128::from(outcome.manifest_size())
    } else {
        total - shared
    };
    let operation = match outcome.operation() {
        Operation::Create | Operation::CreateCacheOnly => "create",
        Operation::Apply => "apply",
    };
    if terminal::stdout_is_tty() {
        let style = OutputStyle::new();
        let mut console = ConsoleWriter::new(out);
        let message = match outcome.operation() {
            Operation::Create => "The backup was created successfully.",
            Operation::CreateCacheOnly => "The manifest-only backup was created successfully.",
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
            sizes(
                &mut fields,
                total,
                actual,
                shared,
                outcome.operation(),
                &style,
            )?;
            if outcome.operation() == Operation::Apply {
                writeln!(fields, "From backup: {from_backup}")?;
                writeln!(fields, "From cache: {from_cache}")?;
            }
            writeln!(fields, "Copied: {copied}")?;
            writeln!(fields, "Hard linked: {linked}")?;
            writeln!(fields, "Checksum: {}", style.muted(manifest.checksum()))?;
        }
        if linked > 0 {
            hard_link_tip(&mut console, shared, outcome.operation(), &style)?;
        }
        if matches!(
            outcome.operation(),
            Operation::Create | Operation::CreateCacheOnly
        ) {
            let protection = if outcome.operation() == Operation::CreateCacheOnly {
                ""
            } else if cfg!(unix) {
                " Its default permissions prevent ordinary recursive deletion."
            } else {
                " This command handles the backup's directory protection."
            };
            tip(
                &mut console,
                &format!(
                    "Tip: Use {} mod backup delete <BACKUP> to delete this backup.{protection}",
                    crate::program::roots().tools
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
        sizes(
            &mut fields,
            total,
            total - shared,
            shared,
            operation,
            &style,
        )?;
        writeln!(
            fields,
            "Method: {}",
            super::super::cache::operations::method(method)
        )?;
    } else {
        writeln!(fields, "Size: {}", size(total, &style))?;
    }
    Ok(())
}

fn sizes(
    out: &mut ConsoleWriter<'_>,
    total: u128,
    actual: u128,
    shared: u128,
    operation: Operation,
    style: &OutputStyle,
) -> Result<()> {
    writeln!(out, "Size: {}", size(total, style))?;
    write!(out, "Actual Size: {}", size(actual, style))?;
    if shared > 0 {
        let label = match operation {
            Operation::Create | Operation::CreateCacheOnly => "cached",
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
        Operation::Create | Operation::CreateCacheOnly => {
            ("the cache", "this backup", "when creating the backup")
        }
        Operation::Apply => (
            "their backup or cache sources",
            "these files",
            "when applying the backup",
        ),
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
