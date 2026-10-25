//! Shared CLI implementation for the server, client and tools executables.

mod app;
mod build_info;
mod cli;
mod commands;
mod diagnostics;
mod display;
mod help;
mod initcall;
mod logging;
mod program;
mod progress;
mod remote;
mod ssh;
mod terminal;
mod theme;
mod widgets;

pub use app::run;
pub use program::Program;
