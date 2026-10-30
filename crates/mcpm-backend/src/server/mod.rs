//! Server operations: recognition, registration, snapshots and mods context.
//!
//! Server configuration codecs stay in [`crate::data::server`]; these operations validate
//! inputs, call the codecs and the library, and define their own failure classifications.
//! Instance and shim operations arrive in a later step.

pub mod context;
pub mod info;
pub mod recognize;
pub mod register;
pub mod types;

pub use context::{ContextInput, ContextKind, ModsContextOperation};
pub use info::{Info, InfoInput, InfoKind};
pub use recognize::{Recognition, RecognizeEvent, RecognizeKind};
pub use register::{Register, RegisterEvent, RegisterKind};
pub use types::{
    Candidate, Initialized, Initialize, LoaderInfo, ModsContext, Phase, Recognize, ServerInfo,
    StartCommand,
};
