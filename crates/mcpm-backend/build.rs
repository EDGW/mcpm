//! Emit compile-time SHA-256 pins for release artifacts declared in `scripts/artifacts.toml`.
//!
//! Does not inspect Cargo's internal `OUT_DIR` layout or perform network I/O; artifact staging
//! beside binaries is handled explicitly by `scripts/package.py` and `scripts/prepare_artifacts.py`.

use std::{io, path::Path, process::Command};

fn main() -> io::Result<()> {
    const SCRIPT: &str = "../../scripts/prepare_artifacts.py";
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={SCRIPT}");
    println!("cargo:rerun-if-changed=../../scripts/artifacts.toml");

    let python = if cfg!(windows) { "python" } else { "python3" };
    let status = Command::new(python)
        .args([Path::new(SCRIPT).as_os_str(), "--cargo-env".as_ref()])
        .status()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot run {python} to read release artifact pins (requires Python 3.11+): {error}"
                ),
            )
        })?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "reading release artifact pins failed: {status}"
        )));
    }
    Ok(())
}
