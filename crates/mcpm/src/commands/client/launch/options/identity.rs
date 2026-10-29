//! CLI player identity options and redacted debug formatting.

use clap::Args;
use mcpm_backend::local::mc::client::launch::{Identity, Secret};

#[derive(Args)]
#[command(next_help_heading = "Identity")]
pub(super) struct IdentityArgs {
    /// Player name; uses offline identity unless an access token is supplied.
    #[arg(long, default_value = "Player")]
    username: String,
    /// Override the offline UUID, or supply the online account UUID.
    #[arg(long)]
    uuid: Option<String>,
    /// Supplied online token; can also be read from MCPM_ACCESS_TOKEN.
    #[arg(
        long,
        env = "MCPM_ACCESS_TOKEN",
        hide_env_values = true,
        requires = "uuid"
    )]
    access_token: Option<String>,
    /// Override legacy (offline) or msa (online) user type.
    #[arg(long)]
    user_type: Option<String>,
    /// Supplied Xbox user identifier.
    #[arg(long, requires = "access_token")]
    xuid: Option<String>,
    /// Supplied authentication client identifier.
    #[arg(long, requires = "access_token")]
    client_id: Option<String>,
}

impl std::fmt::Debug for IdentityArgs {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("IdentityArgs")
            .field("username", &self.username)
            .field("uuid", &self.uuid)
            .field(
                "access_token",
                &self.access_token.as_ref().map(|_| "[REDACTED]"),
            )
            .field("user_type", &self.user_type)
            .field("xuid", &self.xuid.as_ref().map(|_| "[REDACTED]"))
            .field("client_id", &self.client_id.as_ref().map(|_| "[REDACTED]"))
            .finish()
    }
}

impl IdentityArgs {
    pub(super) fn build(&self) -> Identity {
        let mut identity = Identity::offline(&self.username);
        if let Some(uuid) = &self.uuid {
            identity.uuid = uuid.clone();
        }
        if let Some(token) = &self.access_token {
            identity.access_token = Secret::new(token);
            identity.user_type = "msa".into();
        }
        if let Some(kind) = &self.user_type {
            identity.user_type = kind.clone();
        }
        identity.xuid = self.xuid.clone();
        identity.client_id = self.client_id.clone();
        identity
    }
}
