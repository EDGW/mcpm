//! Server-scoped inspection dispatch; server metadata is resolved on the file-owning host.

use anyhow::Result;

use super::{super::mod_source::ServerMods, input::FileArgs};
use crate::{
    commands::mod_support::inspect::{self, Display},
    ssh::SshArgs,
};

mod remote;

pub(super) fn run(
    files: FileArgs,
    parallelism: usize,
    display: Display<'_>,
    ssh: SshArgs,
) -> Result<()> {
    let (path, selection) = files.with_stdin()?.into_parts();
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let (path, ssh) = crate::commands::server::target::resolve(path, ssh)?;
    if ssh.is_requested() {
        remote::run(ssh, path, selection, parallelism, display)
    } else {
        tracing::info!(path = %path.display(), "Reading server mod scope for inspection");
        let source = ServerMods::load(path)?;
        inspect::run(
            selection,
            source.scope(),
            source.loaders(),
            true,
            parallelism,
            display,
        )
    }
}
