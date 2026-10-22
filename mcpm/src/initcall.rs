//! Module-owned startup hooks discovered through linker registration, without a central module list.

use anyhow::Result;

#[derive(Debug)]
pub(crate) struct Initcall(pub fn() -> Result<()>);

inventory::collect!(Initcall);

pub(crate) fn run() -> Result<()> {
    for init in inventory::iter::<Initcall> {
        (init.0)()?;
    }
    Ok(())
}
