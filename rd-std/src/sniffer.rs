pub use dns_sniffer::DNSSnifferNet;
use rd_interface::{
    prelude::*,
    rd_config,
    registry::{Builder, NetRef},
    Net, Registry, Result,
};

mod dns_sniffer;
mod service;

#[rd_config]
#[derive(Debug)]
pub struct DNSNetConfig {
    #[serde(default)]
    net: NetRef,
}

impl Builder<Net> for DNSSnifferNet {
    const NAME: &'static str = "dns_sniffer";
    type Config = DNSNetConfig;
    type Item = Self;

    fn build(config: Self::Config) -> Result<Self> {
        Ok(DNSSnifferNet::new(config.net.value_cloned()))
    }
}

pub fn init(registry: &mut Registry) -> Result<()> {
    registry.add_net::<DNSSnifferNet>();
    Ok(())
}
