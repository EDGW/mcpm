//! Shared read observations for stash commands; storage codecs and rendering remain with their owners.

use mcpm_exec::error::PortableError;

use crate::diagnostics;

pub(super) fn observed_head(
    head: Result<Option<uuid::Uuid>, PortableError>,
    failures: &mut usize,
) -> Option<Option<uuid::Uuid>> {
    match head {
        Ok(head) => Some(head),
        Err(error) => {
            report(error, failures);
            None
        }
    }
}

pub(super) fn report(error: PortableError, failures: &mut usize) {
    diagnostics::error(&error.into_inner());
    *failures += 1;
}
