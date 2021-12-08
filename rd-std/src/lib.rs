use rd_interface::{Registry, Result};

pub mod builtin;
pub mod http;
pub mod mixed;
pub mod rule;
pub mod sniffer;
pub mod socks5;
pub mod transparent;
pub mod util;

pub fn init(registry: &mut Registry) -> Result<()> {
    builtin::init(registry)?;
    sniffer::init(registry)?;
    http::init(registry)?;
    mixed::init(registry)?;
    transparent::init(registry)?;
    rule::init(registry)?;
    socks5::init(registry)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use crate::builtin;
    use rd_interface::{Context, IntoAddress, Net, Registry};
    use tokio::io::{self, AsyncReadExt, AsyncWriteExt};

    pub fn get_registry() -> Registry {
        let mut registry = Registry::new();
        builtin::init(&mut registry).unwrap();
        registry
    }

    pub async fn spawn_echo_server(net: &Net, addr: impl IntoAddress) {
        let listener = net
            .tcp_bind(&mut Context::new(), &addr.into_address().unwrap())
            .await
            .unwrap();
        tokio::spawn(async move {
            loop {
                let (tcp, _) = listener.accept().await.unwrap();
                tokio::spawn(async move {
                    let (mut rx, mut tx) = io::split(tcp);
                    io::copy(&mut rx, &mut tx).await.unwrap();
                });
            }
        });
    }

    pub async fn assert_echo(net: &Net, addr: impl IntoAddress) {
        const BUF: &'static [u8] = b"asdfasdfasdfasj12312313123";
        let mut tcp = net
            .tcp_connect(&mut Context::new(), &addr.into_address().unwrap())
            .await
            .unwrap();
        tcp.write_all(&BUF).await.unwrap();

        let mut buf = [0u8; BUF.len()];
        tcp.read_exact(&mut buf).await.unwrap();

        assert_eq!(buf, BUF);
    }
}
