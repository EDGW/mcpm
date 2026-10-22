//! Regression coverage for bounded concurrency, ordered collection and cached failures/aliases.

use std::{
    collections::HashMap,
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::Result;
use kako_mods_lib::local::modloader::BUILTIN_LOADERS;

use super::{ScanSession, batch};

#[test]
fn bounded_workers_deliver_later_completion_without_waiting_for_first_input() {
    let (release, blocked) = mpsc::channel();
    let blocked = Mutex::new(blocked);
    let (began, running) = mpsc::channel();
    let running = Mutex::new(running);
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let mut order = Vec::new();
    batch::run(
        8,
        2,
        |index| {
            let concurrency = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(concurrency, Ordering::SeqCst);
            if index == 0 {
                began.send(()).unwrap();
                // Timeout avoids hanging the suite if scheduling accidentally becomes serial.
                blocked
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            }
            if index == 1 {
                running
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))
                    .unwrap();
            }
            active.fetch_sub(1, Ordering::SeqCst);
            index
        },
        || Ok(()),
        |index, result| {
            assert_eq!(index, result);
            order.push(index);
            if index == 1 {
                release.send(()).unwrap();
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    assert_ne!(order[0], 0);
    order.sort_unstable();
    assert_eq!(order, (0..8).collect::<Vec<_>>());

    // Consumer/transport errors stop the batch without blocked channel senders.
    let failure = batch::run(
        100,
        2,
        |index| index,
        || Ok(()),
        |_, _| -> Result<()> { anyhow::bail!("consumer closed") },
    );
    assert!(failure.unwrap_err().to_string().contains("consumer closed"));

    let cancelled = std::cell::Cell::new(false);
    let started = AtomicUsize::new(0);
    let failure = batch::run(
        100,
        2,
        |_| {
            started.fetch_add(1, Ordering::SeqCst);
        },
        || {
            if cancelled.get() {
                anyhow::bail!("cancelled");
            }
            Ok(())
        },
        |_, _| {
            cancelled.set(true);
            Ok(())
        },
    );
    assert!(failure.unwrap_err().to_string().contains("cancelled"));
    assert!(
        started.load(Ordering::SeqCst) <= 6,
        "cancellation must stop dispatch rather than drain all work"
    );
}

#[test]
fn normalized_duplicates_share_success_and_failure_but_disabled_alias_is_separate() {
    let root = tempfile::tempdir().unwrap();
    let fixture =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/complex-mod.jar");
    let good = root.path().join("good.jar");
    std::fs::copy(fixture, &good).unwrap();
    let disabled = root.path().join("good.jar.disabled");
    std::fs::copy(&good, &disabled).unwrap();
    let bad = root.path().join("broken.jar");
    std::fs::write(&bad, b"invalid archive").unwrap();
    let missing = root.path().join("missing.jar");
    let paths = [
        good.clone(),
        root.path().join("./good.jar"),
        disabled,
        bad.clone(),
        bad,
        missing.clone(),
        missing.clone(),
        root.path().join("invalid.txt"),
    ];
    let mut scan = ScanSession::new(BUILTIN_LOADERS.clone()).with_parallelism(3);
    let mut files = HashMap::new();
    let mut fresh = 0;
    scan.batch(
        &paths,
        || Ok(()),
        |index, result, first| {
            fresh += usize::from(first);
            files.insert(index, result.file.clone());
            if index >= 3 {
                assert!(result.failed());
            }
            Ok(())
        },
    )
    .unwrap();
    assert_eq!(fresh, 5);
    assert_eq!(scan.failed_count(), 3);
    assert!(Arc::ptr_eq(
        files[&0].as_ref().unwrap(),
        files[&1].as_ref().unwrap()
    ));
    assert!(!Arc::ptr_eq(
        files[&0].as_ref().unwrap(),
        files[&2].as_ref().unwrap()
    ));
    assert!(Arc::ptr_eq(
        files[&5].as_ref().unwrap(),
        files[&6].as_ref().unwrap()
    ));
    assert!(files[&7].is_none());
    std::fs::copy(&good, &missing).unwrap();
    scan.batch(
        &paths,
        || Ok(()),
        |index, result, fresh| {
            assert!(!fresh);
            if index == 5 {
                assert!(result.failed());
            }
            Ok(())
        },
    )
    .unwrap();
}
