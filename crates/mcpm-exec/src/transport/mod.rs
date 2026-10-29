//! Transports: typed in-process execution and the remote process session.
//!
//! In-process calls never serialize; the remote session encodes a host operation's input,
//! routes frames by call GUID and decodes events, output and error on the caller side.

pub(crate) mod inprocess;
pub mod session;
