//! Calls Host graph construction and writes the exported artifact on the caller.

use super::selector::ExternalSelector;
use crate::progress::Progress;
use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::operations::mods::{
    dep_graph::{
        Build, BuildEvent, BuildInput, BuildOutput, ExternalInput, Identifier, write_export,
    },
    loading::ScopeInput,
};
use mcpm_exec::service::Executor;
use std::{
    fs::File,
    io::{self, BufWriter, Write},
    path::PathBuf,
};

#[derive(Args, Debug)]
pub(in crate::commands) struct BuildOptions {
    /// Write JSON to this local file instead of stdout; overwrite an existing file.
    #[arg(short, long, value_name = "FILE")]
    output: Option<PathBuf>,
    /// Add an external candidate as loader:modid[:version]; repeat to add more.
    #[arg(long, value_name = "LOADER:MODID[:VERSION]")]
    external: Vec<ExternalSelector>,
}
impl BuildOptions {
    pub(in crate::commands) async fn run(
        self,
        scope: ScopeInput,
        executor: &Executor,
    ) -> Result<()> {
        let graph = build_graph(
            executor,
            BuildInput {
                scope,
                external: self
                    .external
                    .into_iter()
                    .map(|candidate| ExternalInput {
                        identifier: Identifier::from(&candidate.identifier),
                        version: candidate.version,
                    })
                    .collect(),
            },
        )
        .await?;
        if let Some(path) = self.output {
            let file = File::create(&path)
                .with_context(|| format!("failed to create dependency graph output {path:?}"))?;
            let mut output = BufWriter::new(file);
            write_export(&graph.snapshot, &mut output)
                .with_context(|| format!("failed to write dependency graph to {path:?}"))?;
            output
                .flush()
                .with_context(|| format!("failed to flush dependency graph output {path:?}"))?;
        } else {
            let mut output = io::stdout().lock();
            write_export(&graph.snapshot, &mut output)
                .context("failed to write dependency graph to stdout")?;
            output
                .flush()
                .context("failed to flush dependency graph stdout")?;
        }
        Ok(())
    }
}
pub(in crate::commands) async fn build_graph(
    executor: &Executor,
    input: BuildInput,
) -> Result<BuildOutput> {
    let mut stream = executor.call::<Build>(input).await?;
    let progress = Progress::new("Scanning mod directory...", "Identifying mods");
    let mut result = None;
    while let Some(event) = stream.next().await {
        match event? {
            BuildEvent::Progress { completed, total } => {
                if completed == 0 {
                    progress.set_total(total);
                }
                progress.set_position(completed);
            }
            BuildEvent::Diagnostic(error) => progress.report(&error.into_inner()),
            BuildEvent::Finished(output) => result = Some(output),
        }
    }
    result.context("dependency graph build ended without a result")
}
