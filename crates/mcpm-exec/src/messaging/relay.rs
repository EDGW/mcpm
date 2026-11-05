//! Bidirectional channel forwarding without interpreting operation bodies.

use super::{Channel, ChannelError};

/// Relays ordered bodies and termination in either direction.
/// Dropping either unfinished channel aborts its peer on any transport error.
pub async fn relay(mut a: Channel, mut b: Channel) -> Result<(), ChannelError> {
    loop {
        tokio::select! {
            message = a.recv() => match message? {
                Some(body) => b.send(body)?,
                None => return b.finish(None),
            },
            message = b.recv() => match message? {
                Some(body) => a.send(body)?,
                None => return a.finish(None),
            },
        }
    }
}
