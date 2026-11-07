//! Serialize selected Mod trees only across transport; local execution retains the original objects.

use super::{
    super::{
        mod_records,
        scan::{FailedIdentification, ScannedFile},
    },
    SelectedFile,
};
use anyhow::{Context, Result, ensure};
use mcpm_exec::error::ErrorReport;
use mcpm_lib::local::{
    mcmod::{CachedModMeta, ModEntry, ModEntryName, ModFile},
    modloader::BUILTIN_LOADERS,
};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::sync::Arc;

#[derive(Serialize, Deserialize)]
struct Snapshot {
    path: std::path::PathBuf,
    filename: String,
    metadata: Vec<CachedModMeta>,
    failures: Vec<Failure>,
    records: Option<Vec<mod_records::RecordKey>>,
}
#[derive(Serialize, Deserialize)]
struct Failure {
    loader: String,
    error: ErrorReport,
}

impl Serialize for SelectedFile {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let file = &self.scanned.file;
        let metadata = file
            .metadata
            .iter()
            .map(CachedModMeta::capture)
            .collect::<Result<Vec<_>, _>>()
            .map_err(serde::ser::Error::custom)?;
        Snapshot {
            path: file.entry.path().to_owned(),
            filename: file.entry.filename().to_owned(),
            metadata,
            failures: self
                .scanned
                .failures
                .iter()
                .map(|failure| Failure {
                    loader: failure.loader.id().as_str().to_owned(),
                    error: failure.error.clone(),
                })
                .collect(),
            records: self.records.clone(),
        }
        .serialize(serializer)
    }
}
impl<'de> Deserialize<'de> for SelectedFile {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Snapshot::deserialize(deserializer)?
            .restore()
            .map_err(serde::de::Error::custom)
    }
}
impl Snapshot {
    fn restore(self) -> Result<SelectedFile> {
        let entry = ModEntry::from_parts(self.path, ModEntryName::new(self.filename)?);
        let metadata = self
            .metadata
            .iter()
            .map(|metadata| metadata.to_mod_meta(&BUILTIN_LOADERS))
            .collect::<Result<Vec<_>, _>>()?;
        if let Some(keys) = &self.records {
            ensure!(!keys.is_empty(), "empty selected declaration list");
            for key in keys {
                let meta = metadata
                    .get(key.metadata)
                    .context("invalid selected metadata index")?;
                ensure!(
                    key.record < mod_records::collect(meta, "").len(),
                    "invalid selected declaration index"
                );
            }
        }
        let failures = self
            .failures
            .into_iter()
            .map(|failure| {
                Ok(FailedIdentification {
                    loader: BUILTIN_LOADERS
                        .get(failure.loader.as_str())
                        .cloned()
                        .with_context(|| format!("unsupported loader {:?}", failure.loader))?,
                    error: failure.error,
                })
            })
            .collect::<Result<_>>()?;
        Ok(SelectedFile {
            scanned: Arc::new(ScannedFile {
                file: ModFile { entry, metadata },
                failures,
            }),
            records: self.records,
        })
    }
}
