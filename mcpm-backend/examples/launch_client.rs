//! Print a local launch plan; pass --run explicitly to prepare and launch it.
use anyhow::{Context, Result, bail};
use mcpm_backend::local::mc::client::{
    ClientPlatform, MinecraftDirectory, RuleEnvironment,
    launch::{Identity, LaunchOptions},
};

fn main() -> Result<()> {
    let mut args = std::env::args_os().skip(1);
    let usage = "usage: launch_client ROOT VERSION JAVA PLAYER OS ARCH OS_VERSION BITS [--run]";
    let root = args.next().context(usage)?;
    let version = args
        .next()
        .context(usage)?
        .into_string()
        .map_err(|_| anyhow::anyhow!("VERSION must be UTF-8"))?;
    let java = args.next().context(usage)?;
    let mut text = || -> Result<String> {
        args.next()
            .context(usage)?
            .into_string()
            .map_err(|_| anyhow::anyhow!("identity and platform fields must be UTF-8"))
    };
    let player = text()?;
    let rules = RuleEnvironment::new(ClientPlatform {
        os: text()?,
        arch: text()?,
        version: text()?,
        bits: text()?.parse()?,
    });
    let run = match args.next() {
        None => false,
        Some(flag) if flag == "--run" => true,
        Some(_) => bail!("{usage}"),
    };
    if args.next().is_some() {
        bail!("{usage}");
    }
    let directory = MinecraftDirectory::new(root)?;
    let version = directory.version(version)?;
    let mut options = LaunchOptions::new(java, Identity::offline(player), rules);
    options.directories.game = Some(version.path().canonicalize()?);
    options.memory.maximum_mib = Some(4096);
    let plan = directory.plan_launch(&version, &options)?;
    println!("{plan:#?}");
    if run {
        let mut process = plan.prepare()?.spawn()?;
        let status = process.wait()?;
        if !status.success() {
            bail!("Minecraft exited with {status}");
        }
    }
    Ok(())
}
