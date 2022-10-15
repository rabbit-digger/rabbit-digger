use std::{
    future::pending,
    io,
    net::SocketAddr,
    task::{self, Poll},
};

use crate::ContextExt;
use futures::ready;
use rd_interface::{
    async_trait, config::NetRef, prelude::*, registry::Builder, Address, Context, IServer,
    IUdpChannel, IntoDyn, Net, Result, Server, TcpListener, TcpStream, UdpSocket,
};
use tokio::{
    select,
    time::{sleep, Duration},
};
use tracing::instrument;

/// A server that forwards all connections to target.
#[rd_config]
#[derive(Debug)]
pub struct ForwardServerConfig {
    bind: Address,
    target: Address,
    #[serde(default)]
    udp: bool,
    /// must be IP:PORT
    #[serde(default)]
    udp_bind: Option<Address>,
    #[serde(default)]
    net: NetRef,
    #[serde(default)]
    listen: NetRef,
}

pub struct ForwardServer {
    listen_net: Net,
    net: Net,
    bind: Address,
    udp_bind: Option<Address>,
    target: Address,
    udp: bool,
}

impl ForwardServer {
    fn new(cfg: ForwardServerConfig) -> ForwardServer {
        ForwardServer {
            listen_net: cfg.listen.value_cloned(),
            net: cfg.net.value_cloned(),
            bind: cfg.bind,
            udp_bind: cfg.udp_bind,
            target: cfg.target,
            udp: cfg.udp,
        }
    }
}
#[async_trait]
impl IServer for ForwardServer {
    async fn start(&self) -> Result<()> {
        let listener = self
            .listen_net
            .tcp_bind(&mut Context::new(), &self.bind)
            .await?;

        let tcp_task = self.serve_listener(listener);
        let udp_task = self.serve_udp();

        select! {
            r = tcp_task => r?,
            r = udp_task => r?,
        }

        Ok(())
    }
}

impl ForwardServer {
    #[instrument(err, skip(net, socket))]
    async fn serve_connection(
        target: Address,
        socket: TcpStream,
        net: Net,
        addr: SocketAddr,
    ) -> Result<()> {
        let ctx = &mut Context::from_socketaddr(addr);
        let target = net.tcp_connect(ctx, &target).await?;
        ctx.connect_tcp(socket, target).await?;
        Ok(())
    }
    pub async fn serve_listener(&self, listener: TcpListener) -> Result<()> {
        loop {
            let (socket, addr) = listener.accept().await?;
            let net = self.net.clone();
            let target = self.target.clone();
            let _ = tokio::spawn(async move {
                if let Err(e) = Self::serve_connection(target, socket, net, addr).await {
                    tracing::error!("Error when serve_connection: {:?}", e);
                }
            });
        }
    }
    async fn serve_udp(&self) -> Result<()> {
        if !self.udp {
            pending::<()>().await;
        }

        loop {
            let udp_listener = ListenUdpChannel {
                udp: self
                    .listen_net
                    .udp_bind(&mut Context::new(), &self.bind)
                    .await?,
                client: None,
                target: self.target.clone(),
            }
            .into_dyn();

            let mut ctx = Context::new();
            let bind_addr = self
                .udp_bind
                .clone()
                .map(Ok)
                .unwrap_or_else(|| self.target.to_any_addr_port())?;
            let udp = self.net.udp_bind(&mut ctx, &bind_addr).await?;

            if let Err(e) = ctx.connect_udp(udp_listener, udp).await {
                tracing::error!("udp failed, retry after 3s: {:?}", e);
                sleep(Duration::from_secs(3)).await;
            }
        }
    }
}

impl Builder<Server> for ForwardServer {
    const NAME: &'static str = "forward";
    type Config = ForwardServerConfig;
    type Item = Self;

    fn build(cfg: Self::Config) -> Result<Self> {
        Ok(ForwardServer::new(cfg))
    }
}

struct ListenUdpChannel {
    udp: UdpSocket,
    client: Option<SocketAddr>,
    target: Address,
}

impl IUdpChannel for ListenUdpChannel {
    fn poll_send_to(
        &mut self,
        cx: &mut task::Context<'_>,
        buf: &mut rd_interface::ReadBuf,
    ) -> Poll<io::Result<Address>> {
        let addr = ready!(self.udp.poll_recv_from(cx, buf))?;
        self.client = Some(addr);
        Poll::Ready(Ok(self.target.clone()))
    }

    fn poll_recv_from(
        &mut self,
        cx: &mut task::Context<'_>,
        buf: &[u8],
        _: &SocketAddr,
    ) -> Poll<io::Result<usize>> {
        if let Some(client) = self.client {
            self.udp.poll_send_to(cx, buf, &client.into())
        } else {
            Poll::Ready(Ok(0))
        }
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use rd_interface::{IntoAddress, IntoDyn};
    use tokio::time::sleep;

    use super::*;
    use crate::tests::{
        assert_echo, assert_echo_udp, spawn_echo_server, spawn_echo_server_udp, TestNet,
    };

    #[tokio::test]
    async fn test_forward_server() {
        let net = TestNet::new().into_dyn();

        let server = ForwardServer {
            listen_net: net.clone(),
            net: net.clone(),
            bind: "127.0.0.1:1234".into_address().unwrap(),
            udp_bind: None,
            target: "127.0.0.1:4321".into_address().unwrap(),
            udp: true,
        };
        tokio::spawn(async move { server.start().await.unwrap() });
        spawn_echo_server(&net, "127.0.0.1:4321").await;
        spawn_echo_server_udp(&net, "127.0.0.1:4321").await;

        sleep(Duration::from_millis(1)).await;

        assert_echo(&net, "127.0.0.1:1234").await;
        assert_echo_udp(&net, "127.0.0.1:1234").await;
    }
}
