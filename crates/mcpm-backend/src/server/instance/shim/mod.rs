//! Server shim entry and supervisor.

mod input;
mod output;
mod process;
pub mod supervisor;
pub use supervisor::{StartSpec, run_with_handshake};

pub fn run(spec: StartSpec) -> anyhow::Result<()> {
    supervisor::run(spec)
}
