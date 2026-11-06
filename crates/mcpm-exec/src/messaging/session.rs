//! Session: the channels multiplexed over one carrier.
//!
//! A session owns two tasks. The reader decodes lines from the carrier and routes them; the
//! writer encodes queued frames, so every line reaches the carrier whole.

// Imports

use std::sync::Arc;

use tokio::io::{AsyncBufReadExt, AsyncRead, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::sync::mpsc::{
    UnboundedReceiver, UnboundedSender, WeakUnboundedSender, unbounded_channel,
};
use tokio::task::AbortHandle;
use tracing::debug;

use super::frame::Frame;
use super::router::Router;
use super::stats::TrafficStats;
use super::{Carrier, Channel, SessionError, SessionTrafficStats};

/// Both ends of one carrier, multiplexing any number of channels.
///
/// The session ends when the peer closes its side of the carrier, sends an invalid frame, or
/// the carrier fails. Dropping the session and every channel closes this side of the carrier
/// once queued frames are written.
pub struct Session {
    router: Arc<Router>,
    outbound: UnboundedSender<Frame>,
    incoming: UnboundedReceiver<Channel>,
    traffic: Arc<TrafficStats>,
}

impl Session {
    /// Starts a session on `carrier`.
    ///
    /// # Panics
    ///
    /// Panics outside a Tokio runtime, which runs the session tasks.
    pub fn new(carrier: impl Carrier) -> Self {
        let (reader, writer) = carrier.split();
        let router = Arc::new(Router::new());
        let traffic = Arc::new(TrafficStats::new());
        debug!(session = ?Arc::as_ptr(&router), "session started");
        let (outbound, frames) = unbounded_channel();
        let (accepted, incoming) = unbounded_channel();

        let reader = tokio::spawn({
            let router = router.clone();
            let outbound = outbound.downgrade();
            let traffic = traffic.clone();
            async move {
                let result = read_frames(reader, &router, &outbound, &accepted, &traffic).await;
                debug!(
                    error = result.as_ref().err().map(tracing::field::display),
                    "session reader ended"
                );
                router.close(result);
            }
        });
        tokio::spawn(write_frames(
            writer,
            frames,
            router.clone(),
            reader.abort_handle(),
            traffic.clone(),
        ));

        Self {
            router,
            outbound,
            incoming,
            traffic,
        }
    }

    /// Opens a channel with a fresh GUID. The peer sees it with the first message sent on it.
    pub fn open(&self) -> Channel {
        let (id, inbound) = self.router.open();
        Channel::new(id, self.router.clone(), self.outbound.clone(), inbound)
    }

    /// Waits for the next channel opened by the peer.
    ///
    /// Returns `Ok(None)` once the peer closed the session cleanly and every opened channel was
    /// accepted, or the error that ended the session.
    pub async fn accept(&mut self) -> Result<Option<Channel>, Arc<SessionError>> {
        if let Some(channel) = self.incoming.recv().await {
            return Ok(Some(channel));
        }
        // The reader records the closure before the accept queue disconnects.
        self.router.closure().transpose().map(|_| None)
    }

    /// Waits for carrier closure without consuming incoming channels.
    pub async fn closed(&self) -> Result<(), Arc<SessionError>> {
        self.router.closed().await
    }

    /// Returns Session-wide send/receive rates in bytes per second.
    ///
    /// Each direction has independent 100 ms and 1 s windows and their lifetime peaks.
    /// Counts encoded frames, including protocol fields and newlines, only after successful
    /// decoding on receive or successful batch write and flush on send. Querying expires old
    /// samples without sending traffic; idle rates fall to zero while lifetime peaks remain.
    pub fn traffic_stats(&self) -> SessionTrafficStats {
        self.traffic.snapshot()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        debug!(session = ?Arc::as_ptr(&self.router), "session ended");
    }
}

async fn read_frames(
    reader: impl AsyncRead + Unpin,
    router: &Arc<Router>,
    outbound: &WeakUnboundedSender<Frame>,
    accepted: &UnboundedSender<Channel>,
    traffic: &TrafficStats,
) -> Result<(), SessionError> {
    let mut reader = BufReader::new(reader);
    let mut line = Vec::new();
    loop {
        line.clear();
        if reader
            .read_until(b'\n', &mut line)
            .await
            .map_err(SessionError::Read)?
            == 0
        {
            return Ok(());
        }
        let frame = Frame::decode(&line).map_err(SessionError::Protocol)?;
        traffic.record_received(line.len());
        let id = frame.id;
        let Some(inbound) = router.route(frame) else {
            continue;
        };
        // An unaccepted channel aborts when dropped. Once every sender is gone the writer has
        // shut down, so the channel only ends locally.
        if let Some(outbound) = outbound.upgrade() {
            let _ = accepted.send(Channel::new(id, router.clone(), outbound, inbound));
        } else {
            let _ = router.finish(id);
        }
    }
}

/// Writes queued frames until every sender is gone, then closes this side of the carrier.
///
/// A carrier failure ends the session and stops the reader.
async fn write_frames(
    mut writer: impl AsyncWrite + Unpin,
    mut frames: UnboundedReceiver<Frame>,
    router: Arc<Router>,
    reader: AbortHandle,
    traffic: Arc<TrafficStats>,
) {
    let mut lines = Vec::new();
    let result = async {
        while let Some(frame) = frames.recv().await {
            // Batch the frames already queued into one write. Frames queued meanwhile wait for
            // the next batch, so a busy sender cannot hold the write back.
            lines.clear();
            frame.encode(&mut lines).map_err(SessionError::Encode)?;
            for _ in 0..frames.len() {
                let Ok(frame) = frames.try_recv() else { break };
                frame.encode(&mut lines).map_err(SessionError::Encode)?;
            }
            writer
                .write_all(&lines)
                .await
                .map_err(SessionError::Write)?;
            writer.flush().await.map_err(SessionError::Write)?;
            traffic.record_sent(lines.len());
        }
        writer.shutdown().await.map_err(SessionError::Write)
    }
    .await;
    if let Err(error) = result {
        debug!(%error, "session writer failed");
        // Close before `frames` drops, so senders observe the cause.
        router.close(Err(error));
        reader.abort();
    }
}
