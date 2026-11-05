//! Byte transports that a [`Session`](super::Session) can run on.

// Imports

use tokio::io::{AsyncRead, AsyncWrite, DuplexStream, ReadHalf, WriteHalf};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

/// A pair of byte streams: one carries frames from the peer, the other carries frames to it.
///
/// A session reads and writes concurrently, so a carrier is split into independent halves.
/// Any `(reader, writer)` tuple is a carrier, which covers child process pipes
/// (`(ChildStdout, ChildStdin)`) and the current process (`(Stdin, Stdout)`). Tokio's `Stdout`
/// is not closed when the session ends; the peer sees EOF when the process exits.
pub trait Carrier {
    /// Stream of bytes sent by the peer.
    type Reader: AsyncRead + Unpin + Send + 'static;
    /// Stream of bytes sent to the peer.
    type Writer: AsyncWrite + Unpin + Send + 'static;

    /// Splits the carrier into its reading and writing halves.
    fn split(self) -> (Self::Reader, Self::Writer);
}

impl<R, W> Carrier for (R, W)
where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    type Reader = R;
    type Writer = W;

    fn split(self) -> (R, W) {
        self
    }
}

impl Carrier for TcpStream {
    type Reader = OwnedReadHalf;
    type Writer = OwnedWriteHalf;

    fn split(self) -> (OwnedReadHalf, OwnedWriteHalf) {
        self.into_split()
    }
}

/// In-process carrier, one end of [`tokio::io::duplex`].
impl Carrier for DuplexStream {
    type Reader = ReadHalf<DuplexStream>;
    type Writer = WriteHalf<DuplexStream>;

    fn split(self) -> (Self::Reader, Self::Writer) {
        tokio::io::split(self)
    }
}
