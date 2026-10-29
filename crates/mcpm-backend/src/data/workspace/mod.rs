//! Workspace kind marker and Remote registration.
//!
//! The workspace files describe where a server lives, not the server itself, so they are
//! kept apart from `data/server`; local server metadata stays in `data/server`.

pub mod lock;
pub mod manifest;
pub mod remote;

pub use lock::RegistrationWriter;
pub use manifest::{Kind, WorkspaceError, kind, require_local};
pub use remote::RemoteRegistration;
