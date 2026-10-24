//! Shared transfer preferences and checked copy/hard-link operations.

use super::Content;
use anyhow::{Context, Result, ensure};
use std::path::Path;

/// Requested policy; only capability errors permit fallback from hard linking.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheLinkPreference {
    HardLink,
    PreferHardLink,
    Copy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransferMethod {
    HardLink,
    Copy,
}

pub(crate) fn stage(
    source: &mut Content,
    destination: &Path,
    preference: CacheLinkPreference,
) -> Result<TransferMethod> {
    if preference != CacheLinkPreference::Copy {
        match super::hard_link(source.path(), destination) {
            Ok(()) => {
                let linked = Content::open(destination)?.context("staged hard link is missing")?;
                source.validate()?;
                ensure!(
                    linked.identity() == source.identity(),
                    "hard-link source changed: {:?}",
                    source.path()
                );
                return Ok(TransferMethod::HardLink);
            }
            Err(error)
                if preference == CacheLinkPreference::PreferHardLink
                    && error.allows_copy_fallback() => {}
            Err(error) => return Err(error.into()),
        }
    }
    source.copy_to(destination)?;
    Ok(TransferMethod::Copy)
}
