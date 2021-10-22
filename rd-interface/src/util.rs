use crate::{
    interface::{
        async_trait, AsyncRead, AsyncWrite, INet, ITcpStream, Net, TcpListener, TcpStream,
        UdpSocket,
    },
    Address, Context, Result,
};
use std::{
    collections::VecDeque,
    io,
    net::{Ipv4Addr, Ipv6Addr, SocketAddr},
    pin::Pin,
    task::{self, Poll},
};
use tokio::io::{AsyncReadExt, ReadBuf};

pub struct PeekableTcpStream {
    tcp: TcpStream,
    buf: VecDeque<u8>,
}

impl AsyncRead for PeekableTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let (first, ..) = &self.buf.as_slices();
        if !first.is_empty() {
            let read = first.len().min(buf.remaining());
            let unfilled = buf.initialize_unfilled_to(read);
            unfilled[0..read].copy_from_slice(&first[0..read]);
            buf.advance(read);

            // remove 0..read
            self.buf.drain(0..read);

            Poll::Ready(Ok(()))
        } else {
            Pin::new(&mut self.tcp).poll_read(cx, buf)
        }
    }
}
impl AsyncWrite for PeekableTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.tcp).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.tcp).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.tcp).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.tcp).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.tcp.is_write_vectored()
    }
}

#[async_trait]
impl ITcpStream for PeekableTcpStream {
    async fn peer_addr(&self) -> crate::Result<SocketAddr> {
        self.tcp.peer_addr().await
    }

    async fn local_addr(&self) -> crate::Result<SocketAddr> {
        self.tcp.local_addr().await
    }
}

impl PeekableTcpStream {
    pub fn new(tcp: TcpStream) -> Self {
        PeekableTcpStream {
            tcp,
            buf: VecDeque::new(),
        }
    }
    // Fill self.buf to size using self.tcp.read_exact
    async fn fill_buf(&mut self, size: usize) -> crate::Result<()> {
        if size > self.buf.len() {
            let to_read = size - self.buf.len();
            let mut buf = vec![0u8; to_read];
            self.tcp.read_exact(&mut buf).await?;
            self.buf.append(&mut buf.into());
        }
        Ok(())
    }
    pub async fn peek_exact(&mut self, buf: &mut [u8]) -> crate::Result<()> {
        self.fill_buf(buf.len()).await?;
        let self_buf = self.buf.make_contiguous();
        buf.copy_from_slice(&self_buf[0..buf.len()]);

        Ok(())
    }
    pub fn into_inner(self) -> (TcpStream, VecDeque<u8>) {
        (self.tcp, self.buf)
    }
}

/// A no-op Net returns [`Error::NotImplemented`](crate::Error::NotImplemented) for every method.
pub struct NotImplementedNet;

#[async_trait]
impl INet for NotImplementedNet {}

/// A new Net calls [`tcp_connect()`](crate::INet::tcp_connect()), [`tcp_bind()`](crate::INet::tcp_bind()), [`udp_bind()`](crate::INet::udp_bind()) from different Net.
pub struct CombineNet {
    pub tcp_connect: Net,
    pub tcp_bind: Net,
    pub udp_bind: Net,
    pub lookup_host: Net,
}

#[async_trait]
impl INet for CombineNet {
    async fn tcp_connect(&self, ctx: &mut Context, addr: &Address) -> Result<TcpStream> {
        self.tcp_connect.tcp_connect(ctx, addr).await
    }

    async fn tcp_bind(&self, ctx: &mut Context, addr: &Address) -> Result<TcpListener> {
        self.tcp_bind.tcp_bind(ctx, addr).await
    }

    async fn udp_bind(&self, ctx: &mut Context, addr: &Address) -> Result<UdpSocket> {
        self.udp_bind.udp_bind(ctx, addr).await
    }

    async fn lookup_host(&self, addr: &Address) -> Result<Vec<SocketAddr>> {
        self.lookup_host.lookup_host(addr).await
    }
}

/// Helper function for converting IPv4 mapped IPv6 address
///
/// This is the same as `Ipv6Addr::to_ipv4_mapped`, but it is still unstable in the current libstd
fn to_ipv4_mapped(ipv6: &Ipv6Addr) -> Option<Ipv4Addr> {
    match ipv6.octets() {
        [0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0xff, 0xff, a, b, c, d] => Some(Ipv4Addr::new(a, b, c, d)),
        _ => None,
    }
}

pub fn resolve_mapped_socket_addr(addr: SocketAddr) -> SocketAddr {
    if let SocketAddr::V6(ref a) = addr {
        if let Some(v4) = to_ipv4_mapped(a.ip()) {
            return SocketAddr::new(v4.into(), a.port());
        }
    }

    addr
}
