use std::{
    io,
    mem::replace,
    net::SocketAddr,
    pin::Pin,
    task::{self, Poll},
};

use rd_interface::{
    async_trait, Address, Arc, AsyncRead, AsyncWrite, Context, INet, IntoDyn, Net, ReadBuf, Result,
    TcpListener, TcpStream, UdpSocket, Value,
};
use tokio::{
    sync::{RwLock, Semaphore},
    task::JoinHandle,
};

use crate::Registry;

use super::{
    connection::{Connection, ConnectionConfig},
    event::EventType,
};

#[derive(Clone)]
pub struct RunningNet {
    name: String,
    opt: Value,
    inner: Arc<RwLock<Net>>,
}

impl RunningNet {
    pub fn new(name: String, opt: Value, net: Net) -> RunningNet {
        RunningNet {
            name,
            opt,
            inner: Arc::new(RwLock::new(net)),
        }
    }
    pub async fn net(&self) -> Net {
        self.inner.read().await.clone()
    }
}

#[async_trait]
impl INet for RunningNet {
    async fn tcp_connect(&self, ctx: &mut Context, addr: &Address) -> Result<TcpStream> {
        ctx.append_net(&self.name);
        self.inner.read().await.tcp_connect(ctx, addr).await
    }

    async fn tcp_bind(&self, ctx: &mut Context, addr: &Address) -> Result<TcpListener> {
        self.inner.read().await.tcp_bind(ctx, addr).await
    }

    async fn udp_bind(&self, ctx: &mut Context, addr: &Address) -> Result<UdpSocket> {
        self.inner.read().await.udp_bind(ctx, addr).await
    }
}

pub struct RunningServerNet {
    net: Net,
    config: ConnectionConfig,
}

impl RunningServerNet {
    pub fn new(net: Net, config: ConnectionConfig) -> RunningServerNet {
        RunningServerNet { net, config }
    }
}

#[async_trait]
impl INet for RunningServerNet {
    async fn tcp_connect(
        &self,
        ctx: &mut rd_interface::Context,
        addr: &Address,
    ) -> rd_interface::Result<TcpStream> {
        let src = ctx
            .get_source_addr()
            .map(|s| s.to_string())
            .unwrap_or_default();

        tracing::info!("{:?} {} -> {}", &ctx.net_list(), &src, &addr,);

        let tcp = self.net.tcp_connect(ctx, &addr).await?;
        let tcp = WrapTcpStream::new(tcp, self.config.clone(), addr.clone());
        Ok(tcp.into_dyn())
    }

    // TODO: wrap TcpListener
    async fn tcp_bind(
        &self,
        ctx: &mut rd_interface::Context,
        addr: &Address,
    ) -> rd_interface::Result<TcpListener> {
        self.net.tcp_bind(ctx, addr).await
    }

    // TODO: wrap UdpSocket
    async fn udp_bind(
        &self,
        ctx: &mut rd_interface::Context,
        addr: &Address,
    ) -> rd_interface::Result<UdpSocket> {
        self.net.udp_bind(ctx, addr).await
    }
}

pub struct WrapTcpStream {
    inner: TcpStream,
    conn: Connection,
}

impl WrapTcpStream {
    pub fn new(inner: TcpStream, config: ConnectionConfig, addr: Address) -> WrapTcpStream {
        WrapTcpStream {
            inner,
            conn: Connection::new(config, EventType::NewTcp(addr)),
        }
    }
}

impl AsyncRead for WrapTcpStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf,
    ) -> Poll<io::Result<()>> {
        let before = buf.filled().len();
        match Pin::new(&mut self.inner).poll_read(cx, buf) {
            Poll::Ready(Ok(())) => {
                let s = buf.filled().len() - before;
                self.conn.send(EventType::Inbound(s));
                Ok(()).into()
            }
            r => r,
        }
    }
}

impl AsyncWrite for WrapTcpStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buf) {
            Poll::Ready(Ok(s)) => {
                self.conn.send(EventType::Outbound(s));
                Ok(s).into()
            }
            r => r,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<Result<usize, io::Error>> {
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[async_trait]
impl rd_interface::ITcpStream for WrapTcpStream {
    async fn peer_addr(&self) -> Result<SocketAddr> {
        self.inner.peer_addr().await
    }

    async fn local_addr(&self) -> Result<SocketAddr> {
        self.inner.local_addr().await
    }
}

enum State {
    WaitConfig,
    Running {
        opt: Value,
        handle: JoinHandle<anyhow::Result<()>>,
        semaphore: Arc<Semaphore>,
    },
    Finished {
        result: anyhow::Result<()>,
    },
}

#[derive(Clone)]
pub struct RunningServer {
    name: String,
    server_type: String,
    net: Net,
    listen: Net,
    state: Arc<RwLock<State>>,
}

impl RunningServer {
    pub fn new(name: String, server_type: String, net: Net, listen: Net) -> Self {
        RunningServer {
            name,
            server_type,
            net,
            listen,
            state: Arc::new(RwLock::new(State::WaitConfig)),
        }
    }
    pub async fn start(&self, registry: &Registry, opt: &Value) -> anyhow::Result<()> {
        match &*self.state.read().await {
            // skip if config is not changed
            State::Running {
                opt: running_opt, ..
            } => {
                if opt == running_opt {
                    return Ok(());
                }
            }
            _ => {}
        };

        self.stop().await?;

        let item = registry.get_server(&self.server_type)?;
        let server = item.build(self.listen.clone(), self.net.clone(), opt.clone())?;
        let semaphore = Arc::new(Semaphore::new(0));
        let s2 = semaphore.clone();
        let handle = tokio::spawn(async move {
            let r = server.start().await.map_err(Into::into);
            s2.close();
            r
        });

        *self.state.write().await = State::Running {
            opt: opt.clone(),
            handle,
            semaphore,
        };

        Ok(())
    }
    pub async fn stop(&self) -> anyhow::Result<()> {
        match &*self.state.read().await {
            State::Running {
                handle, semaphore, ..
            } => {
                handle.abort();
                semaphore.close();
            }
            _ => return Ok(()),
        };
        self.join().await;

        Ok(())
    }
    pub async fn join(&self) {
        let state = self.state.read().await;

        match &*state {
            State::Running { semaphore, .. } => {
                let _ = semaphore.acquire().await;
            }
            _ => return,
        };
        drop(state);

        let mut state = self.state.write().await;

        let result = match &mut *state {
            State::Running { handle, .. } => handle.await.map_err(Into::into).and_then(|i| i),
            _ => return,
        };

        *state = State::Finished { result };
    }
    pub async fn take_result(&self) -> Option<anyhow::Result<()>> {
        let mut state = self.state.write().await;

        match &*state {
            State::Finished { .. } => {
                let old = replace(&mut *state, State::WaitConfig);
                return match old {
                    State::Finished { result, .. } => Some(result),
                    _ => unreachable!(),
                };
            }
            _ => return None,
        };
    }
}