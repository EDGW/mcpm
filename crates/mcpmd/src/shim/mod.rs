//! Independent Java-owning process; all runtime semantics belong to backend and lib.

pub async fn run() -> anyhow::Result<()> {
    mcpm_backend::server::shim::serve().await
}
