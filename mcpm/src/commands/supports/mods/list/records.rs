//! Converts remote JSON metadata into the shared local list presentation records.

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::{Context, Result, ensure};
use mcpm_backend::local::{
    mcmod::{ModEntry, ModEntryName, ModFile},
    modloader::{BUILTIN_LOADERS, ModLoaderMeta},
};

use crate::commands::supports::mods::{
    scan::{FailedIdentification, ScannedFile},
    selection::SelectedFile,
};

use super::protocol::{Record, Status};

#[derive(Debug)]
pub(in crate::commands::supports::mods::list) struct Loaded {
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
pub(in crate::commands::supports::mods::list) fn convert(
    records: Vec<Record>,
    report_errors: bool,
) -> Result<Loaded> {
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
            if report_errors {
                crate::diagnostics::error(&diagnostic);
            }
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
            .iter()
            .map(|metadata| metadata.to_mod_meta(&BUILTIN_LOADERS))
            .collect::<Result<_, _>>()
            .with_context(|| format!("invalid remote mod metadata for {:?}", record.path))?;
        if let Some(keys) = &record.records {
            ensure!(!keys.is_empty(), "empty selected declaration list");
            for key in keys {
                let meta = metadata
                    .get(key.metadata)
                    .context("invalid selected metadata index")?;
                ensure!(
                    key.record
                        < crate::commands::supports::mods::mod_records::collect(meta, "").len(),
                    "invalid selected declaration index"
                );
            }
        }
        if record.status == Status::Failed {
            loaded.failed += 1;
        }
        loaded.files.push(SelectedFile {
            scanned: Arc::new(ScannedFile {
                file: ModFile { entry, metadata },
                failures,
            }),
            records: record.records,
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

fn loader(id: &str) -> Result<ModLoaderMeta> {
    BUILTIN_LOADERS
        .get(id)
        .cloned()
        .with_context(|| format!("unsupported remote loader {id:?}"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use mcpm_backend::local::mcmod::MetadataConversionError;
    use serde_json::{Value, json};

    #[test]
    fn remote_metadata_uses_shared_tree_validation_at_every_depth() {
        let leaf = json!({"loader":"fabric", "mods":[], "nested":[]});
        let child = json!({"path":"META-INF/jars/child.jar", "meta":leaf});
        let metadata = json!({"loader":"fabric", "mods":[], "nested":[child]});
        let convert_metadata = |metadata: Value| {
            let record = serde_json::from_value(json!({
                "path":"mods/example.jar", "name":"example", "enabled":true,
                "status":"unrecognized", "metadata":[metadata], "errors":[]
            }))
            .unwrap();
            convert(vec![record], false)
        };

        let loaded = convert_metadata(metadata.clone()).unwrap();
        assert_eq!(
            loaded.files[0].scanned.file.metadata[0].nested[0].path,
            "META-INF/jars/child.jar"
        );

        for path in ["", "/absolute.jar", "../escape.jar", "jars/../escape.jar"] {
            for nested in [false, true] {
                let mut invalid = metadata.clone();
                let invalid_child = json!({"path":path, "meta":leaf});
                if nested {
                    invalid["nested"][0]["meta"]["nested"] = json!([invalid_child]);
                } else {
                    invalid["nested"] = json!([invalid_child]);
                }
                let error = convert_metadata(invalid).unwrap_err();
                assert!(matches!(
                    error.downcast_ref::<MetadataConversionError>(),
                    Some(MetadataConversionError::InvalidNestedPath(rejected)) if rejected == path
                ));
            }
        }

        let mut duplicate = metadata.clone();
        duplicate["nested"] = json!([child, child]);
        assert!(matches!(
            convert_metadata(duplicate)
                .unwrap_err()
                .downcast_ref::<MetadataConversionError>(),
            Some(MetadataConversionError::InvalidNestedPath(_))
        ));

        let mut mixed = metadata;
        mixed["nested"][0]["meta"]["loader"] = json!("forge");
        assert!(matches!(
            convert_metadata(mixed)
                .unwrap_err()
                .downcast_ref::<MetadataConversionError>(),
            Some(MetadataConversionError::MixedLoaders)
        ));
    }
}
