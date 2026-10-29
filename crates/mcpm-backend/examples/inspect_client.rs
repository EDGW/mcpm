//! Inspect a Minecraft root: `cargo run --example inspect_client -- /path/to/.minecraft`.

use anyhow::{Context, Result};
use mcpm_backend::local::{
    mc::client::{MinecraftDirectory, MinecraftVersion},
    modloader::BUILTIN_LOADERS,
};

fn main() -> Result<()> {
    let path = std::env::args_os()
        .nth(1)
        .context("expected a Minecraft root directory")?;
    let directory = MinecraftDirectory::new(path)?;
    for version in directory.versions()? {
        match &version {
            MinecraftVersion::Normal(normal) => match normal.mod_loaders(&BUILTIN_LOADERS) {
                Ok(loaders) => {
                    let loaders: Vec<_> = loaders
                        .iter()
                        .map(|loader| format!("{} {}", loader.loader.name(), loader.version))
                        .collect();
                    println!(
                        "{}: modded={}, loaders=[{}]",
                        version.name(),
                        normal.is_modded(),
                        loaders.join(", ")
                    );
                }
                Err(error) => eprintln!("{}: loader inspection failed: {error:#}", version.name()),
            },
            MinecraftVersion::Broken(broken) => {
                let error: &dyn std::error::Error = broken.error();
                eprint!("{}: Broken: {error}", version.name());
                let mut source = error.source();
                while let Some(error) = source {
                    eprint!(": {error}");
                    source = error.source();
                }
                eprintln!();
            }
        }
    }
    Ok(())
}
