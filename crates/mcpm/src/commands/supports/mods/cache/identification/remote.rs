//! Explicit source-result publication on the file host, without provider configuration or secrets.

use anyhow::{Context, Result, ensure};
use mcpm_backend::local::cache::hash::Blake3Hash;
use mcpm_backend::local::cache::mods::ModsCacheSelection;
use mcpm_backend::local::runtime::RuntimeEnvironment;
use mcpm_backend::source::FileMatch;
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::remote::{
    client::Session,
    command::{Command, Output},
};

struct Publish;
impl Command for Publish {
    const ID: &'static str = "cache.mods.identification.write";
    fn run(contents: Value, output: &Output) -> Result<()> {
        let request: Request =
            serde_json::from_value(contents).context("invalid identification cache publication")?;
        output.checkpoint()?;
        let environment = RuntimeEnvironment::new().with_mods_cache(Some(request.cache));
        let cache = environment.require_mods_cache()?;
        // Trusted client provider result, never loader metadata. The library verifies the
        // stored BLAKE3, size and SHA-1 before publishing through its sole snapshot codec.
        cache.write_ident_info(request.id.parse()?, &request.file)?;
        output.send(())
    }
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    cache: ModsCacheSelection,
    id: String,
    file: FileMatch,
}
pub(super) fn publish(
    session: &Session,
    cache: ModsCacheSelection,
    id: Blake3Hash,
    file: FileMatch,
) -> Result<()> {
    let mut acknowledged = false;
    session
        .submit(
            Publish::ID,
            serde_json::to_value(Request {
                cache,
                id: id.to_string(),
                file,
            })?,
        )?
        .collect(|reply| {
            ensure!(
                !acknowledged,
                "duplicate identification cache publication acknowledgement"
            );
            reply.decode::<()>()?;
            acknowledged = true;
            Ok(())
        })?;
    ensure!(
        acknowledged,
        "missing identification cache publication acknowledgement"
    );
    Ok(())
}
fn init() -> Result<()> {
    crate::remote::registry::register::<Publish>()
}
inventory::submit! { crate::initcall::Initcall(init) }
