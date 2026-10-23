//! Server-scoped inspection dispatch; server metadata is resolved on the file-owning host.

use anyhow::Result;

use super::{
    super::{
        mod_source::ServerMods,
        target::{self, Target},
    },
    input::FileArgs,
};
use crate::{
    commands::mod_support::{
        inspect::{self, Display},
        scan,
    },
    ssh::SshArgs,
};

mod remote;

pub(super) fn run(
    files: FileArgs,
    loading: scan::Options,
    display: Display<'_>,
    ssh: SshArgs,
) -> Result<()> {
    let (path, selection) = files.read()?;
    if selection.is_empty() {
        return inspect::prompt_for_input();
    }
    let Target { path, ssh, .. } = target::resolve(path, ssh)?;
    if ssh.is_requested() {
        remote::run(ssh, path, selection, loading, display)
    } else {
        tracing::info!(path = %path.display(), "Reading server mod scope for inspection");
        let source = ServerMods::load(path)?;
        inspect::run(
            selection,
            source.scope(),
            source.loaders(),
            true,
            loading,
            display,
        )
    }
}
