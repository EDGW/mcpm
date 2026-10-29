//! `mcpm-exec`: the operation contract, placement dispatch, transports, protocol and serve loop.
//!
//! The crate owns dispatch, transport and cancellation failures only. Operation errors cross
//! this boundary unchanged, and no concrete operation, environment or business type is known
//! here.

pub mod error;
pub mod executor;
pub mod operation;
pub mod protocol;
pub mod registry;
pub mod serve;
pub mod ssh;
pub mod transport;

pub use error::{CallFailure, Cancelled, DispatchError, Failure, TransportError};
pub use executor::{Executor, Handle, Provide, Session, Settings, Target};
pub use operation::{Context, Emitter, Operation, Placement, Wire};
pub use protocol::LogLevel;
pub use registry::{Registry, RegistryError};

/// Bounded queue depth shared by the in-process event pipe and the remote call routes.
///
/// One call buffers at most this many events before its producer waits, so a slow caller
/// applies backpressure instead of growing memory.
pub(crate) const EVENT_CAPACITY: usize = 64;
