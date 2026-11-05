//! Shared Host inspection acquisition and transport-safe results for info and dependencies.

use anyhow::Result;
use mcpm_exec::operation::StreamingOutput;
use serde::{Deserialize, Serialize};
use std::num::NonZeroUsize;

use super::{ScopeInput, selection::SelectionInput};
use crate::operations::stream::blocking_stream;

mod load;
pub use load::load;
mod model;
pub use model::*;
mod validation;
pub use validation::Validator;

/// Business selection and execution resources, without presentation settings.
#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub scope: ScopeInput,
    pub selection: SelectionInput,
    pub parallelism: NonZeroUsize,
}

pub async fn start(input: Input) -> Result<StreamingOutput<Event>> {
    let (resolved, selection) = tokio::task::spawn_blocking(move || {
        let resolved = input.scope.resolve()?;
        resolved.environment.mods_cache()?;
        Ok::<_, anyhow::Error>((resolved, input.selection.into_selection()?))
    })
    .await??;
    Ok(blocking_stream(move |sender| {
        load(
            selection,
            resolved.scope,
            resolved.loaders,
            resolved.hide_unrecognized,
            &resolved.environment,
            input.parallelism.get(),
            || sender.checkpoint(),
            |event| sender.send(event),
        )
    }))
}
