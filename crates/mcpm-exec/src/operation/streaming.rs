//! Streaming output: a result whose items follow on the operation channel.
//!
//! The first reply is an empty handshake. Each later message is one item or the error that ends
//! the stream; finishing the channel ends it cleanly. This module is the only place that encodes
//! or decodes those messages.

// Imports

use std::pin::Pin;

use anyhow::{Context, Result};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use thiserror::Error;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tracing::debug;

use super::{OperationOutput, ProtocolError, Reply};
use crate::error::{ErrorReport, RemoteError};
use crate::messaging::{Channel, ChannelError};

/// Items buffered between a producer and the stream reader.
const BUFFER: usize = 64;

/// A stream of `T` returned as an operation output.
///
/// - Executing side: built with [`StreamingOutput::new`]; the producer starts when the stream is
///   first read and pauses while [`BUFFER`] items wait. Delivery to the peer moves items into the
///   unbounded session send queue, so the peer's reading speed does not pause the producer.
/// - Calling side: [`call`](super::call) returns it with the operation channel; items arrive as
///   the peer produces them.
///
/// Dropping the stream stops the producer, or aborts the channel so the peer stops it.
pub struct StreamingOutput<T> {
    source: Source<T>,
}

impl<T: Send + 'static> StreamingOutput<T> {
    /// A stream whose items `producer` sends. An error returned by `producer` ends the stream
    /// after the items already sent.
    pub fn new<F, Fut>(producer: F) -> Self
    where
        F: FnOnce(StreamSender<T>) -> Fut + Send + 'static,
        Fut: Future<Output = Result<()>> + Send + 'static,
    {
        Self {
            source: Source::Pending(Box::new(move |sender| Box::pin(producer(sender)))),
        }
    }

    /// Next item; `None` once the stream ended. An error ends the stream.
    pub async fn next(&mut self) -> Option<Result<T>>
    where
        T: DeserializeOwned,
    {
        loop {
            let item = match &mut self.source {
                Source::Pending(_) => {
                    self.start();
                    continue;
                }
                Source::Local { items, producer } => match items.recv().await {
                    Some(item) => Some(item),
                    // Every sender is gone: the producer returned or panicked.
                    None => producer.panicked().await.map(Err),
                },
                Source::Remote(channel) => next_remote(channel).await,
                Source::Ended => None,
            };
            if !matches!(item, Some(Ok(_))) {
                self.source = Source::Ended;
            }
            return item;
        }
    }

    fn start(&mut self) {
        let Source::Pending(producer) = std::mem::replace(&mut self.source, Source::Ended) else {
            return;
        };
        let (sender, items) = mpsc::channel(BUFFER);
        let task = tokio::spawn({
            let sender = sender.clone();
            async move {
                if let Err(error) = producer(StreamSender {
                    items: sender.clone(),
                })
                .await
                {
                    // Fails only when the consumer is gone, which needs no report.
                    let _ = sender.send(Err(error)).await;
                }
            }
        });
        self.source = Source::Local {
            items,
            producer: Producer(task),
        };
    }
}

impl<T: Serialize + DeserializeOwned + Send + 'static> OperationOutput for StreamingOutput<T> {
    async fn deliver(mut self, reply: Reply) -> Result<()> {
        let mut channel = reply.send(&Handshake {})?;
        loop {
            let item = tokio::select! {
                item = self.next() => item,
                message = channel.recv() => {
                    return match message {
                        Ok(Some(_)) => Err(ProtocolError::Unexpected.into()),
                        Ok(None) | Err(ChannelError::Aborted(_)) => {
                            debug!(channel = %channel.id(), "stream consumer is gone");
                            Ok(())
                        }
                        Err(error) => Err(error.into()),
                    };
                }
            };
            let error = match item.map(|item| item.and_then(|item| encode_item(&item))) {
                Some(Ok(message)) => {
                    channel.send(message)?;
                    continue;
                }
                Some(Err(error)) => error,
                None => {
                    channel.finish(None)?;
                    return Ok(());
                }
            };
            channel.finish(Some(Message::Error(ErrorReport::capture(&error)).encode()))?;
            return Err(error);
        }
    }

    fn receive(value: Value, channel: Channel) -> Result<Self> {
        serde_json::from_value::<Handshake>(value)
            .map_err(|error| ProtocolError::Invalid("stream handshake", error))?;
        Ok(Self {
            source: Source::Remote(channel),
        })
    }
}

enum Source<T> {
    /// Producer not started yet.
    Pending(PendingProducer<T>),
    Local {
        items: mpsc::Receiver<Result<T>>,
        producer: Producer,
    },
    Remote(Channel),
    Ended,
}

/// Type-erased producer passed to [`StreamingOutput::new`].
type PendingProducer<T> =
    Box<dyn FnOnce(StreamSender<T>) -> Pin<Box<dyn Future<Output = Result<()>> + Send>> + Send>;

/// Producer task of a local stream, aborted with the stream.
struct Producer(JoinHandle<()>);

impl Producer {
    /// After the item queue disconnected: the error to report when the producer panicked.
    async fn panicked(&mut self) -> Option<anyhow::Error> {
        match (&mut self.0).await {
            Err(error) if error.is_panic() => {
                Some(anyhow::Error::new(error).context("stream producer panicked"))
            }
            _ => None,
        }
    }
}

impl Drop for Producer {
    fn drop(&mut self) {
        self.0.abort();
    }
}

// region: StreamSender

/// Producer end of a [`StreamingOutput`]. The stream ends when every sender is dropped.
pub struct StreamSender<T> {
    items: mpsc::Sender<Result<T>>,
}

impl<T> StreamSender<T> {
    /// Sends one item, waiting while the buffer is full. Fails once the consumer is gone.
    pub async fn send(&self, item: T) -> Result<(), StreamClosed> {
        self.items.send(Ok(item)).await.map_err(|_| StreamClosed)
    }
}

impl<T> Clone for StreamSender<T> {
    fn clone(&self) -> Self {
        Self {
            items: self.items.clone(),
        }
    }
}

/// The consumer of a stream is gone.
#[derive(Debug, Error)]
#[error("stream consumer is gone")]
pub struct StreamClosed;

// endregion

/// Reads one item of a remote stream.
async fn next_remote<T: DeserializeOwned>(channel: &mut Channel) -> Option<Result<T>> {
    let body = match channel.recv().await {
        Ok(Some(body)) => body,
        Ok(None) => return None,
        Err(error) => return Some(Err(error.into())),
    };
    Some(match Message::decode(body) {
        Ok(Message::Item(value)) => serde_json::from_value(value)
            .map_err(|error| ProtocolError::Invalid("stream item", error).into()),
        Ok(Message::Error(error)) => {
            Err(RemoteError::wrap(error.into_error()).context("remote stream failed"))
        }
        Err(error) => Err(error.into()),
    })
}

fn encode_item<T: Serialize>(item: &T) -> Result<Value> {
    let value = serde_json::to_value(item).context("failed to encode stream item")?;
    Ok(Message::Item(value).encode())
}

/// First reply of a streaming output.
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Handshake {}

/// Message after the handshake.
#[derive(Serialize, Deserialize)]
#[serde(rename_all = "lowercase", deny_unknown_fields)]
enum Message {
    Item(Value),
    Error(ErrorReport),
}

impl Message {
    fn encode(&self) -> Value {
        // Both variants hold JSON-compatible data with string keys.
        serde_json::to_value(self).expect("stream message is always valid JSON")
    }

    fn decode(body: Value) -> Result<Self, ProtocolError> {
        serde_json::from_value(body)
            .map_err(|error| ProtocolError::Invalid("stream message", error))
    }
}
