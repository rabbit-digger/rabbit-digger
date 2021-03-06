use std::{collections::BTreeMap, fmt};

pub use self::net_ref::NetRef;
use crate::{config::Config, INet, IServer, IntoDyn, Net, Result, Server};
pub use schemars::JsonSchema;
use schemars::{schema::RootSchema, schema_for};
use serde::de::DeserializeOwned;
use serde_json::Value;

pub type NetMap = BTreeMap<String, Net>;

mod net_ref;

pub struct Registry {
    pub net: BTreeMap<String, NetResolver>,
    pub server: BTreeMap<String, ServerResolver>,
}

impl fmt::Debug for Registry {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Registry")
            .field("net", &self.net.keys())
            .field("server", &self.server.keys())
            .finish()
    }
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    pub fn new() -> Registry {
        Registry {
            net: BTreeMap::new(),
            server: BTreeMap::new(),
        }
    }
    pub fn add_net<N: NetFactory>(&mut self) {
        self.net.insert(N::NAME.into(), NetResolver::new::<N>());
    }
    pub fn add_server<S: ServerFactory>(&mut self) {
        self.server
            .insert(S::NAME.into(), ServerResolver::new::<S>());
    }
}

pub trait NetFactory {
    const NAME: &'static str;
    type Config: DeserializeOwned + JsonSchema + Config;
    type Net: INet + Sized + 'static;

    fn new(config: Self::Config) -> Result<Self::Net>;
}

pub struct NetResolver {
    build: fn(nets: &NetMap, cfg: Value) -> Result<Net>,
    get_dependency: fn(cfg: Value) -> Result<Vec<String>>,
    schema: RootSchema,
}

impl NetResolver {
    fn new<N: NetFactory>() -> Self {
        let schema = schema_for!(N::Config);
        Self {
            build: |nets, cfg| {
                serde_json::from_value(cfg)
                    .map_err(Into::<crate::Error>::into)
                    .and_then(|mut cfg: N::Config| {
                        cfg.resolve_net(nets)?;
                        Ok(cfg)
                    })
                    .and_then(N::new)
                    .map(|n| n.into_dyn())
            },
            get_dependency: |cfg| {
                serde_json::from_value(cfg)
                    .map_err(Into::<crate::Error>::into)
                    .and_then(|mut cfg: N::Config| cfg.get_dependency())
            },
            schema,
        }
    }
    pub fn build(&self, nets: &NetMap, cfg: Value) -> Result<Net> {
        (self.build)(nets, cfg)
    }
    pub fn get_dependency(&self, cfg: Value) -> Result<Vec<String>> {
        (self.get_dependency)(cfg)
    }
    pub fn schema(&self) -> &RootSchema {
        &self.schema
    }
}

pub trait ServerFactory {
    const NAME: &'static str;
    type Config: DeserializeOwned + JsonSchema;
    type Server: IServer + Sized + 'static;

    fn new(listen: Net, net: Net, config: Self::Config) -> Result<Self::Server>;
}

pub struct ServerResolver {
    build: fn(listen_net: Net, net: Net, cfg: Value) -> Result<Server>,
    schema: RootSchema,
}

impl ServerResolver {
    fn new<N: ServerFactory>() -> Self {
        let mut schema = schema_for!(N::Config);
        let net_schema = schema_for!(NetRef);
        schema
            .schema
            .object()
            .properties
            .insert("net".into(), net_schema.schema.clone().into());
        schema
            .schema
            .object()
            .properties
            .insert("listen".into(), net_schema.schema.into());
        Self {
            build: |listen_net, net: Net, cfg| {
                serde_json::from_value(cfg)
                    .map_err(Into::<crate::Error>::into)
                    .and_then(|cfg| N::new(listen_net, net, cfg))
                    .map(|n| n.into_dyn())
            },
            schema,
        }
    }
    pub fn build(&self, listen_net: Net, net: Net, cfg: Value) -> Result<Server> {
        (self.build)(listen_net, net, cfg)
    }
    pub fn schema(&self) -> &RootSchema {
        &self.schema
    }
}
