use rd_interface::{
    async_trait,
    prelude::*,
    registry::{NetFactory, NetRef},
    Address, Context, INet, Net, Result, TcpListener, TcpStream, UdpSocket,
};

pub struct CombineNet {
    tcp_connect: Net,
    tcp_bind: Net,
    udp_bind: Net,
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
}

#[rd_config]
#[derive(Debug)]
pub struct CombineNetConfig {
    tcp_connect: NetRef,
    tcp_bind: NetRef,
    udp_bind: NetRef,
}

impl NetFactory for CombineNet {
    const NAME: &'static str = "combine";
    type Config = CombineNetConfig;
    type Net = Self;

    fn new(
        CombineNetConfig {
            tcp_connect,
            tcp_bind,
            udp_bind,
        }: Self::Config,
    ) -> Result<Self> {
        Ok(CombineNet {
            tcp_connect: tcp_connect.net(),
            tcp_bind: tcp_bind.net(),
            udp_bind: udp_bind.net(),
        })
    }
}
