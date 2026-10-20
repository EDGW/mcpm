//! Remote search arguments, source configuration, and output dispatch.

use std::io;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use kako_mods_lib::source::{SearchQuery, SearchSort};

use crate::terminal;

mod columns;
mod output;
mod tui;

#[derive(Args)]
#[command(
    after_help = "Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout).\n\nQUERY format:\n  Plain-text keywords passed to the selected source. Quote multiple words as\n  one argument, e.g. \"Steam 'n' Rails\". Omit QUERY to browse without keywords.\n  Use --game-version and --loader for compatibility filters.\n\nExamples:\n  mcpm mod source search create --source modrinth\n  mcpm mod source search \"Steam 'n' Rails\" --source curseforge\n  mcpm mod source search create --source 'curseforge@KEY'\n  mcpm mod source search create --source modrinth --game-version 1.20.1 --loader fabric\n  mcpm mod source search --source modrinth --sort default\n  mcpm mod source search create --source modrinth -N -C INP\n  mcpm mod source search create --source curseforge --no-ui --columns name,slug,id,url\n  mcpm --raw mod source search create --source modrinth --limit 10"
)]
pub(in crate::commands) struct SearchArgs {
    /// Search terms; quote multiple words. Omit to browse projects.
    #[arg(value_name = "QUERY")]
    query: Option<String>,
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: String,
    /// Filter projects by Minecraft version.
    #[arg(long)]
    game_version: Option<String>,
    /// Loader identifier, e.g. forge, neoforge, fabric, or quilt.
    #[arg(long)]
    loader: Option<String>,
    /// Sort order; "default" uses the selected source's own default ordering.
    #[arg(long, value_enum, default_value = "downloads")]
    sort: Sort,
    /// Number of projects to skip.
    #[arg(long, default_value_t = 0)]
    offset: u64,
    /// Results per page (CurseForge: 1..=50, Modrinth: 1..=100).
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..=100))]
    limit: u32,
    /// Disable the interactive TUI and print a project list.
    #[arg(short = 'N', long)]
    no_ui: bool,
    #[command(flatten)]
    columns: columns::ColumnArgs,
}

impl SearchArgs {
    /// Fills omitted filters from the command's context.
    pub(in crate::commands) fn apply_defaults(&mut self, game_version: &str, loader: &str) {
        self.game_version
            .get_or_insert_with(|| game_version.to_owned());
        self.loader.get_or_insert_with(|| loader.to_owned());
    }
}

pub(in crate::commands) fn run(args: SearchArgs) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create source search runtime")?;
    // Client construction and all requests stay inside the runtime's lifetime.
    let _runtime_guard = runtime.enter();
    let source = super::config::build_source(&args.source)?;
    let query = SearchQuery {
        query: args.query.unwrap_or_default(),
        game_version: args.game_version,
        loader: args.loader,
        sort: args.sort.into(),
        offset: args.offset,
        limit: args.limit,
    };
    let progress = super::progress::Progress::new("Searching projects...");
    let page = runtime
        .block_on(source.search(&query))
        .context("mod source search failed")?;
    drop(progress);
    if !args.no_ui
        && !args.columns.is_explicit()
        && terminal::stdout_is_tty()
        && terminal::stderr_is_tty()
    {
        tui::run(source.as_ref(), &runtime, query, page)
    } else {
        output::write_page(&mut io::stdout().lock(), &page, &args.columns.selected())
            .context("failed to write source search results")
    }
}

#[derive(Clone, Copy, ValueEnum)]
enum Sort {
    /// Use the selected source's own default ordering.
    Default,
    /// Highest total download count first.
    Downloads,
    /// Most recently updated first.
    Updated,
    /// Most recently created first (Modrinth only).
    Newest,
}

impl From<Sort> for SearchSort {
    fn from(value: Sort) -> Self {
        match value {
            Sort::Default => Self::SourceDefault,
            Sort::Downloads => Self::Downloads,
            Sort::Updated => Self::Updated,
            Sort::Newest => Self::Newest,
        }
    }
}
