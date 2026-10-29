//! Shared remote source requests and presentation for command entry points.

mod args;
pub(super) use args::SourcesArgs;
pub(super) mod files;
pub(super) mod identify;
mod ops;
mod progress;
mod request;
pub(super) mod search;
