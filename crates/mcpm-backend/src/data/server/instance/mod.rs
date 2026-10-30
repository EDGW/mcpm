//! Server-instance runtime data and the sole state/socket protocol codecs.
pub mod paths;
pub mod protocol;
pub mod state;
pub use paths::InstancePaths;
pub use state::{ExitRecord, InstanceState, InstanceStatus};
