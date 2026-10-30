//! Detached server-instance shim entrypoint; supervision remains in backend.

use std::io::{BufRead, Write};

use anyhow::{Context, Result};
use mcpm_backend::server::instance::shim::{self, StartSpec};

/// Reads one startup specification and runs the backend supervisor.
pub fn run() -> Result<()> {
    let stdin = std::io::stdin();
    let mut line = String::new();
    stdin
        .lock()
        .read_line(&mut line)
        .context("failed to read shim startup specification")?;
    let spec: StartSpec =
        serde_json::from_str(line.trim()).context("invalid shim startup specification")?;
    let mut stdout = std::io::stdout();
    shim::run_with_handshake(spec, |state| {
        let frame = serde_json::json!({
            "type": "started",
            "instance_id": state.instance_id,
            "status": state.status,
        });
        serde_json::to_writer(&mut stdout, &frame).context("failed to encode shim startup")?;
        stdout
            .write_all(b"\n")
            .context("failed to report shim startup")?;
        stdout.flush().context("failed to flush shim startup")
    })
}
