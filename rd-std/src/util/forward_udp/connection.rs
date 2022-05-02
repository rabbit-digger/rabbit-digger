use std::{io, net::SocketAddr};

use crate::{context::AcceptCommand, ContextExt};

use super::{send_back::BackChannel, UdpPacket};
use rd_interface::{Address, Context, IntoDyn, Net, Result};
use tokio::{
    sync::mpsc::{channel, Sender},
    task::JoinHandle,
};

pub(super) struct UdpConnection {
    handle: JoinHandle<Result<()>>,
    send_udp: Sender<(Vec<u8>, SocketAddr)>,
}

impl UdpConnection {
    pub(super) fn new(
        net: Net,
        bind_from: SocketAddr,
        bind_addr: Address,
        send_back: Sender<UdpPacket>,
        channel_size: usize,
    ) -> UdpConnection {
        let (send_udp, rx) = channel(channel_size);
        let back_channel = BackChannel::new(bind_from, send_back, rx).into_dyn();
        let fut = async move {
            Context::from_socketaddr(bind_from)
                .accept(
                    async move { Ok(AcceptCommand::UdpBind(bind_addr, back_channel.into())) },
                    &net,
                )
                .await?;

            Ok(())
        };

        UdpConnection {
            handle: tokio::spawn(fut),
            send_udp,
        }
    }
    pub(super) fn send(&mut self, packet: (Vec<u8>, SocketAddr)) -> Result<()> {
        self.send_udp
            .try_send(packet)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e).into())
    }
}

impl Drop for UdpConnection {
    fn drop(&mut self) {
        self.handle.abort();
    }
}
