//! Reverse lookup of local files on a configured remote mod source.

use std::{
    io::{self, Write},
    path::PathBuf,
};

use anyhow::{Result, ensure};
use clap::Args;

mod columns;
mod output;

#[derive(Args)]
#[command(
    after_help = "Modes: Console and Raw (--raw or redirected stdout).\n\nPaths are literal local file paths. Redirected stdin is appended, one path per line.\nUnmatched files are reported as unmatched; read or request failures exit unsuccessfully.\n\nExamples:\n  mcpm mod source identify --source modrinth ./mods/create.jar\n  mcpm mod source identify --source curseforge ./mods/a.jar ./mods/b.jar\n  mcpm mod source identify --source 'curseforge@KEY' ./mods/create.jar\n  printf '%s\\n' ./mods/a.jar ./mods/b.jar | mcpm mod source identify --source modrinth\n  mcpm --raw mod source identify --source modrinth ./mods/create.jar --columns path,project,id,status"
)]
pub(in crate::commands) struct IdentifyArgs {
    /// Local files to identify by their contents, not their names.
    #[arg(value_name = "PATH", num_args = 0..)]
    paths: Vec<PathBuf>,
    #[command(flatten)]
    options: IdentifyOptions,
}

/// Source and presentation options, independent of local path selection.
#[derive(Args)]
pub(in crate::commands) struct IdentifyOptions {
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: String,
    #[command(flatten)]
    columns: columns::ColumnArgs,
}

pub(in crate::commands) fn run(args: IdentifyArgs) -> Result<()> {
    let paths = crate::commands::mod_support::stdin::append(args.paths)?;
    run_paths(args.options, paths)
}

/// Identifies already-resolved paths without reading stdin again.
pub(in crate::commands) fn run_paths(options: IdentifyOptions, paths: Vec<PathBuf>) -> Result<()> {
    ensure!(
        !paths.is_empty(),
        "provide file paths as arguments or one per stdin line"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = runtime.enter();
    let source = super::config::build_source(&options.source)?;
    let columns = options.columns.selected();
    let mut output = io::stdout().lock();
    let mut failed = 0;
    for (index, path) in paths.iter().enumerate() {
        let result = {
            let _progress = super::progress::Progress::new(format!(
                "Identifying {}...",
                crate::commands::output_style::text(&path.to_string_lossy())
            ));
            runtime.block_on(source.identify_file(path))
        };
        match result {
            Ok(found) => output::write_match(
                &mut output,
                index + 1,
                path,
                source.id(),
                found.as_ref(),
                &columns,
            )?,
            Err(error) => {
                crate::diagnostics::error(
                    &error.context(format!("failed to identify file {path:?}")),
                );
                failed += 1;
            }
        }
    }
    output.flush()?;
    ensure!(failed == 0, "failed to identify {failed} file(s)");
    Ok(())
}
