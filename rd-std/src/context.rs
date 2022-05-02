use std::{borrow::Cow, future::Future, io};

use connect_tcp::connect_tcp;
use connect_udp::connect_udp;
use futures::future::BoxFuture;
use rd_interface::{
    async_trait, Address, AsyncRead, AsyncWrite, Context, Net, Result, TcpStream, UdpChannel,
    UdpSocket,
};

mod connect_tcp;
mod connect_udp;

pub enum Accepter<T, S> {
    Socket(S),
    Future(Box<dyn FnOnce(T) -> BoxFuture<'static, Result<(T, S)>> + Send>),
}

impl<F> From<F> for Accepter<TcpStream, TcpStream>
where
    F: FnOnce(TcpStream) -> BoxFuture<'static, Result<(TcpStream, TcpStream)>> + Send + 'static,
{
    fn from(f: F) -> Self {
        Accepter::Future(Box::new(f))
    }
}

impl From<TcpStream> for Accepter<TcpStream, TcpStream> {
    fn from(s: TcpStream) -> Self {
        Accepter::Socket(s)
    }
}

impl<F> From<F> for Accepter<UdpSocket, UdpChannel>
where
    F: FnOnce(UdpSocket) -> BoxFuture<'static, Result<(UdpSocket, UdpChannel)>> + Send + 'static,
{
    fn from(f: F) -> Self {
        Accepter::Future(Box::new(f))
    }
}

impl From<UdpChannel> for Accepter<UdpSocket, UdpChannel> {
    fn from(s: UdpChannel) -> Self {
        Accepter::Socket(s)
    }
}

impl<T, S> Accepter<T, S> {
    async fn get(self, t: T) -> Result<(T, S)> {
        match self {
            Accepter::Socket(s) => Ok((t, s)),
            Accepter::Future(f) => f(t).await,
        }
    }
}

pub enum AcceptCommand {
    TcpConnect(Address, Accepter<TcpStream, TcpStream>),
    UdpBind(Address, Accepter<UdpSocket, UdpChannel>),
    Reject(Cow<'static, str>),
}

#[async_trait]
pub trait ContextExt {
    #[deprecated(since = "0.1.0", note = "use `ContextExt::accept` instead")]
    async fn connect_udp(&mut self, a: UdpChannel, b: UdpSocket) -> io::Result<()>;

    #[deprecated(since = "0.1.0", note = "use `ContextExt::accept` instead")]
    async fn connect_tcp<A, B>(&mut self, a: A, b: B) -> io::Result<()>
    where
        A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        B: AsyncRead + AsyncWrite + Unpin + Send + 'static;

    async fn accept<Fut>(self, fut: Fut, net: &Net) -> Result<()>
    where
        Fut: Future<Output = Result<AcceptCommand>> + Send;
}

#[async_trait]
impl ContextExt for Context {
    async fn connect_udp(&mut self, a: UdpChannel, b: UdpSocket) -> io::Result<()> {
        connect_udp(self, a, b).await
    }

    async fn connect_tcp<A, B>(&mut self, a: A, b: B) -> io::Result<()>
    where
        A: AsyncRead + AsyncWrite + Unpin + Send + 'static,
        B: AsyncRead + AsyncWrite + Unpin + Send + 'static,
    {
        connect_tcp(self, a, b).await
    }

    async fn accept<Fut>(mut self, fut: Fut, net: &Net) -> Result<()>
    where
        Fut: Future<Output = Result<AcceptCommand>> + Send,
    {
        match fut.await? {
            AcceptCommand::TcpConnect(connect_addr, accepter) => {
                let tcp = net.tcp_connect(&mut self, &connect_addr).await?;
                let (tcp, accepted_tcp) = accepter.get(tcp).await?;
                connect_tcp(&mut self, accepted_tcp, tcp).await?;
            }
            AcceptCommand::UdpBind(bind_addr, accepter) => {
                let udp = net.udp_bind(&mut self, &bind_addr).await?;
                let (udp, accepted_udp) = accepter.get(udp).await?;
                connect_udp(&mut self, accepted_udp, udp).await?;
            }
            AcceptCommand::Reject(_reason) => {}
        }

        Ok(())
    }
}
