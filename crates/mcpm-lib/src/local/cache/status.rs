//! Content verification policies shared by all caches.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VerificationMode {
    Structure,
    Content,
}
