use std::{
    io,
    net::{IpAddr, SocketAddr},
    pin::Pin,
    task::Poll,
    time::Duration,
};

use futures::Stream;
use rd_interface::{
    async_trait, impl_async_read_write, prelude::*, registry::NetFactory, Address, Bytes, BytesMut,
    INet, IntoDyn, Result, TcpListener, TcpStream, UdpSocket,
};
use socket2::{Domain, Socket, Type};
use tokio::{net, time::timeout};
use tokio_util::{codec::BytesCodec, udp::UdpFramed};
use tracing::instrument;

#[rd_config]
#[derive(Debug, Clone, Default)]
pub struct LocalNetConfig {
    /// set ttl
    #[serde(default)]
    pub ttl: Option<u32>,

    /// set nodelay. default is true
    #[serde(default)]
    pub nodelay: Option<bool>,

    /// set SO_MARK on linux
    pub mark: Option<u32>,

    /// bind to device
    pub bind_device: Option<String>,

    /// bind to address
    pub bind_addr: Option<IpAddr>,

    /// timeout of TCP connect, in seconds.
    pub connect_timeout: Option<u64>,
}

pub struct LocalNet(LocalNetConfig);
pub struct CompatTcp(pub(crate) net::TcpStream);
pub struct Listener(net::TcpListener, LocalNetConfig);
pub struct Udp(UdpFramed<BytesCodec, net::UdpSocket>);

impl LocalNet {
    pub fn new(config: LocalNetConfig) -> LocalNet {
        LocalNet(config)
    }
    fn set_socket(&self, socket: &Socket, _addr: SocketAddr, is_tcp: bool) -> Result<()> {
        socket.set_nonblocking(true)?;

        if let Some(local_addr) = self.0.bind_addr {
            socket.bind(&SocketAddr::new(local_addr, 0).into())?;
        }

        if let Some(ttl) = self.0.ttl {
            socket.set_ttl(ttl)?;
        }

        if is_tcp {
            socket.set_nodelay(self.0.nodelay.unwrap_or(true))?;
        }

        #[cfg(target_os = "linux")]
        if let Some(mark) = self.0.mark {
            socket.set_mark(mark)?;
        }

        #[cfg(target_os = "linux")]
        if let Some(device) = &self.0.bind_device {
            socket.bind_device(Some(device.as_bytes()))?;
        }

        #[cfg(target_os = "macos")]
        if let Some(device) = &self.0.bind_device {
            let device = std::ffi::CString::new(device.as_bytes())
                .map_err(rd_interface::error::map_other)?;
            unsafe {
                let idx = libc::if_nametoindex(device.as_ptr());
                if idx == 0 {
                    return Err(io::Error::last_os_error().into());
                }

                const IPV6_BOUND_IF: libc::c_int = 125;
                let ret = match _addr {
                    SocketAddr::V4(_) => libc::setsockopt(
                        std::os::unix::prelude::AsRawFd::as_raw_fd(socket),
                        libc::IPPROTO_IP,
                        libc::IP_BOUND_IF,
                        &idx as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_uint>() as libc::socklen_t,
                    ),
                    SocketAddr::V6(_) => libc::setsockopt(
                        std::os::unix::prelude::AsRawFd::as_raw_fd(socket),
                        libc::IPPROTO_IPV6,
                        IPV6_BOUND_IF,
                        &idx as *const _ as *const libc::c_void,
                        std::mem::size_of::<libc::c_uint>() as libc::socklen_t,
                    ),
                };

                if ret == -1 {
                    return Err(io::Error::last_os_error().into());
                }
            }
        }

        Ok(())
    }
    async fn tcp_connect_single(&self, addr: SocketAddr) -> Result<net::TcpStream> {
        let socket = match addr {
            SocketAddr::V4(_) => Socket::new(Domain::IPV4, Type::STREAM, None)?,
            SocketAddr::V6(_) => Socket::new(Domain::IPV6, Type::STREAM, None)?,
        };

        self.set_socket(&socket, addr, true)?;

        let socket = net::TcpSocket::from_std_stream(socket.into());

        let tcp = match self.0.connect_timeout {
            None => socket.connect(addr).await?,
            Some(secs) => timeout(Duration::from_secs(secs), socket.connect(addr)).await??,
        };

        Ok(tcp)
    }
    async fn tcp_bind_single(&self, addr: SocketAddr) -> Result<net::TcpListener> {
        let listener = net::TcpListener::bind(addr).await?;

        Ok(listener)
    }
    async fn udp_bind_single(&self, addr: SocketAddr) -> Result<net::UdpSocket> {
        let udp = match addr {
            SocketAddr::V4(_) => Socket::new(Domain::IPV4, Type::DGRAM, None)?,
            SocketAddr::V6(_) => Socket::new(Domain::IPV6, Type::DGRAM, None)?,
        };

        self.set_socket(&udp, addr, false)?;

        udp.bind(&addr.into())?;

        let udp = net::UdpSocket::from_std(udp.into())?;

        Ok(udp)
    }
}

impl std::fmt::Debug for LocalNet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalNet").finish()
    }
}

#[instrument(err)]
async fn lookup_host(domain: String, port: u16) -> io::Result<Vec<SocketAddr>> {
    use tokio::net::lookup_host;

    let domain = (domain.as_ref(), port);
    Ok(lookup_host(domain).await?.collect())
}

impl_async_read_write!(CompatTcp, 0);

#[async_trait]
impl rd_interface::ITcpStream for CompatTcp {
    async fn peer_addr(&self) -> Result<SocketAddr> {
        self.0.peer_addr().map_err(Into::into)
    }
    async fn local_addr(&self) -> Result<SocketAddr> {
        self.0.local_addr().map_err(Into::into)
    }
}
impl CompatTcp {
    fn new(t: net::TcpStream) -> CompatTcp {
        CompatTcp(t)
    }
}

#[async_trait]
impl rd_interface::ITcpListener for Listener {
    async fn accept(&self) -> Result<(TcpStream, SocketAddr)> {
        let (socket, addr) = self.0.accept().await?;
        if let Some(ttl) = self.1.ttl {
            socket.set_ttl(ttl)?;
        }
        socket.set_nodelay(self.1.nodelay.unwrap_or(true))?;
        Ok((CompatTcp::new(socket).into_dyn(), addr))
    }

    async fn local_addr(&self) -> Result<SocketAddr> {
        self.0.local_addr().map_err(Into::into)
    }
}

impl Udp {
    async fn send_to_single(&self, buf: &[u8], addr: SocketAddr) -> Result<usize> {
        self.0.send_to(buf, addr).await.map_err(Into::into)
    }
    fn new(socket: net::UdpSocket) -> Udp {
        Udp(socket, Box::new([0u8; 2048]), None)
    }
}

impl rd_interface::Stream for Udp {
    type Item = io::Result<(BytesMut, SocketAddr)>;

    fn poll_next(
        mut self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

impl rd_interface::Sink<(Bytes, SocketAddr)> for Udp {
    type Error = io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.0).poll_ready(cx)
    }

    fn start_send(mut self: Pin<&mut Self>, item: (Bytes, SocketAddr)) -> Result<(), Self::Error> {
        Pin::new(&mut self.0).start_send(item)
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.0).poll_flush(cx)
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Pin::new(&mut self.0).poll_close(cx)
    }
}

#[async_trait]
impl rd_interface::IUdpSocket for Udp {
    async fn local_addr(&self) -> Result<SocketAddr> {
        self.0.local_addr().map_err(Into::into)
    }
}

#[async_trait]
impl INet for LocalNet {
    #[instrument(err)]
    async fn tcp_connect(
        &self,
        _ctx: &mut rd_interface::Context,
        addr: &Address,
    ) -> Result<TcpStream> {
        let addrs = addr.resolve(lookup_host).await?;
        let mut last_err = None;

        for addr in addrs {
            match self.tcp_connect_single(addr).await {
                Ok(stream) => return Ok(CompatTcp::new(stream).into_dyn()),
                Err(e) => last_err = Some(e),
            }
        }

        Err(last_err.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "could not resolve to any address",
            )
            .into()
        }))
    }

    #[instrument(err)]
    async fn tcp_bind(
        &self,
        _ctx: &mut rd_interface::Context,
        addr: &Address,
    ) -> Result<TcpListener> {
        let addrs = addr.resolve(lookup_host).await?;
        let mut last_err = None;

        for addr in addrs {
            match self.tcp_bind_single(addr).await {
                Ok(listener) => return Ok(Listener(listener, self.0.clone()).into_dyn()),
                Err(e) => last_err = Some(e),
            }
        }

        Err(last_err.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "could not resolve to any address",
            )
            .into()
        }))
    }

    #[instrument(err)]
    async fn udp_bind(
        &self,
        _ctx: &mut rd_interface::Context,
        addr: &Address,
    ) -> Result<UdpSocket> {
        let addrs = addr.resolve(lookup_host).await?;
        let mut last_err = None;

        for addr in addrs {
            match self.udp_bind_single(addr).await {
                Ok(udp) => return Ok(Udp::new(udp).into_dyn()),
                Err(e) => last_err = Some(e),
            }
        }

        Err(last_err.unwrap_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "could not resolve to any address",
            )
            .into()
        }))
    }

    #[instrument(err)]
    async fn lookup_host(&self, addr: &Address) -> Result<Vec<SocketAddr>> {
        let addr = addr.resolve(lookup_host).await?;
        Ok(addr)
    }
}

impl NetFactory for LocalNet {
    const NAME: &'static str = "local";
    type Config = LocalNetConfig;
    type Net = Self;

    fn new(config: Self::Config) -> Result<Self> {
        Ok(LocalNet::new(config))
    }
}
