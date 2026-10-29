//! Query or download a fixed release: `cargo run --example download_authlib_injector -- latest|BUILD [DESTINATION|-] [CONFIG]`.

use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use mcpm_backend::{
    download::{DownloadConfig, authlib_injector, sources},
    local::codec::Codec,
};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let selector = args
        .next()
        .context("expected latest or a build number")?
        .into_string()
        .map_err(|_| anyhow::anyhow!("build selector must be UTF-8"))?;
    let destination = args.next().filter(|path| path != "-").map(PathBuf::from);
    let config = match args.next() {
        Some(path) => DownloadConfig::open(Path::new(&path))?,
        None => DownloadConfig::default(),
    };
    let downloader = sources::downloader(&config)?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    runtime.block_on(async {
        let release = if selector == "latest" {
            authlib_injector::latest(&downloader).await?
        } else {
            authlib_injector::build(
                &downloader,
                selector.parse().context("invalid build number")?,
            )
            .await?
        };
        println!("Version: {}", release.version());
        println!("Build: {}", release.build_number());
        println!("SHA-256: {}", release.sha256());
        if let Some(destination) = destination {
            let outcome = release.download_to(&downloader, &destination).await?;
            println!(
                "Downloaded {} bytes to {}",
                outcome.size,
                outcome.destination.display()
            );
            println!("From: {}", outcome.url);
        }
        Ok(())
    })
}
