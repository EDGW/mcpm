//! Executor orchestration and presentation for Host-owned explicit Mod caching.

pub(in crate::commands) use mcpm_backend::operations::mods::cache::{
    Event, Record, Stage, Status, Summary,
};
pub(in crate::commands) mod args;
pub(in crate::commands) mod identification;
pub(in crate::commands) mod output;
mod validation;

pub(in crate::commands) async fn run(
    executor: &mcpm_exec::service::Executor,
    scope: mcpm_backend::operations::mods::loading::ScopeInput,
    selection: mcpm_backend::operations::mods::loading::selection::SelectionInput,
    writing: args::WriteArgs,
    specifications: &[mcpm_backend::operations::mods::source::Configuration],
    mut output: output::Presentation,
) -> anyhow::Result<()> {
    identification::run(
        executor,
        scope,
        selection,
        writing,
        specifications,
        &mut output,
    )
    .await?;
    output.finish()
}
