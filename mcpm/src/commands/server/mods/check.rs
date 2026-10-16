//! Checks the server graph using the shared problem view and reports an empty result in green.

use std::io::{self, Write};

use anyhow::{Context, Result};
use clap::Args;
use owo_colors::Style;

use crate::commands::{mod_support::dep_graph, output_style::OutputStyle};

use super::graph_input::ServerArgs;

#[derive(Args)]
pub(super) struct CheckArgs {
    #[command(flatten)]
    server: ServerArgs,
}

pub(super) fn run(args: CheckArgs) -> Result<()> {
    let (source, external) = args.server.resolve()?;
    let graph =
        dep_graph::build_graph(source.container().directory(), &source.loaders(), external)?;
    let mut output = io::stdout().lock();
    if dep_graph::write_problems(&graph, &mut output)? == 0 {
        let style = OutputStyle::new();
        let result = if graph.identification_failures().is_empty() {
            style.paint("Check passed. No problems found.", Style::new().green())
        } else {
            style.warning("Check incomplete. Some mod files could not be identified.")
        };
        writeln!(output, "{result}").context("failed to write dependency check result")?;
    }
    output
        .flush()
        .context("failed to flush dependency check output")
}
