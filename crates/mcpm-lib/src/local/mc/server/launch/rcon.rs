//! RCON configuration selection and bounded exchanges; properties grammar has one shared codec.

use super::{JavaCommand, LaunchPlan};
use anyhow::{Context, Result, ensure};
use bytes::BytesMut;
use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    task::{Context as TaskContext, Poll},
    time::Duration,
};
use tokio::{
    io::{AsyncRead, AsyncReadExt, AsyncWrite, ReadBuf},
    net::TcpStream,
    time::timeout,
};
use tokio_util::{
    codec::{Decoder, FramedRead, LengthDelimitedCodec},
    io::StreamReader,
};

const DEADLINE: Duration = Duration::from_secs(5);
const MAX_PACKET: usize = 4 * 1024 * 1024;
const MAX_OUTPUT: usize = 8 * 1024 * 1024;

/// Host-only credentials; deliberately neither serializable nor Debug.
pub struct Settings {
    host: String,
    port: u16,
    password: String,
}
impl Settings {
    /// Only application arguments can select properties, never a similarly named JVM argument.
    pub fn from_plan(plan: &LaunchPlan) -> Result<Option<Self>> {
        let application = JavaCommand::parse(plan.arguments())?.application;
        let mut path = "server.properties";
        let mut arguments = application.iter();
        while let Some(argument) = arguments.next() {
            if matches!(argument.as_str(), "--config" | "-c") {
                path = arguments.next().context("properties option has no path")?;
            } else if let Some(value) = argument.strip_prefix("--config=") {
                path = value;
            }
        }
        let path = plan.directory().join(path);
        let Some(properties) = super::super::properties::Properties::read_selected(
            &path,
            &["enable-rcon", "rcon.port", "rcon.password", "server-ip"],
        )?
        else {
            return Ok(None);
        };
        if properties
            .get("enable-rcon")?
            .is_none_or(|value| !value.eq_ignore_ascii_case("true"))
        {
            return Ok(None);
        }
        let password = properties
            .get("rcon.password")?
            .filter(|value| !value.is_empty())
            .context("RCON enabled but rcon.password is empty")?;
        let port = properties
            .get("rcon.port")?
            .map_or(Ok(25575), |value| value.parse::<u16>())
            .context("invalid rcon.port")?;
        let host = match properties.get("server-ip")?.as_deref() {
            None | Some("") | Some("0.0.0.0") => "localhost".to_owned(),
            Some("::") => "::1".to_owned(),
            Some(value) => value.to_owned(),
        };
        Ok(Some(Self {
            host,
            port,
            password,
        }))
    }
}

/// One authenticated connection. Any execution error makes its outcome unknown to the caller.
pub struct Connection {
    inner: rcon::Connection<BoundedIo>,
    observer: std::net::TcpStream,
    reset_budget: Arc<AtomicBool>,
}
impl Connection {
    /// Refuse configured ports belonging to another process, even if credentials happen to match.
    pub async fn connect_owned(settings: &Settings, java_pid: u32) -> Result<Self> {
        timeout(DEADLINE, async {
            let addresses = tokio::net::lookup_host((settings.host.as_str(), settings.port))
                .await
                .context("failed to resolve RCON address")?;
            let mut failure = None;
            for address in addresses {
                let attempt = async {
                    tokio::task::spawn_blocking(move || verify_owner(address, java_pid)).await??;
                    let stream = TcpStream::connect(address)
                        .await
                        .context("RCON connect failed")?;
                    let stream = stream.into_std()?;
                    let observer = stream.try_clone()?;
                    let stream = TcpStream::from_std(stream)?;
                    let reset_budget = Arc::new(AtomicBool::new(false));
                    let inner = rcon::Connection::builder()
                        .enable_minecraft_quirks(true)
                        .handshake(
                            BoundedIo::new(stream, reset_budget.clone()),
                            &settings.password,
                        )
                        .await
                        .context("RCON authentication failed")?;
                    let connection = Self {
                        inner,
                        observer,
                        reset_budget,
                    };
                    tokio::task::spawn_blocking(move || verify_owner(address, java_pid)).await??;
                    Ok::<_, anyhow::Error>(connection)
                }
                .await;
                match attempt {
                    Ok(connection) => return Ok(connection),
                    Err(error) => {
                        failure = Some(match failure {
                            None => error,
                            Some(previous) => {
                                mcpm_exec::error::CombinedError::new(previous, error).into()
                            }
                        })
                    }
                }
            }
            Err(failure.unwrap_or_else(|| anyhow::anyhow!("RCON address resolved to no endpoints")))
        })
        .await
        .context("RCON ownership/authentication timed out")?
    }
    /// Observe EOF/errors without sending a command or consuming protocol bytes.
    pub fn is_connected(&self) -> bool {
        let mut byte = [0];
        match self.observer.peek(&mut byte) {
            Ok(0) => false,
            Ok(_) => true,
            Err(error) => error.kind() == io::ErrorKind::WouldBlock,
        }
    }
    /// The crate owns authentication, framing and multipart completion; only a deadline is added.
    pub async fn execute(&mut self, text: &str) -> Result<String> {
        ensure!(!text.contains('\0'), "NUL in RCON command");
        self.reset_budget.store(true, Ordering::Relaxed);
        timeout(DEADLINE, self.inner.cmd(text))
            .await
            .context("RCON command timed out after possible execution")?
            .context("RCON command exchange failed")
    }
}
fn verify_owner(address: SocketAddr, java_pid: u32) -> Result<()> {
    use netstat2::{AddressFamilyFlags, ProtocolFlags, ProtocolSocketInfo, TcpState};
    let sockets = netstat2::iterate_sockets_info(
        AddressFamilyFlags::IPV4 | AddressFamilyFlags::IPV6,
        ProtocolFlags::TCP,
    )
    .context("cannot establish RCON listener ownership")?;
    let mut found = false;
    for socket in sockets {
        let socket = socket.context("cannot inspect RCON listener ownership")?;
        if let ProtocolSocketInfo::Tcp(tcp) = &socket.protocol_socket_info
            && tcp.state == TcpState::Listen
            && tcp.local_port == address.port()
            && (tcp.local_addr.to_canonical() == address.ip().to_canonical()
                || tcp.local_addr.is_unspecified()
                // Darwin reports IPv4 sockets on a dual-stack Java listener as ::127.0.0.1.
                || matches!((tcp.local_addr, address.ip()),
                    (IpAddr::V6(local), IpAddr::V4(target)) if local.to_ipv4() == Some(target)))
        {
            ensure!(
                !socket.associated_pids.is_empty()
                    && socket.associated_pids.iter().all(|pid| *pid == java_pid),
                "configured RCON listener is not exclusively owned by this Java process"
            );
            found = true;
        }
    }
    ensure!(
        found,
        "this Java process does not yet own the configured RCON listener"
    );
    Ok(())
}

/// Bounds the standard codec before rcon allocates a frame from the peer-provided length.
/// No authentication or command-response protocol is implemented here.
struct Frames(LengthDelimitedCodec);
impl Decoder for Frames {
    type Item = BytesMut;
    type Error = io::Error;
    fn decode(&mut self, input: &mut BytesMut) -> io::Result<Option<BytesMut>> {
        let frame = self.0.decode(input)?;
        if frame.as_ref().is_some_and(|frame| frame.len() < 14) {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "RCON frame shorter than its header",
            ));
        }
        Ok(frame)
    }
}
struct BoundedIo {
    reader:
        tokio::io::Take<StreamReader<FramedRead<tokio::net::tcp::OwnedReadHalf, Frames>, BytesMut>>,
    writer: tokio::net::tcp::OwnedWriteHalf,
    reset_budget: Arc<AtomicBool>,
}
impl BoundedIo {
    fn new(stream: TcpStream, reset_budget: Arc<AtomicBool>) -> Self {
        let (reader, writer) = stream.into_split();
        let codec = LengthDelimitedCodec::builder()
            .little_endian()
            .length_field_length(4)
            .length_adjustment(4)
            .num_skip(0)
            .max_frame_length(MAX_PACKET)
            .new_codec();
        Self {
            reader: StreamReader::new(FramedRead::new(reader, Frames(codec)))
                .take(MAX_OUTPUT as u64),
            writer,
            reset_budget,
        }
    }
}
impl AsyncRead for BoundedIo {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        if self.reset_budget.swap(false, Ordering::Relaxed) {
            self.reader.set_limit(MAX_OUTPUT as u64);
        }
        Pin::new(&mut self.reader).poll_read(cx, buffer)
    }
}
impl AsyncWrite for BoundedIo {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut TaskContext<'_>,
        bytes: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.writer).poll_write(cx, bytes)
    }
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.writer).poll_flush(cx)
    }
    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut TaskContext<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.writer).poll_shutdown(cx)
    }
}
