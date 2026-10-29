//! Where a session dispatches: the calling process, or a host reached over SSH.

use crate::ssh::Connection;

/// Execution location of one session.
///
/// A surface operation ignores it; a host operation runs in-process for [`Target::Local`]
/// and on the connected host for [`Target::Remote`].
#[derive(Clone, Debug)]
pub enum Target {
    Local,
    Remote(Connection),
}

impl Target {
    pub fn is_remote(&self) -> bool {
        matches!(self, Self::Remote(_))
    }

    pub fn connection(&self) -> Option<&Connection> {
        match self {
            Self::Local => None,
            Self::Remote(connection) => Some(connection),
        }
    }
}
