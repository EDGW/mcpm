mod selector;

use super::output;
use anyhow::{Context, Result, bail, ensure};
use clap::{Args, ValueEnum};
use kako_mods_lib::local::{
    mc::server::{MinecraftServer, MinecraftServerMetadata, ServerModLoader, identify},
    modloader::BUILTIN_LOADERS,
};
use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

#[derive(Args)]
pub(super) struct InitArgs {
    /// Existing server directory; defaults to the current directory.
    #[arg(value_name = "PATH", default_value = ".")]
    path: PathBuf,
    /// Statically inspect this script; conflicts with manual version/loader options.
    #[arg(long, value_name = "FILE", conflicts_with_all = ["minecraft_version", "loader", "loader_version"])]
    start_script: Option<PathBuf>,
    /// Minecraft version. Omitted fields are inferred from the default start script.
    #[arg(long, alias = "mc-version", value_name = "VERSION")]
    minecraft_version: Option<String>,
    /// Loader identity, or vanilla for an explicitly unmodded server.
    #[arg(long, value_enum)]
    loader: Option<Loader>,
    /// Loader version, independent of Minecraft and FML versions.
    #[arg(long, value_name = "VERSION")]
    loader_version: Option<String>,
    /// Replace existing server.json only after successful identification and selection.
    #[arg(long)]
    force: bool,
    /// Show the entire flattened command in the initialization result.
    #[arg(long)]
    full_command: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
enum Loader {
    Forge,
    Neoforge,
    Fabric,
    Vanilla,
}
impl Loader {
    fn id(self) -> &'static str {
        match self {
            Self::Forge => "forge",
            Self::Neoforge => "neoforge",
            Self::Fabric => "fabric",
            Self::Vanilla => "vanilla",
        }
    }
}

pub(super) fn run(args: InitArgs) -> Result<()> {
    let manifest = args.path.join(".mcpm/server.json");
    match fs::symlink_metadata(&manifest) {
        Ok(metadata) => {
            ensure!(
                args.force,
                "server configuration already exists at {manifest:?}; use --force to replace it"
            );
            ensure!(
                !metadata.is_dir(),
                "server configuration path is a directory: {manifest:?}"
            );
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => {}
        Err(error) => return Err(error).context("failed to inspect existing server configuration"),
    }
    for value in [&args.minecraft_version, &args.loader_version]
        .into_iter()
        .flatten()
    {
        ensure!(
            !value.trim().is_empty(),
            "version options must not be empty"
        );
    }
    ensure!(
        !(args.loader == Some(Loader::Vanilla) && args.loader_version.is_some()),
        "vanilla does not have a loader version"
    );
    let complete = args.minecraft_version.is_some()
        && match args.loader {
            Some(Loader::Vanilla) => true,
            Some(_) => args.loader_version.is_some(),
            None => false,
        };
    let metadata = if complete {
        MinecraftServerMetadata {
            version: 1,
            minecraft_version: args.minecraft_version.clone().unwrap(),
            loader: args
                .loader
                .filter(|loader| *loader != Loader::Vanilla)
                .map(|loader| ServerModLoader {
                    loader_type: loader.id().into(),
                    version: args.loader_version.clone().unwrap(),
                }),
            start: None,
        }
    } else {
        let script = args
            .start_script
            .clone()
            .unwrap_or_else(|| PathBuf::from(if cfg!(windows) { "run.bat" } else { "run.sh" }));
        let mut candidates = identify::identify(&args.path, &script, &BUILTIN_LOADERS)
            .context("automatic server identification failed; server remains unrecognized; initialize manually with --minecraft-version VERSION --loader forge|neoforge|fabric --loader-version VERSION, or explicitly use --loader vanilla for a known vanilla server")?;
        candidates.retain(|candidate| {
            let metadata = &candidate.metadata;
            args.minecraft_version
                .as_ref()
                .is_none_or(|v| *v == metadata.minecraft_version)
                && args.loader.is_none_or(|loader| match &metadata.loader {
                    Some(info) => info.loader_type == loader.id(),
                    None => loader == Loader::Vanilla,
                })
                && args.loader_version.as_ref().is_none_or(|v| {
                    metadata
                        .loader
                        .as_ref()
                        .is_some_and(|info| info.version == *v)
                })
        });
        if candidates.is_empty() {
            bail!(
                "no identified server launch matches the supplied version/loader options; server remains unrecognized; initialize manually with complete --minecraft-version, --loader and --loader-version options (omit --loader-version only for explicitly selected vanilla)"
            );
        }
        let index = if candidates.len() == 1 {
            0
        } else {
            let Some(index) = selector::select(&candidates)? else {
                return Ok(());
            };
            index
        };
        candidates.swap_remove(index).metadata
    };
    let server = if args.force {
        MinecraftServer::replace(args.path, metadata)?
    } else {
        MinecraftServer::create(args.path, metadata)?
    };
    let mut stdout = io::stdout().lock();
    output::write_info(&mut stdout, &server, args.full_command)
        .context("failed to write initialized server information")?;
    stdout
        .flush()
        .context("failed to flush initialized server information")
}
