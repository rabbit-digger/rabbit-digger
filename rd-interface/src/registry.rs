use std::{collections::HashMap, fmt};

use crate::{config::Value, BoxProxyNet, Result};

pub type NetFromConfig<T> = Box<dyn Fn(BoxProxyNet, Value) -> Result<T>>;
pub enum Plugin {
    Net(NetFromConfig<BoxProxyNet>),
}

pub struct Registry {
    pub net: HashMap<String, NetFromConfig<BoxProxyNet>>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("net", &self.net.keys())
            .finish()
    }
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            net: HashMap::new(),
        }
    }
    pub fn add_plugin(&mut self, name: impl Into<String>, plugin: Plugin) {
        match plugin {
            Plugin::Net(net) => {
                self.net.insert(name.into(), net);
            }
        }
    }
}