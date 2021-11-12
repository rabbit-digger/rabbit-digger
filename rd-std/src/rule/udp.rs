use std::{
    net::SocketAddr,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};

use super::rule_net::Rule;
use futures::{Sink, SinkExt, Stream, StreamExt};
use lru_time_cache::LruCache;
use rd_interface::{
    async_trait, error::map_other, Address, Bytes, BytesMut, Context, IUdpSocket, Net, Result,
    NOT_IMPLEMENTED,
};
use tokio::{
    select,
    sync::{
        mpsc::{unbounded_channel, UnboundedReceiver as Receiver, UnboundedSender as Sender},
        Mutex,
    },
    task::spawn,
    time::timeout,
};

type UdpPacket = (Bytes, SocketAddr);
type NatTable = parking_lot::Mutex<LruCache<String, UdpTunnel>>;

pub struct UdpRuleSocket {
    rule: Rule,
    context: Context,
    nat: NatTable,
    tx: Sender<UdpPacket>,
    rx: Mutex<Receiver<UdpPacket>>,
    bind_addr: Address,
}

struct UdpTunnel(Sender<(Bytes, SocketAddr)>);

impl UdpTunnel {
    fn new(
        net: Net,
        mut context: Context,
        bind_addr: Address,
        send_back: Sender<UdpPacket>,
    ) -> UdpTunnel {
        let (tx, mut rx) = unbounded_channel::<(Bytes, SocketAddr)>();
        spawn(async move {
            // TODO: udp

            // let udp = timeout(
            //     Duration::from_secs(5),
            //     net.udp_bind(&mut context, &bind_addr),
            // )
            // .await
            // .map_err(map_other)??;

            // let send = async {
            //     while let Some((packet, addr)) = rx.recv().await {
            //         if let Err(e) = udp.send((packet, addr)).await {
            //             tracing::error!("drop packet: {:?}", e);
            //         }
            //     }

            //     anyhow::Result::<()>::Ok(())
            // };
            // let recv = async {
            //     while let Some(r) = udp.next().await {
            //         let (buf, addr) = r?;

            //         if send_back.send((buf.freeze(), addr)).is_err() {
            //             break;
            //         }
            //     }
            //     tracing::trace!("send_raw return error");
            //     anyhow::Result::<()>::Ok(())
            // };

            // select! {
            //     r = send => r?,
            //     r = recv => r?,
            // }

            anyhow::Result::<()>::Ok(())
        });
        UdpTunnel(tx)
    }
    fn send_to(&self, buf: Bytes, addr: SocketAddr) -> Result<usize> {
        let len = buf.len();
        match self.0.send((buf, addr)) {
            Err(_) => Err(rd_interface::Error::Other("Other side closed".into())),
            Ok(_) => Ok(len),
        }
    }
}

impl UdpRuleSocket {
    pub fn new(rule: Rule, context: Context, bind_addr: Address) -> UdpRuleSocket {
        let (tx, rx) = unbounded_channel::<UdpPacket>();
        let nat: NatTable = parking_lot::Mutex::new(LruCache::with_expiry_duration_and_capacity(
            Duration::from_secs(30),
            128,
        ));

        UdpRuleSocket {
            rule,
            context,
            nat,
            tx,
            rx: Mutex::new(rx),
            bind_addr,
        }
    }
    async fn get_net_name(&self, ctx: &Context, addr: &Address) -> Result<(&Net, &String)> {
        let rule_item = self.rule.get_rule(ctx, addr).await?;
        Ok((&rule_item.target, &rule_item.target_name))
    }
}

impl Stream for UdpRuleSocket {
    type Item = std::io::Result<(BytesMut, SocketAddr)>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        todo!()
    }
}

impl Sink<(Bytes, SocketAddr)> for UdpRuleSocket {
    type Error = std::io::Error;

    fn poll_ready(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn start_send(
        self: Pin<&mut Self>,
        (buf, addr): (Bytes, SocketAddr),
    ) -> Result<(), Self::Error> {
        todo!()
    }

    fn poll_flush(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }

    fn poll_close(
        self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        Poll::Ready(Ok(()))
    }
}

#[async_trait]
impl IUdpSocket for UdpRuleSocket {
    // async fn recv_from(&self, buf: &mut [u8]) -> Result<(usize, SocketAddr)> {
    //     let (data, addr) = self
    //         .rx
    //         .lock()
    //         .await
    //         .recv()
    //         .await
    //         .ok_or_else(|| rd_interface::Error::Other("Failed to receive UDP".into()))?;

    //     let to_copy = data.len().min(buf.len());
    //     buf[..to_copy].copy_from_slice(&data[..to_copy]);

    //     Ok((to_copy, addr))
    // }

    // async fn send_to(&self, buf: &[u8], addr: Address) -> Result<usize> {
    //     let (net, out_net) = self.get_net_name(&self.context, &addr).await?;
    //     let mut nat = self.nat.lock();

    //     let udp = nat.entry(out_net.to_string()).or_insert_with(|| {
    //         UdpTunnel::new(
    //             net.clone(),
    //             self.context.clone(),
    //             self.bind_addr.clone(),
    //             self.tx.clone(),
    //         )
    //     });

    //     udp.send_to(buf, addr)
    // }

    async fn local_addr(&self) -> Result<SocketAddr> {
        Err(NOT_IMPLEMENTED)
    }
}
