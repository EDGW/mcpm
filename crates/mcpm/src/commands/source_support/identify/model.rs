//! Flow-owned executor and read-only cache location for Host identification.

use mcpm_lib::local::cache::mods::ModsCacheSelection;

/// A flow-owned executor and the resolved cache selection on the file host.
#[derive(Clone, Copy)]
pub(in crate::commands) struct Location<'a> {
    pub executor: &'a mcpm_exec::service::Executor,
    pub cache: Option<&'a ModsCacheSelection>,
}
impl<'a> Location<'a> {
    pub(in crate::commands) fn new(
        executor: &'a mcpm_exec::service::Executor,
        cache: Option<&'a ModsCacheSelection>,
    ) -> Self {
        Self { executor, cache }
    }
    pub(in crate::commands) fn is_local(self) -> bool {
        !self.executor.is_remote()
    }
}
impl std::fmt::Debug for Location<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Location")
            .field("cache", &self.cache)
            .field("remote", &!self.is_local())
            .finish()
    }
}
