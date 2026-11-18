//! Vanilla server recognition from launch evidence, JAR metadata, and official SHA-1 hashes.

use std::{fs::File, io, path::Path};

use anyhow::{Context, Result};
use sha1::{Digest, Sha1};

use crate::local::mc::{jar, server::ServerLaunch};

include!(concat!(env!("OUT_DIR"), "/legacy_server_sha1.rs"));

pub(super) fn vanilla(launch: &ServerLaunch) -> Result<Option<String>> {
    let Some(path) = launch.jar() else {
        return Ok(None);
    };
    if !is_vanilla_main_class(launch.main_class()) {
        return Ok(None);
    }
    let (version, evidence) = match jar::minecraft_version(&path)? {
        Some(version) => (version, "jar_metadata"),
        None => {
            tracing::debug!(jar = %path.display(), "checking SHA-1 for vanilla server JAR without version metadata");
            let version = version_by_sha1(&path)
                .with_context(|| format!("failed to compute vanilla server JAR SHA-1 for {path:?}"))?
                .context("vanilla JAR lacks verifiable Minecraft version metadata and its SHA-1 is not in the official server table")?;
            (version.to_owned(), "sha1")
        }
    };
    tracing::info!(jar = %path.display(), minecraft_version = %version, evidence, "identified vanilla server");
    Ok(Some(version))
}

/// Positive entry-class evidence, shared with explicit registration upgrades.
pub(crate) fn is_vanilla_main_class(main_class: &str) -> bool {
    matches!(
        main_class,
        "net.minecraft.bundler.Main"
            | "net.minecraft.server.Main"
            | "net.minecraft.server.MinecraftServer"
    )
}

fn version_by_sha1(path: &Path) -> io::Result<Option<&'static str>> {
    let mut file = File::open(path)?;
    let mut hasher = Sha1::new();
    io::copy(&mut file, &mut hasher)?;
    let hash: [u8; 20] = hasher.finalize().into();
    Ok(LEGACY_SERVER_SHA1
        .binary_search_by(|(candidate, _)| candidate.cmp(&hash))
        .ok()
        .map(|index| LEGACY_SERVER_SHA1[index].1))
}
