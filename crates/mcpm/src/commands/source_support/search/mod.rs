//! Remote search arguments, source configuration, and output dispatch.

use std::io;

use anyhow::{Context, Result};
use clap::{Args, ValueEnum};
use mcpm_backend::operations::mods::source::{Configuration, server::Context as ServerContext};
use mcpm_lib::source::{SearchQuery, SearchSort};

use crate::terminal;

mod columns;
mod expression;
mod output;
mod tui;
mod versions;

#[derive(Args)]
#[command(after_help = format!("{}\n\n{}", expression::HELP, ui_help()))]
pub(in crate::commands) struct SearchArgs {
    /// Keywords followed by optional @version/@loader filters; quote the expression.
    #[arg(value_name = "QUERY")]
    query: Option<String>,
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: Configuration,
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

impl std::fmt::Debug for SearchArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchArgs")
            .field("source", &self.source.label())
            .finish_non_exhaustive()
    }
}

pub(in crate::commands) fn run(args: SearchArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let runtime = crate::commands::execution::Runtime::new()?;
    let (executor, _, _) = runtime.block_on(ssh.create_executor(None))?;
    run_at(args, &executor, None)
}

pub(in crate::commands) fn run_at(
    args: SearchArgs,
    executor: &mcpm_exec::service::Executor,
    server: Option<ServerContext>,
) -> Result<()> {
    let expression = args.query.unwrap_or_default();
    let defaults = SearchQuery {
        game_versions: args.game_version.into_iter().collect(),
        loaders: args
            .loader
            .into_iter()
            .map(|loader| loader.to_ascii_lowercase())
            .collect(),
        sort: args.sort.into(),
        limit: args.limit,
        ..Default::default()
    };
    let parsed = expression::Expression::parse(&expression)?;
    let query_server = parsed.server_context(server.as_ref());
    let mut query = parsed.apply(&defaults);
    query.offset = args.offset;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .context("failed to create source search runtime")?;
    let source = args.source;
    let progress = super::progress::Progress::new("Searching projects...");
    let page = runtime
        .block_on(super::config::search(
            executor,
            &source,
            &query,
            query_server.as_ref(),
        ))
        .context("mod source search failed")?;
    drop(progress);
    if !args.no_ui
        && !args.columns.is_explicit()
        && terminal::stdout_is_tty()
        && terminal::stderr_is_tty()
    {
        tui::run(
            executor,
            &source,
            &runtime,
            defaults,
            expression,
            query,
            page,
            server.as_ref(),
            query_server,
        )
    } else {
        output::write_page(&mut io::stdout().lock(), &page, &args.columns.selected())
            .context("failed to write source search results")
    }
}

#[derive(Clone, Copy, ValueEnum, Debug)]
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

fn ui_help() -> String {
    format!(
        r#"Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout); Remote supported through --ssh.

Quote the whole expression as one shell argument. Omit QUERY to browse.
Use --game-version and --loader for default compatibility filters.
Source requests, version catalogs and file browsing use the selected execution host.

TUI: / edits the top search field; Enter submits, Esc discards edits.
Value suggestions: Up/Down selects, Enter/Tab inserts, Esc closes; Enter again searches.
Loader suggestions: Forge, NeoForge, Fabric. Versions load from the current source.
Version catalogs are cached for this session; Ctrl-R retries failed loads.
Tab switches Search / Results / Details. Enter on a result opens files.
Syntax errors show a column and keep previous results.
Details: Up/Down selects a field, c copies its full value, Enter expands/collapses.

Examples:
  {tools} mods source search create --source modrinth
  {tools} mods source search 'create @version 1.20.1 @loader fabric' --source modrinth
  {tools} mods source search 'create @loader (forge | neoforge)' --source curseforge
  {tools} mods source search '@version * @loader *' --source modrinth
  {tools} mods source search create --source 'curseforge@KEY'
  {tools} mods source search create --source modrinth --game-version 1.20.1 --loader fabric
  {tools} mods source search --source modrinth --sort default
  {tools} mods source search create --source modrinth -N -C INP
  {tools} mods source search create --source curseforge --no-ui --columns name,slug,id,url
  {tools} --raw mods source search create --source modrinth --limit 10"#,
        tools = crate::program::roots().tools
    )
}
