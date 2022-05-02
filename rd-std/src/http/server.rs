use hyper::{
    client::conn as client_conn, http, server::conn as server_conn, service::service_fn, Body,
    Method, Request, Response,
};
use parking_lot::Mutex;
use rd_interface::{
    async_trait, Address, AsyncRead, AsyncWrite, Context, IServer, ITcpStream, IntoAddress,
    IntoDyn, Net, ReadBuf, Result, TcpStream,
};
use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{self, Poll},
};
use tracing::instrument;

use crate::{context::AcceptCommand, ContextExt};

#[derive(Clone)]
pub struct HttpServer {
    net: Net,
}

impl HttpServer {
    #[instrument(err, skip(self, socket))]
    pub async fn serve_connection(self, socket: TcpStream, addr: SocketAddr) -> anyhow::Result<()> {
        let net = self.net.clone();

        server_conn::Http::new()
            .http1_preserve_header_case(true)
            .http1_title_case_headers(true)
            .http1_keep_alive(true)
            .serve_connection(socket, service_fn(move |req| proxy(net.clone(), req, addr)))
            .with_upgrades()
            .await?;

        Ok(())
    }
    pub fn new(net: Net) -> Self {
        Self { net }
    }
}

pub struct Http {
    server: HttpServer,
    listen_net: Net,
    bind: Address,
}

#[async_trait]
impl IServer for Http {
    async fn start(&self) -> Result<()> {
        let listener = self
            .listen_net
            .tcp_bind(&mut Context::new(), &self.bind)
            .await?;

        loop {
            let (socket, addr) = listener.accept().await?;
            let server = self.server.clone();
            tokio::spawn(async move {
                if let Err(e) = server.serve_connection(socket, addr).await {
                    tracing::error!("Error when serve_connection: {:?}", e);
                }
            });
        }
    }
}

impl Http {
    pub fn new(listen_net: Net, net: Net, bind: Address) -> Self {
        Http {
            server: HttpServer::new(net),
            listen_net,
            bind,
        }
    }
}

async fn proxy(net: Net, req: Request<Body>, addr: SocketAddr) -> anyhow::Result<Response<Body>> {
    if let Some(mut dst) = host_addr(req.uri()) {
        if !dst.contains(':') {
            dst += ":80"
        }
        let dst = dst.into_address()?;

        if req.method() == Method::CONNECT {
            tokio::spawn(async move {
                match hyper::upgrade::on(req).await {
                    Ok(upgraded) => {
                        if let Err(e) = Context::from_socketaddr(addr)
                            .accept(
                                async move {
                                    Ok(AcceptCommand::TcpConnect(
                                        dst,
                                        Wrapper(Mutex::new(upgraded)).into_dyn().into(),
                                    ))
                                },
                                &net,
                            )
                            .await
                        {
                            tracing::debug!("tunnel io error: {}", e);
                        };
                    }
                    Err(e) => tracing::debug!("upgrade error: {}", e),
                }
                Ok(()) as anyhow::Result<()>
            });

            Ok(Response::new(Body::empty()))
        } else {
            let stream = net
                .tcp_connect(&mut Context::from_socketaddr(addr), &dst)
                .await?;

            let (mut request_sender, connection) = client_conn::Builder::new()
                .http1_preserve_header_case(true)
                .http1_title_case_headers(true)
                .handshake(stream)
                .await?;

            tokio::spawn(connection);

            let resp = request_sender.send_request(req).await?;

            Ok(resp)
        }
    } else {
        tracing::error!("host is not socket addr: {:?}", req.uri());
        let mut resp = Response::new(Body::from("CONNECT must be to a socket address"));
        *resp.status_mut() = http::StatusCode::BAD_REQUEST;

        Ok(resp)
    }
}

fn host_addr(uri: &http::Uri) -> Option<String> {
    uri.authority().map(|auth| auth.to_string())
}

struct Wrapper<T>(Mutex<T>);

#[async_trait]
impl<T> ITcpStream for Wrapper<T>
where
    T: AsyncRead + AsyncWrite + Send + Unpin,
{
    fn poll_read(
        &mut self,
        cx: &mut task::Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0.get_mut()).poll_read(cx, buf)
    }

    fn poll_write(&mut self, cx: &mut task::Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.0.get_mut()).poll_write(cx, buf)
    }

    fn poll_flush(&mut self, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0.get_mut()).poll_flush(cx)
    }

    fn poll_shutdown(&mut self, cx: &mut task::Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.0.get_mut()).poll_shutdown(cx)
    }

    async fn peer_addr(&self) -> Result<SocketAddr> {
        Err(rd_interface::Error::NotImplemented)
    }

    async fn local_addr(&self) -> Result<SocketAddr> {
        Err(rd_interface::Error::NotImplemented)
    }
}
