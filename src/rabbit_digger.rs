use std::{collections::BTreeMap, fmt, time::Duration};

use crate::{
    builtin::load_builtin,
    config,
    rabbit_digger::running::{RunningNet, RunningServer, RunningServerNet},
    registry::Registry,
    util::topological_sort,
};
use anyhow::{anyhow, Context, Result};
use futures::{
    future::{try_select, Either},
    stream::FuturesUnordered,
    FutureExt, Stream, StreamExt, TryStreamExt,
};
use rd_interface::{
    config::EmptyConfig, registry::NetGetter, schemars::schema::RootSchema, Arc, IntoDyn, Value,
};
use rd_std::builtin::local::LocalNetConfig;
use serde::{Deserialize, Serialize};
use tokio::{
    pin,
    sync::{mpsc, RwLock},
    task::unconstrained,
    time::{sleep, timeout},
};
use uuid::Uuid;

use self::{
    connection::ConnectionConfig,
    connection_manager::{ConnectionManager, ConnectionState},
    event::Event,
};

mod connection;
mod connection_manager;
mod event;
mod running;

pub type PluginLoader =
    Arc<dyn Fn(&config::Config, &mut Registry) -> Result<()> + Send + Sync + 'static>;

#[derive(Clone)]
pub struct RabbitDiggerBuilder {
    plugin_loader: PluginLoader,
}

#[allow(dead_code)]
struct Running {
    config: RwLock<config::Config>,
    registry_schema: RegistrySchema,
    registry: Registry,
    nets: BTreeMap<String, Arc<RunningNet>>,
    servers: BTreeMap<String, ServerInfo>,
}

enum State {
    WaitConfig,
    Running(Running),
}

impl State {
    fn running(&self) -> Option<&Running> {
        match self {
            State::Running(running) => Some(running),
            _ => None,
        }
    }
}

struct Inner {
    state: RwLock<State>,
    conn_cfg: ConnectionConfig,
}

#[derive(Clone)]
pub struct RabbitDigger {
    manager: ConnectionManager,
    inner: Arc<Inner>,
    plugin_loader: PluginLoader,
}

impl fmt::Debug for RabbitDigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RabbitDigger").finish()
    }
}

impl RabbitDigger {
    async fn recv_event(mut rx: mpsc::UnboundedReceiver<Event>, conn_mgr: ConnectionManager) {
        loop {
            let e = match rx.recv().now_or_never() {
                Some(Some(e)) => e,
                Some(None) => break,
                None => {
                    sleep(Duration::from_millis(100)).await;
                    continue;
                }
            };

            let mut events = Vec::with_capacity(16);
            events.push(e);
            while let Some(Some(e)) = rx.recv().now_or_never() {
                events.push(e);
            }
            conn_mgr.input_events(events.into_iter());
        }
        tracing::warn!("recv_event task exited");
    }
    async fn new(plugin_loader: &PluginLoader) -> Result<RabbitDigger> {
        let (event_sender, event_receiver) = mpsc::unbounded_channel();
        let manager = ConnectionManager::new();

        tokio::spawn(unconstrained(Self::recv_event(
            event_receiver,
            manager.clone(),
        )));

        let inner = Inner {
            state: RwLock::new(State::WaitConfig),
            conn_cfg: ConnectionConfig::new(event_sender),
        };

        Ok(RabbitDigger {
            inner: Arc::new(inner),
            plugin_loader: plugin_loader.clone(),
            manager,
        })
    }
    pub async fn stop(&self) -> Result<()> {
        let inner = &self.inner;
        let state = inner.state.read().await;

        match &*state {
            State::Running(Running { servers, .. }) => {
                for i in servers.values() {
                    i.server.stop().await?;
                }
            }
            _ => {}
        };
        // release the lock to allow other join tasks to write the state
        drop(state);

        self.join().await?;

        Ok(())
    }
    pub async fn join(&self) -> Result<()> {
        let inner = &self.inner;

        match &*inner.state.read().await {
            State::WaitConfig => return Ok(()),
            State::Running(Running { servers, .. }) => {
                let mut race = FuturesUnordered::new();
                for (name, i) in servers {
                    race.push(async move {
                        i.server.join().await;
                        if let Some(result) = i.server.take_result().await {
                            (name, result)
                        } else {
                            tracing::warn!("Failed to take result. This shouldn't happend");
                            (name, Ok(()))
                        }
                    });
                }

                while let Some((name, r)) = race.next().await {
                    if let Err(e) = r {
                        tracing::warn!("Server {} stopped with error: {:?}", name, e);
                    }
                }
            }
        };

        let state = &mut *inner.state.write().await;
        *state = State::WaitConfig;

        Ok(())
    }

    // get current config if it's running
    pub async fn config(&self) -> Result<config::Config> {
        let state = self.inner.state.read().await;
        match &*state {
            State::Running(Running { config, .. }) => {
                return Ok(config.read().await.clone());
            }
            _ => {
                return Err(anyhow!("Not running"));
            }
        };
    }

    // get current connection state
    pub async fn connection<F, R>(&self, f: F) -> R
    where
        F: FnOnce(&ConnectionState) -> R,
    {
        self.manager.borrow_state(f)
    }

    // get state
    pub async fn state_str(&self) -> Result<&'static str> {
        let state = self.inner.state.read().await;
        Ok(match &*state {
            State::WaitConfig => "WaitConfig",
            State::Running { .. } => "Running",
        })
    }

    // get registry schema
    pub async fn registry<F, R>(&self, f: F) -> R
    where
        F: FnOnce(Option<&RegistrySchema>) -> R,
    {
        let state = self.inner.state.read().await;
        f(state.running().map(|i| &i.registry_schema))
    }

    // start all server, all server run in background.
    pub async fn start(&self, config: config::Config) -> Result<()> {
        let inner = &self.inner;

        self.stop().await?;

        let state = &mut *inner.state.write().await;
        let mut registry = Registry::new();

        load_builtin(&mut registry).context("Failed to load builtin")?;
        (self.plugin_loader)(&config, &mut registry).context("Failed to load plugin")?;
        tracing::debug!("Registry:\n{}", registry);

        let nets = build_nets(&registry, config.net.clone()).context("Failed to build net")?;
        let servers = build_server(&nets, &config.server, &inner.conn_cfg)
            .await
            .context("Failed to build server")?;
        tracing::debug!(
            "net and server are built. net count: {}, server count: {}",
            nets.len(),
            servers.len()
        );

        tracing::info!("Server:\n{}", ServerList(&servers));
        for (_, server) in &servers {
            server.server.start(&registry, &server.config).await?;
        }

        *state = State::Running(Running {
            config: RwLock::new(config),
            registry_schema: get_registry_schema(&registry),
            registry,
            nets,
            servers,
        });

        Ok(())
    }

    pub async fn is_running(&self) -> bool {
        matches!(*self.inner.state.read().await, State::Running { .. })
    }

    pub async fn start_stream<S>(self, config_stream: S) -> Result<()>
    where
        S: Stream<Item = Result<config::Config>>,
    {
        futures::pin_mut!(config_stream);

        let mut config = match timeout(Duration::from_secs(10), config_stream.try_next()).await {
            Ok(Ok(Some(cfg))) => cfg,
            Ok(Err(e)) => return Err(e.context("Failed to get first config.")),
            Err(_) | Ok(Ok(None)) => {
                return Err(anyhow!("The config_stream is empty, can not start."))
            }
        };

        loop {
            tracing::info!("rabbit digger is starting...");

            self.start(config).await?;

            let new_config = {
                let join_fut = self.join();
                pin!(join_fut);

                match try_select(join_fut, config_stream.try_next()).await {
                    Ok(Either::Left((_, cfg_fut))) => {
                        tracing::info!("Exited normally, waiting for next config...");
                        cfg_fut.await
                    }
                    Ok(Either::Right((cfg, _))) => Ok(cfg),
                    Err(Either::Left((e, cfg_fut))) => {
                        tracing::error!(
                            "Rabbit digger went to error: {:?}, waiting for next config...",
                            e
                        );
                        cfg_fut.await
                    }
                    Err(Either::Right((e, _))) => Err(e),
                }
            };

            config = match new_config? {
                Some(v) => v,
                None => break,
            };

            self.stop().await?;
        }

        Ok(())
    }

    // Update net when running.
    pub async fn update_net(&self, net_name: &str, opt: Value) -> Result<()> {
        let state = self.inner.state.read().await;
        match &*state {
            State::Running(Running {
                config,
                nets,
                registry,
                ..
            }) => {
                let mut config = config.write().await;
                if let (Some(cfg), Some(running_net)) =
                    (config.net.get_mut(net_name), nets.get(net_name))
                {
                    let mut new_cfg = cfg.clone();
                    new_cfg.opt = opt;

                    let net = build_net(net_name, &new_cfg, &registry, &|key| {
                        nets.get(key).map(|i| i.net())
                    })?;
                    running_net.update_net(net.net()).await;

                    *cfg = new_cfg;
                }
                return Ok(());
            }
            _ => {
                return Err(anyhow!("Not running"));
            }
        };
    }
    // Stop the connection by uuid
    pub async fn stop_connection(&self, uuid: Uuid) -> Result<bool> {
        Ok(self.manager.stop_connection(uuid))
    }
}

impl Default for RabbitDiggerBuilder {
    fn default() -> Self {
        Self::new()
    }
}

impl RabbitDiggerBuilder {
    pub fn new() -> RabbitDiggerBuilder {
        RabbitDiggerBuilder {
            plugin_loader: Arc::new(|_, _| Ok(())),
        }
    }
    pub fn plugin_loader<PL>(mut self, plugin_loader: PL) -> Self
    where
        PL: Fn(&config::Config, &mut Registry) -> Result<()> + Send + Sync + 'static,
    {
        self.plugin_loader = Arc::new(plugin_loader);
        self
    }
    pub async fn build(&self) -> Result<RabbitDigger> {
        let rd = RabbitDigger::new(&self.plugin_loader).await?;
        Ok(rd)
    }
}

#[derive(Clone)]
pub struct ServerInfo {
    name: String,
    listen: String,
    net: String,
    server: RunningServer,
    config: Value,
}

struct ServerList<'a>(&'a BTreeMap<String, ServerInfo>);

impl fmt::Display for ServerInfo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}: {} -> {} {}",
            self.name, self.listen, self.net, self.config
        )
    }
}

impl<'a> fmt::Display for ServerList<'a> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        for i in self.0.values() {
            writeln!(f, "\t{}", i)?;
        }
        Ok(())
    }
}

fn build_net(
    name: &str,
    i: &config::Net,
    registry: &Registry,
    getter: NetGetter,
) -> Result<Arc<RunningNet>> {
    let net_item = registry.get_net(&i.net_type)?;

    let net = net_item.build(getter, i.opt.clone()).context(format!(
        "Failed to build net {:?}. Please check your config.",
        name
    ))?;
    let net = RunningNet::new(name.to_string(), net);
    Ok(net)
}

fn build_nets(
    registry: &Registry,
    mut all_net: BTreeMap<String, config::Net>,
) -> Result<BTreeMap<String, Arc<RunningNet>>> {
    let mut running_map: BTreeMap<String, Arc<RunningNet>> = BTreeMap::new();

    if !all_net.contains_key("noop") {
        all_net.insert(
            "noop".to_string(),
            config::Net::new_opt("noop", EmptyConfig::default())?,
        );
    }
    if !all_net.contains_key("local") {
        all_net.insert(
            "local".to_string(),
            config::Net::new_opt("local", LocalNetConfig::default())?,
        );
    }

    let all_net = topological_sort(all_net, |k, n| {
        registry
            .get_net(&n.net_type)?
            .resolver
            .get_dependency(n.opt.clone())
            .context(format!("Failed to get_dependency for net/server: {}", k))
    })
    .context("Failed to do topological_sort")?
    .ok_or_else(|| anyhow!("There is cyclic dependencies in net",))?;

    for (name, i) in all_net {
        let net = build_net(&name, &i, registry, &|key| {
            running_map.get(key).map(|i| i.net())
        })
        .context(format!("Loading net {}", name))?;
        running_map.insert(name.to_string(), net);
    }

    Ok(running_map)
}

async fn build_server(
    net: &BTreeMap<String, Arc<RunningNet>>,
    config: &config::ConfigServer,
    conn_cfg: &ConnectionConfig,
) -> Result<BTreeMap<String, ServerInfo>> {
    let mut servers = BTreeMap::new();
    let config = config.clone();

    for (name, i) in config {
        let name = &name;

        let load_server = async {
            let listen = net
                .get(&i.listen)
                .ok_or_else(|| {
                    anyhow!(
                        "Listen Net {} is not loaded. Required by {:?}",
                        &i.net,
                        &name
                    )
                })?
                .net();
            let net = RunningServerNet::new(
                name.clone(),
                net.get(&i.net)
                    .ok_or_else(|| {
                        anyhow!("Net {} is not loaded. Required by {:?}", &i.net, &name)
                    })?
                    .net(),
                conn_cfg.clone(),
            )
            .into_dyn();

            let server = RunningServer::new(name.to_string(), i.server_type, net, listen);
            servers.insert(
                name.to_string(),
                ServerInfo {
                    name: name.to_string(),
                    server,
                    config: i.opt,
                    listen: i.listen,
                    net: i.net,
                },
            );
            Ok(()) as Result<()>
        };

        load_server
            .await
            .context(format!("Loading server {}", name))?;
    }

    Ok(servers)
}

#[derive(Debug, Serialize, Deserialize)]
pub struct RegistrySchema {
    net: BTreeMap<String, RootSchema>,
    server: BTreeMap<String, RootSchema>,
}

fn get_registry_schema(registry: &Registry) -> RegistrySchema {
    let mut r = RegistrySchema {
        net: BTreeMap::new(),
        server: BTreeMap::new(),
    };

    for (key, value) in registry.net() {
        r.net.insert(key.clone(), value.resolver.schema().clone());
    }
    for (key, value) in registry.server() {
        r.server
            .insert(key.clone(), value.resolver.schema().clone());
    }

    r
}
