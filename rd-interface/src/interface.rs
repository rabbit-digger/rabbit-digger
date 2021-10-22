use std::net::SocketAddr;

pub use crate::Context;
use crate::NOT_IMPLEMENTED;
pub use crate::{Address, Error, Result};
pub use async_trait::async_trait;
pub use bytes::{Bytes, BytesMut};
pub use futures_util::{Sink, Stream};
use std::io;
pub use std::sync::Arc;
pub use tokio::io::{AsyncRead, AsyncWrite, ReadBuf};

pub trait IntoDyn<DynType> {
    fn into_dyn(self) -> DynType
    where
        Self: Sized + 'static;
}

/// A TcpListener.
#[async_trait]
pub trait ITcpListener: Unpin + Send + Sync {
    async fn accept(&self) -> Result<(TcpStream, SocketAddr)>;
    async fn local_addr(&self) -> Result<SocketAddr>;
}
pub type TcpListener = Box<dyn ITcpListener>;

impl<T: ITcpListener> IntoDyn<TcpListener> for T {
    fn into_dyn(self) -> TcpListener
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

/// A TcpStream.
#[async_trait]
pub trait ITcpStream: AsyncRead + AsyncWrite + Unpin + Send + Sync {
    async fn peer_addr(&self) -> Result<SocketAddr>;
    async fn local_addr(&self) -> Result<SocketAddr>;
}
pub type TcpStream = Box<dyn ITcpStream>;

impl<T: ITcpStream> IntoDyn<TcpStream> for T {
    fn into_dyn(self) -> TcpStream
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

/// A UdpSocket.
#[async_trait]
pub trait IUdpSocket:
    Stream<Item = io::Result<(BytesMut, SocketAddr)>>
    + Sink<(Bytes, SocketAddr), Error = io::Error>
    + Unpin
    + Send
    + Sync
{
    async fn local_addr(&self) -> Result<SocketAddr>;
}
pub type UdpSocket = Box<dyn IUdpSocket>;

impl<T: IUdpSocket> IntoDyn<UdpSocket> for T {
    fn into_dyn(self) -> UdpSocket
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

/// A Net.
#[async_trait]
pub trait INet: Unpin + Send + Sync {
    async fn tcp_connect(&self, _ctx: &mut Context, _addr: &Address) -> Result<TcpStream> {
        Err(NOT_IMPLEMENTED)
    }
    async fn tcp_bind(&self, _ctx: &mut Context, _addr: &Address) -> Result<TcpListener> {
        Err(NOT_IMPLEMENTED)
    }
    async fn udp_bind(&self, _ctx: &mut Context, _addr: &Address) -> Result<UdpSocket> {
        Err(NOT_IMPLEMENTED)
    }
    async fn lookup_host(&self, _addr: &Address) -> Result<Vec<SocketAddr>> {
        Err(NOT_IMPLEMENTED)
    }
}
pub type Net = Arc<dyn INet>;

impl<T: INet> IntoDyn<Net> for T {
    fn into_dyn(self) -> Net
    where
        Self: Sized + 'static,
    {
        Arc::new(self)
    }
}

/// A Server.
#[async_trait]
pub trait IServer: Unpin + Send + Sync {
    /// Start the server, drop to stop.
    async fn start(&self) -> Result<()>;
}
pub type Server = Box<dyn IServer>;

impl<T: IServer> IntoDyn<Server> for T {
    fn into_dyn(self) -> Server
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}

/// The other side of an UdpSocket
#[async_trait]
pub trait IUdpChannel: Send + Sync {
    async fn recv_send_to(&self, data: &mut [u8]) -> Result<(usize, Address)>;
    async fn send_recv_from(&self, data: &[u8], addr: SocketAddr) -> Result<usize>;
}
pub type UdpChannel = Box<dyn IUdpChannel>;

impl<T: IUdpChannel> crate::IntoDyn<UdpChannel> for T {
    fn into_dyn(self) -> UdpChannel
    where
        Self: Sized + 'static,
    {
        Box::new(self)
    }
}
