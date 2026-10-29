//! Offline check: `cargo run --example check_client -- ROOT VERSION OS ARCH OS_VERSION BITS`.

use anyhow::{Context, Result};
use mcpm_backend::local::mc::client::{
    CheckOptions, CheckPlatform, CheckRules, MinecraftDirectory,
};

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let root = args.next().context("expected Minecraft root")?;
    let name = args.next().context("expected version directory name")?;
    let rules = CheckRules::new(CheckPlatform {
        os: args
            .next()
            .context("expected launcher OS name: windows, linux or osx")?,
        arch: args
            .next()
            .context("expected architecture, e.g. aarch64 or amd64")?,
        version: args
            .next()
            .context("expected OS version for library rules")?,
        bits: args
            .next()
            .context("expected architecture bitness: 32 or 64")?
            .parse()?,
    });
    let directory = MinecraftDirectory::new(root)?;
    let version = directory.version(name)?;
    let report = directory.check(&version, CheckOptions::default(), &rules);
    println!(
        "{}: intact={}, files={}, unverified={}, issues={}",
        report.version,
        report.is_intact(),
        report.files.len(),
        report.unverified_count(),
        report.issues.len()
    );
    for issue in &report.issues {
        eprintln!("{:?} {:?}: {}", issue.target, issue.path, issue.problem);
        let mut source = std::error::Error::source(&issue.problem);
        while let Some(error) = source {
            eprintln!("  caused by: {error}");
            source = error.source();
        }
    }
    Ok(())
}
