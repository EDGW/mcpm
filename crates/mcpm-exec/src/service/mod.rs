//! Typed operation execution on the local host or through a reusable daemon link.

pub use crate::operation::{Connection, Target};
use crate::{
    messaging::{Session, SessionTrafficStats},
    operation::{self, Operation},
};
use anyhow::Result;
use std::sync::Arc;

pub mod control;
mod link;

/// One flow's execution session; all remote calls share one IPC connection.
#[derive(Clone)]
pub struct Executor {
    inner: Inner,
}

#[derive(Clone)]
enum Inner {
    Local,
    Remote(Arc<Session>),
}

impl Executor {
    /// Whether Host operations in this flow execute on a remote daemon.
    pub fn is_remote(&self) -> bool {
        matches!(self.inner, Inner::Remote(_))
    }

    /// Returns this flow's local Session traffic snapshot without performing an operation.
    /// Local execution has no messaging Session and returns `None`.
    pub fn traffic_stats(&self) -> Option<SessionTrafficStats> {
        match &self.inner {
            Inner::Local => None,
            Inner::Remote(session) => Some(session.traffic_stats()),
        }
    }

    pub async fn open(target: Target) -> Result<Self> {
        Self::open_with_idle_timeout(target, 600).await
    }

    /// Applies an idle timeout only when a new link must be started.
    pub async fn open_with_idle_timeout(target: Target, idle_timeout: u64) -> Result<Self> {
        let inner = match target {
            Target::Local => Inner::Local,
            Target::Remote(connection) => {
                Inner::Remote(Arc::new(link::open(&connection, idle_timeout).await?))
            }
        };
        Ok(Self { inner })
    }

    /// Local and shallow operations never serialize either their inputs or outputs.
    pub async fn call<O: Operation>(&self, input: O::Input) -> Result<O::Output> {
        if O::SHALLOW {
            return operation::execute::<O>(input).await;
        }
        match &self.inner {
            Inner::Local => operation::execute::<O>(input).await,
            Inner::Remote(session) => {
                tracing::info!(
                    op = O::ID,
                    shallow = O::SHALLOW,
                    "operation dispatched to remote host"
                );
                operation::call::<O>(session, &input).await
            }
        }
    }

    /// Subscribes to remote diagnostics without opening another client connection.
    pub async fn subscribe_logs(
        &self,
        level: String,
    ) -> Result<Option<operation::StreamingOutput<control::LogRecord>>> {
        match &self.inner {
            Inner::Local => Ok(None),
            Inner::Remote(session) => Ok(Some(
                operation::call::<control::Logs>(session, &control::LogInput { level }).await?,
            )),
        }
    }
}
