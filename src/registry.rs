//! A registry with plugin name

use anyhow::{anyhow, Result};
use rd_interface::{
    config::Value,
    registry::{NetFromConfig, ServerFromConfig},
    Arc, Net, NotImplementedNet, Server,
};
use std::{collections::HashMap, fmt};

pub struct NetItem {
    pub plugin_name: String,
    pub factory: NetFromConfig<Net>,
}

pub struct ServerItem {
    pub plugin_name: String,
    pub factory: ServerFromConfig<Server>,
}

impl NetItem {
    pub fn build(&self, net: impl Into<Option<Net>>, config: Value) -> rd_interface::Result<Net> {
        let net = net.into();

        (self.factory)(net.unwrap_or_else(|| Arc::new(NotImplementedNet)), config)
    }
}

impl ServerItem {
    pub fn build(&self, listen_net: Net, net: Net, config: Value) -> rd_interface::Result<Server> {
        (self.factory)(listen_net, net, config)
    }
}

pub struct Registry {
    pub net: HashMap<String, NetItem>,
    pub server: HashMap<String, ServerItem>,
}

impl fmt::Debug for NetItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("NetItem")
            .field("plugin_name", &self.plugin_name)
            .finish()
    }
}

impl fmt::Debug for ServerItem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ServerItem")
            .field("plugin_name", &self.plugin_name)
            .finish()
    }
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("net", &self.net)
            .field("server", &self.server)
            .finish()
    }
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            net: HashMap::new(),
            server: HashMap::new(),
        }
    }
    pub fn add_registry(&mut self, plugin_name: String, registry: rd_interface::Registry) {
        for (k, v) in registry.net {
            self.net.insert(
                k,
                NetItem {
                    plugin_name: plugin_name.clone(),
                    factory: v,
                },
            );
        }
        for (k, v) in registry.server {
            self.server.insert(
                k,
                ServerItem {
                    plugin_name: plugin_name.clone(),
                    factory: v,
                },
            );
        }
    }
    pub fn get_net(&self, net_type: &str) -> Result<&NetItem> {
        self.net
            .get(net_type)
            .ok_or(anyhow!("Net type is not loaded: {}", net_type))
    }
    pub fn get_server(&self, server_type: &str) -> Result<&ServerItem> {
        self.server
            .get(server_type)
            .ok_or(anyhow!("Server type is not loaded: {}", server_type))
    }
}