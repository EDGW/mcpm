//! Shared remote source requests and presentation for command entry points.

mod args;
pub(super) use args::SourcesArgs;
mod config;
pub(super) use config::build_source;
pub(super) mod files;
pub(super) mod identify;
mod progress;
mod request;
pub(super) mod search;
