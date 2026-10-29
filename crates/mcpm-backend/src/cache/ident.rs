//! `cache.mods.ident.write`: publishes source matches for actual cache objects.
//!
//! Each entry is written through the cache's only snapshot codec, which verifies the
//! stored content before publishing. Per-entry failures are events; only an unusable
//! cache or a protected stash stops the whole operation.

use std::{future::Future, path::PathBuf};

use anyhow::Context as _;
use kako_mc_lib::error::ErrorReport;
use kako_mc_lib::source::FileMatch;
use mcpm_exec::{Context, Operation, Placement, Wire};
use serde::{Deserialize, Serialize};

use crate::data::cache::hash::Blake3Hash;
use crate::data::cache::mods::ModsCacheSelection;
use crate::data::runtime::RuntimeEnvironment;
use crate::data::server::stash;
use crate::error::OperationError;

/// Writes verified identification results into one Mod cache.
#[derive(Debug)]
pub struct IdentWrite;

crate::host_operation!(IdentWrite, "cache.mods.ident.write");

impl Operation for IdentWrite {
    const ID: &'static str = <Self as crate::register::HostOperation>::ID;
    const PLACEMENT: Placement = Placement::Host;
    const WIRE: Option<Wire<Self>> = Some(<Self as crate::register::HostOperation>::WIRE);
    type Env = crate::HostEnv;
    type Input = IdentWriteInput;
    type Event = IdentWriteEvent;
    type Output = IdentWriteSummary;
    type Error = OperationError<IdentWriteKind>;

    fn run(
        input: IdentWriteInput,
        cx: Context<'_, Self>,
    ) -> impl Future<Output = Result<Self::Output, Self::Error>> + Send {
        let emitter = cx.emitter();
        async move {
            cx.blocking(move || write(input, &emitter))
                .await
                .map_err(|error: IdentWriteFailure| error.into_operation())
        }
    }
}

/// One batch of matches, all addressed to the same cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentWriteInput {
    pub cache: ModsCacheSelection,
    #[serde(default)]
    pub entries: Vec<IdentEntry>,
    /// Rewrite a match even when the stored snapshot already holds the same one.
    #[serde(default)]
    pub force: bool,
    /// Refuse to write into a cache owned by a server stash.
    #[serde(default = "default_protect")]
    pub protect_stash: bool,
}

/// One content key and its verified match.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentEntry {
    /// BLAKE3 content identity of the imported object.
    pub key: String,
    pub file: FileMatch,
}

/// Per-entry publication results.
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum IdentWriteEvent {
    Item { index: usize, status: IdentWriteStatus },
}

/// What happened to one entry.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum IdentWriteStatus {
    Written,
    Reused,
    Failed { error: ErrorReport },
}

/// Terminal totals of one batch.
#[derive(Debug, Clone, Copy, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct IdentWriteSummary {
    pub written: usize,
    pub reused: usize,
    pub failed: usize,
}

fn default_protect() -> bool {
    true
}

fn write(
    input: IdentWriteInput,
    emitter: &mcpm_exec::Emitter<IdentWriteEvent>,
) -> Result<IdentWriteSummary, IdentWriteFailure> {
    let selection = input.cache.clone();
    if input.protect_stash {
        let path = selection.path().map_err(IdentWriteFailure::Cache)?;
        if let Some(server) = stash::cache_owner(&path).map_err(IdentWriteFailure::Cache)? {
            return Err(IdentWriteFailure::StashProtected { path, server });
        }
    }
    let environment = RuntimeEnvironment::new().with_mods_cache(Some(selection));
    let cache = environment
        .require_mods_cache()
        .map_err(IdentWriteFailure::Cache)?;
    let mut summary = IdentWriteSummary::default();
    for (index, entry) in input.entries.into_iter().enumerate() {
        emitter
            .checkpoint()
            .map_err(IdentWriteFailure::Cancelled)?;
        let status = publish(cache, &entry, input.force);
        match &status {
            IdentWriteStatus::Written => summary.written += 1,
            IdentWriteStatus::Reused => summary.reused += 1,
            IdentWriteStatus::Failed { .. } => summary.failed += 1,
        }
        emitter
            .emit(IdentWriteEvent::Item { index, status })
            .map_err(IdentWriteFailure::Cancelled)?;
    }
    Ok(summary)
}

fn publish(
    cache: &crate::data::cache::mods::ModsCacheDir,
    entry: &IdentEntry,
    force: bool,
) -> IdentWriteStatus {
    let result = (|| -> anyhow::Result<IdentWriteStatus> {
        let key: Blake3Hash = entry.key.parse().context("invalid cache content identity")?;
        let source = entry.file.reference.project.source.0.as_str();
        if !force && cache.ident_info(key).get(source) == Some(&entry.file) {
            return Ok(IdentWriteStatus::Reused);
        }
        cache
            .write_ident_info(key, &entry.file)
            .context("failed to publish the source identification")?;
        Ok(IdentWriteStatus::Written)
    })();
    result.unwrap_or_else(|error| IdentWriteStatus::Failed {
        error: ErrorReport::capture(&error),
    })
}

#[derive(Debug, thiserror::Error)]
enum IdentWriteFailure {
    #[error("failed to open the Mod cache")]
    Cache(#[source] anyhow::Error),
    #[error(
        "refusing to modify server stash cache at {path:?} (server: {server:?})"
    )]
    StashProtected { path: PathBuf, server: PathBuf },
    #[error("identification cache publication was cancelled")]
    Cancelled(mcpm_exec::Cancelled),
}

impl IdentWriteFailure {
    fn into_operation(self) -> OperationError<IdentWriteKind> {
        let kind = match &self {
            Self::Cache(_) => IdentWriteKind::Cache,
            Self::StashProtected { .. } => IdentWriteKind::StashProtected,
            Self::Cancelled(_) => IdentWriteKind::Cancelled,
        };
        OperationError::from_source(kind, self)
    }
}

/// Stable classification of an identification write failure.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum IdentWriteKind {
    Cache,
    StashProtected,
    Cancelled,
}
