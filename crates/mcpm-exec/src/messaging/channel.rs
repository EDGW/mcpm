//! Channel handle: one logical conversation inside a session.

// Imports

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use tracing::debug;
use uuid::Uuid;

use super::ChannelError;
use super::frame::Frame;
use super::router::Router;

/// A channel of a [`Session`](super::Session), identified by its GUID.
///
/// Messages on one channel arrive in order; messages of different channels interleave freely.
/// Dropping an unfinished channel aborts it.
pub struct Channel {
    id: Uuid,
    router: Arc<Router>,
    outbound: UnboundedSender<Frame>,
    inbound: UnboundedReceiver<Value>,
    prefetched: Option<Value>,
}

impl Channel {
    pub(crate) fn new(
        id: Uuid,
        router: Arc<Router>,
        outbound: UnboundedSender<Frame>,
        inbound: UnboundedReceiver<Value>,
    ) -> Self {
        debug!(session = ?Arc::as_ptr(&router), channel = %id, "channel started");
        Self {
            id,
            router,
            outbound,
            inbound,
            prefetched: None,
        }
    }

    /// GUID shared by both ends of this channel.
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Queues `body` for the peer. The first message of a locally opened channel opens it on
    /// the peer.
    pub fn send(&self, body: Value) -> Result<(), ChannelError> {
        self.router.announce(self.id)?;
        self.push(Frame::message(self.id, body))
    }

    /// Finishes the channel for both sides, optionally sending a final message.
    ///
    /// Fails with [`ChannelError::Finished`] or [`ChannelError::Aborted`] when the peer ended it
    /// first; messages already received are lost with the handle in that case.
    pub fn finish(mut self, body: Option<Value>) -> Result<(), ChannelError> {
        self.end(Frame::finish(self.id, body))
    }

    /// Receives the next message from the peer.
    ///
    /// Returns `Ok(None)` after the peer finished the channel and every received message was
    /// consumed. After those messages, fails with [`ChannelError::Aborted`] when the peer aborted
    /// it, or [`ChannelError::SessionClosed`] when the session ended first.
    pub async fn recv(&mut self) -> Result<Option<Value>, ChannelError> {
        if let Some(body) = self.prefetched.take() {
            return Ok(Some(body));
        }
        match self.inbound.recv().await {
            Some(body) => Ok(Some(body)),
            None => self.router.ended(self.id).map(|()| None),
        }
    }

    /// Restores the request inspected by the operation dispatcher before forwarding.
    pub(crate) fn unread(&mut self, body: Value) {
        debug_assert!(self.prefetched.is_none());
        self.prefetched = Some(body);
    }

    fn end(&mut self, frame: Frame) -> Result<(), ChannelError> {
        // A channel the peer never saw ends silently, unless the frame carries a message.
        let announced = self.router.finish(self.id)?;
        if announced || frame.body.is_some() {
            self.push(frame)?;
        }
        Ok(())
    }

    fn push(&self, frame: Frame) -> Result<(), ChannelError> {
        self.outbound
            .send(frame)
            .map_err(|_| self.router.session_closed())
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // Already ended by either side, or the session ended: nothing to tell the peer.
        let _ = self.end(Frame::abort(self.id));
        debug!(session = ?Arc::as_ptr(&self.router), channel = %self.id, "channel ended");
    }
}
