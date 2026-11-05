//! Collects caller-side graph input and displays backend-built dependency trees.

mod filter;
mod output;
use super::selector::ModSelector;
use anyhow::{Context, Result};
use clap::Args;
use mcpm_backend::operations::mods::dep_graph::{
    BuildOutput, GraphInput, Identifier, Inspect, InspectAction, InspectInput, InspectOutput,
    ViewInput,
};
use std::{
    io::{self, IsTerminal, Read, Write},
    path::PathBuf,
};

#[derive(Args, Debug)]
pub(super) struct ViewArgs {
    /// Local graph JSON input; omit or use - to read stdin.
    #[arg(short, long, value_name = "FILE")]
    input: Option<PathBuf>,
    /// Root nodes as loader:modid; defaults to all nodes with candidates, sorted by loader/modid.
    #[arg(value_name = "LOADER:MODID", num_args = 0..)]
    targets: Vec<ModSelector>,
    /// Show mods declaring relations to the target, following incoming edges.
    #[arg(short = 'R', long)]
    reverse: bool,
    /// Hide roots with no displayed children after applying all filters.
    #[arg(short = 'S', long, visible_alias = "iir")]
    ignore_isolated_root: bool,
    #[command(flatten)]
    filter: filter::FilterArgs,
}
#[derive(Debug, thiserror::Error)]
#[error("no graph input: specify -i FILE or pipe graph JSON on stdin")]
struct MissingInput;
pub(super) async fn run(args: ViewArgs) -> Result<()> {
    let graph = if let Some(path) = args.input.filter(|path| path.as_os_str() != "-") {
        GraphInput::File(path)
    } else {
        let input = io::stdin();
        if input.is_terminal() {
            return Err(MissingInput.into());
        }
        let mut bytes = Vec::new();
        input
            .lock()
            .read_to_end(&mut bytes)
            .context("failed to read dependency graph from stdin")?;
        GraphInput::Bytes(bytes)
    };
    let executor = mcpm_exec::service::Executor::open(mcpm_exec::service::Target::Local).await?;
    let InspectOutput::View(view) = executor
        .call::<Inspect>(InspectInput {
            graph,
            action: InspectAction::View(ViewInput {
                targets: args
                    .targets
                    .into_iter()
                    .map(|target| Identifier::from(&target.0))
                    .collect(),
                reverse: args.reverse,
                ignore_isolated_root: args.ignore_isolated_root,
                filter: args.filter.into_input(),
            }),
        })
        .await?
    else {
        anyhow::bail!("unexpected dependency graph inspection result");
    };
    output::write_graph(&view, &mut io::stdout().lock())?;
    Ok(())
}
/// Displays the problem trees returned by the same Host build used for export.
pub(in crate::commands) fn write_problems(
    graph: &BuildOutput,
    output: &mut impl Write,
) -> Result<usize> {
    output::write_graph(&graph.problems, output)
}
