use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};

use super::UdpConnector;
use futures::{Future, Sink, Stream, StreamExt};
use lru_time_cache::LruCache;
use rd_interface::{Address, Bytes, BytesMut, Context, Net};

// Stream: (data, from, to)
// Sink: (data, to, from)
pub trait RawUdpSource:
    Stream<Item = io::Result<(Bytes, SocketAddr, SocketAddr)>>
    + Sink<(BytesMut, SocketAddr, SocketAddr), Error = io::Error>
    + Unpin
    + Send
    + Sync
{
}

struct ForwardUdp<S> {
    s: S,
    net: Net,
    conn: LruCache<SocketAddr, UdpConnector>,
}

impl<S> ForwardUdp<S>
where
    S: RawUdpSource,
{
    fn new(s: S, net: Net) -> Self {
        ForwardUdp {
            s,
            net,
            conn: LruCache::with_expiry_duration_and_capacity(Duration::from_secs(30), 256),
        }
    }
}

impl<S> ForwardUdp<S> {
    fn get(&mut self, from: SocketAddr) -> &mut UdpConnector {
        let net = &self.net;
        self.conn.entry(from).or_insert_with(|| {
            let net = net.clone();
            UdpConnector::new(Box::new(move |item: &(Bytes, SocketAddr)| {
                let target_addr: Address = item.1.into();
                Box::pin(async move {
                    net.udp_bind(
                        &mut Context::from_socketaddr(from),
                        &target_addr.to_any_addr_port()?,
                    )
                    .await
                })
            }))
        })
    }
}

impl<S> Future for ForwardUdp<S>
where
    S: RawUdpSource,
{
    type Output = io::Result<()>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> task::Poll<Self::Output> {
        match self.s.poll_next_unpin(cx) {
            Poll::Ready(Some(result)) => {
                let (data, from, to) = result?;
                let mut udp = self.get(from);
                udp.poll_next_unpin(cx)
            }
            Poll::Ready(None) => return Poll::Ready(Ok(())),
            Poll::Pending => return Poll::Pending,
        };

        todo!()
    }
}

pub async fn forward_udp<S>(s: S, net: Net) -> io::Result<()>
where
    S: RawUdpSource,
{
    ForwardUdp::new(s, net).await
}
