//! Concurrent ping-pong over one session pair.
//!
//! Each rally runs on its own channel: A sends 0, B answers with the number plus one, and so on
//! until 50. Both sides check that every received number is the previous one plus one. Rallies
//! run in parallel on a multi-threaded runtime, so their frames interleave on the carrier.
//!
//! Logs go to the console with `cargo test -p mcpm-exec --test messaging -- --nocapture`.

// Imports

use std::time::Duration;

use mcpm_exec::messaging::{Channel, Session};
use serde_json::json;
use tokio::task::JoinSet;
use tracing::{debug, info};

const RALLIES: usize = 30;
const LAST: u64 = 50;

#[tokio::test(flavor = "multi_thread", worker_threads = 30)]
async fn concurrent_rallies_count_to_last() {
    let _ = tracing_subscriber::fmt()
        .with_max_level(tracing::Level::DEBUG)
        .with_test_writer()
        .with_thread_ids(true)
        .try_init();

    tokio::time::timeout(Duration::from_secs(30), run())
        .await
        .expect("rallies timed out");
}

async fn run() {
    let (a, b) = tokio::io::duplex(1024);
    let client = Session::new(a);
    let mut server = Session::new(b);

    let responder = tokio::spawn(async move {
        let mut rallies = JoinSet::new();
        while let Some(channel) = server.accept().await.expect("session failed") {
            rallies.spawn(respond(channel));
        }
        let mut count = 0;
        while let Some(rally) = rallies.join_next().await {
            rally.expect("responder panicked");
            count += 1;
        }
        count
    });

    let mut rallies = JoinSet::new();
    for rally in 0..RALLIES {
        rallies.spawn(initiate(rally, client.open()));
    }
    while let Some(rally) = rallies.join_next().await {
        rally.expect("initiator panicked");
    }

    // Closing side A ends the session for B once every channel is finished.
    drop(client);
    assert_eq!(responder.await.expect("responder panicked"), RALLIES);
}

/// Side A: sends even numbers, finishes the channel with `LAST`.
async fn initiate(rally: usize, mut channel: Channel) {
    let id = channel.id();
    info!(rally, %id, "rally started");
    let mut sent = 0;
    channel.send(json!(sent)).expect("send failed");
    loop {
        let received = next_number(&mut channel).await;
        assert_eq!(
            received,
            sent + 1,
            "rally {rally}: A received an unexpected number"
        );
        debug!(rally, received, "A received");
        sent = received + 1;
        if sent == LAST {
            channel.finish(Some(json!(sent))).expect("finish failed");
            break;
        }
        channel.send(json!(sent)).expect("send failed");
    }
    info!(rally, %id, "rally finished on A");
}

/// Side B: answers every number plus one until `LAST`, then expects the end of the channel.
async fn respond(mut channel: Channel) {
    let id = channel.id();
    let mut expected = 0;
    loop {
        let received = next_number(&mut channel).await;
        assert_eq!(
            received, expected,
            "channel {id}: B received an unexpected number"
        );
        debug!(%id, received, "B received");
        if received == LAST {
            break;
        }
        channel.send(json!(received + 1)).expect("send failed");
        expected = received + 2;
    }
    assert_eq!(
        channel.recv().await.expect("session failed"),
        None,
        "channel {id}: not finished"
    );
    info!(%id, "rally finished on B");
}

async fn next_number(channel: &mut Channel) -> u64 {
    let body = channel
        .recv()
        .await
        .expect("session failed")
        .expect("channel finished early");
    body.as_u64().expect("body is not a number")
}
