//! Console and newline-delimited JSON rendering for explicit cache management.

use std::{io::Write, path::Path};

use anyhow::Result;
use kako_mods_lib::local::mcmod::cache::{
    CacheEntry, CacheId, CacheStatus, CacheUnavailableReason, CachedIdentification, ExportMethod,
};
use kako_mods_lib::local::modloader::BUILTIN_LOADERS;
use serde_json::{Value, json};

use crate::{
    commands::{
        mod_support::inspect,
        output_style::{OutputStyle, text},
    },
    terminal,
    theme::{self, Role},
};

pub(super) fn info(out: &mut impl Write, entry: &CacheEntry) -> Result<()> {
    if !terminal::stdout_is_tty() {
        return self::entry(out, entry);
    }
    let style = OutputStyle::new();
    writeln!(
        out,
        "{}",
        style.muted(format!(
            "File: {}",
            text(&entry.data_path().to_string_lossy())
        ))
    )?;
    if let Some(digest) = entry.digest() {
        writeln!(
            out,
            "{}",
            style.muted(format!("Size: {} bytes", digest.size()))
        )?;
        writeln!(
            out,
            "{}",
            style.muted(format!("SHA-256: {}", digest.sha256()))
        )?;
        if entry.status() == &CacheStatus::Available {
            for (_, result) in digest.identifications() {
                let CachedIdentification::Recognized(snapshot) = result else {
                    continue;
                };
                let metadata = snapshot.to_mod_meta(&BUILTIN_LOADERS)?;
                inspect::write_metadata(
                    out,
                    entry.data_path(),
                    &inspect::Metadata::from_mod_meta(&metadata),
                )?;
            }
        }
    } else {
        writeln!(out, "{}", style.muted("Size: unavailable"))?;
        writeln!(out, "{}", style.muted("SHA-256: unavailable"))?;
    }
    match entry.status() {
        CacheStatus::Available => {}
        CacheStatus::NotFound => {
            writeln!(out, "Status: {}", style.normal("not found"))?;
            writeln!(
                out,
                "{}",
                style.muted("Neither the data file nor its digest exists.")
            )?;
        }
        CacheStatus::Unavailable(reason) => {
            writeln!(out, "Status: {}", style.warning("unavailable"))?;
            match reason {
                CacheUnavailableReason::MissingDigest => {
                    writeln!(
                        out,
                        "{}",
                        style.muted("Reason: Identification digest is missing.")
                    )?;
                    writeln!(
                        out,
                        "{}",
                        style.muted("Hint: Refresh metadata to create an identification digest.")
                    )?;
                }
                CacheUnavailableReason::InvalidData => {
                    writeln!(
                        out,
                        "{}",
                        style.muted("Reason: Cached data or digest is invalid or incompatible.")
                    )?;
                }
                CacheUnavailableReason::UnexpectedError(error) => {
                    writeln!(
                        out,
                        "{}",
                        style.muted(format!(
                            "Reason: Could not read cache object: {}",
                            text(error)
                        ))
                    )?;
                }
            }
        }
    }
    Ok(())
}

pub(super) fn entry(out: &mut impl Write, entry: &CacheEntry) -> Result<()> {
    let (status, reason) = state(entry.status());
    if !terminal::stdout_is_tty() {
        writeln!(out, "{}", entry_record(entry))?;
    } else {
        let id = entry.id().to_string();
        if terminal::stdout_color_enabled() {
            writeln!(
                out,
                "{}  {status}",
                theme::console(Role::Primary).bold().style(&id)
            )?;
        } else {
            writeln!(out, "{id}  {status}")?;
        }
        if let Some(reason) = reason {
            writeln!(out, "  Reason: {reason}")?;
        }
        writeln!(out, "  File: {}", entry.data_path().display())?;
        if let Some(digest) = entry.digest() {
            writeln!(
                out,
                "  Size: {} bytes\n  SHA-256: {}",
                digest.size(),
                digest.sha256()
            )?;
            for (loader, metadata) in digest.identifications() {
                match metadata {
                    CachedIdentification::Recognized(meta) => writeln!(
                        out,
                        "  {loader}: {} direct mods, {} nested archives",
                        meta.mods.len(),
                        meta.nested.len()
                    )?,
                    CachedIdentification::Unrecognized => {
                        writeln!(out, "  {loader}: not recognized")?
                    }
                    CachedIdentification::Missing => {}
                }
            }
        }
    }
    Ok(())
}

fn entry_record(entry: &CacheEntry) -> Value {
    let (status, reason) = state(entry.status());
    json!({"id":entry.id().to_string(), "status":status, "reason":reason,
        "data_path":entry.data_path(), "digest_path":entry.digest_path(), "digest":entry.digest()})
}

pub(super) fn imported(
    out: &mut impl Write,
    entry: &CacheEntry,
    outcome: kako_mods_lib::local::mcmod::cache::ImportOutcome,
) -> Result<()> {
    use kako_mods_lib::local::mcmod::cache::ImportMethod;
    let method = outcome.method.map(|method| match method {
        ImportMethod::Copy => "copy",
        ImportMethod::HardLink => "hard_link",
    });
    if terminal::stdout_is_tty() {
        self::entry(out, entry)?;
        writeln!(
            out,
            "  Import: {}",
            if outcome.published { "added" } else { "reused" }
        )?;
        if let Some(method) = method {
            writeln!(out, "  Method: {method}")?;
        }
    } else {
        let mut record = entry_record(entry);
        record["method"] = method.into();
        record["published"] = outcome.published.into();
        writeln!(out, "{record}")?;
    }
    Ok(())
}

pub(super) fn verification(out: &mut impl Write, id: CacheId, status: &CacheStatus) -> Result<()> {
    let (status, reason) = state(status);
    record(
        out,
        json!({"id":id.to_string(),"status":status,"reason":reason}),
        &format!(
            "{id}  {status}{}",
            reason
                .map(|reason| format!(" ({reason})"))
                .unwrap_or_default()
        ),
    )
}

pub(super) fn exported(
    out: &mut impl Write,
    id: CacheId,
    destination: &Path,
    method: ExportMethod,
) -> Result<()> {
    let method = match method {
        ExportMethod::Copy => "copy",
        ExportMethod::HardLink => "hard_link",
    };
    record(
        out,
        json!({"id":id.to_string(),"destination":destination,"method":method}),
        &format!("{id}  {method} -> {}", destination.display()),
    )
}

pub(super) fn removed(out: &mut impl Write, id: CacheId, removed: bool) -> Result<()> {
    record(
        out,
        json!({"id":id.to_string(),"removed":removed}),
        &format!("{id}  {}", if removed { "removed" } else { "not found" }),
    )
}

fn record(out: &mut impl Write, value: Value, console: &str) -> Result<()> {
    if terminal::stdout_is_tty() {
        writeln!(out, "{console}")?;
    } else {
        writeln!(out, "{value}")?;
    }
    Ok(())
}

fn state(status: &CacheStatus) -> (&'static str, Option<&str>) {
    match status {
        CacheStatus::Available => ("available", None),
        CacheStatus::NotFound => ("not_found", None),
        CacheStatus::Unavailable(reason) => (
            "unavailable",
            Some(match reason {
                CacheUnavailableReason::MissingDigest => "missing_digest",
                CacheUnavailableReason::InvalidData => "invalid_data",
                CacheUnavailableReason::UnexpectedError(error) => error.as_str(),
            }),
        ),
    }
}

/// Reports the result of explicit cache initialization.
pub(super) fn initialized(
    out: &mut impl std::io::Write,
    path: &std::path::Path,
) -> anyhow::Result<()> {
    if crate::terminal::stdout_is_tty() {
        writeln!(out, "Initialized: {}", path.display())?;
    } else {
        serde_json::to_writer(
            &mut *out,
            &serde_json::json!({"operation":"init", "directory":path, "version":kako_mods_lib::local::mcmod::cache::CACHE_DIRECTORY_VERSION}),
        )?;
        writeln!(out)?;
    }
    Ok(())
}

/// Reports how many objects were registered by an explicit index rebuild.
pub(super) fn rebuilt_index(out: &mut impl Write, indexed: usize) -> Result<()> {
    record(
        out,
        json!({"operation":"rebuild_index", "indexed":indexed}),
        &format!("Index rebuilt: {indexed} objects"),
    )
}
