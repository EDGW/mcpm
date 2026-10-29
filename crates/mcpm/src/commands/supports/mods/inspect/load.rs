//! Ordered mod selection and inspection events, independent of transport and rendering.

use mcpm_backend::local::runtime::RuntimeEnvironment;
use std::{cell::RefCell, collections::HashMap, path::PathBuf};

use anyhow::Result;
use mcpm_backend::local::modloader::{LoaderId, ModLoaderMeta};

use super::{Event, File};
use crate::{
    commands::supports::mods::{
        scan::ScanSession,
        selection::{Resolver, Scope, Selection},
    },
    diagnostics::ErrorReport,
};

pub(in crate::commands) fn load(
    selection: Selection,
    scope: Scope,
    loaders: HashMap<LoaderId, ModLoaderMeta>,
    hide_unrecognized: bool,
    environment: &RuntimeEnvironment,
    loading: impl Into<crate::commands::supports::mods::scan::Options>,
    checkpoint: impl Fn() -> Result<()>,
    emit: impl FnMut(Event) -> Result<()>,
) -> Result<()> {
    let started = std::time::Instant::now();
    tracing::info!("Resolving mod inspection inputs");
    let emit = RefCell::new(emit);
    let observe = |event| {
        (emit.borrow_mut())(match event {
            crate::commands::supports::mods::scan::Event::Progress { completed, total } => {
                Event::Scan { completed, total }
            }
            crate::commands::supports::mods::scan::Event::Diagnostic { error } => {
                Event::Error { error }
            }
        })
    };
    let mut resolver = Resolver::new(
        ScanSession::new(loaders, environment)
            .with_options(loading.into())?
            .with_checkpoint(&checkpoint)
            .with_observer(&observe),
        scope,
    );
    let mut unrecognized = 0;
    let mut ignored = 0;
    let mut files = 0;
    let selection_failures = resolver.visit(
        selection,
        &checkpoint,
        |error| (emit.borrow_mut())(Event::Error { error }),
        |selected| {
            let file = &selected.scanned.file;
            if selected.scanned.failures.is_empty() && file.metadata.is_empty() {
                if hide_unrecognized {
                    ignored += 1;
                } else {
                    let error = UnrecognizedModFile {
                        path: file.entry.path().to_owned(),
                    }
                    .into();
                    (emit.borrow_mut())(Event::Error {
                        error: ErrorReport::capture(&error),
                    })?;
                    unrecognized += 1;
                }
                return Ok(());
            }
            tracing::debug!(path = %file.entry.path().display(), loaders = file.metadata.len(), failures = selected.scanned.failures.len(), "Emitting inspected mod file");
            (emit.borrow_mut())(Event::File {
                index: files,
                file: File::from(&selected),
            })?;
            files += 1;
            Ok(())
        },
    )?;
    let failed = resolver.scan.failed_count() + selection_failures + unrecognized;
    (emit.borrow_mut())(Event::Done {
        files,
        failed,
        ignored,
        cache: Some(resolver.scan.cache_summary()),
    })?;
    tracing::debug!(
        failed,
        ignored,
        elapsed_ms = started.elapsed().as_millis(),
        "Mod inspection acquisition completed"
    );
    Ok(())
}

#[derive(Debug, thiserror::Error)]
#[error("no supported loader recognized mod file {path:?}")]
struct UnrecognizedModFile {
    path: PathBuf,
}

#[cfg(test)]
mod tests {
    use std::cell::Cell;

    use mcpm_backend::local::modloader::BUILTIN_LOADERS;
    use serde_json::{Value, json};

    use super::*;
    use crate::commands::supports::mods::{inspect::Validator, selection::SelectionInput};

    #[derive(Debug, thiserror::Error)]
    #[error("inspection consumer unavailable")]
    struct ConsumerFailure;

    fn selection(value: Value) -> Selection {
        serde_json::from_value::<SelectionInput>(value)
            .unwrap()
            .into_selection()
            .unwrap()
    }

    #[test]
    fn consumer_failure_during_resolution_is_fatal_and_preserves_the_source() {
        let root = tempfile::tempdir().unwrap();
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
            root.path().join("good.jar"),
        )
        .unwrap();
        std::fs::write(root.path().join("broken.jar"), b"bad archive").unwrap();

        // Exercise direct reads, full scans, and lazy indexing. Every sink error
        // must escape acquisition without becoming a recoverable selection error.
        for input in [
            json!({"queries":[{"kind":"file","value":"good.jar"}]}),
            json!({"queries":[{"kind":"file","value":"missing.jar"}]}),
            json!({"queries":[{"kind":"modid","value":"omnibus_nested"}]}),
            json!({"all":true}),
        ] {
            let mut events = Vec::new();
            load(
                selection(input.clone()),
                Scope::directory(root.path().into()),
                BUILTIN_LOADERS.clone(),
                false,
                &RuntimeEnvironment::new().with_mods_cache(None),
                2,
                || Ok(()),
                |event| {
                    events.push(event);
                    Ok(())
                },
            )
            .unwrap();

            // Fail once at each possible event boundary, including inside scans.
            for stop in 0..events.len() {
                let mut delivered = 0;
                let mut rejected = false;
                let failure = load(
                    selection(input.clone()),
                    Scope::directory(root.path().into()),
                    BUILTIN_LOADERS.clone(),
                    false,
                    &RuntimeEnvironment::new().with_mods_cache(None),
                    2,
                    || Ok(()),
                    |_| {
                        assert!(
                            !rejected,
                            "acquisition emitted events after the consumer failed"
                        );
                        if delivered == stop {
                            rejected = true;
                            return Err(ConsumerFailure.into());
                        }
                        delivered += 1;
                        Ok(())
                    },
                )
                .unwrap_err();
                assert!(failure.is::<ConsumerFailure>(), "{failure:#}");
            }
        }
    }

    #[test]
    fn cancellation_inside_a_scan_does_not_emit_diagnostic_or_completion() {
        let root = tempfile::tempdir().unwrap();
        std::fs::write(root.path().join("broken.jar"), b"bad archive").unwrap();
        for input in [
            json!({"queries":[{"kind":"file","value":"broken.jar"}]}),
            json!({"all":true}),
            json!({"queries":[{"kind":"name","value":"missing"}]}),
        ] {
            let cancelled = Cell::new(false);
            let failure = load(
                selection(input),
                Scope::directory(root.path().into()),
                BUILTIN_LOADERS.clone(),
                false,
                &RuntimeEnvironment::new().with_mods_cache(None),
                2,
                || {
                    if cancelled.get() {
                        Err(ConsumerFailure.into())
                    } else {
                        Ok(())
                    }
                },
                |event| {
                    assert!(matches!(event, Event::Scan { completed: 0, .. }));
                    cancelled.set(true);
                    Ok(())
                },
            )
            .unwrap_err();
            assert!(failure.is::<ConsumerFailure>(), "{failure:#}");
        }
    }

    #[test]
    fn input_errors_remain_recoverable_and_produce_a_valid_completion() {
        let root = tempfile::tempdir().unwrap();
        std::fs::copy(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar"),
            root.path().join("good.jar"),
        )
        .unwrap();
        let mut validator = Validator::default();
        let mut files = 0;
        load(
            selection(json!({"queries":[
                {"kind":"modid","value":"missing-id"},
                {"kind":"file","value":"good.jar"}
            ]})),
            Scope::directory(root.path().into()),
            BUILTIN_LOADERS.clone(),
            false,
            &RuntimeEnvironment::new().with_mods_cache(None),
            2,
            || Ok(()),
            |event| {
                validator.accept(&event)?;
                match event {
                    Event::File { .. } => files += 1,
                    Event::Done { failed, .. } => assert_eq!(failed, 1),
                    _ => {}
                }
                Ok(())
            },
        )
        .unwrap();
        validator.finish().unwrap();
        assert_eq!(files, 1);
    }
}
