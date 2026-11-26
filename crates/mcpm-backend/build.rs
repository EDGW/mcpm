//! Stage pinned release artifacts in the Cargo profile directory shared by mcpm and mcpmd.

use std::{env, io, path::Path, process::Command};

fn main() -> io::Result<()> {
    const SCRIPT: &str = "../../scripts/prepare_artifacts.py";
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={SCRIPT}");
    println!("cargo:rerun-if-changed=../../scripts/artifacts.toml");

    let out_dir = env::var_os("OUT_DIR").expect("Cargo must set OUT_DIR");
    // Cargo's package output is <profile>/build/<package-hash>/out, also with --target.
    let build_dir = Path::new(&out_dir)
        .ancestors()
        .nth(2)
        .filter(|path| path.ends_with("build"))
        .expect("expected OUT_DIR below the Cargo profile's build directory");
    let profile_dir = build_dir.parent().expect("missing Cargo profile directory");
    let python = if cfg!(windows) { "python" } else { "python3" };
    let status = Command::new(python)
        .args([Path::new(SCRIPT), profile_dir])
        .status()
        .map_err(|error| {
            io::Error::new(
                error.kind(),
                format!(
                    "cannot run {python} to prepare release artifacts (requires Python 3.11+): {error}"
                ),
            )
        })?;
    if !status.success() {
        return Err(io::Error::other(format!(
            "release artifact preparation failed: {status}"
        )));
    }
    Ok(())
}
