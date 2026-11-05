//! Host-side Mod list acquisition; local calls retain loaded trees without serialization.

use super::loading::{
    self, ScopeInput,
    scan::{self, CacheSummary, ScanSession},
    selection::{SelectedFile, SelectionInput},
};
use crate::operations::stream::blocking_stream;
use anyhow::{Context, Result};
use mcpm_exec::{
    error::PortableError,
    operation::{Operation, StreamingOutput},
};
use mcpm_lib::local::{
    cache::mods::ModsCacheSelection,
    mcmod::{ModDirectory, ModEntry},
};
use serde::{Deserialize, Serialize};
use std::{cell::RefCell, num::NonZeroUsize, path::PathBuf};

pub struct List;
impl Operation for List {
    const ID: &'static str = "mod.list";
    type Input = Input;
    type Output = StreamingOutput<Event>;
    async fn run(input: Input) -> Result<Self::Output> {
        let resolved = tokio::task::spawn_blocking(move || {
            let resolved = input.scope.resolve()?;
            resolved.environment.mods_cache()?;
            Ok::<_, anyhow::Error>(resolved)
        })
        .await??;
        Ok(blocking_stream(move |sender| {
            let cache = loading::cache_selection(&resolved.environment)?;
            let mut source_loaders = if resolved.hide_unrecognized {
                resolved
                    .loaders
                    .keys()
                    .map(|id| id.as_str().to_owned())
                    .collect::<Vec<_>>()
            } else {
                Vec::new()
            };
            source_loaders.sort();
            let observe = |event| match event {
                scan::Event::Progress { completed, total } => {
                    sender.send(Event::Progress { completed, total })
                }
                scan::Event::Diagnostic { error } => sender.send(Event::Error { error }),
            };
            let counts = match input.selection {
                Selection::Queries(selection) => loading::list_selection::load(
                    selection.into_selection()?,
                    resolved.scope,
                    resolved.loaders,
                    &resolved.environment,
                    input.parallelism.get(),
                    || sender.checkpoint(),
                    observe,
                    |file| sender.send(Event::File { file }),
                )?,
                Selection::Paths(paths) => {
                    let mut entries = Vec::new();
                    let mut input_errors = 0;
                    for path in paths {
                        sender.checkpoint()?;
                        match read_input(path.clone())
                            .with_context(|| format!("failed to read list input {path:?}"))
                        {
                            Ok(found) => entries.extend(found),
                            Err(error) => {
                                input_errors += 1;
                                sender.send(Event::Error {
                                    error: error.into(),
                                })?;
                            }
                        }
                    }
                    let mut session = ScanSession::new(resolved.loaders, &resolved.environment)
                        .with_options(input.parallelism.get().into())?;
                    let paths = entries
                        .into_iter()
                        .map(|entry| entry.path().to_owned())
                        .collect::<Vec<_>>();
                    let completed = RefCell::new(0);
                    observe(scan::Event::Progress {
                        completed: 0,
                        total: paths.len(),
                    })?;
                    let mut files = vec![None; paths.len()];
                    session.batch(
                        &paths,
                        || sender.checkpoint(),
                        |index, result, fresh| {
                            if fresh {
                                for error in &result.errors {
                                    sender.send(Event::Error {
                                        error: error
                                            .error
                                            .clone()
                                            .into_error()
                                            .context(format!(
                                                "failed to inspect {:?}",
                                                paths[index]
                                            ))
                                            .into(),
                                    })?;
                                }
                            }
                            files[index] = result.file.clone();
                            *completed.borrow_mut() += 1;
                            observe(scan::Event::Progress {
                                completed: *completed.borrow(),
                                total: paths.len(),
                            })
                        },
                    )?;
                    for scanned in files.into_iter().flatten() {
                        sender.send(Event::File {
                            file: SelectedFile {
                                scanned,
                                records: None,
                            },
                        })?;
                    }
                    loading::list_selection::Counts {
                        input_errors,
                        failed: session.failed_count(),
                        cache: session.cache_summary(),
                    }
                }
            };
            sender.send(Event::Done {
                input_errors: counts.input_errors,
                failed: counts.failed,
                cache: counts.cache,
                source_loaders,
                hide_unrecognized: resolved.hide_unrecognized,
                cache_selection: cache,
            })
        }))
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Input {
    pub scope: ScopeInput,
    pub selection: Selection,
    pub parallelism: NonZeroUsize,
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "kind", content = "input", rename_all = "snake_case")]
pub enum Selection {
    Paths(Vec<PathBuf>),
    Queries(SelectionInput),
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum Event {
    Progress {
        completed: usize,
        total: usize,
    },
    Error {
        error: PortableError,
    },
    File {
        file: SelectedFile,
    },
    Done {
        input_errors: usize,
        failed: usize,
        cache: CacheSummary,
        source_loaders: Vec<String>,
        hide_unrecognized: bool,
        cache_selection: Option<ModsCacheSelection>,
    },
}

fn read_input(path: PathBuf) -> Result<Vec<ModEntry>> {
    let metadata = std::fs::metadata(&path)?;
    if metadata.is_dir() {
        Ok(ModDirectory::new(path)?.entries()?)
    } else if metadata.is_file() {
        Ok(vec![ModEntry::new(path)?])
    } else {
        anyhow::bail!("list input {path:?} is neither a regular file nor a directory")
    }
}
