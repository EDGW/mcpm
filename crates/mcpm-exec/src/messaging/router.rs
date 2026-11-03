//! Channel table of a session.
//!
//! The router is the single source of truth for channel state: open channels and their inbound
//! queues, how each channel ended, and how the session ended. Session tasks and channel handles
//! share one router and never keep a second copy of this state.

// Imports

use std::collections::HashMap;
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};

use serde_json::Value;
use tokio::sync::mpsc::{UnboundedReceiver, UnboundedSender, unbounded_channel};
use tracing::debug;
use uuid::Uuid;

use super::frame::Frame;
use super::{ChannelError, SessionError};

/// Shared channel table. Locks are held only for table updates, never across `.await`.
pub(crate) struct Router {
    table: Mutex<Table>,
}

impl Router {
    pub fn new() -> Self {
        Self {
            table: Mutex::new(Table { open: HashMap::new(), ended: HashMap::new(), closure: None }),
        }
    }

    /// Registers a locally opened channel. The peer learns about it from its first frame.
    ///
    /// After the session ended, the channel is born closed: it is not registered and its queue
    /// is already disconnected.
    pub fn open(&self) -> (Uuid, UnboundedReceiver<Value>) {
        let (inbound, receiver) = unbounded_channel();
        let id = Uuid::new_v4();
        let mut table = self.table();
        if table.closure.is_none() {
            table.open.insert(id, Entry { inbound, announced: false });
        }
        (id, receiver)
    }

    /// Delivers a frame from the peer.
    ///
    /// Returns the inbound queue of a channel the peer just opened. Frames for finished channels
    /// are dropped: they cross a FINISH frame sent by this side.
    pub fn route(&self, frame: Frame) -> Option<UnboundedReceiver<Value>> {
        let Frame { id, flags, body } = frame;
        let mut table = self.table();
        if table.closure.is_some() {
            return None;
        }
        if table.ended.contains_key(&id) {
            debug!(channel = %id, "dropped frame for an ended channel");
            return None;
        }
        let ending = flags.finish.then_some(if flags.abort { Ending::Aborted } else { Ending::Finished });

        let Some(entry) = table.open.get(&id) else {
            let Some(body) = body else {
                debug!(channel = %id, "peer ended a channel it never used");
                table.ended.extend(ending.map(|ending| (id, ending)));
                return None;
            };
            let (inbound, receiver) = unbounded_channel();
            // The receiver is alive, so this send cannot fail.
            let _ = inbound.send(body);
            match ending {
                Some(ending) => {
                    table.ended.insert(id, ending);
                }
                None => {
                    table.open.insert(id, Entry { inbound, announced: true });
                }
            }
            return Some(receiver);
        };

        if let Some(body) = body {
            // A channel handle unregisters itself before dropping its receiver, so the queue
            // of a registered channel is always connected.
            let _ = entry.inbound.send(body);
        }
        if let Some(ending) = ending {
            table.open.remove(&id);
            table.ended.insert(id, ending);
        }
        None
    }

    /// Marks an open channel as known to the peer, before this side sends on it.
    pub fn announce(&self, id: Uuid) -> Result<(), ChannelError> {
        let mut table = self.table();
        match table.open.get_mut(&id) {
            Some(entry) => {
                entry.announced = true;
                Ok(())
            }
            None => Err(table.not_open(id)),
        }
    }

    /// Ends an open channel locally. Returns whether the peer knows the channel, which decides
    /// whether a final frame must be sent.
    pub fn finish(&self, id: Uuid) -> Result<bool, ChannelError> {
        let mut table = self.table();
        match table.open.remove(&id) {
            Some(entry) => {
                table.ended.insert(id, Ending::Finished);
                Ok(entry.announced)
            }
            None => Err(table.not_open(id)),
        }
    }

    /// Explains a disconnected inbound queue: `Ok` when the channel was finished, otherwise the
    /// abort or the session end.
    pub fn ended(&self, id: Uuid) -> Result<(), ChannelError> {
        let table = self.table();
        match table.ended.get(&id) {
            Some(Ending::Finished) => Ok(()),
            Some(Ending::Aborted) => Err(ChannelError::Aborted(id)),
            None => table.session_closed().map_or(Ok(()), Err),
        }
    }

    /// Error for channel operations after the session ended.
    pub fn session_closed(&self) -> ChannelError {
        self.table().session_closed().unwrap_or(ChannelError::SessionClosed(None))
    }

    /// Records how the session ended and disconnects every open channel. The first call wins.
    pub fn close(&self, result: Result<(), SessionError>) {
        let mut table = self.table();
        if table.closure.is_none() {
            table.closure = Some(result.map_err(Arc::new));
            table.open.clear();
        }
    }

    /// How the session ended, if it did.
    pub fn closure(&self) -> Option<Result<(), Arc<SessionError>>> {
        self.table().closure.clone()
    }

    fn table(&self) -> MutexGuard<'_, Table> {
        // Table updates cannot panic halfway, so a poisoned table is still consistent.
        self.table.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

struct Table {
    /// Every open channel. Empty once the session has ended.
    open: HashMap<Uuid, Entry>,
    /// How each ended channel ended, kept for the whole session so late frames are not taken
    /// for new channels.
    ended: HashMap<Uuid, Ending>,
    closure: Option<Result<(), Arc<SessionError>>>,
}

impl Table {
    fn not_open(&self, id: Uuid) -> ChannelError {
        match self.ended.get(&id) {
            Some(Ending::Finished) => ChannelError::Finished(id),
            Some(Ending::Aborted) => ChannelError::Aborted(id),
            None => self.session_closed().unwrap_or(ChannelError::Finished(id)),
        }
    }

    fn session_closed(&self) -> Option<ChannelError> {
        let closure = self.closure.as_ref()?;
        Some(ChannelError::SessionClosed(closure.as_ref().err().cloned()))
    }
}

struct Entry {
    inbound: UnboundedSender<Value>,
    /// Whether the peer has seen a frame of this channel.
    announced: bool,
}

/// How a channel ended. Only the peer can abort: a local abort drops the only handle.
#[derive(Clone, Copy)]
enum Ending {
    Finished,
    Aborted,
}
