//! Remote source operations, provider construction and provider-neutral DTOs.
//!
//! Provider configuration, credentials and HTTP clients exist only in surface
//! environments. Server-side operations return compatibility facts instead, so a provider
//! is never constructed on a remote host.

pub mod files;
pub mod game;
pub mod provider;
pub mod search;
pub mod types;

pub use files::{
    FilesError, FilesInput, FilesOutput, FilesRequest, ProjectFiles, ProjectFilesResult,
};
pub use game::{GameVersions, GameVersionsError, GameVersionsInput};
pub use search::{Search, SearchError, SearchInput};
