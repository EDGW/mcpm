//! Remote operation registry on stdin/stdout; all diagnostics use stderr or log streams.

use anyhow::Result;
use mcpm_exec::{
    messaging::Session,
    operation::{self, Operation},
    service::control::{HelloInfo, VERSION},
};
use std::sync::{Arc, OnceLock};

static IDS: OnceLock<Vec<String>> = OnceLock::new();

pub async fn run() -> Result<()> {
    crate::logs::init()?;
    let mut registry = mcpm_backend::operations::registry()?;
    registry.register::<Hello>()?;
    registry.register::<crate::logs::Logs>()?;
    let _ = IDS.set(registry.ids().into_iter().map(str::to_owned).collect());
    let mut session = Session::new((tokio::io::stdin(), tokio::io::stdout()));
    operation::serve(&mut session, Arc::new(registry)).await?;
    Ok(())
}

struct Hello;
impl Operation for Hello {
    const ID: &'static str = "mcpmd.hello";
    type Input = ();
    type Output = HelloInfo;
    async fn run(_: ()) -> Result<HelloInfo> {
        Ok(HelloInfo {
            version: VERSION.into(),
            operations: IDS.get().cloned().unwrap_or_default(),
        })
    }
}
