//! Capture Git contributors at build time for the CLI's detailed version output.

use std::{env, fs, path::Path, path::PathBuf, process::Command};

use anyhow::{Context, Result, ensure};

fn main() -> Result<()> {
    println!("cargo:rerun-if-changed=build.rs");
    let source_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").context("missing manifest directory")?);
    let contributors = match contributors(&source_dir) {
        Ok(contributors) => contributors,
        Err(error) => {
            println!(
                "cargo:warning=Contributors unavailable: {}",
                format!("{error:#}").replace(['\r', '\n'], " ")
            );
            "Unavailable (Git history could not be read)\n".to_owned()
        }
    };
    let output_dir =
        PathBuf::from(env::var_os("OUT_DIR").context("missing build output directory")?);
    fs::write(output_dir.join("contributors.txt"), contributors)
        .context("failed to save build-time contributors")
}

fn contributors(source_dir: &Path) -> Result<String> {
    let paths = git(
        source_dir,
        &[
            "rev-parse",
            "--path-format=absolute",
            "--show-toplevel",
            "--git-path",
            "HEAD",
            "--git-path",
            "refs",
            "--git-path",
            "packed-refs",
            "--git-path",
            "shallow",
        ],
    )?;
    for (index, path) in paths.lines().enumerate() {
        let path = if index == 0 {
            Path::new(path).join(".mailmap")
        } else {
            PathBuf::from(path)
        };
        if path.exists() {
            println!("cargo:rerun-if-changed={}", path.display());
        }
    }
    git(source_dir, &["shortlog", "-sne", "--no-merges", "HEAD"])
}

fn git(source_dir: &Path, args: &[&str]) -> Result<String> {
    let command = format!("git {}", args.join(" "));
    let output = Command::new("git")
        .current_dir(source_dir)
        .args(args)
        .output()
        .with_context(|| format!("failed to run {command}"))?;
    ensure!(
        output.status.success(),
        "{command} exited with {}: {}",
        output.status,
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8(output.stdout).with_context(|| format!("{command} returned invalid UTF-8"))
}
