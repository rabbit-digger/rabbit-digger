use std::{
    io,
    net::SocketAddr,
    pin::Pin,
    task::{self, Poll},
    time::Duration,
};

use super::rule_net::Rule;
use futures::{ready, Future, FutureExt, Sink, SinkExt, Stream, StreamExt};
use parking_lot::Mutex;
use rd_interface::{
    async_trait, Address, Bytes, BytesMut, Context, IUdpSocket, Result, UdpSocket, NOT_IMPLEMENTED,
};
use tokio::{pin, sync::Notify, time::timeout};

type BoxFuture<T> = Pin<Box<dyn Future<Output = T> + Send + 'static>>;

enum State {
    Idle {
        rule: Rule,
        context: Context,
        bind_addr: Address,
    },
    Binding(Mutex<BoxFuture<Result<UdpSocket>>>),
    Binded(UdpSocket),
}

pub struct UdpRuleSocket {
    notify: Notify,
    state: State,
}

impl UdpRuleSocket {
    pub fn new(rule: Rule, context: Context, bind_addr: Address) -> UdpRuleSocket {
        UdpRuleSocket {
            notify: Notify::new(),
            state: State::Idle {
                rule,
                context,
                bind_addr,
            },
        }
    }
}

impl Stream for UdpRuleSocket {
    type Item = std::io::Result<(BytesMut, SocketAddr)>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut task::Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match &mut self.state {
                State::Binded(udp) => return udp.poll_next_unpin(cx),
                State::Idle { .. } | State::Binding(_) => {
                    let n = self.notify.notified();
                    pin!(n);
                    ready!(n.poll(cx));
                }
            }
        }
    }
}

async fn send_first_packet(
    mut ctx: Context,
    bind_addr: Address,
    (bytes, addr): (Bytes, SocketAddr),
    rule: Rule,
) -> Result<UdpSocket> {
    let target = addr.into();
    let rule_item = rule.get_rule(&ctx, &target).await?;

    let mut udp = timeout(
        Duration::from_secs(5),
        rule_item.target.udp_bind(&mut ctx, &bind_addr),
    )
    .await??;
    udp.send((bytes, addr)).await?;

    Ok(udp)
}

impl Sink<(Bytes, SocketAddr)> for UdpRuleSocket {
    type Error = std::io::Error;

    fn poll_ready(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match &mut self.state {
            State::Binded(udp) => udp.poll_ready_unpin(cx),
            State::Idle { .. } => Poll::Ready(Ok(())),
            State::Binding(fut) => {
                let udp = ready!(fut.lock().poll_unpin(cx));
                self.state = State::Binded(udp?);
                self.notify.notify_one();
                Poll::Ready(Ok(()))
            }
        }
    }

    fn start_send(mut self: Pin<&mut Self>, item: (Bytes, SocketAddr)) -> Result<(), Self::Error> {
        match &mut self.state {
            State::Binded(udp) => udp.start_send_unpin(item),
            State::Idle {
                context,
                bind_addr,
                rule,
            } => {
                // TODO: more efficient, remove clone
                self.state = State::Binding(Mutex::new(Box::pin(send_first_packet(
                    context.clone(),
                    bind_addr.clone(),
                    item,
                    rule.clone(),
                ))));
                Ok(())
            }
            State::Binding(_) => Err(io::Error::new(
                io::ErrorKind::Other,
                "start_send called twice",
            )),
        }
    }

    fn poll_flush(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match &mut self.state {
            State::Binded(udp) => udp.poll_flush_unpin(cx),
            State::Idle { .. } | State::Binding(_) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::Other,
                "rule udp not ready",
            ))),
        }
    }

    fn poll_close(
        mut self: Pin<&mut Self>,
        cx: &mut task::Context<'_>,
    ) -> Poll<Result<(), Self::Error>> {
        match &mut self.state {
            State::Binded(udp) => udp.poll_close_unpin(cx),
            State::Idle { .. } | State::Binding(_) => Poll::Ready(Ok(())),
        }
    }
}

#[async_trait]
impl IUdpSocket for UdpRuleSocket {
    async fn local_addr(&self) -> Result<SocketAddr> {
        Err(NOT_IMPLEMENTED)
    }
}
