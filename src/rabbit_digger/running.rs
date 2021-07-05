use std::mem;

use rd_interface::{
    async_trait, Address, Arc, Context, INet, Net, Result, TcpListener, TcpStream, UdpSocket,
};
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RunningNet {
    name: String,
    inner: Arc<RwLock<Net>>,
}

impl RunningNet {
    pub fn new(name: String, net: Net) -> RunningNet {
        RunningNet {
            name,
            inner: Arc::new(RwLock::new(net)),
        }
    }
    pub async fn replace(&self, net: Net) -> Net {
        mem::replace(&mut *self.inner.write().await, net)
    }
}

#[async_trait]
impl INet for RunningNet {
    async fn tcp_connect(&self, ctx: &mut Context, addr: &Address) -> Result<TcpStream> {
        self.inner.read().await.tcp_connect(ctx, addr).await
    }

    async fn tcp_bind(&self, ctx: &mut Context, addr: &Address) -> Result<TcpListener> {
        self.inner.read().await.tcp_bind(ctx, addr).await
    }

    async fn udp_bind(&self, ctx: &mut Context, addr: &Address) -> Result<UdpSocket> {
        self.inner.read().await.udp_bind(ctx, addr).await
    }
}
