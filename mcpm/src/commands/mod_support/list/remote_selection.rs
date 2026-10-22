//! Transport adapter for ordered list selections, reusing list records and local presentation.

use anyhow::{Context, Result, ensure};
use kako_mods_lib::local::modloader::{BUILTIN_LOADERS, LoaderId, ModLoaderMeta};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::HashMap;

use super::{ListDisplayArgs, protocol::Record, records, selection};
use crate::{
    commands::{
        mod_support::{
            scan::{self, ProgressState},
            scan_output::ScanOutput,
            selection::{Scope, Selection},
        },
        source_support::identify::Location,
    },
    diagnostics::ErrorReport,
    remote::{client::Session, command::Output},
    ssh::SshArgs,
};

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Scope {
        loaders: Vec<String>,
    },
    Scan {
        completed: usize,
        total: usize,
    },
    Error {
        error: ErrorReport,
    },
    Item {
        index: usize,
        record: Record,
    },
    Done {
        files: usize,
        input_errors: usize,
        failed: usize,
    },
}

pub(in crate::commands) fn serve(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    parallelism: usize,
    output: &Output,
) -> Result<()> {
    output.send(Event::Scope {
        loaders: super::source_loaders(&loaders, true),
    })?;
    let mut files = 0;
    let counts = selection::load(
        selection,
        scope,
        loaders,
        parallelism,
        || output.checkpoint(),
        |event| {
            output.send(match event {
                scan::Event::Progress { completed, total } => Event::Scan { completed, total },
                scan::Event::Diagnostic { error } => {
                    crate::logging::record_error(&error.clone().into_error());
                    Event::Error { error }
                }
            })
        },
        |selected| {
            output.send(Event::Item {
                index: files,
                record: Record::from_selected(&selected),
            })?;
            files += 1;
            Ok(())
        },
    )?;
    output.send(Event::Done {
        files,
        input_errors: counts.input_errors,
        failed: counts.failed,
    })
}

pub(in crate::commands) fn run_ssh(
    ssh: SshArgs,
    command: &str,
    contents: Value,
    display: ListDisplayArgs,
) -> Result<()> {
    tracing::info!(command, "Opening SSH data source for server mod list");
    let mut session =
        Session::start(&mut ssh.command()?).context("failed to open SSH mcpm session")?;
    let output = ScanOutput::remote();
    let mut validator = Validator::default();
    let mut files = Vec::new();
    let mut source_loaders = Vec::new();
    let mut counts = None;
    session.submit(command, contents)?.collect(|reply| {
        let event: Event = reply
            .decode()
            .context("invalid remote server mod list contents")?;
        validator.accept(&event)?;
        match event {
            Event::Scope { loaders } => source_loaders = loaders,
            Event::Scan { completed, total } => {
                output.event(scan::Event::Progress { completed, total })?
            }
            Event::Error { error } => output.event(scan::Event::Diagnostic { error })?,
            Event::Item { record, .. } => {
                let loaded = records::convert(vec![record], false)?;
                ensure!(
                    loaded.files.len() == 1 && loaded.input_errors == 0,
                    "invalid selected list file"
                );
                files.extend(loaded.files);
            }
            Event::Done {
                input_errors,
                failed,
                ..
            } => counts = Some((input_errors, failed)),
        }
        Ok(())
    })?;
    validator.finish()?;
    drop(output);
    let (input_errors, failed) = counts.context("missing remote server mod list completion")?;
    let result = super::present(
        files,
        true,
        display,
        input_errors,
        failed,
        source_loaders,
        Location::Remote(&session),
    );
    session.close()?;
    result
}

#[derive(Default)]
struct Validator {
    scope: bool,
    scan: ProgressState,
    files: usize,
    errors: bool,
    done: bool,
}
impl Validator {
    fn accept(&mut self, event: &Event) -> Result<()> {
        ensure!(!self.done, "remote list data after completion");
        if let Event::Scope { loaders } = event {
            ensure!(
                !self.scope && !loaders.is_empty(),
                "invalid remote list scope"
            );
            let mut seen = std::collections::HashSet::new();
            ensure!(
                loaders
                    .iter()
                    .all(|id| BUILTIN_LOADERS.contains_key(id.as_str()) && seen.insert(id)),
                "unknown or duplicate remote scope loader"
            );
            self.scope = true;
            return Ok(());
        }
        ensure!(self.scope, "remote list missing scope");
        match event {
            Event::Scan { completed, total } => self.scan.accept(*completed, *total)?,
            Event::Error { .. } => self.errors = true,
            Event::Item { index, record } => {
                ensure!(record.name.is_some(), "selected list item is not a file");
                ensure!(
                    record.errors.is_empty() || self.errors,
                    "list file failure without diagnostics"
                );
                ensure!(
                    self.scan.is_complete() && *index == self.files,
                    "remote list file index or scan mismatch"
                );
                self.files += 1;
            }
            Event::Done {
                files,
                input_errors,
                failed,
            } => {
                ensure!(
                    self.scan.is_complete() && *files == self.files,
                    "remote list completion mismatch"
                );
                ensure!(
                    (*input_errors > 0 || *failed > 0) == self.errors,
                    "remote list failures disagree with diagnostics"
                );
                self.done = true;
            }
            Event::Scope { .. } => unreachable!(),
        }
        Ok(())
    }
    fn finish(self) -> Result<()> {
        ensure!(self.done, "missing remote list completion");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scope() -> Event {
        Event::Scope {
            loaders: vec!["fabric".into()],
        }
    }
    fn done(files: usize, failed: usize) -> Event {
        Event::Done {
            files,
            input_errors: 0,
            failed,
        }
    }
    fn item(index: usize, failed: bool) -> Event {
        Event::Item { index, record: serde_json::from_value(serde_json::json!({
            "path":"C:\\server\\example.jar", "name":"example", "enabled":true,
            "status":if failed { "failed" } else { "unrecognized" }, "metadata":[],
            "errors":if failed { serde_json::json!([{"loader":"fabric","message":"bad archive","causes":[]}]) } else { serde_json::json!([]) }
        })).unwrap() }
    }

    #[test]
    fn validates_stream_order_progress_and_failure_accounting_independent_of_rows() {
        let mut valid = Validator::default();
        // An index-only failure may be absent from all selected rows.
        for event in [
            scope(),
            Event::Scan {
                completed: 0,
                total: 1,
            },
            Event::Error {
                error: ErrorReport {
                    message: "bad archive".into(),
                    causes: Vec::new(),
                },
            },
            Event::Scan {
                completed: 1,
                total: 1,
            },
            item(0, false),
            item(1, false),
            done(2, 1),
        ] {
            valid.accept(&event).unwrap();
        }
        valid.finish().unwrap();
        for events in [
            vec![done(0, 0)],
            vec![scope(), scope()],
            vec![
                scope(),
                Event::Scan {
                    completed: 0,
                    total: 1,
                },
                item(0, false),
            ],
            vec![scope(), item(1, false)],
            vec![scope(), item(0, true)],
            vec![scope(), done(0, 1)],
            vec![scope(), item(0, false), done(0, 0)],
            vec![scope(), done(0, 0), item(0, false)],
        ] {
            let mut validator = Validator::default();
            assert!(events.iter().any(|event| validator.accept(event).is_err()));
        }
        assert!(Validator::default().finish().is_err());
    }
}
