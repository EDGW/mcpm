//! Presentation-only connection details supplied by server command routing.

/// Remote connection fields supplied by command routing; rendering never reads workspace files.
pub(crate) struct RemoteInfo {
    pub path: String,
    pub ssh: String,
    pub destination: Option<String>,
    pub executable: String,
}
