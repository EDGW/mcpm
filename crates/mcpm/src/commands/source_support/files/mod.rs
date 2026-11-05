//! Batch remote file listing from project IDs, slugs, and line-based stdin.

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use mcpm_backend::operations::mods::source::{
    Configuration, files::Project, server::Context as ServerContext,
};
use mcpm_lib::source::{FilePage, FileQuery, ProjectFile, ReleaseType};

mod columns;
mod output;
pub(super) mod tui;
pub(in crate::commands) use tui::open_selected;

#[derive(Args)]
#[command(
    after_help = format!("Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout); Remote supported through --ssh.\n\nSource requests, selected files and pagination use the selected execution host.\n\nExamples:\n  {tools} mods source files --source modrinth create ZzjhlDgM\n  {tools} mods source files --source curseforge 688231 create\n  {tools} mods source files --source 'curseforge@KEY' create-steam-n-rails\n  printf '%s\\n' create create-steam-n-rails | {tools} mods source files --source modrinth\n  {tools} mods source files --source modrinth create -N\n  {tools} mods source files --source modrinth sodium --selected yaoBL9D9\n  {tools} mods source files --source curseforge 688231 --columns id,filename,size\n  {tools} --raw mods source files --source curseforge 688231", tools = crate::program::roots().tools)
)]
pub(in crate::commands) struct FilesArgs {
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: Configuration,
    /// Project IDs or exact slugs. Redirected stdin is appended, one per line.
    /// Multiple projects automatically use --no-ui list output.
    #[arg(value_name = "PROJECT", num_args = 0..)]
    projects: Vec<String>,
    /// Pin this file above the list, regardless of filters or page (one project only).
    #[arg(long, value_name = "FILE_ID")]
    selected: Option<String>,
    /// Filter files by Minecraft version.
    #[arg(long)]
    game_version: Option<String>,
    /// Match any loader, e.g. forge,fabric; repeat or separate with commas.
    #[arg(long, visible_alias = "loaders", value_delimiter = ',')]
    loader: Vec<String>,
    /// Filter by release channel before pagination.
    #[arg(long, value_enum)]
    release_type: Option<Channel>,
    /// Number of matching files to skip for each project.
    #[arg(long, default_value_t = 0)]
    offset: u64,
    /// Maximum matching files per project; newest first.
    #[arg(long, default_value_t = 20, value_parser = clap::value_parser!(u32).range(1..))]
    limit: u32,
    /// Disable interactive browsing and print numbered file summaries.
    #[arg(short = 'N', long)]
    no_ui: bool,
    #[command(flatten)]
    columns: columns::ColumnArgs,
}

impl std::fmt::Debug for FilesArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FilesArgs")
            .field("source", &self.source.label())
            .finish_non_exhaustive()
    }
}

pub(in crate::commands) fn run(args: FilesArgs, ssh: crate::ssh::SshArgs) -> Result<()> {
    let runtime = crate::commands::execution::Runtime::new()?;
    let (executor, _, _) = runtime.block_on(ssh.create_executor(None))?;
    run_at(args, &executor, None)
}

pub(in crate::commands) fn run_at(
    mut args: FilesArgs,
    executor: &mcpm_exec::service::Executor,
    server: Option<ServerContext>,
) -> Result<()> {
    let stdin = io::stdin();
    if !stdin.is_terminal() {
        for line in stdin.lock().lines() {
            let line = line.context("failed to read project identifier from stdin")?;
            let value = line.trim();
            if !value.is_empty() {
                args.projects.push(value.to_owned());
            }
        }
    }
    ensure!(
        !args.projects.is_empty(),
        "provide project IDs or slugs as arguments or one per stdin line"
    );
    ensure!(
        args.selected.is_none() || args.projects.len() == 1,
        "--selected requires exactly one project"
    );
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let _guard = runtime.enter();
    let source = args.source;
    let mut failed = 0;
    let interactive = !args.no_ui
        && !args.columns.is_explicit()
        && args.projects.len() == 1
        && crate::terminal::stdout_is_tty()
        && crate::terminal::stderr_is_tty();
    let mut projects = Vec::new();
    let mut selected_file = None;
    let mut output = io::stdout().lock();
    let query = FileQuery {
        game_versions: args.game_version.into_iter().collect(),
        loaders: args.loader,
        release_type: args.release_type.map(Into::into),
        offset: args.offset,
        limit: args.limit,
    };
    for value in args.projects {
        let progress = super::progress::Progress::new(format!(
            "Resolving project {}...",
            crate::display::style::text(&value)
        ));
        let result = runtime.block_on(super::config::files(
            &executor,
            &source,
            Project::Identifier(value.clone()),
            &query,
            args.selected.clone(),
            server.as_ref(),
        ));
        drop(progress);
        match result {
            Ok(result) => {
                let (reference, page, selected) = (result.project, result.page, result.selected);
                if interactive {
                    projects.push((value, reference, page));
                    selected_file = selected;
                } else {
                    output::write_files(
                        &mut output,
                        &reference,
                        &page,
                        selected.as_ref(),
                        &args.columns,
                    )?;
                }
            }
            Err(error) => {
                crate::diagnostics::error(
                    &error.context(format!("failed to list files for project {value:?}")),
                );
                failed += 1;
            }
        }
    }
    output.flush()?;
    drop(output);
    if interactive && !projects.is_empty() {
        tui::run(
            executor,
            &source,
            &runtime,
            query,
            projects,
            selected_file,
            server.as_ref(),
        )?;
    }
    ensure!(failed == 0, "failed to list files for {failed} project(s)");
    Ok(())
}

/// Keeps provider pagination and numbering intact while hiding the pinned file.
fn other_files<'a>(
    page: &'a FilePage,
    selected: Option<&'a ProjectFile>,
) -> impl Iterator<Item = (usize, &'a ProjectFile)> {
    page.files.iter().enumerate().filter(move |(_, file)| {
        selected.is_none_or(|selected| file.reference != selected.reference)
    })
}

#[derive(Clone, Copy, ValueEnum, Debug)]
enum Channel {
    Release,
    Beta,
    Alpha,
}

impl From<Channel> for ReleaseType {
    fn from(value: Channel) -> Self {
        match value {
            Channel::Release => Self::Release,
            Channel::Beta => Self::Beta,
            Channel::Alpha => Self::Alpha,
        }
    }
}
