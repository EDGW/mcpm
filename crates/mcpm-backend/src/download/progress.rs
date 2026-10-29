//! Observational request and transfer events, independent of targets and terminal rendering.

/// Separates release lookups from file transfers without exposing metadata response bodies.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RequestKind {
    Metadata,
    File,
}

/// Synchronous progress notifications. Observers must return promptly; errors stop the
/// operation without source fallback. Cancellation remains dropping the operation future.
/// Observer failure does not roll back files published before a completion notification.
#[derive(Debug, Clone, Copy)]
pub enum Event<'a> {
    /// Starts a fresh attempt. Byte counters and speed estimates must reset here.
    AttemptStarted { kind: RequestKind, source: &'a str },
    /// Bytes written in this attempt. Total is an expectation or HTTP size hint, not proof.
    Bytes { received: u64, total: Option<u64> },
    /// The body ended; declared size and digests still need their final verification.
    Verifying,
    /// A remote failure allows another source attempt; the final Result owns diagnostics.
    AttemptFailed { kind: RequestKind, source: &'a str },
    /// Metadata was decoded, or a verified file was published to the requested destination.
    Completed { kind: RequestKind },
}
