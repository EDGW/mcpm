//! Batch remote file listing from project IDs, slugs, and line-based stdin.

use std::io::{self, BufRead, IsTerminal, Write};

use anyhow::{Context, Result, ensure};
use clap::{Args, ValueEnum};
use kako_mods_lib::source::{FileId, FilePage, FileQuery, FileRef, ProjectFile, ReleaseType};

mod columns;
mod output;
pub(super) mod tui;
pub(in crate::commands) use tui::open_selected;

#[derive(Args, Debug)]
#[command(
    after_help = "Modes: TUI (default in terminals), Console (--no-ui), Raw (--raw or redirected stdout).\n\nExamples:\n  mcpm mod source files --source modrinth create ZzjhlDgM\n  mcpm mod source files --source curseforge 688231 create\n  mcpm mod source files --source 'curseforge@KEY' create-steam-n-rails\n  printf '%s\\n' create create-steam-n-rails | mcpm mod source files --source modrinth\n  mcpm mod source files --source modrinth create -N\n  mcpm mod source files --source modrinth sodium --selected yaoBL9D9\n  mcpm mod source files --source curseforge 688231 --columns id,filename,size\n  mcpm --raw mod source files --source curseforge 688231"
)]
pub(in crate::commands) struct FilesArgs {
    /// modrinth, curseforge (Curse.tools), or curseforge@KEY (official API).
    #[arg(long, value_name = "SOURCE")]
    source: String,
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

impl FilesArgs {
    /// Fills omitted filters from the command's context.
    pub(in crate::commands) fn apply_defaults(&mut self, game_version: &str, loader: &str) {
        self.game_version
            .get_or_insert_with(|| game_version.to_owned());
        if self.loader.is_empty() {
            self.loader.push(loader.to_owned());
        }
    }
}

pub(in crate::commands) fn run(mut args: FilesArgs) -> Result<()> {
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
    let source = super::config::build_source(&args.source)?;
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
        let result = runtime.block_on(async {
            let reference = source.resolve_project(&value).await?;
            drop(progress);
            let _progress = super::progress::Progress::new(format!(
                "Fetching files for {}...",
                crate::display::style::text(&value)
            ));
            let selected = match &args.selected {
                Some(id) => Some(
                    source
                        .get_file(&FileRef {
                            project: reference.clone(),
                            id: FileId(id.clone()),
                        })
                        .await
                        .with_context(|| format!("failed to fetch selected file {id:?}"))?,
                ),
                None => None,
            };
            let page = source.list_files(&reference, &query).await?;
            Ok::<_, anyhow::Error>((reference, page, selected))
        });
        match result {
            Ok((reference, page, selected)) => {
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
        tui::run(source.as_ref(), &runtime, query, projects, selected_file)?;
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
