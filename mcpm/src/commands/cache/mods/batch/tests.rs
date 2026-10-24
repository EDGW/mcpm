//! Ordered cache-batch coordination with continuing and stopping failure policies.

use std::{
    sync::{
        Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

use anyhow::Result;
use kako_mods_lib::local::cache::hash::Blake3Hash;

use super::{FailurePolicy, run};

#[test]
fn parallel_batches_bound_work_and_deliver_results_in_input_order() -> Result<()> {
    let ids: Vec<_> = (0..8).map(|value| Blake3Hash::new([value; 32])).collect();
    let active = AtomicUsize::new(0);
    let maximum = AtomicUsize::new(0);
    let (began, started) = mpsc::channel();
    let started = Mutex::new(started);
    let (release, released) = mpsc::channel();
    let released = Mutex::new(released);
    let mut order = Vec::new();
    run(
        &ids,
        2,
        FailurePolicy::Stop,
        |id| {
            let count = active.fetch_add(1, Ordering::SeqCst) + 1;
            maximum.fetch_max(count, Ordering::SeqCst);
            if id == ids[0] {
                began.send(())?;
                released
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))?;
            }
            if id == ids[2] {
                started
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))?;
                release.send(())?;
            }
            active.fetch_sub(1, Ordering::SeqCst);
            Ok(id)
        },
        |id, result| {
            assert_eq!(result?, id);
            order.push(id);
            Ok(())
        },
    )?;
    assert_eq!(order, ids);
    assert_eq!(maximum.load(Ordering::SeqCst), 2);
    Ok(())
}

#[test]
fn failed_entry_stops_new_work_and_waits_for_inflight_operations() {
    let ids: Vec<_> = (0..100).map(|value| Blake3Hash::new([value; 32])).collect();
    let (began, started) = mpsc::channel();
    let started = Mutex::new(started);
    let (release, released) = mpsc::channel();
    let released = Mutex::new(released);
    let processed = AtomicUsize::new(0);
    let finished = AtomicUsize::new(0);
    let mut delivered = Vec::new();
    let error = run(
        &ids,
        2,
        FailurePolicy::Stop,
        |id| -> Result<()> {
            processed.fetch_add(1, Ordering::SeqCst);
            if id == ids[0] {
                started
                    .lock()
                    .unwrap()
                    .recv_timeout(Duration::from_secs(3))?;
                anyhow::bail!("refresh failed");
            }
            began.send(())?;
            released
                .lock()
                .unwrap()
                .recv_timeout(Duration::from_secs(3))?;
            finished.fetch_add(1, Ordering::SeqCst);
            Ok(())
        },
        |id, result| {
            delivered.push(id);
            release.send(())?;
            result
        },
    )
    .unwrap_err();
    assert_eq!(error.to_string(), "refresh failed");
    assert_eq!(delivered, [ids[0]]);
    assert_eq!(processed.load(Ordering::SeqCst), 2);
    assert_eq!(finished.load(Ordering::SeqCst), 1);
}

#[test]
fn continuing_batches_deliver_every_worker_failure_in_input_order() -> Result<()> {
    let ids: Vec<_> = (0..32).map(|value| Blake3Hash::new([value; 32])).collect();
    for parallelism in [1, 2] {
        let processed = AtomicUsize::new(0);
        let mut delivered = Vec::new();
        let mut failures = Vec::new();
        run(
            &ids,
            parallelism,
            FailurePolicy::Continue,
            |id| {
                processed.fetch_add(1, Ordering::SeqCst);
                if id == ids[0] || id == ids[16] {
                    anyhow::bail!("verification failed for {id}");
                }
                Ok(())
            },
            |id, result| {
                delivered.push(id);
                if result.is_err() {
                    failures.push(id);
                }
                Ok(())
            },
        )?;
        assert_eq!(processed.load(Ordering::SeqCst), ids.len());
        assert_eq!(delivered, ids);
        assert_eq!(failures, [ids[0], ids[16]]);
    }
    Ok(())
}
