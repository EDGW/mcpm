//! Line-delimited JSON messaging over any byte stream pair.
//!
#![doc = include_str!("../docs/messaging.md")]

// Re-exported Modules

mod carrier;
pub use carrier::*;

mod channel;
pub use channel::*;

mod error;
pub use error::*;

mod frame;
pub use frame::FrameError;

mod session;
pub use session::*;

mod relay;
pub use relay::*;

mod stats;
pub use stats::{SessionTrafficStats, TrafficRates};

// Private Modules

mod router;
