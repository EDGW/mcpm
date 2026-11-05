//! Executing side: the operation table and the runs of operations requested by the peer.

// Imports

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::pin::Pin;
use std::sync::Arc;

use anyhow::{Context, anyhow};
use serde_json::Value;
use thiserror::Error;
use tracing::debug;

use super::protocol::{Reply, Request};
use super::{Operation, OperationOutput, ProtocolError};
use crate::messaging::{Channel, Session, SessionError};

/// Runs every operation the peer requests on `session` until the session ends.
///
/// Each operation runs in its own task. Returns how the session ended.
pub async fn serve(
    session: &mut Session,
    registry: Arc<Registry>,
) -> Result<(), Arc<SessionError>> {
    serve_or_forward(session, registry, |channel| {
        tokio::spawn(async move {
            let mut channel = channel;
            let Some(body) = channel.recv().await.ok().flatten() else {
                return;
            };
            let error = match Request::decode(body) {
                Ok(request) => anyhow!("unknown operation `{}`", request.op),
                Err(error) => error.into(),
            };
            fail(channel, &error);
        });
    })
    .await
}

/// Runs registered operations locally and hands unknown operation channels to `forward`.
/// The callback receives the original request as the channel's first body.
pub async fn serve_or_forward(
    session: &mut Session,
    registry: Arc<Registry>,
    forward: impl Fn(Channel) + Send + Sync + 'static,
) -> Result<(), Arc<SessionError>> {
    let forward = Arc::new(forward);
    while let Some(channel) = session.accept().await? {
        let registry = registry.clone();
        let forward = forward.clone();
        tokio::spawn(async move { execute(channel, registry, forward).await });
    }
    Ok(())
}

/// Decodes the request of a new channel and dispatches it.
async fn execute(
    mut channel: Channel,
    registry: Arc<Registry>,
    forward: Arc<impl Fn(Channel) + Send + Sync + 'static>,
) {
    // A channel opened by the peer always starts with a message.
    let body = match channel.recv().await {
        Ok(Some(body)) => body,
        Ok(None) => return,
        Err(error) => {
            debug!(channel = %channel.id(), %error, "operation channel ended before its request");
            return;
        }
    };
    if let Some(op) = body.get("op").and_then(Value::as_str)
        && !registry.handlers.contains_key(op)
    {
        channel.unread(body);
        forward(channel);
        return;
    }
    let request = match Request::decode(body) {
        Ok(request) => request,
        Err(error) => return fail(channel, &error.into()),
    };
    match registry.handlers.get(&*request.op) {
        Some(handler) => handler(request.input, channel).await,
        None => fail(channel, &anyhow!("unknown operation `{}`", request.op)),
    }
}

// region: Registry

/// Operations that [`serve`] runs, by [`Operation::ID`].
#[derive(Default)]
pub struct Registry {
    handlers: HashMap<&'static str, Handler>,
}

impl Registry {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `O`. Fails when another operation already uses its ID.
    pub fn register<O: Operation>(&mut self) -> Result<(), DuplicateOperation> {
        match self.handlers.entry(O::ID) {
            Entry::Occupied(_) => Err(DuplicateOperation(O::ID)),
            Entry::Vacant(entry) => {
                entry.insert(|input, channel| Box::pin(run::<O>(input, channel)));
                Ok(())
            }
        }
    }

    /// Registered IDs in stable order, for daemon capability negotiation.
    pub fn ids(&self) -> Vec<&'static str> {
        let mut ids: Vec<_> = self.handlers.keys().copied().collect();
        ids.sort_unstable();
        ids
    }
}

/// Two operations share one ID.
#[derive(Debug, Error)]
#[error("operation `{0}` is already registered")]
pub struct DuplicateOperation(pub &'static str);

/// Type-erased [`run`] of one operation.
type Handler = fn(Value, Channel) -> Pin<Box<dyn Future<Output = ()> + Send>>;

// endregion

/// Runs `O` until it completes or the caller ends the channel, then delivers the result.
async fn run<O: Operation>(input: Value, mut channel: Channel) {
    let id = channel.id();
    let input = match serde_json::from_value::<O::Input>(input).context("invalid operation input") {
        Ok(input) => input,
        Err(error) => return fail(channel, &error),
    };
    let output = tokio::select! {
        output = super::execute::<O>(input) => output,
        message = channel.recv() => {
            match message {
                Ok(Some(_)) => fail(channel, &ProtocolError::Unexpected.into()),
                Ok(None) | Err(_) => debug!(op = O::ID, channel = %id, "operation channel ended before the result"),
            }
            return;
        }
    };
    match output {
        Ok(output) => match output.deliver(Reply::new(channel)).await {
            Ok(()) => debug!(op = O::ID, channel = %id, "operation finished"),
            Err(error) => {
                debug!(op = O::ID, channel = %id, error = format!("{error:#}"), "operation delivery stopped")
            }
        },
        Err(error) => {
            debug!(op = O::ID, channel = %id, error = format!("{error:#}"), "operation failed");
            fail(channel, &error);
        }
    }
}

/// Reports `error` as the result of the operation on `channel`.
fn fail(channel: Channel, error: &anyhow::Error) {
    if let Err(error) = Reply::new(channel).fail(error) {
        debug!(%error, "failed to report an operation error");
    }
}
