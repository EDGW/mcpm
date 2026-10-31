//! Channel handle: one logical conversation inside a session.

// Imports

use std::sync::Arc;

use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender};
use uuid::Uuid;

use super::ChannelError;
use super::frame::Frame;
use super::router::Router;

/// A channel of a [`Session`](super::Session), identified by its GUID.
///
/// Messages on one channel arrive in order; messages of different channels interleave freely.
/// Dropping an unfinished channel finishes it without a final message.
pub struct Channel {
    id: Uuid,
    router: Arc<Router>,
    outbound: UnboundedSender<Frame>,
    inbound: UnboundedReceiver<Value>,
}

impl Channel {
    pub(crate) fn new(
        id: Uuid,
        router: Arc<Router>,
        outbound: UnboundedSender<Frame>,
        inbound: UnboundedReceiver<Value>,
    ) -> Self {
        Self { id, router, outbound, inbound }
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
    /// Fails with [`ChannelError::Finished`] when the peer finished it first; messages already
    /// received are lost with the handle in that case.
    pub fn finish(mut self, body: Option<Value>) -> Result<(), ChannelError> {
        self.finish_with(body)
    }

    /// Receives the next message from the peer.
    ///
    /// Returns `Ok(None)` after the channel is finished and every received message was consumed,
    /// and [`ChannelError::SessionClosed`] when the session ended first.
    pub async fn recv(&mut self) -> Result<Option<Value>, ChannelError> {
        match self.inbound.recv().await {
            Some(body) => Ok(Some(body)),
            None => self.router.ended(self.id).map(|()| None),
        }
    }

    fn finish_with(&mut self, body: Option<Value>) -> Result<(), ChannelError> {
        // A channel the peer never saw is finished silently, unless it carries a message.
        let announced = self.router.finish(self.id)?;
        if announced || body.is_some() {
            self.push(Frame::finish(self.id, body))?;
        }
        Ok(())
    }

    fn push(&self, frame: Frame) -> Result<(), ChannelError> {
        self.outbound.send(frame).map_err(|_| self.router.session_closed())
    }
}

impl Drop for Channel {
    fn drop(&mut self) {
        // Already finished by either side, or the session ended: nothing to tell the peer.
        let _ = self.finish_with(None);
    }
}
