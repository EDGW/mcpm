//! Process and local-IPC primitives shared by link holders and future shims.
//!
//! This module owns lifecycle mechanics only. Payloads and state machines stay with their
//! callers, so the executor remains independent of backend operations.

pub mod lock;
pub mod socket;
pub mod spawn;

pub use lock::{ExclusiveLock, LockError};
pub use socket::{LocalStream, SocketAddress, SocketError};
#[cfg(windows)]
pub use socket::{NamedPipeListener, NamedPipeStream};
pub use spawn::{Detached, SpawnError, StartupPipe};
