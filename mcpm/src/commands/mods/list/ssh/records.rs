//! Converts remote JSON metadata into the shared local list presentation records.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use kako_mods_lib::local::{
    mcmod::{ModEntry, ModEntryName, ModFile, ModMeta, NestedModMeta},
    modloader::{BUILTIN_LOADERS, ModLoaderMeta},
};

use crate::commands::mod_support::{
    scan::{FailedIdentification, ScannedFile},
    selection::SelectedFile,
};

use super::super::remote::protocol::{Metadata, Record, Status};

#[derive(Debug)]
pub(super) struct Loaded {
    pub files: Vec<SelectedFile>,
    pub input_errors: usize,
    pub failed: usize,
}

impl Loaded {
    pub fn has_errors(&self) -> bool {
        self.input_errors > 0 || self.failed > 0
    }
}

#[tracing::instrument(level = "debug", skip_all, err(level = "debug"))]
pub(super) fn convert(records: Vec<Record>) -> Result<Loaded> {
    tracing::debug!(count = records.len(), "Converting remote mod records");
    let mut loaded = Loaded {
        files: Vec::new(),
        input_errors: 0,
        failed: 0,
    };
    for mut record in records {
        tracing::trace!(path = %record.path, status = ?record.status, "Converting remote mod record");
        ensure!(
            (record.status == Status::Failed) != record.errors.is_empty(),
            "remote record status and errors disagree for {:?}",
            record.path
        );
        let mut failures = Vec::new();
        for error in std::mem::take(&mut record.errors) {
            let source = error.error.into_error();
            let reason = format!("{source:#}");
            let diagnostic =
                crate::diagnostics::RemoteError::wrap(source).context(record.path.clone());
            crate::diagnostics::error(&diagnostic);
            if record.name.is_some() {
                failures.push(FailedIdentification {
                    loader: loader(
                        error
                            .loader
                            .as_deref()
                            .context("missing loader ID in remote identification failure")?,
                    )?,
                    reason,
                });
            }
        }
        if record.name.is_none() {
            ensure!(
                record.status == Status::Failed
                    && record.enabled.is_none()
                    && record.metadata.is_empty(),
                "invalid remote input failure record for {:?}",
                record.path
            );
            loaded.input_errors += 1;
            continue;
        }
        let suffix = if record
            .enabled
            .context("missing enabled state in remote mod record")?
        {
            ".jar"
        } else {
            ".jar.disabled"
        };
        let filename = format!("{}{suffix}", record.name.as_deref().unwrap());
        let entry = ModEntry::from_parts(PathBuf::from(&record.path), ModEntryName::new(filename)?);
        let metadata: Vec<_> = record
            .metadata
            .into_iter()
            .map(metadata)
            .collect::<Result<_>>()?;
        if record.status == Status::Failed {
            loaded.failed += 1;
        }
        loaded.files.push(SelectedFile {
            scanned: Arc::new(ScannedFile {
                file: ModFile { entry, metadata },
                failures,
            }),
            records: None,
        });
    }
    tracing::debug!(
        files = loaded.files.len(),
        input_errors = loaded.input_errors,
        failed = loaded.failed,
        "Remote mod records converted"
    );
    Ok(loaded)
}

fn metadata(remote: Metadata) -> Result<ModMeta> {
    let loader = loader(&remote.loader)?;
    let mut nested = Vec::new();
    for child in remote.nested {
        ensure!(
            child.meta.loader == remote.loader,
            "remote nested loader differs from its parent"
        );
        nested.push(NestedModMeta {
            path: child.path,
            meta: metadata(child.meta)?,
        });
    }
    Ok(ModMeta {
        loader,
        mods: remote.mods,
        nested,
    })
}

fn loader(id: &str) -> Result<ModLoaderMeta> {
    BUILTIN_LOADERS
        .get(id)
        .cloned()
        .with_context(|| format!("unsupported remote loader {id:?}"))
}
