use super::{
    addr::{Address, IntoAddress},
    ext::UdpSocketConnect,
};
pub use async_trait::async_trait;
use futures::future::FutureExt;
pub use futures::future::RemoteHandle;
pub use futures::io::{AsyncRead, AsyncWrite};
use std::{
    future::Future,
    io::Result,
    net::{Shutdown, SocketAddr},
    time::Duration,
};

/// A TcpListener
#[async_trait]
pub trait TcpListener<TcpStream>: Unpin + Send + Sync {
    async fn accept(&self) -> Result<(TcpStream, SocketAddr)>;
    async fn local_addr(&self) -> Result<SocketAddr>;
}

/// A TcpStream
#[async_trait]
pub trait TcpStream: AsyncRead + AsyncWrite + Unpin + Send + Sync {
    async fn peer_addr(&self) -> Result<SocketAddr>;
    async fn local_addr(&self) -> Result<SocketAddr>;
    async fn shutdown(&self, how: Shutdown) -> Result<()>;
}

/// A UdpSocket
#[async_trait]
pub trait UdpSocket: Unpin + Send + Sync {
    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)>;
    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> Result<usize>;
    async fn local_addr(&self) -> Result<SocketAddr>;
}

pub trait UdpSocketExt: UdpSocket + Sized {
    fn ext(self) -> UdpSocketConnect<Self> {
        UdpSocketConnect::new(self)
    }
}
impl<T: UdpSocket + Sized> UdpSocketExt for T {}

/// A proxy tcp stream
#[async_trait]
pub trait ProxyTcpStream: Unpin + Send + Sync {
    type TcpStream: TcpStream;
    async fn tcp_connect<A: IntoAddress>(&self, addr: A) -> Result<Self::TcpStream>;
}

#[async_trait]
impl<T: ProxyTcpStream> ProxyTcpStream for &T {
    type TcpStream = T::TcpStream;

    async fn tcp_connect<A: IntoAddress>(&self, addr: A) -> Result<Self::TcpStream> {
        T::tcp_connect(*self, addr).await
    }
}

/// A proxy tcp listener
#[async_trait]
pub trait ProxyTcpListener: Unpin + Send + Sync {
    type TcpStream: TcpStream;
    type TcpListener: TcpListener<Self::TcpStream>;
    async fn tcp_bind<A: IntoAddress>(&self, addr: A) -> Result<Self::TcpListener>;
}

#[async_trait]
impl<T: ProxyTcpListener> ProxyTcpListener for &T {
    type TcpStream = T::TcpStream;
    type TcpListener = T::TcpListener;

    async fn tcp_bind<A: IntoAddress>(&self, addr: A) -> Result<Self::TcpListener> {
        T::tcp_bind(*self, addr).await
    }
}

/// A proxy udp socket
#[async_trait]
pub trait ProxyUdpSocket: Unpin + Send + Sync {
    type UdpSocket: UdpSocket;
    async fn udp_bind<A: IntoAddress>(&self, addr: A) -> Result<Self::UdpSocket>;
}

#[async_trait]
impl<T: ProxyUdpSocket> ProxyUdpSocket for &T {
    type UdpSocket = T::UdpSocket;

    async fn udp_bind<A: IntoAddress>(&self, addr: A) -> Result<Self::UdpSocket> {
        T::udp_bind(self, addr).await
    }
}

/// A dns resolver
#[async_trait]
pub trait ProxyResolver: Unpin + Send + Sync {
    async fn resolve<A: IntoAddress>(&self, addr: A) -> Result<SocketAddr> {
        Ok(match addr.into_address()? {
            Address::IPv4(v4) => SocketAddr::V4(v4),
            Address::IPv6(v6) => SocketAddr::V6(v6),
            Address::Domain(domain, port) => self.resolve_domain((&domain, port)).await?,
        })
    }
    async fn resolve_domain(&self, domain: (&str, u16)) -> Result<SocketAddr>;
}

#[async_trait]
pub trait Runtime: Unpin + Send + Sync {
    fn spawn_handle<Fut>(&self, future: Fut) -> RemoteHandle<Fut::Output>
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send,
    {
        let (future, handle) = future.remote_handle();
        self.spawn(future);
        handle
    }
    fn spawn<Fut>(&self, future: Fut)
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send;
    async fn sleep(&self, duration: Duration);
}

#[async_trait]
impl<T: Runtime> Runtime for &T {
    fn spawn<Fut>(&self, future: Fut)
    where
        Fut: Future + Send + 'static,
        Fut::Output: Send,
    {
        T::spawn(*self, future)
    }
    #[inline(always)]
    async fn sleep(&self, duration: Duration) {
        T::sleep(self, duration).await
    }
}

#[async_trait]
impl<T: TcpStream + ?Sized> TcpStream for Box<T> {
    async fn peer_addr(&self) -> Result<SocketAddr> {
        T::peer_addr(&self).await
    }

    async fn local_addr(&self) -> Result<SocketAddr> {
        T::local_addr(&self).await
    }

    async fn shutdown(&self, how: Shutdown) -> Result<()> {
        T::shutdown(&self, how).await
    }
}

#[async_trait]
impl<T: UdpSocket + ?Sized> UdpSocket for Box<T> {
    async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
        T::recv_from(&self, buf).await
    }

    async fn send_to(&self, buf: &[u8], addr: SocketAddr) -> Result<usize> {
        T::send_to(&self, buf, addr).await
    }

    async fn local_addr(&self) -> Result<SocketAddr> {
        T::local_addr(&self).await
    }
}

#[async_trait]
impl<T: ProxyTcpListener + ?Sized> ProxyTcpListener for Box<T> {
    type TcpStream = T::TcpStream;
    type TcpListener = T::TcpListener;

    #[inline(always)]
    async fn tcp_bind<A: IntoAddress>(&self, addr: A) -> Result<Self::TcpListener> {
        T::tcp_bind(&self, addr).await
    }
}

#[async_trait]
impl<T: ProxyTcpStream + ?Sized> ProxyTcpStream for Box<T> {
    type TcpStream = T::TcpStream;

    #[inline(always)]
    async fn tcp_connect<A: IntoAddress>(&self, addr: A) -> Result<Self::TcpStream> {
        T::tcp_connect(&self, addr).await
    }
}

#[async_trait]
impl<T: ProxyUdpSocket + ?Sized> ProxyUdpSocket for Box<T> {
    type UdpSocket = T::UdpSocket;

    #[inline(always)]
    async fn udp_bind<A: IntoAddress>(&self, addr: A) -> Result<Self::UdpSocket> {
        T::udp_bind(&self, addr).await
    }
}

pub trait ProxyNet: ProxyTcpStream + ProxyTcpListener + ProxyUdpSocket {}
