//! Server graph request parameters and workspace-aware Executor selection.

use crate::ssh::SshArgs;
use anyhow::Result;
use clap::Args;
use mcpm_backend::operations::mods::loading::ScopeInput;
use mcpm_exec::service::Executor;
use std::path::PathBuf;

#[derive(Args, Debug)]
pub(super) struct ServerArgs {
    /// Registered server directory; defaults to the current directory.
    #[arg(long = "server", value_name = "PATH", default_value = ".")]
    path: PathBuf,
}
impl ServerArgs {
    pub(super) async fn resolve(self, ssh: SshArgs) -> Result<(Executor, ScopeInput)> {
        let mut scope = ScopeInput::Server {
            path: self.path,
            no_cache: false,
        };
        let executor = crate::commands::supports::mods::target::executor(&mut scope, ssh).await?;
        Ok((executor, scope))
    }
}
