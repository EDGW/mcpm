//! Typed cache-write request and Host event consumption; no caller-side source scheduling.

use super::{
    args::{Meta, WriteArgs},
    output::Presentation,
};
use anyhow::Result;
use mcpm_backend::operations::mods::{
    cache::{Cache, IdentificationOptions, Input},
    loading::{ScopeInput, selection::SelectionInput},
    source::Configuration,
};
use mcpm_exec::service::Executor;

pub(super) async fn run(
    executor: &Executor,
    scope: ScopeInput,
    selection: SelectionInput,
    writing: WriteArgs,
    specifications: &[Configuration],
    output: &mut Presentation,
) -> Result<()> {
    let enabled = writing.selected_metas()?.contains(&Meta::Ident);
    let identification = enabled.then(|| IdentificationOptions {
        sources: specifications.to_vec(),
        force: writing.force(),
    });
    let mut stream = executor
        .call::<Cache>(Input {
            scope,
            selection,
            options: writing.options()?,
            identification,
        })
        .await?;
    while let Some(event) = stream.next().await {
        output.event(event?)?;
    }
    Ok(())
}
